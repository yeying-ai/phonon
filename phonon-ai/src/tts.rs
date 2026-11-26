use crate::AiError;
use crate::client::{HttpClient, retry_transient};
use crate::voice::VoiceConfig;
use futures::StreamExt;
use phonon_core::AudioBuffer;
use serde::Serialize;
use std::future::Future;
use tracing::{info, instrument};

/// Trait for Text-to-Speech providers.
///
/// Implementors convert text into an `AudioBuffer` using a specified voice.
/// Async methods return `impl Future + Send` (RPITIT); use [`crate::AnyTts`]
/// when runtime dispatch across providers is needed.
pub trait TtsProvider: Send + Sync {
  /// Optional fingerprint capturing provider-side synthesis settings that do
  /// not live on [`VoiceConfig`] (e.g. model name, output sample rate, format),
  /// so [`crate::CachingTts`] invalidates cache entries when those change.
  fn cache_fingerprint(&self) -> Option<String> {
    None
  }
  /// Synthesizes the given text into an `AudioBuffer`.
  ///
  /// # Arguments
  ///
  /// * `text` - The text to synthesize.
  /// * `voice` - The voice configuration to use.
  ///
  /// # Returns
  ///
  /// An `AudioBuffer` containing the synthesized speech.
  fn synthesize(
    &self,
    text: &str,
    voice: &VoiceConfig,
  ) -> impl Future<Output = Result<AudioBuffer, AiError>> + Send;

  /// Synthesizes text and sends audio chunks to a channel as they become available.
  ///
  /// This enables streaming playback — the caller can start processing/playing
  /// the first chunks before synthesis is complete.
  ///
  /// The default implementation calls `synthesize()` and sends the entire buffer
  /// as a single chunk. Providers that support true streaming (like OpenAI)
  /// may override this for lower latency.
  ///
  /// # Arguments
  ///
  /// * `text` - The text to synthesize.
  /// * `voice` - The voice configuration to use.
  /// * `tx` - Channel sender for audio buffer chunks.
  fn synthesize_streaming(
    &self,
    text: &str,
    voice: &VoiceConfig,
    tx: tokio::sync::mpsc::Sender<Result<AudioBuffer, AiError>>,
  ) -> impl Future<Output = Result<(), AiError>> + Send {
    async move {
      let buffer = self.synthesize(text, voice).await;
      let _ = tx.send(buffer).await;
      Ok(())
    }
  }
}

/// OpenAI TTS provider.
///
/// Uses the OpenAI `/audio/speech` API to synthesize text to speech.
/// Returns audio as WAV (via Symphonia decoding of the MP3 response).
#[derive(Debug, Clone)]
pub struct OpenAiTts {
  client: HttpClient,
  format: String,
}

/// OpenAI TTS API request body.
#[derive(Serialize)]
struct TtsRequest<'a> {
  model: &'a str,
  input: &'a str,
  voice: &'a str,
  #[serde(skip_serializing_if = "Option::is_none")]
  speed: Option<f64>,
  response_format: &'a str,
}

impl OpenAiTts {
  /// Creates a new `OpenAiTts` using the default OpenAI endpoint.
  ///
  /// Reads the API key from `OPENAI_API_KEY` environment variable.
  pub fn new() -> Result<Self, AiError> {
    let client = HttpClient::openai()?;
    Ok(Self::with_client(client))
  }

  /// Creates a new `OpenAiTts` with a custom HTTP client.
  ///
  /// Useful for OpenAI-compatible APIs (e.g. local TTS servers).
  pub fn with_client(client: HttpClient) -> Self {
    Self {
      client,
      format: "mp3".to_string(),
    }
  }

  /// Set the `response_format` to request (default `"mp3"`). Some compatible
  /// servers lack mp3 (GLM only returns wav / pcm); any format `decode_audio`
  /// reads works.
  pub fn with_format(mut self, format: impl Into<String>) -> Self {
    self.format = format.into();
    self
  }
}

/// Decodes a provider's encoded audio response; `extension` hints the format.
pub(crate) fn decode(bytes: &[u8], extension: &str) -> Result<AudioBuffer, AiError> {
  phonon_io::decode_audio(bytes, extension).map_err(|e| AiError::Decode(e.to_string()))
}

