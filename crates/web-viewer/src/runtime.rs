//! Runtime engine bindings for the web viewer.
//!
//! Surfaces a [`LegaiaRuntime`] wrapper around
//! [`legaia_engine_core::world::World`] that the JS layer can drive each
//! frame to produce engine state (per-actor positions, current scene mode,
//! pending field events) without needing a full disc image staged to a
//! virtual filesystem.
//!
//! ### Minimal mode (no disc)
//! `new()` constructs a bare `World` + `MenuRuntime` that proves the
//! engine VMs compile to `wasm32-unknown-unknown` and that the per-frame
//! tick path is callable from JS.
//!
//! ### Disc mode (after `load_disc`)
//! Once the JS layer feeds raw PROT.DAT bytes + CDNAME.TXT text via
//! `load_disc`, the runtime builds a full `SceneHost` in-memory (no
//! filesystem access). `enter_scene(name)` then boots a named scene and
//! starts ticking the field-VM through `SceneHost::tick`.

#[cfg(target_arch = "wasm32")]
use legaia_engine_audio::WebAudioOut;
use legaia_engine_core::menu_runtime::MenuRuntime;
use legaia_engine_core::scene::SceneHost;
use legaia_engine_core::world::{SceneMode, World};
use legaia_engine_vm::menu::{MenuInput, open as menu_open};
use wasm_bindgen::prelude::*;

/// Bridge object the JS shim instantiates once at page load. Holds a
/// `World` + a `MenuRuntime` for the headless path, and an optional
/// `SceneHost` once `load_disc` has been called.
#[wasm_bindgen]
pub struct LegaiaRuntime {
    world: World,
    menu: MenuRuntime,
    scene_host: Option<SceneHost>,
    #[cfg(target_arch = "wasm32")]
    audio_out: Option<WebAudioOut>,
}

#[wasm_bindgen]
impl LegaiaRuntime {
    #[wasm_bindgen(constructor)]
    pub fn new() -> LegaiaRuntime {
        console_error_panic_hook::set_once();
        let mut world = World::default();
        world.spawn_actor(0).default_pos = legaia_engine_vm::Position::new(0, 0);
        world.mode = SceneMode::Title;
        let menu = MenuRuntime::new("/saves");
        Self {
            world,
            menu,
            scene_host: None,
            #[cfg(target_arch = "wasm32")]
            audio_out: None,
        }
    }

    /// Load a disc image from raw in-memory bytes.
    ///
    /// `raw_bytes` may be either:
    /// - A Mode2/2352 full disc image (`.bin`): PROT.DAT and CDNAME.TXT are
    ///   extracted automatically via ISO9660 walk.
    /// - The raw contents of `PROT.DAT` directly.
    ///
    /// `cdname_text` overrides any CDNAME.TXT found on the disc. Pass an empty
    /// string to use the disc's own CDNAME.TXT (full disc) or skip scene-name
    /// resolution (PROT.DAT-only path without a CDNAME).
    ///
    /// Returns the number of PROT entries parsed, or throws a JS error on
    /// parse failure.
    pub fn load_disc(&mut self, raw_bytes: Vec<u8>, cdname_text: String) -> Result<u32, JsValue> {
        use crate::disc::{extract_cdname_txt, extract_prot_dat, is_mode2_2352_disc};

        #[cfg(target_arch = "wasm32")]
        web_sys::console::log_1(&format!("load_disc: {} bytes", raw_bytes.len()).into());

        let (prot_bytes, auto_cdname) = if is_mode2_2352_disc(&raw_bytes) {
            #[cfg(target_arch = "wasm32")]
            web_sys::console::log_1(
                &"load_disc: detected Mode2/2352 disc; extracting PROT.DAT".into(),
            );
            let prot = extract_prot_dat(&raw_bytes)
                .ok_or_else(|| JsValue::from_str("load_disc: PROT.DAT not found in disc image"))?;
            let cdname = extract_cdname_txt(&raw_bytes);
            (prot, cdname)
        } else {
            #[cfg(target_arch = "wasm32")]
            web_sys::console::log_1(&"load_disc: treating input as raw PROT.DAT".into());
            (raw_bytes, None)
        };

        #[cfg(target_arch = "wasm32")]
        web_sys::console::log_1(
            &format!(
                "load_disc: PROT bytes = {}; building SceneHost",
                prot_bytes.len()
            )
            .into(),
        );

        let cdname_resolved = if !cdname_text.is_empty() {
            Some(cdname_text.as_str())
        } else {
            auto_cdname.as_deref()
        };
        let host = SceneHost::from_prot_bytes(prot_bytes, cdname_resolved)
            .map_err(|e| JsValue::from_str(&format!("load_disc: {e}")))?;

        #[cfg(target_arch = "wasm32")]
        web_sys::console::log_1(
            &format!(
                "load_disc: SceneHost OK, {} entries",
                host.index.entry_count()
            )
            .into(),
        );

        let count = host.index.entry_count() as u32;
        self.scene_host = Some(host);
        Ok(count)
    }

