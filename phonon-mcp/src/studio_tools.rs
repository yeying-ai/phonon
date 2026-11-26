//! Studio co-editing & voice-processing MCP tools.
//!
//! Enables external LLM agents (Claude Desktop, Cursor, Codex, etc.) to
//! inspect and atomically edit `.phonon.json` Studio projects (or CLI
//! `PodcastScript` JSON files) inside `PHONON_MCP_ROOT` so Phonon Studio can
//! hot-reload changes in real time with full Undo safety.

use crate::error::McpError;
use crate::tools::{Tool, ToolHandler, ToolRegistry, resolve_for_read, resolve_for_write, store};
use phonon_core::{VoiceEnhanceConfig, enhance_voice, measure_loudness, splice_ranges};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

static ATOMIC_SEQ: AtomicU64 = AtomicU64::new(0);

/// Register Phase 2 voice DSP (`audio_enhance_voice`, `audio_splice`) and
/// Studio project co-editing tools (`studio_*`).
pub(crate) fn register_phase2_tools(r: &mut ToolRegistry) {
  register_audio_enhance_voice(r);
  register_audio_splice(r);
  register_studio_get_project(r);
  register_studio_edit_segment(r);
  register_studio_add_segment(r);
  register_studio_delete_segment(r);
  register_studio_set_mastering(r);
}

// ─── Audio Voice Enhance & Splice ─────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct EnhanceVoiceArgs {
  handle: crate::registry::AudioHandle,
  #[serde(default)]
  preset: Option<String>,
  #[serde(default)]
  denoise_strength: Option<f32>,
  #[serde(default)]
  target_lufs: Option<f32>,
}

fn register_audio_enhance_voice(r: &mut ToolRegistry) {
  let registry = r.registry.clone();
  let handler: ToolHandler = Arc::new(move |args| {
    let registry = registry.clone();
    Box::pin(async move {
      let a: EnhanceVoiceArgs =
        serde_json::from_value(args).map_err(|e| McpError::InvalidArgs(e.to_string()))?;
      let mut cfg = match a.preset.as_deref().unwrap_or("podcast_studio") {
        "podcast_studio" => VoiceEnhanceConfig::podcast_studio(),
        "noisy_room" => VoiceEnhanceConfig::noisy_room(),
        "clean_mic" => VoiceEnhanceConfig::clean_mic(),
        other => {
          return Err(McpError::InvalidArgs(format!(
            "unknown preset '{other}' (expected 'podcast_studio', 'noisy_room', or 'clean_mic')"
          )));
        }
      };
      if let Some(d) = a.denoise_strength {
        if !d.is_finite() || !(0.0..=1.0).contains(&d) {
          return Err(McpError::InvalidArgs(
            "denoise_strength must be within [0.0, 1.0]".to_string(),
          ));
        }
        cfg.denoise_strength = d;
      }
      if let Some(lufs) = a.target_lufs {
        if !lufs.is_finite() || !(-60.0..=-5.0).contains(&lufs) {
          return Err(McpError::InvalidArgs(
            "target_lufs must be within [-60.0, -5.0]".to_string(),
          ));
        }
        cfg.target_lufs = Some(lufs);
      }

      let src = registry.get(&a.handle)?;
      let enhanced = tokio::task::spawn_blocking(move || enhance_voice(&src, &cfg))
        .await
        .map_err(|e| McpError::Io(format!("enhance task failed: {e}")))??;
      let report = measure_loudness(&enhanced);
      let mut res = store(&registry, enhanced)?;
      if let Some(obj) = res.as_object_mut() {
        obj.insert(
          "integrated_lufs".to_string(),
          finite_or_null(report.integrated_lufs),
        );
        obj.insert(
          "true_peak_dbtp".to_string(),
          finite_or_null(report.true_peak_dbtp),
        );
      }
      Ok(res)
    })
  });

  r.register(Tool {
    name: "audio_enhance_voice".into(),
    description:
      "Enhance spoken voice locally (80Hz high-pass, adaptive STFT spectral denoise, broadcast vocal EQ, compression, and BS.1770 LUFS mastering)."
        .into(),
    input_schema: json!({
      "type": "object",
      "properties": {
        "handle": { "type": "string", "description": "UUID handle of an audio buffer" },
        "preset": {
          "type": "string",
          "enum": ["podcast_studio", "noisy_room", "clean_mic"],
          "description": "Voice enhancement preset (default: podcast_studio)"
        },
        "denoise_strength": { "type": "number", "minimum": 0.0, "maximum": 1.0 },
        "target_lufs": { "type": "number", "minimum": -60.0, "maximum": -5.0 }
      },
      "required": ["handle"]
    }),
    handler,
  });
}

