//! Full-ISO relayout: grow a Mode 2/2352 disc image by whole sectors and cascade
//! every downstream LBA reference so the disc stays structurally valid.
//!
//! Where [`crate::write::patch_file_logical`] performs **same-size** in-place
//! edits (never moves an LBA), this module handles the harder case the official
//! PAL discs used at mastering: making a file (specifically `PROT.DAT`) **gain
//! sectors** so an interior asset can grow, then shifting every file that sits
//! after it on the disc.
//!
//! ## Why this is safe for Legaia (see `docs/formats/disc.md`)
//!
//! - Files are located by **ISO9660 name/directory lookup**, never by a
//!   hardcoded absolute LBA in the executable (proven: no little-endian LBA
//!   literal for any post-`PROT.DAT` file exists in USA or any PAL executable).
//! - The PROT internal TOC stores **PROT.DAT-relative** LBAs, so growing an
//!   interior PROT entry only needs an internal-TOC shift, not a disc-LBA
//!   cascade. That rewrite is the caller's job (it knows the TOC); this module
//!   takes the already-grown `PROT.DAT` payload and fixes the *disc*.
//!
//! ## The cascade this module performs
//!
//! Given the old image, `PROT.DAT`'s start LBA, its old sector count, and its new
//! (larger, whole-sector) logical payload, [`grow_prot_dat`]:
//!
//! 1. Rebuilds `PROT.DAT`'s sectors from the new payload (fresh EDC/ECC + correct
//!    per-sector MSF headers).
//! 2. **Relocates** every sector after `PROT.DAT` by `G` sectors, rewriting only
//!    each sector's sync + MSF header (Form 1 EDC/ECC do not cover the header, so
//!    a pure move needs no ECC recompute; Form 2 has no ECC).
//! 3. Renumbers every ISO9660 LBA reference `> prot_lba` by `+G`, grows
//!    `PROT.DAT`'s directory-record size by `+G*2048`, and grows the PVD volume
//!    space by `+G` - re-encoding each touched sector's EDC/ECC.
//!
//! No game bytes are embedded; the code is generic ISO9660 + ECMA-130 mechanics.

use anyhow::{Result, bail};

use crate::iso9660::find_file_in_image;
use crate::raw::{SECTOR_SIZE, USER_DATA_OFFSET, USER_DATA_SIZE};
use crate::write::{encode_mode2_form1_sector, is_form2};

/// LBA -> BCD MSF header bytes `[min, sec, frac]` (`v = lba + 150`, standard PSX).
fn lba_to_msf_bcd(lba: u32) -> [u8; 3] {
    let v = lba + 150;
    let m = v / (75 * 60);
    let s = (v / 75) % 60;
    let f = v % 75;
    let bcd = |n: u32| -> u8 { (((n / 10) << 4) | (n % 10)) as u8 };
    [bcd(m), bcd(s), bcd(f)]
}

/// Rewrite a physical sector's 12-byte sync pattern + 4-byte header so its stored
/// MSF address matches `lba` (the header mode byte and everything after the
/// header are left untouched). Form 1 EDC/ECC are computed with the header zeroed,
/// so a sector that only *moves* (unchanged user data) stays EDC/ECC-valid after
/// this - no recompute needed.
fn set_sector_address(sector: &mut [u8], lba: u32) {
    // Sync: 00 FF*10 00.
    sector[0] = 0x00;
    for b in sector.iter_mut().take(11).skip(1) {
        *b = 0xFF;
    }
    sector[11] = 0x00;
    let msf = lba_to_msf_bcd(lba);
    sector[12] = msf[0];
    sector[13] = msf[1];
    sector[14] = msf[2];
    // sector[15] (mode byte) unchanged.
}

fn read_u32_le(image: &[u8], off: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        image.get(off..off + 4)?.try_into().ok()?,
    ))
}

/// One directory record's location, as discovered by walking the *old* image.
struct DirRecordRef {
    /// LBA of the directory extent that physically holds this record.
    dir_extent_lba: u32,
    /// Byte offset of the record within that extent's logical bytes.
    rec_off: usize,
    /// The record's target extent LBA (LE field at rec_off+2).
    target_lba: u32,
    /// The record's data length (LE field at rec_off+10).
    size: u32,
    is_dir: bool,
    name: String,
}

/// Read a file's logical bytes from an in-memory image by (lba, size).
fn read_logical(image: &[u8], lba: u32, size: u32) -> Option<Vec<u8>> {
    let n = (size as usize).div_ceil(USER_DATA_SIZE);
    let mut out = Vec::with_capacity(n * USER_DATA_SIZE);
    for i in 0..n {
        let base = (lba as usize + i) * SECTOR_SIZE + USER_DATA_OFFSET;
        out.extend_from_slice(image.get(base..base + USER_DATA_SIZE)?);
    }
    out.truncate(size as usize);
    Some(out)
}

