//! PROT.DAT archive reader.
//!
//! `Archive::from_reader` is the clean-room analogue of the retail boot-time
//! TOC loader: it parses the PROT.DAT header sectors and walks the same TOC
//! pair (`toc[p+2]` start LBA, `toc[p+3]` next start) the SCUS dispatcher
//! reads into `0x801C70F0` at boot. See
//! [`docs/subsystems/boot.md`](../../../docs/subsystems/boot.md#toc-loader-fun_8003e4e8).
//!
//! ## An entry's size is the sector gap to the next entry
//!
//! `size_sectors = toc[p+3] - toc[p+2]`, which is exactly what retail's own
//! span routine [`FUN_8003E68C`](crate::runtime_toc::entry_sector_span)
//! returns and what the loader hands to the sector read. The footprints
//! **tile `PROT.DAT` exactly** - monotonic starts, no gaps, no overlaps, and
//! a sum equal to the contiguous span between the first entry's start LBA and
//! the archive's last sector. [`crate::tiling`] states that as a checkable
//! property and `crates/prot/tests/archive_tiling_real.rs` runs it against a
//! real disc.
//!
//! The historical `toc[p+5] - toc[p+3] + 4` expression is **not** entry `p`'s
//! size: `toc[p+3]` is entry `p+1`'s start LBA and `toc[p+5]` is entry
//! `p+3`'s, so it evaluates to `footprint(p+1) + footprint(p+2) + 4` - a span
//! of the two *following* entries. It is retained as
//! [`Entry::declared_span_sectors`] for diagnostics only. See
//! [`docs/formats/prot.md`](../../../docs/formats/prot.md).
//!
//! PORT: FUN_8003E4E8

use std::io::{Cursor, Read, Seek, SeekFrom};
use std::path::Path;

use anyhow::{Context, Result, bail};
use serde::Serialize;

pub const SECTOR: u32 = 0x800;

/// Cap on an entry's sector span. Anything bigger than this is a wrap
/// (a non-monotonic or zero-padded TOC row whose `next_start - start_lba`
/// goes negative) rather than a real on-disc footprint. The largest
/// legitimate footprint in the retail TOC is the monster archive's 7760
/// sectors (~15 MiB); a 64K-sector cap (= 128 MiB) is a comfortable upper
/// bound while still rejecting wrapped negatives.
const MAX_REASONABLE_FOOTPRINT_SECTORS: u32 = 64 * 1024;

#[derive(Debug, Clone, Serialize)]
pub struct Header {
    pub header_offset: u64,
    pub file_num: u32,
    pub header_sectors: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct Entry {
    pub index: u32,
    pub start_lba: u32,
    /// The entry's size in sectors: `toc[p+3] - toc[p+2]`, the sector gap to
    /// the next entry's start LBA. This is what [`Archive::read_entry`]
    /// returns and what retail's span routine `FUN_8003E68C` computes.
    pub size_sectors: u32,
    pub byte_offset: u64,
    pub size_bytes: u64,
    /// The historical `toc[p+5] - toc[p+3] + 4` expression, kept for
    /// diagnostics and for tests that pin the defect it caused.
    ///
    /// **This is not a size of entry `p`.** It expands to
    /// `footprint(p+1) + footprint(p+2) + 4`, so it measures the two
    /// *following* entries. Where it exceeds `size_sectors` (it does for most
    /// of the archive) a reader that trusts it runs past the entry into its
    /// neighbours' sectors; where it falls short it truncates the entry
    /// mid-payload.
    pub declared_span_sectors: u32,
    pub declared_span_bytes: u64,
}

trait ReadSeek: Read + Seek + Send {}
impl<T: Read + Seek + Send> ReadSeek for T {}

pub struct Archive {
    reader: Box<dyn ReadSeek>,
    file_len: u64,
    pub header: Header,
    pub toc: Vec<u32>,
    pub entries: Vec<Entry>,
}

impl Archive {
    pub fn open(path: &Path) -> Result<Self> {
        use std::fs::File;
        let file = File::open(path).with_context(|| format!("opening {}", path.display()))?;
        let file_len = file.metadata()?.len();
        Self::from_reader(Box::new(file), file_len)
    }

