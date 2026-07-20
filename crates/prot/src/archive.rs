//! PROT.DAT archive reader.
//!
//! `Archive::from_reader` is the clean-room analogue of the retail boot-time
//! TOC loader: it parses the PROT.DAT header sectors and walks the same TOC
//! triple (`toc[p+2]` start LBA, `toc[p+3]` next start, `toc[p+5]` payload end)
//! the SCUS dispatcher reads into `0x801C70F0` at boot. See
//! [`docs/subsystems/boot.md`](../../../docs/subsystems/boot.md#toc-loader-fun_8003e4e8).
//!
//! PORT: FUN_8003E4E8

use std::io::{Cursor, Read, Seek, SeekFrom};
use std::path::Path;

use anyhow::{Context, Result, bail};
use serde::Serialize;

pub const SECTOR: u32 = 0x800;

/// Cap on the trailing-gap extension. Anything bigger than this is almost
/// certainly a wrap (entries with negative `next_start - start_lba` from
/// non-monotonic TOC sections) rather than a real on-disc footprint. The
/// largest legitimate trailing gap observed in the retail TOC is ~15 MiB
/// (7628 sectors for entry 867); a 64K-sector cap (= 128 MiB) is a comfortable
/// upper bound while still rejecting wrapped negatives.
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
    /// On-disc footprint in sectors - `max(indexed_size, next_start - start_lba)`.
    /// This is what `read_entry` returns; covers trailing-overlay content the
    /// SCUS boot loader reads past the indexed end (see boot.md).
    pub size_sectors: u32,
    pub byte_offset: u64,
    pub size_bytes: u64,
    /// TOC-indexed payload size (the historical `toc[p+5] - toc[p+3] + 4`
    /// formula). For entries where the boot loader reads past the indexed
    /// end into trailing-overlay sectors, this is smaller than `size_sectors`.
    /// Equal to `size_sectors` for entries without a trailing gap.
    pub indexed_size_sectors: u32,
    pub indexed_size_bytes: u64,
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
        //   start_lba           = toc[p+2]                       (absolute LBA)
        //   indexed_size_sectors = toc[p+5] - toc[p+3] + 4       (TOC-indexed payload)
        //   size_sectors         = max(indexed_size_sectors,
        //                              toc[p+3] - toc[p+2])      (on-disc footprint)
        //
        // The indexed formula describes the entry's TOC-declared payload, but
        // the SCUS boot loader sometimes reads CONTIGUOUS sectors past the
        // indexed end into the next entry's LBA - those trailing sectors
        // carry "trailing-overlay" content (e.g. PROT 899's trailing 60
        // sectors are the title-screen overlay code; see boot.md). We
        // surface the larger of the two so consumers see the full on-disc
        // footprint. For entries that don't overlap the next, both formulas
        // agree.
        //
        // The "+4" in the indexed formula was verified against entry 873's
        // efect.dat 2-pack byte-equality (also entries 871, 872, 877, 888,
        // 891). The trailing-gap extension was confirmed by capturing
        // multi-sector DMA writes during cold boot (see
        // scripts/pcsx-redux/autorun_title_overlay_writer_hunt.lua).
        let count = (header.file_num.saturating_sub(1)) as usize;
        let mut entries = Vec::with_capacity(count);
        for p in 0..count {
            if p + 5 >= toc.len() {
                break;
            }
            let start_lba = toc[p + 2];
            // A fully-zeroed TOC row is tail padding, not an entry - but the
            // "+4" in the indexed formula makes it look like a sane 4-sector
            // entry at LBA 0 (the archive header). Skip it before the size
            // heuristics run.
            if start_lba == 0 && toc[p + 3] == 0 && toc[p + 5] == 0 {
                continue;
            }
            let indexed_raw = toc[p + 5].wrapping_sub(toc[p + 3]).wrapping_add(4);
            // Trailing-gap candidate: sectors from start_lba to the next
            // entry's start_lba. This is retail's own span routine, so take it
            // from the port rather than recomputing it here - the wrapping
            // subtraction (which keeps unsorted entries from blowing up, so
            // they fall back to the indexed size) is part of what is ported.
            // `p + 5 < toc.len()` above already bounds the `p + 3` read, so
            // the `None` arm is unreachable; treat it as tail padding anyway.
            let Some(footprint_sectors) =
                crate::runtime_toc::entry_sector_span_from_archive_toc(&toc, p)
            else {
                continue;
            };
            let footprint_sane =
                footprint_sectors > 0 && footprint_sectors <= MAX_REASONABLE_FOOTPRINT_SECTORS;
            // The last TOC page rows sit against a zeroed tail: for the final
            // real entries `toc[p+5]` (and eventually `toc[p+3]`) are 0, so
            // the indexed formula underflows to a huge wrapped value. Those
            // entries are real on-disc content (retail extraction 1231 is the
            // dance minigame's SFX VAB, 1232 the last data sector) - fall back
            // to the LBA footprint instead of silently dropping them.
            let indexed_size_sectors = if indexed_raw <= MAX_REASONABLE_FOOTPRINT_SECTORS {
                indexed_raw
            } else if footprint_sane {
                footprint_sectors
            } else {
                // Neither formula yields a sane size: a phantom row in the
                // zeroed tail. Skip it.
                continue;
            };
            // Only honor the trailing extension when it's a sane positive
            // number (entries aren't always strictly sorted; an unsorted
            // pair would produce a huge wrapped value).
            let size_sectors = if footprint_sane && footprint_sectors > indexed_size_sectors {
                footprint_sectors
            } else {
                indexed_size_sectors
            };
            let byte_offset = (start_lba as u64) * (SECTOR as u64);
            let size_bytes = (size_sectors as u64) * (SECTOR as u64);
            let indexed_size_bytes = (indexed_size_sectors as u64) * (SECTOR as u64);
            if byte_offset.saturating_add(size_bytes) > file_len {
                continue;
            }
            entries.push(Entry {
                index: p as u32,
                start_lba,
                size_sectors,
                byte_offset,
                size_bytes,
                indexed_size_sectors,
                indexed_size_bytes,
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

    /// Read an entry's full on-disc footprint (indexed payload + trailing
    /// gap, if any). This is what consumers usually want - it matches what
    /// the SCUS boot loader reads when it issues a multi-sector ReadN.
    pub fn read_entry(&mut self, entry: &Entry, out: &mut Vec<u8>) -> Result<()> {
        out.clear();
        out.resize(entry.size_bytes as usize, 0);
        self.reader.seek(SeekFrom::Start(entry.byte_offset))?;
        self.reader.read_exact(out)?;
        Ok(())
    }

    /// Read only an entry's TOC-indexed sub-region (the historical
    /// `toc[p+5] - toc[p+3] + 4` slice). Use when you specifically want the
    /// indexed payload without any trailing-overlay sectors - most callers
    /// should prefer [`Self::read_entry`].
    pub fn read_entry_indexed(&mut self, entry: &Entry, out: &mut Vec<u8>) -> Result<()> {
        out.clear();
        out.resize(entry.indexed_size_bytes as usize, 0);
        self.reader.seek(SeekFrom::Start(entry.byte_offset))?;
        self.reader.read_exact(out)?;
        Ok(())
    }

    /// Trailing-gap size in sectors (`size_sectors - indexed_size_sectors`).
    /// Zero for entries without a trailing gap.
    pub fn trailing_gap_sectors(entry: &Entry) -> u32 {
        entry
            .size_sectors
            .saturating_sub(entry.indexed_size_sectors)
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
    /// the last real rows read `toc[p+5]` (and then `toc[p+3]`) out of the
    /// zeroed padding past the TOC, so the indexed size formula underflows.
    fn tail_shaped_prot() -> Vec<u8> {
        let sectors = 20u32;
        let mut img = vec![0u8; (sectors * SECTOR) as usize];
        // Header: [pad, file_num - 1, header_sectors]
        img[4..8].copy_from_slice(&6u32.to_le_bytes());
        img[8..12].copy_from_slice(&1u32.to_le_bytes());
        // TOC (starts at +8, entry p reads toc[p+2], toc[p+3], toc[p+5]):
        // start LBAs 1, 3, 10, 15, 19, 20, then the zeroed tail.
        for (i, lba) in [1u32, 3, 10, 15, 19, 20].iter().enumerate() {
            let off = 8 + (2 + i) * 4;
            img[off..off + 4].copy_from_slice(&lba.to_le_bytes());
        }
        img
    }

    /// The trailing-gap footprint is retail's own span routine
    /// (`FUN_8003E68C`), not a second implementation of it.
    ///
    /// This pins the *call*, not the arithmetic: the two agreed by
    /// construction when the parser was recomputing `toc[p+3] - toc[p+2]`
    /// inline, so it would not have caught the duplication. What it catches is
    /// the next edit that re-inlines a formula here and lets the two drift -
    /// including in the wrapping case, which is where a hand-rolled span is
    /// most likely to diverge from the port.
    #[test]
    fn footprint_comes_from_the_ported_span_routine() {
        let img = tail_shaped_prot();
        let arch = Archive::from_bytes(img).expect("synthetic archive parses");
        for e in &arch.entries {
            let span =
                crate::runtime_toc::entry_sector_span_from_archive_toc(&arch.toc, e.index as usize)
                    .expect("bounded by the parser's own p + 5 < len guard");
            // Every surviving entry took either the indexed size or the span;
            // the span must be the one the port computes.
            assert!(
                e.size_sectors == e.indexed_size_sectors || e.size_sectors == span,
                "entry {} size {} is neither the indexed size {} nor the ported span {span}",
                e.index,
                e.size_sectors,
                e.indexed_size_sectors,
            );
            // Entry 4's row has a zeroed `toc[p+3]`-side neighbour, so its
            // size is the span outright - the case that would wrap if the
            // subtraction were rewritten without `wrapping_sub`.
            if e.index == 4 {
                assert_eq!(e.size_sectors, span);
            }
        }
    }

    /// The last entries before the zeroed TOC tail must resolve via the LBA
    /// footprint rather than being silently dropped (retail extraction 1231 -
    /// the dance minigame's SFX VAB - and 1232 are exactly this shape).
    #[test]
    fn toc_tail_entries_resolve_by_footprint() {
        let arch = Archive::from_bytes(tail_shaped_prot()).expect("synthetic archive parses");
        // Ordinary interior entry: the indexed formula applies untouched.
        let e2 = arch.entries.iter().find(|e| e.index == 2).expect("entry 2");
        assert_eq!(e2.start_lba, 10);
        assert_eq!(e2.indexed_size_sectors, 9); // toc[7]=20 - toc[5]=15 + 4
        // Entry 3: `toc[p+5]` is in the zeroed tail, so the indexed formula
        // underflows; the footprint (19 - 15) is the real size.
        let e3 = arch
            .entries
            .iter()
            .find(|e| e.index == 3)
            .expect("entry 3 kept");
        assert_eq!(e3.start_lba, 15);
        assert_eq!(e3.size_sectors, 4);
        assert_eq!(e3.indexed_size_sectors, 4);
        // Entry 4: both `toc[p+4]` and `toc[p+5]` are zero; footprint = 1.
        let e4 = arch
            .entries
            .iter()
            .find(|e| e.index == 4)
            .expect("entry 4 kept");
        assert_eq!(e4.start_lba, 19);
        assert_eq!(e4.size_sectors, 1);
        // Entry 5 starts at the file end with no next LBA: a phantom row of
        // the zeroed tail, still dropped.
        assert!(arch.entries.iter().all(|e| e.index != 5));
        // Rows past entry 5 are all-zero padding: the "+4" indexed formula
        // would read each as a sane 4-sector entry at LBA 0 (the header) -
        // the zero-row guard must drop them (retail: a phantom idx-1234
        // entry that inflated the archive to 1234 entries).
        assert!(arch.entries.iter().all(|e| e.start_lba != 0));
        assert_eq!(arch.entries.last().map(|e| e.index), Some(4));
    }
}
