//! PROT 0909 Viguro (`0x87`): enemy-row damage sweep with two rendezvous phases.

use super::*;

// ---------------------------------------------------------------------------
// The five tick-body addresses
// ---------------------------------------------------------------------------

/// PROT 0909 (`summon_viguro`) tick body - `0x801CF4EC` row `6`, reached from
/// PROT 0898 at `0x801F1F9C`.
pub const VIGURO_TICK: u32 = 0x801F_69F4;

// ---------------------------------------------------------------------------
// PROT 0909 - Viguro
// ---------------------------------------------------------------------------

/// The two phases PROT 0909's chain does **not** name. The tick parks on each
/// until the module's stager bumps `ctx[+0x279]` from the effect script.
pub const VIGURO_RENDEZVOUS_PHASES: [u8; 2] = [3, 8];

/// PROT 0909's damage arm.
pub const VIGURO_DAMAGE_PHASE: u8 = 0x0D;

/// PROT 0909's settle arm - the one that writes [`SERU_B_DONE_PHASE`].
pub const VIGURO_SETTLE_PHASE: u8 = 0x0E;

/// `ctx[+0x278]` arm `0` writes (`addiu v0,zero,3; sb v0,0x278(v1)` at
/// `0x801F6BA0`), paired with `ctx[+0x27A] = 0x80` (`0x801F6B94`).
pub const VIGURO_ARM0_CTX_278: u8 = 3;

/// `ctx[+0x27A]` arm `0` writes.
pub const VIGURO_ARM0_CTX_27A: u8 = 0x80;

/// The summon seat's render flag in arm `7` (`addiu v0,zero,4; sb
/// v0,0x21c(s4)` at `0x801F6F54`).
pub const VIGURO_ARM7_RENDER_FLAG: u8 = 4;

/// The render flag arm `0x0B` writes on every live enemy seat
/// (`addiu a1,zero,4` at `0x801F7240`, stored at `0x801F726C`).
pub const VIGURO_ARM11_ROW_RENDER_FLAG: u8 = 4;

/// The summon seat's render flag in the terminal arm (`addiu v0,zero,2; sb
/// v0,0x21c(s4)` at `0x801F78F8`).
pub const VIGURO_DONE_RENDER_FLAG: u8 = 2;

/// The `+0x0C` root speed the damage arm writes on each seat it hit
/// (`0x801F76EC`).
pub const VIGURO_HIT_ROOT_SPEED: i32 = 0x1000;