    /// Parse an in-memory PROT.DAT image (WASM-safe; no filesystem access).
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self> {
        let file_len = bytes.len() as u64;
        Self::from_reader(Box::new(Cursor::new(bytes)), file_len)
    }

    fn from_reader(mut reader: Box<dyn ReadSeek>, file_len: u64) -> Result<Self> {
        let header = detect_header(reader.as_mut(), file_len)?;

        let toc_start = header.header_offset + 0x08;
        let toc_end = header.header_offset + (header.header_sectors as u64) * (SECTOR as u64);
        let toc_bytes = (toc_end - toc_start) as usize;
        let mut buf = vec![0u8; toc_bytes];
        reader.seek(SeekFrom::Start(toc_start))?;
        reader.read_exact(&mut buf)?;
        let toc: Vec<u32> = buf
            .chunks_exact(4)
            .map(|c| u32::from_le_bytes(c.try_into().unwrap()))
            .collect();

        // For entry p:
        //   start_lba    = toc[p+2]                (LBA relative to PROT.DAT)
        //   size_sectors = toc[p+3] - toc[p+2]     (the entry's sectors)
        //
        // The span comes from the port of retail's own routine so the
        // arithmetic - including the `wrapping_sub` that makes a zero-padded
        // or non-monotonic row produce an obviously-bogus value instead of
        // panicking - has exactly one implementation in the crate.
        let count = (header.file_num.saturating_sub(1)) as usize;
        let mut entries = Vec::with_capacity(count);
        for p in 0..count {
            if p + 3 >= toc.len() {
                break;
            }
            let start_lba = toc[p + 2];
            // LBA 0 holds the archive header, so a row pointing there is TOC
            // padding rather than an entry. The zeroed tail rows are exactly
            // this shape.
            if start_lba == 0 {
                continue;
            }
            let Some(size_sectors) =
                crate::runtime_toc::entry_sector_span_from_archive_toc(&toc, p)
            else {
                continue;
            };
            // A wrapped (non-monotonic / zero-padded) row, or a zero-length
            // one: not an entry.
            if size_sectors == 0 || size_sectors > MAX_REASONABLE_FOOTPRINT_SECTORS {
                continue;
            }
            let byte_offset = (start_lba as u64) * (SECTOR as u64);
            let size_bytes = (size_sectors as u64) * (SECTOR as u64);
            if byte_offset.saturating_add(size_bytes) > file_len {
                continue;
            }
            // Diagnostics only - see `Entry::declared_span_sectors`. Against
            // the zeroed TOC tail the expression underflows to a wrapped
            // value; fall back to the real size there, then clamp to the
            // image so a caller reading this window can't run off the end.
            // On retail neither guard binds outside the final two rows.
            let declared_raw = toc
                .get(p + 5)
                .copied()
                .unwrap_or(0)
                .wrapping_sub(toc[p + 3])
                .wrapping_add(4);
            let room = ((file_len - byte_offset) / (SECTOR as u64)) as u32;
            let declared_span_sectors = if declared_raw <= MAX_REASONABLE_FOOTPRINT_SECTORS {
                declared_raw.min(room)
            } else {
                size_sectors
            };
            let declared_span_bytes = (declared_span_sectors as u64) * (SECTOR as u64);
            entries.push(Entry {
                index: p as u32,
                start_lba,
                size_sectors,
                byte_offset,
                size_bytes,
                declared_span_sectors,
                declared_span_bytes,
            });
        }

        Ok(Self {
            reader,
            file_len,
            header,
            toc,
            entries,
        })
    }

    pub fn file_len(&self) -> u64 {
        self.file_len
    }

