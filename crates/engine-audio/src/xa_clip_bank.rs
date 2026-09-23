//! Pre-decoded **CD-XA clip bank** for the battle's one-shot voice / sting
//! cues - the clips retail streams off the disc through the clip starter
//! `FUN_8003D53C(clip_slot, channel, duration_sectors)`.
//!
//! REF: FUN_8003D53C - the starter this stands in for. Retail's version is a
//! `CdlSetloc` + `CdlSetfilter{file 1, chan}` + `CdlReadS` state machine
//! over the physical disc, stopping at `start_lba + (dur*150+149)/60`
//! (`legaia_engine_shell::xa_clip::clip_end_lba_offset`). The engine has no
//! streaming drive: the host demuxes the clip files it needs at boot and
//! decodes every channel to PCM, and a cue becomes "mix this PCM now" -
//! [`XaClipBank::clip`] plus [`XaClipBank::cut_frames`] for the retail stop
//! point.
//!
//! The bank is keyed on the **clip slot** (`XA<slot + 1>.XA`, the boot-built
//! table at `0x801C6ED8` - `docs/subsystems/audio.md`) and the CD-XA channel
//! inside that file's interleave. Which slots a host stages is its own
//! decision; the battle needs `26` (`XA27`, the eight stereo attack stings
//! the melee kernel's `0x10C` cue lands on) and `0x1D` (`XA30`, the ten mono
//! per-character grunts).
//!
//! Sibling of [`crate::shout::ArtsShoutBank`], which keys the arts-voice
//! banks per character + action constant because its retail selector
//! (`FUN_8004C140`) does; this one is the raw `(slot, channel)` space the
//! generic starter takes.

use std::collections::BTreeMap;

/// One decoded XA channel.
#[derive(Debug, Clone, Default)]
pub struct XaClip {
    /// Decoded PCM - interleaved L/R frames when `stereo`, else mono.
    pub pcm: Vec<i16>,
    /// Source sample rate (37 800 Hz for the battle banks).
    pub sample_rate: u32,
    pub stereo: bool,
}

impl XaClip {
    /// Frames (samples per channel) in the clip.
    pub fn frames(&self) -> usize {
        if self.stereo {
            self.pcm.len() / 2
        } else {
            self.pcm.len()
        }
    }
}

/// `(clip_slot, channel) -> clip`, plus each slot's channel count (the
/// interleave the retail stop point has to be divided by).
///
/// Two staging tiers share the map. Boot-staged clips (the battle's `XA27`
/// / `XA30`) are pinned. **Lazily** staged clips - the cast voices, one
/// channel span decoded the first time a cast names it
/// ([`XaClipBank::insert_lazy`]) - live under [`LAZY_CLIP_CAP`], oldest out
/// first, so a player who works through the whole band never holds more
/// than a few decoded clips at once (a 55 s mono clip at 37.8 kHz is
/// 4 MB of PCM).
#[derive(Debug, Clone, Default)]
pub struct XaClipBank {
    clips: BTreeMap<(u8, u8), XaClip>,
    channels_per_slot: BTreeMap<u8, u8>,
    /// Lazily staged keys, oldest first.
    lazy_order: std::collections::VecDeque<(u8, u8)>,
}

/// Lazily staged clips kept resident at once. Six is one summon bed plus
/// the spells a party cycles through in a fight; the seventh evicts the
/// oldest, which a later cast simply re-decodes.
pub const LAZY_CLIP_CAP: usize = 6;

/// The clip starter's read span in **physical sectors** from the file's
/// start: `(dur * 150 + 149) / 60` (`FUN_8003D53C` `0x8003D698..0x8003D6D8`,
/// the `0x88888889` reciprocal). `dur` is the vsync span the dispatcher
/// hands it; 150 sectors per second over 60 vsyncs per second.
// REF: FUN_8003D53C
pub fn read_span_sectors(duration_sectors: u32) -> u32 {
    ((u64::from(duration_sectors) * 150 + 149) / 60) as u32
}

/// One raw Mode 2 sector as a disc image or a page's disc bytes carry it.
pub const RAW_SECTOR_BYTES: usize = 2352;

