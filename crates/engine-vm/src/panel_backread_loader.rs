//! The slot-B **"FIELD BACK READ" staged-loader tick** (`FUN_801F6B24`,
//! PROT 0978 at the slot-B link base `0x801F69D8`, entry = file `+0x14C`).
//!
//! `FUN_80025358` state 2 calls this once per frame while the battle-end
//! reward path stages a background texture; the return value is the caller's
//! "still loading" flag.
//!
//! The body is read out of the statically extracted PROT 0978 image at its own
//! base. `scripts/ghidra-analysis/locate-entry-image.py` reports a stack-frame
//! prologue for this VA in `978/field_back_read` and in no other based image,
//! and the entry is the third word (`lui`/`lw` of the debug-print gate are
//! scheduled ahead of the `addiu sp,sp,-0x20`), which is why a naive
//! first-word prologue scan calls it a leaf.
//!
//! ## What the streamer does
//!
//! It reads a four-slice, `0xA000`-byte-per-slice texture into VRAM. Each
//! slice is uploaded through `FUN_800583C8` as a `0x140 x 0x40` rect at
//! `x = 0x180`, `y = 0 / 0x40 / 0x80 / 0xC0` - `0x140 * 0x40 * 2 == 0xA000`
//! exactly, so the four strips tile one `320 x 256` 16-bit region at VRAM
//! `x = 384`. The staging buffers alternate between `_DAT_8007B728` and
//! `_DAT_8007B72C`, both offset `+0x28000`.
//!
//! The dev source path is `h:\prot\field\other6\tim\int.tim` /
//! `tim_int2.tim`; the retail branch resolves PROT index `0x4C7 + variant`
//! through `FUN_8003E8A8` instead, with `variant` from
//! [`backread_texture_variant`].
//!
//! ## NOT WIRED
//!
//! The engine has no staged sub-overlay loader. `FUN_80025358`, the only
//! caller, is itself unported, and the engine's own asset path resolves PROT
//! entries synchronously rather than through a frame-sliced CD read - so there
//! is no host that would call this and nothing that owns `_DAT_8007B6C8`.

/// Phase counter the loader indexes on (`_DAT_8007B6C8`).
pub const BACKREAD_PHASE_GLOBAL: u32 = 0x8007_B6C8;

/// Arms in the primary dispatch table at `0x801F6AA8`.
pub const BACKREAD_PHASES: i32 = 12;

/// Bytes per staged slice.
pub const BACKREAD_SLICE_BYTES: u32 = 0xA000;

/// VRAM x of every uploaded strip.
pub const BACKREAD_RECT_X: i16 = 0x180;
/// VRAM width of every uploaded strip.
pub const BACKREAD_RECT_W: i16 = 0x140;
/// VRAM height of every uploaded strip.
pub const BACKREAD_RECT_H: i16 = 0x40;

/// PROT index the retail branch resolves: `0x4C7 + variant`.
pub const BACKREAD_PROT_BASE: u32 = 0x4C7;

/// Which of the two background textures this party state selects.
///
/// Retail computes `sltu (u16)_DAT_8008480E, (u16)_DAT_80084824 >> 1` - a
/// halfword of the first party record against half a halfword of a later one.
/// The comparison is unsigned and the shift is logical.
///
/// PORT: FUN_801f6b24 (`lhu 0x4824`/`lhu 0x480e`/`srl`/`sltu`)
pub fn backread_texture_variant(lhs: u16, rhs: u16) -> u32 {
    u32::from(lhs < (rhs >> 1))
}

/// VRAM `(x, y, w, h)` for slice `n`.
///
/// PORT: FUN_801f6b24 (the `0x801F735C..0x801F7362` rect stores, `y`
/// immediates `0` / `0x40` / `0x80` / `0xC0`)
pub fn backread_slice_rect(n: u32) -> (i16, i16, i16, i16) {
    (
        BACKREAD_RECT_X,
        (n as i16) * BACKREAD_RECT_H,
        BACKREAD_RECT_W,
        BACKREAD_RECT_H,
    )
}

/// What one tick of the loader does, by phase.
///
/// PORT: FUN_801f6b24 (the 12-entry jump table at `0x801F6AA8`)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackreadStep {
    /// Phases 0 and 1: the table's first two arms both jump straight to the
    /// `return 1` exit **without** advancing the counter, so the sequence only
    /// starts once something else moves the phase to 2.
    Stall,
    /// Even phases 2, 4, 6, 8: upload the previous slice (all but phase 2) and
    /// issue the next `0xA000` read.
    Transfer { slice: u32 },
    /// Odd phases 3, 5, 7, 9: poll `FUN_8003DE7C(1)`. The phase advances only
    /// when the poll reports the read complete.
    Poll,
    /// Phase 10: upload the last slice and advance.
    FinalUpload { slice: u32 },
    /// Phase 11: the terminal arm - it is the only one that returns `0`.
    Done,
    /// Phase >= 12: no arm; returns `1` and leaves the counter alone.
    OutOfRange,
}

