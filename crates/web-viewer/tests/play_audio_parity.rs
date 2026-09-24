//! Disc-gated oracle for the play page's **audio parity** work: the CD-XA
//! lane (`LegaiaRuntime::play_xa_*`) and the widened battle cue path.
//!
//! What this pins without a speaker:
//!
//! 1. **The XA lane wants exactly the files the native boot stages**, and
//!    every one resolves to an extent on the visitor's disc through the same
//!    `disc_file_extent_json` the page slices with.
//! 2. **Installing a shout file decodes real audio.** `XA2.XA`'s raw sectors,
//!    sliced from the disc the way the page slices them, demux into Vahn's
//!    channels and the capture-verified voiced art (Somersault, action
//!    `0x27` - the anchor `engine-shell/tests/arts_shout_battle.rs` uses)
//!    resolves through the executable's cue pools to non-silent PCM.
//! 3. **Installing a clip file stages the grunt bank** with the retail
//!    read-span cut, and an un-installed slot stays unstaged.
//! 4. **A cast cue is never a truncated descriptor**: `0x118` declines on the
//!    voice leg and is counted as such, rather than keying descriptor `0x18`.
//! 5. **The level-up cue routes to slot 11** and, unstaged, takes the class-2
//!    fallback it always took.
//!
//! No Sony bytes are asserted, only structural facts. Skips + passes when
//! `LEGAIA_DISC_BIN` is unset.

#![cfg(not(target_arch = "wasm32"))]

use std::io::{Read, Seek, SeekFrom};

use legaia_web_viewer::runtime::LegaiaRuntime;

const RAW_SECTOR: u64 = 2352;

fn disc_path() -> Option<String> {
    let p = std::env::var("LEGAIA_DISC_BIN").ok()?;
    std::path::Path::new(&p).is_file().then_some(p)
}

fn loaded_in_town() -> Option<(String, LegaiaRuntime)> {
    let disc = disc_path()?;
    let bytes = std::fs::read(&disc).ok()?;
    let mut rt = LegaiaRuntime::new();
    rt.load_disc(bytes, String::new()).ok()?;
    rt.enter_field("town01").ok()?;
    Some((disc, rt))
}

/// The ISO path the lane advertises for a bare file name.
fn wanted_path(rt: &LegaiaRuntime, name: &str) -> String {
    let wanted: Vec<String> = serde_json::from_str(&rt.play_xa_wanted_files_json()).unwrap();
    wanted
        .into_iter()
        .find(|p| p.to_ascii_uppercase().ends_with(name))
        .unwrap_or_else(|| name.to_string())
}

/// Slice a file's raw sectors out of the disc image exactly as the page
/// does: `lba * 2352` for `ceil(size / 2048) * 2352` bytes.
fn raw_sectors_of(disc: &str, rt: &LegaiaRuntime, file: &str) -> Option<Vec<u8>> {
    let ext: serde_json::Value = serde_json::from_str(&rt.disc_file_extent_json(file)).ok()?;
    let lba = ext["lba"].as_u64()?;
    let size = ext["size"].as_u64()?;
    let sectors = size.div_ceil(2048);
    let mut f = std::fs::File::open(disc).ok()?;
    f.seek(SeekFrom::Start(lba * RAW_SECTOR)).ok()?;
    let mut buf = vec![0u8; (sectors * RAW_SECTOR) as usize];
    f.read_exact(&mut buf).ok()?;
    Some(buf)
}

#[test]
fn xa_lane_wants_the_native_staging_set_and_every_file_is_on_the_disc() {
    let Some((_, rt)) = loaded_in_town() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let wanted: Vec<String> =
        serde_json::from_str(&rt.play_xa_wanted_files_json()).expect("wanted json");
    let names: Vec<&str> = wanted
        .iter()
        .map(|p| {
            p.rsplit('/')
                .next()
                .unwrap_or(p)
                .split(';')
                .next()
                .unwrap_or(p)
        })
        .collect();
    assert_eq!(names, ["XA2.XA", "XA4.XA", "XA6.XA", "XA27.XA", "XA30.XA"]);
    for f in &wanted {
        let ext = rt.disc_file_extent_json(f);
        assert_ne!(ext, "null", "{f} must resolve to an ISO extent");
    }
}

