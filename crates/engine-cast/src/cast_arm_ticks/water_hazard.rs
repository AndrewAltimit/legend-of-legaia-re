//! PROT 0956 Water Hazard tick.

use super::*;

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
