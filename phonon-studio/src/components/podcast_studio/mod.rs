#![allow(non_snake_case)]

pub mod actions;
mod episode_inspector;
mod left_rail;
mod script_desk;

use crate::components::header::save_podcast_project;
use crate::project::mcp_client_config_json;
use crate::state::AppState;
use crate::studio_ops::check_external_project_reload;
use actions::run_ai_draft;
use dioxus::prelude::*;
use dioxus_shadcn::{
  Button, ButtonSize, ButtonVariant, Dialog, DialogClose, DialogContent, DialogDescription,
  DialogOverlay, DialogTitle, Input, InputSize, Label, LayoutOrientation, NativeSelect,
  NativeSelectOption, ResizableHandle, ResizablePanel, ResizablePanelGroup, ResizablePanelState,
  SonnerVariant, Switch, SwitchSize, resizable_resize_pair,
};
use episode_inspector::RightEpisodeInspector;
use left_rail::LeftConfigRail;
use script_desk::CenterScriptDesk;
use std::time::Duration;

/// Palette dots assigned to speakers deterministically by sorted order.
pub(super) const SPEAKER_DOTS: &[&str] = &[
  "bg-[#E8A238]",
  "bg-[#38B2AC]",
  "bg-[#818CF8]",
  "bg-[#F472B6]",
  "bg-[#4ADE80]",
  "bg-[#FB923C]",
];

#[component]
pub fn PodcastStudioView() -> Element {
  let mut state = use_context::<AppState>();
  let mut left_pct = use_signal(|| 22.0_f64);
  let mut right_pct = use_signal(|| 24.0_f64);

  // MCP Live Sync watcher: hot-reloads atomic edits made by external agents via
  // `phonon-mcp` and snapshots the previous state onto Undo history.
  use_future(move || async move {
    loop {
      tokio::time::sleep(Duration::from_millis(500)).await;
      if !(state.mcp_live_sync)() {
        continue;
      }
      let path_opt = state.project_path.read().clone();
      let Some(path) = path_opt else {
        continue;
      };
      let last_mtime = (state.last_synced_mtime)();
      let checked =
        tokio::task::spawn_blocking(move || check_external_project_reload(&path, last_mtime)).await;
      if let Ok(Some((new_mtime, parse_res))) = checked {
        state.last_synced_mtime.set(Some(new_mtime));
        match parse_res {
          Ok(updated_proj) => {
            let segs = updated_proj.script.segments.len();
            state.mutate_project(|p| *p = updated_proj);
            state.notify(
              format!("Synced external MCP edit ({segs} segments — click Undo to revert)"),
              SonnerVariant::Info,
            );
          }
          Err(e) => {
            state.notify(
              format!("MCP sync reload skipped (invalid JSON): {e}"),
              SonnerVariant::Warning,
            );
          }
        }
      }
    }
  });

  let left_size = left_pct();
  let right_size = right_pct();
  let center_size = (100.0 - left_size - right_size).clamp(32.0, 68.0);

  rsx! {
    div {
      class: "flex-1 flex min-h-0 overflow-hidden bg-background",

      ResizablePanelGroup {
        orientation: LayoutOrientation::Horizontal,
        class: "flex-1 min-h-0",

        ResizablePanel {
          size: left_size,
          min_size: 16.0,
          max_size: 34.0,
          class: "min-h-0 bg-card",
          LeftConfigRail {}
        }

        ResizableHandle {
          orientation: LayoutOrientation::Horizontal,
          value: left_size,
          min: 16.0,
          max: 34.0,
          step: 2.0,
          "aria-label": "Resize left studio config rail",
          on_resize: move |delta: f64| {
            let cur_left = left_pct();
            let cur_center = (100.0 - cur_left - right_pct()).max(32.0);
            let (next_left, _) = resizable_resize_pair(
              ResizablePanelState::new(cur_left, 16.0, 34.0),
              ResizablePanelState::new(cur_center, 32.0, 68.0),
              delta,
            );
            left_pct.set(next_left.size);
          },
        }

        ResizablePanel {
          size: center_size,
          min_size: 32.0,
          max_size: 68.0,
          class: "min-h-0 flex flex-col",
          CenterScriptDesk {}
        }

        ResizableHandle {
          orientation: LayoutOrientation::Horizontal,
          value: center_size,
          min: 32.0,
          max: 68.0,
          step: 2.0,
          "aria-label": "Resize right episode inspector",
          on_resize: move |delta: f64| {
            let cur_right = right_pct();
            let cur_center = (100.0 - left_pct() - cur_right).max(32.0);
            let (_, next_right) = resizable_resize_pair(
              ResizablePanelState::new(cur_center, 32.0, 68.0),
              ResizablePanelState::new(cur_right, 18.0, 36.0),
              delta,
            );
            right_pct.set(next_right.size);
          },
        }

        ResizablePanel {
          size: right_size,
          min_size: 18.0,
          max_size: 36.0,
          class: "min-h-0 bg-card",
          RightEpisodeInspector {}
        }
      }

      AiDraftDialog {}
      McpBridgeDialog {}
    }
  }
}

