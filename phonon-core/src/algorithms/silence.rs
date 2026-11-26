use crate::{AudioBuffer, PhononError, Result};

/// Generates a silent `AudioBuffer` of the specified duration.
///
/// # Arguments
///
/// * `duration_secs` - Duration of silence in seconds. Must be non-negative.
/// * `sample_rate` - Sample rate in Hz (e.g. 44100).
/// * `channels` - Number of channels (1 = Mono, 2 = Stereo).
///
/// # Returns
///
/// An `AudioBuffer` filled with zeros (silence).
///
/// # Errors
///
/// Returns an error if `sample_rate` is 0 or `channels` is 0.
pub fn silence(duration_secs: f64, sample_rate: u32, channels: u16) -> Result<AudioBuffer> {
  if sample_rate == 0 {
    return Err(PhononError::InvalidSampleRate(0));
  }
  if channels == 0 {
    return Err(PhononError::ChannelMismatch {
      expected: 1,
      actual: 0,
    });
  }
  if !duration_secs.is_finite() {
    return Err(PhononError::InvalidParam(
      "Duration must be finite".to_string(),
    ));
  }
  if duration_secs <= 0.0 {
    return Ok(AudioBuffer {
      samples: Vec::new(),
      sample_rate,
      channels,
    });
  }

  let ch = channels as usize;
  let frames_f64 = duration_secs * sample_rate as f64;
  let max_samples = (isize::MAX as usize) / std::mem::size_of::<f32>();
  if !frames_f64.is_finite() || frames_f64 > (max_samples / ch) as f64 {
    return Err(PhononError::InvalidParam(
      "Silence duration exceeds buffer capacity limits".to_string(),
    ));
  }
  let frames = frames_f64 as usize;
  let total_samples = frames
    .checked_mul(ch)
    .ok_or_else(|| PhononError::InvalidParam("Silence length overflow".to_string()))?;

  Ok(AudioBuffer {
    samples: vec![0.0; total_samples],
    sample_rate,
    channels,
  })
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_silence_basic() {
    let buf = silence(1.0, 44100, 1).unwrap();
    assert_eq!(buf.samples.len(), 44100);
    assert_eq!(buf.sample_rate, 44100);
    assert_eq!(buf.channels, 1);
    assert!(buf.samples.iter().all(|&s| s == 0.0));
  }

  #[test]
  fn test_silence_stereo() {
    let buf = silence(0.5, 44100, 2).unwrap();
    // 0.5s * 44100 * 2 channels = 44100
    assert_eq!(buf.samples.len(), 44100);
    assert!((buf.duration_secs() - 0.5).abs() < 0.001);
  }

  #[test]
  fn test_silence_zero_duration() {
    let buf = silence(0.0, 44100, 1).unwrap();
    assert!(buf.samples.is_empty());
  }

  #[test]
  fn test_silence_negative_duration() {
    let buf = silence(-1.0, 44100, 1).unwrap();
    assert!(buf.samples.is_empty());
  }

  #[test]
  fn test_silence_zero_sample_rate() {
    assert!(silence(1.0, 0, 1).is_err());
  }

  #[test]
  fn test_silence_zero_channels() {
    assert!(silence(1.0, 44100, 0).is_err());
  }

  #[test]
  fn test_silence_nan_inf_error_and_frame_aligned() {
    assert!(silence(f64::NAN, 44100, 2).is_err());
    assert!(silence(f64::INFINITY, 44100, 2).is_err());
    assert!(silence(1e30, 44100, 2).is_err());
    // Frame-aligned even for fractional durations that would otherwise yield odd stereo length.
    let buf = silence(0.000_034, 44100, 2).unwrap();
    assert_eq!(buf.samples.len() % 2, 0);
  }
}
