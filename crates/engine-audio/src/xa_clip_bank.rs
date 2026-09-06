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
#[derive(Debug, Clone, Default)]
pub struct XaClipBank {
    clips: BTreeMap<(u8, u8), XaClip>,
    channels_per_slot: BTreeMap<u8, u8>,
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
        let phys = (u64::from(duration_sectors) * 150 + 149) / 60;
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
}
