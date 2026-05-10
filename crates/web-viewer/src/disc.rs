//! Pure-Rust in-memory disc walker. No `std::fs`, no JS bindings - works on
//! both the wasm32 and native targets, so it can be unit-tested with a real
//! disc image when one is available.
//!
//! Two formats handled:
//!   1. Mode2/2352 .bin disc images. ISO9660 walk → returns `PROT.DAT` bytes.
//!   2. Raw PROT.DAT - TOC parse.

const SECTOR: u32 = 0x800;
const RAW_SECTOR_SIZE: usize = 2352;
const RAW_USER_DATA_OFFSET: usize = 24;
const RAW_USER_DATA_SIZE: usize = 2048;

/// One PROT.DAT entry's location inside its owning buffer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntryMeta {
    pub index: u32,
    pub byte_offset: u64,
    pub size_bytes: u64,
}

/// Read the user-data portion of one Mode2/2352 sector at `lba`.
fn read_sector(disc: &[u8], lba: u32) -> Option<&[u8]> {
    let start = lba as usize * RAW_SECTOR_SIZE + RAW_USER_DATA_OFFSET;
    let end = start + RAW_USER_DATA_SIZE;
    (end <= disc.len()).then(|| &disc[start..end])
}

/// Read `count` consecutive user-data sectors starting at `lba`.
fn read_user_data(disc: &[u8], lba: u32, count: u32) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(count as usize * RAW_USER_DATA_SIZE);
    for i in 0..count {
        out.extend_from_slice(read_sector(disc, lba + i)?);
    }
    Some(out)
}

#[derive(Clone)]
struct IsoRecord {
    lba: u32,
    size: u32,
    is_dir: bool,
    name: String,
}

fn parse_iso_record(buf: &[u8]) -> Option<IsoRecord> {
    if buf.len() < 33 {
        return None;
    }
    let lba = u32::from_le_bytes(buf[2..6].try_into().ok()?);
    let size = u32::from_le_bytes(buf[10..14].try_into().ok()?);
    let flags = buf[25];
    let is_dir = flags & 0x02 != 0;
    let name_len = buf[32] as usize;
    if 33 + name_len > buf.len() {
        return None;
    }
    let raw_name = &buf[33..33 + name_len];
    let name = match raw_name {
        [0] => ".".into(),
        [1] => "..".into(),
        _ => {
            let s = String::from_utf8_lossy(raw_name);
            s.split(';').next().unwrap_or("").to_string()
        }
    };
    Some(IsoRecord {
        lba,
        size,
        is_dir,
        name,
    })
}

fn list_directory(disc: &[u8], dir: &IsoRecord) -> Option<Vec<IsoRecord>> {
    let sector_count = dir.size.div_ceil(RAW_USER_DATA_SIZE as u32);
    let mut buf = read_user_data(disc, dir.lba, sector_count)?;
    buf.truncate(dir.size as usize);

    let mut out = Vec::new();
    let mut offset = 0usize;
    while offset < buf.len() {
        let len = buf[offset] as usize;
        if len == 0 {
            // Records don't span 2048-byte logical blocks; pad to next.
            let next = offset.div_ceil(RAW_USER_DATA_SIZE) * RAW_USER_DATA_SIZE;
            let next = if next == offset {
                offset + RAW_USER_DATA_SIZE
            } else {
                next
            };
            if next >= buf.len() {
                break;
            }
            offset = next;
            continue;
        }
        if offset + len > buf.len() {
            break;
        }
        if let Some(rec) = parse_iso_record(&buf[offset..offset + len])
            && rec.name != "."
            && rec.name != ".."
        {
            out.push(rec);
        }
        offset += len;
    }
    Some(out)
}

/// True if `disc` looks like a Mode2/2352 PSX disc image (sector-aligned size +
/// valid ISO9660 PVD at LBA 16).
pub fn is_mode2_2352_disc(disc: &[u8]) -> bool {
    if disc.len() < 17 * RAW_SECTOR_SIZE || !disc.len().is_multiple_of(RAW_SECTOR_SIZE) {
        return false;
    }
    match read_sector(disc, 16) {
        Some(pvd) => pvd[0] == 1 && &pvd[1..6] == b"CD001",
        None => false,
    }
}

