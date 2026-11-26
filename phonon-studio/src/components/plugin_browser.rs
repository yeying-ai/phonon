#![allow(non_snake_case)]

use crate::state::{ActivePluginState, AppState};
use crate::studio_ops::instantiate_active_plugins;
use dioxus::prelude::*;

/// Plugin browser sidebar panel for Audio Lab.
///
/// Syncs parameter changes live to the active `AudioPlayer` effect chain and
/// ensures plugins added before pressing Play are preserved.
#[component]
pub fn PluginBrowser() -> Element {
  let mut state = use_context::<AppState>();

  let registry = (state.plugin_registry)();
  let mut available: Vec<(&str, String)> = registry
    .list_ids()
    .into_iter()
    .map(|id| {
      let name = registry
        .create(id)
        .map(|p| p.name().to_string())
        .unwrap_or_else(|| id.to_string());
      (id, name)
    })
    .collect();
  available.sort_by(|a, b| a.1.cmp(&b.1));

  let active_plugins = (state.active_plugins)();

  rsx! {
    div {
      class: "flex flex-col gap-4",

      // Available Plugins
      div {
        h3 { class: "text-xs font-semibold text-[#ECEFF4] mb-2.5", "Available DSP Plugins" }
        div { class: "flex flex-col gap-1.5",
          for (id, name) in available.iter() {
            div {
              class: "flex items-center justify-between bg-[#141619] border border-[#2D333C] rounded px-2.5 py-1.5",
              span { class: "text-xs text-[#ECEFF4]", "{name}" }
              button {
                class: "text-[11px] bg-[#252A31] border border-[#2D333C] text-[#ECEFF4] px-2 py-0.5 rounded hover:border-[#38B2AC]",
                onclick: {
                  let plugin_id = id.to_string();
                  move |_| add_plugin(&mut state, &plugin_id)
                },
                "+ Add"
              }
            }
          }
        }
      }

      div { class: "h-px bg-[#2D333C]" }

      // Active Chain
      div {
        h3 { class: "text-xs font-semibold text-[#ECEFF4] mb-2.5", "Active Chain ({active_plugins.len()})" }

        if active_plugins.is_empty() {
          p { class: "text-xs text-[#647080]", "No plugins in the chain. Add one above to process playback and export." }
        } else {
          div { class: "flex flex-col gap-2.5 mb-3",
            for (idx, plugin) in active_plugins.iter().enumerate() {
              div {
                key: "plugin-{idx}-{plugin.id}",
                class: "bg-[#141619] border border-[#2D333C] rounded p-2.5 flex flex-col gap-2",
                div {
                  class: "flex items-center justify-between",
                  span { class: "text-xs text-[#ECEFF4] font-medium", "{idx + 1}. {plugin.name}" }
                  button {
                    class: "text-[11px] text-[#647080] hover:text-[#E05252] px-1",
                    title: "Remove plugin",
                    onclick: move |_| remove_plugin(&mut state, idx),
                    "×"
                  }
                }
                for (pname, val, min, max, _default) in plugin.params.iter() {
                  div { class: "flex flex-col gap-1",
                    div {
                      class: "flex items-center justify-between text-[11px]",
                      label { class: "text-[#9AA4B2]", "{pname}" }
                      span { class: "font-mono tabular-nums text-[#ECEFF4]", "{val:.2}" }
                    }
                    input {
                      r#type: "range",
                      min: "{min}",
                      max: "{max}",
                      step: "0.01",
                      value: "{val}",
                      class: "w-full accent-[#38B2AC]",
                      oninput: {
                        let param_name = pname.clone();
                        move |evt| {
                          if let Ok(v) = evt.value().parse::<f32>() {
                            update_plugin_param(&mut state, idx, &param_name, v);
                          }
                        }
                      },
                    }
                  }
                }
              }
            }
          }

          button {
            class: "w-full px-3 py-1.5 rounded text-xs font-medium bg-[#252A31] border border-[#E05252]/40 text-[#E05252] hover:bg-[#E05252]/15 transition",
            onclick: move |_| clear_all_plugins(&mut state),
            "Clear Chain"
          }
        }
      }
    }
  }
}

fn sync_chain_to_player(state: &AppState) {
  let registry = (state.plugin_registry)();
  let specs = state.active_plugins.read().clone();
  let plugins = instantiate_active_plugins(&registry, &specs);
  if let Some(player) = state.player_state.read().as_ref() {
    player.replace_plugins(plugins);
  }
}

fn add_plugin(state: &mut AppState, plugin_id: &str) {
  let registry = (state.plugin_registry)();
  let Some(plugin) = registry.create(plugin_id) else {
    state.status_msg.set(format!("Unknown plugin: {plugin_id}"));
    return;
  };

  let active = ActivePluginState {
    id: plugin.id().to_string(),
    name: plugin.name().to_string(),
    params: plugin
      .parameters()
      .into_iter()
      .map(|p| (p.name.clone(), p.current, p.min, p.max, p.default))
      .collect(),
  };

  state.active_plugins.write().push(active);
  sync_chain_to_player(state);
  state.status_msg.set(format!("Added plugin: {plugin_id}"));
}

fn remove_plugin(state: &mut AppState, idx: usize) {
  let removed = {
    let mut plugins = state.active_plugins.write();
    if idx < plugins.len() {
      Some(plugins.remove(idx).name)
    } else {
      None
    }
  };
  if let Some(name) = removed {
    sync_chain_to_player(state);
    state.status_msg.set(format!("Removed plugin: {name}"));
  }
}

fn update_plugin_param(state: &mut AppState, plugin_idx: usize, param_name: &str, value: f32) {
  if !value.is_finite() {
    return;
  }
  let mut clamped = value;
  {
    let mut plugins = state.active_plugins.write();
    if let Some(plugin) = plugins.get_mut(plugin_idx)
      && let Some(param) = plugin.params.iter_mut().find(|p| p.0 == param_name)
    {
      clamped = value.clamp(param.2, param.3);
      param.1 = clamped;
    }
  }
  // Live parameter sync to the real-time AudioPlayer effect chain.
  if let Some(player) = state.player_state.read().as_ref() {
    player.set_plugin_param(plugin_idx, param_name, clamped);
  }
}

fn clear_all_plugins(state: &mut AppState) {
  if let Some(player) = state.player_state.read().as_ref() {
    player.clear_effects();
  }
  state.active_plugins.write().clear();
  state.status_msg.set("Plugin chain cleared".to_string());
}
