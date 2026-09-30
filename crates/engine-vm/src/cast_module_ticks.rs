//! The **code** half of the slot-B cast-module band (PROT 0903..0966): the
//! thirteen routines `docs/subsystems/cast-module.md`'s worklist grades
//! **PORT** because they read or write simulation state rather than only
//! handing spawn records to the pool spawner.
//!
//! The band's DATA half - the arm switches whose arms do nothing but call
//! `FUN_80021B04` / `FUN_80050ED4` / `FUN_801DFDF0` / `FUN_80024E80` with a
//! module-resident record pointer - is already the engine's
//! (`legaia_asset::cast_effect_pool`, staged by
//! `World::spawn_cast_module_fx`). What no record can express, and what this
//! module carries, is the state those routines touch:
//!
//! | retail field | what it is | mirror here |
//! |---|---|---|
//! | `+0x0C` | root-speed / magnitude word | [`CastActorState::root_speed`] |
//! | `+0x10` | pending HP-bar delta | [`CastActorState::hp_bar_delta`] |
//! | `+0x14C` | live HP | [`CastActorState::hp`] |
//! | `+0x16E` | flag bank (bit `0x4` = non-targetable) | [`CastActorState::flags`] |
//! | `+0x1D9` / `+0x1DA` / `+0x1DC` | playing clip / staged clip / restage bump | [`CastActorState::playing_anim`] / [`CastActorState::staged_anim`] / [`CastActorState::restage`] |
//! | `+0x1DD` | active-target byte | [`CastActorState::target_code`] |
//! | `+0x1F1` | the victim's own knockdown-reaction id | [`CastActorState::knockdown_anim`] |
//! | `+0x154` / `+0x156` | AGL working / base (the action gauge) | [`CastActorState::agl`] / [`CastActorState::agl_base`] |
//! | `+0x21C` / `+0x21D` | render flag / animation-rate scalar | [`CastActorState::render_flag`] / [`CastActorState::anim_rate`] |
//! | ctx `+0`, `+1` | party count, monster count | [`CastModuleCtx::party_count`] / [`CastModuleCtx::monster_count`] |
//! | ctx `+0x13` | caster seat (NOT the summon band's wrapper `a1`) | [`CastModuleCtx::caster_seat`] |
//! | ctx `+0x278` | module scratch byte | [`CastModuleCtx::ctx_278`] |
//! | ctx `+0x279` | the module phase | [`CastModuleCtx::phase`] |
//!
//! Every routine here is read off its **owning** image, not off whichever
//! dump prints at the VA: a band image ends in a byte-identical copy of a
//! sibling's tail, so seven of these addresses were catalogued against an
//! image that only holds the residue
//! (`docs/subsystems/cast-module.md#a-module-image-ends-in-another-images-bytes`).
//!
//! ## What is ported, and what is not
//!
//! Ported per routine, byte-exactly: the dispatch bound (the `sltiu`
//! immediate, or the `beq`/`slti` chain's span), the arm map where the head
//! is a word table, the simulation-state writes, the damage step where the
//! routine has one - its **baked** power constant, the wrapper it calls, its
//! clamp shape, the `+0x10` accumulate, the HP write, the reaction stage and
//! the anim-rate write - and the phase advance.
//!
//! Not ported, and disclosed per item: the GPU-packet arms, the camera arms,
//! and, for the five tick bodies whose arm map is a `beq` chain or a
//! 256-entry table, the per-arm frame gating that decides *when* each step
//! fires. That is per-phase timing and it is pinned by capture, not by the
//! static window. A tick body is 451 to 2074 instructions and most of that is
//! packet emission.
//!
//! ## Two clamp shapes, not one
//!
//! `docs/subsystems/cast-module.md` describes a single apply shape for the
//! band ("clamp the roll against HP `+0x14C`, accumulate into `+0x10`, write
//! HP back"). The bytes carry **two**, and they differ in whether the hit can
//! kill:
//!
//! * [`apply_hit_floor_zero`] - `sltu hp, dmg` then `dmg = hp`: an
//!   **unsigned** clamp to HP, so the victim can be left at 0. PROT 0945,
//!   0957 (both tick bodies), 0958, 0960.
//! * [`apply_hit_floor_one`] - `addiu cap, hp, -1; slt cap, dmg` then
//!   `dmg = cap`: a **signed** clamp to `HP - 1`, so the hit can never kill
//!   and a *negative* roll heals. PROT 0927 (Juggernaut) and PROT 0966 (Evil
//!   Seru Magic) - the two band-wide AoE stagers.
//!
//! Shape B is a property of those two **stagers**, not of their images. Both
//! images also hold a tick body whose own damage site takes shape A: PROT
//! 0927's at `0x801F7E0C` / clamp `0x801F7E38` (same baked `0x12`), and PROT
//! 0966's at `0x801F8610` / clamp `0x801F863C` with a **different** baked
//! power, `0x327` rather than the stager's `0x100`. The band has exactly two
//! shape-B sites - `0x801F8758` (0927) and `0x801F8F08` (0966) - and every
//! other damage-wrapper call in PROT 0903..0966 clamps with `sltu`. So an
//! entry-keyed lookup ([`damage_shape_for`]) answers for the module's
//! **stager**; a tick's magnitude and kill-capability must come from the tick.
//!
//! The unsigned comparison in the first shape is load-bearing: the wrappers
//! return `attacker_roll - defender_roll` as a signed word, and a negative
//! result reads as a huge unsigned value, so `sltu` fires and the clamp
//! rewrites the damage to the victim's whole HP. A negative roll on those
//! modules therefore **kills outright** rather than healing.
//!
//! Provenance: disassembly of each owning image at slot-B base
//! `0x801F69D8` (`scripts/ghidra-analysis/disasm-overlay-fn.py
//! extracted/overlays/overlay_<label>_<entry>.bin --base 0x801F69D8 --addr
//! <va>`), plus PROT 0898's three entry tables for the owner attribution
//! (`docs/subsystems/cast-module.md#the-entry-tables-and-where-the-addresses-live`).

