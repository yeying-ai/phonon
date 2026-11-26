//! JSON-RPC 2.0 stdio server implementing the MCP wire protocol.
//!
//! MCP over stdio uses newline-delimited JSON: each line is one
//! complete JSON-RPC 2.0 message (request, response, or notification).
//!
//! Methods supported:
//! - `initialize` — handshake; returns server info + capabilities
//! - `notifications/initialized` — no-op acknowledgement
//! - `ping` — keep-alive, returns `{}`
//! - `tools/list` — returns the registered tools
//! - `tools/call` — invokes a tool
//!
//! Anything else returns a JSON-RPC method-not-found error.

use crate::tools::ToolRegistry;
use serde::Serialize;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tracing::{debug, info, warn};

/// MCP protocol version that this server speaks. Mirrors the spec id used
/// by Claude Desktop and other MCP clients in 2025.
pub const PROTOCOL_VERSION: &str = "2025-06-18";

/// Server-identifying info returned by `initialize`.
#[derive(Debug, Clone, Serialize)]
pub struct ServerInfo {
  pub name: String,
  pub version: String,
}

impl Default for ServerInfo {
  fn default() -> Self {
    Self {
      name: "phonon-mcp".into(),
      version: env!("CARGO_PKG_VERSION").into(),
    }
  }
}

/// Stdio MCP server.
pub struct Server {
  pub info: ServerInfo,
  pub tools: ToolRegistry,
}

/// Maximum size of a single JSON-RPC line (4 MiB) to prevent unbounded memory
/// allocation from malformed or hostile clients.
const MAX_LINE_BYTES: usize = 4 * 1024 * 1024;

enum LineRead {
  Eof,
  Line(String),
  TooLong,
}

async fn read_bounded_line<R>(
  reader: &mut BufReader<R>,
  max_bytes: usize,
) -> std::io::Result<LineRead>
where
  R: tokio::io::AsyncRead + Unpin,
{
  let mut buf = Vec::new();
  let mut overflowed = false;
  loop {
    let available = reader.fill_buf().await?;
    if available.is_empty() {
      if overflowed {
        return Ok(LineRead::TooLong);
      }
      if buf.is_empty() {
        return Ok(LineRead::Eof);
      }
      break;
    }

    let (consume_len, found_newline) = match available.iter().position(|&b| b == b'\n') {
      Some(idx) => (idx + 1, true),
      None => (available.len(), false),
    };

    if !overflowed {
      if buf.len().saturating_add(consume_len) > max_bytes {
        overflowed = true;
        buf.clear();
      } else {
        buf.extend_from_slice(&available[..consume_len]);
      }
    }
    reader.consume(consume_len);

    if found_newline {
      if overflowed {
        return Ok(LineRead::TooLong);
      }
      break;
    }
  }

  if buf.ends_with(b"\n") {
    buf.pop();
    if buf.ends_with(b"\r") {
      buf.pop();
    }
  }
  String::from_utf8(buf)
    .map(LineRead::Line)
    .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}

fn truncate_for_log(s: &str, max_bytes: usize) -> &str {
  if s.len() <= max_bytes {
    return s;
  }
  let mut end = max_bytes;
  while end > 0 && !s.is_char_boundary(end) {
    end -= 1;
  }
  &s[..end]
}

impl Server {
  /// Server with the built-in tools; file access is confined to `fs_root`.
  pub fn new(fs_root: impl Into<std::path::PathBuf>) -> Self {
    Self {
      info: ServerInfo::default(),
      tools: ToolRegistry::new(fs_root),
    }
  }

  pub fn with_tools(tools: ToolRegistry) -> Self {
    Self {
      info: ServerInfo::default(),
      tools,
    }
  }

