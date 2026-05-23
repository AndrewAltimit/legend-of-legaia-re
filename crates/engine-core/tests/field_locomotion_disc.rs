//! Disc-gated verification of the field free-movement locomotion against
//! real scene data. Boots a real field scene and drives the player with a
//! synthetic pad stream through the same per-frame `World::tick` path
//! `play-window` uses, asserting the player advances on the correct world
//! axes and that movement is deterministic across two identical runs.
//!
//! Collision: the per-scene base walkable grid is loaded from the field
//! map file (`DATA\FIELD\<scene>.MAP`, the unique 0x12000-byte block entry)
//! by `enter_field_scene` - its `+0x4000..+0x8000` region copies verbatim
//! into the collision grid (verified byte-exact against live RAM for
//! town01). The field-VM `0x4C` nibble-7 ops layer story-conditional deltas
//! on top as the prescript runs. This test asserts the base grid loads
//! non-empty, then drives the player from a known-walkable spawn and checks
//! each d-pad axis. The collision math itself is covered by the
//! synthetic-grid unit tests in `world.rs`.
//!
//! Skips silently when `extracted/` or `LEGAIA_DISC_BIN` is missing - CI
//! runs without disc data.

use std::path::PathBuf;

use legaia_engine_core::input::PadButton;
use legaia_engine_core::scene::{DefaultMapIdResolver, SceneHost};
use legaia_engine_core::world::{FIELD_COLD_SPAWN_XZ, SceneMode};

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

/// Find a tile whose 5x5 neighbourhood is fully walkable and return its
/// world-space centre `(x, z)`. Gives the locomotion checks room to move
/// ~1 tile on each axis without clipping a real wall. Falls back to a fixed
/// coordinate if the grid is empty / no open block exists.
fn open_spawn(grid: &[u8]) -> (i16, i16) {
    const STRIDE: usize = 0x80;
    if grid.len() >= STRIDE * STRIDE {
        for row in 2usize..STRIDE - 2 {
            for col in 2usize..STRIDE - 2 {
                let open = (row - 2..=row + 2)
                    .all(|r| (col - 2..=col + 2).all(|c| grid[r * STRIDE + c] & 0xF0 == 0));
                if open {
                    return ((col * 128 + 64) as i16, (row * 128 + 64) as i16);
                }
            }
        }
    }
    (1000, 1000)
}

/// Find a walkable tile with a solid wall two tiles to the east, so walking
/// `+X` into it is reliably blocked. Returns the walkable tile's world centre.
fn wall_to_east(grid: &[u8]) -> Option<(i16, i16)> {
    const STRIDE: usize = 0x80;
    if grid.len() < STRIDE * STRIDE {
        return None;
    }
    for row in 0usize..STRIDE {
        for col in 1usize..STRIDE - 2 {
            let walkable = grid[row * STRIDE + col] & 0xF0 == 0;
            let wall_east = grid[row * STRIDE + col + 1] & 0xF0 == 0xF0
                && grid[row * STRIDE + col + 2] & 0xF0 == 0xF0;
            if walkable && wall_east {
                return Some(((col * 128 + 64) as i16, (row * 128 + 64) as i16));
            }
        }
    }
    None
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

    // The base walkable grid is loaded from the scene's `.MAP` file at
    // entry (before any tick). Every field/town scene carries one.
    let base_walls = wall_byte_count(&host.world.field_collision_grid);
    eprintln!("[{scene}] base collision-grid wall tiles (from .MAP): {base_walls}");
    assert!(
        base_walls > 0,
        "[{scene}] expected a non-empty base collision grid loaded from the .MAP file"
    );

    // Pick a known-walkable spawn from the loaded grid so the axis checks
    // have room to move without clipping a real wall.
    let (sx, sz) = open_spawn(&host.world.field_collision_grid);

    // Whether this scene runs a real MAN-resolved scene-entry system
    // script (kingdom-bundle scenes) rather than falling back to event-
    // script record 0 (which is a trigger table and halts the VM at pc 0).
    let has_man_entry = host
        .scene
        .as_ref()
        .and_then(|s| s.field_man_entry_script(&host.index).ok().flatten())
        .is_some();

    // Let any prescript ops run (one field-VM op per tick) and track how
    // many distinct PCs the field VM visits.
    let mut visited = std::collections::BTreeSet::new();
    for _ in 0..4_000 {
        host.world.set_pad(0);
        let _ = host.world.tick();
        visited.insert(host.world.field_pc);
    }
    eprintln!(
        "[{scene}] collision-grid wall tiles after prescript: {} (man_entry={has_man_entry}, distinct field PCs visited={})",
        wall_byte_count(&host.world.field_collision_grid),
        visited.len()
    );
    if has_man_entry {
        assert!(
            visited.len() > 1,
            "[{scene}] MAN-backed scene should run its entry script (the VM \
             must advance past pc 0), but the field VM stayed at a single PC"
        );
    }

    // Locomotion: each direction moves the player on the expected world
    // axis (camera azimuth 0: Up=+Z, Down=-Z, Right=+X, Left=-X).
    host.world.actors[0].move_state.world_x = sx;
    host.world.actors[0].move_state.world_z = sz;
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

    // Walls work: place the player one tile west of a real base wall and
    // walk hard into it. In open space 40 frames travels ~320 units; the
    // base wall (loaded from the .MAP) must stop the player well short.
    if let Some((wx, wz)) = wall_to_east(&host.world.field_collision_grid) {
        host.world.actors[0].move_state.world_x = wx;
        host.world.actors[0].move_state.world_z = wz;
        let into_wall = walk(host, PadButton::Right, 40);
        assert!(
            into_wall.0 < 200,
            "[{scene}] base wall should block +X movement, got {into_wall:?}"
        );
        eprintln!(
            "[{scene}] base wall blocks: walked +X only {} units into wall",
            into_wall.0
        );
    }
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

/// A cold field entry (the New Game opening path) drops the player at the
/// retail `FUN_801D6704` cold-boot spawn `(0xA40, 0, 0xA40)`, and that spawn
/// is a standable tile in real town01 (Rim Elm) - not inside a base-grid wall.
/// This pins Vahn's opening position so the engine no longer leaves him at the
/// `(0, 0)` map corner.
#[test]
fn cold_field_entry_spawns_player_at_authored_walkable_position() {
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
    host.enter_field_scene("town01", 0).expect("enter town01");

    let ms = &host.world.actors[0].move_state;
    assert_eq!(
        (ms.world_x, ms.world_y, ms.world_z),
        (FIELD_COLD_SPAWN_XZ, 0, FIELD_COLD_SPAWN_XZ),
        "cold field entry must place the player at the retail cold-boot spawn"
    );
    assert!(
        !host
            .world
            .field_tile_is_wall(FIELD_COLD_SPAWN_XZ, FIELD_COLD_SPAWN_XZ),
        "the cold-boot spawn tile must be walkable in town01"
    );
    eprintln!(
        "[town01] cold-boot spawn ({FIELD_COLD_SPAWN_XZ}, 0, {FIELD_COLD_SPAWN_XZ}) is walkable"
    );
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
