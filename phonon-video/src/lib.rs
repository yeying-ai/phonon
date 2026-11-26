//! # phonon-video
//!
//! Video audio track extraction and replacement using ffmpeg CLI.
//!
//! This crate wraps the `ffmpeg` command-line tool to provide:
//!
//! - **Audio extraction**: Extract audio tracks from video files into `AudioBuffer`.
//! - **Audio replacement**: Replace a video's audio track with a new one.
//! - **Probe**: Inspect video/audio stream metadata.
//!
//! ## Requirements
//!
//! The `ffmpeg` and `ffprobe` commands must be available on `$PATH`.
//!
//! ## Example
//!
//! ```rust,no_run
//! use phonon_video::{extract_audio, replace_audio, probe_streams};
//!
//! # async fn example() -> Result<(), phonon_video::VideoError> {
//! // Extract audio from a video file
//! let buffer = extract_audio("input.mp4", None).await?;
//! println!("Extracted {} seconds of audio", buffer.duration_secs());
//!
//! // Replace the audio track
//! replace_audio("input.mp4", "new_audio.wav", "output.mp4").await?;
//!
//! // Inspect streams
//! let streams = probe_streams("input.mp4").await?;
//! for s in &streams {
//!     println!("{}: {}", s.codec_type, s.codec_name);
//! }
//! # Ok(())
//! # }
//! ```

use phonon_core::AudioBuffer;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use thiserror::Error;
use tracing::{info, instrument};

/// Default timeout for ffmpeg / ffprobe child processes (10 minutes).
const FFMPEG_TIMEOUT: Duration = Duration::from_secs(600);
/// Whitelist of ffmpeg protocols allowed for local file operations.
const LOCAL_PROTOCOLS: &str = "file,pipe";

fn looks_like_protocol(raw: &str) -> bool {
  if raw.contains("://") {
    return true;
  }
  let lower = raw.to_ascii_lowercase();
  [
    "pipe:", "concat:", "subfile:", "http:", "https:", "udp:", "tcp:", "rtmp:", "rtsp:", "data:",
    "crypto:", "ftp:",
  ]
  .iter()
  .any(|prefix| lower.starts_with(prefix))
}

fn prepare_local_input(path: &Path, label: &str) -> Result<PathBuf> {
  if path.as_os_str().is_empty() {
    return Err(VideoError::InvalidInput(format!("{label} path is empty")));
  }
  if !path.exists() {
    return Err(VideoError::InvalidInput(format!(
      "{label} does not exist: {}",
      path.display()
    )));
  }
  let lossy = path.to_string_lossy();
  if looks_like_protocol(&lossy) && !path.is_absolute() {
    return Err(VideoError::InvalidInput(format!(
      "{label} path must be a local file, not a protocol URL: {}",
      path.display()
    )));
  }
  // Prefix leading '-' relative paths with './' so ffmpeg/ffprobe never parse them as flags.
  if lossy.starts_with('-') {
    Ok(Path::new(".").join(path))
  } else {
    Ok(path.to_path_buf())
  }
}

fn prepare_local_output(path: &Path) -> Result<PathBuf> {
  if path.as_os_str().is_empty() {
    return Err(VideoError::InvalidInput("Output path is empty".to_string()));
  }
  let lossy = path.to_string_lossy();
  if lossy == "-" || looks_like_protocol(&lossy) {
    return Err(VideoError::InvalidInput(format!(
      "Output path must be a local file path: {}",
      path.display()
    )));
  }
  if lossy.starts_with('-') {
    Ok(Path::new(".").join(path))
  } else {
    Ok(path.to_path_buf())
  }
}

fn ensure_distinct_paths(input: &Path, output: &Path) -> Result<()> {
  if let (Ok(a), Ok(b)) = (std::fs::canonicalize(input), std::fs::canonicalize(output))
    && a == b
  {
    return Err(VideoError::InvalidInput(
      "Output path must differ from input video path".to_string(),
    ));
  }
  Ok(())
}

async fn run_media_cmd(
  mut cmd: tokio::process::Command,
  tool: &str,
) -> Result<std::process::Output> {
  cmd
    .stdin(Stdio::null())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .kill_on_drop(true);
  match tokio::time::timeout(FFMPEG_TIMEOUT, cmd.output()).await {
    Ok(Ok(out)) => Ok(out),
    Ok(Err(e)) => Err(VideoError::Ffmpeg(format!("{tool} failed: {e}"))),
    Err(_) => Err(VideoError::Ffmpeg(format!(
      "{tool} timed out after {}s",
      FFMPEG_TIMEOUT.as_secs()
    ))),
  }
}

