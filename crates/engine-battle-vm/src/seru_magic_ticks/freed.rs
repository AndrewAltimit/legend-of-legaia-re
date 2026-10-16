//! PROT 0912 Freed (`0x8A`): enemy-row damage sweep over twenty arms.

use super::*;

/// PROT 0912 (`summon_freed`) tick body - `0x801CF4EC` row `9`
/// (`0x801F1FCC`). Same VA as [`ORB_TICK`] in a different image, which is why
/// the band's dispatch has to be keyed on `(entry, body)`.
pub const FREED_TICK: u32 = 0x801F_69D8;

// ---------------------------------------------------------------------------
// PROT 0912 - Freed
// ---------------------------------------------------------------------------

/// The `+0x1BA` shatter word PROT 0912's arm 7 leaves on its creature as it
/// hides it (`lbu v0,0x69(a2)` / `sll v0,v0,0x7` / `sh v0,0x1ba(s2)` at
/// `0x801F74E0..0x801F74EC`): `scalar << 7`.
pub const FREED_SHATTER_START: u16 = (crate::cast_module_camera::SPEED_SCALAR as u16) << 7;

/// What PROT 0912's assembly arms take off that word per engine tick
/// (`2 * frame * scalar` a pass, `0x801F75C8` / `0x801F7724`).
pub const FREED_SHATTER_STEP_PER_TICK: u16 =
    2 * crate::cast_module_camera::MODULE_DRAIN_PER_TICK as u16;

/// The arm PROT 0912's creature re-forms in (`0x801F76F0`).
pub const FREED_ASSEMBLE_PHASE: u8 = 9;

/// The arm before it, whose countdown the shatter word drains beside
/// (`0x801F7544`).
pub const FREED_SHATTER_DRAIN_PHASE: u8 = 8;

/// The arm that waits on the creature's strike clip (`0x801F784C`).
pub const FREED_STRIKE_WAIT_PHASE: u8 = 0x0B;

/// PROT 0912's damage arm.
pub const FREED_DAMAGE_PHASE: u8 = 0x11;

/// PROT 0912's settle arm.
pub const FREED_SETTLE_PHASE: u8 = 0x13;

/// PROT 0912's post-hit hold, between the damage and settle arms.
pub const FREED_HOLD_PHASE: u8 = 0x12;

/// `ctx[+0x278]` arm `0x0E` writes (`0x801F7D5C`).
pub const FREED_ARM14_CTX_278: u8 = 2;

/// The colour word `+0x04` arm 7 stores on the creature as it breaks it up
/// (`lui v0,0x3fff; ori v0,v0,0xffff` at `0x801F74C4`).
pub const FREED_SHARD_COLOUR: u32 = 0x3FFF_FFFF;
/// The neutral colour word arm `0x0F` restores on every living monster
/// (`lui a1,0x2008; ori a1,a1,0x200` at `0x801F7E0C`).
pub const FREED_ROW_NEUTRAL_COLOUR: u32 = 0x2008_0200;
/// The colour word arm `0x10` freezes every living monster in
/// (`lui a3,0x3ff0` at `0x801F7ED8`).
pub const FREED_FROZEN_COLOUR: u32 = 0x3FF0_0000;

/// The `+0x0C` root speed arm `0x10` and arm `7` write (`0x801F7F0C`,
/// `0x801F74D4`).
pub const FREED_ROOT_SPEED: i32 = 0x1000;

/// The animation rate the damage arm leaves on each seat it hit
/// (`0x801F8140`).
pub const FREED_HIT_ANIM_RATE: u8 = 2;

/// The clip cursor arm `0x0B` waits on: the creature's anim-player node
/// `+0x68` (a 12.4 keyframe cursor) has to reach this
/// (`lh v0,0x68(v0)` / `slti v0,v0,0x49` at `0x801F7854..0x801F785C`).
pub const FREED_STRIKE_CURSOR: u16 = 0x49;

/// Arm `8`'s re-arm of the module countdown, a **store** of `scalar * 180`
/// (`0x801F75D8..0x801F75F4`, `((s * 3) << 4) - s * 3`, `<< 2`).
pub const FREED_ARM8_REARM_MUL: i32 = 180;

