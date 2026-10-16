//! PROT 0903 Gimard (`0x81`): single-victim damage tick.

use super::*;

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

/// What PROT 0903's settle arm adds to the summon seat's `+0x1BA` shatter
/// word per engine tick: retail adds `2 * [0x1F800393] * [0x1F80037D]` a
/// pass (`mult` / `sll v1,t3,0x1` / `sh v0,0x1ba(s3)` at
/// `0x801F76A4..0x801F76C4`), and an engine tick is one vsync of that frame
/// product - twice the speed scalar.
pub const GIMARD_SHATTER_STEP_PER_TICK: u16 =
    2 * crate::cast_module_camera::MODULE_DRAIN_PER_TICK as u16;

/// The `ctx+0x278` PROT 0903 stores as its arm 4 cut lands
/// (`li v0,0x3; sb v0,0x278(v1)` at `0x801F6F24..0x801F6F2C`).
pub const GIMARD_CUT_CTX_278: u8 = 3;

/// The `ctx+0x278` PROT 0903 stores as its arm 6 seats the fire tunnel
/// (`li v0,0x2; sb v0,0x278(v1)` at `0x801F7168..0x801F7170`).
pub const GIMARD_TUNNEL_CTX_278: u8 = 2;

/// The render flag PROT 0903 poses the summon seat at in arm 9.
pub const GIMARD_POSE_RENDER_FLAG: u8 = 3;

/// The tint word PROT 0903 stamps on the summon seat in arm 9
/// (`li v0,0x3ff; sw v0,0x4(s3)` at `0x801F7318..0x801F731C`): the red
/// channel alone at full, which is what turns the creature red for its
/// breath. `gimard_burning_attack` reads it easing at `0x3FC`.
pub const GIMARD_POSE_PRESENT_WORD: u32 = 0x3FF;

/// The `+0x21F` selector PROT 0903 sets beside it (`sb t3,0x21f(s3)` at
/// `0x801F7324`, `t3 = 1` from the prologue).
pub const GIMARD_POSE_21F: u8 = 1;

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
/// * arms `4` / `6` - `ctx+0x278 =` [`GIMARD_CUT_CTX_278`] /
///   [`GIMARD_TUNNEL_CTX_278`]: above `1`, the battle ambient pass takes the
///   backdrop pair's depth cue to full and off the draw
///   (`battle_ground_grid::backdrop_cue_ceiling`), so the creature's attack
///   plays inside the module's fire tunnel with no stage behind it;
/// * arm `9` - the summon seat's render flag `= 3` (`0x801F7310`), its tint
///   word `+0x04 = 0x3FF` and `+0x21F = 1`;
/// * arm `10` - the summon seat's `+0x1DA` and `+0x1DC` each `+= 1`
///   (`0x801F7360..0x801F7374`);
/// * arm `11` - advances **first** (`0x801F7478`), then, unless the victim
///   carries [`FLAG_NON_TARGETABLE`], `FUN_801DD0AC(0x12, 7, victim_seat)` at
///   `0x801F74AC`, the unsigned clamp at `0x801F74D8`, `+0x10 +=`,
///   `+0x14C -=`, [`stage_reaction_bits`], then the victim's burn
///   presentation `+0x0C = 0x1000`, `+0x04 = 0x3FF`, `+0x21F = 1` and render
///   flag `= 0` (applied with or without a damage roll). Then
///   unconditionally the summon seat's render flag `= 2`, its `+0x1DD = 0`,
///   and the caster's render flag `= 0`;
/// * arm `12` - the victim's animation rate, and `0xFF` once the victim has
///   settled: a dead one faded to a zero `+0x04`, a living one back on idle
///   (the countdown ahead of it is the director's);
/// * arm `0xFF` - `return 0`.
///
/// Not ported: the packet arms (1..8 are almost entirely
/// `FUN_801D829C` / `FUN_80021B04` emission), the `FUN_8004E2F0` animation
/// poll arm 11 holds on, the camera magnitude `ctx+0x6D0` the landed hit
/// scales by `3/2`, and `ctx+0x243`.
///
/// Wired: `World::run_cast_module_code`.
///
/// PORT: FUN_801F69D8 (PROT 0903; phase chain + the single-target hit + the ctx+0x278 stores; packet and camera arms unported)
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
        4 => {
            c.ctx_278 = GIMARD_CUT_CTX_278;
            CastArmStep::Advance
        }
        6 => {
            c.ctx_278 = GIMARD_TUNNEL_CTX_278;
            CastArmStep::Advance
        }
        GIMARD_POSE_ARM => {
            if let Some(s) = seats.get_mut(who.summon as usize) {
                s.render_flag = GIMARD_POSE_RENDER_FLAG;
                s.present_04 = GIMARD_POSE_PRESENT_WORD;
                s.render_21f = GIMARD_POSE_21F;
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
            {
                if let Some(roll) = hit {
                    let applied = site.apply(v, roll);
                    stage_reaction_bits(v);
                    hits.push(SweepHit {
                        seat: who.victim,
                        applied,
                    });
                }
                // The victim's burn presentation, inside the same
                // targetable branch but not the damage's: `+0x21F = 1`,
                // node `+0x74 = 0xFF`, `+0x04 = 0x3FF`, `+0x0C = 0x1000`,
                // `+0x21C = 0` (`0x801F757C..0x801F759C`). A host that folds
                // the damage elsewhere still needs it - the render flag `0`
                // is what lets the victim's death fade run, which arm 12
                // waits out.
                v.root_speed = SERU_HIT_ROOT_SPEED;
                v.render_flag = 0;
                v.present_04 = GIMARD_POSE_PRESENT_WORD;
                v.render_21f = GIMARD_POSE_21F;
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
            // A dead victim settles once its defeat fade has walked the
            // render word `+0x04` to zero (`lhu 0x14c` / `lw v0,0x4(s2)` /
            // `bne v0,zero` at `0x801F765C..0x801F768C`), a living one once
            // it is back on idle (`+0x1D9 == 0`). The module's own countdown
            // gates the arm ahead of both (`gimard_direct`).
            let settled = seats.get(who.victim as usize).is_some_and(|v| {
                if v.hp == 0 {
                    v.present_04 == 0
                } else {
                    v.playing_anim == 0
                }
            });
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
