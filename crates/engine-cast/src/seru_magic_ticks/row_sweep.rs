//! Shared pieces of the `0x87..=0x8B` bodies: the damage-shape table, the latched phase runner, the floor-one clamp, the reaction stagers, the enemy-row predicates and the settle countdown.

use super::*;

// ---------------------------------------------------------------------------
// Damage shapes
// ---------------------------------------------------------------------------

/// Every damage site in these five modules goes through `FUN_801DD0AC` with
/// `a1 = 7` - the shared battle kernel's **summon** branch, the same wrapper
/// PROT 0927's AoE stager uses.
///
/// The rest of the band's shared callees, named throughout this module and
/// ported (or owned) elsewhere: the two spawn stagers the pool drives, the
/// two camera helpers, the text draw, the flash spawner, the summon-creature
/// installer and the damage-number ring push.
///
/// REF: FUN_801DD0AC, FUN_80021B04, FUN_80050ED4, FUN_801D829C, FUN_801DCEAC, FUN_8003541C, FUN_80024E80, FUN_801F19EC, FUN_801F44A0
const SUMMON_WRAPPER: CastWrapper = CastWrapper::SharedSummon;

/// The baked per-hit powers, in `(PROT entry, routine, power)` form.
///
/// Each is the `a0` an `addiu a0,zero,<imm>` sets immediately before the
/// `jal 0x801DD0AC`, read off the disassembly rather than off any table:
/// PROT 0909 `0x801F75E8`, PROT 0910 `0x801F8874`, PROT 0912 `0x801F8018`,
/// PROT 0913 `0x801F8438`. Three of the four sit in a `bne` **delay slot**
/// (every one except PROT 0910's), so a backward scan from the `jal` that
/// stops at the branch misses them.
pub const SERU_B_DAMAGE_SHAPES: [CastDamageShape; 4] = [
    CastDamageShape {
        prot_entry: 909,
        routine: VIGURO_TICK,
        wrapper: SUMMON_WRAPPER,
        never_kills: false,
        powers: &[0x12],
    },
    CastDamageShape {
        prot_entry: 910,
        routine: SWORDIE_SLASH,
        wrapper: SUMMON_WRAPPER,
        never_kills: false,
        powers: &[0x12],
    },
    CastDamageShape {
        prot_entry: 912,
        routine: FREED_TICK,
        wrapper: SUMMON_WRAPPER,
        never_kills: false,
        powers: &[0x10],
    },
    CastDamageShape {
        prot_entry: 913,
        routine: NOVA_TICK,
        wrapper: SUMMON_WRAPPER,
        never_kills: false,
        powers: &[0x12],
    },
];

/// The damage shape of one of the five modules, or `None` for PROT 0911,
/// whose only `+0x14C` write is a **heal**.
///
/// Kept apart from [`crate::cast_module_ticks::CAST_DAMAGE_SHAPES`] on
/// purpose: that table answers for a module's *stager*, and these four sites
/// are all in tick bodies (PROT 0910's in a tick body's callee). Nothing in
/// the engine reads this into the generic cast fold yet - see the module
/// note on the fold seam.
pub fn seru_b_damage_shape(prot_entry: u32) -> Option<&'static CastDamageShape> {
    SERU_B_DAMAGE_SHAPES
        .iter()
        .find(|s| s.prot_entry == prot_entry)
}

// ---------------------------------------------------------------------------
// The idioms these five bodies add
// ---------------------------------------------------------------------------

