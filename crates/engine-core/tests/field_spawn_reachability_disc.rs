//! Disc-gated sweep of the cold field-entry spawn across every CDNAME field
//! scene: the chosen spawn must be standable AND **reachable** - i.e. inside
//! a reasonably large connected component of the walkability lattice, not a
//! walled-off pocket.
//!
//! The failure classes this pins ("stuck in an invisible wall"):
//!
//! - a spawn can pass the point tests (on the walk-visible floor, clear of
//!   the wall bits) while sitting in a walled-off pocket or a secondary
//!   region cut off from the scene's main playable area;
//! - a spawn on a `.MAP` kind-0 intra-scene teleport tile (a door pad) is
//!   warped by the first tile-crossing dispatch;
//! - a scene's entry script can spawn a C1-gated first-visit record whose
//!   choreography `MoveTo`s the PLAYER into a spot the base collision grid
//!   walls in (izumi's spring: retail plays the modal cutscene there and
//!   leaves the scene through the record's closing `0x3F`; the engine's
//!   concurrent rendition can strand the player instead - the
//!   stranded-player rescue in `World::step_helper_contexts` re-seats them).
//!
//! `World::resolve_cold_field_spawn` keeps the retail `0xA40` seat only when
//! it is standable, inside the scene's largest connected open-floor
//! component, and not a kind-0 teleport tile; otherwise it relocates to a
//! kind-0 door-arrival anchor in that component or to the component's
//! centroid.
//!
//! Reachability measure: `World::field_walk_component_size` - the 4-connected
//! flood-fill size, in 64-unit sub-cells (the wall-bit granularity, four per
//! 128-unit tile), of the open lattice around a world point.
//!
//! Skips silently when `extracted/` or `LEGAIA_DISC_BIN` is missing - CI runs
//! without disc data.

use std::path::PathBuf;

use legaia_engine_core::input::PadButton;
use legaia_engine_core::scene::{DefaultMapIdResolver, SceneHost, is_world_map_scene};
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

/// Minimum connected-component size (in 64-unit sub-cells; 4 per tile) for a
/// spawn to count as "room to walk". 80 sub-cells = 20 tiles of open floor.
/// A scene whose *largest* component is smaller than this is genuinely tiny
/// (e.g. `juui1`, whose base grid walls everything but a single tile until
/// scripts repaint it) - for those the spawn must simply be in that largest
/// component.
const MIN_COMPONENT_SUBCELLS: usize = 80;

/// The player's current world `(x, z)`.
fn player_xz(host: &SceneHost) -> (i16, i16) {
    let ms = &host.world.actors[0].move_state;
    (ms.world_x, ms.world_z)
}

/// Every CDNAME field scene that boots with a walkability grid spawns the
/// player on a standable tile whose connected walkable region is the scene's
/// largest (or at least [`MIN_COMPONENT_SUBCELLS`]), and 30 idle ticks later -
/// after the scene-entry script ran and the walk-on trigger dispatch had its
/// chance to fire - the player still stands in such a region (no first-tick
/// teleport into a walled pocket).
#[test]
fn cold_spawn_is_reachable_across_all_field_scenes() {
    let Some(mut host) = open_host() else {
        eprintln!("[skip] extracted/ or disc missing");
        return;
    };
    let extracted = extracted_dir().unwrap();
    let cdname = legaia_prot::cdname::parse(&extracted.join("CDNAME.TXT")).expect("parse cdname");
    let mut names: Vec<String> = cdname.values().cloned().collect();
    names.sort();
    names.dedup();

    let mut swept = 0usize;
    let mut failures: Vec<String> = Vec::new();
    for name in &names {
        if is_world_map_scene(name) {
            continue;
        }
        if host.enter_field_scene(name, 0).is_err() {
            // Not a bootable field scene (battle stages, sound banks, ...).
            continue;
        }
        if !matches!(host.world.mode, SceneMode::Field) {
            continue;
        }
        let has_floor = host
            .world
            .field_object_cells
            .iter()
            .any(|c| *c & legaia_asset::field_objects::CELL_WALK_VISIBLE != 0);
        if !has_floor || host.world.field_collision_grid.len() < 0x80 * 0x80 {
            // No walkability data (cutscene-only shells like `dream` /
            // `kor*`): the resolver keeps the retail seat, nothing to sweep.
            continue;
        }
        swept += 1;

        let largest = host.world.field_largest_walk_component_size();
        let ok_component = |comp: usize| comp >= MIN_COMPONENT_SUBCELLS.min(largest) && comp > 0;

        let (x0, z0) = player_xz(&host);
        let comp0 = host.world.field_walk_component_size(x0, z0);
        if !host.world.field_tile_is_walk_visible(x0, z0) || host.world.field_tile_is_wall(x0, z0) {
            failures.push(format!("[{name}] spawn ({x0},{z0}) is not standable"));
            continue;
        }
        if !ok_component(comp0) {
            failures.push(format!(
                "[{name}] spawn ({x0},{z0}) component {comp0} < {} (largest {largest})",
                MIN_COMPONENT_SUBCELLS.min(largest)
            ));
            continue;
        }

        // Idle ticks: the entry script runs (incl. its 0x4C collision
        // repaints and any op-0x44 record spawn - izumi's first-visit record
        // `MoveTo`s the player to the spring), then keep ticking until every
        // spawned helper context drains (they frame-cap at
        // `CUTSCENE_TIMELINE_MAX_FRAMES`), so the stranded-player rescue has
        // run. The player must end standing clear of the walls.
        let mut budget = 1500usize;
        loop {
            let _ = host.tick();
            budget -= 1;
            if budget == 0
                || !matches!(host.world.mode, SceneMode::Field)
                || (budget <= 1470 && host.world.helper_contexts.is_empty())
            {
                break;
            }
        }
        if !matches!(host.world.mode, SceneMode::Field) {
            continue; // a scripted transition took over; nothing to assert
        }
        let (x1, z1) = player_xz(&host);
        let comp1 = host.world.field_walk_component_size(x1, z1);
        if comp1 == 0 {
            failures.push(format!(
                "[{name}] after the entry scripts drained the player is at ({x1},{z1}) \
                 inside a wall / off the floor (spawned at ({x0},{z0}) component {comp0}, \
                 largest {largest})"
            ));
        }
    }
    eprintln!("swept {swept} field scenes");
    assert!(swept > 50, "expected a real corpus sweep, got {swept}");
    assert!(
        failures.is_empty(),
        "unreachable / enclosed spawns:\n{}",
        failures.join("\n")
    );
}

