//! Disc-gated: **ramp / stair tiles take their floor height from the `.MAP`
//! kind-2 elevation-override records, not from the collision grid's corner
//! nibbles.**
//!
//! Rim Elm's two shore ramps (the stone stairs down to the beach, one either
//! side of the village green) sit on collision-grid tiles whose elevation
//! nibble is `0` - sea level. Their real elevation lives entirely in the
//! `.MAP` trigger block's kind-2 sub-table, which `FUN_80019278` consults for
//! any tile whose object-grid cell word carries bit `0x800`
//! (`legaia_engine_core::world::CELL_ELEVATION_OVERRIDE`).
//!
//! The regression this guards: a sampler that only bilinear-interpolates the
//! corner nibbles reads the whole ramp as sea level, so a walking actor drops
//! 192 units at the top of the stairs and walks *under* the drawn stair mesh -
//! the "player clips straight through the ramp" bug. Each assertion below is
//! contrasted against the value that model would produce, so the test is
//! non-vacuous: it fails on the pre-fix sampler.
//!
//! Skips when `LEGAIA_DISC_BIN` / `extracted/` are missing (disc-gated
//! convention).

use std::path::PathBuf;
use std::sync::Arc;

use legaia_engine_core::scene::{ProtIndex, Scene};
use legaia_engine_core::world::{CELL_ELEVATION_OVERRIDE, World};

/// `.MAP` object-grid offset (one `u16` cell per tile).
const OBJECT_GRID_OFFSET: usize = 0x8000;
/// `.MAP` primary trigger block (kind-2 sub-table lives here).
const TRIGGER_BLOCK_OFFSET: usize = 0x10000;
/// `.MAP` fallback trigger block (the next entry's leading sectors).
const TRIGGER_FALLBACK_OFFSET: usize = 0x12000;

/// The centre of the plateau the player spawns on: the village green, whose
/// tiles have NO override bit and sit on elevation tier 6.
const PLATEAU_TILE: (i32, i32) = (20, 20);
/// Height of that plateau in the retail (negated-LUT, up = negative) frame.
const PLATEAU_Y: i32 = -192;

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

fn gate() -> Option<Arc<ProtIndex>> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    let extracted = extracted_dir().or_else(|| {
        eprintln!("[skip] extracted/ missing");
        None
    })?;
    Some(Arc::new(
        ProtIndex::open_extracted(&extracted).expect("open prot index"),
    ))
}

/// Build a field `World` for `scene` with everything the floor sampler reads:
/// the collision grid, the negated floor-height LUT, the object-grid cells and
/// the kind-2 elevation overrides. Mirrors `SceneHost::enter_field_scene`.
fn field_world(index: &ProtIndex, scene_name: &str) -> World {
    let scene = Scene::load(index, scene_name).expect("load scene");
    let lut = scene
        .field_floor_height_lut(index)
        .expect("floor LUT")
        .expect("scene has a MAN");
    let grid = scene
        .field_collision_grid(index)
        .expect("collision grid")
        .expect("scene has a field map");
    let map_idx = scene.field_map_index(index).expect("field map entry");
    let map = index.entry_bytes_extended(map_idx).expect("map bytes");

    let mut world = World::new();
    world.install_field_player(0);
    world.load_field_collision_grid(&grid);
    world.field_floor_height_lut = lut.map(|v| v.wrapping_neg());
    world.load_field_object_cells(&map[OBJECT_GRID_OFFSET..]);
    world.load_field_elevation_overrides(
        &map[TRIGGER_BLOCK_OFFSET..],
        map.get(TRIGGER_FALLBACK_OFFSET..).unwrap_or_default(),
    );
    world.follow_terrain_height = true;
    world
}

/// Centre of tile `(c, r)` in world units.
fn tile_centre(c: i32, r: i32) -> (i32, i32) {
    (c * 128 + 64, r * 128 + 64)
}

/// The collision-grid elevation nibble of tile `(c, r)`.
fn nibble(world: &World, c: i32, r: i32) -> u8 {
    world.field_collision_grid[(r * 0x80 + c) as usize] & 0x0F
}

