#![allow(non_snake_case)]

use crate::state::AppState;
use crate::studio_ops::bake_plugins_into_buffer;
use dioxus::prelude::*;
use dioxus_shadcn::{
  Button, ButtonSize, ButtonVariant, Label, NativeSelect, NativeSelectOption, Switch, SwitchSize,
};

#[component]
pub fn ExportPanel() -> Element {
  let mut state = use_context::<AppState>();
  let bake = (state.bake_plugins_on_export)();
  let plugin_count = state.active_plugins.read().len();
  let bitrate = (state.export_bitrate)();

  rsx! {
    div {
      class: "flex flex-col gap-3",
      h3 { class: "text-xs font-semibold text-foreground", "Export Audio" }

      // Bake active plugins option (dioxus-shadcn Switch)
      div {
        class: "flex items-center justify-between gap-2 py-0.5",
        Label {
          r#for: "audiolab-bake-plugins",
          class: "text-xs font-normal text-muted-foreground cursor-pointer",
          "Bake active plugin chain ({plugin_count})"
        }
        Switch {
          id: "audiolab-bake-plugins",
          size: SwitchSize::Sm,
          checked: bake,
          on_checked_change: move |checked: bool| state.bake_plugins_on_export.set(checked),
        }
      }

      // MP3 Bitrate
      div {
        class: "flex flex-col gap-1",
        Label { class: "text-[11px] font-normal text-muted-foreground", "MP3 Bitrate (kbps)" }
        NativeSelect {
          class: "h-8 px-2 py-1 text-xs font-mono bg-secondary",
          value: "{bitrate}",
          on_value_change: move |val: String| {
            if let Ok(v) = val.parse::<u32>() {
              state.export_bitrate.set(v);
            }
          },
          NativeSelectOption { value: "128", selected: bitrate == 128, "128 kbps" }
          NativeSelectOption { value: "192", selected: bitrate == 192, "192 kbps" }
          NativeSelectOption { value: "256", selected: bitrate == 256, "256 kbps" }
          NativeSelectOption { value: "320", selected: bitrate == 320, "320 kbps" }
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
    Button {
      variant: if primary { ButtonVariant::Primary } else { ButtonVariant::Outline },
      size: ButtonSize::Sm,
      class: if primary {
        "w-full text-xs font-semibold".to_string()
      } else {
        "w-full text-xs bg-secondary hover:border-muted-foreground".to_string()
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