/// The countdown re-arm a gated arm of PROT 0912 adds on its way out, as a
/// shift of the speed scalar (`lbu 0x37D` / `sll` / `addu` onto what the
/// drain left). `None` where the arm adds none: arm `2` and arm `0x0B`
/// **store** theirs, arm `8` stores `scalar * 180`, arm `0x11` adds its own
/// after the sweep, and the gate-free arms touch nothing.
pub const fn freed_rearm_shift(arm: u8) -> Option<i32> {
    Some(match arm {
        3 => 6,    // `0x801F6EEC..0x801F6F08`
        4 => 6,    // `0x801F7140..0x801F7154`
        5 => 6,    // `0x801F7394..0x801F73A8`
        6 => 5,    // `0x801F7428..0x801F743C`
        7 => 5,    // `0x801F752C..0x801F7540`
        0x0C => 7, // `0x801F7A14..0x801F7A28`
        0x0D => 6, // `0x801F7B34..0x801F7B48`
        0x0E => 7, // `0x801F7CCC..0x801F7CDC`
        0x0F => 6, // `0x801F7E14..0x801F7E24`
        0x10 => 6, // `0x801F7F38..0x801F7F48`, joined at `0x801F8168`
        0x12 => 5, // `0x801F8200..0x801F8210`
        _ => return None,
    })
}

/// Whether arm `arm` of PROT 0912 opens with the countdown gate - `subu`
/// of `rate * speed` from `0x801F92A8`, then `bgtz` to the busy return.
pub const fn freed_arm_is_gated(arm: u8) -> bool {
    matches!(arm, 3..=8 | 0x0A | 0x0C..=0x12)
}

