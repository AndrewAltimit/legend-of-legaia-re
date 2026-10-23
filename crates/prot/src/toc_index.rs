//! Zero-copy index over a `PROT.DAT` buffer: each entry's byte offset and
//! size inside a borrowed slice, without copying the entry bytes out. The
//! browser hosts hold a whole disc's `PROT.DAT` in WASM memory and slice it
//! through [`parse_prot_toc`]; [`crate::archive::Archive`] is the owning
//! reader. Both size an entry through the one span rule in
//! [`crate::runtime_toc`].
//!
//! The `PROT.DAT` bytes themselves come off an in-memory disc image through
//! `legaia_iso::image::extract_prot_dat`.

const SECTOR: u32 = 0x800;

/// One PROT.DAT entry's location inside its owning buffer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntryMeta {
    pub index: u32,
    pub byte_offset: u64,
    pub size_bytes: u64,
}

/// Parse the PROT.DAT TOC and return a vector of entry locations within `buf`.
/// The header lives at file offset 0x000 or 0x800; layout (matching
/// `crate::archive::detect_header`):
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
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| u32::from_le_bytes(*c))
        .collect();

    // An entry's size is the sector gap to the next entry, and the span comes
    // from the port of retail's own routine rather than a second copy of the
    // arithmetic - `crates/prot`'s `Archive` calls the same function. The walk
    // is open-coded here only because it must stay zero-copy over a borrowed
    // 121 MB buffer in WASM; the size rule is not duplicated. See
    // docs/formats/prot.md (in particular why `toc[p+5] - toc[p+3] + 4` is
    // *not* this entry's size).
    const MAX_FOOTPRINT_SECTORS: u32 = 64 * 1024;
    let count = file_num.saturating_sub(1) as usize;
    let mut entries = Vec::with_capacity(count);
    for p in 0..count {
        if p + 3 >= toc.len() {
            break;
        }
        let start_lba = toc[p + 2];
        // LBA 0 is the archive header - a row pointing there is TOC padding.
        if start_lba == 0 {
            continue;
        }
        let Some(size_sectors) = crate::runtime_toc::entry_sector_span_from_archive_toc(&toc, p)
        else {
            continue;
        };
        // A wrapped (non-monotonic / zero-padded) or empty row is not an entry.
        if size_sectors == 0 || size_sectors > MAX_FOOTPRINT_SECTORS {
            continue;
        }
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
