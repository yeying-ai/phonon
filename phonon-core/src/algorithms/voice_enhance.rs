//! Pure-Rust local broadcast voice enhancement and adaptive STFT spectral
//! noise suppression.
//!
//! Combines four deterministic local stages with zero external binary or model
//! downloads:
//! 1. Sub-bass rumble High-Pass filter (default 80 Hz)
//! 2. Adaptive STFT Wiener-style spectral denoiser (minimum-statistics noise
//!    floor estimation + temporally smoothed gain floor to prevent musical noise)
//! 3. Broadcast vocal presence EQ (mud trim + clarity presence + gentle air)
//! 4. Voice compression + optional BS.1770-4 loudness mastering (-1 dBTP ceiling)

use crate::algorithms::{
  CompressorConfig, EqBand, FilterType, apply_eq, filter, normalize_lufs, with_compression,
};
use crate::{AudioBuffer, PhononError, Result};
use num_complex::Complex;
use rustfft::FftPlanner;
use std::f32::consts::PI;

/// Configuration for the local broadcast voice enhancement chain.
#[derive(Debug, Clone, PartialEq)]
pub struct VoiceEnhanceConfig {
  /// High-pass rumble cutoff in Hz (`<= 0.0` disables). Default: `80.0`.
  pub highpass_hz: f64,
  /// Spectral noise reduction strength in `0.0..=1.0` (`0.0` disables). Default: `0.65`.
  pub denoise_strength: f32,
  /// Apply broadcast vocal EQ (mud cut at 250 Hz, presence at 3.2 kHz, air at 10 kHz).
  pub presence_eq: bool,
  /// Apply smooth speech dynamics compression.
  pub compress: bool,
  /// Optional BS.1770-4 integrated loudness target in LUFS (true-peak ceiling `-1.0 dBTP`).
  pub target_lufs: Option<f32>,
}

impl Default for VoiceEnhanceConfig {
  fn default() -> Self {
    Self::podcast_studio()
  }
}

impl VoiceEnhanceConfig {
  /// Balanced broadcast voice preset for home podcast recordings.
  pub fn podcast_studio() -> Self {
    Self {
      highpass_hz: 80.0,
      denoise_strength: 0.65,
      presence_eq: true,
      compress: true,
      target_lufs: Some(-16.0),
    }
  }

  /// Stronger spectral noise suppression for fans, AC hum, or noisy rooms.
  pub fn noisy_room() -> Self {
    Self {
      highpass_hz: 100.0,
      denoise_strength: 0.9,
      presence_eq: true,
      compress: true,
      target_lufs: Some(-16.0),
    }
  }

  /// Gentle polish for already quiet studio microphones.
  pub fn clean_mic() -> Self {
    Self {
      highpass_hz: 70.0,
      denoise_strength: 0.25,
      presence_eq: true,
      compress: true,
      target_lufs: Some(-16.0),
    }
  }
}

