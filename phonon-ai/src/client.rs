use crate::AiError;
use reqwest::Client;
use std::future::Future;
use std::time::Duration;
use tracing::warn;

/// Attempts per request in [`retry_transient`], including the first one.
const MAX_ATTEMPTS: u32 = 5;
/// Attempts when rate-limited (429): the waits add up to 63.5 s, past the
/// per-minute windows vendors count requests in.
const RATE_LIMIT_ATTEMPTS: u32 = 8;
/// First backoff delay in [`retry_transient`]; doubles on each retry.
const RETRY_BASE_DELAY: Duration = Duration::from_millis(500);
/// Upper bound for establishing a connection, separate from the overall
/// request timeout so an unreachable host fails fast.
pub(crate) const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// Hard ceiling on downloaded audio payloads from provider-supplied URLs
/// (100 MiB) to prevent OOM from unexpected or malicious responses.
pub(crate) const MAX_DOWNLOAD_BYTES: usize = 100 * 1024 * 1024;

/// Validate a URL returned by an upstream provider before issuing a request.
///
/// Always requires `https://`, forbids embedded credentials, and rejects
/// loopback / private / link-local / metadata IP literals and local hostnames.
/// When `required_host` is `Some(host)` (for authenticated polling URLs), the
/// URL host must match `host` (case-insensitively) on the default HTTPS port.
pub(crate) fn validate_provider_url(
  raw: &str,
  required_host: Option<&str>,
) -> Result<reqwest::Url, AiError> {
  let url =
    reqwest::Url::parse(raw).map_err(|e| AiError::Decode(format!("Invalid provider URL: {e}")))?;
  if url.scheme() != "https" {
    return Err(AiError::Decode(format!(
      "Provider URL must use https scheme, got {}",
      url.scheme()
    )));
  }
  if !url.username().is_empty() || url.password().is_some() {
    return Err(AiError::Decode(
      "Provider URL must not contain credentials".to_string(),
    ));
  }
  let host_str = url
    .host_str()
    .ok_or_else(|| AiError::Decode("Provider URL is missing a host".to_string()))?;

  if let Some(expected) = required_host {
    let host_ok = host_str.eq_ignore_ascii_case(expected);
    let port_ok = matches!(url.port(), None | Some(443));
    if !host_ok || !port_ok {
      return Err(AiError::Decode(format!(
        "Provider URL host is not allowed (expected https://{expected})"
      )));
    }
    return Ok(url);
  }

  let bare_host = host_str
    .strip_prefix('[')
    .and_then(|s| s.strip_suffix(']'))
    .unwrap_or(host_str);

  if let Ok(ip) = bare_host.parse::<std::net::IpAddr>() {
    let disallowed = match ip {
      std::net::IpAddr::V4(v4) => is_disallowed_ipv4(v4),
      std::net::IpAddr::V6(v6) => is_disallowed_ipv6(v6),
    };
    if disallowed {
      return Err(AiError::Decode(format!(
        "Provider URL points to a non-public IP address: {ip}"
      )));
    }
  } else {
    let lower = bare_host.to_ascii_lowercase();
    if lower == "localhost"
      || lower.ends_with(".localhost")
      || lower.ends_with(".local")
      || lower.ends_with(".internal")
    {
      return Err(AiError::Decode(format!(
        "Provider URL points to a local domain: {bare_host}"
      )));
    }
  }
  Ok(url)
}

fn is_disallowed_ipv4(ip: std::net::Ipv4Addr) -> bool {
  ip.is_private()
    || ip.is_loopback()
    || ip.is_link_local()
    || ip.is_broadcast()
    || ip.is_documentation()
    || ip.is_unspecified()
    || ip.is_multicast()
    // CGNAT 100.64.0.0/10
    || (ip.octets()[0] == 100 && (ip.octets()[1] & 0b1100_0000) == 64)
}

fn is_disallowed_ipv6(ip: std::net::Ipv6Addr) -> bool {
  if let Some(v4) = ip.to_ipv4_mapped() {
    return is_disallowed_ipv4(v4);
  }
  let seg0 = ip.segments()[0];
  let is_unique_local = (seg0 & 0xfe00) == 0xfc00;
  let is_unicast_link_local = (seg0 & 0xffc0) == 0xfe80;
  ip.is_loopback()
    || ip.is_unspecified()
    || ip.is_multicast()
    || is_unique_local
    || is_unicast_link_local
}

