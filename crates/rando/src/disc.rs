//! Disc bridge: apply same-size asset edits to a real disc image.
//!
//! Ties the editing primitives (e.g. [`crate::monster::set_drop`]) to the
//! sector-level write-back in [`legaia_iso::write`]. The chain a PROT-entry edit
//! travels:
//!
//! ```text
//! disc image (2352-byte sectors)
//!   -> ISO 9660: PROT.DAT lives at disc sector `prot_lba`
//!     -> PROT TOC: entry N starts at `start_lba[N] * 2048` bytes into PROT.DAT
//!       -> asset: an edit at `offset_in_entry` bytes into the entry
//! ```
//!
//! so a PROT-entry-relative byte offset maps to the PROT.DAT-logical offset
//! `start_lba[N] * 2048 + offset_in_entry`, which
//! [`legaia_iso::write::patch_file_logical`] turns into physical-sector writes
//! plus EDC/ECC re-encode. Every edit is **same-size** — it overwrites bytes in
//! place and never moves an LBA, so no TOC or directory needs rewriting.
//!
//! [`DiscPatcher`] owns a mutable copy of the user's disc; it reads and writes
//! that copy and is serialized by the caller. It embeds no game bytes.

use anyhow::{Context, Result, bail};
use legaia_asset::monster_archive::SLOT_STRIDE;
use legaia_iso::iso9660::find_file_in_image;
use legaia_iso::raw::{SECTOR_SIZE, USER_DATA_OFFSET, USER_DATA_SIZE};
use legaia_prot::archive::Archive as ProtArchive;

/// PROT entry index of the monster `battle_data` archive.
pub const MONSTER_ARCHIVE_ENTRY: usize = 867;

/// One PROT entry's on-disc placement, captured once at open time.
#[derive(Debug, Clone, Copy)]
struct EntrySpan {
    /// Start LBA (sectors) within PROT.DAT.
    start_lba: u32,
    /// Full on-disc footprint in bytes (what the loader reads).
    size_bytes: u64,
}

/// A mutable disc image plus the addressing it needs to patch PROT entries.
pub struct DiscPatcher {
    image: Vec<u8>,
    /// Disc sector where `PROT.DAT` begins (ISO 9660 directory record).
    prot_lba: u32,
    /// Per-PROT-entry placement.
    entries: Vec<EntrySpan>,
}

/// Read `sector_count` sectors of 2048-byte user data starting at `lba` out of
/// an in-memory 2352-byte-per-sector disc image.
fn read_user_data(image: &[u8], lba: u32, sector_count: usize) -> Result<Vec<u8>> {
    let mut out = Vec::with_capacity(sector_count * USER_DATA_SIZE);
    for i in 0..sector_count {
        let base = (lba as usize + i) * SECTOR_SIZE + USER_DATA_OFFSET;
        let slice = image
            .get(base..base + USER_DATA_SIZE)
            .with_context(|| format!("sector {} past end of disc image", lba as usize + i))?;
        out.extend_from_slice(slice);
    }
    Ok(out)
}

impl DiscPatcher {
    /// Parse a disc image: locate `PROT.DAT` and read its TOC. Takes ownership
    /// of the image bytes so later patches mutate them in place.
    pub fn open(image: Vec<u8>) -> Result<Self> {
        let (prot_lba, prot_size) =
            find_file_in_image(&image, "PROT.DAT").context("PROT.DAT not found in disc image")?;
        let sectors = (prot_size as usize).div_ceil(USER_DATA_SIZE);
        let mut payload = read_user_data(&image, prot_lba, sectors)?;
        payload.truncate(prot_size as usize);
        let archive = ProtArchive::from_bytes(payload).context("parse PROT.DAT TOC")?;
        let entries = archive
            .entries
            .iter()
            .map(|e| EntrySpan {
                start_lba: e.start_lba,
                size_bytes: e.size_bytes,
            })
            .collect();
        Ok(Self {
            image,
            prot_lba,
            entries,
        })
    }

    /// Number of PROT entries.
    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    /// Absolute disc sector (LBA) where PROT entry `index`'s content begins —
    /// `prot_lba + start_lba[index]`. This is the value the game's CD reader
    /// (`FUN_8005E4D4`) takes, so an injected loader stub can be given this LBA
    /// as a literal to stream the entry in at runtime. `None` if out of range.
    pub fn entry_disc_lba(&self, index: usize) -> Option<u32> {
        self.entries.get(index).map(|e| self.prot_lba + e.start_lba)
    }

