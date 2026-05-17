//! Title-screen TIM extractor.
//!
//! The "Legend of Legaia" title screen lives as a 256x256 8bpp TIM in
//! PROT entries `0888..=0890` (labelled `sound_data2` per CDNAME, but
//! the multi-bank sound-data cluster carries title art in the trailing
//! pool past the actual sound payload). The byte layout below is stable
//! across the NA retail build.
//!
//! ## Sources by PROT entry
//!
//! ```text
//!   PROT 0888 (sound_data2)    @ file offset 0x1AA28    — PRIMARY
//!   PROT 0889 (sound_data2)    @ file offset 0x19A28    — same content,
//!   PROT 0890 (sound_data2)    @ file offset 0x14228    — multi-bank duplicates
//! ```
//!
//! The TIM is 33312 bytes total (8-byte header + 12 + 512 CLUT block +
//! 12 + 65536 pixel block). Pixel-block VRAM target is `fb=(512,256)`,
//! CLUT block target is in-line with the pixel block's frame buffer.
//!
//! Pinned by `scripts/scan_tims_and_match_prot.py` against a full
//! 2 MiB main-RAM dump captured at the live title screen (sstate8),
//! cross-referenced against the in-RAM copy at vaddr `0x80170DF8`.
//! Renders to the complete title screen: wordmark, orb, "PRESS START
//! BUTTON", "NEW GAME" / "CONTINUE" menu, copyright lines.
//!
//! See [`crate::init_pak`] for the parallel publisher-logo case
//! (PROT 0895, `bat_back_dat` per CDNAME, actually init.pak).
//!
//! ## Related title-overlay TIMs
//!
//! The title overlay's save/load sub-menu draws two sprite-descriptor
//! TIMs embedded inside the overlay binary itself, in PROT entry
//! 0899's extended footprint (the trailing-gap title overlay landed
//! in commit `df4f1ce`):
//!
//! ```text
//!   PROT 0899 @ file offset 0x16908    — save-menu UI atlas
//!                                         (256x256 4bpp)
//!   PROT 0899 @ file offset 0x1F908    — animated memory-card icon
//!                                         (256x16 4bpp, 14 frames)
//! ```
//!
//! These are referenced as runtime sprite-descriptor templates at
//! vaddrs `0x801E5120` / `0x801EE120` by `FUN_801DD35C` (the title
//! tick); see [`crate::title_overlay`] in `engine-vm` for the
//! dispatcher port.
//!
//! ## Provenance
//!
//! Methodology: scan a PSX main-RAM dump for TIM-magic-headed records,
//! byte-grep the extracted PROT corpus for each candidate. The
//! runtime patches only `fb_x`/`fb_y` for CLUT relocation; the rest
//! of each TIM is byte-equal to the on-disc source. Full repro in
//! `scripts/scan_tims_and_match_prot.py --help`.

use anyhow::{Context, Result};

/// Primary PROT entry index for the main title TIM (NA retail build).
///
/// Entries 0889 and 0890 carry duplicate copies; see
/// [`TITLE_TIM_ALTERNATE_SOURCES`].
pub const PROT_INDEX_PRIMARY: u16 = 888;

/// Alternate PROT entry indices carrying the same title TIM at
/// different file offsets (multi-bank duplicates in the
/// `sound_data2` cluster).
pub const TITLE_TIM_ALTERNATE_SOURCES: &[(u16, usize)] = &[(889, 0x19A28), (890, 0x14228)];

/// File offset within PROT 0888 where the main title TIM begins.
pub const TITLE_TIM_OFFSET: usize = 0x1AA28;

/// Total byte length of the title TIM (header + CLUT block + pixel
/// block). The main title is 256x256 8bpp + 256-colour CLUT:
/// `8 + (12 + 256*2) + (12 + 256*256) = 66080` bytes.
pub const TITLE_TIM_SIZE: usize = 66080;

/// PROT entry index carrying the title-overlay's save-menu sprite TIMs
/// embedded in its trailing-overlay binary.
pub const PROT_INDEX_OVERLAY: u16 = 899;

/// File offset within PROT 0899 (extended footprint) of the save-menu
/// UI sprite atlas (256x256 4bpp; memory-card icons + Japanese strings).
/// Drawn via sprite-descriptor template `0x801E5120` at runtime.
pub const OVERLAY_SAVE_MENU_TIM_OFFSET: usize = 0x16908;

