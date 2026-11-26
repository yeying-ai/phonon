#![allow(non_snake_case)]

use crate::components::{
  effects_panel::EffectsPanel, eq_panel::EqPanel, export_panel::ExportPanel, info_panel::InfoPanel,
  plugin_browser::PluginBrowser, waveform::WaveformView,
};
use crate::state::{AppState, SidePanel};
use dioxus::prelude::*;

#[component]
pub fn AudioLabView() -> Element {
  let state = use_context::<AppState>();

  rsx! {
    div {
      class: "flex-1 flex min-h-0 overflow-hidden bg-[#141619]",

      // Main waveform / spectrum + telemetry area
      main {
        class: "flex-1 flex flex-col items-center justify-start p-6 overflow-y-auto",

        if (state.display_info)().is_some() {
          WaveformView {}
          InfoPanel {}
        } else {
          div {
            class: "text-center text-[#9AA4B2] mt-20 max-w-md flex flex-col gap-2",
            h2 { class: "text-lg font-semibold text-[#ECEFF4]", "Audio Lab" }
            p {
              class: "text-xs leading-relaxed",
              "Inspect waveforms, FFT spectra, BS.1770-4 loudness, 5-band parametric EQ, and real-time DSP plugins. Click Open Audio or drag & drop an audio file here."
            }
          }
        }
      }

      // Side DSP / Export drawer
      if (state.side_panel)() != SidePanel::None {
        aside {
          class: "w-80 shrink-0 bg-[#1C1F24] border-l border-[#2D333C] p-4 overflow-y-auto flex flex-col gap-4",

          match (state.side_panel)() {
            SidePanel::Effects => rsx! { EffectsPanel {} },
            SidePanel::Eq => rsx! { EqPanel {} },
            SidePanel::Export => rsx! { ExportPanel {} },
            SidePanel::Plugins => rsx! { PluginBrowser {} },
            SidePanel::None => rsx! {},
          }
        }
      }
    }
  }
}
