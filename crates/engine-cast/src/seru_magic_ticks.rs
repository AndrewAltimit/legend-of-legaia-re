//! The **player Seru-magic** tick bodies of the slot-B cast band - spell ids
//! `0x81..=0x8B`, PROT 0903..0913, one child module per Seru.
//!
//! The band is documented in two halves, because the two halves share
//! different idioms: ids `0x81..=0x86` (`hit_sites` plus `gimard` /
//! `theeder` / `vera` / `gizam` / `nighto` / `zenoir`) and ids `0x87..=0x8B`
//! (`row_sweep` plus `viguro` / `swordie` / `orb` / `spoon` / `freed` /
//! `nova`).
//!
//! # Ids `0x81..=0x86`
//!
//! The first six **player Seru-magic** tick bodies of the slot-B cast band -
//! spell ids `0x81..=0x86`, PROT 0903..0908.
//!
//! [`crate::cast_module_ticks`] carries the band's capture-class and
//! summon-creature ticks. This module carries the other half the
//! `0x801CF4EC` table reaches: the eleven arms a *player* Seru cast runs,
//! of which these are the first six. Each arm in that table is a 16-byte
//! trampoline in PROT 0898 that does nothing but `jal` the module's own
//! entry and fold the return into `s0`:
//!
//! | id | PROT | `0x801CF4EC` arm | stub | tick entry |
//! |---|---|---|---|---|
//! | `0x81` | 903 Gimard | word 0 | `0x801F1F3C` | `0x801F69D8` |
//! | `0x82` | 904 Theeder | word 1 | `0x801F1F4C` | `0x801F69D8` |
//! | `0x83` | 905 Vera | word 2 | `0x801F1F5C` | `0x801F69D8` |
//! | `0x84` | 906 Gizam | word 3 | `0x801F1F6C` | `0x801F69F4` |
//! | `0x85` | 907 Nighto | word 4 | `0x801F1F7C` | `0x801F69E8` |
//! | `0x86` | 908 Zenoir | word 5 | `0x801F1F8C` | `0x801F69D8` |
//!
//! Read out of PROT 0898's own bytes at base `0x801CE818` (file `+0xCD4` is
//! the table, `+0x23724` the first stub), so the arm -> tick pairing is a
//! property of the disc rather than of a dump filename. Four of the six
//! ticks sit at the load base `0x801F69D8`, which is why every item here
//! names its PROT entry as well as its VA.
//!
//! ## Drive shape
//!
//! All six are `beq`/`slti` **chains**, not word tables, over a contiguous
//! phase run plus an explicit `0xFF` terminal arm - the shape
//! [`crate::cast_module_ticks::kemaro_tick`] takes, not the `sltiu`-bounded
//! one the capture ticks take. Each opens with
//!
//! ```text
//! ctx    = *0x8007BD24
//! caster = actor_table[ctx + 0x13]          ; 0x801C9370
//! victim = actor_table[caster + 0x1DD]
//! summon = actor_table[7]                   ; lw rX, 0x1C(0x801C9370)
//! ```
//!
//! and returns `1` while busy, `0` from the `0xFF` arm only. A phase the
//! chain does not name returns `1` **without advancing** - retail parks
//! there. This port returns [`CastTickStep::Done`] for such a phase instead,
//! as anti-softlock; the one place that matters in practice is PROT 0906,
//! whose chain has a real hole (see [`gizam_tick`]).
//!
//! ## What is ported
//!
//! Per body: the dispatch bound and arm map, every write to a field the
//! actor mirror carries (HP `+0x14C`, flags `+0x16E`, clips `+0x1DA` /
//! `+0x1DC`, target `+0x1DD`, render flag `+0x21C`, animation rate `+0x21D`,
//! root speed `+0x0C`, HP-bar delta `+0x10`), the `ctx+0x278` discipline and
//! the phase walk, and each damage or restore site exactly - its baked power,
//! its wrapper, its scale, its clamp and its reaction stage.
//!
//! Not ported, and disclosed per body: the GPU-packet arms, the camera arms,
//! the per-arm frame gating (every arm here is fenced by a countdown on
//! `ctx+0x6D8` or a module-local timer against the scratchpad frame-delta
//! bytes `0x1F800369` / `0x1F80037D`), the fields outside the mirror
//! (`+0x04` render word, `+0x21B`, `+0x21F`, `+0x225`, `+0x176`, `+0x170`,
//! `ctx+0x243` / `+0x6D0` / `+0x6D8` / `+0x6DA`), and the geometry the two
//! cone sweeps gate on.
//!
//! ## Three clamp shapes, not two
//!
//! `crate::cast_module_ticks` documents two apply shapes for the band.
//! PROT 0908's first damage site is a **third**: `addiu v1, hp, -1` then
//! `sltu v1, dmg` - an *unsigned* compare against `HP - 1`
//! ([`apply_hit_cap_hp_minus_one`]). It cannot kill, like the signed `HP - 1`
//! shape, but a negative wrapper return still reads as a huge unsigned value
//! and clamps to `HP - 1` rather than healing. Its two sibling sites in the
//! same routine take the ordinary kill-capable `sltu` against live HP.
//!
//! Provenance: disassembly of each owning image at the slot-B base
//! `0x801F69D8` (`scripts/ghidra-analysis/disasm-overlay-fn.py
//! extracted/overlays/overlay_<label>_<entry>.bin --base 0x801F69D8 --addr
//! <va>`), whose frame-matched extents are 3396 / 6020 / 5792 / 3404 / 5568 /
//! 6456 bytes for 0903..0908.
//!
//! # Ids `0x87..=0x8B`
//!
//! The player Seru-magic band's **tick bodies** for action ids `0x87..=0x8B`
//! (PROT 0909..0913) - the second half of the eleven `0x801CF4EC` arms
//! `docs/subsystems/cast-module.md` grades "code, not data".
//!
//! Each of these five modules carries **two** routines: the `0x801F6734` row
//! the move VM's opcode `0x20` calls (the spawn stager, mostly the pool's
//! data half) and the `0x801CF4EC` arm the battle phase re-enters every frame
//! (the tick body, the choreography). Only PROT 0909's stager touches state;
//! the other four are pure spawn dispatchers. So for this band the tick body
//! *is* the module, and it is what this file ports.
//!
//! | Id | PROT | tick body | shape |
//! |---|---|---|---|
//! | `0x87` | 0909 `summon_viguro` | [`VIGURO_TICK`] | enemy-row damage sweep, two rendezvous phases |
//! | `0x88` | 0910 `summon_swordie` | [`SWORDIE_TICK`] | four-slash damage in a **callee**, [`SWORDIE_SLASH`] |
//! | `0x89` | 0911 `summon_orb` | [`ORB_TICK`] | whole-row **heal** plus a status cleanse |
//! | `0x8A` | 0912 `summon_freed` | [`FREED_TICK`] | enemy-row damage sweep, 20 arms |
//! | `0x8B` | 0913 `summon_nova` | [`NOVA_TICK`] | single-victim damage, 21 arms |
//!
//! Types and idioms come from [`crate::cast_module_ticks`]; this module adds
//! only what the five bodies need that the band did not already have.
//!
//! ## What is ported
//!
//! The dispatch chain's arm map (every one of these five heads is a
//! `beq`/`slti` chain on `ctx[+0x279]`, not a word table), every simulation
//! write that has a mirror on [`CastActorState`] / [`CastModuleCtx`], the
//! damage and heal steps with their baked constants and clamp shapes, the
//! phase walk including its two rendezvous holds, the `ctx+0x278` discipline
//! and the terminal arm's return.
//!
//! Not ported, and disclosed per body below: the packet arms (`FUN_80021B04`
//! / `FUN_80050ED4` spawn sites - those records are
//! `legaia_overlay_images::cast_effect_pool`'s), the camera arms (`FUN_801D829C` /
//! `FUN_801DCEAC`), the seat **pose** fields (`+0x34` / `+0x38` / `+0x46` /
//! `+0x36` / `+0x1BA` / `+0x176` / `+0x21B` / `+0x225` / `+0x21F`), the
//! `+0x04` blend word, the per-arm frame gating (every body times its arms on
//! the scratchpad frame-delta pair `0x1F80037D` / `0x1F800393` against a
//! module-local countdown word, which is capture-pinned timing and not static
//! shape - except PROT 0910's arms `6..=0x0A`, whose timers are what schedule
//! its four slashes and are ported in [`swordie_tick`] / [`SwordieSlashes`],
//! and the post-hit countdown the other four re-arm in their hit arm and
//! drain in their settle arm, which decides when the Done band opens and is
//! ported through each body's `settle` word), and the text draws
//! (`FUN_8003541C`).
//!
//! ## Three clamp shapes, not two
//!
//! [`crate::cast_module_ticks`] records two apply shapes for the band - the
//! kill-capable unsigned clamp to HP ([`apply_hit_floor_zero`]) and the
//! signed clamp to `HP - 1`
//! ([`crate::cast_module_ticks::apply_hit_floor_one`]). PROT 0910's per-slash
//! applier is a **third**: an *unsigned* clamp to `HP - 1`
//! ([`apply_hit_unsigned_floor_one`]), and PROT 0910 picks between it and the
//! kill-capable one **per hit**, on the slash counter - so kill-capability is
//! a property of the hit index, not of the module and not even of the
//! routine.
//!
//! ## The phase is driven from two call sites, so the arm map has holes
//!
//! PROT 0909's chain names phases `0,1,2,4,5,6,7,9,0xA..0xE,0xFF` and **not**
//! `3` or `8`: those two fall through to the epilogue with the busy register
//! still `1`, so the tick parks. What releases them is the module's own
//! stager, whose arms `0` and `1` each carry `lbu v0,0x279(v1); addiu v0,v0,1;
//! sb v0,0x279(v1)` (`0x801F7BB8` and `0x801F7C48`). The choreography is
//! therefore a rendezvous between the per-frame tick and the move-VM effect
//! script, and a port that advanced through `3` and `8` would run the cast
//! ahead of its own script. [`viguro_tick`] holds there
//! ([`CastTickStep::Busy`], no advance).
//!
//! PROT 0911's chain has the same *shape* of hole at `6`, `7` and `8` and a
//! different cause: its arm `5` writes `sb s7,0x279(s5)` with `s7 = 9`
//! (`0x801F7780`, the constant materialised eleven hundred instructions
//! earlier at `0x801F6A74` and held in a saved register), so the body
//! **jumps** the phase to `9` and those three arms are unreachable.
//!
//! Provenance: disassembly of each owning image at slot-B base `0x801F69D8`
//! (`scripts/ghidra-analysis/disasm-overlay-fn.py
//! extracted/overlays/overlay_summon_<name>_<entry>.bin --base 0x801F69D8
//! --addr <va>`), plus `crates/asset/data/static-overlays.toml` for the
//! per-entry anchor VA that names each tick.

