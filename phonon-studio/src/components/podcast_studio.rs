#![allow(non_snake_case)]

use crate::project::{LoudnessHealth, format_db, format_timecode};
use crate::state::{AppState, InspectorTab, play_buffer};
use crate::studio_ops::{
  EpisodeExportFormat, assemble_episode, draft_script_from_topic, export_episode_file,
  preset_voices_for, synthesize_segment,
};
use dioxus::prelude::*;
use phonon_ai::VoiceConfig;
use std::collections::HashMap;
use std::sync::Arc;

/// Palette dots assigned to speakers deterministically by sorted order.
const SPEAKER_DOTS: &[&str] = &[
  "bg-[#E8A238]",
  "bg-[#38B2AC]",
  "bg-[#818CF8]",
  "bg-[#F472B6]",
  "bg-[#4ADE80]",
  "bg-[#FB923C]",
];

#[component]
pub fn PodcastStudioView() -> Element {
  rsx! {
    div {
      class: "flex-1 flex min-h-0 overflow-hidden bg-[#141619]",
      LeftConfigRail {}
      CenterScriptDesk {}
      RightEpisodeInspector {}
    }
  }
}

// ─── Left Rail: Providers, Cast, Show & Mastering ─────────────────────────────

#[component]
fn LeftConfigRail() -> Element {
  let mut state = use_context::<AppState>();
  let mut new_speaker_name = use_signal(String::new);

  let project = state.project.read().clone();
  let providers = state.provider_statuses.read().clone();
  let speakers = project.sorted_speakers();
  let preset_voices = preset_voices_for(project.tts_provider.as_deref());

  let lufs_select_value = match project.script.config.target_lufs {
    Some(v) if (v - (-16.0)).abs() < 0.5 => "-16",
    Some(v) if (v - (-14.0)).abs() < 0.5 => "-14",
    Some(v) if (v - (-18.0)).abs() < 0.5 => "-18",
    Some(v) if (v - (-23.0)).abs() < 0.5 => "-23",
    Some(_) => "-16",
    None => "off",
  };

  let bgm_label = project
    .script
    .config
    .bgm_audio
    .as_deref()
    .map(short_filename)
    .unwrap_or_else(|| "None".to_string());
  let intro_label = project
    .script
    .config
    .intro_audio
    .as_deref()
    .map(short_filename)
    .unwrap_or_else(|| "None".to_string());
  let outro_label = project
    .script
    .config
    .outro_audio
    .as_deref()
    .map(short_filename)
    .unwrap_or_else(|| "None".to_string());

  rsx! {
    aside {
      class: "w-72 shrink-0 bg-[#1C1F24] border-r border-[#2D333C] overflow-y-auto p-3.5 flex flex-col gap-5",

      // 1. AI Providers & Cache
      section {
        class: "flex flex-col gap-2.5",
        div { class: "text-xs font-semibold text-[#ECEFF4]", "AI Providers & Cache" }

        div {
          label { class: "block text-[11px] text-[#9AA4B2] mb-1", "TTS Provider" }
          select {
            class: "w-full bg-[#252A31] border border-[#2D333C] rounded px-2 py-1.5 text-xs text-[#ECEFF4] focus:outline-none focus:border-[#E8A238]",
            value: project.tts_provider.clone().unwrap_or_else(|| "auto".to_string()),
            onchange: move |evt| {
              let val = evt.value();
              state.mutate_project(|p| {
                p.tts_provider = if val == "auto" { None } else { Some(val) };
              });
            },
            option { value: "auto", "Auto (detect env key, fallback Edge)" }
            for p in providers.iter().filter(|p| p.has_tts) {
              {
                let suffix = match (p.key_env, p.key_present) {
                  (None, _) => "free / keyless",
                  (Some(_), true) => "key ready",
                  (Some(env), false) => env,
                };
                rsx! {
                  option { value: "{p.name}", "{p.name} ({suffix})" }
                }
              }
            }
          }
        }

        div {
          label { class: "block text-[11px] text-[#9AA4B2] mb-1", "TTS Model Override (optional)" }
          input {
            class: "w-full bg-[#252A31] border border-[#2D333C] rounded px-2 py-1 text-xs text-[#ECEFF4] placeholder-[#647080] focus:outline-none focus:border-[#E8A238]",
            placeholder: "Preset default",
            value: project.tts_model.clone().unwrap_or_default(),
            oninput: move |evt| {
              let v = evt.value();
              let mut p = state.project.write();
              p.tts_model = if v.trim().is_empty() { None } else { Some(v) };
            },
          }
        }

        if !preset_voices.is_empty() {
          button {
            class: "w-full px-2.5 py-1.5 rounded text-xs font-medium bg-[#252A31] border border-[#2D333C] text-[#ECEFF4] hover:border-[#38B2AC] transition",
            onclick: move |_| {
              let voices = preset_voices_for(state.project.read().tts_provider.as_deref());
              let n = state.mutate_project(|p| p.apply_provider_voices(voices));
              state.status_msg.set(format!("Mapped verified preset voices to {n} speaker(s)"));
            },
            "Assign preset voices to cast"
          }
        }

        div {
          label { class: "block text-[11px] text-[#9AA4B2] mb-1", "LLM Provider (AI Draft)" }
          select {
            class: "w-full bg-[#252A31] border border-[#2D333C] rounded px-2 py-1.5 text-xs text-[#ECEFF4] focus:outline-none focus:border-[#E8A238]",
            value: project.llm_provider.clone().unwrap_or_else(|| "auto".to_string()),
            onchange: move |evt| {
              let val = evt.value();
              state.mutate_project(|p| {
                p.llm_provider = if val == "auto" { None } else { Some(val) };
              });
            },
            option { value: "auto", "Auto (first configured LLM key)" }
            for p in providers.iter().filter(|p| p.has_llm) {
              {
                let suffix = if p.key_present { "key ready" } else { p.key_env.unwrap_or("missing key") };
                rsx! {
                  option { value: "{p.name}", "{p.name} ({suffix})" }
                }
              }
            }
          }
        }

        label {
          class: "flex items-center gap-2 text-xs text-[#9AA4B2] cursor-pointer pt-0.5",
          input {
            r#type: "checkbox",
            class: "accent-[#38B2AC]",
            checked: project.use_cache,
            onchange: move |evt| {
              let checked = evt.checked();
              state.project.write().use_cache = checked;
            },
          }
          "Reuse persistent TTS disk cache"
        }
      }

      div { class: "h-px bg-[#2D333C]" }

      // 2. Speakers / Cast
      section {
        class: "flex flex-col gap-2.5",
        div {
          class: "flex items-center justify-between",
          span { class: "text-xs font-semibold text-[#ECEFF4]", "Speakers ({speakers.len()})" }
        }

        for (idx, name) in speakers.iter().enumerate() {
          if let Some(voice) = project.script.speakers.get(name) {
            {
              let dot = SPEAKER_DOTS[idx % SPEAKER_DOTS.len()];
              let speaker_key = name.clone();
              let speaker_key_voice = name.clone();
              let speaker_key_preset = name.clone();
              let speaker_key_speed = name.clone();
              let speaker_key_emotion = name.clone();
              let speaker_key_del = name.clone();
              let voice_id = voice.voice_id.clone();
              let speed = voice.speed;
              let emotion = voice.emotion.clone().unwrap_or_default();

              rsx! {
                div {
                  key: "{speaker_key}",
                  class: "bg-[#141619] border border-[#2D333C] rounded-md p-2.5 flex flex-col gap-2",

                  div {
                    class: "flex items-center justify-between gap-2",
                    div {
                      class: "flex items-center gap-2 min-w-0",
                      span { class: "inline-block w-2 h-2 rounded-full shrink-0 {dot}" }
                      span { class: "text-xs font-medium text-[#ECEFF4] truncate", "{name}" }
                    }
                    button {
                      class: "text-[11px] text-[#647080] hover:text-[#E05252] px-1",
                      title: "Remove speaker",
                      onclick: move |_| {
                        let res = state.mutate_project(|p| p.remove_speaker(&speaker_key_del));
                        if let Err(e) = res {
                          state.status_msg.set(e);
                        }
                      },
                      "×"
                    }
                  }

                  if !preset_voices.is_empty() {
                    select {
                      class: "w-full bg-[#252A31] border border-[#2D333C] rounded px-2 py-1 text-[11px] text-[#ECEFF4]",
                      value: "{voice_id}",
                      onchange: move |evt| {
                        let v = evt.value();
                        if !v.is_empty() {
                          state.mutate_project(|p| {
                            if let Some(cfg) = p.script.speakers.get_mut(&speaker_key_preset) {
                              cfg.voice_id = v;
                            }
                          });
                        }
                      },
                      option { value: "", "Pick preset voice..." }
                      for pv in preset_voices.iter() {
                        option { value: "{pv}", "{pv}" }
                      }
                    }
                  }

                  input {
                    class: "w-full bg-[#252A31] border border-[#2D333C] rounded px-2 py-1 text-[11px] font-mono text-[#ECEFF4] placeholder-[#647080]",
                    placeholder: "Voice ID",
                    value: "{voice_id}",
                    oninput: move |evt| {
                      let v = evt.value();
                      if let Some(cfg) = state.project.write().script.speakers.get_mut(&speaker_key_voice) {
                        cfg.voice_id = v;
                      }
                    },
                  }

                  div {
                    class: "flex items-center gap-2",
                    span { class: "text-[11px] text-[#9AA4B2] w-11 shrink-0", "Speed" }
                    input {
                      r#type: "range",
                      min: "0.5",
                      max: "2.0",
                      step: "0.05",
                      value: "{speed}",
                      class: "flex-1 accent-[#E8A238]",
                      oninput: move |evt| {
                        if let Ok(s) = evt.value().parse::<f64>()
                          && let Some(cfg) = state.project.write().script.speakers.get_mut(&speaker_key_speed)
                        {
                          cfg.speed = s.clamp(0.25, 4.0);
                        }
                      },
                    }
                    span { class: "text-[11px] font-mono tabular-nums text-[#9AA4B2] w-9 text-right", "{speed:.2}x" }
                  }

                  input {
                    class: "w-full bg-[#252A31] border border-[#2D333C] rounded px-2 py-1 text-[11px] text-[#ECEFF4] placeholder-[#647080]",
                    placeholder: "Emotion (optional, e.g. happy, calm)",
                    value: "{emotion}",
                    oninput: move |evt| {
                      let v = evt.value();
                      if let Some(cfg) = state.project.write().script.speakers.get_mut(&speaker_key_emotion) {
                        cfg.emotion = if v.trim().is_empty() { None } else { Some(v) };
                      }
                    },
                  }
                }
              }
            }
          }
        }

        // Add speaker form
        div {
          class: "flex gap-1.5",
          input {
            class: "flex-1 min-w-0 bg-[#252A31] border border-[#2D333C] rounded px-2 py-1 text-xs text-[#ECEFF4] placeholder-[#647080]",
            placeholder: "New speaker name",
            value: "{new_speaker_name}",
            oninput: move |evt| new_speaker_name.set(evt.value()),
          }
          button {
            class: "px-2.5 py-1 rounded text-xs font-medium bg-[#252A31] border border-[#2D333C] text-[#ECEFF4] hover:border-[#E8A238]",
            onclick: move |_| {
              let name = new_speaker_name.read().trim().to_string();
              if name.is_empty() {
                return;
              }
              let default_voice = preset_voices_for(state.project.read().tts_provider.as_deref())
                .first()
                .copied()
                .unwrap_or("zh-CN-XiaoxiaoNeural");
              let res = state.mutate_project(|p| {
                p.add_speaker(name.clone(), VoiceConfig::new(default_voice))
              });
              match res {
                Ok(()) => {
                  new_speaker_name.set(String::new());
                  state.status_msg.set(format!("Added speaker '{name}'"));
                }
                Err(e) => state.status_msg.set(e),
              }
            },
            "+ Add"
          }
        }
      }

      div { class: "h-px bg-[#2D333C]" }

      // 3. Show & Broadcast Mastering
      section {
        class: "flex flex-col gap-2.5",
        div { class: "text-xs font-semibold text-[#ECEFF4]", "Broadcast Mastering & Mix" }

        div {
          label { class: "block text-[11px] text-[#9AA4B2] mb-1", "Loudness Standard (BS.1770-4)" }
          select {
            class: "w-full bg-[#252A31] border border-[#2D333C] rounded px-2 py-1.5 text-xs text-[#ECEFF4]",
            value: "{lufs_select_value}",
            onchange: move |evt| {
              let target = match evt.value().as_str() {
                "-16" => Some(-16.0),
                "-14" => Some(-14.0),
                "-18" => Some(-18.0),
                "-23" => Some(-23.0),
                _ => None,
              };
              state.mutate_project(|p| p.script.config.target_lufs = target);
            },
            option { value: "-16", "-16 LUFS (Apple Podcasts / Stereo)" }
            option { value: "-14", "-14 LUFS (Spotify / YouTube)" }
            option { value: "-18", "-18 LUFS (Mono Speech)" }
            option { value: "-23", "-23 LUFS (EBU R128 Broadcast)" }
            option { value: "off", "Peak Normalize Only" }
          }
        }

        MasteringSlider {
          label: "Segment Pause",
          min: "0.0",
          max: "2.0",
          step: "0.05",
          value: format!("{}", project.script.config.pause_between_segments_secs),
          display: format!("{:.2}s", project.script.config.pause_between_segments_secs),
          onchange: move |v: f64| {
            state.project.write().script.config.pause_between_segments_secs = v;
          },
        }

        MasteringSlider {
          label: "Crossfade",
          min: "0.0",
          max: "1.0",
          step: "0.05",
          value: format!("{}", project.script.config.crossfade_secs),
          display: format!("{:.2}s", project.script.config.crossfade_secs),
          onchange: move |v: f64| {
            state.project.write().script.config.crossfade_secs = v;
          },
        }

        MasteringSlider {
          label: "BGM Level",
          min: "0.0",
          max: "0.4",
          step: "0.01",
          value: format!("{}", project.script.config.bgm_volume),
          display: format!("{:.0}%", project.script.config.bgm_volume * 100.0),
          onchange: move |v: f64| {
            state.project.write().script.config.bgm_volume = v as f32;
          },
        }

        MasteringSlider {
          label: "Fade In",
          min: "0.0",
          max: "5.0",
          step: "0.1",
          value: format!("{}", project.script.config.fade_in_secs),
          display: format!("{:.1}s", project.script.config.fade_in_secs),
          onchange: move |v: f64| {
            state.project.write().script.config.fade_in_secs = v;
          },
        }

        MasteringSlider {
          label: "Fade Out",
          min: "0.0",
          max: "5.0",
          step: "0.1",
          value: format!("{}", project.script.config.fade_out_secs),
          display: format!("{:.1}s", project.script.config.fade_out_secs),
          onchange: move |v: f64| {
            state.project.write().script.config.fade_out_secs = v;
          },
        }

        // BGM / Intro / Outro pickers
        AssetPickerRow {
          label: "BGM",
          current: bgm_label,
          onpick: move |_| async move {
            if let Some(p) = pick_audio_path().await {
              state.mutate_project(|proj| {
                proj.script.config.bgm = None;
                proj.script.config.bgm_audio = Some(p);
              });
            }
          },
          onclear: move |_| {
            state.mutate_project(|proj| {
              proj.script.config.bgm = None;
              proj.script.config.bgm_audio = None;
            });
          },
        }
        AssetPickerRow {
          label: "Intro",
          current: intro_label,
          onpick: move |_| async move {
            if let Some(p) = pick_audio_path().await {
              state.mutate_project(|proj| {
                proj.script.config.intro = None;
                proj.script.config.intro_audio = Some(p);
              });
            }
          },
          onclear: move |_| {
            state.mutate_project(|proj| {
              proj.script.config.intro = None;
              proj.script.config.intro_audio = None;
            });
          },
        }
        AssetPickerRow {
          label: "Outro",
          current: outro_label,
          onpick: move |_| async move {
            if let Some(p) = pick_audio_path().await {
              state.mutate_project(|proj| {
                proj.script.config.outro = None;
                proj.script.config.outro_audio = Some(p);
              });
            }
          },
          onclear: move |_| {
            state.mutate_project(|proj| {
              proj.script.config.outro = None;
              proj.script.config.outro_audio = None;
            });
          },
        }
      }
    }
  }
}