fn parse_name(b: &[u8]) -> String {
    match b {
        [0] => ".".into(),
        [1] => "..".into(),
        _ => {
            let s = String::from_utf8_lossy(b);
            s.split(';').next().unwrap_or("").to_string()
        }
    }
}

/// Walk one directory extent, yielding every record (including `.` / `..`).
fn walk_dir_extent(image: &[u8], dir_lba: u32, dir_size: u32, out: &mut Vec<DirRecordRef>) {
    let Some(buf) = read_logical(image, dir_lba, dir_size) else {
        return;
    };
    let mut off = 0usize;
    while off < buf.len() {
        let len = buf[off] as usize;
        if len == 0 {
            let next = off.div_ceil(USER_DATA_SIZE) * USER_DATA_SIZE;
            let next = if next == off {
                off + USER_DATA_SIZE
            } else {
                next
            };
            if next >= buf.len() {
                break;
            }
            off = next;
            continue;
        }
        if off + len > buf.len() || len < 33 {
            break;
        }
        let target_lba = u32::from_le_bytes(buf[off + 2..off + 6].try_into().unwrap());
        let size = u32::from_le_bytes(buf[off + 10..off + 14].try_into().unwrap());
        let flags = buf[off + 25];
        let name_len = buf[off + 32] as usize;
        let name = if 33 + name_len <= len {
            parse_name(&buf[off + 33..off + 33 + name_len])
        } else {
            String::new()
        };
        out.push(DirRecordRef {
            dir_extent_lba: dir_lba,
            rec_off: off,
            target_lba,
            size,
            is_dir: flags & 0x02 != 0,
            name,
        });
        off += len;
    }
}

/// Collect every directory record in the image (root + every subdirectory),
/// walking from the PVD root. Discovered on the **old** image, so the target/dir
/// LBAs are pre-relocation.
fn collect_dir_records(image: &[u8]) -> Result<Vec<DirRecordRef>> {
    let pvd_base = 16 * SECTOR_SIZE + USER_DATA_OFFSET;
    let pvd = image
        .get(pvd_base..pvd_base + USER_DATA_SIZE)
        .ok_or_else(|| anyhow::anyhow!("image too small for PVD"))?;
    if pvd[0] != 1 || &pvd[1..6] != b"CD001" {
        bail!("no ISO9660 PVD at LBA 16");
    }
    let root_lba = u32::from_le_bytes(pvd[156 + 2..156 + 6].try_into().unwrap());
    let root_size = u32::from_le_bytes(pvd[156 + 10..156 + 14].try_into().unwrap());

    let mut records = Vec::new();
    let mut dir_queue = vec![(root_lba, root_size)];
    let mut visited = std::collections::HashSet::new();
    visited.insert(root_lba);
    while let Some((lba, size)) = dir_queue.pop() {
        let start = records.len();
        walk_dir_extent(image, lba, size, &mut records);
        for r in &records[start..] {
            if r.is_dir && r.name != "." && r.name != ".." && visited.insert(r.target_lba) {
                dir_queue.push((r.target_lba, r.size));
            }
        }
    }
    Ok(records)
}

/// Re-encode the EDC/ECC of a single physical sector in the image (Form 1 only;
/// Form 2 sectors carry no ECC and are left as-is after a content edit is not
/// expected on them).
fn reencode_sector(image: &mut [u8], lba: u32) -> Result<()> {
    let base = lba as usize * SECTOR_SIZE;
    let sector = image
        .get_mut(base..base + SECTOR_SIZE)
        .ok_or_else(|| anyhow::anyhow!("sector {lba} past end of image"))?;
    if is_form2(sector) {
        return Ok(());
    }
    encode_mode2_form1_sector(sector)
}

/// Write `bytes` into the logical user data at `off` bytes into the file/extent
/// that begins at `lba`, re-encoding each touched Form 1 sector. Stays within
/// existing sectors (no growth).
fn patch_logical(image: &mut [u8], lba: u32, off: usize, bytes: &[u8]) -> Result<()> {
    let mut written = 0usize;
    let mut cur = off;
    while written < bytes.len() {
        let internal = cur / USER_DATA_SIZE;
        let in_sec = cur % USER_DATA_SIZE;
        let disc_lba = lba + internal as u32;
        let base = disc_lba as usize * SECTOR_SIZE;
        if base + SECTOR_SIZE > image.len() {
            bail!("logical patch past end of image at sector {disc_lba}");
        }
        let take = (USER_DATA_SIZE - in_sec).min(bytes.len() - written);
        let ud = base + USER_DATA_OFFSET + in_sec;
        image[ud..ud + take].copy_from_slice(&bytes[written..written + take]);
        reencode_sector(image, disc_lba)?;
        written += take;
        cur += take;
    }
    Ok(())
}

