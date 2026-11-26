#![allow(non_snake_case)]

use crate::state::{AppState, transform_audio};
use dioxus::prelude::*;
use dioxus_shadcn::{
  Button, ButtonSize, ButtonVariant, Card, Label, Select, SelectContent, SelectItem, SelectTrigger,
  Separator, Slider,
};
use phonon_core::{
  AudioBuffer, VoiceEnhanceConfig, algorithms, detect_speech_regions, enhance_voice, splice_ranges,
};

fn enhance_preset_label(preset: &str) -> &'static str {
  match preset {
    "noisy_room" => "Noisy Room (Strong STFT Denoise)",
    "clean_mic" => "Clean Mic (Gentle Polish + Presence)",
    _ => "Podcast Studio (Balanced Denoise + EQ)",
  }
}

fn compress_preset_label(preset: &str) -> &'static str {
  match preset {
    "voice" => "Voice",
    "gentle" => "Gentle",
    "limiter" => "Limiter",
    _ => "None",
  }
}

#[component]
pub fn EffectsPanel() -> Element {
  let mut state = use_context::<AppState>();
  let mut enhance_preset = use_signal(|| "podcast_studio".to_string());
  let eff = (state.effects)();
  let enhance_val = enhance_preset();
  let compress_val = eff.compress_preset.clone();

  rsx! {
    div {
      class: "flex flex-col gap-3",
      h3 { class: "text-xs font-semibold text-foreground", "Offline DSP & Voice Enhance" }

      // One-Click Local Broadcast Voice Enhancement & Auto Silence Strip
      Card {
        class: "bg-background border-primary/40 p-2.5 flex flex-col gap-2 shadow-none",
        span { class: "text-[11px] font-semibold text-primary", "Local Broadcast Voice Enhance" }

        Select {
          value: enhance_val.clone(),
          on_value_change: move |val: String| enhance_preset.set(val),
          SelectTrigger {
            class: "h-8 px-2 py-1 text-xs bg-secondary",
            "aria-label": "Voice enhance preset",
            span { class: "truncate", "{enhance_preset_label(&enhance_val)}" }
          }
          SelectContent {
            SelectItem { value: "podcast_studio", class: "text-xs", "Podcast Studio (Balanced Denoise + EQ)" }
            SelectItem { value: "noisy_room", class: "text-xs", "Noisy Room (Strong STFT Denoise)" }
            SelectItem { value: "clean_mic", class: "text-xs", "Clean Mic (Gentle Polish + Presence)" }
          }
        }

        div {
          class: "grid grid-cols-2 gap-1.5",
          Button {
            variant: ButtonVariant::Primary,
            size: ButtonSize::Xs,
            class: "font-semibold",
            onclick: move |_| {
              let preset_name = enhance_preset.read().clone();
              async move {
                let cfg = match preset_name.as_str() {
                  "noisy_room" => VoiceEnhanceConfig::noisy_room(),
                  "clean_mic" => VoiceEnhanceConfig::clean_mic(),
                  _ => VoiceEnhanceConfig::podcast_studio(),
                };
                transform_audio(
                  &mut state,
                  move |buf| {
                    if let Ok(enhanced) = enhance_voice(buf, &cfg) {
                      *buf = enhanced;
                    }
                  },
                  format!("Applied local Broadcast Voice Enhance ({preset_name})"),
                ).await;
              }
            },
            "Enhance Voice"
          }
          Button {
            variant: ButtonVariant::Outline,
            size: ButtonSize::Xs,
            class: "bg-secondary hover:border-info",
            title: "Detect and remove silent pauses longer than 0.45s with smooth 15ms cosine crossfades",
            onclick: move |_| async move {
              transform_audio(
                &mut state,
                |buf| {
                  if let Ok(regions) = detect_speech_regions(buf, -38.0, 0.45, 0.06)
                    && !regions.is_empty()
                    && let Ok(spliced) = splice_ranges(buf, &regions, 0.015)
                  {
                    *buf = spliced;
                  }
                },
                "Stripped long silences (15ms cosine crossfade)".to_string(),
              ).await;
            },
            "Strip Silence"
          }
        }
      }

      // Quick Broadcast LUFS Normalize
      Card {
        class: "bg-background p-2.5 flex flex-col gap-2 shadow-none",
        div {
          class: "flex items-center justify-between text-[11px]",
          Label { class: "text-[11px] font-normal text-muted-foreground", "BS.1770 LUFS Target" }
          span { class: "font-mono tabular-nums text-info", "{eff.lufs_target:.1} LUFS" }
        }
        Slider {
          value: f64::from(eff.lufs_target),
          min: -24.0,
          max: -12.0,
          step: 0.5,
          track_class: "h-1.5",
          range_class: "bg-info",
          thumb_class: "h-3.5 w-3.5 border-info",
          "aria-label": "BS.1770 LUFS target",
          on_value_change: move |v: f64| {
            state.effects.write().lufs_target = v as f32;
          },
        }
        Button {
          variant: ButtonVariant::Outline,
          size: ButtonSize::Xs,
          class: "w-full bg-secondary border-info/50 text-info hover:bg-info/15",
          onclick: move |_| async move {
            let target = (state.effects)().lufs_target;
            transform_audio(
              &mut state,
              move |buf| algorithms::normalize_lufs(buf, target, -1.0),
              format!("Normalized to {target:.1} LUFS (-1.0 dBTP ceiling)"),
            ).await;
          },
          "Normalize Loudness (LUFS)"
        }
      }

      Separator { decorative: true }

      SliderControl {
        label: "Peak Normalize",
        min: 0.1,
        max: 1.0,
        step: 0.05,
        value: f64::from(eff.normalize_target),
        display: format!("{:.2}", eff.normalize_target),
        onchange: move |v: f64| { state.effects.write().normalize_target = v as f32; },
      }

      SliderControl {
        label: "Fade In (s)",
        min: 0.0,
        max: 5.0,
        step: 0.1,
        value: eff.fade_in_secs,
        display: format!("{:.1}s", eff.fade_in_secs),
        onchange: move |v: f64| { state.effects.write().fade_in_secs = v; },
      }

      SliderControl {
        label: "Fade Out (s)",
        min: 0.0,
        max: 5.0,
        step: 0.1,
        value: eff.fade_out_secs,
        display: format!("{:.1}s", eff.fade_out_secs),
        onchange: move |v: f64| { state.effects.write().fade_out_secs = v; },
      }

      SliderControl {
        label: "Speed",
        min: 0.25,
        max: 4.0,
        step: 0.05,
        value: eff.speed_factor,
        display: format!("{:.2}x", eff.speed_factor),
        onchange: move |v: f64| { state.effects.write().speed_factor = v; },
      }

      div {
        class: "flex flex-col gap-1",
        Label { class: "text-[11px] font-normal text-muted-foreground", "Compressor" }
        Select {
          value: compress_val.clone(),
          on_value_change: move |val: String| {
            state.effects.write().compress_preset = val;
          },
          SelectTrigger {
            class: "h-8 px-2 py-1 text-xs bg-secondary",
            "aria-label": "Compressor preset",
            span { class: "truncate", "{compress_preset_label(&compress_val)}" }
          }
          SelectContent {
            SelectItem { value: "none", class: "text-xs", "None" }
            SelectItem { value: "voice", class: "text-xs", "Voice" }
            SelectItem { value: "gentle", class: "text-xs", "Gentle" }
            SelectItem { value: "limiter", class: "text-xs", "Limiter" }
          }
        }
      }

      Button {
        variant: ButtonVariant::Primary,
        size: ButtonSize::Sm,
        class: "w-full text-xs font-semibold mt-1",
        onclick: move |_| async move {
          apply_effects(&mut state).await;
        },
        "Apply Effects"
      }
    }
  }
}

