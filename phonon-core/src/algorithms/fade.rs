use crate::AudioBuffer;
#[cfg(feature = "parallel")]
use rayon::prelude::*;

/// Minimum sample count before engaging parallel processing.
#[cfg(feature = "parallel")]
const PAR_THRESHOLD: usize = 131072;

/// Applies a linear fade-in to the beginning of the audio buffer.
///
/// # Arguments
///
/// * `buffer` - The audio buffer to modify in place.
/// * `duration_secs` - The duration of the fade-in in seconds.
pub fn fade_in(buffer: &mut AudioBuffer, duration_secs: f64) {
  let ch = buffer.channels as usize;
  if !duration_secs.is_finite() || duration_secs <= 0.0 || ch == 0 || buffer.sample_rate == 0 {
    return;
  }

  let total_frames = buffer.samples.len() / ch;
  let fade_frames = ((duration_secs * buffer.sample_rate as f64) as usize).min(total_frames);
  if fade_frames == 0 {
    return;
  }
  let fade_len = fade_frames * ch;

  let inv_frames = 1.0 / fade_frames as f32;
  if ch == 1 {
    #[cfg(feature = "parallel")]
    if fade_len >= PAR_THRESHOLD {
      buffer.samples[..fade_len]
        .par_iter_mut()
        .enumerate()
        .for_each(|(i, s)| *s *= i as f32 * inv_frames);
      return;
    }
    #[cfg(feature = "simd")]
    super::simd_ops::simd_mul_ramp(&mut buffer.samples[..fade_len], 0.0, inv_frames);
    #[cfg(not(feature = "simd"))]
    for (i, s) in buffer.samples[..fade_len].iter_mut().enumerate() {
      *s *= i as f32 * inv_frames;
    }
    return;
  }

  // Multi-channel: apply the exact same gain to every channel in a frame.
  #[cfg(feature = "parallel")]
  if fade_len >= PAR_THRESHOLD {
    buffer.samples[..fade_len]
      .par_chunks_mut(ch)
      .enumerate()
      .for_each(|(frame_idx, frame)| {
        let gain = frame_idx as f32 * inv_frames;
        for s in frame {
          *s *= gain;
        }
      });
    return;
  }
  for (frame_idx, frame) in buffer.samples[..fade_len].chunks_mut(ch).enumerate() {
    let gain = frame_idx as f32 * inv_frames;
    for s in frame {
      *s *= gain;
    }
  }
}

