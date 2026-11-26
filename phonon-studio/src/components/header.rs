#![allow(non_snake_case)]

use crate::project::{StudioProject, format_timecode};
use crate::state::{
  AppState, RecordingTarget, SidePanel, WorkspaceTab, build_display_info, load_audio,
  start_mic_recording, stop_mic_recording,
};
use crate::studio_ops::{
  recording_output_path, save_recorded_wav_atomic, write_project_atomic_file,
};
use dioxus::prelude::*;
use dioxus_shadcn::{
  Badge, BadgeSize, BadgeStyle, BadgeVariant, Button, ButtonSize, ButtonVariant, Separator,
  SeparatorOrientation, Tabs, TabsList, TabsTrigger, Tooltip, TooltipContent, TooltipSide,
  TooltipTrigger, button_class, use_density,
};
use phonon_engine::PlaybackState;
use std::collections::HashMap;
use std::sync::Arc;

#[component]
pub fn Header() -> Element {
  let mut state = use_context::<AppState>();
  let density = use_density();
  let active_tab = (state.active_tab)();
  let active_tab_key = match active_tab {
    WorkspaceTab::PodcastStudio => "podcast",
    WorkspaceTab::AudioLab => "audiolab",
  };
  let can_undo = state.history.read().can_undo();
  let can_redo = state.history.read().can_redo();
  let show_ai_draft = (state.show_ai_draft)();
  let show_mcp_panel = (state.show_mcp_panel)();
  let mcp_synced = state.project_path.read().is_some() && (state.mcp_live_sync)();

  // Resolve active TTS provider badge without holding signal borrows across RSX.
  let (provider_label, provider_ready) = {
    let proj = state.project.read();
    let statuses = state.provider_statuses.read();
    match proj.tts_provider.as_deref().filter(|p| *p != "auto") {
      Some(name) => {
        let ready = statuses
          .iter()
          .find(|s| s.name == name)
          .map(|s| s.key_present)
          .unwrap_or(true);
        (name.to_string(), ready)
      }
      None => {
        let auto = statuses
          .iter()
          .find(|s| s.has_tts && s.key_present)
          .map(|s| s.name)
          .unwrap_or("edge");
        (format!("Auto ({auto})"), true)
      }
    }
  };

  let mcp_trigger_class = if show_mcp_panel {
    button_class(
      ButtonVariant::Primary,
      ButtonSize::Xs,
      density,
      "gap-1.5 bg-info text-info-foreground hover:bg-info/90",
    )
  } else {
    button_class(
      ButtonVariant::Outline,
      ButtonSize::Xs,
      density,
      "gap-1.5 bg-secondary text-foreground hover:border-info",
    )
  };

  let undo_trigger_class = button_class(ButtonVariant::Secondary, ButtonSize::Xs, density, "px-2");

  rsx! {
    header {
      class: "h-12 shrink-0 bg-card px-4 flex items-center justify-between border-b border-border select-none",

      // Left: Brand + Workspace segmented switcher (dioxus-shadcn Tabs)
      div {
        class: "flex items-center gap-4",
        div {
          class: "flex items-center gap-2 font-semibold text-sm tracking-tight text-foreground",
          span { class: "inline-block w-2 h-2 rounded-full bg-primary" }
          "Phonon Studio"
        }

        Tabs {
          value: active_tab_key.to_string(),
          on_value_change: move |next: String| {
            match next.as_str() {
              "audiolab" => state.active_tab.set(WorkspaceTab::AudioLab),
              _ => state.active_tab.set(WorkspaceTab::PodcastStudio),
            }
          },
          TabsList {
            class: "h-8 p-0.5 bg-background border border-border",
            "aria-label": "Studio workspaces",
            TabsTrigger {
              value: "podcast",
              class: "h-7 px-3 py-1 text-xs",
              "Podcast Studio"
            }
            TabsTrigger {
              value: "audiolab",
              class: "h-7 px-3 py-1 text-xs",
              "Audio Lab"
            }
          }
        }
      }

      // Right: Context-aware actions
      if active_tab == WorkspaceTab::PodcastStudio {
        div {
          class: "flex items-center gap-2",

          // Provider readiness pill
          Badge {
            variant: if provider_ready { BadgeVariant::Success } else { BadgeVariant::Destructive },
            style: BadgeStyle::Soft,
            size: BadgeSize::Sm,
            class: "hidden md:inline-flex items-center gap-1.5 font-normal",
            span {
              class: if provider_ready {
                "inline-block w-1.5 h-1.5 rounded-full bg-success"
              } else {
                "inline-block w-1.5 h-1.5 rounded-full bg-destructive"
              }
            }
            span { class: "font-mono tabular-nums", "TTS: {provider_label}" }
          }

          Button {
            variant: if show_ai_draft { ButtonVariant::Primary } else { ButtonVariant::Outline },
            size: ButtonSize::Xs,
            class: if show_ai_draft { String::new() } else { "bg-secondary hover:border-primary".to_string() },
            onclick: move |_| state.show_ai_draft.set(!show_ai_draft),
            "AI Draft"
          }

          Tooltip {
            delay_ms: 400,
            TooltipTrigger {
              class: mcp_trigger_class,
              onclick: move |_| state.show_mcp_panel.set(!show_mcp_panel),
              span {
                class: if mcp_synced {
                  "inline-block w-1.5 h-1.5 rounded-full bg-success"
                } else {
                  "inline-block w-1.5 h-1.5 rounded-full bg-muted-foreground"
                }
              }
              "MCP Sync"
            }
            TooltipContent {
              side: TooltipSide::Bottom,
              "Configure MCP Agent live co-editing"
            }
          }

          Separator {
            orientation: SeparatorOrientation::Vertical,
            decorative: true,
            class: "h-4 mx-0.5",
          }

          Tooltip {
            delay_ms: 400,
            TooltipTrigger {
              class: undo_trigger_class.clone(),
              disabled: !can_undo,
              onclick: move |_| {
                state.undo_project();
              },
              "Undo"
            }
            TooltipContent {
              side: TooltipSide::Bottom,
              "Undo last project edit"
            }
          }

          Tooltip {
            delay_ms: 400,
            TooltipTrigger {
              class: undo_trigger_class,
              disabled: !can_redo,
              onclick: move |_| {
                state.redo_project();
              },
              "Redo"
            }
            TooltipContent {
              side: TooltipSide::Bottom,
              "Redo project edit"
            }
          }

          Separator {
            orientation: SeparatorOrientation::Vertical,
            decorative: true,
            class: "h-4 mx-0.5",
          }

          Button {
            variant: ButtonVariant::Secondary,
            size: ButtonSize::Xs,
            onclick: move |_| {
              state.mutate_project(|p| *p = StudioProject::starter());
              state.project_path.set(None);
              state.segment_audio.set(HashMap::new());
              state.rendered_episode.set(None);
              state.episode_view.set(None);
              state.status_msg.set("Created new starter podcast project".to_string());
            },
            "New"
          }
          Button {
            variant: ButtonVariant::Secondary,
            size: ButtonSize::Xs,
            onclick: move |_| async move {
              open_podcast_project(&mut state).await;
            },
            "Open Script"
          }
          Button {
            variant: ButtonVariant::Secondary,
            size: ButtonSize::Xs,
            onclick: move |_| async move {
              save_podcast_project(&mut state).await;
            },
            "Save"
          }
        }
      } else {
        {
          let rec_target = (state.recording_target)();
          let rec_peak = (state.recording_peak)();
          let rec_secs = (state.recording_secs)();
          let vu_pct = (rec_peak.clamp(0.0, 1.0) * 100.0).round() as u32;
          let rec_tc = format_timecode(rec_secs);
          let rec_trigger_class = button_class(
            ButtonVariant::Outline,
            ButtonSize::Xs,
            density,
            "bg-secondary text-destructive hover:border-destructive",
          );
          rsx! {
            div {
              class: "flex items-center gap-2",
              SideToggle { label: "Effects", panel: SidePanel::Effects }
              SideToggle { label: "EQ", panel: SidePanel::Eq }
              SideToggle { label: "Plugins", panel: SidePanel::Plugins }
              SideToggle { label: "Export", panel: SidePanel::Export }

              Separator {
                orientation: SeparatorOrientation::Vertical,
                decorative: true,
                class: "h-4 mx-0.5",
              }

              if rec_target == Some(RecordingTarget::AudioLab) {
                div {
                  class: "flex items-center gap-2 px-2 py-0.5 rounded-md bg-destructive/15 border border-destructive/60 text-xs text-destructive",
                  span { class: "inline-block w-2 h-2 rounded-full bg-destructive animate-pulse" }
                  span { class: "font-mono tabular-nums", "{rec_tc}" }
                  div {
                    class: "w-14 h-1.5 rounded bg-background overflow-hidden",
                    div {
                      class: "h-full bg-destructive transition-all duration-75",
                      style: "width: {vu_pct}%;",
                    }
                  }
                  Button {
                    variant: ButtonVariant::Destructive,
                    size: ButtonSize::Xs,
                    class: "h-6 px-2 text-xs font-semibold",
                    onclick: move |_| async move {
                      stop_audiolab_recording(&mut state).await;
                    },
                    "■ Stop"
                  }
                }
              } else {
                Tooltip {
                  delay_ms: 400,
                  TooltipTrigger {
                    class: rec_trigger_class,
                    disabled: rec_target.is_some(),
                    onclick: move |_| {
                      if let Err(e) = start_mic_recording(&mut state, RecordingTarget::AudioLab) {
                        state.status_msg.set(e);
                      }
                    },
                    "● Record"
                  }
                  TooltipContent {
                    side: TooltipSide::Bottom,
                    "Record directly from your default microphone into Audio Lab"
                  }
                }
              }

              Button {
                variant: ButtonVariant::Primary,
                size: ButtonSize::Xs,
                onclick: move |_| async move {
                  open_audio_file(&mut state).await;
                },
                "Open Audio"
              }
            }
          }
        }
      }
    }
  }
}

