//! Disc-gated verification of the cold field-entry spawn resolver.
//!
//! A cold (non-warp) field entry - the scene picker / any non-door entry -
//! used to seat the player at the fixed retail camera-window centre
//! `FIELD_COLD_SPAWN_XZ` (`0xA40`, `0xA40`). That coordinate is only Vahn's
//! authored Rim Elm (`town01`) spawn; for most other scenes it lands off the
//! authored walkable floor (in a wall or in the surrounding void), so the
//! player spawned out of bounds. `World::resolve_cold_field_spawn` now keeps
//! the retail seat when it is genuinely standable (town01, and any scene whose
//! `0xA40` centre is on the walkable floor) and otherwise relocates onto the
//! walkable tile nearest the scene's own playable-floor centroid.
//!
//! This test boots a spread of real scenes - a town (`town01`), a village
//! (`geremi`), an underground dungeon (`cave01`), and a tower interior
//! (`tower`) - and asserts the resolved spawn is in-bounds and standable
//! (on the `.MAP` walk-visible floor AND clear of the collision-grid wall
//! bits), while `town01`'s spawn stays byte-identical to the pinned retail
//! seat.
//!
//! Skips silently when `extracted/` or `LEGAIA_DISC_BIN` is missing - CI runs
//! without disc data.

use std::path::PathBuf;

use legaia_engine_core::scene::{DefaultMapIdResolver, SceneHost};
use legaia_engine_core::world::FIELD_COLD_SPAWN_XZ;

fn extracted_dir() -> Option<PathBuf> {
    for p in ["extracted", "../../extracted"] {
        let d = PathBuf::from(p);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

const STRIDE: usize = 0x80;
const WALK_VIS: u16 = 0x1000;

fn open_host() -> Option<SceneHost> {
    let extracted = extracted_dir()?;
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return None;
    }
    let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    host.set_map_resolver(Box::new(DefaultMapIdResolver::from_index(&host.index)));
    Some(host)
}

/// The player's current spawn world `(x, z)`.
fn spawn_xz(host: &SceneHost) -> (i16, i16) {
    let ms = &host.world.actors[0].move_state;
    (ms.world_x, ms.world_z)
}

/// Count of walk-visible floor cells in the loaded object grid.
fn walk_visible_floor(host: &SceneHost) -> usize {
    host.world
        .field_object_cells
        .iter()
        .filter(|c| **c & WALK_VIS != 0)
        .count()
}

/// Cold entry into each scene lands the player on an in-bounds, standable
/// tile - on the `.MAP` walk-visible floor and clear of the collision walls -
/// never off-grid or inside a wall.
#[test]
fn cold_field_entry_spawn_is_in_bounds_for_a_spread_of_scenes() {
    let Some(mut host) = open_host() else {
        eprintln!("[skip] extracted/ or disc missing");
        return;
    };

    // town (default seat), village (relocated), underground dungeon
    // (relocated), tower interior (relocated).
    for scene in ["town01", "geremi", "cave01", "tower"] {
        host.enter_field_scene(scene, 0)
            .unwrap_or_else(|e| panic!("enter_field_scene('{scene}') failed: {e:#}"));

        assert!(
            host.world.field_collision_grid.len() >= STRIDE * STRIDE,
            "[{scene}] expected a loaded collision grid"
        );
        let floor = walk_visible_floor(&host);
        assert!(
            floor > 0,
            "[{scene}] expected a non-empty walk-visible floor from the .MAP object grid"
        );

        let (x, z) = spawn_xz(&host);
        assert!(
            host.world.field_tile_is_walk_visible(x, z),
            "[{scene}] cold spawn ({x},{z}) must be on the authored walkable floor, not off-map void"
        );
        assert!(
            !host.world.field_tile_is_wall(x, z),
            "[{scene}] cold spawn ({x},{z}) must not be inside a collision wall"
        );
        eprintln!(
            "[{scene}] cold spawn ({x},{z}) is in-bounds + standable (walk-visible floor tiles: {floor})"
        );
    }
}

/// town01's cold spawn stays byte-identical to the pinned retail seat
/// `(0xA40, 0, 0xA40)` - the resolver must not regress the New Game opening.
#[test]
fn town01_cold_spawn_is_unchanged() {
    let Some(mut host) = open_host() else {
        eprintln!("[skip] extracted/ or disc missing");
        return;
    };
    host.enter_field_scene("town01", 0).expect("enter town01");

    let ms = &host.world.actors[0].move_state;
    assert_eq!(
        (ms.world_x, ms.world_y, ms.world_z),
        (FIELD_COLD_SPAWN_XZ, 0, FIELD_COLD_SPAWN_XZ),
        "town01 cold entry must keep the retail cold-boot spawn exactly"
    );
    assert!(
        host.world
            .field_tile_is_walk_visible(FIELD_COLD_SPAWN_XZ, FIELD_COLD_SPAWN_XZ)
            && !host
                .world
                .field_tile_is_wall(FIELD_COLD_SPAWN_XZ, FIELD_COLD_SPAWN_XZ),
        "the town01 cold-boot seat must itself be a standable tile (why the resolver keeps it)"
    );
    eprintln!(
        "[town01] cold spawn ({FIELD_COLD_SPAWN_XZ}, 0, {FIELD_COLD_SPAWN_XZ}) unchanged + standable"
    );
}

/// The resolver is non-destructive when it can't do better: a scene entered
/// twice resolves to the same spawn, and a relocated scene's spawn differs
/// from the fixed retail seat (proving the relocation actually fires).
#[test]
fn relocated_scene_spawn_is_deterministic_and_moved() {
    let Some(mut host) = open_host() else {
        eprintln!("[skip] extracted/ or disc missing");
        return;
    };

    host.enter_field_scene("geremi", 0).expect("enter geremi");
    let first = spawn_xz(&host);
    host.enter_field_scene("geremi", 0)
        .expect("re-enter geremi");
    let second = spawn_xz(&host);
    assert_eq!(
        first, second,
        "geremi cold spawn must resolve deterministically across re-entries"
    );
    assert_ne!(
        first,
        (FIELD_COLD_SPAWN_XZ, FIELD_COLD_SPAWN_XZ),
        "geremi's fixed 0xA40 seat is off-floor, so the resolver must relocate it"
    );
    eprintln!("[geremi] relocated cold spawn {first:?} is deterministic");
}
