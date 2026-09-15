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

use crate::battle_damage_wrappers::{WrapperAttacker, WrapperDefender};
use crate::cast_module_ticks::{
    ANIM_RATE_NORMAL, CHOREOGRAPHY_DONE_PHASE, CastActorState, CastArmStep, CastDamageShape,
    CastModuleCtx, CastTickStep, CastWrapper, FIRST_MONSTER_SEAT, FLAG_NON_TARGETABLE, SweepHit,
    TARGET_CODE_ENEMY_ROW, advance_phase, apply_hit_floor_zero, roll_module_hit,
};

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
fn run_chain(
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

// ---------------------------------------------------------------------------
// PROT 0903 - Gimard (spell id 0x81)
// ---------------------------------------------------------------------------

/// PROT 0903's tick entry, and the load base four of these six sit at.
pub const GIMARD_TICK: u32 = 0x801F_69D8;
/// The phase arm PROT 0903 raises the summon seat's render flag in
/// (`li v0,0x3; sb v0,0x21c(s3)` at `0x801F7310`).
pub const GIMARD_POSE_ARM: u8 = 9;
/// The phase arm PROT 0903 bumps the summon seat's clip in
/// (`0x801F7360..0x801F7374`).
pub const GIMARD_STAGE_ARM: u8 = 10;
/// The phase arm PROT 0903's single damage site fires in.
pub const GIMARD_HIT_ARM: u8 = 11;
/// The phase arm PROT 0903 latches `0xFF` from (`sb v0,0x0(s6)` at
/// `0x801F7698`).
pub const GIMARD_SETTLE_ARM: u8 = 12;
/// The render flag PROT 0903 poses the summon seat at in arm 9.
pub const GIMARD_POSE_RENDER_FLAG: u8 = 3;
/// The render flag PROT 0903 drops the summon seat to once the hit has
/// landed (`li v0,0x2; sb v0,0x21c(s3)` at `0x801F75E4`).
pub const GIMARD_DONE_RENDER_FLAG: u8 = 2;
/// The animation rate PROT 0903's settle arm writes onto the victim.
///
/// Retail's source is the scratchpad frame-delta byte `0x1F80037D` shifted
/// right once (`srl v0,v0,0x1; sb v0,0x21d(s2)` at `0x801F76D0`), which at
/// the normal rate is [`ANIM_RATE_NORMAL`] halved.
pub const GIMARD_SETTLE_ANIM_RATE: u8 = ANIM_RATE_NORMAL >> 1;

/// PROT 0903 (Gimard, spell id `0x81`) tick body - 3396 bytes, 849
/// instructions.
///
/// Thirteen phase arms `0..=12` plus `0xFF`, through the `beq`/`slti` chain
/// at `0x801F6A74..0x801F6B18`. Arm targets `0x801F6B20`, `6BA0`, `6C8C`,
/// `6D2C`, `6E70`, `6F30`, `704C`, `71AC`, `7238`, `72C8`, `7328`, `73A4`,
/// `7618`, and `0x801F76DC` for `0xFF`. Four advance sites: the shared tails
/// `0x801F7224` and `0x801F7390`, arm 11's own `0x801F7478`, and the `0xFF`
/// latch `0x801F7698`.
///
/// Simulation state, by arm:
///
/// * arm `0` - `ctx+0x278 = 0` (`0x801F6B8C`), `ctx+0x243 = 1`;
/// * arm `9` - the summon seat's render flag `= 3` (`0x801F7310`);
/// * arm `10` - the summon seat's `+0x1DA` and `+0x1DC` each `+= 1`
///   (`0x801F7360..0x801F7374`);
/// * arm `11` - advances **first** (`0x801F7478`), then, unless the victim
///   carries [`FLAG_NON_TARGETABLE`], `FUN_801DD0AC(0x12, 7, victim_seat)` at
///   `0x801F74AC`, the unsigned clamp at `0x801F74D8`, `+0x10 +=`,
///   `+0x14C -=`, [`stage_reaction_bits`], `+0x0C = 0x1000` and the victim's
///   render flag `= 0`. Then unconditionally the summon seat's render flag
///   `= 2`, its `+0x1DD = 0`, and the caster's render flag `= 0`;
/// * arm `12` - the victim's animation rate, and `0xFF` once the victim has
///   settled;
/// * arm `0xFF` - `return 0`.
///
/// Not ported: the packet arms (1..8 are almost entirely
/// `FUN_801D829C` / `FUN_80021B04` emission), the `FUN_8004E2F0` animation
/// poll arm 11 holds on, the camera magnitude `ctx+0x6D0` the landed hit
/// scales by `3/2`, and `ctx+0x243`.
///
/// Wired: `World::run_cast_module_code`.
///
/// PORT: FUN_801F69D8 (PROT 0903; phase chain + the single-target hit; packet and camera arms unported)
pub fn gimard_tick(
    ctx: &mut CastModuleCtx,
    seats: &mut [CastActorState],
    who: SeruSeats,
    hit: Option<i32>,
) -> (CastTickStep, Vec<SweepHit>) {
    let mut hits = Vec::new();
    let site = SERU_HIT_SITES[0];
    let step = run_chain(ctx, |c| match c.phase {
        0 => {
            c.ctx_278 = 0;
            CastArmStep::Advance
        }
        GIMARD_POSE_ARM => {
            if let Some(s) = seats.get_mut(who.summon as usize) {
                s.render_flag = GIMARD_POSE_RENDER_FLAG;
            }
            CastArmStep::Advance
        }
        GIMARD_STAGE_ARM => {
            if let Some(s) = seats.get_mut(who.summon as usize) {
                s.staged_anim = s.staged_anim.wrapping_add(1);
                s.restage = s.restage.wrapping_add(1);
            }
            CastArmStep::Advance
        }
        GIMARD_HIT_ARM => {
            if let Some(v) = seats.get_mut(who.victim as usize)
                && (v.flags & FLAG_NON_TARGETABLE) == 0
                && let Some(roll) = hit
            {
                let applied = site.apply(v, roll);
                stage_reaction_bits(v);
                v.root_speed = SERU_HIT_ROOT_SPEED;
                v.render_flag = 0;
                hits.push(SweepHit {
                    seat: who.victim,
                    applied,
                });
            }
            if let Some(s) = seats.get_mut(who.summon as usize) {
                s.render_flag = GIMARD_DONE_RENDER_FLAG;
                s.target_code = 0;
            }
            if let Some(s) = seats.get_mut(who.caster as usize) {
                s.render_flag = 0;
            }
            CastArmStep::Advance
        }
        GIMARD_SETTLE_ARM => {
            let settled = seats
                .get(who.victim as usize)
                .is_some_and(|v| v.hp == 0 || v.playing_anim == 0);
            if let Some(v) = seats.get_mut(who.victim as usize) {
                v.anim_rate = GIMARD_SETTLE_ANIM_RATE;
            }
            if settled {
                c.phase = CHOREOGRAPHY_DONE_PHASE;
                return CastArmStep::Hold;
            }
            CastArmStep::Hold
        }
        CHOREOGRAPHY_DONE_PHASE => CastArmStep::Finish,
        p if p <= GIMARD_SETTLE_ARM => CastArmStep::Advance,
        _ => CastArmStep::Finish,
    });
    (step, hits)
}

// ---------------------------------------------------------------------------
// PROT 0904 - Theeder (spell id 0x82)
// ---------------------------------------------------------------------------

/// PROT 0904's tick entry.
pub const THEEDER_TICK: u32 = 0x801F_69D8;
/// The arm PROT 0904 raises the summon seat in (`li v0,0x4` at `0x801F6ECC`).
pub const THEEDER_RISE_ARM: u8 = 4;
/// The arm PROT 0904 clears the summon seat's render flag in
/// (`0x801F70A4`).
pub const THEEDER_SHOW_ARM: u8 = 5;
/// The arm PROT 0904 bumps the summon seat's clip in (`0x801F71A8`).
pub const THEEDER_STAGE_ARM: u8 = 6;
/// The arm PROT 0904 retargets the caster and the summon seat onto the enemy
/// row in (`li v0,0x9; sb v0,0x1dd(..)` at `0x801F79A4`).
pub const THEEDER_RETARGET_ARM: u8 = 11;
/// The arm PROT 0904's expanding-ring sweep fires in.
pub const THEEDER_SWEEP_ARM: u8 = 12;
/// The arm PROT 0904 settles from.
pub const THEEDER_SETTLE_ARM: u8 = 14;
/// The render flag PROT 0904 raises the summon seat to in arm 4.
pub const THEEDER_RISE_RENDER_FLAG: u8 = 4;

/// PROT 0904 (Theeder, spell id `0x82`) tick body - 6020 bytes, 1505
/// instructions.
///
/// Fifteen phase arms `0..=14` plus `0xFF`, through the chain at
/// `0x801F6A7C..0x801F6B2C`. Its prologue guards the victim lookup
/// (`sltiu v0, s6, 0x8` at `0x801F6A30`): a `+0x1DD` of `8` or more leaves the
/// victim register untouched, which is why the ring sweep addresses seats
/// through the actor table rather than through that register.
///
/// Arm 12 is an **expanding-ring sweep**, not a single hit. `ctx+0x6D8` grows
/// by the frame delta times `8` each tick (`0x801F7AE4`), and the arm walks
/// `actor_table[3 ..= 6]` - a hard-coded `sltiu s3, 0x7`, not `ctx[+1]` -
/// hitting each seat that is alive, is not already reacting (`+0x1D9 == 0`),
/// is inside a `+-0x30` cone of the ring's direction (`0x801F7D0C`), and is
/// not [`FLAG_NON_TARGETABLE`]. Per hit: `FUN_801DD0AC(0x11, 7, seat)` -
/// the `0x11` is in the `bne` delay slot at `0x801F7D2C` - the unsigned clamp
/// at `0x801F7D6C`, `+0x10 +=`, `+0x14C -=`, `+0x04 = 0x3FF0000`, render flag
/// `= 0`, `+0x21F = 2` and [`stage_reaction_bits`]. The arm advances only
/// once `ctx+0x6D8` has passed `0x1000`, so the sweep repeats for as long as
/// the ring is growing and the `+0x1D9` guard is what stops a seat being hit
/// twice.
///
/// Other simulation state: `ctx+0x278 = 0` in arm 0 (`0x801F6C10`), the
/// summon seat's render flag `= 4` in arm 4 and `= 0` in arm 5, its clip pair
/// `+= 1` in arm 6, and in arm 11 both the caster's and the summon seat's
/// `+0x1DD = ` [`TARGET_CODE_ENEMY_ROW`] with the original victim index
/// latched into the module word `0x801F9200`. Arm `0xFF` restores the caster's
/// `+0x1DD` from that word (`0x801F8124`).
///
/// Not ported: the packet and camera arms, the ring geometry (the host
/// supplies which seats are in the cone), and the arm-14 settle poll.
///
/// Wired: `World::run_cast_module_code`.
///
/// PORT: FUN_801F69D8 (PROT 0904; phase chain + the expanding-ring sweep; packet and camera arms unported)
pub fn theeder_tick(
    ctx: &mut CastModuleCtx,
    seats: &mut [CastActorState],
    who: SeruSeats,
    mut rolls: impl FnMut(u8) -> Option<i32>,
) -> (CastTickStep, Vec<SweepHit>) {
    let mut hits = Vec::new();
    let site = SERU_HIT_SITES[1];
    let step = run_chain(ctx, |c| match c.phase {
        0 => {
            c.ctx_278 = 0;
            CastArmStep::Advance
        }
        THEEDER_RISE_ARM => {
            if let Some(s) = seats.get_mut(who.summon as usize) {
                s.render_flag = THEEDER_RISE_RENDER_FLAG;
            }
            CastArmStep::Advance
        }
        THEEDER_SHOW_ARM => {
            if let Some(s) = seats.get_mut(who.summon as usize) {
                s.render_flag = 0;
            }
            CastArmStep::Advance
        }
        THEEDER_STAGE_ARM => {
            if let Some(s) = seats.get_mut(who.summon as usize) {
                s.staged_anim = s.staged_anim.wrapping_add(1);
                s.restage = s.restage.wrapping_add(1);
            }
            CastArmStep::Advance
        }
        THEEDER_RETARGET_ARM => {
            for slot in [who.caster, who.summon] {
                if let Some(s) = seats.get_mut(slot as usize) {
                    s.target_code = TARGET_CODE_ENEMY_ROW;
                }
            }
            CastArmStep::Advance
        }
        THEEDER_SWEEP_ARM => {
            for seat in FIRST_MONSTER_SEAT..MONSTER_ROW_END {
                let Some(v) = seats.get_mut(seat as usize) else {
                    continue;
                };
                if v.hp == 0 || v.playing_anim != 0 || (v.flags & FLAG_NON_TARGETABLE) != 0 {
                    continue;
                }
                let Some(roll) = rolls(seat) else {
                    continue;
                };
                let applied = site.apply(v, roll);
                v.render_flag = 0;
                // The two mirrors the arm stamps beside the HP write
                // (`0x801F7D9C` / `0x801F7DB4`): the presentation word `+0x04`
                // and the render byte `+0x21F`.
                v.present_04 = THEEDER_HIT_PRESENT_WORD;
                v.render_21f = THEEDER_HIT_RENDER_21F;
                stage_reaction_bits(v);
                hits.push(SweepHit { seat, applied });
            }
            CastArmStep::Advance
        }
        CHOREOGRAPHY_DONE_PHASE => CastArmStep::Finish,
        p if p <= THEEDER_SETTLE_ARM => CastArmStep::Advance,
        _ => CastArmStep::Finish,
    });
    (step, hits)
}

/// The `+0x04` mesh-tint word PROT 0904's ring sweep stamps on every seat it
/// hits.
pub const THEEDER_HIT_PRESENT_WORD: u32 = 0x03FF_0000;
/// The `+0x21F` render byte the same hit writes.
pub const THEEDER_HIT_RENDER_21F: u8 = 2;

/// Half-width of PROT 0904's ring-sweep **cone**, in 12-bit angle units
/// (`addiu v0, v0, -0x30` at `0x801F7D0C`).
///
/// A seat is hit when the bearing difference between it and the ring's rim is
/// within this of zero **modulo a turn**: retail subtracts `0x30` from the
/// absolute difference and compares the result **unsigned** against `0xFB1`,
/// so a difference under `0x30` underflows past that bound and a difference at
/// or above `0xFE1` exceeds it. Both ends of the cone are in.
///
/// The host supplies the membership ([`crate::cast_module_ticks`] consumers
/// call `World::seats_in_cone`); this constant is here so the two sides quote
/// one number.
pub const THEEDER_CONE_HALF_WIDTH: u16 = 0x30;

/// One past the last monster seat both row sweeps in this set walk
/// (`sltiu rX, 0x7` at `0x801F7E84` in PROT 0904, `0x801F74A8` in PROT 0906
/// and `0x801F7FE4` in PROT 0908) - a hard-coded bound, not `ctx[+1]`.
pub const MONSTER_ROW_END: u8 = 7;

// ---------------------------------------------------------------------------
// PROT 0905 - Vera (spell id 0x83)
// ---------------------------------------------------------------------------

/// PROT 0905's tick entry.
pub const VERA_TICK: u32 = 0x801F_69D8;
/// The arm PROT 0905 clears the target's render flag in (`0x801F6D98`).
pub const VERA_SHOW_ARM: u8 = 1;
/// The arm PROT 0905 poses the summon seat in (`0x801F7300..0x801F733C`).
pub const VERA_POSE_ARM: u8 = 5;
/// The arm PROT 0905 stages the summon seat's cast clips in
/// (`0x801F78E0`, `0x801F7944`).
pub const VERA_CAST_ARM: u8 = 8;
/// The arm PROT 0905 restores HP and cures status in.
pub const VERA_RESTORE_ARM: u8 = 9;
/// The arm PROT 0905 fades out and latches `0xFF` from (`0x801F8034`).
pub const VERA_FADE_ARM: u8 = 10;
/// The base of PROT 0905's restore (`addiu v0, v0, 0xe0` at `0x801F7C60`).
pub const VERA_HEAL_BASE: u32 = 0xE0;
/// The per-magic-level step of PROT 0905's restore
/// (`sll v0, v0, 0x5` at `0x801F7C5C`).
pub const VERA_HEAL_PER_LEVEL: u32 = 0x20;
/// The magic level PROT 0905's status cure unlocks at
/// (`sltiu v0, v0, 0x3; bne` at `0x801F7D5C`).
pub const VERA_CURE_MIN_LEVEL: u8 = 3;
/// The `+0x16E` keep-masks PROT 0905's four cure tiers `and` with
/// (`0x801F7DB4`, `0x801F7E30`, `0x801F7EAC`, `0x801F7F28`).
///
/// The tier is chosen by the battle-overlay word `0x801F6960`, which sits
/// below the slot-B base and so is not the module's own data; a tier outside
/// `1..=4` cures nothing.
pub const VERA_CURE_MASKS: [u16; 4] = [0xFFFC, 0xFF84, 0xFB84, 0xFB84];
/// The `+0x16E` bits whose presence makes PROT 0905's tiers `2..=4` play the
/// cure cue and light the target's `+0x220..+0x223` markers
/// (`andi v0, v0, 0x3c`).
pub const VERA_CURE_CUE_BITS: u16 = 0x003C;

/// What PROT 0905's restore arm needs that the actor mirror does not carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VeraRestore {
    /// `record[+0x729 + slot]` - the caster's magic level for this spell,
    /// found by scanning the 32 learned-spell ids at `record[+0x705 + i]` for
    /// the caster's queued action (`0x801F7C28..0x801F7C50`). The record base
    /// is `0x80084140 + (0x8007BD10[caster_seat] - 1) * 0x414`.
    pub magic_level: u8,
    /// `target[+0x14E]` - max HP, which the clamp needs and the mirror has no
    /// field for.
    pub max_hp: u16,
    /// The cure tier the battle-overlay word `0x801F6960` selects, `1..=4`.
    pub cure_tier: u8,
}

