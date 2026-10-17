//! PROT 0907 Nighto (`0x85`): instant-death / confusion roll and tick.

use super::*;

// ---------------------------------------------------------------------------
// PROT 0907 - Nighto (spell id 0x85)
// ---------------------------------------------------------------------------

/// PROT 0907's tick entry - `0x801F69E8`, not the load base.
pub const NIGHTO_TICK: u32 = 0x801F_69E8;

/// The arm PROT 0907 stages the summon seat's first clip in (`0x801F7804`).
pub const NIGHTO_STAGE_ARM: u8 = 7;

/// The arm PROT 0907 bumps that clip in (`0x801F7A44`).
pub const NIGHTO_BUMP_ARM: u8 = 8;

/// The arm PROT 0907 shows the victim in (`0x801F7CFC`).
pub const NIGHTO_SHOW_ARM: u8 = 12;

/// The arm PROT 0907 forks kill-vs-confuse in.
pub const NIGHTO_FORK_ARM: u8 = 13;

/// The arm PROT 0907's kill outcome settles through.
pub const NIGHTO_KILL_SETTLE_ARM: u8 = 14;

/// The arm PROT 0907's confuse outcome lands in.
pub const NIGHTO_CONFUSE_ARM: u8 = 15;

/// The `+0x16E` bits PROT 0907 sets on a confused victim
/// (`ori v0, v0, 0x380` at `0x801F7F50`).
pub const NIGHTO_CONFUSE_BITS: u16 = 0x0380;

/// The neutral colour word `0x20080200` PROT 0907's arm 12 shows the victim
/// with.
pub const NIGHTO_NEUTRAL_COLOUR: u32 = 0x2008_0200;

/// The render flag / `+0x225` pair PROT 0907 writes on a killed victim
/// (`li v0,0x2` at `0x801F7E54`).
pub const NIGHTO_KILL_RENDER_FLAG: u8 = 2;

/// The render flag PROT 0907 writes on a confused victim
/// (`li v0,0x6` at `0x801F7E18`).
pub const NIGHTO_CONFUSE_RENDER_FLAG: u8 = 6;

/// The animation rate PROT 0907's `0xFF` arm leaves the victim at.
///
/// It is the `li v0, 0x2` in the dispatch chain's **branch delay slot** at
/// `0x801F6B44` - the value is set on the way to the arm, not inside it, so a
/// reader of the arm alone sees `sb v0, 0x21d(s4)` with no source.
pub const NIGHTO_EXIT_ANIM_RATE: u8 = 2;

/// The modulus PROT 0907's arm-0 kill roll takes
/// (`rand() % 8` at `0x801F6B50..0x801F6B7C`, stored to the module word
/// `0x801F8534`). A remainder of `0` is the instant death; anything else is
/// the confuse branch.
pub const NIGHTO_KILL_MODULUS: u32 = 8;

/// The base of PROT 0907's resist modulus (`li v1, 0x13` at `0x801F6C70`);
/// the divisor is `NIGHTO_RESIST_BASE - magic_level`, so a higher magic level
/// narrows the range the throw has to land under.
pub const NIGHTO_RESIST_BASE: u32 = 0x13;

/// PROT 0907's resist threshold (`slti v1, v1, 0x9` at `0x801F6CA8`): a
/// remainder of `9` or more sets the module word `0x801F853C` and the spell
/// does nothing.
pub const NIGHTO_RESIST_THRESHOLD: u32 = 9;

/// The modulus the extra roll only one character index takes
/// (`rand() % 3 == 0` at `0x801F6CF0..0x801F6D28`).
pub const NIGHTO_EXTRA_MODULUS: u32 = 3;

/// The character index (`0x8007BD10[caster_seat]`, 1-based) that takes that
/// extra roll (`li v0, 0x3; bne v1, v0` at `0x801F6CE4`).
pub const NIGHTO_EXTRA_ROLL_CHARACTER: u8 = 3;

/// The inputs PROT 0907's arm 0 draws before the fork ever runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NightoRoll {
    /// `FUN_80056798()` at `0x801F6B50` - `% 8` picks kill over confuse.
    pub kill_roll: u32,
    /// `FUN_80056798()` at `0x801F6C28` - the resist throw.
    pub resist_roll: u32,
    /// `record[+0x729 + slot]`, found the same way
    /// [`VeraRestore::magic_level`] is found.
    pub magic_level: u8,
    /// `ctx[+0x287] != 0 && monster_record[+0x20] != 0`, where the record is
    /// `0x801C9348[victim_seat - 3]`. A monster whose record carries that byte
    /// is immune to the whole spell, and nothing else in the band reads it.
    pub target_immune: bool,
    /// `FUN_80056798()` at `0x801F6CF0`, drawn **only** when the caster's
    /// character index is [`NIGHTO_EXTRA_ROLL_CHARACTER`].
    pub extra_roll: Option<u32>,
}

