//! PROT 0906 Gizam (`0x84`): monster-row sweep tick with a phase hole.

use super::*;

// ---------------------------------------------------------------------------
// PROT 0906 - Gizam (spell id 0x84)
// ---------------------------------------------------------------------------

/// PROT 0906's tick entry - `0x801F69F4`, **not** the load base. The base
/// holds the module's head table; the `0x801CF4EC` stub `0x801F1F6C` jumps
/// here.
pub const GIZAM_TICK: u32 = 0x801F_69F4;

/// The phase PROT 0906's chain names no arm for. See [`gizam_tick`].
pub const GIZAM_MISSING_ARM: u8 = 5;

/// The arm PROT 0906 hides the summon seat in (`li v0,0xff` at `0x801F6D30`).
pub const GIZAM_HIDE_ARM: u8 = 4;

/// The arm PROT 0906 bumps the summon seat's clip in (`0x801F6F4C`).
pub const GIZAM_STAGE_ARM: u8 = 8;

/// The arm PROT 0906's monster-row sweep fires in.
pub const GIZAM_SWEEP_ARM: u8 = 13;

/// The last arm PROT 0906's chain names.
pub const GIZAM_LAST_ARM: u8 = 14;

/// The render flag PROT 0906 hides the summon seat behind.
pub const GIZAM_HIDDEN_RENDER_FLAG: u8 = 0xFF;

/// The `+0x16E` bit PROT 0906's sweep sets on every seat it hits
/// (`ori v0, v0, 0x1; sh v0, 0x16e(v1)` at `0x801F749C`).
pub const GIZAM_STATUS_BIT: u16 = 0x0001;

/// PROT 0906 (Gizam, spell id `0x84`) tick body - 3404 bytes, 851
/// instructions.
///
/// Fifteen phase arms through the chain at `0x801F6AA0..0x801F6B50`, and the
/// chain has a **hole**: for `v1` in `4..=7` it tests `6`, then `>= 7`, then
/// `4`, and falls to the epilogue otherwise - so phase `5` reaches no arm.
/// Arm 4 nevertheless advances into it (`0x801F7194`), and arm 2 can skip it
/// (`ctx+0x6D8 != 0 ? phase += 1 : phase += 2` at `0x801F6CC0`), with arm 3
/// setting `phase = 4` outright. Retail parks at phase 5 returning busy, and
/// the escape is the **stager**'s: `FUN_801F7740` arm `2`
/// (`0x801F783C..0x801F7858`) is nothing but `ctx[+0x279] += 1`, and the
/// module's head table at `0x801F69D8` - seven words `0x801F7788`, `780C`,
/// `783C`, `785C`, `7920`, `7964`, `79A8`, read off the image - is what puts
/// that body on arm 2. The port's [`crate::cast_module_ticks::gizam_stager`]
/// already performs it; whether the band's arm cursor reaches 2 while the tick
/// is parked at 5 is the per-arm frame gating this set has never measured. **The port advances through phase 5** as a
/// no-op so the choreography reaches arm 13 - a deliberate divergence, taken
/// because holding there would strand the cast.
///
/// This is also the module whose `0x801F6734` spawn stager is already ported
/// as `cast_module_ticks::gizam_stager` (`FUN_801F7740`). That is a different
/// routine in the same image; the arm here is the tick.
///
/// Arm 13 sweeps `actor_table[3 ..= 6]`, skipping a dead seat and a
/// [`FLAG_NON_TARGETABLE`] one. Per hit: `FUN_801DD0AC(0x12, 7, seat)` - the
/// `0x12` is in the `bne` delay slot at `0x801F7364` - the unsigned clamp
/// against `HP - 1`, `+0x10 +=`, `+0x14C -=`,
/// [`stage_reaction_assign`], `+0x04 = 0x3FF0000`, `+0x0C = 0x1000`, render
/// flag `= 0`, the facing write, and `+0x16E |=` [`GIZAM_STATUS_BIT`]. The
/// arm fires only once its `ctx+0x6D8` countdown goes negative, then hides
/// the summon seat and advances.
///
/// Other simulation state: `ctx+0x278 = 0` in arm 0 (`0x801F6BF0`), the
/// summon seat's render flag `= 0xFF` in arm 4, its clip pair `+= 1` in arm 8
/// with the first monster seat's render flag cleared, and in arm `0xFF` its
/// `+0x1DA = 0` plus a render-flag clear over the three party seats.
///
/// Not ported: the packet and camera arms, the countdowns, and the
/// `+0x21B` / `+0x176` / `+0x04` writes outside the mirror.
///
/// Wired: `World::run_cast_module_code`.
///
/// PORT: FUN_801F69F4 (PROT 0906; phase chain + the monster-row sweep; packet and camera arms unported)
pub fn gizam_tick(
    ctx: &mut CastModuleCtx,
    seats: &mut [CastActorState],
    who: SeruSeats,
    mut rolls: impl FnMut(u8) -> Option<i32>,
) -> (CastTickStep, Vec<SweepHit>) {
    let mut hits = Vec::new();
    let site = SERU_HIT_SITES[2];
    let step = run_chain(ctx, |c| match c.phase {
        0 => {
            c.ctx_278 = 0;
            CastArmStep::Advance
        }
        GIZAM_HIDE_ARM => {
            if let Some(s) = seats.get_mut(who.summon as usize) {
                s.render_flag = GIZAM_HIDDEN_RENDER_FLAG;
            }
            CastArmStep::Advance
        }
        GIZAM_STAGE_ARM => {
            if let Some(s) = seats.get_mut(who.summon as usize) {
                s.staged_anim = s.staged_anim.wrapping_add(1);
                s.restage = s.restage.wrapping_add(1);
            }
            if let Some(v) = seats.get_mut(FIRST_MONSTER_SEAT as usize)
                && v.hp != 0
            {
                v.render_flag = 0;
            }
            CastArmStep::Advance
        }
        GIZAM_SWEEP_ARM => {
            for seat in FIRST_MONSTER_SEAT..MONSTER_ROW_END {
                let Some(v) = seats.get_mut(seat as usize) else {
                    continue;
                };
                if v.hp == 0 || (v.flags & FLAG_NON_TARGETABLE) != 0 {
                    continue;
                }
                let Some(roll) = rolls(seat) else {
                    continue;
                };
                let applied = site.apply(v, roll);
                stage_reaction_assign(v);
                v.root_speed = SERU_HIT_ROOT_SPEED;
                v.render_flag = 0;
                v.flags |= GIZAM_STATUS_BIT;
                hits.push(SweepHit { seat, applied });
            }
            if let Some(s) = seats.get_mut(who.summon as usize) {
                s.render_flag = GIZAM_HIDDEN_RENDER_FLAG;
            }
            CastArmStep::Advance
        }
        CHOREOGRAPHY_DONE_PHASE => {
            if let Some(s) = seats.get_mut(who.summon as usize) {
                s.staged_anim = 0;
            }
            for seat in 0..FIRST_MONSTER_SEAT {
                if let Some(v) = seats.get_mut(seat as usize) {
                    v.render_flag = 0;
                }
            }
            CastArmStep::Finish
        }
        p if p <= GIZAM_LAST_ARM => CastArmStep::Advance,
        _ => CastArmStep::Finish,
    });
    (step, hits)
}
