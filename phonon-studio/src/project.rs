//! Pure-Rust project model, Undo/Redo history, and episode view types for
//! Phonon Studio.
//!
//! Kept strictly free of Dioxus UI primitives so every state transition,
//! serialization format, cache invalidation key, and history operation can be
//! verified by headless unit tests.

use phonon_ai::VoiceConfig;
use phonon_core::AudioBuffer;
use phonon_io::{CoverArt, CoverMime, PodcastTags};
use phonon_podcast::{Chapter, PipelineConfig, PodcastScript, Segment};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

/// Schema version for persisted `.phonon.json` studio project files.
pub const STUDIO_PROJECT_VERSION: u32 = 1;

/// Maximum undo snapshots retained in memory.
const MAX_HISTORY_DEPTH: usize = 64;

/// Editable episode metadata tags and optional cover image path.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct EpisodeTagDraft {
  #[serde(default)]
  pub title: String,
  #[serde(default)]
  pub artist: String,
  #[serde(default)]
  pub album: String,
  #[serde(default)]
  pub year: String,
  #[serde(default)]
  pub comment: String,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub cover_path: Option<String>,
}

impl EpisodeTagDraft {
  /// Build [`PodcastTags`] for export, reading the cover image from disk if set.
  pub fn to_podcast_tags(&self, fallback_title: &str) -> Result<PodcastTags, String> {
    let clean = |s: &str| {
      let trimmed = s.trim();
      if trimmed.is_empty() {
        None
      } else {
        Some(trimmed.to_string())
      }
    };

    let year = match clean(&self.year) {
      Some(y) => Some(
        y.parse::<i32>()
          .map_err(|_| format!("Invalid release year '{}': expected integer", y))?,
      ),
      None => None,
    };

    let cover = match &self.cover_path {
      Some(path) if !path.trim().is_empty() => {
        let p = Path::new(path.trim());
        let data = std::fs::read(p)
          .map_err(|e| format!("Failed to read cover art '{}': {}", p.display(), e))?;
        let mime = match p
          .extension()
          .and_then(|e| e.to_str())
          .map(|s| s.to_ascii_lowercase())
          .as_deref()
        {
          Some("png") => CoverMime::Png,
          _ => CoverMime::Jpeg,
        };
        Some(CoverArt {
          mime,
          data,
          description: Some("Episode Cover".to_string()),
        })
      }
      _ => None,
    };

    Ok(PodcastTags {
      title: clean(&self.title).or_else(|| clean(fallback_title)),
      artist: clean(&self.artist),
      album: clean(&self.album),
      year,
      comment: clean(&self.comment),
      cover,
      ..PodcastTags::default()
    })
  }
}

/// A single clickable word/clause/speech span aligned to a recorded audio file.
///
/// Toggling `deleted` non-destructively cuts that audio slice via smooth
/// micro-crossfades during preview and episode assembly.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TranscriptCutSpan {
  pub id: usize,
  pub start_secs: f64,
  pub end_secs: f64,
  pub text: String,
  #[serde(default)]
  pub deleted: bool,
  #[serde(default)]
  pub is_filler: bool,
}

/// Non-destructive per-segment media override supporting recorded human speech,
/// text-driven audio cutting (`spans`), trim bounds, and local broadcast voice
/// enhancement.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct SegmentMediaOverride {
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub recorded_path: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub trim_start_secs: Option<f64>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub trim_end_secs: Option<f64>,
  #[serde(default)]
  pub enhance_voice: bool,
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub spans: Vec<TranscriptCutSpan>,
}

impl SegmentMediaOverride {
  /// Compute the ordered `(start_secs, end_secs)` audio intervals to keep,
  /// merging adjacent contiguous spans within 25 ms so unedited speech plays
  /// without unnecessary crossfades.
  pub fn kept_ranges(&self, total_duration: f64) -> Vec<(f64, f64)> {
    let min_t = self.trim_start_secs.unwrap_or(0.0).max(0.0);
    let max_t = self
      .trim_end_secs
      .unwrap_or(total_duration)
      .min(total_duration);
    if max_t <= min_t {
      return Vec::new();
    }

    if self.spans.is_empty() {
      return vec![(min_t, max_t)];
    }

    let mut raw: Vec<(f64, f64)> = self
      .spans
      .iter()
      .filter(|s| !s.deleted && s.start_secs.is_finite() && s.end_secs.is_finite())
      .filter_map(|s| {
        let a = s.start_secs.max(min_t);
        let b = s.end_secs.min(max_t);
        (b > a).then_some((a, b))
      })
      .collect();
    raw.sort_by(|a, b| a.0.total_cmp(&b.0));

    let mut merged: Vec<(f64, f64)> = Vec::with_capacity(raw.len());
    for (s, e) in raw {
      if let Some(last) = merged.last_mut()
        && s - last.1 <= 0.025
      {
        last.1 = last.1.max(e);
      } else {
        merged.push((s, e));
      }
    }
    merged
  }