/// Apply adaptive STFT Wiener-style spectral noise suppression per channel.
///
/// Estimates the stationary noise floor per frequency bin using the 10th
/// percentile magnitude across frames, then computes a temporally smoothed
/// Wiener gain with a bounded spectral floor (`-18 * strength` dB) to avoid
/// musical-noise artifacts.
pub fn denoise_spectral(buffer: &AudioBuffer, strength: f32) -> Result<AudioBuffer> {
  if buffer.sample_rate == 0 {
    return Err(PhononError::InvalidSampleRate(0));
  }
  let ch = buffer.frame_stride()?;
  if !strength.is_finite() {
    return Err(PhononError::InvalidParam(
      "denoise_strength must be finite".to_string(),
    ));
  }
  let strength = strength.clamp(0.0, 1.0);
  if strength <= 0.0 || buffer.samples.is_empty() {
    return Ok(buffer.clone());
  }

  let total_frames = buffer.samples.len() / ch;
  let fft_size: usize = if buffer.sample_rate <= 24_000 {
    512
  } else {
    1024
  };
  let hop = fft_size / 2;
  if total_frames < fft_size {
    return Ok(buffer.clone());
  }

  // Analysis/synthesis Hann window (root-Hann so product is exact Hann,
  // which satisfies 50% overlap COLA with constant sum = 1.0).
  let window: Vec<f32> = (0..fft_size)
    .map(|i| {
      let hann = 0.5 * (1.0 - (2.0 * PI * i as f32 / fft_size as f32).cos());
      hann.sqrt()
    })
    .collect();

  let mut planner = FftPlanner::<f32>::new();
  let fft = planner.plan_fft_forward(fft_size);
  let ifft = planner.plan_fft_inverse(fft_size);
  let bins = fft_size / 2 + 1;
  let inv_n = 1.0 / fft_size as f32;

  // Oversubtraction and spectral floor tuned to suppress broadband/hum noise
  // while keeping consonant intelligibility intact.
  let over_sub = 1.0 + 2.2 * strength;
  let min_gain = 10.0_f32.powf((-18.0 * strength) / 20.0);
  let smooth_prev = 0.65_f32;
  let smooth_curr = 1.0 - smooth_prev;

  let mut out_samples = vec![0.0f32; total_frames * ch];

  for c in 0..ch {
    // De-interleave channel c.
    let mut mono = Vec::with_capacity(total_frames);
    for f in 0..total_frames {
      mono.push(buffer.samples[f * ch + c]);
    }

    // Pad slightly so the last hop covers the tail cleanly.
    let num_stft_frames = (total_frames.saturating_sub(fft_size)) / hop + 2;
    let padded_len = (num_stft_frames - 1) * hop + fft_size;
    mono.resize(padded_len, 0.0);

    // Pass 1: STFT forward transform for all frames and compute broadband frame energy.
    let mut spectra: Vec<Vec<Complex<f32>>> = Vec::with_capacity(num_stft_frames);
    let mut frame_energies: Vec<(usize, f32)> = Vec::with_capacity(num_stft_frames);

    for frame_idx in 0..num_stft_frames {
      let start = frame_idx * hop;
      let mut buf: Vec<Complex<f32>> = (0..fft_size)
        .map(|i| Complex::new(mono[start + i] * window[i], 0.0))
        .collect();
      fft.process(&mut buf);
      let energy: f32 = buf[..bins].iter().map(Complex::norm_sqr).sum();
      frame_energies.push((frame_idx, energy));
      spectra.push(buf);
    }

    // Estimate unbiased stationary noise PSD by averaging per-bin power across
    // the quietest 15% of frames (minimum-statistics frame selection avoids
    // Rayleigh single-bin tail bias).
    frame_energies.sort_by(|a, b| a.1.total_cmp(&b.1));
    let noise_frame_count = ((num_stft_frames * 15) / 100)
      .max(1)
      .min(num_stft_frames);
    let mut noise_psd = vec![0.0f32; bins];
    for &(frame_idx, _) in &frame_energies[..noise_frame_count] {
      let spec = &spectra[frame_idx];
      for (k, psd) in noise_psd.iter_mut().enumerate() {
        *psd += spec[k].norm_sqr();
      }
    }
    let inv_noise_frames = 1.0 / noise_frame_count as f32;
    for psd in &mut noise_psd {
      *psd *= inv_noise_frames;
    }

    // Pass 2: Wiener spectral gain + temporal smoothing + IFFT overlap-add.
    let mut prev_gains = vec![min_gain; bins];
    let mut recon = vec![0.0f32; padded_len];
    let mut norm = vec![0.0f32; padded_len];

    for (frame_idx, spec) in spectra.iter_mut().enumerate() {
      for k in 0..bins {
        let mag_sq = spec[k].norm_sqr();
        let raw_gain = if mag_sq <= 1e-12 {
          min_gain
        } else {
          let clean_psd = (mag_sq - over_sub * noise_psd[k]).max(0.0);
          (clean_psd / mag_sq).sqrt().clamp(min_gain, 1.0)
        };
        let gain = (smooth_prev * prev_gains[k] + smooth_curr * raw_gain).clamp(min_gain, 1.0);
        prev_gains[k] = gain;

        spec[k] *= gain;
        if k > 0 && k < fft_size - k {
          spec[fft_size - k] *= gain;
        }
      }

      ifft.process(spec);
      let start = frame_idx * hop;
      for i in 0..fft_size {
        let w = window[i];
        recon[start + i] += spec[i].re * inv_n * w;
        norm[start + i] += w * w;
      }
    }

    // Re-interleave normalized samples.
    for f in 0..total_frames {
      let s = if norm[f] > 1e-6 {
        recon[f] / norm[f]
      } else {
        recon[f]
      };
      out_samples[f * ch + c] = if s.is_finite() {
        s.clamp(-1.0, 1.0)
      } else {
        0.0
      };
    }
  }

  Ok(AudioBuffer {
    samples: out_samples,
    sample_rate: buffer.sample_rate,
    channels: buffer.channels,
  })
}

