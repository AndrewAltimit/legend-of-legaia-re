//! Four-symbol **code-lock** actor - the field puzzle where a fixed five-press
//! sequence opens a door.
//!
//! `FUN_801EED58` (field overlay PROT 0897, base `0x801CE818`) is an ordinary
//! actor handler: a five-case jump table on the actor's phase halfword
//! `+0x54`, with the per-phase dwell counter in `+0x9C` and the shared
//! "hand back to the caller" epilogue every sibling in that band uses. It
//! always finishes by ticking the text-actor list (`FUN_80031D00`), so the
//! dialogue box keeps animating while the puzzle is up.
//!
//! ## The press mask is face buttons, not the d-pad
//!
//! Phase 1 tests the just-pressed mask `_DAT_8007B874` against `0xF0` and
//! then four single bits. That mask is the packed layout
//! `~((pad[2] << 8) | pad[3]) & 0xFFFF` built by `FUN_8001822C`, so the low
//! byte is libpad's face/shoulder byte and the d-pad lives at
//! `0x1000`/`0x2000`/`0x4000`/`0x8000` (see
//! `docs/subsystems/boot.md`). Bits `0x10`/`0x20`/`0x40`/`0x80` are therefore
//! Triangle / Circle / Cross / Square, and they map to stored symbols
//! `3` / `1` / `0` / `2`.
//!
//! The four tests are sequential `if`s over one latched mask rather than a
//! chain of `else if`s, so a frame that latches several bits stores the
//! **last** one tested - the priority order is Circle, Cross, Square,
//! Triangle, with Triangle winning. Only one symbol is stored per frame
//! either way, and the index still advances exactly once.
//!
//! ## Phases
//!
//! | phase | body |
//! |---|---|
//! | `0` | open the prompt window, reset the entry index, advance |
//! | `1` | one symbol per pad edge into `ctx[+0x54 + index]`, cue `0x36`; advance after the fifth |
//! | `2` | dwell `0x0C` frames |
//! | `3` | compare the five entered symbols against the descriptor, cue `0x25` + `FUN_8003CE08(9)` on a match, `0x23` + `FUN_8003CE34(9)` otherwise |
//! | `4` | dwell `0x14` frames, then release: `ctx[+0x2E] = -1`, `ctx[+0x40] = actor[+0x50]`, `actor[+0x50] = 0x1A`, phase `0` |
//!
//! The target code is **not** in the actor: it is the five bytes at `+1..+5`
//! of the field descriptor `_DAT_8007B450`, i.e. inline operand bytes of the
//! scene's field-VM script.
//!
//! `see ghidra/scripts/funcs/801eed58.txt`

/// Face-button bits inside the packed just-pressed mask, and the symbol each
/// one stores. Listed in the order the retail body tests them.
pub const SYMBOL_FOR_BIT: [(u16, u8); 4] = [(0x20, 1), (0x40, 0), (0x80, 2), (0x10, 3)];

/// The mask phase 1 requires before it looks at any individual bit.
pub const ANY_SYMBOL_MASK: u16 = 0xF0;

/// Number of symbols in a code.
pub const CODE_LEN: usize = 5;

/// SFX cue for one accepted press.
pub const CUE_PRESS: u8 = 0x36;
/// SFX cue for a correct code.
pub const CUE_SUCCESS: u8 = 0x25;
/// SFX cue for a wrong code.
pub const CUE_FAILURE: u8 = 0x23;

/// Dwell frames between the fifth press and the comparison (phase 2).
pub const DWELL_BEFORE_COMPARE: i16 = 0x0C;
/// Dwell frames between the comparison and the release (phase 4).
pub const DWELL_BEFORE_RELEASE: i16 = 0x14;

/// The sub-handler id the actor parks in when it releases (`actor[+0x50]`).
pub const RELEASE_HANDLER: u16 = 0x1A;

