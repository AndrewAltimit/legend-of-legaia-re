//! The browser play page's host: [`runtime::LegaiaRuntime`], the native
//! window's `BootSession` over the page's WebGL renderer and WebAudio output.
//! The minigames page's runtime is `legaia-web-minigames`; the play page
//! holds one of its `LegaiaMinigames` for door-warp minigame art.
//!
//! An rlib: the wasm module the site loads is `legaia-web-viewer`'s cdylib,
//! which links this crate, so these `#[wasm_bindgen]` exports ship in the
//! same bundle as the viewer pages'. Module by module, see
//! `docs/subsystems/browser-host.md`.

pub mod boot_continue;
pub mod boot_title;
pub mod cards;
pub mod field_actors;
pub mod field_npc;
pub mod field_scene_anim;
mod host_slot;
mod packet_color;
pub mod pad_bindings;
pub mod play;
pub mod play_battle;
pub mod play_battle_body_blend;
pub mod play_battle_fx;
pub mod play_battle_limb_dim;
pub mod play_battle_render;
pub mod play_battle_vram;
pub mod play_bgm;
pub mod play_camera;
pub mod play_cheats;
pub mod play_cutscene;
pub mod play_dance_art;
pub mod play_dev_menu;
pub mod play_dialog;
pub mod play_field_fx;
pub mod play_field_hud;
pub mod play_fishing;
pub mod play_fmv;
pub mod play_fog_volume;
pub mod play_frame_step;
pub mod play_host_parity;
pub mod play_hotkeys;
pub mod play_lighting;
pub mod play_menu;
pub mod play_minigame_arena;
pub mod play_minigame_slots;
pub mod play_minigames;
pub mod play_name_entry;
pub mod play_sfx;
pub mod play_shop;
pub mod play_tile_board;
pub mod play_toggles;
pub mod play_world_map_markers;
pub mod play_xa;
pub mod resume;
pub mod runtime;
pub mod session_save;

use wasm_bindgen::prelude::*;

/// Log a line to the browser console.
///
/// Natively this writes to stderr instead: `web_sys::console` is a
/// wasm-bindgen import and calling it off-wasm aborts the process, which
/// would keep the runtime out of reach of the native disc-gated tests.
fn console_log(s: &str) {
    #[cfg(target_arch = "wasm32")]
    web_sys::console::log_1(&JsValue::from_str(s));
    #[cfg(not(target_arch = "wasm32"))]
    eprintln!("{s}");
}
