//! Browser **pause menu**: the real retail field-menu, rendered from the same
//! `legaia-engine-ui` draw builders the native `play-window` uses.
//!
//! The play page's original pause menu was a lightweight DOM overlay. This
//! module replaces it with the byte-pinned retail chrome: the gold 9-slice
//! window frames + navy filigree come from the disc's menu-UI atlas (PROT 0899
//! plus the PROT.DAT system-UI sheet, assembled by
//! [`legaia_engine_menus::save_menu_atlas::build_atlas`]), the glyphs from the
//! proportional dialog font, and every rectangle from the shipped
//! `*_draws_for` functions in `legaia-engine-ui`. The window geometry is the
//! disc-parsed descriptor table ([`legaia_game_tables::menu_windows`]), with the
//! same pinned fallback the native window keeps.
//!
//! The **root command list is the shared retail picker**
//! ([`FieldMenuSession`]), not a page-local cursor - and it is the engine
//! session's own (`BootSession::field_menu`), not a page copy:
//! [`LegaiaRuntime::play_menu_open`] hands the press to
//! `BootSession::press_field_menu`, the call the native window makes, which
//! seeds the picker with the world's money + play time, samples the two row
//! gates retail reads at every draw into a [`FieldMenuGate`](legaia_engine_menus::field_menu::FieldMenuGate) (the op-`0x49`
//! entry context and the scene MAN's save-allow bit) and switches the world
//! into [`SceneMode::Menu`](legaia_engine_mode::mode::SceneMode::Menu). So the open rule, the row ink and the confirm
//! routing are one engine answer on both hosts, and a row cannot draw white
//! here and buzz there - which it did, letting a player Save in a scene whose
//! own data forbids it.
//!
//! The page drives it exactly like the field: hand it edge-triggered pad words,
//! then blit the two draw lists (sprites off the chrome atlas, texts off the
//! font atlas) over the frozen scene. Every row - the top-level command list
//! plus the Items / Magic / Equip / Status / Options / Load / Save
//! sub-screens - runs the real [`FieldMenuSubsession`] the native
//! `play-window` builds, and renders through the identical
//! `legaia-engine-ui` draw builders; the site is just a different framebuffer
//! over the same menu.
//!
//! ## What this module still decides
//!
//! Not the composition. Which windows a screen frames, which painter draws its
//! title tab, where the modals sit in the sprite order and the final 320x240
//! stage scale all live in
//! [`legaia_engine_ui::pause_menu`](legaia_engine_ui::pause_menu), shared with
//! the native window - this file used to carry a private copy, and two
//! divergences had already grown in it (the tab painter and the Use-confirm
//! frame order). What is left here is the **projection**: turning a live
//! `engine-core` session into the plain view structs the composition takes,
//! which needs `engine-core` types the wgpu-free UI crate deliberately does
//! not depend on.
//!
//! ## Load / Save
//!
//! Load and Save drive the retail save-select screen ([`SaveSelectSession`],
//! `docs/subsystems/save-screen.md`) against the page's **memory-card rack**
//! ([`crate::cards`]) in its two-stage card-slots mode:
//!
//! 1. **Browsing** - the `SLOT 1` / `SLOT 2` pills are the console's two
//!    memory-card ports. A pill is selectable when the page has inserted a
//!    card image there.
//! 2. **NowChecking** - the "Now checking. Do not remove MEMORY CARD" dialog
//!    slides in while the card is read.
//! 3. **SlotPreview** - the card's fifteen blocks as retail's 5x3 portrait
//!    grid, with the focused block's info panel sliding up underneath.
//! 4. Confirming loads that block into the live world, or (Save) raises the
//!    overwrite prompt and then writes the session into the card image.
//!
//! None of that flow lives here. The grid cursor, the once-per-port card
//! read, the rule that a Load may not confirm an empty block and the mapping
//! from a finished session back to `(port, cell)` are
//! [`legaia_engine_menus::save_screen::SaveScreenFlow`] - one kernel both this
//! page and the native `play-window` drive, so the two cannot disagree about
//! what the screen addresses. This host supplies only the **bytes**: the
//! fifteen block snapshots behind a port, and the load / write against the
//! card image.