  /// Run the request loop on the given async reader / writer until the
  /// stream closes. Each line of input is one JSON-RPC message.
  pub async fn run<R, W>(&self, reader: R, mut writer: W) -> std::io::Result<()>
  where
    R: tokio::io::AsyncRead + Unpin + Send,
    W: tokio::io::AsyncWrite + Unpin + Send,
  {
    let mut reader = BufReader::new(reader);
    info!("phonon-mcp server starting");
    loop {
      let response = match read_bounded_line(&mut reader, MAX_LINE_BYTES).await? {
        LineRead::Eof => break,
        LineRead::TooLong => {
          warn!(max_bytes = MAX_LINE_BYTES, "JSON-RPC line exceeded limit");
          Some(json!({
            "jsonrpc": "2.0",
            "id": Value::Null,
            "error": {
              "code": -32600,
              "message": format!("Request line exceeds {MAX_LINE_BYTES} byte limit")
            }
          }))
        }
        LineRead::Line(line) => {
          if line.trim().is_empty() {
            continue;
          }
          self.handle_line(&line).await
        }
      };
      if let Some(out) = response {
        let mut s = serde_json::to_string(&out).unwrap_or_else(|_| "{}".into());
        s.push('\n');
        writer.write_all(s.as_bytes()).await?;
        writer.flush().await?;
      }
    }
    info!("phonon-mcp server stopping");
    Ok(())
  }

  /// Convenience entry point — runs against actual stdin/stdout.
  pub async fn run_stdio(&self) -> std::io::Result<()> {
    self.run(tokio::io::stdin(), tokio::io::stdout()).await
  }