  /// Active (non-deleted) text assembled from aligned spans, if any spans exist.
  pub fn active_text(&self) -> Option<String> {
    if self.spans.is_empty() {
      return None;
    }
    let kept: Vec<&str> = self
      .spans
      .iter()
      .filter(|s| !s.deleted)
      .map(|s| s.text.as_str())
      .collect();
    if kept.is_empty() {
      return Some(String::new());
    }
    // Join with spaces only when adjacent spans are ASCII/Latin words.
    let mut out = String::new();
    for part in kept {
      let needs_space = !out.is_empty()
        && out
          .chars()
          .last()
          .is_some_and(|c| c.is_ascii_alphanumeric())
        && part
          .chars()
          .next()
          .is_some_and(|c| c.is_ascii_alphanumeric());
      if needs_space {
        out.push(' ');
      }
      out.push_str(part);
    }
    Some(out)
  }

  /// Mark all filler spans as deleted; returns how many spans changed state.
  pub fn delete_all_fillers(&mut self) -> usize {
    let mut changed = 0;
    for span in &mut self.spans {
      if span.is_filler && !span.deleted {
        span.deleted = true;
        changed += 1;
      }
    }
    changed
  }

  /// Restore all deleted spans in this segment.
  pub fn restore_all_spans(&mut self) -> usize {
    let mut changed = 0;
    for span in &mut self.spans {
      if span.deleted {
        span.deleted = false;
        changed += 1;
      }
    }
    changed
  }
}

/// Returns `true` when `token` (ignoring surrounding punctuation/whitespace)
/// matches a common spoken Chinese or English filler word.
pub fn is_filler_token(token: &str) -> bool {
  let cleaned: String = token
    .trim()
    .trim_matches(|c: char| {
      c.is_ascii_punctuation()
        || matches!(
          c,
          '，' | '。' | '、' | '？' | '！' | '；' | '：' | '…' | '—' | '“' | '”'
        )
    })
    .to_ascii_lowercase();
  matches!(
    cleaned.as_str(),
    "嗯"
      | "嗯嗯"
      | "啊"
      | "呃"
      | "额"
      | "那个"
      | "就是说"
      | "然后呢"
      | "um"
      | "uh"
      | "erm"
      | "hmm"
      | "like"
      | "you know"
  )
}

/// Split a timed STT transcript into clickable word/clause spans with
/// proportional timestamps and automatic filler tagging.
pub fn spans_from_transcript(transcript: &phonon_ai::TranscriptResult) -> Vec<TranscriptCutSpan> {
  let mut out = Vec::new();
  let mut next_id = 0usize;

  for seg in &transcript.segments {
    let text = seg.text.trim();
    if text.is_empty() || !seg.start.is_finite() || !seg.end.is_finite() || seg.end <= seg.start {
      continue;
    }
    let tokens = tokenize_for_cut_spans(text);
    if tokens.is_empty() {
      continue;
    }
    let weights: Vec<usize> = tokens
      .iter()
      .map(|t| t.chars().filter(|c| c.is_alphanumeric()).count().max(1))
      .collect();
    let total_w: usize = weights.iter().sum::<usize>().max(1);
    let dur = seg.end - seg.start;
    let mut acc = 0usize;

    for (tok, w) in tokens.into_iter().zip(weights) {
      let s = seg.start + dur * (acc as f64 / total_w as f64);
      acc += w;
      let e = seg.start + dur * (acc as f64 / total_w as f64);
      let filler = is_filler_token(&tok);
      out.push(TranscriptCutSpan {
        id: next_id,
        start_secs: s,
        end_secs: e,
        text: tok,
        deleted: false,
        is_filler: filler,
      });
      next_id += 1;
    }
  }

  out
}

/// Offline fallback when no STT key is configured: convert energy-detected
/// speech intervals into clickable cut chips.
pub fn spans_from_speech_regions(regions: &[(f64, f64)]) -> Vec<TranscriptCutSpan> {
  regions
    .iter()
    .enumerate()
    .map(|(idx, &(start_secs, end_secs))| TranscriptCutSpan {
      id: idx,
      start_secs,
      end_secs,
      text: format!(
        "Clip {:02} ({}–{})",
        idx + 1,
        format_timecode(start_secs),
        format_timecode(end_secs)
      ),
      deleted: false,
      is_filler: false,
    })
    .collect()
}

