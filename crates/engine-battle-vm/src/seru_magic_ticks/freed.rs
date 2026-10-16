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

/// PROT 0912's damage arm.
pub const FREED_DAMAGE_PHASE: u8 = 0x11;

/// PROT 0912's settle arm.
pub const FREED_SETTLE_PHASE: u8 = 0x13;

/// PROT 0912's post-hit hold, between the damage and settle arms.
pub const FREED_HOLD_PHASE: u8 = 0x12;

/// `ctx[+0x278]` arm `0x0E` writes (`0x801F7D5C`).
pub const FREED_ARM14_CTX_278: u8 = 2;

/// The `+0x0C` root speed arm `0x10` and arm `7` write (`0x801F7F0C`,
/// `0x801F74D4`).
pub const FREED_ROOT_SPEED: i32 = 0x1000;

/// The animation rate the damage arm leaves on each seat it hit
/// (`0x801F8140`).
pub const FREED_HIT_ANIM_RATE: u8 = 2;

/// PROT 0912 (Freed, action id `0x8A`) tick body.
///
/// A `beq`/`slti` chain at `0x801F6A50..0x801F6B6C` over phases `0..=0x13`
/// and `0xFF` - twenty consecutive arms, the widest in this band, through
/// `s4 = ctx + 0x279`. `s2 = actor_table[7]` is the summon seat.
///
/// Simulation writes, by arm:
///
/// * `0` - `ctx[+0x278] = 0`;
/// * `2` - summon `+0x21C = 0xFF`, `+0x04 = 0`;
/// * `7` - summon `+0x0C = 0x1000`, `+0x21C = 0xFF`, shatter
///   `+0x1BA = scalar << 7`;
/// * `9` - summon `+0x21C = 0`; the shatter word drains by `2 * frame *
///   scalar` a pass and the arm holds until it reaches zero (arm `8`'s own
///   drain of the same word rides its countdown, which is not modelled, so
///   the port's creature re-forms from the full `scalar << 7`);
/// * `0x0A` - summon `+0x1DA += 1`, `+0x1DC += 1`;
/// * `0x0E` - summon `+0x21C = 0xFF` (out of `s0`, loaded `0xFF` at
///   `0x801F7D04` for a colour store nine instructions earlier - another
///   reused-register constant), `+0x04 = 0`, `ctx[+0x278] = 2`;
/// * `0x0F` - every live enemy seat `+0x21C = 0`;
/// * `0x10` - every live enemy seat `+0x0C = 0x1000`, `+0x21C = 0xFF`,
///   `+0x21D = 0`;
/// * `0x11` - the damage sweep: `actor_table[3..7]`, skip dead / `+0x16E & 4`,
///   `FUN_801DD0AC(0x10, 7, seat)`, clamp **shape A** (`sltu` at
///   `0x801F8050`), `+0x10 +=`, `+0x14C -=`, the OR restage form,
///   `+0x21C = 0`, `+0x21D = 2`;
/// * `0x13` - hold until the enemy row is idle, then phase `0xFF`;
/// * `0xFF` - return zero.
///
/// This body's three module-local words (`0x801F92A8`, `0x801F92AC`,
/// `0x801F92B0`) sit in the image's own zero-filled scratch, which begins at
/// file `+0x28D0`: the run above the spawn records is partly this module's
/// BSS, not donor residue.
///
/// Wired: `World::run_cast_module_code`.
///
/// PORT: FUN_801F69D8 (PROT 0912 tick; phase chain + the enemy-row damage sweep, packet/camera arms unported)
pub fn freed_tick(
    ctx: &mut CastModuleCtx,
    seats: &mut [CastActorState],
    summon_seat: u8,
    settle: &mut i32,
    mut rolls: impl FnMut(u8) -> i32,
) -> (CastTickStep, Vec<SweepHit>) {
    let mut hits = Vec::new();
    let step = run_latched(ctx, |c| match c.phase {
        0 => {
            c.ctx_278 = 0;
            CastArmStep::Advance
        }
        2 => {
            if let Some(s) = seats.get_mut(summon_seat as usize) {
                s.render_flag = 0xFF;
            }
            CastArmStep::Advance
        }
        7 => {
            if let Some(s) = seats.get_mut(summon_seat as usize) {
                s.root_speed = FREED_ROOT_SPEED;
                s.render_flag = 0xFF;
                s.shatter = FREED_SHATTER_START;
            }
            CastArmStep::Advance
        }
        FREED_ASSEMBLE_PHASE => {
            // `+0x1BA -= 2 * frame * scalar` a pass, hold while it reads
            // positive as a halfword, then `+0x1BA = 0` (`0x801F7720..
            // 0x801F7744`): the creature's triangles fly back together.
            let Some(s) = seats.get_mut(summon_seat as usize) else {
                return CastArmStep::Advance;
            };
            s.render_flag = 0;
            s.shatter = s.shatter.wrapping_sub(FREED_SHATTER_STEP_PER_TICK);
            if s.shatter as i16 > 0 {
                return CastArmStep::Hold;
            }
            s.shatter = 0;
            CastArmStep::Advance
        }
        0x0A => {
            bump_stage(seats, summon_seat);
            CastArmStep::Advance
        }
        0x0E => {
            if let Some(s) = seats.get_mut(summon_seat as usize) {
                s.render_flag = 0xFF;
            }
            c.ctx_278 = FREED_ARM14_CTX_278;
            CastArmStep::Advance
        }
        0x0F => {
            for seat in FIRST_MONSTER_SEAT..ROW_SWEEP_SEAT_END {
                if let Some(s) = seats.get_mut(seat as usize)
                    && s.hp != 0
                {
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
        FREED_HOLD_PHASE => {
            // Drain, hold while positive, then re-arm by `scalar << 5`
            // (`0x801F81E4..0x801F8210`).
            *settle -= SETTLE_DRAIN_PER_TICK;
            if *settle > 0 {
                return CastArmStep::Hold;
            }
            *settle += SETTLE_SCALAR << 5;
            CastArmStep::Advance
        }
        FREED_SETTLE_PHASE => {
            // The gated drain first (`0x801F8244..0x801F827C`), then the row
            // poll (`0x801F8284..0x801F8304`).
            if settle_countdown_holds(settle) {
                return CastArmStep::Hold;
            }
            if row_is_idle(seats, FIRST_MONSTER_SEAT, ROW_SWEEP_SEAT_END) {
                c.phase = SERU_B_DONE_PHASE;
            }
            CastArmStep::Hold
        }
        SERU_B_DONE_PHASE => CastArmStep::Finish,
        1 | 3 | 4 | 5 | 6 | 8 | 0x0B | 0x0C | 0x0D => CastArmStep::Advance,
        _ => CastArmStep::Finish,
    });
    (step, hits)
}