#[derive(Debug, Deserialize)]
struct SpliceArgs {
  handle: crate::registry::AudioHandle,
  keep_ranges: Vec<[f64; 2]>,
  #[serde(default = "default_crossfade")]
  crossfade_secs: f64,
}

fn default_crossfade() -> f64 {
  0.015
}

fn register_audio_splice(r: &mut ToolRegistry) {
  let registry = r.registry.clone();
  let handler: ToolHandler = Arc::new(move |args| {
    let registry = registry.clone();
    Box::pin(async move {
      let a: SpliceArgs =
        serde_json::from_value(args).map_err(|e| McpError::InvalidArgs(e.to_string()))?;
      let ranges: Vec<(f64, f64)> = a.keep_ranges.into_iter().map(|[s, e]| (s, e)).collect();
      let src = registry.get(&a.handle)?;
      let xfade = a.crossfade_secs;
      let spliced = tokio::task::spawn_blocking(move || splice_ranges(&src, &ranges, xfade))
        .await
        .map_err(|e| McpError::Io(format!("splice task failed: {e}")))??;
      store(&registry, spliced)
    })
  });

  r.register(Tool {
    name: "audio_splice".into(),
    description:
      "Non-destructively keep only the specified [start_secs, end_secs] time ranges and stitch them with smooth cosine micro-crossfades."
        .into(),
    input_schema: json!({
      "type": "object",
      "properties": {
        "handle": { "type": "string" },
        "keep_ranges": {
          "type": "array",
          "items": {
            "type": "array",
            "items": { "type": "number" },
            "minItems": 2,
            "maxItems": 2
          },
          "description": "Ordered list of [start_secs, end_secs] intervals to retain"
        },
        "crossfade_secs": { "type": "number", "minimum": 0.0, "default": 0.015 }
      },
      "required": ["handle", "keep_ranges"]
    }),
    handler,
  });
}

// ─── Studio Project JSON Co-Editing Tools ─────────────────────────────────────

fn read_project_json(root: &Path, rel_path: &str) -> Result<(std::path::PathBuf, Value), McpError> {
  let path = resolve_for_read(root, rel_path)?;
  let raw = std::fs::read_to_string(&path)
    .map_err(|e| McpError::Io(format!("read {}: {e}", path.display())))?;
  let value: Value = serde_json::from_str(&raw)
    .map_err(|e| McpError::InvalidArgs(format!("invalid JSON in {rel_path}: {e}")))?;
  if !value.is_object() {
    return Err(McpError::InvalidArgs(
      "project JSON root must be an object".to_string(),
    ));
  }
  Ok((path, value))
}

/// Return a mutable reference to the script object inside either a
/// `StudioProject` (`root["script"]`) or a plain CLI `PodcastScript` (`root`).
fn script_obj_mut(doc: &mut Value) -> Result<&mut Map<String, Value>, McpError> {
  if doc.get("script").and_then(Value::as_object).is_some() {
    doc
      .get_mut("script")
      .and_then(Value::as_object_mut)
      .ok_or_else(|| McpError::InvalidArgs("invalid script object".to_string()))
  } else {
    doc
      .as_object_mut()
      .ok_or_else(|| McpError::InvalidArgs("project root must be an object".to_string()))
  }
}

fn script_obj_ref(doc: &Value) -> Result<&Map<String, Value>, McpError> {
  if let Some(script) = doc.get("script").and_then(Value::as_object) {
    Ok(script)
  } else {
    doc
      .as_object()
      .ok_or_else(|| McpError::InvalidArgs("project root must be an object".to_string()))
  }
}

