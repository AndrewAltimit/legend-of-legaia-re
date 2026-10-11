//! From-scratch **Baka Fighter duel minigame** rules engine.
//!
//! A faithful port of the fight logic in the Baka Fighter overlay (PROT 0976):
//! the rock-paper-scissors exchange resolver, the HP-tiered damage kernel, the
//! comeback-critical roll, the scripted-pattern CPU move picker, and the
//! best-of-3 round/match bookkeeping - driven by the already-parsed roster +
//! action tables ([`legaia_minigame_formats::baka_opponents`]). This is the *rules*
//! layer: it consumes chosen attack types (pad presses on the player side)
//! and produces resolved exchanges, damage, round wins and the gold prize,
//! exactly as the retail overlay does. The duel's 3D presentation (fighter
//! meshes, clips, banners, HUD) is a host concern and is not covered here.
//!
//! Every formula and constant is the reading from
//! [`docs/subsystems/minigame-baka-fighter.md`](../../../docs/subsystems/minigame-baka-fighter.md),
//! re-derived from the overlay dumps cited on each item.
//!
//! **Exchange timing.** A fight built from the disc tables
//! ([`BakaFight::from_tables`]) carries each fighter's action-record speed and
//! strike-frame column ([`StrikeTable`]) and runs retail's frame cursor over
//! the chosen attack ([`StrikeClock`]): the exchange is booked on the tick the
//! winner's strike keyframe is crossed. The special (type 4) is no player
//! choice: it is retail's auto-finisher, thrown by whoever just put the foe at
//! 0 HP, and the round is won by its *last* strike landing - a special with
//! strikes left keeps playing after its first. What the port still does not model is the clip **tail** - retail
//! plays each attack clip to its ANM frame count and only then drops back to
//! idle, while the port clears the exchange once it is booked and paces
//! re-entry with the retail cooldown decay. A fight built from bare configs
//! ([`BakaFight::new`]) has no strike data and resolves the tick both sides
//! have chosen; with no special clip to play, its knockout ends the round on
//! the spot.
//!
//! Chain: retail `FUN_801d3468` (match resolution SM) → `FUN_801d3a14`
//! (exchange win-condition) → `FUN_801d3b18` (damage) → `FUN_801d6660`
//! (comeback-crit roll); CPU picks via `FUN_801d487c`.

mod cabinet_flow;
mod digit_cells;
mod exchange_rules;
mod fight;
mod fighter;
mod fighter_pack;
mod frame_tick;
mod hud_widgets;
mod score;
mod strike_clock;
mod tally;

pub use cabinet_flow::*;
pub use digit_cells::*;
pub use fight::*;
pub use fighter::*;
pub use fighter_pack::*;
pub use hud_widgets::*;
pub use score::*;
pub use strike_clock::*;
pub use tally::*;

/// Starting HP each round (`FUN_801d1744` round seed: `DAT_801dbfc4 = 0xc80`).
pub const HP_START: i32 = 0xC80;

/// HP at/above this uses stat tier `[0]` (the `0x8c1` threshold).
pub const HP_TIER_HIGH: i32 = 0x8C1;

/// HP at/above this (but below [`HP_TIER_HIGH`]) uses tier `[1]` (`0x3c1`).
pub const HP_TIER_MID: i32 = 0x3C1;

/// The comeback-crit roll only fires while `0 < HP < 0x280` (`FUN_801d6660`).
pub const CRIT_HP_BAND: i32 = 0x280;

/// Per-consecutive-hit damage bonus step (`(combo - 1) * 0x40`).
pub const COMBO_DAMAGE_STEP: i32 = 0x40;

/// Post-exchange cooldown seed (`FUN_801d3468` writes 200).
pub const COOLDOWN_RESET: i32 = 200;

/// Cooldown decay per frame step (`cooldown -= frame_step * 0x10`).
pub const COOLDOWN_DECAY: i32 = 0x10;

#[cfg(test)]
mod tests;
