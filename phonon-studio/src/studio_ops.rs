//! Headless async & DSP operations for Phonon Studio.
//!
//! Bridges [`StudioProject`] with `phonon-ai`, `phonon-podcast`, `phonon-core`,
//! `phonon-engine`, and `phonon-io` without depending on Dioxus signals so
//! every workflow can be unit-tested independently.

use crate::project::{
  ChapterView, EpisodeRenderView, SegmentAudioEntry, SegmentMediaOverride, StudioProject,
  TranscriptCutSpan, spans_from_speech_regions, spans_from_transcript,
};
use phonon_ai::{CachingTts, Config, SttProvider, TranscribeOptions, TtsProvider, builtin_presets};
use phonon_core::{
  AudioBuffer, DspPlugin, PluginRegistry, VoiceEnhanceConfig, algorithms, detect_speech_regions,
  enhance_voice, splice_ranges,
};
use phonon_io::{FlacConfig, Mp3Config, OggConfig};
use phonon_podcast::{
  Episode, OpenAiScriptGenerator, PodcastScript, ScriptGenerator, ScriptRequest,
  render_episode_from_buffers,
};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::SystemTime;
use tokio::sync::Semaphore;

static STUDIO_TMP_SEQ: AtomicU64 = AtomicU64::new(0);

/// Runtime summary of a built-in AI provider preset and whether its API key is
/// present in the process environment. Secret values are never stored or exposed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderStatus {
  pub name: &'static str,
  pub key_env: Option<&'static str>,
  pub key_present: bool,
  pub has_tts: bool,
  pub has_llm: bool,
  pub default_tts_model: Option<&'static str>,
  pub voices: &'static [&'static str],
}

/// Snapshot of an active DSP plugin configured in the Audio Lab plugin chain.
#[derive(Debug, Clone, PartialEq)]
pub struct ActivePluginSpec {
  pub id: String,
  pub name: String,
  /// `(param_name, current_value, min, max, default)`
  pub params: Vec<(String, f32, f32, f32, f32)>,
}

/// Inspect all built-in AI provider presets and environment key availability.
pub fn inspect_providers() -> Vec<ProviderStatus> {
  builtin_presets()
    .into_iter()
    .map(|p| {
      let key_present = match p.key_env {
        None => true,
        Some(var) => std::env::var(var).is_ok_and(|v| !v.trim().is_empty()),
      };
      ProviderStatus {
        name: p.name,
        key_env: p.key_env,
        key_present,
        has_tts: p.has_tts,
        has_llm: p.has_llm,
        default_tts_model: p.default_tts_model,
        voices: p.voices,
      }
    })
    .collect()
}

/// Return the verified voice list for a built-in provider name (empty if unknown/auto).
pub fn preset_voices_for(provider: Option<&str>) -> &'static [&'static str] {
  let Some(name) = provider else {
    // Auto defaults to Edge when no keys are set, so expose Edge voices as quick picks.
    return builtin_presets()
      .into_iter()
      .find(|p| p.name == "edge")
      .map_or(&[][..], |p| p.voices);
  };
  builtin_presets()
    .into_iter()
    .find(|p| p.name == name)
    .map_or(&[][..], |p| p.voices)
}

/// Build a `phonon_ai::Config` applying studio overrides on top of any local
/// `phonon.toml` / `~/.config/phonon/config.toml`.
pub fn build_ai_config(
  tts_provider: Option<&str>,
  tts_model: Option<&str>,
  llm_provider: Option<&str>,
) -> Result<Config, String> {
  let mut cfg = Config::load(None).map_err(|e| format!("Failed to load AI config: {e}"))?;
  let clean = |opt: Option<&str>| {
    opt
      .map(str::trim)
      .filter(|s| !s.is_empty() && *s != "auto")
      .map(ToString::to_string)
  };
  let tts_p = clean(tts_provider);
  let tts_m = clean(tts_model);
  if tts_p.is_some() || tts_m.is_some() {
    cfg.override_tts(tts_p, tts_m);
  }
  if let Some(llm_p) = clean(llm_provider) {
    cfg.override_llm(Some(llm_p), None);
  }
  Ok(cfg)
}