/// Errors from video processing operations.
#[derive(Error, Debug)]
pub enum VideoError {
  /// ffmpeg/ffprobe not found or failed to execute.
  #[error("ffmpeg error: {0}")]
  Ffmpeg(String),

  /// Audio decoding/encoding error.
  #[error("Audio error: {0}")]
  Audio(String),

  /// I/O error.
  #[error("IO error: {0}")]
  Io(String),

  /// Invalid input or unsupported format.
  #[error("Invalid input: {0}")]
  InvalidInput(String),
}

pub type Result<T> = std::result::Result<T, VideoError>;

// ---------------------------------------------------------------------------
// Stream probe
// ---------------------------------------------------------------------------

/// Metadata for a single stream in a media file.
#[derive(Debug, Clone, Deserialize)]
pub struct StreamInfo {
  /// Stream index.
  pub index: usize,
  /// Codec type: "video", "audio", "subtitle", etc.
  pub codec_type: String,
  /// Codec name (e.g. "h264", "aac", "mp3").
  pub codec_name: String,
  /// Sample rate (audio only).
  #[serde(default)]
  pub sample_rate: Option<String>,
  /// Number of channels (audio only).
  #[serde(default)]
  pub channels: Option<u16>,
  /// Duration in seconds (may be missing for some containers).
  #[serde(default)]
  pub duration: Option<String>,
  /// Bit rate (if available).
  #[serde(default)]
  pub bit_rate: Option<String>,
}

/// ffprobe JSON output structure.
#[derive(Deserialize)]
struct FfprobeOutput {
  streams: Vec<StreamInfo>,
}

/// Check that ffmpeg is available on PATH.
pub async fn check_ffmpeg() -> Result<()> {
  let mut cmd = tokio::process::Command::new("ffmpeg");
  cmd.args(["-nostdin", "-version"]);
  let output = run_media_cmd(cmd, "ffmpeg")
    .await
    .map_err(|e| VideoError::Ffmpeg(format!("ffmpeg not found: {e}")))?;

  if !output.status.success() {
    return Err(VideoError::Ffmpeg("ffmpeg -version failed".into()));
  }

  Ok(())
}

/// Probe the streams in a media file using `ffprobe`.
#[instrument(skip_all, fields(path = %path.as_ref().display()))]
pub async fn probe_streams(path: impl AsRef<Path>) -> Result<Vec<StreamInfo>> {
  let path = prepare_local_input(path.as_ref(), "File")?;

  let mut cmd = tokio::process::Command::new("ffprobe");
  cmd
    .args([
      "-v",
      "quiet",
      "-protocol_whitelist",
      LOCAL_PROTOCOLS,
      "-print_format",
      "json",
      "-show_streams",
      "--",
    ])
    .arg(&path);
  let output = run_media_cmd(cmd, "ffprobe").await?;

  if !output.status.success() {
    let stderr = String::from_utf8_lossy(&output.stderr);
    return Err(VideoError::Ffmpeg(format!("ffprobe error: {stderr}")));
  }

  let result: FfprobeOutput = serde_json::from_slice(&output.stdout)
    .map_err(|e| VideoError::Ffmpeg(format!("Failed to parse ffprobe output: {e}")))?;

  info!(streams = result.streams.len(), "Probed media streams");

  Ok(result.streams)
}

// ---------------------------------------------------------------------------
// Audio extraction
// ---------------------------------------------------------------------------

