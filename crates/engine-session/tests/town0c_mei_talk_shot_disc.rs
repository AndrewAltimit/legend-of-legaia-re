//! A talk that stages its own camera shot holds the camera as a scripted
//! record does, so the user's follow knobs stay out of it.
//!
//! `town0c` P1[24], Mei meeting Noa in her house (system flags `0x11` and
//! `0x6B4` up, `0x211` clear), is a talk record that snaps three op-`0x45`
//! shots and glides a fourth. The first stands the eye `2170` units back
//! from Mei by the bed (pitch `182`, yaw `-707`, `H` `616`). The port framed
//! a talk's shot through the follow arm with the `Far` distance preset
//! applied, which put the eye behind the house wall: the wall filled the
//! lower half of the frame, and the widened tile window drew the wall
//! retail's crop leaves out. `World::scripted_shot_holds_camera` now counts a
//! talk holding a shot it staged, which keeps both at retail's framing.
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

/// The talk's first shot: pitch, yaw, eye depth, `H`.
const BED_SHOT: (i32, i32, i32, i32) = (182, -707, 2170, 616);

#[test]
fn meis_talk_holds_its_own_shot() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };
    let cfg = BootConfig {
        scene: "town0c".into(),
        enable_audio: false,
    };
    let mut s = NativeSession::open(&extracted, &cfg).expect("open session");
    s.host.world.toggles.use_vm_dialogue = true;
    for f in [0x11u16, 0x6B4] {
        s.host.world.system_flag_set(f);
    }
    s.enter_field_live("town0c", &FieldLiveOpts::default())
        .expect("enter town0c");
    s.set_host_drains_queues(true);
    for _ in 0..10 {
        s.tick().expect("tick");
    }
    // One tile south of Mei's seat `(95, 57)`, facing her (Z-).
    s.host.world.seat_player_at_tile(95, 58);
    for _ in 0..30 {
        s.tick().expect("tick");
    }
    {
        let w = &mut s.host.world;
        let slot = usize::from(w.player_actor_slot.expect("player"));
        w.actors[slot].move_state.render_26 = 0x800;
    }
    let mut held_on_shot = None;
    let mut talked = false;
    let mut released = false;
    for t in 0..2500u32 {
        let busy = s.host.world.dialogue_owns_input();
        talked |= busy;
        if talked && !busy {
            released = !s.host.world.scripted_shot_holds_camera();
            break;
        }
        let pad = if t == 2 || (busy && t % 8 == 0) {
            PadButton::Cross.mask()
        } else {
            0
        };
        s.host.world.set_pad(pad);
        s.tick().expect("tick");
        let g = &s.camera.globals.0;
        if held_on_shot.is_none() && (g[0], g[1], g[5], g[9]) == BED_SHOT {
            held_on_shot = Some(s.host.world.scripted_shot_holds_camera());
        }
    }
    assert_eq!(
        held_on_shot,
        Some(true),
        "the talk's bed shot is a scripted shot"
    );
    assert!(released, "the follow knobs come back once the talk ends");
}
