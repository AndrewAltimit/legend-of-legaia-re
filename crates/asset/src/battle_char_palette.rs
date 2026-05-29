//! Battle party-character **palette** (the in-battle Vahn / Noa / Gala CLUTs).
//!
//! PORT: FUN_80052FA0  (decode + sub-record assembly)
//! REF:  FUN_8001A55C  (LZS, via [`legaia_lzs::decompress`])
//! REF:  FUN_80053B9C  (CLUT-to-VRAM upload; the STP bit-15 transform)
//! REF:  FUN_80052770  (player-file loader / disc index `char + 0x360`)
//!
//! The party's in-battle character CLUTs are **not** a plain disc blob: they
//! are embedded inside the per-character `data\battle\PLAYERn` file and only
//! materialise after the loader decodes a small record set and STP-copies the
//! CLUT structs to VRAM rows `481 + slot` (Vahn=481, Noa=482, Gala=483). This
//! module is the clean-room port of that decode+assembly, validated byte-exact
//! against a live battle VRAM capture.
//!
//! ## Why `a0` is an output-byte budget (the decode that used to diverge)
//!
//! `FUN_8001A55C`'s first argument is a **decompressed-byte budget**: it is
//! decremented once per literal and once per match-copied byte, and the loop
//! runs `while budget > 0`. Decoding a sub-record "standalone" without honoring
//! that budget runs off the end of the stream into the next record's bytes.
//! [`legaia_lzs::decompress`] already models this (`while out.len() < size`), so
//! the port just passes each record's stored budget.
//!
//! ## Pre-staged PLAYER-file layout (all offsets record0-relative)
//!
//! ```text
//! +0x00  u32  desc_off    descriptor-table offset (= end of record0 stream)
//! +0x04  u32  clut_a_off  offset of CLUT A within record0's DECODED output
//! +0x08  u32  clut_b_off  offset of CLUT B within record0's DECODED output
//! +0x0C  u32  budget      record0 decoded size
//! +0x10  ..   LZS stream for record0
//!
//! desc_off: 12-byte entries [u32 id, u32 running_a, u32 size]; the table runs
//!           while `a[i+1] == a[i] + size[i]`. sub#0 follows immediately, so
//!           sub0_off = desc_off + count*12.
//! sub0_off + 0x2000*k  (k = 0..5): a staged sub-record
//!   +0x00  u32  budget          sub-record decoded size
//!   +0x04  ..   LZS stream
//! ```
//!
//! ## Assembly (`FUN_80052FA0`, one 0x19000 work buffer)
//!
//! Decode record0 at offset 0; read CLUT A @`clut_a_off` and CLUT B
//! @`clut_b_off` *immediately* (the sub-records overwrite that region). Set
//! `cur = clut_a_off`. For each of the five sub-records: decode it at `cur`;
//! `adv = u32[cur+0x0C]`, `flag = u16[cur+0x12]`; if `flag != 0` the sub's
//! trailing CLUT sits at `cur + adv`; then `cur += adv`.
//!
//! A CLUT struct is `[u16 base][u16 count][count × u16 BGR555]`. Upload
//! (`FUN_80053B9C`) sets **bit 15 (STP / semi-transparency)** on every non-zero
//! colour; `0x0000` stays `0x0000`. `count == 0` structs are no-ops.

use anyhow::{Result, bail};

const WORK_SIZE: usize = 0x19000;
const SUB_STRIDE: usize = 0x2000;
const SUB_COUNT: usize = 5;

/// One CLUT band uploaded to a battle character's VRAM row. Colours are stored
/// in **disc form** (STP bit-15 clear); use [`PaletteBand::vram_words`] for the
/// runtime form the game writes to VRAM.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaletteBand {
    /// CLUT x-index (in 16-bit colour cells) within the VRAM row.
    pub base: u16,
    /// BGR555 colours exactly as decoded from the PLAYER file.
    pub colors: Vec<u16>,
}

impl PaletteBand {
    /// The colours as the game writes them to VRAM: bit 15 (STP) set on every
    /// non-zero colour, `0x0000` left untouched (`FUN_80053B9C`).
    pub fn vram_words(&self) -> Vec<u16> {
        self.colors
            .iter()
            .map(|&c| if c != 0 { c | 0x8000 } else { 0 })
            .collect()
    }
}

/// The decoded palette for one party character. Bands are in upload order; if
/// two bands share a `base` the later one wins (the earlier upload is
/// overwritten in VRAM).
#[derive(Debug, Clone, Default)]
pub struct BattleCharPalette {
    pub bands: Vec<PaletteBand>,
}

fn rd_u32(b: &[u8], o: usize) -> Result<u32> {
    b.get(o..o + 4)
        .map(|s| u32::from_le_bytes(s.try_into().unwrap()))
        .ok_or_else(|| anyhow::anyhow!("u32 read out of range at 0x{o:X}"))
}

