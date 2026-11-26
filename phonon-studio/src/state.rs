//! Shared Dioxus signal state and audio playback orchestration for Phonon Studio.

use crate::project::{EpisodeRenderView, ProjectHistory, SegmentAudioEntry, StudioProject};
use crate::studio_ops::{
  ActivePluginSpec, ProviderStatus, generate_waveform_path, inspect_providers,
  instantiate_active_plugins,
};
use dioxus::prelude::*;
use phonon_core::{AudioBuffer, PluginRegistry, algorithms};
use phonon_engine::{AudioPlayer, PlaybackState};
use phonon_podcast::Episode;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

/// Alias preserving compatibility with existing Audio Lab components.
pub type ActivePluginState = ActivePluginSpec;

/// Active top-level workspace tab.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkspaceTab {
  PodcastStudio,
  AudioLab,
}

/// Active tab in the Podcast Studio right-hand episode inspector.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InspectorTab {
  Master,
  Chapters,
  Subtitles,
}

/// Lightweight display metadata for Audio Lab (no raw samples — safe to clone).
#[derive(Clone, Debug, PartialEq)]
pub struct AudioDisplayInfo {
  pub path: String,
  pub sample_rate: u32,
  pub channels: u16,
  pub duration: f64,
  pub sample_count: usize,
  pub waveform_path: String,
  pub rms: f32,
  pub peak: f32,
  pub peak_dbfs: f32,
  pub integrated_lufs: f32,
  pub true_peak_dbtp: f32,
  /// Pre-computed spectrum bins (magnitudes for first channel, downsampled to 64 bars).
  pub spectrum_bars: Vec<f32>,
}

/// Effect parameters controlled by the user in Audio Lab.
#[derive(Clone, Debug, PartialEq)]
pub struct EffectParams {
  pub normalize_target: f32,
  pub lufs_target: f32,
  pub fade_in_secs: f64,
  pub fade_out_secs: f64,
  pub speed_factor: f64,
  pub compress_preset: String,
  /// EQ preset name ("none", "voice", "bass_heavy", "bright", "custom").
  pub eq_preset: String,
  /// Custom EQ band gains in dB (5 bands: sub, low, mid, high-mid, high).
  pub eq_gains: [f64; 5],
}

impl Default for EffectParams {
  fn default() -> Self {
    Self {
      normalize_target: 0.9,
      lufs_target: -16.0,
      fade_in_secs: 0.0,
      fade_out_secs: 0.0,
      speed_factor: 1.0,
      compress_preset: "none".to_string(),
      eq_preset: "none".to_string(),
      eq_gains: [0.0; 5],
    }
  }
}

/// Which visualisation tab is active in Audio Lab.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VizMode {
  Waveform,
  Spectrum,
}

/// Sidebar panel visibility in Audio Lab.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SidePanel {
  None,
  Effects,
  Eq,
  Export,
  Plugins,
}

/// Global application state provided via Dioxus context.
#[derive(Clone, Copy)]
pub struct AppState {
  // ── Navigation ───────────────────────────────────────────────────────
  pub active_tab: Signal<WorkspaceTab>,
  pub status_msg: Signal<String>,

  // ── AI Podcast Studio ────────────────────────────────────────────────
  pub project: Signal<StudioProject>,
  pub project_path: Signal<Option<PathBuf>>,
  pub history: Signal<ProjectHistory>,
  pub provider_statuses: Signal<Vec<ProviderStatus>>,
  pub segment_audio: Signal<HashMap<usize, SegmentAudioEntry>>,
  pub synthesizing_segments: Signal<HashSet<usize>>,
  pub is_assembling: Signal<bool>,
  pub is_drafting: Signal<bool>,
  pub show_ai_draft: Signal<bool>,
  pub rendered_episode: Signal<Option<Arc<Episode>>>,
  pub episode_view: Signal<Option<EpisodeRenderView>>,
  pub inspector_tab: Signal<InspectorTab>,
  pub episode_mp3_bitrate: Signal<u32>,

  // ── Audio Lab (Single-File DSP) ──────────────────────────────────────
  pub display_info: Signal<Option<AudioDisplayInfo>>,
  pub audio_buffer: Signal<Option<Arc<AudioBuffer>>>,
  pub effects: Signal<EffectParams>,
  pub side_panel: Signal<SidePanel>,
  pub export_bitrate: Signal<u32>,
  pub bake_plugins_on_export: Signal<bool>,
  pub viz_mode: Signal<VizMode>,
  pub plugin_registry: Signal<Arc<PluginRegistry>>,
  pub active_plugins: Signal<Vec<ActivePluginSpec>>,