  /// Handle a single line of JSON-RPC. Returns `None` for notifications
  /// (which by spec must not get a response) and parse errors that have
  /// no id to attach a response to.
  pub(crate) async fn handle_line(&self, line: &str) -> Option<Value> {
    let req: Value = match serde_json::from_str(line) {
      Ok(v) => v,
      Err(e) => {
        warn!(err = %e, preview = truncate_for_log(line, 256), "JSON parse error");
        // Per spec, we can't reply without an id, but emit a parse-error
        // response with id=null to surface the problem.
        return Some(json!({
          "jsonrpc": "2.0",
          "id": Value::Null,
          "error": { "code": -32700, "message": format!("Parse error: {}", e) }
        }));
      }
    };

    let id = req.get("id").cloned();
    let method = req.get("method").and_then(|m| m.as_str()).unwrap_or("");
    let params = req.get("params").cloned().unwrap_or(Value::Null);
    debug!(method = method, id = ?id, "incoming request");

    // Notifications have no id and never get a response.
    let is_notification = id.is_none();

    let result = self.dispatch(method, params).await;

    if is_notification {
      return None;
    }

    let id = id.unwrap_or(Value::Null);
    Some(match result {
      Ok(value) => json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": value,
      }),
      Err((code, message)) => json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message }
      }),
    })
  }

  async fn dispatch(&self, method: &str, params: Value) -> Result<Value, (i64, String)> {
    match method {
      "initialize" => Ok(json!({
        "protocolVersion": PROTOCOL_VERSION,
        "serverInfo": self.info,
        "capabilities": {
          "tools": { "listChanged": false }
        }
      })),
      "notifications/initialized" => Ok(Value::Null),
      "ping" => Ok(json!({})),
      "tools/list" => Ok(json!({ "tools": self.tools.list_json() })),
      "tools/call" => {
        let name = params
          .get("name")
          .and_then(|v| v.as_str())
          .ok_or_else(|| (-32602, "tools/call: missing 'name'".to_string()))?;
        let args = params.get("arguments").cloned().unwrap_or(json!({}));
        match self.tools.call(name, args).await {
          Ok(value) => Ok(json!({
            "content": [
              { "type": "text", "text": value.to_string() }
            ],
            "isError": false,
            "structuredContent": value,
          })),
          Err(e) => Ok(json!({
            "content": [
              { "type": "text", "text": e.to_string() }
            ],
            "isError": true,
          })),
        }
      }
      _ => Err((-32601, format!("Method not found: {}", method))),
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use phonon_core::AudioBuffer;
  use serde_json::json;

  fn server() -> Server {
    Server::new(std::env::temp_dir())
  }

  #[tokio::test]
  async fn test_initialize_returns_server_info() {
    let s = server();
    let req = json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize" });
    let resp = s.handle_line(&req.to_string()).await.unwrap();
    assert_eq!(resp["id"], 1);
    assert_eq!(resp["result"]["protocolVersion"], PROTOCOL_VERSION);
    assert_eq!(resp["result"]["serverInfo"]["name"], "phonon-mcp");
  }

  #[tokio::test]
  async fn test_notifications_have_no_response() {
    let s = server();
    let req = json!({ "jsonrpc": "2.0", "method": "notifications/initialized" });
    assert!(s.handle_line(&req.to_string()).await.is_none());
  }

  #[tokio::test]
  async fn test_tools_list_includes_audio_info() {
    let s = server();
    let req = json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" });
    let resp = s.handle_line(&req.to_string()).await.unwrap();
    let tools = resp["result"]["tools"].as_array().unwrap();
    let names: Vec<&str> = tools.iter().filter_map(|t| t["name"].as_str()).collect();
    assert!(names.contains(&"audio_info"));
  }

  #[tokio::test]
  async fn test_tools_call_audio_info_roundtrip() {
    let s = server();
    let buf = AudioBuffer {
      samples: vec![0.5; 48000],
      sample_rate: 48000,
      channels: 1,
    };
    let h = s.tools.registry.insert(buf).unwrap();
    let req = json!({
      "jsonrpc": "2.0",
      "id": 3,
      "method": "tools/call",
      "params": {
        "name": "audio_info",
        "arguments": { "handle": h }
      }
    });
    let resp = s.handle_line(&req.to_string()).await.unwrap();
    assert_eq!(resp["id"], 3);
    assert_eq!(resp["result"]["isError"], false);
    let structured = &resp["result"]["structuredContent"];
    assert_eq!(structured["sample_rate"], 48000);
  }

  #[tokio::test]
  async fn test_tools_call_unknown_tool_returns_error_content() {
    let s = server();
    let req = json!({
      "jsonrpc": "2.0",
      "id": 4,
      "method": "tools/call",
      "params": { "name": "no_such_tool", "arguments": {} }
    });
    let resp = s.handle_line(&req.to_string()).await.unwrap();
    assert_eq!(resp["result"]["isError"], true);
    let text = resp["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("Tool not found"));
  }

  #[tokio::test]
  async fn test_unknown_method_returns_jsonrpc_error() {
    let s = server();
    let req = json!({ "jsonrpc": "2.0", "id": 5, "method": "bogus/method" });
    let resp = s.handle_line(&req.to_string()).await.unwrap();
    assert_eq!(resp["error"]["code"], -32601);
  }

  #[tokio::test]
  async fn test_parse_error_returns_id_null_response() {
    let s = server();
    let resp = s.handle_line("{ this is not json").await.unwrap();
    assert_eq!(resp["id"], Value::Null);
    assert_eq!(resp["error"]["code"], -32700);
  }

  #[tokio::test]
  async fn test_ping() {
    let s = server();
    let req = json!({ "jsonrpc": "2.0", "id": 6, "method": "ping" });
    let resp = s.handle_line(&req.to_string()).await.unwrap();
    assert_eq!(resp["id"], 6);
    assert_eq!(resp["result"], json!({}));
  }

  #[tokio::test]
  async fn test_read_bounded_line_rejects_oversized_and_recovers() {
    let data = format!("{}\n{{\"ok\":true}}\n", "x".repeat(64));
    let mut reader = BufReader::new(data.as_bytes());
    assert!(matches!(
      read_bounded_line(&mut reader, 32).await.unwrap(),
      LineRead::TooLong
    ));
    match read_bounded_line(&mut reader, 32).await.unwrap() {
      LineRead::Line(line) => assert_eq!(line, "{\"ok\":true}"),
      _ => panic!("expected following line after oversized line"),
    }
  }
}