#[component]
fn MasteringSlider(
  label: &'static str,
  min: &'static str,
  max: &'static str,
  step: &'static str,
  value: String,
  display: String,
  onchange: EventHandler<f64>,
) -> Element {
  rsx! {
    div {
      class: "flex items-center gap-2",
      span { class: "text-[11px] text-[#9AA4B2] w-24 shrink-0", "{label}" }
      input {
        r#type: "range",
        min: "{min}",
        max: "{max}",
        step: "{step}",
        value: "{value}",
        class: "flex-1 accent-[#38B2AC]",
        oninput: move |evt| {
          if let Ok(v) = evt.value().parse::<f64>() {
            onchange.call(v);
          }
        },
      }
      span { class: "text-[11px] font-mono tabular-nums text-[#9AA4B2] w-10 text-right", "{display}" }
    }
  }
}

#[component]
fn AssetPickerRow(
  label: &'static str,
  current: String,
  onpick: EventHandler<MouseEvent>,
  onclear: EventHandler<MouseEvent>,
) -> Element {
  let has_file = current != "None";
  rsx! {
    div {
      class: "flex items-center gap-1.5 text-[11px]",
      span { class: "text-[#9AA4B2] w-11 shrink-0", "{label}" }
      span {
        class: "flex-1 min-w-0 truncate px-2 py-1 rounded bg-[#141619] border border-[#2D333C] text-[#ECEFF4]",
        title: "{current}",
        "{current}"
      }
      button {
        class: "px-2 py-1 rounded bg-[#252A31] text-[#ECEFF4] hover:bg-[#2D333C]",
        onclick: move |e| onpick.call(e),
        "Pick"
      }
      if has_file {
        button {
          class: "px-1.5 py-1 rounded bg-[#252A31] text-[#9AA4B2] hover:text-[#E05252]",
          onclick: move |e| onclear.call(e),
          "×"
        }
      }
    }
  }
}

