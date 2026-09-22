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
//! which seeds the retail new-game defaults and enters the opening scene.
//!
//! **The mode word is held here too.** `engine-shell`'s `BootSession` holds
//! the port's seat at the retail mode table (`engine-core::mode::ModeSeat`)
//! and carries `_DAT_8007B83C` through the boot chain; this page holds one on
//! [`LegaiaRuntime`] (`mode_seat`), reconciled once per frame by
//! `LegaiaRuntime::tick_mode_seat` through the same two seat entry points -
//! so the battle-intro mode hand-off and the mode-change edge land on the same
//! frame on both hosts, and the page can report the word
//! (`mode_state_json`). Nothing on this screen is drawn from it: the screen
//! selection below is the *sub-mode* word, a different register already shared
//! with the native window. What the page still lacks is a session object, which
//! is what its BGM director and its save flow wait on - not the seat.
//!
//! **All three rows are live.** Continue is enabled off a save scan (the
//! memory-card rack, this host's save store) exactly as the native window
//! picks `TitleSession::new()` vs `::without_save_data()` off `scan_save_dir`,
//! and Continue / Options route to the retail save-select and options
//! screens through the pause menu's own rows
//! ([`LegaiaRuntime::play_menu_open_row`]) rather than through a second copy
//! of either screen. The page used to call `without_save_data()`
//! unconditionally and then discard the outcome, so both rows were dead on a
//! host that could already load and persist saves. Publisher logos are still
//! not wired.
//!
//! **The attract plays the movie.** Retail's `AttractIdle` arm hands the
//! screen to `fmv_id 0` (`MV1.STR`) and re-enters the front end afterwards;
//! the native window plays it through its MDEC path. This page arms the same
//! movie through the play page's FMV lane ([`crate::play_fmv`]): the session
//! is frozen in `TitlePhase::Attract` while the page installs and plays the
//! sectors, and `finish_attract` runs when the page reports the end (or a
//! face-button / Select edge aborts it - retail's `fmv_id 0` skip). A page
//! without the FMV script never declares support, so the movie finishes the
//! frame it arms and the countdown is counted as a skip
//! ([`LegaiaRuntime::boot_title_attract_skips`]).

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

    /// Build the publisher-logo atlas off PROT 0895 (`init.pak`),
    /// best-effort. The browser twin of the native `play-window` boot, which
    /// decodes the same entry through the same
    /// `publisher_logos::build_atlas_from_init_pak`.
    fn ensure_publisher_logos_atlas(&mut self) {
        if self.boot_logos_atlas.is_some() || self.boot_logos_failed {
            return;
        }
        self.boot_logos_failed = true;
        let Some(host) = self.scene_host.as_ref() else {
            return;
        };
        let Ok(bytes) = host
            .index
            .entry_bytes(legaia_asset::init_pak::PROT_INDEX as u32)
        else {
            crate::console_log("boot logos: PROT 0895 (init.pak) unavailable");
            return;
        };
        match legaia_engine_core::publisher_logos::build_atlas_from_init_pak(&bytes) {
            Ok(a) => {
                self.boot_logos_failed = false;
                self.boot_logos_atlas = Some(a);
            }
            Err(e) => crate::console_log(&format!("boot logos: atlas build failed: {e:#}")),
        }
    }

    /// Build the menu-glyph atlas (the small-caps sheet at
    /// [`legaia_asset::menu_glyph_atlas`]) off the loaded PROT, best-effort.
    /// Only the no-title-art path needs it, but it is cheap and stable, so it
    /// is built alongside the title art.
    fn ensure_menu_glyph_atlas(&mut self) {
        if self.menu_glyph_atlas.is_some() {
            return;
        }
        let Some(host) = self.scene_host.as_ref() else {
            return;
        };
        self.menu_glyph_atlas = host
            .index
            .prot_dat_raw_bytes(
                legaia_asset::menu_glyph_atlas::PROT_DAT_OFFSET,
                legaia_asset::menu_glyph_atlas::TIM_SIZE,
            )
            .ok()
            .and_then(|b| {
                legaia_engine_core::menu_glyph_atlas::build_atlas_from_prot_dat_slice(&b).ok()
            });
    }

    /// A picked title row whose pause-menu door would not open returns the
    /// player to the TITLE, not into a new game.
    ///
    /// `play_menu_open_row` fails whenever the row is unavailable on this
    /// world - a standing op-`0x49` entry-context park blocks Load, and a
    /// menu that will not open blocks both - and this page used to answer
    /// that with `"new_game"`, so a player who pressed Continue could find
    /// themselves in the opening cutscene with their save untouched. The
    /// native window never has this door: `TitleOutcome::Continue` installs
    /// `BootUiState::SaveSelect` and `Options` installs `BootUiState::Options`
    /// directly, and neither can decay into New Game
    /// (`window/boot_cutscene.rs`).
    ///
    /// Returns `""` (the "title still running" answer), having put a fresh
    /// title session back up with its attract countdown armed.
    fn reopen_title_after_failed_row(&mut self, row: &str) -> String {
        crate::console_log(&format!(
            "boot title: the {row} row would not open on this world; returning to the title"
        ));
        let mut session = if self.boot_title_has_save_data() {
            TitleSession::new()
        } else {
            TitleSession::without_save_data()
        };
        session.attract_enabled = true;
        self.boot_title = Some(session);
        String::new()
    }
}

