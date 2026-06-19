use anyhow::{Result, bail};

use crate::raw::{RawDisc, SECTOR_SIZE, USER_DATA_OFFSET, USER_DATA_SIZE};

#[derive(Debug, Clone)]
pub struct DirectoryRecord {
    pub lba: u32,
    pub size: u32,
    pub is_dir: bool,
    pub name: String,
}

pub struct Volume {
    pub volume_id: String,
    pub root: DirectoryRecord,
}

pub fn read_volume(disc: &mut RawDisc) -> Result<Volume> {
    let pvd = disc.read_sector(16)?;
    if pvd[0] != 1 || &pvd[1..6] != b"CD001" {
        bail!("not ISO9660 (PVD missing or wrong type at LBA 16)");
    }
    let volume_id = std::str::from_utf8(&pvd[40..72])
        .unwrap_or("")
        .trim_end()
        .to_string();
    let root = parse_record(&pvd[156..156 + 34])?;
    Ok(Volume { volume_id, root })
}

fn parse_record(buf: &[u8]) -> Result<DirectoryRecord> {
    if buf.len() < 33 {
        bail!("directory record too short ({} bytes)", buf.len());
    }
    let lba = u32::from_le_bytes(buf[2..6].try_into().unwrap());
    let size = u32::from_le_bytes(buf[10..14].try_into().unwrap());
    let flags = buf[25];
    let is_dir = flags & 0x02 != 0;
    let name_len = buf[32] as usize;
    if 33 + name_len > buf.len() {
        bail!("directory record name length out of bounds");
    }
    let name = parse_name(&buf[33..33 + name_len]);
    Ok(DirectoryRecord {
        lba,
        size,
        is_dir,
        name,
    })
}

fn parse_name(b: &[u8]) -> String {
    match b {
        [0] => ".".into(),
        [1] => "..".into(),
        _ => {
            let s = String::from_utf8_lossy(b);
            match s.find(';') {
                Some(pos) => s[..pos].to_string(),
                None => s.into_owned(),
            }
        }
    }
}

/// Largest directory-extent size we'll honour from an on-disc record, in
/// bytes. The 32-bit `size` field of a directory record is attacker-
/// controlled; a junk value near `u32::MAX` would otherwise drive
/// [`RawDisc::read_user_data`] to reserve/read ~8 TiB before the underlying
/// read fails. Real ISO9660 directories are a handful of 2 KiB blocks; 64 MiB
/// is far past any plausible directory while keeping the allocation bounded.
const MAX_DIRECTORY_BYTES: u32 = 64 * 1024 * 1024;