/// Atomically persist `doc` to `rel_path` (same-directory temp file + rename)
/// after verifying the target path stays confined within `root`.
fn write_project_atomic(root: &Path, rel_path: &str, doc: &Value) -> Result<(), McpError> {
  let target = resolve_for_write(root, rel_path, true)?;
  let parent = target
    .parent()
    .ok_or_else(|| McpError::Io("project path has no parent directory".to_string()))?;
  let pretty = serde_json::to_string_pretty(doc)?;
  let seq = ATOMIC_SEQ.fetch_add(1, Ordering::Relaxed);
  let pid = std::process::id();
  let tmp = parent.join(format!(".phonon-mcp.{pid}.{seq}.tmp.json"));
  if let Err(e) = std::fs::write(&tmp, pretty) {
    let _ = std::fs::remove_file(&tmp);
    return Err(McpError::Io(format!("write temp project failed: {e}")));
  }
  if let Err(e) = std::fs::rename(&tmp, &target) {
    let _ = std::fs::remove_file(&tmp);
    return Err(McpError::Io(format!("atomic rename failed: {e}")));
  }
  Ok(())
}

#[derive(Debug, Deserialize)]
struct ProjectPathArgs {
  path: String,
}

fn register_studio_get_project(r: &mut ToolRegistry) {
  let fs_root = r.fs_root.clone();
  let handler: ToolHandler = Arc::new(move |args| {
    let fs_root = fs_root.clone();
    Box::pin(async move {
      let ProjectPathArgs { path } =
        serde_json::from_value(args).map_err(|e| McpError::InvalidArgs(e.to_string()))?;
      let (_, doc) = read_project_json(&fs_root, &path)?;
      let script = script_obj_ref(&doc)?;
      let title = script.get("title").and_then(Value::as_str).unwrap_or("");
      let speakers: Vec<String> = script
        .get("speakers")
        .and_then(Value::as_object)
        .map(|m| m.keys().cloned().collect())
        .unwrap_or_default();
      let overrides = doc.get("segment_overrides").and_then(Value::as_object);
      let segments: Vec<Value> = script
        .get("segments")
        .and_then(Value::as_array)
        .map(|arr| {
          arr
            .iter()
            .enumerate()
            .map(|(i, seg)| {
              let ov = overrides.and_then(|m| m.get(&i.to_string()));
              json!({
                "index": i,
                "speaker": seg.get("speaker").and_then(Value::as_str).unwrap_or(""),
                "text": seg.get("text").and_then(Value::as_str).unwrap_or(""),
                "chapter": seg.get("chapter").cloned().unwrap_or(Value::Null),
                "language": seg.get("language").cloned().unwrap_or(Value::Null),
                "recorded_path": ov.and_then(|o| o.get("recorded_path")).cloned().unwrap_or(Value::Null),
                "enhance_voice": ov.and_then(|o| o.get("enhance_voice")).and_then(Value::as_bool).unwrap_or(false),
              })
            })
            .collect()
        })
        .unwrap_or_default();

      Ok(json!({
        "path": path,
        "title": title,
        "tts_provider": doc.get("tts_provider").cloned().unwrap_or(Value::Null),
        "speakers": speakers,
        "segment_count": segments.len(),
        "segments": segments,
        "config": script.get("config").cloned().unwrap_or(json!({})),
      }))
    })
  });

  r.register(Tool {
    name: "studio_get_project".into(),
    description:
      "Inspect a Phonon Studio project (.phonon.json) or CLI podcast script inside PHONON_MCP_ROOT."
        .into(),
    input_schema: json!({
      "type": "object",
      "properties": {
        "path": { "type": "string", "description": "Path to .phonon.json or script JSON inside PHONON_MCP_ROOT" }
      },
      "required": ["path"]
    }),
    handler,
  });
}

#[derive(Debug, Deserialize)]
struct EditSegmentArgs {
  path: String,
  index: usize,
  #[serde(default)]
  text: Option<String>,
  #[serde(default)]
  speaker: Option<String>,
  #[serde(default)]
  chapter: Option<String>,
  #[serde(default)]
  language: Option<String>,
  #[serde(default)]
  enhance_voice: Option<bool>,
  #[serde(default)]
  delete_filler_spans: Option<bool>,
}

