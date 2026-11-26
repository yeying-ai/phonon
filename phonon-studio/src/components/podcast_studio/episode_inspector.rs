#![allow(non_snake_case)]

use super::actions::run_export_episode;
use super::{AssetPickerRow, short_filename};
use crate::project::{LoudnessHealth, format_db, format_timecode};
use crate::state::{AppState, InspectorTab, play_buffer};
use crate::studio_ops::EpisodeExportFormat;
use dioxus::prelude::*;
use dioxus_shadcn::{
  Badge, BadgeSize, BadgeStyle, BadgeVariant, Button, ButtonSize, ButtonVariant, Card, Empty,
  EmptyDescription, EmptyHeader, EmptyTitle, Input, InputSize, Label, NativeSelect,
  NativeSelectOption, Separator, Tabs, TabsList, TabsTrigger, TabsVariant,
};
use std::sync::Arc;

#[component]
pub(super) fn RightEpisodeInspector() -> Element {
  let mut state = use_context::<AppState>();
  let tab = (state.inspector_tab)();
  let tab_key = match tab {
    InspectorTab::Master => "master",
    InspectorTab::Chapters => "chapters",
    InspectorTab::Subtitles => "subtitles",
  };
  let view = (state.episode_view)();
  let project = state.project.read().clone();
  let bitrate = (state.episode_mp3_bitrate)();
  let chapter_count = view.as_ref().map(|v| v.chapters.len()).unwrap_or(0);

  rsx! {
    aside {
      class: "h-full bg-card flex flex-col overflow-hidden",

      // Inspector tab strip (dioxus-shadcn Underline Tabs)
      div {
        class: "border-b border-border bg-card/80 px-2 pt-1.5",
        Tabs {
          variant: TabsVariant::Underline,
          value: tab_key.to_string(),
          on_value_change: move |next: String| {
            match next.as_str() {
              "chapters" => state.inspector_tab.set(InspectorTab::Chapters),
              "subtitles" => state.inspector_tab.set(InspectorTab::Subtitles),
              _ => state.inspector_tab.set(InspectorTab::Master),
            }
          },
          TabsList {
            class: "w-full gap-1 border-b-0",
            "aria-label": "Episode inspector sections",
            TabsTrigger { value: "master", class: "flex-1 text-[11px] py-1.5", "Master & Export" }
            TabsTrigger { value: "chapters", class: "flex-1 text-[11px] py-1.5", "Chapters ({chapter_count})" }
            TabsTrigger { value: "subtitles", class: "flex-1 text-[11px] py-1.5", "Subtitles" }
          }
        }
      }

      // Inspector body
      div {
        class: "flex-1 overflow-y-auto p-3.5 flex flex-col gap-4",

        match tab {
          InspectorTab::Master => rsx! {
            if let Some(ref ep_view) = view {
              {
                let health = ep_view.loudness_health(project.script.config.target_lufs);
                let (health_label, health_variant) = match health {
                  LoudnessHealth::BroadcastReady => ("Broadcast Ready", BadgeVariant::Success),
                  LoudnessHealth::OffTarget => ("Off Loudness Target", BadgeVariant::Warning),
                  LoudnessHealth::ClippingRisk => ("True-Peak Risk", BadgeVariant::Destructive),
                };
                rsx! {
                  // Waveform card + Play episode button
                  Card {
                    class: "bg-background p-2.5 flex flex-col gap-2 shadow-none",
                    div {
                      class: "flex items-center justify-between gap-2",
                      span {
                        class: "text-xs font-mono tabular-nums text-muted-foreground",
                        "{format_timecode(ep_view.duration_secs)} · {ep_view.sample_rate} Hz · {ep_view.channels}ch"
                      }
                      Button {
                        variant: ButtonVariant::Primary,
                        size: ButtonSize::Xs,
                        class: "h-6 px-2.5 text-xs font-semibold",
                        onclick: move |_| async move {
                          let ep = state.rendered_episode.read().clone();
                          if let Some(ep) = ep {
                            play_buffer(
                              &mut state,
                              Arc::new(ep.audio.clone()),
                              "Assembled Episode".to_string(),
                              false,
                              None,
                            ).await;
                          }
                        },
                        "▶ Play Episode"
                      }
                    }
                    svg {
                      width: "100%",
                      height: "84",
                      view_box: "0 0 1000 140",
                      preserve_aspect_ratio: "none",
                      path {
                        d: "{ep_view.waveform_path}",
                        class: "stroke-info fill-info/80",
                        stroke_width: "1",
                        vector_effect: "non-scaling-stroke"
                      }
                    }
                  }

                  // BS.1770-4 Telemetry
                  div {
                    class: "flex flex-col gap-2",
                    div {
                      class: "flex items-center justify-between",
                      span { class: "text-xs font-semibold text-foreground", "Loudness Telemetry" }
                      Badge {
                        variant: health_variant,
                        style: BadgeStyle::Soft,
                        size: BadgeSize::Xs,
                        class: "py-0.5",
                        "{health_label}"
                      }
                    }
                    div {
                      class: "grid grid-cols-2 gap-2",
                      MeterCell { label: "Integrated", value: format_db(ep_view.integrated_lufs, "LUFS") }
                      MeterCell { label: "True Peak", value: format_db(ep_view.true_peak_dbtp, "dBTP") }
                      MeterCell { label: "Short-Term Max", value: format_db(ep_view.short_term_max_lufs, "LUFS") }
                      MeterCell { label: "Loudness Range", value: format!("{:.1} LU", ep_view.range_lu) }
                    }
                  }
                }
              }
            } else {
              Empty {
                class: "min-h-28 p-4 gap-2 bg-background",
                EmptyHeader {
                  class: "gap-1",
                  EmptyTitle { class: "text-xs", "Episode not assembled yet" }
                  EmptyDescription {
                    class: "text-[11px] leading-relaxed",
                    "Preview individual segments or click Assemble Episode to inspect BS.1770 loudness, chapters, subtitles, and export."
                  }
                }
              }
            }

            Separator { decorative: true }

            // Metadata Tags
            div {
              class: "flex flex-col gap-2",
              span { class: "text-xs font-semibold text-foreground", "ID3 / Episode Metadata" }
              TagInput {
                label: "Title",
                value: project.tags.title.clone(),
                oninput: move |v: String| state.project.write().tags.title = v,
              }
              TagInput {
                label: "Artist / Host",
                value: project.tags.artist.clone(),
                oninput: move |v: String| state.project.write().tags.artist = v,
              }
              TagInput {
                label: "Show / Album",
                value: project.tags.album.clone(),
                oninput: move |v: String| state.project.write().tags.album = v,
              }
              div {
                class: "grid grid-cols-2 gap-2",
                TagInput {
                  label: "Year",
                  value: project.tags.year.clone(),
                  oninput: move |v: String| state.project.write().tags.year = v,
                }
                div {
                  class: "flex flex-col gap-1",
                  Label { class: "text-[11px] font-normal text-muted-foreground", "MP3 Bitrate" }
                  NativeSelect {
                    class: "h-7 px-2 py-0.5 text-xs font-mono bg-secondary",
                    value: "{bitrate}",
                    on_value_change: move |val: String| {
                      if let Ok(b) = val.parse::<u32>() {
                        state.episode_mp3_bitrate.set(b);
                      }
                    },
                    NativeSelectOption { value: "128", selected: bitrate == 128, "128 kbps" }
                    NativeSelectOption { value: "192", selected: bitrate == 192, "192 kbps" }
                    NativeSelectOption { value: "256", selected: bitrate == 256, "256 kbps" }
                    NativeSelectOption { value: "320", selected: bitrate == 320, "320 kbps" }
                  }
                }
              }
              AssetPickerRow {
                label: "Cover",
                current: project.tags.cover_path.as_deref().map(short_filename).unwrap_or_else(|| "None".to_string()),
                onpick: move |_| async move {
                  let handle = rfd::AsyncFileDialog::new()
                    .add_filter("Cover Image", &["jpg", "jpeg", "png"])
                    .pick_file()
                    .await;
                  if let Some(h) = handle {
                    state.project.write().tags.cover_path = Some(h.path().to_string_lossy().to_string());
                  }
                },
                onclear: move |_| {
                  state.project.write().tags.cover_path = None;
                },
              }
            }

            // Export Actions
            div {
              class: "flex flex-col gap-1.5 pt-1",
              Button {
                variant: ButtonVariant::Primary,
                size: ButtonSize::Sm,
                class: "w-full text-xs font-semibold",
                disabled: view.is_none(),
                onclick: move |_| async move {
                  run_export_episode(&mut state, EpisodeExportFormat::Mp3, "MP3", "mp3").await;
                },
                "Export MP3 (ID3 + Chapters)"
              }
              div {
                class: "grid grid-cols-3 gap-1.5",
                Button {
                  variant: ButtonVariant::Outline,
                  size: ButtonSize::Xs,
                  class: "bg-secondary hover:border-muted-foreground",
                  disabled: view.is_none(),
                  onclick: move |_| async move {
                    run_export_episode(&mut state, EpisodeExportFormat::Wav, "WAV", "wav").await;
                  },
                  "WAV"
                }
                Button {
                  variant: ButtonVariant::Outline,
                  size: ButtonSize::Xs,
                  class: "bg-secondary hover:border-muted-foreground",
                  disabled: view.is_none(),
                  onclick: move |_| async move {
                    run_export_episode(&mut state, EpisodeExportFormat::Flac, "FLAC", "flac").await;
                  },
                  "FLAC"
                }
                Button {
                  variant: ButtonVariant::Outline,
                  size: ButtonSize::Xs,
                  class: "bg-secondary hover:border-muted-foreground",
                  disabled: view.is_none(),
                  onclick: move |_| async move {
                    run_export_episode(&mut state, EpisodeExportFormat::Ogg, "OGG", "ogg").await;
                  },
                  "OGG"
                }
              }
              div {
                class: "grid grid-cols-2 gap-1.5",
                Button {
                  variant: ButtonVariant::Outline,
                  size: ButtonSize::Xs,
                  class: "bg-secondary text-muted-foreground hover:text-foreground",
                  disabled: view.is_none(),
                  onclick: move |_| async move {
                    run_export_episode(&mut state, EpisodeExportFormat::Srt, "SRT Subtitles", "srt").await;
                  },
                  "Export .srt"
                }
                Button {
                  variant: ButtonVariant::Outline,
                  size: ButtonSize::Xs,
                  class: "bg-secondary text-muted-foreground hover:text-foreground",
                  disabled: view.is_none(),
                  onclick: move |_| async move {
                    run_export_episode(&mut state, EpisodeExportFormat::ChaptersTxt, "Chapters TXT", "txt").await;
                  },
                  "Chapters .txt"
                }
              }
            }
          },

          InspectorTab::Chapters => rsx! {
            if let Some(ref ep_view) = view {
              if ep_view.chapters.is_empty() {
                Empty {
                  class: "min-h-24 p-4 bg-background",
                  EmptyDescription { class: "text-xs", "No chapters generated." }
                }
              } else {
                div {
                  class: "flex flex-col gap-1.5",
                  p { class: "text-[11px] text-muted-foreground mb-1", "Click any chapter to seek and play from its start marker." }
                  for (i, ch) in ep_view.chapters.iter().enumerate() {
                    {
                      let start = ch.start_secs;
                      let title = ch.title.clone();
                      rsx! {
                        Button {
                          key: "ch-{i}",
                          variant: ButtonVariant::Outline,
                          size: ButtonSize::Sm,
                          class: "w-full justify-between bg-background px-2.5 py-2 h-auto text-left hover:border-primary",
                          onclick: move |_| {
                            let label = format!("Chapter: {title}");
                            async move {
                              let ep = state.rendered_episode.read().clone();
                              if let Some(ep) = ep {
                                play_buffer(
                                  &mut state,
                                  Arc::new(ep.audio.clone()),
                                  label,
                                  false,
                                  Some(start),
                                ).await;
                              }
                            }
                          },
                          span { class: "text-xs text-foreground truncate", "{ch.title}" }
                          span { class: "text-[11px] font-mono tabular-nums text-info shrink-0", "{format_timecode(ch.start_secs)}" }
                        }
                      }
                    }
                  }
                }
              }
            } else {
              Empty {
                class: "min-h-24 p-4 bg-background",
                EmptyDescription { class: "text-xs", "Assemble the episode first to inspect chapter timestamps." }
              }
            }
          },

          InspectorTab::Subtitles => rsx! {
            if let Some(ref ep_view) = view {
              div {
                class: "flex flex-col gap-2.5 h-full",
                Button {
                  variant: ButtonVariant::Outline,
                  size: ButtonSize::Xs,
                  class: "w-full bg-secondary hover:border-info",
                  onclick: move |_| async move {
                    run_export_episode(&mut state, EpisodeExportFormat::Srt, "SRT Subtitles", "srt").await;
                  },
                  "Save Subtitles (.srt)"
                }
                pre {
                  class: "flex-1 bg-background border border-border rounded-md p-2.5 text-[11px] font-mono text-muted-foreground whitespace-pre-wrap overflow-y-auto leading-relaxed",
                  "{ep_view.srt}"
                }
              }
            } else {
              Empty {
                class: "min-h-24 p-4 bg-background",
                EmptyDescription { class: "text-xs", "Assemble the episode first to generate synchronized SRT subtitles." }
              }
            }
          },
        }
      }
    }
  }
}

#[component]
fn MeterCell(label: &'static str, value: String) -> Element {
  rsx! {
    Card {
      class: "bg-background px-2.5 py-2 flex flex-col gap-0.5 shadow-none",
      span { class: "text-[11px] text-muted-foreground", "{label}" }
      span { class: "text-xs font-mono tabular-nums font-semibold text-foreground", "{value}" }
    }
  }
}

#[component]
fn TagInput(label: &'static str, value: String, oninput: EventHandler<String>) -> Element {
  rsx! {
    div {
      class: "flex flex-col gap-1",
      Label { class: "text-[11px] font-normal text-muted-foreground", "{label}" }
      Input {
        size: InputSize::Sm,
        class: "h-7 px-2 text-xs bg-secondary",
        value: "{value}",
        on_value_change: move |val: String| oninput.call(val),
      }
    }
  }
}