#[wasm_bindgen]
impl LegaiaRuntime {
    /// Start the boot title screen. No-op with no disc loaded. The fade-in is
    /// skipped so the card shows immediately.
    ///
    /// **Continue is enabled off a live save scan**, the way the native
    /// window picks `TitleSession::new()` vs `::without_save_data()` from
    /// `scan_save_dir`. The browser's save store is the memory-card rack
    /// ([`crate::cards`]) - the same rack the pause menu's Load / Save rows
    /// write through - so the scan is "does any inserted card hold a readable
    /// block". The page used to call `without_save_data()` unconditionally and
    /// then discard the outcome, which left both non-New-Game rows dead on a
    /// host that could already load and persist saves.
    pub fn boot_title_start(&mut self) {
        if self.scene_host.is_none() {
            return;
        }
        // Reuse the pause-menu font atlas for the text draws.
        let _ = self.ensure_menu_assets();
        self.ensure_title_atlas();
        self.ensure_menu_glyph_atlas();
        let mut session = if self.boot_title_has_save_data() {
            TitleSession::new()
        } else {
            TitleSession::without_save_data()
        };
        // The attract hand-off is armed on this host too, so the idle
        // countdown reaches the same state the native window reaches; the
        // movie itself plays through the FMV lane - see `boot_title_step`.
        session.attract_enabled = true;
        self.boot_title = Some(session);
        self.boot_title_attract_skips = 0;
    }

    /// How many times the attract countdown fired on this page since the
    /// title opened **without the movie playing** - the page never declared
    /// FMV support ([`LegaiaRuntime::play_fmv_set_supported`]), the install
    /// timed out, or the movie was not on the loaded image. A real playback
    /// (installed, drawn, finished by the page or the skip edge) does not
    /// count. The page reads it to disclose the deviation rather than
    /// silently looping the menu.
    pub fn boot_title_attract_skips(&self) -> u32 {
        self.boot_title_attract_skips
    }

    /// Whether the page holds any loadable save - the browser twin of the
    /// native boot's `scan_save_dir(..).any(|s| s.present)`. Public so the
    /// page can label the Continue row honestly before the title even opens.
    pub fn boot_title_has_save_data(&self) -> bool {
        (0..crate::cards::CARD_SLOTS)
            .any(|port| self.card_block_snapshots(port).iter().any(|b| b.present))
    }

    pub fn boot_title_is_active(&self) -> bool {
        self.boot_title.is_some()
    }