fn register_studio_edit_segment(r: &mut ToolRegistry) {
  let fs_root = r.fs_root.clone();
  let handler: ToolHandler = Arc::new(move |args| {
    let fs_root = fs_root.clone();
    Box::pin(async move {
      let a: EditSegmentArgs =
        serde_json::from_value(args).map_err(|e| McpError::InvalidArgs(e.to_string()))?;
      let (_, mut doc) = read_project_json(&fs_root, &a.path)?;

      let speaker_exists = a.speaker.as_ref().map(|sp| {
        script_obj_ref(&doc)
          .ok()
          .and_then(|s| s.get("speakers"))
          .and_then(Value::as_object)
          .is_some_and(|m| m.contains_key(sp))
      });
      if let Some(false) = speaker_exists {
        return Err(McpError::InvalidArgs(format!(
          "unknown speaker '{}'",
          a.speaker.as_deref().unwrap_or("")
        )));
      }

      {
        let script = script_obj_mut(&mut doc)?;
        let segments = script
          .get_mut("segments")
          .and_then(Value::as_array_mut)
          .ok_or_else(|| McpError::InvalidArgs("project has no segments array".to_string()))?;
        let seg_len = segments.len();
        let seg = segments
          .get_mut(a.index)
          .and_then(Value::as_object_mut)
          .ok_or_else(|| {
            McpError::InvalidArgs(format!(
              "segment index {} out of range (total {})",
              a.index, seg_len
            ))
          })?;

        if let Some(text) = a.text {
          if text.trim().is_empty() {
            return Err(McpError::InvalidArgs(
              "segment text cannot be empty".to_string(),
            ));
          }
          seg.insert("text".to_string(), Value::String(text));
        }
        if let Some(sp) = a.speaker {
          seg.insert("speaker".to_string(), Value::String(sp));
        }
        if let Some(ch) = a.chapter {
          if ch.trim().is_empty() {
            seg.remove("chapter");
          } else {
            seg.insert("chapter".to_string(), Value::String(ch));
          }
        }
        if let Some(lang) = a.language {
          if lang.trim().is_empty() {
            seg.remove("language");
          } else {
            seg.insert("language".to_string(), Value::String(lang));
          }
        }
      }

      // Optional Studio-level media override edits (enhance_voice / filler span deletion).
      let mut fillers_marked = 0usize;
      if (a.enhance_voice.is_some() || a.delete_filler_spans == Some(true))
        && let Some(root_obj) = doc.as_object_mut()
      {
        let overrides = root_obj
          .entry("segment_overrides".to_string())
          .or_insert_with(|| Value::Object(Map::new()));
        if let Some(ov_map) = overrides.as_object_mut() {
          let entry = ov_map
            .entry(a.index.to_string())
            .or_insert_with(|| Value::Object(Map::new()));
          if let Some(ov_obj) = entry.as_object_mut() {
            if let Some(enh) = a.enhance_voice {
              ov_obj.insert("enhance_voice".to_string(), Value::Bool(enh));
            }
            if a.delete_filler_spans == Some(true)
              && let Some(spans) = ov_obj.get_mut("spans").and_then(Value::as_array_mut)
            {
              for span in spans {
                if span.get("is_filler").and_then(Value::as_bool) == Some(true)
                  && let Some(so) = span.as_object_mut()
                {
                  so.insert("deleted".to_string(), Value::Bool(true));
                  fillers_marked += 1;
                }
              }
            }
          }
        }
      }

      write_project_atomic(&fs_root, &a.path, &doc)?;
      Ok(json!({
        "updated": true,
        "index": a.index,
        "fillers_marked_deleted": fillers_marked,
      }))
    })
  });

  r.register(Tool {
    name: "studio_edit_segment".into(),
    description:
      "Atomically edit a segment's text, speaker, chapter, language, enhance_voice flag, or mark filler spans deleted in a Phonon Studio project."
        .into(),
    input_schema: json!({
      "type": "object",
      "properties": {
        "path": { "type": "string" },
        "index": { "type": "integer", "minimum": 0 },
        "text": { "type": "string" },
        "speaker": { "type": "string" },
        "chapter": { "type": "string" },
        "language": { "type": "string" },
        "enhance_voice": { "type": "boolean" },
        "delete_filler_spans": { "type": "boolean" }
      },
      "required": ["path", "index"]
    }),
    handler,
  });
}

#[derive(Debug, Deserialize)]
struct AddSegmentArgs {
  path: String,
  speaker: String,
  text: String,
  #[serde(default)]
  after_index: Option<usize>,
  #[serde(default)]
  chapter: Option<String>,
  #[serde(default)]
  language: Option<String>,
}