fn rd_u16(b: &[u8], o: usize) -> Result<u16> {
    b.get(o..o + 2)
        .map(|s| u16::from_le_bytes(s.try_into().unwrap()))
        .ok_or_else(|| anyhow::anyhow!("u16 read out of range at 0x{o:X}"))
}

/// Read a `[u16 base][u16 count][count × u16]` CLUT struct from `buf` at `off`.
/// Returns `None` for a count-0 (no-op) struct. Errors only on truncation.
fn read_clut(buf: &[u8], off: usize) -> Result<Option<PaletteBand>> {
    let base = rd_u16(buf, off)?;
    let count = rd_u16(buf, off + 2)? as usize;
    if count == 0 {
        return Ok(None);
    }
    let start = off + 4;
    let end = start + count * 2;
    let bytes = buf
        .get(start..end)
        .ok_or_else(|| anyhow::anyhow!("CLUT colours out of range at 0x{start:X}..0x{end:X}"))?;
    let colors = bytes
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    Ok(Some(PaletteBand { base, colors }))
}

/// Locate sub#0 by walking the descriptor table (`a[i+1] == a[i] + size[i]`).
fn descriptor_table_end(file: &[u8], desc_off: usize) -> Result<usize> {
    let mut o = desc_off;
    let mut prev_end: Option<u32> = None;
    let mut count = 0usize;
    loop {
        // Need a full 12-byte entry to test the running-sum invariant.
        let (a, size) = match (rd_u32(file, o + 4), rd_u32(file, o + 8)) {
            (Ok(a), Ok(s)) => (a, s),
            _ => break,
        };
        if let Some(prev) = prev_end
            && a != prev
        {
            break;
        }
        prev_end = Some(a.wrapping_add(size));
        o += 12;
        count += 1;
        if count > 8192 {
            bail!("descriptor table did not terminate (>{count} entries)");
        }
    }
    if count == 0 {
        bail!("empty descriptor table at 0x{desc_off:X}");
    }
    Ok(o)
}

