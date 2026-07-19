//! Browser **opening-chain / cutscene presentation**: the prologue scene legs
//! (`opdeene` → `opstati` → `opurud` → …), the narration crawl + title card,
//! the "It was the Seru." caption, the prologue sepia grade + gold depth-cue
//! ramp, and the retail intro-skip handoff - the browser side of what the
//! native `play-window`'s redraw loop drives each frame.
//!
//! The chain itself is engine-resident: entering `opdeene` through
//! `enter_field_scene` arms `opening_chain_active`, installs the scene's
//! cutscene timeline (whose SceneChange ops walk the remaining legs), and the
//! narration roller advances inside `World::tick`. This module only exposes
//! the per-frame reads the page renders from, plus the two host duties:
//!
//! - **input lock**: while the narration crawl / title card is up the pad is
//!   frozen (the timeline owns the scene) but the world keeps ticking - the
//!   page reads [`LegaiaRuntime::play_cutscene_state_json`]'s `locked` and
//!   feeds pad `0`;
//! - **intro skip**: a Cross press routes into
//!   [`LegaiaRuntime::play_take_prologue_handoff`] (retail `FUN_801D1344` -
//!   the whole remaining opening skips to `town01`).
//!
//! ### Browser deviations (documented, not drift)
//! - The page has no STR/MDEC playback on the play path, so an FMV the
//!   timeline triggers is auto-finished ([`LegaiaRuntime::tick_frame`] calls
//!   `finish_cutscene` when `SceneMode::Cutscene` arms with an FMV pending).
//! - The cutscene camera the page builds from
//!   [`LegaiaRuntime::play_cutscene_camera_json`] is the retail op-`0x45`
//!   param decode (focus / pitch / yaw / H / eye trio - the native
//!   `cutscene_view` mirror) mapped onto the page's orbit projection, an
//!   approximation of the native PSX GTE camera.
//!
//! REF: FUN_801D1344, FUN_80037174, FUN_801DE084

use super::*;
use crate::runtime::LegaiaRuntime;
use legaia_engine_ui::{self as ui, TextDraw};

impl LegaiaRuntime {
    fn world(&self) -> Option<&legaia_engine_core::world::World> {
        self.scene_host.as_ref().map(|h| &h.world)
    }
}

#[wasm_bindgen]
impl LegaiaRuntime {
    /// Per-frame cutscene presentation state:
    /// ```text
    /// { "locked": bool,          // freeze the pad this frame (feed 0)
    ///   "chain": bool,           // opening chain playing (skip available)
    ///   "narration": bool, "card": bool,
    ///   "caption_alpha": 0.0,    // "It was the Seru." fade (0 = hidden)
    ///   "grade": { "gold": [r,g,b], "strength": s } | null,
    ///   "cue": { "far": [r,g,b], "near_z": f, "far_z": f, "max_ir0": f } | null }
    /// ```
    /// `grade` / `cue` mirror `World::scene_color_grade` /
    /// `World::scene_depth_cue` - the prologue sepia multiply + gold DPCS
    /// depth-cue ramp the native window stages into its renderer each frame.
    pub fn play_cutscene_state_json(&self) -> String {
        let Some(w) = self.world() else {
            return "null".to_string();
        };
        let narration = w.cutscene_narration_active();
        let card = w.cutscene_card.is_some();
        let grade = w
            .scene_color_grade()
            .map(|g| serde_json::json!({ "gold": g.gold, "strength": g.strength }));
        let cue = w.scene_depth_cue().map(|c| {
            serde_json::json!({
                "far": c.far, "near_z": c.near_z, "far_z": c.far_z,
                "max_ir0": c.max_ir0,
            })
        });
        serde_json::json!({
            "locked": narration || card,
            "chain": w.opening_chain_active,
            "narration": narration,
            "card": card,
            "caption_alpha": if w.cutscene_caption.is_some() {
                w.cutscene_caption_alpha
            } else {
                0.0
            },
            "grade": grade,
            "cue": cue,
        })
        .to_string()
    }

    /// The narration crawl + title card as font-atlas text quads over a
    /// `surface_w` x `surface_h` canvas - the same
    /// `{ "open", "texts" }` quad shape as the menu / dialog draws (blit off
    /// the font atlas; there are no chrome sprites). Line Ys are the
    /// roller's PSX 240-line window scaled to the surface; each line is
    /// centred, white - the native window's narration draw.
    /// REF: FUN_80037174
    pub fn play_cutscene_text_draws_json(&mut self, surface_w: u32, surface_h: u32) -> String {
        const CLOSED: &str = r#"{"open":false,"texts":[]}"#;
        let has_text = self
            .world()
            .is_some_and(|w| w.cutscene_narration_active() || w.cutscene_card.is_some());
        if !has_text || !self.ensure_menu_assets() {
            return CLOSED.to_string();
        }
        let (Some(w), Some(assets)) = (
            self.scene_host.as_ref().map(|h| &h.world),
            self.menu_assets.as_ref(),
        ) else {
            return CLOSED.to_string();
        };
        let font = assets.font_ref();
        let white = [1.0f32, 1.0, 1.0, 1.0];
        let center_x = (surface_w.max(1) / 2) as i32;
        let scale = surface_h.max(1) as f32 / 240.0;
        let mut texts: Vec<TextDraw> = Vec::new();
        // Bottom-up subtitle crawl: every visible line centred at its
        // current window Y (PSX 240-line space, scaled to the surface).
        if let Some(narration) = w.cutscene_narration.as_ref() {
            for line in narration.visible_lines() {
                let y = (line.y as f32 * scale) as i32;
                if y < 0 || y > surface_h as i32 - 8 {
                    continue;
                }
                texts.extend(ui::cutscene_narration_draws_for(
                    font, line.text, center_x, y, white,
                ));
            }
        }
        // Static title card: the pages shown together, centred, at the
        // capture-pinned band y=92..130.
        if let Some(card) = w.cutscene_card.as_ref() {
            for (i, text) in card.iter().enumerate() {
                let y = ((92 + 16 * i as i32) as f32 * scale) as i32;
                texts.extend(ui::cutscene_narration_draws_for(
                    font, text, center_x, y, white,
                ));
            }
        }
        serde_json::json!({
            "open": !texts.is_empty(),
            "texts": texts.iter().map(crate::play_menu::quad_json).collect::<Vec<_>>(),
        })
        .to_string()
    }