#[component]
fn SideToggle(label: &'static str, panel: SidePanel) -> Element {
  let mut state = use_context::<AppState>();
  let active = (state.side_panel)() == panel;
  rsx! {
    Button {
      variant: if active { ButtonVariant::Primary } else { ButtonVariant::Secondary },
      size: ButtonSize::Xs,
      class: if active {
        "bg-info text-info-foreground hover:bg-info/90".to_string()
      } else {
        String::new()
      },
      onclick: move |_| {
        let next = if (state.side_panel)() == panel {
          SidePanel::None
        } else {
          panel
        };
        state.side_panel.set(next);
      },
      "{label}"
    }
  }
}

async fn stop_audiolab_recording(state: &mut AppState) {
  let Some((_, buf)) = stop_mic_recording(state) else {
    return;
  };
  let project_path = state.project_path.read().clone();
  let out_path = recording_output_path(project_path.as_deref(), "audiolab");
  let save_path = out_path.clone();
  let path_str = out_path.to_string_lossy().to_string();

  let saved = tokio::task::spawn_blocking(move || {
    save_recorded_wav_atomic(&save_path, &buf)?;
    let info = build_display_info(&buf, &path_str);
    Ok::<_, String>((buf, info))
  })
  .await;

  match saved {
    Ok(Ok((buf, info))) => {
      let next_token = (state.active_track_token)().wrapping_add(1);
      state.active_track_token.set(next_token);
      state.playback_status.set(PlaybackState::Stopped);
      state.playhead_secs.set(0.0);
      state.playhead_duration_secs.set(info.duration);
      state.display_info.set(Some(info));
      state.audio_buffer.set(Some(Arc::new(buf)));
      state.status_msg.set(format!(
        "Recorded & loaded into Audio Lab ({})",
        out_path.display()
      ));
    }
    Ok(Err(e)) => state.status_msg.set(e),
    Err(e) => state
      .status_msg
      .set(format!("Audio Lab save task error: {e}")),
  }
}

