//! PROT 0944 Guilty Cross: the damage tick and its curse tick.

use super::*;

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
