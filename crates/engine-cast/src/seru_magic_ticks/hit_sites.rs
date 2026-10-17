//! Shared pieces of the `0x81..=0x86` bodies: the seat triple, the phase-chain runner, the damage-site table, the clamp shapes and the reaction stagers.

use super::*;

// ---------------------------------------------------------------------------
// Shared shape
// ---------------------------------------------------------------------------

/// The three seats every body in this set resolves in its prologue.
///
/// Retail re-derives each from `ctx` on every use; the port hands them in as
/// indices into one seat row so a sweep and a single-target write cannot
/// alias two copies of the same actor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SeruSeats {
    /// `actor_table[ctx + 0x13]`.
    pub caster: u8,
    /// `actor_table[caster + 0x1DD]`. For PROT 0905 (Vera) this is an **ally**
    /// seat - the spell's record class is `0x03`, the ally-side one.
    pub victim: u8,
    /// `actor_table[7]`, reached as `lw rX, 0x1C(0x801C9370)`.
    pub summon: u8,
}

/// Highest phase arm each body's `beq` chain names, keyed by PROT entry.
///
/// A phase above this (and not [`CHOREOGRAPHY_DONE_PHASE`]) falls through to
/// the routine's epilogue.
pub const SERU_TICK_LAST_ARM: [(u32, u8); 6] = [
    (903, 12),
    (904, 14),
    (905, 10),
    (906, 14),
    (907, 15),
    (908, 12),
];

/// The last phase arm PROT `prot_entry`'s chain names.
pub fn seru_last_arm(prot_entry: u32) -> Option<u8> {
    SERU_TICK_LAST_ARM
        .iter()
        .find(|(e, _)| *e == prot_entry)
        .map(|(_, a)| *a)
}

/// Run one arm of a `beq`-chain body: `Advance` walks `ctx+0x279` one step,
/// `Hold` leaves it where it is, `Finish` is the `0xFF` arm's `return 0`.
pub(super) fn run_chain(
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
// Damage sites
// ---------------------------------------------------------------------------

/// Which clamp a site's `a0`-baked hit takes on its way onto `+0x14C`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeruClamp {
    /// `sltu hp, dmg` - unsigned against live HP. Kill-capable, and a
    /// negative wrapper return kills outright
    /// ([`apply_hit_floor_zero`]).
    HpUnsigned,
    /// `addiu cap, hp, -1; sltu cap, dmg` - unsigned against `HP - 1`.
    /// Cannot kill, and a negative return still clamps rather than heals
    /// ([`apply_hit_cap_hp_minus_one`]).
    HpMinusOneUnsigned,
}

/// One `jal` into a damage wrapper inside a player-Seru tick body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SeruHitSite {
    /// Extraction PROT entry of the owning module.
    pub prot_entry: u32,
    /// Retail VA of the routine the site lives in.
    pub routine: u32,
    /// Retail VA of the `jal` itself.
    pub site: u32,
    /// The `a0` immediate the site bakes, as the one-element slice
    /// [`CastDamageShape`] takes.
    pub powers: &'static [u16],
    /// The wrapper the site calls.
    pub wrapper: CastWrapper,
    /// Numerator of the scale applied to the wrapper's return *before* the
    /// clamp (PROT 0908's three sites each scale differently; everyone else
    /// is `1/1`).
    pub scale_num: u32,
    /// Denominator of that scale.
    pub scale_den: u32,
    /// The clamp the site takes.
    pub clamp: SeruClamp,
}