  // ── Shared Real-Time Transport ───────────────────────────────────────
  pub player_state: Signal<Option<AudioPlayer>>,
  pub volume: Signal<f32>,
  pub playback_source: Signal<String>,
  pub active_track_token: Signal<u64>,
  pub loaded_track_token: Signal<u64>,
  pub playhead_secs: Signal<f64>,
  pub playhead_duration_secs: Signal<f64>,
  pub playback_status: Signal<PlaybackState>,
}

impl AppState {
  pub fn init() -> Self {
    Self {
      active_tab: Signal::new(WorkspaceTab::PodcastStudio),
      status_msg: Signal::new("Ready — Edit script or click Preview on any segment".to_string()),

      project: Signal::new(StudioProject::starter()),
      project_path: Signal::new(None),
      history: Signal::new(ProjectHistory::new()),
      provider_statuses: Signal::new(inspect_providers()),
      segment_audio: Signal::new(HashMap::new()),
      synthesizing_segments: Signal::new(HashSet::new()),
      is_assembling: Signal::new(false),
      is_drafting: Signal::new(false),
      show_ai_draft: Signal::new(false),
      rendered_episode: Signal::new(None),
      episode_view: Signal::new(None),
      inspector_tab: Signal::new(InspectorTab::Master),
      episode_mp3_bitrate: Signal::new(192),

      display_info: Signal::new(None),
      audio_buffer: Signal::new(None),
      effects: Signal::new(EffectParams::default()),
      side_panel: Signal::new(SidePanel::Effects),
      export_bitrate: Signal::new(192),
      bake_plugins_on_export: Signal::new(true),
      viz_mode: Signal::new(VizMode::Waveform),
      plugin_registry: Signal::new(Arc::new(PluginRegistry::with_builtins())),
      active_plugins: Signal::new(Vec::new()),

      player_state: Signal::new(None),
      volume: Signal::new(1.0),
      playback_source: Signal::new("Idle".to_string()),
      active_track_token: Signal::new(0),
      loaded_track_token: Signal::new(0),
      playhead_secs: Signal::new(0.0),
      playhead_duration_secs: Signal::new(0.0),
      playback_status: Signal::new(PlaybackState::Stopped),
    }
  }

  /// Snapshot current project before applying a structural edit so Undo works.
  pub fn mutate_project<F, R>(&mut self, edit: F) -> R
  where
    F: FnOnce(&mut StudioProject) -> R,
  {
    let snapshot = self.project.read().clone();
    self.history.write().push_snapshot(&snapshot);
    let mut proj = self.project.write();
    edit(&mut proj)
  }

  /// Undo the previous project mutation.
  pub fn undo_project(&mut self) -> bool {
    let mut proj = self.project.write();
    let changed = self.history.write().undo(&mut proj);
    if changed {
      self.status_msg.set("Undid last change".to_string());
    }
    changed
  }

  /// Redo a previously undone project mutation.
  pub fn redo_project(&mut self) -> bool {
    let mut proj = self.project.write();
    let changed = self.history.write().redo(&mut proj);
    if changed {
      self.status_msg.set("Redid change".to_string());
    }
    changed
  }
}

/// Computes spectrum bars (downsampled to `num_bars` buckets).
pub fn compute_spectrum_bars(buffer: &AudioBuffer, num_bars: usize) -> Vec<f32> {
  let spectra = match algorithms::compute_spectrum(buffer) {
    Ok(s) if !s.is_empty() => s,
    _ => return vec![0.0; num_bars],
  };

  let mags = &spectra[0];
  if mags.is_empty() {
    return vec![0.0; num_bars];
  }

  let useful_bins = mags.len();
  let mut bars = Vec::with_capacity(num_bars);

  for bar_idx in 0..num_bars {
    let lo = ((bar_idx as f64 / num_bars as f64).powi(2) * useful_bins as f64) as usize;
    let hi = (((bar_idx + 1) as f64 / num_bars as f64).powi(2) * useful_bins as f64) as usize;
    let lo = lo.min(useful_bins);
    let hi = hi.max(lo + 1).min(useful_bins);

    let sum: f32 = mags[lo..hi].iter().sum();
    let avg = sum / (hi - lo) as f32;
    bars.push(avg);
  }

  let max_bar = bars.iter().fold(0.0f32, |a, &b| a.max(b));
  if max_bar > 0.0 {
    for b in &mut bars {
      *b /= max_bar;
    }
  }
  bars
}

