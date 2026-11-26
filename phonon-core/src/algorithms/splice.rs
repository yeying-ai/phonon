//! Non-destructive range splicing with cosine micro-crossfades and offline
//! energy-based speech region detection (VAD) for text-driven audio editing.

use crate::{AudioBuffer, PhononError, Result};
use std::f32::consts::PI;

/// Splice and concatenate `keep_ranges` (`(start_secs, end_secs)`) from
/// `buffer` using smooth half-cosine micro-crossfades at every join boundary.
///
/// Ranges are clamped to `[0.0, buffer.duration_secs()]`, aligned to whole
/// channel frames, and processed in the order provided. Adjacent ranges that
/// touch or overlap slightly are smoothly crossfaded without clicks or pops.
pub fn splice_ranges(
  buffer: &AudioBuffer,
  keep_ranges: &[(f64, f64)],
  crossfade_secs: f64,
) -> Result<AudioBuffer> {
  if buffer.sample_rate == 0 {
    return Err(PhononError::InvalidSampleRate(0));
  }
  let ch = buffer.frame_stride()?;
  if !crossfade_secs.is_finite() || crossfade_secs < 0.0 {
    return Err(PhononError::InvalidParam(format!(
      "crossfade_secs must be finite and non-negative, got {crossfade_secs}"
    )));
  }
  if keep_ranges.is_empty() {
    return Err(PhononError::InvalidParam(
      "keep_ranges must contain at least one range".to_string(),
    ));
  }

  let total_frames = buffer.samples.len() / ch;
  let sr = buffer.sample_rate as f64;
  let mut frame_slices: Vec<(usize, usize)> = Vec::with_capacity(keep_ranges.len());

  for &(start_s, end_s) in keep_ranges {
    if !start_s.is_finite() || !end_s.is_finite() {
      return Err(PhononError::InvalidParam(
        "range bounds must be finite".to_string(),
      ));
    }
    if end_s <= start_s {
      return Err(PhononError::InvalidParam(format!(
        "range end ({end_s}) must be greater than start ({start_s})"
      )));
    }
    let start_f = ((start_s.max(0.0) * sr).round() as usize).min(total_frames);
    let end_f = ((end_s.max(0.0) * sr).round() as usize).min(total_frames);
    if end_f > start_f {
      frame_slices.push((start_f, end_f));
    }
  }

  if frame_slices.is_empty() {
    return Err(PhononError::InvalidParam(
      "no audio frames fall inside the requested keep_ranges".to_string(),
    ));
  }

  let req_fade_frames = (crossfade_secs * sr).round() as usize;
  let est_frames: usize = frame_slices.iter().map(|(a, b)| b - a).sum();
  let mut out: Vec<f32> = Vec::with_capacity(est_frames * ch);
  let mut out_frames: usize = 0;

  for (idx, &(start_f, end_f)) in frame_slices.iter().enumerate() {
    let seg_frames = end_f - start_f;
    let seg_samples = &buffer.samples[start_f * ch..end_f * ch];

    if idx == 0 {
      out.extend_from_slice(seg_samples);
      out_frames = seg_frames;
      continue;
    }

    // Clamp crossfade to at most half of each adjacent clip so neither clip is consumed.
    let fade_frames = req_fade_frames.min(out_frames / 2).min(seg_frames / 2);

    if fade_frames > 0 {
      let overlap_start = (out_frames - fade_frames) * ch;
      let denom = fade_frames as f32;
      for f in 0..fade_frames {
        // Equal-amplitude raised-cosine curve: smooth first derivative at endpoints.
        let t = (f as f32 + 0.5) / denom;
        let fade_in = 0.5 * (1.0 - (PI * t).cos());
        let fade_out = 1.0 - fade_in;
        for c in 0..ch {
          let o_idx = overlap_start + f * ch + c;
          let n_idx = f * ch + c;
          out[o_idx] = (out[o_idx] * fade_out + seg_samples[n_idx] * fade_in).clamp(-1.0, 1.0);
        }
      }
      out.extend_from_slice(&seg_samples[fade_frames * ch..]);
      out_frames += seg_frames - fade_frames;
    } else {
      out.extend_from_slice(seg_samples);
      out_frames += seg_frames;
    }
  }

  Ok(AudioBuffer {
    samples: out,
    sample_rate: buffer.sample_rate,
    channels: buffer.channels,
  })
}

