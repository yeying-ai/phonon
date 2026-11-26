#![allow(non_snake_case)]

use crate::state::{AppState, VizMode};
use dioxus::prelude::*;

#[component]
pub fn WaveformView() -> Element {
  let state = use_context::<AppState>();
  let Some(info) = (state.display_info)() else {
    return rsx! {};
  };

  rsx! {
    div {
      class: "w-full max-w-5xl bg-[#1C1F24] border border-[#2D333C] rounded-md p-4",

      div {
        class: "flex gap-1.5 mb-3",
        button {
          class: if (state.viz_mode)() == VizMode::Waveform {
            "px-2.5 py-1 rounded text-xs font-medium bg-[#38B2AC] text-[#141619]"
          } else {
            "px-2.5 py-1 rounded text-xs font-medium bg-[#252A31] text-[#9AA4B2] hover:text-[#ECEFF4]"
          },
          onclick: {
            let mut viz = state.viz_mode;
            move |_| viz.set(VizMode::Waveform)
          },
          "Waveform"
        }
        button {
          class: if (state.viz_mode)() == VizMode::Spectrum {
            "px-2.5 py-1 rounded text-xs font-medium bg-[#E8A238] text-[#141619]"
          } else {
            "px-2.5 py-1 rounded text-xs font-medium bg-[#252A31] text-[#9AA4B2] hover:text-[#ECEFF4]"
          },
          onclick: {
            let mut viz = state.viz_mode;
            move |_| viz.set(VizMode::Spectrum)
          },
          "FFT Spectrum"
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
              class: "stroke-[#38B2AC] fill-[#38B2AC]/80",
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
