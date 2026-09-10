//! Battle **XA voice-stream selector** - which whole-clip voice stream, if
//! any, a battle action arms this frame.
//!
//! PORT: FUN_8004DA00
//!
//! NOT WIRED: the device half is not modelled. The starter this hands its
//! clip to, `FUN_8003EAE4`, cancels any in-flight read, positions the drive
//! on the clip file (`FUN_8005C160(2, slot, ..)` = `CdlSetloc` shape) and
//! issues CD command `0x15` (`li a0,0x15; jal 0x8005C034` at `0x8003EB68`,
//! `CdlSeekL`), then raises `gp+0x908` / `gp+0x910` and stores the slot at
//! `gp+0x890` - driver flags whose consumer (the CD-callback state machine
//! that would chain a `CdlReadS` once the seek lands) is not traced. So the
//! stream this arms is a drive-side sequence: a seek, then whatever the
//! driver does with those flags. The engine has no drive to seek and no
//! driver to poll them; its clips are pre-decoded
//! (`legaia_engine_audio::XaClipBank`) and start after a modelled response
//! delay. **The read chain is no longer untraced**: it is the eleven-state
//! callback ring `FUN_8003D764`, ported below as [`xa_transport_step`], whose
//! sole state writer is `FUN_8003D53C`. What that leaves is a transport the
//! engine has no hardware for, not an unknown - the clip it streams is a
//! whole `XA<n>` channel from the file start, not the cut one-shot the melee
//! kernel's `FUN_8003D53C` requests take.
//!
//! Retail reaches this pass through a **static actor template**
//! (`docs/reference/functions/runtime-libs.md`), not a call: the battle
//! scene-loader `FUN_800513F0` spawns the template at `0x800767F4` into the
//! system actor pool as its last act, and the pool walk then runs the
//! template's `+0x08` tick - this routine - once per frame for the whole
//! battle. That is why no `jal` in any image targets `0x8004DA00`; its single
//! reference on the disc is the template word.
//!
//! REF: FUN_8003EAE4 - the drive seek + driver-flag arm this hands the chosen
//! clip id to. This module is device-free and only decides.
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

// ---------------------------------------------------------------------------
// The CD-callback ring the armed clip runs on - `FUN_8003D764`
// ---------------------------------------------------------------------------

/// Number of states the callback's own bound admits
/// (`sltiu v0, v0, 0xb` at `0x8003D798`).
pub const XA_RING_STATES: u32 = 11;

/// Callback reason meaning "the last command completed"
/// (`li v0, 2; bne a0, v0` opening every arm).
pub const CD_REASON_COMPLETE: u8 = 2;

/// Callback reason meaning "disc error" - the only other value any arm tests.
pub const CD_REASON_DISC_ERROR: u8 = 5;

/// Status-byte bit state `6` waits for before it stops polling
/// (`andi v0, v0, 0x20` at `0x8003D91C`).
pub const CD_STATUS_READY_BIT: u8 = 0x20;

/// What one callback edge asks the transport to do.
///
/// Every variant that names a command corresponds to one `jal 0x8005C034`
/// site - retail's `CdControlF`-shape issuer, `a0` the command byte and `a1`
/// the parameter block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XaRingStep {
    /// Issue CD command `cmd`. `params` is which parameter block retail hands
    /// it: `None` for `a1 = 0`.
    Command {
        /// The `a0` command byte.
        cmd: u8,
        /// `Some(va)` when retail passes a parameter block, and the block's
        /// address so a reader can find it.
        params: Option<u32>,
    },
    /// State `6` / `8`'s poll retry: issue `CdlNop` and rewind the state.
    Poll,
    /// The clip reached its end LBA (state `8`) or the ready bit came up
    /// (state `6`): retail calls `FUN_8003EE7C` and then `CdlNop`.
    Finish {
        /// The `a0` retail hands `FUN_8003EE7C` - `0x75` from state `6`'s
        /// ready branch, `0` from state `8`'s end-of-clip branch.
        arg: u8,
    },
    /// State `10`: `FUN_8005BECC(0)` then `FUN_8003EE00` - the teardown.
    Teardown,
    /// A disc error: retail issues `CdlNop`, bumps the error counter at
    /// `gp+0x8E8`, and rewinds.
    Error,
    /// The callback reason was neither `2` nor `5`, or the state was past the
    /// bound - retail falls straight to its epilogue.
    Idle,
}