#[component]
fn AiDraftDialog() -> Element {
  let mut state = use_context::<AppState>();
  let mut draft_topic = use_signal(String::new);
  let mut draft_segments = use_signal(|| 8usize);
  let mut draft_lang = use_signal(|| "zh-CN".to_string());

  let open = (state.show_ai_draft)();
  let is_drafting = (state.is_drafting)();
  let lang_val = draft_lang();

  rsx! {
    Dialog {
      open,
      on_open_change: move |next: bool| state.show_ai_draft.set(next),
      DialogOverlay {}
      DialogContent {
        class: "max-w-lg bg-card p-5 gap-4",
        DialogTitle { class: "text-sm", "AI Podcast Script Draft" }
        DialogDescription {
          class: "text-xs",
          "Generate a structured multi-speaker podcast script with chapters using your configured LLM provider."
        }
        DialogClose {
          class: "text-muted-foreground hover:text-foreground px-1.5 py-0.5 text-sm",
          "×"
        }

        div {
          class: "flex flex-col gap-3",
          div {
            class: "flex flex-col gap-1",
            Label { class: "text-[11px] font-normal text-muted-foreground", "Topic or outline" }
            Input {
              size: InputSize::Sm,
              class: "bg-background",
              placeholder: "e.g. Rust 音频引擎与本地 AI 播客制作实践",
              value: "{draft_topic}",
              on_value_change: move |val: String| draft_topic.set(val),
            }
          }

          div {
            class: "grid grid-cols-2 gap-3",
            div {
              class: "flex flex-col gap-1",
              Label { class: "text-[11px] font-normal text-muted-foreground", "Target Segments" }
              Input {
                size: InputSize::Sm,
                class: "bg-background font-mono",
                r#type: "number",
                min: "2",
                max: "40",
                value: "{draft_segments}",
                on_value_change: move |val: String| {
                  if let Ok(n) = val.parse::<usize>() {
                    draft_segments.set(n.clamp(2, 40));
                  }
                },
              }
            }
            div {
              class: "flex flex-col gap-1",
              Label { class: "text-[11px] font-normal text-muted-foreground", "Language" }
              NativeSelect {
                class: "h-8 px-2 py-1 text-xs bg-background",
                value: "{lang_val}",
                on_value_change: move |val: String| draft_lang.set(val),
                NativeSelectOption { value: "zh-CN", selected: lang_val == "zh-CN", "zh-CN" }
                NativeSelectOption { value: "en-US", selected: lang_val == "en-US", "en-US" }
                NativeSelectOption { value: "ja-JP", selected: lang_val == "ja-JP", "ja-JP" }
              }
            }
          }
        }

        div {
          class: "flex items-center justify-end gap-2 pt-1",
          Button {
            variant: ButtonVariant::Outline,
            size: ButtonSize::Xs,
            class: "bg-secondary",
            onclick: move |_| state.show_ai_draft.set(false),
            "Cancel"
          }
          Button {
            variant: ButtonVariant::Primary,
            size: ButtonSize::Xs,
            class: "font-semibold",
            loading: is_drafting,
            onclick: move |_| {
              let topic = draft_topic.read().clone();
              let count = (draft_segments)();
              let lang = draft_lang.read().clone();
              async move {
                run_ai_draft(&mut state, topic, count, lang).await;
              }
            },
            if is_drafting { "Drafting..." } else { "Generate Script" }
          }
        }
      }
    }
  }
}