/// Load, splice (non-destructive text-cut spans), and optionally broadcast-enhance
/// a recorded human voice segment completely locally.
pub fn render_recorded_segment(ov: &SegmentMediaOverride) -> Result<AudioBuffer, String> {
  let Some(path) = ov.recorded_path.as_deref().filter(|s| !s.trim().is_empty()) else {
    return Err("Segment has no recorded audio path".to_string());
  };
  let mut buf = phonon_io::read_audio(path)
    .map_err(|e| format!("Failed to read recorded audio '{path}': {e}"))?;
  let dur = buf.duration_secs();
  let kept = ov.kept_ranges(dur);
  if kept.is_empty() {
    return Err("All transcript spans in this recorded segment are deleted".to_string());
  }
  let covers_full =
    kept.len() == 1 && kept[0].0 <= 0.005 && (dur - kept[0].1).abs() <= 0.005;
  if !covers_full {
    buf = splice_ranges(&buf, &kept, 0.015)
      .map_err(|e| format!("Failed to splice recorded segment: {e}"))?;
  }
  if ov.enhance_voice {
    // Keep LUFS leveling to the episode pipeline so BGM & multi-speaker leveling stay consistent.
    let mut cfg = VoiceEnhanceConfig::podcast_studio();
    cfg.target_lufs = None;
    buf = enhance_voice(&buf, &cfg)
      .map_err(|e| format!("Local voice enhancement failed: {e}"))?;
  }
  Ok(buf)
}

/// Align a recorded audio file into clickable cut spans.
///
/// Uses the configured `phonon-ai` STT provider when an API key is available;
/// automatically falls back to offline energy-based speech region detection
/// (`detect_speech_regions`) so text-cut & silence-stripping still work without
/// any network or API key.
///
/// Returns `(spans, synced_text, used_stt)`.
pub async fn align_recorded_segment(
  recorded_path: String,
  language_hint: Option<String>,
) -> Result<(Vec<TranscriptCutSpan>, String, bool), String> {
  // Attempt cloud/local STT first if configured.
  if let Ok(cfg) = Config::load(None)
    && let Ok(stt) = cfg.stt()
  {
    let mut opts = TranscribeOptions::new();
    if let Some(lang) = language_hint
      .as_deref()
      .map(str::trim)
      .filter(|s| !s.is_empty())
    {
      // ISO-639-1 hint ("zh-CN" -> "zh").
      let short = lang.split('-').next().unwrap_or(lang);
      opts = opts.with_language(short);
    }
    if let Ok(result) = stt.transcribe_file(&recorded_path, Some(opts)).await {
      let spans = spans_from_transcript(&result);
      if !spans.is_empty() {
        return Ok((spans, result.text, true));
      }
    }
  }

  // Offline fallback: 20 ms energy VAD speech region detection.
  let path_clone = recorded_path.clone();
  let regions = tokio::task::spawn_blocking(move || {
    let buf = phonon_io::read_audio(&path_clone)
      .map_err(|e| format!("Failed to read recording '{path_clone}': {e}"))?;
    let mut regs = detect_speech_regions(&buf, -38.0, 0.35, 0.06)
      .map_err(|e| format!("Speech detection failed: {e}"))?;
    if regs.is_empty() && buf.duration_secs() > 0.0 {
      regs.push((0.0, buf.duration_secs()));
    }
    Ok::<_, String>(regs)
  })
  .await
  .map_err(|e| format!("Alignment task failed: {e}"))??;

  let spans = spans_from_speech_regions(&regions);
  let text = format!(
    "[Recorded voice: {} active clip(s) detected offline]",
    spans.len()
  );
  Ok((spans, text, false))
}