/// Walk a Mode2/2352 disc image, find a named file in the root directory,
/// and return its bytes (sector-stripped, file-size-truncated).
fn extract_root_file(disc: &[u8], name: &str) -> Option<Vec<u8>> {
    if !is_mode2_2352_disc(disc) {
        return None;
    }
    let pvd = read_sector(disc, 16)?;
    let root = parse_iso_record(&pvd[156..156 + 34])?;
    if !root.is_dir {
        return None;
    }
    for e in list_directory(disc, &root)? {
        if !e.is_dir && e.name.eq_ignore_ascii_case(name) {
            let sector_count = e.size.div_ceil(RAW_USER_DATA_SIZE as u32);
            let mut bytes = read_user_data(disc, e.lba, sector_count)?;
            bytes.truncate(e.size as usize);
            return Some(bytes);
        }
    }
    None
}

/// Walk a Mode2/2352 disc image, find the file named `PROT.DAT` in the root
/// directory, and return its bytes (sector-stripped, file-size-truncated).
pub fn extract_prot_dat(disc: &[u8]) -> Option<Vec<u8>> {
    extract_root_file(disc, "PROT.DAT")
}

/// Walk a Mode2/2352 disc image, find `CDNAME.TXT` in the root directory,
/// and return its text content. Returns `None` if the disc is not a valid
/// Mode2/2352 image or the file is absent or not valid UTF-8.
pub fn extract_cdname_txt(disc: &[u8]) -> Option<String> {
    let bytes = extract_root_file(disc, "CDNAME.TXT")?;
    String::from_utf8(bytes).ok()
}

/// Parse the PROT.DAT TOC and return a vector of entry locations within `buf`.
/// The header lives at file offset 0x000 or 0x800; layout (matching
/// `legaia-prot::archive::detect_header`):
///   header[0..4]   - unused
///   header[4..8]   - i32 `file_num - 1` (count of entries minus one)
///   header[8..12]  - i32 `header_sectors` (TOC sector count)
///   header[12..]   - TOC u32 array
pub fn parse_prot_toc(buf: &[u8]) -> Option<Vec<EntryMeta>> {
    let mut header_offset = None;
    for &off in &[0x000usize, 0x800] {
        if off + 12 > buf.len() {
            continue;
        }
        let file_num_minus_1 = i32::from_le_bytes(buf[off + 4..off + 8].try_into().ok()?);
        let header_sectors = i32::from_le_bytes(buf[off + 8..off + 12].try_into().ok()?);
        if file_num_minus_1 <= 0 || header_sectors <= 0 || header_sectors > 0x100 {
            continue;
        }
        let header_end = off + (header_sectors as usize) * SECTOR as usize;
        if header_end > buf.len() {
            continue;
        }
        header_offset = Some((off, file_num_minus_1 as u32 + 1, header_sectors as u32));
        break;
    }
    let (hoff, file_num, header_sectors) = header_offset?;

    let toc_start = hoff + 8;
    let toc_end = hoff + (header_sectors as usize) * SECTOR as usize;
    let toc: Vec<u32> = buf[toc_start..toc_end]
        .chunks_exact(4)
        .map(|c| u32::from_le_bytes(c.try_into().unwrap()))
        .collect();

    let count = file_num.saturating_sub(1) as usize;
    let mut entries = Vec::with_capacity(count);
    for p in 0..count {
        if p + 5 >= toc.len() {
            break;
        }
        let start_lba = toc[p + 2];
        let size_sectors = toc[p + 5].wrapping_sub(toc[p + 3]).wrapping_add(4);
        let byte_offset = (start_lba as u64) * (SECTOR as u64);
        let size_bytes = (size_sectors as u64) * (SECTOR as u64);
        if byte_offset.saturating_add(size_bytes) > buf.len() as u64
            || size_bytes == 0
            || size_bytes > 32 * 1024 * 1024
        {
            continue;
        }
        entries.push(EntryMeta {
            index: p as u32,
            byte_offset,
            size_bytes,
        });
    }

    if entries.len() < 100 {
        return None;
    }
    Some(entries)
}
