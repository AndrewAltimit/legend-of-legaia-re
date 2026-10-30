//! Disc-gated: on every Rebirth map, the party can still walk from its
//! first spawn to every other spawn, every exit and every trigger.
//!
//! Solid props and solid decorations close the collision sub-cells under
//! their meshes, so a cluster of crystals or a row of crates set solid can
//! wall a passage shut as easily as it stops the player walking through
//! them. The walk is a flood fill over the live world's wall probe
//! (`World::field_tile_is_wall`) at sub-cell resolution (64 units, four to
//! a tile), from the first spawn's tile. A spawn or a walk-on trigger must
//! have a reached sub-cell inside its tile; an exit the same on one tile of
//! its rectangle; a press trigger (examined from the next tile) one within
//! a tile of its rectangle. Heights are not walked: a tier step is not a
//! wall here. Skips when `LEGAIA_DISC_BIN` is unset.

use std::collections::VecDeque;
use std::sync::Arc;

use legaia_engine_scene::expansion_map::tile_centre_world;
use legaia_engine_scene_host::scene_host::SceneHost;
use legaia_expansion_format::ExpansionPack;
use legaia_expansion_format::map::{MapDef, TileRect};

fn rebirth_pack() -> ExpansionPack {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../packs/rebirth");
    let files = legaia_expansion_format::read_files(std::path::Path::new(dir)).expect("read");
    ExpansionPack::from_files(&files).expect("the pack parses")
}

/// The reached sub-cells of a map, `2 * cols` by `2 * rows`.
struct Reach {
    w: i32,
    h: i32,
    seen: Vec<bool>,
}

impl Reach {
    fn at(&self, sc: i32, sr: i32) -> bool {
        sc >= 0 && sr >= 0 && sc < self.w && sr < self.h && self.seen[(sr * self.w + sc) as usize]
    }

    /// Any reached sub-cell inside `rect` grown by `grow` tiles.
    fn touches(&self, rect: &TileRect, grow: i32) -> bool {
        let (c0, r0) = (i32::from(rect[0]) - grow, i32::from(rect[1]) - grow);
        let (c1, r1) = (i32::from(rect[2]) + grow, i32::from(rect[3]) + grow);
        (r0 * 2..=r1 * 2 + 1).any(|sr| (c0 * 2..=c1 * 2 + 1).any(|sc| self.at(sc, sr)))
    }
}

/// The world point of sub-cell `(sc, sr)`'s centre.
fn sub_centre(def: &MapDef, sc: i32, sr: i32) -> (i16, i16) {
    let (x, z) = tile_centre_world(def, (sc / 2) as u16, (sr / 2) as u16);
    let off = |s: i32| if s % 2 == 0 { -32 } else { 32 };
    (x + off(sc), z + off(sr))
}

fn flood(host: &SceneHost, def: &MapDef) -> Reach {
    let rows = def.tiles.rows.len() as i32;
    let cols = def
        .tiles
        .rows
        .iter()
        .map(|r| r.chars().count())
        .max()
        .unwrap_or(0) as i32;
    let (w, h) = (cols * 2, rows * 2);
    let mut seen = vec![false; (w * h) as usize];
    let open = |sc: i32, sr: i32| {
        let (x, z) = sub_centre(def, sc, sr);
        !host.world.field_tile_is_wall(x, z)
    };
    let start = def.spawns.first().expect("a map has a spawn").tile;
    let mut q = VecDeque::new();
    for (dc, dr) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
        let (sc, sr) = (i32::from(start[0]) * 2 + dc, i32::from(start[1]) * 2 + dr);
        if open(sc, sr) {
            seen[(sr * w + sc) as usize] = true;
            q.push_back((sc, sr));
        }
    }
    while let Some((sc, sr)) = q.pop_front() {
        for (dc, dr) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
            let (nc, nr) = (sc + dc, sr + dr);
            if nc < 0 || nr < 0 || nc >= w || nr >= h {
                continue;
            }
            let i = (nr * w + nc) as usize;
            if !seen[i] && open(nc, nr) {
                seen[i] = true;
                q.push_back((nc, nr));
            }
        }
    }
    Reach { w, h, seen }
}

#[test]
fn every_spawn_exit_and_trigger_on_the_rebirth_maps_is_reachable() {
    let Some(disc) = std::env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let pack = rebirth_pack();
    let mut host = SceneHost::open_disc(disc).expect("open the disc");
    host.activate_expansion_pack(Arc::new(pack.clone()))
        .expect("the Rebirth pack activates");
    let mut cut_off = Vec::new();
    let mut checked = 0;
    for (id, def) in &pack.maps {
        host.enter_expansion_map(id, "")
            .unwrap_or_else(|e| panic!("{id}: {e:#}"));
        let reach = flood(&host, def);
        let reached = reach.seen.iter().filter(|s| **s).count();
        assert!(
            reached > 16,
            "{id}: the flood from the first spawn reached {reached} sub-cells"
        );
        for s in &def.spawns {
            checked += 1;
            let t = [s.tile[0], s.tile[1], s.tile[0], s.tile[1]];
            if !reach.touches(&t, 0) {
                cut_off.push(format!("{id}: spawn {} at {:?}", s.id, s.tile));
            }
        }
        for e in &def.exits {
            checked += 1;
            if !reach.touches(&e.rect, 0) {
                cut_off.push(format!("{id}: exit {} at {:?}", e.id, e.rect));
            }
        }
        for t in &def.triggers {
            checked += 1;
            let grow = i32::from(t.press);
            if !reach.touches(&t.rect, grow) {
                cut_off.push(format!("{id}: trigger {} at {:?}", t.id, t.rect));
            }
        }
    }
    assert!(checked > 50, "only {checked} places checked");
    assert!(
        cut_off.is_empty(),
        "walled off from the first spawn:\n{}",
        cut_off.join("\n")
    );
}
