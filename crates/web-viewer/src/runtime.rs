//! Runtime engine bindings for the web viewer.
//!
//! Surfaces a tiny [`LegaiaRuntime`] wrapper around
//! [`legaia_engine_core::world::World`] that the JS layer can drive each
//! frame to produce engine state (per-actor positions, current scene mode,
//! pending field events) without needing a full disc image staged to a
//! virtual filesystem.
//!
//! This is the *minimal* runtime surface — it proves that
//! `legaia-engine-vm` + `legaia-engine-core` compile to `wasm32-unknown-
//! unknown` and that the per-frame tick path is callable from JS. Full
//! [`legaia_engine_core::scene::SceneHost`] integration (which needs disc
//! bytes routed through a `Vfs` trait that doesn't touch the filesystem)
//! is a follow-up — see `docs/subsystems/engine.md`.

use legaia_engine_core::menu_runtime::MenuRuntime;
use legaia_engine_core::world::{SceneMode, World};
use legaia_engine_vm::menu::{MenuInput, open as menu_open};
use wasm_bindgen::prelude::*;

/// Bridge object the JS shim instantiates once at page load. Holds a
/// `World` + a `MenuRuntime` whose save dir is a virtual `/saves/` path
/// (writes to disk are no-ops in the browser; engines that want
/// persistence drive `world.save_party()` directly and write the bytes
/// through `localStorage`).
#[wasm_bindgen]
pub struct LegaiaRuntime {
    world: World,
    menu: MenuRuntime,
}

#[wasm_bindgen]
impl LegaiaRuntime {
    #[wasm_bindgen(constructor)]
    pub fn new() -> LegaiaRuntime {
        let mut world = World::default();
        // Seed an active actor so JS sees something change after `tick`.
        world.spawn_actor(0).default_pos = legaia_engine_vm::Position::new(0, 0);
        world.mode = SceneMode::Title;
        let menu = MenuRuntime::new("/saves");
        Self { world, menu }
    }

    /// Tick the world once. Returns the current frame counter.
    pub fn tick(&mut self) -> u64 {
        self.world.tick();
        self.world.frame
    }

    /// Boolean: true if the menu is open.
    pub fn menu_is_open(&self) -> bool {
        self.menu.is_open()
    }

    /// Open the menu (sets MenuCtx state to Idle).
    pub fn open_menu(&mut self) {
        menu_open(&mut self.menu.ctx);
    }

    /// Tick the menu state machine with a packed PSX-pad button mask.
    /// The mask matches `legaia_engine_vm::menu::MenuInput` field order:
    /// `cross | (circle<<1) | (triangle<<2) | (square<<3) | (up<<4) | (down<<5) | (left<<6) | (right<<7)`.
    pub fn menu_tick(&mut self, button_mask: u8) -> JsValue {
        let input = MenuInput {
            cross: button_mask & 0x01 != 0,
            circle: button_mask & 0x02 != 0,
            triangle: button_mask & 0x04 != 0,
            square: button_mask & 0x08 != 0,
            up: button_mask & 0x10 != 0,
            down: button_mask & 0x20 != 0,
            left: button_mask & 0x40 != 0,
            right: button_mask & 0x80 != 0,
        };
        let event = self.menu.tick(&mut self.world, input);
        // Format-string the event for JS to display.
        JsValue::from_str(&format!("{event:?}"))
    }

    /// Read the menu's current label (e.g. "STATUS", "SAVE — PICK SLOT")
    /// for HUD rendering.
    pub fn menu_label(&self) -> String {
        self.menu.current_label().to_string()
    }

    /// Read the active scene mode as a stable enum string.
    pub fn scene_mode(&self) -> String {
        format!("{:?}", self.world.mode)
    }

    /// Number of currently active actors.
    pub fn active_actor_count(&self) -> u32 {
        self.world.actors.iter().filter(|a| a.active).count() as u32
    }

    /// Frame counter.
    pub fn frame(&self) -> u64 {
        self.world.frame
    }
}

impl Default for LegaiaRuntime {
    fn default() -> Self {
        Self::new()
    }
}