    /// PROT entry `index`'s on-disc footprint in bytes (what the loader reads).
    pub fn entry_footprint(&self, index: usize) -> Option<u64> {
        self.entries.get(index).map(|e| e.size_bytes)
    }

    /// Read PROT entry `index`'s full on-disc footprint from the current
    /// (possibly already-patched) image, so reads after writes are correct.
    pub fn read_entry(&self, index: usize) -> Result<Vec<u8>> {
        let span = self
            .entries
            .get(index)
            .with_context(|| format!("PROT entry {index} out of range"))?;
        let sectors = (span.size_bytes as usize).div_ceil(USER_DATA_SIZE);
        let mut out = read_user_data(&self.image, self.prot_lba + span.start_lba, sectors)?;
        out.truncate(span.size_bytes as usize);
        Ok(out)
    }

    /// Overwrite `bytes` at `offset_in_entry` bytes into PROT entry `index`,
    /// re-encoding every touched sector's EDC/ECC. Same-size, in-place; never
    /// grows the image or moves an LBA.
    pub fn patch_prot_entry(
        &mut self,
        index: usize,
        offset_in_entry: u64,
        bytes: &[u8],
    ) -> Result<()> {
        let span = *self
            .entries
            .get(index)
            .with_context(|| format!("PROT entry {index} out of range"))?;
        let end = offset_in_entry + bytes.len() as u64;
        if end > span.size_bytes {
            bail!(
                "patch [{offset_in_entry}, +{}] exceeds entry {index} footprint ({} bytes)",
                bytes.len(),
                span.size_bytes
            );
        }
        let logical_off = span.start_lba as u64 * USER_DATA_SIZE as u64 + offset_in_entry;
        legaia_iso::write::patch_file_logical(&mut self.image, self.prot_lba, logical_off, bytes)
    }

    /// Replace monster `id`'s `0x14000`-byte slot in the `battle_data` archive
    /// with `new_slot` (which must be exactly one slot). Use with a slot built
    /// by [`crate::monster::set_drop`] / [`crate::monster::repack_slot`].
    pub fn patch_monster_slot(&mut self, id: u16, new_slot: &[u8]) -> Result<()> {
        if id == 0 {
            bail!("monster id is 1-based; 0 is invalid");
        }
        if new_slot.len() != SLOT_STRIDE {
            bail!(
                "monster slot must be {SLOT_STRIDE} bytes, got {}",
                new_slot.len()
            );
        }
        let offset_in_entry = (id as u64 - 1) * SLOT_STRIDE as u64;
        self.patch_prot_entry(MONSTER_ARCHIVE_ENTRY, offset_in_entry, new_slot)
    }

    /// Read monster `id`'s current `0x14000`-byte slot from the image.
    pub fn monster_slot(&self, id: u16) -> Result<Vec<u8>> {
        if id == 0 {
            bail!("monster id is 1-based; 0 is invalid");
        }
        let entry = self.read_entry(MONSTER_ARCHIVE_ENTRY)?;
        let start = (id as usize - 1) * SLOT_STRIDE;
        let end = start + SLOT_STRIDE;
        if end > entry.len() {
            bail!("monster id {id} slot past end of archive");
        }
        Ok(entry[start..end].to_vec())
    }

    /// Read an arbitrary ISO 9660 file by name from the current (possibly
    /// patched) image. Used for static tables that live outside `PROT.DAT` —
    /// e.g. the steal table in `SCUS_942.54`.
    pub fn read_named_file(&self, name: &str) -> Option<Vec<u8>> {
        legaia_iso::iso9660::read_file_in_image(&self.image, name)
    }

    /// Overwrite `bytes` at `logical_off` bytes into an arbitrary ISO 9660 file
    /// (by name), re-encoding every touched sector's EDC/ECC. Same-size,
    /// in-place; never grows the image or moves an LBA. This is the non-PROT
    /// sibling of [`Self::patch_prot_entry`] — the steal randomizer uses it to
    /// edit the `SCUS_942.54` steal table.
    pub fn patch_named_file(&mut self, name: &str, logical_off: u64, bytes: &[u8]) -> Result<()> {
        let (lba, size) = find_file_in_image(&self.image, name)
            .with_context(|| format!("{name} not found in disc image"))?;
        let end = logical_off + bytes.len() as u64;
        if end > size as u64 {
            bail!(
                "patch [{logical_off}, +{}] exceeds {name} ({size} bytes)",
                bytes.len()
            );
        }
        legaia_iso::write::patch_file_logical(&mut self.image, lba, logical_off, bytes)
    }