    /// The retail sub-mode of `FUN_801DD35C` the title is in, or `0xFF` when
    /// no title is open. A cold boot reports `0x10` (`AttractIdle`), the
    /// value a cold-boot capture sees - the `0x02` text menu is unreachable.
    /// The native window logs the same value on the same transitions.
    pub fn boot_title_submode(&self) -> u8 {
        self.boot_title
            .as_ref()
            .map_or(0xFF, |s| s.retail_submode())
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
        let input = TitleInput {
            up: hit(edge, 0x0010),
            down: hit(edge, 0x0040),
            cross: hit(edge, 0x4000),
            start: hit(edge, 0x0008),
            circle: hit(edge, 0x2000),
        };
        // The attract hand-off, browser side. Retail's `AttractIdle` arm
        // gives the screen to `fmv_id 0` and returns to the title; the
        // native window plays that movie through its MDEC path
        // (`service_title_attract`). Here the movie is armed on the play
        // page's FMV lane the frame the countdown fires, and the session
        // stays frozen in `Attract` until that lane reports the end.
        let (attract_fmv, attract_playing) = {
            let Some(session) = self.boot_title.as_mut() else {
                return String::new();
            };
            let _ = session.tick(input);
            let pending = session.attract_pending();
            if pending.is_some() {
                session.mark_attract_started();
            }
            (pending, session.attract_playing())
        };
        if let Some(fmv_id) = attract_fmv {
            self.fmv_arm(fmv_id, crate::play_fmv::FmvOrigin::Attract);
        }
        if attract_playing {
            let fmv_id = self.fmv.armed_for().map_or(
                legaia_engine_vm::title_overlay::ATTRACT_FMV_ID,
                |(_, id)| id,
            );
            // Retail's abort: `fmv_id 0` on a face button / Select
            // (`FUN_801CF098`'s `_DAT_8007B850 & 0x1F0` test).
            if crate::play_fmv::skip_edge_hit(fmv_id, edge) {
                self.fmv.request_finish();
            }
            match self.fmv.poll() {
                crate::play_fmv::FmvPoll::Hold => return String::new(),
                crate::play_fmv::FmvPoll::Finished { played } => {
                    self.fmv_audio_stop();
                    if let Some(session) = self.boot_title.as_mut() {
                        session.finish_attract();
                    }
                    if played {
                        // Retail re-enters the front end through `Init`,
                        // theme and all; the movie paused the sequencer.
                        self.fmv_resume_sequencer();
                    } else {
                        self.boot_title_attract_skips =
                            self.boot_title_attract_skips.saturating_add(1);
                        crate::console_log(&format!(
                            "title attract: fmv_id={fmv_id} finished unplayed; returning to the title"
                        ));
                    }
                    return String::new();
                }
            }
        }
        let Some(session) = self.boot_title.as_mut() else {
            return String::new();
        };
        use legaia_engine_core::title::TitleOutcome;
        match session.outcome() {
            Some(o) => {
                // The session is not simply dropped at the hand-off: retail
                // keeps the title art on screen behind the save-select, so
                // Continue parks it as the backdrop
                // (`boot_title_backdrop_draws_json`) and only New Game /
                // Options release it. Retail composes the boot options screen
                // against black, so Options takes the drop.
                let parked = self.boot_title.take();
                match o {
                    TitleOutcome::NewGame => "new_game".to_string(),
                    // Continue and Options route to the same two screens the
                    // native window routes them to - the retail save-select
                    // and the retail options screen - reached here through
                    // the pause menu's own rows rather than through a second
                    // copy of either. The page then drives `play_menu_input`
                    // / `play_menu_draws_json` until the menu closes.
                    TitleOutcome::Continue => {
                        if self.play_menu_open_row("Load") {
                            self.boot_title_backdrop = parked;
                            "continue".to_string()
                        } else {
                            self.reopen_title_after_failed_row("Load")
                        }
                    }
                    TitleOutcome::Options => {
                        if self.play_menu_open_row("Options") {
                            "options".to_string()
                        } else {
                            self.reopen_title_after_failed_row("Options")
                        }
                    }
                }
            }
            None => String::new(),
        }
    }

    /// Abort the title flow (page navigated away / cancelled).
    pub fn boot_title_close(&mut self) {
        self.boot_title = None;
        self.boot_title_backdrop = None;
    }

    /// Start the **publisher-logo** boot phase - the stage retail plays
    /// before the title card (PROT 0895's own sequencer, `FUN_801CEFD4`:
    /// SCEA, then Contrail, then PROKION, each with its own fade-in / hold /
    /// fade-out). `false` when the disc carries no readable `init.pak`, in
    /// which case the caller goes straight to the title, exactly as the
    /// native `--boot-ui` chain does with an unbuildable atlas.
    ///
    /// Only the native window used to play these: `engine_core::publisher_logos`
    /// and its atlas builder were shared, and the browser play page entered
    /// the boot chain one stage late, at the title.
    pub fn boot_logos_start(&mut self) -> bool {
        if self.scene_host.is_none() {
            return false;
        }
        self.ensure_publisher_logos_atlas();
        if self.boot_logos_atlas.is_none() {
            return false;
        }
        self.boot_logos = Some(legaia_engine_core::publisher_logos::PublisherLogosSession::new());
        true
    }

