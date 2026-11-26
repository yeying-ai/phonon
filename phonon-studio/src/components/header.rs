#![allow(non_snake_case)]

use crate::project::StudioProject;
use crate::state::{AppState, SidePanel, WorkspaceTab, load_audio};
use crate::studio_ops::write_project_atomic_file;
use dioxus::prelude::*;
use std::collections::HashMap;

#[component]
pub fn Header() -> Element {
  let mut state = use_context::<AppState>();
  let active_tab = (state.active_tab)();
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

  rsx! {
    header {
      class: "h-12 shrink-0 bg-[#1C1F24] px-4 flex items-center justify-between border-b border-[#2D333C] select-none",

      // Left: Brand + Workspace segmented switcher
      div {
        class: "flex items-center gap-4",
        div {
          class: "flex items-center gap-2 font-semibold text-sm tracking-tight text-[#ECEFF4]",
          span { class: "inline-block w-2 h-2 rounded-full bg-[#E8A238]" }
          "Phonon Studio"
        }

        div {
          class: "flex items-center bg-[#141619] p-0.5 rounded-md border border-[#2D333C]",
          button {
            class: if active_tab == WorkspaceTab::PodcastStudio {
              "px-3 py-1 rounded text-xs font-medium bg-[#252A31] text-[#ECEFF4] shadow-xs"
            } else {
              "px-3 py-1 rounded text-xs font-medium text-[#9AA4B2] hover:text-[#ECEFF4]"
            },
            onclick: move |_| state.active_tab.set(WorkspaceTab::PodcastStudio),
            "Podcast Studio"
          }
          button {
            class: if active_tab == WorkspaceTab::AudioLab {
              "px-3 py-1 rounded text-xs font-medium bg-[#252A31] text-[#ECEFF4] shadow-xs"
            } else {
              "px-3 py-1 rounded text-xs font-medium text-[#9AA4B2] hover:text-[#ECEFF4]"
            },
            onclick: move |_| state.active_tab.set(WorkspaceTab::AudioLab),
            "Audio Lab"
          }
        }
      }

      // Right: Context-aware actions
      if active_tab == WorkspaceTab::PodcastStudio {
        div {
          class: "flex items-center gap-2",

          // Provider readiness pill
          div {
            class: "hidden md:flex items-center gap-1.5 px-2.5 py-1 rounded bg-[#141619] border border-[#2D333C] text-xs text-[#9AA4B2]",
            span {
              class: if provider_ready {
                "inline-block w-1.5 h-1.5 rounded-full bg-[#38B2AC]"
              } else {
                "inline-block w-1.5 h-1.5 rounded-full bg-[#E05252]"
              }
            }
            span { class: "font-mono tabular-nums", "TTS: {provider_label}" }
          }

          button {
            class: if show_ai_draft {
              "px-2.5 py-1.5 rounded text-xs font-medium bg-[#E8A238] text-[#141619] hover:opacity-90 transition"
            } else {
              "px-2.5 py-1.5 rounded text-xs font-medium bg-[#252A31] text-[#ECEFF4] border border-[#2D333C] hover:border-[#E8A238] transition"
            },
            onclick: move |_| state.show_ai_draft.set(!show_ai_draft),
            "AI Draft"
          }

          button {
            class: if show_mcp_panel {
              "px-2.5 py-1.5 rounded text-xs font-medium bg-[#38B2AC] text-[#141619] flex items-center gap-1.5 transition"
            } else {
              "px-2.5 py-1.5 rounded text-xs font-medium bg-[#252A31] text-[#ECEFF4] border border-[#2D333C] hover:border-[#38B2AC] flex items-center gap-1.5 transition"
            },
            title: "Configure MCP Agent live co-editing",
            onclick: move |_| state.show_mcp_panel.set(!show_mcp_panel),
            span {
              class: if mcp_synced {
                "inline-block w-1.5 h-1.5 rounded-full bg-[#38B2AC]"
              } else {
                "inline-block w-1.5 h-1.5 rounded-full bg-[#647080]"
              }
            }
            "MCP Sync"
          }

          div { class: "h-4 w-px bg-[#2D333C] mx-0.5" }

          button {
            class: if can_undo {
              "px-2 py-1.5 rounded text-xs bg-[#252A31] text-[#ECEFF4] hover:bg-[#2D333C]"
            } else {
              "px-2 py-1.5 rounded text-xs bg-[#1C1F24] text-[#647080] cursor-not-allowed"
            },
            disabled: !can_undo,
            title: "Undo last edit",
            onclick: move |_| {
              state.undo_project();
            },
            "Undo"
          }
          button {
            class: if can_redo {
              "px-2 py-1.5 rounded text-xs bg-[#252A31] text-[#ECEFF4] hover:bg-[#2D333C]"
            } else {
              "px-2 py-1.5 rounded text-xs bg-[#1C1F24] text-[#647080] cursor-not-allowed"
            },
            disabled: !can_redo,
            title: "Redo edit",
            onclick: move |_| {
              state.redo_project();
            },
            "Redo"
          }

          div { class: "h-4 w-px bg-[#2D333C] mx-0.5" }

          button {
            class: "px-2.5 py-1.5 rounded text-xs font-medium bg-[#252A31] text-[#ECEFF4] hover:bg-[#2D333C]",
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
          button {
            class: "px-2.5 py-1.5 rounded text-xs font-medium bg-[#252A31] text-[#ECEFF4] hover:bg-[#2D333C]",
            onclick: move |_| async move {
              open_podcast_project(&mut state).await;
            },
            "Open Script"
          }
          button {
            class: "px-2.5 py-1.5 rounded text-xs font-medium bg-[#252A31] text-[#ECEFF4] hover:bg-[#2D333C]",
            onclick: move |_| async move {
              save_podcast_project(&mut state).await;
            },
            "Save"
          }
        }
      } else {
        div {
          class: "flex items-center gap-2",
          SideToggle { label: "Effects", panel: SidePanel::Effects }
          SideToggle { label: "EQ", panel: SidePanel::Eq }
          SideToggle { label: "Plugins", panel: SidePanel::Plugins }
          SideToggle { label: "Export", panel: SidePanel::Export }

          div { class: "h-4 w-px bg-[#2D333C] mx-0.5" }

          button {
            class: "px-3 py-1.5 rounded text-xs font-medium bg-[#E8A238] text-[#141619] hover:opacity-90 transition",
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

#[component]
fn SideToggle(label: &'static str, panel: SidePanel) -> Element {
  let mut state = use_context::<AppState>();
  let active = (state.side_panel)() == panel;
  rsx! {
    button {
      class: if active {
        "px-2.5 py-1.5 rounded text-xs font-medium bg-[#38B2AC] text-[#141619]"
      } else {
        "px-2.5 py-1.5 rounded text-xs font-medium bg-[#252A31] text-[#ECEFF4] hover:bg-[#2D333C]"
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
