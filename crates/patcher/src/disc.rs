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
//! plus EDC/ECC re-encode. Every edit is **same-size** - it overwrites bytes in
//! place and never moves an LBA, so no TOC or directory needs rewriting.
//!
//! [`DiscPatcher`] owns a mutable copy of the user's disc; it reads and writes
//! that copy and is serialized by the caller. It embeds no game bytes.

use std::collections::BTreeMap;

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
    /// `PROT.DAT`'s logical size in whole 2048-byte sectors.
    prot_sectors: u32,
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
            prot_sectors: sectors as u32,
            entries,
        })
    }

    /// Number of PROT entries.
    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    /// `(byte_offset, size_bytes, index)` for every PROT entry, in the same
    /// index space [`Self::patch_prot_entry`] takes - the span shape
    /// `legaia_asset::tim_catalog::build_from_spans` and its deep sibling
    /// consume, so a caller can catalog the flat `PROT.DAT` payload without
    /// re-parsing the TOC.
    pub fn entry_spans(&self) -> Vec<(u64, u64, u32)> {
        self.entries
            .iter()
            .enumerate()
            .map(|(i, e)| {
                (
                    e.start_lba as u64 * USER_DATA_SIZE as u64,
                    e.size_bytes,
                    i as u32,
                )
            })
            .collect()
    }

    /// Absolute disc sector (LBA) where PROT entry `index`'s content begins -
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

    /// PROT entry `index`'s **true on-disc footprint** in bytes: the span to the
    /// next entry (`next_start - start`), which is the real size a scene bundle
    /// occupies. This differs from [`Self::entry_footprint`] /
    /// [`Self::read_entry`], which return `max(indexed, footprint)` and can
    /// over-read into later entries. `None` if `index` is out of range.
    pub fn entry_true_footprint_sectors(&self, index: usize) -> Option<u32> {
        let start = self.entries.get(index)?.start_lba;
        let next = self
            .entries
            .get(index + 1)
            .map(|e| e.start_lba)
            .unwrap_or(self.prot_sectors);
        Some(next.saturating_sub(start))
    }

    /// Read PROT entry `index`'s true-footprint bytes (`next_start - start`
    /// sectors) from the current image. This is the payload a relayout grows.
    pub fn read_entry_footprint(&self, index: usize) -> Result<Vec<u8>> {
        let span = self
            .entries
            .get(index)
            .with_context(|| format!("PROT entry {index} out of range"))?;
        let sectors = self
            .entry_true_footprint_sectors(index)
            .with_context(|| format!("PROT entry {index} footprint"))?;
        read_user_data(
            &self.image,
            self.prot_lba + span.start_lba,
            sectors as usize,
        )
    }

    /// Grow a set of PROT entries by whole sectors, cascading every downstream
    /// disc LBA reference (a **full-ISO relayout**; see
    /// [`legaia_iso::relayout`]).
    ///
    /// `new_payloads[index]` is entry `index`'s new full-footprint payload; its
    /// length must be a whole 2048-byte-sector multiple `>=` the entry's current
    /// true footprint. The caller builds each payload (e.g. a scene bundle whose
    /// MAN grew + shifted sub-assets + rewritten descriptor offsets). This method
    /// rewrites `PROT.DAT`'s **PROT-relative** internal TOC so every entry after a
    /// grown one shifts, rebuilds `PROT.DAT`, and fixes the ISO9660 directory /
    /// path-table / PVD references + re-encodes every touched sector.
    ///
    /// The PROT entry **index space is preserved** (no entries added/removed), so
    /// later same-size in-place edits keyed by index still resolve. After this
    /// call the patcher is re-opened against the grown image, so entry LBAs are
    /// current.
    pub fn grow_prot_entries(&mut self, new_payloads: &BTreeMap<usize, Vec<u8>>) -> Result<()> {
        if new_payloads.is_empty() {
            return Ok(());
        }
        // The current PROT.DAT logical payload.
        let old_prot = read_user_data(&self.image, self.prot_lba, self.prot_sectors as usize)?;
        let sector = USER_DATA_SIZE;

        // Per-entry growth (in sectors) + validation.
        let n = self.entries.len();
        let mut growth = vec![0u32; n];
        for (&idx, payload) in new_payloads {
            if idx >= n {
                bail!("PROT entry {idx} out of range for growth");
            }
            if !payload.len().is_multiple_of(sector) {
                bail!("grown PROT entry {idx} payload is not a whole number of sectors");
            }
            let new_sectors = (payload.len() / sector) as u32;
            let old_sectors = self
                .entry_true_footprint_sectors(idx)
                .with_context(|| format!("PROT entry {idx} footprint"))?;
            if new_sectors < old_sectors {
                bail!(
                    "grown PROT entry {idx} shrank ({new_sectors} < {old_sectors} sectors); relayout only grows"
                );
            }
            growth[idx] = new_sectors - old_sectors;
        }

        // Cumulative shift applied to entry j = sum of growth of entries < j.
        let start0 = self.entries[0].start_lba as usize * sector;
        let mut new_prot = Vec::with_capacity(old_prot.len());
        new_prot.extend_from_slice(&old_prot[..start0]);
        for e in 0..n {
            let start = self.entries[e].start_lba as usize * sector;
            let foot_sectors = self.entry_true_footprint_sectors(e).unwrap() as usize;
            let end = start + foot_sectors * sector;
            match new_payloads.get(&e) {
                Some(p) => new_prot.extend_from_slice(p),
                None => new_prot.extend_from_slice(&old_prot[start..end]),
            }
        }
        debug_assert_eq!(new_prot.len() % sector, 0);

        // Rewrite the internal TOC: entry j's start LBA word sits at PROT byte
        // `8 + (j+2)*4` (PROT-relative; see docs/formats/prot.md). Shift by the
        // cumulative growth of every earlier entry.
        let mut cum = 0u32;
        for (j, span) in self.entries.iter().enumerate() {
            let new_start = span.start_lba + cum;
            let off = 8 + (j + 2) * 4;
            if off + 4 <= start0 {
                new_prot[off..off + 4].copy_from_slice(&new_start.to_le_bytes());
            }
            cum += growth[j];
        }

        // Disc-level relayout: grow PROT.DAT + cascade every LBA reference.
        let new_image = legaia_iso::relayout::grow_prot_dat(
            &self.image,
            self.prot_lba,
            self.prot_sectors,
            &new_prot,
        )?;

        // Re-open against the grown image so entry LBAs / prot_sectors are current
        // and later index-keyed same-size edits resolve.
        *self = Self::open(new_image)?;
        Ok(())
    }

    /// Move the boundaries between a **contiguous run** of PROT entries
    /// without growing the image: `payloads[index]` is each entry's new
    /// full-footprint payload (a whole number of sectors); the run's total
    /// footprint must stay exactly what it was, so the sectors one entry
    /// gives up are the sectors its neighbour gains. Rewrites the run's
    /// bytes at their new positions and the PROT-relative internal TOC
    /// start words of every entry after the first (same-size edits of the
    /// TOC sectors - no LBA outside the run moves, no relayout, and a PPF
    /// patch still expresses it). Entry indices are preserved.
    ///
    /// This is how a player battle file gains room for a transplanted
    /// equipment record: the retail files tile their footprints exactly,
    /// but a neighbour repacked with the optimal LZS parse frees a few
    /// sectors, and the boundary between the two simply moves.
    pub fn reassign_prot_entries(&mut self, payloads: &BTreeMap<usize, Vec<u8>>) -> Result<()> {
        if payloads.is_empty() {
            return Ok(());
        }
        let sector = USER_DATA_SIZE;
        let first = *payloads.keys().next().unwrap();
        let last = *payloads.keys().next_back().unwrap();
        if payloads.len() != last - first + 1 {
            bail!("PROT entry reassignment needs a contiguous run of entries");
        }
        let mut old_total = 0u32;
        let mut new_total = 0u32;
        for (&idx, payload) in payloads {
            if !payload.len().is_multiple_of(sector) {
                bail!("reassigned PROT entry {idx} payload is not a whole number of sectors");
            }
            old_total += self
                .entry_true_footprint_sectors(idx)
                .with_context(|| format!("PROT entry {idx} footprint"))?;
            new_total += (payload.len() / sector) as u32;
        }
        if old_total != new_total {
            bail!(
                "reassigned PROT entries {first}..={last} would change the run's footprint \
                 ({old_total} -> {new_total} sectors); only a relayout can grow it"
            );
        }
        // New starts, sequential from the run's first entry.
        let mut start = self.entries[first].start_lba;
        let mut new_starts: Vec<(usize, u32, usize)> = Vec::with_capacity(payloads.len());
        for (&idx, payload) in payloads {
            new_starts.push((idx, start, payload.len()));
            start += (payload.len() / sector) as u32;
        }
        // Payloads land at their new positions (the run is rewritten whole,
        // so overlapping old/new spans never read stale bytes).
        for (idx, start, _) in &new_starts {
            let logical_off = *start as u64 * sector as u64;
            legaia_iso::write::patch_file_logical(
                &mut self.image,
                self.prot_lba,
                logical_off,
                &payloads[idx],
            )
            .with_context(|| format!("write reassigned PROT entry {idx}"))?;
        }
        // Internal TOC start words (`8 + (j+2)*4`, PROT-relative) for the
        // entries whose start moved; entry `first` stays where it was.
        for (idx, start, _) in &new_starts {
            if *idx == first {
                continue;
            }
            let off = 8 + (idx + 2) * 4;
            legaia_iso::write::patch_file_logical(
                &mut self.image,
                self.prot_lba,
                off as u64,
                &start.to_le_bytes(),
            )
            .with_context(|| format!("rewrite TOC start of PROT entry {idx}"))?;
        }
        for (idx, start, len) in new_starts {
            self.entries[idx] = EntrySpan {
                start_lba: start,
                size_bytes: len as u64,
            };
        }
        Ok(())
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

    /// Silence a Mode 2 Form 2 XA audio file in place: zero the ADPCM
    /// payload of every Form 2 sector (subheaders - the channel routing -
    /// survive, so the streamer still plays the file; it just decodes to
    /// silence). Form 1 sectors inside the extent are left alone. Returns
    /// the number of sectors silenced.
    pub fn silence_xa_file(&mut self, name: &str) -> Result<usize> {
        self.silence_xa(name, None)
    }

    /// [`Self::silence_xa_file`] restricted to a channel set: only Form 2
    /// sectors whose subheader channel (byte `0x11`) is in `channels` are
    /// zeroed; every other channel's audio survives. Multi-voice banks
    /// like `XA30.XA` interleave many speakers into one file, one channel
    /// each - the party's grunt channels can go quiet without touching
    /// anyone else's.
    pub fn silence_xa_channels(&mut self, name: &str, channels: &[u8]) -> Result<usize> {
        self.silence_xa(name, Some(channels))
    }

    /// Every distinct channel number present among a Mode 2 Form 2 XA
    /// file's sectors, ascending.
    pub fn xa_channels(&self, name: &str) -> Result<Vec<u8>> {
        let (lba, size) = legaia_iso::iso9660::find_path_in_image(&self.image, name)
            .with_context(|| format!("{name} not found in disc image"))?;
        let sectors = (size as usize).div_ceil(USER_DATA_SIZE);
        let mut seen = std::collections::BTreeSet::new();
        for i in 0..sectors {
            let base = (lba as usize + i) * SECTOR_SIZE;
            let Some(sector) = self.image.get(base..base + SECTOR_SIZE) else {
                break;
            };
            if legaia_iso::write::is_form2(sector) {
                seen.insert(sector[0x11]);
            }
        }
        Ok(seen.into_iter().collect())
    }

    /// The CD-XA coding byte (subheader `+0x13`) of a channel's first
    /// Form 2 sector: bits 0-1 = mono/stereo, 2-3 = sample rate
    /// (0 = 37800 Hz, 1 = 18900 Hz), 4-5 = bits per sample.
    pub fn xa_channel_coding(&self, name: &str, chan: u8) -> Result<u8> {
        let (lba, size) = legaia_iso::iso9660::find_path_in_image(&self.image, name)
            .with_context(|| format!("{name} not found in disc image"))?;
        let sectors = (size as usize).div_ceil(USER_DATA_SIZE);
        for i in 0..sectors {
            let base = (lba as usize + i) * SECTOR_SIZE;
            let Some(sector) = self.image.get(base..base + SECTOR_SIZE) else {
                break;
            };
            if legaia_iso::write::is_form2(sector) && sector[0x11] == chan {
                return Ok(sector[0x13]);
            }
        }
        bail!("{name}: channel {chan} has no Form 2 sectors")
    }

    /// Demux and decode one channel of a Mode 2 Form 2 XA file to mono
    /// PCM (stereo channels down-mix). Returns `(pcm, sample_rate)`.
    /// Reads the file as it CURRENTLY stands on the image - callers that
    /// need retail audio must read before muting the channel.
    pub fn read_xa_channel_pcm(&self, name: &str, chan: u8) -> Result<(Vec<i16>, u32)> {
        let (lba, size) = legaia_iso::iso9660::find_path_in_image(&self.image, name)
            .with_context(|| format!("{name} not found in disc image"))?;
        let sectors = (size as usize).div_ceil(USER_DATA_SIZE);
        let mut payload = Vec::new();
        let mut coding = None;
        for i in 0..sectors {
            let base = (lba as usize + i) * SECTOR_SIZE;
            let Some(sector) = self.image.get(base..base + SECTOR_SIZE) else {
                break;
            };
            if legaia_iso::write::is_form2(sector)
                && sector[0x12] & 0x04 != 0
                && sector[0x11] == chan
            {
                coding.get_or_insert(sector[0x13]);
                payload.extend_from_slice(&sector[0x18..0x18 + 2304]);
            }
        }
        let coding =
            coding.with_context(|| format!("{name}: channel {chan} has no audio sectors"))?;
        let stereo = coding & 0x03 != 0;
        let rate = if (coding >> 2) & 0x03 == 1 {
            18900
        } else {
            37800
        };
        let opts = legaia_xa::DecodeOptions {
            channels: if stereo {
                legaia_xa::Channels::Stereo
            } else {
                legaia_xa::Channels::Mono
            },
            sample_rate: rate,
            ..Default::default()
        };
        let (pcm, _) = legaia_xa::decode(&payload, opts)
            .with_context(|| format!("decode {name} channel {chan}"))?;
        let mono = if stereo {
            pcm.chunks_exact(2)
                .map(|c| ((c[0] as i32 + c[1] as i32) / 2) as i16)
                .collect()
        } else {
            pcm
        };
        Ok((mono, rate))
    }

    /// Write encoded XA-ADPCM sound groups into one channel of a Mode 2
    /// Form 2 XA file: the channel's Form 2 sectors (in stream order)
    /// take 18 groups each from `groups` until it runs out; the rest of
    /// the channel is zeroed (silence). Subheaders survive untouched.
    /// Returns the number of sectors written with audio.
    pub fn write_xa_channel(&mut self, name: &str, chan: u8, groups: &[u8]) -> Result<usize> {
        const GROUPS_PER_SECTOR: usize = 18;
        const GROUP_BYTES: usize = 128;
        let (lba, size) = legaia_iso::iso9660::find_path_in_image(&self.image, name)
            .with_context(|| format!("{name} not found in disc image"))?;
        let sectors = (size as usize).div_ceil(USER_DATA_SIZE);
        let mut cursor = 0usize;
        let mut written = 0usize;
        for i in 0..sectors {
            let base = (lba as usize + i) * SECTOR_SIZE;
            let Some(sector) = self.image.get_mut(base..base + SECTOR_SIZE) else {
                break;
            };
            if !legaia_iso::write::is_form2(sector) || sector[0x11] != chan {
                continue;
            }
            sector[0x18..0x92C].fill(0);
            if cursor < groups.len() {
                let take = (groups.len() - cursor).min(GROUPS_PER_SECTOR * GROUP_BYTES);
                sector[0x18..0x18 + take].copy_from_slice(&groups[cursor..cursor + take]);
                cursor += take;
                written += 1;
            }
            legaia_iso::write::encode_mode2_form2_sector(sector)?;
        }
        if written == 0 {
            bail!("{name}: channel {chan} has no Form 2 sectors");
        }
        Ok(written)
    }

    /// Raw sound-group payload of one channel of a Mode 2 Form 2 XA
    /// file, in stream order (2304 bytes = 18 groups per audio sector),
    /// plus the channel's coding byte.
    pub fn read_xa_channel_payload(&self, name: &str, chan: u8) -> Result<(Vec<u8>, u8)> {
        let (lba, size) = legaia_iso::iso9660::find_path_in_image(&self.image, name)
            .with_context(|| format!("{name} not found in disc image"))?;
        let sectors = (size as usize).div_ceil(USER_DATA_SIZE);
        let mut payload = Vec::new();
        let mut coding = None;
        for i in 0..sectors {
            let base = (lba as usize + i) * SECTOR_SIZE;
            let Some(sector) = self.image.get(base..base + SECTOR_SIZE) else {
                break;
            };
            if legaia_iso::write::is_form2(sector)
                && sector[0x12] & 0x04 != 0
                && sector[0x11] == chan
            {
                coding.get_or_insert(sector[0x13]);
                payload.extend_from_slice(&sector[0x18..0x18 + 2304]);
            }
        }
        let coding =
            coding.with_context(|| format!("{name}: channel {chan} has no audio sectors"))?;
        Ok((payload, coding))
    }

    /// Overwrite one channel's sound-group payload starting at the
    /// channel's `first`-th audio sector (0-based, stream order).
    /// `payload` must be a whole number of 2304-byte sector payloads.
    /// Sectors outside the span keep their bytes; every touched sector
    /// is EDC re-encoded. Returns the number of sectors rewritten.
    pub fn rewrite_xa_channel_span(
        &mut self,
        name: &str,
        chan: u8,
        first: usize,
        payload: &[u8],
    ) -> Result<usize> {
        if payload.is_empty() || !payload.len().is_multiple_of(2304) {
            bail!("span payload must be a whole number of 2304-byte sectors");
        }
        let want = payload.len() / 2304;
        let (lba, size) = legaia_iso::iso9660::find_path_in_image(&self.image, name)
            .with_context(|| format!("{name} not found in disc image"))?;
        let sectors = (size as usize).div_ceil(USER_DATA_SIZE);
        let mut ordinal = 0usize;
        let mut written = 0usize;
        for i in 0..sectors {
            let base = (lba as usize + i) * SECTOR_SIZE;
            let Some(sector) = self.image.get_mut(base..base + SECTOR_SIZE) else {
                break;
            };
            if !legaia_iso::write::is_form2(sector)
                || sector[0x12] & 0x04 == 0
                || sector[0x11] != chan
            {
                continue;
            }
            if ordinal >= first && ordinal < first + want {
                let src = (ordinal - first) * 2304;
                sector[0x18..0x18 + 2304].copy_from_slice(&payload[src..src + 2304]);
                legaia_iso::write::encode_mode2_form2_sector(sector)?;
                written += 1;
            }
            ordinal += 1;
        }
        if written != want {
            bail!("{name}: channel {chan} span {first}+{want} covered only {written} sectors");
        }
        Ok(written)
    }

    fn silence_xa(&mut self, name: &str, channels: Option<&[u8]>) -> Result<usize> {
        let (lba, size) = legaia_iso::iso9660::find_path_in_image(&self.image, name)
            .with_context(|| format!("{name} not found in disc image"))?;
        // The mastering records Form 2 extents with 2048-unit sizes (the
        // XA/ files' LBA gaps equal size/2048 exactly).
        let sectors = (size as usize).div_ceil(USER_DATA_SIZE);
        let mut silenced = 0usize;
        for i in 0..sectors {
            let base = (lba as usize + i) * SECTOR_SIZE;
            let Some(sector) = self.image.get_mut(base..base + SECTOR_SIZE) else {
                break;
            };
            if !legaia_iso::write::is_form2(sector) {
                continue;
            }
            if channels.is_some_and(|set| !set.contains(&sector[0x11])) {
                continue;
            }
            sector[0x18..0x92C].fill(0);
            legaia_iso::write::encode_mode2_form2_sector(sector)?;
            silenced += 1;
        }
        if silenced == 0 {
            bail!("{name}: no Form 2 sectors found to silence");
        }
        Ok(silenced)
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

    /// Read `len` bytes at `logical_off` bytes into `PROT.DAT` from the
    /// current (possibly patched) image, clamped to the file's end. Lets a
    /// caller with a **flat PROT.DAT coordinate** (e.g. a TIM in the unindexed
    /// system-UI gap before entry 0, which no [`Self::read_entry`] covers)
    /// read a window without copying the whole multi-hundred-MB payload.
    pub fn read_prot_bytes(&self, logical_off: u64, len: usize) -> Result<Vec<u8>> {
        let total = self.prot_sectors as u64 * USER_DATA_SIZE as u64;
        if logical_off >= total {
            bail!("offset {logical_off} past end of PROT.DAT ({total} bytes)");
        }
        let end = (logical_off + len as u64).min(total);
        let first = (logical_off / USER_DATA_SIZE as u64) as u32;
        let last = end.div_ceil(USER_DATA_SIZE as u64) as u32;
        let raw = read_user_data(&self.image, self.prot_lba + first, (last - first) as usize)?;
        let skip = (logical_off - first as u64 * USER_DATA_SIZE as u64) as usize;
        Ok(raw[skip..skip + (end - logical_off) as usize].to_vec())
    }

    /// Read an arbitrary ISO 9660 file by name from the current (possibly
    /// patched) image. Used for static tables that live outside `PROT.DAT` -
    /// e.g. the steal table in `SCUS_942.54`.
    pub fn read_named_file(&self, name: &str) -> Option<Vec<u8>> {
        legaia_iso::iso9660::read_file_in_image(&self.image, name)
    }

    /// Overwrite `bytes` at `logical_off` bytes into an arbitrary ISO 9660 file
    /// (by name), re-encoding every touched sector's EDC/ECC. Same-size,
    /// in-place; never grows the image or moves an LBA. This is the non-PROT
    /// sibling of [`Self::patch_prot_entry`] - the steal randomizer uses it to
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

// ---------------------------------------------------------------------------
// The DMY.DAT annex: room outside PROT.DAT for records that outgrow it.
// ---------------------------------------------------------------------------

/// Where an annexed player file's records went.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnnexPlacement {
    /// Absolute disc sector of the first annexed record.
    pub lba: u32,
    /// Sectors the region occupies.
    pub sectors: u32,
    /// The descriptor displacement written into the table: the byte distance
    /// from the entry's data base to `lba`.
    pub base: u32,
}

/// Marker the allocator keeps in `DMY.DAT`'s last sector so a second patch
/// of an already-annexed disc allocates past what the first one placed.
const ANNEX_MAGIC: &[u8; 4] = b"LGAX";
const ANNEX_VERSION: u32 = 1;
/// The first DMY sector is its own archive header; leaving it lets the
/// archive walkers still see a well-formed (if junk) file.
const ANNEX_FIRST_SECTOR: u32 = 1;
/// Player-file prologue the loader reads before seeking to any slot: the
/// header, `record[0]` and the descriptor table, `0x8000` bytes.
pub const PLAYER_FILE_DATA_BASE: usize = 0x8000;

impl DiscPatcher {
    /// `DMY.DAT`'s disc extent `(lba, sectors)`. The file is developer
    /// fixtures no retail code path loads (`docs/formats/dmy.md`), all Form 1
    /// sectors, which makes it the disc's spare room.
    fn dmy_extent(&self) -> Result<(u32, u32)> {
        let (lba, size) = find_file_in_image(&self.image, "DMY.DAT")
            .context("DMY.DAT not found in disc image (no annex room)")?;
        Ok((lba, (size as usize).div_ceil(USER_DATA_SIZE) as u32))
    }

    /// Sectors of `DMY.DAT` already handed out, from the marker (0 when the
    /// disc has never been annexed).
    fn annex_used(&self, dmy_lba: u32, dmy_sectors: u32) -> Result<u32> {
        let marker = read_user_data(&self.image, dmy_lba + dmy_sectors - 1, 1)?;
        if &marker[..4] != ANNEX_MAGIC {
            return Ok(0);
        }
        let version = u32::from_le_bytes(marker[4..8].try_into().unwrap());
        if version != ANNEX_VERSION {
            bail!("DMY.DAT annex marker has unknown version {version}");
        }
        Ok(u32::from_le_bytes(marker[8..12].try_into().unwrap()))
    }

    /// Reserve `sectors` whole sectors in the annex and return the absolute
    /// disc LBA of the first. Allocation is a bump pointer persisted in the
    /// marker sector; nothing is written to the reserved sectors here.
    pub fn annex_alloc(&mut self, sectors: u32) -> Result<u32> {
        if sectors == 0 {
            bail!("annex allocation of zero sectors");
        }
        let (dmy_lba, dmy_sectors) = self.dmy_extent()?;
        let used = self
            .annex_used(dmy_lba, dmy_sectors)?
            .max(ANNEX_FIRST_SECTOR);
        // The marker owns the last sector.
        let room = dmy_sectors.saturating_sub(1);
        if used + sectors > room {
            bail!(
                "DMY.DAT annex is full: {sectors} sector(s) wanted, {} free of {room}",
                room.saturating_sub(used)
            );
        }
        let mut marker = vec![0u8; USER_DATA_SIZE];
        marker[..4].copy_from_slice(ANNEX_MAGIC);
        marker[4..8].copy_from_slice(&ANNEX_VERSION.to_le_bytes());
        marker[8..12].copy_from_slice(&(used + sectors).to_le_bytes());
        legaia_iso::write::patch_file_logical(
            &mut self.image,
            dmy_lba,
            (dmy_sectors as u64 - 1) * USER_DATA_SIZE as u64,
            &marker,
        )
        .context("write DMY.DAT annex marker")?;
        Ok(dmy_lba + used)
    }

    /// Sectors the annex still has to give.
    pub fn annex_free_sectors(&self) -> Result<u32> {
        let (dmy_lba, dmy_sectors) = self.dmy_extent()?;
        let used = self
            .annex_used(dmy_lba, dmy_sectors)?
            .max(ANNEX_FIRST_SECTOR);
        Ok(dmy_sectors.saturating_sub(1).saturating_sub(used))
    }

    /// Where PROT entry `index`'s player file keeps its records, if they
    /// were annexed: the placement decoded from the in-place table.
    pub fn player_file_annex(&self, index: usize) -> Result<Option<AnnexPlacement>> {
        let head = self.read_entry_footprint(index)?;
        let Some(chain) = legaia_asset::player_file_annex::chain(&head) else {
            return Ok(None);
        };
        if !chain.is_annexed() {
            return Ok(None);
        }
        let entry_lba = self
            .entry_disc_lba(index)
            .with_context(|| format!("PROT entry {index} LBA"))?;
        let data_lba = entry_lba + (PLAYER_FILE_DATA_BASE / USER_DATA_SIZE) as u32;
        Ok(Some(AnnexPlacement {
            lba: data_lba + chain.base / USER_DATA_SIZE as u32,
            sectors: (chain.region_len() / USER_DATA_SIZE) as u32,
            base: chain.base,
        }))
    }

    /// PROT entry `index` as the **retail-shaped** player file: the entry's
    /// own sectors when its records are in place, or the in-place header
    /// with the annexed records read back behind it when they are not.
    /// Every `battle_data_pack` reader takes the result as-is.
    pub fn read_player_file(&self, index: usize) -> Result<Vec<u8>> {
        let head = self.read_entry_footprint(index)?;
        let Some(place) = self.player_file_annex(index)? else {
            return Ok(head);
        };
        let region = read_user_data(&self.image, place.lba, place.sectors as usize)
            .with_context(|| format!("read annexed records of PROT entry {index}"))?;
        legaia_asset::player_file_annex::materialize(&head, PLAYER_FILE_DATA_BASE, &region)
            .with_context(|| format!("materialise annexed PROT entry {index}"))
    }

    /// Park a rebuilt retail-shaped player `file` (records chained from 0
    /// at [`PLAYER_FILE_DATA_BASE`]) with its records in the annex: the
    /// header goes in place over PROT entry `index` (same size), the slot
    /// region into freshly allocated `DMY.DAT` sectors, and the table's
    /// offsets are displaced to reach them. The entry's old record sectors
    /// are left as they were; nothing reads them any more.
    pub fn annex_player_file(&mut self, index: usize, file: &[u8]) -> Result<AnnexPlacement> {
        let entry_lba = self
            .entry_disc_lba(index)
            .with_context(|| format!("PROT entry {index} LBA"))?;
        let foot = self
            .entry_true_footprint_sectors(index)
            .with_context(|| format!("PROT entry {index} footprint"))?;
        if (foot as usize) * USER_DATA_SIZE < PLAYER_FILE_DATA_BASE {
            bail!("PROT entry {index} is shorter than a player-file prologue");
        }
        let chain = legaia_asset::player_file_annex::chain(file)
            .with_context(|| format!("rebuilt file for PROT entry {index} is not a player file"))?;
        let sectors = (chain.region_len() / USER_DATA_SIZE) as u32;
        let lba = self.annex_alloc(sectors)?;
        let data_lba = entry_lba + (PLAYER_FILE_DATA_BASE / USER_DATA_SIZE) as u32;
        if lba <= data_lba {
            bail!(
                "annex at LBA {lba} is not past PROT entry {index}'s data base (forward seek only)"
            );
        }
        let base = (lba - data_lba) * USER_DATA_SIZE as u32;
        let (header, region) =
            legaia_asset::player_file_annex::split(file, PLAYER_FILE_DATA_BASE, base)?;
        self.patch_prot_entry(index, 0, &header)
            .with_context(|| format!("write annexed header of PROT entry {index}"))?;
        legaia_iso::write::patch_file_logical(&mut self.image, lba, 0, &region)
            .with_context(|| format!("write annexed records of PROT entry {index}"))?;
        Ok(AnnexPlacement { lba, sectors, base })
    }

    /// Overwrite `bytes` at `offset` into PROT entry `index`'s player file
    /// **as [`Self::read_player_file`] presents it** - routed to the entry
    /// when the file is in place, and to the annex for offsets inside an
    /// annexed slot region. A write must not straddle the two halves.
    pub fn patch_player_file(&mut self, index: usize, offset: u64, bytes: &[u8]) -> Result<()> {
        let Some(place) = self.player_file_annex(index)? else {
            return self.patch_prot_entry(index, offset, bytes);
        };
        let db = PLAYER_FILE_DATA_BASE as u64;
        let end = offset + bytes.len() as u64;
        if end <= db {
            return self.patch_prot_entry(index, offset, bytes);
        }
        if offset < db {
            bail!(
                "player-file write [{offset}, +{}] straddles the annex boundary",
                bytes.len()
            );
        }
        let region_len = place.sectors as u64 * USER_DATA_SIZE as u64;
        if end - db > region_len {
            bail!(
                "player-file write [{offset}, +{}] runs past the annexed region ({region_len} bytes)",
                bytes.len()
            );
        }
        legaia_iso::write::patch_file_logical(&mut self.image, place.lba, offset - db, bytes)
            .with_context(|| format!("write annexed records of PROT entry {index}"))
    }
}

/// Synthetic-disc builders shared by this module's tests and the texture
/// module's disc-free replacement tests.
#[cfg(test)]
pub(crate) mod synth {
    use super::*;

    /// Build a tiny but real Mode 2 Form 1 disc whose ISO 9660 root holds a
    /// single file, "PROT.DAT", with the given logical payload. Enough structure
    /// for find_file_in_image + the read/write paths.
    pub(crate) fn synth_disc(prot_payload: &[u8]) -> Vec<u8> {
        synth_disc_with_dmy(prot_payload, 0)
    }

    /// [`synth_disc`] plus a `DMY.DAT` of `dmy_sectors` zeroed sectors right
    /// after `PROT.DAT` - the annex's room.
    pub(crate) fn synth_disc_with_dmy(prot_payload: &[u8], dmy_sectors: usize) -> Vec<u8> {
        const PVD_LBA: u32 = 16;
        const ROOT_LBA: u32 = 17;
        const PROT_LBA: u32 = 18;

        let prot_sectors = prot_payload.len().div_ceil(USER_DATA_SIZE).max(1);
        let dmy_lba = PROT_LBA + prot_sectors as u32;
        let total_sectors = PROT_LBA as usize + prot_sectors + dmy_sectors;
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

        // Root directory at sector 17: a file record for PROT.DAT (and DMY.DAT).
        let mut root = vec![0u8; USER_DATA_SIZE];
        let mut at = 0usize;
        let mut file_rec = |name: &[u8], lba: u32, len: u32| {
            let rec_len = 33 + name.len();
            root[at] = rec_len as u8;
            root[at + 2..at + 6].copy_from_slice(&lba.to_le_bytes());
            root[at + 10..at + 14].copy_from_slice(&len.to_le_bytes());
            root[at + 25] = 0x00; // file
            root[at + 32] = name.len() as u8;
            root[at + 33..at + 33 + name.len()].copy_from_slice(name);
            at += rec_len;
        };
        file_rec(b"PROT.DAT;1", PROT_LBA, prot_payload.len() as u32);
        if dmy_sectors > 0 {
            file_rec(b"DMY.DAT;1", dmy_lba, (dmy_sectors * USER_DATA_SIZE) as u32);
        }
        put(&mut image, ROOT_LBA, &root);

        // PROT.DAT payload (DMY.DAT stays the zeroed Form 1 sectors above).
        put(&mut image, PROT_LBA, prot_payload);
        image
    }

    /// A minimal PROT.DAT logical payload the real `Archive::from_bytes` parses
    /// into three entries (0 at LBA 1, 1 at LBA 2, 2 at LBA 3); entry 1 holds
    /// `entry1_data` at its start.
    ///
    /// Header (at byte 0): `[pad u32][file_num_minus_1 u32][header_sectors u32]`.
    /// The archive's TOC begins at byte 8, so `toc[0]` aliases `header_sectors`;
    /// `toc[j]` lives at byte `8 + 4*j`. For entry `p` the walker reads
    /// `start = toc[p+2]` and `size = toc[p+3] - toc[p+2]` - the sector gap to
    /// the next entry, which is retail's own `FUN_8003E68C` arithmetic. (The
    /// superseded `toc[p+5] - toc[p+3] + 4` expression measured the two
    /// *successors*' span and over-read; see `docs/formats/prot.md`.)
    ///
    /// Two walker behaviours the LBAs here are chosen to satisfy, because either
    /// one silently **shifts entry indices** rather than failing:
    ///
    /// - a row with `start_lba == 0` is TOC padding, not an entry (LBA 0 holds
    ///   the archive header), so no entry may start there - hence LBAs 1..4
    ///   rather than 0..3;
    /// - an entry whose `start*2048 + size` runs past the file is dropped, so
    ///   the payload is 8 sectors, comfortably past the highest LBA used.
    pub(crate) fn synth_prot(entry1_data: &[u8]) -> Vec<u8> {
        let sec = USER_DATA_SIZE;
        let mut prot = vec![0u8; 8 * sec];
        let put = |p: &mut [u8], off: usize, v: u32| {
            p[off..off + 4].copy_from_slice(&v.to_le_bytes());
        };
        // Header: file_num_minus_1 = 3 -> 3 usable entries (p = 0,1,2).
        put(&mut prot, 4, 3);
        // toc[j] at byte 8 + 4*j. Monotone, and starting at LBA 1 so that no
        // entry row lands on the header sector.
        let tw = |p: &mut [u8], j: usize, v: u32| put(p, 8 + 4 * j, v);
        tw(&mut prot, 0, 1); // toc[0] = header_sectors = 1
        tw(&mut prot, 1, 0); // toc[1] (not read for any entry)
        tw(&mut prot, 2, 1); // toc[2] entry0 start -> LBA 1
        tw(&mut prot, 3, 2); // toc[3] entry1 start -> LBA 2; entry0 size = 1 sector
        tw(&mut prot, 4, 3); // toc[4] entry2 start -> LBA 3; entry1 size = 1 sector
        tw(&mut prot, 5, 4); // toc[5]              -> entry2 size = 1 sector
        tw(&mut prot, 6, 5); // toc[6] monotone tail
        tw(&mut prot, 7, 6); // toc[7] monotone tail

        // Entry 1 begins at LBA 2.
        let entry1_off = 2 * sec;
        prot[entry1_off..entry1_off + entry1_data.len()].copy_from_slice(entry1_data);
        prot
    }
}

#[cfg(test)]
mod tests {
    use super::synth::{synth_disc, synth_prot};
    use super::*;

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
        let prot_sector_base = (18 + 2) * SECTOR_SIZE; // PROT_LBA + entry1 lba (2)
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

#[cfg(test)]
mod annex_tests {
    use super::synth::{synth_disc_with_dmy, synth_prot};
    use super::*;

    /// A retail-shaped player file: 16-sector prologue with a 3-row table,
    /// then three records of one sector each (a sane `dec_size` prefix).
    fn player_file() -> Vec<u8> {
        let db = PLAYER_FILE_DATA_BASE;
        let mut f = vec![0u8; db];
        let table = 0x40u32;
        f[0..4].copy_from_slice(&table.to_le_bytes());
        f[4..8].copy_from_slice(&0x100u32.to_le_bytes());
        f[8..12].copy_from_slice(&0x200u32.to_le_bytes());
        f[12..16].copy_from_slice(&0x300u32.to_le_bytes());
        let rows = [(0x22u32, 0u32), (0u32, 0x800), (0x5u32, 0x1000)];
        for (i, (id, off)) in rows.iter().enumerate() {
            let p = table as usize + i * 12;
            f[p..p + 4].copy_from_slice(&id.to_le_bytes());
            f[p + 4..p + 8].copy_from_slice(&off.to_le_bytes());
            f[p + 8..p + 12].copy_from_slice(&0x800u32.to_le_bytes());
        }
        for i in 0..3u8 {
            let mut slot = vec![0x10 + i; 0x800];
            slot[..4].copy_from_slice(&0x100u32.to_le_bytes());
            f.extend_from_slice(&slot);
        }
        f
    }

    /// PROT with entry 1 sized to hold a full player file (19 sectors).
    fn prot_with_player_file(file: &[u8]) -> Vec<u8> {
        let sec = USER_DATA_SIZE;
        let mut prot = synth_prot(&[]);
        // Re-shape: entry 1 spans LBA 2..=20, entry 2 starts at 21.
        let put = |p: &mut [u8], j: usize, v: u32| {
            p[8 + 4 * j..12 + 4 * j].copy_from_slice(&v.to_le_bytes());
        };
        put(&mut prot, 4, 21);
        put(&mut prot, 5, 22);
        put(&mut prot, 6, 23);
        put(&mut prot, 7, 24);
        prot.resize(26 * sec, 0);
        prot[2 * sec..2 * sec + file.len()].copy_from_slice(file);
        prot
    }

    #[test]
    fn annex_round_trips_a_player_file_and_persists_its_marker() {
        let file = player_file();
        let disc = synth_disc_with_dmy(&prot_with_player_file(&file), 12);
        let mut patcher = DiscPatcher::open(disc).unwrap();
        assert!(patcher.player_file_annex(1).unwrap().is_none());
        assert_eq!(patcher.read_player_file(1).unwrap()[..file.len()], file[..]);
        // 12 DMY sectors: 1 header, 1 marker, 10 to give.
        assert_eq!(patcher.annex_free_sectors().unwrap(), 10);

        // Grow the file by one record and annex it.
        let mut grown = file.clone();
        let table = 0x40usize + 3 * 12;
        grown[table..table + 4].copy_from_slice(&0xBAu32.to_le_bytes());
        grown[table + 4..table + 8].copy_from_slice(&0x1800u32.to_le_bytes());
        grown[table + 8..table + 12].copy_from_slice(&0x800u32.to_le_bytes());
        let mut slot = vec![0x77u8; 0x800];
        slot[..4].copy_from_slice(&0x100u32.to_le_bytes());
        grown.extend_from_slice(&slot);
        let place = patcher.annex_player_file(1, &grown).unwrap();
        assert_eq!(place.sectors, 4);
        assert_eq!(patcher.annex_free_sectors().unwrap(), 6);
        let (dmy_lba, _) = find_file_in_image(patcher.image(), "DMY.DAT").unwrap();
        assert_eq!(
            place.lba,
            dmy_lba + 1,
            "first allocation skips the DMY header sector"
        );
        let entry_lba = patcher.entry_disc_lba(1).unwrap();
        assert_eq!(
            place.base,
            (place.lba - entry_lba - 16) * USER_DATA_SIZE as u32
        );

        // In place: the header, table displaced; the old records untouched.
        let head = patcher.read_entry_footprint(1).unwrap();
        let chain = legaia_asset::player_file_annex::chain(&head).unwrap();
        assert_eq!(chain.base, place.base);
        assert_eq!(
            head[PLAYER_FILE_DATA_BASE..PLAYER_FILE_DATA_BASE + 0x800],
            file[PLAYER_FILE_DATA_BASE..PLAYER_FILE_DATA_BASE + 0x800]
        );
        // Read back retail-shaped.
        let back = patcher.read_player_file(1).unwrap();
        assert_eq!(back, grown);
        let p = patcher.player_file_annex(1).unwrap().unwrap();
        assert_eq!(p, place);

        // A write inside the annexed region lands in the annex, a header
        // write in the entry, a straddling one is refused.
        patcher
            .patch_player_file(1, 0x8000 + 0x1800 + 4, &[0xEE; 4])
            .unwrap();
        let back = patcher.read_player_file(1).unwrap();
        assert_eq!(&back[0x8000 + 0x1804..0x8000 + 0x1808], &[0xEE; 4]);
        patcher.patch_player_file(1, 0x10, &[1, 2, 3, 4]).unwrap();
        assert_eq!(
            &patcher.read_entry_footprint(1).unwrap()[0x10..0x14],
            &[1, 2, 3, 4]
        );
        assert!(patcher.patch_player_file(1, 0x7FFE, &[0; 4]).is_err());
        assert!(
            patcher
                .patch_player_file(1, 0x8000 + 0x2000 - 2, &[0; 4])
                .is_err()
        );

        // Reopen: the marker persists and the next allocation follows.
        let mut again = DiscPatcher::open(patcher.into_image()).unwrap();
        assert_eq!(again.annex_free_sectors().unwrap(), 6);
        assert_eq!(again.annex_alloc(2).unwrap(), dmy_lba + 5);
        assert!(again.annex_alloc(5).is_err(), "over the room");
        assert_eq!(again.annex_free_sectors().unwrap(), 4);
    }

    #[test]
    fn annex_needs_a_dmy_dat() {
        let disc = synth_disc_with_dmy(&prot_with_player_file(&player_file()), 0);
        let mut patcher = DiscPatcher::open(disc).unwrap();
        assert!(patcher.annex_free_sectors().is_err());
        assert!(patcher.annex_alloc(1).is_err());
        assert!(
            patcher.read_player_file(1).is_ok(),
            "reading needs no annex"
        );
    }
}
