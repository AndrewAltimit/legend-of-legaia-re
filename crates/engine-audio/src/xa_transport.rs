//! The CD-callback ring an armed XA voice clip runs on - `FUN_8003D764`.
//!
//! `battle_voice`'s `NOT WIRED` note used to rest on "the CD-callback state
//! machine that would chain a `CdlReadS` once the seek lands is not traced".
//! This is that machine, decoded off `ghidra/scripts/funcs/8003d764.txt` and
//! the state table at `0x800111C4` in `extracted/SCUS_942.54`.
//!
//! **No `PORT:` marker, deliberately.**
//! `scripts/ci/port-catalog-ignore.toml` files `8003d764` under
//! `[cd_transport_shims]` because every one of its states is a drive command,
//! and the engine has no drive: it decodes each XA clip ahead of time and
//! mixes it ([`crate::XaClipBank`]). What this module adds to that scope row
//! is the *shape* of the walk - the two rewind distances and the two arms
//! that skip the callback-reason test - none of which the row carries and all
//! of which a capture can be checked against.
//!
//! REF: FUN_8003D764 (the eleven-state ring), FUN_8003D53C (its sole state writer)

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
