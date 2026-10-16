//! PROT 0913 Nova (`0x8B`): single-victim damage tick.

use super::*;

/// PROT 0913 (`summon_nova`) tick body - `0x801CF4EC` row `10`
/// (`0x801F1FDC`). The largest body in the band at 7260 B / 1815
/// instructions.
pub const NOVA_TICK: u32 = 0x801F_69F0;

// ---------------------------------------------------------------------------
// PROT 0913 - Nova
// ---------------------------------------------------------------------------

/// PROT 0913's damage arm.
pub const NOVA_DAMAGE_PHASE: u8 = 0x13;

/// PROT 0913's settle arm.
pub const NOVA_SETTLE_PHASE: u8 = 0x14;

/// The arm that lifts the victim on a rising counter (`0x801F7F34`).
pub const NOVA_LIFT_PHASE: u8 = 0x10;

/// Arm `0x10` holds while its counter is at most this (`slti a2,0x401` /
/// `bne` at `0x801F80A8..0x801F80AC`).
pub const NOVA_LIFT_END: i32 = 0x400;

/// The clip arm `9` stages on the summon seat, and the same value it writes
/// to `ctx[+0x278]` in the same arm out of the same register (`li s0,0x2` at
/// `0x801F7788`, stored at `0x801F778C` and `0x801F77F0`).
pub const NOVA_ARM9_CLIP: u8 = 2;

/// The victim's render flag in arm `0x0F` (`li v1,0x7` at `0x801F7E4C`).
pub const NOVA_ARM15_VICTIM_RENDER_FLAG: u8 = 7;

/// What PROT 0913's arm `arm` does to the module countdown `0x801F9124` on
/// its way out, as the multiple of the speed scalar it **stores** there
/// (`lbu 0x37D` and a shift / multiply, then `sw`). `None` for an arm that
/// leaves the word alone: arms `7` and `0x0D` (it is left spent), arm
/// `0x0F` stores zero, and arm `0x13` adds rather than stores.
pub const fn nova_rearm_mul(arm: u8) -> Option<i32> {
    Some(match arm {
        2 => 64,    // `<< 6`, `0x801F6E40`
        3 => 160,   // `* 5 << 5`, `0x801F6EE4`
        4 => 64,    // `0x801F6FE4`
        5 => 32,    // `0x801F7074`
        6 => 240,   // `0x801F719C`
        8 => 128,   // `0x801F72FC`
        9 => 128,   // `0x801F77E8`
        0x0A => 64, // `0x801F784C`
        0x0B => 64, // `0x801F792C`
        0x0C => 48, // `0x801F79C8`
        0x0E => 64, // `0x801F7D78`
        0x0F => 0,  // `sw zero` at `0x801F7ED4`
        _ => return None,
    })
}

/// Whether arm `arm` of PROT 0913 opens with the countdown gate - `subu`
/// of `rate * speed` from `0x801F9124`, then `bgtz` to the busy return.
pub const fn nova_arm_is_gated(arm: u8) -> bool {
    matches!(arm, 3..=7 | 9..=0x0D | 0x0F)
}