/// The latched-busy return every body in this file takes, spelled out because
/// `cast_module_ticks`' own copy is private to that module.
///
/// Each of the five seeds a register (or, for PROT 0911 and 0913, a stack
/// word) with `1` in its prologue and returns it; only a terminal arm clears
/// it. An arm that runs and does not reach a `ctx[+0x279] += 1` site
/// therefore reports **busy**, which is what a rendezvous hold is.
pub(super) fn run_latched(
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

/// Apply shape **C** - an *unsigned* clamp to `HP - 1`.
///
/// ```text
/// v0 = victim[+0x14C]
/// v1 = v0 - 1                  ; the cap
/// sltu v0, v1, dmg             ; UNSIGNED
/// if v0 { dmg = v1 }
/// victim[+0x10]  += dmg
/// victim[+0x14C] -= dmg
/// ```
///
/// PROT 0910's `0x801F88D8..0x801F8910`. It differs from both shapes the band
/// already carries: [`apply_hit_floor_zero`] caps at HP (so it can kill) and
/// `apply_hit_floor_one` compares **signed** (so a negative roll heals). Here
/// the comparison is unsigned against `HP - 1`, so the hit can neither kill
/// nor heal - a negative roll reads as a huge unsigned value and clamps to
/// the cap, leaving the victim on 1 HP.
///
/// The cap is formed with `lhu` then `addiu -1` in a 32-bit register, so a
/// victim already on `0` HP has a cap of `0xFFFFFFFF`: nothing clamps and the
/// `sh` wraps HP below zero. The port keeps that wrap rather than saturating
/// the cap to `0`.
///
/// Returns the damage actually applied.
pub fn apply_hit_unsigned_floor_one(victim: &mut CastActorState, roll: i32) -> u32 {
    let cap = u32::from(victim.hp).wrapping_sub(1);
    let mut dmg = roll as u32;
    if cap < dmg {
        dmg = cap;
    }
    victim.hp_bar_delta = victim.hp_bar_delta.wrapping_add(dmg as i32);
    victim.hp = u32::from(victim.hp).wrapping_sub(dmg) as u16;
    dmg
}

/// The band's three-leg reaction pick, in the form these five bodies spell it
/// (`0x801F7678..0x801F76D8` in PROT 0909 and the same block in 0910 / 0912 /
/// 0913):
///
/// ```text
/// if victim[+0x14C] == 0 || victim[+0x1F2] != 0 { victim[+0x1DA] = victim[+0x1F1] }
/// else { victim[+0x1DA] = victim[+0x1EF] != 0 ? victim[+0x1EF] : victim[+0x1F0] ; alt }
/// ```
///
/// `crate::cast_module_ticks::stage_reaction` carries only the two-leg form
/// (`+0x1F2` picks `+0x1F1` or `+0x1EF`). The three-leg one is what the band
/// actually runs: a **dead** victim takes the knockdown clip regardless of the
/// gate, and a zero `+0x1EF` falls on to `+0x1F0`.
///
/// Returns `true` when the knockdown leg was taken, because the restage write
/// that follows differs between the two legs and between modules.
fn pick_reaction_clip(victim: &mut CastActorState) -> bool {
    if victim.hp == 0 || victim.reaction_gate != 0 {
        victim.staged_anim = victim.knockdown_anim;
        true
    } else {
        victim.staged_anim = if victim.reaction_alt != 0 {
            victim.reaction_alt
        } else {
            victim.reaction_alt2
        };
        false
    }
}

/// PROT 0909's restage form: the knockdown leg **sets** `+0x1DC = 1`
/// (`sb s5,0x1dc(v0)` at `0x801F76AC` with `s5` seeded `1` at `0x801F75B0`)
/// and the alternate leg sets `5` (`0x801F76D4`). Neither is a bump - the
/// counter is assigned.
pub(super) fn stage_reaction_set(victim: &mut CastActorState) {
    let knockdown = pick_reaction_clip(victim);
    victim.restage = if knockdown { 1 } else { 5 };
}

/// PROT 0910 / 0912 / 0913's restage form: the alternate leg ORs in `4`
/// (`0x801F80E8`, `0x801F84E0`, `0x801F896C`) and **both** legs then OR in `1`
/// (`0x801F8100`, `0x801F84F0`, `0x801F8978`). So `+0x1DC` is a bit set here
/// and a counter in PROT 0909 - one field, two readings, in one band.
pub(super) fn stage_reaction_or(victim: &mut CastActorState) {
    if !pick_reaction_clip(victim) {
        victim.restage |= 0x04;
    }
    victim.restage |= 0x01;
}

/// A summon-creature module's **non-lethal pre-hit**, the hit some modules
/// land before their killing one: the roll is clamped to `HP - 1`
/// (`sltu` against `+0x14C - 1`, [`apply_hit_cap_hp_minus_one`]), `+0x10`
/// accumulates it, and the reaction is staged in the `OR` form. PROT 0915
/// arm 7 (`0x801F7004..`), PROT 0928 arm `0xF` (`0x801F80C4..0x801F81EC`),
/// PROT 0932 arm `0x12` and PROT 0933 arm `0xC` each run it, over their
/// latched victim or their living monster row. Returns the damage applied.
pub fn summon_pre_hit(victim: &mut CastActorState, dmg: u32) -> u32 {
    let applied = super::apply_hit_cap_hp_minus_one(victim, dmg);
    stage_reaction_or(victim);
    applied
}

/// Is this seat in the enemy row's sweep? Both PROT 0909's and PROT 0912's
/// loops open with `lhu +0x14C; beqz skip` then `lhu +0x16E; andi 4; bnez
/// skip`, over `actor_table[3 .. 7]` (`addiu a0,s6,0xc` then `sltiu s3,7`).
pub(super) fn row_seat_is_hittable(actor: &CastActorState) -> bool {
    actor.hp != 0 && (actor.flags & FLAG_NON_TARGETABLE) == 0
}

/// The seat range PROT 0909's and PROT 0912's sweeps walk: `3..7`, fixed in
/// the bytes as `addiu s3,zero,3` / `sltiu v0,s3,0x7` - not `ctx[+1]`.
pub const ROW_SWEEP_SEAT_END: u8 = 7;

/// "Every enemy seat has finished reacting", the gate PROT 0909 arm `0x0E`,
/// PROT 0912 arm `0x13` and PROT 0913 arm `0x14` all hold on before they
/// write the terminal phase:
///
/// ```text
/// live seat -> require +0x1D9 == 0   (no clip playing)
/// dead seat -> require +0x04  == 0   (the fade word has run out)
/// ```
///
/// `+0x04` is [`CastActorState::present_04`], the battle actor's render
/// word, which a dead seat's defeat fade walks to zero - so the band holds
/// until the row's dead have faded out, as retail's does.
pub(super) fn row_is_idle(seats: &[CastActorState], first: u8, end: u8) -> bool {
    (first..end).all(|s| {
        seats
            .get(s as usize)
            .map(|a| {
                if a.hp == 0 {
                    a.present_04 == 0
                } else {
                    a.playing_anim == 0
                }
            })
            .unwrap_or(true)
    })
}

/// The terminal phase every one of the five chains names (`0xFF`).
pub const SERU_B_DONE_PHASE: u8 = 0xFF;

/// The settle countdown's drain per engine tick. Retail drains the module's
/// countdown word by `0x1F800393 * 0x1F80037D` (frame step times the speed
/// scalar) a battle pass; the engine ticks once a vsync, so a tick drains
/// the scalar alone - the convention [`SwordieClock`] states for PROT 0910.
pub const SETTLE_DRAIN_PER_TICK: i32 =
    legaia_engine_battle_vm::battle_anim_rate::RATE_NORMAL as i32;

/// The scalar the arms shift into their countdown re-arms (`lbu 0x37D`).
pub(super) const SETTLE_SCALAR: i32 = legaia_engine_battle_vm::battle_anim_rate::RATE_NORMAL as i32;

/// The gated drain the settle arms of PROT 0909, 0912 and 0913 open with
/// (`lw; blez; subu; sw; bgtz`): a word already spent is left alone, a live
/// one drains, and the arm holds while it is still positive.
pub(super) fn settle_countdown_holds(cd: &mut i32) -> bool {
    if *cd <= 0 {
        return false;
    }
    *cd -= SETTLE_DRAIN_PER_TICK;
    *cd > 0
}

/// The five modules' seat conventions in one place, so a host does not have
/// to re-derive them: every body takes `actor_table[7]` as the summon seat
/// ([`SUMMON_SEAT`]), the enemy row as `3..7` ([`FIRST_MONSTER_SEAT`] ..
/// [`ROW_SWEEP_SEAT_END`]), and the single-victim bodies resolve their target
/// as `actor_table[caster[+0x1DD]]` behind a `sltiu ...,8` bound.
pub const SERU_B_SUMMON_SEAT: u8 = SUMMON_SEAT;
