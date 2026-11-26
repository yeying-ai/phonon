#![allow(non_snake_case)]

use crate::state::{AppState, VizMode};
use dioxus::prelude::*;
use dioxus_shadcn::{Card, Tabs, TabsList, TabsTrigger};

#[component]
pub fn WaveformView() -> Element {
  let mut state = use_context::<AppState>();
  let Some(info) = (state.display_info)() else {
    return rsx! {};
  };
  let viz_key = match (state.viz_mode)() {
    VizMode::Waveform => "waveform",
    VizMode::Spectrum => "spectrum",
  };

  rsx! {
    Card {
      class: "w-full max-w-5xl p-4 shadow-none",

      div {
        class: "mb-3",
        Tabs {
          value: viz_key.to_string(),
          on_value_change: move |next: String| {
            match next.as_str() {
              "spectrum" => state.viz_mode.set(VizMode::Spectrum),
              _ => state.viz_mode.set(VizMode::Waveform),
            }
          },
          TabsList {
            class: "h-8 p-0.5 bg-background border border-border",
            "aria-label": "Visualization mode",
            TabsTrigger {
              value: "waveform",
              class: "h-7 px-2.5 py-1 text-xs",
              "Waveform"
            }
            TabsTrigger {
              value: "spectrum",
              class: "h-7 px-2.5 py-1 text-xs",
              "FFT Spectrum"
            }
          }
        }
      }

      match (state.viz_mode)() {
        VizMode::Waveform => rsx! {
          svg {
            width: "100%",
            height: "200",
            view_box: "0 0 1000 200",
            preserve_aspect_ratio: "none",
            path {
              d: "{info.waveform_path}",
              class: "stroke-info fill-info/80",
              stroke_width: "1",
              vector_effect: "non-scaling-stroke"
            }
          }
        },
        VizMode::Spectrum => rsx! {
          super::spectrum::SpectrumView { bars: info.spectrum_bars.clone() }
        },
      }
    }
  }
}