/// Which symbol a just-pressed mask stores, or `None` when no face bit is
/// latched. Mirrors the sequential-`if` precedence of the retail body.
pub fn symbol_for_mask(pressed: u16) -> Option<u8> {
    if pressed & ANY_SYMBOL_MASK == 0 {
        return None;
    }
    let mut chosen = None;
    for (bit, symbol) in SYMBOL_FOR_BIT {
        if pressed & bit != 0 {
            chosen = Some(symbol);
        }
    }
    chosen
}

/// One frame's observable output, for a host that owns the audio and the
/// window rather than having the state machine reach for them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CodeLockFrame {
    /// Open the prompt window this frame (phase 0).
    pub open_window: bool,
    /// SFX cue to play, if any.
    pub cue: Option<u8>,
    /// The comparison verdict, emitted on the frame phase 3 runs.
    pub verdict: Option<bool>,
    /// The actor released control this frame (phase 4 completing).
    pub released: bool,
}

/// The code-lock actor's own state - the three fields retail keeps in the
/// actor record plus the entry buffer it writes into the scene context.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeLockActor {
    /// Phase halfword `actor[+0x54]`.
    pub phase: u16,
    /// Dwell counter `actor[+0x9C]`.
    pub dwell: i16,
    /// Entry cursor `_DAT_8007BB88`, `0..=CODE_LEN`.
    pub index: usize,
    /// The symbols entered so far - retail's `ctx[+0x54..+0x59]`.
    pub entered: [u8; CODE_LEN],
}

impl Default for CodeLockActor {
    fn default() -> Self {
        Self {
            phase: 0,
            dwell: 0,
            index: 0,
            entered: [0; CODE_LEN],
        }
    }
}