    /// Poll the retail prologue intro-skip (`FUN_801D1344`): while the
    /// opening chain plays with the handoff bit armed, a confirm press skips
    /// the whole remaining opening to `town01`. Returns the target scene
    /// label once (the page then enters it), else `""`.
    ///
    /// The engine-side handoff marks the upcoming `town01` entry as the
    /// new-game opening, which installs the establishing-sweep timeline whose
    /// pinned op-`0x49` opens the name-entry overlay. That mark is kept: the
    /// page draws the overlay ([`crate::play_name_entry`]), so the skip lands
    /// in the same naming prompt the native window reaches.
    pub fn play_take_prologue_handoff(&mut self, confirm: bool) -> String {
        let Some(h) = self.scene_host.as_mut() else {
            return String::new();
        };
        match h.world.take_prologue_handoff(confirm) {
            Some(target) => target.to_string(),
            None => String::new(),
        }
    }

    /// The "It was the Seru." caption image (a baked TIM the prologue blits,
    /// faded, between the two narration crawls), RGBA8. Empty when the
    /// current scene carries none.
    pub fn cutscene_caption_rgba(&self) -> Vec<u8> {
        self.world()
            .and_then(|w| w.cutscene_caption.as_ref())
            .map(|c| c.rgba.clone())
            .unwrap_or_default()
    }

    /// `[width, height]` of the caption image; `[0, 0]` when none.
    pub fn cutscene_caption_dims(&self) -> Vec<u32> {
        self.world()
            .and_then(|w| w.cutscene_caption.as_ref())
            .map(|c| vec![c.width, c.height])
            .unwrap_or_else(|| vec![0, 0])
    }

    /// Camera parameters for the cutscene shot, decoded from the timeline's
    /// executed op-`0x45` Camera Configure params - the browser mirror of
    /// the native window's `cutscene_view` (see that fn for the retail
    /// provenance: focus X/Z stored negated in params 6/8; pitch/yaw in
    /// params 0/1, PSX 4096 = turn; H in param 9; the eye-space translation
    /// trio in params 3/4/5, divided by retail's folded-in 6x world scale).
    /// Shape:
    /// ```text
    /// { "active": bool,  // a cutscene timeline is running
    ///   "focus": [x, y, z], "pitch": rad, "yaw": rad,
    ///   "h": f, "tr": [x, y, z] }
    /// ```
    /// `null` before a scene is entered.
    /// REF: FUN_801DE084, FUN_800172C0
    pub fn play_cutscene_camera_json(&self) -> String {
        use std::f32::consts::TAU;
        const CUTSCENE_WORLD_SCALE: f32 = 6.0;
        let Some(w) = self.world() else {
            return "null".to_string();
        };
        let params = &w.camera_state.params;
        let param = |slot: u8| {
            params
                .iter()
                .find(|p| p.slot == slot)
                .map(|p| p.value as i16 as f32)
        };
        // Focus X/Z fall back to the lead actor (the cutscene anchor) when a
        // beat hasn't staged them; focus Y follows retail's 0.
        let (px, pz) = w
            .actors
            .first()
            .filter(|a| a.active || a.tmd_binding.is_some())
            .map(|a| (a.move_state.world_x as f32, a.move_state.world_z as f32))
            .unwrap_or((0.0, 0.0));
        let focus = [
            param(6).map(|v| -v).unwrap_or(px),
            param(7).unwrap_or(0.0),
            param(8).map(|v| -v).unwrap_or(pz),
        ];
        let yaw = param(1).map(|v| v / 4096.0 * TAU).unwrap_or(0.0);
        let pitch = param(0)
            .map(|v| v / 4096.0 * TAU)
            .unwrap_or_else(|| 0.45f32.atan());
        let h = param(9).filter(|&h| h > 1.0).unwrap_or(512.0);
        let s = CUTSCENE_WORLD_SCALE;
        let tr = [
            param(3).unwrap_or(0.0) / s,
            param(4).unwrap_or(1200.0) / s,
            param(5).filter(|&z| z.abs() > 1.0).unwrap_or(17000.0) / s,
        ];
        serde_json::json!({
            "active": w.cutscene_timeline_active(),
            "focus": focus, "pitch": pitch, "yaw": yaw, "h": h, "tr": tr,
        })
        .to_string()
    }
}
