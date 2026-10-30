//! Disc-free: an expansion-pack map's tiles, baked into a field image, read
//! back through the engine's own collision and floor model.
//!
//! The expectations are the retail semantics as `docs/subsystems/
//! field-locomotion.md` states them, written here independently of the
//! builder: a tile is `128 x 128` world units split into four `64 x 64`
//! quarters (`tl` = low X / low Z, `br` = high X / high Z), a wall quarter
//! blocks every point inside it (the wall probe `FUN_801D56C4`), and a
//! tile's floor stands at `-lut[tier]` (Y-down) under the bilinear sampler
//! `FUN_80019278` wherever all four of its corner tiles share the tier.

use legaia_engine_core::world::World;
use legaia_engine_mode::mode::SceneMode;
use legaia_engine_scene::expansion_map::{BaseInputs, build_field_map_image, floor_lut};
use legaia_engine_system::input::PadButton;
use legaia_expansion_format::map::MapDef;
use legaia_expansion_format::{ExpansionPack, PackFiles};

const MAP: &str = r########"
name = "grid"
base = { scene = { kind = "scene", name = "tower" } }
floor_heights = [0, 96]

[tiles]
origin = [20, 30]
rows = [
  "#######",
  "#.....#",
  "#.qqq.#",
  "#.qqq.#",
  "#.qqq.#",
  "#..h..#",
  "#######",
]
[tiles.legend]
"#" = { walls = "all" }
"." = {}
"q" = { height = 1 }
"h" = { walls = ["tl", "br"] }
"########;

fn def() -> MapDef {
    let mut files = PackFiles::new();
    files.insert(
        "pack.toml".into(),
        b"id = \"t\"\nname = \"t\"\nversion = \"0.0.0\"\n".to_vec(),
    );
    files.insert("maps/grid.toml".into(), MAP.as_bytes().to_vec());
    let pack = ExpansionPack::from_files(&files).expect("pack parses");
    pack.maps["grid"].clone()
}

/// A world whose field layer is the map's: the image's `+0x4000` grid and the
/// map's floor ladder, installed the way the field entry installs a retail
/// scene's (`load_field_collision_grid`, the MAN ladder negated).
fn world_on(def: &MapDef) -> World {
    let grid = def.tile_grid().unwrap();
    let img = build_field_map_image(def, &grid, BaseInputs::default()).unwrap();
    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.install_field_player(0);
    world.load_field_collision_grid(&img[0x4000..0x8000]);
    world.terrain.floor_height_lut = floor_lut(def, None).map(i16::wrapping_neg);
    world
}

/// World X / Z of a point `(dx, dz)` units into map tile `(c, r)`, with the
/// map at origin `(20, 30)`.
fn at(c: i32, r: i32, dx: i32, dz: i32) -> (i16, i16) {
    (((20 + c) * 128 + dx) as i16, ((30 + r) * 128 + dz) as i16)
}

#[test]
fn wall_tiles_block_every_quarter_and_floor_tiles_none() {
    let world = world_on(&def());
    for (dx, dz) in [(16, 16), (100, 16), (16, 100), (100, 100), (63, 63)] {
        let (x, z) = at(0, 0, dx, dz);
        assert!(world.field_tile_is_wall(x, z), "'#' at +({dx}, {dz})");
        let (x, z) = at(1, 1, dx, dz);
        assert!(!world.field_tile_is_wall(x, z), "'.' at +({dx}, {dz})");
        let (x, z) = at(3, 3, dx, dz);
        assert!(!world.field_tile_is_wall(x, z), "'q' at +({dx}, {dz})");
    }
}

#[test]
fn sub_cell_walls_block_only_their_quarters() {
    let world = world_on(&def());
    // 'h' at (3, 5) walls tl and br.
    let wall = |dx, dz| {
        let (x, z) = at(3, 5, dx, dz);
        world.field_tile_is_wall(x, z)
    };
    assert!(wall(20, 20), "tl");
    assert!(!wall(100, 20), "tr");
    assert!(!wall(20, 100), "bl");
    assert!(wall(100, 100), "br");
}

#[test]
fn off_map_reads_as_wall() {
    let world = world_on(&def());
    let (x, z) = at(-3, 2, 64, 64);
    assert!(world.field_tile_is_wall(x, z));
    let (x, z) = at(2, 9, 64, 64);
    assert!(world.field_tile_is_wall(x, z));
}

#[test]
fn floor_heights_follow_the_tier_ladder() {
    let world = world_on(&def());
    // The centre of the 3x3 dais: every corner tile is tier 1 -> -96.
    let (x, z) = at(3, 3, 64, 64);
    assert_eq!(world.sample_field_floor_height(x.into(), z.into()), -96);
    // A tier is the height at its tile's low-X / low-Z corner: (5, 1)'s four
    // corner tiles (5, 1), (6, 1), (5, 2), (6, 2) are all tier 0.
    let (x, z) = at(5, 1, 64, 64);
    assert_eq!(world.sample_field_floor_height(x.into(), z.into()), 0);
    // (1, 1) has the dais tile (2, 2) as its far corner: the bilinear centre
    // weighs it a quarter, -96 / 4.
    let (x, z) = at(1, 1, 64, 64);
    assert_eq!(world.sample_field_floor_height(x.into(), z.into()), -24);
    // Its near corner itself is tier 0.
    let (x, z) = at(1, 1, 0, 0);
    assert_eq!(world.sample_field_floor_height(x.into(), z.into()), 0);
}

#[test]
fn the_player_walks_open_floor_and_stops_at_the_wall() {
    let mut world = world_on(&def());
    let (x, z) = at(1, 1, 64, 64);
    world.actors[0].move_state.world_x = x;
    world.actors[0].move_state.world_z = z;
    // Right (+X) along row 1: four open tiles, then the east wall at col 6.
    for _ in 0..200 {
        world.set_pad(PadButton::Right.mask());
        let _ = world.tick();
    }
    let ms = &world.actors[0].move_state;
    let east_wall_x = (20 + 6) * 128;
    assert!(
        i32::from(ms.world_x) > (20 + 4) * 128,
        "walked the open row, x = {}",
        ms.world_x
    );
    assert!(
        i32::from(ms.world_x) <= east_wall_x && !world.field_tile_is_wall(ms.world_x, ms.world_z),
        "stopped at the wall face {east_wall_x}, x = {}",
        ms.world_x
    );
    // Down (-Z) from row 1 into the north wall never enters a wall tile.
    let (x, z) = at(1, 1, 64, 64);
    world.actors[0].move_state.world_x = x;
    world.actors[0].move_state.world_z = z;
    for _ in 0..30 {
        world.set_pad(PadButton::Down.mask());
        let _ = world.tick();
        let ms = &world.actors[0].move_state;
        assert!(
            !world.field_tile_is_wall(ms.world_x, ms.world_z),
            "inside a wall at ({}, {})",
            ms.world_x,
            ms.world_z
        );
    }
    assert!(
        i32::from(world.actors[0].move_state.world_z) >= (30 + 1) * 128,
        "the -Z walk stopped at the north wall, not before row 1's own edge"
    );
}
