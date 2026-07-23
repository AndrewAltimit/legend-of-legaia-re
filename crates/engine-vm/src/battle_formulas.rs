//! Battle damage / cost / RNG formulas.
//!
//! Clean-room Rust port of the in-game battle math. Each function is keyed
//! to a citation in `docs/subsystems/battle-formulas.md` so the provenance
//! stays traceable. None of these functions touch `FUN_800402F4`'s full
//! selector-dispatch - that lives next to the state machine in
//! [`crate::battle_action`]. This module is the **arithmetic kernel** that
//! every selector eventually feeds into.
//!
//! PORT: FUN_80056798 (PsyQ rand; full per-formula attribution lives on
//! the individual `pub fn` docs below).
//! PORT: FUN_800402F4 (selector-dispatch lives in battle_action; this
//! module ports the arithmetic kernel the dispatch feeds into).
//! PORT: FUN_801DD0AC (damage roll - both branches. Summon branch
//! (`attacker_slot == 7`): `summon_attacker_roll` / `summon_defender_roll` /
//! `summon_bonus_roll` / `summon_predamage`. Arts/physical branch
//! (`attacker_slot != 7`, seeded by the `0x801F4F5C` move-power table):
//! `arts_attacker_roll` / `arts_bonus_roll` / `arts_physical_predamage`
//! (defender roll shared with the summon branch). The live `FUN_801DDB30`
//! mitigation/finisher glue is not reproduced here - see the REF below.)
//! PORT: FUN_801DD864 (summon-roll scale stage - `apply_element_affinity` /
//! `apply_status_weaken` / `apply_magic_power`).
//! PORT: FUN_8004E568 (victory-spoils gold + EXP scaling - `victory_gold_*` /
//! `victory_exp_per_member`. The reward resolver's drop roll + level-up
//! application live in engine-core `apply_battle_loot` / `apply_battle_xp`.)
//! PORT: FUN_801DDB30 (damage finisher - the closed-form damage-finalisation
//! arithmetic (`damage_finish`: equipment elemental-resistance halving, the
//! guard halve, the no-damage `rand%9+8` floor, the summon power-percent scale,
//! the 9999 cap) + the spirit-gauge fill (`spirit_gauge_fill`). The finisher's
//! state-mutating tail - damage-popup accumulator, AI revenge table, MP drain,
//! and the per-element stat-debuff switch - reads/writes ~20 battle globals and
//! stays in the live battle context; see the REF below + `damage_finish` docs.)
//! The four routines below carry their `// PORT:` tag on the individual `pub
//! fn` that ports them (in `round.rs` / `stat_init.rs`), so they are listed
//! here as `REF:` only - a second `PORT:` line would double-count them in
//! `scripts/ci/port-catalog.py`, which counts tag occurrences.
//!
//! REF: FUN_801DA780 (per-round initiative-key seeding - `seed_initiative` /
//! `wounded_bonus` / `apply_side_lockout`. This is the battle-resident seeder;
//! the `overlay_0897_801e23ec` VA the base roll was long attributed to is an
//! aliased dump of it, and the alias dropped the wounded / Slow / ability-bit
//! terms.)
//! REF: FUN_801D88CC (per-round AGL restore - `round_reset_agility` /
//! `needs_retarget`.)
//! REF: FUN_801F0348 (target-size camera framing - `camera_height_for_frame`.)
//! REF: FUN_80053CB8 (party battle-actor stat init - `init_party_battle_stats` /
//! `equip_stat_bonuses`.)
//! REF: FUN_801E295C, FUN_801EED1C (the action-SM glue that drives the kernels
//! and applies the finisher's coupled global side effects).
//! REF: FUN_801DABA4 (the turn-order selector that consumes the seeded keys;
//! ported as `engine-core::World::next_combatant_by_initiative`).

#![allow(clippy::too_many_arguments)]

mod actor_tween;
mod arms_fold;
mod arts;
mod basic;
mod damage_finish;
mod escape;
mod round;
mod stat_init;
mod summon;
mod victory;

pub use actor_tween::*;
pub use arms_fold::*;
pub use arts::*;
pub use basic::*;
pub use damage_finish::*;
pub use escape::*;
pub use round::*;
pub use stat_init::*;
pub use summon::*;
pub use victory::*;

#[cfg(test)]
mod tests;
