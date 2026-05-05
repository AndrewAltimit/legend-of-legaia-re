use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use anyhow::{Result, bail};
use serde::Serialize;

pub const SECTOR: u32 = 0x800;

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
    pub size_sectors: u32,
    pub byte_offset: u64,
    pub size_bytes: u64,
}

pub struct Archive {
    file: File,
    file_len: u64,
    pub header: Header,
    pub toc: Vec<u32>,
    pub entries: Vec<Entry>,
}

impl Archive {
    pub fn open(path: &Path) -> Result<Self> {
        let mut file = File::open(path)?;
        let file_len = file.metadata()?.len();

        let header = detect_header(&mut file, file_len)?;

        let toc_start = header.header_offset + 0x08;
        let toc_end = header.header_offset + (header.header_sectors as u64) * (SECTOR as u64);
        let toc_bytes = (toc_end - toc_start) as usize;
        let mut buf = vec![0u8; toc_bytes];
        file.seek(SeekFrom::Start(toc_start))?;
        file.read_exact(&mut buf)?;
        let toc: Vec<u32> = buf
            .chunks_exact(4)
            .map(|c| u32::from_le_bytes(c.try_into().unwrap()))
            .collect();

        // For entry p:
        //   start_lba    = toc[p+2]                    (absolute LBA)
        //   size_sectors = toc[p+5] - toc[p+3] + 4
        //
        // The earlier interpretation `start_lba = toc[p+5] - toc[p+2]`
        // happened to equal `size_sectors` for many entries (since the TOC
        // stores monotonically-spaced absolute LBAs), accidentally producing
        // size in the start slot and then reading garbage at a low file
        // offset. Verified 2026-05 against a battle save state:
        // entry 873 at toc[875] = 0x9086 (file off 0x4843000) byte-matches
        // the live `_DAT_8007BD5C` runtime efect.dat 2-pack, while the old
        // formula reads sector 0x77 (a different file). Same correction
        // verified for entries 871, 872, 877, 888, 891.
        let count = (header.file_num.saturating_sub(1)) as usize;
        let mut entries = Vec::with_capacity(count);
        for p in 0..count {
            if p + 5 >= toc.len() {
                break;
            }
            let start_lba = toc[p + 2];
            let size_sectors = toc[p + 5].wrapping_sub(toc[p + 3]).wrapping_add(4);
            let byte_offset = (start_lba as u64) * (SECTOR as u64);
            let size_bytes = (size_sectors as u64) * (SECTOR as u64);
            if byte_offset.saturating_add(size_bytes) > file_len {
                continue;
            }
            entries.push(Entry {
                index: p as u32,
                start_lba,
                size_sectors,
                byte_offset,
                size_bytes,
            });
        }

        Ok(Self {
            file,
            file_len,
            header,
            toc,
            entries,
        })
    }

    pub fn file_len(&self) -> u64 {
        self.file_len
    }

    pub fn read_entry(&mut self, entry: &Entry, out: &mut Vec<u8>) -> Result<()> {
        out.clear();
        out.resize(entry.size_bytes as usize, 0);
        self.file.seek(SeekFrom::Start(entry.byte_offset))?;
        self.file.read_exact(out)?;
        Ok(())
    }
}

fn detect_header(file: &mut File, len: u64) -> Result<Header> {
    for &off in &[0x000u64, 0x800u64] {
        if off + 12 > len {
            continue;
        }
        file.seek(SeekFrom::Start(off))?;
        let mut buf = [0u8; 12];
        file.read_exact(&mut buf)?;
        let file_num_minus_1 = i32::from_le_bytes(buf[4..8].try_into().unwrap());
        let header_sectors = i32::from_le_bytes(buf[8..12].try_into().unwrap());
        if file_num_minus_1 <= 0 || header_sectors <= 0 {
            continue;
        }
        let file_num = (file_num_minus_1 + 1) as u32;
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
