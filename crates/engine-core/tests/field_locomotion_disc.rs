//! Disc-gated verification of the field free-movement locomotion against
//! real scene data. Boots a real field scene and drives the player with a
//! synthetic pad stream through the same per-frame `World::tick` path
//! `play-window` uses, asserting the player advances on the correct world
//! axes and that movement is deterministic across two identical runs.
//!
//! Note on collision: the per-scene wall grid is painted by the field-VM
//! `0x4C` nibble-7 op as the scene's live event script runs. On the
//! currently-loaded record-0 prologue the VM halts at its first op, and
//! the wall paints for these scenes live elsewhere (town01: record 62 on
//! disc; map03: runtime-projected from the field-pack preamble, not on
//! disc at all). So the grid stays empty here and there is nothing to
//! clip into - this test verifies the movement half end-to-end and
//! reports the painted-wall count for visibility. The collision math
//! itself is covered by the synthetic-grid unit tests in `world.rs`.
//!
//! Skips silently when `extracted/` or `LEGAIA_DISC_BIN` is missing - CI
//! runs without disc data.

use std::path::PathBuf;

use legaia_engine_core::input::PadButton;
use legaia_engine_core::scene::{DefaultMapIdResolver, SceneHost};
use legaia_engine_core::world::SceneMode;

fn extracted_dir() -> Option<PathBuf> {
    for p in ["extracted", "../../extracted"] {
        let d = PathBuf::from(p);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

fn wall_byte_count(grid: &[u8]) -> usize {
    grid.iter().filter(|b| **b & 0xF0 != 0).count()
}

/// Drive `scene` for `frames` of held `btn` and return the player's net
/// (x, z) displacement.
fn walk(host: &mut SceneHost, btn: PadButton, frames: usize) -> (i32, i32) {
    let before = {
        let ms = &host.world.actors[0].move_state;
        (ms.world_x as i32, ms.world_z as i32)
    };
    for _ in 0..frames {
        host.world.set_pad(btn.mask());
        let _ = host.world.tick();
    }
    let ms = &host.world.actors[0].move_state;
    (ms.world_x as i32 - before.0, ms.world_z as i32 - before.1)
}

fn verify_scene(host: &mut SceneHost, scene: &str) {
    host.enter_field_scene(scene, 0)
        .unwrap_or_else(|e| panic!("enter_field_scene('{scene}') failed: {e:#}"));
    assert!(matches!(host.world.mode, SceneMode::Field));
    assert_eq!(
        host.world.player_actor_slot,
        Some(0),
        "field entry installs the party leader as the player"
    );

    // Let any prescript ops run (one field-VM op per tick).
    for _ in 0..4_000 {
        host.world.set_pad(0);
        let _ = host.world.tick();
    }
    eprintln!(
        "[{scene}] collision-grid wall tiles after prescript: {}",
        wall_byte_count(&host.world.field_collision_grid)
    );

    // Locomotion: each direction moves the player on the expected world
    // axis (camera azimuth 0: Up=+Z, Down=-Z, Right=+X, Left=-X).
    host.world.actors[0].move_state.world_x = 1000;
    host.world.actors[0].move_state.world_z = 1000;
    let up = walk(host, PadButton::Up, 20);
    assert!(
        up.1 > 0 && up.0 == 0,
        "[{scene}] Up should move +Z only, got {up:?}"
    );
    let down = walk(host, PadButton::Down, 20);
    assert!(
        down.1 < 0 && down.0 == 0,
        "[{scene}] Down should move -Z only, got {down:?}"
    );
    let right = walk(host, PadButton::Right, 20);
    assert!(
        right.0 > 0 && right.1 == 0,
        "[{scene}] Right should move +X only, got {right:?}"
    );
    let left = walk(host, PadButton::Left, 20);
    assert!(
        left.0 < 0 && left.1 == 0,
        "[{scene}] Left should move -X only, got {left:?}"
    );
    eprintln!("[{scene}] locomotion OK: up={up:?} down={down:?} left={left:?} right={right:?}");
}

#[test]
fn field_locomotion_drives_player_on_real_scene() {
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }
    let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    host.set_map_resolver(Box::new(DefaultMapIdResolver::from_index(&host.index)));

    for scene in ["town01", "map03"] {
        verify_scene(&mut host, scene);
    }
}

/// Same pad stream twice -> bit-identical player trajectory on real data.
#[test]
fn field_locomotion_deterministic_on_real_scene() {
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }
    let pads = [
        PadButton::Up.mask(),
        PadButton::Up.mask() | PadButton::Right.mask(),
        PadButton::Right.mask(),
        PadButton::Down.mask(),
        PadButton::Left.mask(),
        0,
        PadButton::Up.mask(),
    ];
    let run = || -> (i16, i16) {
        let mut host = SceneHost::open_extracted(extracted.as_path()).expect("open SceneHost");
        host.set_map_resolver(Box::new(DefaultMapIdResolver::from_index(&host.index)));
        host.enter_field_scene("town01", 0).expect("enter town01");
        host.world.actors[0].move_state.world_x = 1500;
        host.world.actors[0].move_state.world_z = 1500;
        for &p in pads.iter().cycle().take(120) {
            host.world.set_pad(p);
            let _ = host.world.tick();
        }
        let ms = &host.world.actors[0].move_state;
        (ms.world_x, ms.world_z)
    };
    assert_eq!(
        run(),
        run(),
        "identical pad stream is bit-identical on real scene"
    );
}