/// Atomically save a Studio project JSON file (temp file + rename) and return
/// its resulting modification timestamp so the MCP live-sync watcher ignores
/// our own writes.
pub fn write_project_atomic_file(path: &Path, json: &str) -> Result<SystemTime, String> {
  let parent = path
    .parent()
    .filter(|p| !p.as_os_str().is_empty())
    .unwrap_or_else(|| Path::new("."));
  let seq = STUDIO_TMP_SEQ.fetch_add(1, Ordering::Relaxed);
  let pid = std::process::id();
  let tmp = parent.join(format!(".phonon-studio.{pid}.{seq}.tmp.json"));
  if let Err(e) = std::fs::write(&tmp, json) {
    let _ = std::fs::remove_file(&tmp);
    return Err(format!("Failed to write temporary project file: {e}"));
  }
  if let Err(e) = std::fs::rename(&tmp, path) {
    let _ = std::fs::remove_file(&tmp);
    return Err(format!("Failed to replace project file atomically: {e}"));
  }
  std::fs::metadata(path)
    .and_then(|m| m.modified())
    .map_err(|e| format!("Failed to stat saved project file: {e}"))
}

/// Check if an external agent (e.g. `phonon-mcp`) atomically updated `path`
/// since `last_seen`. Returns `Some((new_mtime, parse_result))` only when the
/// file's modification time advanced.
pub fn check_external_project_reload(
  path: &Path,
  last_seen: Option<SystemTime>,
) -> Option<(SystemTime, Result<StudioProject, String>)> {
  let mtime = std::fs::metadata(path).ok()?.modified().ok()?;
  if let Some(prev) = last_seen
    && mtime <= prev
  {
    return None;
  }
  let res = std::fs::read_to_string(path)
    .map_err(|e| format!("Failed to read synced project: {e}"))
    .and_then(|raw| StudioProject::from_json(&raw));
  Some((mtime, res))
}

/// Synthesize a single segment for preview or regeneration.
///
/// If the segment has a Phase 2 `recorded_path` override, renders it locally
/// (non-destructive text-cut splicing + optional broadcast voice enhancement)
/// without calling cloud TTS. Otherwise uses `CachingTts` when enabled.
pub async fn synthesize_segment(
  project: &StudioProject,
  seg_idx: usize,
  force_refresh: bool,
) -> Result<SegmentAudioEntry, String> {
  let seg = project
    .script
    .segments
    .get(seg_idx)
    .ok_or_else(|| format!("Segment index {seg_idx} out of range"))?;
  let fingerprint = project
    .segment_fingerprint(seg_idx)
    .ok_or_else(|| "Unable to compute segment fingerprint".to_string())?;

  // Local recorded-voice path takes precedence over cloud TTS.
  if let Some(ov) = project.segment_overrides.get(&seg_idx)
    && ov
      .recorded_path
      .as_deref()
      .is_some_and(|p| !p.trim().is_empty())
  {
    let ov_owned = ov.clone();
    let buffer = tokio::task::spawn_blocking(move || render_recorded_segment(&ov_owned))
      .await
      .map_err(|e| format!("Recorded segment render task failed: {e}"))??;
    return Ok(SegmentAudioEntry {
      fingerprint,
      duration_secs: buffer.duration_secs(),
      buffer: Arc::new(buffer),
    });
  }

  if seg.text.trim().is_empty() {
    return Err(format!("Segment {} has empty text", seg_idx + 1));
  }
  let mut voice = project
    .script
    .speakers
    .get(&seg.speaker)
    .ok_or_else(|| format!("Unknown speaker '{}'", seg.speaker))?
    .clone();
  if let Some(ref lang) = seg.language
    && !lang.trim().is_empty()
  {
    voice.language = Some(lang.clone());
  }

  let cfg = build_ai_config(
    project.tts_provider.as_deref(),
    project.tts_model.as_deref(),
    None,
  )?;
  let tts = cfg
    .tts()
    .map_err(|e| format!("TTS provider initialization failed: {e}"))?;
  let provider_id = format!("studio-{}", tts.provider());

  let buffer = if project.use_cache {
    let cached = CachingTts::new(tts, provider_id)
      .map_err(|e| format!("Failed to initialize TTS cache: {e}"))?;
    if force_refresh {
      let stale_path = cached.cache_path(&seg.text, &voice);
      let _ = tokio::task::spawn_blocking(move || std::fs::remove_file(stale_path)).await;
    }
    cached
      .synthesize(&seg.text, &voice)
      .await
      .map_err(|e| format!("TTS synthesis failed: {e}"))?
  } else {
    tts
      .synthesize(&seg.text, &voice)
      .await
      .map_err(|e| format!("TTS synthesis failed: {e}"))?
  };

  Ok(SegmentAudioEntry {
    fingerprint,
    duration_secs: buffer.duration_secs(),
    buffer: Arc::new(buffer),
  })
}