/// Parse a pre-staged `data\battle\PLAYERn` file into its battle CLUT bands.
///
/// `file` is the whole PLAYER file (record0 begins at offset 0).
pub fn parse_player_file(file: &[u8]) -> Result<BattleCharPalette> {
    let desc_off = rd_u32(file, 0)? as usize;
    let clut_a_off = rd_u32(file, 4)? as usize;
    let clut_b_off = rd_u32(file, 8)? as usize;
    let budget = rd_u32(file, 0xC)? as usize;
    if budget > WORK_SIZE {
        bail!("record0 budget 0x{budget:X} exceeds work buffer 0x{WORK_SIZE:X}");
    }
    let stream = file
        .get(0x10..)
        .ok_or_else(|| anyhow::anyhow!("file truncated before record0 stream"))?;

    // The work buffer mirrors the loader's single 0x19000 allocation: record0
    // decodes at offset 0, the sub-records overwrite from `clut_a_off` on.
    let mut work = vec![0u8; WORK_SIZE];
    let rec0 = legaia_lzs::decompress(stream, budget)?;
    work[..rec0.len()].copy_from_slice(&rec0);

    let mut bands: Vec<PaletteBand> = Vec::new();
    // CLUT A and CLUT B come from record0 and must be read before the
    // sub-records overwrite the region starting at `clut_a_off`.
    if let Some(b) = read_clut(&work, clut_a_off)? {
        bands.push(b);
    }
    if let Some(b) = read_clut(&work, clut_b_off)? {
        bands.push(b);
    }

    let sub0_off = descriptor_table_end(file, desc_off)?;
    let mut cur = clut_a_off;
    for k in 0..SUB_COUNT {
        let p = sub0_off + SUB_STRIDE * k;
        let sub_budget = rd_u32(file, p)? as usize;
        if cur + 0x14 > WORK_SIZE || cur.checked_add(sub_budget).is_none_or(|e| e > WORK_SIZE) {
            bail!("sub-record #{k} dst 0x{cur:X}+0x{sub_budget:X} overruns work buffer");
        }
        let sub_stream = file
            .get(p + 4..)
            .ok_or_else(|| anyhow::anyhow!("sub-record #{k} stream truncated"))?;
        let dec = legaia_lzs::decompress(sub_stream, sub_budget)?;
        work[cur..cur + dec.len()].copy_from_slice(&dec);

        let adv = rd_u32(&work, cur + 0x0C)? as usize;
        let flag = rd_u16(&work, cur + 0x12)?;
        let clut_pos = cur + adv;
        if flag != 0
            && let Some(b) = read_clut(&work, clut_pos)?
        {
            bands.push(b);
        }
        cur = clut_pos;
    }

    Ok(BattleCharPalette { bands })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// All-literal LZS encoding: control byte 0xFF (8 literals) + 8 bytes,
    /// repeated. Decodes to exactly `data` for any budget == data.len().
    fn lit_compress(data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        for chunk in data.chunks(8) {
            out.push(0xFF);
            out.extend_from_slice(chunk);
        }
        out
    }

    fn clut_struct(base: u16, colors: &[u16]) -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(&base.to_le_bytes());
        v.extend_from_slice(&(colors.len() as u16).to_le_bytes());
        for &c in colors {
            v.extend_from_slice(&c.to_le_bytes());
        }
        v
    }

    /// Build a minimal pre-staged PLAYER file: record0 carries CLUT B, sub#0
    /// carries a trailing CLUT, subs #1..#4 are flag-0 no-ops.
    #[test]
    fn assembles_record0_and_sub_cluts() {
        // --- record0 decoded image (0x100 bytes) ---
        let clut_a_off = 0x20usize; // empty (count 0)
        let clut_b_off = 0x40usize;
        let budget = 0x100usize;
        let mut rec0 = vec![0u8; budget];
        // CLUT A: count 0 -> no-op
        rec0[clut_a_off..clut_a_off + 4].copy_from_slice(&clut_struct(0x00, &[]));
        // CLUT B: base 0x10, two colours (one zero -> stays zero under STP)
        let cb = clut_struct(0x10, &[0x0000, 0x1234]);
        rec0[clut_b_off..clut_b_off + cb.len()].copy_from_slice(&cb);

        // --- sub#0 decoded image: header +0xC=adv, +0x12=flag, trailing CLUT ---
        let adv = 0x40usize;
        let mut sub0 = vec![0u8; 0x80];
        sub0[0x0C..0x10].copy_from_slice(&(adv as u32).to_le_bytes());
        sub0[0x12..0x14].copy_from_slice(&1u16.to_le_bytes()); // flag != 0
        // sub#0 CLUT lands at work[clut_a_off + adv]; within sub0 that's at
        // offset adv (since sub0 decodes at work[clut_a_off]).
        let sc = clut_struct(0x70, &[0x7FFF, 0x0001]);
        sub0[adv..adv + sc.len()].copy_from_slice(&sc);

        // flag-0 filler sub (valid 0x14-byte header, advance keeps us in range)
        let mut filler = vec![0u8; 0x20];
        filler[0x0C..0x10].copy_from_slice(&0x20u32.to_le_bytes()); // adv
        // flag stays 0

        // --- descriptor table: 2 running-sum entries, then sub#0 ---
        let mut desc = Vec::new();
        for (id, a, sz) in [(1u32, 0u32, 0x100u32), (2, 0x100, 0x100)] {
            desc.extend_from_slice(&id.to_le_bytes());
            desc.extend_from_slice(&a.to_le_bytes());
            desc.extend_from_slice(&sz.to_le_bytes());
        }

        // --- lay out the file ---
        let rec0_stream = lit_compress(&rec0);
        let mut file = Vec::new();
        // header (16 bytes) + record0 stream, then descriptor table
        let desc_off = 0x10 + rec0_stream.len();
        let sub0_off = desc_off + desc.len();
        file.extend_from_slice(&(desc_off as u32).to_le_bytes());
        file.extend_from_slice(&(clut_a_off as u32).to_le_bytes());
        file.extend_from_slice(&(clut_b_off as u32).to_le_bytes());
        file.extend_from_slice(&(budget as u32).to_le_bytes());
        file.extend_from_slice(&rec0_stream);
        file.extend_from_slice(&desc);
        debug_assert_eq!(file.len(), sub0_off);
        // five sub slots at 0x2000 stride
        let sub_images: [&[u8]; SUB_COUNT] = [&sub0, &filler, &filler, &filler, &filler];
        for (k, img) in sub_images.iter().enumerate() {
            let p = sub0_off + SUB_STRIDE * k;
            file.resize(p, 0);
            let stream = lit_compress(img);
            file.extend_from_slice(&(img.len() as u32).to_le_bytes());
            file.extend_from_slice(&stream);
        }

        let pal = parse_player_file(&file).expect("parse");
        // CLUT A (count 0) dropped; expect CLUT B then sub#0 CLUT.
        assert_eq!(pal.bands.len(), 2, "bands: {:?}", pal.bands);
        assert_eq!(pal.bands[0].base, 0x10);
        assert_eq!(pal.bands[0].colors, vec![0x0000, 0x1234]);
        assert_eq!(pal.bands[1].base, 0x70);
        assert_eq!(pal.bands[1].colors, vec![0x7FFF, 0x0001]);

        // STP transform: bit-15 set on non-zero, zero preserved.
        assert_eq!(pal.bands[0].vram_words(), vec![0x0000, 0x9234]);
        assert_eq!(pal.bands[1].vram_words(), vec![0xFFFF, 0x8001]);
    }

    #[test]
    fn rejects_oversized_budget() {
        let mut file = vec![0u8; 0x20];
        file[0xC..0x10].copy_from_slice(&0xFFFFu32.to_le_bytes()); // < WORK ok
        file[0xC..0x10].copy_from_slice(&(WORK_SIZE as u32 + 1).to_le_bytes());
        assert!(parse_player_file(&file).is_err());
    }
}
