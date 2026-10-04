//! Disc-gated regression: **the player can step off Octam's gondola after
//! the first-arrival cutscene.**
//!
//! `ropeway` `P2[6]` (the arrival at Octam) seats the player on the gondola's
//! footprint (`A3 F8 24 1F`, tile `(36, 31)`) and ends there. The gondola is
//! a window-sweep placement (`FUN_801D7B50`, the `+0x24` actor list), which
//! no collision routine reads - the candidate gather `FUN_801CF754` walks
//! only the scene-init list at `+0x0C`. Giving it a solid box boxed the
//! player in on all four probes once the cutscene ended.
//!
//! The test installs `P2[6]` directly, plays it to its end with a confirm
//! press on every other frame (so the length of its waits and dialogue does
//! not matter), checks the player landed on tile `(36, 31)`, then re-seats
//! the player there once per direction and asserts that holding that
//! direction moves it off the tile. The soak fixture
//! `scripts/replays/soak/fixed/ropeway_player_parked_on_gondola.replay.toml`
//! guards the same defect through a recorded pad stream; this test does not
//! depend on when the cutscene ends.
//!
//! Skip-passes without `LEGAIA_DISC_BIN` / `extracted/`.

use std::path::PathBuf;

use legaia_engine_core::input::PadButton;
use legaia_engine_core::scene::SceneHost;

fn extracted_dir() -> Option<PathBuf> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    let d = std::env::var_os("LEGAIA_EXTRACTED_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../..")
                .join("extracted")
        });
    if d.join("PROT.DAT").is_file() {
        Some(d)
    } else {
        eprintln!("[skip] extracted/ missing - run legaia-extract first");
        None
    }
}

fn player_pos(host: &SceneHost) -> (i16, i16, i16) {
    let w = &host.world;
    let a = &w.actors[w.player_actor_slot.expect("player slot") as usize];
    (
        a.move_state.world_x,
        a.move_state.world_y,
        a.move_state.world_z,
    )
}

/// The field tile a world position falls in (`(v - 0x40) >> 7`, the
/// placement grid the `A3` seat op addresses).
fn tile(x: i16, z: i16) -> (i16, i16) {
    ((x - 0x40) >> 7, (z - 0x40) >> 7)
}

/// The gondola seat `P2[6]` ends on: `A3 F8 24 1F`.
const SEAT_TILE: (i16, i16) = (0x24, 0x1F);

/// Frames the cutscene may take, waits and dialogue included.
const CUTSCENE_CAP: usize = 30_000;

#[test]
fn ropeway_arrival_player_steps_off_gondola() {
    let Some(extracted) = extracted_dir() else {
        return;
    };
    let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    host.enter_field_scene("ropeway", 0).expect("enter ropeway");
    for _ in 0..90 {
        let _ = host.tick();
    }
    let man = host
        .world
        .field_vm
        .channels_man
        .clone()
        .expect("ropeway's MAN is seeded on scene entry");
    let man_file = legaia_asset::man_section::parse(&man).expect("parse ropeway MAN");
    host.world.cutscene.timeline = None;
    assert!(
        host.world
            .install_cutscene_timeline_record(&man_file, &man, 2, 6, false),
        "ropeway P2[6] resolves to a record"
    );

    // Play the record out; the confirm press pages its dialogue.
    let mut frames = 0;
    while host.world.cutscene.timeline.is_some() && frames < CUTSCENE_CAP {
        let pad = if frames % 2 == 0 {
            PadButton::Cross.mask()
        } else {
            0
        };
        host.world.set_pad(pad);
        let _ = host.tick();
        frames += 1;
    }
    host.world.set_pad(0);
    assert!(
        host.world.cutscene.timeline.is_none(),
        "P2[6] still running after {CUTSCENE_CAP} frames"
    );
    for _ in 0..30 {
        let _ = host.tick();
    }
    let seat = player_pos(&host);
    eprintln!("[ropeway] P2[6] ended after {frames} frames, player at {seat:?}");
    assert_eq!(
        tile(seat.0, seat.2),
        SEAT_TILE,
        "P2[6] leaves the player on the gondola seat"
    );
    assert!(
        host.world.dialog.inline.is_none() && host.world.dialog.current.is_none(),
        "no dialogue is left open over free roam"
    );

    for d in [
        PadButton::Up,
        PadButton::Right,
        PadButton::Down,
        PadButton::Left,
    ] {
        {
            let w = &mut host.world;
            let slot = w.player_actor_slot.expect("player slot") as usize;
            let ms = &mut w.actors[slot].move_state;
            (ms.world_x, ms.world_y, ms.world_z) = seat;
        }
        for _ in 0..60 {
            host.world.set_pad(d.mask());
            let _ = host.tick();
        }
        host.world.set_pad(0);
        for _ in 0..4 {
            let _ = host.tick();
        }
        let at = player_pos(&host);
        eprintln!("[ropeway] {d:?} from {seat:?} -> {at:?}");
        assert!(
            host.world.cutscene.timeline.is_none(),
            "stepping {d:?} off the gondola started a cutscene"
        );
        assert_ne!(
            tile(at.0, at.2),
            SEAT_TILE,
            "holding {d:?} for 60 frames left the player on the gondola seat ({at:?})"
        );
    }
}
