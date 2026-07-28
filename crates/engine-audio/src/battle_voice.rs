//! Battle **XA voice-stream selector** - which whole-clip voice stream, if
//! any, a battle action arms this frame.
//!
//! PORT: FUN_8004DA00
//!
//! NOT WIRED: no consumer yet. The engine's battle scene host does not run a
//! per-frame voice pass, so nothing calls [`battle_voice_step`]. What it
//! wants is the live battle context's four bytes plus the acting actor's
//! action pair, which live in `legaia-engine-core`'s battle state; wiring is a
//! call site there, not a change here.
//!
//! Retail reaches this pass through a **static actor template**
//! (`docs/reference/functions/runtime-libs.md`), not a call: the battle
//! scene-loader `FUN_800513F0` spawns the template at `0x800767F4` into the
//! system actor pool as its last act, and the pool walk then runs the
//! template's `+0x08` tick - this routine - once per frame for the whole
//! battle. That is why no `jal` in any image targets `0x8004DA00`; its single
//! reference on the disc is the template word.
//!
//! REF: FUN_8003EAE4 - the whole-clip stream starter this hands the chosen
//! clip id to. The engine equivalent is the host's XA clip player; this module
//! is device-free and only decides.
//!
//! REF: FUN_800513F0 - the battle scene loader that spawns the template.
//!
//! # What it decides
//!
//! A stream is armed at most once per action. Four gates have to pass, the
//! acting seat then selects a party slot (or a monster), and the action's
//! **class** byte picks the clip. The full table lives in
//! `docs/reference/functions/battle.md`; the shape that matters here is that
//! the routine has three distinct outcomes, and the retail code treats the
//! difference between two of them as load-bearing:
//!
//! * [`BattleVoiceStep::Arm`] - start the clip and latch its id.
//! * [`BattleVoiceStep::ClearLatch`] - the three "not ready yet" gates
//!   (`ctx[+0x26B]`, `_DAT_8007BD71`, `ctx[+0x276]`) each fall through to the
//!   latch store with `-1` in hand, so the frames *between* actions are what
//!   re-arms the pass.
//! * [`BattleVoiceStep::Hold`] - the remaining exits (`ctx[+0x7] == 0x5A`, a
//!   latch that is already set, and a class with no voice) branch **past** the
//!   store and leave the latch alone.
//!
//! Source: `ghidra/scripts/funcs/8004da00.txt` (disassembly).

/// The latch value meaning "no clip is playing" (`_DAT_8007BDB0 == -1`).
pub const NO_CLIP: i32 = -1;

/// Battle-context phase byte that suppresses the pass without clearing the
/// latch (`ctx[+0x7] == 0x5A`).
pub const PHASE_SUPPRESS: u8 = 0x5A;

/// Party-slot fanfare clip base: slot `n` streams `XA(0x19 + n + 1)`.
pub const FANFARE_CLIP_BASE: u8 = 0x19;

/// Fallback clip for a spell whose spell-table class byte is `>= 0x14`.
pub const SPELL_FALLBACK_CLIP: u8 = 7;

/// Spell-table class byte below which a spell uses the caster's own fanfare
/// clip instead of the shared fallback.
pub const SPELL_FANFARE_CLASS_LIMIT: u8 = 0x14;

/// Seats `0..SEAT_PARTY_COUNT` are party members; higher seats are monsters.
pub const SEAT_PARTY_COUNT: u8 = 3;

/// What one tick of the selector resolves to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BattleVoiceStep {
    /// Start `clip` through the whole-clip stream player and latch its id.
    Arm { clip: u8 },
    /// Reset the latch to [`NO_CLIP`] - the pass is idle and re-armable.
    ClearLatch,
    /// Do nothing at all, latch included.
    Hold,
}

/// The battle-context bytes the pass reads.
///
/// Field names follow what the bytes do; the offsets they come from are in
/// the doc comments so a capture can be checked against them.
#[derive(Debug, Clone, Copy, Default)]
pub struct BattleVoiceCtx {
    /// `ctx[+0x26B]` - non-zero suppresses and clears.
    pub suppress: u8,
    /// `ctx[+0x276]` - zero suppresses and clears.
    pub action_live: u8,
    /// `ctx[+0x7]` - [`PHASE_SUPPRESS`] suppresses without clearing.
    pub phase: u8,
    /// `ctx[+0x274]` - the acting seat.
    pub seat: u8,
}

/// The acting actor's two action bytes.
#[derive(Debug, Clone, Copy, Default)]
pub struct BattleVoiceAction {
    /// `actor[+0x1DE]` - action class.
    pub class: u8,
    /// `actor[+0x1DF]` - action id (a spell id for class 2).
    pub id: u8,
}

