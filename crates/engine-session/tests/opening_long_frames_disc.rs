//! Disc-gated: the opening's field scenes open on long frames, as retail's do.
//!
//! `opstati` and `opurud` seat four CLUT-cell cyclers each at entry: two grey
//! the scene's CLUT rows `(0, 473, 320, 2)` and `(0, 476, 320, 34)`
//! (`2B 00 FC00 00`) and halt into two that capture the grey rows and tint
//! them (`2B 38 80 00`), so the scenes draw in sepia. Each upload recolours
//! every texel of its strip (`FUN_80019D50`), about `1200` hblanks for the
//! wide one, so a frame that uploads lasts seven to nine vsyncs on retail
//! while it credits a step of three or four. A per-vsync PCSX-Redux timeline
//! off `s1_newgame_field` (`autorun_audio_timeline.lua` with the frame-time
//! words `0x8007B6C0` / `0x8007B6C4`, and an exec-breakpoint stamp of
//! `FUN_80019D50`) reads those frames on `opstati`'s game frames 1, 3 and 4
//! and `opurud`'s 1, 2, 4 and 5, and `opstati`'s first `4A 5A` holds the
//! record `101` vsyncs where `90` would be its credit. The dialog pager runs
//! once a game frame too, so `opurud`'s title card, typed over those frames,
//! parks its record `61` vsyncs.
//!
//! Skips (and passes) without `LEGAIA_DISC_BIN`.

use legaia_engine_session::native::NativeSession;
use legaia_engine_session::session::{BootConfig, FieldLiveOpts};

fn extracted_dir() -> Option<std::path::PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = std::path::PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

#[test]
fn the_openings_cyclers_lengthen_their_frames_and_tint_the_scene() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };
    let cfg = BootConfig {
        scene: "town01".into(),
        enable_audio: false,
    };
    let mut s = NativeSession::open(&extracted, &cfg).expect("open boot session");
    assert!(s.start_new_game(&FieldLiveOpts::default()).is_some());
    // Per scene: the waits of its first eight game frames, and the vsyncs
    // the record's first `4A 5A` held (`opstati` only).
    let mut waits: std::collections::BTreeMap<String, Vec<u8>> = Default::default();
    let mut first_wait: Option<(u32, u32)> = None;
    let mut card_hold = 0u32;
    let mut strip_before: Option<Vec<u16>> = None;
    let mut strip_after: Option<Vec<u16>> = None;
    let strip = |s: &NativeSession| -> Option<Vec<u16>> {
        let r = s.host.resources.as_ref()?;
        Some((0..320).map(|x| r.vram.pixel(x, 490)).collect())
    };
    for t in 0..12_000u32 {
        s.tick().expect("tick");
        let w = &s.host.world;
        let label = w.active_scene_label.clone();
        if label == "map01" {
            break;
        }
        if w.clock.game_tick_fired && matches!(label.as_str(), "opstati" | "opurud") {
            let v = waits.entry(label.clone()).or_default();
            if v.len() < 8 {
                v.push(w.clock.game_frame_wait());
            }
        }
        if label == "opurud" && w.cutscene.timeline.as_ref().map(|tl| tl.pc) == Some(97) {
            card_hold += 1;
        }
        if label == "opstati" {
            let pc = w.cutscene.timeline.as_ref().map(|tl| tl.pc);
            match (pc, first_wait) {
                (Some(90), None) => first_wait = Some((t, t)),
                (Some(90), Some((a, _))) => first_wait = Some((a, t)),
                _ => {}
            }
            if strip_before.is_none() {
                strip_before = strip(&s);
            }
            if t % 600 == 0 {
                strip_after = strip(&s);
            }
        }
    }
    let opstati = waits.get("opstati").expect("opstati played");
    let opurud = waits.get("opurud").expect("opurud played");
    let long = |v: &[u8]| v.iter().filter(|&&w| w > 4).count();
    assert!(
        long(opstati) >= 3 && long(opurud) >= 4,
        "the uploading frames outlast their step: opstati {opstati:?}, opurud {opurud:?}"
    );
    let (a, b) = first_wait.expect("opstati's 4A 5A ran");
    let held = b - a + 1;
    assert!(
        (97..=105).contains(&held),
        "opstati's first 4A 5A holds retail's 101 vsyncs: {held}"
    );
    eprintln!(
        "[long-frames] opstati {opstati:?} opurud {opurud:?} first 4A 5A {held} card {card_hold}"
    );
    // `opurud`'s title card (`CC F8 89` at `+0x5C`, its lines at `+0x61`)
    // types over the scene's long frames: retail's pager runs once a game
    // frame and parks the record on the card for 61 vsyncs; a pager that
    // ran on the long frames' extra vsyncs typed it in 42.
    assert!(
        (50..=64).contains(&card_hold),
        "opurud's card holds its record near retail's 61 vsyncs: {card_hold}"
    );
    let (before, after) = (strip_before.unwrap(), strip_after.unwrap());
    assert_ne!(before, after, "the cyclers recolour the scene's CLUT rows");
}