use super::*;
use crate::runtime::LegaiaRuntime;
use legaia_engine_menu_runtime::field_menu_dispatch::{self, FieldMenuSubsession};
use legaia_engine_menus::field_menu::{FieldMenuOutcome, FieldMenuRow, FieldMenuSession};
use legaia_engine_menus::save_menu_atlas::{SaveMenuAtlas, build_atlas};
use legaia_engine_menus::save_screen::{SaveCommitKind, SaveRefusal, SaveScreenFlow};
use legaia_engine_menus::save_select::SaveSelectSession;
use legaia_engine_system::input::PadButton;
use legaia_engine_ui::{SaveMenuAtlasRects, SpriteDraw, TextDraw};

use legaia_engine_screens::pause_screens;
/// Stage origin + integer scale that upscales the 320x240 boot-UI stage to
/// fill the play surface, centred. Shared with the native window rather than
/// mirrored: `legaia_engine_ui::pause_menu::stage_transform`.
pub(crate) use legaia_engine_ui::pause_menu::stage_transform;
use legaia_engine_ui::pause_menu::{
    MenuRects, PauseMenuCtx, PauseMenuDraws, art_learned_notice_draws, spell_level_notice_draws,
};

mod assets;
mod card_io;
mod input;
mod open_close;
mod panel_draws;
mod sub_screen_draws;

/// The disc-sourced menu chrome (assembled atlas + its band rects) plus the
/// disc-parsed window-descriptor table. Built once, lazily, the first time the
/// menu opens (needs the loaded PROT for the atlas).
pub struct PlayMenuAssets {
    font: legaia_font::Font,
    chrome: Option<(SaveMenuAtlas, SaveMenuAtlasRects)>,
    windows: Option<legaia_game_tables::menu_windows::MenuWindowTable>,
}
/// Active pause-menu state: the shared root-list state machine plus the open
/// sub-screen, if any.
pub struct PlayMenu {
    // The retail root command picker is not held here: it is the session's
    // own `BootSession::field_menu`, opened and closed through
    // `BootSession::press_field_menu` / `open_field_menu` /
    // `close_field_menu` exactly as the native window opens and closes it.
    // The page used to keep a private `FieldMenuSession` plus the resume
    // mode and re-spell the open (gates, mode switch, seat) beside the
    // session's copy - two answers to one press. This struct holds only what
    // the page's sub-screen driver adds on top.
    sub: Option<PlaySub>,
    /// The save screen's shared driver: block-grid cursor + the card read
    /// behind it. Lives in `engine-core` so this page and the native window
    /// step the same cursor and gate the same confirms.
    save_flow: SaveScreenFlow,
    /// CDNAME label of the scene an in-canvas card Load landed in, waiting for
    /// the page to pick it up ([`LegaiaRuntime::play_menu_take_load_scene`]).
    /// Retail resumes the save in the scene it was written in; the page owns
    /// scene entry, so the menu parks the label here.
    pending_load_scene: Option<String>,
    /// Engine key **name** of the page's most recent physical key-down
    /// ([`LegaiaRuntime::play_menu_key`]), consumed once by the Options
    /// sub-session's Key Config screen.
    ///
    /// The pad word cannot carry this: the screen exists to bind keys that
    /// are *not* in the table yet, and an unbound key has no bit. The native
    /// window latches the same thing in its `pending_key_name`.
    pending_key: Option<String>,
    /// Opened by the boot title's Options row, not by Start: the sub-screen's
    /// exit closes the whole menu, so the title comes back instead of a root
    /// picker the title never showed. The native window keeps the same flag
    /// (`menu_from_title`). Continue is no pause row on either host
    /// (`crate::boot_continue`).
    ///
    /// A card Load that succeeded parks its scene label on this struct for
    /// the page to collect; the exit then leaves the menu open so the label
    /// survives until `play_menu_take_load_scene`, and the page closes the
    /// menu itself once it has it.
    from_title: bool,
    /// The menu owns the sim tick ([`LegaiaRuntime::play_menu_tick`]): set on
    /// the tick after the pause wipe spawns it, or at the open when no wipe
    /// runs (the boot title's rows). Until then the field ticks under the
    /// wipe, as it does in the native window.
    owns_frame: bool,
}

