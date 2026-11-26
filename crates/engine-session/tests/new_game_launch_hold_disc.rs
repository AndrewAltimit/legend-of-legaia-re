//! Disc-gated: a New Game holds the black between the title's confirm and the
//! opening for as long as retail's does.
//!
//! Retail's NEW GAME runs the title tick's `LaunchFade` and `LaunchGame`
//! frames (109, unpressed) and then master mode 2's field MAIN INIT, which
//! streams the field overlay and `opdeene` (380 vsyncs on a per-vsync
//! PCSX-Redux film from `title_attract`). The opening record then reaches its
//! first crawl on the field's first frames. The port's loads are instant by
//! default (`skip_load_holds`); the faithful mode holds the MAIN INIT too.

use std::path::PathBuf;

use legaia_engine_session::native::NativeSession;
use legaia_engine_session::session::{BootConfig, FieldLiveOpts};

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

/// Ticks from the New Game until the opening record first runs.
fn ticks_to_the_record(skip_load_holds: bool) -> Option<u32> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return None;
    };
    let cfg = BootConfig {
        scene: "town01".into(),
        enable_audio: false,
    };
    let mut s = NativeSession::open(&extracted, &cfg).expect("open boot session");
    s.host.world.toggles.skip_load_holds = skip_load_holds;
    s.start_new_game(&FieldLiveOpts::default())
        .expect("the New Game entered its opening");
    assert!(
        s.host.world.launch_fade.is_some(),
        "the launch fade is armed"
    );
    let start_pc = s.host.world.cutscene.timeline.as_ref().map(|tl| tl.pc);
    for t in 0..1000 {
        s.tick().expect("tick");
        let w = &s.host.world;
        let held = w.launch_fade.is_some() || w.field_init_hold > 0;
        let pc = w.cutscene.timeline.as_ref().map(|tl| tl.pc);
        if held {
            assert_eq!(pc, start_pc, "the record waits behind the hold (t{t})");
        } else if pc != start_pc {
            return Some(t);
        }
    }
    panic!("the opening record never ran");
}

#[test]
fn the_launch_fade_holds_the_opening() {
    let Some(t) = ticks_to_the_record(true) else {
        return;
    };
    assert_eq!(t, 109, "LaunchFade's 108 frames and LaunchGame's");
}

#[test]
fn the_faithful_mode_holds_the_main_init_too() {
    let Some(t) = ticks_to_the_record(false) else {
        return;
    };
    assert_eq!(
        t,
        109 + 380,
        "the launch, then mode 2's overlay and scene loads"
    );
}
