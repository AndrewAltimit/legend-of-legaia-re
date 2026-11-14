//! Disc-gated: `ropeway`'s ruins report (`P1[45]`, the book in Octam's
//! shrine) plays its vision out and walks the player back.
//!
//! Answering Yes, the record fades to white, seats the player on the vision
//! spot (`A3 F8 70 36`), stages its shot and then pages through four lines,
//! each behind a morph of placement `42` (channel `0x40`): `CB 40 05 01 ..`
//! arms five VDF lanes, `B1 40 0C` raises the gate, `AC 40 0C` / `AB 40 0A`
//! / `AC 40 0D` drop the recycle bit and raise HOLD, and `AC 40 0B` /
//! `AD 40 0B` waits for the envelope's ADVANCE. `FUN_80020740` raises
//! `+0x62 & 0x800` when the last lane peaks under HOLD
//! (`0x80020948..0x80020960`). The record then fades again, seats the player
//! back at the bookcase (`A3 F8 A2 DD`), hands the shot back (`45 C0 00 00`)
//! and closes.
//!
//! The talk runner ran the morph ops on its stand-in context, so no lanes
//! were armed, nothing raised ADVANCE, and the first `AD 40 0B` (`+0x1F0`)
//! waited until the spin net ended the talk: the player was left on the
//! vision spot with the shot still staged.
//!
//! Skips and passes without `LEGAIA_DISC_BIN` / `extracted/`.

use std::path::PathBuf;

use legaia_engine_core::world::MorphOwner;
use legaia_engine_field::inline_dialogue::INLINE_SPIN_PARK_TIMEOUT;
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

const BOOK: u8 = 45;
const MURAL: u8 = 42;
/// `A3 F8 A2 DD`: tile `(0x22, 0x5D)` plus the half-tile the high bits add.
const BACK: (i16, i16) = (0x22 * 128 + 0x80, 0x5D * 128 + 0x80);
/// `A3 F8 70 36`.
const VISION: (i16, i16) = (0x70 * 128 + 0x40, 0x36 * 128 + 0x40);

#[test]
fn ropeway_ruins_report_walks_the_player_back() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };
    let cfg = BootConfig {
        scene: "ropeway".into(),
        enable_audio: false,
    };
    let mut s = NativeSession::open(&extracted, &cfg).expect("open session");
    s.host.world.toggles.use_vm_dialogue = true;
    s.enter_field_live("ropeway", &FieldLiveOpts::default())
        .expect("enter ropeway");
    // The hosts bind every placement's clip cursor at entry; a talk's
    // `2D 08` waits on the talker's own clip.
    let _ = legaia_engine_session::scene_actors::bind_scene_actors(&mut s.host);
    s.set_host_drains_queues(true);
    for _ in 0..20 {
        s.tick().unwrap();
    }
    let &(nx, nz) = s
        .host
        .world
        .npcs
        .positions
        .get(&BOOK)
        .expect("the ruins report is placed");
    s.host
        .world
        .seat_player_at_tile((nx >> 7) as u8, ((nz >> 7) as u8).saturating_add(1));
    for _ in 0..4 {
        s.tick().unwrap();
    }
    let player = |s: &NativeSession| {
        let w = &s.host.world;
        let a = &w.actors[w.player_actor_slot.expect("player") as usize].move_state;
        (a.world_x, a.world_z)
    };
    s.host.world.trigger_field_interact(0, BOOK);
    let (mut talked, mut ended) = (false, false);
    let (mut saw_vision, mut max_park, mut peaked) = (false, 0, false);
    // The picker's cursor starts on Yes; a press every eight frames pages
    // the boxes and takes it.
    for t in 0..6000u32 {
        let busy = s.host.world.dialogue_owns_input();
        talked |= busy;
        if talked && !busy {
            ended = true;
            break;
        }
        saw_vision |= player(&s) == VISION;
        if let Some(id) = s.host.world.dialog.inline.as_ref() {
            max_park = max_park.max(id.park_frames);
        }
        peaked |= s
            .host
            .world
            .field_morph_lanes(MorphOwner::Placement(MURAL))
            .is_some_and(|l| l.len() == 5 && l.iter().all(|&(_, w)| w == 0x1000));
        let pad = if busy && t % 8 == 0 {
            PadButton::Cross.mask()
        } else {
            0
        };
        s.host.world.set_pad(pad);
        s.tick().unwrap();
    }
    eprintln!(
        "[ropeway] ended={ended} vision={saw_vision} peaked={peaked} max_park={max_park} at {:?}",
        player(&s)
    );
    assert!(ended, "the ruins report never closed");
    assert!(saw_vision, "Yes seats the player on the vision spot");
    assert!(peaked, "placement {MURAL}'s five lanes run to the peak");
    assert!(
        max_park < INLINE_SPIN_PARK_TIMEOUT,
        "a morph wait ran into the spin net ({max_park} frames)"
    );
    assert_eq!(
        player(&s),
        BACK,
        "the record's `A3 F8 A2 DD` puts the player back at the bookcase"
    );
    assert!(
        !s.camera.script_owns_focus(),
        "the record's APPLY hands the shot back"
    );
}
