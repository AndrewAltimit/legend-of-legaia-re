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
//! `0x801F86B0` in 0950, `0x801F86A0` in 0956, `0x801F89AC` in 0962),
//! decremented by the scratchpad frame step `*(0x1F80037D) * *(0x1F800393)`;
//! it decides *when* an arm completes, not what it does, and it is pinned by
//! capture, not by the static window.
//!
//! Provenance: disassembly of each owning image at slot-B base `0x801F69D8`
//! (`see ghidra/scripts/funcs/overlay_cast_<label>_<entry>_<va>.txt`, the
//! DISASSEMBLY section), head tables read out of the image bytes at
//! `table_va - 0x801F69D8`.

use crate::cast_module_ticks::{
    ANIM_RATE_NORMAL, CastActorState, CastArmStep, CastDamageShape, CastModuleCtx, CastTickStep,
    CastWrapper, FIRST_MONSTER_SEAT, SweepHit, advance_phase, aoe_seat_is_hittable, stage_clip,
};

// ---------------------------------------------------------------------------
// Body constants - one per (entry, body) pair, named by owner
// ---------------------------------------------------------------------------

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
/// [`CastModuleCtx::actor_count`] as the band already defines it.
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

// ---------------------------------------------------------------------------
// The record fields these bodies touch that `CastActorState` has no slot for
// ---------------------------------------------------------------------------

/// The five extra record halfwords / bytes this band's arms reach that
/// [`CastActorState`] does not carry.
///
/// Kept beside the shared view rather than inside it so this module is purely
/// additive: `cast_module_ticks` is edited by no lane to make these bodies
/// work.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CastArmExtState {
    /// `+0x150` - MP **working**. PROT 0943's `0xB5` body zeroes it on every
    /// seat (`sh zero, 0x150(v0)` at `0x801F6D08`).
    pub mp: u16,
    /// `+0x152` - MP **base**, zeroed in the same breath (`0x801F6D1C`).
    pub mp_base: u16,
    /// `+0x178` - where that body stashes the pre-drain `+0x150`
    /// (`lhu v0, 0x150(v1); sh v0, 0x178(v1)` at `0x801F6CF4`/`0x801F6CFC`).
    /// No routine in PROT 0903..0966 reads it back, so it is carried as an
    /// outcome rather than mirrored onto the engine actor.
    pub mp_stash: u16,
    /// `+0x172` - max HP. PROT 0940's split writes the caster's live HP into
    /// both `+0x172` and `+0x14C` of the seat it allocates (`0x801F8098` /
    /// `0x801F809C`).
    pub max_hp: u16,
    /// `+0x1F3` - the fifth byte of the reaction-clip run PROT 0940's `0xAC`
    /// body blanks (`sb zero, 0x1f3(v0)` at `0x801F76C0`).
    pub reaction_extra: u8,
}

// ---------------------------------------------------------------------------
// Damage shapes, keyed by (entry, body)
// ---------------------------------------------------------------------------

/// The nine baked damage shapes of the fourteen arms, keyed by the
/// `(PROT entry, body VA)` pair.
///
/// [`crate::cast_module_ticks::CAST_DAMAGE_SHAPES`] cannot hold them: PROT
/// 0941 pairs a body with **no** wrapper against one baked `0xC0`, and PROT
/// 0950 pairs `0x100` against `0x29A`, so an entry-keyed lookup answers for
/// whichever body it meets first.
///
/// `docs/subsystems/cast-module.md` grades `0x801F730C` (PROT 0941's `0x51`
/// Steal) as carrying **one** damage wrapper. It carries none: the body's ten
/// distinct `jal` targets include no `FUN_801DD0AC` / `FUN_801DD4B0` /
/// `FUN_801DD6B4`, and its outcome is an inventory removal
/// (`jal 0x80042310`), not a hit.
///
/// REF: FUN_801F6A04 (`0x801F6FA0` bakes `0xC0`, `0x801F6FAC` the `jal`)
/// REF: FUN_801F6A24 (`0x801F7700` / `0x801F770C`)
/// REF: FUN_801F7298 (`0x801F78C4` / `0x801F78D0`, `0x801F7A38` / `0x801F7A40`)
pub const ARM_DAMAGE_SHAPES: [(u32, u32, CastDamageShape); 8] = [
    (
        941,
        STEAL_SWEEP_TICK,
        CastDamageShape {
            prot_entry: 941,
            routine: STEAL_SWEEP_TICK,
            wrapper: CastWrapper::Respect,
            never_kills: false,
            powers: &[0x00C0],
        },
    ),
    (
        944,
        GUILTY_CROSS_TICK,
        CastDamageShape {
            prot_entry: 944,
            routine: GUILTY_CROSS_TICK,
            wrapper: CastWrapper::Bypass,
            never_kills: false,
            powers: &[0x038E],
        },
    ),
    (
        950,
        ROLLING_FLARE_TICK,
        CastDamageShape {
            prot_entry: 950,
            routine: ROLLING_FLARE_TICK,
            wrapper: CastWrapper::Respect,
            never_kills: false,
            powers: &[0x0100],
        },
    ),
    (
        950,
        ROLLING_FLARE_SWEEP_TICK,
        CastDamageShape {
            prot_entry: 950,
            routine: ROLLING_FLARE_SWEEP_TICK,
            wrapper: CastWrapper::Respect,
            never_kills: false,
            powers: &[0x029A],
        },
    ),
    (
        956,
        WATER_HAZARD_TICK,
        CastDamageShape {
            prot_entry: 956,
            routine: WATER_HAZARD_TICK,
            wrapper: CastWrapper::Respect,
            never_kills: false,
            powers: &[0x00D0, 0x00D0],
        },
    ),
    (
        962,
        BLADE_BREATH_A_TICK,
        CastDamageShape {
            prot_entry: 962,
            routine: BLADE_BREATH_A_TICK,
            wrapper: CastWrapper::Respect,
            never_kills: false,
            powers: &[0x0200],
        },
    ),
    (
        962,
        BLADE_BREATH_B_TICK,
        CastDamageShape {
            prot_entry: 962,
            routine: BLADE_BREATH_B_TICK,
            wrapper: CastWrapper::Respect,
            never_kills: false,
            powers: &[0x0200],
        },
    ),
    (
        962,
        BLADE_BREATH_C_TICK,
        CastDamageShape {
            prot_entry: 962,
            routine: BLADE_BREATH_C_TICK,
            wrapper: CastWrapper::Respect,
            never_kills: false,
            powers: &[0x0200],
        },
    ),
];

/// The damage shape of one `(entry, body)` arm, if it has one.
pub fn arm_damage_shape_for(prot_entry: u32, body: u32) -> Option<&'static CastDamageShape> {
    ARM_DAMAGE_SHAPES
        .iter()
        .find(|(e, b, _)| *e == prot_entry && *b == body)
        .map(|(_, _, s)| s)
}

/// Does this `(entry, body)` arm write `+0x14C` itself over a seat range the
/// spell record cannot express - i.e. must the band's generic fold stand
/// aside for it?
///
/// True for the three **whole-row sweeps** here, the same property
/// [`crate::cast_module_ticks::tick_body_owns_the_fold`] records for PROT
/// 0938's two bodies and PROT 0965's. The single-target arms are not in this
/// set: they write `+0x14C` too, but over the one seat the record already
/// names, so the engine folds them once through its own kernel seeded with
/// the module's baked power.
pub fn arm_owns_the_fold(prot_entry: u32, body: u32) -> bool {
    arm_sweep_arm(prot_entry, body).is_some()
}

