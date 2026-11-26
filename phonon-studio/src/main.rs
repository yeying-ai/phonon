#![allow(non_snake_case)]

mod components;
mod project;
mod state;
mod studio_ops;

use components::{
  audio_lab::AudioLabView, header::Header, podcast_studio::PodcastStudioView, transport::Transport,
};
use dioxus::html::HasFileData;
use dioxus::prelude::*;
use dioxus_logger::tracing;
use project::StudioProject;
use state::{AppState, WorkspaceTab, load_audio};
use std::collections::HashMap;

/// Tailwind CSS asset for desktop
const TAILWIND_CSS: Asset = asset!("/assets/tailwind.css");

fn main() {
  if let Err(e) = dioxus_logger::init(tracing::Level::INFO) {
    eprintln!("Failed to init logger: {e}");
  }
  launch(App);
}

fn App() -> Element {
  let state = use_hook(AppState::init);
  use_context_provider(|| state);

  rsx! {
    document::Stylesheet { href: TAILWIND_CSS }
    div {
      class: "flex flex-col h-dvh bg-[#141619] text-[#ECEFF4] overflow-hidden",

      // Global drag-and-drop: supports both .json scripts and audio files
      ondragover: move |evt| { evt.prevent_default(); },
      ondrop: move |evt| {
        evt.prevent_default();
        let mut s = state;
        async move {
          handle_drop(&mut s, evt).await;
        }
      },

      Header {}

      match (state.active_tab)() {
        WorkspaceTab::PodcastStudio => rsx! { PodcastStudioView {} },
        WorkspaceTab::AudioLab => rsx! { AudioLabView {} },
      }

      Transport {}
    }
  }
}

/// Handle drag-and-drop for both podcast script JSON files and raw audio files.
async fn handle_drop(state: &mut AppState, evt: Event<DragData>) {
  let file_list = evt.data().files();
  let Some(first_file) = file_list.first() else {
    return;
  };
  let first_name = first_file.name();
  let path_lower = first_name.to_lowercase();
  let path_buf = std::path::PathBuf::from(&first_name);

  if path_lower.ends_with(".json") {
    let read_path = path_buf.clone();
    let loaded = tokio::task::spawn_blocking(move || {
      let raw = std::fs::read_to_string(&read_path)
        .map_err(|e| format!("Failed to read '{}': {e}", read_path.display()))?;
      StudioProject::from_json(&raw)
    })
    .await;
    match loaded {
      Ok(Ok(proj)) => {
        let count = proj.script.segments.len();
        state.project.set(proj);
        state.history.write().clear();
        state.project_path.set(Some(path_buf));
        state.segment_audio.set(HashMap::new());
        state.rendered_episode.set(None);
        state.episode_view.set(None);
        state.active_tab.set(WorkspaceTab::PodcastStudio);
        state.status_msg.set(format!(
          "Imported script ({count} segments) via drag & drop"
        ));
      }
      Ok(Err(e)) => state.status_msg.set(format!("Invalid script JSON: {e}")),
      Err(e) => state.status_msg.set(format!("Task error: {e}")),
    }
    return;
  }

  let supported = ["wav", "mp3", "flac", "ogg", "aac", "m4a", "mp4"];
  if !supported.iter().any(|ext| path_lower.ends_with(ext)) {
    state
      .status_msg
      .set("Unsupported file format (drop .json script or audio file)".to_string());
    return;
  }

  state.active_tab.set(WorkspaceTab::AudioLab);
  load_audio(state, path_buf, "File loaded (drag & drop)").await;
}