use crate::battle_damage_wrappers::{
    WrapperAttacker, WrapperDefender, atk_wrapper_predamage, int_wrapper_predamage,
    wrapper_net_damage,
};

mod arms;
mod arms_0955;
mod capture;
mod idioms;
mod power;
mod stagers;
mod state;
mod ticks;

pub use arms::*;
pub use arms_0955::*;
pub use capture::*;
pub use idioms::*;
pub use power::*;
pub use stagers::*;
pub use state::*;
pub use ticks::*;

/// Link base every band image is loaded at (`0x801F69D8`), and therefore the
/// base every VA in this module is printed under.
pub const CAST_MODULE_LINK_BASE: u32 = 0x801F_69D8;

/// The summon seat the stagers pose: `actor_table[7]`, reached in the bytes as
/// `lw rX, 0x1C(0x801C9370)`.
pub const SUMMON_SEAT: u8 = 7;

/// First monster seat - where the Juggernaut sweep starts
/// (`addiu s4, zero, 0xc; addu s2, s4, actor_table` = `&table[3]`).
pub const FIRST_MONSTER_SEAT: u8 = 3;

/// `+0x16E` bit `0x4` - "non-targetable". Both AoE stagers skip a victim
/// carrying it (`lhu v0,0x16e(v); andi v0,v0,4; bnez`).
pub const FLAG_NON_TARGETABLE: u16 = 0x0004;

/// The `+0x1DD` group code the Viguro stager writes onto the summon seat:
/// `9`, the enemy row (`addiu v0,zero,9; sb v0,0x1dd(a1)` at `0x801F7B98`).
pub const TARGET_CODE_ENEMY_ROW: u8 = 9;

/// The animation-rate scalar a seat runs at normally.
pub const ANIM_RATE_NORMAL: u8 = 8;

#[cfg(test)]
mod tests;
