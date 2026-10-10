//! The extended actor mirror, the per-`(entry, body)` damage-shape table, the hit / target-seat kernels and the two arm runners every body in this module goes through.

use super::*;

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
        return (0..ctx.party_count).collect();
    }
    if target_code < 7 {
        return vec![target_code];
    }
    let first = FIRST_MONSTER_SEAT;
    (first..first.saturating_add(ctx.monster_count)).collect()
}

/// The shared tail of the ten table-dispatched bodies: an arm that ran to
/// completion falls into `lbu v0, 0(sX); addiu v0, v0, 1; sb v0, 0(sX)`.
pub(super) fn run_arm_tick(
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
pub(super) fn run_chain_tick(
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