/// What PROT 0905's restore arm did this frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct VeraOutcome {
    /// HP actually restored after the missing-HP clamp.
    pub restored: u16,
    /// `true` when the tier was high enough to reach the `+0x16E` mask.
    pub cured: bool,
    /// `true` when the target was carrying one of [`VERA_CURE_CUE_BITS`], so
    /// retail played the cure cue and set `+0x220..+0x223`.
    pub cure_cue: bool,
}

/// PROT 0905's restore amount, clamped exactly the way the bytes clamp it.
///
/// ```text
/// want    = magic_level * 0x20 + 0xE0
/// missing = target[+0x14E] - target[+0x14C]
/// if (missing < want)            ; SIGNED slt at 0x801F7C6C
///     if ((missing << 16) == 0) return 0   ; nothing missing -> no restore
///     want = missing
/// ```
pub fn vera_heal_amount(magic_level: u8, hp: u16, max_hp: u16) -> u16 {
    let want = u32::from(magic_level) * VERA_HEAL_PER_LEVEL + VERA_HEAL_BASE;
    let missing = i32::from(max_hp).wrapping_sub(i32::from(hp));
    if missing < want as i32 {
        if missing as u16 == 0 {
            return 0;
        }
        return missing as u16;
    }
    want as u16
}