/// Demux + decode **one** CD-XA channel out of a run of raw 2352-byte
/// sectors - the sectors the starter would read for a cue, from the file's
/// first sector to its stop point. Returns the decoded clip and the file's
/// interleave width (`widest channel seen + 1`, so the stop point can be
/// divided by it even when other channels were not decoded). `None` when
/// the channel carries no Form 2 audio sector in the run, or is not 4-bit.
///
/// Both hosts' lazy staging goes through here: the native window reads
/// the sectors off the disc image, the play page slices them out of the
/// bytes it holds; neither keeps the raw sectors afterwards.
pub fn decode_channel_span(raw_sectors: &[u8], channel: u8) -> Option<(XaClip, u8)> {
    use legaia_xa::demux::{
        AUDIO_BYTES_PER_SECTOR, SUBHEADER_OFFSET, USER_DATA_OFFSET, parse_subheader,
    };
    let mut widest = 0u8;
    let mut audio = Vec::new();
    let mut coding: Option<(u32, bool, u8)> = None;
    for raw in raw_sectors.as_chunks::<RAW_SECTOR_BYTES>().0 {
        let mut sub_bytes = [0u8; 8];
        sub_bytes.copy_from_slice(&raw[SUBHEADER_OFFSET..SUBHEADER_OFFSET + 8]);
        let (sub, ok) = parse_subheader(&sub_bytes);
        if !ok || !sub.is_audio() || !sub.is_form2() {
            continue;
        }
        widest = widest.max(sub.ch_no);
        if sub.ch_no != channel {
            continue;
        }
        coding.get_or_insert((sub.sample_rate(), sub.is_stereo(), sub.bits_per_sample()));
        audio.extend_from_slice(&raw[USER_DATA_OFFSET..USER_DATA_OFFSET + AUDIO_BYTES_PER_SECTOR]);
    }
    let (sample_rate, stereo, bits) = coding?;
    if bits != 4 || audio.is_empty() {
        return None;
    }
    let channels = if stereo {
        legaia_xa::Channels::Stereo
    } else {
        legaia_xa::Channels::Mono
    };
    let (pcm, _) = legaia_xa::decode(
        &audio,
        legaia_xa::DecodeOptions {
            channels,
            sample_rate,
            bits: legaia_xa::BitsPerSample::Four,
        },
    )
    .ok()?;
    if pcm.is_empty() {
        return None;
    }
    Some((
        XaClip {
            pcm,
            sample_rate,
            stereo,
        },
        widest.saturating_add(1),
    ))
}

/// CD-XA audio frames one 2352-byte sector carries at 37.8 kHz: 18 sound
/// groups x 128 bytes = 18 x 224 4-bit samples = 4032 mono samples, or
/// 2016 stereo frames (`docs/formats/xa.md`).
pub const MONO_FRAMES_PER_SECTOR: usize = 4032;
pub const STEREO_FRAMES_PER_SECTOR: usize = 2016;

impl XaClipBank {
    pub fn new() -> Self {
        Self::default()
    }

    /// `true` once at least one clip is staged.
    pub fn has_clips(&self) -> bool {
        !self.clips.is_empty()
    }

    /// Stage one decoded channel of `XA<slot + 1>.XA`.
    pub fn insert(&mut self, slot: u8, channel: u8, clip: XaClip) {
        self.clips.insert((slot, channel), clip);
        let n = self.channels_per_slot.entry(slot).or_insert(0);
        *n = (*n).max(channel.saturating_add(1));
    }

    /// Record a slot's interleave width when the demux saw more channels
    /// than were decoded (an 8-bit or stereo channel skipped, say).
    pub fn set_channel_count(&mut self, slot: u8, channels: u8) {
        let n = self.channels_per_slot.entry(slot).or_insert(0);
        *n = (*n).max(channels);
    }

    /// Channels interleaved in `slot`'s file (1 when unknown).
    pub fn channel_count(&self, slot: u8) -> u8 {
        self.channels_per_slot
            .get(&slot)
            .copied()
            .unwrap_or(1)
            .max(1)
    }

    pub fn clip(&self, slot: u8, channel: u8) -> Option<&XaClip> {
        self.clips.get(&(slot, channel))
    }

    /// `true` when `(slot, channel)` is decoded and resident.
    pub fn is_staged(&self, slot: u8, channel: u8) -> bool {
        self.clips.contains_key(&(slot, channel))
    }