/// The module phase a whole-row sweep arm applies its damage on.
///
/// * PROT 0941's `0xB9` - table word 2 (`0x801F6E38`), the loop at
///   `0x801F6F60`.
/// * PROT 0950's `0xAB` - table word 10 (`0x801F74C4`), the loop reaching the
///   `jal` at `0x801F770C`.
/// * PROT 0956's `0x71` - the chain's `beq v1, 2` arm (`0x801F7634`), which
///   holds both wrapper sites.
pub fn arm_sweep_arm(prot_entry: u32, body: u32) -> Option<u8> {
    match (prot_entry, body) {
        (941, STEAL_SWEEP_TICK) => Some(2),
        (950, ROLLING_FLARE_SWEEP_TICK) => Some(10),
        (956, WATER_HAZARD_TICK) => Some(2),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Shared idioms these fourteen add to the band's vocabulary
// ---------------------------------------------------------------------------

/// The **unpaired-OR** restage: `lbu v0, 0x1dc(s0); ori v0, v0, 1;
/// sb v0, 0x1dc(s0)`.
///
/// PROT 0941's Steal spells its restage this way at `0x801F7598` and
/// `0x801F7BD4`, where the rest of the band either increments `+0x1DC` or
/// assigns it. [`crate::cast_module_ticks::stage_clip`] is the increment; this
/// is the third form, and it is idempotent where the increment is not.
pub fn stage_clip_or(actor: &mut CastActorState, clip: u8) {
    actor.staged_anim = clip;
    actor.restage |= 1;
}

/// The band's single-seat apply, as the nine damaging arms here spell it:
/// clamp unsigned against the victim's live HP, accumulate into `+0x10`,
/// subtract from `+0x14C`, stage the knockdown clip and set the reaction
/// animation rate.
///
/// `restage` is the value the arm stores into `+0x1DC` - an **assignment**,
/// not a bump, in seven of the nine (`sb v0, 0x1dc(...)` with `li v0, 1`).
/// Returns the damage actually applied.
pub fn apply_arm_hit(victim: &mut CastActorState, roll: i32, restage: u8, anim_rate: u8) -> u32 {
    let hp = u32::from(victim.hp);
    let mut dmg = roll as u32;
    if hp < dmg {
        dmg = hp;
    }
    victim.anim_rate = anim_rate;
    victim.hp_bar_delta = victim.hp_bar_delta.wrapping_add(dmg as i32);
    victim.hp = hp.wrapping_sub(dmg) as u16;
    victim.staged_anim = victim.knockdown_anim;
    victim.restage = restage;
    dmg
}

/// Which seats an arm's `+0x1DD` group code selects, as PROT 0944's `0x53`
/// body and PROT 0956's `0x71` body both spell it:
/// `< 7` is one seat, `== 8` the low row `0 .. ctx[+0]`, anything else the
/// monster row `3 .. 3 + ctx[+1]`.
///
/// PROT 0956 folds a code below `3` into the low-row leg as well
/// (`sltiu v0, code, 3` then the `== 8` test), so a single-seat code there
/// still sweeps the row; PROT 0944 does not. The `low_row_below_three` flag
/// is that difference.
pub fn arm_target_seats(
    ctx: &CastModuleCtx,
    target_code: u8,
    low_row_below_three: bool,
) -> Vec<u8> {
    if target_code == TARGET_CODE_LOW_ROW
        || (low_row_below_three && target_code < FIRST_MONSTER_SEAT)
    {
        return (0..ctx.actor_count).collect();
    }
    if target_code < 7 {
        return vec![target_code];
    }
    let first = FIRST_MONSTER_SEAT;
    (first..first.saturating_add(ctx.monster_count)).collect()
}

/// The shared tail of the ten table-dispatched bodies: an arm that ran to
/// completion falls into `lbu v0, 0(sX); addiu v0, v0, 1; sb v0, 0(sX)`.
fn run_arm_tick(
    ctx: &mut CastModuleCtx,
    arms: u8,
    arm: impl FnOnce(&mut CastModuleCtx) -> CastArmStep,
) -> CastTickStep {
    if ctx.phase >= arms {
        // Retail reports Busy here (see the module header); the port reports
        // Done so a stray phase cannot hold battle phase `0x70` forever.
        return CastTickStep::Done;
    }
    match arm(ctx) {
        CastArmStep::Advance => {
            advance_phase(ctx);
            CastTickStep::Busy
        }
        CastArmStep::Hold => CastTickStep::Busy,
        CastArmStep::Finish => CastTickStep::Done,
    }
}

/// The shared tail of the four chain-dispatched bodies. There is no bound to
/// test: a phase the chain does not name falls straight to `move v0, sX` and
/// reports **Busy** in retail. Each of these bodies drives its own phase from
/// `0` and latches `0xFF`, so no reachable phase is unnamed - only a stray
/// seed is, and each body reports [`CastArmStep::Finish`] there for the same
/// anti-softlock reason [`run_arm_tick`] does past a table's bound.
fn run_chain_tick(
    ctx: &mut CastModuleCtx,
    arm: impl FnOnce(&mut CastModuleCtx) -> CastArmStep,
) -> CastTickStep {
    match arm(ctx) {
        CastArmStep::Advance => {
            advance_phase(ctx);
            CastTickStep::Busy
        }
        CastArmStep::Hold => CastTickStep::Busy,
        CastArmStep::Finish => CastTickStep::Done,
    }
}

// ---------------------------------------------------------------------------
// PROT 0940 - cast_glare_divide
// ---------------------------------------------------------------------------

/// PROT 0940 (Glare Divide) tick body for action `0xAC`.
///
/// Eight phase arms behind `sltiu v0, v1, 8` (`0x801F72B4`) through the head
/// table at `0x801F69D8` (arm targets `0x801F72E0`, `735C`, `73C4`, `74CC`,
/// `7584`, `76E8`, `77BC`, `7840`). Its whole simulation footprint is two
/// arms:
///
/// * arm `4` blanks the **first monster seat**'s five-byte reaction-clip run
///   `+0x1EF..+0x1F3` (`0x801F7690`..`0x801F76C0`). The seat is
///   `actor_table[3]`, reached as `lw v0, 0xc(s0)` - and `s0` is **not** the
///   caster there: `lui s0, 0x801d; addiu s0, s0, -0x6c90` at
///   `0x801F7648`/`0x801F764C` reassigns it to the actor table base
///   `0x801C9370` four instructions earlier, so the displacement `0xC` is the
///   table index, not a record field. A backward scan that stopped at the
///   prologue's `lw s0, 0(v0)` would read this as `caster[+0x0C]`;
/// * arm `7` is terminal - it clears `ctx[+0x0D]`, writes the framing
///   halfword `ctx[+0x6DA] = 0x780`, and clears the busy register, so the
///   body reports Done and the phase parks at `7`.
///
/// Not ported: arms `0`..`3`, `5` and `6`, which are the sound cue
/// `FUN_8004FCC8(0x1AA)`, the `FUN_801D829C` camera framings, the
/// `FUN_80050ED4` / `FUN_80021B04` / `FUN_801DFDF0` pool spawns and the
/// `0x800840BC`/`0x800840C0` camera walk, plus the module-resident countdown
/// at `0x801F864C` that gates every arm.
///
/// Wired: `World::run_cast_module_code`, the `(940, GLARE_DIVIDE_BLIND_TICK)`
/// arm of the trampoline dispatch.
///
/// PORT: FUN_801F7240 (PROT 0940 action 0xAC; phase machine + reaction-clip
/// blank; packet / camera arms unported)
pub fn glare_divide_blind_tick(
    ctx: &mut CastModuleCtx,
    first_monster: &mut CastActorState,
    first_monster_ext: &mut CastArmExtState,
) -> CastTickStep {
    run_arm_tick(ctx, 8, |c| match c.phase {
        4 => {
            first_monster.reaction_alt = 0;
            first_monster.reaction_alt2 = 0;
            first_monster.knockdown_anim = 0;
            first_monster.reaction_gate = 0;
            first_monster_ext.reaction_extra = 0;
            CastArmStep::Advance
        }
        7 => {
            c.ctx_0d = 0;
            CastArmStep::Finish
        }
        _ => CastArmStep::Advance,
    })
}

/// Which half of a Divide the weakening branch hit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitWeakened {
    /// `rand() & 1 == 0` - the new seat takes the 1 HP.
    Clone,
    /// `rand() & 1 != 0` - the original caster does.
    Caster,
}

/// What PROT 0940's split arm asked the host to materialise.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GlareDivideSplit {
    /// The seat the arm allocated: `3 + ctx[+1]` read **before** the
    /// increment (`lbu v0, 1(v1); andi s3, v0, 0xff; addiu v0, v0, 1;
    /// sb v0, 1(v1)` at `0x801F7E08`..`0x801F7E1C`, then
    /// `piVar15 = &actor_table[s3 + 3]`).
    pub clone_seat: u8,
    /// `+0x14C` and `+0x172` of the new seat, both the caster's live HP.
    pub clone_hp: u16,
    /// The `+0x1DD` value the arm displaced off the caster, which the `0xFF`
    /// arm puts back.
    pub saved_caster_target: u8,
    /// `None` for action `0x50`; `Some(..)` for `0xAE`, whose extra branch
    /// (`li v0, 0xae; bne v1, v0` at `0x801F80F0`) renames both halves and
    /// pins one of them to 1 HP.
    pub weakened: Option<SplitWeakened>,
}

/// PROT 0940 (Glare Divide) tick body for actions `0x50` and `0xAE` - the
/// **split**, the one arm in the band that allocates a battle seat.
///
/// A `beq`/`slti` chain over `{0, 1, 2, 3, 0xFF}` (`0x801F7920`..`0x801F7968`);
/// any other phase falls to `move v0, s5` and reports Busy.
///
/// * arm `0` - sound cue `0x141` when the caster's queued id is `0x50`, else
///   `0x1A9`, and the framing call. Advances;
/// * arm `1` - `caster[+0x21C] = 10` (`0x801F7B5C`). Advances;
/// * arm `2` - the split, below. Advances;
/// * arm `3` - latches `ctx[+0x279] = 0xFF` (`0x801F81EC`) and stays Busy;
/// * arm `0xFF` - restores `caster[+0x1DD]` from the module word `0x801F8658`
///   and clears the busy register.
///
/// The split itself, in store order:
///
/// ```text
/// seat            = ctx[+1] ; ctx[+1] += 1            ; clone = actor_table[seat + 3]
/// 0x801C9348[seat]= 0x801C9348[ctx[+0x13] - 3]        ; the clone shares the monster record
/// ctx[+0x1A]     += 1                                 ; the turn cursor
/// clone[+0x16C]   = 0                                 ; it has already acted
/// clone[+0x1DE]   = 2 ; clone[+0x1DF] = 0x50 ; clone[+0x1DD] = 9
/// clone[+0x172]   = clone[+0x14C] = caster[+0x14C]
/// saved           = caster[+0x1DD] ; caster[+0x21C] = 0 ; caster[+0x1DD] = 9
/// if caster[+0x1DF] == 0xAE {
///     rand() & 1 == 0 ? clone : caster
///         [+0x14C] = 1 ; [+0x150] = 0 ; [+0x158] = 1 ; [+0x156] = [+0x154] = 0x20
/// }
/// ```
///
/// So `0x50` splits at full HP and `0xAE` leaves one of the two halves on 1 HP
/// with ATK 1, no MP and AGL `0x20` - a coin flip that can land on the caster.
///
/// Not ported: the actor-object allocation (`FUN_80054CB0` / `FUN_80024C88`),
/// the unaligned `swl`/`swr` pose copies into the clone's `+0x34..+0x4B`, the
/// `FUN_8003CA38` / `FUN_8003CA78` name edit that gives the halves their
/// suffix, the `0x80076768`-table copy and the camera arms.
///
/// `rand` is the `FUN_80056798` draw; `None` suppresses the weakening branch,
/// which is what an id of `0x50` does anyway.
///
/// Wired: `World::run_cast_module_code`.
///
/// PORT: FUN_801F78B8 (PROT 0940 actions 0x50 / 0xAE; split + phase latch;
/// object allocation and name edit unported)
#[allow(clippy::too_many_arguments)]
pub fn glare_divide_split_tick(
    ctx: &mut CastModuleCtx,
    caster: &mut CastActorState,
    caster_ext: &CastArmExtState,
    action_id: u8,
    saved_target: u8,
    rand: Option<u32>,
) -> (CastTickStep, Option<GlareDivideSplit>) {
    let _ = caster_ext;
    let mut split = None;
    let step = run_chain_tick(ctx, |c| match c.phase {
        0 => CastArmStep::Advance,
        1 => {
            caster.render_flag = 10;
            CastArmStep::Advance
        }
        2 => {
            let seat = FIRST_MONSTER_SEAT.wrapping_add(c.monster_count);
            c.monster_count = c.monster_count.wrapping_add(1);
            c.turn_cursor = c.turn_cursor.wrapping_add(1);
            let saved = caster.target_code;
            caster.render_flag = 0;
            caster.target_code = crate::cast_module_ticks::TARGET_CODE_ENEMY_ROW;
            let weakened = (action_id == 0xAE).then(|| match rand.unwrap_or(0) & 1 {
                0 => SplitWeakened::Clone,
                _ => SplitWeakened::Caster,
            });
            if weakened == Some(SplitWeakened::Caster) {
                caster.hp = 1;
                caster.atk = 1;
                caster.agl_base = SPLIT_WEAK_AGL;
                caster.agl = SPLIT_WEAK_AGL;
            }
            split = Some(GlareDivideSplit {
                clone_seat: seat,
                clone_hp: caster.hp,
                saved_caster_target: saved,
                weakened,
            });
            CastArmStep::Advance
        }
        3 => {
            c.phase = LATCHED_DONE_PHASE;
            CastArmStep::Hold
        }
        LATCHED_DONE_PHASE => {
            caster.target_code = saved_target;
            CastArmStep::Finish
        }
        // A phase the chain does not name: Done, not retail's Busy.
        _ => CastArmStep::Finish,
    });
    (step, split)
}