/// Extract the audio track from a video file into an `AudioBuffer`.
///
/// Uses ffmpeg to extract audio as WAV, then reads it via phonon-io.
///
/// # Arguments
///
/// * `video_path` - Path to the input video file.
/// * `sample_rate` - Optional target sample rate. If `None`, uses the original.
#[instrument(skip_all, fields(video = %video_path.as_ref().display()))]
pub async fn extract_audio(
  video_path: impl AsRef<Path>,
  sample_rate: Option<u32>,
) -> Result<AudioBuffer> {
  let video_path = prepare_local_input(video_path.as_ref(), "Video file")?;

  let tmp_path = tempfile::Builder::new()
    .suffix(".wav")
    .tempfile()
    .map_err(|e| VideoError::Io(e.to_string()))?
    .into_temp_path();

  let mut cmd = tokio::process::Command::new("ffmpeg");
  cmd
    .args(["-nostdin", "-protocol_whitelist", LOCAL_PROTOCOLS, "-i"])
    .arg(&video_path)
    .args(["-vn", "-acodec", "pcm_s16le"]);

  if let Some(sr) = sample_rate {
    cmd.args(["-ar", &sr.to_string()]);
  }

  cmd.arg("-y").arg(&*tmp_path);

  info!("Extracting audio from video via ffmpeg");

  let output = run_media_cmd(cmd, "ffmpeg").await?;

  if !output.status.success() {
    let stderr = String::from_utf8_lossy(&output.stderr);
    return Err(VideoError::Ffmpeg(format!(
      "ffmpeg extraction error: {stderr}"
    )));
  }

  let read_path = tmp_path.to_path_buf();
  let buffer = tokio::task::spawn_blocking(move || phonon_io::read_audio(&read_path))
    .await
    .map_err(|e| VideoError::Io(format!("read_audio task failed: {e}")))?
    .map_err(|e| VideoError::Audio(e.to_string()))?;

  info!(
    duration = buffer.duration_secs(),
    sample_rate = buffer.sample_rate,
    channels = buffer.channels,
    "Audio extraction complete"
  );

  Ok(buffer)
}

/// Extract the audio track and save directly to a file.
#[instrument(skip_all, fields(video = %video_path.as_ref().display(), output = %output_path.as_ref().display()))]
pub async fn extract_audio_to_file(
  video_path: impl AsRef<Path>,
  output_path: impl AsRef<Path>,
) -> Result<()> {
  let video_path = prepare_local_input(video_path.as_ref(), "Video file")?;
  let output_path = prepare_local_output(output_path.as_ref())?;
  ensure_distinct_paths(&video_path, &output_path)?;

  let mut cmd = tokio::process::Command::new("ffmpeg");
  cmd
    .args(["-nostdin", "-protocol_whitelist", LOCAL_PROTOCOLS, "-i"])
    .arg(&video_path)
    .args(["-vn", "-y"])
    .arg(&output_path);
  let output = run_media_cmd(cmd, "ffmpeg").await?;

  if !output.status.success() {
    let stderr = String::from_utf8_lossy(&output.stderr);
    return Err(VideoError::Ffmpeg(format!("ffmpeg error: {stderr}")));
  }

  info!("Audio extracted to file");
  Ok(())
}

// ---------------------------------------------------------------------------
// Audio replacement
// ---------------------------------------------------------------------------

/// Replace the audio track of a video file with a new audio file.
///
/// The video stream is copied without re-encoding; audio is encoded using
/// the output container's default codec.
///
/// # Arguments
///
/// * `video_path` - Input video file.
/// * `audio_path` - New audio file (WAV, MP3, etc.).
/// * `output_path` - Output video file path.
#[instrument(skip_all, fields(
    video = %video_path.as_ref().display(),
    audio = %audio_path.as_ref().display(),
    output = %output_path.as_ref().display()
))]
pub async fn replace_audio(
  video_path: impl AsRef<Path>,
  audio_path: impl AsRef<Path>,
  output_path: impl AsRef<Path>,
) -> Result<()> {
  let video_path = prepare_local_input(video_path.as_ref(), "Video file")?;
  let audio_path = prepare_local_input(audio_path.as_ref(), "Audio file")?;
  let output_path = prepare_local_output(output_path.as_ref())?;
  ensure_distinct_paths(&video_path, &output_path)?;

  info!("Replacing audio track in video");

  let mut cmd = tokio::process::Command::new("ffmpeg");
  cmd
    .args(["-nostdin", "-protocol_whitelist", LOCAL_PROTOCOLS, "-i"])
    .arg(&video_path)
    .arg("-i")
    .arg(&audio_path)
    .args([
      "-c:v",
      "copy", // Copy video stream without re-encoding
      "-map",
      "0:v:0", // Take video from first input
      "-map",
      "1:a:0",     // Take audio from second input
      "-shortest", // End when shorter stream ends
      "-y",
    ])
    .arg(&output_path);
  let output = run_media_cmd(cmd, "ffmpeg").await?;

  if !output.status.success() {
    let stderr = String::from_utf8_lossy(&output.stderr);
    return Err(VideoError::Ffmpeg(format!(
      "ffmpeg replace error: {stderr}"
    )));
  }

  info!("Audio track replaced successfully");
  Ok(())
}