// ─── Center Stage: Script Editor & Per-Segment Cached Preview ─────────────────

#[component]
fn CenterScriptDesk() -> Element {
  let mut state = use_context::<AppState>();
  let mut draft_topic = use_signal(String::new);
  let mut draft_segments = use_signal(|| 8usize);
  let mut draft_lang = use_signal(|| "zh-CN".to_string());

  let project = state.project.read().clone();
  let speakers = project.sorted_speakers();
  let seg_audio_map = state.segment_audio.read().clone();
  let busy_set = state.synthesizing_segments.read().clone();
  let is_assembling = (state.is_assembling)();
  let is_drafting = (state.is_drafting)();
  let show_ai_draft = (state.show_ai_draft)();

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
      class: "flex-1 flex flex-col min-w-0 overflow-hidden",

      // Episode title & assembly bar
      div {
        class: "px-5 py-3 border-b border-[#2D333C] bg-[#181B20] flex flex-wrap items-center justify-between gap-3",

        div {
          class: "flex-1 min-w-[220px] flex flex-col gap-1",
          input {
            class: "w-full bg-transparent text-base font-semibold text-[#ECEFF4] placeholder-[#647080] focus:outline-none focus:bg-[#252A31] rounded px-1.5 py-0.5 -ml-1.5",
            placeholder: "Episode title",
            value: "{project.script.title}",
            oninput: move |evt| {
              let v = evt.value();
              let mut p = state.project.write();
              p.script.title = v.clone();
              if p.tags.title.is_empty() {
                p.tags.title = v;
              }
            },
          }
          div {
            class: "flex items-center gap-3 text-[11px] font-mono tabular-nums text-[#9AA4B2]",
            span { "{total_segments} segments" }
            span { class: "text-[#38B2AC]", "{fresh_count}/{total_segments} cached" }
          }
        }

        div {
          class: "flex items-center gap-2",
          button {
            class: "px-3 py-1.5 rounded text-xs font-medium bg-[#252A31] border border-[#2D333C] text-[#ECEFF4] hover:border-[#9AA4B2]",
            onclick: move |_| {
              state.mutate_project(|p| {
                let last = p.script.segments.len().checked_sub(1);
                p.add_segment_after(last, None);
              });
            },
            "+ Add Segment"
          }
          button {
            class: if is_assembling {
              "px-4 py-1.5 rounded text-xs font-semibold bg-[#E8A238]/60 text-[#141619] cursor-wait"
            } else {
              "px-4 py-1.5 rounded text-xs font-semibold bg-[#E8A238] text-[#141619] hover:opacity-90 transition"
            },
            disabled: is_assembling,
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

      // Optional collapsible AI Script Draft drawer
      if show_ai_draft {
        div {
          class: "px-5 py-3 bg-[#1C1F24] border-b border-[#E8A238]/40 flex flex-wrap items-end gap-3",
          div {
            class: "flex-1 min-w-[240px]",
            label { class: "block text-[11px] text-[#9AA4B2] mb-1", "Topic or outline for AI script draft" }
            input {
              class: "w-full bg-[#141619] border border-[#2D333C] rounded px-2.5 py-1.5 text-xs text-[#ECEFF4] placeholder-[#647080] focus:outline-none focus:border-[#E8A238]",
              placeholder: "e.g. Rust 音频引擎与本地 AI 播客制作实践",
              value: "{draft_topic}",
              oninput: move |evt| draft_topic.set(evt.value()),
            }
          }
          div {
            class: "w-24",
            label { class: "block text-[11px] text-[#9AA4B2] mb-1", "Segments" }
            input {
              r#type: "number",
              min: "2",
              max: "40",
              class: "w-full bg-[#141619] border border-[#2D333C] rounded px-2 py-1.5 text-xs font-mono text-[#ECEFF4]",
              value: "{draft_segments}",
              oninput: move |evt| {
                if let Ok(n) = evt.value().parse::<usize>() {
                  draft_segments.set(n.clamp(2, 40));
                }
              },
            }
          }
          div {
            class: "w-28",
            label { class: "block text-[11px] text-[#9AA4B2] mb-1", "Language" }
            select {
              class: "w-full bg-[#141619] border border-[#2D333C] rounded px-2 py-1.5 text-xs text-[#ECEFF4]",
              value: "{draft_lang}",
              onchange: move |evt| draft_lang.set(evt.value()),
              option { value: "zh-CN", "zh-CN" }
              option { value: "en-US", "en-US" }
              option { value: "ja-JP", "ja-JP" }
            }
          }
          button {
            class: "px-3.5 py-1.5 rounded text-xs font-semibold bg-[#E8A238] text-[#141619] hover:opacity-90 disabled:opacity-50",
            disabled: is_drafting,
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
            let (badge_text, badge_class) = if is_busy {
              ("Synthesizing...".to_string(), "bg-[#E8A238]/20 text-[#E8A238] border-[#E8A238]/40")
            } else if let (Some(fp), Some(entry)) = (fp.as_deref(), cached_entry) {
              if entry.fingerprint == fp {
                (
                  format!("Cached {:.1}s", entry.duration_secs),
                  "bg-[#38B2AC]/15 text-[#38B2AC] border-[#38B2AC]/40",
                )
              } else {
                ("Edited · stale".to_string(), "bg-[#E8A238]/15 text-[#E8A238] border-[#E8A238]/30")
              }
            } else {
              ("Not synthesized".to_string(), "bg-[#252A31] text-[#647080] border-[#2D333C]")
            };

            let chapter_val = seg.chapter.clone().unwrap_or_default();
            let lang_val = seg.language.clone().unwrap_or_default();
            let text_val = seg.text.clone();
            let num_label = format!("{:02}", idx + 1);
            let can_remove = total_segments > 1;

            rsx! {
              article {
                key: "seg-{idx}",
                class: "bg-[#1C1F24] border border-[#2D333C] rounded-md p-3 flex flex-col gap-2.5",

                // Segment header controls
                div {
                  class: "flex flex-wrap items-center justify-between gap-2",

                  div {
                    class: "flex items-center gap-2 flex-wrap",
                    span { class: "text-xs font-mono tabular-nums text-[#647080] w-5", "{num_label}" }
                    span { class: "inline-block w-2 h-2 rounded-full {dot}" }

                    select {
                      class: "bg-[#252A31] border border-[#2D333C] rounded px-2 py-1 text-xs font-medium text-[#ECEFF4]",
                      value: "{seg.speaker}",
                      onchange: move |evt| {
                        let sp = evt.value();
                        state.mutate_project(|p| {
                          if let Some(s) = p.script.segments.get_mut(idx) {
                            s.speaker = sp;
                          }
                        });
                      },
                      for sp in speakers.iter() {
                        option { value: "{sp}", "{sp}" }
                      }
                    }

                    input {
                      class: "w-36 bg-[#141619] border border-[#2D333C] rounded px-2 py-1 text-[11px] text-[#ECEFF4] placeholder-[#647080] focus:outline-none focus:border-[#38B2AC]",
                      placeholder: "Chapter title...",
                      value: "{chapter_val}",
                      oninput: move |evt| {
                        let v = evt.value();
                        if let Some(s) = state.project.write().script.segments.get_mut(idx) {
                          s.chapter = if v.trim().is_empty() { None } else { Some(v) };
                        }
                      },
                    }

                    input {
                      class: "w-16 bg-[#141619] border border-[#2D333C] rounded px-1.5 py-1 text-[11px] font-mono text-[#9AA4B2] placeholder-[#647080]",
                      placeholder: "lang",
                      title: "Optional language override (e.g. zh-CN, en-US)",
                      value: "{lang_val}",
                      oninput: move |evt| {
                        let v = evt.value();
                        if let Some(s) = state.project.write().script.segments.get_mut(idx) {
                          s.language = if v.trim().is_empty() { None } else { Some(v) };
                        }
                      },
                    }

                    span {
                      class: "px-2 py-0.5 rounded border text-[11px] font-mono tabular-nums {badge_class}",
                      "{badge_text}"
                    }
                  }

                  // Action buttons
                  div {
                    class: "flex items-center gap-1",
                    button {
                      class: "px-2.5 py-1 rounded text-xs font-medium bg-[#252A31] border border-[#2D333C] text-[#ECEFF4] hover:border-[#E8A238] disabled:opacity-40",
                      disabled: is_busy,
                      title: "Preview segment (uses cache when fresh)",
                      onclick: move |_| async move {
                        run_preview_segment(&mut state, idx, false).await;
                      },
                      "▶ Preview"
                    }
                    button {
                      class: "px-2 py-1 rounded text-xs bg-[#252A31] border border-[#2D333C] text-[#9AA4B2] hover:text-[#ECEFF4] disabled:opacity-40",
                      disabled: is_busy,
                      title: "Force regenerate this segment (bypass cache)",
                      onclick: move |_| async move {
                        run_preview_segment(&mut state, idx, true).await;
                      },
                      "↻"
                    }
                    button {
                      class: "px-1.5 py-1 rounded text-xs bg-[#252A31] text-[#9AA4B2] hover:text-[#ECEFF4] disabled:opacity-30",
                      disabled: idx == 0,
                      title: "Move up",
                      onclick: move |_| {
                        if let Some(target) = state.mutate_project(|p| p.move_segment(idx, -1)) {
                          swap_cached_entries(&mut state, idx, target);
                        }
                      },
                      "↑"
                    }
                    button {
                      class: "px-1.5 py-1 rounded text-xs bg-[#252A31] text-[#9AA4B2] hover:text-[#ECEFF4] disabled:opacity-30",
                      disabled: idx + 1 >= total_segments,
                      title: "Move down",
                      onclick: move |_| {
                        if let Some(target) = state.mutate_project(|p| p.move_segment(idx, 1)) {
                          swap_cached_entries(&mut state, idx, target);
                        }
                      },
                      "↓"
                    }
                    button {
                      class: "px-1.5 py-1 rounded text-xs bg-[#252A31] text-[#9AA4B2] hover:text-[#ECEFF4]",
                      title: "Duplicate segment",
                      onclick: move |_| {
                        if state.mutate_project(|p| p.duplicate_segment(idx)).is_some() {
                          shift_cache_after_insert(&mut state, idx + 1);
                        }
                      },
                      "Dup"
                    }
                    button {
                      class: "px-1.5 py-1 rounded text-xs bg-[#252A31] text-[#9AA4B2] hover:text-[#ECEFF4]",
                      title: "Insert segment below",
                      onclick: move |_| {
                        state.mutate_project(|p| {
                          p.add_segment_after(Some(idx), None);
                        });
                        // Shift in-memory audio cache keys above insertion index.
                        shift_cache_after_insert(&mut state, idx + 1);
                      },
                      "+"
                    }
                    button {
                      class: "px-1.5 py-1 rounded text-xs bg-[#252A31] text-[#9AA4B2] hover:text-[#E05252] disabled:opacity-30",
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
                textarea {
                  rows: "3",
                  class: "w-full bg-[#141619] border border-[#2D333C] rounded p-2.5 text-sm leading-relaxed text-[#ECEFF4] placeholder-[#647080] resize-y focus:outline-none focus:border-[#E8A238]",
                  placeholder: "Enter dialogue for this segment...",
                  value: "{text_val}",
                  oninput: move |evt| {
                    let v = evt.value();
                    if let Some(s) = state.project.write().script.segments.get_mut(idx) {
                      s.text = v;
                    }
                  },
                }
              }
            }
          }
        }
      }
    }
  }
}

// ─── Right Inspector: Master Waveform, LUFS, Chapters, SRT & Export ──────────

#[component]
fn RightEpisodeInspector() -> Element {
  let mut state = use_context::<AppState>();
  let tab = (state.inspector_tab)();
  let view = (state.episode_view)();
  let project = state.project.read().clone();
  let bitrate = (state.episode_mp3_bitrate)();
  let chapter_count = view.as_ref().map(|v| v.chapters.len()).unwrap_or(0);

  rsx! {
    aside {
      class: "w-80 shrink-0 bg-[#1C1F24] border-l border-[#2D333C] flex flex-col overflow-hidden",

      // Inspector tab strip
      div {
        class: "flex border-b border-[#2D333C] bg-[#181B20] px-2 pt-2 gap-1",
        InspectorTabBtn { label: "Master & Export".to_string(), active: tab == InspectorTab::Master, onclick: move |_| state.inspector_tab.set(InspectorTab::Master) }
        InspectorTabBtn { label: format!("Chapters ({chapter_count})"), active: tab == InspectorTab::Chapters, onclick: move |_| state.inspector_tab.set(InspectorTab::Chapters) }
        InspectorTabBtn { label: "Subtitles".to_string(), active: tab == InspectorTab::Subtitles, onclick: move |_| state.inspector_tab.set(InspectorTab::Subtitles) }
      }

      // Inspector body
      div {
        class: "flex-1 overflow-y-auto p-3.5 flex flex-col gap-4",

        match tab {
          InspectorTab::Master => rsx! {
            if let Some(ref ep_view) = view {
              {
                let health = ep_view.loudness_health(project.script.config.target_lufs);
                let (health_label, health_class) = match health {
                  LoudnessHealth::BroadcastReady => ("Broadcast Ready", "bg-[#38B2AC]/15 text-[#38B2AC] border-[#38B2AC]/40"),
                  LoudnessHealth::OffTarget => ("Off Loudness Target", "bg-[#E8A238]/15 text-[#E8A238] border-[#E8A238]/40"),
                  LoudnessHealth::ClippingRisk => ("True-Peak Risk", "bg-[#E05252]/15 text-[#E05252] border-[#E05252]/40"),
                };
                rsx! {
                  // Waveform card + Play episode button
                  div {
                    class: "bg-[#141619] border border-[#2D333C] rounded-md p-2.5 flex flex-col gap-2",
                    div {
                      class: "flex items-center justify-between",
                      span {
                        class: "text-xs font-mono tabular-nums text-[#9AA4B2]",
                        "{format_timecode(ep_view.duration_secs)} · {ep_view.sample_rate} Hz · {ep_view.channels}ch"
                      }
                      button {
                        class: "px-2.5 py-1 rounded text-xs font-semibold bg-[#E8A238] text-[#141619] hover:opacity-90",
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
                        class: "stroke-[#38B2AC] fill-[#38B2AC]/80",
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
                      span { class: "text-xs font-semibold text-[#ECEFF4]", "Loudness Telemetry" }
                      span { class: "px-2 py-0.5 rounded border text-[11px] font-medium {health_class}", "{health_label}" }
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
              div {
                class: "bg-[#141619] border border-dashed border-[#2D333C] rounded-md p-4 text-center flex flex-col gap-1.5",
                span { class: "text-xs font-medium text-[#ECEFF4]", "Episode not assembled yet" }
                p { class: "text-[11px] text-[#9AA4B2] leading-relaxed", "Preview individual segments or click Assemble Episode to inspect BS.1770 loudness, chapters, subtitles, and export." }
              }
            }

            div { class: "h-px bg-[#2D333C]" }

            // Metadata Tags
            div {
              class: "flex flex-col gap-2",
              span { class: "text-xs font-semibold text-[#ECEFF4]", "ID3 / Episode Metadata" }
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
                  label { class: "block text-[11px] text-[#9AA4B2] mb-1", "MP3 Bitrate" }
                  select {
                    class: "w-full bg-[#252A31] border border-[#2D333C] rounded px-2 py-1 text-xs font-mono text-[#ECEFF4]",
                    value: "{bitrate}",
                    onchange: move |evt| {
                      if let Ok(b) = evt.value().parse::<u32>() {
                        state.episode_mp3_bitrate.set(b);
                      }
                    },
                    option { value: "128", "128 kbps" }
                    option { value: "192", "192 kbps" }
                    option { value: "256", "256 kbps" }
                    option { value: "320", "320 kbps" }
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
              button {
                class: "w-full px-3 py-2 rounded text-xs font-semibold bg-[#E8A238] text-[#141619] hover:opacity-90 disabled:opacity-40",
                disabled: view.is_none(),
                onclick: move |_| async move {
                  run_export_episode(&mut state, EpisodeExportFormat::Mp3, "MP3", "mp3").await;
                },
                "Export MP3 (ID3 + Chapters)"
              }
              div {
                class: "grid grid-cols-3 gap-1.5",
                button {
                  class: "px-2 py-1.5 rounded text-xs bg-[#252A31] border border-[#2D333C] text-[#ECEFF4] hover:border-[#9AA4B2] disabled:opacity-40",
                  disabled: view.is_none(),
                  onclick: move |_| async move {
                    run_export_episode(&mut state, EpisodeExportFormat::Wav, "WAV", "wav").await;
                  },
                  "WAV"
                }
                button {
                  class: "px-2 py-1.5 rounded text-xs bg-[#252A31] border border-[#2D333C] text-[#ECEFF4] hover:border-[#9AA4B2] disabled:opacity-40",
                  disabled: view.is_none(),
                  onclick: move |_| async move {
                    run_export_episode(&mut state, EpisodeExportFormat::Flac, "FLAC", "flac").await;
                  },
                  "FLAC"
                }
                button {
                  class: "px-2 py-1.5 rounded text-xs bg-[#252A31] border border-[#2D333C] text-[#ECEFF4] hover:border-[#9AA4B2] disabled:opacity-40",
                  disabled: view.is_none(),
                  onclick: move |_| async move {
                    run_export_episode(&mut state, EpisodeExportFormat::Ogg, "OGG", "ogg").await;
                  },
                  "OGG"
                }
              }
              div {
                class: "grid grid-cols-2 gap-1.5",
                button {
                  class: "px-2 py-1.5 rounded text-xs bg-[#252A31] border border-[#2D333C] text-[#9AA4B2] hover:text-[#ECEFF4] disabled:opacity-40",
                  disabled: view.is_none(),
                  onclick: move |_| async move {
                    run_export_episode(&mut state, EpisodeExportFormat::Srt, "SRT Subtitles", "srt").await;
                  },
                  "Export .srt"
                }
                button {
                  class: "px-2 py-1.5 rounded text-xs bg-[#252A31] border border-[#2D333C] text-[#9AA4B2] hover:text-[#ECEFF4] disabled:opacity-40",
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
                p { class: "text-xs text-[#9AA4B2]", "No chapters generated." }
              } else {
                div {
                  class: "flex flex-col gap-1.5",
                  p { class: "text-[11px] text-[#9AA4B2] mb-1", "Click any chapter to seek and play from its start marker." }
                  for (i, ch) in ep_view.chapters.iter().enumerate() {
                    {
                      let start = ch.start_secs;
                      let title = ch.title.clone();
                      rsx! {
                        button {
                          key: "ch-{i}",
                          class: "w-full text-left px-2.5 py-2 rounded bg-[#141619] border border-[#2D333C] hover:border-[#E8A238] flex items-center justify-between gap-2 transition",
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
                          span { class: "text-xs text-[#ECEFF4] truncate", "{ch.title}" }
                          span { class: "text-[11px] font-mono tabular-nums text-[#38B2AC] shrink-0", "{format_timecode(ch.start_secs)}" }
                        }
                      }
                    }
                  }
                }
              }
            } else {
              p { class: "text-xs text-[#9AA4B2]", "Assemble the episode first to inspect chapter timestamps." }
            }
          },

          InspectorTab::Subtitles => rsx! {
            if let Some(ref ep_view) = view {
              div {
                class: "flex flex-col gap-2.5 h-full",
                button {
                  class: "w-full px-3 py-1.5 rounded text-xs font-medium bg-[#252A31] border border-[#2D333C] text-[#ECEFF4] hover:border-[#38B2AC]",
                  onclick: move |_| async move {
                    run_export_episode(&mut state, EpisodeExportFormat::Srt, "SRT Subtitles", "srt").await;
                  },
                  "Save Subtitles (.srt)"
                }
                pre {
                  class: "flex-1 bg-[#141619] border border-[#2D333C] rounded p-2.5 text-[11px] font-mono text-[#9AA4B2] whitespace-pre-wrap overflow-y-auto leading-relaxed",
                  "{ep_view.srt}"
                }
              }
            } else {
              p { class: "text-xs text-[#9AA4B2]", "Assemble the episode first to generate synchronized SRT subtitles." }
            }
          },
        }
      }
    }
  }
}

#[component]
fn InspectorTabBtn(label: String, active: bool, onclick: EventHandler<MouseEvent>) -> Element {
  rsx! {
    button {
      class: if active {
        "flex-1 px-2 py-1.5 text-[11px] font-medium text-[#ECEFF4] border-b-2 border-[#E8A238]"
      } else {
        "flex-1 px-2 py-1.5 text-[11px] font-medium text-[#9AA4B2] border-b-2 border-transparent hover:text-[#ECEFF4]"
      },
      onclick: move |e| onclick.call(e),
      "{label}"
    }
  }
}

#[component]
fn MeterCell(label: &'static str, value: String) -> Element {
  rsx! {
    div {
      class: "bg-[#141619] border border-[#2D333C] rounded px-2.5 py-2 flex flex-col gap-0.5",
      span { class: "text-[11px] text-[#9AA4B2]", "{label}" }
      span { class: "text-xs font-mono tabular-nums font-semibold text-[#ECEFF4]", "{value}" }
    }
  }
}

#[component]
fn TagInput(label: &'static str, value: String, oninput: EventHandler<String>) -> Element {
  rsx! {
    div {
      label { class: "block text-[11px] text-[#9AA4B2] mb-1", "{label}" }
      input {
        class: "w-full bg-[#252A31] border border-[#2D333C] rounded px-2 py-1 text-xs text-[#ECEFF4] focus:outline-none focus:border-[#E8A238]",
        value: "{value}",
        oninput: move |evt| oninput.call(evt.value()),
      }
    }
  }
}

// ─── Async Actions & Cache Bookkeeping ────────────────────────────────────────

async fn run_preview_segment(state: &mut AppState, seg_idx: usize, force_refresh: bool) {
  let project = state.project.read().clone();
  let Some(fp) = project.segment_fingerprint(seg_idx) else {
    state
      .status_msg
      .set(format!("Segment {} is invalid", seg_idx + 1));
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
      state.status_msg.set(format!("Preview failed: {e}"));
    }
  }
}

async fn run_assemble_episode(state: &mut AppState) {
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
      state.status_msg.set(msg);
    }
    Err(e) => {
      state.status_msg.set(format!("Assembly failed: {e}"));
    }
  }
}

async fn run_ai_draft(
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
      state
        .status_msg
        .set(format!("Drafted {segs} segments — ready to preview"));
    }
    Err(e) => {
      state.status_msg.set(format!("AI draft failed: {e}"));
    }
  }
}

async fn run_export_episode(
  state: &mut AppState,
  format: EpisodeExportFormat,
  label: &str,
  ext: &str,
) {
  let Some(episode) = state.rendered_episode.read().clone() else {
    state
      .status_msg
      .set("Assemble the episode before exporting".to_string());
    return;
  };
  let tags = {
    let proj = state.project.read();
    match proj.tags.to_podcast_tags(&proj.script.title) {
      Ok(t) => t,
      Err(e) => {
        state.status_msg.set(e);
        return;
      }
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
    Ok(()) => state
      .status_msg
      .set(format!("Exported {label} to {}", path.display())),
    Err(e) => state.status_msg.set(format!("Export failed: {e}")),
  }
}

async fn pick_audio_path() -> Option<String> {
  rfd::AsyncFileDialog::new()
    .add_filter("Audio", &["wav", "mp3", "flac", "ogg", "aac", "m4a"])
    .pick_file()
    .await
    .map(|h| h.path().to_string_lossy().to_string())
}

fn short_filename(path: &str) -> String {
  std::path::Path::new(path)
    .file_name()
    .map(|n| n.to_string_lossy().to_string())
    .unwrap_or_else(|| path.to_string())
}

fn swap_cached_entries(state: &mut AppState, a: usize, b: usize) {
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

fn shift_cache_after_insert(state: &mut AppState, inserted_at: usize) {
  let old: HashMap<_, _> = std::mem::take(&mut *state.segment_audio.write());
  let mut next = HashMap::with_capacity(old.len());
  for (k, v) in old {
    next.insert(if k >= inserted_at { k + 1 } else { k }, v);
  }
  *state.segment_audio.write() = next;
}

fn shift_cache_after_remove(state: &mut AppState, removed_at: usize) {
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