    /// Boot a named scene (CDNAME label, e.g. `"town01"`). Requires
    /// `load_disc` to have been called first. Loads the scene's assets,
    /// enters `SceneMode::Field`, and seeds the field-VM with record 0 of
    /// the scene's event-script pack. Throws a JS error if the disc hasn't
    /// been loaded or the scene name is unknown.
    pub fn enter_scene(&mut self, name: &str) -> Result<(), JsValue> {
        let host = self
            .scene_host
            .as_mut()
            .ok_or_else(|| JsValue::from_str("enter_scene: call load_disc first"))?;
        host.enter_field_scene(name, 0)
            .map_err(|e| JsValue::from_str(&format!("enter_scene: {e}")))?;
        Ok(())
    }

    /// Attempt to initialise the WebAudio backend. Must be called from a
    /// user-gesture handler (browser autoplay policy). Returns `true` if
    /// audio started successfully, `false` otherwise (e.g. blocked by the
    /// browser before any interaction or on a platform without WebAudio).
    ///
    /// Idempotent — calling a second time replaces the existing backend.
    pub fn audio_init(&mut self) -> bool {
        #[cfg(target_arch = "wasm32")]
        {
            match WebAudioOut::new() {
                Ok(out) => {
                    self.audio_out = Some(out);
                    true
                }
                Err(e) => {
                    web_sys::console::error_1(&format!("audio_init: {e}").into());
                    false
                }
            }
        }
        #[cfg(not(target_arch = "wasm32"))]
        false
    }

    /// Tick the world once. Returns the current frame counter.
    pub fn tick(&mut self) -> u64 {
        if let Some(ref mut host) = self.scene_host {
            host.world.tick();
            host.world.frame
        } else {
            self.world.tick();
            self.world.frame
        }
    }

    /// `true` if a disc has been loaded via `load_disc`.
    pub fn disc_loaded(&self) -> bool {
        self.scene_host.is_some()
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
        JsValue::from_str(&format!("{event:?}"))
    }

    /// Read the menu's current label (e.g. "STATUS", "SAVE — PICK SLOT")
    /// for HUD rendering.
    pub fn menu_label(&self) -> String {
        self.menu.current_label().to_string()
    }

    /// Read the active scene mode as a stable enum string.
    pub fn scene_mode(&self) -> String {
        if let Some(ref host) = self.scene_host {
            format!("{:?}", host.world.mode)
        } else {
            format!("{:?}", self.world.mode)
        }
    }

    /// Number of currently active actors.
    pub fn active_actor_count(&self) -> u32 {
        if let Some(ref host) = self.scene_host {
            host.world.actors.iter().filter(|a| a.active).count() as u32
        } else {
            self.world.actors.iter().filter(|a| a.active).count() as u32
        }
    }

    /// Frame counter.
    pub fn frame(&self) -> u64 {
        if let Some(ref host) = self.scene_host {
            host.world.frame
        } else {
            self.world.frame
        }
    }
}

impl Default for LegaiaRuntime {
    fn default() -> Self {
        Self::new()
    }
}
