//! Disc-gated: sliding along `town01`'s door at tile `(22, 18)` keeps the
//! slide's full speed.
//!
//! The door is a static-class prop (bind record `P0[4]`, `25 21 21`), so
//! every step the walk presses into it posts its touch (`FUN_801D5B5C`),
//! which raises the player's `+0x10 & 0x80000`, and the record ends on its
//! first `0x21`, which clears it again (`FUN_80039B7C`, `0x80039EF8`).
//! Retail's player tick runs ahead of every placed object in the pass, so
//! the record's slice runs in the pass whose walk posted the touch and the
//! next walk is free.
//!
//! The port walks every vsync and ran prop records on game frames only, so
//! at frame step 2 a touch landed between game frames, the lock stood over
//! the next game frame's walk gate, and the player slid on every other
//! vsync: half speed along the whole door.
//!
//! Skips and passes without `LEGAIA_DISC_BIN` / `extracted/`.

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

#[test]
fn town01_door_slide_keeps_full_speed() {
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
    let mut s = NativeSession::open(&extracted, &cfg).expect("open session");
    s.host.world.toggles.use_vm_dialogue = true;
    s.enter_field_live("town01", &FieldLiveOpts::default())
        .expect("enter town01");
    s.set_host_drains_queues(true);
    // Play the entry beat out.
    for t in 0..3000u32 {
        let w = &s.host.world;
        if t >= 30
            && !(w.cutscene_timeline_active()
                || w.dialogue_owns_input()
                || w.script_context_engages_player())
        {
            break;
        }
        s.host.world.set_pad(if t % 8 == 0 {
            PadButton::Cross.mask()
        } else {
            0
        });
        s.tick().unwrap();
    }
    assert!(
        s.host.world.props.bank.props.contains_key(&(22, 18)),
        "town01 places its door prop at (22, 18)"
    );
    // From (21, 20), down-right runs into the door's corner and slides
    // along its face.
    s.host.world.seat_player_at_tile(21, 20);
    s.host.world.set_pad(0);
    for _ in 0..6 {
        s.tick().unwrap();
    }
    let pad = PadButton::Down.mask() | PadButton::Right.mask();
    let (mut touches, mut held, mut stalls) = (0, 0, 0);
    let mut prev = None;
    let mut prev_touched = false;
    for _ in 0..60 {
        s.host.world.encounters.roll_hold = 0;
        s.host.world.set_pad(pad);
        s.tick().unwrap();
        let w = &s.host.world;
        // The touch post's tail stores the eight-tick roll hold.
        let touched = w.encounters.roll_hold > 0;
        touches += u32::from(touched);
        held += u32::from(w.locomotion.locked_at_walk_gate);
        let a = &w.actors[w.player_actor_slot.expect("player") as usize].move_state;
        let p = (a.world_x, a.world_z);
        // A vsync right after a touch that did not move the player.
        if prev_touched && prev == Some(p) {
            stalls += 1;
        }
        prev = Some(p);
        prev_touched = touched;
    }
    eprintln!("[town01 door] touches={touches} held={held} stalls={stalls}");
    assert!(touches >= 8, "the slide never pressed into the door");
    assert_eq!(held, 0, "a touch's lock stood over a walk gate");
    assert_eq!(stalls, 0, "the slide stopped on a vsync along the door");
}
