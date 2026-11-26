#![allow(non_snake_case)]

use crate::project::format_timecode;
use crate::state::{AppState, WorkspaceTab, play_buffer};
use dioxus::prelude::*;
use phonon_engine::PlaybackState;
use std::sync::Arc;
use std::time::Duration;

#[component]
pub fn Transport() -> Element {
  let mut state = use_context::<AppState>();

  // Poll live playhead position & playback state while active.
  use_future(move || async move {
    loop {
      tokio::time::sleep(Duration::from_millis(120)).await;
      let snapshot = {
        let reader = state.player_state.read();
        reader.as_ref().map(|p| {
          (
            p.playback_state(),
            p.position_secs(),
            p.duration_secs(),
            p.is_finished(),
          )
        })
      };
      if let Some((pb_state, pos, dur, finished)) = snapshot {
        if (state.playback_status)() != pb_state {
          state.playback_status.set(pb_state);
        }
        if pb_state == PlaybackState::Playing {
          state.playhead_secs.set(pos);
          if dur > 0.0 && (dur - (state.playhead_duration_secs)()).abs() > 0.05 {
            state.playhead_duration_secs.set(dur);
          }
        } else if finished && (state.playhead_secs)() > 0.0 {
          state.playhead_secs.set(0.0);
        }
      }
    }
  });

  let pos = (state.playhead_secs)();
  let dur = (state.playhead_duration_secs)();
  let pb_status = (state.playback_status)();
  let vol = (state.volume)();
  let source = (state.playback_source)();
  let status_msg = (state.status_msg)();

  let time_display = format!("{} / {}", format_timecode(pos), format_timecode(dur));
  let vol_pct = format!("{:.0}%", vol * 100.0);
  let seek_max = dur.max(0.01);

  rsx! {
    footer {
      class: "h-14 shrink-0 bg-[#1C1F24] px-4 border-t border-[#2D333C] flex items-center justify-between gap-4 select-none",

      // Left: Transport buttons
      div {
        class: "flex items-center gap-1.5 shrink-0",
        button {
          class: if pb_status == PlaybackState::Playing {
            "px-3 py-1.5 rounded text-xs font-semibold bg-[#E8A238] text-[#141619]"
          } else {
            "px-3 py-1.5 rounded text-xs font-semibold bg-[#252A31] text-[#ECEFF4] hover:bg-[#E8A238] hover:text-[#141619] transition"
          },
          title: "Play / Resume",
          onclick: move |_| async move {
            handle_play_click(&mut state).await;
          },
          "▶"
        }
        button {
          class: if pb_status == PlaybackState::Paused {
            "px-2.5 py-1.5 rounded text-xs bg-[#E8A238]/25 text-[#E8A238] border border-[#E8A238]/40"
          } else {
            "px-2.5 py-1.5 rounded text-xs bg-[#252A31] text-[#ECEFF4] hover:bg-[#2D333C]"
          },
          title: "Pause",
          onclick: move |_| {
            if let Some(player) = state.player_state.read().as_ref() {
              player.pause();
              state.playback_status.set(PlaybackState::Paused);
              state.status_msg.set("Paused".to_string());
            }
          },
          "⏸"
        }
        button {
          class: "px-2.5 py-1.5 rounded text-xs bg-[#252A31] text-[#9AA4B2] hover:text-[#E05252] hover:bg-[#2D333C]",
          title: "Stop",
          onclick: move |_| {
            if let Some(player) = state.player_state.read().as_ref() {
              player.stop();
              state.playback_status.set(PlaybackState::Stopped);
              state.playhead_secs.set(0.0);
              state.status_msg.set("Stopped".to_string());
            }
          },
          "⏹"
        }
      }

      // Center: Interactive Seek bar + Timecode + Source label
      div {
        class: "flex-1 min-w-0 max-w-2xl flex items-center gap-3",
        span {
          class: "hidden lg:inline-block text-[11px] text-[#9AA4B2] truncate max-w-[160px] shrink-0",
          title: "{source}",
          "{source}"
        }
        input {
          r#type: "range",
          min: "0",
          max: "{seek_max}",
          step: "0.05",
          value: "{pos.clamp(0.0, seek_max)}",
          disabled: dur <= 0.0,
          class: "flex-1 accent-[#E8A238] disabled:opacity-40",
          oninput: move |evt| {
            if let Ok(target) = evt.value().parse::<f64>() {
              state.playhead_secs.set(target);
              if let Some(player) = state.player_state.read().as_ref() {
                let _ = player.seek(target);
              }
            }
          },
        }
        span {
          class: "text-xs font-mono tabular-nums text-[#ECEFF4] shrink-0 w-32 text-right",
          "{time_display}"
        }
      }

      // Right: Live Volume + Status
      div {
        class: "flex items-center gap-3 shrink-0",
        div {
          class: "flex items-center gap-1.5",
          span { class: "text-[11px] text-[#9AA4B2]", "Vol" }
          input {
            r#type: "range",
            min: "0",
            max: "1.5",
            step: "0.01",
            value: "{vol}",
            class: "w-20 accent-[#38B2AC]",
            oninput: move |evt| {
              if let Ok(v) = evt.value().parse::<f32>() {
                state.volume.set(v);
                if let Some(player) = state.player_state.read().as_ref() {
                  player.set_volume(v);
                }
              }
            },
          }
          span { class: "text-[11px] font-mono tabular-nums text-[#9AA4B2] w-9 text-right", "{vol_pct}" }
        }

        div {
          class: "hidden xl:block text-xs text-[#9AA4B2] max-w-[260px] truncate border-l border-[#2D333C] pl-3",
          title: "{status_msg}",
          "{status_msg}"
        }
      }
    }
  }
}

async fn handle_play_click(state: &mut AppState) {
  // Resume ONLY when the player is paused on the exact same active track token.
  let can_resume = {
    let reader = state.player_state.read();
    reader.as_ref().is_some_and(|p| {
      p.playback_state() == PlaybackState::Paused
        && (state.active_track_token)() == (state.loaded_track_token)()
        && (state.loaded_track_token)() != 0
    })
  };
  if can_resume {
    if let Some(player) = state.player_state.read().as_ref() {
      player.resume();
    }
    state.playback_status.set(PlaybackState::Playing);
    state.status_msg.set("Resumed".to_string());
    return;
  }

  match (state.active_tab)() {
    WorkspaceTab::AudioLab => {
      let Some(buf) = state.audio_buffer.read().clone() else {
        state
          .status_msg
          .set("Open an audio file in Audio Lab first".to_string());
        return;
      };
      let name = state
        .display_info
        .read()
        .as_ref()
        .map(|i| {
          std::path::Path::new(&i.path)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| i.path.clone())
        })
        .unwrap_or_else(|| "Audio Lab".to_string());
      play_buffer(state, buf, name, true, None).await;
    }
    WorkspaceTab::PodcastStudio => {
      let ep = state.rendered_episode.read().clone();
      if let Some(ep) = ep {
        play_buffer(
          state,
          Arc::new(ep.audio.clone()),
          "Assembled Episode".to_string(),
          false,
          None,
        )
        .await;
      } else {
        state
          .status_msg
          .set("Click Preview on a segment or Assemble Episode first".to_string());
      }
    }
  }
}
