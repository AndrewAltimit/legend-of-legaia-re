//! Disc-gated: every solid thing on every Rebirth tower floor stops the
//! party.
//!
//! A pack prop carries no collision box of its own (`solid` closes its
//! tile), and a free-standing pier is a fully walled tile with open floor
//! either side: both are things a player sees and expects to bump into. For
//! each, the walk starts two tiles to its north on open floor and holds the
//! pad toward it (`Up` walks south, `+Z`); the party must stop at the tile's
//! north face or skid round it (retail's wall-slide resolver); standing
//! inside the tile's span is the failure. A walk a trigger or a fight cuts
//! short proves nothing, so it is counted apart rather than passed. Skips when `LEGAIA_DISC_BIN` is
//! unset.

use std::sync::Arc;

use legaia_engine_mode::mode::SceneMode;
use legaia_engine_scene::expansion_map::tile_centre_world;
use legaia_engine_scene_host::scene_host::{SceneHost, SceneTickEvent};
use legaia_engine_system::input::PadButton;
use legaia_expansion_format::ExpansionPack;
use legaia_expansion_format::map::{MapDef, TileGrid};

fn rebirth_pack() -> ExpansionPack {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../packs/rebirth");
    let files = legaia_expansion_format::read_files(std::path::Path::new(dir)).expect("read");
    ExpansionPack::from_files(&files).expect("the pack parses")
}

fn open(grid: &TileGrid, col: i32, row: i32, tier: u8) -> bool {
    col >= 0
        && row >= 0
        && grid
            .get(col as usize, row as usize)
            .is_some_and(|t| !t.void && t.walls == 0 && t.tier == tier)
}

/// The tiles to walk into: solid props, and fully walled tiles with open
/// floor on both sides (west and east) - piers, statue bases, braziers'
/// own walled tiles.
fn obstacles(def: &MapDef, grid: &TileGrid) -> Vec<(String, u16, u16)> {
    let mut out: Vec<(String, u16, u16)> = def
        .props
        .iter()
        .filter(|p| p.solid && p.top.is_none())
        .map(|p| (format!("prop {}", p.id), p.tile[0], p.tile[1]))
        .collect();
    for row in 0..grid.height {
        for col in 0..grid.width {
            let t = grid.get(col, row).unwrap();
            if t.void || t.walls != 0xF {
                continue;
            }
            let (c, r) = (col as i32, row as i32);
            let free_sides = [(c - 1, r), (c + 1, r)]
                .iter()
                .filter(|&&(x, y)| {
                    grid.get(x.max(0) as usize, y as usize)
                        .is_some_and(|n| x >= 0 && !n.void && n.walls == 0)
                })
                .count();
            if free_sides == 2 && !out.iter().any(|o| (o.1, o.2) == (col as u16, row as u16)) {
                out.push((
                    format!("walled tile '{}'", grid.char_at(col, row).unwrap()),
                    col as u16,
                    row as u16,
                ));
            }
        }
    }
    out
}

#[test]
fn every_solid_prop_and_pier_on_the_rebirth_floors_blocks_the_walk() {
    let Some(disc) = std::env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let pack = rebirth_pack();
    let mut host = SceneHost::open_disc(disc).expect("open the disc");
    host.activate_expansion_pack(Arc::new(pack.clone()))
        .expect("the Rebirth pack activates");
    // The first-arrival cutscenes hold the field; mark them seen.
    for c in pack.cutscenes.cutscenes.keys() {
        host.world
            .expansion
            .quests
            .progress
            .flags
            .set(&format!("cutscene.{c}"));
    }
    let (mut blocked, mut inconclusive) = (0usize, Vec::new());
    let mut walked_through = Vec::new();
    // The tower floors: the isles maps are held by `expansion_solid_props_disc`.
    for (id, def) in pack
        .maps
        .iter()
        .filter(|(id, _)| id.starts_with("rebirth_tower_"))
    {
        let grid = def.tile_grid().expect("grid");
        for (what, col, row) in obstacles(def, &grid) {
            let (c, r) = (i32::from(col), i32::from(row));
            // Approach from the north: two open tiles on the obstacle's own
            // floor tier would be ideal; the tier of the tile in front is
            // what the walk stands on.
            let Some(front) = grid.get(col as usize, (r - 1).max(0) as usize) else {
                continue;
            };
            let tier = front.tier;
            if r < 2 || !open(&grid, c, r - 1, tier) || !open(&grid, c, r - 2, tier) {
                continue;
            }
            host.enter_expansion_map(id, "")
                .unwrap_or_else(|e| panic!("{id}: {e:#}"));
            let _ = host.tick();
            let (x, z) = tile_centre_world(def, col, row - 2);
            host.world.actors[0].move_state.world_x = x;
            host.world.actors[0].move_state.world_z = z;
            let _ = host.tick();
            let start_z = i32::from(host.world.actors[0].move_state.world_z);
            let face = i32::from(tile_centre_world(def, col, row).1) - 64;
            let label = format!("{id} {what} at ({col}, {row})");
            let mut cut = None;
            let mut through = false;
            for _ in 0..150 {
                host.world.set_pad(PadButton::Up.mask());
                match host.tick().expect("tick") {
                    SceneTickEvent::Stepped => {}
                    other => {
                        cut = Some(format!("{other:?}"));
                        break;
                    }
                }
                if host.active_expansion_map() != Some(id.as_str()) {
                    cut = Some("left the map".into());
                    break;
                }
                // A fight the walk rolled re-seats the party on the stage.
                if host.world.mode != SceneMode::Field {
                    cut = Some(format!("{:?}", host.world.mode));
                    break;
                }
                // Inside the obstacle's tile, not slid off round its side.
                let ms = &host.world.actors[0].move_state;
                let (px, pz) = (i32::from(ms.world_x), i32::from(ms.world_z));
                // The wall-slide resolver skids the party round a lone pier
                // and back onto its line once past: only a point inside
                // the tile's own span is a walk through it.
                if pz > face + 16 && pz < face + 128 - 16 && (px - i32::from(x)).abs() < 48 {
                    through = true;
                    walked_through.push(format!("{label}: reached ({px}, {pz}), face z {face}"));
                    break;
                }
            }
            let pz = i32::from(host.world.actors[0].move_state.world_z);
            if through {
            } else if pz - start_z < 64 || cut.is_some() {
                inconclusive.push(format!(
                    "{label}: moved {} units, {}",
                    pz - start_z,
                    cut.unwrap_or_else(|| "no event".into())
                ));
            } else {
                blocked += 1;
            }
        }
    }
    for l in &inconclusive {
        eprintln!("[inconclusive] {l}");
    }
    eprintln!(
        "[solids] {blocked} blocked, {} inconclusive",
        inconclusive.len()
    );
    assert!(walked_through.is_empty(), "{walked_through:#?}");
    assert!(blocked > 0, "no obstacle was walked into");
}