/// Which way PROT 0907's arm-0 rolls send arm 13.
///
/// Retail's two divides are signed (`div` + `mfhi`), and the SCUS RNG
/// `FUN_80056798` returns a non-negative word, so an unsigned remainder is
/// the same value. The resist divisor is clamped to `1` here because a magic
/// level of `0x13` would divide by zero - retail would take the `break 0x1c00`
/// trap, which no reachable magic level triggers.
pub fn nighto_outcome(roll: &NightoRoll) -> NightoOutcome {
    let mut resisted = if roll.target_immune {
        true
    } else {
        let modulus = NIGHTO_RESIST_BASE
            .saturating_sub(u32::from(roll.magic_level))
            .max(1);
        roll.resist_roll % modulus >= NIGHTO_RESIST_THRESHOLD
    };
    if let Some(extra) = roll.extra_roll
        && extra.is_multiple_of(NIGHTO_EXTRA_MODULUS)
    {
        resisted = true;
    }
    // Retail's arm 0 parks BOTH words unconditionally, and arm 13 forks on
    // the kill word alone - so the resist is a second, independent axis, not
    // a third outcome that replaces the fork.
    let deadly = roll.kill_roll.is_multiple_of(NIGHTO_KILL_MODULUS);
    match (deadly, resisted) {
        (true, false) => NightoOutcome::Kill,
        (true, true) => NightoOutcome::KillResisted,
        (false, false) => NightoOutcome::Confuse,
        (false, true) => NightoOutcome::ConfuseResisted,
    }
}

/// Which way PROT 0907's arm-13 fork went, and whether the resist word
/// suppressed its writes.
///
/// Two independent bits, not three outcomes. The `beqz` at `0x801F7E04`
/// branches on the **kill** word `0x801F8534` alone, and that is what decides
/// which phase the cast leaves arm 13 on. The resist word `0x801F853C` is
/// tested separately *inside* each leg (`0x801F7E0C` on the confuse leg,
/// `0x801F7E48` on the death leg, `0x801F7F38` again in arm 15) and only
/// suppresses the victim writes - it never redirects the phase. A cast that
/// rolled a non-zero kill word and resisted therefore still ends on arm 15
/// with nothing written, which is what the capture shows (`13 -> 15` twice).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NightoOutcome {
    /// `0x801F8534 == 0`, `0x801F853C == 0`: the death leg, and it writes -
    /// HP zeroed, `+0x21C` / `+0x225` set to `2`, `+0x16E` cleared.
    Kill,
    /// `0x801F8534 == 0`, `0x801F853C != 0`: the death leg with its writes
    /// skipped (`bnez` at `0x801F7E50`). The phase still advances to
    /// [`NIGHTO_KILL_SETTLE_ARM`].
    KillResisted,
    /// `0x801F8534 != 0`, `0x801F853C == 0`: the confuse leg - `+0x21C = 6`
    /// here, the `+0x16E` bits in arm 15.
    Confuse,
    /// `0x801F8534 != 0`, `0x801F853C != 0`: the confuse leg with both of its
    /// victim writes skipped. Still [`NIGHTO_CONFUSE_ARM`].
    ConfuseResisted,
}

impl NightoOutcome {
    /// Did the kill word read zero? This is the arm-13 fork, and nothing
    /// else.
    pub fn deadly(self) -> bool {
        matches!(self, Self::Kill | Self::KillResisted)
    }

    /// Did the resist word read non-zero? This suppresses the victim writes
    /// on whichever leg the fork took.
    pub fn resisted(self) -> bool {
        matches!(self, Self::KillResisted | Self::ConfuseResisted)
    }
}