// ---------------------------------------------------------------------------
// PROT 0941 - cast_steal
// ---------------------------------------------------------------------------

/// One bag slot as PROT 0941's Steal reads it: the two bytes at
/// `0x80085958 + slot * 2`, `[item id, count]`.
pub type StealBagSlot = (u8, u8);

/// What PROT 0941's `0x51` body decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StealOutcome {
    /// The victim is a party seat: one of this item came out of the bag
    /// (`jal 0x80042310` with `(id, 1)`, the inventory consume-by-id helper).
    FromBag { item: u8 },
    /// The victim is a party seat but `0x400` draws found no bag slot whose
    /// id, count and item-table record are all non-zero, so nothing is taken.
    BagEmpty,
    /// The victim is a monster seat: the static steal table
    /// `0x80077828 + monster_id * 2` decided, fields `[chance, item]`.
    FromMonster {
        /// The table's `+0` chance byte, compared against `rand() % 100`.
        chance: u8,
        /// The table's `+1` item id, handed over only on a hit.
        item: u8,
        /// `rand() % 100`.
        roll: u8,
        /// `roll < chance`.
        hit: bool,
    },
}

/// The bag draw PROT 0941's Steal makes, spelled out because it is a
/// **rejection sampler** and not a scan.
///
/// ```text
/// tries = 0
/// do {
///     slot = rand() % 0x100
///     while ctx[+0x11] == 4 && slot < *(0x8007B5EA) { slot = rand() % 0x100 }
/// } while (bag[slot].id == 0 || bag[slot].count == 0 || !item_valid(bag[slot].id))
///          && ++tries < 0x400
/// ```
///
/// The index is `(slot << 16) >> 15`, i.e. `slot * 2` - a byte offset into the
/// 256-slot, two-bytes-per-slot bag at `0x80085958`
/// (`docs/subsystems/inventory.md`). The inner `0x8007B5EA` floor only arms
/// when `ctx[+0x11] == 4`, which is how the module keeps a scripted fight from
/// stealing out of the low half of the bag.
///
/// Returns the chosen slot, or `None` when the `0x400` budget ran out.
///
/// [`steal_bag_slot_from_draw`] is the one-draw half, for a caller whose RNG
/// cannot be handed out as a closure: the draw count is observable (retail
/// advances the shared `FUN_80056798` cursor once per rejected slot), so a
/// host must not pre-draw a batch it might not consume.
pub fn steal_pick_bag_slot(
    bag: &[StealBagSlot],
    min_slot: Option<u8>,
    mut rand: impl FnMut() -> u32,
    item_valid: impl Fn(u8) -> bool,
) -> Option<u8> {
    for _ in 0..STEAL_DRAW_BUDGET {
        let mut slot = (rand() % 0x100) as u8;
        if let Some(floor) = min_slot {
            let mut guard = 0u32;
            while slot < floor && guard < STEAL_DRAW_BUDGET {
                slot = (rand() % 0x100) as u8;
                guard += 1;
            }
        }
        if let Some(hit) = steal_bag_slot_from_draw(bag, slot, &item_valid) {
            return Some(hit);
        }
    }
    None
}

/// The `0x400` rejection budget the loop counts down
/// (`addiu v0, v0, 1; sltiu v0, v0, 0x400`).
pub const STEAL_DRAW_BUDGET: u32 = 0x400;

/// One draw of [`steal_pick_bag_slot`]'s loop: is the bag slot this `rand()`
/// landed on acceptable?
///
/// The test is `bag[slot].id != 0 && bag[slot].count != 0 && item_valid(id)`,
/// where retail's third leg is a non-zero halfword in the static item table at
/// `id * 0xC`. Split out so a host whose RNG cannot be borrowed into a closure
/// still spends exactly the draws retail spends.
pub fn steal_bag_slot_from_draw(
    bag: &[StealBagSlot],
    slot: u8,
    item_valid: impl Fn(u8) -> bool,
) -> Option<u8> {
    let &(id, count) = bag.get(slot as usize)?;
    (id != 0 && count != 0 && item_valid(id)).then_some(slot)
}

/// PROT 0941 (Steal) tick body for action `0x51` - the **enemy steal**.
///
/// A `beq`/`slti` chain over `{0, 1, 2, 3, 0xFF}` (`0x801F73AC`..`0x801F73F8`).
///
/// * arm `0` - `ctx[+0x0D] = 1`, halve the framing halfword `ctx[+0x6D0]`,
///   write the per-seat dodge angles `ctx[+0x6E6 + i*2]` for the seven seats
///   that are neither caster nor victim, stage the run clip
///   `FUN_80050E2C(record+0x4C, 1, record[+0x4A])` with the **OR** restage
///   `+0x1DC |= 1` (`0x801F7598`). Advances;
/// * arm `1` - walk the caster to the victim (`FUN_8004E2F0` arrival test),
///   then resolve the steal (below), write the message id `ctx[+0x18] = 0x5B`
///   and double `ctx[+0x6D0]` back. Advances;
/// * arm `2` - `ctx[+0x0D] = 0`, face away, `+0x1DA = 0` with `+0x1DC |= 1`
///   (`0x801F7BCC`/`0x801F7BD4`), `ctx[+0x0D] = 1`. Advances;
/// * arm `3` - latches `ctx[+0x279] = 0xFF` (`0x801F7CFC`) and stays Busy;
/// * arm `0xFF` - clears the busy register.
///
/// The resolution splits on the victim's seat:
///
/// * a **party** seat (`< 3`) - draw a bag slot ([`steal_pick_bag_slot`]),
///   stash the id at `0x801C8FE0 + (caster_seat - 3) * 4`, build the "stole
///   X" line and call the inventory consume helper `FUN_80042310(id, 1)`;
/// * a **monster** seat - `rand() % 100 < steal_table[monster_id * 2]` decides,
///   the item is `steal_table[monster_id * 2 + 1]`, and a hit also bumps the
///   per-seat counter at `0x801C8FE0 + (caster_seat + 5) * 4`.
///
/// The steal table is the same `0x80077828` one `docs/formats/steal-table.md`
/// documents, fields `[chance, item]` - so the enemy-side steal and the
/// player-side one read one table.
///
/// This body reaches **no** damage wrapper. Its ten distinct `jal` targets are
/// `0x80019B28` (angle), `0x8003CA78` / `0x8003CAC4` (string build),
/// `0x80042310` (inventory consume), `0x8004E2F0` (arrival),
/// `0x8004FE5C` (voice), `0x80050E2C` (clip pick), `0x80056798` (rand),
/// `0x801D5854` (camera) and `0x801D8DE8` (message).
///
/// Not ported: the walk, the angles, the string build and the camera.
///
/// Wired: `World::run_cast_module_code`.
///
/// PORT: FUN_801F730C (PROT 0941 action 0x51; phase machine + staging + the
/// steal resolution; walk / camera / string arms unported)
pub fn steal_tick(
    ctx: &mut CastModuleCtx,
    caster: &mut CastActorState,
    run_clip: u8,
    outcome: Option<StealOutcome>,
) -> (CastTickStep, Option<StealOutcome>) {
    let mut taken = None;
    let step = run_chain_tick(ctx, |c| match c.phase {
        0 => {
            c.ctx_0d = 1;
            stage_clip_or(caster, run_clip);
            CastArmStep::Advance
        }
        1 => {
            taken = outcome;
            CastArmStep::Advance
        }
        2 => {
            c.ctx_0d = 0;
            stage_clip_or(caster, 0);
            c.ctx_0d = 1;
            CastArmStep::Advance
        }
        3 => {
            c.phase = LATCHED_DONE_PHASE;
            CastArmStep::Hold
        }
        LATCHED_DONE_PHASE => CastArmStep::Finish,
        // A phase the chain does not name: Done, not retail's Busy.
        _ => CastArmStep::Finish,
    });
    (step, taken)
}

/// PROT 0941 (Steal) tick body for action `0xB9` - a **whole-row sweep**, and
/// a different routine from the `0x51` arm despite sharing the image.
///
/// Five phase arms behind `sltiu v0, v1, 5` (`0x801F6A84`) through the head
/// table at `0x801F69D8` (arm targets `0x801F6AB0`, `6B38`, `6E38`, `7150`,
/// `7290`).
///
/// * arm `1` - hides every seat `0 .. ctx[+0]` (`+0x21C = 0xFF`), then
///   re-shows them (`+0x21C = 0`) once the countdown expires;
/// * arm `2` - the sweep. For each seat `0 .. ctx[+0]` that is alive and not
///   carrying `+0x16E & 4`: `FUN_801DD4B0(0xC0, ctx[+0x13], seat)`, the
///   damage-number popup `FUN_801F44A0`, the **unsigned** clamp against
///   `+0x14C` (`sltu v0, a0, s1` at `0x801F6FD8` - this sweep kills), `+0x10`
///   accumulate, HP write, `+0x1DA = +0x1F1` with `+0x1DC += 1`, and
///   `+0x21D = 2` on **every** seat including the ones it skipped;
/// * arm `4` - terminal: `ctx[+0x0D] = 0`, `ctx[+0x6DA] = 0x780`, busy
///   cleared.
///
/// Not ported: the two `FUN_80024E80` screen-prim fills that bracket the
/// sweep, the `FUN_801D829C` framing and the countdown.
///
/// `damage` is the caller's per-seat roll of the module's own baked power
/// ([`arm_damage_shape_for`]); `sweep` is false on every phase but `2`, which
/// is what keeps the roll off the frames retail never draws.
///
/// Wired: `World::run_cast_module_code`.
///
/// PORT: FUN_801F6A04 (PROT 0941 action 0xB9; phase machine + whole-row sweep;
/// packet / camera arms unported)
pub fn steal_sweep_tick(
    ctx: &mut CastModuleCtx,
    seats: &mut [CastActorState],
    sweep: bool,
    damage: impl Fn(u8) -> i32,
) -> (CastTickStep, Vec<SweepHit>) {
    let mut hits = Vec::new();
    let step = run_arm_tick(ctx, 5, |c| match c.phase {
        1 => {
            // The arm hides every seat (`+0x21C = 0xFF`) for the length of
            // its countdown and shows them again (`+0x21C = 0`) when it
            // expires. The port does not model the countdown, so it applies
            // the arm's **exit** state - the hide is one frame of
            // presentation and leaving it latched would blank the field.
            for seat in 0..c.actor_count {
                if let Some(s) = seats.get_mut(seat as usize) {
                    s.render_flag = 0;
                }
            }
            CastArmStep::Advance
        }
        2 => {
            if sweep {
                for seat in 0..c.actor_count {
                    let Some(s) = seats.get_mut(seat as usize) else {
                        continue;
                    };
                    if aoe_seat_is_hittable(s) {
                        let applied = apply_arm_hit(s, damage(seat), s.restage.wrapping_add(1), 2);
                        hits.push(SweepHit { seat, applied });
                    }
                    s.anim_rate = 2;
                }
            }
            CastArmStep::Advance
        }
        4 => {
            c.ctx_0d = 0;
            CastArmStep::Finish
        }
        _ => CastArmStep::Advance,
    });
    (step, hits)
}