/// Synthesize any missing/stale segments (respecting `tts_concurrency` and
/// `CachingTts`) and assemble the complete broadcast-mastered [`Episode`].
pub async fn assemble_episode(
  project: &StudioProject,
  existing_cache: &HashMap<usize, SegmentAudioEntry>,
) -> Result<
  (
    Episode,
    HashMap<usize, SegmentAudioEntry>,
    EpisodeRenderView,
  ),
  String,
> {
  project
    .script
    .validate()
    .map_err(|e| format!("Script validation failed: {e}"))?;

  let mut next_cache: HashMap<usize, SegmentAudioEntry> = HashMap::new();
  let mut pending_recorded: Vec<usize> = Vec::new();
  let mut pending_tts: Vec<usize> = Vec::new();

  for idx in 0..project.script.segments.len() {
    let Some(fp) = project.segment_fingerprint(idx) else {
      return Err(format!("Cannot fingerprint segment {}", idx + 1));
    };
    if let Some(entry) = existing_cache.get(&idx)
      && entry.fingerprint == fp
    {
      next_cache.insert(idx, entry.clone());
    } else if project
      .segment_overrides
      .get(&idx)
      .and_then(|ov| ov.recorded_path.as_deref())
      .is_some_and(|p| !p.trim().is_empty())
    {
      pending_recorded.push(idx);
    } else {
      pending_tts.push(idx);
    }
  }

  // Render recorded human segments locally (spliced + optionally voice-enhanced).
  for idx in pending_recorded {
    let ov = project
      .segment_overrides
      .get(&idx)
      .cloned()
      .ok_or_else(|| format!("Missing override for recorded segment {}", idx + 1))?;
    let fp = project
      .segment_fingerprint(idx)
      .ok_or_else(|| "Fingerprint error".to_string())?;
    let buf = tokio::task::spawn_blocking(move || render_recorded_segment(&ov))
      .await
      .map_err(|e| format!("Recorded segment {} task failed: {e}", idx + 1))??;
    next_cache.insert(
      idx,
      SegmentAudioEntry {
        fingerprint: fp,
        duration_secs: buf.duration_secs(),
        buffer: Arc::new(buf),
      },
    );
  }

  // Only initialize TTS provider if at least one non-recorded segment needs synthesis.
  if !pending_tts.is_empty() {
    let pending_indices = pending_tts;
    let cfg = build_ai_config(
      project.tts_provider.as_deref(),
      project.tts_model.as_deref(),
      None,
    )?;
    let tts = cfg
      .tts()
      .map_err(|e| format!("TTS provider initialization failed: {e}"))?;
    let provider_id = format!("studio-{}", tts.provider());
    let concurrency = project.script.config.tts_concurrency.max(1);
    let semaphore = Arc::new(Semaphore::new(concurrency));

    if project.use_cache {
      let cached = Arc::new(
        CachingTts::new(tts, provider_id)
          .map_err(|e| format!("Failed to initialize TTS cache: {e}"))?,
      );
      let mut tasks = Vec::with_capacity(pending_indices.len());
      for idx in pending_indices {
        let seg = project.script.segments[idx].clone();
        let mut voice = project
          .script
          .speakers
          .get(&seg.speaker)
          .ok_or_else(|| format!("Unknown speaker '{}'", seg.speaker))?
          .clone();
        if let Some(ref lang) = seg.language
          && !lang.trim().is_empty()
        {
          voice.language = Some(lang.clone());
        }
        let fp = project
          .segment_fingerprint(idx)
          .ok_or_else(|| "Fingerprint error".to_string())?;
        let sem = semaphore.clone();
        let provider = cached.clone();
        tasks.push(async move {
          let _permit = sem
            .acquire()
            .await
            .map_err(|e| format!("Concurrency semaphore error: {e}"))?;
          let buf = provider
            .synthesize(&seg.text, &voice)
            .await
            .map_err(|e| format!("Segment {} synthesis failed: {e}", idx + 1))?;
          Ok::<_, String>((
            idx,
            SegmentAudioEntry {
              fingerprint: fp,
              duration_secs: buf.duration_secs(),
              buffer: Arc::new(buf),
            },
          ))
        });
      }
      for (idx, entry) in futures_join_all(tasks).await? {
        next_cache.insert(idx, entry);
      }
    } else {
      let provider = Arc::new(tts);
      let mut tasks = Vec::with_capacity(pending_indices.len());
      for idx in pending_indices {
        let seg = project.script.segments[idx].clone();
        let mut voice = project
          .script
          .speakers
          .get(&seg.speaker)
          .ok_or_else(|| format!("Unknown speaker '{}'", seg.speaker))?
          .clone();
        if let Some(ref lang) = seg.language
          && !lang.trim().is_empty()
        {
          voice.language = Some(lang.clone());
        }
        let fp = project
          .segment_fingerprint(idx)
          .ok_or_else(|| "Fingerprint error".to_string())?;
        let sem = semaphore.clone();
        let p = provider.clone();
        tasks.push(async move {
          let _permit = sem
            .acquire()
            .await
            .map_err(|e| format!("Concurrency semaphore error: {e}"))?;
          let buf = p
            .synthesize(&seg.text, &voice)
            .await
            .map_err(|e| format!("Segment {} synthesis failed: {e}", idx + 1))?;
          Ok::<_, String>((
            idx,
            SegmentAudioEntry {
              fingerprint: fp,
              duration_secs: buf.duration_secs(),
              buffer: Arc::new(buf),
            },
          ))
        });
      }
      for (idx, entry) in futures_join_all(tasks).await? {
        next_cache.insert(idx, entry);
      }
    }
  }

  let mut raw_buffers = Vec::with_capacity(project.script.segments.len());
  for idx in 0..project.script.segments.len() {
    let entry = next_cache
      .get(&idx)
      .ok_or_else(|| format!("Missing audio buffer for segment {}", idx + 1))?;
    raw_buffers.push(AudioBuffer::clone(&entry.buffer));
  }

  let mut episode = render_episode_from_buffers(&project.script, raw_buffers)
    .await
    .map_err(|e| format!("Episode assembly failed: {e}"))?;

  let tags = project.tags.to_podcast_tags(&project.script.title)?;
  episode = episode.with_tags(tags);

  let ep_audio = episode.audio.clone();
  let chapters: Vec<ChapterView> = episode.chapters.iter().map(ChapterView::from).collect();
  let srt = episode.srt.clone().unwrap_or_default();

  let render_view = tokio::task::spawn_blocking(move || {
    let report = algorithms::measure_loudness(&ep_audio);
    let waveform_path = generate_waveform_path(&ep_audio.samples, 1000.0, 140.0);
    EpisodeRenderView {
      duration_secs: ep_audio.duration_secs(),
      sample_rate: ep_audio.sample_rate,
      channels: ep_audio.channels,
      waveform_path,
      integrated_lufs: report.integrated_lufs,
      short_term_max_lufs: report.short_term_max_lufs,
      range_lu: report.range_lu,
      true_peak_dbtp: report.true_peak_dbtp,
      chapters,
      srt,
    }
  })
  .await
  .map_err(|e| format!("Loudness analysis task failed: {e}"))?;

  Ok((episode, next_cache, render_view))
}