/// PROT 0912 (Freed, action id `0x8A`) tick body.
///
/// A `beq`/`slti` chain at `0x801F6A50..0x801F6B6C` over phases `0..=0x13`
/// and `0xFF` - twenty consecutive arms, the widest in this band, through
/// `s4 = ctx + 0x279`. `s2 = actor_table[7]` is the summon seat.
///
/// Nearly every arm is gated on the one module countdown `0x801F92A8`
/// (`settle` here, [`freed_arm_is_gated`]): `subu` of `rate * speed`,
/// **hold** while it stays positive, then a re-arm on the way out
/// ([`freed_rearm_shift`]). Arm `9` drains it too but holds on the shatter
/// word, arm `0x0B` holds on the creature's clip cursor, and arm `0x13` on
/// the enemy row. With the engine's one-vsync tick the arms hold `3`: 128,
/// `4`: 64, `5`: 64, `6`: 64, `7`: 32, `8`: 32, `9`: 32, `0x0A`: 148,
/// `0x0C`: 16, `0x0D`: 128, `0x0E`: 64, `0x0F`: 128, `0x10`: 64, `0x11`: 64,
/// `0x12`: 128 and `0x13`: 32 ticks.
///
/// Simulation writes, by arm:
///
/// * `0` - `ctx[+0x278] = 0`;
/// * `2` - once the stream poll passes, seat the creature, summon
///   `+0x21C = 0xFF`, `+0x04 = 0`, countdown `= scalar << 7`
///   (`0x801F6E94..0x801F6EA8`);
/// * `7` - summon `+0x0C = 0x1000`, `+0x21C = 0xFF`, shatter
///   `+0x1BA = scalar << 7` ([`FREED_SHATTER_START`]);
/// * `8` - the shatter word drains by `2 * rate * speed` a pass beside the
///   countdown (`0x801F75B8..0x801F75D4`); on the way out the countdown is
///   **stored** `scalar * 180`;
/// * `9` - the countdown keeps draining and the shatter word with it; the
///   arm holds on the **shatter word** while it reads positive as a halfword
///   (`bgtz` at `0x801F7734`), then summon `+0x21C = 0`, `+0x1BA = 0`: the
///   creature's triangles fly back together;
/// * `0x0A` - summon `+0x1DA += 1`, `+0x1DC += 1`;
/// * `0x0B` - hold until the creature's clip cursor reaches
///   [`FREED_STRIKE_CURSOR`], then countdown `= scalar << 4`
///   (`0x801F78C8..0x801F78DC`);
/// * `0x0E` - summon `+0x21C = 0xFF` (out of `s0`, loaded `0xFF` at
///   `0x801F7D04` for a colour store nine instructions earlier - another
///   reused-register constant), `+0x04 = 0`, `ctx[+0x278] = 2`;
/// * `0x0F` - every live enemy seat `+0x21C = 0`;
/// * `0x10` - every live enemy seat `+0x0C = 0x1000`, `+0x21C = 0xFF`,
///   `+0x21D = 0`;
/// * `0x11` - the damage sweep: `actor_table[3..7]`, skip dead / `+0x16E & 4`,
///   `FUN_801DD0AC(0x10, 7, seat)`, clamp **shape A** (`sltu` at
///   `0x801F8050`), `+0x10 +=`, `+0x14C -=`, the OR restage form,
///   `+0x21C = 0`, `+0x21D = 2`, then countdown `+= scalar << 7`;
/// * `0x13` - the gated drain, then hold until the enemy row is idle, then
///   phase `0xFF`;
/// * `0xFF` - return zero.
///
/// `creature_cursor` is the creature's anim-node `+0x68`, or `None` when the
/// host has no clip playing on it; arm `0x0B` then passes rather than wait
/// on a cursor nothing advances.
///
/// This body's three module-local words (`0x801F92A8`, `0x801F92AC`,
/// `0x801F92B0`) sit in the image's own zero-filled scratch, which begins at
/// file `+0x28D0`: the run above the spawn records is partly this module's
/// BSS, not donor residue.
///
/// Wired: `World::run_cast_module_code`.
///
/// PORT: FUN_801F69D8 (PROT 0912 tick; phase chain, every arm's countdown / shatter / clip-cursor hold, the enemy-row damage sweep; packet/camera arms unported)
pub fn freed_tick(
    ctx: &mut CastModuleCtx,
    seats: &mut [CastActorState],
    summon_seat: u8,
    settle: &mut i32,
    creature_cursor: Option<u16>,
    mut rolls: impl FnMut(u8) -> i32,
) -> (CastTickStep, Vec<SweepHit>) {
    let mut hits = Vec::new();
    let step = run_latched(ctx, |c| {
        let arm = c.phase;
        let gated = freed_arm_is_gated(arm);
        if gated || arm == FREED_ASSEMBLE_PHASE {
            *settle -= SETTLE_DRAIN_PER_TICK;
        }
        if (arm == FREED_SHATTER_DRAIN_PHASE || arm == FREED_ASSEMBLE_PHASE)
            && let Some(s) = seats.get_mut(summon_seat as usize)
        {
            s.shatter = s.shatter.wrapping_sub(FREED_SHATTER_STEP_PER_TICK);
        }
        if gated && *settle > 0 {
            return CastArmStep::Hold;
        }
        let out = match arm {
            0 => {
                c.ctx_278 = 0;
                CastArmStep::Advance
            }
            2 => {
                if let Some(s) = seats.get_mut(summon_seat as usize) {
                    s.render_flag = 0xFF;
                    // `sw zero,0x4(s2)` at `0x801F6CA0`.
                    s.present_04 = 0;
                }
                *settle = SETTLE_SCALAR << 7;
                CastArmStep::Advance
            }
            7 => {
                if let Some(s) = seats.get_mut(summon_seat as usize) {
                    s.root_speed = FREED_ROOT_SPEED;
                    s.render_flag = 0xFF;
                    s.shatter = FREED_SHATTER_START;
                    // `+0x04 = 0x3FFFFFFF` (`0x801F74C4..0x801F74CC`): the
                    // shards draw lit.
                    s.present_04 = FREED_SHARD_COLOUR;
                }
                CastArmStep::Advance
            }
            FREED_SHATTER_DRAIN_PHASE => {
                *settle = SETTLE_SCALAR * FREED_ARM8_REARM_MUL;
                CastArmStep::Advance
            }
            FREED_ASSEMBLE_PHASE => {
                // `+0x1BA -= 2 * frame * scalar` a pass, hold while it reads
                // positive as a halfword, then `+0x1BA = 0` (`0x801F7720..
                // 0x801F7744`): the creature's triangles fly back together.
                let Some(s) = seats.get_mut(summon_seat as usize) else {
                    return CastArmStep::Advance;
                };
                if s.shatter as i16 > 0 {
                    return CastArmStep::Hold;
                }
                s.render_flag = 0;
                s.shatter = 0;
                CastArmStep::Advance
            }
            0x0A => {
                bump_stage(seats, summon_seat);
                CastArmStep::Advance
            }
            FREED_STRIKE_WAIT_PHASE => {
                if creature_cursor.is_some_and(|cur| cur < FREED_STRIKE_CURSOR) {
                    return CastArmStep::Hold;
                }
                *settle = SETTLE_SCALAR << 4;
                CastArmStep::Advance
            }
            0x0E => {
                if let Some(s) = seats.get_mut(summon_seat as usize) {
                    s.render_flag = 0xFF;
                    // `sw zero,0x4(s2)` at `0x801F7D58`.
                    s.present_04 = 0;
                }
                c.ctx_278 = FREED_ARM14_CTX_278;
                CastArmStep::Advance
            }
            0x0F => {
                for seat in FIRST_MONSTER_SEAT..ROW_SWEEP_SEAT_END {
                    if let Some(s) = seats.get_mut(seat as usize)
                        && s.hp != 0
                    {
                        // `+0x04 = 0x20080200` (`sw a1,0x4(v1)` at
                        // `0x801F7E50`), the neutral word.
                        s.present_04 = FREED_ROW_NEUTRAL_COLOUR;
                        s.render_flag = 0;
                    }
                }
                CastArmStep::Advance
            }
            0x10 => {
                for seat in FIRST_MONSTER_SEAT..ROW_SWEEP_SEAT_END {
                    if let Some(s) = seats.get_mut(seat as usize)
                        && s.hp != 0
                    {
                        s.root_speed = FREED_ROOT_SPEED;
                        s.render_flag = 0xFF;
                        s.anim_rate = 0;
                        // `+0x04 = 0x3FF00000` (`sw a3,0x4(v1)` at
                        // `0x801F7F00`): the frozen row draws blue-white.
                        s.present_04 = FREED_FROZEN_COLOUR;
                    }
                }
                CastArmStep::Advance
            }
            FREED_DAMAGE_PHASE => {
                for seat in FIRST_MONSTER_SEAT..ROW_SWEEP_SEAT_END {
                    let Some(v) = seats.get_mut(seat as usize) else {
                        continue;
                    };
                    if !row_seat_is_hittable(v) {
                        continue;
                    }
                    let applied = apply_hit_floor_zero(v, rolls(seat));
                    stage_reaction_or(v);
                    v.render_flag = 0;
                    v.anim_rate = FREED_HIT_ANIM_RATE;
                    hits.push(SweepHit { seat, applied });
                }
                // `countdown += scalar << 7` (`0x801F8154..0x801F816C`).
                *settle += SETTLE_SCALAR << 7;
                CastArmStep::Advance
            }
            FREED_SETTLE_PHASE => {
                // The gated drain first (`0x801F8244..0x801F827C`), then the
                // row poll (`0x801F8284..0x801F8304`).
                if settle_countdown_holds(settle) {
                    return CastArmStep::Hold;
                }
                if row_is_idle(seats, FIRST_MONSTER_SEAT, ROW_SWEEP_SEAT_END) {
                    c.phase = SERU_B_DONE_PHASE;
                }
                CastArmStep::Hold
            }
            SERU_B_DONE_PHASE => CastArmStep::Finish,
            1 | 3 | 4 | 5 | 6 | 0x0C | 0x0D | FREED_HOLD_PHASE => CastArmStep::Advance,
            _ => CastArmStep::Finish,
        };
        if out == CastArmStep::Advance
            && let Some(shift) = freed_rearm_shift(arm)
        {
            *settle += SETTLE_SCALAR << shift;
        }
        out
    });
    (step, hits)
}