// ---------------------------------------------------------------------------
// PROT 0943 - cast_curse
// ---------------------------------------------------------------------------

/// PROT 0943 (Curse) tick body for action `0x40` - the **single-target**
/// curse.
///
/// Five phase arms behind `sltiu v0, v1, 5` (`0x801F6F7C`) through the head
/// table at **`0x801F69F0`** - not the load base, because the module's other
/// body's five-word table occupies `0x801F69D8..0x801F69EC` first. Arm targets
/// `0x801F6FA8`, `7164`, `7284`, `73C0`, `74C0`.
///
/// * arm `0` - sound cue `0x142`, `caster[+0x1DA] = 0x0B` with `+0x1DC += 1`,
///   hide every party seat flagged in `0x8007BD10` and every live monster
///   seat, hide the victim (`+0x21C = 0xFF`) and put the caster at
///   `+0x21C = 6`;
/// * arm `1` - `caster[+0x1DA] = 0`;
/// * arm `2` - `caster[+0x21C] = 0xFF`, `victim[+0x21C] = 0`;
/// * arm `3` - **`victim[+0x16E] |= 0x1000`** (`0x801F7408`), the curse mark;
/// * arm `4` - terminal: `ctx[+0x0D] = 0`, re-show every seat, busy cleared.
///
/// Not ported: the `FUN_80021B04` / `FUN_80024E80` spawns, the `FUN_801D829C`
/// framings and the countdown at `0x801F7A04`.
///
/// Wired: `World::run_cast_module_code`.
///
/// PORT: FUN_801F6EF4 (PROT 0943 action 0x40; phase machine + the 0x1000
/// curse mark; packet / camera arms unported)
pub fn curse_single_tick(
    ctx: &mut CastModuleCtx,
    caster: &mut CastActorState,
    victim: &mut CastActorState,
) -> CastTickStep {
    run_arm_tick(ctx, 5, |c| match c.phase {
        0 => {
            stage_clip(caster, 0x0B);
            victim.render_flag = 0xFF;
            caster.render_flag = 6;
            CastArmStep::Advance
        }
        1 => {
            caster.staged_anim = 0;
            CastArmStep::Advance
        }
        2 => {
            caster.render_flag = 0xFF;
            victim.render_flag = 0;
            CastArmStep::Advance
        }
        3 => {
            victim.flags |= FLAG_CURSED;
            CastArmStep::Advance
        }
        _ => {
            c.ctx_0d = 0;
            caster.render_flag = 0;
            victim.render_flag = 0;
            CastArmStep::Finish
        }
    })
}

/// PROT 0943 (Curse) tick body for action `0xB5` - the **MP drain**.
///
/// Five phase arms behind `sltiu v0, v1, 5` (`0x801F6A70`) through the head
/// table at `0x801F69D8` (arm targets `0x801F6A9C`, `6B78`, `6C20`, `6DE0`,
/// `6E58`).
///
/// Arm `2` is the whole point: over `0 .. ctx[+0]`, with **no** liveness or
/// `+0x16E & 4` guard at all,
///
/// ```text
/// seat[+0x178] = seat[+0x150]      ; 0x801F6CF4 / 0x801F6CFC
/// seat[+0x150] = 0                 ; 0x801F6D08
/// seat[+0x152] = 0                 ; 0x801F6D1C
/// ```
///
/// so every actor on the field loses its MP and the module keeps the old
/// working value at `+0x178`. Arms `0` and `2` also drive `+0x21C` (hide, then
/// show) across the same range, and arm `4` is terminal (`ctx[+0x0D] = 0`,
/// `ctx[+0x6DA] = 0x780`).
///
/// `docs/subsystems/cast-module.md`'s stat-writer table attributes these two
/// stores to a PROT 0943 routine at `0x801F69D8`. There is no routine there:
/// `0x801F69D8` is this body's own five-word head table, and `0x801F6D08` /
/// `0x801F6D1C` both sit inside this body's frame-matched extent
/// (`0x801F6A04 + 0x4F0 = 0x801F6EF4`).
///
/// Not ported: the `FUN_80050ED4` / `FUN_80024E80` spawns and the countdown.
///
/// Returns the drained MP per seat, in visit order.
///
/// Wired: `World::run_cast_module_code`.
///
/// PORT: FUN_801F6A04 (PROT 0943 action 0xB5; phase machine + the field-wide
/// MP drain; packet arms unported)
pub fn curse_mp_drain_tick(
    ctx: &mut CastModuleCtx,
    seats: &mut [CastActorState],
    exts: &mut [CastArmExtState],
) -> (CastTickStep, Vec<(u8, u16)>) {
    let mut drained = Vec::new();
    let step = run_arm_tick(ctx, 5, |c| match c.phase {
        0 => {
            for seat in 0..c.actor_count {
                if let Some(s) = seats.get_mut(seat as usize) {
                    s.render_flag = 0xFF;
                }
            }
            CastArmStep::Advance
        }
        2 => {
            for seat in 0..c.actor_count {
                if let Some(s) = seats.get_mut(seat as usize) {
                    s.render_flag = 0;
                }
                let Some(e) = exts.get_mut(seat as usize) else {
                    continue;
                };
                e.mp_stash = e.mp;
                drained.push((seat, e.mp));
                e.mp = 0;
                e.mp_base = 0;
            }
            CastArmStep::Advance
        }
        4 => {
            c.ctx_0d = 0;
            CastArmStep::Finish
        }
        _ => CastArmStep::Advance,
    });
    (step, drained)
}

// ---------------------------------------------------------------------------
// PROT 0944 - cast_guilty_cross
// ---------------------------------------------------------------------------

/// PROT 0944 (Guilty Cross) tick body for action `0x37`.
///
/// Six phase arms behind `sltiu v0, v1, 6` (`0x801F6A9C`) through the head
/// table at `0x801F69D8` (arm targets `0x801F6AC8`, `6CC4`, `6E04`, `6FD4`,
/// `7238`, `7378`).
///
/// The damage is rolled in arm `0` and applied in arm `3`, which is the only
/// body in the band that separates them:
///
/// * arm `0` - `0x801F8370 = FUN_801DD6B4(0x38E, ctx[+0x13], caster[+0x1DD])`
///   (`li a0, 0x38e` at `0x801F6AD0`, the `jal` at `0x801F6AD8`; the **bypass**
///   wrapper, so a party defender's resist block is skipped). The latched roll
///   then picks the sound cue - `0x1AD` when it is at least the victim's HP,
///   `0x1AB` otherwise - so the module announces a kill before it lands.
///   Also `caster[+0x1DA] = 8` with `+0x1DC += 1`, `caster[+0x21D] = 2`, and
///   the hide sweep;
/// * arm `1` - `caster[+0x21D] = 4`, `caster[+0x1DA] = 0`;
/// * arm `2` - `caster[+0x21C] = 0xFF`, `victim[+0x21C] = 0`;
/// * arm `3` - the apply: popup, **unsigned** clamp against `+0x14C`,
///   `victim[+0x21D] = 2`, `+0x10` accumulate, `+0x1DC = 1` (an
///   **assignment**), `+0x1DA = +0x1F1`, HP write;
/// * arm `4` - `caster[+0x21C] = 0`;
/// * arm `5` - terminal: re-show all seven seats that are alive and put them
///   back on `+0x21D = 8`, `ctx[+0x0D] = 0`, busy cleared.
///
/// Not ported: the screen-prim washes, the `FUN_80050ED4` spawns, the framing
/// calls and the countdown at `0x801F8360`.
///
/// `hit` is the latched roll; `None` leaves the HP outcome to the band seam's
/// own fold, which is seeded with this same baked power.
///
/// Wired: `World::run_cast_module_code`.
///
/// PORT: FUN_801F6A04 (PROT 0944 action 0x37; phase machine + the latched
/// 0x38E bypass hit; packet / camera arms unported)
pub fn guilty_cross_tick(
    ctx: &mut CastModuleCtx,
    caster: &mut CastActorState,
    victim: &mut CastActorState,
    hit: Option<i32>,
) -> CastTickStep {
    run_arm_tick(ctx, 6, |c| match c.phase {
        0 => {
            stage_clip(caster, 8);
            caster.anim_rate = 2;
            caster.render_flag = 0;
            CastArmStep::Advance
        }
        1 => {
            caster.anim_rate = 4;
            caster.staged_anim = 0;
            CastArmStep::Advance
        }
        2 => {
            caster.render_flag = 0xFF;
            victim.render_flag = 0;
            CastArmStep::Advance
        }
        3 => {
            if let Some(roll) = hit {
                apply_arm_hit(victim, roll, 1, 2);
            }
            CastArmStep::Advance
        }
        4 => {
            caster.render_flag = 0;
            CastArmStep::Advance
        }
        _ => {
            c.ctx_0d = 0;
            caster.anim_rate = ANIM_RATE_NORMAL;
            victim.anim_rate = ANIM_RATE_NORMAL;
            CastArmStep::Finish
        }
    })
}