/// Builds an [`AudioDisplayInfo`] (waveform, spectrum, RMS/peak, BS.1770 LUFS)
/// from a loaded Audio Lab buffer.
pub fn build_display_info(buffer: &AudioBuffer, path: &str) -> AudioDisplayInfo {
  let waveform = generate_waveform_path(&buffer.samples, 1000.0, 200.0);
  let spectrum_bars = compute_spectrum_bars(buffer, 64);
  let loudness = algorithms::measure_loudness(buffer);
  AudioDisplayInfo {
    path: path.to_string(),
    sample_rate: buffer.sample_rate,
    channels: buffer.channels,
    duration: buffer.duration_secs(),
    sample_count: buffer.samples.len(),
    waveform_path: waveform,
    rms: algorithms::rms(buffer),
    peak: algorithms::peak(buffer),
    peak_dbfs: algorithms::peak_dbfs(buffer),
    integrated_lufs: loudness.integrated_lufs,
    true_peak_dbtp: loudness.true_peak_dbtp,
    spectrum_bars,
  }
}

/// Ensure the shared `AudioPlayer` is initialized and returns `(rate, channels)`.
pub fn ensure_player(state: &mut AppState) -> Result<(u32, u16), String> {
  if state.player_state.read().is_none() {
    let player = AudioPlayer::new().map_err(|e| format!("Audio Init Failed: {e}"))?;
    player.set_volume((state.volume)());
    state.player_state.set(Some(player));
  }
  let reader = state.player_state.read();
  let player = reader
    .as_ref()
    .ok_or_else(|| "Audio player unavailable".to_string())?;
  Ok((player.device_sample_rate(), player.device_channels()))
}

/// Play an arbitrary [`AudioBuffer`] through the shared `AudioPlayer`.
///
/// Resamples/channel-converts off the UI thread, stops any previously paused
/// track cleanly, syncs Audio Lab plugins only when `apply_lab_plugins` is
/// `true`, and optionally seeks to `start_secs`.
pub async fn play_buffer(
  state: &mut AppState,
  source: Arc<AudioBuffer>,
  label: String,
  apply_lab_plugins: bool,
  start_secs: Option<f64>,
) {
  let (device_rate, device_channels) = match ensure_player(state) {
    Ok(cfg) => cfg,
    Err(e) => {
      state.status_msg.set(e);
      return;
    }
  };

  let vol = (state.volume)();
  let duration = source.duration_secs();
  let src_rate = source.sample_rate;
  let src_ch = source.channels;

  // Sync or clear real-time plugin chain according to playback source.
  let plugins_to_sync = if apply_lab_plugins {
    let registry = (state.plugin_registry)();
    let specs = state.active_plugins.read().clone();
    Some(instantiate_active_plugins(&registry, &specs))
  } else {
    None
  };
  if let Some(player) = state.player_state.read().as_ref() {
    player.set_volume(vol);
    match plugins_to_sync {
      Some(plugins) => player.replace_plugins(plugins),
      None => player.clear_effects(),
    }
  }

  state.status_msg.set(format!(
    "Preparing {} ({}Hz/{}ch -> {}Hz/{}ch)...",
    label, src_rate, src_ch, device_rate, device_channels
  ));

  let prepared = tokio::task::spawn_blocking(move || {
    let mut buffer = AudioBuffer::clone(&source);
    if buffer.sample_rate != device_rate {
      buffer = algorithms::resample(&buffer, device_rate)?;
    }
    if buffer.channels != device_channels {
      buffer = algorithms::ensure_channels(&buffer, device_channels)?;
    }
    Ok::<_, phonon_core::PhononError>(buffer)
  })
  .await;

  let buffer = match prepared {
    Ok(Ok(buffer)) => buffer,
    Ok(Err(e)) => {
      state.status_msg.set(format!("Playback Error: {e}"));
      return;
    }
    Err(e) => {
      state.status_msg.set(format!("Playback task error: {e}"));
      return;
    }
  };

  let play_res = match state.player_state.read().as_ref() {
    Some(player) => {
      let res = player.play(&buffer);
      if res.is_ok()
        && let Some(secs) = start_secs
        && secs > 0.0
      {
        let _ = player.seek(secs);
      }
      res
    }
    None => return,
  };

  match play_res {
    Ok(()) => {
      let next_token = (state.active_track_token)().wrapping_add(1);
      state.active_track_token.set(next_token);
      state.loaded_track_token.set(next_token);
      state.playback_source.set(label.clone());
      state.playhead_duration_secs.set(duration);
      state
        .playhead_secs
        .set(start_secs.unwrap_or(0.0).clamp(0.0, duration));
      state.playback_status.set(PlaybackState::Playing);
      state.status_msg.set(format!("Playing: {label}"));
    }
    Err(e) => state.status_msg.set(format!("Playback Error: {e}")),
  }
}