    /// `true` while the logo phase owns the screen.
    pub fn boot_logos_is_active(&self) -> bool {
        self.boot_logos.is_some()
    }

    /// Advance the logo sequencer one frame with an edge-triggered PSX pad
    /// word. Returns `true` once the phase is over (the caller then opens the
    /// title card). Start / Cross skip the rest of the sequence, which is the
    /// same request the native boot chain honours.
    pub fn boot_logos_step(&mut self, edge: u16) -> bool {
        let Some(session) = self.boot_logos.as_mut() else {
            return true;
        };
        if hit(edge, 0x0008) || hit(edge, 0x4000) {
            session.request_skip();
        }
        session.tick();
        if session.is_done() {
            self.boot_logos = None;
            return true;
        }
        false
    }

    /// The publisher-logo atlas (RGBA8) the logo quads sample. Empty when it
    /// did not resolve.
    pub fn boot_logos_atlas_rgba(&self) -> Vec<u8> {
        self.boot_logos_atlas
            .as_ref()
            .map(|a| a.rgba.clone())
            .unwrap_or_default()
    }

    /// `[width, height]` of the publisher-logo atlas; `[0, 0]` when none.
    pub fn boot_logos_atlas_dims(&self) -> Vec<u32> {
        self.boot_logos_atlas
            .as_ref()
            .map(|a| vec![a.width, a.height])
            .unwrap_or_else(|| vec![0, 0])
    }