fn register_studio_add_segment(r: &mut ToolRegistry) {
  let fs_root = r.fs_root.clone();
  let handler: ToolHandler = Arc::new(move |args| {
    let fs_root = fs_root.clone();
    Box::pin(async move {
      let a: AddSegmentArgs =
        serde_json::from_value(args).map_err(|e| McpError::InvalidArgs(e.to_string()))?;
      if a.text.trim().is_empty() {
        return Err(McpError::InvalidArgs("text cannot be empty".to_string()));
      }
      let (_, mut doc) = read_project_json(&fs_root, &a.path)?;
      let inserted_at = {
        let script = script_obj_mut(&mut doc)?;
        let has_speaker = script
          .get("speakers")
          .and_then(Value::as_object)
          .is_some_and(|m| m.contains_key(&a.speaker));
        if !has_speaker {
          return Err(McpError::InvalidArgs(format!(
            "unknown speaker '{}'",
            a.speaker
          )));
        }
        let segments = script
          .get_mut("segments")
          .and_then(Value::as_array_mut)
          .ok_or_else(|| McpError::InvalidArgs("project has no segments array".to_string()))?;
        let mut seg = Map::new();
        seg.insert("speaker".to_string(), Value::String(a.speaker));
        seg.insert("text".to_string(), Value::String(a.text));
        if let Some(ch) = a.chapter.filter(|s| !s.trim().is_empty()) {
          seg.insert("chapter".to_string(), Value::String(ch));
        }
        if let Some(lang) = a.language.filter(|s| !s.trim().is_empty()) {
          seg.insert("language".to_string(), Value::String(lang));
        }
        let pos = match a.after_index {
          Some(i) if i < segments.len() => i + 1,
          _ => segments.len(),
        };
        segments.insert(pos, Value::Object(seg));
        pos
      };

      write_project_atomic(&fs_root, &a.path, &doc)?;
      Ok(json!({ "inserted_at": inserted_at }))
    })
  });

  r.register(Tool {
    name: "studio_add_segment".into(),
    description: "Insert a new dialogue segment into a Phonon Studio project atomically.".into(),
    input_schema: json!({
      "type": "object",
      "properties": {
        "path": { "type": "string" },
        "speaker": { "type": "string" },
        "text": { "type": "string" },
        "after_index": { "type": "integer", "minimum": 0 },
        "chapter": { "type": "string" },
        "language": { "type": "string" }
      },
      "required": ["path", "speaker", "text"]
    }),
    handler,
  });
}

#[derive(Debug, Deserialize)]
struct DeleteSegmentArgs {
  path: String,
  index: usize,
}

fn register_studio_delete_segment(r: &mut ToolRegistry) {
  let fs_root = r.fs_root.clone();
  let handler: ToolHandler = Arc::new(move |args| {
    let fs_root = fs_root.clone();
    Box::pin(async move {
      let a: DeleteSegmentArgs =
        serde_json::from_value(args).map_err(|e| McpError::InvalidArgs(e.to_string()))?;
      let (_, mut doc) = read_project_json(&fs_root, &a.path)?;
      let remaining = {
        let script = script_obj_mut(&mut doc)?;
        let segments = script
          .get_mut("segments")
          .and_then(Value::as_array_mut)
          .ok_or_else(|| McpError::InvalidArgs("project has no segments array".to_string()))?;
        if segments.len() <= 1 {
          return Err(McpError::InvalidArgs(
            "cannot delete the last remaining segment".to_string(),
          ));
        }
        if a.index >= segments.len() {
          return Err(McpError::InvalidArgs(format!(
            "segment index {} out of range",
            a.index
          )));
        }
        segments.remove(a.index);
        segments.len()
      };

      write_project_atomic(&fs_root, &a.path, &doc)?;
      Ok(json!({ "deleted": a.index, "remaining_segments": remaining }))
    })
  });

  r.register(Tool {
    name: "studio_delete_segment".into(),
    description: "Delete a segment from a Phonon Studio project atomically.".into(),
    input_schema: json!({
      "type": "object",
      "properties": {
        "path": { "type": "string" },
        "index": { "type": "integer", "minimum": 0 }
      },
      "required": ["path", "index"]
    }),
    handler,
  });
}