/// PROT 0905 (Vera, spell id `0x83`) tick body - 5792 bytes, 1448
/// instructions.
///
/// Eleven phase arms `0..=10` plus `0xFF`, through the chain at
/// `0x801F6A70..0x801F6B10`. It holds **no** damage-wrapper call at all.
///
/// That is the module `static-overlays.toml` still labels
/// `summon_stager_x83`; spell `0x83` is **Vera**, whose SCUS record class is
/// the ally-side `0x03`, and arm 9 is a **restore**, not a hit:
///
/// ```text
/// level = record[+0x729 + slot]              ; slot found by scanning +0x705
/// heal  = level * 0x20 + 0xE0                ; clamped to +0x14E - +0x14C
/// if (target[+0x14C] != 0 && !(target[+0x16E] & 4))
///     target[+0x14C] += heal                 ; 0x801F7CFC
///     target[+0x10]   = -heal                ; 0x801F7D0C, an assignment
/// if (level >= 3)  target[+0x16E] &= VERA_CURE_MASKS[tier - 1]
/// ```
///
/// `+0x10` is **stored**, not accumulated, and it is stored negative - the
/// one site in this set that does not `addu` into the pending HP-bar delta.
///
/// Other simulation state: the target's render flag `= 0` in arm 1, the
/// summon seat's `+0x0C = 0x1000` / animation rate `= 8` / render flag `= 0`
/// / clip pair `+= 1` in arm 5, `+0x1DA = 2` then `= 3` with a `+0x1DC` bump
/// and render flag `= 9` in arm 8, and the fade countdown in arm 10 that
/// latches `0xFF`. Arm `0xFF` writes no actor state; it only returns `0`.
///
/// Not ported: the packet arms, the cure cue's `+0x220..+0x223` markers and
/// the tier-4 `+0x170` doubling (neither field is in the mirror), and the
/// battle-overlay tier selector `0x801F6960`.
///
/// Wired: `World::run_cast_module_code`.
///
/// PORT: FUN_801F69D8 (PROT 0905; phase chain + the HP restore and status cure; packet arms unported)
pub fn vera_tick(
    ctx: &mut CastModuleCtx,
    seats: &mut [CastActorState],
    who: SeruSeats,
    restore: Option<VeraRestore>,
) -> (CastTickStep, Option<VeraOutcome>) {
    let mut outcome = None;
    let step = run_chain(ctx, |c| match c.phase {
        VERA_SHOW_ARM => {
            if let Some(t) = seats.get_mut(who.victim as usize) {
                t.render_flag = 0;
            }
            CastArmStep::Advance
        }
        VERA_POSE_ARM => {
            if let Some(s) = seats.get_mut(who.summon as usize) {
                s.root_speed = SERU_HIT_ROOT_SPEED;
                s.anim_rate = ANIM_RATE_NORMAL;
                s.render_flag = 0;
                s.staged_anim = s.staged_anim.wrapping_add(1);
                s.restage = s.restage.wrapping_add(1);
            }
            CastArmStep::Advance
        }
        VERA_CAST_ARM => {
            if let Some(s) = seats.get_mut(who.summon as usize) {
                s.staged_anim = 3;
                s.anim_rate = ANIM_RATE_NORMAL;
                s.restage = s.restage.wrapping_add(1);
                s.render_flag = 9;
            }
            if let Some(t) = seats.get_mut(who.victim as usize) {
                t.render_flag = 0;
            }
            CastArmStep::Advance
        }
        VERA_RESTORE_ARM => {
            if let Some(r) = restore
                && let Some(t) = seats.get_mut(who.victim as usize)
            {
                let mut out = VeraOutcome::default();
                let heal = vera_heal_amount(r.magic_level, t.hp, r.max_hp);
                if t.hp != 0 && (t.flags & FLAG_NON_TARGETABLE) == 0 {
                    t.hp = t.hp.wrapping_add(heal);
                    // Retail stores, it does not accumulate: `sw -heal`.
                    t.hp_bar_delta = -i32::from(heal as i16);
                    out.restored = heal;
                }
                if r.magic_level >= VERA_CURE_MIN_LEVEL
                    && let Some(&mask) = VERA_CURE_MASKS.get(r.cure_tier.wrapping_sub(1) as usize)
                {
                    out.cure_cue = r.cure_tier >= 2 && (t.flags & VERA_CURE_CUE_BITS) != 0;
                    t.flags &= mask;
                    out.cured = true;
                }
                outcome = Some(out);
            }
            CastArmStep::Advance
        }
        CHOREOGRAPHY_DONE_PHASE => CastArmStep::Finish,
        p if p <= VERA_FADE_ARM => CastArmStep::Advance,
        _ => CastArmStep::Finish,
    });
    (step, outcome)
}

// ---------------------------------------------------------------------------
// PROT 0906 - Gizam (spell id 0x84)
// ---------------------------------------------------------------------------

/// PROT 0906's tick entry - `0x801F69F4`, **not** the load base. The base
/// holds the module's head table; the `0x801CF4EC` stub `0x801F1F6C` jumps
/// here.
pub const GIZAM_TICK: u32 = 0x801F_69F4;
/// The phase PROT 0906's chain names no arm for. See [`gizam_tick`].
pub const GIZAM_MISSING_ARM: u8 = 5;
/// The arm PROT 0906 hides the summon seat in (`li v0,0xff` at `0x801F6D30`).
pub const GIZAM_HIDE_ARM: u8 = 4;
/// The arm PROT 0906 bumps the summon seat's clip in (`0x801F6F4C`).
pub const GIZAM_STAGE_ARM: u8 = 8;
/// The arm PROT 0906's monster-row sweep fires in.
pub const GIZAM_SWEEP_ARM: u8 = 13;
/// The last arm PROT 0906's chain names.
pub const GIZAM_LAST_ARM: u8 = 14;
/// The render flag PROT 0906 hides the summon seat behind.
pub const GIZAM_HIDDEN_RENDER_FLAG: u8 = 0xFF;
/// The `+0x16E` bit PROT 0906's sweep sets on every seat it hits
/// (`ori v0, v0, 0x1; sh v0, 0x16e(v1)` at `0x801F749C`).
pub const GIZAM_STATUS_BIT: u16 = 0x0001;

/// PROT 0906 (Gizam, spell id `0x84`) tick body - 3404 bytes, 851
/// instructions.
///
/// Fifteen phase arms through the chain at `0x801F6AA0..0x801F6B50`, and the
/// chain has a **hole**: for `v1` in `4..=7` it tests `6`, then `>= 7`, then
/// `4`, and falls to the epilogue otherwise - so phase `5` reaches no arm.
/// Arm 4 nevertheless advances into it (`0x801F7194`), and arm 2 can skip it
/// (`ctx+0x6D8 != 0 ? phase += 1 : phase += 2` at `0x801F6CC0`), with arm 3
/// setting `phase = 4` outright. Retail parks at phase 5 returning busy; the
/// escape is outside this routine. **The port advances through phase 5** as a
/// no-op so the choreography reaches arm 13 - a deliberate divergence, taken
/// because holding there would strand the cast.
///
/// This is also the module whose `0x801F6734` spawn stager is already ported
/// as `cast_module_ticks::gizam_stager` (`FUN_801F7740`). That is a different
/// routine in the same image; the arm here is the tick.
///
/// Arm 13 sweeps `actor_table[3 ..= 6]`, skipping a dead seat and a
/// [`FLAG_NON_TARGETABLE`] one. Per hit: `FUN_801DD0AC(0x12, 7, seat)` - the
/// `0x12` is in the `bne` delay slot at `0x801F7364` - the unsigned clamp
/// against `HP - 1`, `+0x10 +=`, `+0x14C -=`,
/// [`stage_reaction_assign`], `+0x04 = 0x3FF0000`, `+0x0C = 0x1000`, render
/// flag `= 0`, the facing write, and `+0x16E |=` [`GIZAM_STATUS_BIT`]. The
/// arm fires only once its `ctx+0x6D8` countdown goes negative, then hides
/// the summon seat and advances.
///
/// Other simulation state: `ctx+0x278 = 0` in arm 0 (`0x801F6BF0`), the
/// summon seat's render flag `= 0xFF` in arm 4, its clip pair `+= 1` in arm 8
/// with the first monster seat's render flag cleared, and in arm `0xFF` its
/// `+0x1DA = 0` plus a render-flag clear over the three party seats.
///
/// Not ported: the packet and camera arms, the countdowns, and the
/// `+0x21B` / `+0x176` / `+0x04` writes outside the mirror.
///
/// Wired: `World::run_cast_module_code`.
///
/// PORT: FUN_801F69F4 (PROT 0906; phase chain + the monster-row sweep; packet and camera arms unported)
pub fn gizam_tick(
    ctx: &mut CastModuleCtx,
    seats: &mut [CastActorState],
    who: SeruSeats,
    mut rolls: impl FnMut(u8) -> Option<i32>,
) -> (CastTickStep, Vec<SweepHit>) {
    let mut hits = Vec::new();
    let site = SERU_HIT_SITES[2];
    let step = run_chain(ctx, |c| match c.phase {
        0 => {
            c.ctx_278 = 0;
            CastArmStep::Advance
        }
        GIZAM_HIDE_ARM => {
            if let Some(s) = seats.get_mut(who.summon as usize) {
                s.render_flag = GIZAM_HIDDEN_RENDER_FLAG;
            }
            CastArmStep::Advance
        }
        GIZAM_STAGE_ARM => {
            if let Some(s) = seats.get_mut(who.summon as usize) {
                s.staged_anim = s.staged_anim.wrapping_add(1);
                s.restage = s.restage.wrapping_add(1);
            }
            if let Some(v) = seats.get_mut(FIRST_MONSTER_SEAT as usize)
                && v.hp != 0
            {
                v.render_flag = 0;
            }
            CastArmStep::Advance
        }
        GIZAM_SWEEP_ARM => {
            for seat in FIRST_MONSTER_SEAT..MONSTER_ROW_END {
                let Some(v) = seats.get_mut(seat as usize) else {
                    continue;
                };
                if v.hp == 0 || (v.flags & FLAG_NON_TARGETABLE) != 0 {
                    continue;
                }
                let Some(roll) = rolls(seat) else {
                    continue;
                };
                let applied = site.apply(v, roll);
                stage_reaction_assign(v);
                v.root_speed = SERU_HIT_ROOT_SPEED;
                v.render_flag = 0;
                v.flags |= GIZAM_STATUS_BIT;
                hits.push(SweepHit { seat, applied });
            }
            if let Some(s) = seats.get_mut(who.summon as usize) {
                s.render_flag = GIZAM_HIDDEN_RENDER_FLAG;
            }
            CastArmStep::Advance
        }
        CHOREOGRAPHY_DONE_PHASE => {
            if let Some(s) = seats.get_mut(who.summon as usize) {
                s.staged_anim = 0;
            }
            for seat in 0..FIRST_MONSTER_SEAT {
                if let Some(v) = seats.get_mut(seat as usize) {
                    v.render_flag = 0;
                }
            }
            CastArmStep::Finish
        }
        p if p <= GIZAM_LAST_ARM => CastArmStep::Advance,
        _ => CastArmStep::Finish,
    });
    (step, hits)
}