    /// Draw list for the current logo frame, in surface pixels:
    /// `{ "active": true, "sprites": [...atlas quads...] }`, rendered over
    /// black by the page.
    ///
    /// The quads come out of the shared
    /// `legaia_engine_ui::ui_boot_logos::publisher_logo_sprite_draws` the
    /// native window draws through, against retail's own 640x480 boot stage -
    /// so the two hosts letterbox the logos identically and a fade level is
    /// the same alpha in both.
    pub fn boot_logos_draws_json(&self, surface_w: u32, surface_h: u32) -> String {
        use legaia_engine_core::publisher_logos::{LOGO_COUNT, STAGE};
        use legaia_engine_ui::ui_boot_logos::{LogoQuadView, publisher_logo_sprite_draws};
        let (Some(session), Some(atlas)) =
            (self.boot_logos.as_ref(), self.boot_logos_atlas.as_ref())
        else {
            return r#"{"active":false,"sprites":[]}"#.to_string();
        };
        let idx = session.current_logo();
        if idx >= LOGO_COUNT {
            return r#"{"active":true,"sprites":[]}"#.to_string();
        }
        let quads: Vec<LogoQuadView> = session
            .current_quads()
            .iter()
            .map(|q| LogoQuadView {
                src: q.src,
                dst: q.dst,
            })
            .collect();
        let draws = publisher_logo_sprite_draws(
            &quads,
            atlas.rects[idx],
            STAGE,
            session.alpha(),
            surface_w.max(1),
            surface_h.max(1),
        );
        let sprites: Vec<serde_json::Value> = draws
            .iter()
            .map(|d| {
                serde_json::json!({
                    "dst": [d.dst.0, d.dst.1, d.dst.2, d.dst.3],
                    "src": [d.src.0, d.src.1, d.src.2, d.src.3],
                    "color": [d.color[0], d.color[1], d.color[2], d.color[3]],
                })
            })
            .collect();
        serde_json::json!({ "active": true, "sprites": sprites }).to_string()
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

    /// The menu-glyph atlas (RGBA8 stencil) the no-title-art menu rows
    /// sample. Empty when it did not resolve.
    pub fn boot_title_glyph_atlas_rgba(&self) -> Vec<u8> {
        self.menu_glyph_atlas
            .as_ref()
            .map(|a| a.rgba.clone())
            .unwrap_or_default()
    }

    /// `[width, height]` of the menu-glyph atlas; `[0, 0]` when none.
    pub fn boot_title_glyph_atlas_dims(&self) -> Vec<u32> {
        self.menu_glyph_atlas
            .as_ref()
            .map(|a| vec![a.width, a.height])
            .unwrap_or_else(|| vec![0, 0])
    }

    /// Draw lists for the current title state, in surface pixels:
    /// `{ "active": true, "sprites": [...title-atlas quads...],
    ///    "glyphs": [...menu-glyph-atlas quads...],
    ///    "texts": [...font quads...] }`. Rendered over black by the page.
    ///
    /// The three layers are mutually exclusive by design, and the split
    /// mirrors the native window exactly. With the disc's title art present
    /// the TIM's own NEW GAME / CONTINUE bands carry the menu (`sprites`).
    /// Without it the rows fall back to the shared
    /// [`ui::title_menu_draws_for`] builder sampling the menu-glyph atlas
    /// (`glyphs`) - the same fallback the native window's
    /// `title_menu_glyph_sprite_draws` serves - and only if that atlas is
    /// missing too does the font stand-in (`texts`) draw.
    pub fn boot_title_draws_json(&self, surface_w: u32, surface_h: u32) -> String {
        let Some(session) = self.boot_title.as_ref() else {
            return r#"{"active":false,"sprites":[],"glyphs":[],"texts":[]}"#.to_string();
        };
        let (origin, scale) = stage_transform(surface_w.max(1), surface_h.max(1));
        let sprites = self.title_band_sprites(session, origin, scale);
        let glyphs = self.title_menu_glyph_sprites(session, surface_w.max(1), surface_h.max(1));
        // Text stand-ins only when the disc art is missing (atlas_present =
        // false makes the builder emit ASCII fallbacks); with art present the
        // bands carry the wordmark / menu, so the font layer is empty.
        let mut texts: Vec<TextDraw> = Vec::new();
        if self.title_atlas.is_none()
            && let Some(font) = self.menu_assets.as_ref().map(|a| a.font_ref())
        {
            // The screen is selected by retail's own sub-mode word through
            // the shared `ui::title_text_phase` table - the same call the
            // native window makes - so neither host decides it locally. The
            // session's phase supplies only the cursor row.
            let (menu_open, cursor) = match session.phase() {
                TitlePhase::MainMenu { cursor } => (true, cursor),
                _ => (false, 0u8),
            };
            let phase = ui::title_text_phase(session.retail_submode(), menu_open);
            // The menu rows are the glyph layer's job whenever that atlas
            // resolved; the font only ever stands in for a row nothing else
            // can draw. (The Press Start prompt has no glyph-atlas form, so
            // phase 1 always falls to the font here.)
            let phase = if phase == 2 && !glyphs.is_empty() {
                0
            } else {
                phase
            };
            // The duty is HALF the session's own period, not a literal:
            // `blink_phase` is `(phase + 1) % blink_period` and the period is
            // 30, so a literal 30 made this predicate a tautology and the
            // prompt never blinked on this host.
            let blink_on = matches!(
                session.phase(),
                TitlePhase::PressStart { blink_phase } if blink_phase < session.blink_period / 2
            );
            let mut d = ui::title_draws_for(font, phase, cursor, false, blink_on, false, (96, 96));
            ui::scale_stage_text_draws(&mut d, origin, scale);
            texts.extend(d);
        }
        serde_json::json!({
            "active": true,
            "sprites": sprites.iter().map(quad_json).collect::<Vec<_>>(),
            "glyphs": glyphs.iter().map(quad_json).collect::<Vec<_>>(),
            "texts": texts.iter().map(quad_json).collect::<Vec<_>>(),
        })
        .to_string()
    }
}

impl LegaiaRuntime {
    /// The title menu's NEW GAME / CONTINUE rows drawn from the **menu-glyph
    /// atlas** through the shared [`ui::title_menu_draws_for`] builder - the
    /// no-title-art fallback, and a port of the native window's
    /// `title_menu_glyph_sprite_draws` down to the anchor math.
    ///
    /// Empty unless the session is in `MainMenu`, the glyph atlas resolved,
    /// and the title art did **not**: with the title TIM present its own
    /// bands carry the rows, and drawing both would double-render them.
    fn title_menu_glyph_sprites(
        &self,
        session: &TitleSession,
        surface_w: u32,
        surface_h: u32,
    ) -> Vec<SpriteDraw> {
        if self.menu_glyph_atlas.is_none() || self.title_atlas.is_some() {
            return Vec::new();
        }
        let (menu_open, cursor) = match session.phase() {
            TitlePhase::MainMenu { cursor } => (true, cursor),
            _ => (false, 0),
        };
        if ui::title_text_phase(session.retail_submode(), menu_open) != 2 {
            return Vec::new();
        }
        // Anchor inside the same centred + integer-scaled 256x256 title stage
        // the art path uses. The rows sit between the wordmark band (ends at
        // src y=140) and the copyright bands (start at src y=195).
        let atlas_w: u32 = 256;
        let atlas_h: u32 = 256;
        let title_scale = (surface_w / atlas_w).min(surface_h / atlas_h).clamp(1, 4);
        let ts = title_scale as i32;
        let stage_x0 = (surface_w as i32 - atlas_w as i32 * ts) / 2;
        let stage_y0 = (surface_h as i32 - atlas_h as i32 * ts) / 2;
        // "NEW GAME" is 8 cells x 8 px wide at a 1x glyph multiplier; centre
        // that inside the 256-wide stage.
        let menu_w_src = 8 * 8;
        let pen = (
            stage_x0 + (atlas_w as i32 - menu_w_src) / 2 * ts,
            stage_y0 + 152 * ts,
        );
        ui::title_menu_draws_for(2, cursor, session.continue_enabled, pen, title_scale)
    }