/// Grow `PROT.DAT` in a Mode 2/2352 disc image and cascade every downstream LBA
/// reference.
///
/// - `image`: the original disc image bytes.
/// - `prot_lba`, `old_prot_sectors`: `PROT.DAT`'s disc start LBA and current
///   sector count (from the ISO9660 directory record).
/// - `new_prot_payload`: the new `PROT.DAT` logical payload. Its length must be a
///   whole multiple of 2048 and larger than the old payload by `G*2048` bytes.
///   The caller (which owns the PROT internal-TOC knowledge) is responsible for
///   having rebuilt the payload with grown entries + a shifted internal TOC; this
///   function only fixes the *disc*.
///
/// Returns the new image. Every touched sector is EDC/ECC-valid; every sector
/// after `PROT.DAT` is MSF-relocated by `G` sectors.
pub fn grow_prot_dat(
    image: &[u8],
    prot_lba: u32,
    old_prot_sectors: u32,
    new_prot_payload: &[u8],
) -> Result<Vec<u8>> {
    if !new_prot_payload.len().is_multiple_of(USER_DATA_SIZE) {
        bail!(
            "new PROT.DAT payload ({} bytes) is not a whole number of 2048-byte sectors",
            new_prot_payload.len()
        );
    }
    let new_prot_sectors = (new_prot_payload.len() / USER_DATA_SIZE) as u32;
    if new_prot_sectors < old_prot_sectors {
        bail!("relayout only grows: new={new_prot_sectors} < old={old_prot_sectors} sectors");
    }
    let growth = new_prot_sectors - old_prot_sectors;
    if growth == 0 {
        // Nothing to relayout; caller should have used the same-size path.
        return Ok(image.to_vec());
    }
    if !image.len().is_multiple_of(SECTOR_SIZE) {
        bail!(
            "image length {} is not a whole number of sectors",
            image.len()
        );
    }
    let total_old_sectors = image.len() / SECTOR_SIZE;
    let prot_end = prot_lba as usize + old_prot_sectors as usize;
    if prot_end > total_old_sectors {
        bail!("PROT.DAT extent runs past end of image");
    }

    // Template physical sector for freshly-built PROT.DAT sectors: reuse
    // PROT.DAT's own sector-0 framing (sync + header + subheader), which is a
    // Form 1 data sector, so the new sectors match the file's own shape.
    let tmpl_base = prot_lba as usize * SECTOR_SIZE;
    let template: [u8; SECTOR_SIZE] = image[tmpl_base..tmpl_base + SECTOR_SIZE]
        .try_into()
        .map_err(|_| anyhow::anyhow!("PROT.DAT sector 0 missing"))?;
    if is_form2(&template) {
        bail!("PROT.DAT sector 0 is Form 2; expected Form 1 data");
    }

    let new_total_sectors = total_old_sectors + growth as usize;
    let mut out = vec![0u8; new_total_sectors * SECTOR_SIZE];

    // 1. Front matter [0, prot_lba): copied verbatim (patched later).
    out[..tmpl_base].copy_from_slice(&image[..tmpl_base]);

    // 2. PROT.DAT sectors: freshly built from the new payload.
    for i in 0..new_prot_sectors as usize {
        let lba = prot_lba + i as u32;
        let base = lba as usize * SECTOR_SIZE;
        let sec = &mut out[base..base + SECTOR_SIZE];
        // Copy sync/header/subheader framing from the template, then user data.
        sec.copy_from_slice(&template);
        let ud = &new_prot_payload[i * USER_DATA_SIZE..(i + 1) * USER_DATA_SIZE];
        sec[USER_DATA_OFFSET..USER_DATA_OFFSET + USER_DATA_SIZE].copy_from_slice(ud);
        set_sector_address(sec, lba);
        encode_mode2_form1_sector(sec)?;
    }

    // 3. Post-PROT.DAT sectors: relocated by `growth`, MSF header rewritten only.
    for old_lba in prot_end..total_old_sectors {
        let new_lba = old_lba + growth as usize;
        let src = &image[old_lba * SECTOR_SIZE..(old_lba + 1) * SECTOR_SIZE];
        let dst_base = new_lba * SECTOR_SIZE;
        out[dst_base..dst_base + SECTOR_SIZE].copy_from_slice(src);
        set_sector_address(&mut out[dst_base..dst_base + SECTOR_SIZE], new_lba as u32);
    }

    // 4. Renumber every ISO9660 LBA reference > prot_lba, grow PROT.DAT's record
    //    size, and grow the PVD volume space. Records are discovered on the OLD
    //    image (pre-relocation), then the edit is applied at the record's NEW
    //    physical location.
    renumber_iso(&mut out, image, prot_lba, growth)?;

    Ok(out)
}

