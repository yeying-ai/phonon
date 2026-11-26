#![allow(non_snake_case)]

use super::actions::{
  run_align_recording, run_assemble_episode, run_attach_recording, run_preview_segment,
  run_stop_segment_recording, shift_cache_after_insert, shift_cache_after_remove,
  swap_cached_entries,
};
use super::{SPEAKER_DOTS, short_filename};
use crate::project::{SegmentMediaOverride, format_timecode};
use crate::state::{AppState, RecordingTarget, start_mic_recording};
use dioxus::prelude::*;
use dioxus_shadcn::{
  Badge, BadgeSize, BadgeStyle, BadgeVariant, Button, ButtonSize, ButtonVariant, Card, Input,
  InputSize, InputVariant, NativeSelect, NativeSelectOption, Textarea, TextareaSize,
};

#[component]
pub(super) fn CenterScriptDesk() -> Element {
  let mut state = use_context::<AppState>();

  let project = state.project.read().clone();
  let speakers = project.sorted_speakers();
  let seg_audio_map = state.segment_audio.read().clone();
  let busy_set = state.synthesizing_segments.read().clone();
  let aligning_set = state.aligning_segments.read().clone();
  let is_assembling = (state.is_assembling)();
  let rec_target = (state.recording_target)();
  let rec_peak = (state.recording_peak)();
  let rec_secs = (state.recording_secs)();

  let total_segments = project.script.segments.len();
  let fresh_count = (0..total_segments)
    .filter(|&i| {
      project
        .segment_fingerprint(i)
        .zip(seg_audio_map.get(&i))
        .is_some_and(|(fp, entry)| entry.fingerprint == fp)
    })
    .count();

  rsx! {
    main {
      class: "flex-1 flex flex-col min-h-0 min-w-0 overflow-hidden",

      // Episode title & assembly bar
      div {
        class: "px-5 py-3 border-b border-border bg-card/60 flex flex-wrap items-center justify-between gap-3",

        div {
          class: "flex-1 min-w-[220px] flex flex-col gap-1",
          Input {
            variant: InputVariant::Ghost,
            size: InputSize::Sm,
            class: "text-base font-semibold px-1.5 -ml-1.5 h-8",
            placeholder: "Episode title",
            value: "{project.script.title}",
            on_value_change: move |v: String| {
              let mut p = state.project.write();
              p.script.title = v.clone();
              if p.tags.title.is_empty() {
                p.tags.title = v;
              }
            },
          }
          div {
            class: "flex items-center gap-2 text-[11px] font-mono tabular-nums text-muted-foreground",
            Badge {
              variant: BadgeVariant::Secondary,
              style: BadgeStyle::Soft,
              size: BadgeSize::Xs,
              "{total_segments} segments"
            }
            Badge {
              variant: BadgeVariant::Success,
              style: BadgeStyle::Soft,
              size: BadgeSize::Xs,
              "{fresh_count}/{total_segments} cached"
            }
          }
        }

        div {
          class: "flex items-center gap-2",
          Button {
            variant: ButtonVariant::Outline,
            size: ButtonSize::Xs,
            class: "bg-secondary hover:border-muted-foreground",
            onclick: move |_| {
              state.mutate_project(|p| {
                let last = p.script.segments.len().checked_sub(1);
                p.add_segment_after(last, None);
              });
            },
            "+ Add Segment"
          }
          Button {
            variant: ButtonVariant::Primary,
            size: ButtonSize::Xs,
            class: "px-3.5 font-semibold",
            loading: is_assembling,
            onclick: move |_| async move {
              run_assemble_episode(&mut state).await;
            },
            if is_assembling {
              "Assembling..."
            } else {
              "Assemble Episode"
            }
          }
        }
      }

      // Segment timeline list
      div {
        class: "flex-1 overflow-y-auto px-5 py-4 flex flex-col gap-3",
        for (idx, seg) in project.script.segments.iter().enumerate() {
          {
            let speaker_idx = speakers
              .iter()
              .position(|s| s == &seg.speaker)
              .unwrap_or(0);
            let dot = SPEAKER_DOTS[speaker_idx % SPEAKER_DOTS.len()];
            let fp = project.segment_fingerprint(idx);
            let cached_entry = seg_audio_map.get(&idx);
            let is_busy = busy_set.contains(&idx);
            let (badge_text, badge_variant) = if is_busy {
              ("Synthesizing...".to_string(), BadgeVariant::Warning)
            } else if let (Some(fp), Some(entry)) = (fp.as_deref(), cached_entry) {
              if entry.fingerprint == fp {
                (format!("Cached {:.1}s", entry.duration_secs), BadgeVariant::Success)
              } else {
                ("Edited · stale".to_string(), BadgeVariant::Warning)
              }
            } else {
              ("Not synthesized".to_string(), BadgeVariant::Secondary)
            };

            let chapter_val = seg.chapter.clone().unwrap_or_default();
            let lang_val = seg.language.clone().unwrap_or_default();
            let text_val = seg.text.clone();
            let num_label = format!("{:02}", idx + 1);
            let can_remove = total_segments > 1;
            let override_opt = project.segment_overrides.get(&idx).cloned();
            let recorded_path = override_opt
              .as_ref()
              .and_then(|o| o.recorded_path.clone());
            let enhance_on = override_opt.as_ref().is_some_and(|o| o.enhance_voice);
            let cut_spans = override_opt
              .as_ref()
              .map(|o| o.spans.clone())
              .unwrap_or_default();
            let active_fillers = cut_spans
              .iter()
              .filter(|s| s.is_filler && !s.deleted)
              .count();
            let deleted_spans = cut_spans.iter().filter(|s| s.deleted).count();
            let is_aligning = aligning_set.contains(&idx);

            rsx! {
              Card {
                key: "seg-{idx}",
                class: "p-3 flex flex-col gap-2.5 shadow-none",

                // Segment header controls
                div {
                  class: "flex flex-wrap items-center justify-between gap-2",

                  div {
                    class: "flex items-center gap-2 flex-wrap",
                    span { class: "text-xs font-mono tabular-nums text-muted-foreground w-5", "{num_label}" }
                    span { class: "inline-block w-2 h-2 rounded-full {dot}" }

                    NativeSelect {
                      class: "w-auto h-7 px-2 py-0.5 text-xs font-medium bg-secondary",
                      value: "{seg.speaker}",
                      on_value_change: move |sp: String| {
                        state.mutate_project(|p| {
                          if let Some(s) = p.script.segments.get_mut(idx) {
                            s.speaker = sp;
                          }
                        });
                      },
                      for sp in speakers.iter() {
                        NativeSelectOption {
                          value: "{sp}",
                          selected: seg.speaker == *sp,
                          "{sp}"
                        }
                      }
                    }

                    Input {
                      size: InputSize::Sm,
                      class: "w-36 h-7 px-2 text-[11px] bg-background",
                      placeholder: "Chapter title...",
                      value: "{chapter_val}",
                      on_value_change: move |v: String| {
                        if let Some(s) = state.project.write().script.segments.get_mut(idx) {
                          s.chapter = if v.trim().is_empty() { None } else { Some(v) };
                        }
                      },
                    }

                    Input {
                      size: InputSize::Sm,
                      class: "w-16 h-7 px-1.5 text-[11px] font-mono bg-background",
                      placeholder: "lang",
                      title: "Optional language override (e.g. zh-CN, en-US)",
                      value: "{lang_val}",
                      on_value_change: move |v: String| {
                        if let Some(s) = state.project.write().script.segments.get_mut(idx) {
                          s.language = if v.trim().is_empty() { None } else { Some(v) };
                        }
                      },
                    }

                    Badge {
                      variant: badge_variant,
                      style: BadgeStyle::Soft,
                      size: BadgeSize::Xs,
                      class: "font-mono tabular-nums py-0.5",
                      "{badge_text}"
                    }
                  }

                  // Action buttons
                  div {
                    class: "flex items-center gap-1",
                    Button {
                      variant: ButtonVariant::Outline,
                      size: ButtonSize::Xs,
                      class: "h-6 px-2 text-xs bg-secondary hover:border-primary",
                      disabled: is_busy,
                      title: "Preview segment (uses cache when fresh)",
                      onclick: move |_| async move {
                        run_preview_segment(&mut state, idx, false).await;
                      },
                      "▶ Preview"
                    }
                    Button {
                      variant: ButtonVariant::Outline,
                      size: ButtonSize::Xs,
                      class: "h-6 px-2 text-xs bg-secondary text-muted-foreground hover:text-foreground",
                      disabled: is_busy,
                      title: "Force regenerate this segment (bypass cache)",
                      onclick: move |_| async move {
                        run_preview_segment(&mut state, idx, true).await;
                      },
                      "↻"
                    }
                    Button {
                      variant: ButtonVariant::Secondary,
                      size: ButtonSize::Xs,
                      class: "h-6 px-1.5 text-xs text-muted-foreground hover:text-foreground",
                      disabled: idx == 0,
                      title: "Move up",
                      onclick: move |_| {
                        if let Some(target) = state.mutate_project(|p| p.move_segment(idx, -1)) {
                          swap_cached_entries(&mut state, idx, target);
                        }
                      },
                      "↑"
                    }
                    Button {
                      variant: ButtonVariant::Secondary,
                      size: ButtonSize::Xs,
                      class: "h-6 px-1.5 text-xs text-muted-foreground hover:text-foreground",
                      disabled: idx + 1 >= total_segments,
                      title: "Move down",
                      onclick: move |_| {
                        if let Some(target) = state.mutate_project(|p| p.move_segment(idx, 1)) {
                          swap_cached_entries(&mut state, idx, target);
                        }
                      },
                      "↓"
                    }
                    Button {
                      variant: ButtonVariant::Secondary,
                      size: ButtonSize::Xs,
                      class: "h-6 px-1.5 text-xs text-muted-foreground hover:text-foreground",
                      title: "Duplicate segment",
                      onclick: move |_| {
                        if state.mutate_project(|p| p.duplicate_segment(idx)).is_some() {
                          shift_cache_after_insert(&mut state, idx + 1);
                        }
                      },
                      "Dup"
                    }
                    Button {
                      variant: ButtonVariant::Secondary,
                      size: ButtonSize::Xs,
                      class: "h-6 px-1.5 text-xs text-muted-foreground hover:text-foreground",
                      title: "Insert segment below",
                      onclick: move |_| {
                        state.mutate_project(|p| {
                          p.add_segment_after(Some(idx), None);
                        });
                        shift_cache_after_insert(&mut state, idx + 1);
                      },
                      "+"
                    }
                    Button {
                      variant: ButtonVariant::Ghost,
                      size: ButtonSize::Xs,
                      class: "h-6 px-1.5 text-xs text-muted-foreground hover:text-destructive",
                      disabled: !can_remove,
                      title: "Delete segment",
                      onclick: move |_| {
                        if state.mutate_project(|p| p.remove_segment(idx)) {
                          shift_cache_after_remove(&mut state, idx);
                        }
                      },
                      "×"
                    }
                  }
                }

                // Dialogue text editor
                Textarea {
                  size: TextareaSize::Sm,
                  class: "bg-background text-sm leading-relaxed resize-y",
                  rows: "3",
                  placeholder: "Enter dialogue for this segment...",
                  value: "{text_val}",
                  on_value_change: move |v: String| {
                    if let Some(s) = state.project.write().script.segments.get_mut(idx) {
                      s.text = v;
                    }
                  },
                }

                // Recorded Human Voice + In-App Mic + Local Enhance + Text-Cut Bar
                div {
                  class: "flex flex-wrap items-center justify-between gap-2 pt-0.5 text-[11px]",
                  div {
                    class: "flex items-center gap-1.5 flex-wrap",
                    if rec_target == Some(RecordingTarget::Segment(idx)) {
                      {
                        let lang_for_stop = seg.language.clone();
                        let vu_pct = (rec_peak.clamp(0.0, 1.0) * 100.0).round() as u32;
                        let rec_tc = format_timecode(rec_secs);
                        rsx! {
                          div {
                            class: "flex items-center gap-2 px-2 py-0.5 rounded-md bg-destructive/15 border border-destructive/60 text-destructive",
                            span { class: "inline-block w-2 h-2 rounded-full bg-destructive animate-pulse" }
                            span { class: "font-mono tabular-nums font-medium", "REC {rec_tc}" }
                            div {
                              class: "w-16 h-1.5 rounded bg-background overflow-hidden",
                              div {
                                class: "h-full bg-destructive transition-all duration-75",
                                style: "width: {vu_pct}%;",
                              }
                            }
                            Button {
                              variant: ButtonVariant::Destructive,
                              size: ButtonSize::Xs,
                              class: "h-5 px-2 text-[11px] font-semibold",
                              onclick: move |_| {
                                let l = lang_for_stop.clone();
                                async move {
                                  run_stop_segment_recording(&mut state, idx, l).await;
                                }
                              },
                              "■ Stop & Attach"
                            }
                          }
                        }
                      }
                    } else if let Some(ref rec) = recorded_path {
                      {
                        let rec_short = short_filename(rec);
                        let rec_for_align = rec.clone();
                        let lang_for_align = seg.language.clone();
                        let other_recording = rec_target.is_some();
                        rsx! {
                          span {
                            title: "{rec}",
                            Badge {
                              variant: BadgeVariant::Info,
                              style: BadgeStyle::Soft,
                              size: BadgeSize::Xs,
                              class: "font-mono py-0.5",
                              "Rec: {rec_short}"
                            }
                          }
                          Button {
                            variant: ButtonVariant::Outline,
                            size: ButtonSize::Xs,
                            class: "h-6 px-2 text-[11px] bg-secondary text-destructive hover:border-destructive",
                            disabled: other_recording,
                            title: "Re-record this segment directly from your microphone",
                            onclick: move |_| {
                              if let Err(e) = start_mic_recording(&mut state, RecordingTarget::Segment(idx)) {
                                state.status_msg.set(e);
                              }
                            },
                            "● Re-rec"
                          }
                          Button {
                            variant: if enhance_on { ButtonVariant::Soft } else { ButtonVariant::Outline },
                            size: ButtonSize::Xs,
                            class: if enhance_on {
                              "h-6 px-2 text-[11px] border border-primary/60 text-primary".to_string()
                            } else {
                              "h-6 px-2 text-[11px] bg-secondary text-muted-foreground hover:text-foreground".to_string()
                            },
                            title: "Apply local STFT denoise + broadcast vocal EQ/compression",
                            onclick: move |_| {
                              state.mutate_project(|p| {
                                if let Some(ov) = p.segment_overrides.get_mut(&idx) {
                                  ov.enhance_voice = !ov.enhance_voice;
                                }
                              });
                            },
                            if enhance_on { "Voice Enhance: ON" } else { "Voice Enhance: OFF" }
                          }
                          Button {
                            variant: ButtonVariant::Outline,
                            size: ButtonSize::Xs,
                            class: "h-6 px-2 text-[11px] bg-secondary text-muted-foreground hover:text-foreground",
                            loading: is_aligning,
                            title: "Run STT alignment (falls back to offline speech clip detection without STT key)",
                            onclick: move |_| {
                              let p = rec_for_align.clone();
                              let l = lang_for_align.clone();
                              async move {
                                run_align_recording(&mut state, idx, p, l).await;
                              }
                            },
                            if is_aligning { "Aligning..." } else { "Transcribe / Align" }
                          }
                          Button {
                            variant: ButtonVariant::Ghost,
                            size: ButtonSize::Xs,
                            class: "h-6 px-1.5 text-[11px] text-muted-foreground hover:text-destructive",
                            title: "Detach recording and revert segment to AI TTS",
                            onclick: move |_| {
                              state.mutate_project(|p| {
                                p.segment_overrides.remove(&idx);
                              });
                              state.segment_audio.write().remove(&idx);
                              state.status_msg.set(format!("Segment {:02} switched back to AI TTS", idx + 1));
                            },
                            "Use TTS"
                          }
                        }
                      }
                    } else {
                      {
                        let lang_for_attach = seg.language.clone();
                        let other_recording = rec_target.is_some();
                        rsx! {
                          Button {
                            variant: ButtonVariant::Outline,
                            size: ButtonSize::Xs,
                            class: "h-6 px-2 text-[11px] bg-secondary text-destructive hover:border-destructive",
                            disabled: other_recording,
                            title: "Record this segment directly from your default microphone",
                            onclick: move |_| {
                              if let Err(e) = start_mic_recording(&mut state, RecordingTarget::Segment(idx)) {
                                state.status_msg.set(e);
                              }
                            },
                            "● Record Mic"
                          }
                          Button {
                            variant: ButtonVariant::Outline,
                            size: ButtonSize::Xs,
                            class: "h-6 px-2 text-[11px] bg-secondary text-muted-foreground hover:text-foreground hover:border-info",
                            title: "Attach an existing voice recording file for text-based audio cutting and local voice enhancement",
                            onclick: move |_| {
                              let l = lang_for_attach.clone();
                              async move {
                                run_attach_recording(&mut state, idx, l).await;
                              }
                            },
                            "+ Attach Audio"
                          }
                        }
                      }
                    }
                  }

                  if !cut_spans.is_empty() {
                    div {
                      class: "flex items-center gap-1.5",
                      if active_fillers > 0 {
                        Button {
                          variant: ButtonVariant::Soft,
                          size: ButtonSize::Xs,
                          class: "h-6 px-2 text-[11px] border border-primary/50 text-primary",
                          onclick: move |_| {
                            let removed = state.mutate_project(|p| {
                              let count = p
                                .segment_overrides
                                .get_mut(&idx)
                                .map(SegmentMediaOverride::delete_all_fillers)
                                .unwrap_or(0);
                              if let Some(active) = p
                                .segment_overrides
                                .get(&idx)
                                .and_then(SegmentMediaOverride::active_text)
                                && !active.trim().is_empty()
                                && let Some(s) = p.script.segments.get_mut(idx)
                              {
                                s.text = active;
                              }
                              count
                            });
                            state.status_msg.set(format!("Cut {removed} filler span(s) in segment {:02}", idx + 1));
                          },
                          "Cut Fillers ({active_fillers})"
                        }
                      }
                      if deleted_spans > 0 {
                        Button {
                          variant: ButtonVariant::Outline,
                          size: ButtonSize::Xs,
                          class: "h-6 px-2 text-[11px] bg-secondary text-muted-foreground hover:text-foreground",
                          onclick: move |_| {
                            state.mutate_project(|p| {
                              if let Some(ov) = p.segment_overrides.get_mut(&idx) {
                                ov.restore_all_spans();
                                if let Some(active) = ov.active_text()
                                  && !active.trim().is_empty()
                                  && let Some(s) = p.script.segments.get_mut(idx)
                                {
                                  s.text = active;
                                }
                              }
                            });
                          },
                          "Restore ({deleted_spans})"
                        }
                      }
                    }
                  }
                }

                // Interactive Click-to-Cut Text Chips ("改文本即剪音频" domain control)
                if !cut_spans.is_empty() {
                  div {
                    class: "bg-background border border-border rounded p-2 flex flex-wrap gap-1.5",
                    for (span_idx, span) in cut_spans.iter().enumerate() {
                      {
                        let chip_class = if span.deleted {
                          "px-2 py-0.5 rounded border text-xs line-through opacity-45 bg-card border-destructive/40 text-destructive hover:opacity-80"
                        } else if span.is_filler {
                          "px-2 py-0.5 rounded border text-xs bg-primary/15 border-primary/60 text-primary hover:bg-primary/25"
                        } else {
                          "px-2 py-0.5 rounded border text-xs bg-secondary border-border text-foreground hover:border-destructive/60"
                        };
                        let tooltip = format!(
                          "{}–{} · click to {}",
                          format_timecode(span.start_secs),
                          format_timecode(span.end_secs),
                          if span.deleted { "restore" } else { "cut audio" }
                        );
                        rsx! {
                          button {
                            key: "span-{idx}-{span_idx}",
                            r#type: "button",
                            class: "{chip_class}",
                            title: "{tooltip}",
                            onclick: move |_| {
                              state.mutate_project(|p| {
                                if let Some(ov) = p.segment_overrides.get_mut(&idx)
                                  && let Some(sp) = ov.spans.get_mut(span_idx)
                                {
                                  sp.deleted = !sp.deleted;
                                  if let Some(active) = ov.active_text()
                                    && !active.trim().is_empty()
                                    && let Some(s) = p.script.segments.get_mut(idx)
                                  {
                                    s.text = active;
                                  }
                                }
                              });
                            },
                            "{span.text}"
                          }
                        }
                      }
                    }
                  }
                }
              }
            }
          }
        }
      }
    }
  }
}
