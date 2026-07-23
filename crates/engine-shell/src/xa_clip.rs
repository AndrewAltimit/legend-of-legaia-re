//! XA voice-clip dispatch arithmetic - the pure computations behind the
//! "XA channel selection" mechanism in
//! [`docs/subsystems/cutscene.md`](../../../docs/subsystems/cutscene.md#xa-channel-selection).
//!
//! The `XA*.XA` clips (voice banks + streamed music) play through the SCUS clip
//! starter `FUN_8003D53C(clip_slot, channel, duration_sectors)`. The menu voice
//! dispatcher `FUN_8004FCC8` derives those three arguments from a single cue id;
//! this module ports that derivation and the starter's end-LBA arithmetic. The
//! surrounding routines are libcd `CdlSetfilter` / `CdControl` state machines
//! (hardware I/O, not portable), but the id-to-(slot, channel, length) mapping
//! and the sector-count conversions are pure integer computation - and the
//! per-cue slot/channel are exactly what the cutscene-audio census needs.
//!
//! All arithmetic is reproduced from the disassembly, not the decompiled C.
//!
//! # NOT WIRED
//!
//! Two prerequisites are missing before anything can consume a
//! `(clip_slot, channel, duration_sectors)` triple.
//!
//! **A drive model.** The clip starter is a `CdlSetfilter` / `CdlReadS` state
//! machine over the physical disc, playing sectors out of `XA<n>.XA` in real
//! time through the SPU's CD input. The engine has no streaming CD device: its
//! two XA consumers both read whole files up front - `play-str` demuxes the
//! single track interleaved inside an `MV*.STR`, and the arts-shout bank
//! pre-decodes `XA2`/`XA4`/`XA6.XA` into memory at boot (`read_arts_shout_bank`
//! in `crate::boot`). Neither needs a slot, a filter channel or a sector
//! duration.
//!
//! **A producer for this cue-id space.** Every cue the world raises today
//! comes from the static SFX descriptor table (ids below [`XA_CUE_BASE`]) or
//! from a move-power record, so no id ever reaches the voice arm.
//! `legaia_engine_audio::classify_cue` ports the same dispatcher's *routing*
//! decision and is on the frame path, but the hosts log its `Voice` result
//! rather than playing it - see `window/event_handler/redraw.rs`. Wiring this
//! module means giving the engine a streamed-voice output first; the two
//! `FUN_8003D53C` helpers below then have a caller with a real start LBA.

/// A cue id at or above this value addresses an XA clip; below it the dispatcher
/// takes a different (SFX-queue) path (`sltiu v0,s0,0x100` at `8004fcd4`).
pub const XA_CUE_BASE: u32 = 0x100;

/// Largest `duration_sectors` the clip starter accepts before clamping
/// (`slti v0,s0,0x2a31` at `8003d5c8`; `0x2a30 < param_3` clamps).
pub const MAX_CLIP_DURATION_SECTORS: u32 = 0x2A30;

/// The three drive arguments `FUN_8004FCC8` builds for `FUN_8003D53C`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct XaClipCue {
    /// Clip slot - selects `XA<slot + 1>.XA` in the runtime clip table at
    /// `0x801C6ED8`.
    pub clip_slot: u32,
    /// Sector filter channel (`CdlSetfilter{file: 1, chan}`).
    pub channel: u32,
}

/// Derive the clip slot for an XA cue id (`8004fd04`..`8004fd34`).
///
/// The base slot is `(id - 0x100) >> 3`; three low slots are then remapped onto
/// the high music/streamed banks: `1 -> 0x1A`, `3 -> 0x1B`, `5 -> 0x1C`. The
/// remaps are applied in sequence off a single computed value, so only one can
/// ever match.
// PORT: FUN_8004fcc8
pub fn voice_clip_slot(id: u32) -> u32 {
    let slot = (id - XA_CUE_BASE) >> 3;
    match slot {
        1 => 0x1A,
        3 => 0x1B,
        5 => 0x1C,
        other => other,
    }
}

/// Derive the sector-filter channel for an XA cue id
/// (`andi a1,a1,0x7` at `8004fd60`).
// PORT: FUN_8004fcc8
pub fn voice_clip_channel(id: u32) -> u32 {
    (id - XA_CUE_BASE) & 7
}

/// The full `(clip_slot, channel)` pair for a cue id at or above
/// [`XA_CUE_BASE`].
// PORT: FUN_8004fcc8
pub fn voice_clip_cue(id: u32) -> XaClipCue {
    XaClipCue {
        clip_slot: voice_clip_slot(id),
        channel: voice_clip_channel(id),
    }
}