/// Tokenize a transcript line into clickable cut units:
/// - Whitespace-delimited words for Latin text
/// - Clause/punctuation chunks + isolated Chinese filler particles (`嗯`, `啊`, `呃`, `额`, `那个`) for CJK text
fn tokenize_for_cut_spans(text: &str) -> Vec<String> {
  let has_cjk = text.chars().any(
    |c| matches!(c, '\u{3040}'..='\u{30ff}' | '\u{3400}'..='\u{9fff}' | '\u{ac00}'..='\u{d7af}'),
  );
  if !has_cjk {
    return text.split_whitespace().map(ToString::to_string).collect();
  }

  // First split by clause punctuation while keeping punctuation attached to the preceding token.
  let mut clauses = Vec::new();
  let mut current = String::new();
  for ch in text.chars() {
    current.push(ch);
    if matches!(
      ch,
      '，' | '。' | '、' | '？' | '！' | '；' | ',' | '.' | '?' | '!' | ';' | ' '
    ) {
      let trimmed = current.trim().to_string();
      if !trimmed.is_empty() {
        clauses.push(trimmed);
      }
      current.clear();
    }
  }
  let tail = current.trim().to_string();
  if !tail.is_empty() {
    clauses.push(tail);
  }

  // Separate leading CJK filler particles so users can cut "嗯"/"呃"/"那个" without losing the whole clause.
  let mut tokens = Vec::with_capacity(clauses.len());
  for clause in clauses {
    let mut rest = clause.as_str();
    loop {
      let matched = ["就是说", "然后呢", "那个", "嗯嗯", "嗯", "呃", "额", "啊"]
        .into_iter()
        .find(|f| rest.starts_with(f) && rest.len() > f.len());
      if let Some(filler) = matched {
        tokens.push(filler.to_string());
        rest = rest[filler.len()..].trim_start();
      } else {
        break;
      }
    }
    if !rest.is_empty() {
      tokens.push(rest.to_string());
    }
  }
  tokens
}

/// Build a ready-to-paste MCP client configuration snippet (`claude_desktop_config.json` /
/// Cursor MCP config) pointing `PHONON_MCP_ROOT` at the current project's directory.
pub fn mcp_client_config_json(project_path: Option<&Path>) -> String {
  let root = project_path
    .and_then(Path::parent)
    .map(|p| p.display().to_string())
    .unwrap_or_else(|| "/path/to/your/podcast-folder".to_string());
  serde_json::json!({
    "mcpServers": {
      "phonon": {
        "command": "phonon-mcp",
        "env": {
          "PHONON_MCP_ROOT": root
        }
      }
    }
  })
  .to_string()
}

/// Root project document managed by Phonon Studio.
///
/// Wraps [`PodcastScript`] with studio-level provider selection, caching
/// preference, episode export metadata, and Phase 2 segment override hooks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StudioProject {
  #[serde(default = "default_project_version")]
  pub version: u32,
  pub script: PodcastScript,
  /// Selected TTS provider name (`None` = auto from config/env, fallback Edge).
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub tts_provider: Option<String>,
  /// Optional TTS model override.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub tts_model: Option<String>,
  /// Selected LLM provider name for AI script drafting (`None` = auto).
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub llm_provider: Option<String>,
  /// Enable content-addressed filesystem caching via `CachingTts`.
  #[serde(default = "default_true")]
  pub use_cache: bool,
  /// Episode tags embedded into exported MP3 / FLAC / OGG files.
  #[serde(default)]
  pub tags: EpisodeTagDraft,
  /// Reserved Phase 2 per-segment non-destructive media overrides.
  #[serde(default, skip_serializing_if = "HashMap::is_empty")]
  pub segment_overrides: HashMap<usize, SegmentMediaOverride>,
}

fn default_project_version() -> u32 {
  STUDIO_PROJECT_VERSION
}

fn default_true() -> bool {
  true
}

impl StudioProject {
  /// Creates a ready-to-preview starter episode using keyless Edge TTS voices
  /// so first-time users can synthesize, preview, master, and export right away.
  pub fn starter() -> Self {
    let mut speakers = HashMap::new();
    speakers.insert(
      "host".to_string(),
      VoiceConfig::new("zh-CN-YunyangNeural")
        .with_speed(1.0)
        .with_language("zh-CN"),
    );
    speakers.insert(
      "guest".to_string(),
      VoiceConfig::new("zh-CN-XiaoxiaoNeural")
        .with_speed(1.0)
        .with_language("zh-CN"),
    );

    let config = PipelineConfig {
      pause_between_segments_secs: 0.45,
      bgm_volume: 0.08,
      target_lufs: Some(-16.0),
      output_sample_rate: 44100,
      output_channels: 1,
      ..PipelineConfig::default()
    };

    let script = PodcastScript {
      title: "AI 播客工作台 · 第一期试音".to_string(),
      speakers,
      segments: vec![
        Segment {
          speaker: "host".to_string(),
          text: "欢迎来到 Phonon Studio。你可以直接修改任意一段台词，点击单段试听，系统会自动缓存已合成的音频。".to_string(),
          language: Some("zh-CN".to_string()),
          chapter: Some("开场介绍".to_string()),
        },
        Segment {
          speaker: "guest".to_string(),
          text: "是的，不需要每次改两个字就重新合成整期节目。支持 MiniMax、通义千问、智谱、OpenAI 和免费的 Edge 语音，导出前自动对齐到负十六 LUFS 播客响度标准。".to_string(),
          language: Some("zh-CN".to_string()),
          chapter: None,
        },
        Segment {
          speaker: "host".to_string(),
          text: "在右侧面板可以预览波形、章节时间轴与 SRT 字幕，一键导出带封面和 ID3 章节标记的 MP3。".to_string(),
          language: Some("zh-CN".to_string()),
          chapter: Some("母带与导出".to_string()),
        },
      ],
      config,
    };

    Self {
      version: STUDIO_PROJECT_VERSION,
      tags: EpisodeTagDraft {
        title: script.title.clone(),
        artist: "Phonon Studio".to_string(),
        album: "Phonon Originals".to_string(),
        year: "2026".to_string(),
        comment: "Produced with Phonon Studio".to_string(),
        cover_path: None,
      },
      script,
      tts_provider: None,
      tts_model: None,
      llm_provider: None,
      use_cache: true,
      segment_overrides: HashMap::new(),
    }
  }