/// Lightweight concurrent join helper avoiding an extra `futures` dependency in
/// `phonon-studio`.
async fn futures_join_all<F, T>(futs: Vec<F>) -> Result<Vec<T>, String>
where
  F: std::future::Future<Output = Result<T, String>> + Send + 'static,
  T: Send + 'static,
{
  let mut handles = Vec::with_capacity(futs.len());
  for fut in futs {
    handles.push(tokio::spawn(fut));
  }
  let mut out = Vec::with_capacity(handles.len());
  for handle in handles {
    let item = handle
      .await
      .map_err(|e| format!("Background synthesis task failed: {e}"))??;
    out.push(item);
  }
  Ok(out)
}

/// Draft a new [`PodcastScript`] from a topic using the configured LLM provider
/// while preserving the current project's speaker cast and mastering config.
pub async fn draft_script_from_topic(
  project: &StudioProject,
  topic: String,
  target_segments: usize,
  language: String,
) -> Result<PodcastScript, String> {
  let topic = topic.trim().to_string();
  if topic.is_empty() {
    return Err("Please enter a topic for AI script drafting".to_string());
  }

  let cfg = build_ai_config(None, None, project.llm_provider.as_deref())?;
  let endpoint = cfg
    .llm()
    .map_err(|e| format!("LLM provider initialization failed: {e}"))?;
  let generator = OpenAiScriptGenerator::from_endpoint(endpoint);

  let mut request = ScriptRequest::new(topic)
    .with_target_segments(target_segments)
    .with_language(language);
  request.speakers.clear();
  for name in project.sorted_speakers() {
    if let Some(voice) = project.script.speakers.get(&name) {
      request = request.with_speaker(name, voice.voice_id.clone(), None);
    }
  }
  if request.speakers.is_empty() {
    request = request
      .with_host("host", "zh-CN-YunyangNeural")
      .with_guest("guest", "zh-CN-XiaoxiaoNeural");
  }

  let mut drafted = generator
    .generate(&request)
    .await
    .map_err(|e| format!("AI script generation failed: {e}"))?;

  // Preserve full VoiceConfig details (speed/emotion/model/language) and
  // mastering PipelineConfig from the studio project.
  for (name, voice) in &project.script.speakers {
    if drafted.speakers.contains_key(name) {
      drafted.speakers.insert(name.clone(), voice.clone());
    }
  }
  drafted.config = project.script.config.clone();
  Ok(drafted)
}

