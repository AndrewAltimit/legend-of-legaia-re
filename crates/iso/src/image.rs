//! In-memory Mode2/2352 disc walker over a borrowed byte slice. No
//! `std::fs`, no JS bindings - works on both the wasm32 and native targets,
//! which is what the browser hosts load a visitor's disc image through, and
//! it can be unit-tested with a real disc image when one is available.
//!
//! [`walk_iso_files`] enumerates every file (subdirectories included);
//! [`extract_prot_dat`], [`extract_cdname_txt`] and [`extract_scus`] return a
//! root file's logical bytes (sector-stripped, file-size-truncated), matching
//! the name case-insensitively with the `;1` version suffix stripped.
//! [`crate::iso9660::read_file_in_image`] is the patcher's sibling reader,
//! which matches the root name exactly. The PROT.DAT table of contents those
//! bytes carry is parsed by `legaia_prot::toc_index`.

const RAW_SECTOR_SIZE: usize = 2352;
const RAW_USER_DATA_OFFSET: usize = 24;
const RAW_USER_DATA_SIZE: usize = 2048;

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

/// One file enumerated from the on-disc ISO9660 filesystem.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileEntry {
    /// Path relative to the ISO root (e.g. `MV1.STR`, `MOV/MV3.STR`).
    pub path: String,
    /// Start LBA on the raw disc (Mode2/2352 sector index).
    pub lba: u32,
    /// File size in bytes as declared by the directory record.
    pub size: u32,
}

fn walk_directory_recursive(
    disc: &[u8],
    dir: &IsoRecord,
    prefix: &str,
    out: &mut Vec<FileEntry>,
    depth: u32,
) {
    if depth > 8 {
        return;
    }
    let Some(children) = list_directory(disc, dir) else {
        return;
    };
    for c in children {
        let full = if prefix.is_empty() {
            c.name.clone()
        } else {
            format!("{prefix}/{}", c.name)
        };
        if c.is_dir {
            walk_directory_recursive(disc, &c, &full, out, depth + 1);
        } else {
            out.push(FileEntry {
                path: full,
                lba: c.lba,
                size: c.size,
            });
        }
    }
}

/// Walk every directory on a Mode2/2352 disc and return a flat list of files
/// with their (path, lba, size). Returns an empty vec when the disc isn't a
/// valid Mode2/2352 image.
pub fn walk_iso_files(disc: &[u8]) -> Vec<FileEntry> {
    let mut out = Vec::new();
    if !is_mode2_2352_disc(disc) {
        return out;
    }
    let Some(pvd) = read_sector(disc, 16) else {
        return out;
    };
    let Some(root) = parse_iso_record(&pvd[156..156 + 34]) else {
        return out;
    };
    if !root.is_dir {
        return out;
    }
    walk_directory_recursive(disc, &root, "", &mut out, 0);
    out
}

/// Read the raw 2352-byte sector at `lba` (sync + header + subheader + user
/// data + EDC). Used by the XA demuxer, which needs the subheader bytes.
/// Returns `None` if the sector index is past the disc end.
pub fn read_raw_sector(disc: &[u8], lba: u32) -> Option<&[u8]> {
    let start = lba as usize * RAW_SECTOR_SIZE;
    let end = start + RAW_SECTOR_SIZE;
    (end <= disc.len()).then(|| &disc[start..end])
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

/// Walk a Mode2/2352 disc image and return the bytes of the SCUS_942.54
/// executable (the retail NA build's main `.EXE`). The semicolon-version
/// suffix used by ISO9660 is already stripped by `parse_iso_record`.
pub fn extract_scus(disc: &[u8]) -> Option<Vec<u8>> {
    extract_root_file(disc, "SCUS_942.54")
}
