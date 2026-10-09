//! Disc-gated: a field's ambient effect scripts sound their cues.
//!
//! `kor5`'s scene-entry effect tree runs a move-VM part that stores cue
//! `0x204` straight into SFX ring slot 3 every 63 vsyncs - op `0x1D`,
//! `sh op[1], DAT_8007B6DE` (`0x80023680..0x8002368C` in `FUN_80023070`).
//! A key-on census of a `kor5` memory-card state
//! (`scripts/pcsx-redux/autorun_keyon_census.lua`) sees that store at
//! `0x80023688` and the drained three-voice key-on two vsyncs later, on a
//! 63-vsync cadence, with no input. The engine's field move-VM host left the
//! op on the trait's no-op default, so the cue never reached either host's
//! ring.
//!
//! Skip-pass (CLAUDE.md disc-gated convention): `LEGAIA_DISC_BIN` unset or
//! `extracted/` missing.

use std::path::PathBuf;

use legaia_engine_core::world::SfxRingOp;
use legaia_engine_shell::boot::{BootConfig, BootSession, FieldLiveOpts};

fn extracted_dir() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("LEGAIA_EXTRACTED_DIR").map(PathBuf::from)
        && d.join("PROT.DAT").exists()
    {
        return Some(d);
    }
    ["extracted", "../extracted", "../../extracted"]
        .into_iter()
        .map(PathBuf::from)
        .find(|d| d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists())
}

#[test]
fn kor5_ambient_script_writes_its_cue_into_ring_slot_3() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };
    let cfg = BootConfig {
        scene: "kor5".to_string(),
        enable_audio: false,
    };
    let mut s = BootSession::open(&extracted, &cfg).expect("boot session");
    s.host.world.seed_free_roam_story_baseline("kor5");
    s.enter_field_live("kor5", &FieldLiveOpts::default())
        .expect("enter kor5");

    let mut writes = Vec::new();
    for frame in 0..400u32 {
        s.tick().expect("tick");
        for op in s.host.world.take_sfx_ring_ops() {
            if op == SfxRingOp::WriteSlot(3, 0x204) {
                writes.push(frame);
            }
        }
    }
    eprintln!("[ran] kor5 slot-3 cue 0x204 writes at frames {writes:?}");
    assert!(
        writes.len() >= 3,
        "the ambient part should store cue 0x204 into slot 3 repeatedly: {writes:?}"
    );
    // The cue resolves: runtime id `0x204` is a row of the scene's prescript
    // record 0, category 3 (the side-band slot), and the scene's op-`0x36`
    // sub-1 request leaves a side-band bank to sound it out of - retail keys
    // it from VAB slot 3, program 1, tones 1..=3.
    let row = s
        .host
        .world
        .runtime_sfx_descriptor(0x204)
        .expect("cue 0x204 resolves through kor5's runtime descriptor bank");
    eprintln!(
        "[ran] cue 0x204 row {row:02X?}, side-band bank {:?}",
        s.host.world.side_band_bank()
    );
    assert_eq!(row[4], 3, "category 3 = the side-band VAB slot");
    assert!(
        s.host.world.side_band_bank().is_some(),
        "kor5 requests a side-band bank for its ambient cue"
    );
    // One seated part stores the cue once per period, as retail's census
    // sees one store per period.
    assert!(
        writes.windows(2).all(|w| w[0] != w[1]),
        "the ambient record is seated once: {writes:?}"
    );
    // Retail repeats every 63 vsyncs. One session tick is one game tick of
    // `frame_step` vsyncs - two in a field scene - so that is 31-32 ticks.
    let step = u32::from(s.host.world.clock.frame_step.max(1));
    for pair in writes.windows(2) {
        let vsyncs = (pair[1] - pair[0]) * step;
        assert!(
            (60..=66).contains(&vsyncs),
            "cue cadence {vsyncs} vsyncs, retail repeats every 63: {writes:?}"
        );
    }
}
