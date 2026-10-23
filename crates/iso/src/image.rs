//! In-memory Mode2/2352 disc access over a borrowed byte slice. No
//! `std::fs`, no JS bindings - works on both the wasm32 and native targets,
//! which is what the browser hosts load a visitor's disc image through, and
//! it can be unit-tested with a real disc image when one is available.
//!
//! [`walk_iso_files`] enumerates every file (subdirectories included);
//! [`extract_prot_dat`], [`extract_cdname_txt`] and [`extract_scus`] return a
//! root file's logical bytes (sector-stripped, file-size-truncated), matching
//! the name case-insensitively with the `;1` version suffix stripped. The
//! directory walk itself is [`crate::iso9660`]'s in-memory one
//! ([`list_directory_in_image`], [`read_file_in_image`]), the same reader the
//! patcher uses, so the browser and the CLI resolve a name identically. The
//! PROT.DAT table of contents those bytes carry is parsed by
//! `legaia_prot::toc_index`.

use crate::iso9660::{DirectoryRecord, list_directory_in_image, read_file_in_image, root_in_image};

const RAW_SECTOR_SIZE: usize = 2352;
const RAW_USER_DATA_OFFSET: usize = 24;
const RAW_USER_DATA_SIZE: usize = 2048;

/// Deepest directory nesting [`walk_iso_files`] descends into. A cyclic or
/// corrupt directory tree stops here instead of recursing without end.
const MAX_WALK_DEPTH: u32 = 8;

/// True if `disc` looks like a Mode2/2352 PSX disc image (sector-aligned size +
/// valid ISO9660 PVD at LBA 16).
pub fn is_mode2_2352_disc(disc: &[u8]) -> bool {
    if disc.len() < 17 * RAW_SECTOR_SIZE || !disc.len().is_multiple_of(RAW_SECTOR_SIZE) {
        return false;
    }
    let start = 16 * RAW_SECTOR_SIZE + RAW_USER_DATA_OFFSET;
    let pvd = &disc[start..start + RAW_USER_DATA_SIZE];
    pvd[0] == 1 && &pvd[1..6] == b"CD001"
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
    dir: &DirectoryRecord,
    prefix: &str,
    out: &mut Vec<FileEntry>,
    depth: u32,
) {
    if depth > MAX_WALK_DEPTH {
        return;
    }
    let Some(children) = list_directory_in_image(disc, dir) else {
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
/// with their (path, lba, size), in on-disc record order. Returns an empty vec
/// when the disc isn't a valid Mode2/2352 image.
pub fn walk_iso_files(disc: &[u8]) -> Vec<FileEntry> {
    let mut out = Vec::new();
    if !is_mode2_2352_disc(disc) {
        return out;
    }
    let Some(root) = root_in_image(disc) else {
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

/// A named root file's bytes (sector-stripped, file-size-truncated), or `None`
/// when `disc` is not a Mode2/2352 image or has no such file.
fn extract_root_file(disc: &[u8], name: &str) -> Option<Vec<u8>> {
    if !is_mode2_2352_disc(disc) {
        return None;
    }
    read_file_in_image(disc, name)
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
/// executable (the retail NA build's main `.EXE`). The ISO9660 `;1` version
/// suffix is stripped by the directory-record parser.
pub fn extract_scus(disc: &[u8]) -> Option<Vec<u8>> {
    extract_root_file(disc, "SCUS_942.54")
}
