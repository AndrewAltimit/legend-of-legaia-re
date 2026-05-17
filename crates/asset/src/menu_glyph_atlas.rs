//! Menu-glyph atlas extractor — the small-caps font used to render the
//! title-screen menu rows ("NEW GAME" / "CONTINUE" / "OPTIONS") and the
//! same shape across the rest of the menu UI.
//!
//! ## Source on disc
//!
//! The atlas lives at **`PROT.DAT` file offset `0x11218`**, inside a
//! 240 KB unindexed system-UI gap between the TOC (sectors 0..2) and
//! the first indexed entry (`init_data` at LBA 121). The per-entry
//! extractor walks TOC indices and never visits this region, so the
//! atlas does NOT appear in any extracted `PROT/NNNN_*.BIN` file.
//!
//! Reach it by reading `PROT.DAT` directly (see
//! [`legaia_prot::Archive::read_raw`]). See [`docs/subsystems/boot.md`]
//! for the loader pathway hypothesis.
//!
//! ## TIM layout
//!
//! ```text
//! header  : 8 bytes  (magic=0x10, flags=0x08)
//! CLUT    : 12 + 16*16*2 = 524 bytes
//!           rect = (fb_x=0, fb_y=510, w=16, h=16)
//!           — 16 CLUT rows × 16 colours (4bpp palette banks)
//! pixels  : 12 + 64*256*2 = 32780 bytes
//!           rect = (fb_x=960, fb_y=256, fb_w=64, h=256)
//!           — 4bpp, decoded width = 256 px, height = 256 px
//! total   : 33312 bytes
//! ```
//!
//! Glyphs are arranged in fixed-width rows in the lower half of the
//! atlas. The rows we care about for menu rendering:
//!
//! | Row             | Y range  | Cell w | First-cell X | Content              |
//! |-----------------|----------|--------|--------------|----------------------|
//! | `digits`        | 209..220 | 8      | 8            | `0123456789`         |
//! | `alphabet`      | 224..238 | 8      | 8            | `ABCDEFGHIJKLMNOPQRSTUVWXYZ` |
//!
//! Both rows are 14 px tall (the rendered glyph height); cells are
//! 8 px wide on an 8 px pitch with no inter-cell gutter. The first
//! cell of each row starts at `x = 8`.
//!
//! The atlas also carries debug-only content (a `<DEMO>` row, an
//! "ここは常駐エフェクトが入る予定 / Pochi" debug string, a `FONT CLUT`
//! palette-bar indicator, and various cursor / arrow sprites) — all
//! ignored by the engine. The retail menu paths only sample the two
//! rows above.
//!
//! ## Provenance
//!
//! Pinned by `scripts/scan_tims_and_match_prot.py --sig-mode pixel`
//! against a full main-RAM dump captured at the live title-menu
//! state (sstate8). The in-RAM TIM at vaddr `0x80106478` is
//! byte-equal to `PROT.DAT[0x11218..0x11218 + 33312]`.
//!
//! See also: [`crate::title_pak`] for the title-art bands (drawn
//! together with the menu glyphs in the same phase).

use anyhow::{Context, Result};

/// `PROT.DAT` byte offset of the menu-glyph atlas TIM (NA retail).
pub const PROT_DAT_OFFSET: u64 = 0x11218;

/// Total byte length of the menu-glyph atlas TIM.
/// `8 + (12 + 16*16*2) + (12 + 64*256*2) = 33312`.
pub const TIM_SIZE: usize = 33312;

/// Atlas width in source pixels (256 = decoded 4bpp width = `fb_w * 4`).
pub const ATLAS_WIDTH: u32 = 256;
/// Atlas height in source pixels.
pub const ATLAS_HEIGHT: u32 = 256;

/// Source rect (atlas pixels, `(x, y, w, h)`) of the uppercase
/// alphabet row. 26 cells × 8 px wide × 14 px tall, starting at
/// `(x=8, y=224)`.
pub const ALPHABET_ROW: (u32, u32, u32, u32) = (8, 224, 26 * 8, 14);

/// Source rect of the digits row. 10 cells × 8 px wide × 11 px tall,
/// starting at `(x=8, y=209)`. The first cell renders `0`.
pub const DIGITS_ROW: (u32, u32, u32, u32) = (8, 209, 10 * 8, 11);

