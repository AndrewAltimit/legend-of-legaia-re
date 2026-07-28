//! Disc-gated: a cold field entry seats the player **inside the scene's
//! playable area**, and free-roam keeps them there.
//!
//! The defect this pins: on the three kingdom overworlds (`map01` / `map02` /
//! `map03`) the collision grid leaves the **sea** open. Retail can afford
//! that - the coastline is a closed wall ring, the party only ever arrives on
//! the overworld through a door warp onto land, and so the water side is
//! simply unreachable. But the sea is also each map's *largest* connected
//! open-floor region by roughly 4:1, so the engine's cold-spawn resolver
//! (which had no authored seat to use and picked the largest region) put the
//! player offshore with the entire continent walled off behind the coast:
//! free-roam wandered the open water and nothing else was reachable.
//!
//! `World::resolve_cold_field_spawn` now skips a region that reaches three or
//! more of the map's outer edges - the shape the surrounding sea has and no
//! enclosed area of a scene does. The assertions below are behavioural: the
//! spawn stands on raised ground, walking any direction from it stays in the
//! same connected region, and a size-only pick would *not* have (the largest
//! region is bigger than the one the player is in, and is the flat one).
//!
//! Skips silently when `extracted/` or `LEGAIA_DISC_BIN` is missing.

use std::path::PathBuf;

use legaia_engine_core::input::PadButton;
use legaia_engine_core::scene::{DefaultMapIdResolver, SceneHost};
use legaia_engine_core::world::FIELD_COLD_SPAWN_XZ;

/// The three kingdom overworld scenes.
const OVERWORLDS: [&str; 3] = ["map01", "map02", "map03"];

fn extracted_dir() -> Option<PathBuf> {
    for p in ["extracted", "../../extracted"] {
        let d = PathBuf::from(p);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

fn host(extracted: &PathBuf) -> SceneHost {
    let mut host = SceneHost::open_extracted(extracted).expect("open SceneHost");
    host.set_map_resolver(Box::new(DefaultMapIdResolver::from_index(&host.index)));
    host
}

fn player_xz(host: &SceneHost) -> (i16, i16) {
    let ms = &host.world.actors[0].move_state;
    (ms.world_x, ms.world_z)
}

/// The scene's floor-elevation tier under `(x, z)` - the `.MAP` collision
/// byte's low nibble, read with the plain (floor) indexing its retail
/// consumer `FUN_80019278` uses. `0` is sea level.
fn floor_tier(host: &SceneHost, x: i16, z: i16) -> u8 {
    let (tx, tz) = ((x >> 7) as usize, (z >> 7) as usize);
    host.world.field_collision_grid[tz * 0x80 + tx] & 0x0F
}

fn skip() -> bool {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return true;
    }
    false
}

/// A cold entry into a kingdom overworld lands on the continent, and holding
/// any direction from there never leaves it.
#[test]
fn overworld_cold_entry_lands_on_the_continent_and_stays_there() {
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };
    if skip() {
        return;
    }
    let mut host = host(&extracted);

    for scene in OVERWORLDS {
        host.enter_field_scene(scene, 0)
            .unwrap_or_else(|e| panic!("enter {scene}: {e:#}"));
        let (px, pz) = player_xz(&host);
        let region = host.world.field_walk_component_size(px, pz);
        let largest = host.world.field_largest_walk_component_size();

        assert!(
            region > 0,
            "[{scene}] the spawn must be standable open floor"
        );
        // Non-vacuity: the region the player is in is NOT the biggest one, so
        // the old size-only pick would have gone somewhere else - the sea.
        assert!(
            largest > region,
            "[{scene}] the sea is still the largest open region ({largest} \
             sub-cells vs the continent's {region}); if this ever ties, the \
             test has stopped guarding the size-only pick"
        );
        assert!(
            floor_tier(&host, px, pz) > 0,
            "[{scene}] the spawn stands on raised ground, not at sea level \
             (tile {},{})",
            px >> 7,
            pz >> 7
        );

        // Free-roam: hold each direction long enough to cross the whole map
        // and assert the player never stands in the sea. The sea and the
        // continent are disconnected regions, so "the region under the player
        // is the largest one" is exactly "offshore". A zero reading is a
        // player resting pressed against a wall - the locomotion probe's
        // biased read and the spawn test's plain walk-visible read disagree
        // by up to a sub-cell there, which is a standoff, not a position off
        // the map.
        for (label, btn) in [
            ("Up", PadButton::Up),
            ("Down", PadButton::Down),
            ("Right", PadButton::Right),
            ("Left", PadButton::Left),
        ] {
            host.world.actors[0].move_state.world_x = px;
            host.world.actors[0].move_state.world_z = pz;
            for frame in 0..1_500 {
                host.world.set_pad(btn.mask());
                let _ = host.world.tick();
                if frame % 25 != 0 {
                    continue;
                }
                let (ex, ez) = player_xz(&host);
                assert_ne!(
                    host.world.field_walk_component_size(ex, ez),
                    largest,
                    "[{scene}] holding {label} walked offshore to tile \
                     ({},{}) after {frame} frames",
                    ex >> 7,
                    ez >> 7
                );
            }
            host.world.set_pad(0);
        }
        eprintln!(
            "[{scene}] spawn tile ({},{}) tier {} - continent {region} sub-cells, \
             largest (sea) {largest}",
            px >> 7,
            pz >> 7,
            floor_tier(&host, px, pz),
        );
    }
}