async fn open_podcast_project(state: &mut AppState) {
  state
    .status_msg
    .set("Select a Studio project or CLI podcast script (.json)...".to_string());
  let file_handle = rfd::AsyncFileDialog::new()
    .add_filter("Podcast Project / Script", &["json"])
    .pick_file()
    .await;
  let Some(handle) = file_handle else {
    state.status_msg.set("Open cancelled".to_string());
    return;
  };
  let path = handle.path().to_path_buf();
  let path_for_read = path.clone();

  let loaded = tokio::task::spawn_blocking(move || {
    let mtime = std::fs::metadata(&path_for_read)
      .and_then(|m| m.modified())
      .ok();
    let raw = std::fs::read_to_string(&path_for_read)
      .map_err(|e| format!("Failed to read '{}': {e}", path_for_read.display()))?;
    StudioProject::from_json(&raw).map(|proj| (proj, mtime))
  })
  .await;

  match loaded {
    Ok(Ok((proj, mtime))) => {
      let seg_count = proj.script.segments.len();
      state.project.set(proj);
      state.history.write().clear();
      state.project_path.set(Some(path.clone()));
      state.last_synced_mtime.set(mtime);
      state.segment_audio.set(HashMap::new());
      state.rendered_episode.set(None);
      state.episode_view.set(None);
      state.status_msg.set(format!(
        "Loaded {} ({seg_count} segments)",
        path
          .file_name()
          .map(|n| n.to_string_lossy().to_string())
          .unwrap_or_else(|| path.display().to_string())
      ));
    }
    Ok(Err(e)) => state.status_msg.set(format!("Load failed: {e}")),
    Err(e) => state.status_msg.set(format!("Task error: {e}")),
  }
}

