//! The shared "interior" texture page + strip CLUT the town field meshes
//! sample at texpage `(960,256)` / CLUT row 510.
//!
//! A 256×256 4bpp TIM with image rect `(960,256)` 64×256 and a declared
//! **16×16 CLUT block** at `(0,510)` ships raw (uncompressed) at
//! `PROT.DAT` byte offset [`PROT_DAT_OFFSET`] - member 8 of the
//! **system-UI TIM-pack at raw PROT TOC entry 0** (sectors 3..55, the
//! head region the extraction index space skips, so no per-entry
//! extraction file carries it; see [`crate::system_ui_bundle`] for the
//! whole bundle and [`docs/formats/tim-pack.md`](../../../docs/formats/tim-pack.md)).
//! The retail per-TIM uploader `FUN_800198E0` uploads the CLUT block as
//! a **flat 256-entry strip** on row 510 (x = 0..256), NOT the declared
//! 16-row rect (which would overflow VRAM at y ≥ 512); the upload
//! happens once at boot, so the strip is byte-identical to VRAM row 510
//! in every captured save **from the title screen onward**, across
//! every scene and mode. Town scene meshes (23 town01 tile instances,
//! incl. the spawn plaza) reference CBA `(64,510)` = entry 64 of that
//! strip (declared sub-row 4) with texture pages inside the `(960,256)`
//! image - content that is otherwise absent from the town's own PROT
//! block, which is why the engine's per-scene targeted build can't
//! source it.
//!
//! ~70 of the image's 256 rows differ from this TIM in live VRAM: they
//! are overwritten **by the sibling bundle members** uploaded after it in
//! pack order - the UI sprite strip at `PROT.DAT[0x19438]` (image
//! `(960,400)` 60×24), the cursor parts at `0x19490..` (images at
//! `(976,256..)`), and the six bare row-patch blocks at `0x1A018..`
//! (single rows at `(960, 456..=458)` / `(960, 460..=462)`) - not by any
//! scene upload. Uploading the whole bundle in table order
//! ([`crate::system_ui_bundle::SystemUiBundle::upload_to_vram`])
//! reproduces the retail bytes exactly (byte-verified via the VRAM
//! static-mask parity oracle).

use std::io::{Read, Seek, SeekFrom};

use anyhow::{Context, Result, bail};
use legaia_tim::{Tim, Vram};

/// Byte offset of the family TIM inside the retail NA `PROT.DAT` image -
/// member 8 of the raw-TOC-entry-0 TIM-pack (entry-relative offset
/// `0xFA18` from sector 3); the only raw copy in the image.
pub const PROT_DAT_OFFSET: u64 = 0x11218;

/// Generous read window covering the TIM (header + 512-byte CLUT block +
/// 32 KiB image = 33 312 bytes).
pub const READ_WINDOW: usize = 0x9000;

/// Declared CLUT rect of the family TIM (the fingerprint [`find_in_entry`]
/// filters on): 16 palettes × 16 entries at `(0,510)`.
pub const CLUT_RECT: (u16, u16, u16, u16) = (0, 510, 16, 16);

/// Image rect of the family TIM: 64 VRAM words (256 4bpp texels) × 256
/// rows at `(960,256)`.
pub const IMAGE_RECT: (u16, u16, u16, u16) = (960, 256, 64, 256);

/// Read the family TIM straight out of a `PROT.DAT` image: a windowed
/// read at [`PROT_DAT_OFFSET`] validated by [`find_in_entry`]'s rect
/// fingerprint (so an unexpected image layout errors instead of placing
/// garbage in VRAM).
pub fn read_from_prot_dat(path: &std::path::Path) -> Result<Tim> {
    let mut f = std::fs::File::open(path)
        .with_context(|| format!("open PROT.DAT at {}", path.display()))?;
    f.seek(SeekFrom::Start(PROT_DAT_OFFSET))
        .context("seek to the interior-page TIM")?;
    let mut buf = vec![0u8; READ_WINDOW];
    let n = f.read(&mut buf).context("read the interior-page window")?;
    buf.truncate(n);
    find_in_entry(&buf).context("interior-page TIM not at its pinned PROT.DAT offset")
}

