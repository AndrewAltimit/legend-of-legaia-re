//! Global runtime CLUT ramp installed at PSX VRAM `(fb_x=0..240, fb_y=479)`.
//!
//! Row 479 of the retail Legaia framebuffer carries a 15-slot palette
//! ramp that the runtime stages in RAM at `0x800F19xx` and DMAs to VRAM
//! during early init. The ramp's 15 CLUTs traverse a hue wheel in 5-bit
//! BGR555 space at smoothly descending peak intensity (20, 19, 17, 16,
//! 15, 13, 12, 11, 9, 8, 6, 5, 4, 2, 1). Field / town NPC TMDs reference
//! the lower half of this ramp via CBA `0x77C8..0x77CF` (slots 8..15);
//! sprite / effect prims sample the upper half.
//!
//! ## Why this matters
//!
//! Town01's NPC TMDs reference CBA cells that fall on row 479 slots
//! 8..14. The 32-byte CLUT payloads at those slots **do not appear as
//! static data anywhere in the extracted disc** - a brute-force scan
//! of every PROT entry and `SCUS_942.54` returns zero matches for the
//! 32-byte slot 8 sequence. The bytes are produced at runtime by code
//! that lives in `SCUS_942.54` or one of the RAM overlays (the static
//! analysis sweep across every imported program finds no LUI+ADDIU
//! load/store landing in the `0x800F1800..0x800F1B00` window, which
//! means the writer accesses the buffer through an indirect base
//! pointer rather than a direct absolute address; pinning the function
//! is left for future work). Without those CLUT rows the engine's
//! targeted-upload pass drops every textured prim that samples them as
//! `MissingClut`.
//!
//! This module captures the bytes verbatim from retail VRAM and exposes
//! a single `apply_global_hue_ramp` helper so the engine's scene-build
//! pre-pass can paint them into its software VRAM. The capture is
//! corroborated across every retail save state we have access to:
//! row 479 fb_x=0..240 is bit-identical in every non-battle frame, and
//! the only saves that differ are battle scenes where the row is
//! repurposed by the battle overlay.
//!
//! When the runtime generator is reverse-engineered, the same surface
//! can be backed by an algorithmic implementation - the bytes themselves
//! stay the canonical fixture.

use legaia_tim::Vram;

/// VRAM row holding the global hue ramp.
pub const ROW_Y: u16 = 479;
/// First populated slot's pixel column.
pub const FIRST_SLOT_X: u16 = 0;
/// Number of 16-pixel slots in the ramp (slots 0..14).
pub const SLOT_COUNT: usize = 15;
/// Width of one CLUT slot in 16bpp halfwords (= 16 BGR555 entries).
pub const SLOT_WIDTH_PX: u16 = 16;
/// Bytes per slot.
pub const SLOT_BYTES: usize = 32;

