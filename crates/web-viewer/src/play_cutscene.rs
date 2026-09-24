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
//! - An FMV the timeline triggers plays through [`crate::play_fmv`] (the
//!   page slices the STR segment out of its disc bytes, the engine decodes
//!   it, the page draws the frames); a page that never declares support
//!   still auto-finishes the beat with the hand-off applied.
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
    /// depth-cue ramp the native window stages into its renderer each frame -
    /// **already composed with the scripted screen tint**
    /// (`World::scene_screen_tint`, the op-`4C 12` scene-entry fade), the way
    /// the native window's staging match composes it
    /// (`window/event_handler/redraw.rs`):
    ///
    /// * no prologue grade + a tint -> `grade = { gold: tint, strength: 1 }`,
    ///   which is the native `(None, Some(t))` arm verbatim;
    /// * a prologue grade -> `grade` stays the untinted gold and the tint
    ///   rides `palette_grade`, the native `(Some(g), t)` arm;
    /// * either way the depth cue's far colour is multiplied by the tint, so
    ///   a fade-to-black reaches full black on far-cued geometry.
    ///
    /// Before this, `tint` reached the page's engine-built geometry (the
    /// field-FX parts are coloured engine-side) and nothing else, so an
    /// ordinary town's scene-entry fade darkened the smoke puffs over a town
    /// that never faded. `palette_grade` has no consumer in
    /// `site/js/webgl-tmd.js` yet - see `docs/tooling/host-drift.md`.
    /// Abandon a live opening cutscene chain before a **user-initiated**
    /// scene entry (the page's scene picker, a card load, the title's
    /// re-entry) - [`legaia_engine_core::world::World::abandon_opening_chain`].
    /// The chain's own hand-offs (the intro-skip, the post-movie leg) never
    /// go through the picker, so they keep their state. Returns whether a
    /// chain was live. The engine camera's own half of a direct entry is
    /// `Camera::reset_for_scene_entry`, run inside `enter_field` itself.
    pub fn play_abandon_opening_chain(&mut self) -> bool {
        self.scene_host
            .as_mut()
            .is_some_and(|h| h.world.abandon_opening_chain())
    }

    pub fn play_cutscene_state_json(&self) -> String {
        let Some(w) = self.world() else {
            return "null".to_string();
        };
        let narration = w.cutscene_narration_active();
        let card = w.cutscene.card.is_some();
        // The three staging arms of the native window's redraw, resolved
        // here so the page's renderer takes the same two calls it does.
        let tint = w.scene_screen_tint();
        let (grade, palette_grade) = match (w.scene_color_grade(), tint) {
            (Some(g), t) => (
                Some(serde_json::json!({ "gold": g.gold, "strength": g.strength })),
                serde_json::json!({ "mul": t.unwrap_or([1.0; 3]), "on": true }),
            ),
            (None, Some(t)) => (
                Some(serde_json::json!({ "gold": t, "strength": 1.0 })),
                serde_json::json!({ "mul": [1.0f32, 1.0, 1.0], "on": false }),
            ),
            (None, None) => (
                None,
                serde_json::json!({ "mul": [1.0f32, 1.0, 1.0], "on": false }),
            ),
        };
        let cue = w.scene_depth_cue().map(|c| {
            let far = match tint {
                Some(t) => [c.far[0] * t[0], c.far[1] * t[1], c.far[2] * t[2]],
                None => c.far,
            };
            serde_json::json!({
                "far": far, "near_z": c.near_z, "far_z": c.far_z,
                "max_ir0": c.max_ir0,
            })
        });
        serde_json::json!({
            "locked": narration || card,
            "chain": w.cutscene.opening_chain_active,
            "narration": narration,
            "card": card,
            "caption_alpha": if w.cutscene.caption.is_some() {
                w.cutscene.caption_alpha
            } else {
                0.0
            },
            "grade": grade,
            "palette_grade": palette_grade,
            "cue": cue,
        })
        .to_string()
    }

    /// The narration crawl + title card as font-atlas text quads over a
    /// `surface_w` x `surface_h` canvas - the same
    /// `{ "open", "texts" }` quad shape as the menu / dialog draws (blit off
    /// the font atlas; there are no chrome sprites). The lines are laid out
    /// in the 320x240 stage (the roller's PSX 240-line Ys, centred, white)
    /// and scaled through the page's stage transform - the native window's
    /// narration draw.
    /// REF: FUN_80037174
    pub fn play_cutscene_text_draws_json(&mut self, surface_w: u32, surface_h: u32) -> String {
        const CLOSED: &str = r#"{"open":false,"texts":[]}"#;
        let has_text = self
            .world()
            .is_some_and(|w| w.cutscene_narration_active() || w.cutscene.card.is_some());
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
        // The crawl and the title card in retail's 320x240 stage, through
        // the shared `cutscene_text_stage_draws`, then the page's stage
        // transform - the native window's narration draw, pass for pass.
        let lines = w
            .cutscene
            .narration
            .as_ref()
            .map(|n| n.visible_lines())
            .unwrap_or_default();
        let crawl: Vec<(&str, i32)> = lines.iter().map(|l| (l.text, l.y)).collect();
        let card: Vec<&str> = w
            .cutscene
            .card
            .iter()
            .flatten()
            .map(String::as_str)
            .collect();
        let mut texts: Vec<TextDraw> =
            ui::cutscene_text_stage_draws(font, &crawl, &card, [1.0, 1.0, 1.0, 1.0]);
        let (origin, scale) = crate::play_menu::stage_transform(surface_w, surface_h);
        ui::scale_stage_text_draws(&mut texts, origin, scale);
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
            .and_then(|w| w.cutscene.caption.as_ref())
            .map(|c| c.rgba.clone())
            .unwrap_or_default()
    }

    /// `[width, height]` of the caption image; `[0, 0]` when none.
    pub fn cutscene_caption_dims(&self) -> Vec<u32> {
        self.world()
            .and_then(|w| w.cutscene.caption.as_ref())
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
    ///   "focus": [x, y, z], "pitch": rad, "yaw": rad, "roll": rad,
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
        let params = &w.camera.state.params;
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
        // Slot 2 = roll (`_DAT_8007B794`, the GTE `RotMatrixZ` angle) - the
        // third factor `FUN_8001CF50` composes. Retail authors it in eight
        // scenes (`engine-core/tests/thread_camera_roll_execution.rs`), so the
        // page gets it alongside pitch and yaw rather than dropping the term.
        let roll = param(2).map(|v| v / 4096.0 * TAU).unwrap_or(0.0);
        let h = param(9).filter(|&h| h > 1.0).unwrap_or(512.0);
        let s = CUTSCENE_WORLD_SCALE;
        let tr = [
            param(3).unwrap_or(0.0) / s,
            param(4).unwrap_or(1200.0) / s,
            param(5).filter(|&z| z.abs() > 1.0).unwrap_or(17000.0) / s,
        ];
        serde_json::json!({
            "active": w.cutscene_timeline_active(),
            "focus": focus, "pitch": pitch, "yaw": yaw, "roll": roll,
            "h": h, "tr": tr,
        })
        .to_string()
    }
}