/// The open sub-screen. Every row runs the real [`FieldMenuSubsession`] the
/// native `play-window` builds, and renders through the exact same
/// `legaia-engine-ui` draw builders.
enum PlaySub {
    // Boxed: the sub-session enum is large, and this is a per-menu allocation.
    Session(Box<FieldMenuSubsession>),
}

impl PlayMenu {
    fn new() -> Self {
        PlayMenu {
            pending_key: None,
            sub: None,
            save_flow: SaveScreenFlow::new(),
            pending_load_scene: None,
            from_title: false,
            owns_frame: false,
        }
    }
}

/// `(edge & button)` test on a PSX-encoded pad-edge word.
fn pressed(edge: u16, b: PadButton) -> bool {
    edge & b.mask() != 0
}
/// Serialize one draw quad to JSON. `TextDraw` and `SpriteDraw` are the same
/// shape (`dst` / `src` rect + RGBA tint); the page samples the font atlas for
/// quads in the `texts` list and the chrome atlas for the `sprites` list.
pub(crate) fn quad_json(d: &TextDraw) -> serde_json::Value {
    serde_json::json!({
        "dst": [d.dst.0, d.dst.1, d.dst.2, d.dst.3],
        "src": [d.src.0, d.src.1, d.src.2, d.src.3],
        "color": [d.color[0], d.color[1], d.color[2], d.color[3]],
    })
}

/// Test-only probes for the disc-gated pause-menu oracles
/// (`tests/menu_parity.rs`). Native-only so the wasm export surface the page
/// consumes stays exactly the player-facing API.
#[cfg(not(target_arch = "wasm32"))]
impl LegaiaRuntime {
    /// The runtime's live [`legaia_engine_menus::options::OptionsState`] as
    /// JSON - what the next Options open seeds itself from.
    pub fn debug_options_json(&self) -> String {
        serde_json::to_string(&self.options_state).unwrap_or_default()
    }

    /// The **open** Options sub-session's own state as JSON, or `None` when
    /// that screen is not the one holding the pad. Comparing it against
    /// [`Self::debug_options_json`] is what proves the seed is the runtime's
    /// state rather than a fresh default.
    pub fn debug_open_options_json(&self) -> Option<String> {
        let m = self.play_menu.as_ref()?;
        let PlaySub::Session(session) = m.sub.as_ref()?;
        match session.as_ref() {
            FieldMenuSubsession::Config(o) => serde_json::to_string(o.state()).ok(),
            _ => None,
        }
    }
}

#[cfg(test)]
mod blip_tests {
    use super::*;

    /// The menu's blip rides the edge the refusal box let through: the press
    /// that dismisses a refusal is silent, as on the native window, and an
    /// ordinary root-list press still blips exactly once.
    #[test]
    fn a_press_the_refusal_box_swallows_does_not_blip() {
        let mut rt = LegaiaRuntime::new();
        let host = legaia_engine_scene_host::scene_host::SceneHost::from_prot_bytes(
            crate::play_fmv::tests::synthetic_prot(),
            None,
        )
        .expect("synthetic host");
        rt.scene_host.install(host, None).expect("session");
        rt.scene_host
            .session_mut()
            .expect("session")
            .open_field_menu();
        rt.play_menu = Some(PlayMenu::new());
        let down = PadButton::Down.mask();
        let cross = PadButton::Cross.mask();
        rt.play_menu_input(down);
        assert_eq!(rt.sfx.menu_cue_requests, 1, "a root cursor move blips");
        rt.play_menu
            .as_mut()
            .unwrap()
            .save_flow
            .refuse(SaveRefusal::CardWriteFailed);
        rt.play_menu_input(cross);
        assert_eq!(
            rt.sfx.menu_cue_requests, 1,
            "the dismissing press is silent"
        );
        assert!(rt.play_menu_blips_inline());
    }
}