/// PROT 0913 (Nova, action id `0x8B`) tick body - the band's largest.
///
/// A `beq`/`slti` chain at `0x801F6A8C..0x801F6BC4` over phases `0..=0x14`
/// and `0xFF`, twenty-one consecutive arms. `s8 = actor_table[7]` is the
/// summon seat and `s2 = actor_table[caster[+0x1DD]]` the victim; the busy
/// flag is the stack word `0x38(sp)`.
///
/// The arms pace on the one module countdown `0x801F9124` (`settle` here):
/// a gated arm ([`nova_arm_is_gated`]) drains it by `rate * speed` and
/// **holds** while it stays positive (`bgtz` to `0x801F8618`), then stores
/// the next arm's span ([`nova_rearm_mul`]). Arm `0x10` instead **raises** it
/// by `(rate * speed) / 2` and holds while it is at most `0x400`; arm `0x13`
/// drains it and holds until it is spent (`blez` at `0x801F82A0`). With the
/// engine's one-vsync tick the arms hold `3`: 64, `4`: 160, `5`: 64, `6`: 32,
/// `7`: 240, `9`: 128, `0x0A`: 128, `0x0B`: 64, `0x0C`: 64, `0x0D`: 48,
/// `0x0F`: 64, `0x10`: 257 and `0x13`: 129 ticks; `8`, `0x0E`, `0x11` and
/// `0x12` pass at once.
///
/// Simulation writes, by arm:
///
/// * `0` - `ctx[+0x278] = 0`;
/// * `3` - summon `+0x1DA += 1`;
/// * `9` - summon `+0x1DA = 2`, `+0x1DC += 1`, `ctx[+0x278] = 2`;
/// * `0x0D` - victim `+0x21C = 0`, summon `+0x04 = 0`, `+0x21C = 0xFF`;
/// * `0x0F` - victim `+0x21C = 7`;
/// * `0x10` - `ctx[+0x278] = 0` on the way out (`0x801F8154`);
/// * `0x13` - once the countdown is spent, the damage step: **one** victim,
///   and the only guard is `+0x16E & 4` - there is no dead-seat skip, so
///   retail will roll against a corpse. `FUN_801DD0AC(0x12, 7,
///   caster[+0x1DD])`, clamp **shape A** (`sltu` at `0x801F8470`),
///   `+0x10 +=`, `+0x14C -=`, the OR restage form, then `+0x04 = 0` and
///   `+0x21C = 0` over `actor_table[0 .. ctx[+0]]`, and countdown
///   `+= scalar << 7`;
/// * `0x14` - hold until the victim is idle, then phase `0xFF`;
/// * `0xFF` - return zero.
///
/// Not ported: the victim's lift and spin in arms `0x10` and `0x13`
/// (`+0x36 = -sin(countdown) * ctx[+0x6D0] >> 14` / `>> 12`, `+0x46`
/// stepped a pass), the jitter loops over the effect handles, and the
/// packet and camera writes.
///
/// `docs/reference/functions/battle.md` groups PROT 0913 with the band's
/// *heal* modules. It is not one: the store at `0x801F8490` is `subu` under a
/// `FUN_801DD0AC` roll, the same damage shape PROT 0909 and 0912 take.
///
/// Wired: `World::run_cast_module_code`.
///
/// PORT: FUN_801F69F0 (PROT 0913 tick; phase chain, every arm's countdown hold, the single-victim damage step; victim lift, packet/camera arms unported)
pub fn nova_tick(
    ctx: &mut CastModuleCtx,
    seats: &mut [CastActorState],
    summon_seat: u8,
    victim_seat: u8,
    roll: i32,
    settle: &mut i32,
) -> (CastTickStep, Option<SweepHit>) {
    let mut hit = None;
    let step = run_latched(ctx, |c| {
        let arm = c.phase;
        let gated = nova_arm_is_gated(arm);
        if gated {
            *settle -= SETTLE_DRAIN_PER_TICK;
            if *settle > 0 {
                return CastArmStep::Hold;
            }
        }
        let out = match arm {
            0 => {
                c.ctx_278 = 0;
                CastArmStep::Advance
            }
            3 => {
                if let Some(s) = seats.get_mut(summon_seat as usize) {
                    s.staged_anim = s.staged_anim.wrapping_add(1);
                }
                CastArmStep::Advance
            }
            9 => {
                if let Some(s) = seats.get_mut(summon_seat as usize) {
                    s.staged_anim = NOVA_ARM9_CLIP;
                    s.restage = s.restage.wrapping_add(1);
                }
                c.ctx_278 = NOVA_ARM9_CLIP;
                CastArmStep::Advance
            }
            0x0D => {
                if let Some(s) = seats.get_mut(victim_seat as usize) {
                    s.render_flag = 0;
                }
                if let Some(s) = seats.get_mut(summon_seat as usize) {
                    s.render_flag = 0xFF;
                }
                CastArmStep::Advance
            }
            0x0F => {
                if let Some(s) = seats.get_mut(victim_seat as usize) {
                    s.render_flag = NOVA_ARM15_VICTIM_RENDER_FLAG;
                }
                CastArmStep::Advance
            }
            NOVA_LIFT_PHASE => {
                // `W += (rate * speed) >> 1` (`sra 1`, stored in the delay
                // slot at `0x801F8054`), hold while `W <= 0x400`.
                *settle += SETTLE_DRAIN_PER_TICK >> 1;
                if *settle <= NOVA_LIFT_END {
                    return CastArmStep::Hold;
                }
                c.ctx_278 = 0;
                CastArmStep::Advance
            }
            NOVA_DAMAGE_PHASE => {
                // Drain, and `blez` out once spent (`0x801F8280..0x801F82A0`).
                *settle -= SETTLE_DRAIN_PER_TICK;
                if *settle > 0 {
                    return CastArmStep::Hold;
                }
                if let Some(v) = seats.get_mut(victim_seat as usize)
                    && (v.flags & FLAG_NON_TARGETABLE) == 0
                {
                    let applied = apply_hit_floor_zero(v, roll);
                    stage_reaction_or(v);
                    hit = Some(SweepHit {
                        seat: victim_seat,
                        applied,
                    });
                }
                for seat in 0..c.party_count {
                    if let Some(s) = seats.get_mut(seat as usize) {
                        s.render_flag = 0;
                    }
                }
                // `countdown += scalar << 7` (`0x801F8550..0x801F8568`).
                *settle += SETTLE_SCALAR << 7;
                CastArmStep::Advance
            }
            NOVA_SETTLE_PHASE => {
                // The gated drain (`0x801F858C..0x801F85C0`), then the victim:
                // a dead one once its render word `+0x04` has faded to zero, a
                // living one once it is back on idle (`0x801F85C4..0x801F85F4`).
                if settle_countdown_holds(settle) {
                    return CastArmStep::Hold;
                }
                let settled = seats
                    .get(victim_seat as usize)
                    .map(|v| {
                        if v.hp == 0 {
                            v.present_04 == 0
                        } else {
                            v.playing_anim == 0
                        }
                    })
                    .unwrap_or(true);
                if settled {
                    c.phase = SERU_B_DONE_PHASE;
                }
                CastArmStep::Hold
            }
            SERU_B_DONE_PHASE => {
                if let Some(s) = seats.get_mut(summon_seat as usize) {
                    s.anim_rate = ANIM_RATE_NORMAL;
                }
                CastArmStep::Finish
            }
            1 | 2 | 4 | 5 | 6 | 7 | 8 | 0x0A | 0x0B | 0x0C | 0x0E | 0x11 | 0x12 => {
                CastArmStep::Advance
            }
            _ => CastArmStep::Finish,
        };
        if out == CastArmStep::Advance
            && let Some(mul) = nova_rearm_mul(arm)
        {
            *settle = SETTLE_SCALAR * mul;
        }
        out
    });
    (step, hit)
}