#[test]
fn installing_xa2_decodes_vahns_somersault_shout_to_non_silent_pcm() {
    let Some((disc, mut rt)) = loaded_in_town() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    // Before install: nothing resolves, and the probe says so.
    assert_eq!(rt.play_xa_probe_shout_peak(0, 0x27), 0);
    let path = wanted_path(&rt, "XA2.XA");
    let sectors = raw_sectors_of(&disc, &rt, &path).expect("XA2.XA extent + bytes");
    assert!(rt.play_xa_install(&path, &sectors), "XA2.XA must decode");
    let v: serde_json::Value = serde_json::from_str(&rt.play_xa_state_json()).unwrap();
    assert_eq!(v["shout_bank"], true);
    assert_eq!(
        v["voice_tables"], true,
        "the SCUS cue pools must decode, or no art can pick a channel"
    );
    let channels = v["installed"]["XA2.XA"].as_u64().expect("channel count");
    assert!(
        (1..=16).contains(&channels),
        "XA2 is a 16-channel shout bank; decoded {channels}"
    );
    // Somersault (action 0x27) is the capture-verified voiced anchor.
    let peak = rt.play_xa_probe_shout_peak(0, 0x27);
    assert!(
        peak > 256,
        "Somersault must resolve to audible PCM through the cue pools; peak {peak}"
    );
    // Terra (cslot 3) has no clip file and stays silent.
    assert_eq!(rt.play_xa_probe_shout_peak(3, 0x27), 0);
    // The sectors were borrowed: re-installing replaces, never appends.
    assert!(rt.play_xa_install(&path, &sectors));
    let v2: serde_json::Value = serde_json::from_str(&rt.play_xa_state_json()).unwrap();
    assert_eq!(v2["installed"]["XA2.XA"], v["installed"]["XA2.XA"]);
}

#[test]
fn installing_xa30_stages_the_grunt_clips_with_the_retail_cut() {
    let Some((disc, mut rt)) = loaded_in_town() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let path = wanted_path(&rt, "XA30.XA");
    let sectors = raw_sectors_of(&disc, &rt, &path).expect("XA30.XA extent + bytes");
    assert!(rt.play_xa_install(&path, &sectors), "XA30.XA must decode");
    let v: serde_json::Value = serde_json::from_str(&rt.play_xa_state_json()).unwrap();
    assert_eq!(v["clip_bank"], true);
    let channels = v["installed"]["XA30.XA"].as_u64().expect("channel count");
    assert!(channels >= 1, "XA30 carries the per-character grunts");
    // Slot 0x1D channel 0: a short read span cuts fewer frames than a long one,
    // and both are bounded by the clip.
    let short = rt.play_xa_probe_clip_frames(0x1D, 0, 6);
    let long = rt.play_xa_probe_clip_frames(0x1D, 0, 6000);
    assert!(short > 0, "channel 0 must be staged");
    assert!(
        short <= long,
        "a longer span never cuts shorter: {short} > {long}"
    );
    // XA27 was not installed: its slot stays unstaged.
    assert_eq!(rt.play_xa_probe_clip_frames(26, 0, 60), 0);
}

#[test]
fn a_cast_cue_declines_on_the_voice_leg_instead_of_keying_a_truncated_descriptor() {
    let Some((_, mut rt)) = loaded_in_town() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    // The truncated form (0x18) is a real, populated descriptor on this disc
    // (category 2, audible wherever the class-2 bank is resident) - which is
    // exactly why truncation was wrong rather than silent.
    assert_eq!(rt.play_sfx_cue_slot(0x18), 2);
    // The cast cue itself renders nothing through the descriptor probe...
    assert_eq!(rt.play_sfx_probe_peak(0x118, 44_100), 0);
    // ...and a live request is declined on the voice leg and counted.
    let before: serde_json::Value = serde_json::from_str(&rt.play_sfx_state_json()).unwrap();
    assert!(!rt.play_sfx(0x118));
    let after: serde_json::Value = serde_json::from_str(&rt.play_sfx_state_json()).unwrap();
    assert_eq!(
        after["voice_cues_dropped"].as_u64().unwrap(),
        before["voice_cues_dropped"].as_u64().unwrap() + 1
    );
    assert_eq!(
        after["queued"].as_u64().unwrap(),
        before["queued"].as_u64().unwrap() + 1,
        "the request reached the queue"
    );
}

#[test]
fn level_up_cue_names_slot_11_and_is_silent_until_staged() {
    let Some((_, mut rt)) = loaded_in_town() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    assert_eq!(rt.play_sfx_cue_slot(0x50), 11, "cue 0x50 is category 11");
    let v: serde_json::Value = serde_json::from_str(&rt.play_sfx_state_json()).unwrap();
    assert_eq!(v["reward_bank_staged"], false);
    assert_eq!(
        rt.play_sfx_cue_bank_prot(0x50),
        0,
        "an unstaged slot 11 is closed, and a closed slot is silent"
    );
    // The duck starts at its reference level on a fresh runtime.
    assert_eq!(v["duck_level"], 0xD7);
    assert_eq!(v["duck_target"], 0xD7);
}