    /// Stage one **lazily** decoded channel (a cast voice), evicting the
    /// oldest lazy clip once more than [`LAZY_CLIP_CAP`] are resident. A
    /// boot-staged clip is never evicted; re-staging a lazy key refreshes
    /// its age.
    pub fn insert_lazy(&mut self, slot: u8, channel: u8, clip: XaClip, channels: u8) {
        self.lazy_order.retain(|k| *k != (slot, channel));
        self.lazy_order.push_back((slot, channel));
        self.insert(slot, channel, clip);
        self.set_channel_count(slot, channels);
        while self.lazy_order.len() > LAZY_CLIP_CAP {
            if let Some(old) = self.lazy_order.pop_front() {
                self.clips.remove(&old);
            }
        }
    }

    /// Lazily staged clips currently resident.
    pub fn lazy_count(&self) -> usize {
        self.lazy_order.len()
    }

    /// Frames of `(slot, channel)` the retail read span covers, capped at
    /// the clip length.
    ///
    /// The starter stops the drive `(dur*150+149)/60` physical sectors past
    /// the file start (`FUN_8003D53C` `0x8003D698..0x8003D6D8`); a channel
    /// owns one sector in every `channel_count`, and each of its sectors
    /// carries [`MONO_FRAMES_PER_SECTOR`] / [`STEREO_FRAMES_PER_SECTOR`]
    /// frames. `None` when the clip is not staged.
    pub fn cut_frames(&self, slot: u8, channel: u8, duration_sectors: u32) -> Option<usize> {
        let clip = self.clip(slot, channel)?;
        let phys = u64::from(read_span_sectors(duration_sectors));
        let own = phys / u64::from(self.channel_count(slot));
        let per_sector = if clip.stereo {
            STEREO_FRAMES_PER_SECTOR
        } else {
            MONO_FRAMES_PER_SECTOR
        } as u64;
        Some((own * per_sector).min(clip.frames() as u64) as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bank() -> XaClipBank {
        let mut b = XaClipBank::new();
        // A 10-channel mono bank: channel 0 holds 16 sectors' worth.
        b.insert(
            0x1D,
            0,
            XaClip {
                pcm: vec![1; 16 * MONO_FRAMES_PER_SECTOR],
                sample_rate: 37_800,
                stereo: false,
            },
        );
        b.set_channel_count(0x1D, 10);
        // An 8-channel stereo bank: channel 4 holds 70 sectors' worth.
        b.insert(
            26,
            4,
            XaClip {
                pcm: vec![1; 70 * STEREO_FRAMES_PER_SECTOR * 2],
                sample_rate: 37_800,
                stereo: true,
            },
        );
        b.set_channel_count(26, 8);
        b
    }

    #[test]
    fn cut_divides_the_physical_span_by_the_interleave() {
        let b = bank();
        // Vahn's grunt: dur 0x26 -> 95 physical sectors -> 9 of the
        // channel's own -> 9 * 4032 mono frames.
        assert_eq!(
            b.cut_frames(0x1D, 0, 0x26),
            Some(9 * MONO_FRAMES_PER_SECTOR)
        );
        // The melee sting: dur 224 -> 562 physical -> 70 own sectors =
        // the whole 70-sector clip.
        assert_eq!(
            b.cut_frames(26, 4, 224),
            Some(70 * STEREO_FRAMES_PER_SECTOR)
        );
        // A span past the clip's end is capped.
        assert_eq!(
            b.cut_frames(26, 4, 10_000),
            Some(70 * STEREO_FRAMES_PER_SECTOR)
        );
        assert_eq!(b.cut_frames(26, 5, 224), None);
    }

    #[test]
    fn channel_count_tracks_the_widest_channel_seen() {
        let mut b = XaClipBank::new();
        assert_eq!(b.channel_count(3), 1);
        b.insert(3, 5, XaClip::default());
        assert_eq!(b.channel_count(3), 6);
        b.set_channel_count(3, 4);
        assert_eq!(b.channel_count(3), 6);
        b.set_channel_count(3, 8);
        assert_eq!(b.channel_count(3), 8);
    }

    /// The starter's stop point, in physical sectors from the file start.
    #[test]
    fn read_span_is_the_starters_ceiling_scale_by_two_and_a_half() {
        assert_eq!(read_span_sectors(0), 2); // 149 / 60
        assert_eq!(read_span_sectors(60), 152);
        // The two captured casts: 686 vsyncs -> 1717 sectors, 568 -> 1422.
        assert_eq!(read_span_sectors(686), (686 * 150 + 149) / 60);
        assert_eq!(read_span_sectors(568), (568 * 150 + 149) / 60);
    }

    /// Lazy clips live under the cap, oldest out; boot-staged clips stay.
    #[test]
    fn lazy_clips_evict_oldest_past_the_cap_and_never_the_pinned() {
        let mut b = bank();
        for ch in 0..=LAZY_CLIP_CAP as u8 {
            b.insert_lazy(6, ch, XaClip::default(), 8);
        }
        assert_eq!(b.lazy_count(), LAZY_CLIP_CAP);
        assert!(!b.is_staged(6, 0), "the oldest lazy clip is gone");
        assert!(b.is_staged(6, LAZY_CLIP_CAP as u8));
        assert!(b.is_staged(0x1D, 0), "a boot-staged clip is pinned");
        assert!(b.is_staged(26, 4));
        assert_eq!(b.channel_count(6), 8);
        // Re-staging refreshes the age rather than duplicating the key.
        b.insert_lazy(6, 1, XaClip::default(), 8);
        assert_eq!(b.lazy_count(), LAZY_CLIP_CAP);
        b.insert_lazy(6, 9, XaClip::default(), 10);
        assert!(b.is_staged(6, 1), "refreshed, so not the oldest any more");
        assert!(!b.is_staged(6, 2));
        assert_eq!(b.channel_count(6), 10);
    }

    /// One synthetic raw Mode 2 Form 2 audio sector for `(file, ch)`.
    fn sector(ch_no: u8, coding: u8, fill: u8) -> Vec<u8> {
        use legaia_xa::demux::{AUDIO_BYTES_PER_SECTOR, SUBHEADER_OFFSET, USER_DATA_OFFSET};
        let mut s = vec![0u8; RAW_SECTOR_BYTES];
        // submode: audio (0x04) | form 2 (0x20) | real-time (0x40).
        let sub = [1, ch_no, 0x64, coding];
        s[SUBHEADER_OFFSET..SUBHEADER_OFFSET + 4].copy_from_slice(&sub);
        s[SUBHEADER_OFFSET + 4..SUBHEADER_OFFSET + 8].copy_from_slice(&sub);
        for b in &mut s[USER_DATA_OFFSET..USER_DATA_OFFSET + AUDIO_BYTES_PER_SECTOR] {
            *b = fill;
        }
        s
    }

    /// The span decoder keeps one channel, reports the interleave from every
    /// channel it passed over, and honours the subheader's coding.
    #[test]
    fn channel_span_decodes_one_channel_and_reports_the_interleave() {
        let mut run = Vec::new();
        run.extend(sector(0, 0x00, 0x11)); // 37.8 kHz mono
        run.extend(sector(4, 0x00, 0x22));
        run.extend(sector(6, 0x01, 0x33)); // stereo, another channel
        run.extend(sector(4, 0x00, 0x22));
        run.extend([0u8; 100]); // trailing partial sector ignored
        let (clip, width) = decode_channel_span(&run, 4).expect("channel 4 decodes");
        assert_eq!(width, 7, "widest channel seen was 6");
        assert!(!clip.stereo);
        assert_eq!(clip.sample_rate, 37_800);
        assert_eq!(clip.pcm.len(), 2 * MONO_FRAMES_PER_SECTOR);
        let (stereo, _) = decode_channel_span(&run, 6).expect("channel 6 decodes");
        assert!(stereo.stereo);
        assert_eq!(stereo.pcm.len(), 2 * STEREO_FRAMES_PER_SECTOR);
        assert!(decode_channel_span(&run, 5).is_none(), "no such channel");
        // An 8-bit channel is refused: the decoder is 4-bit only.
        let mut eight = sector(2, 0x10, 0x44);
        eight[legaia_xa::demux::SUBHEADER_OFFSET + 3] = 0x10;
        eight[legaia_xa::demux::SUBHEADER_OFFSET + 7] = 0x10;
        assert!(decode_channel_span(&eight, 2).is_none());
    }
}