/// Observed in PSX VRAM at `fb_y=479, fb_x=0..240` (slots 0..14) across
/// every non-battle retail save state in the corpus. Slot 15 (fb_x=240)
/// is intentionally left zeroed by the runtime.
///
/// Decoded peak values per slot (5-bit BGR555 max channel):
/// `[20, 19, 17, 16, 15, 13, 12, 11, 9, 8, 6, 5, 4, 2, 1]`.
pub const GLOBAL_HUE_RAMP_ROW_479: [[u8; SLOT_BYTES]; SLOT_COUNT] = [
    // slot 0, fb_x=0, peak=20
    [
        0x00, 0x00, 0x33, 0xba, 0x74, 0xca, 0x6d, 0xa1, 0xe0, 0xd1, 0x80, 0xca, 0x84, 0x82, 0x8b,
        0x82, 0x93, 0x82, 0xd4, 0x81, 0xd4, 0x80, 0x10, 0x84, 0x14, 0xa0, 0x14, 0xbc, 0x11, 0xd0,
        0x0a, 0xd0,
    ],
    // slot 1, fb_x=16, peak=19
    [
        0x00, 0x00, 0xc0, 0x4c, 0xa0, 0x4d, 0x60, 0x46, 0x60, 0x2a, 0x60, 0x0e, 0x64, 0x02, 0x6b,
        0x02, 0x72, 0x02, 0xb3, 0x01, 0xd3, 0x00, 0x13, 0x04, 0x13, 0x20, 0x13, 0x3c, 0x11, 0x4c,
        0x09, 0x4c,
    ],
    // slot 2, fb_x=32, peak=17
    [
        0x00, 0x00, 0xc0, 0x44, 0xa0, 0x45, 0x20, 0x42, 0x20, 0x26, 0x20, 0x0e, 0x24, 0x02, 0x2a,
        0x02, 0x31, 0x02, 0x91, 0x01, 0xb1, 0x00, 0x11, 0x04, 0x11, 0x1c, 0x11, 0x34, 0x0f, 0x44,
        0x08, 0x44,
    ],
    // slot 3, fb_x=48, peak=16
    [
        0x00, 0x00, 0xa0, 0x40, 0x60, 0x41, 0x00, 0x3e, 0x00, 0x22, 0x00, 0x0a, 0x03, 0x02, 0x09,
        0x02, 0x0f, 0x02, 0x70, 0x01, 0xb0, 0x00, 0x10, 0x04, 0x10, 0x18, 0x10, 0x34, 0x0e, 0x40,
        0x08, 0x40,
    ],
    // slot 4, fb_x=64, peak=15
    [
        0x00, 0x00, 0xa0, 0x3c, 0x60, 0x3d, 0xe0, 0x35, 0xe0, 0x21, 0xe0, 0x09, 0xe3, 0x01, 0xe8,
        0x01, 0xee, 0x01, 0x4f, 0x01, 0x8f, 0x00, 0x0f, 0x04, 0x0f, 0x18, 0x0f, 0x2c, 0x0d, 0x3c,
        0x07, 0x3c,
    ],
    // slot 5, fb_x=80, peak=13
    [
        0x00, 0x00, 0x80, 0x34, 0x20, 0x35, 0xa0, 0x31, 0xa0, 0x1d, 0xa0, 0x09, 0xa2, 0x01, 0xa8,
        0x01, 0xad, 0x01, 0x2d, 0x01, 0x8d, 0x00, 0x0d, 0x04, 0x0d, 0x14, 0x0d, 0x2c, 0x0c, 0x34,
        0x06, 0x34,
    ],
    // slot 6, fb_x=96, peak=12
    [
        0x00, 0x00, 0x80, 0x30, 0x00, 0x31, 0x80, 0x2d, 0x80, 0x19, 0x80, 0x09, 0x82, 0x01, 0x87,
        0x01, 0x8b, 0x01, 0x0c, 0x01, 0x8c, 0x00, 0x0c, 0x04, 0x0c, 0x14, 0x0c, 0x24, 0x0b, 0x30,
        0x06, 0x30,
    ],
    // slot 7, fb_x=112, peak=11
    [
        0x00, 0x00, 0x60, 0x2c, 0xe0, 0x2c, 0x60, 0x29, 0x60, 0x19, 0x60, 0x09, 0x62, 0x01, 0x66,
        0x01, 0x6a, 0x01, 0xeb, 0x00, 0x6b, 0x00, 0x0b, 0x04, 0x0b, 0x10, 0x0b, 0x20, 0x09, 0x2c,
        0x05, 0x2c,
    ],
    // slot 8, fb_x=128, peak=9
    [
        0x00, 0x00, 0x60, 0x24, 0xc0, 0x24, 0x20, 0x21, 0x20, 0x15, 0x20, 0x09, 0x22, 0x01, 0x25,
        0x01, 0x28, 0x01, 0xc9, 0x00, 0x49, 0x00, 0x09, 0x04, 0x09, 0x10, 0x09, 0x1c, 0x08, 0x24,
        0x04, 0x24,
    ],
    // slot 9, fb_x=144, peak=8
    [
        0x00, 0x00, 0x40, 0x20, 0xa0, 0x20, 0x00, 0x1d, 0x00, 0x11, 0x00, 0x05, 0x02, 0x01, 0x04,
        0x01, 0x08, 0x01, 0xa8, 0x00, 0x48, 0x00, 0x08, 0x04, 0x08, 0x0c, 0x08, 0x18, 0x07, 0x20,
        0x04, 0x20,
    ],
    // slot 10, fb_x=160, peak=6
    [
        0x00, 0x00, 0x40, 0x18, 0x80, 0x18, 0xc0, 0x18, 0xc0, 0x10, 0xc0, 0x04, 0xc1, 0x00, 0xc4,
        0x00, 0xc6, 0x00, 0x86, 0x00, 0x46, 0x00, 0x06, 0x04, 0x06, 0x08, 0x06, 0x14, 0x06, 0x18,
        0x03, 0x18,
    ],
    // slot 11, fb_x=176, peak=5
    [
        0x00, 0x00, 0x20, 0x14, 0x60, 0x14, 0xa0, 0x14, 0xa0, 0x0c, 0xa0, 0x04, 0xa1, 0x00, 0xa3,
        0x00, 0xa5, 0x00, 0x65, 0x00, 0x25, 0x00, 0x05, 0x04, 0x05, 0x08, 0x05, 0x10, 0x04, 0x14,
        0x02, 0x14,
    ],
    // slot 12, fb_x=192, peak=4
    [
        0x00, 0x00, 0x20, 0x10, 0x40, 0x10, 0x80, 0x10, 0x80, 0x08, 0x80, 0x04, 0x81, 0x00, 0x82,
        0x00, 0x84, 0x00, 0x44, 0x00, 0x24, 0x00, 0x04, 0x04, 0x04, 0x08, 0x04, 0x0c, 0x04, 0x10,
        0x02, 0x10,
    ],
    // slot 13, fb_x=208, peak=2
    [
        0x00, 0x00, 0x20, 0x08, 0x20, 0x08, 0x40, 0x08, 0x40, 0x08, 0x40, 0x04, 0x41, 0x00, 0x42,
        0x00, 0x42, 0x00, 0x22, 0x00, 0x22, 0x00, 0x02, 0x04, 0x02, 0x04, 0x02, 0x08, 0x02, 0x08,
        0x01, 0x08,
    ],
    // slot 14, fb_x=224, peak=1
    [
        0x00, 0x00, 0x00, 0x04, 0x20, 0x04, 0x20, 0x04, 0x20, 0x04, 0x20, 0x04, 0x21, 0x00, 0x21,
        0x00, 0x21, 0x00, 0x21, 0x00, 0x01, 0x00, 0x01, 0x00, 0x01, 0x04, 0x01, 0x04, 0x01, 0x04,
        0x01, 0x04,
    ],
];

