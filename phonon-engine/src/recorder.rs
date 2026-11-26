//! Real-time microphone capture backed by `cpal`.
//!
//! Captures interleaved `f32` PCM from the system default input device with
//! lock-free live peak metering (`input_peak`) and duration tracking
//! (`duration_secs`), plus a hard maximum duration cap to avoid unbounded
//! memory growth during long sessions.

use crate::{EngineError, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use phonon_core::AudioBuffer;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use tracing::{error, info, instrument};

/// Default recording cap (30 minutes) to bound RAM usage.
pub const DEFAULT_MAX_RECORD_SECS: f64 = 1800.0;

/// Internal shared capture state between the `cpal` input callback and caller.
///
/// Only the input callback holds the `samples` mutex while recording is active,
/// and it acquires it with `try_lock` so it never blocks. Calling
/// [`AudioRecorder::stop`] drops the input stream first, then takes the buffer.
struct CaptureState {
  samples: Mutex<Vec<f32>>,
  recorded_samples: AtomicUsize,
  peak_bits: AtomicU32,
  paused: AtomicBool,
  capped: AtomicBool,
  sample_rate: u32,
  channels: u16,
  max_samples: usize,
}

impl CaptureState {
  fn new(sample_rate: u32, channels: u16, max_secs: f64) -> Self {
    let ch = channels.max(1) as usize;
    let clamped_secs = if max_secs.is_finite() && max_secs > 0.0 {
      max_secs
    } else {
      DEFAULT_MAX_RECORD_SECS
    };
    let max_frames = (sample_rate as f64 * clamped_secs).round().max(1.0) as usize;
    let max_samples = max_frames.saturating_mul(ch);
    // Pre-allocate up to 15 seconds to minimize reallocations in the callback.
    let initial_cap = (sample_rate as usize)
      .saturating_mul(ch)
      .saturating_mul(15)
      .min(max_samples);

    Self {
      samples: Mutex::new(Vec::with_capacity(initial_cap)),
      recorded_samples: AtomicUsize::new(0),
      peak_bits: AtomicU32::new(0.0f32.to_bits()),
      paused: AtomicBool::new(false),
      capped: AtomicBool::new(false),
      sample_rate,
      channels,
      max_samples,
    }
  }

  fn push_f32(&self, input: &[f32]) {
    if input.is_empty() {
      return;
    }
    let mut block_peak = 0.0f32;
    for &s in input {
      if s.is_finite() {
        block_peak = block_peak.max(s.abs());
      }
    }
    self
      .peak_bits
      .store(block_peak.min(1.0).to_bits(), Ordering::Relaxed);

    if self.paused.load(Ordering::Relaxed) || self.capped.load(Ordering::Relaxed) {
      return;
    }

    if let Ok(mut buf) = self.samples.try_lock() {
      let remaining = self.max_samples.saturating_sub(buf.len());
      if remaining == 0 {
        self.capped.store(true, Ordering::Relaxed);
        return;
      }
      let take = input.len().min(remaining);
      buf.extend(input[..take].iter().map(|&s| {
        if s.is_finite() {
          s.clamp(-1.0, 1.0)
        } else {
          0.0
        }
      }));
      self.recorded_samples.store(buf.len(), Ordering::Relaxed);
      if take < input.len() {
        self.capped.store(true, Ordering::Relaxed);
      }
    }
  }

  fn push_i16(&self, input: &[i16], scratch: &mut Vec<f32>) {
    scratch.clear();
    scratch.extend(input.iter().map(|&s| s as f32 / 32768.0));
    self.push_f32(scratch);
  }

  fn push_u16(&self, input: &[u16], scratch: &mut Vec<f32>) {
    scratch.clear();
    scratch.extend(input.iter().map(|&s| (s as f32 / 32768.0) - 1.0));
    self.push_f32(scratch);
  }

  fn duration_secs(&self) -> f64 {
    let total = self.recorded_samples.load(Ordering::Relaxed);
    let denom = self.sample_rate as f64 * self.channels.max(1) as f64;
    if denom > 0.0 {
      total as f64 / denom
    } else {
      0.0
    }
  }

  fn input_peak(&self) -> f32 {
    f32::from_bits(self.peak_bits.load(Ordering::Relaxed))
  }

  fn finish(&self) -> Result<AudioBuffer> {
    let mut guard = self
      .samples
      .lock()
      .map_err(|_| EngineError::Device("Recorder sample lock poisoned".to_string()))?;
    let mut samples = std::mem::take(&mut *guard);
    let ch = self.channels.max(1) as usize;
    let aligned_len = (samples.len() / ch) * ch;
    samples.truncate(aligned_len);
    Ok(AudioBuffer {
      samples,
      sample_rate: self.sample_rate,
      channels: self.channels,
    })
  }
}

/// Real-time microphone recorder using the system default audio input device.
pub struct AudioRecorder {
  stream: cpal::Stream,
  state: Arc<CaptureState>,
}

impl AudioRecorder {
  /// Starts recording from the default input device with the default 30-minute cap.
  #[instrument]
  pub fn start() -> Result<Self> {
    Self::start_with_max_secs(DEFAULT_MAX_RECORD_SECS)
  }

  /// Starts recording from the default input device with a custom maximum duration.
  #[instrument]
  pub fn start_with_max_secs(max_secs: f64) -> Result<Self> {
    let host = cpal::default_host();
    let device = host
      .default_input_device()
      .ok_or_else(|| EngineError::Device("No audio input (microphone) device available".into()))?;

    let config = device
      .default_input_config()
      .map_err(|e| EngineError::Config(e.to_string()))?;

    let sample_rate = config.sample_rate().0;
    let channels = config.channels();
    if sample_rate == 0 || channels == 0 {
      return Err(EngineError::Config(format!(
        "Invalid input device format: {sample_rate}Hz / {channels}ch"
      )));
    }
    info!(sample_rate, channels, "Initialized audio input device");

    let state = Arc::new(CaptureState::new(sample_rate, channels, max_secs));
    let state_clone = state.clone();
    let err_fn = |err| error!("an error occurred on input stream: {}", err);

    let stream = match config.sample_format() {
      cpal::SampleFormat::F32 => device.build_input_stream(
        &config.into(),
        move |data: &[f32], _: &cpal::InputCallbackInfo| state_clone.push_f32(data),
        err_fn,
        None,
      ),
      cpal::SampleFormat::I16 => {
        let mut scratch = Vec::with_capacity(4096);
        device.build_input_stream(
          &config.into(),
          move |data: &[i16], _: &cpal::InputCallbackInfo| state_clone.push_i16(data, &mut scratch),
          err_fn,
          None,
        )
      }
      cpal::SampleFormat::U16 => {
        let mut scratch = Vec::with_capacity(4096);
        device.build_input_stream(
          &config.into(),
          move |data: &[u16], _: &cpal::InputCallbackInfo| state_clone.push_u16(data, &mut scratch),
          err_fn,
          None,
        )
      }
      other => {
        return Err(EngineError::Config(format!(
          "Unsupported input sample format: {other:?}"
        )));
      }
    }
    .map_err(|e| EngineError::Device(e.to_string()))?;

    stream
      .play()
      .map_err(|e| EngineError::Play(e.to_string()))?;

    Ok(Self { stream, state })
  }

  /// Input device sample rate in Hz.
  pub fn sample_rate(&self) -> u32 {
    self.state.sample_rate
  }

  /// Input device channel count.
  pub fn channels(&self) -> u16 {
    self.state.channels
  }

  /// Temporarily pause recording without closing the input stream.
  pub fn pause(&self) {
    self.state.paused.store(true, Ordering::Relaxed);
  }

  /// Resume a paused recording.
  pub fn resume(&self) {
    self.state.paused.store(false, Ordering::Relaxed);
  }

  /// Whether recording is currently paused.
  pub fn is_paused(&self) -> bool {
    self.state.paused.load(Ordering::Relaxed)
  }

  /// Whether the maximum recording duration cap has been reached.
  pub fn is_capped(&self) -> bool {
    self.state.capped.load(Ordering::Relaxed)
  }

  /// Recorded duration in seconds so far (lock-free).
  pub fn duration_secs(&self) -> f64 {
    self.state.duration_secs()
  }

  /// Recent input peak amplitude in `[0.0, 1.0]` for live VU metering (lock-free).
  pub fn input_peak(&self) -> f32 {
    self.state.input_peak()
  }

  /// Stops capturing, closes the input stream, and returns the recorded [`AudioBuffer`].
  pub fn stop(self) -> Result<AudioBuffer> {
    let AudioRecorder { stream, state } = self;
    // Drop the cpal input stream before taking samples so no callback is in flight.
    drop(stream);
    state.finish()
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_capture_state_f32_peak_and_finish() {
    let state = CaptureState::new(16_000, 1, 10.0);
    state.push_f32(&[0.0, 0.25, -0.6, 0.4]);
    assert!((state.input_peak() - 0.6).abs() < 1e-5);
    assert!((state.duration_secs() - (4.0 / 16_000.0)).abs() < 1e-7);

    let buf = state.finish().unwrap();
    assert_eq!(buf.sample_rate, 16_000);
    assert_eq!(buf.channels, 1);
    assert_eq!(buf.samples, vec![0.0, 0.25, -0.6, 0.4]);
  }

  #[test]
  fn test_capture_state_pause_skips_samples_but_updates_meter() {
    let state = CaptureState::new(16_000, 1, 10.0);
    state.push_f32(&[0.2, 0.2]);
    state.paused.store(true, Ordering::Relaxed);
    state.push_f32(&[0.8, -0.9]);
    assert!((state.input_peak() - 0.9).abs() < 1e-5);
    let buf = state.finish().unwrap();
    assert_eq!(buf.samples.len(), 2);
  }

  #[test]
  fn test_capture_state_enforces_max_duration_cap_and_frame_alignment() {
    let state = CaptureState::new(100, 2, 0.05); // 5 stereo frames = 10 samples max
    state.push_f32(&[0.5; 7]);
    assert!(!state.capped.load(Ordering::Relaxed));
    state.push_f32(&[0.5; 6]);
    assert!(state.capped.load(Ordering::Relaxed));

    let buf = state.finish().unwrap();
    assert_eq!(buf.samples.len(), 10);
    assert_eq!(buf.samples.len() % 2, 0);
  }

  #[test]
  fn test_capture_state_i16_and_u16_conversions_and_nan_sanitization() {
    let state = CaptureState::new(16_000, 1, 10.0);
    let mut scratch = Vec::new();
    state.push_i16(&[0, 16384, -32768], &mut scratch);
    state.push_u16(&[32768, 65535], &mut scratch);
    state.push_f32(&[f32::NAN, f32::INFINITY, -2.5]);

    let buf = state.finish().unwrap();
    assert_eq!(buf.samples.len(), 8);
    assert!((buf.samples[1] - 0.5).abs() < 1e-4);
    assert!((buf.samples[2] - (-1.0)).abs() < 1e-4);
    assert!((buf.samples[3] - 0.0).abs() < 1e-4);
    assert!(buf.samples[4] > 0.99);
    assert_eq!(buf.samples[5], 0.0); // NaN sanitized
    assert_eq!(buf.samples[6], 0.0); // Inf sanitized
    assert_eq!(buf.samples[7], -1.0); // Clamped
  }
}
