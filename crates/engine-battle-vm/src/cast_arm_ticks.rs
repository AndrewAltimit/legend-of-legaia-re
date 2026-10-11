//! The **fourteen trampoline-reached tick bodies** of the capture-class band
//! that [`crate::cast_module_ticks`] does not carry: PROT 0940, 0941, 0943,
//! 0944, 0950, 0956 and 0962.
//!
//! [`crate::cast_module_ticks`] ports the twelve bodies PROT 0938 / 0951 /
//! 0952 / 0955 / 0965 reach. The other seven trampolines in
//! [`crate::cast_module_ticks::CAPTURE_TRAMPOLINES`] name fourteen more, and
//! every one is a whole choreography of the same class - 1264 to 4052 bytes,
//! five to fourteen phase arms, and in nine of them a damage site with a
//! power constant baked into the `jal`'s `a0`.
//!
//! ## Why the key is `(entry, body)` and never the body VA
//!
//! `0x801F6A04` is an arm in **three** of these images and frame-matches at
//! three different sizes - 1264 B in PROT 0943, 2312 B in 0941, 2668 B in
//! 0944. Three of the constants below therefore carry the same value and are
//! told apart only by their owning PROT entry, which is why the dispatch seam
//! (`World::run_cast_module_code`) matches on the pair.
//!
//! ## Dispatch shapes
//!
//! Ten of the fourteen jump through a word table; the head table is **not**
//! always at the load base, because a module holding two bodies stacks the
//! two tables (PROT 0943's `0xB5` table fills `0x801F69D8..0x801F69EC` and its
//! `0x40` table starts at `0x801F69F0`; PROT 0950's fourteen-word table runs
//! to `0x801F6A10`, where its five-word one begins, and the body itself opens
//! at `0x801F6A24`). The other four dispatch through a `beq`/`slti` chain over
//! the phase set `{0, 1, 2, 3, 0xFF}` (PROT 0962's three add `4`), and those
//! four are exactly the bodies that **latch** the terminal phase: an arm
//! stores `0xFF` into `ctx[+0x279]` rather than incrementing it, and the
//! `0xFF` arm is the one that clears the busy register.
//!
//! | Body | Owner | Action id | Dispatch | Arms |
//! |---|---|---|---|---|
//! | `0x801F7240` | 940 | `0xAC` | `sltiu 8`, table `0x801F69D8` | 8 |
//! | `0x801F78B8` | 940 | `0x50` / `0xAE` | chain | `{0,1,2,3,0xFF}` |
//! | `0x801F730C` | 941 | `0x51` | chain | `{0,1,2,3,0xFF}` |
//! | `0x801F6A04` | 941 | `0xB9` | `sltiu 5`, table `0x801F69D8` | 5 |
//! | `0x801F6EF4` | 943 | `0x40` | `sltiu 5`, table `0x801F69F0` | 5 |
//! | `0x801F6A04` | 943 | `0xB5` | `sltiu 5`, table `0x801F69D8` | 5 |
//! | `0x801F6A04` | 944 | `0x37` | `sltiu 6`, table `0x801F69D8` | 6 |
//! | `0x801F7470` | 944 | `0x53` | `sltiu 5`, table `0x801F69F0` | 5 |
//! | `0x801F79F8` | 950 | `0x5A` | `sltiu 5`, table `0x801F6A10` | 5 |
//! | `0x801F6A24` | 950 | `0xAB` | `sltiu 0xE`, table `0x801F69D8` | 14 |
//! | `0x801F7298` | 956 | `0x71` | chain | `{0,1,2,3,0xFF}` |
//! | `0x801F7AE4` | 962 | `0xA2` | chain | `{0,1,2,3,4,0xFF}` |
//! | `0x801F74A0` | 962 | `0xA3` | chain | `{0,1,2,3,4,0xFF}` |
//! | `0x801F6D54` | 962 | `0xA4` | chain | `{0,1,2,3,4,0xFF}` |
//!
//! ## Past the bound
//!
//! Every table body opens `li sX, 1` and returns `sX`; its out-of-bound
//! `beq v0, zero` jumps to the `move v0, sX` at the epilogue, so a phase the
//! table does not name reports **Busy** in retail. Every one of them also has
//! a terminal arm that clears the register and does not advance, so the phase
//! cannot reach the bound on its own. The port models out-of-bound as
//! [`CastTickStep::Done`] instead - anti-softlock, so a host that seeds a
//! stray phase byte still leaves battle phase `0x70`. The retail value is
//! recorded here rather than implemented because nothing reaches it.
//!
//! ## What is ported, and what is not
//!
//! Ported byte-exactly: the dispatch bound and arm map, every simulation-state
//! write (HP, the MP pair, the stat halfwords, `+0x16C`, `+0x16E`, the clip
//! trio, `+0x1DD`, `+0x21C` / `+0x21D`, `ctx[+0x0D]` / `ctx[+0x1A]` /
//! `ctx[+0x279]`), the damage step (the baked power, the wrapper, the clamp
//! shape, the `+0x10` accumulate, the HP write, the reaction stage and the
//! animation-rate write) and the phase advance.
//!
//! Not ported, and disclosed per body below: the packet arms
//! (`FUN_80024E80` / `FUN_80050ED4` / `FUN_80021B04` / `FUN_801DFDF0`), the
//! camera arms (`0x800840BC` / `0x800840C0` and the `FUN_801D829C` framing
//! calls), the sound cues (`FUN_8004FCC8` / `FUN_8004FE5C`), the pose and
//! angle stores (`+0x04`, `+0x34`..`+0x4B`, `+0x46`, `+0x21B`, `+0x21F`) and
//! the **per-arm frame gating**. That last one is the module-resident
//! countdown each body keeps in its own image (`0x801F864C` in PROT 0940,
//! `0x801F83EC` in 0941, `0x801F7A04` in 0943, `0x801F8360` in 0944,
//! `0x801F86B0` in 0950, `0x801F86A0` in 0956, `0x801F89AC` in 0962). It
//! decides *when* an arm completes, not what it does, and the host runs it
//! beside these ports: `crate::cast_module_camera::capture_countdown` for
//! eleven of the bodies, the camera directors for `0xAC` and `0x37`.
//!
//! The drain is **per arm, not per module**, and the arms differ in the
//! multiplier rather than in the quantity: every counted arm subtracts a
//! multiple of the frame-delta byte `*(0x1F800393)`, and which multiple is
//! baked into the arm. Three forms appear, two of them inside one body:
//!
//! | form | example | instructions |
//! |---|---|---|
//! | `byte * *(0x1F80037D)` | PROT 0940 `0x50` arm 1 | `lbu 0x69`/`lbu 0x7f` off `0x1F800314`, `mult` at `0x801F7A94`, `subu` at `0x801F7AA4` |
//! | `byte << 1` | PROT 0940 `0x50` arm 2 | `lbu 0x393`, `sll v1,v1,1` at `0x801F7B78`, `subu` at `0x801F7B7C` |
//! | `byte` | PROT 0943 `0xB5` | `lbu 0x7f` off `0x1F800314`, `subu` at `0x801F6B94` |
//!
//! PROT 0941's `0x51` last arm takes the same product form at
//! `0x801F7CCC..0x801F7CE8` against its own word `0x801F83EC`, with
//! the two `lbu`s in the opposite order - the product is the same.
//!
//! So a capture that sees one arm draw its word down by `4` while the
//! `*(0x1F80037D) * *(0x1F800393)` product reads `16..32` is not seeing a
//! baked constant `4`: it is seeing the bare byte on a frame where the byte
//! read `4`. Reading the product as the universal step - what this list said
//! before - predicts the wrong dwell for every arm that does not use it.
//!
//! Provenance: disassembly of each owning image at slot-B base `0x801F69D8`
//! (`see ghidra/scripts/funcs/overlay_cast_<label>_<entry>_<va>.txt`, the
//! DISASSEMBLY section), head tables read out of the image bytes at
//! `table_va - 0x801F69D8`.

