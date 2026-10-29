//! The expansion game modes' screen, projected off the world's mode seat
//! once for both play hosts.
//!
//! The prim half joins the frame through
//! [`crate::screen_layers::compose_screen_prims`], which hands the whole
//! under-text list to it while the seat owns the frame; the text half is
//! [`pack_mode_stage_texts`], which each host's text layer appends and
//! scales with the rest of its stage group. Both come from
//! `legaia_engine_ui::expansion_modes`, so the two hosts draw one screen.

use legaia_engine_core::world::World;
use legaia_engine_expansion::modes::{ModeSource, ModeStage};
use legaia_engine_mode::mode::SceneMode;
use legaia_engine_ui::TextDraw;
use legaia_engine_ui::expansion_modes::{ModeScreen, mode_screen_prims, mode_screen_texts};
use legaia_render_kernels::screen_prim::ScreenPrim;

/// The seat's screen this frame, `None` unless the seat owns the frame.
pub fn pack_mode_screen(world: &World) -> Option<ModeScreen<'_>> {
    if world.mode != SceneMode::Expansion {
        return None;
    }
    let seat = world.expansion.mode_seat.as_deref()?;
    Some(match &seat.stage {
        ModeStage::Picker {
            entries,
            cursor,
            error,
        } => ModeScreen::Picker {
            heading: match seat.source {
                ModeSource::Pack => "Pack modes",
                ModeSource::Builtin => "Practice modes",
            },
            entries,
            cursor: *cursor,
            error: error.as_deref(),
        },
        ModeStage::Playing { mode, quit_prompt } => ModeScreen::Playing {
            mode,
            log: &seat.log,
            quit_prompt: *quit_prompt,
        },
        ModeStage::Results {
            name,
            kind,
            rows,
            granted,
            completed,
        } => ModeScreen::Results {
            name,
            kind: *kind,
            rows,
            granted,
            completed: *completed,
        },
    })
}

/// The seat's flat prims in display space; empty unless it owns the frame.
pub fn pack_mode_prims(world: &World) -> Vec<ScreenPrim> {
    pack_mode_screen(world)
        .map(|s| mode_screen_prims(&s))
        .unwrap_or_default()
}

/// The seat's labels in **stage** pixels (the host scales them with its
/// stage group); empty unless it owns the frame.
pub fn pack_mode_stage_texts(world: &World, font: &legaia_font::Font) -> Vec<TextDraw> {
    pack_mode_screen(world)
        .map(|s| mode_screen_texts(font, &s))
        .unwrap_or_default()
}
