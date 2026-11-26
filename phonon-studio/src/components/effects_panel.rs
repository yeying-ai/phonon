#![allow(non_snake_case)]

use crate::state::{AppState, transform_audio};
use dioxus::prelude::*;
use phonon_core::{AudioBuffer, algorithms};

#[component]
pub fn EffectsPanel() -> Element {
  let mut state = use_context::<AppState>();
  let eff = (state.effects)();

  rsx! {
    div {
      class: "flex flex-col gap-3",
      h3 { class: "text-xs font-semibold text-[#ECEFF4]", "Offline DSP & Loudness" }

      // Quick Broadcast LUFS Normalize
      div {
        class: "bg-[#141619] border border-[#2D333C] rounded p-2.5 flex flex-col gap-2",
        div {
          class: "flex items-center justify-between text-[11px]",
          span { class: "text-[#9AA4B2]", "BS.1770 LUFS Target" }
          span { class: "font-mono tabular-nums text-[#38B2AC]", "{eff.lufs_target:.1} LUFS" }
        }
        input {
          r#type: "range",
          min: "-24",
          max: "-12",
          step: "0.5",
          value: "{eff.lufs_target}",
          class: "w-full accent-[#38B2AC]",
          oninput: move |evt| {
            if let Ok(v) = evt.value().parse::<f32>() {
              state.effects.write().lufs_target = v;
            }
          },
        }
        button {
          class: "w-full px-2.5 py-1.5 rounded text-xs font-medium bg-[#252A31] border border-[#38B2AC]/50 text-[#38B2AC] hover:bg-[#38B2AC]/15 transition",
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

      // Normalize Peak
      SliderControl {
        label: "Peak Normalize",
        min: "0.1",
        max: "1.0",
        step: "0.05",
        value: format!("{}", eff.normalize_target),
        display: format!("{:.2}", eff.normalize_target),
        onchange: move |v: f32| { state.effects.write().normalize_target = v; },
      }

      SliderControl {
        label: "Fade In (s)",
        min: "0",
        max: "5",
        step: "0.1",
        value: format!("{}", eff.fade_in_secs),
        display: format!("{:.1}s", eff.fade_in_secs),
        onchange: move |v: f32| { state.effects.write().fade_in_secs = v as f64; },
      }

      SliderControl {
        label: "Fade Out (s)",
        min: "0",
        max: "5",
        step: "0.1",
        value: format!("{}", eff.fade_out_secs),
        display: format!("{:.1}s", eff.fade_out_secs),
        onchange: move |v: f32| { state.effects.write().fade_out_secs = v as f64; },
      }

      SliderControl {
        label: "Speed",
        min: "0.25",
        max: "4.0",
        step: "0.05",
        value: format!("{}", eff.speed_factor),
        display: format!("{:.2}x", eff.speed_factor),
        onchange: move |v: f32| { state.effects.write().speed_factor = v as f64; },
      }

      div {
        label { class: "text-[11px] text-[#9AA4B2] block mb-1", "Compressor" }
        select {
          class: "w-full bg-[#252A31] border border-[#2D333C] text-[#ECEFF4] rounded px-2 py-1.5 text-xs",
          value: "{eff.compress_preset}",
          onchange: move |evt| {
            state.effects.write().compress_preset = evt.value();
          },
          option { value: "none", "None" }
          option { value: "voice", "Voice" }
          option { value: "gentle", "Gentle" }
          option { value: "limiter", "Limiter" }
        }
      }

      button {
        class: "w-full px-3 py-2 rounded text-xs font-semibold bg-[#E8A238] text-[#141619] hover:opacity-90 transition mt-1",
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
  min: &'static str,
  max: &'static str,
  step: &'static str,
  value: String,
  display: String,
  onchange: EventHandler<f32>,
) -> Element {
  rsx! {
    div { class: "flex flex-col gap-1",
      div {
        class: "flex items-center justify-between text-[11px]",
        label { class: "text-[#9AA4B2]", "{label}" }
        span { class: "font-mono tabular-nums text-[#ECEFF4]", "{display}" }
      }
      input {
        r#type: "range",
        min: "{min}",
        max: "{max}",
        step: "{step}",
        value: "{value}",
        class: "w-full accent-[#E8A238]",
        oninput: move |evt| {
          if let Ok(v) = evt.value().parse::<f32>() {
            onchange.call(v);
          }
        },
      }
    }
  }
}
