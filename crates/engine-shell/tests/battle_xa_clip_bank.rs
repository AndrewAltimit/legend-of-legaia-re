//! Disc-gated: the battle's one-shot CD-XA clip bank the native boot stages
//! (`read_battle_xa_clip_bank`) has the shape the melee kernel's two sound
//! sites address, and the retail read spans cut where the disassembly says.
//!
//! `XA30.XA` (clip slot `0x1D`) is the ten-channel mono grunt bank
//! `FUN_801EC3E4` fires at `0x801EEB44` with `(0, 0x26)` / `(4, 0x2E)` /
//! `(6, 0x1A)`; `XA27.XA` (slot `26`) is the eight-channel stereo sting bank
//! the sound funnel's voice leg resolves `0x10C` to (channel 4, duration
//! entry `0x0C` of `DAT_800788B8`). Skip-passes without `LEGAIA_DISC_BIN`.

use std::path::PathBuf;

use legaia_engine_audio::xa_clip_bank::{MONO_FRAMES_PER_SECTOR, STEREO_FRAMES_PER_SECTOR};
use legaia_engine_shell::boot::read_battle_xa_clip_bank;

fn disc() -> Option<PathBuf> {
    let p = PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.exists().then_some(p)
}

#[test]
fn the_grunt_and_sting_banks_stage_with_their_retail_interleave() {
    let Some(disc) = disc() else {
        eprintln!("skip: LEGAIA_DISC_BIN unset");
        return;
    };
    let bank = read_battle_xa_clip_bank(&disc).expect("XA27 / XA30 demux off the disc");

    // XA30: ten mono channels; the three grunt channels the kernel names.
    assert_eq!(
        bank.channel_count(0x1D),
        10,
        "XA30 interleaves ten channels"
    );
    for ch in [0u8, 4, 6] {
        let clip = bank.clip(0x1D, ch).expect("grunt channel decoded");
        assert!(!clip.stereo, "XA30 channel {ch} is mono");
        assert_eq!(clip.sample_rate, 37_800);
        assert!(
            clip.frames() > MONO_FRAMES_PER_SECTOR,
            "channel {ch} has audio"
        );
    }
    // Vahn's `dur 0x26`: 95 physical sectors, 9 of the channel's own.
    assert_eq!(
        bank.cut_frames(0x1D, 0, 0x26),
        Some(9 * MONO_FRAMES_PER_SECTOR),
        "the grunt is cut at the retail read span"
    );

    // XA27: eight stereo channels; the `0x10C` sting on channel 4.
    assert_eq!(bank.channel_count(26), 8, "XA27 interleaves eight channels");
    let sting = bank.clip(26, 4).expect("sting channel decoded");
    assert!(sting.stereo, "XA27 is a stereo bank");
    assert_eq!(sting.sample_rate, 37_800);
    // Duration entry 373 -> 224 sectors -> 562 physical -> 70 own sectors,
    // which is the whole clip.
    let cut = bank.cut_frames(26, 4, 224).expect("cut resolves");
    assert_eq!(
        cut,
        sting.frames(),
        "the sting's span covers the whole clip"
    );
    assert_eq!(cut, 70 * STEREO_FRAMES_PER_SECTOR);
}

/// The animation cue tracks' party voice band lands on the per-character
/// banks `XA1` / `XA3` / `XA5` and `XA27` / `XA28` / `XA29`; Vahn's Spirit
/// clip (art-bank record 0) opens with cue `0xC8` -> `0x100` -> clip slot `0`
/// channel `0`, so `XA1` channel 0 must be staged with audio.
#[test]
fn the_spirit_voice_bank_stages() {
    let Some(disc) = disc() else {
        eprintln!("skip: LEGAIA_DISC_BIN unset");
        return;
    };
    let bank = read_battle_xa_clip_bank(&disc).expect("battle XA banks demux off the disc");
    for slot in [0u8, 2, 4, 26, 27, 28] {
        assert!(bank.channel_count(slot) > 0, "clip slot {slot} staged");
    }
    let spirit = bank.clip(0, 0).expect("XA1 channel 0 decoded");
    eprintln!(
        "[ran] XA1: {} channels, ch0 {} frames stereo={} rate={}",
        bank.channel_count(0),
        spirit.frames(),
        spirit.stereo,
        spirit.sample_rate
    );
    assert!(spirit.frames() > 0, "the Spirit voice has audio");
}
