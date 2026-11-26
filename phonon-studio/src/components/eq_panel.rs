#![allow(non_snake_case)]

use crate::state::{AppState, transform_audio};
use dioxus::prelude::*;
use phonon_core::{EqBand, FilterType, algorithms};

const BAND_INFO: [(f64, &str); 5] = [
  (60.0, "Sub"),
  (250.0, "Low"),
  (1000.0, "Mid"),
  (4000.0, "Hi-Mid"),
  (12000.0, "High"),
];

#[component]
pub fn EqPanel() -> Element {
  let mut state = use_context::<AppState>();

  rsx! {
    div {
      class: "flex flex-col gap-3",
      h3 { class: "text-xs font-semibold text-[#ECEFF4]", "5-Band Parametric EQ" }

      div {
        label { class: "text-[11px] text-[#9AA4B2] block mb-1", "Preset" }
        select {
          class: "w-full bg-[#252A31] border border-[#2D333C] text-[#ECEFF4] rounded px-2 py-1.5 text-xs",
          value: "{(state.effects)().eq_preset}",
          onchange: move |evt| {
            let preset = evt.value();
            let gains = match preset.as_str() {
              "voice" => [0.0, 0.0, 3.0, 2.0, -3.0],
              "bass_heavy" => [8.0, 5.0, 0.0, -1.0, -3.0],
              "bright" => [-2.0, -1.0, 0.0, 3.0, 5.0],
              _ => [0.0; 5],
            };
            let mut eff = state.effects.write();
            eff.eq_preset = preset;
            eff.eq_gains = gains;
          },
          option { value: "none", "Flat" }
          option { value: "voice", "Voice Presence" }
          option { value: "bass_heavy", "Warm Bass" }
          option { value: "bright", "Air & Detail" }
          option { value: "custom", "Custom" }
        }
      }

      div { class: "flex flex-col gap-2.5",
        for (idx, (freq, label)) in BAND_INFO.iter().enumerate() {
          {
            let gain = (state.effects)().eq_gains[idx];
            let freq_label = format!("{label} ({freq:.0} Hz)");
            rsx! {
              div { class: "flex items-center gap-2",
                span { class: "text-[11px] text-[#9AA4B2] w-24 shrink-0", "{freq_label}" }
                input {
                  r#type: "range",
                  min: "-12",
                  max: "12",
                  step: "0.5",
                  value: "{gain}",
                  class: "flex-1 accent-[#38B2AC]",
                  oninput: move |evt| {
                    if let Ok(v) = evt.value().parse::<f64>() {
                      let mut eff = state.effects.write();
                      eff.eq_gains[idx] = v;
                      eff.eq_preset = "custom".to_string();
                    }
                  },
                }
                span { class: "text-[11px] font-mono tabular-nums text-[#ECEFF4] w-12 text-right", "{gain:+.1} dB" }
              }
            }
          }
        }
      }

      div { class: "flex gap-2 pt-1",
        button {
          class: "flex-1 px-3 py-1.5 rounded text-xs bg-[#252A31] border border-[#2D333C] text-[#9AA4B2] hover:text-[#ECEFF4]",
          onclick: move |_| {
            let mut eff = state.effects.write();
            eff.eq_gains = [0.0; 5];
            eff.eq_preset = "none".to_string();
          },
          "Reset"
        }
        button {
          class: "flex-1 px-3 py-1.5 rounded text-xs font-semibold bg-[#E8A238] text-[#141619] hover:opacity-90",
          onclick: move |_| async move {
            apply_eq(&mut state).await;
          },
          "Apply EQ"
        }
      }
    }
  }
}

async fn apply_eq(state: &mut AppState) {
  let eff = (state.effects)();
  let bands: Vec<EqBand> = BAND_INFO
    .iter()
    .zip(eff.eq_gains.iter())
    .filter(|(_, gain)| gain.abs() > 0.1)
    .map(|((freq, _), gain)| {
      let filter_type = if *freq <= 80.0 {
        FilterType::LowShelf
      } else if *freq >= 10000.0 {
        FilterType::HighShelf
      } else {
        FilterType::Peaking
      };
      EqBand::new(filter_type, *freq, 1.0, *gain)
    })
    .collect();

  if bands.is_empty() {
    state
      .status_msg
      .set("EQ: all bands flat, nothing to apply".to_string());
    return;
  }

  let done = format!("EQ applied ({} bands)", bands.len());
  transform_audio(state, move |buf| algorithms::apply_eq(buf, &bands), done).await;
}
