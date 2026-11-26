#![allow(non_snake_case)]

use crate::state::{AppState, transform_audio};
use dioxus::prelude::*;
use dioxus_shadcn::{
  Button, ButtonSize, ButtonVariant, Label, NativeSelect, NativeSelectOption, NativeSelectSize,
  Slider,
};
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
  let preset_val = (state.effects)().eq_preset;

  rsx! {
    div {
      class: "flex flex-col gap-3",
      h3 { class: "text-xs font-semibold text-foreground", "5-Band Parametric EQ" }

      div {
        class: "flex flex-col gap-1",
        Label { class: "text-[11px] font-normal text-muted-foreground", "Preset" }
        NativeSelect {
          size: NativeSelectSize::Sm,
          class: "bg-secondary",
          value: "{preset_val}",
          on_value_change: move |preset: String| {
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
          NativeSelectOption { value: "none", selected: preset_val == "none", "Flat" }
          NativeSelectOption { value: "voice", selected: preset_val == "voice", "Voice Presence" }
          NativeSelectOption { value: "bass_heavy", selected: preset_val == "bass_heavy", "Warm Bass" }
          NativeSelectOption { value: "bright", selected: preset_val == "bright", "Air & Detail" }
          NativeSelectOption { value: "custom", selected: preset_val == "custom", "Custom" }
        }
      }

      div { class: "flex flex-col gap-3",
        for (idx, (freq, label)) in BAND_INFO.iter().enumerate() {
          {
            let gain = (state.effects)().eq_gains[idx];
            let freq_label = format!("{label} ({freq:.0} Hz)");
            rsx! {
              div { class: "flex items-center gap-2",
                span { class: "text-[11px] text-muted-foreground w-24 shrink-0", "{freq_label}" }
                Slider {
                  value: gain,
                  min: -12.0,
                  max: 12.0,
                  step: 0.5,
                  class: "flex-1",
                  track_class: "h-1.5",
                  range_class: "bg-info",
                  thumb_class: "h-3.5 w-3.5 border-info",
                  "aria-label": "{freq_label} gain",
                  on_value_change: move |v: f64| {
                    let mut eff = state.effects.write();
                    eff.eq_gains[idx] = v;
                    eff.eq_preset = "custom".to_string();
                  },
                }
                span {
                  class: "text-[11px] font-mono tabular-nums text-foreground w-12 text-right",
                  "{gain:+.1} dB"
                }
              }
            }
          }
        }
      }

      div { class: "flex gap-2 pt-1",
        Button {
          variant: ButtonVariant::Outline,
          size: ButtonSize::Xs,
          class: "flex-1 bg-secondary text-muted-foreground hover:text-foreground",
          onclick: move |_| {
            let mut eff = state.effects.write();
            eff.eq_gains = [0.0; 5];
            eff.eq_preset = "none".to_string();
          },
          "Reset"
        }
        Button {
          variant: ButtonVariant::Primary,
          size: ButtonSize::Xs,
          class: "flex-1 font-semibold",
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
