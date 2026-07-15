//! Browser **boot title screen**: the retail title card + New Game / Continue
//! menu, drawn from the disc's own title art (PROT 0888) through the shared
//! `legaia-engine-ui` builders and the engine's [`TitleSession`] state machine.
//!
//! This is the front of the `--boot-ui` chain the native `play-window` runs
//! (publisher logos -> title -> save-select -> field). The play page enters it
//! from the "New game" button: [`TitleSession`] drives FadeIn -> PressStart ->
//! MainMenu, and the page blits the title-TIM bands (wordmark, Press Start,
//! NEW GAME / CONTINUE rows, copyright lines) onto the same overlay canvas the
//! pause menu uses, over black. Picking New Game hands control back to the page,
//! which seeds the retail new-game defaults and enters the opening scene exactly
//! as before. Publisher logos + the Continue save-slot grid are not yet wired.

use super::*;
use crate::runtime::LegaiaRuntime;
use legaia_engine_core::title::{TitleInput, TitlePhase, TitleSession};
use legaia_engine_core::title_screen_atlas::build_atlas_from_prot_888;
use legaia_engine_ui::{self as ui, SpriteDraw, TextDraw};

/// `(edge & mask)` test on a PSX-encoded pad-edge word.
fn hit(edge: u16, mask: u16) -> bool {
    edge & mask != 0
}

/// Same quad JSON shape as the pause menu: `dst` / `src` rect + RGBA tint.
fn quad_json(d: &TextDraw) -> serde_json::Value {
    serde_json::json!({
        "dst": [d.dst.0, d.dst.1, d.dst.2, d.dst.3],
        "src": [d.src.0, d.src.1, d.src.2, d.src.3],
        "color": [d.color[0], d.color[1], d.color[2], d.color[3]],
    })
}

/// Stage origin + integer scale (320x240 boot stage centred on the surface) -
/// identical math to the pause menu / native window.
fn stage_transform(surface_w: u32, surface_h: u32) -> ((i32, i32), u32) {
    let sw = ui::BOOT_UI_STAGE_W;
    let sh = ui::BOOT_UI_STAGE_H;
    let scale = (surface_w / sw).min(surface_h / sh).clamp(1, 4);
    let x0 = (surface_w as i32 - (sw * scale) as i32) / 2;
    let y0 = (surface_h as i32 - (sh * scale) as i32) / 2;
    ((x0, y0), scale)
}

impl LegaiaRuntime {
    /// Build the title art off the loaded PROT 0888, best-effort.
    fn ensure_title_atlas(&mut self) {
        if self.title_atlas.is_some() {
            return;
        }
        let Some(host) = self.scene_host.as_ref() else {
            return;
        };
        let bytes = host
            .index
            .entry_bytes(legaia_asset::title_pak::PROT_INDEX_PRIMARY as u32);
        let atlas = bytes.ok().and_then(|b| {
            build_atlas_from_prot_888(&b, legaia_asset::title_pak::TITLE_TIM_OFFSET).ok()
        });
        if atlas.is_none() {
            crate::console_log("boot title: PROT 0888 title art unavailable");
        }
        self.title_atlas = atlas;
    }
}

#[wasm_bindgen]
impl LegaiaRuntime {
    /// Start the boot title screen. No-op with no disc loaded. Continue is left
    /// disabled (the browser boot does not preload an engine save); the fade-in
    /// is skipped so the card shows immediately.
    pub fn boot_title_start(&mut self) {
        if self.scene_host.is_none() {
            return;
        }
        // Reuse the pause-menu font atlas for the text draws.
        let _ = self.ensure_menu_assets();
        self.ensure_title_atlas();
        let mut session = TitleSession::without_save_data();
        session.skip_fade_in();
        self.boot_title = Some(session);
    }

    pub fn boot_title_is_active(&self) -> bool {
        self.boot_title.is_some()
    }

    /// `true` once the disc title art resolved (else the card renders text-only).
    pub fn boot_title_has_atlas(&self) -> bool {
        self.title_atlas.is_some()
    }

    /// Advance the title one frame with an edge-triggered PSX pad word. Returns
    /// `""` while the title runs, or the chosen outcome once the player
    /// confirms: `"new_game"`, `"continue"`, or `"options"`. The caller acts on
    /// the outcome (seed + enter the opening scene for New Game) and the title
    /// clears itself.
    pub fn boot_title_step(&mut self, edge: u16) -> String {
        let Some(session) = self.boot_title.as_mut() else {
            return String::new();
        };
        let input = TitleInput {
            up: hit(edge, 0x0010),
            down: hit(edge, 0x0040),
            cross: hit(edge, 0x4000),
            start: hit(edge, 0x0008),
            circle: hit(edge, 0x2000),
        };
        let _ = session.tick(input);
        use legaia_engine_core::title::TitleOutcome;
        match session.outcome() {
            Some(o) => {
                self.boot_title = None;
                match o {
                    TitleOutcome::NewGame => "new_game".to_string(),
                    TitleOutcome::Continue => "continue".to_string(),
                    TitleOutcome::Options => "options".to_string(),
                }
            }
            None => String::new(),
        }
    }

    /// Abort the title flow (page navigated away / cancelled).
    pub fn boot_title_close(&mut self) {
        self.boot_title = None;
    }

    /// The title art atlas (RGBA8) the sprite bands sample. Empty when none.
    pub fn boot_title_atlas_rgba(&self) -> Vec<u8> {
        self.title_atlas
            .as_ref()
            .map(|a| a.rgba.clone())
            .unwrap_or_default()
    }