/// izumi regression ("stuck in an invisible wall"): the scene's entry script
/// spawns its first-visit record (P2[3], C1-gated on the clear flag it
/// self-latches via `SysFlag.Set 0x266`), whose choreography `MoveTo`s the
/// player into the spring pocket - a spot the base collision grid walls in.
/// Retail plays the whole modal cutscene there and exits through the record's
/// closing `0x3F`; the engine's concurrent rendition stranded the player. The
/// spawn resolver must seat the cold spawn in the largest walkable component,
/// and once the partially-executed record drains, the stranded-player rescue
/// must leave the player somewhere pad input actually works.
#[test]
fn izumi_spawn_is_walkable_and_pad_moves_the_player() {
    let Some(mut host) = open_host() else {
        eprintln!("[skip] extracted/ or disc missing");
        return;
    };
    host.enter_field_scene("izumi", 0).expect("enter izumi");

    let (x0, z0) = player_xz(&host);
    let comp = host.world.field_walk_component_size(x0, z0);
    let largest = host.world.field_largest_walk_component_size();
    assert_eq!(
        comp, largest,
        "izumi spawn ({x0},{z0}) must sit in the largest walkable component"
    );
    assert!(
        comp >= MIN_COMPONENT_SUBCELLS,
        "izumi's main component should be large, got {comp}"
    );

    // Let the entry script run its course: it spawns the first-visit record,
    // which parks the player at the spring for its choreography; when the
    // record drains (completion or frame cap) the rescue re-seats the player.
    // Budget in SIM ticks. Spawned-record contexts step on the 60 Hz
    // retail-frame sub-clock, so ~1.67 sim ticks buy one record frame - the
    // budget has to cover the record's frame cap in display frames.
    let mut budget = 2600usize;
    loop {
        let _ = host.tick();
        budget -= 1;
        if budget == 0 || (budget <= 2550 && host.world.helper_contexts.is_empty()) {
            break;
        }
    }
    let (x1, z1) = player_xz(&host);
    assert!(
        host.world.field_walk_component_size(x1, z1) >= MIN_COMPONENT_SUBCELLS,
        "after the entry record drains the izumi player at ({x1},{z1}) must stand in open floor"
    );

    // Pad input moves the player: drive each direction for 40 frames and
    // require real displacement on both axes across the set (the old spawn
    // was wall-blocked in all four directions).
    let mut max_dx = 0i32;
    let mut max_dz = 0i32;
    for btn in [
        PadButton::Up,
        PadButton::Down,
        PadButton::Left,
        PadButton::Right,
    ] {
        let (bx, bz) = player_xz(&host);
        for _ in 0..40 {
            host.world.set_pad(btn.mask());
            let _ = host.world.tick();
        }
        host.world.set_pad(0);
        let (ax, az) = player_xz(&host);
        max_dx = max_dx.max((ax as i32 - bx as i32).abs());
        max_dz = max_dz.max((az as i32 - bz as i32).abs());
    }
    assert!(
        max_dx >= 32 && max_dz >= 32,
        "pad input must move the izumi player on both axes (max |dx|={max_dx}, max |dz|={max_dz})"
    );
}
