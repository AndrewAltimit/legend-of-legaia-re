//! Is the Ravine (`keikoku`) interior walkable, and from which mouth?
//!
//! The instrument behind the rung-5 verdict in
//! `crates/engine-shell/tests/critical_path_replay.rs`. It answers the three
//! candidates that leg's stall left open - the arrival seat, the mouth
//! `portal_tile` picks, and the interior grid - by measuring all four mouths
//! side by side instead of the one the ladder happens to take. It also prints
//! the scene's own door table, which is what turns "a trigger tile" into "the
//! door back to `map01` at `(64, 67)`".
//!
//! It is the cheap half of that investigation: entering the scene directly
//! costs a couple of seconds where reproducing it through the ladder costs a
//! three-minute pad-driven run of rungs 1-4 first.
//!
//! Four floods per mouth. Row 1 answers "is the interior open at all", and the
//! two deltas below it are the two separate seals - `2 -> 3` is the planner
//! refusing its own first step, `3 -> 4` is a radius that cannot tell the door
//! home from the corridor onward.
//!
//! | flood | avoid set |
//! |---|---|
//! | 1 | nothing |
//! | 2 | every trigger tile within `DUNGEON_TRAVERSE_TILES`, seat included |
//! | 3 | the same, minus the seat tile |
//! | 4 | the arrival record's whole band, and only that |
//!
//! Run: `cargo run -p legaia-engine-shell --example keikoku_reach_probe`
//! (needs `extracted/` beside the workspace root).

use std::collections::{BTreeMap, HashSet, VecDeque};

use legaia_engine_core::scene::SceneHost;
use legaia_engine_core::world::{SceneMode, WorldMapEntityConfig};

const TILE: i16 = 128;
const SUBCELL: i16 = 32;
/// Mirrors `critical_path_replay::DUNGEON_TRAVERSE_TILES`.
const TRAVERSE_TILES: i32 = 12;
/// The flood is deliberately uncapped by geometry: `field_tile_is_wall` masks
/// both axes with `& 0x7F`, so an open map wraps and the reachable set is
/// unbounded. This caps the work, and a run that hits it has already answered
/// "is anything reachable" many times over.
const FLOOD_CAP: usize = 400_000;

type Cell = (i16, i16);

fn tile_center(t: (i16, i16)) -> (i16, i16) {
    (t.0 * TILE + 0x40, t.1 * TILE + 0x40)
}

fn tile_of(x: i16, z: i16) -> (i16, i16) {
    ((x - 0x40) >> 7, (z - 0x40) >> 7)
}

fn cell_of(x: i16, z: i16) -> Cell {
    ((x + SUBCELL / 2) / SUBCELL, (z + SUBCELL / 2) / SUBCELL)
}

fn cell_center(c: Cell) -> (i16, i16) {
    (c.0 * SUBCELL, c.1 * SUBCELL)
}

fn dist(a: (i16, i16), b: (i16, i16)) -> i32 {
    ((a.0 - b.0).abs() + (a.1 - b.1).abs()) as i32
}

const STEPS: [((i16, i16), usize); 4] = [((0, -1), 0), ((-1, 0), 1), ((0, 1), 2), ((1, 0), 3)];

/// BFS the walkability grid the way `critical_path_replay::plan_path` does,
/// refusing any step whose **destination tile** is in `avoid`. That is the
/// exact semantics under test: it is what makes a seat inside an avoided tile
/// unable to move at all.
fn flood(host: &SceneHost, from: Cell, avoid: &HashSet<(i16, i16)>) -> HashSet<Cell> {
    let mut seen: HashSet<Cell> = HashSet::new();
    let mut queue = VecDeque::new();
    seen.insert(from);
    queue.push_back(from);
    while let Some(cur) = queue.pop_front() {
        if seen.len() > FLOOD_CAP {
            break;
        }
        let (cx, cz) = cell_center(cur);
        for ((dx, dz), dir) in STEPS {
            let next = (cur.0 + dx, cur.1 + dz);
            if next.0 < 0 || next.1 < 0 || seen.contains(&next) {
                continue;
            }
            if host.world.field_dir_blocked(cx, cz, dir)
                || host.world.field_actor_dir_blocked(cx, cz, dir)
            {
                continue;
            }
            if !avoid.is_empty() {
                let (nx, nz) = cell_center(next);
                if avoid.contains(&(nx >> 7, nz >> 7)) {
                    continue;
                }
            }
            seen.insert(next);
            queue.push_back(next);
        }
    }
    seen
}

