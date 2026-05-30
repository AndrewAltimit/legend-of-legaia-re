//! Battle party-character **palette** (the in-battle Vahn / Noa / Gala CLUTs).
//!
//! PORT: FUN_80052FA0  (decode + sub-record assembly)
//! REF:  FUN_8001A55C  (LZS, via [`legaia_lzs::decompress`])
//! REF:  FUN_80053B9C  (CLUT-to-VRAM upload; the STP bit-15 transform)
//! REF:  FUN_80052770  (player-file loader / disc index `char + 0x360`)
//!
//! The party's in-battle character CLUTs are **not** a plain disc blob: they are
//! embedded inside the per-character battle record (`edstati3`, the data the
//! `data\battle\PLAYERn` path resolves to — a PROT entry, not an ISO file) and
//! only materialise after the loader decodes a small record set and STP-copies
//! the CLUT structs to VRAM rows `481 + slot` (Vahn=481, Noa=482, Gala=483).
//! This module is the clean-room port of that decode+assembly, validated
//! byte-exact against a live battle VRAM capture and against the on-disc data.
//!
//! ## Why `a0` is an output-byte budget (the decode that used to "diverge")
//!
//! `FUN_8001A55C`'s first argument is a **decompressed-byte budget**: it is
//! decremented once per literal and once per match-copied byte, and the loop
//! runs `while budget > 0`. Decoding a record "standalone" without honoring that
//! budget runs off the stream into the next record's bytes.
//! [`legaia_lzs::decompress`] already models this (`while out.len() < size`), so
//! the port just passes each record's stored budget.
//!
//! ## On-disc record layout (the parser is self-describing)
//!
//! `record0` begins at `rec0` (offset 0 of the resolved record; PROT `0861` has
//! a `"pochi…"` header so its copy sits at file `0x1000` — use [`find_record0`]):
//!
//! ```text
//! rec0+0x00  u32  desc_off    descriptor-table offset (rec0-relative)
//! rec0+0x04  u32  clut_a_off  offset of CLUT A within record0's DECODED output
//! rec0+0x08  u32  clut_b_off  offset of CLUT B
//! rec0+0x0C  u32  budget      record0 decoded size; LZS stream begins at +0x10
//! ```
//!
//! The descriptor table at `rec0+desc_off` is 12-byte entries `[u32 id, u32
//! running_a, u32 size]` that run while `a[i+1] == a[i] + size[i]`; `id == 0`
//! marks a section boundary. The five sub-records are located by:
//!
//! - `sec_base = rec0 + align_up(recbase - rec0, 0x2000)` (recbase = table end;
//!   the `0x2000` alignment is rec0-relative — `0x1000` happens to match for
//!   Vahn/Noa but misplaces Gala by 0x1000)
//! - sub0..3 = `sec_base + a[entry following each internal id=0 separator]`
//! - sub4    = `rec0 + (a_last + size_last)` (the descriptor's total span)
//!
//! (At load time `FUN_80052770` stages these five at a `0x2000` stride in RAM;
//! on disc they are scattered, and these offsets reproduce them exactly.) Each
//! sub-record is `[u32 budget][LZS stream]`.
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
//! colour; `0x0000` stays `0x0000`. `count == 0` structs are no-ops. Vahn's
//! three effective bands: `base 0x00` (record0 CLUT B), `0x40` (sub0's trailing
//! CLUT), `0x70` (sub4's trailing CLUT).

use anyhow::{Result, bail};

const WORK_SIZE: usize = 0x19000;

/// One CLUT band uploaded to a battle character's VRAM row. Colours are stored
/// in **disc form** (STP bit-15 clear); use [`PaletteBand::vram_words`] for the
/// runtime form the game writes to VRAM.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaletteBand {
    /// CLUT x-index (in 16-bit colour cells) within the VRAM row.
    pub base: u16,
    /// BGR555 colours exactly as decoded from the record.
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
    o.checked_add(4)
        .and_then(|e| b.get(o..e))
        .map(|s| u32::from_le_bytes(s.try_into().unwrap()))
        .ok_or_else(|| anyhow::anyhow!("u32 read out of range at 0x{o:X}"))
}