/// Convert a clip's raw length field to `duration_sectors`, the third argument
/// to the clip starter (`8004fd4c`..`8004fd78`).
///
/// `(len * 60 + 99) / 100` - a ceiling scale by `3/5` (written here as the
/// equivalent `div_ceil`, since the `+99` bias is exactly `100 - 1`). `len` is
/// the per-clip value the dispatcher reads from the length table at
/// `DAT_800788B8`.
// PORT: FUN_8004fcc8
pub fn voice_clip_duration_sectors(len_field: u16) -> u32 {
    (len_field as u32 * 60).div_ceil(100)
}

/// The clip starter's end-LBA offset from the clip's start sector
/// (`8003d5c8` clamp, `8003d698`..`8003d6d8`).
///
/// `duration_sectors` is clamped to [`MAX_CLIP_DURATION_SECTORS`] first, then the
/// offset is `(duration * 150 + 149) / 60` - a ceiling scale by `2.5` (150 CD
/// sectors per second over the 60-unit duration granularity). The drive polls
/// `CdlGetlocP` against `start_lba + offset` to stop the clip.
// PORT: FUN_8003d53c
pub fn clip_end_lba_offset(duration_sectors: u32) -> u32 {
    let d = duration_sectors.min(MAX_CLIP_DURATION_SECTORS);
    (d * 150 + 149) / 60
}

/// Whether a `duration_sectors` value would be clamped by the clip starter
/// (which also latches `_DAT_8007B828 = 0xD431` when it clamps).
// PORT: FUN_8003d53c
pub fn clip_duration_is_clamped(duration_sectors: u32) -> bool {
    duration_sectors > MAX_CLIP_DURATION_SECTORS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clip_slot_shifts_by_eight_then_remaps() {
        // id - 0x100, >> 3: eight consecutive ids share a slot.
        assert_eq!(voice_clip_slot(0x100), 0); // idx 0 -> slot 0
        assert_eq!(voice_clip_slot(0x107), 0);
        assert_eq!(voice_clip_slot(0x110), 2); // idx 0x10 -> slot 2
        // The three remapped slots point at the streamed/music banks.
        assert_eq!(voice_clip_slot(0x108), 0x1A); // idx 8 -> slot 1 -> 0x1A
        assert_eq!(voice_clip_slot(0x118), 0x1B); // idx 0x18 -> slot 3 -> 0x1B
        assert_eq!(voice_clip_slot(0x128), 0x1C); // idx 0x28 -> slot 5 -> 0x1C
        // Slots 2 and 4 are not remapped.
        assert_eq!(voice_clip_slot(0x120), 4);
    }

    #[test]
    fn clip_channel_is_low_three_bits() {
        assert_eq!(voice_clip_channel(0x100), 0);
        assert_eq!(voice_clip_channel(0x105), 5);
        assert_eq!(voice_clip_channel(0x10F), 7);
        assert_eq!(voice_clip_channel(0x117), 7);
    }

    #[test]
    fn cue_pairs_slot_and_channel() {
        // id 0x108: idx 8 -> slot 1 (remapped 0x1A), channel 0.
        assert_eq!(
            voice_clip_cue(0x108),
            XaClipCue {
                clip_slot: 0x1A,
                channel: 0
            }
        );
        // id 0x11D: idx 0x1D -> slot 3 (0x1B), channel 5.
        assert_eq!(
            voice_clip_cue(0x11D),
            XaClipCue {
                clip_slot: 0x1B,
                channel: 5
            }
        );
    }

    #[test]
    fn duration_is_a_ceiling_scale_by_three_fifths() {
        assert_eq!(voice_clip_duration_sectors(0), 0);
        assert_eq!(voice_clip_duration_sectors(100), 60);
        // (1*60 + 99) / 100 = 159 / 100 = 1 (ceiling of 0.6).
        assert_eq!(voice_clip_duration_sectors(1), 1);
        // (2*60 + 99) / 100 = 219 / 100 = 2.
        assert_eq!(voice_clip_duration_sectors(2), 2);
        assert_eq!(voice_clip_duration_sectors(50), (50u32 * 60).div_ceil(100));
        assert_eq!(
            voice_clip_duration_sectors(u16::MAX),
            (65535u32 * 60).div_ceil(100)
        );
    }

    #[test]
    fn end_offset_is_a_ceiling_scale_by_two_and_a_half() {
        assert_eq!(clip_end_lba_offset(0), 2); // 149 / 60 = 2
        assert_eq!(clip_end_lba_offset(60), (60 * 150 + 149) / 60);
        assert_eq!(clip_end_lba_offset(4), (4 * 150 + 149) / 60);
    }

    #[test]
    fn duration_clamp_matches_the_starter_bound() {
        assert!(!clip_duration_is_clamped(MAX_CLIP_DURATION_SECTORS));
        assert!(clip_duration_is_clamped(MAX_CLIP_DURATION_SECTORS + 1));
        // The offset uses the clamped value.
        assert_eq!(
            clip_end_lba_offset(MAX_CLIP_DURATION_SECTORS + 100),
            clip_end_lba_offset(MAX_CLIP_DURATION_SECTORS)
        );
    }
}