/// Decode `path` and compute its display info on the blocking pool for Audio Lab.
/// Stops any currently paused player track so clicking Play loads the new file.
pub async fn load_audio(state: &mut AppState, path: PathBuf, done_msg: &str) {
  let path_str = path.to_string_lossy().to_string();
  state
    .status_msg
    .set(format!("Loading {}...", path.display()));

  let loaded = tokio::task::spawn_blocking(move || {
    phonon_io::read_audio(&path).map(|buf| {
      let info = build_display_info(&buf, &path_str);
      (buf, info)
    })
  })
  .await;

  match loaded {
    Ok(Ok((buf, info))) => {
      if let Some(player) = state.player_state.read().as_ref() {
        player.stop();
      }
      let next_token = (state.active_track_token)().wrapping_add(1);
      state.active_track_token.set(next_token);
      state.playback_status.set(PlaybackState::Stopped);
      state.playhead_secs.set(0.0);
      state.playhead_duration_secs.set(info.duration);
      state.display_info.set(Some(info));
      state.audio_buffer.set(Some(Arc::new(buf)));
      state.status_msg.set(done_msg.to_string());
    }
    Ok(Err(e)) => state.status_msg.set(format!("Error: {e}")),
    Err(e) => state.status_msg.set(format!("Task error: {e}")),
  }
}

/// Apply `transform` to a copy of the loaded Audio Lab buffer on the blocking
/// pool, recompute display info, and invalidate paused player state.
pub async fn transform_audio<F>(state: &mut AppState, transform: F, done_msg: String)
where
  F: FnOnce(&mut AudioBuffer) + Send + 'static,
{
  let Some(source) = state.audio_buffer.read().clone() else {
    return;
  };
  let path = (state.display_info)().map(|i| i.path).unwrap_or_default();
  state.status_msg.set("Processing...".to_string());

  let processed = tokio::task::spawn_blocking(move || {
    let mut buf = AudioBuffer::clone(&source);
    transform(&mut buf);
    let info = build_display_info(&buf, &path);
    (buf, info)
  })
  .await;

  match processed {
    Ok((buf, info)) => {
      if let Some(player) = state.player_state.read().as_ref() {
        player.stop();
      }
      let next_token = (state.active_track_token)().wrapping_add(1);
      state.active_track_token.set(next_token);
      state.playback_status.set(PlaybackState::Stopped);
      state.playhead_secs.set(0.0);
      state.playhead_duration_secs.set(info.duration);
      state.display_info.set(Some(info));
      state.audio_buffer.set(Some(Arc::new(buf)));
      state.status_msg.set(done_msg);
    }
    Err(e) => state.status_msg.set(format!("Task error: {e}")),
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_build_display_info_computes_lufs_and_waveform() {
    let sr = 44100u32;
    let samples: Vec<f32> = (0..sr)
      .map(|i| 0.3 * (2.0 * std::f64::consts::PI * 440.0 * i as f64 / sr as f64).sin() as f32)
      .collect();
    let buf = AudioBuffer {
      samples,
      sample_rate: sr,
      channels: 1,
    };
    let info = build_display_info(&buf, "tone.wav");
    assert_eq!(info.sample_rate, 44100);
    assert_eq!(info.channels, 1);
    assert!((info.duration - 1.0).abs() < 1e-3);
    assert!(info.integrated_lufs.is_finite());
    assert!(info.true_peak_dbtp.is_finite());
    assert!(info.waveform_path.starts_with("M 0"));
    assert_eq!(info.spectrum_bars.len(), 64);
  }
}