/// The disc-sourced tables the selector indexes.
///
/// All four are slices rather than fixed arrays so a caller can hand over
/// exactly what it parsed; an index past the end resolves to
/// [`BattleVoiceStep::Hold`] rather than panicking, which is the safe reading
/// of a table the port has not loaded.
#[derive(Debug, Clone, Copy, Default)]
pub struct BattleVoiceTables<'a> {
    /// `DAT_8007BD10` - seat -> party slot (1-based; slot `0` means absent).
    pub party_order: &'a [u8],
    /// `DAT_8007BD09` - seat -> monster voice index.
    pub monster_index: &'a [u8],
    /// `0x800787AF` - monster voice index -> clip id.
    pub monster_clips: &'a [u8],
    /// `DAT_800754C8` leading byte per spell id - the spell class.
    pub spell_class: &'a [u8],
}

/// One tick of `FUN_8004DA00`'s decision half.
///
/// `cd_busy` is `_DAT_8007BC20 != 0`, `stream_gate` is `_DAT_8007BD71` (which
/// retail requires to be `0xFF`), and `latch` is `_DAT_8007BDB0`.
pub fn battle_voice_step(
    cd_busy: bool,
    stream_gate: u8,
    latch: i32,
    ctx: BattleVoiceCtx,
    action: BattleVoiceAction,
    tables: BattleVoiceTables<'_>,
) -> BattleVoiceStep {
    // The three gates that reset the latch. `cd_busy` short-circuits into the
    // same store (retail jumps straight to it with -1 already loaded).
    if cd_busy || ctx.suppress != 0 || stream_gate != 0xFF || ctx.action_live == 0 {
        return BattleVoiceStep::ClearLatch;
    }
    // The two that leave it alone.
    if ctx.phase == PHASE_SUPPRESS || latch != NO_CLIP {
        return BattleVoiceStep::Hold;
    }

    let seat = ctx.seat;
    if seat >= SEAT_PARTY_COUNT {
        // The monster arm never reads the action class at all.
        let Some(&index) = tables.monster_index.get(seat as usize) else {
            return BattleVoiceStep::Hold;
        };
        return match tables.monster_clips.get(index as usize) {
            Some(&clip) => BattleVoiceStep::Arm { clip },
            None => BattleVoiceStep::Hold,
        };
    }

    let Some(&slot) = tables.party_order.get(seat as usize) else {
        return BattleVoiceStep::Hold;
    };

    let clip = match action.class {
        1 => slot.wrapping_add(FANFARE_CLIP_BASE),
        2 => {
            let class = tables.spell_class.get(action.id as usize).copied();
            match class {
                Some(c) if c < SPELL_FANFARE_CLASS_LIMIT => slot.wrapping_add(FANFARE_CLIP_BASE),
                Some(_) => SPELL_FALLBACK_CLIP,
                // An unloaded spell table is not evidence of either arm.
                None => return BattleVoiceStep::Hold,
            }
        }
        // Retail computes `(slot - 1) * 2` in a 32-bit register, so an absent
        // seat (slot 0) produces a negative clip id there and a wrapped one
        // here. Both are nonsense; the seat is never absent when a class-3/4
        // action is resolving.
        3 | 4 => slot.wrapping_sub(1).wrapping_mul(2),
        // Class 0 and anything >= 5 have no voice.
        _ => return BattleVoiceStep::Hold,
    };
    BattleVoiceStep::Arm { clip }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PARTY_ORDER: [u8; 6] = [1, 2, 3, 0, 0, 0];
    const MONSTER_INDEX: [u8; 6] = [0, 0, 0, 4, 5, 6];
    const MONSTER_CLIPS: [u8; 8] = [0, 1, 2, 3, 0x08, 0x09, 0x0A, 0x0B];
    // Spell id 0x81 is a low-class (fanfare) spell, 0x82 a high-class one.
    fn voice_spell_classes() -> Vec<u8> {
        let mut v = vec![0x40u8; 0x100];
        v[0x81] = 0x02;
        v[0x82] = 0x20;
        v
    }

    fn voice_tables(spells: &[u8]) -> BattleVoiceTables<'_> {
        BattleVoiceTables {
            party_order: &PARTY_ORDER,
            monster_index: &MONSTER_INDEX,
            monster_clips: &MONSTER_CLIPS,
            spell_class: spells,
        }
    }

    fn ready_ctx(seat: u8) -> BattleVoiceCtx {
        BattleVoiceCtx {
            suppress: 0,
            action_live: 1,
            phase: 0,
            seat,
        }
    }

    // Helper names in a `#[cfg(test)]` module are still caller nodes in the
    // port catalog's call graph, so a common one (`step`, `tables`, `new`)
    // resolves onto this file by name and reports the module live. Keep them
    // distinctive.
    fn voice_step(ctx: BattleVoiceCtx, action: BattleVoiceAction, latch: i32) -> BattleVoiceStep {
        let spells = voice_spell_classes();
        battle_voice_step(false, 0xFF, latch, ctx, action, voice_tables(&spells))
    }

    #[test]
    fn not_ready_gates_clear_the_latch() {
        let action = BattleVoiceAction { class: 1, id: 0 };
        let mut busy = ready_ctx(0);
        busy.suppress = 1;
        assert_eq!(voice_step(busy, action, 0x1A), BattleVoiceStep::ClearLatch);

        let mut idle = ready_ctx(0);
        idle.action_live = 0;
        assert_eq!(voice_step(idle, action, 0x1A), BattleVoiceStep::ClearLatch);

        let spells = voice_spell_classes();
        assert_eq!(
            battle_voice_step(
                false,
                0x00,
                0x1A,
                ready_ctx(0),
                action,
                voice_tables(&spells)
            ),
            BattleVoiceStep::ClearLatch,
        );
        assert_eq!(
            battle_voice_step(
                true,
                0xFF,
                0x1A,
                ready_ctx(0),
                action,
                voice_tables(&spells)
            ),
            BattleVoiceStep::ClearLatch,
        );
    }

    #[test]
    fn suppress_phase_and_live_latch_hold_instead_of_clearing() {
        let action = BattleVoiceAction { class: 1, id: 0 };
        let mut phased = ready_ctx(0);
        phased.phase = PHASE_SUPPRESS;
        assert_eq!(voice_step(phased, action, NO_CLIP), BattleVoiceStep::Hold);
        // Already latched: nothing happens, and the latch is not reset.
        assert_eq!(
            voice_step(ready_ctx(0), action, 0x1A),
            BattleVoiceStep::Hold
        );
    }

    #[test]
    fn class_one_arms_the_seats_fanfare_clip() {
        // Seats 0/1/2 -> party slots 1/2/3 -> XA27/XA28/XA29 (0x1A..0x1C).
        for (seat, want) in [(0u8, 0x1Au8), (1, 0x1B), (2, 0x1C)] {
            let got = voice_step(
                ready_ctx(seat),
                BattleVoiceAction { class: 1, id: 0 },
                NO_CLIP,
            );
            assert_eq!(got, BattleVoiceStep::Arm { clip: want }, "seat {seat}");
        }
    }

    #[test]
    fn class_two_splits_on_the_spell_class_byte() {
        let low = voice_step(
            ready_ctx(1),
            BattleVoiceAction { class: 2, id: 0x81 },
            NO_CLIP,
        );
        assert_eq!(low, BattleVoiceStep::Arm { clip: 0x1B });
        let high = voice_step(
            ready_ctx(1),
            BattleVoiceAction { class: 2, id: 0x82 },
            NO_CLIP,
        );
        assert_eq!(
            high,
            BattleVoiceStep::Arm {
                clip: SPELL_FALLBACK_CLIP
            }
        );
    }

    #[test]
    fn classes_three_and_four_use_the_long_bank() {
        // Party slots 1/2/3 -> (slot - 1) * 2 = XA1 / XA3 / XA5.
        for (seat, want) in [(0u8, 0u8), (1, 2), (2, 4)] {
            for class in [3u8, 4] {
                let got = voice_step(ready_ctx(seat), BattleVoiceAction { class, id: 0 }, NO_CLIP);
                assert_eq!(got, BattleVoiceStep::Arm { clip: want }, "seat {seat}");
            }
        }
    }

    #[test]
    fn voiceless_classes_hold() {
        for class in [0u8, 5, 6, 0xFF] {
            let got = voice_step(ready_ctx(0), BattleVoiceAction { class, id: 0 }, NO_CLIP);
            assert_eq!(got, BattleVoiceStep::Hold, "class {class}");
        }
    }

    #[test]
    fn monster_seats_ignore_the_action_class() {
        // Seat 3 -> monster index 4 -> clip 0x08, whatever the class byte is.
        for class in [0u8, 1, 5, 0xFF] {
            let got = voice_step(ready_ctx(3), BattleVoiceAction { class, id: 0 }, NO_CLIP);
            assert_eq!(got, BattleVoiceStep::Arm { clip: 0x08 }, "class {class}");
        }
    }

    #[test]
    fn missing_tables_hold_rather_than_guess() {
        let empty: [u8; 0] = [];
        let spells = voice_spell_classes();
        let mut t = voice_tables(&spells);
        t.party_order = &empty;
        assert_eq!(
            battle_voice_step(
                false,
                0xFF,
                NO_CLIP,
                ready_ctx(0),
                BattleVoiceAction { class: 1, id: 0 },
                t,
            ),
            BattleVoiceStep::Hold,
        );

        let mut t = voice_tables(&empty);
        t.party_order = &PARTY_ORDER;
        assert_eq!(
            battle_voice_step(
                false,
                0xFF,
                NO_CLIP,
                ready_ctx(0),
                BattleVoiceAction { class: 2, id: 0x81 },
                t,
            ),
            BattleVoiceStep::Hold,
        );
    }
}