pub fn list_directory(disc: &mut RawDisc, dir: &DirectoryRecord) -> Result<Vec<DirectoryRecord>> {
    if !dir.is_dir {
        bail!("not a directory: {}", dir.name);
    }
    if dir.size > MAX_DIRECTORY_BYTES {
        bail!(
            "directory extent {} bytes exceeds the {} byte sanity limit",
            dir.size,
            MAX_DIRECTORY_BYTES
        );
    }
    let sector_count = dir.size.div_ceil(USER_DATA_SIZE as u32);
    let mut buf = Vec::new();
    disc.read_user_data(dir.lba, sector_count, &mut buf)?;
    buf.truncate(dir.size as usize);

    let mut entries = Vec::new();
    let mut offset = 0usize;
    while offset < buf.len() {
        let len = buf[offset] as usize;
        if len == 0 {
            // Directory records do not span logical block boundaries; skip
            // padding to the next 2048-byte block.
            let next = offset.div_ceil(USER_DATA_SIZE) * USER_DATA_SIZE;
            let next = if next == offset {
                offset + USER_DATA_SIZE
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
        let entry = parse_record(&buf[offset..offset + len])?;
        if entry.name != "." && entry.name != ".." {
            entries.push(entry);
        }
        offset += len;
    }
    Ok(entries)
}

/// Locate a top-level file in an **in-memory** Mode 2/2352 disc image by name,
/// returning its `(lba, size_bytes)`. Unlike [`walk_files`] (which streams from
/// a [`RawDisc`] file handle), this works on a byte slice - what an in-memory
/// patcher holds. Only the root directory is searched, which is where the disc's
/// data files (`PROT.DAT`, `SCUS_942.54`, …) live.
///
/// Returns `None` if the image isn't ISO 9660, the root directory can't be read,
/// or no root entry matches `name` (case-sensitive, version suffix stripped).
pub fn find_file_in_image(image: &[u8], name: &str) -> Option<(u32, u32)> {
    let user = |lba: usize| -> Option<&[u8]> {
        let base = lba * SECTOR_SIZE + USER_DATA_OFFSET;
        image.get(base..base + USER_DATA_SIZE)
    };

    // Primary Volume Descriptor at LBA 16.
    let pvd = user(16)?;
    if pvd[0] != 1 || &pvd[1..6] != b"CD001" {
        return None;
    }
    let root = parse_record(&pvd[156..156 + 34]).ok()?;
    if !root.is_dir || root.size > MAX_DIRECTORY_BYTES {
        return None;
    }

    // Read the root directory extent into a contiguous buffer.
    let sector_count = root.size.div_ceil(USER_DATA_SIZE as u32) as usize;
    let mut buf = Vec::with_capacity(sector_count * USER_DATA_SIZE);
    for i in 0..sector_count {
        buf.extend_from_slice(user(root.lba as usize + i)?);
    }
    buf.truncate(root.size as usize);

    // Walk records (same boundary rules as `list_directory`).
    let mut offset = 0usize;
    while offset < buf.len() {
        let len = buf[offset] as usize;
        if len == 0 {
            let next = offset.div_ceil(USER_DATA_SIZE) * USER_DATA_SIZE;
            let next = if next == offset {
                offset + USER_DATA_SIZE
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
        if let Ok(rec) = parse_record(&buf[offset..offset + len])
            && !rec.is_dir
            && rec.name == name
        {
            return Some((rec.lba, rec.size));
        }
        offset += len;
    }
    None
}

/// Read a top-level file's logical bytes out of an **in-memory** Mode 2/2352
/// disc image by name. Locates the file with [`find_file_in_image`], then
/// concatenates the 2048-byte user-data payloads of its sectors and truncates
/// to the directory-record size.
///
/// Returns `None` if the file isn't found or a sector runs past the image.
pub fn read_file_in_image(image: &[u8], name: &str) -> Option<Vec<u8>> {
    let (lba, size) = find_file_in_image(image, name)?;
    let sector_count = (size as usize).div_ceil(USER_DATA_SIZE);
    let mut out = Vec::with_capacity(sector_count * USER_DATA_SIZE);
    for i in 0..sector_count {
        let base = (lba as usize + i) * SECTOR_SIZE + USER_DATA_OFFSET;
        out.extend_from_slice(image.get(base..base + USER_DATA_SIZE)?);
    }
    out.truncate(size as usize);
    Some(out)
}

pub fn walk_files(
    disc: &mut RawDisc,
    root: &DirectoryRecord,
) -> Result<Vec<(String, DirectoryRecord)>> {
    use std::collections::HashSet;

    let mut out = Vec::new();
    let mut stack = vec![(String::new(), root.clone())];
    // A malformed disc can contain a directory record whose `lba` points at an
    // ancestor (or itself), which would make this descent loop forever. Track
    // the directory extents we've already entered and skip any we revisit.
    let mut visited: HashSet<u32> = HashSet::new();
    visited.insert(root.lba);
    while let Some((prefix, dir)) = stack.pop() {
        let mut entries = list_directory(disc, &dir)?;
        // Stable order: directories pushed last so files come out
        // alphabetically within each directory.
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        for entry in entries.into_iter().rev() {
            let path = if prefix.is_empty() {
                entry.name.clone()
            } else {
                format!("{}/{}", prefix, entry.name)
            };
            if entry.is_dir {
                // Only descend into a directory extent once. Prevents an
                // unbounded loop on a cyclic / self-referential directory tree.
                if visited.insert(entry.lba) {
                    stack.push((path, entry));
                }
            } else {
                out.push((path, entry));
            }
        }
    }
    Ok(out)
}