/// Every damage-wrapper call site in PROT 0903..0908, in image and then
/// address order.
///
/// PROT 0905 (Vera) and PROT 0907 (Nighto) hold none - Vera restores HP and
/// Nighto zeroes it outright, neither through a wrapper.
///
/// Each `a0` here is read off the instruction that sets it, which for PROT
/// 0904 and PROT 0906 is a **branch delay slot** (`0x801F7D2C` and
/// `0x801F7364` respectively): a backward scan from the `jal` that stops at
/// the preceding branch loses the constant entirely.
pub const SERU_HIT_SITES: [SeruHitSite; 6] = [
    // PROT 0903, the single-target hit in arm 11.
    SeruHitSite {
        prot_entry: 903,
        routine: 0x801F_69D8,
        site: 0x801F_74AC,
        powers: &[0x12],
        wrapper: CastWrapper::SharedSummon,
        scale_num: 1,
        scale_den: 1,
        clamp: SeruClamp::HpUnsigned,
    },
    // PROT 0904, the expanding-ring sweep in arm 12.
    SeruHitSite {
        prot_entry: 904,
        routine: 0x801F_69D8,
        site: 0x801F_7D38,
        powers: &[0x11],
        wrapper: CastWrapper::SharedSummon,
        scale_num: 1,
        scale_den: 1,
        clamp: SeruClamp::HpUnsigned,
    },
    // PROT 0906, the monster-row sweep in arm 13. Its clamp is `sltu a0, s1`
    // against live HP (`0x801F739C`) - kill-capable, like 0903's and 0904's.
    SeruHitSite {
        prot_entry: 906,
        routine: 0x801F_69F4,
        site: 0x801F_7370,
        powers: &[0x12],
        wrapper: CastWrapper::SharedSummon,
        scale_num: 1,
        scale_den: 1,
        clamp: SeruClamp::HpUnsigned,
    },
    // PROT 0908's three sites. The first is the quarter-strength opener in
    // arm 8 and the only never-kill site in this set.
    SeruHitSite {
        prot_entry: 908,
        routine: 0x801F_69D8,
        site: 0x801F_76CC,
        powers: &[0x12],
        wrapper: CastWrapper::SharedSummon,
        scale_num: 1,
        scale_den: 4,
        clamp: SeruClamp::HpMinusOneUnsigned,
    },
    SeruHitSite {
        prot_entry: 908,
        routine: 0x801F_69D8,
        site: 0x801F_7C14,
        powers: &[0x10],
        wrapper: CastWrapper::SharedSummon,
        scale_num: 3,
        scale_den: 4,
        clamp: SeruClamp::HpUnsigned,
    },
    SeruHitSite {
        prot_entry: 908,
        routine: 0x801F_69D8,
        site: 0x801F_7DF8,
        powers: &[0x12],
        wrapper: CastWrapper::SharedSummon,
        scale_num: 2,
        scale_den: 3,
        clamp: SeruClamp::HpUnsigned,
    },
];

/// The site at retail VA `site`, if this set holds it.
pub fn seru_hit_site(site: u32) -> Option<&'static SeruHitSite> {
    SERU_HIT_SITES.iter().find(|s| s.site == site)
}

impl SeruHitSite {
    /// The `a0` immediate this site bakes.
    pub fn power(&self) -> u16 {
        self.powers.first().copied().unwrap_or(0)
    }

    /// This site's damage shape in the form [`roll_module_hit`] takes.
    pub fn damage_shape(&self) -> CastDamageShape {
        CastDamageShape {
            prot_entry: self.prot_entry,
            routine: self.routine,
            wrapper: self.wrapper,
            never_kills: matches!(self.clamp, SeruClamp::HpMinusOneUnsigned),
            powers: self.powers,
        }
    }

    /// The wrapper's raw return scaled the way this site scales it.
    ///
    /// Retail's three shapes are all **unsigned**: `srl s0, s0, 2` for the
    /// quarter, `s0*3` then `srl 2` for the three-quarters, and `s0*2` through
    /// the `0xAAAAAAAB` reciprocal for the two-thirds. So a negative wrapper
    /// return is scaled as a huge unsigned number here too, and only the
    /// clamp brings it back.
    pub fn scale(&self, roll: i32) -> u32 {
        (roll as u32).wrapping_mul(self.scale_num) / self.scale_den
    }

    /// Roll, scale and apply one hit, returning the damage that landed.
    pub fn apply(&self, victim: &mut CastActorState, roll: i32) -> u32 {
        let dmg = self.scale(roll);
        match self.clamp {
            SeruClamp::HpUnsigned => apply_hit_floor_zero(victim, dmg as i32),
            SeruClamp::HpMinusOneUnsigned => apply_hit_cap_hp_minus_one(victim, dmg),
        }
    }
}

/// Apply shape **C** - the unsigned `HP - 1` clamp (PROT 0908's `0x801F76CC`).
///
/// ```text
/// v0 = victim[+0x14C]
/// v1 = v0 - 1
/// sltu v0, v1, dmg        ; UNSIGNED, against HP - 1
/// if v0 { dmg = v1 }
/// victim[+0x10]  += dmg
/// victim[+0x14C] -= dmg
/// ```
///
/// The cap is `HP - 1`, so a live victim is left at 1 HP at worst. The
/// comparison being unsigned is what separates this from
/// [`crate::cast_module_ticks::apply_hit_floor_one`]: there a negative roll
/// passes the signed compare and *heals*, here it compares above any cap and
/// takes the victim to 1 HP.
///
/// Returns the damage actually applied.
pub fn apply_hit_cap_hp_minus_one(victim: &mut CastActorState, dmg: u32) -> u32 {
    let cap = u32::from(victim.hp).wrapping_sub(1);
    let dmg = if cap < dmg { cap } else { dmg };
    victim.hp_bar_delta = victim.hp_bar_delta.wrapping_add(dmg as i32);
    victim.hp = u32::from(victim.hp).wrapping_sub(dmg) as u16;
    dmg
}