    /// `[width, height]` of the title atlas; `[0, 0]` when none.
    pub fn boot_title_atlas_dims(&self) -> Vec<u32> {
        self.title_atlas
            .as_ref()
            .map(|a| vec![a.width, a.height])
            .unwrap_or_else(|| vec![0, 0])
    }

    /// Draw lists for the current title state, in surface pixels:
    /// `{ "active": true, "sprites": [...title-atlas quads...],
    ///    "texts": [...font quads...] }`. Rendered over black by the page.
    pub fn boot_title_draws_json(&self, surface_w: u32, surface_h: u32) -> String {
        let Some(session) = self.boot_title.as_ref() else {
            return r#"{"active":false,"sprites":[],"texts":[]}"#.to_string();
        };
        let (origin, scale) = stage_transform(surface_w.max(1), surface_h.max(1));
        let sprites = self.title_band_sprites(session, origin, scale);
        // Text stand-ins only when the disc art is missing (atlas_present =
        // false makes the builder emit ASCII fallbacks); with art present the
        // bands carry the wordmark / menu, so the font layer is empty.
        let mut texts: Vec<TextDraw> = Vec::new();
        if self.title_atlas.is_none()
            && let Some(font) = self.menu_assets.as_ref().map(|a| a.font_ref())
        {
            let (phase, cursor) = match session.phase() {
                TitlePhase::FadeIn { .. } | TitlePhase::PressStart { .. } => (1u8, 0u8),
                TitlePhase::MainMenu { cursor } => (2u8, cursor),
                TitlePhase::Done(_) => (2u8, 0u8),
            };
            let blink_on = matches!(session.phase(), TitlePhase::PressStart { blink_phase } if blink_phase < 30);
            let mut d = ui::title_draws_for(font, phase, cursor, false, blink_on, false, (96, 96));
            ui::scale_stage_text_draws(&mut d, origin, scale);
            texts.extend(d);
        }
        serde_json::json!({
            "active": true,
            "sprites": sprites.iter().map(quad_json).collect::<Vec<_>>(),
            "texts": texts.iter().map(quad_json).collect::<Vec<_>>(),
        })
        .to_string()
    }
}

impl LegaiaRuntime {
    /// Compose the title-TIM bands (wordmark, Press Start, NEW GAME / CONTINUE,
    /// copyright lines) into surface-pixel sprite quads - a faithful port of the
    /// native window's `title_screen_sprite_draws`.
    fn title_band_sprites(
        &self,
        session: &TitleSession,
        origin: (i32, i32),
        scale: u32,
    ) -> Vec<SpriteDraw> {
        use legaia_asset::title_pak;
        let mut out: Vec<SpriteDraw> = Vec::new();
        if self.title_atlas.is_none() {
            return out;
        }
        // Fade-in dims the whole card via alpha; other phases are opaque.
        let alpha = match session.phase() {
            TitlePhase::FadeIn { frames_remaining } => {
                let total = session.fade_in_frames.max(1) as f32;
                1.0 - (frames_remaining as f32 / total).clamp(0.0, 1.0)
            }
            TitlePhase::Done(_) => return out,
            _ => 1.0,
        };
        let color = [1.0, 1.0, 1.0, alpha];
        let (sx0, sy0) = origin;
        let si = scale as i32;
        let tpx = ui::TITLE_ART_POS.0;
        let tpy = ui::TITLE_ART_POS.1;
        let push = |out: &mut Vec<SpriteDraw>,
                    src: (u32, u32, u32, u32),
                    dsx: i32,
                    dsy: i32,
                    tint: [f32; 4]| {
            let (_, _, sw, sh) = src;
            out.push(SpriteDraw {
                dst: (
                    sx0 + (tpx + dsx) * si,
                    sy0 + (tpy + dsy) * si,
                    sw * scale,
                    sh * scale,
                ),
                src,
                color: tint,
            });
        };

        // Wordmark art always.
        let wm = title_pak::TITLE_BAND_WORDMARK;
        push(&mut out, wm, wm.0 as i32, wm.1 as i32, color);

        // Press Start prompt during that phase only.
        if matches!(session.phase(), TitlePhase::PressStart { .. }) {
            let ps = title_pak::TITLE_BAND_PRESS_START;
            push(&mut out, ps, ps.0 as i32, ps.1 as i32, color);
        }

        // NEW GAME / CONTINUE rows during the main menu (selected bright).
        if let TitlePhase::MainMenu { cursor } = session.phase() {
            let dim = [color[0] * 0.5, color[1] * 0.5, color[2] * 0.5, color[3]];
            let ng = title_pak::TITLE_BAND_MENU_NEW_GAME;
            let co = title_pak::TITLE_BAND_MENU_CONTINUE;
            let art_w = ui::TITLE_ART_SIZE.0 as u32;
            let ng_x = ((art_w - ng.2) / 2) as i32;
            let co_x = ((art_w - co.2) / 2) as i32;
            let ng_y: i32 = 154;
            let co_y: i32 = ng_y + ng.3 as i32 + 4;
            push(
                &mut out,
                ng,
                ng_x,
                ng_y,
                if cursor == 0 { color } else { dim },
            );
            push(
                &mut out,
                co,
                co_x,
                co_y,
                if cursor == 1 { color } else { dim },
            );
        }

        // Copyright lines always (post-fade).
        let tm = title_pak::TITLE_BAND_TM_COPYRIGHT;
        push(&mut out, tm, tm.0 as i32, tm.1 as i32, color);
        let cc = title_pak::TITLE_BAND_C_COPYRIGHT;
        push(&mut out, cc, cc.0 as i32, cc.1 as i32, color);
        out
    }
}