fn rd_u16(b: &[u8], o: usize) -> Result<u16> {
    o.checked_add(2)
        .and_then(|e| b.get(o..e))
        .map(|s| u16::from_le_bytes(s.try_into().unwrap()))
        .ok_or_else(|| anyhow::anyhow!("u16 read out of range at 0x{o:X}"))
}

/// Read a `[u16 base][u16 count][count × u16]` CLUT struct from `buf` at `off`.
/// Returns `None` for a count-0 (no-op) struct. Errors only on truncation.
fn read_clut(buf: &[u8], off: usize) -> Result<Option<PaletteBand>> {
    let base = rd_u16(buf, off)?;
    let count = rd_u16(buf, off.saturating_add(2))? as usize;
    if count == 0 {
        return Ok(None);
    }
    let start = off.saturating_add(4);
    let end = start.saturating_add(count * 2);
    let bytes = buf
        .get(start..end)
        .ok_or_else(|| anyhow::anyhow!("CLUT colours out of range at 0x{start:X}..0x{end:X}"))?;
    let colors = bytes
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    Ok(Some(PaletteBand { base, colors }))
}

/// A descriptor-table entry: `(id, running_a, size)`.
type DescEntry = (u32, u32, u32);

/// Walk the descriptor table at `desc_off` (entries `[id, running_a, size]`,
/// running while `a[i+1] == a[i] + size[i]`). Returns `(entries, recbase)`.
fn walk_descriptors(file: &[u8], desc_off: usize) -> Result<(Vec<DescEntry>, usize)> {
    let mut o = desc_off;
    let mut prev_end: Option<u32> = None;
    let mut entries = Vec::new();
    loop {
        let (id, a, size) = match (rd_u32(file, o), rd_u32(file, o + 4), rd_u32(file, o + 8)) {
            (Ok(id), Ok(a), Ok(s)) => (id, a, s),
            _ => break,
        };
        if let Some(prev) = prev_end
            && a != prev
        {
            break;
        }
        prev_end = Some(a.wrapping_add(size));
        entries.push((id, a, size));
        o += 12;
        if entries.len() > 8192 {
            bail!(
                "descriptor table did not terminate (>{} entries)",
                entries.len()
            );
        }
    }
    if entries.is_empty() {
        bail!("empty descriptor table at 0x{desc_off:X}");
    }
    Ok((entries, o))
}

/// Derive the five sub-record file offsets from the descriptor table, exactly as
/// the runtime loader stages them. See the module docs.
fn derive_sub_offsets(rec0: usize, entries: &[DescEntry], recbase: usize) -> Vec<usize> {
    // Saturating arithmetic throughout: a malformed candidate record (find_record0
    // probes many offsets) can carry garbage descriptor values, and `usize` is
    // 32-bit on wasm — an offset that overruns the file is rejected later by the
    // bounds checks in `parse_record`, so saturating to a huge value is safe.
    let sec_base = rec0 + (recbase.saturating_sub(rec0).saturating_add(0x1FFF) & !0x1FFF);
    let mut subs = Vec::new();
    // sub0..3: the record following each internal id=0 separator (a separator
    // that is the table's last entry is the terminator, not a section start).
    for (i, (id, _, _)) in entries.iter().enumerate() {
        if *id == 0 && i + 1 < entries.len() {
            subs.push(sec_base.saturating_add(entries[i + 1].1 as usize));
        }
    }
    // sub4: the byte just past the whole descriptor span.
    let (_, a_last, sz_last) = entries[entries.len() - 1];
    subs.push(rec0.saturating_add((a_last.saturating_add(sz_last)) as usize));
    subs
}