    /// Read the entry - all `size_sectors` of it, and nothing that belongs to
    /// a neighbour. This is what every consumer wants.
    pub fn read_entry(&mut self, entry: &Entry, out: &mut Vec<u8>) -> Result<()> {
        out.clear();
        out.resize(entry.size_bytes as usize, 0);
        self.reader.seek(SeekFrom::Start(entry.byte_offset))?;
        self.reader.read_exact(out)?;
        Ok(())
    }

    /// Read the historical `toc[p+5] - toc[p+3] + 4` window
    /// ([`Entry::declared_span_bytes`]).
    ///
    /// **Diagnostic only.** That expression measures the two entries *after*
    /// `p`, so this window neither starts nor ends where entry `p` does: it
    /// over-reads into the neighbours for most of the archive and truncates
    /// the entry for the rest. It exists so a caller can reproduce the
    /// pre-correction view (and so the tiling test can show the two apart).
    /// Use [`Self::read_entry`] for anything that parses.
    pub fn read_entry_declared_span(&mut self, entry: &Entry, out: &mut Vec<u8>) -> Result<()> {
        out.clear();
        out.resize(entry.declared_span_bytes as usize, 0);
        self.reader.seek(SeekFrom::Start(entry.byte_offset))?;
        self.reader.read_exact(out)?;
        Ok(())
    }

