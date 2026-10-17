//! PROT 0908 Zenoir (`0x86`): opener, finisher and splash tick.

use super::*;

// ---------------------------------------------------------------------------
// PROT 0908 - Zenoir (spell id 0x86)
// ---------------------------------------------------------------------------

/// PROT 0908's tick entry.
pub const ZENOIR_TICK: u32 = 0x801F_69D8;

/// The arm PROT 0908 latches the victim seat in (`sw v1, -0x7458(v0)` at
/// `0x801F6B74`, i.e. module word `0x801F8BA8`).
pub const ZENOIR_LATCH_ARM: u8 = 0;

/// The arm PROT 0908 shows the summon seat in (`0x801F7098`).
pub const ZENOIR_SHOW_ARM: u8 = 4;

/// The arm PROT 0908 stages the summon seat's first clip in (`0x801F723C`).
pub const ZENOIR_STAGE_ARM: u8 = 5;

/// The arm PROT 0908 stages the strike clip and poses the summon seat in,
/// once the first clip plays (`0x801F73C8`, `0x801F743C`).
pub const ZENOIR_POSE_ARM: u8 = 6;

/// The arm PROT 0908 shows the victim in as the strike clip commits
/// (`0x801F75E0`).
pub const ZENOIR_STRIKE_ARM: u8 = 7;

/// The neutral colour word `0x20080200` PROT 0908 writes to show a seat.
pub const ZENOIR_NEUTRAL_COLOUR: u32 = 0x2008_0200;

/// The arm PROT 0908 lifts the creature in and drops its clip back to `0`
/// (`0x801F787C`).
pub const ZENOIR_RISE_ARM: u8 = 9;

/// The arm PROT 0908's quarter-strength opening hit fires in.
pub const ZENOIR_OPENER_ARM: u8 = 8;

/// The arm PROT 0908 bumps the summon seat's clip in (`0x801F79EC`).
pub const ZENOIR_BUMP_ARM: u8 = 10;

/// The arm PROT 0908's finisher and its splash fire in.
pub const ZENOIR_FINISH_ARM: u8 = 11;

/// The last arm PROT 0908's chain names.
pub const ZENOIR_LAST_ARM: u8 = 12;

/// The render flag PROT 0908 poses the summon seat at in arm 5
/// (`li v0,0x7` at `0x801F7438`).
pub const ZENOIR_POSE_RENDER_FLAG: u8 = 7;

/// The render flag PROT 0908's finisher arm leaves the summon seat and a
/// surviving victim at (`li s0,0x2` at `0x801F7BB0`).
pub const ZENOIR_FINISH_RENDER_FLAG: u8 = 2;

/// The `ctx+0x278` value PROT 0908's finisher arm writes
/// (`li s0,0x2` at `0x801F7BB0`, stored at `0x801F7BF4`).
pub const ZENOIR_FINISH_CTX_278: u8 = 2;

