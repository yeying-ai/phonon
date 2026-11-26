#![allow(non_snake_case)]

use crate::project::format_timecode;
use crate::state::{AppState, WorkspaceTab, play_buffer};
use dioxus::prelude::*;
use dioxus_shadcn::{
  ButtonSize, ButtonVariant, Separator, SeparatorOrientation, Slider, Tooltip, TooltipContent,
  TooltipSide, TooltipTrigger, button_class, use_density,
};
use phonon_engine::PlaybackState;
use std::sync::Arc;
use std::time::Duration;

#[component]
pub fn Transport() -> Element {
  let mut state = use_context::<AppState>();
  let density = use_density();

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

  let play_btn_class = if pb_status == PlaybackState::Playing {
    button_class(
      ButtonVariant::Primary,
      ButtonSize::Xs,
      density,
      "px-3 font-semibold",
    )
  } else {
    button_class(
      ButtonVariant::Secondary,
      ButtonSize::Xs,
      density,
      "px-3 font-semibold hover:bg-primary hover:text-primary-foreground",
    )
  };

  let pause_btn_class = if pb_status == PlaybackState::Paused {
    button_class(
      ButtonVariant::Soft,
      ButtonSize::Xs,
      density,
      "border border-primary/40 text-primary",
    )
  } else {
    button_class(ButtonVariant::Secondary, ButtonSize::Xs, density, "")
  };

  let stop_btn_class = button_class(
    ButtonVariant::Secondary,
    ButtonSize::Xs,
    density,
    "text-muted-foreground hover:text-destructive",
  );

  rsx! {
    footer {
      class: "h-14 shrink-0 bg-card px-4 border-t border-border flex items-center justify-between gap-4 select-none",

      // Left: Transport buttons
      div {
        class: "flex items-center gap-1.5 shrink-0",

        Tooltip {
          delay_ms: 400,
          TooltipTrigger {
            class: play_btn_class,
            "aria-label": "Play or resume",
            onclick: move |_| async move {
              handle_play_click(&mut state).await;
            },
            "▶"
          }
          TooltipContent {
            side: TooltipSide::Top,
            "Play / Resume"
          }
        }

        Tooltip {
          delay_ms: 400,
          TooltipTrigger {
            class: pause_btn_class,
            "aria-label": "Pause playback",
            onclick: move |_| {
              if let Some(player) = state.player_state.read().as_ref() {
                player.pause();
                state.playback_status.set(PlaybackState::Paused);
                state.status_msg.set("Paused".to_string());
              }
            },
            "⏸"
          }
          TooltipContent {
            side: TooltipSide::Top,
            "Pause"
          }
        }

        Tooltip {
          delay_ms: 400,
          TooltipTrigger {
            class: stop_btn_class,
            "aria-label": "Stop playback",
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
          TooltipContent {
            side: TooltipSide::Top,
            "Stop"
          }
        }
      }

      // Center: Interactive Seek Slider + Timecode + Source label
      div {
        class: "flex-1 min-w-0 max-w-2xl flex items-center gap-3",
        span {
          class: "hidden lg:inline-block text-[11px] text-muted-foreground truncate max-w-[160px] shrink-0",
          title: "{source}",
          "{source}"
        }
        Slider {
          value: pos.clamp(0.0, seek_max),
          min: 0.0,
          max: seek_max,
          step: 0.05,
          disabled: dur <= 0.0,
          class: "flex-1",
          track_class: "h-1.5",
          thumb_class: "h-3.5 w-3.5",
          "aria-label": "Playback seek position",
          on_value_change: move |target: f64| {
            state.playhead_secs.set(target);
            if let Some(player) = state.player_state.read().as_ref() {
              let _ = player.seek(target);
            }
          },
        }
        span {
          class: "text-xs font-mono tabular-nums text-foreground shrink-0 w-32 text-right",
          "{time_display}"
        }
      }

      // Right: Live Volume Slider + Status
      div {
        class: "flex items-center gap-3 shrink-0",
        div {
          class: "flex items-center gap-2",
          span { class: "text-[11px] text-muted-foreground", "Vol" }
          Slider {
            value: f64::from(vol),
            min: 0.0,
            max: 1.5,
            step: 0.01,
            class: "w-20",
            track_class: "h-1.5",
            range_class: "bg-info",
            thumb_class: "h-3.5 w-3.5 border-info",
            "aria-label": "Playback volume",
            on_value_change: move |v: f64| {
              let vol_f32 = v as f32;
              state.volume.set(vol_f32);
              if let Some(player) = state.player_state.read().as_ref() {
                player.set_volume(vol_f32);
              }
            },
          }
          span {
            class: "text-[11px] font-mono tabular-nums text-muted-foreground w-9 text-right",
            "{vol_pct}"
          }
        }

        Separator {
          orientation: SeparatorOrientation::Vertical,
          decorative: true,
          class: "hidden xl:block h-5",
        }

        div {
          class: "hidden xl:block text-xs text-muted-foreground max-w-[260px] truncate",
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
