//! The browser play page's half of the dance minigame's **retail art**: the
//! residency claim over the hall's HUD texture page, and the count-in banner
//! as screen-space PSX primitives.
//!
//! The native window's twin is `window/minigames.rs`'s `stage_dance_hud_art`
//! plus `redraw`'s `dance_countin_prims`. The two hosts stage the same rects
//! through the same kernel
//! ([`legaia_engine_core::dance::stage_dance_hud_vram`]) and emit the same
//! quads through the same builder
//! ([`legaia_engine_ui::ui_dance::dance_countin_prims`]); what differs is only
//! *where the VRAM lives*, and that is a genuine per-host fact.
//!
//! # The two hosts were not equally far from this
//!
//! It reads as one gap - "the count-in banner is text" - and it was two. The
//! native window hosts the dance over the scene the player walked in from and
//! keeps drawing that scene behind the HUD, so it had neither the page nor a
//! quad emit. This page already replaces its whole VRAM texture for the dance
//! (`play_mg_dance_body_vram`, the `other7` scene's own upload, restored on
//! exit through `play_mg_take_vram_restore`) and already draws the hall, so it
//! had the texels all along and only ever lacked the emit. A gap stated once
//! for "both hosts" can be two different gaps, and the cheaper one gets fixed
//! by the fix for the dearer one without anybody noticing which was which.
//!
//! The residency **predicate** is therefore shared even though the staging is
//! not: `World::minigames.dance_hud_art_staged` is written by whichever host
//! owns the VRAM and read by both draw paths, so the choice between retail's
//! sprite and the placeholder letterforms is one decision rather than two
//! spellings of "did the upload work".

use crate::runtime::LegaiaRuntime;
use legaia_engine_core::world::SceneMode;

impl LegaiaRuntime {
    /// Publish this frame's dance residency claim into the world, so the
    /// shared draw predicate is answered on this host too.
    ///
    /// The page's dance VRAM is built with the hall, not per frame, so the
    /// claim is simply whether that build landed the HUD rects
    /// (`LegaiaMinigames::dance_hud_art_staged`). Cleared the moment the
    /// dance is not the world's mode, because the page puts the field texture
    /// back on that same edge.
    pub(crate) fn sync_dance_hud_residency(&mut self) {
        let staged = self
            .minigame_art()
            .is_some_and(|a| a.dance_hud_art_staged())
            && self
                .scene_host
                .as_ref()
                .is_some_and(|h| h.world.mode == SceneMode::Dance);
        if let Some(host) = self.scene_host.as_mut() {
            host.world.minigames.dance_hud_art_staged = staged;
        }
    }

    /// The dance count-in banner as screen-space PSX primitives, for the
    /// page's own prim pass.
    ///
    /// Empty outside the count-in and empty without the page, which is when
    /// `play_minigames`'s text builder draws the placeholder instead - the
    /// same either/or the native window applies, off the same world flag.
    pub(crate) fn dance_countin_prims(&self) -> Vec<legaia_engine_ui::screen_prim::ScreenPrim> {
        use legaia_engine_ui::ui_dance as ud;
        let Some(host) = self.scene_host.as_ref() else {
            return Vec::new();
        };
        let mg = &host.world.minigames;
        if !mg.dance_hud_art_staged {
            return Vec::new();
        }
        let Some(env) = mg.dance_countin_banner.as_ref() else {
            return Vec::new();
        };
        let art = mg
            .dance
            .as_ref()
            .and_then(|g| g.widget(0))
            .map(|(w, abr)| ud::DanceCountInArt::from_widget(&w, abr))
            .unwrap_or_default();
        ud::dance_countin_prims(
            ud::DanceCountInView {
                x_offset: env.x_offset,
                brightness: env.brightness,
                hold: env.hold,
            },
            art,
            ud::COUNTIN_OT,
        )
    }
}