impl CodeLockActor {
    /// Fresh actor, parked at phase `0`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Advance one frame.
    ///
    /// `pressed` is the just-pressed mask `_DAT_8007B874`; `locked` is the
    /// busy gate `_DAT_8007BB80 != 0` (input is ignored while it holds);
    /// `frame_delta` is `DAT_1F800393`, the per-frame tick the dwell counters
    /// accumulate; `code` is the five descriptor bytes.
    ///
    /// PORT: FUN_801eed58
    ///
    /// NOT WIRED: the engine has no field-VM binding that installs this
    /// sub-handler yet - the scene descriptor byte that selects it is read by
    /// the op-`0x49` handler table, which the port dispatches through
    /// `crate::field` without a code-lock arm.
    pub fn tick(
        &mut self,
        pressed: u16,
        locked: bool,
        frame_delta: i16,
        code: &[u8; CODE_LEN],
    ) -> CodeLockFrame {
        let mut out = CodeLockFrame::default();
        match self.phase {
            0 => {
                out.open_window = true;
                self.index = 0;
                self.phase += 1;
            }
            1 => {
                if locked {
                    return out;
                }
                let Some(symbol) = symbol_for_mask(pressed) else {
                    return out;
                };
                if self.index < CODE_LEN {
                    self.entered[self.index] = symbol;
                }
                out.cue = Some(CUE_PRESS);
                self.index += 1;
                if self.index >= CODE_LEN {
                    self.dwell = 0;
                    self.phase += 1;
                }
            }
            2 => {
                self.dwell = self.dwell.wrapping_add(frame_delta);
                if self.dwell < DWELL_BEFORE_COMPARE {
                    return out;
                }
                self.dwell = 0;
                self.phase += 1;
            }
            3 => {
                let ok = self.entered == *code;
                out.verdict = Some(ok);
                out.cue = Some(if ok { CUE_SUCCESS } else { CUE_FAILURE });
                self.phase += 1;
            }
            4 => {
                self.dwell = self.dwell.wrapping_add(frame_delta);
                if self.dwell < DWELL_BEFORE_RELEASE {
                    return out;
                }
                self.dwell = 0;
                out.released = true;
                self.phase = 0;
            }
            _ => {}
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CODE: [u8; CODE_LEN] = [1, 0, 2, 3, 1];

    fn press(sym_bit: u16) -> u16 {
        sym_bit
    }

    #[test]
    fn face_bits_map_to_symbols() {
        assert_eq!(symbol_for_mask(0x20), Some(1));
        assert_eq!(symbol_for_mask(0x40), Some(0));
        assert_eq!(symbol_for_mask(0x80), Some(2));
        assert_eq!(symbol_for_mask(0x10), Some(3));
    }

    #[test]
    fn dpad_bits_are_not_symbols() {
        for bit in [0x1000u16, 0x2000, 0x4000, 0x8000] {
            assert_eq!(symbol_for_mask(bit), None);
        }
    }

    #[test]
    fn simultaneous_bits_take_the_last_test() {
        // Circle + Triangle: Triangle is tested last and wins.
        assert_eq!(symbol_for_mask(0x20 | 0x10), Some(3));
        // Circle + Cross: Cross is tested after Circle.
        assert_eq!(symbol_for_mask(0x20 | 0x40), Some(0));
    }

    #[test]
    fn phase_zero_opens_the_window_and_resets() {
        let mut a = CodeLockActor::new();
        a.index = 3;
        let f = a.tick(0, false, 1, &CODE);
        assert!(f.open_window);
        assert_eq!(a.index, 0);
        assert_eq!(a.phase, 1);
    }

    #[test]
    fn lock_suppresses_entry() {
        let mut a = CodeLockActor::new();
        a.tick(0, false, 1, &CODE);
        let f = a.tick(press(0x20), true, 1, &CODE);
        assert_eq!(f.cue, None);
        assert_eq!(a.index, 0);
    }

    #[test]
    fn correct_code_reports_success_then_releases() {
        let mut a = CodeLockActor::new();
        a.tick(0, false, 1, &CODE);
        for sym in CODE {
            let bit = SYMBOL_FOR_BIT.iter().find(|(_, s)| *s == sym).unwrap().0;
            let f = a.tick(press(bit), false, 1, &CODE);
            assert_eq!(f.cue, Some(CUE_PRESS));
        }
        assert_eq!(a.phase, 2);
        // Dwell 0x0C frames at one tick each.
        for _ in 0..DWELL_BEFORE_COMPARE {
            a.tick(0, false, 1, &CODE);
        }
        assert_eq!(a.phase, 3);
        let f = a.tick(0, false, 1, &CODE);
        assert_eq!(f.verdict, Some(true));
        assert_eq!(f.cue, Some(CUE_SUCCESS));
        for _ in 0..DWELL_BEFORE_RELEASE - 1 {
            assert!(!a.tick(0, false, 1, &CODE).released);
        }
        assert!(a.tick(0, false, 1, &CODE).released);
        assert_eq!(a.phase, 0);
    }

    #[test]
    fn wrong_code_reports_failure() {
        let mut a = CodeLockActor::new();
        a.tick(0, false, 1, &CODE);
        for _ in 0..CODE_LEN {
            a.tick(press(0x40), false, 1, &CODE); // all zeros
        }
        for _ in 0..DWELL_BEFORE_COMPARE {
            a.tick(0, false, 1, &CODE);
        }
        let f = a.tick(0, false, 1, &CODE);
        assert_eq!(f.verdict, Some(false));
        assert_eq!(f.cue, Some(CUE_FAILURE));
    }

    #[test]
    fn dwell_accumulates_by_the_frame_delta() {
        let mut a = CodeLockActor::new();
        a.phase = 2;
        // A delta of 4 reaches 0x0C in three frames, not twelve.
        a.tick(0, false, 4, &CODE);
        a.tick(0, false, 4, &CODE);
        assert_eq!(a.phase, 2);
        a.tick(0, false, 4, &CODE);
        assert_eq!(a.phase, 3);
    }
}
