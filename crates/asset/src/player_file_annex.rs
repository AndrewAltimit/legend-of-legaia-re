//! Player battle files with their slot region **annexed** outside the PROT
//! entry.
//!
//! The retail loader (`FUN_80052770`) reads a player file in two halves:
//! a fixed 16-sector prologue (`FUN_800559EC(…, 0x8000)`: header,
//! `record[0]`, descriptor table) from the entry's TOC start LBA, then the
//! five equipment-selected slots by seeking **forward** from the end of the
//! previous read by the descriptor's byte offset (case 5 turns the offsets
//! into gaps, `FUN_80055A5C` shifts the gap to sectors and adds it to the
//! CD position). The offsets are 32-bit and relative, nothing bounds them
//! by the entry's TOC span (the span `FUN_8003E8A8` returns is discarded by
//! `FUN_800558FC`), and the seek is a plain LBA add - so a file's slot region
//! can sit anywhere later on the disc as long as its records keep chain
//! order. That is what lets a rebuilt file that outgrows the PROT pool keep
//! its header in place and park its records in `DMY.DAT` (never loaded by
//! retail; Form 1 sectors) without a relayout: a same-size image, still a PPF.
//!
//! This module is the pure half: telling an annexed table from a retail one,
//! splitting a retail-shaped rebuilt file into the in-place header and the
//! remote slot region, and materialising the retail-shaped file back from
//! the two so every existing parser keeps working. The disc I/O (where the
//! annex lives, reading it back) is the patcher's.

use anyhow::{Result, bail};

/// Sector size; every slot offset and size is a multiple of it.
pub const SECTOR: usize = 0x800;

/// One descriptor-table row as it sits in the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Row {
    pub id: u32,
    pub offset: u32,
    pub size: u32,
}

/// The descriptor chain of a player file, wherever it points.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chain {
    /// Byte offset of the table inside the file (header word 0).
    pub table_offset: usize,
    /// Offset of the first record from the data base - `0` on a retail file,
    /// the annex displacement on an annexed one.
    pub base: u32,
    /// Rows in chain order (terminator excluded).
    pub rows: Vec<Row>,
}

impl Chain {
    /// Total bytes of the slot region (`sum(size)`).
    pub fn region_len(&self) -> usize {
        self.rows.iter().map(|r| r.size as usize).sum()
    }

    /// `true` when the records live outside the entry's own sectors.
    pub fn is_annexed(&self) -> bool {
        self.base != 0
    }
}

/// Walk the descriptor table without assuming the chain starts at 0: the
/// first row's offset is the base, every later row must start where the
/// previous ended. `None` when the head does not read as a player file at
/// all (see `battle_data_pack::detect` for the header signature).
pub fn chain(buf: &[u8]) -> Option<Chain> {
    if buf.len() < 0x10 {
        return None;
    }
    let word = legaia_bytes::u32_le(buf, 0)?;
    if (word >> 24) != 0 {
        return None;
    }
    let table_offset = word as usize;
    if table_offset < 0x10 || table_offset + 12 > buf.len() {
        return None;
    }
    let clut_a = legaia_bytes::u32_le(buf, 4)?;
    let clut_b = legaia_bytes::u32_le(buf, 8)?;
    let budget = legaia_bytes::u32_le(buf, 12)?;
    if clut_a == 0 || clut_a >= clut_b || clut_b >= budget {
        return None;
    }
    let mut rows = Vec::new();
    let mut base = 0u32;
    let mut expected: Option<u32> = None;
    let mut p = table_offset;
    while p + 12 <= buf.len() && rows.len() < 256 {
        let id = legaia_bytes::u32_le(buf, p)?;
        let offset = legaia_bytes::u32_le(buf, p + 4)?;
        let size = legaia_bytes::u32_le(buf, p + 8)?;
        if size == 0 {
            if id != 0 || offset != 0 {
                return None;
            }
            break;
        }
        if id > 0xFF || size > 0x40_0000 || !(size as usize).is_multiple_of(SECTOR) {
            return None;
        }
        match expected {
            None => {
                if !(offset as usize).is_multiple_of(SECTOR) {
                    return None;
                }
                base = offset;
            }
            Some(e) if e != offset => return None,
            _ => {}
        }
        rows.push(Row { id, offset, size });
        expected = Some(offset.checked_add(size)?);
        p += 12;
    }
    if rows.is_empty() {
        return None;
    }
    Some(Chain {
        table_offset,
        base,
        rows,
    })
}

/// Rewrite every row's offset by `delta` (wrapping), in place in `buf`'s
/// table. The chain shape is untouched; only where it points changes.
fn shift_table(buf: &mut [u8], chain: &Chain, delta: u32) {
    for (i, row) in chain.rows.iter().enumerate() {
        let p = chain.table_offset + i * 12 + 4;
        let v = row.offset.wrapping_add(delta);
        buf[p..p + 4].copy_from_slice(&v.to_le_bytes());
    }
}