#[component]
fn McpBridgeDialog() -> Element {
  let mut state = use_context::<AppState>();
  let open = (state.show_mcp_panel)();
  let mcp_live_sync = (state.mcp_live_sync)();
  let project_path = state.project_path.read().clone();
  let snippet = mcp_client_config_json(project_path.as_deref());
  let path_label = project_path
    .as_ref()
    .map(|p| p.display().to_string())
    .unwrap_or_else(|| "Not bound yet — save project to enable live co-editing".to_string());

  rsx! {
    Dialog {
      open,
      on_open_change: move |next: bool| state.show_mcp_panel.set(next),
      DialogOverlay {}
      DialogContent {
        class: "max-w-2xl bg-card p-5 gap-3.5",
        DialogTitle { class: "text-sm", "Local MCP Agent Bridge (phonon-mcp)" }
        DialogDescription {
          class: "text-xs font-mono truncate",
          "{path_label}"
        }
        DialogClose {
          class: "text-muted-foreground hover:text-foreground px-1.5 py-0.5 text-sm",
          "×"
        }

        div {
          class: "flex flex-wrap items-center justify-between gap-2",
          div {
            class: "flex items-center gap-2",
            Switch {
              id: "mcp-live-sync-switch",
              size: SwitchSize::Sm,
              checked: mcp_live_sync,
              on_checked_change: move |checked: bool| state.mcp_live_sync.set(checked),
            }
            Label {
              r#for: "mcp-live-sync-switch",
              class: "text-xs font-normal text-muted-foreground cursor-pointer",
              "Auto-reload MCP edits (500ms)"
            }
          }
          if project_path.is_none() {
            Button {
              variant: ButtonVariant::Primary,
              size: ButtonSize::Xs,
              class: "bg-info text-info-foreground hover:bg-info/90 font-semibold",
              onclick: move |_| async move {
                save_podcast_project(&mut state).await;
              },
              "Save & Bind File"
            }
          }
        }

        pre {
          class: "bg-background border border-border rounded-md p-3 text-[11px] font-mono text-muted-foreground overflow-x-auto select-all",
          "{snippet}"
        }

        p {
          class: "text-[11px] text-muted-foreground leading-relaxed",
          "Exposed tools: studio_get_project, studio_edit_segment, studio_add_segment, studio_delete_segment, studio_set_mastering, audio_enhance_voice, audio_splice, audio_eq, audio_compress. External edits push onto Undo history."
        }
      }
    }
  }
}

#[component]
pub(super) fn AssetPickerRow(
  label: &'static str,
  current: String,
  onpick: EventHandler<MouseEvent>,
  onclear: EventHandler<MouseEvent>,
) -> Element {
  let has_file = current != "None";
  rsx! {
    div {
      class: "flex items-center gap-1.5 text-[11px]",
      span { class: "text-muted-foreground w-11 shrink-0", "{label}" }
      span {
        class: "flex-1 min-w-0 truncate px-2 py-1 rounded bg-background border border-border text-foreground",
        title: "{current}",
        "{current}"
      }
      Button {
        variant: ButtonVariant::Secondary,
        size: ButtonSize::Xs,
        class: "h-6 px-2 text-[11px]",
        onclick: move |e| onpick.call(e),
        "Pick"
      }
      if has_file {
        Button {
          variant: ButtonVariant::Ghost,
          size: ButtonSize::Xs,
          class: "h-6 w-6 p-0 text-muted-foreground hover:text-destructive",
          title: "Clear {label}",
          onclick: move |e| onclear.call(e),
          "×"
        }
      }
    }
  }
}

pub(super) async fn pick_audio_path() -> Option<String> {
  rfd::AsyncFileDialog::new()
    .add_filter("Audio", &["wav", "mp3", "flac", "ogg", "aac", "m4a"])
    .pick_file()
    .await
    .map(|h| h.path().to_string_lossy().to_string())
}

pub(super) fn short_filename(path: &str) -> String {
  std::path::Path::new(path)
    .file_name()
    .map(|n| n.to_string_lossy().to_string())
    .unwrap_or_else(|| path.to_string())
}