/// Parse the battle-CLUT bands out of a character's `edstati3` record. `rec0` is
/// the record0 header offset (0 for a bare record; see [`find_record0`] for the
/// PROT `0861` `"pochi"`-padded copy).
pub fn parse_record(file: &[u8], rec0: usize) -> Result<BattleCharPalette> {
    let desc_off = rec0 + rd_u32(file, rec0)? as usize;
    let clut_a_off = rd_u32(file, rec0 + 4)? as usize;
    let clut_b_off = rd_u32(file, rec0 + 8)? as usize;
    let budget = rd_u32(file, rec0 + 0xC)? as usize;
    if budget > WORK_SIZE {
        bail!("record0 budget 0x{budget:X} exceeds work buffer 0x{WORK_SIZE:X}");
    }
    let stream = file
        .get(rec0 + 0x10..)
        .ok_or_else(|| anyhow::anyhow!("file truncated before record0 stream"))?;

    // The work buffer mirrors the loader's single 0x19000 allocation: record0
    // decodes at offset 0, the sub-records overwrite from `clut_a_off` on.
    let mut work = vec![0u8; WORK_SIZE];
    let rec0_out = legaia_lzs::decompress(stream, budget)?;
    work[..rec0_out.len()].copy_from_slice(&rec0_out);

    let mut bands: Vec<PaletteBand> = Vec::new();
    // CLUT A and CLUT B come from record0 and must be read before the
    // sub-records overwrite the region starting at `clut_a_off`.
    if let Some(b) = read_clut(&work, clut_a_off)? {
        bands.push(b);
    }
    if let Some(b) = read_clut(&work, clut_b_off)? {
        bands.push(b);
    }

    let (entries, recbase) = walk_descriptors(file, desc_off)?;
    let sub_offsets = derive_sub_offsets(rec0, &entries, recbase);

    let mut cur = clut_a_off;
    for (k, &p) in sub_offsets.iter().enumerate() {
        let sub_budget = rd_u32(file, p)? as usize;
        if cur.saturating_add(0x14) > WORK_SIZE
            || cur.checked_add(sub_budget).is_none_or(|e| e > WORK_SIZE)
        {
            bail!("sub-record #{k} dst 0x{cur:X}+0x{sub_budget:X} overruns work buffer");
        }
        let sub_stream = file
            .get(p.saturating_add(4)..)
            .ok_or_else(|| anyhow::anyhow!("sub-record #{k} at 0x{p:X} stream truncated"))?;
        let dec = legaia_lzs::decompress(sub_stream, sub_budget)?;
        work[cur..cur + dec.len()].copy_from_slice(&dec);

        let adv = rd_u32(&work, cur + 0x0C)? as usize;
        let flag = rd_u16(&work, cur + 0x12)?;
        let clut_pos = cur.saturating_add(adv);
        if flag != 0
            && let Some(b) = read_clut(&work, clut_pos)?
        {
            bands.push(b);
        }
        cur = clut_pos;
    }

    Ok(BattleCharPalette { bands })
}

/// Locate record0 inside a PROT `edstati3` entry, skipping any `"pochi"` filler
/// header. Scans 4-byte-aligned offsets for a header whose fields validate (a
/// sane budget and CLUT offsets that fall inside the decoded record).
pub fn find_record0(file: &[u8]) -> Option<usize> {
    let mut o = 0;
    while o + 0x10 <= file.len() {
        let desc_off = u32::from_le_bytes(file[o..o + 4].try_into().unwrap()) as usize;
        let clut_a = u32::from_le_bytes(file[o + 4..o + 8].try_into().unwrap()) as usize;
        let clut_b = u32::from_le_bytes(file[o + 8..o + 12].try_into().unwrap()) as usize;
        let budget = u32::from_le_bytes(file[o + 12..o + 16].try_into().unwrap()) as usize;
        let plausible = (0x100..file.len() - o).contains(&desc_off)
            && (0x1000..=WORK_SIZE).contains(&budget)
            && clut_a < budget
            && clut_b < budget
            && clut_a >= 0x10
            && clut_b >= 0x10;
        if plausible && parse_record(file, o).is_ok() {
            return Some(o);
        }
        o += 4;
    }
    None
}