    /// Compose the title-TIM bands (wordmark, Press Start, NEW GAME /
    /// CONTINUE, copyright lines) into surface-pixel sprite quads.
    ///
    /// The composition itself is the shared `ui::title_band_sprites` kernel
    /// the native window draws through; this resolves only the *state* - the
    /// fade ramp and which bands the session's phase puts on screen.
    fn title_band_sprites(
        &self,
        session: &TitleSession,
        origin: (i32, i32),
        scale: u32,
    ) -> Vec<SpriteDraw> {
        if self.title_atlas.is_none() {
            return Vec::new();
        }
        // Fade-in dims the whole card via alpha; other phases are opaque.
        let alpha = match session.phase() {
            TitlePhase::FadeIn { frames_remaining } => {
                let total = session.fade_in_frames.max(1) as f32;
                1.0 - (frames_remaining as f32 / total).clamp(0.0, 1.0)
            }
            TitlePhase::Done(_) => return Vec::new(),
            _ => 1.0,
        };
        let mut state = ui::TitleBandState::card(alpha);
        state.press_start = matches!(session.phase(), TitlePhase::PressStart { .. });
        if let TitlePhase::MainMenu { cursor } = session.phase() {
            state.menu = Some((cursor, true));
        }
        ui::title_band_sprites(state, origin, scale)
    }

    /// The **save-screen backdrop**: the title art kept behind the Load /
    /// Save chrome at retail's dim, rather than the screen being composed
    /// over black.
    ///
    /// The page reaches retail's save-select through the pause menu's own
    /// Load row, and used to drop its title session at that hand-off, so the
    /// art went with it. The session is parked in `boot_title_backdrop`
    /// instead and drawn through the same `ui::title_band_sprites` kernel the
    /// live card uses, with `ui::TitleBandState::backdrop`'s dim. Retail
    /// pivots to pure black once a slot is confirmed, which is why the
    /// confirm phases draw nothing.
    pub fn boot_title_backdrop_draws_json(&self, surface_w: u32, surface_h: u32) -> String {
        if !self.boot_title_backdrop_visible() {
            return r#"{"active":false,"sprites":[]}"#.to_string();
        }
        let (origin, scale) = stage_transform(surface_w.max(1), surface_h.max(1));
        let sprites = ui::title_band_sprites(ui::TitleBandState::backdrop(), origin, scale);
        serde_json::json!({
            "active": true,
            "sprites": sprites.iter().map(quad_json).collect::<Vec<_>>(),
        })
        .to_string()
    }

    /// Whether the parked backdrop session owns the frame behind the menu.
    ///
    /// Alive only while the Load row's save-select is the open sub-screen,
    /// and suppressed for the two phases retail composes against black
    /// (`NowChecking` / `SlotPreview`) - the same test the native window's
    /// `title_screen_sprite_draws` makes.
    pub(crate) fn boot_title_backdrop_visible(&self) -> bool {
        self.boot_title_backdrop.is_some()
            && self.title_atlas.is_some()
            && self.play_menu_save_select_over_title()
    }
}