/// Width of a single glyph cell (alphabet + digits share this pitch).
pub const GLYPH_W: u32 = 8;
/// Height of a single alphabet-row glyph cell.
pub const ALPHABET_GLYPH_H: u32 = 14;
/// Height of a single digit-row glyph cell.
pub const DIGITS_GLYPH_H: u32 = 11;
/// X coordinate of the first alphabet cell (the `A` cell).
pub const ALPHABET_FIRST_X: u32 = 8;
/// Y coordinate of the alphabet row's top edge.
pub const ALPHABET_Y: u32 = 224;
/// X coordinate of the first digit cell (the `0` cell).
pub const DIGITS_FIRST_X: u32 = 8;
/// Y coordinate of the digit row's top edge.
pub const DIGITS_Y: u32 = 209;

/// PSX TIM magic word (`0x00000010` LE).
const TIM_MAGIC: u32 = 0x0000_0010;

/// View into the menu-glyph atlas TIM (no copy).
#[derive(Debug, Clone)]
pub struct MenuGlyphTim<'a> {
    /// Reference into the input buffer (no copy).
    pub bytes: &'a [u8],
    /// PSX VRAM target rect for the pixel block — `(fb_x, fb_y, fb_w, h)`.
    /// `fb_w` is in 16-bit halfword units; in 4bpp the decoded width
    /// is `fb_w * 4` pixels.
    pub pixel_rect: (u16, u16, u16, u16),
    /// PSX VRAM target rect for the CLUT block — `(fb_x, fb_y, w, h)`.
    pub clut_rect: (u16, u16, u16, u16),
}

/// Look up the source rect (atlas pixels, `(x, y, w, h)`) of a single
/// printable character. Returns `None` for characters not present in
/// the atlas (e.g. punctuation outside the digits / alphabet rows).
///
/// Space is intentionally `None` — callers should reserve [`GLYPH_W`]
/// pixels of horizontal space for it without emitting any draw.
pub fn glyph_rect(c: char) -> Option<(u32, u32, u32, u32)> {
    let upper = c.to_ascii_uppercase();
    match upper {
        'A'..='Z' => {
            let i = (upper as u32) - ('A' as u32);
            Some((
                ALPHABET_FIRST_X + i * GLYPH_W,
                ALPHABET_Y,
                GLYPH_W,
                ALPHABET_GLYPH_H,
            ))
        }
        '0'..='9' => {
            let i = (upper as u32) - ('0' as u32);
            Some((
                DIGITS_FIRST_X + i * GLYPH_W,
                DIGITS_Y,
                GLYPH_W,
                DIGITS_GLYPH_H,
            ))
        }
        _ => None,
    }
}

/// Extract the menu-glyph atlas TIM from raw `PROT.DAT` bytes.
///
/// Validates the TIM header at [`PROT_DAT_OFFSET`] (magic + flags +
/// CLUT + pixel block sizes). Returns a borrowed view; the caller
/// keeps the backing buffer alive.
pub fn extract_from_prot_dat(prot_dat: &[u8]) -> Result<MenuGlyphTim<'_>> {
    let off = PROT_DAT_OFFSET as usize;
    if off + TIM_SIZE > prot_dat.len() {
        anyhow::bail!(
            "PROT.DAT too small ({} bytes) for menu-glyph TIM at 0x{:X} +{}",
            prot_dat.len(),
            off,
            TIM_SIZE
        );
    }
    parse_tim_at(prot_dat, off)
}

/// Extract the menu-glyph atlas TIM from a buffer that already contains
/// the TIM bytes starting at offset 0 — e.g. the
/// [`TIM_SIZE`]-byte slice returned by an opaque PROT.DAT-raw reader.
///
/// Validates the TIM header at offset 0 the same way as
/// [`extract_from_prot_dat`]. Use this when the caller already paid
/// the seek cost and only fetched the TIM slice.
pub fn extract_from_tim_slice(tim_bytes: &[u8]) -> Result<MenuGlyphTim<'_>> {
    if tim_bytes.len() < TIM_SIZE {
        anyhow::bail!(
            "menu-glyph TIM slice too small ({} bytes), expected at least {}",
            tim_bytes.len(),
            TIM_SIZE
        );
    }
    parse_tim_at(tim_bytes, 0)
}