/// PCM sample rate used by OpenAI when `response_format: "pcm"` (24 kHz 16-bit LE mono).
const OPENAI_PCM_SAMPLE_RATE: u32 = 24000;

/// Convert raw 16-bit little-endian PCM bytes into f32 samples in `[-1.0, 1.0]`.
pub fn pcm_i16le_to_f32(bytes: &[u8]) -> Vec<f32> {
  let sample_count = bytes.len() / 2;
  let mut out = Vec::with_capacity(sample_count);
  for chunk in bytes.chunks_exact(2) {
    let sample = i16::from_le_bytes([chunk[0], chunk[1]]);
    out.push(sample as f32 / i16::MAX as f32);
  }
  out
}

/// Split `text` into chunks of at most `max_chars` characters for providers
/// with a per-request limit: at sentence ends where possible, then at
/// commas, then between words, then anywhere. Whitespace-only chunks are
/// dropped.
pub fn split_text(text: &str, max_chars: usize) -> Vec<String> {
  const SENTENCE: &[char] = &['。', '！', '？', '；', '!', '?', ';', '\n'];
  const CLAUSE: &[char] = &['，', '、', '：', ',', ':'];
  let max_chars = max_chars.max(1);
  // "2.5" and "e.g.x" aren't sentence ends; a period before a space is.
  let sentence_end = |c: char, next: Option<char>| {
    SENTENCE.contains(&c) || (c == '.' && next.is_none_or(char::is_whitespace))
  };

  let mut pieces = Vec::new();
  for sentence in split_after(text, sentence_end) {
    if sentence.chars().count() <= max_chars {
      pieces.push(sentence.to_string());
      continue;
    }
    for clause in split_after(sentence, |c, _| CLAUSE.contains(&c)) {
      pieces.extend(cut_between_words(clause, max_chars));
    }
  }

  let mut chunks: Vec<String> = Vec::new();
  let mut current = String::new();
  for piece in pieces {
    if current.chars().count() + piece.chars().count() > max_chars && !current.is_empty() {
      chunks.push(std::mem::take(&mut current));
    }
    current.push_str(&piece);
  }
  chunks.push(current);
  chunks.retain(|c| !c.trim().is_empty());
  chunks
}

/// Split after every character where `is_end(c, next)` holds, keeping it
/// with its piece.
fn split_after(text: &str, is_end: impl Fn(char, Option<char>) -> bool) -> Vec<&str> {
  let mut pieces = Vec::new();
  let mut start = 0;
  let mut chars = text.char_indices().peekable();
  while let Some((i, c)) = chars.next() {
    if is_end(c, chars.peek().map(|&(_, n)| n)) {
      let end = i + c.len_utf8();
      pieces.push(&text[start..end]);
      start = end;
    }
  }
  if start < text.len() {
    pieces.push(&text[start..]);
  }
  pieces
}

/// Cut `text` into pieces of at most `max_chars`, each ending after the last
/// whitespace that fits; a word longer than `max_chars` (or CJK text, which
/// has no spaces) is cut anywhere.
fn cut_between_words(text: &str, max_chars: usize) -> Vec<String> {
  let chars: Vec<char> = text.chars().collect();
  let mut pieces = Vec::new();
  let mut rest = &chars[..];
  while rest.len() > max_chars {
    let window = &rest[..max_chars];
    let cut = window
      .iter()
      .rposition(|c| c.is_whitespace())
      .map_or(max_chars, |i| i + 1);
    pieces.push(rest[..cut].iter().collect());
    rest = &rest[cut..];
  }
  if !rest.is_empty() {
    pieces.push(rest.iter().collect());
  }
  pieces
}

impl TtsProvider for OpenAiTts {
  fn cache_fingerprint(&self) -> Option<String> {
    Some(format!("openai:{}", self.format))
  }

