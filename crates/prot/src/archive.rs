use std::io::{Cursor, Read, Seek, SeekFrom};
use std::path::Path;

use anyhow::{Result, bail};
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
    /// On-disc footprint in sectors — `max(indexed_size, next_start - start_lba)`.
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
        let file = File::open(path)?;
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
        // indexed end into the next entry's LBA — those trailing sectors
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
            let indexed_size_sectors = toc[p + 5].wrapping_sub(toc[p + 3]).wrapping_add(4);
            // Trailing-gap candidate: bytes from start_lba to next entry's
            // start_lba. Use wrapping_sub so unsorted entries don't blow up
            // — they fall back to the indexed size.
            let next_start_lba = toc[p + 3];
            let footprint_sectors = next_start_lba.wrapping_sub(start_lba);
            // Only honor the trailing extension when it's a sane positive
            // number (entries aren't always strictly sorted; an unsorted
            // pair would produce a huge wrapped value).
            let extended_size_sectors = if footprint_sectors <= MAX_REASONABLE_FOOTPRINT_SECTORS
                && footprint_sectors > indexed_size_sectors
            {
                footprint_sectors
            } else {
                indexed_size_sectors
            };
            let size_sectors = extended_size_sectors;
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
    /// gap, if any). This is what consumers usually want — it matches what
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
    /// indexed payload without any trailing-overlay sectors — most callers
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