  /// Wrap an existing CLI [`PodcastScript`] into a `StudioProject`.
  pub fn from_script(script: PodcastScript) -> Self {
    let title = script.title.clone();
    Self {
      version: STUDIO_PROJECT_VERSION,
      script,
      tts_provider: None,
      tts_model: None,
      llm_provider: None,
      use_cache: true,
      tags: EpisodeTagDraft {
        title,
        ..EpisodeTagDraft::default()
      },
      segment_overrides: HashMap::new(),
    }
  }

  /// Parse either a `.phonon.json` [`StudioProject`] or a plain CLI
  /// [`PodcastScript`] JSON file.
  pub fn from_json(json: &str) -> Result<Self, String> {
    if let Ok(project) = serde_json::from_str::<StudioProject>(json) {
      project
        .script
        .validate()
        .map_err(|e| format!("Invalid studio project script: {e}"))?;
      return Ok(project);
    }

    let script = PodcastScript::from_json(json)
      .map_err(|e| format!("Failed to parse as StudioProject or PodcastScript: {e}"))?;
    Ok(Self::from_script(script))
  }

  /// Serialize this project to indented JSON.
  pub fn to_json_pretty(&self) -> Result<String, String> {
    serde_json::to_string_pretty(self).map_err(|e| format!("Serialize project failed: {e}"))
  }

  /// Deterministic content-addressable fingerprint for segment `idx`.
  ///
  /// Any change to provider, model, speaker voice settings, language, or text
  /// produces a new fingerprint so stale in-memory previews are immediately
  /// flagged without hashing raw PCM.
  pub fn segment_fingerprint(&self, idx: usize) -> Option<String> {
    let seg = self.script.segments.get(idx)?;
    let voice = self.script.speakers.get(&seg.speaker)?;
    let mut h = blake3::Hasher::new();

    let feed_str = |hasher: &mut blake3::Hasher, s: &str| {
      hasher.update(&(s.len() as u64).to_le_bytes());
      hasher.update(s.as_bytes());
    };
    let feed_opt = |hasher: &mut blake3::Hasher, opt: Option<&str>| match opt {
      None => {
        hasher.update(&[0]);
      }
      Some(s) => {
        hasher.update(&[1]);
        feed_str(hasher, s);
      }
    };

    feed_opt(&mut h, self.tts_provider.as_deref());
    feed_opt(&mut h, self.tts_model.as_deref());
    feed_str(&mut h, &seg.speaker);
    feed_str(&mut h, &voice.voice_id);
    feed_str(&mut h, &voice.model);
    h.update(&voice.speed.to_bits().to_le_bytes());
    feed_opt(&mut h, voice.emotion.as_deref());
    feed_opt(&mut h, voice.style.as_deref());
    let eff_lang = seg.language.as_deref().or(voice.language.as_deref());
    feed_opt(&mut h, eff_lang);
    feed_str(&mut h, &seg.text);

    // Include Phase 2 recorded-audio override, cut spans, and enhance flag so
    // any text-cut or voice-enhance toggle immediately invalidates segment cache.
    if let Some(ov) = self.segment_overrides.get(&idx) {
      h.update(&[1]);
      feed_opt(&mut h, ov.recorded_path.as_deref());
      h.update(&[u8::from(ov.enhance_voice)]);
      for opt_t in [ov.trim_start_secs, ov.trim_end_secs] {
        match opt_t {
          Some(t) => {
            h.update(&[1]);
            h.update(&t.to_bits().to_le_bytes());
          }
          None => {
            h.update(&[0]);
          }
        }
      }
      h.update(&(ov.spans.len() as u64).to_le_bytes());
      for span in &ov.spans {
        h.update(&span.start_secs.to_bits().to_le_bytes());
        h.update(&span.end_secs.to_bits().to_le_bytes());
        h.update(&[u8::from(span.deleted)]);
      }
    } else {
      h.update(&[0]);
    }

    Some(h.finalize().to_hex().to_string())
  }

  /// Sorted list of speaker names for deterministic UI rendering.
  pub fn sorted_speakers(&self) -> Vec<String> {
    let mut names: Vec<String> = self.script.speakers.keys().cloned().collect();
    names.sort();
    names
  }

