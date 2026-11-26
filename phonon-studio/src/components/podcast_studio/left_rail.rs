#![allow(non_snake_case)]

use super::{AssetPickerRow, SPEAKER_DOTS, pick_audio_path, short_filename};
use crate::state::AppState;
use crate::studio_ops::preset_voices_for;
use dioxus::prelude::*;
use dioxus_shadcn::{
  Badge, BadgeSize, BadgeStyle, BadgeVariant, Button, ButtonSize, ButtonVariant, Card, Input,
  InputSize, Label, NativeSelect, NativeSelectOption, Separator, Slider, Switch, SwitchSize,
};
use phonon_ai::VoiceConfig;

#[component]
pub(super) fn LeftConfigRail() -> Element {
  let mut state = use_context::<AppState>();
  let mut new_speaker_name = use_signal(String::new);

  let project = state.project.read().clone();
  let providers = state.provider_statuses.read().clone();
  let speakers = project.sorted_speakers();
  let preset_voices = preset_voices_for(project.tts_provider.as_deref());

  let tts_provider_val = project
    .tts_provider
    .clone()
    .unwrap_or_else(|| "auto".to_string());
  let llm_provider_val = project
    .llm_provider
    .clone()
    .unwrap_or_else(|| "auto".to_string());

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
      class: "h-full bg-card overflow-y-auto p-3.5 flex flex-col gap-5",

      // 1. AI Providers & Cache
      section {
        class: "flex flex-col gap-2.5",
        div { class: "text-xs font-semibold text-foreground", "AI Providers & Cache" }

        div {
          class: "flex flex-col gap-1",
          Label { class: "text-[11px] font-normal text-muted-foreground", "TTS Provider" }
          NativeSelect {
            class: "h-8 px-2 py-1 text-xs bg-secondary",
            value: "{tts_provider_val}",
            on_value_change: move |val: String| {
              state.mutate_project(|p| {
                p.tts_provider = if val == "auto" { None } else { Some(val) };
              });
            },
            NativeSelectOption {
              value: "auto",
              selected: tts_provider_val == "auto",
              "Auto (detect env key, fallback Edge)"
            }
            for p in providers.iter().filter(|p| p.has_tts) {
              {
                let suffix = match (p.key_env, p.key_present) {
                  (None, _) => "free / keyless",
                  (Some(_), true) => "key ready",
                  (Some(env), false) => env,
                };
                rsx! {
                  NativeSelectOption {
                    value: "{p.name}",
                    selected: tts_provider_val == p.name,
                    "{p.name} ({suffix})"
                  }
                }
              }
            }
          }
        }

        div {
          class: "flex flex-col gap-1",
          Label { class: "text-[11px] font-normal text-muted-foreground", "TTS Model Override (optional)" }
          Input {
            size: InputSize::Sm,
            class: "h-7 bg-secondary px-2 text-xs",
            placeholder: "Preset default",
            value: project.tts_model.clone().unwrap_or_default(),
            on_value_change: move |v: String| {
              let mut p = state.project.write();
              p.tts_model = if v.trim().is_empty() { None } else { Some(v) };
            },
          }
        }

        if !preset_voices.is_empty() {
          Button {
            variant: ButtonVariant::Outline,
            size: ButtonSize::Xs,
            class: "w-full bg-secondary hover:border-info",
            onclick: move |_| {
              let voices = preset_voices_for(state.project.read().tts_provider.as_deref());
              let n = state.mutate_project(|p| p.apply_provider_voices(voices));
              state.status_msg.set(format!("Mapped verified preset voices to {n} speaker(s)"));
            },
            "Assign preset voices to cast"
          }
        }

        div {
          class: "flex flex-col gap-1",
          Label { class: "text-[11px] font-normal text-muted-foreground", "LLM Provider (AI Draft)" }
          NativeSelect {
            class: "h-8 px-2 py-1 text-xs bg-secondary",
            value: "{llm_provider_val}",
            on_value_change: move |val: String| {
              state.mutate_project(|p| {
                p.llm_provider = if val == "auto" { None } else { Some(val) };
              });
            },
            NativeSelectOption {
              value: "auto",
              selected: llm_provider_val == "auto",
              "Auto (first configured LLM key)"
            }
            for p in providers.iter().filter(|p| p.has_llm) {
              {
                let suffix = if p.key_present { "key ready" } else { p.key_env.unwrap_or("missing key") };
                rsx! {
                  NativeSelectOption {
                    value: "{p.name}",
                    selected: llm_provider_val == p.name,
                    "{p.name} ({suffix})"
                  }
                }
              }
            }
          }
        }

        div {
          class: "flex items-center justify-between gap-2 pt-0.5",
          Label {
            r#for: "tts-disk-cache-switch",
            class: "text-xs font-normal text-muted-foreground cursor-pointer",
            "Reuse persistent TTS disk cache"
          }
          Switch {
            id: "tts-disk-cache-switch",
            size: SwitchSize::Sm,
            checked: project.use_cache,
            on_checked_change: move |checked: bool| {
              state.project.write().use_cache = checked;
            },
          }
        }
      }

      Separator { decorative: true }

      // 2. Speakers / Cast
      section {
        class: "flex flex-col gap-2.5",
        div {
          class: "flex items-center justify-between",
          span { class: "text-xs font-semibold text-foreground", "Speakers" }
          Badge {
            variant: BadgeVariant::Secondary,
            style: BadgeStyle::Soft,
            size: BadgeSize::Xs,
            class: "font-mono tabular-nums",
            "{speakers.len()}"
          }
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
                Card {
                  key: "{speaker_key}",
                  class: "bg-background p-2.5 flex flex-col gap-2 shadow-none",

                  div {
                    class: "flex items-center justify-between gap-2",
                    div {
                      class: "flex items-center gap-2 min-w-0",
                      span { class: "inline-block w-2 h-2 rounded-full shrink-0 {dot}" }
                      span { class: "text-xs font-medium text-foreground truncate", "{name}" }
                    }
                    Button {
                      variant: ButtonVariant::Ghost,
                      size: ButtonSize::Xs,
                      class: "h-6 w-6 p-0 text-muted-foreground hover:text-destructive",
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
                    NativeSelect {
                      class: "h-7 px-2 py-0.5 text-[11px] bg-secondary",
                      value: "{voice_id}",
                      on_value_change: move |v: String| {
                        if !v.is_empty() {
                          state.mutate_project(|p| {
                            if let Some(cfg) = p.script.speakers.get_mut(&speaker_key_preset) {
                              cfg.voice_id = v;
                            }
                          });
                        }
                      },
                      NativeSelectOption {
                        value: "",
                        selected: !preset_voices.iter().any(|pv| *pv == voice_id),
                        "Pick preset voice..."
                      }
                      for pv in preset_voices.iter() {
                        NativeSelectOption {
                          value: "{pv}",
                          selected: voice_id == *pv,
                          "{pv}"
                        }
                      }
                    }
                  }

                  Input {
                    size: InputSize::Sm,
                    class: "h-7 px-2 text-[11px] font-mono bg-secondary",
                    placeholder: "Voice ID",
                    value: "{voice_id}",
                    on_value_change: move |v: String| {
                      if let Some(cfg) = state.project.write().script.speakers.get_mut(&speaker_key_voice) {
                        cfg.voice_id = v;
                      }
                    },
                  }

                  div {
                    class: "flex items-center gap-2",
                    span { class: "text-[11px] text-muted-foreground w-11 shrink-0", "Speed" }
                    Slider {
                      value: speed,
                      min: 0.5,
                      max: 2.0,
                      step: 0.05,
                      class: "flex-1",
                      track_class: "h-1.5",
                      thumb_class: "h-3.5 w-3.5",
                      "aria-label": "{name} speaking speed",
                      on_value_change: move |s: f64| {
                        if let Some(cfg) = state.project.write().script.speakers.get_mut(&speaker_key_speed) {
                          cfg.speed = s.clamp(0.25, 4.0);
                        }
                      },
                    }
                    span {
                      class: "text-[11px] font-mono tabular-nums text-muted-foreground w-9 text-right",
                      "{speed:.2}x"
                    }
                  }

                  Input {
                    size: InputSize::Sm,
                    class: "h-7 px-2 text-[11px] bg-secondary",
                    placeholder: "Emotion (optional, e.g. happy, calm)",
                    value: "{emotion}",
                    on_value_change: move |v: String| {
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
          Input {
            size: InputSize::Sm,
            class: "flex-1 min-w-0 h-7 px-2 text-xs bg-secondary",
            placeholder: "New speaker name",
            value: "{new_speaker_name}",
            on_value_change: move |v: String| new_speaker_name.set(v),
          }
          Button {
            variant: ButtonVariant::Outline,
            size: ButtonSize::Xs,
            class: "bg-secondary hover:border-primary",
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

      Separator { decorative: true }

      // 3. Show & Broadcast Mastering
      section {
        class: "flex flex-col gap-2.5",
        div { class: "text-xs font-semibold text-foreground", "Broadcast Mastering & Mix" }

        div {
          class: "flex flex-col gap-1",
          Label { class: "text-[11px] font-normal text-muted-foreground", "Loudness Standard (BS.1770-4)" }
          NativeSelect {
            class: "h-8 px-2 py-1 text-xs bg-secondary",
            value: "{lufs_select_value}",
            on_value_change: move |val: String| {
              let target = match val.as_str() {
                "-16" => Some(-16.0),
                "-14" => Some(-14.0),
                "-18" => Some(-18.0),
                "-23" => Some(-23.0),
                _ => None,
              };
              state.mutate_project(|p| p.script.config.target_lufs = target);
            },
            NativeSelectOption { value: "-16", selected: lufs_select_value == "-16", "-16 LUFS (Apple Podcasts / Stereo)" }
            NativeSelectOption { value: "-14", selected: lufs_select_value == "-14", "-14 LUFS (Spotify / YouTube)" }
            NativeSelectOption { value: "-18", selected: lufs_select_value == "-18", "-18 LUFS (Mono Speech)" }
            NativeSelectOption { value: "-23", selected: lufs_select_value == "-23", "-23 LUFS (EBU R128 Broadcast)" }
            NativeSelectOption { value: "off", selected: lufs_select_value == "off", "Peak Normalize Only" }
          }
        }

        MasteringSlider {
          label: "Segment Pause",
          min: 0.0,
          max: 2.0,
          step: 0.05,
          value: project.script.config.pause_between_segments_secs,
          display: format!("{:.2}s", project.script.config.pause_between_segments_secs),
          onchange: move |v: f64| {
            state.project.write().script.config.pause_between_segments_secs = v;
          },
        }

        MasteringSlider {
          label: "Crossfade",
          min: 0.0,
          max: 1.0,
          step: 0.05,
          value: project.script.config.crossfade_secs,
          display: format!("{:.2}s", project.script.config.crossfade_secs),
          onchange: move |v: f64| {
            state.project.write().script.config.crossfade_secs = v;
          },
        }

        MasteringSlider {
          label: "BGM Level",
          min: 0.0,
          max: 0.4,
          step: 0.01,
          value: f64::from(project.script.config.bgm_volume),
          display: format!("{:.0}%", project.script.config.bgm_volume * 100.0),
          onchange: move |v: f64| {
            state.project.write().script.config.bgm_volume = v as f32;
          },
        }

        MasteringSlider {
          label: "Fade In",
          min: 0.0,
          max: 5.0,
          step: 0.1,
          value: project.script.config.fade_in_secs,
          display: format!("{:.1}s", project.script.config.fade_in_secs),
          onchange: move |v: f64| {
            state.project.write().script.config.fade_in_secs = v;
          },
        }

        MasteringSlider {
          label: "Fade Out",
          min: 0.0,
          max: 5.0,
          step: 0.1,
          value: project.script.config.fade_out_secs,
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
  min: f64,
  max: f64,
  step: f64,
  value: f64,
  display: String,
  onchange: EventHandler<f64>,
) -> Element {
  rsx! {
    div {
      class: "flex items-center gap-2",
      span { class: "text-[11px] text-muted-foreground w-24 shrink-0", "{label}" }
      Slider {
        value,
        min,
        max,
        step,
        class: "flex-1",
        track_class: "h-1.5",
        range_class: "bg-info",
        thumb_class: "h-3.5 w-3.5 border-info",
        "aria-label": label,
        on_value_change: move |v: f64| onchange.call(v),
      }
      span {
        class: "text-[11px] font-mono tabular-nums text-muted-foreground w-10 text-right",
        "{display}"
      }
    }
  }
}
