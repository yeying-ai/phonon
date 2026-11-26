#![allow(non_snake_case)]

use crate::project::{format_db, format_timecode};
use crate::state::AppState;
use dioxus::prelude::*;

#[component]
pub fn InfoPanel() -> Element {
  let state = use_context::<AppState>();
  let Some(info) = (state.display_info)() else {
    return rsx! {};
  };

  rsx! {
    div {
      class: "bg-[#1C1F24] p-4 rounded-md border border-[#2D333C] w-full max-w-5xl mt-4",
      div {
        class: "grid grid-cols-2 md:grid-cols-5 gap-4",
        InfoItem { label: "File", value: info.path.clone(), mono: false }
        InfoItem { label: "Sample Rate", value: format!("{} Hz", info.sample_rate), mono: true }
        InfoItem { label: "Channels", value: format!("{}", info.channels), mono: true }
        InfoItem { label: "Duration", value: format_timecode(info.duration), mono: true }
        InfoItem { label: "Samples", value: format!("{}", info.sample_count), mono: true }
        InfoItem { label: "Integrated LUFS", value: format_db(info.integrated_lufs, "LUFS"), mono: true }
        InfoItem { label: "True Peak", value: format_db(info.true_peak_dbtp, "dBTP"), mono: true }
        InfoItem { label: "Peak dBFS", value: format_db(info.peak_dbfs, "dBFS"), mono: true }
        InfoItem { label: "RMS", value: format!("{:.4}", info.rms), mono: true }
        InfoItem { label: "Peak", value: format!("{:.4}", info.peak), mono: true }
      }
    }
  }
}

#[component]
fn InfoItem(label: &'static str, value: String, mono: bool) -> Element {
  rsx! {
    div {
      class: "flex flex-col gap-0.5 min-w-0",
      label { class: "text-[11px] text-[#9AA4B2]", "{label}" }
      span {
        class: if mono {
          "text-xs font-mono tabular-nums font-medium text-[#ECEFF4] truncate"
        } else {
          "text-xs font-medium text-[#ECEFF4] truncate"
        },
        title: "{value}",
        "{value}"
      }
    }
  }
}