/// File offset within PROT 0899 (extended footprint) of the animated
/// memory-card icon strip (256x16 4bpp, 14 frames). Drawn via
/// sprite-descriptor template `0x801EE120` at runtime.
pub const OVERLAY_CARD_ICON_TIM_OFFSET: usize = 0x1F908;

/// Total byte length of [`OVERLAY_SAVE_MENU_TIM_OFFSET`]'s TIM.
pub const OVERLAY_SAVE_MENU_TIM_SIZE: usize = 33312;

/// Total byte length of [`OVERLAY_CARD_ICON_TIM_OFFSET`]'s TIM.
pub const OVERLAY_CARD_ICON_TIM_SIZE: usize = 2592;

/// Source sub-rect, in atlas pixels `(x, y, w, h)`, of the orb +
/// "Legend of Legaia" wordmark band inside the 256×256 title TIM.
/// Always drawn in PressStart and MainMenu phases - matches retail.
pub const TITLE_BAND_WORDMARK: (u32, u32, u32, u32) = (0, 17, 256, 124);

/// Source sub-rect of the `<DEMO>` band. **Demo-only** - retail builds
/// never draw this region, even though it sits in the same TIM. Kept
/// here as a reference; engines should NOT emit a draw for this rect.
pub const TITLE_BAND_DEMO: (u32, u32, u32, u32) = (96, 151, 64, 10);

/// Source sub-rect of the "PRESS START BUTTON" prompt label. Drawn
/// only during the PressStart phase, matching retail.
pub const TITLE_BAND_PRESS_START: (u32, u32, u32, u32) = (60, 178, 196, 16);

/// Source sub-rect of the "TM of Sony Computer Entertainment America
/// Inc." copyright line. Drawn in all post-fade phases.
pub const TITLE_BAND_TM_COPYRIGHT: (u32, u32, u32, u32) = (4, 195, 244, 14);

/// Source sub-rect of the "© 1998,1999 Sony Computer Entertainment
/// Inc." copyright line. Drawn in all post-fade phases.
pub const TITLE_BAND_C_COPYRIGHT: (u32, u32, u32, u32) = (8, 209, 234, 14);

/// Source sub-rect of the **"NEW GAME"** menu row. Retail's two-row
/// main-menu strings sit in a single horizontal strip at `y=227..237`
/// inside the title TIM, in the same stylised small-caps font as the
/// "PRESS START BUTTON" and copyright bands. Drawn during the
/// `MainMenu` phase. Colour-based selection: bright/white when the
/// cursor is on this row, dim/gray otherwise.
pub const TITLE_BAND_MENU_NEW_GAME: (u32, u32, u32, u32) = (0, 227, 65, 10);

/// Source sub-rect of the **"CONTINUE"** menu row. Same band as
/// [`TITLE_BAND_MENU_NEW_GAME`]; sampled at a different `x` so retail
/// can stack the two rows vertically on screen.
pub const TITLE_BAND_MENU_CONTINUE: (u32, u32, u32, u32) = (65, 227, 62, 10);

/// PSX TIM magic word (`0x00000010` LE).
const TIM_MAGIC: u32 = 0x0000_0010;

/// A title-screen TIM extracted from one of the title-PROT entries.
#[derive(Debug, Clone)]
pub struct TitleTim<'a> {
    /// File offset within the source PROT entry.
    pub file_offset: usize,
    /// Total byte length (header + CLUT + pixel).
    pub byte_len: usize,
    /// Reference into the input buffer (no copy).
    pub bytes: &'a [u8],
    /// PSX VRAM target rect for the pixel block — `(fb_x, fb_y, w, h)`.
    pub pixel_rect: (u16, u16, u16, u16),
    /// PSX VRAM target rect for the CLUT block — `(fb_x, fb_y, w, h)`.
    pub clut_rect: (u16, u16, u16, u16),
    /// TIM colour mode (`0`=4bpp, `1`=8bpp, `2`=15bpp, `3`=24bpp).
    pub mode: u8,
}