/// PROT 0944 (Guilty Cross) tick body for action `0x53` - the **scoped**
/// curse, and the routine that spells the band's `+0x1DD` group codes out.
///
/// Five phase arms behind `sltiu v0, v1, 5` (`0x801F74F8`) through the head
/// table at `0x801F69F0` (arm targets `0x801F7524`, `7728`, `7848`, `7AF4`,
/// `7D58`). No damage wrapper.
///
/// Arm `3` ORs `+0x16E |= 0x1000` over a seat set the caster's `+0x1DD`
/// picks, and the three legs are spelled as `sltiu code, 7` /
/// `beq code, 8` / else:
///
/// * `< 7` - the one seat `actor_table[code]`, with no liveness guard;
/// * `== 8` - every live seat in `0 .. ctx[+0]`;
/// * otherwise - every live seat in `3 .. 3 + ctx[+1]`, the monster row.
///
/// Arms `0`..`2` are the same hide / show / stage choreography as the module's
/// sibling (`caster[+0x1DA] = 0x0B` with `+0x1DC += 1`, `caster[+0x21C] = 6`,
/// then `0`), and arm `4` is terminal.
///
/// Not ported: the spawns, the framing and the countdown.
///
/// Returns the seats it marked.
///
/// Wired: `World::run_cast_module_code`.
///
/// PORT: FUN_801F7470 (PROT 0944 action 0x53; phase machine + the scoped
/// 0x1000 curse mark; packet / camera arms unported)
pub fn guilty_cross_curse_tick(
    ctx: &mut CastModuleCtx,
    caster_target_code: u8,
    caster: &mut CastActorState,
    seats: &mut [CastActorState],
) -> (CastTickStep, Vec<u8>) {
    let mut marked = Vec::new();
    let step = run_arm_tick(ctx, 5, |c| match c.phase {
        0 => {
            stage_clip(caster, 0x0B);
            caster.render_flag = 6;
            CastArmStep::Advance
        }
        1 => {
            caster.staged_anim = 0;
            CastArmStep::Advance
        }
        2 => {
            caster.render_flag = 0xFF;
            CastArmStep::Advance
        }
        3 => {
            let single = caster_target_code < 7;
            for seat in arm_target_seats(c, caster_target_code, false) {
                let Some(s) = seats.get_mut(seat as usize) else {
                    continue;
                };
                // The single-seat leg has no liveness guard; the two row legs
                // do (`lh v0, 0x14c(..); beqz`).
                if !single && s.hp == 0 {
                    continue;
                }
                s.flags |= FLAG_CURSED;
                marked.push(seat);
            }
            CastArmStep::Advance
        }
        _ => {
            c.ctx_0d = 0;
            caster.render_flag = 0;
            CastArmStep::Finish
        }
    });
    (step, marked)
}

// ---------------------------------------------------------------------------
// PROT 0950 - cast_rolling_flare
// ---------------------------------------------------------------------------

/// PROT 0950 (Rolling Flare) tick body for action `0x5A` - the single-target
/// charge.
///
/// Five phase arms behind `sltiu v0, v1, 5` (`0x801F7A84`) through the head
/// table at **`0x801F6A10`** - the module's fourteen-word table for its other
/// body fills `0x801F69D8..0x801F6A10` first, and the other body opens at
/// `0x801F6A24` where this table ends. Arm targets `0x801F7AB0`, `7C10`,
/// `7CF0`, `7E20`, `8028`.
///
/// * arm `0` - `caster[+0x1DA] = 1` with `+0x1DC += 1`;
/// * arm `2` - `caster[+0x1DA] = 8` with `+0x1DC += 1`;
/// * arm `3` - `caster[+0x21C] = victim[+0x21C] = 7` and `+0x21F = 1` on both
///   as the countdown closes, then `FUN_801DD4B0(0x100, ctx[+0x13],
///   caster[+0x1DD])` (`li a0, 0x100` at `0x801F7F5C`, `jal` at `0x801F7F80`),
///   the popup, the **unsigned** clamp, `victim[+0x1DC] = 1`, `+0x10`
///   accumulate, `+0x1DA = +0x1F1`, HP write, then `caster[+0x21B] = 0`,
///   `caster[+0x176] = 0`, `caster[+0x1DA] = 0` and `+0x21C = 0` on both;
/// * arm `4` - terminal, and it **waits on the victim's clip**: it holds until
///   `victim[+0x1D9]` is `8` when the victim is dead and `0` when it is not
///   (`0x801F80E0`..`0x801F80FC`), then writes the monster record's
///   `+0x1F = 0x25`, the caster object's `+0x58 = 0x4A0`, clears `ctx[+0x0D]`
///   and the busy register.
///
/// That clip gate is a simulation read, not presentation: it is why the arm
/// can sit on phase `4` for an unbounded number of frames.
///
/// Not ported: the unaligned pose copies out of `0x801F86C0`, the framing and
/// the countdown at `0x801F86B0`.
///
/// Wired: `World::run_cast_module_code`.
///
/// PORT: FUN_801F79F8 (PROT 0950 action 0x5A; phase machine + the 0x100 hit +
/// the clip gate; pose / camera arms unported)
pub fn rolling_flare_tick(
    ctx: &mut CastModuleCtx,
    caster: &mut CastActorState,
    victim: &mut CastActorState,
    hit: Option<i32>,
) -> CastTickStep {
    run_arm_tick(ctx, 5, |c| match c.phase {
        0 => {
            stage_clip(caster, 1);
            CastArmStep::Advance
        }
        2 => {
            stage_clip(caster, 8);
            CastArmStep::Advance
        }
        3 => {
            caster.render_flag = 7;
            victim.render_flag = 7;
            if let Some(roll) = hit {
                apply_arm_hit(victim, roll, 1, victim.anim_rate);
            }
            caster.staged_anim = 0;
            caster.render_flag = 0;
            victim.render_flag = 0;
            CastArmStep::Advance
        }
        4 => {
            let want = if victim.hp == 0 { 8 } else { 0 };
            if victim.playing_anim != want {
                return CastArmStep::Hold;
            }
            c.ctx_0d = 0;
            CastArmStep::Finish
        }
        _ => CastArmStep::Advance,
    })
}

/// PROT 0950 (Rolling Flare) tick body for action `0xAB` - the band's longest
/// arm map, and a **whole-row sweep**.
///
/// Fourteen phase arms behind `sltiu v0, v1, 0xe` (`0x801F6A9C`) through the
/// head table at `0x801F69D8` (arm targets `0x801F6AC8`, `6B50`, `6C80`,
/// `6DEC`, `6F04`, `6FD4`, `7088`, `71A0`, `7280`, `73F0`, `74C4`, `7848`,
/// `78E0`, `793C`).
///
/// * arm `1` - hides every seat `0 .. ctx[+0]`, then re-shows them;
/// * arm `6` - `caster[+0x1DA] = 10` with `+0x1DC += 1`;
/// * arms `8` and `9` - `caster[+0x21D] = 0`;
/// * arm `10` - `caster[+0x21D] = 2`, `caster[+0x1DA] = 0`, then the sweep:
///   for each seat `0 .. ctx[+0]` that is alive and not `+0x16E & 4`,
///   `FUN_801DD4B0(0x29A, ctx[+0x13], seat)` (`li a0, 0x29a` at `0x801F7700`,
///   `jal` at `0x801F770C`), the popup, the **unsigned** clamp, `+0x10`,
///   HP write, `+0x1DA = +0x1F1` with `+0x1DC += 1` and `+0x21D = 4`;
/// * arm `13` - terminal: every seat back to `+0x21D = 8`, `ctx[+0x0D] = 0`,
///   busy cleared.
///
/// Not ported: the eleven other arms, which are pool spawns, screen-prim
/// washes, the framing walk and the countdown.
///
/// Wired: `World::run_cast_module_code`.
///
/// PORT: FUN_801F6A24 (PROT 0950 action 0xAB; 14-arm phase machine +
/// whole-row 0x29A sweep; packet / camera arms unported)
pub fn rolling_flare_sweep_tick(
    ctx: &mut CastModuleCtx,
    caster: &mut CastActorState,
    seats: &mut [CastActorState],
    sweep: bool,
    damage: impl Fn(u8) -> i32,
) -> (CastTickStep, Vec<SweepHit>) {
    let mut hits = Vec::new();
    let step = run_arm_tick(ctx, 14, |c| match c.phase {
        1 => {
            // As in PROT 0941's sweep: the arm's countdown brackets a hide
            // and a show, and the port applies the exit state.
            for seat in 0..c.actor_count {
                if let Some(s) = seats.get_mut(seat as usize) {
                    s.render_flag = 0;
                }
            }
            CastArmStep::Advance
        }
        6 => {
            stage_clip(caster, 10);
            CastArmStep::Advance
        }
        8 | 9 => {
            caster.anim_rate = 0;
            CastArmStep::Advance
        }
        10 => {
            caster.anim_rate = 2;
            caster.staged_anim = 0;
            if sweep {
                for seat in 0..c.actor_count {
                    let Some(s) = seats.get_mut(seat as usize) else {
                        continue;
                    };
                    if !aoe_seat_is_hittable(s) {
                        continue;
                    }
                    let applied = apply_arm_hit(s, damage(seat), s.restage.wrapping_add(1), 4);
                    hits.push(SweepHit { seat, applied });
                }
            }
            CastArmStep::Advance
        }
        13 => {
            for seat in 0..c.actor_count {
                if let Some(s) = seats.get_mut(seat as usize) {
                    s.anim_rate = ANIM_RATE_NORMAL;
                }
            }
            c.ctx_0d = 0;
            CastArmStep::Finish
        }
        _ => CastArmStep::Advance,
    });
    (step, hits)
}

// ---------------------------------------------------------------------------
// PROT 0956 - cast_water_hazard
// ---------------------------------------------------------------------------

