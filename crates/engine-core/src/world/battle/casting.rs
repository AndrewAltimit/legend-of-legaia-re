//! Spell / summon / monster-special damage resolution and element affinity.

use super::*;

mod element_affinity;
mod outcome_fold;
mod power_and_xp;
mod predamage;
mod spell_resolution;

/// The boss signature casts whose streamed capture-class module calls the
/// **resist-bypass** damage wrapper `FUN_801DD6B4` instead of the respecting
/// `FUN_801DD4B0`.
///
/// The two wrappers differ in exactly one thing: the fifth argument they push
/// for the shared finisher `FUN_801DDB30`. At the `jal` sites in the battle
/// overlay dump (`ghidra/scripts/funcs/overlay_battle_action_801dd4b0.txt`
/// `801dd678` and `..._801dd6b4.txt` `801dd828`), the respecting wrapper stores
/// `zero` into `0x10(sp)` and the bypass wrapper stores `1`. `param_5 = 1` is
/// the flag `battle_formulas::DamageFinish::bypass_party_resist` models: it
/// skips the party defender's elemental-resistance stage outright, so the
/// elemental jewels / guards / All Guard do not apply to these hits. The guard
/// halve, the no-damage floor and the cap all still run.
///
/// Which spells land on the bypass wrapper is a census over every
/// capture-class module, kept with the disc patcher that retargets those
/// thirteen `jal` words (`crates/code-hooks/src/jewel_fix.rs`, whose module header
/// carries the per-module table): Guilty Cross, Bloody Horns, Terio Punch,
/// Bull Charge, Blazing Slash, Megaton Press, Plasma Strike. Spells that share
/// one of those modules but whose dispatched tick either carries no damage call
/// or calls the respecting wrapper - Curse All `0x53`, Astral Slash `0xB8`,
/// Neo Star Slash `0xA6` - are deliberately **not** here. Astral Slash rolls
/// no wrapper at all: its damage is the staged clip's melee hits, and the
/// band's fold skips it (`World::run_cast_module_code`).
///
/// This is retail behaviour and therefore the engine's baseline; making these
/// casts respect guards is the opt-in `--jewel-fix` disc patch, not a default.
///
/// REF: FUN_801DD6B4, FUN_801DD4B0, FUN_801DDB30
const CAPTURE_BYPASS_MOVE_IDS: [u8; 7] = [0x37, 0x5C, 0x5D, 0x5E, 0x79, 0x7A, 0x7B];

/// The three player Seru heals: Vera (PROT 0905), Orb (0911), Spoon (0919).
const VERA_SPELL_ID: u8 = 0x83;
const ORB_SPELL_ID: u8 = 0x89;
const SPOON_SPELL_ID: u8 = 0x91;

/// The two high-block summons that strike nothing and credit spell XP in
/// their own arm: Horn (PROT 0930) and Jedo (PROT 0931).
const HORN_SPELL_ID: u8 = 0x9C;
const JEDO_SPELL_ID: u8 = 0x9D;

#[cfg(test)]
mod capture_bypass_tests;

/// The engine has exactly **one** spell model, and the battle-action state
/// machine reads it.
///
/// This module exists because it did not: `World` carried a `spell_costs` map
/// and a `capture_spells` set that nothing ever wrote, the battle-action host
/// answered `spell_mp_cost` / `is_capture_spell` from them, and the whole live
/// cast path answered the same two questions from
/// [`crate::world::DiscTables::spell_catalog`] and the disc spell table instead. Both halves
/// compiled, both were tested, and the state-machine half was silently a
/// free-magic stub.
#[cfg(test)]
mod one_spell_model_tests;
