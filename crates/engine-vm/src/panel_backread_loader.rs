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
//! ## Which entries it streams
//!
//! The two textures are named, both on the disc and in the image's own dev
//! strings. `FUN_8003E8A8` is called with `0x4C7 + variant`, and that is a
//! **RAM TOC index**, so the CDNAME `+2` frame skew applies: the two
//! candidates are extraction entries **1221** and **1222**, in the `other6`
//! block (`#define other6 1222` -> extraction 1220, `#define other7 1228` ->
//! extraction 1226, so the block is 1220..=1225). Both entries are exactly
//! `0x28000` bytes - four `0xA000` slices, which is the whole staged read and
//! the staging buffer's `+0x28000` offset - and both open on raw 16-bit pixel
//! data with no TIM header, matching the raw-rect upload path.
//!
//! The image carries exactly two path strings, and they are the dev sources
//! for those two entries in order: `h:\prot\field\other6\tim\int.tim` and
//! `h:\prot\field\other6\tim\int2.tim`. (The second was previously written
//! here as `tim_int2.tim`, which is not a string in the image.)
//!
//! | `variant` | PROT index | extraction entry | dev source | when |
//! |---|---|---|---|---|
//! | `0` | `0x4C7` | 1221 | `int.tim` | slot 0's HP is at or above half |
//! | `1` | `0x4C8` | 1222 | `int2.tim` | slot 0's HP is below half |
//!
//! `variant` is [`backread_texture_variant`]'s `s0`, computed once at
//! `0x801F6B8C..0x801F6BAC`: `lhu v0,0x4824(v0)` / `srl v0,v0,0x1` /
//! `lhu v1,0x480e(v1)` / `sltu s0,v1,v0`. Those two globals are party slot 0's
//! record (`0x80084708`) at `+0x11C` (`hp_max_record`) and `+0x106`
//! (`hp_curr_live`), so the halved max is compared against live HP and the
//! damaged half of the party gets the second backdrop.
//!
//! ## Where each half lands in the port
//!
//! REF: FUN_8003E8A8 - the PROT-index loader the retail branch resolves
//! `0x4C7 + variant` through.
//! REF: FUN_80025358 - the only caller, itself unported.
//!
//! The loader does three things, and the port keeps two of them:
//!
//! - **The pick** ([`backread_texture_variant`]) is live. Every dome leg ends
//!   through `legaia_engine_core::world::World::exit_muscle_dome`, which runs
//!   it (via `legaia_engine_core::muscle_ringside::still_prot_index`) over the
//!   fighter's live HP and the lead record's `+0x11C` maximum and leaves the
//!   answer on `MinigameState::muscle_ringside_still` - the still a
//!   re-entered hub then shows. Both hosts reach it through their dome pad
//!   path (`World::tick_muscle_dome`'s Won / Lost arm).
//! - **The upload rectangles** ([`backread_slice_rect`]) are live.
//!   `legaia_engine_ui::ringside_backdrop::still_sheet_rgba` lays the entry's
//!   four bands down at exactly these rects, relative to `(384, 0)`, and both
//!   hosts build the still's sheet through it: the native window's
//!   `load_muscle_hub_assets` (baked into the hub atlas) and the play page's
//!   `play_mg_muscle_hub_sheet_rgba(8, variant)`.
//! - **The frame-sliced read schedule** ([`BackreadStep`],
//!   [`backread_tick`]) is replaced. Its twelve arms exist to overlap four
//!   20-sector `FUN_8003E800` reads with `FUN_8003DE7C` polls across frames;
//!   the port reads the whole `0x28000`-byte entry at once through the scene
//!   host's `ProtIndex`, so there is no read in flight to poll and nothing
//!   for a host to step.
//!
//! "The only caller" is measured, not assumed: a five-form reference scan
//! ([`docs/tooling/address-reference-scan.md`](../../../docs/tooling/address-reference-scan.md))
//! over `SCUS_942.54`, the based overlay images and the raw PROT entries finds
//! exactly one reference to `0x801F6B24` - the `jal` at `0x80025404`, inside
//! `FUN_80025358` - and no word, `j`, branch or `lui`+`addiu` form anywhere.
//!
//! The drawer is the contest hub's `FUN_801D00F8` (PROT `0977`, file
//! `+0x18E0`), whose still arm writes two `POLY_FT4` packets addressing
//! tpages `0x106` and `0x109` - VRAM `x = 384` and `576` as *page* indices,
//! which is why a search for the literal `0x180` never found the consumer.
//! It is ported as `legaia_engine_ui::ringside_backdrop::ringside_still_quads`
//! and drawn by both hosts over the re-entered hub; the level it is drawn at
//! is `legaia_engine_core::muscle_ringside::HubBackdrop`.
//! [`ringside-still.md`](../../../docs/formats/ringside-still.md) carries the
//! packets and the `_DAT_801D1AE0` re-entry latch that selects the arm.

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

/// Which of the two background textures this party state selects: `0` =
/// extraction entry 1221 (`int.tim`), `1` = 1222 (`int2.tim`). See the module
/// doc for the entry table.
///
/// Retail computes `sltu (u16)_DAT_8008480E, (u16)_DAT_80084824 >> 1`. Both
/// halfwords are in party slot 0's record at `0x80084708`: `+0x106`
/// (`hp_curr_live`) against `+0x11C` (`hp_max_record`) halved - so the
/// variant is "slot 0 is below half HP". The comparison is unsigned and the
/// shift is logical.
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
/// REPLACED-BY: the scene host's synchronous whole-entry read
/// (`legaia_engine_core::scene::ProtIndex::entry_bytes_extended`), which both
/// hosts' still-sheet builds call for extraction 1221 / 1222 - no sector read
/// is in flight for a phase to wait on.
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
    /// REPLACED-BY: the scene host's synchronous whole-entry read - see
    /// [`BackreadStep`].
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
/// REPLACED-BY: the scene host's synchronous whole-entry read
/// (`legaia_engine_core::scene::ProtIndex::entry_bytes_extended`) - the port
/// never splits the still into 20-sector reads, so there is no poll to step.
/// Retail's own call site is the `jal` at `0x80025404` in `FUN_80025358`.
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