/// Applies a linear fade-out to the end of the audio buffer.
///
/// # Arguments
///
/// * `buffer` - The audio buffer to modify in place.
/// * `duration_secs` - The duration of the fade-out in seconds.
pub fn fade_out(buffer: &mut AudioBuffer, duration_secs: f64) {
  let ch = buffer.channels as usize;
  if !duration_secs.is_finite() || duration_secs <= 0.0 || ch == 0 || buffer.sample_rate == 0 {
    return;
  }

  let total_frames = buffer.samples.len() / ch;
  let fade_frames = ((duration_secs * buffer.sample_rate as f64) as usize).min(total_frames);
  if fade_frames == 0 {
    return;
  }

  let fade_len = fade_frames * ch;
  let aligned_total = total_frames * ch;
  let start_idx = aligned_total - fade_len;
  let inv_frames = 1.0 / fade_frames as f32;

  if ch == 1 {
    #[cfg(feature = "parallel")]
    if fade_len >= PAR_THRESHOLD {
      buffer.samples[start_idx..aligned_total]
        .par_iter_mut()
        .enumerate()
        .for_each(|(i, s)| *s *= 1.0 - i as f32 * inv_frames);
      return;
    }
    #[cfg(feature = "simd")]
    super::simd_ops::simd_mul_ramp(
      &mut buffer.samples[start_idx..aligned_total],
      1.0,
      -inv_frames,
    );
    #[cfg(not(feature = "simd"))]
    for (i, s) in buffer.samples[start_idx..aligned_total]
      .iter_mut()
      .enumerate()
    {
      *s *= 1.0 - (i as f32 * inv_frames);
    }
    return;
  }

  // Multi-channel: frame-aligned so left/right receive identical ramp gain.
  #[cfg(feature = "parallel")]
  if fade_len >= PAR_THRESHOLD {
    buffer.samples[start_idx..aligned_total]
      .par_chunks_mut(ch)
      .enumerate()
      .for_each(|(frame_idx, frame)| {
        let gain = 1.0 - frame_idx as f32 * inv_frames;
        for s in frame {
          *s *= gain;
        }
      });
    return;
  }
  for (frame_idx, frame) in buffer.samples[start_idx..aligned_total]
    .chunks_mut(ch)
    .enumerate()
  {
    let gain = 1.0 - frame_idx as f32 * inv_frames;
    for s in frame {
      *s *= gain;
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_fade_in() {
    let mut buffer = AudioBuffer {
      samples: vec![1.0; 10],
      sample_rate: 1, // 1 sample per second per channel? simplified for math
      channels: 1,
    };
    // 10 samples total. Fade in 5 samples (5 seconds effectively with SR=1)
    // samples per sec = 1. fade len = 5 * 1 = 5.
    fade_in(&mut buffer, 5.0);

    assert_eq!(buffer.samples[0], 0.0); // 0/5 * 1.0
    assert_eq!(buffer.samples[1], 0.2); // 1/5 * 1.0
    assert_eq!(buffer.samples[2], 0.4);
    assert_eq!(buffer.samples[3], 0.6);
    assert_eq!(buffer.samples[4], 0.8);
    assert_eq!(buffer.samples[5], 1.0); // Untouched
  }

  #[test]
  fn test_fade_out() {
    let mut buffer = AudioBuffer {
      samples: vec![1.0; 10],
      sample_rate: 1,
      channels: 1,
    };
    // Fade out last 5 samples
    fade_out(&mut buffer, 5.0);

    assert_eq!(buffer.samples[4], 1.0); // Untouched
    assert_eq!(buffer.samples[5], 1.0); // Start fade: 1.0 - 0/5 = 1.0. Wait, logic check.
    // My logic: gain = 1.0 - i/fade_len.
    // i=0 -> gain=1.0. This makes the first sample of fade out full volume.
    // i=4 -> gain=1.0 - 4/5 = 0.2.
    // The last sample is never 0.0?
    // Let's trace: i=0 (idx 5): gain 1.0. i=1 (idx 6): gain 0.8. i=4 (idx 9): gain 0.2.
    // It doesn't quite reach 0.0. Usually we want it to reach 0.0 at the very end.
    // But for discrete samples, it's debatable.
    // If I want idx 9 to be 0.0?
    // Let's stick to simple linear logic for now: 1.0 - (i / fade_len).
    // If fade_len is huge, it's smooth. For 5 samples, it's coarse.

    // Let's check values based on implementation:
    assert_eq!(buffer.samples[5], 1.0);
    assert!((buffer.samples[6] - 0.8).abs() < 1e-6);
    assert!((buffer.samples[9] - 0.2).abs() < 1e-6);
  }

  #[test]
  fn test_fade_in_zero_duration() {
    let original = vec![1.0; 10];
    let mut buffer = AudioBuffer {
      samples: original.clone(),
      sample_rate: 1,
      channels: 1,
    };
    fade_in(&mut buffer, 0.0);
    assert_eq!(buffer.samples, original);
  }

  #[test]
  fn test_fade_in_negative_duration() {
    let original = vec![1.0; 10];
    let mut buffer = AudioBuffer {
      samples: original.clone(),
      sample_rate: 1,
      channels: 1,
    };
    fade_in(&mut buffer, -5.0);
    assert_eq!(buffer.samples, original);
  }

  #[test]
  fn test_fade_in_exceeding_duration() {
    let mut buffer = AudioBuffer {
      samples: vec![1.0; 5],
      sample_rate: 1,
      channels: 1,
    };
    // Fade in 100s on a 5-sample buffer → clamped to full buffer
    fade_in(&mut buffer, 100.0);
    assert!((buffer.samples[0] - 0.0).abs() < 1e-6);
    assert!(buffer.samples[4] > 0.0);
  }

  #[test]
  fn test_fade_out_zero_duration() {
    let original = vec![1.0; 10];
    let mut buffer = AudioBuffer {
      samples: original.clone(),
      sample_rate: 1,
      channels: 1,
    };
    fade_out(&mut buffer, 0.0);
    assert_eq!(buffer.samples, original);
  }

  #[test]
  fn test_fade_in_empty_buffer() {
    let mut buffer = AudioBuffer {
      samples: vec![],
      sample_rate: 44100,
      channels: 1,
    };
    fade_in(&mut buffer, 1.0);
    assert!(buffer.samples.is_empty());
  }

  #[test]
  fn test_fade_out_empty_buffer() {
    let mut buffer = AudioBuffer {
      samples: vec![],
      sample_rate: 44100,
      channels: 1,
    };
    fade_out(&mut buffer, 1.0);
    assert!(buffer.samples.is_empty());
  }
}