/// Retail's parameter blocks, kept as addresses because their contents are
/// the caller's: `0x8007BBF0` is the target MSF the seek and the `Setloc`
/// share, `0x8007BBC0` the mode / filter block whose `+1` byte state `3`
/// overwrites with `gp+0x954`.
pub const XA_RING_LOCATION_PARAMS: u32 = 0x8007_BBF0;
/// See [`XA_RING_LOCATION_PARAMS`].
pub const XA_RING_MODE_PARAMS: u32 = 0x8007_BBC0;

/// One edge of the CD-callback ring `FUN_8003D764`, the state machine that
/// actually streams the clip [`battle_voice_step`] arms.
///
/// This is the read chain this module's `NOT WIRED` note named as its
/// prerequisite. It is a **transport** sequence, and the port keeps it as a
/// decoded sequence rather than driving anything: the engine has no drive,
/// and its clips are already decoded ([`crate::XaClipBank`]). What the
/// decode buys is that the note no longer stands on "not traced", and that
/// the order of operations is checkable against a capture.
///
/// The eleven states, off the jump table at `0x800111C4` and each arm's own
/// `jal 0x8005C034`:
///
/// | state | on `reason == 2` | on `reason == 5` |
/// |---|---|---|
/// | `0` | `CdlSeekL` (`0x15`), location block | reset to `0` |
/// | `1` | `CdlSetmode` (`0x0E`), mode block; `gp+0x8A8 = 0xC8` | reset to `0` |
/// | `2` | `CdlSetloc` (`0x02`), location block | reset to `0` |
/// | `3` | `CdlSetfilter` (`0x0D`), mode block with `+1 = gp+0x954`; `gp+0x8A8 = 1` | reset to `0` |
/// | `4` | `CdlReadS` (`0x1B`) | reset to `0` |
/// | `5` | `CdlNop` (`0x01`) | reset to `0` |
/// | `6` | status `& 0x20` set: advance and finish with `0x75`; clear: `CdlNop` and **rewind two states** | reset to `0` |
/// | `7` | `CdlGetlocL` (`0x10`) | reset to `0` |
/// | `8` | position past `gp+0x974`: state `9` and finish with `0`; else `CdlNop` and **rewind one** | rewind one |
/// | `9` | `CdlPause` (`0x09`), then state `10` | (no reason test - the arm always runs) |
/// | `10` | teardown | (no reason test) |
///
/// States `9` and `10` are the two that do **not** test the reason byte at
/// all, so a disc error arriving in either is processed as a completion.
/// That asymmetry is retail's, not a simplification here.
///
/// `abort` is the head gate: `*(u16*)0x8007B876 & 1` at `0x8003D778` forces
/// the reason to [`CD_REASON_DISC_ERROR`] and clears `0x8007B874`.
///
/// Returns `(next_state, step)`. A `state` at or past [`XA_RING_STATES`]
/// returns the state unchanged and [`XaRingStep::Idle`], which is what
/// retail's failed `sltiu` does.
///
/// REPLACED-BY: `legaia_engine_audio::XaClipBank` - the engine decodes each
/// XA clip ahead of time and mixes it, so no host issues these commands; the
/// sequence is ported as a decoded state machine for parity checking, not as
/// a driver.
///
/// PORT: FUN_8003D764 (the eleven-state callback ring; the `CdControlF` issuer and the two callbacks stay device-side)
pub fn xa_transport_step(
    state: u32,
    reason: u8,
    abort: bool,
    status: u8,
    position_lba: i32,
    end_lba: i32,
) -> (u32, XaRingStep) {
    if state >= XA_RING_STATES {
        return (state, XaRingStep::Idle);
    }
    let reason = if abort { CD_REASON_DISC_ERROR } else { reason };
    let cmd = |c: u8, p: Option<u32>| XaRingStep::Command { cmd: c, params: p };

    // States 9 and 10 open with no reason test.
    match state {
        9 => return (10, cmd(0x09, None)),
        10 => return (10, XaRingStep::Teardown),
        _ => {}
    }
    if reason == CD_REASON_DISC_ERROR {
        // State 8 rewinds one; every other state resets to zero.
        let next = if state == 8 { 7 } else { 0 };
        return (next, XaRingStep::Error);
    }
    if reason != CD_REASON_COMPLETE {
        return (state, XaRingStep::Idle);
    }
    match state {
        0 => (1, cmd(0x15, Some(XA_RING_LOCATION_PARAMS))),
        1 => (2, cmd(0x0E, Some(XA_RING_MODE_PARAMS))),
        2 => (3, cmd(0x02, Some(XA_RING_LOCATION_PARAMS))),
        3 => (4, cmd(0x0D, Some(XA_RING_MODE_PARAMS))),
        4 => (5, cmd(0x1B, None)),
        5 => (6, cmd(0x01, None)),
        6 => {
            if status & CD_STATUS_READY_BIT != 0 {
                (7, XaRingStep::Finish { arg: 0x75 })
            } else {
                (4, XaRingStep::Poll)
            }
        }
        7 => (8, cmd(0x10, None)),
        _ => {
            if end_lba < position_lba {
                (9, XaRingStep::Finish { arg: 0 })
            } else {
                (7, XaRingStep::Poll)
            }
        }
    }
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

    fn step(state: u32, reason: u8) -> (u32, XaRingStep) {
        xa_transport_step(state, reason, false, 0, 0, 0)
    }

    /// The command order the ring issues for one clip, states `0..=5`.
    #[test]
    fn the_ring_issues_seek_mode_loc_filter_reads_nop_in_order() {
        let want = [0x15u8, 0x0E, 0x02, 0x0D, 0x1B, 0x01];
        let mut state = 0;
        for (i, cmd) in want.into_iter().enumerate() {
            let (next, act) = step(state, CD_REASON_COMPLETE);
            assert!(
                matches!(act, XaRingStep::Command { cmd: c, .. } if c == cmd),
                "state {i} issued {act:?}"
            );
            assert_eq!(next, state + 1);
            state = next;
        }
        assert_eq!(state, 6);
    }

    /// State 6 polls the status byte and rewinds **two** states until the
    /// ready bit comes up.
    #[test]
    fn state_six_rewinds_two_until_the_ready_bit() {
        let (next, act) = xa_transport_step(6, CD_REASON_COMPLETE, false, 0, 0, 0);
        assert_eq!((next, act), (4, XaRingStep::Poll));
        let (next, act) =
            xa_transport_step(6, CD_REASON_COMPLETE, false, CD_STATUS_READY_BIT, 0, 0);
        assert_eq!((next, act), (7, XaRingStep::Finish { arg: 0x75 }));
    }

    /// State 8 compares the decoded position against the end LBA and rewinds
    /// **one** while the clip is still running.
    #[test]
    fn state_eight_polls_the_position_against_the_end_lba() {
        let (next, act) = xa_transport_step(8, CD_REASON_COMPLETE, false, 0, 100, 200);
        assert_eq!((next, act), (7, XaRingStep::Poll));
        let (next, act) = xa_transport_step(8, CD_REASON_COMPLETE, false, 0, 201, 200);
        assert_eq!((next, act), (9, XaRingStep::Finish { arg: 0 }));
        // The comparison is `end < position`, so landing exactly on the end
        // LBA keeps polling.
        let (next, _) = xa_transport_step(8, CD_REASON_COMPLETE, false, 0, 200, 200);
        assert_eq!(next, 7);
    }

    /// A disc error resets to `0` everywhere except state `8`, which rewinds
    /// one - and states `9` / `10` never test the reason at all.
    #[test]
    fn the_error_leg_is_not_uniform() {
        for s in 0..8u32 {
            assert_eq!(step(s, CD_REASON_DISC_ERROR), (0, XaRingStep::Error));
        }
        assert_eq!(step(8, CD_REASON_DISC_ERROR), (7, XaRingStep::Error));
        // 9 and 10 process an error as if it were a completion.
        assert!(matches!(
            step(9, CD_REASON_DISC_ERROR),
            (10, XaRingStep::Command { cmd: 0x09, .. })
        ));
        assert_eq!(step(10, CD_REASON_DISC_ERROR), (10, XaRingStep::Teardown));
    }

    /// The head abort gate forces the error reason, and a state past the
    /// bound does nothing at all.
    #[test]
    fn the_abort_gate_and_the_bound() {
        assert_eq!(
            xa_transport_step(3, CD_REASON_COMPLETE, true, 0, 0, 0),
            (0, XaRingStep::Error)
        );
        assert_eq!(
            xa_transport_step(XA_RING_STATES, CD_REASON_COMPLETE, false, 0, 0, 0),
            (XA_RING_STATES, XaRingStep::Idle)
        );
        // A reason the arms do not test leaves the state alone.
        assert_eq!(step(2, 3), (2, XaRingStep::Idle));
    }
}
