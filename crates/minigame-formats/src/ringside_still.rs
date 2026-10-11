//! The two headerless 16bpp stills the field background reader uploads at
//! VRAM `(384, 0)` - extraction PROT `1221` / `1222`.
//!
//! Each entry is exactly [`ENTRY_BYTES`] bytes, which is `320 * 256 * 2` to
//! the byte, and carries no TIM header: the rectangle lives in the consumer's
//! code, not in the file. That is why the format needs a page at all - nothing
//! in the bytes says how wide they are.
//!
//! ## What names the rectangle
//!
//! `FUN_801F6B24` in the PROT `0978` `field_back_read` image (slot-B base
//! `0x801F69D8`) stores a PSX `RECT` at `0x801F735C` and re-uploads it four
//! times, once per 20-sector read:
//!
//! ```text
//! 801f6be4  li   v0,0x180         ; rect.x  = 384
//! 801f6bec  sh   v0,0x735c(at)
//! 801f6bf0  li   v0,0x140         ; rect.w  = 320
//! 801f6bf8  sh   v0,0x7360(at)
//! 801f6bfc  li   v0,0x40          ; rect.h  = 64
//! 801f6c04  sh   v0,0x7362(at)
//! 801f6c20  sh   zero,0x735e(at)  ; rect.y  = 0, then 0x40 / 0x80 / 0xC0
//! ```
//!
//! and the read that fills each band is `FUN_8003E964(sector, 0)` seeking
//! `0 / 0x14 / 0x28 / 0x3C` followed by `FUN_8003E800(dst, 0x14, 1)` - 20
//! sectors, `0xA000` bytes, exactly `320 * 64 * 2`. The upload itself is
//! `FUN_800583C8`, the `LoadImage` wrapper (it passes the literal
//! `"LoadImage"` at `0x800156D4` to the debug hook before the libgpu vtable
//! call). So all four of `x`, `w`, `h` and the per-band `y` are immediates in
//! the consumer, and the band size is confirmed twice over - by the seek
//! stride and by the rectangle's own area.
//!
//! ## Which entry is which
//!
//! Nothing materialises the raw TOC index as a literal; it is computed, and
//! the `addiu` that forms it sits in the `jal`'s delay slot:
//!
//! ```text
//! 801f6b90  lhu  v0,0x4824(v0)    ; party slot 0 hp_max_record  (record +0x11C)
//! 801f6b98  lhu  v1,0x480e(v1)    ; party slot 0 hp_curr_live   (record +0x106)
//! 801f6ba4  srl  v0,v0,0x1
//! 801f6bac  sltu s0,v1,v0         ; s0 = current HP < max / 2
//! 801f6c3c  addiu a0,s0,0x4c7     ; raw TOC 0x4C7 + s0
//! 801f6c40  jal  0x8003e8a8       ; the LBA resolver
//! ```
//!
//! Raw TOC `0x4C7` / `0x4C8` are extraction [`PROT_INDEX_DEFAULT`] /
//! [`PROT_INDEX_LOW_HP`] under the `+2` correction
//! (`docs/formats/cdname.md`), so the second still is the below-half-HP
//! variant. See `ghidra/scripts/funcs/overlay_field_back_read_0978_801f6b24.txt`,
//! `docs/formats/ringside-still.md` and
//! `docs/subsystems/minigame-muscle-dome.md`.

/// Extraction PROT index of the default still (dev name `int.tim`).
pub const PROT_INDEX_DEFAULT: u32 = 1221;
/// Extraction PROT index of the below-half-HP still (dev name `int2.tim`).
pub const PROT_INDEX_LOW_HP: u32 = 1222;

/// Pixel width of the whole still - the `rect.w` immediate `0x140`.
pub const WIDTH: usize = 320;
/// Pixel height of the whole still: [`BAND_COUNT`] bands of [`BAND_HEIGHT`].
pub const HEIGHT: usize = BAND_COUNT * BAND_HEIGHT;
/// VRAM x the consumer uploads every band to - the `rect.x` immediate `0x180`.
pub const VRAM_X: u16 = 384;
/// VRAM y of band 0. The per-band `rect.y` is `VRAM_Y + i * BAND_HEIGHT`.
pub const VRAM_Y: u16 = 0;