/// PROT 0908 (Zenoir, spell id `0x86`) tick body - 6456 bytes, 1614
/// instructions, and the widest damage footprint in this set.
///
/// Thirteen phase arms `0..=12` plus `0xFF`, through the chain at
/// `0x801F6AA4..0x801F6B38`. `docs/subsystems/cast-module.md` records `0`
/// phase `+0x279` stores for this module; that census counted the
/// `sb rX, 0x279(rY)` displacement form, and this routine materialises
/// `s5 = ctx + 0x279` in its prologue instead - there are **six** phase
/// stores, at `0x801F6DEC`, `0x801F6E04`, `0x801F78E4`, `0x801F7A28`,
/// `0x801F7BE4` and `0x801F82A0`.
///
/// It also resolves its victim differently from its five siblings: arm 0
/// latches `caster[+0x1DD]` into the module word `0x801F8BA8`
/// (`0x801F6B74`), every later arm reads the victim seat back out of that
/// word, and arm `0xFF` writes it back onto `caster[+0x1DD]`
/// (`0x801F82D8`). So a retarget during the cast cannot move the hit.
///
/// Its three damage sites all call `FUN_801DD0AC` with `a1 = 7` and all
/// scale the wrapper's return before clamping - no other module in the band
/// does:
///
/// | site | arm | power | scale | clamp |
/// |---|---|---|---|---|
/// | `0x801F76CC` | 8 | `0x12` | `dmg / 4` | unsigned vs `HP - 1` |
/// | `0x801F7C14` | 11 | `0x10` | `dmg * 3 / 4` | unsigned vs HP |
/// | `0x801F7DF8` | 11 | `0x12` | `dmg * 2 / 3` | unsigned vs HP |
///
/// The third is a **splash**: it walks `actor_table[3 ..= 6]` skipping the
/// primary victim, a dead seat, one whose render flag is non-zero, and a
/// [`FLAG_NON_TARGETABLE`] one. Between the two loops each site keeps a
/// module-local four-entry accumulator (`0x801F6978` seats, `0x801F6980`
/// halfwords) that the HP-bar readout reads; that is presentation and is not
/// mirrored.
///
/// Other simulation state: `ctx+0x278 = 0` in arm 0 and `= 2` in arm 11, the
/// summon seat shown in arm 4, its first clip staged in arm 5, posed at
/// render flag `7` with a second `+0x1DA` bump in arm 6, its clip zeroed in
/// arm 9, both the caster's and the summon seat's `+0x1DD =`
/// [`TARGET_CODE_ENEMY_ROW`] in arm 8, and the clip pair bumped again in arm
/// 10.
///
/// The arm gates (countdowns, the creature's clip and reach, its height) and
/// the camera are the director's (`cast_module_camera::zenoir_direct`),
/// which the host runs first; this body runs on the pass it lets through.
/// Not ported: the packet arms, the damage-number accumulators, and
/// `+0x21B` / `+0x21F` / `+0x225` / `+0x176`.
///
/// Wired: `World::run_cast_module_code`.
///
/// PORT: FUN_801F69D8 (PROT 0908; phase chain + the three scaled damage sites; packet arms unported, gates and camera in zenoir_direct)
pub fn zenoir_tick(
    ctx: &mut CastModuleCtx,
    seats: &mut [CastActorState],
    who: SeruSeats,
    mut rolls: impl FnMut(u8) -> Option<i32>,
) -> (CastTickStep, Vec<SweepHit>) {
    let mut hits = Vec::new();
    let opener = SERU_HIT_SITES[3];
    let finisher = SERU_HIT_SITES[4];
    let splash = SERU_HIT_SITES[5];
    let step = run_chain(ctx, |c| match c.phase {
        ZENOIR_LATCH_ARM => {
            c.ctx_278 = 0;
            CastArmStep::Advance
        }
        ZENOIR_SHOW_ARM => {
            if let Some(s) = seats.get_mut(who.summon as usize) {
                s.render_flag = 0;
            }
            CastArmStep::Advance
        }
        // Arm 5 stages the creature's first clip (`+0x1DA += 1` at
        // `0x801F723C`); arm 6 - once that clip is playing - stages the
        // strike behind it and poses the seat (`0x801F73C8`, `0x801F743C`).
        ZENOIR_STAGE_ARM => {
            if let Some(s) = seats.get_mut(who.summon as usize) {
                s.staged_anim = s.staged_anim.wrapping_add(1);
            }
            CastArmStep::Advance
        }
        ZENOIR_POSE_ARM => {
            if let Some(s) = seats.get_mut(who.summon as usize) {
                s.staged_anim = s.staged_anim.wrapping_add(1);
                s.render_flag = ZENOIR_POSE_RENDER_FLAG;
            }
            CastArmStep::Advance
        }
        // Arm 7's pass shows the victim for the strike - `+0x04 =
        // 0x20080200`, `+0x21C = 0` (`0x801F75E0..0x801F7610`) - and sets
        // the creature's `+0x21F = 1`, which the mirror does not carry.
        ZENOIR_STRIKE_ARM => {
            if let Some(v) = seats.get_mut(who.victim as usize) {
                v.present_04 = ZENOIR_NEUTRAL_COLOUR;
                v.render_flag = 0;
            }
            CastArmStep::Advance
        }
        // `sb zero,0x1da(s6)` at `0x801F787C`: the creature drops back to
        // its first entry as it rises over the victim.
        ZENOIR_RISE_ARM => {
            if let Some(s) = seats.get_mut(who.summon as usize) {
                s.staged_anim = 0;
            }
            CastArmStep::Advance
        }
        ZENOIR_OPENER_ARM => {
            for slot in [who.caster, who.summon] {
                if let Some(s) = seats.get_mut(slot as usize) {
                    s.target_code = TARGET_CODE_ENEMY_ROW;
                }
            }
            if let Some(v) = seats.get_mut(who.victim as usize)
                && (v.flags & FLAG_NON_TARGETABLE) == 0
                && let Some(roll) = rolls(who.victim)
            {
                let applied = opener.apply(v, roll);
                stage_reaction_alt_only(v);
                v.root_speed = SERU_HIT_ROOT_SPEED;
                v.render_flag = 0;
                hits.push(SweepHit {
                    seat: who.victim,
                    applied,
                });
            }
            CastArmStep::Advance
        }
        ZENOIR_BUMP_ARM => {
            if let Some(s) = seats.get_mut(who.summon as usize) {
                s.restage = s.restage.wrapping_add(1);
                s.staged_anim = s.staged_anim.wrapping_add(1);
            }
            // The arm then shows every live monster within `0x400` of the
            // creature it has just moved over the victim
            // (`0x801F7A2C..0x801F7A9C`); the victim is always one of them.
            // The distance test needs seat positions the mirror does not
            // carry, so only the victim is shown here.
            if let Some(v) = seats.get_mut(who.victim as usize)
                && v.hp != 0
            {
                v.present_04 = ZENOIR_NEUTRAL_COLOUR;
                v.render_flag = 0;
            }
            CastArmStep::Advance
        }
        ZENOIR_FINISH_ARM => {
            if let Some(s) = seats.get_mut(who.summon as usize) {
                s.render_flag = ZENOIR_FINISH_RENDER_FLAG;
            }
            c.ctx_278 = ZENOIR_FINISH_CTX_278;
            if let Some(v) = seats.get_mut(who.victim as usize)
                && (v.flags & FLAG_NON_TARGETABLE) == 0
                && let Some(roll) = rolls(who.victim)
            {
                let applied = finisher.apply(v, roll);
                stage_reaction_assign(v);
                v.root_speed = SERU_HIT_ROOT_SPEED;
                v.render_flag = 0;
                if v.hp != 0 {
                    v.render_flag = ZENOIR_FINISH_RENDER_FLAG;
                }
                hits.push(SweepHit {
                    seat: who.victim,
                    applied,
                });
            }
            for seat in FIRST_MONSTER_SEAT..MONSTER_ROW_END {
                if seat == who.victim {
                    continue;
                }
                let Some(v) = seats.get_mut(seat as usize) else {
                    continue;
                };
                if v.hp == 0 || v.render_flag != 0 || (v.flags & FLAG_NON_TARGETABLE) != 0 {
                    continue;
                }
                let Some(roll) = rolls(seat) else {
                    continue;
                };
                let applied = splash.apply(v, roll);
                stage_reaction_assign(v);
                v.root_speed = SERU_HIT_ROOT_SPEED;
                v.render_flag = 0;
                hits.push(SweepHit { seat, applied });
            }
            CastArmStep::Advance
        }
        // Arm 12 leaves straight for `0xFF` (`sb v0,0x0(s5)` with `v0 =
        // 0xFF` at `0x801F829C`), not through `phase + 1`.
        ZENOIR_LAST_ARM => {
            c.phase = CHOREOGRAPHY_DONE_PHASE;
            CastArmStep::Hold
        }
        CHOREOGRAPHY_DONE_PHASE => {
            if let Some(c) = seats.get_mut(who.caster as usize) {
                c.target_code = who.victim;
            }
            CastArmStep::Finish
        }
        p if p <= ZENOIR_LAST_ARM => CastArmStep::Advance,
        _ => CastArmStep::Finish,
    });
    (step, hits)
}

