//! A pack map's living ground on the world's floor-height ladder: the field
//! tick drives the motion's rungs and nothing else, and a world with no map
//! leaves the ladder alone.

use super::*;
use legaia_engine_expansion::map::ActiveMap;
use legaia_engine_mode::mode::SceneMode;
use legaia_expansion_format::map::MapDef;

fn parse() -> MapDef {
    let mut files = legaia_expansion_format::PackFiles::new();
    files.insert(
        "pack.toml".into(),
        b"id = \"g\"\nname = \"G\"\nversion = \"1.0.0\"\n".to_vec(),
    );
    files.insert("maps/moat.toml".into(), MAP.as_bytes().to_vec());
    legaia_expansion_format::ExpansionPack::from_files(&files)
        .expect("pack parses")
        .maps["moat"]
        .clone()
}

const MAP: &str = r######"
name = "Moat"
floor_heights = [0, 56]
base = { scene = { kind = "scene", name = "rugi" } }
[tiles]
rows = ["#....#", "#.LL.#", "#....#"]
[tiles.legend]
"#" = { walls = "all" }
"." = {}
"L" = { height = 1, walls = "all" }
[[ground_motion]]
kind = "swell"
chars = "L"
amplitude = 10
period = 120
phases = 2
wavelength = 2.0
angle = 90.0
"######;

fn world_on(def: MapDef) -> World {
    let mut w = World {
        mode: SceneMode::Field,
        ..World::default()
    };
    let lut = def.floor_ladder().unwrap().map(i16::wrapping_neg);
    w.terrain.floor_height_lut = lut;
    let grid = std::sync::Arc::new(def.tile_grid().unwrap());
    w.expansion.map = Some(ActiveMap::new(
        "moat",
        std::sync::Arc::new(def),
        grid,
        (1, 0),
        |_| false,
    ));
    w
}

#[test]
fn the_field_tick_swings_the_motions_rungs_about_their_rest() {
    let def = parse();
    let mut w = world_on(def);
    let rest = w.terrain.floor_height_lut;
    // Tier 1 rests at 56 (scratchpad -56); the two phase rungs are 15, 14.
    assert_eq!(rest[15], -56);
    assert_eq!(rest[14], -56);
    w.frame = 30;
    w.tick_field_timer_actors(1);
    let live = w.terrain.floor_height_lut;
    // Step 0 peaks a quarter cycle in; step 1 lags half a period (60
    // frames) behind it and is at its trough.
    assert_eq!(live[15], -66);
    assert_eq!(live[14], -46);
    // The map's own tiers never move.
    assert_eq!(live[..14], rest[..14]);
    // Back at rest on the half cycle.
    w.frame = 60;
    w.tick_field_timer_actors(1);
    assert_eq!(w.terrain.floor_height_lut, rest);
}

#[test]
fn no_map_and_no_motion_leave_the_ladder_alone() {
    let mut def = parse();
    def.ground_motion.clear();
    let mut w = world_on(def);
    let rest = w.terrain.floor_height_lut;
    w.frame = 30;
    w.tick_field_timer_actors(1);
    assert_eq!(w.terrain.floor_height_lut, rest);
    w.expansion.map = None;
    w.tick_field_timer_actors(1);
    assert_eq!(w.terrain.floor_height_lut, rest);
}