  /// Insert a new segment after `after_idx` (or at the end if `None`).
  /// Returns the newly inserted segment's index.
  pub fn add_segment_after(&mut self, after_idx: Option<usize>, speaker: Option<String>) -> usize {
    let default_speaker = speaker
      .filter(|s| self.script.speakers.contains_key(s))
      .or_else(|| {
        after_idx
          .and_then(|i| self.script.segments.get(i))
          .map(|prev| {
            // Alternate speaker automatically when 2+ speakers exist.
            let names = self.sorted_speakers();
            if names.len() >= 2 {
              names
                .iter()
                .find(|n| **n != prev.speaker)
                .cloned()
                .unwrap_or_else(|| prev.speaker.clone())
            } else {
              prev.speaker.clone()
            }
          })
      })
      .or_else(|| self.sorted_speakers().into_iter().next())
      .unwrap_or_else(|| "host".to_string());

    if !self.script.speakers.contains_key(&default_speaker) {
      self
        .script
        .speakers
        .insert(default_speaker.clone(), VoiceConfig::default());
    }

    let new_seg = Segment {
      speaker: default_speaker,
      text: "在此输入新段落台词…".to_string(),
      language: None,
      chapter: None,
    };

    let insert_at = match after_idx {
      Some(i) if i < self.script.segments.len() => i + 1,
      _ => self.script.segments.len(),
    };
    self.script.segments.insert(insert_at, new_seg);
    self.shift_overrides_after_insert(insert_at);
    insert_at
  }

  /// Duplicate segment `idx` immediately after it.
  pub fn duplicate_segment(&mut self, idx: usize) -> Option<usize> {
    let mut cloned = self.script.segments.get(idx)?.clone();
    cloned.chapter = None;
    let cloned_ov = self.segment_overrides.get(&idx).cloned();
    let insert_at = idx + 1;
    self.script.segments.insert(insert_at, cloned);
    self.shift_overrides_after_insert(insert_at);
    if let Some(ov) = cloned_ov {
      self.segment_overrides.insert(insert_at, ov);
    }
    Some(insert_at)
  }

  /// Remove segment `idx`. Keeps at least one segment so the script remains valid.
  pub fn remove_segment(&mut self, idx: usize) -> bool {
    if self.script.segments.len() <= 1 || idx >= self.script.segments.len() {
      return false;
    }
    self.script.segments.remove(idx);
    self.shift_overrides_after_remove(idx);
    true
  }

  /// Move segment `idx` up (`delta = -1`) or down (`delta = 1`).
  pub fn move_segment(&mut self, idx: usize, delta: isize) -> Option<usize> {
    if idx >= self.script.segments.len() {
      return None;
    }
    let target = idx as isize + delta;
    if target < 0 || target as usize >= self.script.segments.len() {
      return None;
    }
    let target = target as usize;
    self.script.segments.swap(idx, target);
    let ov_a = self.segment_overrides.remove(&idx);
    let ov_b = self.segment_overrides.remove(&target);
    if let Some(a) = ov_a {
      self.segment_overrides.insert(target, a);
    }
    if let Some(b) = ov_b {
      self.segment_overrides.insert(idx, b);
    }
    Some(target)
  }

  fn shift_overrides_after_insert(&mut self, inserted_at: usize) {
    if self.segment_overrides.is_empty() {
      return;
    }
    let old = std::mem::take(&mut self.segment_overrides);
    for (k, v) in old {
      self
        .segment_overrides
        .insert(if k >= inserted_at { k + 1 } else { k }, v);
    }
  }

  fn shift_overrides_after_remove(&mut self, removed_at: usize) {
    if self.segment_overrides.is_empty() {
      return;
    }
    let old = std::mem::take(&mut self.segment_overrides);
    for (k, v) in old {
      if k < removed_at {
        self.segment_overrides.insert(k, v);
      } else if k > removed_at {
        self.segment_overrides.insert(k - 1, v);
      }
    }
  }

  /// Add a new speaker to the cast.
  pub fn add_speaker(&mut self, name: String, voice: VoiceConfig) -> Result<(), String> {
    let trimmed = name.trim().to_string();
    if trimmed.is_empty() {
      return Err("Speaker name cannot be empty".to_string());
    }
    if self.script.speakers.contains_key(&trimmed) {
      return Err(format!("Speaker '{trimmed}' already exists"));
    }
    self.script.speakers.insert(trimmed, voice);
    Ok(())
  }

  /// Remove a speaker if not referenced by any segment and not the last speaker.
  pub fn remove_speaker(&mut self, name: &str) -> Result<(), String> {
    if self.script.speakers.len() <= 1 {
      return Err("Cannot remove the last speaker".to_string());
    }
    let used_count = self
      .script
      .segments
      .iter()
      .filter(|s| s.speaker == name)
      .count();
    if used_count > 0 {
      return Err(format!(
        "Speaker '{name}' is still used by {used_count} segment(s)"
      ));
    }
    self.script.speakers.remove(name);
    Ok(())
  }

  /// Assign verified default voices from a chosen provider across all speakers
  /// in deterministic alphabetical order.
  pub fn apply_provider_voices(&mut self, voices: &[&str]) -> usize {
    if voices.is_empty() {
      return 0;
    }
    let names = self.sorted_speakers();
    let mut updated = 0;
    for (idx, name) in names.iter().enumerate() {
      if let Some(cfg) = self.script.speakers.get_mut(name) {
        cfg.voice_id = voices[idx % voices.len()].to_string();
        updated += 1;
      }
    }
    updated
  }
}

