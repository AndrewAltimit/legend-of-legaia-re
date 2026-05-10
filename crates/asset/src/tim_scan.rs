//! Find PSX TIMs embedded in arbitrary buffers.
//!
//! Mirrors [`crate::tmd_scan`] but for `legaia-tim`. PSX TIM magic is the
//! u32 `0x10` (`version=1`, `kind=0` packed into a single word). The TIM
//! parser does the heavy validation (CLUT block length sanity, image block
//! length sanity, w/h fit), so the scanner just hands every magic match to
//! the parser and keeps successful parses.
//!
//! Used by `asset tim-scan` to inventory every TIM across the PROT corpus -
//! companion to `tmd-scan`. Most character-mesh PROT entries (battle_data,
//! level_up, monster_se) host TIMs co-located with the TMDs they texture.

use legaia_lzs::decompress_container;
use legaia_tim as tim;

/// One TIM found inside a source buffer.
#[derive(Debug, Clone)]
pub struct Hit {
    /// Byte offset inside the source buffer where the magic lives.
    pub offset: usize,
    /// Total bytes occupied by the TIM (header + optional CLUT block + image).
    pub byte_len: usize,
    /// Pixel width.
    pub width: u32,
    /// Pixel height.
    pub height: u32,
    /// Bits per pixel (4, 8, 16, or 24).
    pub bpp: u32,
    /// Whether the TIM has an attached CLUT.
    pub has_clut: bool,
}

/// Where a hit was found relative to the original PROT entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Raw,
    Lzs(usize),
}

/// Per-PROT-entry scan result. Holds the decompressed LZS sections so the
/// caller can re-slice for `Source::Lzs(i)` hits without re-decoding.
#[derive(Debug, Clone)]
pub struct EntryScan {
    pub hits: Vec<(Source, Hit)>,
    pub lzs_sections: Vec<Vec<u8>>,
    pub lzs_ok: bool,
}

/// Scan one buffer for embedded TIMs. Walks every word-aligned offset.
pub fn scan_buffer(buf: &[u8]) -> Vec<Hit> {
    let mut hits = Vec::new();
    if buf.len() < 8 {
        return hits;
    }
    let mut off = 0usize;
    while off + 4 <= buf.len() {
        let magic = u32::from_le_bytes(buf[off..off + 4].try_into().unwrap());
        if magic == tim::TIM_MAGIC
            && let Some(hit) = try_parse_at(&buf[off..], off)
        {
            hits.push(hit);
        }
        off += 4;
    }
    hits
}

fn try_parse_at(slice: &[u8], source_off: usize) -> Option<Hit> {
    let parsed = tim::parse(slice).ok()?;
    let width = parsed.pixel_width() as u32;
    let height = parsed.pixel_height() as u32;
    if width == 0 || height == 0 {
        return None;
    }
    let bpp = match parsed.mode {
        tim::PixelMode::Bpp4 => 4,
        tim::PixelMode::Bpp8 => 8,
        tim::PixelMode::Bpp16 => 16,
        tim::PixelMode::Bpp24 => 24,
        tim::PixelMode::Mixed => 0, // 0 = unknown / non-uniform
    };
    let has_clut = parsed.clut.is_some();
    let byte_len = tim_byte_extent(&parsed);
    Some(Hit {
        offset: source_off,
        byte_len,
        width,
        height,
        bpp,
        has_clut,
    })
}

/// Total bytes occupied by a parsed TIM: 8-byte header + CLUT block (if any)
/// + image block. Each block is `12 + w*h*2` bytes.
fn tim_byte_extent(parsed: &tim::Tim) -> usize {
    let mut end = 8;
    if let Some(c) = &parsed.clut {
        end += 12 + (c.w as usize) * (c.h as usize) * 2;
    }
    end += 12 + (parsed.image.fb_w as usize) * (parsed.image.h as usize) * 2;
    end
}

/// Scan one PROT entry: raw scan first, then LZS-strict scan.
pub fn scan_entry(buf: &[u8]) -> EntryScan {
    let mut hits: Vec<(Source, Hit)> = scan_buffer(buf)
        .into_iter()
        .map(|h| (Source::Raw, h))
        .collect();

    let (lzs_sections, lzs_ok) = match decompress_container(buf) {
        Ok(sections) => {
            for (idx, section) in sections.iter().enumerate() {
                for h in scan_buffer(section) {
                    hits.push((Source::Lzs(idx), h));
                }
            }
            (sections, true)
        }
        Err(_) => (Vec::new(), false),
    };

    EntryScan {
        hits,
        lzs_sections,
        lzs_ok,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_buffer_returns_no_hits() {
        assert!(scan_buffer(&[]).is_empty());
        assert!(scan_buffer(&[0; 8]).is_empty());
    }

    #[test]
    fn random_bytes_with_magic_but_no_parse_is_rejected() {
        // Magic at offset 0, but rest is junk → parser must reject.
        let mut buf = vec![0u8; 64];
        buf[0..4].copy_from_slice(&tim::TIM_MAGIC.to_le_bytes());
        // bs_len wildly out of range
        buf[8..12].copy_from_slice(&0xFFFFFFFFu32.to_le_bytes());
        assert!(scan_buffer(&buf).is_empty());
    }
}