/// Replace the audio track using an `AudioBuffer` instead of a file.
///
/// The buffer is first written to a temporary WAV file, then used for replacement.
#[instrument(skip_all, fields(
    video = %video_path.as_ref().display(),
    output = %output_path.as_ref().display(),
    buffer_duration = buffer.duration_secs()
))]
pub async fn replace_audio_with_buffer(
  video_path: impl AsRef<Path>,
  buffer: &AudioBuffer,
  output_path: impl AsRef<Path>,
) -> Result<()> {
  let tmp_path = tempfile::Builder::new()
    .suffix(".wav")
    .tempfile()
    .map_err(|e| VideoError::Io(e.to_string()))?
    .into_temp_path();

  let write_path = tmp_path.to_path_buf();
  let owned = buffer.clone();
  tokio::task::spawn_blocking(move || phonon_io::write_audio(&write_path, &owned))
    .await
    .map_err(|e| VideoError::Io(format!("write_audio task failed: {e}")))?
    .map_err(|e| VideoError::Audio(e.to_string()))?;

  replace_audio(video_path, &*tmp_path, output_path).await
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_stream_info_deserialization() {
    let json = r#"{
            "index": 0,
            "codec_type": "video",
            "codec_name": "h264",
            "sample_rate": null,
            "channels": null,
            "duration": "120.5",
            "bit_rate": "5000000"
        }"#;
    let info: StreamInfo = serde_json::from_str(json).expect("parse stream info");
    assert_eq!(info.index, 0);
    assert_eq!(info.codec_type, "video");
    assert_eq!(info.codec_name, "h264");
    assert!(info.sample_rate.is_none());
    assert!(info.channels.is_none());
    assert_eq!(info.duration.as_deref(), Some("120.5"));
  }

  #[test]
  fn test_stream_info_audio() {
    let json = r#"{
            "index": 1,
            "codec_type": "audio",
            "codec_name": "aac",
            "sample_rate": "44100",
            "channels": 2,
            "duration": "120.5",
            "bit_rate": "128000"
        }"#;
    let info: StreamInfo = serde_json::from_str(json).expect("parse audio stream");
    assert_eq!(info.codec_type, "audio");
    assert_eq!(info.sample_rate.as_deref(), Some("44100"));
    assert_eq!(info.channels, Some(2));
  }

  #[test]
  fn test_ffprobe_output_deserialization() {
    let json = r#"{
            "streams": [
                {"index": 0, "codec_type": "video", "codec_name": "h264"},
                {"index": 1, "codec_type": "audio", "codec_name": "aac", "sample_rate": "48000", "channels": 2}
            ]
        }"#;
    let output: FfprobeOutput = serde_json::from_str(json).expect("parse ffprobe output");
    assert_eq!(output.streams.len(), 2);
    assert_eq!(output.streams[0].codec_type, "video");
    assert_eq!(output.streams[1].codec_type, "audio");
  }

  #[tokio::test]
  async fn test_extract_audio_nonexistent_file() {
    let result = extract_audio("/nonexistent/video.mp4", None).await;
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("does not exist"));
  }

  #[tokio::test]
  async fn test_replace_audio_nonexistent_video() {
    let result = replace_audio(
      "/nonexistent/video.mp4",
      "/nonexistent/audio.wav",
      "/tmp/output.mp4",
    )
    .await;
    assert!(result.is_err());
  }

  #[tokio::test]
  async fn test_probe_nonexistent_file() {
    let result = probe_streams("/nonexistent/video.mp4").await;
    assert!(result.is_err());
  }

  #[test]
  fn test_output_path_rejects_stdout_and_protocols() {
    assert!(prepare_local_output(Path::new("")).is_err());
    assert!(prepare_local_output(Path::new("-")).is_err());
    assert!(prepare_local_output(Path::new("http://evil.example/out.mp4")).is_err());
    assert!(prepare_local_output(Path::new("udp://127.0.0.1:1234")).is_err());
    assert!(prepare_local_output(Path::new("pipe:1")).is_err());
    assert_eq!(
      prepare_local_output(Path::new("-leading-dash.mp4")).unwrap(),
      PathBuf::from("./-leading-dash.mp4")
    );
  }

  #[test]
  fn test_ensure_distinct_paths_rejects_same_file() {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    assert!(ensure_distinct_paths(tmp.path(), tmp.path()).is_err());
  }
}
