//! The CD-XA voice banks both play hosts stage: which files, and how a
//! file's demuxed channels become bank entries.
//!
//! Two banks ride the XA lane - the Tactical-Arts **shouts**
//! ([`ArtsShoutBank`], `XA2` / `XA4` / `XA6`, keyed by character slot and
//! picked through the `SCUS_942.54` cue pools of `FUN_8004C140`) and the
//! battle's **one-shot clips** ([`XaClipBank`], keyed by the raw
//! `(clip_slot, channel)` of the retail starter `FUN_8003D53C`). The native
//! boot reads the files' raw sectors off the disc image
//! ([`crate::boot::read_arts_shout_bank`] /
//! [`crate::boot::read_battle_xa_clip_bank`]); the browser play page slices
//! the same sectors out of the disc bytes it holds. Where the sectors come
//! from is the hosts' business. What a channel becomes - which channels a
//! file stages, how a shout is trimmed, which cue pools it carries, how wide
//! a clip file's interleave is - is this module's, so both hosts stage the
//! same banks from the same bytes.
//!
//! REF: FUN_8004C140 (arts-voice cue selector), FUN_8003D53C (CD-XA clip
//! starter both banks stand in for).

use legaia_art::arts_voice::{ArtsVoiceTable, clip_file};
use legaia_engine_audio::{ArtsShoutBank, ShoutClip, XaClip, XaClipBank};
use legaia_xa::demux::ChannelStream;

/// Clip slots the battle's one-shot CD-XA cues address, as `(slot, file)`.
/// The animation cue tracks' party voice band (`0xC8..=0xFF` re-based
/// `+0x38`, `FUN_800508DC` -> `FUN_8004FE5C`) lands on `(id - 0x100) >> 3`
/// with the `1 / 3 / 5 -> 26 / 27 / 28` remap: Vahn's `0xC8..=0xD7` on
/// slots `0` / `26`, Noa's `0xD8..=0xE7` on `2` / `27`, Gala's
/// `0xE8..=0xF7` on `4` / `28` - Vahn's Spirit clip opens with `0xC8`,
/// `XA1.XA` channel 0. `26` also carries the melee kernel's `0x10C` sting
/// and `0x1D` = `XA30.XA` the per-character block grunt. Slot `i` is
/// `XA<i+1>.XA` by the boot-built clip table's own construction
/// (`docs/subsystems/audio.md`).
pub const BATTLE_XA_CLIP_SLOTS: &[(u8, &str)] = &[
    (0, "XA1.XA"),
    (2, "XA3.XA"),
    (4, "XA5.XA"),
    (26, "XA27.XA"),
    (27, "XA28.XA"),
    (28, "XA29.XA"),
    (0x1D, "XA30.XA"),
];

/// Trailing samples under this magnitude are channel-padding silence, trimmed
/// off a shout so its audible end matches the retail read-span cutoff closely
/// enough for the back-to-back promotion queue.
pub const SHOUT_TAIL_SILENCE: u16 = 8;

/// `(character slot, clip file)` for the three voiced characters
/// (`legaia_art::arts_voice::clip_file`).
pub fn shout_files() -> impl Iterator<Item = (u8, &'static str)> {
    (0u8..3).filter_map(|c| clip_file(c as usize).map(|f| (c, f)))
}

/// Every XA file the two banks stage, shouts first then clips.
pub fn wanted_files() -> Vec<&'static str> {
    shout_files()
        .map(|(_, f)| f)
        .chain(BATTLE_XA_CLIP_SLOTS.iter().map(|(_, f)| *f))
        .collect()
}

/// The bare upper-case file name a path names (`XA/XA2.XA;1` -> `XA2.XA`) -
/// the key both tables above are spelled in.
pub fn file_key(path: &str) -> String {
    let base = path.rsplit(['/', '\\']).next().unwrap_or(path);
    let base = base.split(';').next().unwrap_or(base);
    base.trim().to_ascii_uppercase()
}

/// Which bank a file stages into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XaBankFile {
    /// A shout file, for this character slot.
    Shout(u8),
    /// A one-shot clip file, for this clip slot.
    Clip(u8),
}

/// The bank `path` stages into, or `None` for a file neither bank wants.
pub fn bank_file(path: &str) -> Option<XaBankFile> {
    let key = file_key(path);
    if let Some((cslot, _)) = shout_files().find(|(_, f)| *f == key) {
        return Some(XaBankFile::Shout(cslot));
    }
    BATTLE_XA_CLIP_SLOTS
        .iter()
        .find(|(_, f)| *f == key)
        .map(|&(slot, _)| XaBankFile::Clip(slot))
}

/// Trim the trailing channel-padding silence off a decoded shout.
pub fn trim_trailing_silence(pcm: &mut Vec<i16>) {
    let mut end = pcm.len();
    while end > 0 && pcm[end - 1].unsigned_abs() < SHOUT_TAIL_SILENCE {
        end -= 1;
    }
    pcm.truncate(end);
}

