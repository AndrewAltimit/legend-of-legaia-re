//! From-scratch Rust **fishing minigame** rules engine, written from the
//! disassembly - no retail bytes are reproduced here.
//!
//! A port of the confirmed numeric kernels of the fishing overlay (PROT 0972,
//! `data\OTHER1`) - the casting-power oscillator, the tension-gauge tug-of-war,
//! and the catch-scoring / persistent-record model - composed into an
//! interactive fight session. It consumes reel input and a per-frame fish pull
//! and produces a running fight + a scored catch, driven by the already-parsed
//! per-species table ([`legaia_asset::fishing_species`]).
//!
//! What is **Confirmed** (byte / formula pinned in
//! [`docs/subsystems/minigame-fishing.md`](../../../docs/subsystems/minigame-fishing.md)):
//! - the casting-power bounds `0x20..=0x1000` and its `0x40` seed (states `0x14`
//!   / `0xa`);
//! - the tension-gauge update: reel-held divisors `rod*9 + 0x23` (button `0x40`)
//!   / `rod*6 + 0x19` (button `0x80`), reel-released decrement
//!   `(rod*0x40 + 0x4a) * frame_step`, and the `[0, 0x1000]` clamp
//!   (`FUN_801d4004` tail);
//! - the catch award `value * (strength + 0x9c0) / 0x32000`, the `999999`
//!   persistent-point cap, and the best-catch (value + fish id) update
//!   (`FUN_801d5298`); the award itself is [`FishingSpecies::score_for`].
//!
//! What is an **engine-side reconstruction** (the retail win/lose conditions are
//! in this module's [Open](../../../docs/subsystems/minigame-fishing.md#open)
//! list): [`PondSession`] composes the confirmed kernels into the retail
//! cast -> wait -> strike -> fight -> score loop, and the glue it adds (flight
//! timing, the line-record reel-down rates, the snap-at-max-tension loss) is
//! marked at each call site. No Sony bytes are baked in - the species, spawn
//! and cadence tables decode from the user's disc ([`FishingTables`]).
//!
//! Chain: retail `FUN_801cf3bc` (mode SM) -> `FUN_801d4004` (fish-AI + tension)
//! -> `FUN_801d5298` (catch scoring).
//!
//! # One session type
//!
//! [`PondSession`] is the only fishing session. All three hosts run it: the
//! native window and the browser play page through `World::enter_fishing` /
//! `World::tick_fishing` (the mode-24 door warp and each host's debug
//! launcher), the minigames page directly. The persistent save-block words it
//! reads and writes back (lure, rod, cast counter, point record, one-time
//! prize mask) live on `World::minigames` between sessions.
//!
//! # Scope
//!
//! This module is the **rules** half only: [`PondSession`] and the kernels
//! it drives ([`CastPower`], [`TensionGauge`], [`FishingRecord`],
//! [`PrizeExchange`]) are called from `world`'s minigame dispatch, which is
//! how the fishing minigame runs.
//!
//! The **presentation** half - the persistent / catch HUD layout, the gauge
//! bars, the digit field and the banner animators - lives in
//! `legaia_engine_ui::ui_fishing`, next to the consumer that renders it, in
//! line with the project's split between simulation (this crate) and
//! renderer-agnostic draw-list builders (`engine-ui`).

use legaia_asset::fishing_species::FishingSpecies;
use legaia_asset::fishing_species::{CADENCE_TOLERANCE, CadenceTemplate};

use crate::levelup::BiosRand;

mod gauges;
mod pond;
mod pond_types;
mod prize;
mod rod_menu;
mod species;

pub use gauges::*;
pub use pond::*;
pub use pond_types::*;
pub use prize::*;
pub use rod_menu::*;
pub use species::*;

/// Tension-gauge ceiling (`FUN_801d4004`: clamp high at `0x1000`). The line
/// depth `DAT_801d9298` is clamped to the same range.
pub const TENSION_MAX: i32 = 0x1000;

/// Divisor of the per-frame pull and line-sink terms (`FUN_801d4004`:
/// `pull * factor / 150`).
pub const SINK_DIVISOR: i32 = 150;
/// Tension-gauge floor (`FUN_801d4004`: clamp low at `0`).
pub const TENSION_MIN: i32 = 0;

/// Casting-power oscillator low bound (`FUN_801cf3bc` state `0x14`).
pub const CAST_POWER_MIN: i32 = 0x20;
/// Casting-power oscillator high bound (`FUN_801cf3bc` state `0x14`).
pub const CAST_POWER_MAX: i32 = 0x1000;
/// Casting-power seed at run-loop init (`FUN_801cf3bc` state `0xa`).
pub const CAST_POWER_SEED: i32 = 0x40;

/// Persistent fishing-point cap (`FUN_801d5298`: `_DAT_8008444c` clamped to
/// `999999`). The HUD row clamps to the same literal, one copy per crate -
/// `legaia_engine_ui::ui_fishing::HUD_POINT_CAP`.
pub const FISH_POINTS_CAP: i32 = 999_999;

/// Reel-held tension divisor for the `0x40` reel button: `rod*9 + 0x23`.
pub const REEL_A_DIV_MUL: i32 = 9;
/// Additive term of the `0x40`-button reel divisor.
pub const REEL_A_DIV_ADD: i32 = 0x23;
/// Reel-held tension divisor for the `0x80` reel button: `rod*6 + 0x19`.
pub const REEL_B_DIV_MUL: i32 = 6;
/// Additive term of the `0x80`-button reel divisor.
pub const REEL_B_DIV_ADD: i32 = 0x19;
/// Reel-released tension decrement multiplier: `(rod*0x40 + 0x4a) * frame_step`.
pub const REEL_RELEASE_MUL: i32 = 0x40;
/// Additive term of the reel-released decrement.
pub const REEL_RELEASE_ADD: i32 = 0x4a;

/// Packed-pad bit of the reel-A button (Cross) in the retail held word
/// `_DAT_8007b850` - the mask [`ReelInput::from_pad_mask`] decodes.
pub const REEL_A_PAD_BIT: u32 = 0x40;
/// Packed-pad bit of the reel-B button (Square) in the retail held word.
/// **Not** Circle - `0x20` is the cast/hook input.
pub const REEL_B_PAD_BIT: u32 = 0x80;

/// Packed-pad bit of D-pad right in the retail held word: rolls the rod
/// right (`andi v0,a0,0x2000` at `0x801D38B0`).
pub const ROD_PAD_RIGHT: u32 = 0x2000;
/// Packed-pad bit of D-pad down: lifts the rod (`andi v0,v0,0x4000` at
/// `0x801D2AC0`).
pub const ROD_PAD_DOWN: u32 = 0x4000;
/// Packed-pad bit of D-pad left: rolls the rod left (`andi v0,a0,0x8000` at
/// `0x801D38C4`).
pub const ROD_PAD_LEFT: u32 = 0x8000;

#[cfg(test)]
mod tests;