pub async fn save_podcast_project(state: &mut AppState) {
  let json = match state.project.read().to_json_pretty() {
    Ok(j) => j,
    Err(e) => {
      state.status_msg.set(e);
      return;
    }
  };
  let default_name = state
    .project_path
    .read()
    .as_ref()
    .and_then(|p| p.file_name())
    .map(|n| n.to_string_lossy().to_string())
    .unwrap_or_else(|| "episode.phonon.json".to_string());

  let file_handle = rfd::AsyncFileDialog::new()
    .add_filter("Phonon Studio Project", &["json"])
    .set_file_name(default_name)
    .save_file()
    .await;
  let Some(handle) = file_handle else {
    return;
  };
  let path = handle.path().to_path_buf();
  let write_path = path.clone();

  let res =
    tokio::task::spawn_blocking(move || write_project_atomic_file(&write_path, &json)).await;
  match res {
    Ok(Ok(mtime)) => {
      state.project_path.set(Some(path.clone()));
      state.last_synced_mtime.set(Some(mtime));
      state.status_msg.set(format!(
        "Saved project to {} (MCP Live Sync ready)",
        path.display()
      ));
    }
    Ok(Err(e)) => state.status_msg.set(format!("Save failed: {e}")),
    Err(e) => state.status_msg.set(format!("Save task error: {e}")),
  }
}

async fn open_audio_file(state: &mut AppState) {
  state.status_msg.set("Opening file dialog...".to_string());
  let file_handle = rfd::AsyncFileDialog::new()
    .add_filter("Audio", &["wav", "mp3", "flac", "ogg", "aac", "m4a", "mp4"])
    .pick_file()
    .await;

  let Some(path) = file_handle else {
    state.status_msg.set("Cancelled".to_string());
    return;
  };

  load_audio(state, path.path().to_path_buf(), "File loaded").await;
}
