//! CD-XA voice read off a disc image on disk: the arts-shout and one-shot
//! clip banks decoded at boot, and the lazy per-channel source the director
//! stages a cast voice from.

use std::collections::BTreeMap;
use std::path::Path;

use legaia_engine_audio::AudioSink;

use super::{SceneSource, read_scus};
use crate::audio_director::{AudioDirector, XaChannelSource};
use crate::xa_banks::BATTLE_XA_CLIP_SLOTS;

/// Demux + decode the battle **arts-voice shout** banks from a disc image:
/// the per-character CD-XA clip files (`XA2.XA` Vahn / `XA4.XA` Noa /
/// `XA6.XA` Gala, 16-channel short-mono banks) plus the `SCUS_942.54`
/// cue tables that map each art's action constant to its candidate-channel
/// pool (`legaia_art::arts_voice`, the `FUN_8004C140` tables).
///
/// Channel demux needs the raw 2352-byte sectors (the CD-XA subheaders carry
/// the channel number; a 2048-byte ISO view strips them), so this reads the
/// disc through [`legaia_iso::raw::RawDisc`] - extracted-directory boots
/// can't stage a shout bank. Returns `None` when the disc / executable /
/// tables don't resolve; the caller degrades to silent arts.
///
/// The per-file staging is [`crate::xa_banks::install_shout_file`], the call
/// the browser page stages the same files through. Public so disc-gated tests
/// can build the same bank the boot path stages.
pub fn read_arts_shout_bank(disc: &Path) -> Option<legaia_engine_audio::ArtsShoutBank> {
    let scus = read_scus(&SceneSource::Disc(disc))?;
    let table = legaia_art::arts_voice::ArtsVoiceTable::parse_from_scus(&scus)?;
    let mut raw = legaia_iso::raw::RawDisc::open(disc).ok()?;
    let volume = legaia_iso::iso9660::read_volume(&mut raw).ok()?;
    let files = legaia_iso::iso9660::walk_files(&mut raw, &volume.root).ok()?;
    let mut bank = legaia_engine_audio::ArtsShoutBank::new();
    for (cslot, name) in crate::xa_banks::shout_files() {
        let rec = find_disc_file(&files, name)?;
        let sectors = rec.size.div_ceil(legaia_iso::raw::USER_DATA_SIZE as u32);
        let streams = legaia_xa::demux::demux_disc_range(&mut raw, rec.lba, sectors).ok()?;
        crate::xa_banks::install_shout_file(&mut bank, cslot, &streams, Some(&table));
    }
    bank.has_clips().then_some(bank)
}

/// The directory record of the ISO file named `name` (`XA/XA2.XA;1` matches
/// `XA2.XA`).
fn find_disc_file<'a>(
    files: &'a [(String, legaia_iso::iso9660::DirectoryRecord)],
    name: &str,
) -> Option<&'a legaia_iso::iso9660::DirectoryRecord> {
    files
        .iter()
        .find_map(|(path, rec)| (crate::xa_banks::file_key(path) == name).then_some(rec))
}

/// Demux + decode the battle **one-shot clip** banks from a disc image into
/// a generic `(clip_slot, channel)` bank: the files in
/// [`BATTLE_XA_CLIP_SLOTS`], staged through
/// [`crate::xa_banks::install_clip_file`] (the browser page stages the same
/// files through the same call). Same disc-only caveat as
/// [`read_arts_shout_bank`]. `None` when nothing decodes.
pub fn read_battle_xa_clip_bank(disc: &Path) -> Option<legaia_engine_audio::XaClipBank> {
    let mut raw = legaia_iso::raw::RawDisc::open(disc).ok()?;
    let volume = legaia_iso::iso9660::read_volume(&mut raw).ok()?;
    let files = legaia_iso::iso9660::walk_files(&mut raw, &volume.root).ok()?;
    let mut bank = legaia_engine_audio::XaClipBank::new();
    for &(slot, name) in BATTLE_XA_CLIP_SLOTS {
        let Some(rec) = find_disc_file(&files, name) else {
            continue;
        };
        let sectors = rec.size.div_ceil(legaia_iso::raw::USER_DATA_SIZE as u32);
        let Ok(streams) = legaia_xa::demux::demux_disc_range(&mut raw, rec.lba, sectors) else {
            continue;
        };
        crate::xa_banks::install_clip_file(&mut bank, slot, &streams);
    }
    bank.has_clips().then_some(bank)
}