    /// Read arbitrary raw bytes from PROT.DAT at `byte_offset`. Used to
    /// reach unindexed gap regions that don't belong to any TOC entry
    /// (e.g. the 240 KB system-UI gap between the TOC and `init_data`
    /// at LBA 0..120, which carries the menu-glyph atlas + boot-time
    /// cursor / icon TIMs; see [`docs/subsystems/boot.md`]).
    pub fn read_raw(&mut self, byte_offset: u64, len: usize, out: &mut Vec<u8>) -> Result<()> {
        if byte_offset.saturating_add(len as u64) > self.file_len {
            bail!(
                "raw read [0x{:X}, +{}] past PROT.DAT end (0x{:X})",
                byte_offset,
                len,
                self.file_len
            );
        }
        out.clear();
        out.resize(len, 0);
        self.reader.seek(SeekFrom::Start(byte_offset))?;
        self.reader.read_exact(out)?;
        Ok(())
    }
}

fn detect_header(reader: &mut dyn ReadSeek, len: u64) -> Result<Header> {
    for &off in &[0x000u64, 0x800u64] {
        if off + 12 > len {
            continue;
        }
        reader.seek(SeekFrom::Start(off))?;
        let mut buf = [0u8; 12];
        reader.read_exact(&mut buf)?;
        let file_num_minus_1 = i32::from_le_bytes(buf[4..8].try_into().unwrap());
        let header_sectors = i32::from_le_bytes(buf[8..12].try_into().unwrap());
        if file_num_minus_1 <= 0 || header_sectors <= 0 {
            continue;
        }
        // `file_num_minus_1` is attacker-controlled; `+ 1` would overflow in
        // debug for `i32::MAX`. Use a checked add and treat overflow as a
        // non-match rather than panicking.
        let Some(file_num) = file_num_minus_1.checked_add(1).map(|n| n as u32) else {
            continue;
        };
        if off + (header_sectors as u64) * (SECTOR as u64) > len {
            continue;
        }
        return Ok(Header {
            header_offset: off,
            file_num,
            header_sectors: header_sectors as u32,
        });
    }
    bail!("PROT-style header not found at offset 0x000 or 0x800");
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a synthetic PROT.DAT whose TOC tail mirrors the retail shape:
    /// the last real rows read `toc[p+3]` (and `toc[p+5]`) out of the zeroed
    /// padding past the TOC.
    fn tail_shaped_prot() -> Vec<u8> {
        let sectors = 20u32;
        let mut img = vec![0u8; (sectors * SECTOR) as usize];
        // Header: [pad, file_num - 1, header_sectors]
        img[4..8].copy_from_slice(&6u32.to_le_bytes());
        img[8..12].copy_from_slice(&1u32.to_le_bytes());
        // TOC (starts at +8, entry p reads toc[p+2] and toc[p+3]):
        // start LBAs 1, 3, 10, 15, 19, 20, then the zeroed tail.
        for (i, lba) in [1u32, 3, 10, 15, 19, 20].iter().enumerate() {
            let off = 8 + (2 + i) * 4;
            img[off..off + 4].copy_from_slice(&lba.to_le_bytes());
        }
        img
    }

    /// An entry's size is retail's own span routine (`FUN_8003E68C`), not a
    /// second implementation of it.
    ///
    /// This pins the *call*, not the arithmetic: the two agree by
    /// construction, so it would not catch a duplicated formula on its own.
    /// What it catches is the next edit that re-inlines the arithmetic here
    /// and lets the two drift - including in the wrapping case, which is where
    /// a hand-rolled span is most likely to diverge from the port.
    #[test]
    fn entry_size_comes_from_the_ported_span_routine() {
        let img = tail_shaped_prot();
        let arch = Archive::from_bytes(img).expect("synthetic archive parses");
        assert!(!arch.entries.is_empty());
        for e in &arch.entries {
            let span =
                crate::runtime_toc::entry_sector_span_from_archive_toc(&arch.toc, e.index as usize)
                    .expect("bounded by the parser's own p + 3 < len guard");
            assert_eq!(
                e.size_sectors, span,
                "entry {} size must be the ported span",
                e.index
            );
        }
    }

    /// The parser keeps every row the span routine resolves, including the
    /// last ones before the zeroed TOC tail (retail extraction 1231 - the
    /// dance minigame's SFX VAB - and 1232 are exactly this shape), and drops
    /// the padding rows behind them.
    #[test]
    fn toc_tail_entries_resolve_and_padding_rows_do_not() {
        let arch = Archive::from_bytes(tail_shaped_prot()).expect("synthetic archive parses");
        let e2 = arch.entries.iter().find(|e| e.index == 2).expect("entry 2");
        assert_eq!(e2.start_lba, 10);
        assert_eq!(e2.size_sectors, 5); // toc[5]=15 - toc[4]=10
        // The historical expression for the same row measures entries 3 and 4
        // instead: (19 - 15) + (20 - 19) + 4 = 9.
        assert_eq!(e2.declared_span_sectors, 9);

        let e3 = arch
            .entries
            .iter()
            .find(|e| e.index == 3)
            .expect("entry 3 kept");
        assert_eq!(e3.start_lba, 15);
        assert_eq!(e3.size_sectors, 4);
        // Entry 4: `toc[p+3]` is the last real row; footprint = 1.
        let e4 = arch
            .entries
            .iter()
            .find(|e| e.index == 4)
            .expect("entry 4 kept");
        assert_eq!(e4.start_lba, 19);
        assert_eq!(e4.size_sectors, 1);
        // Entry 5 starts at the file end and its next LBA is the zeroed tail,
        // so its span wraps: a phantom row, dropped.
        assert!(arch.entries.iter().all(|e| e.index != 5));
        // Rows past entry 5 are all-zero padding, and LBA 0 is the archive
        // header - never an entry (retail: a phantom idx-1234 entry that
        // inflated the archive to 1234 entries).
        assert!(arch.entries.iter().all(|e| e.start_lba != 0));
        assert_eq!(arch.entries.last().map(|e| e.index), Some(4));
    }

    /// `read_entry` returns the entry and stops at its end; the declared-span
    /// window is a different, larger read that runs into the neighbours.
    #[test]
    fn read_entry_stops_at_the_entry_end() {
        let mut arch = Archive::from_bytes(tail_shaped_prot()).expect("synthetic archive parses");
        let e2 = arch
            .entries
            .iter()
            .find(|e| e.index == 2)
            .cloned()
            .expect("entry 2");
        let mut buf = Vec::new();
        arch.read_entry(&e2, &mut buf).expect("entry reads");
        assert_eq!(buf.len(), 5 * SECTOR as usize);
        arch.read_entry_declared_span(&e2, &mut buf)
            .expect("declared window reads");
        assert_eq!(buf.len(), 9 * SECTOR as usize);
    }
}