use crate::cast_module_ticks::{
    ANIM_RATE_NORMAL, CastActorState, CastArmStep, CastDamageShape, CastModuleCtx, CastTickStep,
    CastWrapper, FIRST_MONSTER_SEAT, SweepHit, advance_phase, aoe_seat_is_hittable, stage_clip,
};

mod damage_shapes;
pub use damage_shapes::*;
mod glare_divide;
pub use glare_divide::*;
mod steal;
pub use steal::*;
mod curse;
pub use curse::*;
mod guilty_cross;
pub use guilty_cross::*;
mod rolling_flare;
pub use rolling_flare::*;
mod water_hazard;
pub use water_hazard::*;
mod blade_breath;
pub use blade_breath::*;

// ---------------------------------------------------------------------------
// Body constants - one per (entry, body) pair, named by owner
// ---------------------------------------------------------------------------

/// The arm of [`glare_divide_blind_tick`] that raises the Mystic Shield
/// word `_DAT_8007BD84` (`0x801F7678`) beside its reaction-run blank.
pub const MYSTIC_SHIELD_ARM: u8 = 4;

/// PROT 0940's `0xAC` arm - [`glare_divide_blind_tick`].
pub const GLARE_DIVIDE_BLIND_TICK: u32 = 0x801F_7240;

/// PROT 0940's `0x50` / `0xAE` arm - [`glare_divide_split_tick`]. Two action
/// ids reach the same body; the id is what picks the weakening branch.
pub const GLARE_DIVIDE_SPLIT_TICK: u32 = 0x801F_78B8;