/// Read a response body up to `max_bytes`, failing early without buffering
/// unbounded data into memory.
pub(crate) async fn read_bounded_bytes(
  mut resp: reqwest::Response,
  max_bytes: usize,
) -> Result<Vec<u8>, AiError> {
  if let Some(len) = resp.content_length()
    && len > max_bytes as u64
  {
    return Err(AiError::Decode(format!(
      "Provider download exceeds size limit ({len} > {max_bytes} bytes)"
    )));
  }
  let cap = resp
    .content_length()
    .map(|n| (n as usize).min(max_bytes))
    .unwrap_or(0);
  let mut buf = Vec::with_capacity(cap);
  while let Some(chunk) = resp
    .chunk()
    .await
    .map_err(|e| AiError::Http(e.to_string()))?
  {
    if buf.len().saturating_add(chunk.len()) > max_bytes {
      return Err(AiError::Decode(format!(
        "Provider download exceeds size limit ({max_bytes} bytes)"
      )));
    }
    buf.extend_from_slice(&chunk);
  }
  Ok(buf)
}

/// Read a provider credential from the environment variable `name`.
pub(crate) fn env_key(name: &str) -> Result<String, AiError> {
  let raw = std::env::var(name)
    .map_err(|_| AiError::Config(format!("{name} environment variable not set")))?;
  let trimmed = raw.trim();
  if trimmed.is_empty() {
    return Err(AiError::Config(format!(
      "{name} environment variable is empty"
    )));
  }
  Ok(trimmed.to_string())
}

/// Deterministic-free small jitter (+0%..+25%) so concurrent retries do not
/// synchronize into thundering-herd 429 bursts.
fn jitter_delay(delay: Duration) -> Duration {
  use std::sync::atomic::{AtomicU64, Ordering};
  static COUNTER: AtomicU64 = AtomicU64::new(1);
  let ms = delay.as_millis() as u64;
  if ms < 10 {
    return delay;
  }
  let nanos = std::time::SystemTime::now()
    .duration_since(std::time::UNIX_EPOCH)
    .map(|d| d.subsec_nanos() as u64)
    .unwrap_or(0);
  let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
  // SplitMix64 finalizer for low-overhead uniform spread.
  let mut x = nanos.wrapping_add(seq.wrapping_mul(0x9E37_79B9_7F4A_7C15));
  x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
  x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
  x ^= x >> 31;
  let extra_ms = x % ((ms / 4).max(1) + 1);
  delay + Duration::from_millis(extra_ms)
}

/// Run `op`, retrying with exponential backoff (waiting 0.5, 1, 2, 4 s, ...)
/// while it fails with a [transient](AiError::is_transient) error: up to 5
/// attempts, or 8 (63.5 s of waiting) while rate-limited.
///
/// Only wrap requests that are safe to repeat: a retried TTS call costs
/// another synthesis but has no other side effect.
pub async fn retry_transient<T, F, Fut>(op: F) -> Result<T, AiError>
where
  F: FnMut() -> Fut,
  Fut: Future<Output = Result<T, AiError>>,
{
  retry_with_base_delay(RETRY_BASE_DELAY, op).await
}

async fn retry_with_base_delay<T, F, Fut>(base_delay: Duration, mut op: F) -> Result<T, AiError>
where
  F: FnMut() -> Fut,
  Fut: Future<Output = Result<T, AiError>>,
{
  let mut attempt = 1;
  loop {
    match op().await {
      Err(e) if e.is_transient() && attempt < max_attempts(&e) => {
        let delay = jitter_delay(base_delay * 2u32.pow(attempt - 1));
        warn!(attempt, ?delay, error = %e, "transient provider error, retrying");
        tokio::time::sleep(delay).await;
        attempt += 1;
      }
      result => return result,
    }
  }
}

fn max_attempts(error: &AiError) -> u32 {
  match error {
    AiError::Api { status: 429, .. } => RATE_LIMIT_ATTEMPTS,
    _ => MAX_ATTEMPTS,
  }
}

/// Turn a non-2xx response into `AiError::Api` carrying its status and body.
pub(crate) async fn ensure_success(resp: reqwest::Response) -> Result<reqwest::Response, AiError> {
  let status = resp.status();
  if status.is_success() {
    return Ok(resp);
  }
  let message = resp
    .text()
    .await
    .unwrap_or_else(|_| "Failed to read error body".to_string());
  Err(AiError::Api {
    status: status.as_u16(),
    message,
  })
}