/// The arm PROT 0908 runs its shatter loop in (`0x801F8108`), after the
/// finisher has killed what it kills.
pub const ZENOIR_SHATTER_ARM: u8 = 12;

/// The formation ids PROT 0908 never shatters - the three Berserkers
/// (`li v0,0x4d` / `0xad` / `0xae` at `0x801F821C..0x801F822C`), tested
/// against the formation's **first** cell `0x8007BD0C`.
pub const ZENOIR_SHATTER_SKIP_IDS: [u8; 3] = [0x4D, 0xAD, 0xAE];

/// The arm-12 countdown PROT 0908's finisher arm seeds into `ctx[+0x6D8]` on
/// its way out (`lbu v1,0x69(t0)` / `* 3 << 6` / `sh v0,0(s8)` at
/// `0x801F7BBC..0x801F7BD0`): `scalar * 192`, drained by the frame product
/// while arm 12 runs (`0x801F8268..0x801F82A0`).
pub const ZENOIR_SHATTER_COUNTDOWN_PER_SCALAR: i32 = 192;

/// What PROT 0908's shatter loop gates on, read once per pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ZenoirShatterGate {
    /// `ctx[+0x287]` - the scripted-fight flag.
    pub scripted: bool,
    /// The monster record `+0x20` byte of the **latched victim**: the loop
    /// reads `0x801C9348[victim - 3]` through a pointer it never steps
    /// (`t5`, set at `0x801F813C` from `s3`, the victim seat), so every seat
    /// is tested against the victim's record, not its own.
    pub victim_record_20: u8,
    /// The formation's first monster id (`0x8007BD0C`).
    pub formation_first: u8,
}