/// Supported export formats for a rendered [`Episode`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EpisodeExportFormat {
  Mp3,
  Wav,
  Flac,
  Ogg,
  Srt,
  ChaptersTxt,
}

/// Export a rendered [`Episode`] (with freshly synced metadata tags) to disk on
/// tokio's blocking pool.
pub async fn export_episode_file(
  episode: Arc<Episode>,
  tags: phonon_io::PodcastTags,
  format: EpisodeExportFormat,
  path: PathBuf,
  mp3_bitrate_kbps: u32,
) -> Result<(), String> {
  tokio::task::spawn_blocking(move || {
    let ep = Episode::clone(&episode).with_tags(tags);
    match format {
      EpisodeExportFormat::Mp3 => ep
        .export_mp3(
          &path,
          &Mp3Config {
            bitrate_kbps: mp3_bitrate_kbps,
          },
        )
        .map_err(|e| e.to_string()),
      EpisodeExportFormat::Wav => ep.export_wav(&path).map_err(|e| e.to_string()),
      EpisodeExportFormat::Flac => ep
        .export_flac(&path, &FlacConfig::default())
        .map_err(|e| e.to_string()),
      EpisodeExportFormat::Ogg => ep
        .export_ogg(&path, &OggConfig::default())
        .map_err(|e| e.to_string()),
      EpisodeExportFormat::Srt => ep.write_srt(&path).map_err(|e| e.to_string()),
      EpisodeExportFormat::ChaptersTxt => {
        let txt = phonon_podcast::format_chapters_text(&ep.chapters);
        std::fs::write(&path, txt).map_err(|e| e.to_string())
      }
    }
  })
  .await
  .map_err(|e| format!("Export worker failed: {e}"))?
}

/// Instantiate concrete [`DspPlugin`] objects from UI specs, applying every
/// configured parameter value.
pub fn instantiate_active_plugins(
  registry: &PluginRegistry,
  specs: &[ActivePluginSpec],
) -> Vec<Box<dyn DspPlugin>> {
  let mut out = Vec::with_capacity(specs.len());
  for spec in specs {
    if let Some(mut plugin) = registry.create(&spec.id) {
      for (pname, val, _, _, _) in &spec.params {
        if val.is_finite() {
          let _ = plugin.set_parameter(pname, *val);
        }
      }
      out.push(plugin);
    }
  }
  out
}