/// A credential whose `Debug` output is redacted, so providers can keep
/// deriving `Debug` without leaking keys into logs.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct ApiKey(String);

impl ApiKey {
  pub(crate) fn new(key: impl Into<String>) -> Self {
    Self(key.into())
  }

  /// The raw key, for building request headers only.
  pub(crate) fn expose(&self) -> &str {
    &self.0
  }
}

impl std::fmt::Debug for ApiKey {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.write_str("ApiKey(<redacted>)")
  }
}

/// A thin HTTP client wrapper for TTS API calls.
///
/// Manages the underlying reqwest client, base URL, API key, and timeout settings.
#[derive(Debug, Clone)]
pub struct HttpClient {
  inner: Client,
  base_url: String,
  api_key: ApiKey,
}

impl HttpClient {
  /// Creates a new `HttpClient`.
  ///
  /// # Arguments
  ///
  /// * `api_key` - The API key for authentication.
  /// * `base_url` - The base URL of the API (e.g. "https://api.openai.com/v1").
  /// * `timeout_secs` - Request timeout in seconds. Default: 120.
  pub fn new(
    api_key: impl Into<String>,
    base_url: impl Into<String>,
    timeout_secs: Option<u64>,
  ) -> Result<Self, AiError> {
    let timeout = Duration::from_secs(timeout_secs.unwrap_or(120));
    let inner = Client::builder()
      .timeout(timeout)
      .connect_timeout(CONNECT_TIMEOUT)
      .build()
      .map_err(|e| AiError::Http(e.to_string()))?;

    Ok(Self {
      inner,
      base_url: base_url.into(),
      api_key: ApiKey::new(api_key),
    })
  }

  /// Wraps a caller-configured reqwest client, e.g. one with custom proxy,
  /// TLS or timeout settings.
  pub fn with_client(
    inner: Client,
    api_key: impl Into<String>,
    base_url: impl Into<String>,
  ) -> Self {
    Self {
      inner,
      base_url: base_url.into(),
      api_key: ApiKey::new(api_key),
    }
  }

  /// Creates a client configured for OpenAI TTS.
  ///
  /// Reads the API key from the `OPENAI_API_KEY` environment variable.
  pub fn openai() -> Result<Self, AiError> {
    Self::new(
      env_key("OPENAI_API_KEY")?,
      "https://api.openai.com/v1",
      None,
    )
  }

  /// Creates a client configured for an OpenAI-compatible API with custom base URL.
  ///
  /// Reads the API key from the `OPENAI_API_KEY` environment variable.
  pub fn openai_compatible(base_url: impl Into<String>) -> Result<Self, AiError> {
    Self::new(env_key("OPENAI_API_KEY")?, base_url, None)
  }

  /// Returns a reference to the inner reqwest client.
  pub fn inner(&self) -> &Client {
    &self.inner
  }

  /// Returns the base URL.
  pub fn base_url(&self) -> &str {
    &self.base_url
  }

