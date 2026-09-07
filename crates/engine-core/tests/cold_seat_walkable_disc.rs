//! Every CDNAME scene's **cold seat** is a place the player can stand.
//!
//! A cold field entry is the one seat retail authors as a constant: the field
//! initialiser (`FUN_801D6704`) puts the actor at the camera-window centre
//! [`FIELD_COLD_SPAWN_XZ`] (`0xA40`) when the warp globals are clear. Retail
//! only ever takes that path for the New Game opening (`town01`, where `0xA40`
//! is Vahn's authored Rim Elm spawn); every other arrival is a warp and carries
//! its own `entry_x` / `entry_z`. The engine's scene picker, the asset viewer
//! and every breadth oracle in this crate enter arbitrary scenes cold, so the
//! port resolves the constant against the scene's own grids
//! ([`World::resolve_cold_field_spawn`]).
//!
//! This test is the disc-wide statement of what that resolution must deliver,
//! one row per CDNAME scene:
//!
//! 1. the seat is on the scene's authored floor and clear of the collision
//!    grid's wall bits, **or**
//! 2. it is exactly the retail constant, which is the resolver's documented
//!    last rule for a scene whose grids offer nothing better; and
//! 3. the seat is not walled in on all four sides - a seat the player cannot
//!    walk off is not a seat, whichever rule produced it.
//!
//! Two disc scenes take rule 2. `dream` (the chapter hub) ships an object grid
//! with exactly one non-zero cell and no floor bit at all, and `other4` ships
//! none; neither is boxed, so both are seats a player can use.
//!
//! ## Why the floor test is per-scene
//!
//! The `.MAP` object grid carries two draw-gate bits on the same `u16`:
//! `CELL_WALK_VISIBLE` (`0x1000`, the walk view's) and `CELL_VISIBLE`
//! (`0x2000`, the overhead one's). Most scenes set `0x1000` on every tile the
//! party may stand on - but eighteen field scenes author `0x2000` and never
//! `0x1000`, so a fixed `0x1000` gate reads them as having no floor at all and
//! makes the whole resolver inert there. `World::field_floor_cell_bit` picks
//! the bit each scene actually authored; this test is what pins that the
//! choice covers the disc rather than the sample that was looked at.
//!
//! Skip-pass (CLAUDE.md disc-gated convention): `LEGAIA_DISC_BIN` unset or
//! `extracted/` missing.

use std::path::PathBuf;

use legaia_engine_core::scene::{DefaultMapIdResolver, SceneHost, is_world_map_scene};
use legaia_engine_core::world::FIELD_COLD_SPAWN_XZ;

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

fn open_host() -> Option<SceneHost> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    let extracted = extracted_dir()?;
    let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    host.set_map_resolver(Box::new(DefaultMapIdResolver::from_index(&host.index)));
    if let Ok(scus) = std::fs::read(extracted.join("SCUS_942.54"))
        && let Some(party) = legaia_asset::new_game::StartingParty::from_scus(&scus)
    {
        host.world.seed_starting_party(&party);
    }
    Some(host)
}

#[test]
fn every_scene_cold_seat_is_standable_or_the_retail_seat() {
    let Some(mut host) = open_host() else {
        return;
    };
    let mut names = host.index.cdname_scene_names();
    names.sort();
    names.dedup();

    let mut entered = 0usize;
    let mut on_floor = 0usize;
    let mut retail_seat_fallback: Vec<String> = Vec::new();
    let mut off_floor_and_moved: Vec<String> = Vec::new();
    let mut boxed_in: Vec<String> = Vec::new();

    for name in &names {
        let ok = if is_world_map_scene(name) {
            host.enter_world_map_scene(name).is_ok()
        } else {
            host.enter_field_scene(name, 0).is_ok()
        };
        if !ok {
            // Not every CDNAME block is a scene (data banks, sound tables).
            continue;
        }
        entered += 1;
        let slot = host.world.player_actor_slot.unwrap_or(0) as usize;
        let ms = &host.world.actors[slot].move_state;
        let (x, z) = (ms.world_x, ms.world_z);
        let standable =
            host.world.field_tile_is_walk_visible(x, z) && !host.world.field_tile_is_wall(x, z);
        let is_retail_seat = x == FIELD_COLD_SPAWN_XZ && z == FIELD_COLD_SPAWN_XZ;
        if standable {
            on_floor += 1;
        } else if is_retail_seat {
            retail_seat_fallback.push(name.clone());
        } else {
            off_floor_and_moved.push(format!("{name} @ ({x}, {z})"));
        }
        if (0..4).all(|d| host.world.field_dir_blocked(x, z, d)) {
            boxed_in.push(format!("{name} @ ({x}, {z})"));
        }
    }

    eprintln!(
        "[ok] cold seats: {entered} scene(s) entered; {on_floor} on the authored floor; \
         {} on the retail seat with no floor record ({:?}); {} boxed",
        retail_seat_fallback.len(),
        retail_seat_fallback,
        boxed_in.len()
    );

    assert!(
        entered > 90,
        "the sweep must enter scenes; entered {entered}"
    );
    // Rule 1 / 2: a seat is either on the scene's authored floor, or it is the
    // retail constant the resolver falls back to when the scene records no
    // floor at all. A seat that is neither means the resolver MOVED the player
    // somewhere the scene does not call floor, which is a defect in the pick.
    assert!(
        off_floor_and_moved.is_empty(),
        "cold seat resolved off the authored floor and away from the retail seat: {off_floor_and_moved:?}"
    );
    // Rule 3: whichever rule produced it, the player can walk off it.
    assert!(
        boxed_in.is_empty(),
        "cold seat walled in on all four sides: {boxed_in:?}"
    );
}