fn parse_tim_at(bytes: &[u8], off: usize) -> Result<MenuGlyphTim<'_>> {
    let read_u32 = |p: usize| -> Result<u32> {
        bytes
            .get(p..p + 4)
            .map(|s| u32::from_le_bytes(s.try_into().unwrap()))
            .with_context(|| format!("out-of-range read at 0x{:x}", p))
    };
    let read_u16 = |p: usize| -> Result<u16> {
        bytes
            .get(p..p + 2)
            .map(|s| u16::from_le_bytes(s.try_into().unwrap()))
            .with_context(|| format!("out-of-range read at 0x{:x}", p))
    };

    let magic = read_u32(off)?;
    if magic != TIM_MAGIC {
        anyhow::bail!(
            "bad menu-glyph TIM magic 0x{:08x} at 0x{:x} (expected 0x10)",
            magic,
            off
        );
    }
    let flags = read_u32(off + 4)?;
    let mode = flags & 0x7;
    let has_clut = (flags & 0x8) != 0;
    if mode != 0 {
        anyhow::bail!("menu-glyph TIM expected 4bpp (mode 0), got mode {}", mode);
    }
    if !has_clut {
        anyhow::bail!("menu-glyph TIM expected CLUT (flags bit 3)");
    }

    let mut p = off + 8;
    let clut_size = read_u32(p)? as usize;
    let clut_fb_x = read_u16(p + 4)?;
    let clut_fb_y = read_u16(p + 6)?;
    let clut_w = read_u16(p + 8)?;
    let clut_h = read_u16(p + 10)?;
    p += clut_size;

    let pix_size = read_u32(p)? as usize;
    let pix_fb_x = read_u16(p + 4)?;
    let pix_fb_y = read_u16(p + 6)?;
    let pix_w = read_u16(p + 8)?;
    let pix_h = read_u16(p + 10)?;
    p += pix_size;

    let total = p - off;
    if total != TIM_SIZE {
        anyhow::bail!(
            "menu-glyph TIM size mismatch: parsed {} bytes, expected {}",
            total,
            TIM_SIZE
        );
    }
    let slice = bytes
        .get(off..off + total)
        .with_context(|| format!("TIM at 0x{:x} overruns buffer", off))?;

    Ok(MenuGlyphTim {
        bytes: slice,
        pixel_rect: (pix_fb_x, pix_fb_y, pix_w, pix_h),
        clut_rect: (clut_fb_x, clut_fb_y, clut_w, clut_h),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glyph_rects_are_within_atlas() {
        for c in 'A'..='Z' {
            let r = glyph_rect(c).unwrap();
            assert!(r.0 + r.2 <= ATLAS_WIDTH);
            assert!(r.1 + r.3 <= ATLAS_HEIGHT);
        }
        for c in '0'..='9' {
            let r = glyph_rect(c).unwrap();
            assert!(r.0 + r.2 <= ATLAS_WIDTH);
            assert!(r.1 + r.3 <= ATLAS_HEIGHT);
        }
        // Case-insensitive.
        assert_eq!(glyph_rect('a'), glyph_rect('A'));
        // Unmapped characters return None.
        assert_eq!(glyph_rect(' '), None);
        assert_eq!(glyph_rect('!'), None);
    }

    #[test]
    fn tim_size_is_internally_consistent() {
        // header + CLUT (16 rows × 16 colours) + pixel (256 × 256 4bpp).
        assert_eq!(TIM_SIZE, 8 + (12 + 16 * 16 * 2) + (12 + 256 / 2 * 256));
    }

    /// Disc-gated: extract the menu-glyph TIM from a real `PROT.DAT`.
    /// Skips when the env var isn't set (CI runs without disc data).
    #[test]
    fn extracts_menu_glyph_tim_from_real_prot_dat() {
        // The disc-gated path: read PROT.DAT directly. The 240 KB
        // pre-init_data gap that carries this TIM lives at file offsets
        // 0x1800..0x3C800 (sectors 3..120); the per-entry extractor
        // walks TOC indices and never extracts it.
        let path = "../../extracted/PROT.DAT";
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(_) => {
                eprintln!("skip: extracted/PROT.DAT missing");
                return;
            }
        };
        let tim = extract_from_prot_dat(&bytes).expect("extract menu-glyph TIM at 0x11218");

        // VRAM layout: CLUT @ (0, 510), pixel @ (960, 256), 256×256 @ 4bpp.
        assert_eq!(tim.clut_rect, (0, 510, 16, 16));
        assert_eq!(tim.pixel_rect, (960, 256, 64, 256));
        assert_eq!(tim.bytes.len(), TIM_SIZE);
    }
}