/// The disc side of lazy CD-XA staging: the image path and every
/// `XA<n>.XA`'s `(lba, sectors)` keyed by clip slot `n - 1`.
pub struct XaDiscSource {
    disc: std::path::PathBuf,
    files: BTreeMap<u8, (u32, u32)>,
}

impl XaDiscSource {
    /// Walk the disc's ISO once for the `XA<n>.XA` files. `None` when the
    /// image does not open or carries none.
    fn open(disc: &std::path::Path) -> Option<Self> {
        let mut raw = legaia_iso::raw::RawDisc::open(disc).ok()?;
        let volume = legaia_iso::iso9660::read_volume(&mut raw).ok()?;
        let files = legaia_iso::iso9660::walk_files(&mut raw, &volume.root).ok()?;
        let mut map = BTreeMap::new();
        for (path, rec) in &files {
            let Some(slot) = crate::xa_banks::clip_slot_of(path) else {
                continue;
            };
            let sectors = rec.size.div_ceil(legaia_iso::raw::USER_DATA_SIZE as u32);
            map.insert(slot, (rec.lba, sectors));
        }
        (!map.is_empty()).then_some(Self {
            disc: disc.to_path_buf(),
            files: map,
        })
    }

    /// `count` raw 2352-byte sectors from `lba`, concatenated.
    fn read_sectors(&self, lba: u32, count: u32) -> Option<Vec<u8>> {
        let mut raw = legaia_iso::raw::RawDisc::open(&self.disc).ok()?;
        let mut out = Vec::with_capacity(
            count as usize * legaia_engine_audio::xa_clip_bank::RAW_SECTOR_BYTES,
        );
        for s in 0..count {
            let sector = raw.read_raw_sector(lba + s).ok()?;
            out.extend_from_slice(&sector[..]);
        }
        Some(out)
    }
}

impl XaChannelSource for XaDiscSource {
    fn channel_span(
        &self,
        slot: u8,
        channel: u8,
        duration_sectors: u32,
    ) -> Option<(legaia_engine_audio::XaClip, u8)> {
        let &(lba, file_sectors) = self.files.get(&slot)?;
        let span = legaia_engine_audio::xa_clip_bank::read_span_sectors(duration_sectors)
            .min(file_sectors);
        let raw = self.read_sectors(lba, span)?;
        legaia_engine_audio::xa_clip_bank::decode_channel_span(&raw, channel)
    }
}

/// Read one cast-voice clip straight off a disc image: channel `channel` of
/// `XA<clip_slot + 1>.XA`, from the file's first sector to the clip
/// starter's stop point for `duration_sectors`
/// (`legaia_engine_audio::xa_clip_bank::read_span_sectors`). This is the
/// read the director performs the first time a cast names a clip; exposed
/// so the disc-gated oracle can pin it without an audio device. `None` when
/// the disc, the file or the channel is absent.
// REF: FUN_8003D53C
pub fn read_xa_channel_span(
    disc: &std::path::Path,
    clip_slot: u8,
    channel: u8,
    duration_sectors: u32,
) -> Option<(legaia_engine_audio::XaClip, u8)> {
    XaDiscSource::open(disc)?.channel_span(clip_slot, channel, duration_sectors)
}

impl<S: AudioSink> AudioDirector<S> {
    /// Point lazy staging at the disc image: walk its ISO once for every
    /// `XA<n>.XA` and keep `(slot, lba, sectors)`, the way the boot filler
    /// `FUN_801CFA78` builds the clip table at `0x801C6ED8` (slot `n - 1`
    /// for `XA<n>.XA`; `docs/subsystems/audio.md`). Returns how many files
    /// resolved; `0` leaves lazy staging off.
    pub fn set_xa_lazy_source(&mut self, disc: &std::path::Path) -> usize {
        let Some(src) = XaDiscSource::open(disc) else {
            return 0;
        };
        let n = src.files.len();
        self.set_xa_channel_source(Box::new(src));
        n
    }
}
