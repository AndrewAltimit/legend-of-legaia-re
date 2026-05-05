use anyhow::{Result, bail};

use crate::raw::{RawDisc, USER_DATA_SIZE};

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

pub fn list_directory(disc: &mut RawDisc, dir: &DirectoryRecord) -> Result<Vec<DirectoryRecord>> {
    if !dir.is_dir {
        bail!("not a directory: {}", dir.name);
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

pub fn walk_files(
    disc: &mut RawDisc,
    root: &DirectoryRecord,
) -> Result<Vec<(String, DirectoryRecord)>> {
    let mut out = Vec::new();
    let mut stack = vec![(String::new(), root.clone())];
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
                stack.push((path, entry));
            } else {
                out.push((path, entry));
            }
        }
    }
    Ok(out)
}
