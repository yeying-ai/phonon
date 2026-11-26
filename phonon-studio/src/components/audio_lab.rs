#![allow(non_snake_case)]

use crate::components::{
  effects_panel::EffectsPanel, eq_panel::EqPanel, export_panel::ExportPanel, info_panel::InfoPanel,
  plugin_browser::PluginBrowser, waveform::WaveformView,
};
use crate::state::{AppState, SidePanel};
use dioxus::prelude::*;
use dioxus_shadcn::{
  Empty, EmptyDescription, EmptyHeader, EmptyTitle, LayoutOrientation, ResizableHandle,
  ResizablePanel, ResizablePanelGroup, ResizablePanelState, resizable_resize_pair,
};

#[component]
pub fn AudioLabView() -> Element {
  let state = use_context::<AppState>();
  let mut main_pct = use_signal(|| 72.0_f64);
  let side_panel = (state.side_panel)();

  let main_content = rsx! {
    main {
      class: "h-full w-full flex flex-col items-center justify-start p-6 overflow-y-auto",

      if (state.display_info)().is_some() {
        WaveformView {}
        InfoPanel {}
      } else {
        Empty {
          class: "mt-20 max-w-md bg-card/50",
          EmptyHeader {
            EmptyTitle { class: "text-base", "Audio Lab" }
            EmptyDescription {
              class: "text-xs leading-relaxed",
              "Inspect waveforms, FFT spectra, BS.1770-4 loudness, 5-band parametric EQ, and real-time DSP plugins. Click Open Audio, Record, or drag & drop an audio file here."
            }
          }
        }
      }
    }
  };

  if side_panel == SidePanel::None {
    return rsx! {
      div {
        class: "flex-1 flex min-h-0 overflow-hidden bg-background",
        {main_content}
      }
    };
  }

  let main_size = main_pct();
  let side_size = (100.0 - main_size).clamp(20.0, 48.0);

  rsx! {
    div {
      class: "flex-1 flex min-h-0 overflow-hidden bg-background",
      ResizablePanelGroup {
        orientation: LayoutOrientation::Horizontal,
        class: "flex-1 min-h-0",

        ResizablePanel {
          size: main_size,
          min_size: 52.0,
          max_size: 80.0,
          class: "min-h-0",
          {main_content}
        }

        ResizableHandle {
          orientation: LayoutOrientation::Horizontal,
          value: main_size,
          min: 52.0,
          max: 80.0,
          step: 2.0,
          "aria-label": "Resize Audio Lab inspector drawer",
          on_resize: move |delta: f64| {
            let (next_main, _) = resizable_resize_pair(
              ResizablePanelState::new(main_pct(), 52.0, 80.0),
              ResizablePanelState::new(100.0 - main_pct(), 20.0, 48.0),
              delta,
            );
            main_pct.set(next_main.size);
          },
        }

        ResizablePanel {
          size: side_size,
          min_size: 20.0,
          max_size: 48.0,
          class: "min-h-0 bg-card",
          aside {
            class: "h-full p-4 overflow-y-auto flex flex-col gap-4",

            match side_panel {
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
}