/// PROT 0909 (Viguro, action id `0x87`) tick body.
///
/// A `beq`/`slti` chain at `0x801F6A6C..0x801F6B24` over phases
/// `0,1,2,4,5,6,7,9,0x0A,0x0B,0x0C,0x0D,0x0E,0xFF`, driven through
/// `s7 = ctx + 0x279` (materialised at `0x801F6A70`). `s4 = actor_table[7]`
/// is the summon seat, `s2` the context and `s6` the actor table; the
/// **caster** is only reached in arm `9`, as `actor_table[ctx[+0x13]]`.
///
/// Simulation writes, by arm:
///
/// * `0` - `ctx[+0x278] = 3`, `ctx[+0x27A] = 0x80` (the band of
///   `ctx[+0x27B..0x283]` bytes written beside them is shadow / camera setup);
/// * `3` - **rendezvous hold**, see the module note;
/// * `4` - the summon seat's `+0x1DA` and `+0x1DC` both `+= 1`
///   (`0x801F6D38`), after the arm draws the spell name out of
///   `0x800754C8[caster[+0x1DF]] + 8`;
/// * `7` - summon `+0x21C = 4`, then the same `+0x1DA` / `+0x1DC` bump;
/// * `8` - **rendezvous hold**;
/// * `9` - summon `+0x21C = 0xFF` (the fade-out), `ctx[+0x278] = 0`, and
///   `caster[+0x1DD] = 9`. That `9` is not an immediate: the store is
///   `sb s0,0x1dd(v0)` at `0x801F7058` and `s0` is still the **phase** the
///   prologue loaded at `0x801F6A64`. It happens to equal the enemy-row group
///   code the stager writes, which is what made the constant legible at all;
/// * `0x0B` - `+0x21C = 4` on every live enemy seat;
/// * `0x0D` - the damage sweep (below);
/// * `0x0E` - hold until the enemy row is idle, then write phase `0xFF`;
/// * `0xFF` - `ctx[+0x27A] = 0`, summon `+0x21C = 2`, return zero.
///
/// The damage sweep walks `actor_table[3..7]`, skips a dead or
/// `+0x16E & 4` seat, calls `FUN_801DD0AC(0x12, 7, seat)`, clamps **shape A**
/// (`sltu` against HP at `0x801F7638`, so it can kill), accumulates `+0x10`
/// with a **word** store, writes `+0x14C`, stages the three-leg reaction with
/// the *assigning* restage form, and finishes each seat with `+0x0C = 0x1000`
/// and `+0x21C = 0`. When exactly one seat was hittable it also writes the
/// roll to `0x8007BD14` and `0x78` to `0x8007B64C` - two battle globals the
/// engine has no mirror for, reported back as
/// [`ViguroSweep::single_target_roll`] rather than dropped.
///
/// `rolls` is called once per hittable seat, in seat order, so a host's RNG
/// cursor advances the way retail's does.
///
/// Wired: `World::run_cast_module_code`.
///
/// PORT: FUN_801F69F4 (PROT 0909 tick; phase chain + the enemy-row damage sweep, packet/camera arms unported)
pub fn viguro_tick(
    ctx: &mut CastModuleCtx,
    seats: &mut [CastActorState],
    caster_seat: u8,
    summon_seat: u8,
    settle: &mut i32,
    mut rolls: impl FnMut(u8) -> i32,
) -> (CastTickStep, ViguroSweep) {
    let mut sweep = ViguroSweep::default();
    let step = run_latched(ctx, |c| match c.phase {
        0 => {
            c.ctx_278 = VIGURO_ARM0_CTX_278;
            c.ctx_27a = VIGURO_ARM0_CTX_27A;
            CastArmStep::Advance
        }
        4 => {
            bump_stage(seats, summon_seat);
            CastArmStep::Advance
        }
        7 => {
            if let Some(s) = seats.get_mut(summon_seat as usize) {
                s.render_flag = VIGURO_ARM7_RENDER_FLAG;
            }
            bump_stage(seats, summon_seat);
            CastArmStep::Advance
        }
        9 => {
            if let Some(s) = seats.get_mut(summon_seat as usize) {
                s.render_flag = 0xFF;
            }
            if let Some(s) = seats.get_mut(caster_seat as usize) {
                // The reused-register `9`; see the doc comment.
                s.target_code = crate::cast_module_ticks::TARGET_CODE_ENEMY_ROW;
            }
            c.ctx_278 = 0;
            CastArmStep::Advance
        }
        0x0B => {
            for seat in FIRST_MONSTER_SEAT..ROW_SWEEP_SEAT_END {
                if let Some(s) = seats.get_mut(seat as usize)
                    && s.hp != 0
                {
                    s.render_flag = VIGURO_ARM11_ROW_RENDER_FLAG;
                }
            }
            CastArmStep::Advance
        }
        VIGURO_DAMAGE_PHASE => {
            let live = (FIRST_MONSTER_SEAT..ROW_SWEEP_SEAT_END)
                .filter(|s| {
                    seats
                        .get(*s as usize)
                        .map(row_seat_is_hittable)
                        .unwrap_or(false)
                })
                .count();
            for seat in FIRST_MONSTER_SEAT..ROW_SWEEP_SEAT_END {
                let Some(v) = seats.get_mut(seat as usize) else {
                    continue;
                };
                if !row_seat_is_hittable(v) {
                    continue;
                }
                let roll = rolls(seat);
                if live == 1 {
                    sweep.single_target_roll = Some(roll);
                }
                let applied = apply_hit_floor_zero(v, roll);
                stage_reaction_set(v);
                v.root_speed = VIGURO_HIT_ROOT_SPEED;
                v.render_flag = 0;
                sweep.hits.push(SweepHit { seat, applied });
            }
            // `sll v0,v0,0x7` on the scalar, stored to the countdown word
            // `0x801F8360` (`0x801F7798..0x801F77A8`).
            *settle = SETTLE_SCALAR << 7;
            CastArmStep::Advance
        }
        VIGURO_SETTLE_PHASE => {
            // The countdown drains first (`0x801F77C0..0x801F77F0`); the arm
            // writes `0xFF` only once the row is idle **and** the word is
            // spent (`bgtz` at `0x801F78BC`).
            let held = settle_countdown_holds(settle);
            if row_is_idle(seats, FIRST_MONSTER_SEAT, ROW_SWEEP_SEAT_END) && !held {
                c.phase = SERU_B_DONE_PHASE;
            }
            CastArmStep::Hold
        }
        SERU_B_DONE_PHASE => {
            c.ctx_27a = 0;
            if let Some(s) = seats.get_mut(summon_seat as usize) {
                s.render_flag = VIGURO_DONE_RENDER_FLAG;
            }
            CastArmStep::Finish
        }
        p if VIGURO_RENDEZVOUS_PHASES.contains(&p) => CastArmStep::Hold,
        // Named-but-presentation arms (1, 2, 5, 6, 0x0A, 0x0C) advance.
        1 | 2 | 5 | 6 | 0x0A | 0x0C => CastArmStep::Advance,
        // Retail returns busy for a phase the chain does not name; the port
        // reports Done so a host that holds on `busy` cannot park on a phase
        // nothing in the module can leave.
        _ => CastArmStep::Finish,
    });
    (step, sweep)
}

/// What PROT 0909's damage arm did this frame.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ViguroSweep {
    /// Per-seat outcome, in the sweep's own seat order.
    pub hits: Vec<SweepHit>,
    /// The roll the arm published to `0x8007BD14` because exactly one enemy
    /// seat was hittable (`bne s4,s5` at `0x801F75FC`, with `s5 = 1`). Retail
    /// also sets `0x8007B64C = 0x78` on that path.
    pub single_target_roll: Option<i32>,
}

/// The `+0x1DA` / `+0x1DC` pair bump both PROT 0909 arms `4` and `7` and PROT
/// 0912 arm `0x0A` perform on the summon seat.
pub(super) fn bump_stage(seats: &mut [CastActorState], seat: u8) {
    if let Some(s) = seats.get_mut(seat as usize) {
        s.staged_anim = s.staged_anim.wrapping_add(1);
        s.restage = s.restage.wrapping_add(1);
    }
}