/// Pixel height of one upload - the `rect.h` immediate `0x40`.
pub const BAND_HEIGHT: usize = 64;
/// Sectors one band read covers (`FUN_8003E800(dst, 0x14, 1)`).
pub const BAND_SECTORS: usize = 0x14;
/// Bytes in one band: `BAND_SECTORS * 2048`, and also `WIDTH * BAND_HEIGHT * 2`.
pub const BAND_BYTES: usize = BAND_SECTORS * 2048;
/// Bands per entry - four seeks at sector `0 / 0x14 / 0x28 / 0x3C`.
pub const BAND_COUNT: usize = 4;
/// Whole-entry byte length.
pub const ENTRY_BYTES: usize = BAND_COUNT * BAND_BYTES;

/// Is this extraction index one of the two stills?
pub fn is_ringside_still(prot_index: u32) -> bool {
    prot_index == PROT_INDEX_DEFAULT || prot_index == PROT_INDEX_LOW_HP
}

/// Does this buffer have the shape the four uploads assume?
///
/// The only structural statement a still makes about itself is its length, so
/// that is the whole test. A caller pairs it with the index - the bytes carry
/// no magic, and nothing here can tell a still from any other `0x28000`-byte
/// 16bpp region.
pub fn has_still_shape(buf: &[u8]) -> bool {
    buf.len() == ENTRY_BYTES
}

/// One band's byte span inside the entry, or `None` past the last band.
pub fn band_span(index: usize) -> Option<core::ops::Range<usize>> {
    (index < BAND_COUNT).then(|| index * BAND_BYTES..(index + 1) * BAND_BYTES)
}

/// The VRAM rectangle `(x, y, w, h)` band `index` is uploaded with, in the
/// units `LoadImage` takes: `w` is 16bpp pixels, which for this still is also
/// VRAM halfwords.
pub fn band_rect(index: usize) -> Option<(u16, u16, u16, u16)> {
    (index < BAND_COUNT).then(|| {
        (
            VRAM_X,
            VRAM_Y + (index * BAND_HEIGHT) as u16,
            WIDTH as u16,
            BAND_HEIGHT as u16,
        )
    })
}

/// Decode the still to RGBA8, row-major, `WIDTH * HEIGHT` pixels.
///
/// The 5-bit channels are expanded by `c << 3 | c >> 2` so `0x1F` reaches
/// `0xFF`; the STP bit is dropped (it is clear across both entries, which is
/// what puts them in the residue classifier's `bgr555` bucket in the first
/// place). Returns `None` unless the buffer is exactly [`ENTRY_BYTES`].
pub fn to_rgba8(buf: &[u8]) -> Option<Vec<u8>> {
    if !has_still_shape(buf) {
        return None;
    }
    let mut out = Vec::with_capacity(WIDTH * HEIGHT * 4);
    for px in buf.as_chunks::<2>().0 {
        let h = u16::from_le_bytes([px[0], px[1]]);
        let ch = |shift: u32| {
            let c = ((h >> shift) & 0x1F) as u8;
            c << 3 | c >> 2
        };
        out.extend_from_slice(&[ch(0), ch(5), ch(10), 0xFF]);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn band_arithmetic_closes_on_the_rect() {
        // The two independent statements of the band size agree: 20 sectors,
        // and the area of the rectangle the consumer uploads.
        assert_eq!(BAND_BYTES, WIDTH * BAND_HEIGHT * 2);
        assert_eq!(ENTRY_BYTES, WIDTH * HEIGHT * 2);
        assert_eq!(ENTRY_BYTES, 0x28000);
        assert_eq!(band_span(3), Some(0x1E000..0x28000));
        assert_eq!(band_span(4), None);
        assert_eq!(band_rect(2), Some((384, 128, 320, 64)));
    }

    #[test]
    fn rgba_expands_both_extremes() {
        let mut buf = vec![0u8; ENTRY_BYTES];
        buf[0..2].copy_from_slice(&0x7FFFu16.to_le_bytes());
        buf[2..4].copy_from_slice(&0u16.to_le_bytes());
        let rgba = to_rgba8(&buf).unwrap();
        assert_eq!(&rgba[0..4], &[0xFF, 0xFF, 0xFF, 0xFF]);
        assert_eq!(&rgba[4..8], &[0x00, 0x00, 0x00, 0xFF]);
        assert_eq!(rgba.len(), WIDTH * HEIGHT * 4);
        assert!(to_rgba8(&buf[..16]).is_none());
    }
}