/// PROT 0907 (Nighto, spell id `0x85`) tick body - 5568 bytes, 1392
/// instructions.
///
/// Sixteen phase arms `0..=15` plus `0xFF`, through the chain at
/// `0x801F6A88..0x801F6B48`. It holds **no** damage-wrapper call: the spell's
/// SCUS description ("kill or confuse enemy") is literally what arm 13
/// implements.
///
/// ```text
/// arm 13: if (module[0x801F8534] == 0)                ; 0x801F7E04, the fork
///             if (module[0x801F853C] == 0)            ; 0x801F7E50
///                 victim[+0x14C] = 0                  ; 0x801F7E58
///                 victim[+0x21C] = victim[+0x225] = 2
///                 victim[+0x16E] = 0
///             phase += 1                              ; 0x801F7E88 -> arm 14
///         else
///             if (module[0x801F853C] == 0)            ; 0x801F7E14
///                 victim[+0x21C] = 6
///             phase = 15                              ; 0x801F7E28, UNCONDITIONAL
/// arm 15: victim[+0x21C] = 0                          ; 0x801F7F34
///         if (module[0x801F853C] == 0)                ; 0x801F7F40
///             victim[+0x16E] |= 0x380                 ; 0x801F7F50
///         phase = 0xFF
/// ```
///
/// The fork is the **kill** word; the resist word only suppresses writes. An
/// earlier port read a resisted cast as "advance to arm 14" for both legs,
/// which a capture refuted (`13 -> 15` twice on a resisted Nighto).
///
/// Both module words are written by arm 0, so the outcome is decided the
/// frame the cast starts, not the frame it lands: `0x801F8534 = rand() % 8`
/// at `0x801F6B7C` and `0x801F853C` from the resist throw at
/// `0x801F6C28..0x801F6CC4` - see [`nighto_outcome`], which carries the
/// arithmetic.
///
/// Other simulation state: `ctx+0x278 = 0` in arm 0 (`0x801F7054`), the
/// summon seat's `+0x1DA = 1` in arm 7 and its clip pair `+= 1` in arm 8, the
/// victim's render flag `= 0` and animation rate `= 1` in arm 12, and the
/// victim's animation rate `=` [`NIGHTO_EXIT_ANIM_RATE`] in arm `0xFF`.
///
/// The arm gates (the `ctx+0x6D8` countdowns, the creature's clip) and the
/// camera are the director's (`cast_module_camera::nighto_direct`), which
/// the host runs first; `settle_spent` is that countdown read back for arm
/// 14, whose own hold this body keeps. Not ported: the packet arms and the
/// summon seat's `+0x21B` / `+0x176` clears.
///
/// Wired: `World::run_cast_module_code`.
///
/// PORT: FUN_801F69E8 (PROT 0907; phase chain + the instant-death / confuse fork + arm 14's fade hold; packet arms unported, gates and camera in nighto_direct)
pub fn nighto_tick(
    ctx: &mut CastModuleCtx,
    seats: &mut [CastActorState],
    who: SeruSeats,
    outcome: NightoOutcome,
    settle_spent: bool,
) -> CastTickStep {
    run_chain(ctx, |c| match c.phase {
        0 => {
            c.ctx_278 = 0;
            CastArmStep::Advance
        }
        NIGHTO_STAGE_ARM => {
            if let Some(s) = seats.get_mut(who.summon as usize) {
                s.staged_anim = 1;
            }
            CastArmStep::Advance
        }
        NIGHTO_BUMP_ARM => {
            if let Some(s) = seats.get_mut(who.summon as usize) {
                s.staged_anim = s.staged_anim.wrapping_add(1);
                s.restage = s.restage.wrapping_add(1);
            }
            CastArmStep::Advance
        }
        NIGHTO_SHOW_ARM => {
            if let Some(v) = seats.get_mut(who.victim as usize) {
                v.render_flag = 0;
                // `sw v1,0x4(s4)` with `v1 = 0x20080200` at `0x801F7D00`:
                // the colour word the kill leg's settle waits to see faded.
                v.present_04 = NIGHTO_NEUTRAL_COLOUR;
                v.anim_rate = 1;
            }
            CastArmStep::Advance
        }
        NIGHTO_FORK_ARM => {
            let Some(v) = seats.get_mut(who.victim as usize) else {
                return CastArmStep::Advance;
            };
            v.anim_rate = 1;
            if outcome.deadly() {
                if !outcome.resisted() {
                    v.hp = 0;
                    // One `li v0, 0x2` feeds both stores at `0x801F7E54`.
                    v.render_flag = NIGHTO_KILL_RENDER_FLAG;
                    v.render_225 = NIGHTO_KILL_RENDER_FLAG;
                    v.flags = 0;
                }
                CastArmStep::Advance
            } else {
                if !outcome.resisted() {
                    v.render_flag = NIGHTO_CONFUSE_RENDER_FLAG;
                }
                // `sb v0,0x279(s5)` at `0x801F7E28` is outside the resist
                // test, so a resisted confuse leg lands on 15 all the same.
                c.phase = NIGHTO_CONFUSE_ARM;
                CastArmStep::Hold
            }
        }
        // Arm 14 (`0x801F7E90..0x801F7EF0`): a victim whose colour word
        // has faded to `0` ends the cast; one still showing holds - for
        // good while the kill went through (the death fade owns the
        // release), and until the leg's countdown runs out when resisted.
        NIGHTO_KILL_SETTLE_ARM => {
            let showing = seats.get_mut(who.victim as usize).is_some_and(|v| {
                v.anim_rate = 1;
                v.present_04 != 0
            });
            if showing && (!outcome.resisted() || !settle_spent) {
                return CastArmStep::Hold;
            }
            c.phase = CHOREOGRAPHY_DONE_PHASE;
            CastArmStep::Hold
        }
        NIGHTO_CONFUSE_ARM => {
            if let Some(v) = seats.get_mut(who.victim as usize) {
                v.anim_rate = 1;
                v.render_flag = 0;
                // Arm 15's own gate is the resist word alone (`bnez` at
                // `0x801F7F40`), not "was this a confuse".
                if !outcome.resisted() {
                    v.flags |= NIGHTO_CONFUSE_BITS;
                }
            }
            c.phase = CHOREOGRAPHY_DONE_PHASE;
            CastArmStep::Hold
        }
        CHOREOGRAPHY_DONE_PHASE => {
            if let Some(v) = seats.get_mut(who.victim as usize) {
                v.anim_rate = NIGHTO_EXIT_ANIM_RATE;
            }
            CastArmStep::Finish
        }
        p if p <= NIGHTO_CONFUSE_ARM => CastArmStep::Advance,
        _ => CastArmStep::Finish,
    })
}