/// Collect a character's battle palette the **equipment-robust** way: gather
/// CLUT bands from `record0`'s CLUT A/B plus every section *separator* (`id == 0`)
/// record's flagged trailing CLUT and the trailing "final" record, then keep only
/// bands whose base is one of the columns the character's mesh actually samples
/// (`mesh_cols`, the distinct `(cba & 0x3F) * 16` of the battle TMD).
///
/// [`parse_record`] reproduces a *specific* equipment configuration — its
/// fixed-stride assembly is exact for Vahn's tutorial state, but a character with
/// more equipment variants overflows the `0x19000` work buffer. On disc each band
/// ships once per equipment id plus an `id == 0` separator (the **unequipped
/// default**); this takes the separator default and lets the mesh's sampled
/// columns pick which bands belong to the character. Validated against a
/// full-party battle VRAM capture: Noa (PROT 0864) covers every sampled column at
/// ~98% (misses are equipment patches in the late-game reference).
pub fn collect_palette(file: &[u8], rec0: usize, mesh_cols: &[u16]) -> Result<BattleCharPalette> {
    let desc_off = rec0 + rd_u32(file, rec0)? as usize;
    let clut_a_off = rd_u32(file, rec0 + 4)? as usize;
    let clut_b_off = rd_u32(file, rec0 + 8)? as usize;
    let budget = rd_u32(file, rec0 + 0xC)? as usize;
    if budget > WORK_SIZE {
        bail!("record0 budget 0x{budget:X} exceeds work buffer 0x{WORK_SIZE:X}");
    }
    let stream = file
        .get(rec0 + 0x10..)
        .ok_or_else(|| anyhow::anyhow!("file truncated before record0 stream"))?;
    let rec0_out = legaia_lzs::decompress(stream, budget)?;

    let mut bands: Vec<PaletteBand> = Vec::new();
    let keep = |bands: &mut Vec<PaletteBand>, band: PaletteBand| {
        if mesh_cols.contains(&band.base) && !bands.iter().any(|b| b.base == band.base) {
            bands.push(band);
        }
    };

    for &off in &[clut_a_off, clut_b_off] {
        if let Some(b) = read_clut(&rec0_out, off)? {
            keep(&mut bands, b);
        }
    }

    let (entries, recbase) = walk_descriptors(file, desc_off)?;
    let sec_base = rec0 + (recbase.saturating_sub(rec0).saturating_add(0x1FFF) & !0x1FFF);
    let (_, a_last, sz_last) = entries[entries.len() - 1];
    let total = a_last.saturating_add(sz_last) as usize;

    // Each section separator's record (the unequipped default) + the final record.
    let mut sub_offsets: Vec<usize> = entries
        .iter()
        .filter(|(id, _, _)| *id == 0)
        .map(|(_, a, _)| sec_base.saturating_add(*a as usize))
        .collect();
    sub_offsets.push(rec0.saturating_add(total));

    for p in sub_offsets {
        let Some(stream) = file.get(p.saturating_add(4)..) else {
            continue;
        };
        let Ok(blen) = rd_u32(file, p) else {
            continue;
        };
        let blen = blen as usize;
        if !(0x400..=0x20000).contains(&blen) {
            continue;
        }
        let Ok(dec) = legaia_lzs::decompress(stream, blen) else {
            continue;
        };
        if dec.len() < 0x14 {
            continue;
        }
        let flag = rd_u16(&dec, 0x12)?;
        let adv = rd_u32(&dec, 0xC)? as usize;
        if flag != 0
            && let Ok(Some(b)) = read_clut(&dec, adv)
        {
            keep(&mut bands, b);
        }
    }

    bands.sort_by_key(|b| b.base);
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

    fn desc_entry(id: u32, a: u32, size: u32) -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(&id.to_le_bytes());
        v.extend_from_slice(&a.to_le_bytes());
        v.extend_from_slice(&size.to_le_bytes());
        v
    }

    /// Build a self-deriving `edstati3`-shape record: record0 carries CLUT B,
    /// one descriptor separator yields one section-head sub (with a trailing
    /// CLUT), and the final sub (`rec0 + total`) carries another.
    #[test]
    fn parses_self_deriving_record() {
        let clut_a_off = 0x20usize; // empty (count 0)
        let clut_b_off = 0x40usize;
        let budget = 0x100usize;
        let mut rec0 = vec![0u8; budget];
        rec0[clut_a_off..clut_a_off + 4].copy_from_slice(&clut_struct(0x00, &[]));
        let cb = clut_struct(0x10, &[0x0000, 0x1234]);
        rec0[clut_b_off..clut_b_off + cb.len()].copy_from_slice(&cb);

        // A sub image: header +0xC=adv, +0x12=flag, trailing CLUT at +adv.
        let make_sub = |base: u16, colors: &[u16], flag: u16| {
            let adv = 0x40usize;
            let mut img = vec![0u8; 0x80];
            img[0x0C..0x10].copy_from_slice(&(adv as u32).to_le_bytes());
            img[0x12..0x14].copy_from_slice(&flag.to_le_bytes());
            if flag != 0 {
                let sc = clut_struct(base, colors);
                img[adv..adv + sc.len()].copy_from_slice(&sc);
            }
            img
        };
        let sub_head = make_sub(0x40, &[0x0506, 0x0708], 1);
        let sub_final = make_sub(0x70, &[0x7FFF, 0x0001], 1);

        // Descriptor: a few running-sum entries with one internal id=0 separator
        // (yields one section-head sub) and a terminating entry. The section
        // head's `a` must point sub_head at sec_base + a.
        let stream0 = lit_compress(&rec0);
        let desc_off = 0x10 + stream0.len();
        // recbase = desc_off + 4*12; sec_base = align_up(recbase, 0x2000).
        // Four entries: [run, separator(id0), section-head, terminator(id0)].
        // derive_sub_offsets => sub_head @ sec_base + a_head, sub_final @ total.
        let recbase = desc_off + 4 * 12;
        let sec_base = (recbase + 0x1FFF) & !0x1FFF;
        let a_head = 0x800u32;
        let total = 0x1000u32; // a_last + size_last of the terminator entry

        let mut desc = Vec::new();
        desc.extend_from_slice(&desc_entry(1, 0x0, 0x400)); // run
        desc.extend_from_slice(&desc_entry(0, 0x400, 0x400)); // separator
        desc.extend_from_slice(&desc_entry(2, a_head, 0x400)); // section head
        desc.extend_from_slice(&desc_entry(0, 0xC00, total - 0xC00)); // terminator

        let mut file = Vec::new();
        file.extend_from_slice(&(desc_off as u32).to_le_bytes());
        file.extend_from_slice(&(clut_a_off as u32).to_le_bytes());
        file.extend_from_slice(&(clut_b_off as u32).to_le_bytes());
        file.extend_from_slice(&(budget as u32).to_le_bytes());
        file.extend_from_slice(&stream0);
        file.extend_from_slice(&desc);
        debug_assert_eq!(file.len(), recbase);

        let place = |file: &mut Vec<u8>, off: usize, img: &[u8]| {
            if file.len() < off {
                file.resize(off, 0);
            }
            file.extend_from_slice(&(img.len() as u32).to_le_bytes());
            file.extend_from_slice(&lit_compress(img));
        };
        let head_off = sec_base + a_head as usize;
        let final_off = total as usize; // rec0 == 0 here
        let mut ordered = [(head_off, &sub_head), (final_off, &sub_final)];
        ordered.sort_by_key(|(o, _)| *o);
        for (off, img) in ordered {
            place(&mut file, off, img);
        }

        let pal = parse_record(&file, 0).expect("parse");
        // CLUT A dropped (count 0); expect CLUT B, sub_head, sub_final.
        let by_base: std::collections::BTreeMap<u16, &PaletteBand> =
            pal.bands.iter().map(|b| (b.base, b)).collect();
        assert_eq!(by_base[&0x10].colors, vec![0x0000, 0x1234]);
        assert_eq!(by_base[&0x40].colors, vec![0x0506, 0x0708]);
        assert_eq!(by_base[&0x70].colors, vec![0x7FFF, 0x0001]);
        assert_eq!(by_base[&0x70].vram_words(), vec![0xFFFF, 0x8001]);
    }

    #[test]
    fn rejects_oversized_budget() {
        let mut file = vec![0u8; 0x20];
        file[0xC..0x10].copy_from_slice(&(WORK_SIZE as u32 + 1).to_le_bytes());
        assert!(parse_record(&file, 0).is_err());
    }

    /// `collect_palette`: record0 CLUT B + a separator record's trailing CLUT +
    /// the final record, with a non-mesh-column band filtered out.
    #[test]
    fn collect_filters_to_mesh_columns() {
        let clut_a_off = 0x20usize; // empty
        let clut_b_off = 0x40usize;
        let budget = 0x100usize;
        let mut rec0 = vec![0u8; budget];
        rec0[clut_a_off..clut_a_off + 4].copy_from_slice(&clut_struct(0x00, &[]));
        let cb = clut_struct(0x10, &[0x0000, 0x1234]);
        rec0[clut_b_off..clut_b_off + cb.len()].copy_from_slice(&cb);

        // Sub images must be >= the 0x400 budget floor `collect_palette` uses
        // (real sub-records decode to >= 0x3C0C bytes).
        let make_sub = |base: u16, colors: &[u16]| {
            let adv = 0x40usize;
            let mut img = vec![0u8; 0x440];
            img[0x0C..0x10].copy_from_slice(&(adv as u32).to_le_bytes());
            img[0x12..0x14].copy_from_slice(&1u16.to_le_bytes());
            let sc = clut_struct(base, colors);
            img[adv..adv + sc.len()].copy_from_slice(&sc);
            img
        };
        let sep0_sub = make_sub(0x40, &[0x0506, 0x0708]); // mesh col -> kept
        let sep1_sub = make_sub(0x90, &[0x0A0B, 0x0C0D]); // NOT a mesh col -> dropped
        let final_sub = make_sub(0x70, &[0x7FFF, 0x0001]); // mesh col -> kept

        // Descriptor: two id=0 separators with the running-sum invariant.
        let mut desc = Vec::new();
        desc.extend_from_slice(&desc_entry(0, 0x0, 0x400)); // separator 0
        desc.extend_from_slice(&desc_entry(0, 0x400, 0x400)); // separator 1 (last)
        let total = 0x800u32; // a_last + size_last

        let stream0 = lit_compress(&rec0);
        let desc_off = 0x10 + stream0.len();
        let recbase = desc_off + 2 * 12;
        let sec_base = (recbase + 0x1FFF) & !0x1FFF;

        let mut file = Vec::new();
        file.extend_from_slice(&(desc_off as u32).to_le_bytes());
        file.extend_from_slice(&(clut_a_off as u32).to_le_bytes());
        file.extend_from_slice(&(clut_b_off as u32).to_le_bytes());
        file.extend_from_slice(&(budget as u32).to_le_bytes());
        file.extend_from_slice(&stream0);
        file.extend_from_slice(&desc);

        let place = |file: &mut Vec<u8>, off: usize, img: &[u8]| {
            if file.len() < off {
                file.resize(off, 0);
            }
            file.extend_from_slice(&(img.len() as u32).to_le_bytes());
            file.extend_from_slice(&lit_compress(img));
        };
        // sep0 @ sec_base+0, sep1 @ sec_base+0x400, final @ rec0+total (0x800).
        let mut placements = [
            (total as usize, &final_sub),
            (sec_base, &sep0_sub),
            (sec_base + 0x400, &sep1_sub),
        ];
        placements.sort_by_key(|(o, _)| *o);
        for (off, img) in placements {
            place(&mut file, off, img);
        }

        let pal = collect_palette(&file, 0, &[0x10, 0x40, 0x70]).expect("collect");
        let bases: Vec<u16> = pal.bands.iter().map(|b| b.base).collect();
        assert_eq!(bases, vec![0x10, 0x40, 0x70], "0x90 should be filtered out");
        let by: std::collections::BTreeMap<u16, &PaletteBand> =
            pal.bands.iter().map(|b| (b.base, b)).collect();
        assert_eq!(by[&0x10].colors, vec![0x0000, 0x1234]);
        assert_eq!(by[&0x40].colors, vec![0x0506, 0x0708]);
        assert_eq!(by[&0x70].colors, vec![0x7FFF, 0x0001]);
    }
}