/// Detect contiguous active speech regions `(start_secs, end_secs)` using
/// 20 ms windowed RMS energy against `silence_threshold_db` (e.g. `-38.0` dBFS).
///
/// Silent gaps shorter than `min_silence_secs` are bridged so natural word
/// pauses are preserved. Each retained speech region is expanded by
/// `padding_secs` on both sides (clamped to `[0.0, duration]`) and overlapping
/// regions are merged.
pub fn detect_speech_regions(
  buffer: &AudioBuffer,
  silence_threshold_db: f32,
  min_silence_secs: f64,
  padding_secs: f64,
) -> Result<Vec<(f64, f64)>> {
  if buffer.sample_rate == 0 {
    return Err(PhononError::InvalidSampleRate(0));
  }
  let ch = buffer.frame_stride()?;
  if !silence_threshold_db.is_finite() {
    return Err(PhononError::InvalidParam(
      "silence_threshold_db must be finite".to_string(),
    ));
  }
  if !min_silence_secs.is_finite() || min_silence_secs < 0.0 {
    return Err(PhononError::InvalidParam(
      "min_silence_secs must be finite and non-negative".to_string(),
    ));
  }
  if !padding_secs.is_finite() || padding_secs < 0.0 {
    return Err(PhononError::InvalidParam(
      "padding_secs must be finite and non-negative".to_string(),
    ));
  }

  let total_frames = buffer.samples.len() / ch;
  if total_frames == 0 {
    return Ok(Vec::new());
  }

  let sr = buffer.sample_rate as f64;
  let duration = total_frames as f64 / sr;
  let win_frames = ((sr * 0.02).round() as usize).max(1);
  let threshold_amp = 10.0_f32.powf(silence_threshold_db / 20.0);

  let mut active_windows: Vec<(f64, f64)> = Vec::new();
  let mut frame = 0usize;
  while frame < total_frames {
    let end_f = (frame + win_frames).min(total_frames);
    let slice = &buffer.samples[frame * ch..end_f * ch];
    let sum_sq: f64 = slice.iter().map(|&s| (s as f64) * (s as f64)).sum();
    let rms = (sum_sq / slice.len().max(1) as f64).sqrt() as f32;
    if rms >= threshold_amp {
      let s = frame as f64 / sr;
      let e = end_f as f64 / sr;
      if let Some(last) = active_windows.last_mut()
        && s - last.1 <= min_silence_secs
      {
        last.1 = e;
      } else {
        active_windows.push((s, e));
      }
    }
    frame = end_f;
  }

  // Apply padding and merge any newly overlapping regions.
  let mut padded: Vec<(f64, f64)> = Vec::with_capacity(active_windows.len());
  for (s, e) in active_windows {
    let ps = (s - padding_secs).max(0.0);
    let pe = (e + padding_secs).min(duration);
    if let Some(last) = padded.last_mut()
      && ps <= last.1
    {
      last.1 = last.1.max(pe);
    } else {
      padded.push((ps, pe));
    }
  }

  Ok(padded)
}

#[cfg(test)]
mod tests {
  use super::*;

  fn tone_with_silence(sr: u32) -> AudioBuffer {
    // 0.5s silence + 0.6s 440Hz tone + 0.8s silence + 0.6s 660Hz tone + 0.5s silence = 3.0s
    let mut samples = vec![0.0f32; (sr as f64 * 0.5) as usize];
    for i in 0..((sr as f64 * 0.6) as usize) {
      samples.push(0.4 * (2.0 * std::f64::consts::PI * 440.0 * i as f64 / sr as f64).sin() as f32);
    }
    samples.extend(vec![0.0f32; (sr as f64 * 0.8) as usize]);
    for i in 0..((sr as f64 * 0.6) as usize) {
      samples.push(0.4 * (2.0 * std::f64::consts::PI * 660.0 * i as f64 / sr as f64).sin() as f32);
    }
    samples.extend(vec![0.0f32; (sr as f64 * 0.5) as usize]);
    AudioBuffer {
      samples,
      sample_rate: sr,
      channels: 1,
    }
  }

  #[test]
  fn test_detect_speech_regions_finds_two_active_segments() {
    let buf = tone_with_silence(16000);
    let regions = detect_speech_regions(&buf, -35.0, 0.3, 0.05).expect("detect");
    assert_eq!(regions.len(), 2, "got {regions:?}");
    assert!((regions[0].0 - 0.45).abs() < 0.04);
    assert!((regions[0].1 - 1.15).abs() < 0.04);
    assert!((regions[1].0 - 1.85).abs() < 0.04);
    assert!((regions[1].1 - 2.55).abs() < 0.04);
  }

  #[test]
  fn test_splice_ranges_stitches_with_smooth_crossfade() {
    let sr = 16000u32;
    let buf = tone_with_silence(sr);
    let spliced = splice_ranges(&buf, &[(0.5, 1.1), (1.9, 2.5)], 0.02).expect("splice");
    // Expected duration: 0.6 + 0.6 - 0.02 = 1.18s
    assert!(
      (spliced.duration_secs() - 1.18).abs() < 0.01,
      "got {:.3}s",
      spliced.duration_secs()
    );
    assert!(spliced.samples.iter().all(|s| s.is_finite()));
  }

  #[test]
  fn test_splice_ranges_rejects_invalid_inputs() {
    let buf = tone_with_silence(16000);
    assert!(splice_ranges(&buf, &[], 0.01).is_err());
    assert!(splice_ranges(&buf, &[(1.0, 0.5)], 0.01).is_err());
    assert!(splice_ranges(&buf, &[(0.0, 1.0)], -0.1).is_err());
    assert!(splice_ranges(&buf, &[(f64::NAN, 1.0)], 0.01).is_err());
  }

  #[test]
  fn test_splice_ranges_preserves_stereo_frame_alignment() {
    let sr = 44100u32;
    let buf = AudioBuffer {
      samples: vec![0.3; sr as usize * 2 * 2], // 2s stereo
      sample_rate: sr,
      channels: 2,
    };
    let spliced = splice_ranges(&buf, &[(0.0, 0.5), (1.0, 1.5)], 0.015).expect("stereo splice");
    assert_eq!(spliced.channels, 2);
    assert_eq!(spliced.samples.len() % 2, 0);
  }
}