async fn apply_effects(state: &mut AppState) {
  let eff = (state.effects)();
  let transform = move |buf: &mut AudioBuffer| {
    algorithms::normalize(buf, eff.normalize_target);

    if eff.fade_in_secs > 0.0 {
      algorithms::fade_in(buf, eff.fade_in_secs);
    }
    if eff.fade_out_secs > 0.0 {
      algorithms::fade_out(buf, eff.fade_out_secs);
    }
    if (eff.speed_factor - 1.0).abs() > 0.01
      && let Ok(b) = algorithms::change_speed(buf, eff.speed_factor)
    {
      *buf = b;
    }
    let preset = match eff.compress_preset.as_str() {
      "voice" => Some(algorithms::CompressorConfig::voice()),
      "gentle" => Some(algorithms::CompressorConfig::gentle()),
      "limiter" => Some(algorithms::CompressorConfig::limiter()),
      _ => None,
    };
    if let Some(config) = preset {
      *buf = algorithms::with_compression(buf, &config);
    }
  };
  transform_audio(state, transform, "Effects applied".to_string()).await;
}

#[component]
fn SliderControl(
  label: &'static str,
  min: f64,
  max: f64,
  step: f64,
  value: f64,
  display: String,
  onchange: EventHandler<f64>,
) -> Element {
  rsx! {
    div { class: "flex flex-col gap-1.5",
      div {
        class: "flex items-center justify-between text-[11px]",
        Label { class: "text-[11px] font-normal text-muted-foreground", "{label}" }
        span { class: "font-mono tabular-nums text-foreground", "{display}" }
      }
      Slider {
        value,
        min,
        max,
        step,
        track_class: "h-1.5",
        thumb_class: "h-3.5 w-3.5",
        "aria-label": label,
        on_value_change: move |v: f64| onchange.call(v),
      }
    }
  }
}