/// PROT 0941's `0x51` arm - [`steal_tick`].
pub const STEAL_TICK: u32 = 0x801F_730C;

/// PROT 0941's `0xB9` arm - [`steal_sweep_tick`]. Same VA as
/// [`CURSE_MP_DRAIN_TICK`] and [`GUILTY_CROSS_TICK`], in three different
/// images and at three different sizes.
pub const STEAL_SWEEP_TICK: u32 = 0x801F_6A04;

/// PROT 0943's `0x40` arm - [`curse_single_tick`].
pub const CURSE_SINGLE_TICK: u32 = 0x801F_6EF4;

/// PROT 0943's `0xB5` arm - [`curse_mp_drain_tick`].
pub const CURSE_MP_DRAIN_TICK: u32 = 0x801F_6A04;

/// PROT 0944's `0x37` arm - [`guilty_cross_tick`].
pub const GUILTY_CROSS_TICK: u32 = 0x801F_6A04;

/// PROT 0944's `0x53` arm - [`guilty_cross_curse_tick`].
pub const GUILTY_CROSS_CURSE_TICK: u32 = 0x801F_7470;

/// PROT 0950's `0x5A` arm - [`rolling_flare_tick`].
pub const ROLLING_FLARE_TICK: u32 = 0x801F_79F8;

/// PROT 0950's `0xAB` arm - [`rolling_flare_sweep_tick`].
pub const ROLLING_FLARE_SWEEP_TICK: u32 = 0x801F_6A24;

/// PROT 0956's `0x71` arm - [`water_hazard_tick`].
pub const WATER_HAZARD_TICK: u32 = 0x801F_7298;

/// PROT 0962's `0xA2` arm - [`blade_breath_a_tick`].
pub const BLADE_BREATH_A_TICK: u32 = 0x801F_7AE4;

/// PROT 0962's `0xA3` arm - [`blade_breath_b_tick`].
pub const BLADE_BREATH_B_TICK: u32 = 0x801F_74A0;

/// PROT 0962's `0xA4` arm - [`blade_breath_c_tick`].
pub const BLADE_BREATH_C_TICK: u32 = 0x801F_6D54;

/// The phase byte the four chain-dispatched bodies latch instead of
/// incrementing; the arm that matches it is the one that clears the busy
/// register.
pub const LATCHED_DONE_PHASE: u8 = 0xFF;

/// `+0x16E` bit `0x1000` - the **curse** mark PROT 0943's `0x40` body and
/// PROT 0944's `0x53` body both OR in (`ori v0, v0, 0x1000` at `0x801F7408`
/// and `0x801F7B50` respectively).
pub const FLAG_CURSED: u16 = 0x1000;

/// `+0x16E` bit `0x1` - the status PROT 0956's `0x71` body rolls 1-in-8 for
/// on each swept seat (`jal 0x80056798; andi v0, v0, 7; bnez`). The same bit
/// PROT 0938's Chaos Breath sets, i.e.
/// [`crate::cast_module_ticks::FLAG_VENOM`]; re-exported under this name so
/// the call site reads as the body spells it.
pub const FLAG_WATER_HAZARD_STATUS: u16 = crate::cast_module_ticks::FLAG_VENOM;

/// `+0x1DD` group code `8` - the leg that sweeps `0 .. ctx[+0]`. PROT 0944's
/// `0x53` body and PROT 0956's `0x71` body both branch on it; `9` (and
/// anything else above `7`) sweeps `3 .. 3 + ctx[+1]`, and a code below `7` is
/// one seat.
///
/// Whether `0 .. ctx[+0]` is "the party row" or "the whole table" is a
/// property of `ctx[+0]`, not of these bodies: both readings survive the two
/// range loops PROT 0944's `0x37` arm 0 runs, because that arm's first loop is
/// also gated on the per-seat byte `0x8007BD10[i]`. The port takes
/// [`CastModuleCtx::party_count`] as the band already defines it.
pub const TARGET_CODE_LOW_ROW: u8 = 8;

/// The clone's action category, queued action and target code - the three
/// bytes PROT 0940's split writes onto the seat it allocates
/// (`0x801F8070` / `0x801F807C` / `0x801F8088`).
pub const SPLIT_CLONE_CATEGORY: u8 = 2;

/// See [`SPLIT_CLONE_CATEGORY`]. The clone is queued with Divide's own
/// **first** id, `0x50`, whichever id created it.
pub const SPLIT_CLONE_ACTION: u8 = 0x50;

/// The AGL both halves of a weakened split are pinned to (`li v0, 0x20`).
pub const SPLIT_WEAK_AGL: u16 = 0x20;

#[cfg(test)]
mod tests;