/// Split a retail-shaped rebuilt file (chain from 0, records from
/// `data_base`) into the header to write in place - `data_base` bytes with
/// the table displaced by `base` - and the slot region to park at the
/// annex. `base` is the byte distance from the entry's data base to the
/// annex, a whole number of sectors.
pub fn split(file: &[u8], data_base: usize, base: u32) -> Result<(Vec<u8>, Vec<u8>)> {
    let Some(c) = chain(file) else {
        bail!("not a player file");
    };
    if c.is_annexed() {
        bail!("file is already annexed (chain base 0x{:X})", c.base);
    }
    if !(base as usize).is_multiple_of(SECTOR) || base == 0 {
        bail!("annex base 0x{base:X} is not a positive sector multiple");
    }
    let region_len = c.region_len();
    if data_base + region_len > file.len() {
        bail!(
            "slot region runs past the file ({} + {} > {})",
            data_base,
            region_len,
            file.len()
        );
    }
    let mut header = file[..data_base].to_vec();
    shift_table(&mut header, &c, base);
    let region = file[data_base..data_base + region_len].to_vec();
    Ok((header, region))
}

/// Rebuild the retail-shaped file from an annexed header (`data_base`
/// bytes, table displaced) and the bytes read back from the annex. The
/// result parses with every retail-shaped reader.
pub fn materialize(header: &[u8], data_base: usize, region: &[u8]) -> Result<Vec<u8>> {
    let Some(c) = chain(header) else {
        bail!("not a player file header");
    };
    if !c.is_annexed() {
        bail!("header is not annexed");
    }
    if header.len() < data_base {
        bail!("header shorter than its data base");
    }
    if region.len() < c.region_len() {
        bail!(
            "annex region is {} bytes, the chain needs {}",
            region.len(),
            c.region_len()
        );
    }
    let mut file = header[..data_base].to_vec();
    shift_table(&mut file, &c, c.base.wrapping_neg());
    file.extend_from_slice(&region[..c.region_len()]);
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tiny retail-shaped file: header words, a 3-row table, two-sector
    /// records with a sane `dec_size` prefix each.
    fn synthetic() -> (Vec<u8>, usize) {
        let data_base = 0x1000;
        let mut f = vec![0u8; data_base];
        let table = 0x40u32;
        f[0..4].copy_from_slice(&table.to_le_bytes());
        f[4..8].copy_from_slice(&0x100u32.to_le_bytes());
        f[8..12].copy_from_slice(&0x200u32.to_le_bytes());
        f[12..16].copy_from_slice(&0x300u32.to_le_bytes());
        let rows = [
            (0x22u32, 0u32, 0x800u32),
            (0u32, 0x800, 0x1000),
            (0x5, 0x1800, 0x800),
        ];
        for (i, (id, off, size)) in rows.iter().enumerate() {
            let p = table as usize + i * 12;
            f[p..p + 4].copy_from_slice(&id.to_le_bytes());
            f[p + 4..p + 8].copy_from_slice(&off.to_le_bytes());
            f[p + 8..p + 12].copy_from_slice(&size.to_le_bytes());
        }
        for (i, (_, _, size)) in rows.iter().enumerate() {
            let mut slot = vec![(0x10 + i) as u8; *size as usize];
            slot[..4].copy_from_slice(&0x100u32.to_le_bytes());
            f.extend_from_slice(&slot);
        }
        (f, data_base)
    }

    #[test]
    fn retail_chain_has_zero_base() {
        let (f, _) = synthetic();
        let c = chain(&f).unwrap();
        assert_eq!(c.base, 0);
        assert!(!c.is_annexed());
        assert_eq!(c.rows.len(), 3);
        assert_eq!(c.region_len(), 0x2000);
        assert!(crate::battle_data_pack::detect(&f).is_some());
    }

    #[test]
    fn split_then_materialize_round_trips_and_parses() {
        let (f, db) = synthetic();
        let base = 0x1234_0000u32 & !0x7FF;
        let (header, region) = split(&f, db, base).unwrap();
        assert_eq!(header.len(), db);
        assert_eq!(region.len(), 0x2000);
        let c = chain(&header).unwrap();
        assert!(c.is_annexed());
        assert_eq!(c.base, base);
        assert_eq!(c.rows[1].offset, base + 0x800);
        // An annexed header alone is not a retail-shaped file.
        assert!(crate::battle_data_pack::detect(&header).is_none());
        let back = materialize(&header, db, &region).unwrap();
        assert_eq!(back, f);
    }

    #[test]
    fn split_refuses_annexed_and_unaligned() {
        let (f, db) = synthetic();
        assert!(split(&f, db, 0x801).is_err());
        assert!(split(&f, db, 0).is_err());
        let (header, region) = split(&f, db, 0x800).unwrap();
        let mut again = header.clone();
        again.extend_from_slice(&region);
        assert!(split(&again, db, 0x800).is_err());
        assert!(materialize(&f, db, &region).is_err());
    }
}