/// Roll one hit off a site's baked power through its wrapper.
///
/// A convenience over [`roll_module_hit`] so a host does not have to build a
/// [`CastDamageShape`] per site. The returned value is the wrapper's raw
/// signed net damage, **before** [`SeruHitSite::scale`] and before the clamp.
pub fn roll_seru_hit(
    site: &SeruHitSite,
    attacker: &WrapperAttacker,
    defender: &WrapperDefender,
    element_affinity_pct: u8,
    rng: [u16; 3],
    bonus_rng: impl FnOnce() -> u16,
) -> i32 {
    roll_module_hit(
        &site.damage_shape(),
        0,
        attacker,
        defender,
        element_affinity_pct,
        rng,
        bonus_rng,
    )
}

// ---------------------------------------------------------------------------
// The reaction stage, in its three spellings
// ---------------------------------------------------------------------------

/// The `+0x1DC` **bit-set** reaction stage (PROT 0903 `0x801F7508`, PROT 0904
/// `0x801F7DD8`).
///
/// ```text
/// if (victim[+0x14C] == 0) || (victim[+0x1F2] != 0)
///     victim[+0x1DA] = victim[+0x1F1]
/// else
///     victim[+0x1DA] = victim[+0x1EF] ? victim[+0x1EF] : victim[+0x1F0]
///     victim[+0x1DC] |= 4
/// victim[+0x1DC] |= 1
/// ```
///
/// `+0x1DC` is a **bitfield** at these sites, not the counter
/// [`crate::cast_module_ticks::stage_clip`] increments - the two sites here
/// `ori` bits `0x4` and `0x1` into it.
pub fn stage_reaction_bits(victim: &mut CastActorState) {
    if victim.hp == 0 || victim.reaction_gate != 0 {
        victim.staged_anim = victim.knockdown_anim;
    } else {
        victim.staged_anim = if victim.reaction_alt != 0 {
            victim.reaction_alt
        } else {
            victim.reaction_alt2
        };
        victim.restage |= 0x4;
    }
    victim.restage |= 0x1;
}

/// The `+0x1DC` **assignment** reaction stage (PROT 0906 `0x801F73D4`, PROT
/// 0908 `0x801F7D04`).
///
/// Same fork, but the two arms store `1` and `5` outright rather than OR-ing
/// `0x1` and `0x4|0x1` in. The resulting value is the same; the instruction
/// shape is not, and only one of the two clears bits an earlier stage set.
pub fn stage_reaction_assign(victim: &mut CastActorState) {
    if victim.hp == 0 || victim.reaction_gate != 0 {
        victim.staged_anim = victim.knockdown_anim;
        victim.restage = 1;
    } else {
        victim.staged_anim = if victim.reaction_alt != 0 {
            victim.reaction_alt
        } else {
            victim.reaction_alt2
        };
        victim.restage = 5;
    }
}

/// The reaction stage with **no** knockdown fork (PROT 0908 `0x801F77B4`).
///
/// The site it belongs to cannot kill, so there is no `+0x1F1` branch and no
/// `+0x1F2` gate at all - the alt pair is staged unconditionally and `+0x1DC`
/// is incremented rather than set.
pub fn stage_reaction_alt_only(victim: &mut CastActorState) {
    victim.staged_anim = if victim.reaction_alt != 0 {
        victim.reaction_alt
    } else {
        victim.reaction_alt2
    };
    victim.restage = victim.restage.wrapping_add(1);
}

/// `+0x0C` - the root-speed word every landed hit in this set writes
/// (`li v0, 0x1000; sw v0, 0xc(victim)`).
pub const SERU_HIT_ROOT_SPEED: i32 = 0x1000;

/// One past the last monster seat both row sweeps in this set walk
/// (`sltiu rX, 0x7` at `0x801F7E84` in PROT 0904, `0x801F74A8` in PROT 0906
/// and `0x801F7FE4` in PROT 0908) - a hard-coded bound, not `ctx[+1]`.
pub const MONSTER_ROW_END: u8 = 7;