/// Extract the main title TIM from PROT 0888 (or 889 / 890) bytes.
///
/// Validates the TIM header at [`TITLE_TIM_OFFSET`]. Pass the bytes of
/// PROT entry [`PROT_INDEX_PRIMARY`] (or an alternate from
/// [`TITLE_TIM_ALTERNATE_SOURCES`] - in which case pass the matching
/// offset as `at_offset`).
pub fn extract_title_tim(bytes: &[u8], at_offset: usize) -> Result<TitleTim<'_>> {
    parse_tim_at(bytes, at_offset)
}

/// Extract the save-menu UI sprite atlas from PROT 0899's extended
/// footprint. Returns the 256x256 4bpp TIM at
/// [`OVERLAY_SAVE_MENU_TIM_OFFSET`].
pub fn extract_overlay_save_menu_tim(bytes: &[u8]) -> Result<TitleTim<'_>> {
    parse_tim_at(bytes, OVERLAY_SAVE_MENU_TIM_OFFSET)
}

/// Extract the animated PSX-memory-card icon strip from PROT 0899's
/// extended footprint. Returns the 256x16 4bpp TIM at
/// [`OVERLAY_CARD_ICON_TIM_OFFSET`].
pub fn extract_overlay_card_icon_tim(bytes: &[u8]) -> Result<TitleTim<'_>> {
    parse_tim_at(bytes, OVERLAY_CARD_ICON_TIM_OFFSET)
}

