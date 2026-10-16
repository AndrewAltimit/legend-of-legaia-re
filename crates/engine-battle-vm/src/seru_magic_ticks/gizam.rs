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

/// The arm PROT 0906 bumps the summon seat's clip in (`0x801F6F4C`): the
/// chain sends phase `7` to `0x801F6E44` (`slti v0,v1,0x7` / `beq v0,zero`
/// at `0x801F6AE8`), and the bump sits past that arm's countdown gate.
pub const GIZAM_STAGE_ARM: u8 = 7;

/// The tint word arm 7 lights each living monster seat with
/// (`lui a1,0x2008; ori a1,a1,0x200` at `0x801F6F40`) - the target cursor's
/// bright word.
pub const GIZAM_ROW_LIT_WORD: u32 = 0x2008_0200;

/// The arm whose pass tints the monster row for the bubbles
/// (`0x801F72C4..0x801F72FC`).
pub const GIZAM_BUBBLE_ARM: u8 = 12;

/// The render flag arm 12 stores on every living monster seat (`li a1,0x4`
/// / `sb a1,0x21c(v1)`): the tint pass's blue arm.
pub const GIZAM_BUBBLE_RENDER_FLAG: u8 = 4;

/// The colour word each sweep hit stamps on its seat (`lui v0,0x3ff0` /
/// `sw v0,0x4(v1)` at `0x801F7440..0x801F7444`).
pub const GIZAM_HIT_PRESENT_WORD: u32 = 0x3FF0_0000;

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
/// that body on arm 2. Move-VM op `0x20` calls the stager with its arm
/// operand: record `0x801F7B80`, one of the three arm 4 spawns, calls arms
/// 0, 1 and 2 after `WAIT 95, 63, 30` / `WAIT 0` / `WAIT 63`. The engine runs
/// no op-`0x20` hook, so the director (`cast_module_camera::gizam_direct`)
/// holds phase 5 on that record's clock and the host fires stager arms 0
/// and 1 on it; once the clock runs out the body's phase 5 advances as a
/// no-op, which is stager arm 2's store.
///
/// This is also the module whose `0x801F6734` spawn stager is already ported
/// as `cast_module_ticks::gizam_stager` (`FUN_801F7740`). That is a different
/// routine in the same image; the arm here is the tick.
///
/// Arm 13 sweeps `actor_table[3 ..= 6]`, skipping a dead seat and a
/// [`FLAG_NON_TARGETABLE`] one. Per hit: `FUN_801DD0AC(0x12, 7, seat)` - the
/// `0x12` is in the `bne` delay slot at `0x801F7364` - the unsigned clamp
/// against `HP - 1`, `+0x10 +=`, `+0x14C -=`,
/// [`stage_reaction_assign`], `+0x04 =` [`GIZAM_HIT_PRESENT_WORD`], `+0x0C = 0x1000`, render
/// flag `= 0`, the facing write, and `+0x16E |=` [`GIZAM_STATUS_BIT`]. The
/// arm fires only once its `ctx+0x6D8` countdown goes negative, then hides
/// the summon seat and advances.
///
/// Other simulation state: `ctx+0x278 = 0` in arm 0 (`0x801F6BF0`), the
/// summon seat's render flag `= 0xFF` in arm 4, its clip pair `+= 1` in arm 7
/// with every living monster seat's render flag cleared and the seat lit
/// ([`GIZAM_ROW_LIT_WORD`]), the summon seat's `+0x04 = 0` on arm 13's exit,
/// arm 12's blue render flag on every living monster seat
/// ([`GIZAM_BUBBLE_RENDER_FLAG`]),
/// arm 14's row poll ahead of `0xFF`, and in arm `0xFF` its `+0x1DA = 0`
/// plus a render-flag clear over the three party seats.
///
/// The countdown gates and camera arms are the director's
/// (`cast_module_camera::gizam_direct`). Not ported: the packet arms and the
/// `+0x21B` / `+0x176` writes outside the mirror.
///
/// Wired: `World::run_cast_module_code`.
///
/// PORT: FUN_801F69F4 (PROT 0906; phase chain, arm 12's row tint, the monster-row sweep and arm 14's row poll; countdown and camera arms in gizam_direct)
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
            // `0x801F6F64..0x801F6F94`: every living seat `3..=6` back to
            // render flag `0` and lit with the cursor's bright tint word.
            // Both stores sit past the `beq` on the seat's HP at
            // `0x801F6F74` (its delay slot is a `nop`), so a dead seat keeps
            // its defeat-fade state and its spent colour word - the `+0x04 ==
            // 0` arm 14's row poll waits on.
            for seat in FIRST_MONSTER_SEAT..MONSTER_ROW_END {
                if let Some(v) = seats.get_mut(seat as usize)
                    && v.hp != 0
                {
                    v.render_flag = 0;
                    v.present_04 = GIZAM_ROW_LIT_WORD;
                }
            }
            CastArmStep::Advance
        }
        // Past its countdown gate, arm 12's pass ends on a loop over
        // `actor_table[3..7]` storing render flag `4` on each seat whose HP
        // `+0x14C` is non-zero (`0x801F72D4..0x801F72FC`): the row goes blue
        // under the bubbles until arm 13's hits clear it.
        GIZAM_BUBBLE_ARM => {
            for seat in FIRST_MONSTER_SEAT..MONSTER_ROW_END {
                if let Some(v) = seats.get_mut(seat as usize)
                    && v.hp != 0
                {
                    v.render_flag = GIZAM_BUBBLE_RENDER_FLAG;
                }
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
                if let Some(roll) = rolls(seat) {
                    let applied = site.apply(v, roll);
                    stage_reaction_assign(v);
                    v.flags |= GIZAM_STATUS_BIT;
                    hits.push(SweepHit { seat, applied });
                }
                // The hit's presentation, in the same live / targetable
                // branch as the damage but not part of it: `+0x04 =`
                // [`GIZAM_HIT_PRESENT_WORD`] (`lui v0,0x3ff0; sw v0,0x4(v1)`
                // at `0x801F7440`), `+0x0C = 0x1000`, `+0x21C = 0`. A host
                // that folds the damage elsewhere still needs it: the render
                // flag `0` is what takes the row off arm 12's blue and lets a
                // dying seat's colour word ease back before its defeat fade,
                // which arm 14 waits out.
                v.present_04 = GIZAM_HIT_PRESENT_WORD;
                v.root_speed = SERU_HIT_ROOT_SPEED;
                v.render_flag = 0;
            }
            if let Some(s) = seats.get_mut(who.summon as usize) {
                s.present_04 = 0;
                s.render_flag = GIZAM_HIDDEN_RENDER_FLAG;
            }
            CastArmStep::Advance
        }
        // The row poll at `0x801F7644..0x801F76DC`, behind the settle
        // word's drain (the director's, `gizam_direct`): every monster seat
        // `3 .. 3 + ctx[+0x01]` alive and back on idle (`+0x1D9 == 0`) or
        // dead with its prim word `+0x04` at zero, then `0xFF`.
        GIZAM_LAST_ARM => {
            let end = (FIRST_MONSTER_SEAT as usize + c.monster_count as usize)
                .min(MONSTER_ROW_END as usize);
            let settled = seats
                .get(FIRST_MONSTER_SEAT as usize..end.min(seats.len()))
                .unwrap_or(&[])
                .iter()
                .all(|v| {
                    if v.hp != 0 {
                        v.playing_anim == 0
                    } else {
                        v.present_04 == 0
                    }
                });
            if settled {
                c.phase = CHOREOGRAPHY_DONE_PHASE;
            }
            CastArmStep::Hold
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