use crate::cast_module_ticks::{
    ANIM_RATE_NORMAL, CHOREOGRAPHY_DONE_PHASE, CastActorState, CastArmStep, CastDamageShape,
    CastModuleCtx, CastTickStep, CastWrapper, FIRST_MONSTER_SEAT, FLAG_NON_TARGETABLE, SUMMON_SEAT,
    SweepHit, TARGET_CODE_ENEMY_ROW, advance_phase, apply_hit_floor_zero, roll_module_hit,
};
use legaia_engine_battle_vm::battle_damage_wrappers::{WrapperAttacker, WrapperDefender};
use legaia_engine_battle_vm::battle_formulas::vera_heal_amount;

mod hit_sites;
pub use hit_sites::*;
mod gimard;
pub use gimard::*;
mod theeder;
pub use theeder::*;
mod vera;
pub use vera::*;
mod gizam;
pub use gizam::*;
mod nighto;
pub use nighto::*;
mod zenoir;
pub use zenoir::*;

mod row_sweep;
pub use row_sweep::*;
mod viguro;
pub use viguro::*;
mod swordie;
pub use swordie::*;
mod orb;
pub use orb::*;
mod spoon;
pub use spoon::*;
mod freed;
pub use freed::*;
mod nova;
pub use nova::*;

#[cfg(test)]
mod tests;