/// Bake the active Audio Lab plugin chain into an owned copy of `buffer` for
/// offline export so exported files match real-time playback.
pub fn bake_plugins_into_buffer(
  buffer: &AudioBuffer,
  registry: &PluginRegistry,
  specs: &[ActivePluginSpec],
) -> AudioBuffer {
  let mut out = AudioBuffer::clone(buffer);
  if specs.is_empty() || out.samples.is_empty() {
    return out;
  }
  let mut plugins = instantiate_active_plugins(registry, specs);
  for plugin in &mut plugins {
    plugin.prepare(out.sample_rate, out.channels);
    plugin.process(&mut out.samples, out.sample_rate, out.channels);
  }
  out
}

/// Downsamples audio data into a symmetric SVG waveform polygon path.
pub fn generate_waveform_path(samples: &[f32], width: f32, height: f32) -> String {
  if samples.is_empty() || width <= 0.0 || height <= 0.0 {
    return String::new();
  }

  let pixel_cols = width as usize;
  let samples_per_pixel = (samples.len() as f32 / width).ceil() as usize;
  let step = samples_per_pixel.max(1);
  let mid_y = height / 2.0;
  let scale_y = height / 2.0;

  let mut peaks = Vec::with_capacity(pixel_cols);
  for x in 0..pixel_cols {
    let start = x * step;
    if start >= samples.len() {
      break;
    }
    let end = (start + step).min(samples.len());
    let max_val = samples[start..end]
      .iter()
      .fold(0.0f32, |acc, s| acc.max(s.abs()));
    peaks.push((x, max_val.min(1.0)));
  }

  let mut path = String::with_capacity(peaks.len() * 24 + 16);
  path.push_str(&format!("M 0 {mid_y}"));
  for &(x, peak) in &peaks {
    let y = mid_y - (peak * scale_y);
    path.push_str(&format!(" L {x} {y:.2}"));
  }
  for &(x, peak) in peaks.iter().rev() {
    let y = mid_y + (peak * scale_y);
    path.push_str(&format!(" L {x} {y:.2}"));
  }
  path.push_str(" Z");
  path
}

#[cfg(test)]
mod tests {
  use super::*;

  fn sine_buffer(freq: f64, secs: f64, sr: u32, amp: f32) -> AudioBuffer {
    let n = (sr as f64 * secs) as usize;
    let samples = (0..n)
      .map(|i| amp * (2.0 * std::f64::consts::PI * freq * i as f64 / sr as f64).sin() as f32)
      .collect();
    AudioBuffer {
      samples,
      sample_rate: sr,
      channels: 1,
    }
  }

  #[test]
  fn test_inspect_providers_includes_edge_as_keyless() {
    let providers = inspect_providers();
    let edge = providers
      .iter()
      .find(|p| p.name == "edge")
      .expect("edge provider");
    assert!(edge.key_present);
    assert!(edge.has_tts);
    assert!(!edge.voices.is_empty());
  }

  #[test]
  fn test_bake_plugins_into_buffer_applies_configured_params() {
    let registry = PluginRegistry::with_builtins();
    let buf = sine_buffer(440.0, 0.1, 44100, 0.8);
    let specs = vec![ActivePluginSpec {
      id: "builtin.volume".to_string(),
      name: "Volume".to_string(),
      params: vec![("gain".to_string(), 0.25, 0.0, 2.0, 1.0)],
    }];
    let baked = bake_plugins_into_buffer(&buf, &registry, &specs);
    let orig_peak = algorithms::peak(&buf);
    let baked_peak = algorithms::peak(&baked);
    assert!(
      (baked_peak - orig_peak * 0.25).abs() < 1e-4,
      "orig={orig_peak}, baked={baked_peak}"
    );
  }