  /// Returns the API key.
  pub fn api_key(&self) -> &str {
    self.api_key.expose()
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_debug_redacts_api_key() {
    let client = HttpClient::new("sk-very-secret", "https://example.invalid", None).unwrap();
    let debug = format!("{client:?}");
    assert!(!debug.contains("sk-very-secret"), "{debug}");
    assert_eq!(client.api_key(), "sk-very-secret");
  }

  #[test]
  fn test_env_key_missing_is_config_error() {
    // A name no environment sets, so the test never mutates process state.
    let err = env_key("PHONON_TEST_DEFINITELY_UNSET_KEY").unwrap_err();
    assert!(
      matches!(err, AiError::Config(ref m) if m.contains("PHONON_TEST_DEFINITELY_UNSET_KEY"))
    );
  }

  /// Fails with `errors` in order, then succeeds; returns the call count.
  async fn run_with_errors(errors: Vec<AiError>) -> (Result<u32, AiError>, u32) {
    let calls = std::cell::Cell::new(0u32);
    let errors = std::cell::RefCell::new(errors.into_iter());
    let result = retry_with_base_delay(Duration::ZERO, || {
      calls.set(calls.get() + 1);
      let next = errors.borrow_mut().next();
      async move {
        match next {
          Some(e) => Err(e),
          None => Ok(7),
        }
      }
    })
    .await;
    (result, calls.get())
  }

  fn api(status: u16) -> AiError {
    AiError::Api {
      status,
      message: String::new(),
    }
  }

  #[tokio::test]
  async fn test_retry_recovers_from_transient_errors() {
    let (result, calls) = run_with_errors(vec![api(429), AiError::Http("reset".into())]).await;
    assert_eq!(result.unwrap(), 7);
    assert_eq!(calls, 3);
  }

  /// DashScope answered the first requests of a 4-way burst with 429 twice
  /// in a row in 2 of 4 episode exports. MiniMax allows about 27 requests,
  /// then one every 2-5 s until the minute is up; 4 concurrent requests lost
  /// 5 times in a row and a 30-segment export failed.
  #[tokio::test]
  async fn test_retry_waits_out_a_rate_limit_minute() {
    let (result, calls) =
      run_with_errors((1..RATE_LIMIT_ATTEMPTS).map(|_| api(429)).collect()).await;
    assert_eq!(result.unwrap(), 7);
    assert_eq!(calls, 8);
    let waited: Duration = (1..RATE_LIMIT_ATTEMPTS)
      .map(|n| RETRY_BASE_DELAY * 2u32.pow(n - 1))
      .sum();
    assert!(waited >= Duration::from_secs(60), "{waited:?}");

    let (result, calls) =
      run_with_errors((0..=RATE_LIMIT_ATTEMPTS).map(|_| api(429)).collect()).await;
    assert!(matches!(result, Err(AiError::Api { status: 429, .. })));
    assert_eq!(calls, RATE_LIMIT_ATTEMPTS);
  }

  #[tokio::test]
  async fn test_retry_gives_up_after_max_attempts() {
    let (result, calls) = run_with_errors((0..=MAX_ATTEMPTS).map(|_| api(503)).collect()).await;
    assert!(matches!(result, Err(AiError::Api { status: 503, .. })));
    assert_eq!(calls, MAX_ATTEMPTS);
  }

  #[tokio::test]
  async fn test_retry_does_not_repeat_permanent_errors() {
    let (result, calls) = run_with_errors(vec![api(401)]).await;
    assert!(matches!(result, Err(AiError::Api { status: 401, .. })));
    assert_eq!(calls, 1);
    let (result, calls) = run_with_errors(vec![AiError::Decode("bad mp3".into())]).await;
    assert!(result.is_err());
    assert_eq!(calls, 1);
  }

  #[test]
  fn test_validate_provider_url_blocks_ssrf_and_host_mismatch() {
    assert!(
      validate_provider_url(
        "https://api.replicate.com/v1/predictions/abc",
        Some("api.replicate.com")
      )
      .is_ok()
    );
    assert!(validate_provider_url("https://evil.example/v1/p", Some("api.replicate.com")).is_err());
    assert!(
      validate_provider_url("http://api.replicate.com/v1/p", Some("api.replicate.com")).is_err()
    );
    assert!(
      validate_provider_url(
        "https://api.replicate.com:8443/v1/p",
        Some("api.replicate.com")
      )
      .is_err()
    );
    assert!(validate_provider_url("https://user:pass@api.replicate.com/v1/p", None).is_err());
    assert!(validate_provider_url("https://169.254.169.254/latest/meta-data", None).is_err());
    assert!(validate_provider_url("https://127.0.0.1/secret.wav", None).is_err());
    assert!(validate_provider_url("https://10.0.0.1/audio.wav", None).is_err());
    assert!(validate_provider_url("https://[::1]/audio.wav", None).is_err());
    assert!(validate_provider_url("https://localhost/audio.wav", None).is_err());
    assert!(validate_provider_url("https://cdn.example.com/audio/out.wav?sig=1", None).is_ok());
  }

  #[tokio::test]
  async fn test_read_bounded_bytes_enforces_limit() {
    let ok_resp = reqwest::Response::from(
      http::Response::builder()
        .status(200)
        .body(vec![1u8; 16])
        .unwrap(),
    );
    assert_eq!(read_bounded_bytes(ok_resp, 16).await.unwrap().len(), 16);

    let big_resp = reqwest::Response::from(
      http::Response::builder()
        .status(200)
        .body(vec![1u8; 32])
        .unwrap(),
    );
    assert!(read_bounded_bytes(big_resp, 16).await.is_err());
  }
}
