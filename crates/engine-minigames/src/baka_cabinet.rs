//! The Baka Fighter **cabinet shell**: the overlay's top-level mode state
//! machine, the HUD renderer it calls every frame, and the developer
//! action-table dump one of its states reaches.
//!
//! [`crate::baka_fighter`] holds the *fight* rules - the exchange resolver,
//! the damage kernel, the CPU picker. This module holds the layer above them:
//! the attract card, the player-select screen, the per-round bracket, the
//! win / lose / all-clear sequences, the "NEXT GAME / PAY OUT" tally menu, the
//! in-duel pause menu and the developer menu. Retail runs all of it out of one
//! 2973-instruction dispatcher, `FUN_801CF388`, switching on a single
//! **cabinet-state** word (`DAT_801DBF44`).
//!
//! ## The state word is not a boolean
//!
//! `DAT_801DBF44` is documented elsewhere as "the match-active gate (`== 100`
//! while a round runs)". That is true but narrow: `100` (`0x64`) is one of
//! **37** values the dispatcher branches on, and the rest are the whole cabinet
//! sequence. `FUN_801D4FC8`'s "editor band" gate (`DAT_801DBF44 - 400 < 100`)
//! is the same word - states `0x190` / `0x191` are the developer keyframe
//! editor, which is why that band exists at all.
//!
//! Every unlisted value falls through to the shared epilogue, which draws the
//! arena and (for the in-duel states) the HUD, then decays the screen shake.
//!
//! ## Pad masks
//!
//! The cabinet reads the **packed** Legaia pad word (`_DAT_8007B874` edges,
//! `_DAT_8007B850` held), not the raw PSX layout - see
//! `legaia_engine_system::retail_pad` and the [`PACK_*`](legaia_engine_system::pad::PACK_LEFT)
//! constants. That is what makes the cursor bits `0x8000` / `0x2000` the
//! **left / right** d-pad on the horizontal screens and `0x1000` / `0x4000`
//! **up / down** on the vertical ones, while `0xF0` is "any face button".
//!
//! Provenance: `see ghidra/scripts/funcs/overlay_baka_fighter_801cf388.txt`,
//! `overlay_baka_fighter_801d2afc.txt`,
//! `overlay_baka_fighter_801d553c.txt` - disassembly, not the decompiled C
//! (the dispatcher's C renders most of its exit paths as fake
//! `FUN_801xxxxx()` label-calls).

mod action_dump;
mod cursor;
mod hud;
mod ladder;
mod machine;
mod sheet;
mod states;
mod tick;

pub use action_dump::*;
pub use cursor::*;
pub use hud::*;
pub use ladder::*;
pub use machine::*;
pub use sheet::*;
pub use states::*;

// --------------------------------------------------------------- pad masks

/// Confirm edge the cabinet's menus accept (`0x44` - Cross plus L1).
pub const CABINET_CONFIRM: u16 = 0x0044;
/// Cancel edge the cabinet's menus accept (`0x21` - Circle plus L2).
pub const CABINET_CANCEL: u16 = 0x0021;
/// Attract-screen "press to start" edge (`0x844` - Start, Cross or L1).
pub const CABINET_START: u16 = 0x0844;
/// Any of the four face buttons (`0xF0`) - the instructions screen's dismiss.
pub const CABINET_ANY_FACE: u16 = 0x00F0;
/// In-duel pause-menu edge (`0x110` - Select or Triangle). The HUD's top strip
/// reads "PRESS SELECT TO MENU"; Triangle opens it too.
pub const CABINET_DUEL_MENU: u16 = 0x0110;
/// Cursor-left edge on the horizontal screens (`0x8000`).
pub const CABINET_LEFT: u16 = 0x8000;
/// Cursor-right edge on the horizontal screens (`0x2000`).
pub const CABINET_RIGHT: u16 = 0x2000;
/// Cursor-up edge on the vertical menus (`0x1000`).
pub const CABINET_UP: u16 = 0x1000;
/// Cursor-down edge on the vertical menus (`0x4000`).
pub const CABINET_DOWN: u16 = 0x4000;
/// Developer-menu "dump the action table" edge (`0x4` - L1), state
/// [`ST_DEV_EDITOR_RUN`].
pub const CABINET_DEV_DUMP: u16 = 0x0004;
/// Developer-menu "leave the editor" edge (`0x100` - Select).
pub const CABINET_DEV_EXIT: u16 = 0x0100;
/// Held-pad bit that spawns the editor actor from the round-setup state
/// (`0x10` - Triangle).
pub const CABINET_DEV_SPAWN_HELD: u16 = 0x0010;

// ---------------------------------------------------------------- constants

/// Per-fighter starting HP the round setup writes (`0xC80`).
pub const FULL_HP: i32 = 0xC80;
/// Stage counter value at which the ladder wraps and the all-clear flag is
/// raised (`DAT_801DC10C >= 0xE`).
pub const STAGE_WRAP: i32 = 0x0E;
/// `roster_id = stage + ROSTER_FROM_STAGE`.
pub const ROSTER_FROM_STAGE: i32 = 3;
/// `mesh_index = stage + MESH_FROM_STAGE`.
pub const MESH_FROM_STAGE: i32 = 5;
/// Stage the **first** secret opponent is offered on.
pub const SECRET_A_STAGE: i32 = 5;
/// High score above which that offer fires (`0x3D090`).
pub const SECRET_A_SCORE: i32 = 250_000;
/// Stage the **second** secret opponent is offered on.
pub const SECRET_B_STAGE: i32 = 0x0D;
/// High score above which that offer fires (`0xAAE60`).
pub const SECRET_B_SCORE: i32 = 700_000;
/// Tally bonus row seeded when the opponent took **no** round (a perfect).
pub const PERFECT_BONUS: i32 = 0x7530;
/// Tally bonus row seeded otherwise.
pub const MATCH_BONUS: i32 = 0x4E20;
/// Player-select cursor span (the three party fighters).
pub const SELECT_OPTIONS: i32 = 3;
/// Rows the developer menu offers.
pub const DEV_MENU_ROWS: i32 = 5;

#[cfg(test)]
mod tests;
