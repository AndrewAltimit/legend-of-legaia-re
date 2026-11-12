//! A zero-trigger APPLY inside a record cuts to the follow pose at once:
//! Biron's mist beat (`bylon` `P2[6]`) runs `45 C0 00 00` twenty frames
//! before "Noa: Vahn!", and a retail recording of the beat has the camera on
//! the follow pose (pitch `215`, yaw `-80`, `H` `768`) by the time the box
//! opens. The port armed the follow camera's snap instead, whose tick does
//! not run while a record holds the shot, so the box opened over the
//! previous stair shot.
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

/// The record's "Noa: Vahn!" text op, after the `45 C0 00 00` at `+0x2DA`.
const NOA_TEXT_PC: usize = 0x2ED;

#[test]
fn the_mist_beat_cuts_to_the_follow_pose_before_noa_speaks() {
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
    s.resume_save(save, "bylon", &FieldLiveOpts::default());
    assert!(s.play_scene_record(2, 6), "bylon P2[6] installs");

    let mut reached = None;
    for t in 0..6000u32 {
        let at = s.host.world.cutscene.timeline.as_ref().map(|tl| tl.pc);
        if at == Some(NOA_TEXT_PC) {
            let g = &s.camera.globals.0;
            reached = Some((g[0], g[1], g[9]));
            break;
        }
        let busy = s.host.world.dialogue_owns_input();
        s.host.world.set_pad(if busy && t % 2 == 0 {
            PadButton::Cross.mask()
        } else {
            0
        });
        s.tick().expect("tick");
    }
    let (pitch, yaw, h) = reached.expect("the record reaches Noa's line");
    assert_eq!((pitch, yaw, h), (215, -80, 768), "retail's follow pose");
}
