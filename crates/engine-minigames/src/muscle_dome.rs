//! The World-free half of the **Muscle Dome** rules: the course ladder and
//! per-round score tables parsed out of the arena overlay (PROT 0977), the
//! contest ladder above a leg ([`DomeContest`] - entry word, course lengths,
//! the casino-coin tally and its settlement), the hub screen's fade / hold
//! envelopes, and the constants the leg session shares with them.
//!
//! The leg session itself (`MuscleDomeSession`), its command menu and the
//! fighter loadouts read live `World` state and the battle command screen,
//! so they stay in `legaia-engine-core::muscle_dome`, which re-exports this
//! module's items at their old paths. See
//! [`docs/subsystems/minigame-muscle-dome.md`](../../../docs/subsystems/minigame-muscle-dome.md).

// The flag-bank primitives the contest settlement calls. The port returns the
// flag *decisions* to its caller (`ContestSettlement`) instead of writing a
// bank itself, so the addresses are references rather than ports.
// REF: FUN_8003ce08 (set a system flag - the 0x50A / 0x35 / 0x130+course arms)
// REF: FUN_8003ce34 (clear a system flag - both are cleared before settling)
// REF: FUN_8003ce64 (read a system flag - the course, length and prize gates)
// REF: FUN_800421d4 (give item - the one-shot War God Icon award)

use legaia_asset::element_affinity::ElementAffinity;
use legaia_asset::move_power::{self, MoveRecord};
use legaia_engine_vm::battle_formulas::{
    DamageFinish, DefenderResist, SummonRollActor, arts_physical_predamage_lazy,
    damage_finish_lazy, spirit_gauge_fill, world_rand,
};

mod contest;
mod course;
mod damage;
mod hub;

pub use contest::*;
pub use course::*;
pub use damage::*;
pub use hub::*;

/// Deal size (the retail deal loop builds exactly four slots, one per
/// direction).
pub const HAND_SLOTS: usize = legaia_asset::muscle_dome::HAND_SLOTS;

/// Queue capacity: the turn's first commit zeroes `actor+0x1df..+0x1ee`
/// (16 bytes), bounding the per-turn queue.
pub const QUEUE_CAP: usize = 0x10;

/// The HP-Left readout's scale: `hp * 100 / max_hp`, a plain percentage. The
/// retail expression is the MIPS shift-add chain `((hp<<1 + hp)<<3 + hp)<<2`
/// at `0x801d0f38..0x801d0f4c`.
pub const HP_LEFT_SCALE: i32 = 100;

/// The fighter slot the HUD's HP-Left readout reads: retail takes it off
/// `DAT_801c937c`, actor-table index 3 - the first **enemy** slot (the party
/// occupies 0..=2). Slot 1 is this port's opponent.
pub const HP_LEFT_SLOT: usize = 1;

/// Spell-name id base for the reward (`ctx+0x269 + 0x80`, the player
/// Seru-magic block of the shared spell table).
pub const REWARD_SPELL_ID_BASE: u8 = 0x80;

#[cfg(test)]
mod contest_start_restore_tests;
