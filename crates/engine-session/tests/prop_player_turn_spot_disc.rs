//! Disc-gated: placed-object records that turn the player, beside `vell`'s
//! Bridge Grass (`vell_bridge_grass_disc.rs`).
//!
//! A prop record's `B8 F8 <dir> <frames>` is the walk kernel's RotateToAngle
//! leg on the player (a zero budget writes the compass heading at once), and
//! `CC F8 85|8E|8F <lo> <hi> <bind>` its FaceTarget leg. The prop runner
//! stepped both on a stand-in context and the player kept whatever heading
//! the touch found. The census (`engine-scene/tests/prop_record_actor_ops_disc.rs`)
//! finds them in about a hundred object records; these are two of them:
//!
//! - `taiku` `P0[9]` (flags `0x53B`, `0x384`): walks the player in, turns
//!   them to compass `4` over 15 frames (`B8 F8 84 0F`), then turns them
//!   toward an actor (`CC F8 85 .. `);
//! - `jiji` `P0[5]`, the inn counter: walks the keeper to the counter and
//!   turns the player toward them (`CC F8 85 14 00 0C`) before the greeting.
//!
//! Skips + passes when `LEGAIA_DISC_BIN` is unset.

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

/// The player's engine heading (`0` = `+Z`) after each tick of `record`'s
/// run in `scene`, from a seat beside the object.
fn headings(scene: &str, record: usize, flags: &[u16], seat: Option<(u8, u8)>) -> Option<Vec<i16>> {
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
    let save = legaia_save::SaveFile {
        party: legaia_save::Party::zeroed(1),
        ..Default::default()
    };
    s.resume_save(save, scene, &FieldLiveOpts::default());
    for &f in flags {
        s.host.world.system_flag_set(f);
    }
    for _ in 0..60 {
        s.tick().expect("tick");
    }
    let anchor = s
        .host
        .world
        .props
        .bank
        .props
        .iter()
        .find(|(_, p)| p.record == record)
        .map(|(&a, _)| a)
        .expect("the object's prop");
    let seat = seat.unwrap_or((anchor.0, anchor.1.saturating_add(1)));
    s.host.world.seat_player_at_tile(seat.0, seat.1);
    for _ in 0..10 {
        s.tick().expect("tick");
    }
    assert!(s.host.world.start_prop_interaction(anchor));
    let mut out = Vec::new();
    for t in 0..2000u32 {
        s.host.world.set_pad(if t % 2 == 0 {
            PadButton::Cross.mask()
        } else {
            0
        });
        s.tick().expect("tick");
        let w = &s.host.world;
        let h = w
            .player_actor_slot
            .and_then(|p| w.actors.get(usize::from(p)))
            .map(|a| a.move_state.render_26)
            .expect("player");
        out.push(h);
        if w.dialog.inline.is_none() {
            break;
        }
    }
    Some(out)
}

#[test]
fn taiku_turns_the_player_to_compass_4_then_toward_the_actor() {
    let Some(h) = headings("taiku", 9, &[0x53B, 0x384], None) else {
        return;
    };
    let at_compass = h
        .iter()
        .position(|&x| x & 0xFFF == 0)
        .expect("the B8 F8 84 0F leg lands on compass 4 (engine heading 0)");
    // A budgeted leg ramps: some heading strictly between the start and the
    // landing.
    assert!(
        h[..at_compass].iter().any(|&x| x != h[0] && x & 0xFFF != 0),
        "the turn ramps over its budget"
    );
    let last = *h.last().unwrap();
    assert_ne!(last & 0xFFF, 0, "the face-at then turns the player again");
}

#[test]
fn jiji_inn_counter_turns_the_player_to_the_keeper() {
    // Inside the counter's box test (`CD F8 58 1B 59 1C`).
    let Some(h) = headings("jiji", 5, &[], Some((88, 28))) else {
        return;
    };
    let start = h[0];
    assert!(
        h.iter().any(|&x| (x - start).rem_euclid(0x1000) > 0x40),
        "CC F8 85 14 00 0C turns the player toward the keeper: {start:#x} held"
    );
}