    /// Parse the disc's `CDNAME.TXT` scene-name map. Returns `None` if the file
    /// is absent or unreadable. Used by the scoped encounter randomizer to bucket
    /// scenes into kingdoms (see [`crate::kingdom`]).
    pub fn cdname(&self) -> Option<legaia_prot::cdname::IndexMap> {
        let bytes = self.read_named_file("CDNAME.TXT")?;
        let text = String::from_utf8_lossy(&bytes);
        legaia_prot::cdname::parse_str(&text).ok()
    }

    /// Borrow the current (possibly patched) disc image.
    pub fn image(&self) -> &[u8] {
        &self.image
    }

    /// Consume the patcher and return the patched disc image.
    pub fn into_image(self) -> Vec<u8> {
        self.image
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a tiny but real Mode 2 Form 1 disc whose ISO 9660 root holds a
    /// single file, "PROT.DAT", with the given logical payload. Enough structure
    /// for find_file_in_image + the read/write paths.
    fn synth_disc(prot_payload: &[u8]) -> Vec<u8> {
        const PVD_LBA: u32 = 16;
        const ROOT_LBA: u32 = 17;
        const PROT_LBA: u32 = 18;

        let prot_sectors = prot_payload.len().div_ceil(USER_DATA_SIZE).max(1);
        let total_sectors = PROT_LBA as usize + prot_sectors;
        let mut image = vec![0u8; total_sectors * SECTOR_SIZE];

        // Shape every sector as a valid empty Form 1 sector first.
        for s in 0..total_sectors {
            let base = s * SECTOR_SIZE;
            image[base + 1..base + 11].fill(0xFF); // sync
            image[base + 0x0F] = 0x02; // mode 2
            image[base + 0x12] = 0x08; // submode: data, Form 1
            image[base + 0x16] = 0x08;
        }

        // Write a payload into a file's sectors + encode each.
        let put = |image: &mut [u8], lba: u32, data: &[u8]| {
            let mut off = 0usize;
            let mut sec = lba as usize;
            while off < data.len() {
                let base = sec * SECTOR_SIZE;
                let take = USER_DATA_SIZE.min(data.len() - off);
                image[base + USER_DATA_OFFSET..base + USER_DATA_OFFSET + take]
                    .copy_from_slice(&data[off..off + take]);
                legaia_iso::write::encode_mode2_form1_sector(&mut image[base..base + SECTOR_SIZE])
                    .unwrap();
                off += take;
                sec += 1;
            }
        };

        // PVD at sector 16: "CD001" magic at +1, root dir record at +156.
        let mut pvd = vec![0u8; USER_DATA_SIZE];
        pvd[0] = 1; // PVD type
        pvd[1..6].copy_from_slice(b"CD001");
        let mut root_rec = [0u8; 34];
        root_rec[0] = 34;
        root_rec[2..6].copy_from_slice(&ROOT_LBA.to_le_bytes());
        root_rec[10..14].copy_from_slice(&(USER_DATA_SIZE as u32).to_le_bytes());
        root_rec[25] = 0x02; // directory flag
        root_rec[32] = 1; // name len (the "." record)
        pvd[156..156 + 34].copy_from_slice(&root_rec);
        put(&mut image, PVD_LBA, &pvd);

        // Root directory at sector 17: one file record for PROT.DAT.
        let name = b"PROT.DAT;1";
        let rec_len = 33 + name.len();
        let mut root = vec![0u8; USER_DATA_SIZE];
        root[0] = rec_len as u8;
        root[2..6].copy_from_slice(&PROT_LBA.to_le_bytes());
        root[10..14].copy_from_slice(&(prot_payload.len() as u32).to_le_bytes());
        root[25] = 0x00; // file
        root[32] = name.len() as u8;
        root[33..33 + name.len()].copy_from_slice(name);
        put(&mut image, ROOT_LBA, &root);

        // PROT.DAT payload.
        put(&mut image, PROT_LBA, prot_payload);
        image
    }

    /// A minimal PROT.DAT logical payload the real `Archive::from_bytes` parses
    /// into three entries (0 at LBA 0, 1 at LBA 1, 2 at LBA 2); entry 1 holds
    /// `entry1_data` at its start.
    ///
    /// Header (at byte 0): `[pad u32][file_num_minus_1 u32][header_sectors u32]`.
    /// The archive's TOC begins at byte 8, so `toc[0]` aliases `header_sectors`;
    /// `toc[j]` lives at byte `8 + 4*j`. For entry p the walker reads
    /// `start = toc[p+2]`, `next = toc[p+3]`, `end = toc[p+5]`, with
    /// `indexed = end - next + 4` sectors. An entry whose
    /// `start*2048 + size` runs past the file is dropped (which would shift
    /// indices), so the payload is sized at 8 sectors and the TOC is monotone
    /// (LBAs 0..5) so all three entries fit.
    fn synth_prot(entry1_data: &[u8]) -> Vec<u8> {
        let sec = USER_DATA_SIZE;
        let mut prot = vec![0u8; 8 * sec];
        let put = |p: &mut [u8], off: usize, v: u32| {
            p[off..off + 4].copy_from_slice(&v.to_le_bytes());
        };
        // Header: file_num_minus_1 = 3 -> 3 usable entries (p = 0,1,2).
        put(&mut prot, 4, 3);
        // toc[j] at byte 8 + 4*j. Monotone LBAs 0,1,2,3,4,5.
        let tw = |p: &mut [u8], j: usize, v: u32| put(p, 8 + 4 * j, v);
        tw(&mut prot, 0, 1); // toc[0] = header_sectors = 1
        tw(&mut prot, 1, 0); // toc[1]
        tw(&mut prot, 2, 0); // toc[2] entry0 start
        tw(&mut prot, 3, 1); // toc[3] entry1 start
        tw(&mut prot, 4, 2); // toc[4] entry2 start
        tw(&mut prot, 5, 3); // toc[5] entry0 end -> indexed0 = 3-1+4 = 6 sectors
        tw(&mut prot, 6, 4); // toc[6] entry1 end -> indexed1 = 4-2+4 = 6 sectors
        tw(&mut prot, 7, 5); // toc[7] entry2 end -> indexed2 = 5-3+4 = 6 sectors

        prot[sec..sec + entry1_data.len()].copy_from_slice(entry1_data);
        prot
    }

    #[test]
    fn patch_prot_entry_round_trips_through_the_disc() {
        let payload = b"HELLO-WORLD-ORIGINAL-CONTENT-1234567890".to_vec();
        let prot = synth_prot(&payload);
        let disc = synth_disc(&prot);
        let mut patcher = DiscPatcher::open(disc).unwrap();
        assert!(patcher.entry_count() >= 2, "expected >=2 PROT entries");

        // Entry 1's bytes start with the payload.
        let before = patcher.read_entry(1).unwrap();
        assert!(
            before.starts_with(b"HELLO-WORLD"),
            "entry 1 should start with the seeded payload, got {:?}",
            &before[..16.min(before.len())]
        );

        // Patch 5 bytes at offset 6 within entry 1.
        patcher.patch_prot_entry(1, 6, b"BRAVO").unwrap();
        let after = patcher.read_entry(1).unwrap();
        assert!(
            after.starts_with(b"HELLO-BRAVO"),
            "patched bytes must read back through the disc + ISO + PROT chain, got {:?}",
            &after[..16.min(after.len())]
        );

        // The patched PROT.DAT sector is still EDC/ECC-valid.
        let prot_sector_base = (18 + 1) * SECTOR_SIZE; // PROT_LBA + entry1 lba
        assert!(legaia_iso::write::mode2_form1_sector_is_valid(
            &patcher.image()[prot_sector_base..prot_sector_base + SECTOR_SIZE]
        ));
    }

    #[test]
    fn out_of_range_entry_errors() {
        let disc = synth_disc(&synth_prot(b"x"));
        let mut patcher = DiscPatcher::open(disc).unwrap();
        assert!(patcher.patch_prot_entry(99, 0, b"z").is_err());
    }

    #[test]
    fn monster_slot_id_zero_is_rejected() {
        let disc = synth_disc(&synth_prot(b"x"));
        let patcher = DiscPatcher::open(disc).unwrap();
        assert!(patcher.monster_slot(0).is_err());
    }
}
