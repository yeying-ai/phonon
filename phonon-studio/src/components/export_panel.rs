#![allow(non_snake_case)]

use crate::state::AppState;
use crate::studio_ops::bake_plugins_into_buffer;
use dioxus::prelude::*;

#[component]
pub fn ExportPanel() -> Element {
  let mut state = use_context::<AppState>();
  let bake = (state.bake_plugins_on_export)();
  let plugin_count = state.active_plugins.read().len();

  rsx! {
    div {
      class: "flex flex-col gap-3",
      h3 { class: "text-xs font-semibold text-[#ECEFF4]", "Export Audio" }

      // Bake active plugins option
      label {
        class: "flex items-center gap-2 text-xs text-[#9AA4B2] cursor-pointer",
        input {
          r#type: "checkbox",
          class: "accent-[#38B2AC]",
          checked: bake,
          onchange: move |evt| state.bake_plugins_on_export.set(evt.checked()),
        }
        "Bake active plugin chain ({plugin_count})"
      }

      // MP3 Bitrate
      div {
        label { class: "text-[11px] text-[#9AA4B2] block mb-1", "MP3 Bitrate (kbps)" }
        select {
          class: "w-full bg-[#252A31] border border-[#2D333C] text-[#ECEFF4] rounded px-2 py-1.5 text-xs font-mono",
          value: "{(state.export_bitrate)()}",
          onchange: move |evt| {
            if let Ok(v) = evt.value().parse::<u32>() {
              state.export_bitrate.set(v);
            }
          },
          option { value: "128", "128 kbps" }
          option { value: "192", "192 kbps" }
          option { value: "256", "256 kbps" }
          option { value: "320", "320 kbps" }
        }
      }

      div { class: "flex flex-col gap-2 pt-1",
        ExportButton { format: "MP3", ext: "mp3", primary: true }
        ExportButton { format: "WAV", ext: "wav", primary: false }
        ExportButton { format: "FLAC", ext: "flac", primary: false }
        ExportButton { format: "OGG", ext: "ogg", primary: false }
        ExportButton { format: "AAC", ext: "aac", primary: false }
      }
    }
  }
}

#[component]
fn ExportButton(format: &'static str, ext: &'static str, primary: bool) -> Element {
  let mut state = use_context::<AppState>();
  rsx! {
    button {
      class: if primary {
        "w-full px-3 py-2 rounded text-xs font-semibold bg-[#E8A238] text-[#141619] hover:opacity-90 transition"
      } else {
        "w-full px-3 py-2 rounded text-xs font-medium bg-[#252A31] border border-[#2D333C] text-[#ECEFF4] hover:border-[#9AA4B2] transition"
      },
      onclick: move |_| async move {
        export_audio(&mut state, format, ext).await;
      },
      "Export as {format}"
    }
  }
}

async fn export_audio(state: &mut AppState, format: &str, ext: &str) {
  let Some(buf) = state.audio_buffer.read().clone() else {
    state.status_msg.set("No audio loaded".to_string());
    return;
  };
  let bitrate = (state.export_bitrate)();
  let bake_plugins = (state.bake_plugins_on_export)();
  let registry = (state.plugin_registry)();
  let specs = state.active_plugins.read().clone();

  let file_handle = rfd::AsyncFileDialog::new()
    .add_filter(format, &[ext])
    .set_file_name(format!("output.{ext}"))
    .save_file()
    .await;

  let Some(path) = file_handle else { return };
  let path_buf = path.path().to_path_buf();

  state.status_msg.set(format!("Exporting {format}..."));

  let ext_owned = ext.to_string();
  let result = tokio::task::spawn_blocking(move || {
    let to_write = if bake_plugins && !specs.is_empty() {
      bake_plugins_into_buffer(&buf, &registry, &specs)
    } else {
      phonon_core::AudioBuffer::clone(&buf)
    };
    match ext_owned.as_str() {
      "wav" => phonon_wav::write_wav(&path_buf, &to_write),
      "mp3" => {
        let config = phonon_io::Mp3Config {
          bitrate_kbps: bitrate,
        };
        phonon_io::write_mp3(&path_buf, &to_write, &config)
      }
      "flac" => phonon_io::write_flac(&path_buf, &to_write, &phonon_io::FlacConfig::default()),
      "ogg" => phonon_io::write_ogg(&path_buf, &to_write, &phonon_io::OggConfig::default()),
      "aac" => phonon_io::write_aac(&path_buf, &to_write, &phonon_io::AacConfig::default()),
      _ => Err(phonon_core::PhononError::InvalidParam(format!(
        "Unknown format: {ext_owned}"
      ))),
    }
  })
  .await;

  match result {
    Ok(Ok(())) => state
      .status_msg
      .set(format!("{format} exported successfully")),
    Ok(Err(e)) => state.status_msg.set(format!("Export error: {e}")),
    Err(e) => state.status_msg.set(format!("Task error: {e}")),
  }
}
