//! A cutscene that holds the player from the first tick of a scene and
//! hands the camera back with a glide: `vell`'s Genesis Tree revival
//! (`P2[11]`) installed the moment a save lands, the way `play-window
//! --play-record` and the capture seats start a record. It stages its
//! op-`0x45` shots and releases them with `45 C0 3C 00`, a 60-frame glide to
//! the follow pose.
//!
//! The follow pose is composed from the camera block the MAN load queried
//! (`FUN_801DE3E0` at `0x8003B800`, before any script runs). The port
//! deferred that query to the first follow tick, which never ran while the
//! record held the player, so the hand-back composed the reset block (`H`
//! and eye depth `0`) and the glide shrank the scene to a point before the
//! follow camera snapped back in. The arrival actor's countdown sat in the
//! same skipped tick, so the follow view read its terrain-less fallback
//! through the glide as well.
//!
//! Disc-gated: skips without `LEGAIA_DISC_BIN` or an extracted tree.

use std::path::PathBuf;

use legaia_engine_session::native::NativeSession;
use legaia_engine_session::session::{BootConfig, FieldLiveOpts};
use legaia_engine_system::input::PadButton;

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

/// The record holds the camera as the modal timeline or a spawned context.
fn held(s: &NativeSession) -> bool {
    s.host.world.cutscene_timeline_active() || !s.host.world.field_vm.helper_contexts.is_empty()
}

#[test]
fn a_cutscene_from_the_landing_hands_back_to_the_scene_block_pose() {
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
    s.host.world.toggles.use_vm_dialogue = true;
    let save = legaia_save::SaveFile {
        party: legaia_save::Party::zeroed(3),
        ..Default::default()
    };
    let opts = FieldLiveOpts::default();
    s.resume_save(save, "vell", &opts);
    assert!(s.play_scene_record(2, 11), "vell P2[11] installs");

    let mut ran = false;
    let mut min_h = i32::MAX;
    let mut min_depth = i32::MAX;
    for t in 0..6000u32 {
        let busy = held(&s) || s.host.world.dialogue_owns_input();
        let pad = if busy && t % 2 == 0 {
            PadButton::Cross.mask()
        } else {
            0
        };
        s.host.world.set_pad(pad);
        s.tick().expect("tick");
        if held(&s) {
            ran = true;
        }
        if ran {
            let g = &s.camera.globals.0;
            min_h = min_h.min(g[9]);
            min_depth = min_depth.min(g[5].abs());
        }
        if ran && !held(&s) && s.camera.mover.is_none() {
            break;
        }
    }
    assert!(ran, "the revival ran");
    // Every shot the record stages and the follow pose it returns to carry
    // an `H` of a few hundred and an eye depth of thousands; the reset
    // block's zeros are what a glide toward an unloaded block reaches.
    assert!(
        min_h >= 256,
        "the hand-back glide kept a real H (lowest {min_h})"
    );
    assert!(
        min_depth >= 1000,
        "the hand-back glide kept the eye back (lowest depth {min_depth})"
    );
}