#[derive(Debug, Deserialize)]
struct SetMasteringArgs {
  path: String,
  #[serde(default)]
  target_lufs: Option<f32>,
  #[serde(default)]
  bgm_volume: Option<f32>,
  #[serde(default)]
  pause_between_segments_secs: Option<f64>,
  #[serde(default)]
  crossfade_secs: Option<f64>,
  #[serde(default)]
  fade_in_secs: Option<f64>,
  #[serde(default)]
  fade_out_secs: Option<f64>,
}

fn register_studio_set_mastering(r: &mut ToolRegistry) {
  let fs_root = r.fs_root.clone();
  let handler: ToolHandler = Arc::new(move |args| {
    let fs_root = fs_root.clone();
    Box::pin(async move {
      let a: SetMasteringArgs =
        serde_json::from_value(args).map_err(|e| McpError::InvalidArgs(e.to_string()))?;
      if let Some(lufs) = a.target_lufs
        && (!lufs.is_finite() || !(-60.0..=-5.0).contains(&lufs))
      {
        return Err(McpError::InvalidArgs(
          "target_lufs must be within [-60.0, -5.0]".to_string(),
        ));
      }
      if let Some(v) = a.bgm_volume
        && (!v.is_finite() || !(0.0..=1.0).contains(&v))
      {
        return Err(McpError::InvalidArgs(
          "bgm_volume must be within [0.0, 1.0]".to_string(),
        ));
      }
      for (name, val) in [
        ("pause_between_segments_secs", a.pause_between_segments_secs),
        ("crossfade_secs", a.crossfade_secs),
        ("fade_in_secs", a.fade_in_secs),
        ("fade_out_secs", a.fade_out_secs),
      ] {
        if let Some(v) = val
          && (!v.is_finite() || v < 0.0 || v > 30.0)
        {
          return Err(McpError::InvalidArgs(format!(
            "{name} must be finite and within [0.0, 30.0]"
          )));
        }
      }

      let (_, mut doc) = read_project_json(&fs_root, &a.path)?;
      let config_snapshot = {
        let script = script_obj_mut(&mut doc)?;
        let cfg = script
          .entry("config".to_string())
          .or_insert_with(|| Value::Object(Map::new()));
        let obj = cfg
          .as_object_mut()
          .ok_or_else(|| McpError::InvalidArgs("script.config must be an object".to_string()))?;
        if let Some(lufs) = a.target_lufs {
          obj.insert("target_lufs".to_string(), json!(lufs));
        }
        if let Some(v) = a.bgm_volume {
          obj.insert("bgm_volume".to_string(), json!(v));
        }
        if let Some(v) = a.pause_between_segments_secs {
          obj.insert("pause_between_segments_secs".to_string(), json!(v));
        }
        if let Some(v) = a.crossfade_secs {
          obj.insert("crossfade_secs".to_string(), json!(v));
        }
        if let Some(v) = a.fade_in_secs {
          obj.insert("fade_in_secs".to_string(), json!(v));
        }
        if let Some(v) = a.fade_out_secs {
          obj.insert("fade_out_secs".to_string(), json!(v));
        }
        Value::Object(obj.clone())
      };

      write_project_atomic(&fs_root, &a.path, &doc)?;
      Ok(json!({ "updated": true, "config": config_snapshot }))
    })
  });

  r.register(Tool {
    name: "studio_set_mastering".into(),
    description:
      "Update broadcast mastering & mix settings (target_lufs, bgm_volume, pause, crossfade, fades) in a Phonon Studio project."
        .into(),
    input_schema: json!({
      "type": "object",
      "properties": {
        "path": { "type": "string" },
        "target_lufs": { "type": "number", "minimum": -60.0, "maximum": -5.0 },
        "bgm_volume": { "type": "number", "minimum": 0.0, "maximum": 1.0 },
        "pause_between_segments_secs": { "type": "number", "minimum": 0.0, "maximum": 30.0 },
        "crossfade_secs": { "type": "number", "minimum": 0.0, "maximum": 30.0 },
        "fade_in_secs": { "type": "number", "minimum": 0.0, "maximum": 30.0 },
        "fade_out_secs": { "type": "number", "minimum": 0.0, "maximum": 30.0 }
      },
      "required": ["path"]
    }),
    handler,
  });
}

fn finite_or_null(v: f32) -> Value {
  if v.is_finite() { json!(v) } else { Value::Null }
}