// ---------------------------------------------------------------------------
// PROT 0907 - Nighto (spell id 0x85)
// ---------------------------------------------------------------------------

/// PROT 0907's tick entry - `0x801F69E8`, not the load base.
pub const NIGHTO_TICK: u32 = 0x801F_69E8;
/// The arm PROT 0907 stages the summon seat's first clip in (`0x801F7804`).
pub const NIGHTO_STAGE_ARM: u8 = 7;
/// The arm PROT 0907 bumps that clip in (`0x801F7A44`).
pub const NIGHTO_BUMP_ARM: u8 = 8;
/// The arm PROT 0907 shows the victim in (`0x801F7CFC`).
pub const NIGHTO_SHOW_ARM: u8 = 12;
/// The arm PROT 0907 forks kill-vs-confuse in.
pub const NIGHTO_FORK_ARM: u8 = 13;
/// The arm PROT 0907's kill outcome settles through.
pub const NIGHTO_KILL_SETTLE_ARM: u8 = 14;
/// The arm PROT 0907's confuse outcome lands in.
pub const NIGHTO_CONFUSE_ARM: u8 = 15;
/// The `+0x16E` bits PROT 0907 sets on a confused victim
/// (`ori v0, v0, 0x380` at `0x801F7F50`).
pub const NIGHTO_CONFUSE_BITS: u16 = 0x0380;
/// The render flag / `+0x225` pair PROT 0907 writes on a killed victim
/// (`li v0,0x2` at `0x801F7E54`).
pub const NIGHTO_KILL_RENDER_FLAG: u8 = 2;
/// The render flag PROT 0907 writes on a confused victim
/// (`li v0,0x6` at `0x801F7E18`).
pub const NIGHTO_CONFUSE_RENDER_FLAG: u8 = 6;
/// The animation rate PROT 0907's `0xFF` arm leaves the victim at.
///
/// It is the `li v0, 0x2` in the dispatch chain's **branch delay slot** at
/// `0x801F6B44` - the value is set on the way to the arm, not inside it, so a
/// reader of the arm alone sees `sb v0, 0x21d(s4)` with no source.
pub const NIGHTO_EXIT_ANIM_RATE: u8 = 2;

/// The modulus PROT 0907's arm-0 kill roll takes
/// (`rand() % 8` at `0x801F6B50..0x801F6B7C`, stored to the module word
/// `0x801F8534`). A remainder of `0` is the instant death; anything else is
/// the confuse branch.
pub const NIGHTO_KILL_MODULUS: u32 = 8;
/// The base of PROT 0907's resist modulus (`li v1, 0x13` at `0x801F6C70`);
/// the divisor is `NIGHTO_RESIST_BASE - magic_level`, so a higher magic level
/// narrows the range the throw has to land under.
pub const NIGHTO_RESIST_BASE: u32 = 0x13;
/// PROT 0907's resist threshold (`slti v1, v1, 0x9` at `0x801F6CA8`): a
/// remainder of `9` or more sets the module word `0x801F853C` and the spell
/// does nothing.
pub const NIGHTO_RESIST_THRESHOLD: u32 = 9;
/// The modulus the extra roll only one character index takes
/// (`rand() % 3 == 0` at `0x801F6CF0..0x801F6D28`).
pub const NIGHTO_EXTRA_MODULUS: u32 = 3;
/// The character index (`0x8007BD10[caster_seat]`, 1-based) that takes that
/// extra roll (`li v0, 0x3; bne v1, v0` at `0x801F6CE4`).
pub const NIGHTO_EXTRA_ROLL_CHARACTER: u8 = 3;

/// The inputs PROT 0907's arm 0 draws before the fork ever runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NightoRoll {
    /// `FUN_80056798()` at `0x801F6B50` - `% 8` picks kill over confuse.
    pub kill_roll: u32,
    /// `FUN_80056798()` at `0x801F6C28` - the resist throw.
    pub resist_roll: u32,
    /// `record[+0x729 + slot]`, found the same way
    /// [`VeraRestore::magic_level`] is found.
    pub magic_level: u8,
    /// `ctx[+0x287] != 0 && monster_record[+0x20] != 0`, where the record is
    /// `0x801C9348[victim_seat - 3]`. A monster whose record carries that byte
    /// is immune to the whole spell, and nothing else in the band reads it.
    pub target_immune: bool,
    /// `FUN_80056798()` at `0x801F6CF0`, drawn **only** when the caster's
    /// character index is [`NIGHTO_EXTRA_ROLL_CHARACTER`].
    pub extra_roll: Option<u32>,
}

/// Which way PROT 0907's arm-0 rolls send arm 13.
///
/// Retail's two divides are signed (`div` + `mfhi`), and the SCUS RNG
/// `FUN_80056798` returns a non-negative word, so an unsigned remainder is
/// the same value. The resist divisor is clamped to `1` here because a magic
/// level of `0x13` would divide by zero - retail would take the `break 0x1c00`
/// trap, which no reachable magic level triggers.
pub fn nighto_outcome(roll: &NightoRoll) -> NightoOutcome {
    let mut resisted = if roll.target_immune {
        true
    } else {
        let modulus = NIGHTO_RESIST_BASE
            .saturating_sub(u32::from(roll.magic_level))
            .max(1);
        roll.resist_roll % modulus >= NIGHTO_RESIST_THRESHOLD
    };
    if let Some(extra) = roll.extra_roll
        && extra.is_multiple_of(NIGHTO_EXTRA_MODULUS)
    {
        resisted = true;
    }
    if resisted {
        return NightoOutcome::Resisted;
    }
    if roll.kill_roll.is_multiple_of(NIGHTO_KILL_MODULUS) {
        NightoOutcome::Kill
    } else {
        NightoOutcome::Confuse
    }
}

/// Which way PROT 0907's arm-13 fork went.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NightoOutcome {
    /// The module-local word `0x801F8534` read zero: the victim dies outright.
    Kill,
    /// `0x801F8534` was non-zero and `0x801F853C` zero: the victim is
    /// confused instead, through arm 15.
    Confuse,
    /// `0x801F853C` was non-zero - the spell was resisted and neither branch
    /// writes the victim.
    Resisted,
}