/// Decode one demuxed 4-bit channel to PCM at its subheader rate. `None` for
/// an 8-bit channel (the decoder is 4-bit only) or a decode failure.
fn decode_stream(s: &ChannelStream) -> Option<Vec<i16>> {
    if s.bits_per_sample != 4 {
        return None;
    }
    let channels = if s.stereo {
        legaia_xa::Channels::Stereo
    } else {
        legaia_xa::Channels::Mono
    };
    let (pcm, _) = legaia_xa::decode(
        &s.audio,
        legaia_xa::DecodeOptions {
            channels,
            sample_rate: s.sample_rate,
            bits: legaia_xa::BitsPerSample::Four,
        },
    )
    .ok()?;
    Some(pcm)
}

/// One character's shout clips from its file's demuxed channels: every 4-bit
/// **mono** channel (a stereo or 8-bit stream here would be a mis-identified
/// file), decoded and tail-trimmed, keyed by channel. Empty channels drop.
pub fn shout_clips(streams: &[ChannelStream]) -> Vec<(u8, ShoutClip)> {
    streams
        .iter()
        .filter(|s| !s.stereo)
        .filter_map(|s| {
            let mut pcm = decode_stream(s)?;
            trim_trailing_silence(&mut pcm);
            (!pcm.is_empty()).then_some((
                s.ch_no,
                ShoutClip {
                    pcm,
                    sample_rate: s.sample_rate,
                },
            ))
        })
        .collect()
}

/// One clip slot's channels from its file's demuxed channels: every 4-bit
/// channel, mono or stereo at its subheader rate, untrimmed (the retail read
/// span is cut against the full channel), plus the interleave width
/// (`widest channel + 1`, counting a channel that did not decode, so the read
/// span divides by the file's real interleave).
pub fn clip_channels(streams: &[ChannelStream]) -> (u8, Vec<(u8, XaClip)>) {
    let widest = streams.iter().map(|s| s.ch_no).max().unwrap_or(0);
    let clips = streams
        .iter()
        .filter_map(|s| {
            let pcm = decode_stream(s)?;
            (!pcm.is_empty()).then_some((
                s.ch_no,
                XaClip {
                    pcm,
                    sample_rate: s.sample_rate,
                    stereo: s.stereo,
                },
            ))
        })
        .collect();
    (widest.saturating_add(1), clips)
}

/// Stage character `cslot`'s shout file into `bank`: its clips, then its
/// cue pools from `table` (none without the executable - the clips stage,
/// but no art can pick one). Returns the channels staged.
pub fn install_shout_file(
    bank: &mut ArtsShoutBank,
    cslot: u8,
    streams: &[ChannelStream],
    table: Option<&ArtsVoiceTable>,
) -> u32 {
    let clips = shout_clips(streams);
    let n = clips.len() as u32;
    for (ch, clip) in clips {
        bank.insert_clip(cslot, ch, clip);
    }
    if n > 0
        && let Some(table) = table
    {
        for (action, pool) in table.pools(cslot as usize) {
            bank.set_pool(cslot, action, pool.to_vec());
        }
    }
    n
}