/// PROT 0908's shatter loop (`0x801F81C8..0x801F8264`), one pass: every
/// monster seat `3..=6` that is **dead** (`+0x14C == 0`) and still has a
/// colour word (`+0x04 != 0`, which the defeat fade drains to zero) takes
/// `step` onto its `+0x1BA` shatter word - unless the fight is scripted and
/// the victim's record `+0x20` is set (the boss stand-in), or the formation
/// opens with a Berserker. Retail's step is the frame product
/// `[0x1F800393] * [0x1F80037D]`; the loop runs on every pass of arm 12,
/// ahead of the arm's countdown gate. Returns how many seats it raised.
///
/// `neutral_colour(seat, view)` reports whether a seat's colour word is the
/// engine's resting `0` that stands for retail's neutral `0x20080200` (a
/// seated body that has not entered the defeat fade) - the gate reads that
/// as non-zero. An empty seat's word is a real zero.
///
/// REF: FUN_801F69D8 (PROT 0908's arm-12 shatter loop 0x801F81C8..0x801F8264 - one arm of the body [`zenoir_tick`] ports, run by the host on every pass of the arm)
pub fn zenoir_shatter_pass(
    seats: &mut [CastActorState],
    gate: ZenoirShatterGate,
    step: u16,
    neutral_colour: impl Fn(u8, &CastActorState) -> bool,
) -> usize {
    if gate.scripted && gate.victim_record_20 != 0 {
        return 0;
    }
    if ZENOIR_SHATTER_SKIP_IDS.contains(&gate.formation_first) {
        return 0;
    }
    let mut n = 0;
    for seat in FIRST_MONSTER_SEAT..MONSTER_ROW_END {
        let Some(v) = seats.get_mut(seat as usize) else {
            continue;
        };
        if v.hp != 0 || (v.present_04 == 0 && !neutral_colour(seat, v)) {
            continue;
        }
        v.shatter = v.shatter.wrapping_add(step);
        n += 1;
    }
    n
}

#[cfg(test)]
mod shatter_tests {
    use super::*;

    fn row() -> Vec<CastActorState> {
        let mut seats = vec![CastActorState::default(); 8];
        for s in &mut seats[3..7] {
            s.hp = 10;
            s.present_04 = 0x2008_0200;
        }
        seats
    }

    const OPEN: ZenoirShatterGate = ZenoirShatterGate {
        scripted: false,
        victim_record_20: 0,
        formation_first: 0x10,
    };

    #[test]
    fn dead_seats_with_colour_take_the_step() {
        let mut seats = row();
        seats[3].hp = 0;
        seats[5].hp = 0;
        seats[5].present_04 = 0; // faded out
        assert_eq!(zenoir_shatter_pass(&mut seats, OPEN, 32, |_, _| false), 1);
        assert_eq!(seats[3].shatter, 32);
        assert_eq!(seats[4].shatter, 0, "alive");
        assert_eq!(seats[5].shatter, 0, "colour word drained");
    }

    #[test]
    fn boss_stand_in_and_berserkers_skip() {
        let mut seats = row();
        seats[3].hp = 0;
        let boss = ZenoirShatterGate {
            scripted: true,
            victim_record_20: 1,
            ..OPEN
        };
        assert_eq!(zenoir_shatter_pass(&mut seats, boss, 32, |_, _| false), 0);
        // A scripted fight whose victim record byte is 0 (the lone Golem).
        let golem = ZenoirShatterGate {
            scripted: true,
            victim_record_20: 0,
            ..OPEN
        };
        assert_eq!(zenoir_shatter_pass(&mut seats, golem, 32, |_, _| false), 1);
        for id in ZENOIR_SHATTER_SKIP_IDS {
            let g = ZenoirShatterGate {
                formation_first: id,
                ..OPEN
            };
            assert_eq!(zenoir_shatter_pass(&mut seats, g, 32, |_, _| false), 0);
        }
    }
}
