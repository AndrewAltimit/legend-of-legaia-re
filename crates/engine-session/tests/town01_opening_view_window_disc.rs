//! Disc-gated: Rim Elm's establishing pan (`town01` `P2[3]`, the New Game
//! opening) crops by the record's own visible tile window, so the whole
//! village stays in shot while the camera closes on Vahn's house.
//!
//! Retail runs the entry script's install slice inside the MAN load
//! (`FUN_8003AEB0` -> `FUN_8003AB2C`), before the arrival frame's player tick
//! spawns the opening record. `P1[0]` sets the walking window
//! `(-10, -6, 8, 14)` and the record then sets `(-17, -13, 10, 31)`, the
//! window the `rim_elm_zoom_intro` capture holds. Stepping the record first
//! left the walking window in place for the whole pan, and the crop and the
//! actor cull hid most of the village until the record's next `0x46`.
//!
//! The run starts a New Game and takes the intro skip out of `opdeene`.

use std::path::PathBuf;

use legaia_engine_session::native::NativeSession;
use legaia_engine_session::session::{BootConfig, FieldLiveOpts};
use legaia_engine_world_views::camera_view::cutscene_owns_camera;
use legaia_engine_world_views::field_view_window::{crop_framing, field_view_cells};

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

/// The record's pan window (`46 24 EF F3 0A 1F`).
const PAN_WINDOW: [i8; 4] = [-17, -13, 10, 31];

#[test]
fn the_establishing_pan_crops_by_the_records_window() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };
    let cfg = BootConfig {
        scene: "town01".into(),
        enable_audio: false,
    };
    let mut s = NativeSession::open(&extracted, &cfg).expect("open boot session");
    assert!(
        s.start_new_game(&FieldLiveOpts::default()).is_some(),
        "the New Game entered its opening"
    );
    // Into the crawl, then the intro skip's confirm press, entered the way
    // both play hosts take `TickOwner::PrologueSkip`.
    for _ in 0..300 {
        s.tick().expect("tick");
    }
    let target = s
        .host
        .world
        .take_prologue_handoff(true)
        .expect("a confirm in the crawl skips the opening");
    s.enter_field_live(target, &FieldLiveOpts::default())
        .expect("enter town01");
    assert_eq!(s.host.world.active_scene_label, "town01");

    // The pan runs for several hundred frames before the record's next
    // window; sample it all.
    let mut sampled = 0;
    for t in 0..600 {
        s.tick().expect("tick");
        let w = &s.host.world;
        if !cutscene_owns_camera(w) {
            continue;
        }
        let view = w.npcs.cull_view.expect("the cull view stays published");
        if view.window != PAN_WINDOW {
            // The record's later `46 24 F1 FF 0A 20` for the walk-out.
            assert_eq!(
                view.window,
                [-15, -1, 10, 32],
                "only the record's own windows follow the pan's (t{t})"
            );
            break;
        }
        assert!(
            field_view_cells(w, crop_framing(w, &s.camera, false)).is_some(),
            "the crop holds through the pan (t{t})"
        );
        sampled += 1;
    }
    assert!(
        sampled > 300,
        "the pan held the record's window ({sampled} frames)"
    );
}