/// Stage clip slot `slot`'s file into `bank`: its interleave width and every
/// channel that decoded. Returns the channels staged.
pub fn install_clip_file(bank: &mut XaClipBank, slot: u8, streams: &[ChannelStream]) -> u32 {
    let (width, clips) = clip_channels(streams);
    if clips.is_empty() {
        return 0;
    }
    bank.set_channel_count(slot, width);
    let n = clips.len() as u32;
    for (ch, clip) in clips {
        bank.insert(slot, ch, clip);
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_xa::demux::{
        AUDIO_BYTES_PER_SECTOR, RAW_SECTOR_BYTES, SUBHEADER_OFFSET, USER_DATA_OFFSET,
        demux_raw_sectors,
    };

    /// One synthetic raw Mode 2 Form 2 audio sector for `(file, ch)` with
    /// the given coding byte and a constant audio fill.
    fn sector(file_no: u8, ch_no: u8, coding: u8, fill: u8) -> Vec<u8> {
        let mut s = vec![0u8; RAW_SECTOR_BYTES];
        // submode: audio (0x04) | form 2 (0x20) | real-time (0x40).
        let sub = [file_no, ch_no, 0x64, coding];
        s[SUBHEADER_OFFSET..SUBHEADER_OFFSET + 4].copy_from_slice(&sub);
        s[SUBHEADER_OFFSET + 4..SUBHEADER_OFFSET + 8].copy_from_slice(&sub);
        for b in &mut s[USER_DATA_OFFSET..USER_DATA_OFFSET + AUDIO_BYTES_PER_SECTOR] {
            *b = fill;
        }
        s
    }

    /// The staging set, in order, and every clip slot names `XA<slot+1>.XA`
    /// - the clip table's own construction.
    #[test]
    fn wanted_files_are_the_staging_set() {
        assert_eq!(
            wanted_files(),
            vec![
                "XA2.XA", "XA4.XA", "XA6.XA", "XA1.XA", "XA3.XA", "XA5.XA", "XA27.XA", "XA28.XA",
                "XA29.XA", "XA30.XA"
            ]
        );
        for &(slot, file) in BATTLE_XA_CLIP_SLOTS {
            assert_eq!(file, format!("XA{}.XA", u32::from(slot) + 1));
        }
        assert_eq!(bank_file("XA/XA4.XA;1"), Some(XaBankFile::Shout(1)));
        assert_eq!(bank_file("xa30.xa"), Some(XaBankFile::Clip(0x1D)));
        assert_eq!(bank_file("XA/XA7.XA;1"), None);
    }

    /// The in-memory demux groups sectors per channel, honours the
    /// subheader's coding parameters, and skips what is not Form 2 audio or
    /// whose redundant subheader copy disagrees.
    #[test]
    fn demux_groups_per_channel_and_skips_non_audio() {
        let mut run = Vec::new();
        run.extend(sector(1, 0, 0x00, 0x11)); // 37.8 kHz mono 4-bit
        run.extend(sector(1, 3, 0x05, 0x22)); // 18.9 kHz stereo 4-bit
        run.extend(sector(1, 0, 0x00, 0x33)); // second sector of channel 0
        // Not audio: submode without the audio bit.
        let mut data = sector(1, 5, 0x00, 0x44);
        data[SUBHEADER_OFFSET + 2] = 0x08;
        data[SUBHEADER_OFFSET + 6] = 0x08;
        run.extend(data);
        // Redundant copy mismatch.
        let mut bad = sector(1, 6, 0x00, 0x55);
        bad[SUBHEADER_OFFSET + 5] = 7;
        run.extend(bad);
        // Trailing partial sector is ignored.
        run.extend([0u8; 100]);

        let streams = demux_raw_sectors(&run);
        assert_eq!(streams.len(), 2);
        let ch0 = streams.iter().find(|s| s.ch_no == 0).unwrap();
        assert_eq!(ch0.audio.len(), 2 * AUDIO_BYTES_PER_SECTOR);
        assert_eq!(ch0.sample_rate, 37_800);
        assert!(!ch0.stereo);
        assert_eq!(ch0.audio[0], 0x11);
        assert_eq!(ch0.audio[AUDIO_BYTES_PER_SECTOR], 0x33);
        let ch3 = streams.iter().find(|s| s.ch_no == 3).unwrap();
        assert_eq!(ch3.sample_rate, 18_900);
        assert!(ch3.stereo);
        assert_eq!(ch3.bits_per_sample, 4);
    }

    /// A shout channel that decodes to silence is dropped rather than staged
    /// as an empty clip; a stereo channel is not a shout.
    #[test]
    fn silent_and_stereo_channels_never_become_shouts() {
        let mut run = Vec::new();
        run.extend(sector(1, 0, 0x00, 0x00)); // all-zero groups -> silence
        run.extend(sector(1, 1, 0x01, 0x00)); // stereo
        assert!(shout_clips(&demux_raw_sectors(&run)).is_empty());
        let mut pcm = vec![100, 3, -2, 0, 0];
        trim_trailing_silence(&mut pcm);
        assert_eq!(pcm, vec![100]);
    }

    /// The clip builder records the interleave width from the widest channel
    /// seen, and - unlike the shout builder - keeps a silent clip, so the
    /// retail read-span cut stays measured against the full channel length.
    #[test]
    fn clip_builder_reports_interleave_width_and_keeps_silent_clips() {
        let mut run = Vec::new();
        run.extend(sector(1, 0, 0x00, 0x00));
        run.extend(sector(1, 9, 0x00, 0x00));
        let (width, clips) = clip_channels(&demux_raw_sectors(&run));
        assert_eq!(width, 10);
        assert_eq!(clips.len(), 2);
        assert!(clips.iter().all(|(_, c)| !c.pcm.is_empty() && !c.stereo));
        // An 8-bit channel is refused (the decoder is 4-bit only) but still
        // widens the interleave.
        run.extend(sector(1, 12, 0x10, 0x00));
        let (width, clips) = clip_channels(&demux_raw_sectors(&run));
        assert_eq!(width, 13);
        assert_eq!(clips.len(), 2);
    }

    /// Paths arrive in whatever shape the host has them.
    #[test]
    fn file_keys_normalise_paths() {
        assert_eq!(file_key("XA/XA2.XA;1"), "XA2.XA");
        assert_eq!(file_key("xa27.xa"), "XA27.XA");
        assert_eq!(file_key("/XA30.XA"), "XA30.XA");
    }
}
