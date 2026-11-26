use super::pick_audio_path;
use crate::project::{SegmentMediaOverride, format_db, format_timecode};
use crate::state::{AppState, InspectorTab, play_buffer, stop_mic_recording};
use crate::studio_ops::{
  EpisodeExportFormat, align_recorded_segment, assemble_episode, draft_script_from_topic,
  export_episode_file, recording_output_path, save_recorded_wav_atomic, synthesize_segment,
};
use dioxus::prelude::*;
use dioxus_shadcn::SonnerVariant;
use std::collections::HashMap;
use std::sync::Arc;

pub(super) async fn run_preview_segment(state: &mut AppState, seg_idx: usize, force_refresh: bool) {
  let project = state.project.read().clone();
  let Some(fp) = project.segment_fingerprint(seg_idx) else {
    state.notify(
      format!("Segment {} is invalid", seg_idx + 1),
      SonnerVariant::Error,
    );
    return;
  };
  let speaker = project
    .script
    .segments
    .get(seg_idx)
    .map(|s| s.speaker.clone())
    .unwrap_or_default();
  let label = format!("Segment {:02} · {}", seg_idx + 1, speaker);

  // Instant playback from in-memory cache when fresh and not force-regenerating.
  let cached = state.segment_audio.read().get(&seg_idx).cloned();
  if !force_refresh
    && let Some(cached) = cached
    && cached.fingerprint == fp
  {
    play_buffer(state, cached.buffer, label, false, None).await;
    return;
  }

  state.synthesizing_segments.write().insert(seg_idx);
  state.status_msg.set(if force_refresh {
    format!("Regenerating {label}...")
  } else {
    format!("Synthesizing {label}...")
  });

  let res = synthesize_segment(&project, seg_idx, force_refresh).await;
  state.synthesizing_segments.write().remove(&seg_idx);

  match res {
    Ok(entry) => {
      let buf = entry.buffer.clone();
      state.segment_audio.write().insert(seg_idx, entry);
      play_buffer(state, buf, label, false, None).await;
    }
    Err(e) => {
      state.notify(format!("Preview failed: {e}"), SonnerVariant::Error);
    }
  }
}

pub(super) async fn run_assemble_episode(state: &mut AppState) {
  if (state.is_assembling)() {
    return;
  }
  state.is_assembling.set(true);
  state
    .status_msg
    .set("Synthesizing missing segments & mastering episode...".to_string());

  let project = state.project.read().clone();
  let existing = state.segment_audio.read().clone();
  let res = assemble_episode(&project, &existing).await;
  state.is_assembling.set(false);

  match res {
    Ok((episode, updated_cache, view)) => {
      let msg = format!(
        "Episode assembled ({}, {}, {})",
        format_timecode(view.duration_secs),
        format_db(view.integrated_lufs, "LUFS"),
        format_db(view.true_peak_dbtp, "dBTP")
      );
      state.segment_audio.set(updated_cache);
      state.rendered_episode.set(Some(Arc::new(episode)));
      state.episode_view.set(Some(view));
      state.inspector_tab.set(InspectorTab::Master);
      state.notify(msg, SonnerVariant::Success);
    }
    Err(e) => {
      state.notify(format!("Assembly failed: {e}"), SonnerVariant::Error);
    }
  }
}

pub(super) async fn run_ai_draft(
  state: &mut AppState,
  topic: String,
  target_segments: usize,
  language: String,
) {
  if (state.is_drafting)() {
    return;
  }
  state.is_drafting.set(true);
  state.status_msg.set(format!(
    "Drafting podcast script for \"{topic}\" via LLM..."
  ));

  let project = state.project.read().clone();
  let res = draft_script_from_topic(&project, topic, target_segments, language).await;
  state.is_drafting.set(false);

  match res {
    Ok(script) => {
      let segs = script.segments.len();
      let title = script.title.clone();
      state.mutate_project(|p| {
        p.script = script;
        p.tags.title = title;
      });
      state.segment_audio.set(HashMap::new());
      state.rendered_episode.set(None);
      state.episode_view.set(None);
      state.show_ai_draft.set(false);
      state.notify(
        format!("Drafted {segs} segments — ready to preview"),
        SonnerVariant::Success,
      );
    }
    Err(e) => {
      state.notify(format!("AI draft failed: {e}"), SonnerVariant::Error);
    }
  }
}

pub(super) async fn run_export_episode(
  state: &mut AppState,
  format: EpisodeExportFormat,
  label: &str,
  ext: &str,
) {
  let Some(episode) = state.rendered_episode.read().clone() else {
    state.notify(
      "Assemble the episode before exporting",
      SonnerVariant::Warning,
    );
    return;
  };
  let tags_res = {
    let proj = state.project.read();
    proj.tags.to_podcast_tags(&proj.script.title)
  };
  let tags = match tags_res {
    Ok(t) => t,
    Err(e) => {
      state.notify(e, SonnerVariant::Error);
      return;
    }
  };
  let default_stem = {
    let raw = state.project.read().script.title.clone();
    let safe: String = raw
      .chars()
      .map(|c| {
        if c.is_alphanumeric() || c == '-' || c == '_' {
          c
        } else {
          '_'
        }
      })
      .take(40)
      .collect();
    if safe.trim_matches('_').is_empty() {
      "episode".to_string()
    } else {
      safe
    }
  };

  let handle = rfd::AsyncFileDialog::new()
    .add_filter(label, &[ext])
    .set_file_name(format!("{default_stem}.{ext}"))
    .save_file()
    .await;
  let Some(file) = handle else {
    return;
  };
  let path = file.path().to_path_buf();
  let bitrate = (state.episode_mp3_bitrate)();
  state.status_msg.set(format!("Exporting {label}..."));

  match export_episode_file(episode, tags, format, path.clone(), bitrate).await {
    Ok(()) => state.notify(
      format!("Exported {label} to {}", path.display()),
      SonnerVariant::Success,
    ),
    Err(e) => state.notify(format!("Export failed: {e}"), SonnerVariant::Error),
  }
}