  #[instrument(skip(self, text), fields(text_len = text.len(), voice = %voice.voice_id))]
  async fn synthesize(&self, text: &str, voice: &VoiceConfig) -> Result<AudioBuffer, AiError> {
    info!("Synthesizing text via OpenAI TTS");

    let url = format!("{}/audio/speech", self.client.base_url());

    let speed = if (voice.speed - 1.0).abs() > f64::EPSILON {
      Some(voice.speed)
    } else {
      None
    };

    let request_body = TtsRequest {
      model: &voice.model,
      input: text,
      voice: &voice.voice_id,
      speed,
      response_format: &self.format,
    };

    let audio_bytes = retry_transient(|| async {
      let response = self
        .client
        .inner()
        .post(&url)
        .header("Authorization", format!("Bearer {}", self.client.api_key()))
        .header("Content-Type", "application/json")
        .json(&request_body)
        .send()
        .await
        .map_err(|e| AiError::Http(e.to_string()))?;

      if !response.status().is_success() {
        let status = response.status();
        let body = response
          .text()
          .await
          .unwrap_or_else(|_| "Failed to read error body".to_string());
        return Err(AiError::Api {
          status: status.as_u16(),
          message: body,
        });
      }

      response
        .bytes()
        .await
        .map_err(|e| AiError::Http(e.to_string()))
    })
    .await?;

    info!(bytes = audio_bytes.len(), "Received audio response");

    let buffer = decode(&audio_bytes, &self.format)?;

    info!(
      duration = buffer.duration_secs(),
      samples = buffer.samples.len(),
      "TTS synthesis complete"
    );

    Ok(buffer)
  }