/// Map an old disc LBA to its position in the relocated image.
fn relocate(old_lba: u32, prot_lba: u32, growth: u32) -> u32 {
    if old_lba > prot_lba {
        old_lba + growth
    } else {
        old_lba
    }
}

fn renumber_iso(out: &mut [u8], old_image: &[u8], prot_lba: u32, growth: u32) -> Result<()> {
    // 4a. PVD volume space (LE @80, BE @84). The PVD sits at LBA 16 < prot_lba,
    //     so its physical position is unchanged.
    let pvd_lba = 16u32;
    let pvd_base = pvd_lba as usize * SECTOR_SIZE + USER_DATA_OFFSET;
    let vol_space = read_u32_le(out, pvd_base + 80)
        .ok_or_else(|| anyhow::anyhow!("PVD too small"))?
        .wrapping_add(growth);
    out[pvd_base + 80..pvd_base + 84].copy_from_slice(&vol_space.to_le_bytes());
    out[pvd_base + 84..pvd_base + 88].copy_from_slice(&vol_space.to_be_bytes());
    // PVD path-table location fields (@140 LE, @148 BE) point at LBA 18..21, all
    // < prot_lba, so they are unchanged.
    reencode_sector(out, pvd_lba)?;

    // 4b. Path tables: LE @18 (+opt @19), BE @20 (+opt @21). Each stores directory
    //     extent LBAs; MOV/XA (> prot_lba) shift, root (22 < prot_lba) doesn't.
    let ptbl_size = read_u32_le(old_image, pvd_base + 132).unwrap_or(0);
    for (ptbl_lba, big_endian) in [(18u32, false), (19, false), (20, true), (21, true)] {
        renumber_path_table(out, ptbl_lba, ptbl_size, prot_lba, growth, big_endian)?;
    }

    // 4c. Directory records: discovered on the OLD image, applied at NEW location.
    let records = collect_dir_records(old_image)?;
    for r in &records {
        let dir_new_lba = relocate(r.dir_extent_lba, prot_lba, growth);
        // Shift the record's target extent LBA if it points past PROT.DAT.
        if r.target_lba > prot_lba {
            let new_target = r.target_lba + growth;
            patch_logical(out, dir_new_lba, r.rec_off + 2, &new_target.to_le_bytes())?;
        }
        // Grow PROT.DAT's own file-record size by G*2048 (it is the file we grew).
        if !r.is_dir && r.name == "PROT.DAT" {
            let new_size = r.size + growth * USER_DATA_SIZE as u32;
            patch_logical(out, dir_new_lba, r.rec_off + 10, &new_size.to_le_bytes())?;
        }
    }
    Ok(())
}

fn renumber_path_table(
    out: &mut [u8],
    ptbl_lba: u32,
    ptbl_size: u32,
    prot_lba: u32,
    growth: u32,
    big_endian: bool,
) -> Result<()> {
    if ptbl_size == 0 {
        return Ok(());
    }
    let Some(buf) = read_logical(out, ptbl_lba, ptbl_size) else {
        return Ok(());
    };
    let mut patched = buf.clone();
    let mut off = 0usize;
    while off + 8 <= patched.len() {
        let name_len = patched[off] as usize;
        if name_len == 0 {
            break;
        }
        let ext = if big_endian {
            u32::from_be_bytes(patched[off + 2..off + 6].try_into().unwrap())
        } else {
            u32::from_le_bytes(patched[off + 2..off + 6].try_into().unwrap())
        };
        if ext > prot_lba {
            let ne = ext + growth;
            let b = if big_endian {
                ne.to_be_bytes()
            } else {
                ne.to_le_bytes()
            };
            patched[off + 2..off + 6].copy_from_slice(&b);
        }
        off += 8 + name_len + (name_len & 1);
    }
    if patched != buf {
        patch_logical(out, ptbl_lba, 0, &patched)?;
    }
    Ok(())
}

/// Convenience wrapper: locate `PROT.DAT` by name and grow it. `old_prot_sectors`
/// is derived from the directory-record size.
pub fn grow_prot_dat_by_name(image: &[u8], new_prot_payload: &[u8]) -> Result<Vec<u8>> {
    let (prot_lba, prot_size) = find_file_in_image(image, "PROT.DAT")
        .ok_or_else(|| anyhow::anyhow!("PROT.DAT not found"))?;
    let old_prot_sectors = (prot_size as usize).div_ceil(USER_DATA_SIZE) as u32;
    grow_prot_dat(image, prot_lba, old_prot_sectors, new_prot_payload)
}

#[cfg(test)]
mod tests;
