//! PROT 0950 Rolling Flare: the single-victim tick and the row sweep tick.

use super::*;

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
            for seat in 0..c.party_count {
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
                for seat in 0..c.party_count {
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
            for seat in 0..c.party_count {
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