/// Bounded Undo/Redo snapshot stack for [`StudioProject`].
#[derive(Debug, Clone, Default)]
pub struct ProjectHistory {
  undo_stack: Vec<StudioProject>,
  redo_stack: Vec<StudioProject>,
}

impl ProjectHistory {
  pub fn new() -> Self {
    Self::default()
  }

  /// Save `current` before applying a mutation. Clears the redo stack.
  pub fn push_snapshot(&mut self, current: &StudioProject) {
    if self.undo_stack.len() >= MAX_HISTORY_DEPTH {
      self.undo_stack.remove(0);
    }
    self.undo_stack.push(current.clone());
    self.redo_stack.clear();
  }

  /// Revert `current` to the previous snapshot.
  pub fn undo(&mut self, current: &mut StudioProject) -> bool {
    let Some(prev) = self.undo_stack.pop() else {
      return false;
    };
    let old = std::mem::replace(current, prev);
    self.redo_stack.push(old);
    true
  }

  /// Re-apply a previously undone snapshot.
  pub fn redo(&mut self, current: &mut StudioProject) -> bool {
    let Some(next) = self.redo_stack.pop() else {
      return false;
    };
    let old = std::mem::replace(current, next);
    self.undo_stack.push(old);
    true
  }

  pub fn can_undo(&self) -> bool {
    !self.undo_stack.is_empty()
  }

  pub fn can_redo(&self) -> bool {
    !self.redo_stack.is_empty()
  }

  pub fn clear(&mut self) {
    self.undo_stack.clear();
    self.redo_stack.clear();
  }
}

/// In-memory cached audio for a single synthesized script segment.
#[derive(Debug, Clone)]
pub struct SegmentAudioEntry {
  pub fingerprint: String,
  pub duration_secs: f64,
  pub buffer: Arc<AudioBuffer>,
}

/// UI-friendly chapter row with formatted timestamp.
#[derive(Debug, Clone, PartialEq)]
pub struct ChapterView {
  pub title: String,
  pub start_secs: f64,
  pub end_secs: Option<f64>,
}

impl From<&Chapter> for ChapterView {
  fn from(ch: &Chapter) -> Self {
    Self {
      title: ch.title.clone(),
      start_secs: ch.start,
      end_secs: ch.end,
    }
  }
}

/// Lightweight display metadata for a rendered podcast episode.
#[derive(Debug, Clone, PartialEq)]
pub struct EpisodeRenderView {
  pub duration_secs: f64,
  pub sample_rate: u32,
  pub channels: u16,
  pub waveform_path: String,
  pub integrated_lufs: f32,
  pub short_term_max_lufs: f32,
  pub range_lu: f32,
  pub true_peak_dbtp: f32,
  pub chapters: Vec<ChapterView>,
  pub srt: String,
}

/// Broadcast loudness compliance status derived from BS.1770-4 measurements.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoudnessHealth {
  /// True peak exceeds -1.0 dBTP (risk of inter-sample clipping after MP3/AAC encoding).
  ClippingRisk,
  /// Integrated loudness is within +/- 1.5 LU of target (or standard -16 LUFS) and peak safe.
  BroadcastReady,
  /// Peak is safe, but integrated loudness drifts noticeably from target.
  OffTarget,
}

impl EpisodeRenderView {
  /// Evaluate broadcast compliance against an optional target LUFS (defaults to -16 LUFS).
  pub fn loudness_health(&self, target_lufs: Option<f32>) -> LoudnessHealth {
    if self.true_peak_dbtp.is_finite() && self.true_peak_dbtp > -0.9 {
      return LoudnessHealth::ClippingRisk;
    }
    let goal = target_lufs.unwrap_or(-16.0);
    if self.integrated_lufs.is_finite() && (self.integrated_lufs - goal).abs() <= 1.5 {
      LoudnessHealth::BroadcastReady
    } else {
      LoudnessHealth::OffTarget
    }
  }
}

/// Format seconds as `MM:SS.cc` (or `HH:MM:SS` when >= 1 hour).
pub fn format_timecode(secs: f64) -> String {
  let clamped = if secs.is_finite() && secs > 0.0 {
    secs
  } else {
    0.0
  };
  let total_secs = clamped.floor() as u64;
  let hours = total_secs / 3600;
  let mins = (total_secs % 3600) / 60;
  let rem = clamped - (hours * 3600 + mins * 60) as f64;
  if hours > 0 {
    format!("{hours:02}:{mins:02}:{rem:05.2}")
  } else {
    format!("{mins:02}:{rem:05.2}")
  }
}

