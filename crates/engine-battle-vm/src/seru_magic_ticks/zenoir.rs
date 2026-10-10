//! PROT 0908 Zenoir (`0x86`): opener, finisher and splash tick.

use super::*;

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