/// Find the family TIM inside a raw byte buffer: scan for strict-parsing
/// TIMs and return the one whose CLUT + image rects match [`CLUT_RECT`] /
/// [`IMAGE_RECT`].
pub fn find_in_entry(bytes: &[u8]) -> Result<Tim> {
    for hit in crate::tim_scan::scan_buffer(bytes) {
        let Ok(tim) = legaia_tim::parse(&bytes[hit.offset..]) else {
            continue;
        };
        let Some(clut) = tim.clut.as_ref() else {
            continue;
        };
        if (clut.fb_x, clut.fb_y, clut.w, clut.h) != CLUT_RECT {
            continue;
        }
        let img = &tim.image;
        if (img.fb_x, img.fb_y, img.fb_w, img.h) != IMAGE_RECT {
            continue;
        }
        return Ok(tim);
    }
    bail!(
        "no TIM with CLUT {:?} + image {:?} found in entry",
        CLUT_RECT,
        IMAGE_RECT
    );
}

/// Upload the page the way retail leaves it in VRAM: the image at its
/// declared rect, and the 16×16 CLUT block as a flat 256-entry strip on
/// row 510 starting at x = 0 (the same strip semantics as the
/// [`crate::field_char_textures`] row-478 character palettes).
pub fn upload_to_vram(tim: &Tim, vram: &mut Vram) {
    vram.upload_tim_partial(tim, true, false);
    if let Some(clut) = tim.clut.as_ref() {
        let mut bytes = Vec::with_capacity(clut.entries.len() * 2);
        for &c in &clut.entries {
            bytes.extend_from_slice(&c.to_le_bytes());
        }
        vram.write_clut_row(clut.fb_x, clut.fb_y, &bytes);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal synthetic TIM with the family's rects.
    fn synthetic_family_tim() -> Vec<u8> {
        let mut buf = Vec::new();
        buf.extend_from_slice(&0x10u32.to_le_bytes()); // magic
        buf.extend_from_slice(&0x08u32.to_le_bytes()); // 4bpp + CLUT
        // CLUT block: bnum = 12 + 16*16*2
        let centries = 16u32 * 16;
        buf.extend_from_slice(&(12 + centries * 2).to_le_bytes());
        buf.extend_from_slice(&0u16.to_le_bytes()); // x
        buf.extend_from_slice(&510u16.to_le_bytes()); // y
        buf.extend_from_slice(&16u16.to_le_bytes()); // w
        buf.extend_from_slice(&16u16.to_le_bytes()); // h
        for i in 0..centries {
            buf.extend_from_slice(&(i as u16 | 0x8000).to_le_bytes());
        }
        // Image block: 64 words x 4 rows (shrunk height keeps the test small
        // - find_in_entry filters on the real 256-row rect, so use it).
        let (w, h) = (64u16, 256u16);
        buf.extend_from_slice(&(12 + w as u32 * h as u32 * 2).to_le_bytes());
        buf.extend_from_slice(&960u16.to_le_bytes());
        buf.extend_from_slice(&256u16.to_le_bytes());
        buf.extend_from_slice(&w.to_le_bytes());
        buf.extend_from_slice(&h.to_le_bytes());
        buf.extend_from_slice(&vec![0xAB; w as usize * h as usize * 2]);
        buf
    }

    #[test]
    fn finds_and_uploads_strip() {
        // Embed at a non-zero offset to exercise the scan.
        let mut entry = vec![0u8; 64];
        entry.extend_from_slice(&synthetic_family_tim());
        let tim = find_in_entry(&entry).expect("family TIM found");
        let mut vram = Vram::new();
        upload_to_vram(&tim, &mut vram);
        // Strip lands on row 510 as 256 consecutive entries from x=0; the
        // entry the town meshes reference (x=64) holds block entry 64.
        assert_eq!(vram.pixel(64, 510), 64 | 0x8000);
        assert_eq!(vram.pixel(255, 510), 255 | 0x8000);
        // Row 511 untouched (the declared 16-row rect is NOT placed).
        assert_eq!(vram.pixel(0, 511), 0);
        // Image at its declared rect.
        assert_ne!(vram.pixel(960, 300), 0);
    }

    #[test]
    fn rejects_entry_without_family_tim() {
        assert!(find_in_entry(&[0u8; 256]).is_err());
    }
}