fn tiles_of(cells: &HashSet<Cell>) -> HashSet<(i16, i16)> {
    cells
        .iter()
        .map(|&c| {
            let (x, z) = cell_center(c);
            tile_of(x, z)
        })
        .collect()
}

/// Every gate-1 walk-on trigger tile of the loaded scene.
fn trigger_tiles(host: &SceneHost) -> Vec<(i16, i16)> {
    let mut out = Vec::new();
    for tz in 0i16..128 {
        for tx in 0i16..128 {
            let (wx, wz) = tile_center((tx, tz));
            if host.tile_has_walk_on_trigger(wx, wz) {
                out.push((tx, tz));
            }
        }
    }
    out
}

/// One partition-2 record's `0x3F` destination plus every gate-1 trigger tile
/// that spawns it.
struct Door {
    dest: String,
    dest_tile: (u8, u8),
    tiles: Vec<(i16, i16)>,
}

/// The loaded scene's gate-1 triggers split by partition-2 record: the ones
/// whose record carries a `0x3F` (a door, with the tile it returns to), and
/// the ones whose record does not (a story beat).
type SceneDoors = (BTreeMap<u8, Door>, BTreeMap<u8, Vec<(i16, i16)>>);

fn scene_doors(host: &SceneHost) -> SceneDoors {
    let scene = host.scene.as_ref().expect("scene loaded");
    let man = scene
        .field_man_payload(&host.index)
        .ok()
        .flatten()
        .expect("man payload");
    let mf = legaia_asset::man_section::parse(&man).expect("man parse");
    let (primary, fallback) = scene
        .field_tile_triggers(&host.index)
        .expect("tile triggers");
    let mut triggers = primary;
    triggers.extend(fallback);

    let mut doors: BTreeMap<u8, Door> = BTreeMap::new();
    for site in legaia_engine_core::man_field_scripts::overworld_portal_sites(&mf, &man, &triggers)
    {
        doors
            .entry(site.record)
            .or_insert_with(|| Door {
                dest: site.scene_name.clone(),
                dest_tile: (site.entry_x, site.entry_z),
                tiles: Vec::new(),
            })
            .tiles
            .push((i16::from(site.overworld_x), i16::from(site.overworld_z)));
    }
    let mut beats: BTreeMap<u8, Vec<(i16, i16)>> = BTreeMap::new();
    for t in &triggers {
        if t.gate != 1 || doors.contains_key(&t.record) {
            continue;
        }
        beats
            .entry(t.record)
            .or_default()
            .push((i16::from(t.tile_x), i16::from(t.tile_z)));
    }
    (doors, beats)
}