/// PROT 0907 (Nighto, spell id `0x85`) tick body - 5568 bytes, 1392
/// instructions.
///
/// Sixteen phase arms `0..=15` plus `0xFF`, through the chain at
/// `0x801F6A88..0x801F6B48`. It holds **no** damage-wrapper call: the spell's
/// SCUS description ("kill or confuse enemy") is literally what arm 13
/// implements.
///
/// ```text
/// arm 13: if (module[0x801F8534] == 0)                ; the kill branch
///             if (module[0x801F853C] == 0)
///                 victim[+0x14C] = 0                  ; 0x801F7E58
///                 victim[+0x21C] = victim[+0x225] = 2
///                 victim[+0x16E] = 0
///             phase += 1                              ; -> arm 14, settle
///         else if (module[0x801F853C] == 0)
///             victim[+0x21C] = 6
///             phase = 15                              ; -> the confuse arm
/// arm 15: victim[+0x21C] = 0
///         if (module[0x801F853C] == 0)
///             victim[+0x16E] |= 0x380                 ; 0x801F7F50
///         phase = 0xFF
/// ```
///
/// Both module words are written by arm 0, so the outcome is decided the
/// frame the cast starts, not the frame it lands: `0x801F8534 = rand() % 8`
/// at `0x801F6B7C` and `0x801F853C` from the resist throw at
/// `0x801F6C28..0x801F6CC4` - see [`nighto_outcome`], which carries the
/// arithmetic.
///
/// Other simulation state: `ctx+0x278 = 0` in arm 0 (`0x801F7054`), the
/// summon seat's `+0x1DA = 1` in arm 7 and its clip pair `+= 1` in arm 8, the
/// victim's render flag `= 0` and animation rate `= 1` in arm 12, and the
/// victim's animation rate `=` [`NIGHTO_EXIT_ANIM_RATE`] in arm `0xFF`.
///
/// Not ported: the packet and camera arms, the `ctx+0x6D8` countdowns each of
/// arms 13..15 gate on, and the summon seat's `+0x21B` / `+0x176` clears.
///
/// Wired: `World::run_cast_module_code`.
///
/// PORT: FUN_801F69E8 (PROT 0907; phase chain + the instant-death / confuse fork; packet and camera arms unported)
pub fn nighto_tick(
    ctx: &mut CastModuleCtx,
    seats: &mut [CastActorState],
    who: SeruSeats,
    outcome: NightoOutcome,
) -> CastTickStep {
    run_chain(ctx, |c| match c.phase {
        0 => {
            c.ctx_278 = 0;
            CastArmStep::Advance
        }
        NIGHTO_STAGE_ARM => {
            if let Some(s) = seats.get_mut(who.summon as usize) {
                s.staged_anim = 1;
            }
            CastArmStep::Advance
        }
        NIGHTO_BUMP_ARM => {
            if let Some(s) = seats.get_mut(who.summon as usize) {
                s.staged_anim = s.staged_anim.wrapping_add(1);
                s.restage = s.restage.wrapping_add(1);
            }
            CastArmStep::Advance
        }
        NIGHTO_SHOW_ARM => {
            if let Some(v) = seats.get_mut(who.victim as usize) {
                v.render_flag = 0;
                v.anim_rate = 1;
            }
            CastArmStep::Advance
        }
        NIGHTO_FORK_ARM => {
            let Some(v) = seats.get_mut(who.victim as usize) else {
                return CastArmStep::Advance;
            };
            v.anim_rate = 1;
            match outcome {
                NightoOutcome::Kill => {
                    v.hp = 0;
                    // One `li v0, 0x2` feeds both stores at `0x801F7E54`.
                    v.render_flag = NIGHTO_KILL_RENDER_FLAG;
                    v.render_225 = NIGHTO_KILL_RENDER_FLAG;
                    v.flags = 0;
                    CastArmStep::Advance
                }
                NightoOutcome::Confuse => {
                    v.render_flag = NIGHTO_CONFUSE_RENDER_FLAG;
                    c.phase = NIGHTO_CONFUSE_ARM;
                    CastArmStep::Hold
                }
                NightoOutcome::Resisted => CastArmStep::Advance,
            }
        }
        NIGHTO_KILL_SETTLE_ARM => {
            if let Some(v) = seats.get_mut(who.victim as usize) {
                v.anim_rate = 1;
            }
            c.phase = CHOREOGRAPHY_DONE_PHASE;
            CastArmStep::Hold
        }
        NIGHTO_CONFUSE_ARM => {
            if let Some(v) = seats.get_mut(who.victim as usize) {
                v.anim_rate = 1;
                v.render_flag = 0;
                if outcome == NightoOutcome::Confuse {
                    v.flags |= NIGHTO_CONFUSE_BITS;
                }
            }
            c.phase = CHOREOGRAPHY_DONE_PHASE;
            CastArmStep::Hold
        }
        CHOREOGRAPHY_DONE_PHASE => {
            if let Some(v) = seats.get_mut(who.victim as usize) {
                v.anim_rate = NIGHTO_EXIT_ANIM_RATE;
            }
            CastArmStep::Finish
        }
        p if p <= NIGHTO_CONFUSE_ARM => CastArmStep::Advance,
        _ => CastArmStep::Finish,
    })
}

// ---------------------------------------------------------------------------
// PROT 0908 - Zenoir (spell id 0x86)
// ---------------------------------------------------------------------------

/// PROT 0908's tick entry.
pub const ZENOIR_TICK: u32 = 0x801F_69D8;
/// The arm PROT 0908 latches the victim seat in (`sw v1, -0x7458(v0)` at
/// `0x801F6B74`, i.e. module word `0x801F8BA8`).
pub const ZENOIR_LATCH_ARM: u8 = 0;
/// The arm PROT 0908 shows the summon seat in (`0x801F7098`).
pub const ZENOIR_SHOW_ARM: u8 = 4;
/// The arm PROT 0908 poses the summon seat in (`0x801F723C`, `0x801F743C`).
pub const ZENOIR_POSE_ARM: u8 = 5;
/// The arm PROT 0908's quarter-strength opening hit fires in.
pub const ZENOIR_OPENER_ARM: u8 = 8;
/// The arm PROT 0908 bumps the summon seat's clip in (`0x801F79EC`).
pub const ZENOIR_BUMP_ARM: u8 = 10;
/// The arm PROT 0908's finisher and its splash fire in.
pub const ZENOIR_FINISH_ARM: u8 = 11;
/// The last arm PROT 0908's chain names.
pub const ZENOIR_LAST_ARM: u8 = 12;
/// The render flag PROT 0908 poses the summon seat at in arm 5
/// (`li v0,0x7` at `0x801F7438`).
pub const ZENOIR_POSE_RENDER_FLAG: u8 = 7;
/// The render flag PROT 0908's finisher arm leaves the summon seat and a
/// surviving victim at (`li s0,0x2` at `0x801F7BB0`).
pub const ZENOIR_FINISH_RENDER_FLAG: u8 = 2;
/// The `ctx+0x278` value PROT 0908's finisher arm writes
/// (`li s0,0x2` at `0x801F7BB0`, stored at `0x801F7BF4`).
pub const ZENOIR_FINISH_CTX_278: u8 = 2;