/// Rim Elm's ramp tiles are flagged, carry kind-2 records, and their collision
/// nibbles are sea level - so the nibble surface cannot be the height source.
#[test]
fn rim_elm_ramp_tiles_are_override_tiles_over_sea_level_nibbles() {
    let Some(index) = gate() else { return };
    let world = field_world(&index, "town01");

    assert!(
        !world.field_elevation_overrides.is_empty(),
        "town01 must parse a kind-2 elevation-override table"
    );

    // The left ramp's stair column and its top landing.
    for &(c, r) in &[(17, 17), (16, 16), (17, 16), (17, 15), (17, 14)] {
        assert!(
            world.field_tile_has_elevation_override(c, r),
            "ramp tile ({c},{r}) must carry the 0x800 object-grid bit"
        );
        assert_eq!(
            nibble(&world, c, r),
            0,
            "ramp tile ({c},{r}) sits on a sea-level nibble - its elevation \
             cannot come from the collision grid"
        );
    }

    // The spawn plateau is a plain tile: unflagged, and elevated by its nibble.
    let (pc, pr) = PLATEAU_TILE;
    assert!(!world.field_tile_has_elevation_override(pc, pr));
    assert_eq!(nibble(&world, pc, pr), 6);
}

/// The height the sampler reports on the ramp is the authored one, not the
/// sea-level value the corner-nibble surface would give.
#[test]
fn rim_elm_ramp_heights_come_from_the_override_records() {
    let Some(index) = gate() else { return };
    let world = field_world(&index, "town01");

    // Spawn plateau: unchanged by the override model.
    let (px, pz) = tile_centre(PLATEAU_TILE.0, PLATEAU_TILE.1);
    assert_eq!(world.sample_field_floor_height(px, pz), PLATEAU_Y);

    // The ramp's top landing is level with the plateau it leads off - the
    // player steps onto it, he does not fall 192 units onto the beach. Under
    // the corner-nibble surface this tile reads 0 (sea level), which is what
    // sank the player under the stair mesh.
    for &(c, r) in &[(17, 17), (16, 16), (15, 16), (16, 17)] {
        let (x, z) = tile_centre(c, r);
        assert_eq!(
            world.sample_field_floor_height(x, z),
            PLATEAU_Y,
            "ramp landing tile ({c},{r}) must sit level with the plateau"
        );
    }

    // Walking the stair column (tile x = 17) north off the landing descends in
    // authored steps down to the beach - strictly monotone, never below sea
    // level, and never the single 192-unit plunge of the nibble surface.
    let x = 17 * 128 + 32;
    let heights: Vec<i32> = [17, 16, 15, 14, 13]
        .iter()
        .map(|&r| world.sample_field_floor_height(x, r * 128 + 32))
        .collect();
    assert_eq!(heights.first().copied(), Some(PLATEAU_Y), "landing");
    assert_eq!(heights.last().copied(), Some(0), "beach");
    for w in heights.windows(2) {
        assert!(
            w[1] >= w[0],
            "the stair column must only ever descend: {heights:?}"
        );
        assert!(
            w[1] - w[0] < -PLATEAU_Y,
            "no step may plunge the full plateau height: {heights:?}"
        );
    }
    // The descent passes through real intermediate steps - the whole point of
    // the override model. The corner-nibble surface has none: it reads this
    // column as [0, 0, 0, 0, 0], sea level from the very first stair tile.
    let intermediate: Vec<i32> = heights
        .iter()
        .copied()
        .filter(|&y| y < 0 && y > PLATEAU_Y)
        .collect();
    assert!(
        intermediate.len() >= 2,
        "the stairs must step through intermediate heights: {heights:?}"
    );
}

/// Structural: every scene's kind-2 table stays inside its `.MAP` block and
/// addresses in-range tiles, and the tiles it names carry the `0x800` bit.
/// Guards the 4-byte record stride the parser assumes.
#[test]
fn kind2_tables_are_well_formed_across_the_field_scenes() {
    let Some(index) = gate() else { return };
    let mut scenes_with_overrides = 0usize;
    for name in ["town01", "town0c", "map01", "cave01"] {
        let Ok(scene) = Scene::load(&index, name) else {
            continue;
        };
        if scene.field_map_index(&index).is_none() {
            continue;
        }
        let world = field_world(&index, name);
        if world.field_elevation_overrides.is_empty() {
            continue;
        }
        scenes_with_overrides += 1;
        for rec in &world.field_elevation_overrides {
            // A record whose tile is off the 128x128 grid means the stride /
            // offset math has drifted.
            assert!(
                rec.tile_x < 0x80 && rec.tile_z < 0x80,
                "{name}: kind-2 record {rec:?} names an off-grid tile"
            );
            // Every record's tile must be flagged - the cell bit and the table
            // are two halves of one mechanism.
            let idx = (rec.tile_z as usize) * 0x80 + rec.tile_x as usize;
            assert!(
                world.field_object_cells[idx] & CELL_ELEVATION_OVERRIDE != 0,
                "{name}: kind-2 record {rec:?} names a tile with no 0x800 bit"
            );
        }
    }
    assert!(
        scenes_with_overrides >= 2,
        "the sampled scenes must carry elevation-override tables"
    );
}