/// VRAM upload for one CLUT slot of the global hue ramp.
#[derive(Debug, Clone, Copy)]
pub struct ClutSlot {
    /// VRAM pixel column.
    pub fb_x: u16,
    /// VRAM pixel row (always [`ROW_Y`]).
    pub fb_y: u16,
    /// 32 BGR555 bytes (= 16 entries).
    pub bytes: [u8; SLOT_BYTES],
}

/// Iterator over the 15 CLUT slot uploads.
pub fn global_hue_ramp_uploads() -> impl Iterator<Item = ClutSlot> {
    (0..SLOT_COUNT).map(|slot| {
        let fb_x = FIRST_SLOT_X + (slot as u16) * SLOT_WIDTH_PX;
        ClutSlot {
            fb_x,
            fb_y: ROW_Y,
            bytes: GLOBAL_HUE_RAMP_ROW_479[slot],
        }
    })
}

/// Paint the 15-slot global hue ramp into `vram` at row 479. The retail
/// runtime installs this once at boot and lets it persist across every
/// non-battle scene transition; battle scenes overwrite the row with
/// battle-overlay content and the engine's battle scene loader is
/// responsible for skipping this call when appropriate.
pub fn apply_global_hue_ramp(vram: &mut Vram) {
    for slot in global_hue_ramp_uploads() {
        vram.write_clut_row(slot.fb_x, slot.fb_y, &slot.bytes);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slot_count_matches_table() {
        assert_eq!(GLOBAL_HUE_RAMP_ROW_479.len(), SLOT_COUNT);
    }

    #[test]
    fn slot_widths_pack_into_240_pixels() {
        // Slots 0..14 span fb_x 0..240 in 16-pixel increments.
        let last_x = FIRST_SLOT_X + (SLOT_COUNT as u16) * SLOT_WIDTH_PX;
        assert_eq!(last_x, 240);
    }

    #[test]
    fn slot_0_entry_0_is_transparent_black() {
        // Every retail CLUT in the ramp starts with the canonical
        // `0x0000` entry (transparent black at index 0).
        assert_eq!(&GLOBAL_HUE_RAMP_ROW_479[0][..2], &[0x00, 0x00]);
    }

    #[test]
    fn peaks_descend_monotonically() {
        // Verify the documented peak sequence.
        let want_peaks = [20, 19, 17, 16, 15, 13, 12, 11, 9, 8, 6, 5, 4, 2, 1];
        for (slot, &want) in want_peaks.iter().enumerate() {
            let bytes = &GLOBAL_HUE_RAMP_ROW_479[slot];
            let mut peak = 0u8;
            for chunk in bytes.chunks_exact(2) {
                let h = u16::from_le_bytes([chunk[0], chunk[1]]);
                let r = (h & 0x1F) as u8;
                let g = ((h >> 5) & 0x1F) as u8;
                let b = ((h >> 10) & 0x1F) as u8;
                peak = peak.max(r).max(g).max(b);
            }
            assert_eq!(peak, want, "slot {} peak mismatch", slot);
        }
    }

    #[test]
    fn apply_writes_every_slot_at_correct_xy() {
        let mut vram = Vram::new();
        apply_global_hue_ramp(&mut vram);
        let bytes = vram.as_bytes();
        // VRAM is 1024x512x2 = 1048576 bytes; row stride = 2048 bytes.
        let row_off = (ROW_Y as usize) * 1024 * 2;
        for (slot, expected) in GLOBAL_HUE_RAMP_ROW_479.iter().enumerate() {
            let x_off = (slot * 16) * 2;
            let written = &bytes[row_off + x_off..row_off + x_off + SLOT_BYTES];
            assert_eq!(written, &expected[..], "slot {slot}");
        }
    }
}
