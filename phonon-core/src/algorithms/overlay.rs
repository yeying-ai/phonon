use crate::{AudioBuffer, PhononError, Result};

/// Overlays `overlay_buf` on top of `base` starting at `position_secs`.
///
/// The base buffer length is preserved unless the overlay extends beyond it,
/// in which case the result is extended to fit.
///
/// This is equivalent to pydub's `.overlay(sound, position=...)`.
///
/// # Arguments
///
/// * `base` - The base audio buffer.
/// * `overlay_buf` - The audio to overlay on top.
/// * `position_secs` - The start position (in seconds) where the overlay begins.
///
/// # Errors
///
/// Returns an error if sample rates or channel counts do not match.
pub fn overlay_at_position(
  base: &AudioBuffer,
  overlay_buf: &AudioBuffer,
  position_secs: f64,
) -> Result<AudioBuffer> {
  if base.sample_rate != overlay_buf.sample_rate {
    return Err(PhononError::InvalidSampleRate(overlay_buf.sample_rate));
  }
  if base.channels != overlay_buf.channels {
    return Err(PhononError::ChannelMismatch {
      expected: base.channels,
      actual: overlay_buf.channels,
    });
  }

  let ch = base.frame_stride()?;
  if !position_secs.is_finite() || position_secs < 0.0 {
    return Err(PhononError::InvalidParam(
      "Position must be finite and non-negative".to_string(),
    ));
  }

  let samples_per_sec = base.sample_rate as f64 * ch as f64;
  let offset_f64 = position_secs * samples_per_sec;
  let max_samples = (isize::MAX as usize) / std::mem::size_of::<f32>();
  if !offset_f64.is_finite() || offset_f64 > max_samples as f64 {
    return Err(PhononError::InvalidParam(
      "Overlay position exceeds buffer capacity limits".to_string(),
    ));
  }
  let mut offset = offset_f64 as usize;
  // Align to frame boundary
  offset -= offset % ch;

  // Result length: max of (base length) and (offset + overlay length)
  let overlay_end = offset
    .checked_add(overlay_buf.samples.len())
    .filter(|&len| len <= max_samples)
    .ok_or_else(|| PhononError::InvalidParam("Overlay length overflow".to_string()))?;
  let result_len = base.samples.len().max(overlay_end);
  let mut samples = vec![0.0_f32; result_len];

  // Copy base into result
  samples[..base.samples.len()].copy_from_slice(&base.samples);

  // Add overlay at offset
  for (i, &s) in overlay_buf.samples.iter().enumerate() {
    let idx = offset + i;
    samples[idx] = (samples[idx] + s).clamp(-1.0, 1.0);
  }

  Ok(AudioBuffer {
    samples,
    sample_rate: base.sample_rate,
    channels: base.channels,
  })
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_overlay_at_start() {
    let base = AudioBuffer {
      samples: vec![0.5; 10],
      sample_rate: 10,
      channels: 1,
    };
    let overlay = AudioBuffer {
      samples: vec![0.3; 4],
      sample_rate: 10,
      channels: 1,
    };

    let result = overlay_at_position(&base, &overlay, 0.0).unwrap();
    assert_eq!(result.samples.len(), 10);
    assert!((result.samples[0] - 0.8).abs() < 1e-6);
    assert!((result.samples[3] - 0.8).abs() < 1e-6);
    assert!((result.samples[4] - 0.5).abs() < 1e-6);
  }

  #[test]
  fn test_overlay_at_middle() {
    let base = AudioBuffer {
      samples: vec![0.5; 10],
      sample_rate: 10,
      channels: 1,
    };
    let overlay = AudioBuffer {
      samples: vec![0.3; 3],
      sample_rate: 10,
      channels: 1,
    };

    // Position at 0.5s → offset = 0.5 * 10 = 5 samples
    let result = overlay_at_position(&base, &overlay, 0.5).unwrap();
    assert_eq!(result.samples.len(), 10);
    assert!((result.samples[4] - 0.5).abs() < 1e-6); // before overlay
    assert!((result.samples[5] - 0.8).abs() < 1e-6); // overlay zone
    assert!((result.samples[7] - 0.8).abs() < 1e-6); // overlay zone
    assert!((result.samples[8] - 0.5).abs() < 1e-6); // after overlay
  }

  #[test]
  fn test_overlay_extends_beyond_base() {
    let base = AudioBuffer {
      samples: vec![0.5; 5],
      sample_rate: 10,
      channels: 1,
    };
    let overlay = AudioBuffer {
      samples: vec![0.3; 4],
      sample_rate: 10,
      channels: 1,
    };

    // Offset at 0.3s → 3 samples. Overlay ends at 3+4=7, base is 5 → extend to 7
    let result = overlay_at_position(&base, &overlay, 0.3).unwrap();
    assert_eq!(result.samples.len(), 7);
    assert!((result.samples[5] - 0.3).abs() < 1e-6); // beyond base, only overlay
    assert!((result.samples[6] - 0.3).abs() < 1e-6);
  }

  #[test]
  fn test_overlay_clipping() {
    let base = AudioBuffer {
      samples: vec![0.8; 4],
      sample_rate: 10,
      channels: 1,
    };
    let overlay = AudioBuffer {
      samples: vec![0.8; 4],
      sample_rate: 10,
      channels: 1,
    };

    let result = overlay_at_position(&base, &overlay, 0.0).unwrap();
    // 0.8 + 0.8 = 1.6 → clamped to 1.0
    assert!((result.samples[0] - 1.0).abs() < 1e-6);
  }

  #[test]
  fn test_overlay_sample_rate_mismatch() {
    let base = AudioBuffer {
      samples: vec![0.0],
      sample_rate: 44100,
      channels: 1,
    };
    let overlay = AudioBuffer {
      samples: vec![0.0],
      sample_rate: 48000,
      channels: 1,
    };
    assert!(overlay_at_position(&base, &overlay, 0.0).is_err());
  }

  #[test]
  fn test_overlay_channel_mismatch() {
    let base = AudioBuffer {
      samples: vec![0.0],
      sample_rate: 44100,
      channels: 1,
    };
    let overlay = AudioBuffer {
      samples: vec![0.0, 0.0],
      sample_rate: 44100,
      channels: 2,
    };
    assert!(overlay_at_position(&base, &overlay, 0.0).is_err());
  }

  #[test]
  fn test_overlay_negative_position() {
    let base = AudioBuffer {
      samples: vec![0.0; 10],
      sample_rate: 10,
      channels: 1,
    };
    let overlay = AudioBuffer {
      samples: vec![0.0; 3],
      sample_rate: 10,
      channels: 1,
    };
    assert!(overlay_at_position(&base, &overlay, -1.0).is_err());
  }

  #[test]
  fn test_overlay_stereo() {
    let base = AudioBuffer {
      samples: vec![0.5, 0.5, 0.5, 0.5], // 2 frames
      sample_rate: 10,
      channels: 2,
    };
    let overlay = AudioBuffer {
      samples: vec![0.3, 0.3], // 1 frame
      sample_rate: 10,
      channels: 2,
    };

    // Position 0.1s → 0.1 * 10 * 2 = 2 samples (1 frame offset)
    let result = overlay_at_position(&base, &overlay, 0.1).unwrap();
    assert_eq!(result.samples.len(), 4);
    assert!((result.samples[0] - 0.5).abs() < 1e-6); // untouched
    assert!((result.samples[2] - 0.8).abs() < 1e-6); // overlay
    assert!((result.samples[3] - 0.8).abs() < 1e-6); // overlay
  }

  #[test]
  fn test_overlay_nan_and_inf_error() {
    let base = AudioBuffer {
      samples: vec![0.5; 10],
      sample_rate: 10,
      channels: 1,
    };
    let overlay = AudioBuffer {
      samples: vec![0.3; 4],
      sample_rate: 10,
      channels: 1,
    };
    assert!(overlay_at_position(&base, &overlay, f64::NAN).is_err());
    assert!(overlay_at_position(&base, &overlay, f64::INFINITY).is_err());
    assert!(overlay_at_position(&base, &overlay, 1e30).is_err());
  }
}