  #[tokio::test]
  async fn test_assemble_episode_reuses_fresh_in_memory_cache_offline() {
    let project = StudioProject::starter();
    let mut cache = HashMap::new();
    for idx in 0..project.script.segments.len() {
      let fp = project.segment_fingerprint(idx).expect("fp");
      let buf = sine_buffer(330.0 + idx as f64 * 80.0, 1.0, 44100, 0.3);
      cache.insert(
        idx,
        SegmentAudioEntry {
          fingerprint: fp,
          duration_secs: buf.duration_secs(),
          buffer: Arc::new(buf),
        },
      );
    }

    // All segments are fresh in cache -> zero network calls needed!
    let (episode, next_cache, view) = assemble_episode(&project, &cache)
      .await
      .expect("offline assembly");
    assert_eq!(next_cache.len(), project.script.segments.len());
    assert!(view.duration_secs > 3.0);
    assert!((view.integrated_lufs - (-16.0)).abs() < 1.0);
    assert_eq!(view.chapters.len(), 2);
    assert!(view.srt.contains("Phonon Studio"));
    assert_eq!(
      episode.tags.title.as_deref(),
      Some(project.script.title.as_str())
    );

    // Export MP3 with ID3 tags + chapters + SRT sidecar.
    let dir = tempfile::tempdir().expect("tempdir");
    let mp3_path = dir.path().join("ep.mp3");
    let srt_path = dir.path().join("ep.srt");
    let tags = episode.tags.clone();
    let ep_arc = Arc::new(episode);
    export_episode_file(
      ep_arc.clone(),
      tags.clone(),
      EpisodeExportFormat::Mp3,
      mp3_path.clone(),
      192,
    )
    .await
    .expect("export mp3");
    export_episode_file(
      ep_arc,
      tags,
      EpisodeExportFormat::Srt,
      srt_path.clone(),
      192,
    )
    .await
    .expect("export srt");
    assert!(std::fs::metadata(&mp3_path).expect("mp3 meta").len() > 1000);
    assert!(
      std::fs::read_to_string(&srt_path)
        .expect("srt")
        .contains("-->")
    );
  }

  #[tokio::test]
  async fn test_recorded_segment_text_cut_and_enhance_pipeline_offline() {
    let dir = tempfile::tempdir().expect("tempdir");
    let rec_path = dir.path().join("host_take.wav");
    let raw = sine_buffer(320.0, 3.0, 16000, 0.3);
    phonon_wav::write_wav(&rec_path, &raw).expect("write rec wav");

    let mut project = StudioProject::starter();
    project.script.segments.truncate(1);
    project.segment_overrides.insert(
      0,
      SegmentMediaOverride {
        recorded_path: Some(rec_path.to_string_lossy().to_string()),
        enhance_voice: true,
        spans: vec![
          crate::project::TranscriptCutSpan {
            id: 0,
            start_secs: 0.0,
            end_secs: 1.0,
            text: "嗯".to_string(),
            deleted: true,
            is_filler: true,
          },
          crate::project::TranscriptCutSpan {
            id: 1,
            start_secs: 1.0,
            end_secs: 2.5,
            text: "大家好欢迎收听".to_string(),
            deleted: false,
            is_filler: false,
          },
        ],
        ..SegmentMediaOverride::default()
      },
    );

    // Offline assembly with zero TTS provider: slices 1.5s kept range + enhances + masters to -16 LUFS.
    let (episode, cache, view) = assemble_episode(&project, &HashMap::new())
      .await
      .expect("offline recorded assemble");
    assert_eq!(cache.len(), 1);
    assert!(
      (episode.audio.duration_secs() - 1.5).abs() < 0.05,
      "expected ~1.5s spliced duration, got {:.3}s",
      episode.audio.duration_secs()
    );
    assert!((view.integrated_lufs - (-16.0)).abs() < 1.2);
  }

  #[test]
  fn test_atomic_save_and_external_mcp_reload_detection() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("live.phonon.json");
    let mut project = StudioProject::starter();
    let json1 = project.to_json_pretty().expect("json1");
    let mtime1 = write_project_atomic_file(&path, &json1).expect("save1");

    // Unchanged file must not trigger a reload.
    assert!(check_external_project_reload(&path, Some(mtime1)).is_none());

    // Simulate external MCP edit slightly later (bump file contents & mtime).
    std::thread::sleep(std::time::Duration::from_millis(20));
    project.script.title = "Edited by External MCP Agent".to_string();
    let json2 = project.to_json_pretty().expect("json2");
    let _ = write_project_atomic_file(&path, &json2).expect("save2");

    let (mtime2, reloaded) =
      check_external_project_reload(&path, Some(mtime1)).expect("should detect reload");
    assert!(mtime2 > mtime1);
    assert_eq!(
      reloaded.expect("valid project").script.title,
      "Edited by External MCP Agent"
    );
  }
}