pub(super) async fn run_stop_segment_recording(
  state: &mut AppState,
  seg_idx: usize,
  language_hint: Option<String>,
) {
  let Some((_, buf)) = stop_mic_recording(state) else {
    return;
  };
  let project_path = state.project_path.read().clone();
  let label = format!("seg_{:02}", seg_idx + 1);
  let out_path = recording_output_path(project_path.as_deref(), &label);
  let save_path = out_path.clone();

  let saved = tokio::task::spawn_blocking(move || save_recorded_wav_atomic(&save_path, &buf)).await;
  match saved {
    Ok(Ok(())) => {
      let path_str = out_path.to_string_lossy().to_string();
      state.mutate_project(|p| {
        p.segment_overrides.insert(
          seg_idx,
          SegmentMediaOverride {
            recorded_path: Some(path_str.clone()),
            enhance_voice: true,
            ..SegmentMediaOverride::default()
          },
        );
      });
      state.segment_audio.write().remove(&seg_idx);
      run_align_recording(state, seg_idx, path_str, language_hint).await;
    }
    Ok(Err(e)) => state.notify(e, SonnerVariant::Error),
    Err(e) => state.notify(
      format!("Recording save task error: {e}"),
      SonnerVariant::Error,
    ),
  }
}

pub(super) async fn run_attach_recording(
  state: &mut AppState,
  seg_idx: usize,
  language_hint: Option<String>,
) {
  let Some(path) = pick_audio_path().await else {
    return;
  };
  state.mutate_project(|p| {
    p.segment_overrides.insert(
      seg_idx,
      SegmentMediaOverride {
        recorded_path: Some(path.clone()),
        enhance_voice: true,
        ..SegmentMediaOverride::default()
      },
    );
  });
  state.segment_audio.write().remove(&seg_idx);
  run_align_recording(state, seg_idx, path, language_hint).await;
}

pub(super) async fn run_align_recording(
  state: &mut AppState,
  seg_idx: usize,
  recorded_path: String,
  language_hint: Option<String>,
) {
  state.aligning_segments.write().insert(seg_idx);
  state.status_msg.set(format!(
    "Aligning recording for segment {:02}...",
    seg_idx + 1
  ));

  let res = align_recorded_segment(recorded_path, language_hint).await;
  state.aligning_segments.write().remove(&seg_idx);

  match res {
    Ok((spans, transcript_text, used_stt)) => {
      let count = spans.len();
      let fillers = spans.iter().filter(|s| s.is_filler).count();
      state.mutate_project(|p| {
        if let Some(ov) = p.segment_overrides.get_mut(&seg_idx) {
          ov.spans = spans;
        }
        if used_stt
          && !transcript_text.trim().is_empty()
          && let Some(seg) = p.script.segments.get_mut(seg_idx)
        {
          seg.text = transcript_text;
        }
      });
      state.notify(
        if used_stt {
          format!(
            "STT aligned segment {:02}: {count} clickable spans ({fillers} fillers)",
            seg_idx + 1
          )
        } else {
          format!(
            "Offline VAD detected {count} speech clip(s) in segment {:02}",
            seg_idx + 1
          )
        },
        SonnerVariant::Success,
      );
    }
    Err(e) => {
      state.notify(format!("Alignment failed: {e}"), SonnerVariant::Error);
    }
  }
}

pub(super) fn swap_cached_entries(state: &mut AppState, a: usize, b: usize) {
  let mut cache = state.segment_audio.write();
  let ea = cache.remove(&a);
  let eb = cache.remove(&b);
  if let Some(entry) = ea {
    cache.insert(b, entry);
  }
  if let Some(entry) = eb {
    cache.insert(a, entry);
  }
}

pub(super) fn shift_cache_after_insert(state: &mut AppState, inserted_at: usize) {
  let old: HashMap<_, _> = std::mem::take(&mut *state.segment_audio.write());
  let mut next = HashMap::with_capacity(old.len());
  for (k, v) in old {
    next.insert(if k >= inserted_at { k + 1 } else { k }, v);
  }
  *state.segment_audio.write() = next;
}

pub(super) fn shift_cache_after_remove(state: &mut AppState, removed_at: usize) {
  let old: HashMap<_, _> = std::mem::take(&mut *state.segment_audio.write());
  let mut next = HashMap::with_capacity(old.len());
  for (k, v) in old {
    if k < removed_at {
      next.insert(k, v);
    } else if k > removed_at {
      next.insert(k - 1, v);
    }
  }
  *state.segment_audio.write() = next;
}