/// PROT 0956 (Water Hazard) tick body for action `0x71` - the band's only
/// **two-wrapper** arm, and a sweep whose row the caster's `+0x1DD` picks.
///
/// A `beq`/`slti` chain over `{0, 1, 2, 3, 0xFF}` (`0x801F7338`..`0x801F7384`).
///
/// Arm `2` holds both damage sites, one per leg, and both bake `0xD0` into
/// `FUN_801DD4B0` (`li a0, 0xd0` at `0x801F78C4` and `0x801F7A38`; the `jal`s
/// at `0x801F78D0` and `0x801F7A40`). Both clamp unsigned against `+0x14C`,
/// so both legs kill.
///
/// * the **low** leg (`caster[+0x1DD] < 3` or `== 8`) sweeps `0 .. ctx[+0]`,
///   staging `+0x1DA = +0x1F1` with `+0x1DC = 1`;
/// * the **monster** leg sweeps `3 .. 3 + ctx[+1]`, **skipping the caster's
///   own seat**, and picks the reaction clip the way the rest of the band
///   does - `+0x1F1` with `+0x1DC = 1` when the seat died or `+0x1F2 != 0`,
///   else `+0x1EF` with `+0x1DC = 5`.
///
/// Either leg then rolls `FUN_80056798() & 7 == 0` per hit seat and ORs
/// `+0x16E |= 1` on a zero. Arm `3` latches `ctx[+0x279] = 0xFF`, arm `0xFF`
/// clears the busy register, and arm `1` puts the caster on `+0x21C = 9`.
///
/// Not ported: the twelve-step ring of `FUN_80021B04` spawns arm `2` opens
/// with, the framing and the countdown at `0x801F86A0`.
///
/// `damage` is the caller's per-seat roll; `status` is the caller's per-seat
/// `rand()`, of which only `& 7 == 0` is read.
///
/// Wired: `World::run_cast_module_code`.
///
/// PORT: FUN_801F7298 (PROT 0956 action 0x71; chain phase machine + the
/// two-legged 0xD0 sweep + the 1-in-8 status; packet / camera arms unported)
pub fn water_hazard_tick(
    ctx: &mut CastModuleCtx,
    caster_seat: u8,
    caster_target_code: u8,
    caster: &mut CastActorState,
    seats: &mut [CastActorState],
    damage: impl Fn(u8) -> i32,
    status: impl Fn(u8) -> u32,
) -> (CastTickStep, Vec<SweepHit>) {
    let mut hits = Vec::new();
    let step = run_chain_tick(ctx, |c| match c.phase {
        0 => CastArmStep::Advance,
        1 => {
            caster.render_flag = 9;
            CastArmStep::Advance
        }
        2 => {
            let low_leg = caster_target_code == TARGET_CODE_LOW_ROW
                || caster_target_code < FIRST_MONSTER_SEAT;
            for seat in arm_target_seats(c, caster_target_code, true) {
                if !low_leg && seat == caster_seat {
                    continue;
                }
                let Some(s) = seats.get_mut(seat as usize) else {
                    continue;
                };
                if !aoe_seat_is_hittable(s) {
                    continue;
                }
                let roll = damage(seat);
                let applied = if low_leg {
                    apply_arm_hit(s, roll, 1, s.anim_rate)
                } else {
                    let hp = u32::from(s.hp);
                    let mut dmg = roll as u32;
                    if hp < dmg {
                        dmg = hp;
                    }
                    s.hp_bar_delta = s.hp_bar_delta.wrapping_add(dmg as i32);
                    s.hp = hp.wrapping_sub(dmg) as u16;
                    if s.hp == 0 || s.reaction_gate != 0 {
                        s.staged_anim = s.knockdown_anim;
                        s.restage = 1;
                    } else {
                        s.staged_anim = s.reaction_alt;
                        s.restage = 5;
                    }
                    dmg
                };
                if status(seat) & 7 == 0 {
                    s.flags |= FLAG_WATER_HAZARD_STATUS;
                }
                hits.push(SweepHit { seat, applied });
            }
            CastArmStep::Advance
        }
        3 => {
            c.phase = LATCHED_DONE_PHASE;
            CastArmStep::Hold
        }
        LATCHED_DONE_PHASE => {
            c.ctx_0d = 0;
            caster.render_flag = 0;
            CastArmStep::Finish
        }
        // A phase the chain does not name: Done, not retail's Busy.
        _ => CastArmStep::Finish,
    });
    (step, hits)
}

// ---------------------------------------------------------------------------
// PROT 0962 - cast_blade_breath
// ---------------------------------------------------------------------------

/// One of PROT 0962's three near-identical charge bodies, as a set of the
/// four constants that differ between them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BladeBreathShape {
    /// The clip arm `2` stages on the caster (`+0x1DA`).
    pub windup_clip: u8,
    /// The animation rate arm `2` puts the caster on, `None` where the arm
    /// writes none.
    pub windup_rate: Option<u8>,
    /// The `+0x21F` render sub-flag arm `3` writes on the victim, `None`
    /// where it writes none.
    pub strike_sub_flag: Option<u8>,
    /// Whose `+0x1D9` clip arm `4` waits on before latching `0xFF`: `true`
    /// the victim's, `false` the caster's.
    pub tail_waits_on_victim: bool,
}

/// `0xA2` at `0x801F7AE4`: windup clip `5`, `victim[+0x21F] = 1`, tail on the
/// victim.
pub const BLADE_BREATH_A: BladeBreathShape = BladeBreathShape {
    windup_clip: 5,
    windup_rate: None,
    strike_sub_flag: Some(1),
    tail_waits_on_victim: true,
};
/// `0xA3` at `0x801F74A0`: windup clip `6` at rate `3`,
/// `victim[+0x21F] = 2`, tail on the caster (`caster[+0x21D] = 2` then the
/// `+0x1DC` bump).
pub const BLADE_BREATH_B: BladeBreathShape = BladeBreathShape {
    windup_clip: 6,
    windup_rate: Some(3),
    strike_sub_flag: Some(2),
    tail_waits_on_victim: false,
};
/// `0xA4` at `0x801F6D54`: windup clip `7` at rate `4`, no `+0x21F`, tail on
/// the victim.
pub const BLADE_BREATH_C: BladeBreathShape = BladeBreathShape {
    windup_clip: 7,
    windup_rate: Some(4),
    strike_sub_flag: None,
    tail_waits_on_victim: true,
};

/// The baked power all three PROT 0962 bodies hand `FUN_801DD4B0`
/// (`li a0, 0x200` at `0x801F7F08`, `0x801F787C` and `0x801F72A0`).
pub const BLADE_BREATH_POWER: u16 = 0x0200;

/// The shared body of PROT 0962's three charge arms.
///
/// All three dispatch through a `beq`/`slti` chain over
/// `{0, 1, 2, 3, 4, 0xFF}` and run the same five steps:
///
/// * arm `0` - **stores** `ctx[+0x279] = 1` outright (not an increment), then
///   tests arrival with `FUN_8004E2F0(ctx[+0x13], caster[+0x1DD])`. Arrived,
///   it steps `FUN_80050BB8` 0x20 times and advances the phase again, so the
///   walk is skipped; not arrived, it stages the run clip `1` with
///   `+0x1DC += 1` and stays on phase `1`;
/// * arm `1` - the same walk, advancing once arrival reports;
/// * arm `2` - the sound cue (`0x18B` / `0x18C` / `0x18D`), the facing, the
///   windup clip with `+0x1DC += 1`, and the windup animation rate;
/// * arm `3` - gated on the caster object's `+0x68` reaching `0x300`, then
///   `FUN_801DD4B0(0x200, ctx[+0x13], caster[+0x1DD])`, the popup, the
///   **unsigned** clamp, the `+0x21F` sub-flag, `victim[+0x21D] = 4`, `+0x10`
///   accumulate, HP write, `+0x1DA = +0x1F1` with `+0x1DC += 1`, and
///   `caster[+0x1DA] = 0`;
/// * arm `4` - `victim[+0x21D] = 4`, hold until the watched actor's `+0x1D9`
///   is `0` or `8`, then latch `ctx[+0x279] = 0xFF`;
/// * arm `0xFF` - clears the busy register.
///
/// Not ported: the walk itself, the facing, the cue, the `FUN_80050ED4`
/// spawns, the screen-prim wash and the countdown at `0x801F89AC`.
///
/// PORT: FUN_801F7AE4, FUN_801F74A0, FUN_801F6D54 (PROT 0962 actions 0xA2 /
/// 0xA3 / 0xA4; chain phase machine + the 0x200 hit; walk / packet arms
/// unported)
pub fn blade_breath_tick(
    ctx: &mut CastModuleCtx,
    shape: &BladeBreathShape,
    caster: &mut CastActorState,
    victim: &mut CastActorState,
    arrived: bool,
    hit: Option<i32>,
) -> CastTickStep {
    run_chain_tick(ctx, |c| match c.phase {
        0 => {
            c.phase = 1;
            if arrived {
                CastArmStep::Advance
            } else {
                stage_clip(caster, 1);
                CastArmStep::Hold
            }
        }
        1 => {
            if arrived {
                CastArmStep::Advance
            } else {
                CastArmStep::Hold
            }
        }
        2 => {
            stage_clip(caster, shape.windup_clip);
            if let Some(rate) = shape.windup_rate {
                caster.anim_rate = rate;
            }
            CastArmStep::Advance
        }
        3 => {
            if let Some(roll) = hit {
                apply_arm_hit(victim, roll, victim.restage.wrapping_add(1), 4);
            }
            caster.staged_anim = 0;
            CastArmStep::Advance
        }
        4 => {
            victim.anim_rate = 4;
            let watched = if shape.tail_waits_on_victim {
                victim.playing_anim
            } else {
                caster.playing_anim
            };
            if watched != 0 && watched != 8 {
                return CastArmStep::Hold;
            }
            if !shape.tail_waits_on_victim {
                caster.anim_rate = 2;
                caster.restage = caster.restage.wrapping_add(1);
            }
            c.phase = LATCHED_DONE_PHASE;
            CastArmStep::Hold
        }
        LATCHED_DONE_PHASE => CastArmStep::Finish,
        // A phase the chain does not name: Done, not retail's Busy.
        _ => CastArmStep::Finish,
    })
}

/// PROT 0962's `0xA2` arm - [`blade_breath_tick`] with [`BLADE_BREATH_A`].
///
/// Wired: `World::run_cast_module_code`.
pub fn blade_breath_a_tick(
    ctx: &mut CastModuleCtx,
    caster: &mut CastActorState,
    victim: &mut CastActorState,
    arrived: bool,
    hit: Option<i32>,
) -> CastTickStep {
    blade_breath_tick(ctx, &BLADE_BREATH_A, caster, victim, arrived, hit)
}

/// PROT 0962's `0xA3` arm - [`blade_breath_tick`] with [`BLADE_BREATH_B`].
///
/// Wired: `World::run_cast_module_code`.
pub fn blade_breath_b_tick(
    ctx: &mut CastModuleCtx,
    caster: &mut CastActorState,
    victim: &mut CastActorState,
    arrived: bool,
    hit: Option<i32>,
) -> CastTickStep {
    blade_breath_tick(ctx, &BLADE_BREATH_B, caster, victim, arrived, hit)
}