fn report(host: &mut SceneHost, mouth: (i16, i16), entry: (u8, u8)) {
    host.enter_field_scene("keikoku", 0).expect("enter keikoku");
    // The collision model both shipped hosts run; a bare `World` defaults both
    // off and would be a third model no player meets.
    host.world.leading_edge_wall_probes = true;
    host.world.solid_field_npcs = true;
    host.world.seat_player_at_tile(entry.0, entry.1);
    let slot = host.world.player_actor_slot.expect("player slot") as usize;
    let ms = &host.world.actors[slot].move_state;
    let (px, pz) = (ms.world_x, ms.world_z);
    let seat = tile_of(px, pz);
    let from = cell_of(px, pz);

    println!("=== map01 mouth {mouth:?} -> keikoku entry byte {entry:?}");
    println!("    seated world ({px}, {pz}) tile {seat:?} cell {from:?}");

    let (doors, beats) = scene_doors(host);
    println!("    scene-change records (each returns to its own map01 tile):");
    for (rec, door) in &doors {
        let nearest = door
            .tiles
            .iter()
            .map(|&t| dist(t, seat))
            .min()
            .unwrap_or(-1);
        println!(
            "      rec {rec:<3} -> {} @ {:?}: {:>3} band tile(s), nearest {nearest}",
            door.dest,
            door.dest_tile,
            door.tiles.len()
        );
    }
    println!("    beat records (gate-1, no 0x3F):");
    for (rec, tiles) in &beats {
        let nearest = tiles.iter().map(|&t| dist(t, seat)).min().unwrap_or(-1);
        println!(
            "      rec {rec:<3} {:>3} tile(s), nearest {nearest}: {:?}...",
            tiles.len(),
            &tiles[..tiles.len().min(4)]
        );
    }

    let triggers = trigger_tiles(host);
    let far = |ts: &HashSet<(i16, i16)>| -> usize {
        triggers
            .iter()
            .filter(|t| ts.contains(t) && dist(**t, seat) >= TRAVERSE_TILES)
            .count()
    };

    // 1. Nothing avoided - is the interior open at all?
    let open = flood(host, from, &HashSet::new());
    let open_t = tiles_of(&open);
    println!(
        "    [1] no avoid set        : {:>7} sub-cells, {:>5} tiles, {:>3} of {} trigger tiles \
         reached, {} of them {TRAVERSE_TILES}+ away",
        open.len(),
        open_t.len(),
        triggers.iter().filter(|t| open_t.contains(t)).count(),
        triggers.len(),
        far(&open_t),
    );

    // 2. The radius hazard set, seat tile included - the shape that reported
    //    the scene sealed.
    let radius: HashSet<(i16, i16)> = triggers
        .iter()
        .copied()
        .filter(|&t| dist(t, seat) < TRAVERSE_TILES)
        .collect();
    let sealed = flood(host, from, &radius);
    println!(
        "    [2] radius, seat in     : {:>7} sub-cells, {:>5} tiles, {:>3} far triggers \
         ({} tile(s) avoided)",
        sealed.len(),
        tiles_of(&sealed).len(),
        far(&tiles_of(&sealed)),
        radius.len(),
    );

    // 3. The same set minus the seat tile. The delta between [2] and [3] is
    //    the whole of the "sealed interior".
    let mut radius_no_seat = radius.clone();
    radius_no_seat.remove(&seat);
    let unsealed = flood(host, from, &radius_no_seat);
    println!(
        "    [3] radius, seat out    : {:>7} sub-cells, {:>5} tiles, {:>3} far triggers",
        unsealed.len(),
        tiles_of(&unsealed).len(),
        far(&tiles_of(&unsealed)),
    );

    // 4. The honest hazard: the arrival record's band, whole, and nothing else.
    let arrival_rec = doors
        .iter()
        .min_by_key(|(_, door)| {
            door.tiles
                .iter()
                .map(|&t| dist(t, seat))
                .min()
                .unwrap_or(i32::MAX)
        })
        .map(|(r, _)| *r);
    if let Some(rec) = arrival_rec {
        let band: HashSet<(i16, i16)> = doors[&rec].tiles.iter().copied().collect();
        let walk = tiles_of(&flood(host, from, &band));
        let reachable: Vec<(u8, (i16, i16), i32)> = doors
            .iter()
            .filter(|(r, _)| **r != rec)
            .filter_map(|(r, door)| {
                door.tiles
                    .iter()
                    .filter(|t| walk.contains(t))
                    .min_by_key(|&&t| dist(t, seat))
                    .map(|&t| (*r, t, dist(t, seat)))
            })
            .collect();
        println!(
            "    [4] arrival record {rec} only: {:>5} tiles; other doors reachable \
             (record, tile, tiles away): {reachable:?}",
            walk.len(),
        );
    }
    println!();
}

fn main() {
    let extracted = std::path::PathBuf::from("extracted");
    let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");

    host.enter_world_map_scene("map01").expect("enter map01");
    assert_eq!(host.world.mode, SceneMode::WorldMap);
    let mouths: Vec<((i16, i16), (u8, u8))> = host
        .world
        .world_map_entity_configs
        .iter()
        .zip(host.world.world_map_entity_positions.iter())
        .filter_map(|(cfg, &(x, z))| match cfg {
            WorldMapEntityConfig::OverworldPortal {
                scene_name,
                entry_x,
                entry_z,
                ..
            } if scene_name == "keikoku" => Some((tile_of(x, z), (*entry_x, *entry_z))),
            _ => None,
        })
        .collect();
    println!("map01 keikoku mouths (map01 tile -> keikoku entry byte): {mouths:?}\n");

    // Several mouths share an arrival tile (a two-tile overworld band is two
    // portals naming one door), so report each distinct seat once.
    let mut seen: HashSet<(u8, u8)> = HashSet::new();
    for (mouth, entry) in mouths {
        if seen.insert(entry) {
            report(&mut host, mouth, entry);
        }
    }
}