/// The blast radius, pinned: the only scenes whose cold spawn is not in their
/// largest open region are the three kingdom overworlds. Every town and
/// dungeon keeps the seat it had, and `town01`'s New Game opening stays at
/// the retail cold-boot coordinate byte-for-byte.
#[test]
fn only_the_overworlds_are_reseated_and_town01_is_byte_identical() {
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };
    if skip() {
        return;
    }
    let mut host = host(&extracted);

    host.enter_field_scene("town01", 0).expect("enter town01");
    let ms = &host.world.actors[0].move_state;
    assert_eq!(
        (ms.world_x, ms.world_y, ms.world_z),
        (FIELD_COLD_SPAWN_XZ, 0, FIELD_COLD_SPAWN_XZ),
        "town01's New Game cold spawn is retail-authored and must not move"
    );

    let mut reseated: Vec<String> = Vec::new();
    let mut checked = 0usize;
    for scene in host.index.cdname_scene_names() {
        if host.enter_field_scene(&scene, 0).is_err() {
            continue;
        }
        if host.world.field_collision_grid.len() < 0x4000
            || host.world.field_object_cells.len() < 0x4000
        {
            continue;
        }
        checked += 1;
        let (px, pz) = player_xz(&host);
        let region = host.world.field_walk_component_size(px, pz);
        if region > 0 && region < host.world.field_largest_walk_component_size() {
            reseated.push(scene);
        }
    }
    assert!(checked > 50, "the sweep must cover the disc's field scenes");
    reseated.sort();
    assert_eq!(
        reseated,
        OVERWORLDS.map(String::from).to_vec(),
        "exactly the three kingdom overworlds spawn outside their largest \
         open region (checked {checked} scenes)"
    );
}

/// `World::seat_player_at_tile` is region-aware: a caller that names a tile
/// the walkability grid does not cover gets the nearest standable spot
/// instead of a player parked inside a wall.
///
/// The concrete case is `map03`'s first encounter region: its AABB centre -
/// the natural seat for "put the player somewhere this region rolls" - is a
/// wall tile, and the player seated there could not take a single step.
#[test]
fn a_seat_on_an_unwalkable_tile_is_rescued() {
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };
    if skip() {
        return;
    }
    let mut host = host(&extracted);
    host.enter_field_scene("map03", 0).expect("enter map03");

    let Some(table) = host
        .world
        .field_region_tracker
        .as_ref()
        .map(|t| t.table().clone())
    else {
        eprintln!("[skip] map03 installed no region tracker on this extracted/");
        return;
    };
    // The first region whose AABB centre is a wall tile - the shape that
    // stranded the seat.
    let walled = table.regions.iter().find_map(|r| {
        let cx = ((r.tile_x_min as u16 + r.tile_x_max as u16) / 2) as u8;
        let cz = ((r.tile_z_min as u16 + r.tile_z_max as u16) / 2) as u8;
        let (wx, wz) = (i16::from(cx) * 128 + 64, i16::from(cz) * 128 + 64);
        host.world.field_tile_is_wall(wx, wz).then_some((cx, cz))
    });
    let Some((cx, cz)) = walled else {
        eprintln!("[skip] no map03 region centre lands in a wall on this extracted/");
        return;
    };

    host.world.seat_player_at_tile(cx, cz);
    let (sx, sz) = player_xz(&host);
    assert!(
        !host.world.field_tile_is_wall(sx, sz),
        "the rescued seat must be standable (asked tile {cx},{cz})"
    );

    // ...and the player can actually walk from it. Before the rescue every
    // direction was wall-blocked and this displacement was exactly zero.
    let mut moved = false;
    for btn in [
        PadButton::Up,
        PadButton::Down,
        PadButton::Left,
        PadButton::Right,
    ] {
        host.world.actors[0].move_state.world_x = sx;
        host.world.actors[0].move_state.world_z = sz;
        for _ in 0..60 {
            host.world.set_pad(btn.mask());
            let _ = host.world.tick();
        }
        host.world.set_pad(0);
        let (ex, ez) = player_xz(&host);
        moved |= (ex, ez) != (sx, sz);
    }
    assert!(
        moved,
        "a rescued seat must leave the player able to move (tile {},{})",
        sx >> 7,
        sz >> 7
    );
}