fn parse_tim_at(bytes: &[u8], off: usize) -> Result<TitleTim<'_>> {
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
            "bad TIM magic 0x{:08x} at 0x{:x} (expected 0x10)",
            magic,
            off
        );
    }
    let flags = read_u32(off + 4)?;
    let mode = (flags & 0x7) as u8;
    let has_clut = (flags & 0x8) != 0;
    if mode > 3 {
        anyhow::bail!("invalid TIM mode {}", mode);
    }
    if !has_clut {
        anyhow::bail!("title TIM at 0x{:x} expected CLUT (flags bit 3)", off);
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

    let byte_len = p - off;
    let slice = bytes
        .get(off..off + byte_len)
        .with_context(|| format!("TIM at 0x{:x} overruns file", off))?;

    Ok(TitleTim {
        file_offset: off,
        byte_len,
        bytes: slice,
        pixel_rect: (pix_fb_x, pix_fb_y, pix_w, pix_h),
        clut_rect: (clut_fb_x, clut_fb_y, clut_w, clut_h),
        mode,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Disc-gated: extract the main title TIM from a real PROT 0888.
    /// Skips when `extracted/` is missing (CI runs without disc data).
    #[test]
    fn extracts_real_title_tim_when_disc_extracted() {
        let path = "../../extracted/PROT/0888_sound_data2.BIN";
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(_) => {
                eprintln!("skip: extracted/PROT/0888_sound_data2.BIN missing");
                return;
            }
        };
        let tim =
            extract_title_tim(&bytes, TITLE_TIM_OFFSET).expect("extract main title TIM at 0x1AA28");

        // Canonical layout: 256x256 8bpp + 256-colour CLUT. The runtime
        // patches fb_x/fb_y for CLUT relocation; the dimensions + size
        // are stable.
        assert_eq!(tim.file_offset, TITLE_TIM_OFFSET);
        assert_eq!(tim.byte_len, TITLE_TIM_SIZE);
        assert_eq!(tim.mode, 1); // 8bpp
        assert_eq!(tim.pixel_rect.2, 128); // pw halfwords = 128 (= 256 8bpp pixels)
        assert_eq!(tim.pixel_rect.3, 256); // ph
        assert_eq!(tim.clut_rect.2, 256); // 256-colour CLUT
        assert_eq!(tim.clut_rect.3, 1); // 1 CLUT row
    }

    /// Disc-gated: extract the save-menu UI sprite atlas from PROT 0899.
    /// Requires the EXTENDED footprint (trailing-overlay sectors) so
    /// the extracted file must come from `Archive::read_entry`, not
    /// `read_entry_indexed`.
    #[test]
    fn extracts_overlay_save_menu_tim_when_disc_extracted() {
        let path = "../../extracted/PROT/0899_xxx_dat.BIN";
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(_) => {
                eprintln!("skip: extracted/PROT/0899_xxx_dat.BIN missing");
                return;
            }
        };
        let tim = extract_overlay_save_menu_tim(&bytes).expect("extract save-menu TIM at 0x16908");

        assert_eq!(tim.file_offset, OVERLAY_SAVE_MENU_TIM_OFFSET);
        assert_eq!(tim.byte_len, OVERLAY_SAVE_MENU_TIM_SIZE);
        assert_eq!(tim.mode, 0); // 4bpp
        assert_eq!(tim.pixel_rect.2, 64); // pw halfwords = 64 (= 256 4bpp pixels)
        assert_eq!(tim.pixel_rect.3, 256); // ph
    }

    /// Disc-gated: extract the animated memory-card icon strip.
    #[test]
    fn extracts_overlay_card_icon_tim_when_disc_extracted() {
        let path = "../../extracted/PROT/0899_xxx_dat.BIN";
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(_) => {
                eprintln!("skip: extracted/PROT/0899_xxx_dat.BIN missing");
                return;
            }
        };
        let tim = extract_overlay_card_icon_tim(&bytes).expect("extract card-icon TIM at 0x1F908");

        assert_eq!(tim.file_offset, OVERLAY_CARD_ICON_TIM_OFFSET);
        assert_eq!(tim.byte_len, OVERLAY_CARD_ICON_TIM_SIZE);
        assert_eq!(tim.mode, 0); // 4bpp
        assert_eq!(tim.pixel_rect.2, 64); // pw halfwords = 64
        assert_eq!(tim.pixel_rect.3, 16); // ph (14 frames + small gutter)
    }

    /// Disc-gated: each alternate source PROT entry should carry an
    /// identical (byte-equal) copy of the title TIM at its listed offset.
    #[test]
    fn alternate_sources_byte_equal_when_disc_extracted() {
        let primary_path = "../../extracted/PROT/0888_sound_data2.BIN";
        let primary_bytes = match std::fs::read(primary_path) {
            Ok(b) => b,
            Err(_) => {
                eprintln!("skip: extracted/PROT/0888_sound_data2.BIN missing");
                return;
            }
        };
        let primary = extract_title_tim(&primary_bytes, TITLE_TIM_OFFSET).unwrap();

        for &(prot_idx, alt_offset) in TITLE_TIM_ALTERNATE_SOURCES {
            let alt_path = format!("../../extracted/PROT/{:04}_sound_data2.BIN", prot_idx);
            let alt_bytes = match std::fs::read(&alt_path) {
                Ok(b) => b,
                Err(_) => continue,
            };
            let alt = extract_title_tim(&alt_bytes, alt_offset).unwrap();
            assert_eq!(
                primary.bytes, alt.bytes,
                "PROT {} title TIM at 0x{:x} should byte-equal PROT 888",
                prot_idx, alt_offset
            );
        }
    }

    #[test]
    fn menu_band_constants_partition_the_packed_strip() {
        // The "NEW GAME CONTINUE" footer band at title-TIM y=227..237
        // is a single 128×10 strip. NEW_GAME samples the left half;
        // CONTINUE samples the right half. The two rects must abut
        // (NEW_GAME.x + NEW_GAME.w == CONTINUE.x) so engines can stack
        // them vertically without re-extracting bytes.
        let (ngx, ngy, ngw, ngh) = TITLE_BAND_MENU_NEW_GAME;
        let (cox, coy, _cow, coh) = TITLE_BAND_MENU_CONTINUE;
        assert_eq!(ngy, 227);
        assert_eq!(coy, 227);
        assert_eq!(ngh, 10);
        assert_eq!(coh, 10);
        assert_eq!(ngx + ngw, cox);
    }

    #[test]
    fn constants_are_internally_consistent() {
        // 256x256 8bpp + 256-colour CLUT.
        assert_eq!(TITLE_TIM_SIZE, 8 + (12 + 256 * 2) + (12 + 256 * 256));
        // 256x256 4bpp + 256-colour CLUT.
        assert_eq!(
            OVERLAY_SAVE_MENU_TIM_SIZE,
            8 + (12 + 256 * 2) + (12 + 128 * 256)
        );
        // 256x16 4bpp + 256-colour CLUT.
        assert_eq!(
            OVERLAY_CARD_ICON_TIM_SIZE,
            8 + (12 + 256 * 2) + (12 + 128 * 16)
        );
        // Alternate-source list shouldn't include the primary.
        for &(idx, _) in TITLE_TIM_ALTERNATE_SOURCES {
            assert_ne!(idx, PROT_INDEX_PRIMARY);
        }
    }
}
