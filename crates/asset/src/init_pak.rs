//! Boot `init.pak` parser - extracts the four publisher-logo TIMs from
//! PROT entry 0895 (`bat_back_dat` per CDNAME, actually init.pak).
//!
//! ## Format
//!
//! ```text
//! [u32 LE size_a]
//! [u32 LE flags/version]
//! [u32 LE reserved]
//! [u32 LE size_b]
//! [debug string pool (null-terminated, 16-byte aligned)]
//!   "init program \n"
//!   "h:\prot\field\init\init.pak"
//!   "h:\prot\field\title\title.pak"
//!   "h:\mpack\monster.snd"
//! [pointer / function-table region - references into 0x801CExxx, varies]
//! ...
//! [four PSX TIM blobs at fixed file offsets]
//!   0x021C4: PROKION logo  - 8bpp 88x256, CLUT 256 colours at fb=(0,480),  pixels at fb=(768,0)
//!   0x0D3E4: Contrail logo - 8bpp 92x256, CLUT 256 colours at fb=(0,480),  pixels at fb=(0,  0)
//!   0x18E04: SCEA logo     - 4bpp 64x128, CLUT  16 colours at fb=(0,497),  pixels at fb=(640,0)
//!   0x1CE44: WARNING logo  - 4bpp 64x256, CLUT  16 colours at fb=(0,492),  pixels at fb=(704,0)
//! ```
//!
//! The TIM byte offsets are stable across the known build (NA retail).
//! Each TIM is uncompressed and matches the RAM-extracted publisher-logo
//! frames byte-for-byte modulo the runtime RECT fixup. See
//! [`docs/formats/prot.md`](../../../../docs/formats/prot.md) and
//! [`docs/tooling/overlay-capture.md`](../../../../docs/tooling/overlay-capture.md)
//! for the methodology that validated these.
//!
//! The pack also contains debug strings referencing `init.pak` /
//! `title.pak` paths, but those are debug-print referents only - SCUS
//! doesn't resolve them by name. The four TIMs are the only consumable
//! payload in the retail build.

use anyhow::{Context, Result};

/// PROT entry index for `init.pak` in the NA retail build.
pub const PROT_INDEX: u16 = 895;

/// PSX TIM magic word (`0x00000010` LE).
const TIM_MAGIC: u32 = 0x0000_0010;

/// Number of publisher-logo TIMs in `init.pak`.
pub const PUBLISHER_LOGO_COUNT: usize = 4;

/// One publisher-logo TIM extracted from `init.pak`.
#[derive(Debug, Clone)]
pub struct PublisherLogoTim<'a> {
    /// File offset within PROT 0895 where this TIM blob begins.
    pub file_offset: usize,
    /// Total byte length of the TIM (header + CLUT block + pixel block).
    pub byte_len: usize,
    /// Slice of the input buffer pointing at the TIM blob; parse with
    /// [`legaia_tim::parse_tim`] or similar.
    pub bytes: &'a [u8],
    /// PSX VRAM target rect for the pixel block - `(fb_x, fb_y, w, h)`.
    /// The CLUT block goes into `clut_rect`.
    pub pixel_rect: (u16, u16, u16, u16),
    /// PSX VRAM target rect for the CLUT block - `(fb_x, fb_y, w, h)`.
    pub clut_rect: (u16, u16, u16, u16),
    /// TIM colour mode (`0`=4bpp, `1`=8bpp, `2`=15bpp, `3`=24bpp).
    pub mode: u8,
}

/// Parsed `init.pak` view - references into the input buffer (no copy).
#[derive(Debug, Clone)]
pub struct InitPak<'a> {
    /// The four publisher-logo TIMs in the canonical display order:
    /// `[PROKION, Contrail, SCEA, WARNING]`.
    pub logos: [PublisherLogoTim<'a>; PUBLISHER_LOGO_COUNT],
}

/// Parse the boot `init.pak` (PROT 0895) bytes.
///
/// Returns an error if the file doesn't look like init.pak (wrong
/// magic, missing TIMs at expected offsets, or the TIM headers don't
/// validate).
pub fn parse(bytes: &[u8]) -> Result<InitPak<'_>> {
    // The TIMs live at fixed byte offsets in the NA retail build. We
    // validate each header before treating the file as init.pak.
    const TIM_OFFSETS: [usize; PUBLISHER_LOGO_COUNT] = [0x021C4, 0x0D3E4, 0x18E04, 0x1CE44];

    if bytes.len() < 0x30000 {
        anyhow::bail!(
            "init.pak too small ({} bytes), expected at least 0x30000",
            bytes.len()
        );
    }

    let mut logos = Vec::with_capacity(PUBLISHER_LOGO_COUNT);
    for (idx, &off) in TIM_OFFSETS.iter().enumerate() {
        let logo = parse_tim_at(bytes, off)
            .with_context(|| format!("init.pak TIM #{} at offset 0x{:x}", idx, off))?;
        logos.push(logo);
    }

    Ok(InitPak {
        logos: [
            logos.remove(0),
            logos.remove(0),
            logos.remove(0),
            logos.remove(0),
        ],
    })
}

fn parse_tim_at(bytes: &[u8], off: usize) -> Result<PublisherLogoTim<'_>> {
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
        anyhow::bail!("bad TIM magic 0x{:08x} (expected 0x10)", magic);
    }
    let flags = read_u32(off + 4)?;
    let mode = (flags & 0x7) as u8;
    let has_clut = (flags & 0x8) != 0;
    if mode > 3 {
        anyhow::bail!("invalid TIM mode {}", mode);
    }
    if !has_clut {
        anyhow::bail!("publisher-logo TIM expected to have CLUT (flags bit 3)");
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

    Ok(PublisherLogoTim {
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

    /// Disc-gated: parse a real PROT 0895 entry. Skips when extracted/
    /// is missing (CI runs without disc data).
    #[test]
    fn parses_real_init_pak_when_disc_extracted() {
        let path = "../../extracted/PROT/0895_bat_back_dat.BIN";
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(_) => {
                eprintln!("skip: extracted/PROT/0895_bat_back_dat.BIN missing");
                return;
            }
        };
        let pak = parse(&bytes).expect("parse init.pak");

        // Verify the canonical layout: 4 TIMs at known offsets with
        // expected VRAM rects.
        assert_eq!(pak.logos[0].file_offset, 0x021C4);
        assert_eq!(pak.logos[0].mode, 1); // PROKION: 8bpp
        assert_eq!(pak.logos[0].pixel_rect, (768, 0, 88, 256));
        assert_eq!(pak.logos[0].clut_rect, (0, 480, 256, 1));

        assert_eq!(pak.logos[1].file_offset, 0x0D3E4);
        assert_eq!(pak.logos[1].mode, 1); // Contrail: 8bpp
        assert_eq!(pak.logos[1].pixel_rect, (0, 0, 92, 256));

        assert_eq!(pak.logos[2].file_offset, 0x18E04);
        assert_eq!(pak.logos[2].mode, 0); // SCEA: 4bpp
        assert_eq!(pak.logos[2].pixel_rect, (640, 0, 64, 128));

        assert_eq!(pak.logos[3].file_offset, 0x1CE44);
        assert_eq!(pak.logos[3].mode, 0); // WARNING: 4bpp
        assert_eq!(pak.logos[3].pixel_rect, (704, 0, 64, 256));
    }
}