#[cfg(test)]
mod tests {
  use super::*;
  use phonon_core::AudioBuffer;

  fn sample_project_json() -> String {
    json!({
      "version": 1,
      "script": {
        "title": "Demo Episode",
        "speakers": {
          "host": { "voice_id": "zh-CN-YunyangNeural" },
          "guest": { "voice_id": "zh-CN-XiaoxiaoNeural" }
        },
        "segments": [
          { "speaker": "host", "text": "Hello listeners." },
          { "speaker": "guest", "text": "Great to be here." }
        ],
        "config": {
          "bgm_volume": 0.08,
          "target_lufs": -16.0
        }
      },
      "segment_overrides": {
        "0": {
          "recorded_path": "host.wav",
          "enhance_voice": false,
          "spans": [
            { "id": 0, "start_secs": 0.0, "end_secs": 0.5, "text": "嗯", "deleted": false, "is_filler": true },
            { "id": 1, "start_secs": 0.5, "end_secs": 1.5, "text": "大家好", "deleted": false, "is_filler": false }
          ]
        }
      }
    })
    .to_string()
  }

  #[tokio::test]
  async fn test_studio_tools_inspect_edit_add_delete_and_mastering() {
    let dir = tempfile::tempdir().expect("tempdir");
    let proj_path = dir.path().join("ep.phonon.json");
    std::fs::write(&proj_path, sample_project_json()).expect("write");

    let r = ToolRegistry::new(dir.path());
    let info = r
      .call("studio_get_project", json!({ "path": "ep.phonon.json" }))
      .await
      .expect("get");
    assert_eq!(info["segment_count"], 2);

    let edited = r
      .call(
        "studio_edit_segment",
        json!({
          "path": "ep.phonon.json",
          "index": 0,
          "text": "Updated opening line.",
          "chapter": "Intro",
          "enhance_voice": true,
          "delete_filler_spans": true
        }),
      )
      .await
      .expect("edit");
    assert_eq!(edited["fillers_marked_deleted"], 1);

    let added = r
      .call(
        "studio_add_segment",
        json!({
          "path": "ep.phonon.json",
          "speaker": "host",
          "text": "Closing remarks.",
          "after_index": 1
        }),
      )
      .await
      .expect("add");
    assert_eq!(added["inserted_at"], 2);

    r.call(
      "studio_set_mastering",
      json!({ "path": "ep.phonon.json", "target_lufs": -14.0, "bgm_volume": 0.12 }),
    )
    .await
    .expect("mastering");

    let del = r
      .call(
        "studio_delete_segment",
        json!({ "path": "ep.phonon.json", "index": 2 }),
      )
      .await
      .expect("delete");
    assert_eq!(del["remaining_segments"], 2);

    // Path traversal outside root is denied.
    let err = r
      .call("studio_get_project", json!({ "path": "../escape.json" }))
      .await
      .unwrap_err();
    assert!(matches!(err, McpError::PathDenied(_) | McpError::Io(_)));
  }

  #[tokio::test]
  async fn test_audio_enhance_voice_and_splice_tools() {
    let dir = tempfile::tempdir().expect("tempdir");
    let r = ToolRegistry::new(dir.path());
    let sr = 16000u32;
    let samples: Vec<f32> = (0..(sr * 2))
      .map(|i| 0.3 * (2.0 * std::f64::consts::PI * 440.0 * i as f64 / sr as f64).sin() as f32)
      .collect();
    let h = r
      .registry
      .insert(AudioBuffer {
        samples,
        sample_rate: sr,
        channels: 1,
      })
      .expect("insert");

    let enh = r
      .call(
        "audio_enhance_voice",
        json!({ "handle": h, "preset": "podcast_studio", "target_lufs": -16.0 }),
      )
      .await
      .expect("enhance");
    assert!(enh["handle"].is_string());
    assert!(enh["integrated_lufs"].is_number());

    let spl = r
      .call(
        "audio_splice",
        json!({
          "handle": h,
          "keep_ranges": [[0.0, 0.5], [1.0, 1.5]],
          "crossfade_secs": 0.02
        }),
      )
      .await
      .expect("splice");
    let dur = spl["duration_secs"].as_f64().expect("dur");
    assert!((dur - 0.98).abs() < 0.02, "got {dur}");
  }
}