/// PROT 0908 (Zenoir, spell id `0x86`) tick body - 6456 bytes, 1614
/// instructions, and the widest damage footprint in this set.
///
/// Thirteen phase arms `0..=12` plus `0xFF`, through the chain at
/// `0x801F6AA4..0x801F6B38`. `docs/subsystems/cast-module.md` records `0`
/// phase `+0x279` stores for this module; that census counted the
/// `sb rX, 0x279(rY)` displacement form, and this routine materialises
/// `s5 = ctx + 0x279` in its prologue instead - there are **six** phase
/// stores, at `0x801F6DEC`, `0x801F6E04`, `0x801F78E4`, `0x801F7A28`,
/// `0x801F7BE4` and `0x801F82A0`.
///
/// It also resolves its victim differently from its five siblings: arm 0
/// latches `caster[+0x1DD]` into the module word `0x801F8BA8`
/// (`0x801F6B74`), every later arm reads the victim seat back out of that
/// word, and arm `0xFF` writes it back onto `caster[+0x1DD]`
/// (`0x801F82D8`). So a retarget during the cast cannot move the hit.
///
/// Its three damage sites all call `FUN_801DD0AC` with `a1 = 7` and all
/// scale the wrapper's return before clamping - no other module in the band
/// does:
///
/// | site | arm | power | scale | clamp |
/// |---|---|---|---|---|
/// | `0x801F76CC` | 8 | `0x12` | `dmg / 4` | unsigned vs `HP - 1` |
/// | `0x801F7C14` | 11 | `0x10` | `dmg * 3 / 4` | unsigned vs HP |
/// | `0x801F7DF8` | 11 | `0x12` | `dmg * 2 / 3` | unsigned vs HP |
///
/// The third is a **splash**: it walks `actor_table[3 ..= 6]` skipping the
/// primary victim, a dead seat, one whose render flag is non-zero, and a
/// [`FLAG_NON_TARGETABLE`] one. Between the two loops each site keeps a
/// module-local four-entry accumulator (`0x801F6978` seats, `0x801F6980`
/// halfwords) that the HP-bar readout reads; that is presentation and is not
/// mirrored.
///
/// Other simulation state: `ctx+0x278 = 0` in arm 0 and `= 2` in arm 11, the
/// summon seat shown in arm 4, posed at render flag `7` with a `+0x1DA` bump
/// in arm 5, both the caster's and the summon seat's `+0x1DD =`
/// [`TARGET_CODE_ENEMY_ROW`] in arm 8, and the clip pair bumped again in arm
/// 10.
///
/// Not ported: the packet and camera arms, the countdowns, the damage-number
/// accumulators, and `+0x21B` / `+0x21F` / `+0x225` / `+0x176`.
///
/// Wired: `World::run_cast_module_code`.
///
/// PORT: FUN_801F69D8 (PROT 0908; phase chain + the three scaled damage sites; packet and camera arms unported)
pub fn zenoir_tick(
    ctx: &mut CastModuleCtx,
    seats: &mut [CastActorState],
    who: SeruSeats,
    mut rolls: impl FnMut(u8) -> Option<i32>,
) -> (CastTickStep, Vec<SweepHit>) {
    let mut hits = Vec::new();
    let opener = SERU_HIT_SITES[3];
    let finisher = SERU_HIT_SITES[4];
    let splash = SERU_HIT_SITES[5];
    let step = run_chain(ctx, |c| match c.phase {
        ZENOIR_LATCH_ARM => {
            c.ctx_278 = 0;
            CastArmStep::Advance
        }
        ZENOIR_SHOW_ARM => {
            if let Some(s) = seats.get_mut(who.summon as usize) {
                s.render_flag = 0;
            }
            CastArmStep::Advance
        }
        ZENOIR_POSE_ARM => {
            if let Some(s) = seats.get_mut(who.summon as usize) {
                s.staged_anim = s.staged_anim.wrapping_add(1);
                s.render_flag = ZENOIR_POSE_RENDER_FLAG;
            }
            CastArmStep::Advance
        }
        ZENOIR_OPENER_ARM => {
            for slot in [who.caster, who.summon] {
                if let Some(s) = seats.get_mut(slot as usize) {
                    s.target_code = TARGET_CODE_ENEMY_ROW;
                }
            }
            if let Some(v) = seats.get_mut(who.victim as usize)
                && (v.flags & FLAG_NON_TARGETABLE) == 0
                && let Some(roll) = rolls(who.victim)
            {
                let applied = opener.apply(v, roll);
                stage_reaction_alt_only(v);
                v.root_speed = SERU_HIT_ROOT_SPEED;
                v.render_flag = 0;
                hits.push(SweepHit {
                    seat: who.victim,
                    applied,
                });
            }
            CastArmStep::Advance
        }
        ZENOIR_BUMP_ARM => {
            if let Some(s) = seats.get_mut(who.summon as usize) {
                s.restage = s.restage.wrapping_add(1);
                s.staged_anim = s.staged_anim.wrapping_add(1);
            }
            CastArmStep::Advance
        }
        ZENOIR_FINISH_ARM => {
            if let Some(s) = seats.get_mut(who.summon as usize) {
                s.render_flag = ZENOIR_FINISH_RENDER_FLAG;
            }
            c.ctx_278 = ZENOIR_FINISH_CTX_278;
            if let Some(v) = seats.get_mut(who.victim as usize)
                && (v.flags & FLAG_NON_TARGETABLE) == 0
                && let Some(roll) = rolls(who.victim)
            {
                let applied = finisher.apply(v, roll);
                stage_reaction_assign(v);
                v.root_speed = SERU_HIT_ROOT_SPEED;
                v.render_flag = 0;
                if v.hp != 0 {
                    v.render_flag = ZENOIR_FINISH_RENDER_FLAG;
                }
                hits.push(SweepHit {
                    seat: who.victim,
                    applied,
                });
            }
            for seat in FIRST_MONSTER_SEAT..MONSTER_ROW_END {
                if seat == who.victim {
                    continue;
                }
                let Some(v) = seats.get_mut(seat as usize) else {
                    continue;
                };
                if v.hp == 0 || v.render_flag != 0 || (v.flags & FLAG_NON_TARGETABLE) != 0 {
                    continue;
                }
                let Some(roll) = rolls(seat) else {
                    continue;
                };
                let applied = splash.apply(v, roll);
                stage_reaction_assign(v);
                v.root_speed = SERU_HIT_ROOT_SPEED;
                v.render_flag = 0;
                hits.push(SweepHit { seat, applied });
            }
            CastArmStep::Advance
        }
        CHOREOGRAPHY_DONE_PHASE => {
            if let Some(c) = seats.get_mut(who.caster as usize) {
                c.target_code = who.victim;
            }
            CastArmStep::Finish
        }
        p if p <= ZENOIR_LAST_ARM => CastArmStep::Advance,
        _ => CastArmStep::Finish,
    });
    (step, hits)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seat(hp: u16) -> CastActorState {
        CastActorState {
            hp,
            anim_rate: ANIM_RATE_NORMAL,
            knockdown_anim: 0x40,
            reaction_alt: 0x41,
            reaction_alt2: 0x42,
            ..Default::default()
        }
    }

    fn row() -> Vec<CastActorState> {
        (0..8).map(|_| seat(200)).collect()
    }

    const WHO: SeruSeats = SeruSeats {
        caster: 0,
        victim: 3,
        summon: 7,
    };

    /// Walk a body to its terminal and return how many frames it took.
    fn walk(mut step: impl FnMut(&mut CastModuleCtx) -> CastTickStep) -> (u32, u8) {
        let mut ctx = CastModuleCtx::default();
        for n in 0..512 {
            if step(&mut ctx) == CastTickStep::Done {
                return (n, ctx.phase);
            }
        }
        panic!("body never reported Done");
    }

    // -- the three clamps ---------------------------------------------------

    #[test]
    fn shape_c_cannot_kill_but_a_negative_roll_still_clamps() {
        let mut v = seat(50);
        // A roll far above HP stops one short of death.
        assert_eq!(apply_hit_cap_hp_minus_one(&mut v, 999), 49);
        assert_eq!(v.hp, 1);
        // A negative roll read as unsigned compares above any cap, so it
        // clamps rather than healing - which is what separates shape C from
        // the signed `HP - 1` shape.
        let mut v = seat(50);
        let applied = apply_hit_cap_hp_minus_one(&mut v, (-5i32) as u32);
        assert_eq!(applied, 49);
        assert_eq!(v.hp, 1);
    }

    #[test]
    fn shape_a_site_can_kill_where_shape_c_site_cannot() {
        let opener = SERU_HIT_SITES[3];
        let finisher = SERU_HIT_SITES[4];
        assert_eq!(opener.clamp, SeruClamp::HpMinusOneUnsigned);
        assert_eq!(finisher.clamp, SeruClamp::HpUnsigned);

        // Same raw roll, same victim: the opener leaves 1 HP, the finisher
        // empties the bar.
        let mut a = seat(40);
        opener.apply(&mut a, 4000);
        assert_eq!(a.hp, 1);
        let mut b = seat(40);
        finisher.apply(&mut b, 4000);
        assert_eq!(b.hp, 0);
    }

    #[test]
    fn zenoir_scales_each_site_the_way_its_bytes_do() {
        assert_eq!(SERU_HIT_SITES[3].scale(400), 100); // srl 2
        assert_eq!(SERU_HIT_SITES[4].scale(400), 300); // *3 then srl 2
        assert_eq!(SERU_HIT_SITES[5].scale(300), 200); // *2 / 3
        // No other site in the set scales at all.
        for s in &SERU_HIT_SITES[0..3] {
            assert_eq!(s.scale(1234), 1234);
        }
    }

    #[test]
    fn baked_powers_are_the_call_site_immediates() {
        let powers: Vec<u16> = SERU_HIT_SITES.iter().map(|s| s.power()).collect();
        assert_eq!(powers, vec![0x12, 0x11, 0x12, 0x12, 0x10, 0x12]);
        // Both delay-slot constants survive.
        assert_eq!(seru_hit_site(0x801F_7D38).unwrap().power(), 0x11);
        assert_eq!(seru_hit_site(0x801F_7370).unwrap().power(), 0x12);
        // Every site goes through the shared kernel's summon branch.
        assert!(
            SERU_HIT_SITES
                .iter()
                .all(|s| s.wrapper == CastWrapper::SharedSummon)
        );
    }

    // -- the reaction stages ------------------------------------------------

    #[test]
    fn the_three_reaction_stages_differ_in_more_than_spelling() {
        // Bit-set form on a live victim: alt clip, bits 4 and 1 OR'd in over
        // whatever was there.
        let mut v = seat(10);
        v.restage = 0x80;
        stage_reaction_bits(&mut v);
        assert_eq!(v.staged_anim, 0x41);
        assert_eq!(v.restage, 0x85);

        // Assignment form on the same victim throws the old value away.
        let mut v = seat(10);
        v.restage = 0x80;
        stage_reaction_assign(&mut v);
        assert_eq!(v.restage, 5);

        // Dead victim takes the knockdown clip on both.
        let mut v = seat(0);
        stage_reaction_bits(&mut v);
        assert_eq!(v.staged_anim, 0x40);

        // Alt-only form has no knockdown branch at all, and increments.
        let mut v = seat(0);
        v.restage = 2;
        stage_reaction_alt_only(&mut v);
        assert_eq!(v.staged_anim, 0x41);
        assert_eq!(v.restage, 3);
    }

    #[test]
    fn reaction_falls_through_to_the_second_alternative() {
        let mut v = seat(10);
        v.reaction_alt = 0;
        stage_reaction_bits(&mut v);
        assert_eq!(v.staged_anim, 0x42);
    }

    // -- per-body walks -----------------------------------------------------

    #[test]
    fn gimard_hits_only_on_arm_eleven_and_settles_from_twelve() {
        let mut seats = row();
        let mut ctx = CastModuleCtx {
            phase: GIMARD_HIT_ARM,
            ..Default::default()
        };
        let (step, hits) = gimard_tick(&mut ctx, &mut seats, WHO, Some(30));
        assert_eq!(step, CastTickStep::Busy);
        assert_eq!(
            hits,
            vec![SweepHit {
                seat: 3,
                applied: 30
            }]
        );
        assert_eq!(seats[3].hp, 170);
        assert_eq!(seats[7].render_flag, GIMARD_DONE_RENDER_FLAG);
        assert_eq!(ctx.phase, GIMARD_SETTLE_ARM);

        // Arm 12 latches the terminal once the victim has stopped reacting.
        seats[3].playing_anim = 0;
        let (step, _) = gimard_tick(&mut ctx, &mut seats, WHO, None);
        assert_eq!(step, CastTickStep::Busy);
        assert_eq!(ctx.phase, CHOREOGRAPHY_DONE_PHASE);
        assert_eq!(seats[3].anim_rate, GIMARD_SETTLE_ANIM_RATE);
        let (step, _) = gimard_tick(&mut ctx, &mut seats, WHO, None);
        assert_eq!(step, CastTickStep::Done);
    }

    #[test]
    fn gimard_skips_a_non_targetable_victim() {
        let mut seats = row();
        seats[3].flags = FLAG_NON_TARGETABLE;
        let mut ctx = CastModuleCtx {
            phase: GIMARD_HIT_ARM,
            ..Default::default()
        };
        let (_, hits) = gimard_tick(&mut ctx, &mut seats, WHO, Some(30));
        assert!(hits.is_empty());
        assert_eq!(seats[3].hp, 200);
        // The summon-seat and caster writes are outside the guard.
        assert_eq!(seats[7].target_code, 0);
        assert_eq!(seats[0].render_flag, 0);
    }

    #[test]
    fn theeder_sweeps_the_monster_row_and_skips_a_reacting_seat() {
        let mut seats = row();
        seats[4].playing_anim = 9; // already reacting
        seats[5].hp = 0; // dead
        let mut ctx = CastModuleCtx {
            phase: THEEDER_SWEEP_ARM,
            ..Default::default()
        };
        let (_, hits) = theeder_tick(&mut ctx, &mut seats, WHO, |_| Some(25));
        let seats_hit: Vec<u8> = hits.iter().map(|h| h.seat).collect();
        assert_eq!(seats_hit, vec![3, 6]);
        assert_eq!(seats[3].hp, 175);
        assert_eq!(seats[6].hp, 175);
        // The bound is the hard-coded 7, so seat 7 - the summon - is never in
        // the sweep even though the row is long enough.
        assert_eq!(seats[7].hp, 200);
    }

    #[test]
    fn theeder_retargets_both_seats_onto_the_enemy_row() {
        let mut seats = row();
        let mut ctx = CastModuleCtx {
            phase: THEEDER_RETARGET_ARM,
            ..Default::default()
        };
        theeder_tick(&mut ctx, &mut seats, WHO, |_| None);
        assert_eq!(seats[0].target_code, TARGET_CODE_ENEMY_ROW);
        assert_eq!(seats[7].target_code, TARGET_CODE_ENEMY_ROW);
    }

    #[test]
    fn vera_restores_and_clamps_to_the_missing_half() {
        // Level 4 wants 0x20*4 + 0xE0 = 0x160 = 352.
        assert_eq!(vera_heal_amount(4, 100, 1000), 352);
        // Missing less than that clamps to missing.
        assert_eq!(vera_heal_amount(4, 900, 1000), 100);
        // Nothing missing restores nothing.
        assert_eq!(vera_heal_amount(4, 1000, 1000), 0);
    }

    #[test]
    fn vera_heal_reproduces_the_curated_per_level_table() {
        // `legaia_gamedata::VERA_HEAL` is mined from public walkthroughs and
        // lists levels 1..=9 as 256, 288, ... 512. The bytes' formula
        // (`level << 5` then `+ 0xE0`, at 0x801F7C5C / 0x801F7C60) reproduces
        // it exactly, which is an independent check on the read that neither
        // source could give on its own.
        let curated = [256u16, 288, 320, 352, 384, 416, 448, 480, 512];
        for (i, want) in curated.iter().enumerate() {
            let level = (i + 1) as u8;
            // Plenty of headroom so the missing-HP clamp does not bind.
            assert_eq!(
                vera_heal_amount(level, 0x0001, 0x7000),
                *want,
                "magic level {level}"
            );
        }
    }

    #[test]
    fn vera_writes_a_negative_hp_bar_delta_rather_than_accumulating() {
        let mut seats = row();
        seats[3].hp_bar_delta = 77;
        let mut ctx = CastModuleCtx {
            phase: VERA_RESTORE_ARM,
            ..Default::default()
        };
        let (_, out) = vera_tick(
            &mut ctx,
            &mut seats,
            WHO,
            Some(VeraRestore {
                magic_level: 1,
                max_hp: 1000,
                cure_tier: 1,
            }),
        );
        let out = out.unwrap();
        assert_eq!(out.restored, 0x100);
        assert_eq!(seats[3].hp, 200 + 0x100);
        // Stored, not accumulated - the 77 is gone.
        assert_eq!(seats[3].hp_bar_delta, -0x100);
    }

    #[test]
    fn vera_cures_only_from_magic_level_three() {
        let mut seats = row();
        seats[3].flags = 0x0003;
        let mut ctx = CastModuleCtx {
            phase: VERA_RESTORE_ARM,
            ..Default::default()
        };
        let (_, out) = vera_tick(
            &mut ctx,
            &mut seats,
            WHO,
            Some(VeraRestore {
                magic_level: 2,
                max_hp: 1000,
                cure_tier: 1,
            }),
        );
        assert!(!out.unwrap().cured);
        assert_eq!(seats[3].flags, 0x0003);

        let mut ctx = CastModuleCtx {
            phase: VERA_RESTORE_ARM,
            ..Default::default()
        };
        let (_, out) = vera_tick(
            &mut ctx,
            &mut seats,
            WHO,
            Some(VeraRestore {
                magic_level: VERA_CURE_MIN_LEVEL,
                max_hp: 1000,
                cure_tier: 1,
            }),
        );
        assert!(out.unwrap().cured);
        assert_eq!(seats[3].flags, 0x0000);
    }

    #[test]
    fn gizam_walks_through_the_phase_five_hole() {
        let mut seats = row();
        let mut ctx = CastModuleCtx {
            phase: GIZAM_MISSING_ARM,
            ..Default::default()
        };
        let (step, _) = gizam_tick(&mut ctx, &mut seats, WHO, |_| None);
        assert_eq!(step, CastTickStep::Busy);
        assert_eq!(ctx.phase, GIZAM_MISSING_ARM + 1);
    }

    #[test]
    fn gizam_sweep_marks_every_seat_it_hits() {
        let mut seats = row();
        seats[5].flags = FLAG_NON_TARGETABLE;
        let mut ctx = CastModuleCtx {
            phase: GIZAM_SWEEP_ARM,
            ..Default::default()
        };
        let (_, hits) = gizam_tick(&mut ctx, &mut seats, WHO, |_| Some(60));
        assert_eq!(hits.len(), 3);
        for seat in [3u8, 4, 6] {
            assert_eq!(seats[seat as usize].hp, 140);
            assert_eq!(
                seats[seat as usize].flags & GIZAM_STATUS_BIT,
                GIZAM_STATUS_BIT
            );
            assert_eq!(seats[seat as usize].restage, 5);
        }
        assert_eq!(seats[5].hp, 200);
        assert_eq!(seats[7].render_flag, GIZAM_HIDDEN_RENDER_FLAG);
    }

    #[test]
    fn gizam_sweep_is_kill_capable() {
        // `sltu a0, s1` at 0x801F739C is against live HP, not `HP - 1`: this
        // sweep empties the bar where PROT 0908's opener stops at 1.
        let mut seats = row();
        seats[3].hp = 7;
        let mut ctx = CastModuleCtx {
            phase: GIZAM_SWEEP_ARM,
            ..Default::default()
        };
        gizam_tick(&mut ctx, &mut seats, WHO, |_| Some(9999));
        assert_eq!(seats[3].hp, 0);
        // The knockdown fork fires because the hit killed.
        assert_eq!(seats[3].staged_anim, 0x40);
        assert_eq!(seats[3].restage, 1);
    }

    #[test]
    fn nighto_kill_branch_zeroes_hp_and_clears_status() {
        let mut seats = row();
        seats[3].flags = 0xFFFF;
        let mut ctx = CastModuleCtx {
            phase: NIGHTO_FORK_ARM,
            ..Default::default()
        };
        nighto_tick(&mut ctx, &mut seats, WHO, NightoOutcome::Kill);
        assert_eq!(seats[3].hp, 0);
        assert_eq!(seats[3].flags, 0);
        assert_eq!(seats[3].render_flag, NIGHTO_KILL_RENDER_FLAG);
        assert_eq!(ctx.phase, NIGHTO_KILL_SETTLE_ARM);
    }

    #[test]
    fn nighto_confuse_branch_jumps_the_settle_arm() {
        let mut seats = row();
        let mut ctx = CastModuleCtx {
            phase: NIGHTO_FORK_ARM,
            ..Default::default()
        };
        nighto_tick(&mut ctx, &mut seats, WHO, NightoOutcome::Confuse);
        assert_eq!(ctx.phase, NIGHTO_CONFUSE_ARM);
        assert_eq!(seats[3].hp, 200);
        nighto_tick(&mut ctx, &mut seats, WHO, NightoOutcome::Confuse);
        assert_eq!(seats[3].flags & NIGHTO_CONFUSE_BITS, NIGHTO_CONFUSE_BITS);
        assert_eq!(ctx.phase, CHOREOGRAPHY_DONE_PHASE);
        let step = nighto_tick(&mut ctx, &mut seats, WHO, NightoOutcome::Confuse);
        assert_eq!(step, CastTickStep::Done);
        assert_eq!(seats[3].anim_rate, NIGHTO_EXIT_ANIM_RATE);
    }

    #[test]
    fn nighto_roll_is_one_in_eight_death_behind_a_level_scaled_resist() {
        let land = NightoRoll {
            kill_roll: 0,
            resist_roll: 0,
            magic_level: 1,
            target_immune: false,
            extra_roll: None,
        };
        assert_eq!(nighto_outcome(&land), NightoOutcome::Kill);
        // One off the 8 and it is the confuse branch instead.
        assert_eq!(
            nighto_outcome(&NightoRoll {
                kill_roll: 1,
                ..land
            }),
            NightoOutcome::Confuse
        );
        // The resist throw is `rand() % (0x13 - level)`, so the same throw
        // lands at a high level and is resisted at a low one.
        let throw = 9;
        assert_eq!(
            nighto_outcome(&NightoRoll {
                resist_roll: throw,
                magic_level: 0,
                ..land
            }),
            NightoOutcome::Resisted
        );
        assert_eq!(
            nighto_outcome(&NightoRoll {
                resist_roll: throw,
                magic_level: 0x0A,
                ..land
            }),
            NightoOutcome::Kill
        );
        // The monster record's own immunity byte short-circuits everything.
        assert_eq!(
            nighto_outcome(&NightoRoll {
                target_immune: true,
                ..land
            }),
            NightoOutcome::Resisted
        );
        // And the one character index that takes the extra roll loses it on a
        // multiple of three.
        assert_eq!(
            nighto_outcome(&NightoRoll {
                extra_roll: Some(6),
                ..land
            }),
            NightoOutcome::Resisted
        );
        assert_eq!(
            nighto_outcome(&NightoRoll {
                extra_roll: Some(7),
                ..land
            }),
            NightoOutcome::Kill
        );
    }

    #[test]
    fn nighto_resisted_writes_neither_outcome() {
        let mut seats = row();
        let mut ctx = CastModuleCtx {
            phase: NIGHTO_FORK_ARM,
            ..Default::default()
        };
        nighto_tick(&mut ctx, &mut seats, WHO, NightoOutcome::Resisted);
        assert_eq!(seats[3].hp, 200);
        assert_eq!(seats[3].flags, 0);
    }

    #[test]
    fn zenoir_opener_cannot_kill_and_lands_a_quarter() {
        let mut seats = row();
        let mut ctx = CastModuleCtx {
            phase: ZENOIR_OPENER_ARM,
            ..Default::default()
        };
        let (_, hits) = zenoir_tick(&mut ctx, &mut seats, WHO, |s| (s == 3).then_some(80));
        assert_eq!(
            hits,
            vec![SweepHit {
                seat: 3,
                applied: 20
            }]
        );
        assert_eq!(seats[3].hp, 180);
        assert_eq!(seats[0].target_code, TARGET_CODE_ENEMY_ROW);
    }

    #[test]
    fn zenoir_finisher_splashes_every_other_live_monster_seat() {
        let mut seats = row();
        seats[5].render_flag = 3; // hidden -> skipped by the splash guard
        let mut ctx = CastModuleCtx {
            phase: ZENOIR_FINISH_ARM,
            ..Default::default()
        };
        let (_, hits) = zenoir_tick(&mut ctx, &mut seats, WHO, |_| Some(120));
        let seats_hit: Vec<u8> = hits.iter().map(|h| h.seat).collect();
        assert_eq!(seats_hit, vec![3, 4, 6]);
        // 120 * 3/4 on the primary, 120 * 2/3 on the splash.
        assert_eq!(hits[0].applied, 90);
        assert_eq!(hits[1].applied, 80);
        assert_eq!(ctx.ctx_278, ZENOIR_FINISH_CTX_278);
    }

    #[test]
    fn every_body_reaches_done_from_phase_zero() {
        for (entry, last) in SERU_TICK_LAST_ARM {
            let mut seats = row();
            let (frames, phase) = walk(|ctx| match entry {
                903 => gimard_tick(ctx, &mut seats, WHO, None).0,
                904 => theeder_tick(ctx, &mut seats, WHO, |_| None).0,
                905 => vera_tick(ctx, &mut seats, WHO, None).0,
                906 => gizam_tick(ctx, &mut seats, WHO, |_| None).0,
                907 => nighto_tick(ctx, &mut seats, WHO, NightoOutcome::Resisted),
                _ => zenoir_tick(ctx, &mut seats, WHO, |_| None).0,
            });
            // Every chain walks its whole arm run before the terminal, and
            // none of them wraps.
            assert!(frames >= u32::from(last), "{entry} finished in {frames}");
            assert!(phase >= last, "{entry} ended at phase {phase}");
        }
    }

    #[test]
    fn a_phase_past_the_chain_is_done_rather_than_busy_forever() {
        // Retail returns 1 without advancing here; the port reports Done so a
        // host cannot be parked by a phase no arm names.
        for (entry, last) in SERU_TICK_LAST_ARM {
            let mut seats = row();
            let mut ctx = CastModuleCtx {
                phase: last + 1,
                ..Default::default()
            };
            let step = match entry {
                903 => gimard_tick(&mut ctx, &mut seats, WHO, None).0,
                904 => theeder_tick(&mut ctx, &mut seats, WHO, |_| None).0,
                905 => vera_tick(&mut ctx, &mut seats, WHO, None).0,
                906 => gizam_tick(&mut ctx, &mut seats, WHO, |_| None).0,
                907 => nighto_tick(&mut ctx, &mut seats, WHO, NightoOutcome::Resisted),
                _ => zenoir_tick(&mut ctx, &mut seats, WHO, |_| None).0,
            };
            assert_eq!(step, CastTickStep::Done, "PROT {entry}");
        }
    }

    #[test]
    fn seru_last_arm_answers_for_every_entry_in_the_set() {
        assert_eq!(seru_last_arm(903), Some(12));
        assert_eq!(seru_last_arm(908), Some(12));
        assert_eq!(seru_last_arm(909), None);
    }

    // --- W3-A ---------------------------------------------------------------
    //
    // Retail fixtures: numbers read off live PCSX-Redux captures of each body
    // driven end to end in an ordinary random encounter (one party seat, one
    // monster seat). Each row is `(wrapper argument, wrapper return, HP the
    // victim actually lost)` read at the wrapper's entry and its `jr ra`, so
    // the site's baked power, its scale and its clamp are all pinned by the
    // same observation. `docs/subsystems/cast-module.md` § "Frame gating,
    // measured" carries the per-arm dwell these runs also produced.

    /// Every player-Seru site passes the **summon seat** `7` as the attacker,
    /// not the caster seat: retail's `addiu a1, zero, 7` at `0x801F74A8`
    /// (PROT 0903) and `0x801F8880` (PROT 0910) are literal constants, and the
    /// capture reads `a1 = 7` at all four wrapper entries with `ctx[+0x13] = 0`.
    #[test]
    fn w3a_retail_sites_bake_the_power_the_capture_read() {
        for (site, power) in [
            (0x801F_74ACu32, 0x12u16), // PROT 0903, a0 = 0x12 read live
            (0x801F_7D38, 0x11),       // PROT 0904, a0 = 0x11 read live
            (0x801F_76CC, 0x12),       // PROT 0908 opener
            (0x801F_7C14, 0x10),       // PROT 0908 second site
        ] {
            let s = seru_hit_site(site).expect("site is in the table");
            assert_eq!(s.power(), power, "site 0x{site:08X}");
        }
    }

    /// PROT 0903's single hit: the wrapper returned `247` against a victim on
    /// `76` HP and retail applied `76`, emptying the bar; against a victim on
    /// `9999` HP it returned `417` and applied all of it.
    #[test]
    fn w3a_retail_gimard_hit_is_unscaled_and_kill_capable() {
        let site = seru_hit_site(0x801F_74AC).unwrap();
        let mut v = seat(76);
        assert_eq!(site.apply(&mut v, 247), 76);
        assert_eq!(v.hp, 0);
        let mut v = seat(9999);
        assert_eq!(site.apply(&mut v, 417), 417);
        assert_eq!(v.hp, 9999 - 417);
    }

    /// PROT 0904's ring sweep: wrapper return `371`, applied `371`. The same
    /// state produced the same roll on two separate runs, which is what makes
    /// it usable as a fixture.
    #[test]
    fn w3a_retail_theeder_ring_hit_is_unscaled() {
        let site = seru_hit_site(0x801F_7D38).unwrap();
        let mut v = seat(9999);
        assert_eq!(site.apply(&mut v, 371), 371);
    }

    /// PROT 0908's two reachable sites, measured in one cast: the opener
    /// returned `522` and applied `130` (a quarter), the second returned `541`
    /// and applied `405` (three quarters). The third site `0x801F7DF8` never
    /// fired in the capture.
    #[test]
    fn w3a_retail_zenoir_scales_its_two_reachable_sites() {
        let opener = seru_hit_site(0x801F_76CC).unwrap();
        let mut v = seat(9999);
        assert_eq!(opener.apply(&mut v, 522), 130);
        let second = seru_hit_site(0x801F_7C14).unwrap();
        let mut v = seat(9999 - 130);
        assert_eq!(second.apply(&mut v, 541), 405);
    }

    /// PROT 0905's restore, measured twice: magic level `3`, target on `42` of
    /// `999` HP, retail restored exactly `320` = `3 * 0x20 + 0xE0` with the
    /// missing-HP clamp slack.
    #[test]
    fn w3a_retail_vera_restores_three_hundred_and_twenty_at_level_three() {
        assert_eq!(vera_heal_amount(3, 42, 999), 320);
        // And the clamp really is the missing HP, not the base: the same level
        // against Vahn's own 128-point bar restores only what is missing.
        assert_eq!(vera_heal_amount(3, 42, 128), 86);
    }

    /// The measured phase walks end on the arm this table names: PROT 0903 and
    /// 0908 both latched `0xFF` out of arm `12`, PROT 0904 out of `14`, PROT
    /// 0905 out of `10` and PROT 0907 out of `15`.
    #[test]
    fn w3a_retail_phase_walks_end_on_the_tabled_last_arm() {
        assert_eq!(seru_last_arm(903), Some(12));
        assert_eq!(seru_last_arm(904), Some(14));
        assert_eq!(seru_last_arm(905), Some(10));
        assert_eq!(seru_last_arm(907), Some(15));
        assert_eq!(seru_last_arm(908), Some(12));
    }

    /// PROT 0907's fork, as the capture caught it: the kill roll
    /// `0x801F8534` read `6` and the resist word `0x801F853C` read `1`, so
    /// [`nighto_outcome`] classifies it `Resisted` - and retail left the
    /// victim's HP and `+0x16E` untouched, which is what `Resisted` means.
    ///
    /// What the capture also showed, and this model cannot express, is that
    /// retail's *phase target* forks on the kill roll alone (`beqz` at
    /// `0x801F7E04`): a resisted cast whose kill roll is non-zero still takes
    /// the confuse path's `sb 0xF, 0x279` at `0x801F7E28` and lands on arm
    /// `15`, never on arm `14`. See the FINDINGS note on this module.
    #[test]
    fn w3a_retail_nighto_roll_six_with_resist_is_resisted() {
        let roll = NightoRoll {
            kill_roll: 6,
            resist_roll: 9,
            magic_level: 3,
            target_immune: false,
            extra_roll: None,
        };
        assert_eq!(nighto_outcome(&roll), NightoOutcome::Resisted);
    }

    /// The boss immunity is **forced**, not rolled. On the Gaza 2 fight the
    /// two inputs the gate at `0x801F6BF0..0x801F6C24` reads both hold -
    /// `ctx[+0x287] = 4` and the monster record's `+0x20 = 1` - so retail
    /// branches to `0x801F6CB8`, stores a literal `1` into `0x801F853C` and
    /// never draws the throw at `0x801F6C28`. The cast driven there reads that
    /// `1` on the first tick after arm `0` and leaves the boss on 15000 HP.
    /// `target_immune` is that branch, and it wins over any roll.
    #[test]
    fn w3a_retail_nighto_boss_immunity_beats_a_landing_roll() {
        let land = NightoRoll {
            kill_roll: 0,
            resist_roll: 0,
            magic_level: 3,
            target_immune: false,
            extra_roll: None,
        };
        assert_eq!(nighto_outcome(&land), NightoOutcome::Kill);
        let immune = NightoRoll {
            target_immune: true,
            ..land
        };
        assert_eq!(nighto_outcome(&immune), NightoOutcome::Resisted);
    }
    // --- end W3-A -----------------------------------------------------------
}