impl BackreadStep {
    /// The step this phase selects.
    ///
    /// PORT: FUN_801f6b24
    pub fn for_phase(phase: i32) -> BackreadStep {
        match phase {
            0 | 1 => BackreadStep::Stall,
            2 => BackreadStep::Transfer { slice: 0 },
            4 => BackreadStep::Transfer { slice: 1 },
            6 => BackreadStep::Transfer { slice: 2 },
            8 => BackreadStep::Transfer { slice: 3 },
            3 | 5 | 7 | 9 => BackreadStep::Poll,
            10 => BackreadStep::FinalUpload { slice: 3 },
            11 => BackreadStep::Done,
            _ => BackreadStep::OutOfRange,
        }
    }
}

/// One tick of `FUN_801F6B24`, as `(next_phase, still_loading)`.
///
/// `poll_complete` answers `FUN_8003DE7C(1) == 0` for the odd phases; it is
/// ignored elsewhere. `still_loading` is the caller's return value: `1` on
/// every path but the terminal arm.
///
/// The alternate dispatch table taken when `_DAT_8007BAC0 == 0` (19 arms at
/// `0x801F6EF0`) is a second, unrelated streamer for a caller-supplied PROT
/// index and is **not** modelled here.
///
/// PORT: FUN_801f6b24
///
/// NOT WIRED: nothing hosts a staged sub-overlay load - `FUN_80025358`, the
/// only caller, is unported. See the module disclosure.
pub fn backread_tick(phase: i32, poll_complete: bool) -> (i32, bool) {
    match BackreadStep::for_phase(phase) {
        BackreadStep::Stall | BackreadStep::OutOfRange => (phase, true),
        BackreadStep::Poll => {
            if poll_complete {
                (phase + 1, true)
            } else {
                (phase, true)
            }
        }
        BackreadStep::Transfer { .. } | BackreadStep::FinalUpload { .. } => (phase + 1, true),
        BackreadStep::Done => (phase, false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_four_strips_tile_one_320x256_region() {
        let mut covered = 0i32;
        for n in 0..4 {
            let (x, y, w, h) = backread_slice_rect(n);
            assert_eq!(
                (x, w, h),
                (BACKREAD_RECT_X, BACKREAD_RECT_W, BACKREAD_RECT_H)
            );
            assert_eq!(y, covered as i16);
            covered += i32::from(h);
        }
        assert_eq!(covered, 256);
        assert_eq!(
            u32::from(BACKREAD_RECT_W as u16) * u32::from(BACKREAD_RECT_H as u16) * 2,
            BACKREAD_SLICE_BYTES,
            "one slice is exactly one 16-bit strip"
        );
    }

    #[test]
    fn variant_is_an_unsigned_compare_against_a_logical_halving() {
        assert_eq!(backread_texture_variant(0, 2), 1);
        assert_eq!(backread_texture_variant(1, 2), 0);
        // Unsigned: 0xFFFF >> 1 == 0x7FFF, and 0x8000 is NOT below it.
        assert_eq!(backread_texture_variant(0x8000, 0xFFFF), 0);
        assert_eq!(backread_texture_variant(0x7FFE, 0xFFFF), 1);
    }

    #[test]
    fn odd_phases_hold_until_the_poll_clears() {
        for phase in [3, 5, 7, 9] {
            assert_eq!(backread_tick(phase, false), (phase, true));
            assert_eq!(backread_tick(phase, true), (phase + 1, true));
        }
    }

    #[test]
    fn the_first_two_arms_never_advance_on_their_own() {
        for phase in [0, 1] {
            assert_eq!(backread_tick(phase, true), (phase, true));
        }
    }

    #[test]
    fn only_the_terminal_arm_reports_finished() {
        for phase in 0..BACKREAD_PHASES + 4 {
            let (_, still) = backread_tick(phase, true);
            assert_eq!(still, phase != 11, "phase {phase}");
        }
    }

    #[test]
    fn a_full_run_walks_phase_two_to_the_terminal_arm() {
        let mut phase = 2;
        for _ in 0..32 {
            let (next, still) = backread_tick(phase, true);
            phase = next;
            if !still {
                break;
            }
        }
        assert_eq!(phase, 11);
    }
}