  /// Streaming TTS via OpenAI using raw PCM format.
  ///
  /// Requests `response_format: "pcm"` (24 kHz, 16-bit LE, mono) and streams
  /// each HTTP chunk directly as an `AudioBuffer` through the channel.
  #[instrument(skip(self, text, tx), fields(text_len = text.len(), voice = %voice.voice_id))]
  async fn synthesize_streaming(
    &self,
    text: &str,
    voice: &VoiceConfig,
    tx: tokio::sync::mpsc::Sender<Result<AudioBuffer, AiError>>,
  ) -> Result<(), AiError> {
    info!("Streaming TTS via OpenAI (PCM)");

    let url = format!("{}/audio/speech", self.client.base_url());

    let speed = if (voice.speed - 1.0).abs() > f64::EPSILON {
      Some(voice.speed)
    } else {
      None
    };

    let request_body = TtsRequest {
      model: &voice.model,
      input: text,
      voice: &voice.voice_id,
      speed,
      response_format: "pcm",
    };

    let response = self
      .client
      .inner()
      .post(&url)
      .header("Authorization", format!("Bearer {}", self.client.api_key()))
      .header("Content-Type", "application/json")
      .json(&request_body)
      .send()
      .await
      .map_err(|e| AiError::Http(e.to_string()))?;

    if !response.status().is_success() {
      let status = response.status();
      let body = response
        .text()
        .await
        .unwrap_or_else(|_| "Failed to read error body".to_string());
      return Err(AiError::Api {
        status: status.as_u16(),
        message: body,
      });
    }

    let mut stream = response.bytes_stream();
    // Keep a leftover byte in case a chunk splits a 16-bit sample.
    let mut leftover: Option<u8> = None;
    let mut total_samples: usize = 0;

    while let Some(chunk_result) = stream.next().await {
      let chunk = chunk_result.map_err(|e| AiError::Http(e.to_string()))?;
      if chunk.is_empty() {
        continue;
      }

      // Prepend leftover byte if present.
      let data: Vec<u8> = if let Some(lo) = leftover.take() {
        let mut v = Vec::with_capacity(1 + chunk.len());
        v.push(lo);
        v.extend_from_slice(&chunk);
        v
      } else {
        chunk.to_vec()
      };

      // If odd number of bytes, stash the last byte.
      let usable_len = data.len() & !1; // round down to even
      if data.len() % 2 != 0 {
        leftover = Some(data[data.len() - 1]);
      }

      if usable_len == 0 {
        continue;
      }

      let samples = pcm_i16le_to_f32(&data[..usable_len]);
      total_samples += samples.len();

      let buffer = AudioBuffer {
        samples,
        sample_rate: OPENAI_PCM_SAMPLE_RATE,
        channels: 1,
      };

      if tx.send(Ok(buffer)).await.is_err() {
        // Receiver dropped — stop streaming.
        info!("Streaming receiver dropped, stopping");
        return Ok(());
      }
    }

    info!(total_samples, "OpenAI streaming TTS complete");
    Ok(())
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  // Note: Integration tests with actual OpenAI API require OPENAI_API_KEY
  // and should be run with `cargo test -- --ignored`

  #[test]
  fn test_tts_request_serialization() {
    let req = TtsRequest {
      model: "tts-1",
      input: "Hello world",
      voice: "alloy",
      speed: Some(1.5),
      response_format: "mp3",
    };
    let json = serde_json::to_value(&req).unwrap();
    assert_eq!(json["model"], "tts-1");
    assert_eq!(json["input"], "Hello world");
    assert_eq!(json["voice"], "alloy");
    assert_eq!(json["speed"], 1.5);
    assert_eq!(json["response_format"], "mp3");
  }

  #[test]
  fn test_tts_request_no_speed() {
    let req = TtsRequest {
      model: "tts-1",
      input: "Hello",
      voice: "alloy",
      speed: None,
      response_format: "mp3",
    };
    let json = serde_json::to_value(&req).unwrap();
    assert!(json.get("speed").is_none());
  }

  #[test]
  fn test_split_text_prefers_sentence_boundaries() {
    assert_eq!(split_text("短句。", 10), ["短句。"]);
    assert_eq!(
      split_text("第一句话。第二句话！第三句。", 10),
      ["第一句话。第二句话！", "第三句。"]
    );
    // A sentence over the limit falls back to commas, then a hard cut.
    assert_eq!(
      split_text("一二三，四五六七八。", 5),
      ["一二三，", "四五六七八", "。"]
    );
    assert_eq!(split_text("abcdefg", 3), ["abc", "def", "g"]);
    assert!(split_text("  \n ", 5).is_empty());
  }

  #[test]
  fn test_split_text_english_sentences_and_words() {
    // A period ends a sentence only before whitespace or the end.
    assert_eq!(
      split_text(
        "Running is simple. Start slow, version 2.5 helps. Done.",
        32
      ),
      [
        "Running is simple.",
        " Start slow, version 2.5 helps.",
        " Done."
      ]
    );
    // Over the limit without punctuation: cut between words, not inside one.
    assert_eq!(
      split_text("the quick brown fox jumps over", 12),
      ["the quick ", "brown fox ", "jumps over"]
    );
  }

  #[test]
  fn test_split_text_respects_limit_and_keeps_text() {
    let text = "音频处理很有趣，我们今天聊响度。为什么要标准化？因为听众会切换节目！".repeat(7)
      + &"Loudness matters. Version 2.5 of the spec, e.g. this one, says so: listeners switch shows!"
        .repeat(5);
    for max in [7, 20, 64, 84, 600] {
      let chunks = split_text(&text, max);
      assert!(chunks.iter().all(|c| c.chars().count() <= max), "max {max}");
      assert_eq!(chunks.concat(), text, "max {max}");
    }
    // At 1 char a lone space is its own chunk and is dropped as empty.
    let squeeze = |t: &str| t.split_whitespace().collect::<String>();
    let chunks = split_text(&text, 1);
    assert!(chunks.iter().all(|c| c.chars().count() == 1));
    assert_eq!(squeeze(&chunks.concat()), squeeze(&text));
  }

  #[test]
  fn test_pcm_i16le_to_f32_basic() {
    // i16::MAX in LE = [0xFF, 0x7F]
    let bytes: Vec<u8> = vec![0xFF, 0x7F, 0x01, 0x80, 0x00, 0x00];
    let samples = pcm_i16le_to_f32(&bytes);
    assert_eq!(samples.len(), 3);
    // i16::MAX → 1.0
    assert!((samples[0] - 1.0).abs() < 1e-4);
    // i16::MIN + 1 = -32767 → ~ -1.0
    assert!((samples[1] + 1.0).abs() < 1e-3);
    // 0 → 0.0
    assert!((samples[2]).abs() < 1e-6);
  }

  #[test]
  fn test_pcm_i16le_to_f32_empty() {
    let samples = pcm_i16le_to_f32(&[]);
    assert!(samples.is_empty());
  }

  #[test]
  fn test_pcm_i16le_to_f32_odd_byte_count() {
    // 3 bytes → only 1 sample (first 2 bytes)
    let samples = pcm_i16le_to_f32(&[0x00, 0x40, 0xFF]);
    assert_eq!(samples.len(), 1);
  }
}