/// Format a loudness/peak dB reading cleanly, rendering non-finite values as `--`.
pub fn format_db(val: f32, unit: &str) -> String {
  if val.is_finite() {
    format!("{val:+.1} {unit}")
  } else {
    format!("-- {unit}")
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_starter_project_is_valid_and_roundtrips() {
    let project = StudioProject::starter();
    assert!(project.script.validate().is_ok());
    let json = project.to_json_pretty().expect("serialize");
    let loaded = StudioProject::from_json(&json).expect("deserialize");
    assert_eq!(loaded.version, STUDIO_PROJECT_VERSION);
    assert_eq!(loaded.script.title, project.script.title);
    assert_eq!(loaded.script.segments.len(), project.script.segments.len());
    assert_eq!(loaded.script.config.target_lufs, Some(-16.0));
  }

  #[test]
  fn test_from_json_imports_plain_cli_podcast_script() {
    let cli_json = r#"{
      "title": "CLI Episode",
      "speakers": {
        "host": { "voice_id": "alloy" },
        "guest": { "voice_id": "echo" }
      },
      "segments": [
        { "speaker": "host", "text": "Hello from CLI!" },
        { "speaker": "guest", "text": "Works in Studio too." }
      ]
    }"#;
    let project = StudioProject::from_json(cli_json).expect("import CLI script");
    assert_eq!(project.script.title, "CLI Episode");
    assert_eq!(project.tags.title, "CLI Episode");
    assert_eq!(project.script.segments.len(), 2);
    assert!(project.use_cache);
  }

  #[test]
  fn test_segment_fingerprint_changes_on_any_synthesis_input() {
    let mut project = StudioProject::starter();
    let fp0 = project.segment_fingerprint(0).expect("fp0");

    // Changing text invalidates fingerprint.
    project.script.segments[0].text.push_str(" 新增一句话");
    let fp1 = project.segment_fingerprint(0).expect("fp1");
    assert_ne!(fp0, fp1);

    // Changing provider invalidates fingerprint.
    project.tts_provider = Some("minimax".to_string());
    let fp2 = project.segment_fingerprint(0).expect("fp2");
    assert_ne!(fp1, fp2);

    // Changing speaker speed invalidates fingerprint.
    if let Some(host) = project.script.speakers.get_mut("host") {
      host.speed = 1.15;
    }
    let fp3 = project.segment_fingerprint(0).expect("fp3");
    assert_ne!(fp2, fp3);

    // Changing chapter metadata does NOT invalidate audio cache fingerprint.
    project.script.segments[0].chapter = Some("新章节名".to_string());
    let fp4 = project.segment_fingerprint(0).expect("fp4");
    assert_eq!(fp3, fp4);
  }

  #[test]
  fn test_segment_and_speaker_crud_operations() {
    let mut project = StudioProject::starter();
    let initial_len = project.script.segments.len();

    // Insert after 0 alternates speaker from host -> guest.
    let new_idx = project.add_segment_after(Some(0), None);
    assert_eq!(new_idx, 1);
    assert_eq!(project.script.segments.len(), initial_len + 1);
    assert_eq!(project.script.segments[1].speaker, "guest");

    // Move down and back up.
    assert_eq!(project.move_segment(1, 1), Some(2));
    assert_eq!(project.move_segment(2, -1), Some(1));
    assert_eq!(project.move_segment(0, -1), None);

    // Duplicate & remove.
    let dup = project.duplicate_segment(1).expect("dup");
    assert_eq!(dup, 2);
    assert!(project.remove_segment(2));
    assert!(project.remove_segment(1));
    assert_eq!(project.script.segments.len(), initial_len);

    // Speaker CRUD.
    assert!(project.remove_speaker("host").is_err());
    project
      .add_speaker(
        "narrator".to_string(),
        VoiceConfig::new("zh-CN-XiaoyiNeural"),
      )
      .expect("add narrator");
    assert!(
      project
        .add_speaker("narrator".to_string(), VoiceConfig::default())
        .is_err()
    );
    project
      .remove_speaker("narrator")
      .expect("remove unused narrator");
  }

  #[test]
  fn test_undo_redo_history_roundtrip_and_bound() {
    let mut project = StudioProject::starter();
    let mut history = ProjectHistory::new();
    assert!(!history.can_undo());
    assert!(!history.can_redo());

    history.push_snapshot(&project);
    project.script.title = "Edited Title".to_string();
    assert!(history.can_undo());
    assert!(!history.can_redo());

    assert!(history.undo(&mut project));
    assert_eq!(project.script.title, "AI 播客工作台 · 第一期试音");
    assert!(history.can_redo());

    assert!(history.redo(&mut project));
    assert_eq!(project.script.title, "Edited Title");

    // Bound check: pushing > MAX_HISTORY_DEPTH never grows unboundedly.
    for i in 0..(MAX_HISTORY_DEPTH + 20) {
      history.push_snapshot(&project);
      project.script.title = format!("Title {i}");
    }
    assert_eq!(history.undo_stack.len(), MAX_HISTORY_DEPTH);
  }

  #[test]
  fn test_episode_tag_draft_reads_cover_and_validates_year() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cover = dir.path().join("cover.png");
    std::fs::write(&cover, [0x89, 0x50, 0x4E, 0x47]).expect("write png");

    let draft = EpisodeTagDraft {
      title: "".to_string(),
      artist: "Host".to_string(),
      album: "Show".to_string(),
      year: "2026".to_string(),
      comment: "Notes".to_string(),
      cover_path: Some(cover.to_string_lossy().to_string()),
    };
    let tags = draft.to_podcast_tags("Fallback Title").expect("tags");
    assert_eq!(tags.title.as_deref(), Some("Fallback Title"));
    assert_eq!(tags.year, Some(2026));
    assert_eq!(tags.cover.as_ref().map(|c| c.mime), Some(CoverMime::Png));

    let bad_year = EpisodeTagDraft {
      year: "not-a-year".to_string(),
      ..EpisodeTagDraft::default()
    };
    assert!(bad_year.to_podcast_tags("T").is_err());
  }

  #[test]
  fn test_loudness_health_and_formatting() {
    let view = EpisodeRenderView {
      duration_secs: 65.5,
      sample_rate: 44100,
      channels: 1,
      waveform_path: String::new(),
      integrated_lufs: -16.2,
      short_term_max_lufs: -14.0,
      range_lu: 4.5,
      true_peak_dbtp: -1.1,
      chapters: vec![],
      srt: String::new(),
    };
    assert_eq!(
      view.loudness_health(Some(-16.0)),
      LoudnessHealth::BroadcastReady
    );
    assert_eq!(view.loudness_health(Some(-23.0)), LoudnessHealth::OffTarget);

    let hot = EpisodeRenderView {
      true_peak_dbtp: -0.2,
      ..view
    };
    assert_eq!(
      hot.loudness_health(Some(-16.0)),
      LoudnessHealth::ClippingRisk
    );

    assert_eq!(format_timecode(65.5), "01:05.50");
    assert_eq!(format_timecode(3661.25), "01:01:01.25");
    assert_eq!(format_db(-16.04, "LUFS"), "-16.0 LUFS");
    assert_eq!(format_db(f32::NEG_INFINITY, "LUFS"), "-- LUFS");
  }

  #[test]
  fn test_spans_from_transcript_tags_fillers_and_merges_kept_ranges() {
    let transcript = phonon_ai::TranscriptResult {
      text: "嗯大家好，那个今天聊音频。".to_string(),
      language: Some("zh".to_string()),
      duration: Some(4.0),
      segments: vec![phonon_ai::TranscriptSegment {
        id: 0,
        start: 0.0,
        end: 4.0,
        text: "嗯大家好，那个今天聊音频。".to_string(),
      }],
    };
    let spans = spans_from_transcript(&transcript);
    assert!(spans.len() >= 4, "got {spans:?}");
    assert!(spans[0].is_filler && spans[0].text == "嗯");
    assert!(spans.iter().any(|s| s.is_filler && s.text == "那个"));

    let mut ov = SegmentMediaOverride {
      recorded_path: Some("rec.wav".to_string()),
      spans,
      ..SegmentMediaOverride::default()
    };
    // Before deleting fillers, contiguous spans merge into one continuous range.
    let full = ov.kept_ranges(4.0);
    assert_eq!(full.len(), 1);

    let removed = ov.delete_all_fillers();
    assert_eq!(removed, 2);
    let cut = ov.kept_ranges(4.0);
    assert_eq!(cut.len(), 2, "deleting two fillers leaves two kept clips");
    let text = ov.active_text().expect("active text");
    assert!(!text.contains('嗯') && !text.contains("那个"));
    assert!(text.contains("大家好"));
    assert!(text.contains("今天聊音频"));

    assert_eq!(ov.restore_all_spans(), 2);
    assert_eq!(ov.kept_ranges(4.0).len(), 1);
  }

  #[test]
  fn test_segment_fingerprint_invalidates_on_override_changes() {
    let mut project = StudioProject::starter();
    let fp_tts = project.segment_fingerprint(0).expect("fp_tts");

    project.segment_overrides.insert(
      0,
      SegmentMediaOverride {
        recorded_path: Some("host.wav".to_string()),
        enhance_voice: false,
        spans: vec![
          TranscriptCutSpan {
            id: 0,
            start_secs: 0.0,
            end_secs: 1.0,
            text: "嗯".to_string(),
            deleted: false,
            is_filler: true,
          },
          TranscriptCutSpan {
            id: 1,
            start_secs: 1.0,
            end_secs: 2.0,
            text: "欢迎".to_string(),
            deleted: false,
            is_filler: false,
          },
        ],
        ..SegmentMediaOverride::default()
      },
    );
    let fp_rec = project.segment_fingerprint(0).expect("fp_rec");
    assert_ne!(fp_tts, fp_rec);

    // Toggling enhance_voice invalidates fingerprint.
    if let Some(ov) = project.segment_overrides.get_mut(&0) {
      ov.enhance_voice = true;
    }
    let fp_enh = project.segment_fingerprint(0).expect("fp_enh");
    assert_ne!(fp_rec, fp_enh);

    // Deleting a cut span invalidates fingerprint.
    if let Some(ov) = project.segment_overrides.get_mut(&0) {
      ov.spans[0].deleted = true;
    }
    let fp_cut = project.segment_fingerprint(0).expect("fp_cut");
    assert_ne!(fp_enh, fp_cut);
  }
}