/// Run the full local broadcast voice enhancement chain on `buffer`.
pub fn enhance_voice(buffer: &AudioBuffer, config: &VoiceEnhanceConfig) -> Result<AudioBuffer> {
  if buffer.sample_rate == 0 {
    return Err(PhononError::InvalidSampleRate(0));
  }
  let _ = buffer.frame_stride()?;
  if !config.highpass_hz.is_finite() {
    return Err(PhononError::InvalidParam(
      "highpass_hz must be finite".to_string(),
    ));
  }
  if let Some(lufs) = config.target_lufs
    && (!lufs.is_finite() || !(-60.0..=-5.0).contains(&lufs))
  {
    return Err(PhononError::InvalidParam(
      "target_lufs must be finite and within [-60.0, -5.0]".to_string(),
    ));
  }

  let mut out = buffer.clone();

  // Stage 1: Rumble / plosive High-Pass filter.
  if config.highpass_hz > 0.0 && config.highpass_hz < (out.sample_rate as f64 * 0.45) {
    filter(&mut out, FilterType::HighPass, config.highpass_hz, 0.707, 0.0)?;
  }

  // Stage 2: Adaptive STFT spectral noise suppression.
  if config.denoise_strength > 0.0 {
    out = denoise_spectral(&out, config.denoise_strength)?;
  }

  // Stage 3: Broadcast vocal presence EQ (auto-skips bands above Nyquist).
  if config.presence_eq {
    let nyquist = out.sample_rate as f64 * 0.48;
    let mut bands = Vec::with_capacity(3);
    if 250.0 < nyquist {
      bands.push(EqBand::new(FilterType::Peaking, 250.0, 1.2, -2.0));
    }
    if 3200.0 < nyquist {
      bands.push(EqBand::new(FilterType::Peaking, 3200.0, 1.0, 2.5));
    }
    if 10_000.0 < nyquist {
      bands.push(EqBand::new(FilterType::HighShelf, 10_000.0, 0.707, 1.0));
    }
    if !bands.is_empty() {
      apply_eq(&mut out, &bands);
    }
  }

  // Stage 4: Smooth vocal compression + broadcast LUFS mastering.
  if config.compress {
    out = with_compression(&out, &CompressorConfig::voice());
  }
  if let Some(target) = config.target_lufs {
    normalize_lufs(&mut out, target, -1.0);
  }

  Ok(out)
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::algorithms::rms;

  fn noisy_speech_like_buffer(sr: u32) -> AudioBuffer {
    // 0.6s stationary hum/hiss noise + 1.0s strong vocal tone + 0.6s noise = 2.2s
    let total = (sr as f64 * 2.2) as usize;
    let mut samples = Vec::with_capacity(total);
    let mut seed = 0x1234_5678u32;
    for i in 0..total {
      // Deterministic LCG white noise floor (~0.04 amp) + 50 Hz hum (0.04 amp)
      seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
      let white = ((seed >> 9) as f32 / (1u32 << 23) as f32) * 2.0 - 1.0;
      let t = i as f64 / sr as f64;
      let hum = 0.04 * (2.0 * std::f64::consts::PI * 50.0 * t).sin() as f32;
      let mut s = 0.04 * white + hum;
      if (0.6..1.6).contains(&t) {
        // Simulated vocal formants at 300 Hz + 1200 Hz
        s += 0.35 * (2.0 * std::f64::consts::PI * 300.0 * t).sin() as f32
          + 0.2 * (2.0 * std::f64::consts::PI * 1200.0 * t).sin() as f32;
      }
      samples.push(s.clamp(-1.0, 1.0));
    }
    AudioBuffer {
      samples,
      sample_rate: sr,
      channels: 1,
    }
  }

  #[test]
  fn test_denoise_spectral_reduces_stationary_noise_floor() {
    let sr = 16000u32;
    let noisy = noisy_speech_like_buffer(sr);
    let denoised = denoise_spectral(&noisy, 0.85).expect("denoise");
    assert_eq!(denoised.samples.len(), noisy.samples.len());
    assert!(denoised.samples.iter().all(|s| s.is_finite()));

    // Measure noise floor in the leading 0.5s (skip very first window ramp).
    let start = (sr as f64 * 0.1) as usize;
    let end = (sr as f64 * 0.5) as usize;
    let noise_before = rms(&AudioBuffer {
      samples: noisy.samples[start..end].to_vec(),
      sample_rate: sr,
      channels: 1,
    });
    let noise_after = rms(&AudioBuffer {
      samples: denoised.samples[start..end].to_vec(),
      sample_rate: sr,
      channels: 1,
    });
    assert!(
      noise_after < noise_before * 0.45,
      "expected significant noise reduction: before={noise_before:.4}, after={noise_after:.4}"
    );

    // Speech region (0.7s..1.5s) should remain loud and intact.
    let sp_start = (sr as f64 * 0.7) as usize;
    let sp_end = (sr as f64 * 1.5) as usize;
    let speech_after = rms(&AudioBuffer {
      samples: denoised.samples[sp_start..sp_end].to_vec(),
      sample_rate: sr,
      channels: 1,
    });
    assert!(
      speech_after > noise_after * 6.0,
      "SNR should strongly improve: speech={speech_after:.4}, noise={noise_after:.4}"
    );
  }

  #[test]
  fn test_enhance_voice_hits_broadcast_lufs_and_safe_true_peak() {
    let noisy = noisy_speech_like_buffer(44100);
    let enhanced = enhance_voice(&noisy, &VoiceEnhanceConfig::podcast_studio()).expect("enhance");
    let report = crate::algorithms::measure_loudness(&enhanced);
    assert!(
      (report.integrated_lufs - (-16.0)).abs() < 1.2,
      "got {} LUFS",
      report.integrated_lufs
    );
    assert!(
      report.true_peak_dbtp <= -0.8,
      "expected true peak <= -0.8 dBTP, got {}",
      report.true_peak_dbtp
    );
  }

  #[test]
  fn test_enhance_voice_works_at_16khz_and_rejects_bad_config() {
    let noisy = noisy_speech_like_buffer(16000);
    let enhanced = enhance_voice(&noisy, &VoiceEnhanceConfig::noisy_room()).expect("16khz");
    assert!(enhanced.samples.iter().all(|s| s.is_finite()));

    let mut bad = VoiceEnhanceConfig::podcast_studio();
    bad.target_lufs = Some(2.0);
    assert!(enhance_voice(&noisy, &bad).is_err());
  }
}