/// PROT 0962's `0xA4` arm - [`blade_breath_tick`] with [`BLADE_BREATH_C`].
///
/// Wired: `World::run_cast_module_code`.
pub fn blade_breath_c_tick(
    ctx: &mut CastModuleCtx,
    caster: &mut CastActorState,
    victim: &mut CastActorState,
    arrived: bool,
    hit: Option<i32>,
) -> CastTickStep {
    blade_breath_tick(ctx, &BLADE_BREATH_C, caster, victim, arrived, hit)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx_with(phase: u8, actors: u8, monsters: u8) -> CastModuleCtx {
        CastModuleCtx {
            actor_count: actors,
            monster_count: monsters,
            caster_seat: 3,
            phase,
            ..Default::default()
        }
    }

    fn actor(hp: u16) -> CastActorState {
        CastActorState {
            hp,
            anim_rate: ANIM_RATE_NORMAL,
            knockdown_anim: 0x0C,
            ..Default::default()
        }
    }

    #[test]
    fn the_three_shared_body_vas_are_one_value_and_three_arms() {
        assert_eq!(STEAL_SWEEP_TICK, CURSE_MP_DRAIN_TICK);
        assert_eq!(STEAL_SWEEP_TICK, GUILTY_CROSS_TICK);
        // ...and the damage shape is keyed on the pair, so the same VA gives
        // three different answers.
        assert_eq!(
            arm_damage_shape_for(941, STEAL_SWEEP_TICK).map(|s| s.powers[0]),
            Some(0x00C0)
        );
        assert!(arm_damage_shape_for(943, CURSE_MP_DRAIN_TICK).is_none());
        assert_eq!(
            arm_damage_shape_for(944, GUILTY_CROSS_TICK).map(|s| s.powers[0]),
            Some(0x038E)
        );
    }

    #[test]
    fn steal_carries_no_damage_wrapper() {
        // The doc's fourteen-arm table grades `0x801F730C` as one wrapper;
        // the body reaches none.
        assert!(arm_damage_shape_for(941, STEAL_TICK).is_none());
    }

    #[test]
    fn the_glare_blind_arm_blanks_the_first_monster_seats_reaction_run() {
        let mut ctx = ctx_with(4, 4, 1);
        let mut seat = CastActorState {
            reaction_alt: 3,
            reaction_alt2: 4,
            knockdown_anim: 5,
            reaction_gate: 6,
            ..Default::default()
        };
        let mut ext = CastArmExtState {
            reaction_extra: 7,
            ..Default::default()
        };
        assert_eq!(
            glare_divide_blind_tick(&mut ctx, &mut seat, &mut ext),
            CastTickStep::Busy
        );
        assert_eq!(seat.reaction_alt, 0);
        assert_eq!(seat.reaction_alt2, 0);
        assert_eq!(seat.knockdown_anim, 0);
        assert_eq!(seat.reaction_gate, 0);
        assert_eq!(ext.reaction_extra, 0);
        assert_eq!(ctx.phase, 5, "the arm advances");
    }

    #[test]
    fn the_glare_blind_terminal_arm_parks_and_reports_done() {
        let mut ctx = ctx_with(7, 4, 1);
        ctx.ctx_0d = 1;
        let mut seat = actor(100);
        let mut ext = CastArmExtState::default();
        assert_eq!(
            glare_divide_blind_tick(&mut ctx, &mut seat, &mut ext),
            CastTickStep::Done
        );
        assert_eq!(ctx.phase, 7, "the terminal arm does not advance");
        assert_eq!(ctx.ctx_0d, 0);
    }

    #[test]
    fn the_split_allocates_the_next_monster_seat_and_burns_a_turn() {
        let mut ctx = ctx_with(2, 5, 2);
        let mut caster = actor(250);
        caster.target_code = 1;
        let (step, split) = glare_divide_split_tick(
            &mut ctx,
            &mut caster,
            &CastArmExtState::default(),
            0x50,
            0,
            None,
        );
        let split = split.expect("arm 2 splits");
        assert_eq!(step, CastTickStep::Busy);
        assert_eq!(split.clone_seat, FIRST_MONSTER_SEAT + 2);
        assert_eq!(split.clone_hp, 250);
        assert_eq!(split.saved_caster_target, 1);
        assert_eq!(split.weakened, None, "0x50 splits at full HP");
        assert_eq!(ctx.monster_count, 3);
        assert_eq!(ctx.turn_cursor, 1);
        assert_eq!(caster.target_code, 9);
        assert_eq!(caster.render_flag, 0);
    }

    #[test]
    fn the_0xae_split_can_weaken_the_caster_itself() {
        let mut ctx = ctx_with(2, 5, 1);
        let mut caster = actor(250);
        caster.atk = 90;
        caster.agl = 400;
        let (_, split) = glare_divide_split_tick(
            &mut ctx,
            &mut caster,
            &CastArmExtState::default(),
            0xAE,
            0,
            Some(1),
        );
        assert_eq!(split.unwrap().weakened, Some(SplitWeakened::Caster));
        assert_eq!(caster.hp, 1, "the coin landed on the original");
        assert_eq!(caster.atk, 1);
        assert_eq!(caster.agl, SPLIT_WEAK_AGL);
        assert_eq!(caster.agl_base, SPLIT_WEAK_AGL);

        let mut ctx = ctx_with(2, 5, 1);
        let mut caster = actor(250);
        let (_, split) = glare_divide_split_tick(
            &mut ctx,
            &mut caster,
            &CastArmExtState::default(),
            0xAE,
            0,
            Some(0),
        );
        assert_eq!(split.unwrap().weakened, Some(SplitWeakened::Clone));
        assert_eq!(caster.hp, 250, "the coin landed on the copy");
    }

    #[test]
    fn the_split_latches_ff_and_restores_the_casters_target() {
        let mut ctx = ctx_with(3, 5, 1);
        let mut caster = actor(250);
        caster.target_code = 9;
        let (step, _) = glare_divide_split_tick(
            &mut ctx,
            &mut caster,
            &CastArmExtState::default(),
            0x50,
            4,
            None,
        );
        assert_eq!(step, CastTickStep::Busy);
        assert_eq!(ctx.phase, LATCHED_DONE_PHASE, "arm 3 latches, not advances");
        let (step, _) = glare_divide_split_tick(
            &mut ctx,
            &mut caster,
            &CastArmExtState::default(),
            0x50,
            4,
            None,
        );
        assert_eq!(step, CastTickStep::Done);
        assert_eq!(caster.target_code, 4);
    }

    #[test]
    fn the_steal_sweep_kills_and_stages_the_knockdown() {
        let mut ctx = ctx_with(2, 4, 1);
        let mut seats = vec![actor(40), actor(0), actor(500), actor(9000)];
        seats[3].flags = crate::cast_module_ticks::FLAG_NON_TARGETABLE;
        let (step, hits) = steal_sweep_tick(&mut ctx, &mut seats, true, |_| 60);
        assert_eq!(step, CastTickStep::Busy);
        // Seat 1 is dead and seat 3 is non-targetable, so the sweep visits 0
        // and 2 only.
        assert_eq!(hits.len(), 2);
        assert_eq!(seats[0].hp, 0, "the unsigned clamp lets the hit kill");
        assert_eq!(hits[0].applied, 40, "clamped to the victim's live HP");
        assert_eq!(seats[2].hp, 440);
        assert_eq!(seats[0].staged_anim, 0x0C);
        assert_eq!(seats[1].anim_rate, 2, "even a skipped seat gets +0x21D");
    }

    #[test]
    fn a_negative_roll_kills_on_this_bands_unsigned_clamp() {
        let mut v = actor(300);
        let applied = apply_arm_hit(&mut v, -5, 1, 2);
        assert_eq!(applied, 300);
        assert_eq!(v.hp, 0);
    }

    #[test]
    fn the_mp_drain_empties_every_seat_and_stashes_the_old_value() {
        let mut ctx = ctx_with(2, 4, 1);
        let mut seats = vec![actor(10), actor(0), actor(80), actor(90)];
        let mut exts = vec![
            CastArmExtState {
                mp: 30,
                mp_base: 30,
                ..Default::default()
            },
            CastArmExtState {
                mp: 12,
                mp_base: 12,
                ..Default::default()
            },
            CastArmExtState::default(),
            CastArmExtState {
                mp: 7,
                mp_base: 7,
                ..Default::default()
            },
        ];
        let (step, drained) = curse_mp_drain_tick(&mut ctx, &mut seats, &mut exts);
        assert_eq!(step, CastTickStep::Busy);
        // No liveness guard: seat 1 is dead and still drains.
        assert_eq!(drained.len(), 4);
        assert_eq!(drained[1], (1, 12));
        assert!(exts.iter().all(|e| e.mp == 0 && e.mp_base == 0));
        assert_eq!(exts[0].mp_stash, 30);
    }

    #[test]
    fn the_scoped_curse_marks_the_row_the_target_code_names() {
        let mut seats = vec![actor(10), actor(20), actor(30), actor(40), actor(0)];
        // `8` - the low row `0 .. ctx[+0]`.
        let mut ctx = ctx_with(3, 3, 2);
        let mut caster = actor(99);
        let (_, marked) = guilty_cross_curse_tick(&mut ctx, 8, &mut caster, &mut seats);
        assert_eq!(marked, vec![0, 1, 2]);
        // `9` - the monster row, and the dead seat 4 is skipped.
        let mut ctx = ctx_with(3, 3, 2);
        let mut seats = vec![actor(10), actor(20), actor(30), actor(40), actor(0)];
        let (_, marked) = guilty_cross_curse_tick(&mut ctx, 9, &mut caster, &mut seats);
        assert_eq!(marked, vec![3]);
        assert_eq!(seats[3].flags & FLAG_CURSED, FLAG_CURSED);
        // A code below 7 is one seat, with no liveness guard.
        let mut ctx = ctx_with(3, 3, 2);
        let mut seats = vec![actor(10), actor(20), actor(30), actor(40), actor(0)];
        let (_, marked) = guilty_cross_curse_tick(&mut ctx, 4, &mut caster, &mut seats);
        assert_eq!(marked, vec![4]);
    }

    #[test]
    fn guilty_cross_applies_its_latched_roll_on_arm_three_only() {
        let mut ctx = ctx_with(0, 4, 1);
        let mut caster = actor(500);
        let mut victim = actor(800);
        for _ in 0..3 {
            guilty_cross_tick(&mut ctx, &mut caster, &mut victim, Some(200));
        }
        assert_eq!(victim.hp, 800, "arms 0..2 do not apply");
        assert_eq!(ctx.phase, 3);
        guilty_cross_tick(&mut ctx, &mut caster, &mut victim, Some(200));
        assert_eq!(victim.hp, 600);
        assert_eq!(victim.restage, 1, "an assignment, not a bump");
        assert_eq!(victim.staged_anim, 0x0C);
    }

    #[test]
    fn rolling_flares_tail_waits_on_the_victims_clip() {
        let mut ctx = ctx_with(4, 4, 1);
        let mut caster = actor(500);
        let mut victim = actor(120);
        victim.playing_anim = 3;
        assert_eq!(
            rolling_flare_tick(&mut ctx, &mut caster, &mut victim, None),
            CastTickStep::Busy
        );
        assert_eq!(ctx.phase, 4, "it holds");
        victim.playing_anim = 0;
        assert_eq!(
            rolling_flare_tick(&mut ctx, &mut caster, &mut victim, None),
            CastTickStep::Done
        );
        // A dead victim waits for clip 8, not 0.
        let mut ctx = ctx_with(4, 4, 1);
        let mut victim = actor(0);
        victim.playing_anim = 0;
        assert_eq!(
            rolling_flare_tick(&mut ctx, &mut caster, &mut victim, None),
            CastTickStep::Busy
        );
        victim.playing_anim = 8;
        assert_eq!(
            rolling_flare_tick(&mut ctx, &mut caster, &mut victim, None),
            CastTickStep::Done
        );
    }

    #[test]
    fn the_rolling_flare_sweep_runs_on_arm_ten() {
        assert_eq!(arm_sweep_arm(950, ROLLING_FLARE_SWEEP_TICK), Some(10));
        let mut ctx = ctx_with(10, 3, 0);
        let mut caster = actor(500);
        let mut seats = vec![actor(100), actor(100), actor(100)];
        let (step, hits) =
            rolling_flare_sweep_tick(&mut ctx, &mut caster, &mut seats, true, |_| 25);
        assert_eq!(step, CastTickStep::Busy);
        assert_eq!(hits.len(), 3);
        assert!(seats.iter().all(|s| s.hp == 75 && s.anim_rate == 4));
        assert_eq!(caster.anim_rate, 2);
    }

    #[test]
    fn water_hazard_picks_its_row_off_the_target_code() {
        // The monster leg skips the caster's own seat.
        let mut ctx = ctx_with(2, 3, 3);
        let mut caster = actor(900);
        let mut seats = vec![
            actor(50),
            actor(50),
            actor(50),
            actor(500),
            actor(500),
            actor(500),
        ];
        let (step, hits) = water_hazard_tick(
            &mut ctx,
            4,
            9,
            &mut caster,
            &mut seats,
            |_| 100,
            |_| 1, // never 0 mod 8: no status
        );
        assert_eq!(step, CastTickStep::Busy);
        assert_eq!(
            hits.iter().map(|h| h.seat).collect::<Vec<_>>(),
            vec![3, 5],
            "seat 4 is the caster"
        );
        assert!(
            seats
                .iter()
                .all(|s| s.flags & FLAG_WATER_HAZARD_STATUS == 0)
        );
        // The low leg sweeps `0 .. ctx[+0]` and stages the knockdown.
        let mut ctx = ctx_with(2, 3, 3);
        let mut seats = vec![actor(50), actor(50), actor(50)];
        let (_, hits) = water_hazard_tick(
            &mut ctx,
            4,
            8,
            &mut caster,
            &mut seats,
            |_| 20,
            |_| 8, // 8 & 7 == 0: the status lands
        );
        assert_eq!(hits.len(), 3);
        assert!(seats.iter().all(|s| s.hp == 30 && s.restage == 1));
        assert!(
            seats
                .iter()
                .all(|s| s.flags & FLAG_WATER_HAZARD_STATUS != 0)
        );
    }

    #[test]
    fn water_hazards_monster_leg_picks_the_alternate_reaction() {
        let mut ctx = ctx_with(2, 3, 2);
        let mut caster = actor(900);
        let mut seats = vec![
            actor(10),
            actor(10),
            actor(10),
            CastActorState {
                hp: 400,
                knockdown_anim: 0x0C,
                reaction_alt: 0x07,
                reaction_gate: 0,
                anim_rate: ANIM_RATE_NORMAL,
                ..Default::default()
            },
            CastActorState {
                hp: 400,
                knockdown_anim: 0x0C,
                reaction_alt: 0x07,
                reaction_gate: 1,
                anim_rate: ANIM_RATE_NORMAL,
                ..Default::default()
            },
        ];
        water_hazard_tick(&mut ctx, 9, 9, &mut caster, &mut seats, |_| 10, |_| 1);
        assert_eq!(seats[3].staged_anim, 0x07, "gate clear takes +0x1EF");
        assert_eq!(seats[3].restage, 5);
        assert_eq!(seats[4].staged_anim, 0x0C, "gate set takes +0x1F1");
        assert_eq!(seats[4].restage, 1);
    }

    #[test]
    fn the_three_blade_breaths_differ_only_in_four_constants() {
        assert_eq!(BLADE_BREATH_A.windup_clip, 5);
        assert_eq!(BLADE_BREATH_B.windup_clip, 6);
        assert_eq!(BLADE_BREATH_C.windup_clip, 7);
        for (entry, body) in [
            (962, BLADE_BREATH_A_TICK),
            (962, BLADE_BREATH_B_TICK),
            (962, BLADE_BREATH_C_TICK),
        ] {
            assert_eq!(
                arm_damage_shape_for(entry, body).map(|s| s.powers[0]),
                Some(BLADE_BREATH_POWER)
            );
        }
    }

    #[test]
    fn blade_breath_skips_the_walk_when_the_caster_has_arrived() {
        let mut ctx = ctx_with(0, 4, 1);
        let mut caster = actor(500);
        let mut victim = actor(700);
        // Not arrived: arm 0 stores phase 1 and holds there with the run clip.
        blade_breath_a_tick(&mut ctx, &mut caster, &mut victim, false, None);
        assert_eq!(ctx.phase, 1);
        assert_eq!(caster.staged_anim, 1);
        // Arrived: arm 0 stores 1 and advances past it.
        let mut ctx = ctx_with(0, 4, 1);
        blade_breath_a_tick(&mut ctx, &mut caster, &mut victim, true, None);
        assert_eq!(ctx.phase, 2);
    }

    #[test]
    fn blade_breath_applies_its_0x200_hit_and_latches_ff() {
        let mut ctx = ctx_with(3, 4, 1);
        let mut caster = actor(500);
        let mut victim = actor(700);
        blade_breath_c_tick(&mut ctx, &mut caster, &mut victim, true, Some(250));
        assert_eq!(victim.hp, 450);
        assert_eq!(victim.anim_rate, 4);
        assert_eq!(ctx.phase, 4);
        victim.playing_anim = 0;
        let step = blade_breath_c_tick(&mut ctx, &mut caster, &mut victim, true, None);
        assert_eq!(step, CastTickStep::Busy);
        assert_eq!(ctx.phase, LATCHED_DONE_PHASE);
        let step = blade_breath_c_tick(&mut ctx, &mut caster, &mut victim, true, None);
        assert_eq!(step, CastTickStep::Done);
    }

    #[test]
    fn a_phase_past_a_table_bodys_bound_reports_done() {
        let mut ctx = ctx_with(9, 4, 1);
        let mut seat = actor(10);
        let mut ext = CastArmExtState::default();
        assert_eq!(
            glare_divide_blind_tick(&mut ctx, &mut seat, &mut ext),
            CastTickStep::Done,
            "retail reports Busy here; the port reports Done (anti-softlock)"
        );
    }

    #[test]
    fn the_steal_bag_draw_rejects_empty_and_invalid_slots() {
        let mut bag = vec![(0u8, 0u8); 256];
        bag[7] = (0x20, 3);
        bag[9] = (0x21, 0); // a zero count is rejected
        bag[11] = (0x22, 4); // an invalid item id is rejected
        let mut seq = [11u32, 9, 7].into_iter().cycle();
        let slot = steal_pick_bag_slot(&bag, None, || seq.next().unwrap(), |id| id != 0x22);
        assert_eq!(slot, Some(7));
        // An all-empty bag exhausts the 0x400 budget.
        let empty = vec![(0u8, 0u8); 256];
        assert_eq!(
            steal_pick_bag_slot(&empty, None, || 5, |_| true),
            None,
            "0x400 draws and no valid slot"
        );
    }

    #[test]
    fn the_steal_body_reports_its_outcome_on_arm_one() {
        let mut ctx = ctx_with(0, 4, 1);
        let mut caster = actor(400);
        let out = StealOutcome::FromBag { item: 0x20 };
        let (step, taken) = steal_tick(&mut ctx, &mut caster, 6, Some(out));
        assert_eq!(step, CastTickStep::Busy);
        assert_eq!(taken, None, "arm 0 only stages");
        assert_eq!(caster.staged_anim, 6);
        assert_eq!(caster.restage & 1, 1, "an OR, not a bump");
        assert_eq!(ctx.ctx_0d, 1);
        let (_, taken) = steal_tick(&mut ctx, &mut caster, 6, Some(out));
        assert_eq!(taken, Some(out));
    }

    #[test]
    fn every_arm_maps_to_exactly_one_owner() {
        // The (entry, body) pairs the trampoline table names for the seven
        // modules this file carries.
        use crate::cast_module_ticks::capture_tick_body;
        for (entry, id, body) in [
            (940u32, 0xACu8, GLARE_DIVIDE_BLIND_TICK),
            (940, 0x50, GLARE_DIVIDE_SPLIT_TICK),
            (940, 0xAE, GLARE_DIVIDE_SPLIT_TICK),
            (941, 0x51, STEAL_TICK),
            (941, 0xB9, STEAL_SWEEP_TICK),
            (943, 0x40, CURSE_SINGLE_TICK),
            (943, 0xB5, CURSE_MP_DRAIN_TICK),
            (944, 0x37, GUILTY_CROSS_TICK),
            (944, 0x53, GUILTY_CROSS_CURSE_TICK),
            (950, 0x5A, ROLLING_FLARE_TICK),
            (950, 0xAB, ROLLING_FLARE_SWEEP_TICK),
            (956, 0x71, WATER_HAZARD_TICK),
            (962, 0xA2, BLADE_BREATH_A_TICK),
            (962, 0xA3, BLADE_BREATH_B_TICK),
            (962, 0xA4, BLADE_BREATH_C_TICK),
        ] {
            assert_eq!(
                capture_tick_body(entry, id),
                Some(body),
                "PROT {entry} id {id:#04X}"
            );
        }
    }
}
