//! Disc-gated: the Rebirth pack's outdoor Isles maps, built on a sculpted
//! terrain (`docs/formats/expansion-pack-maps.md`, "Terrain"), entered as
//! live field scenes.
//!
//! On each map the world's floor sampler answers the terrain's surface (the
//! surface its meshes draw), the player arrives standing on it, every spawn
//! is walkable, and the deep water the terrain floods is a wall. Skips when
//! `LEGAIA_DISC_BIN` is unset.

use std::sync::Arc;

use legaia_engine_scene_host::scene_host::SceneHost;
use legaia_expansion_format::ExpansionPack;
use legaia_expansion_format::validate::{has_errors, validate};

const TERRAIN_MAPS: [&str; 3] = ["isles_beach", "isles_village", "isles_grotto"];

fn rebirth_host() -> Option<SceneHost> {
    let Some(disc) = std::env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return None;
    };
    let mut host = SceneHost::open_disc(disc).expect("open the disc");
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../packs/rebirth");
    let pack = ExpansionPack::from_dir(std::path::Path::new(dir)).expect("Rebirth loads");
    let issues = validate(&pack);
    assert!(!has_errors(&issues), "{issues:#?}");
    host.activate_expansion_pack(Arc::new(pack))
        .expect("activates");
    Some(host)
}

#[test]
fn the_isles_maps_floor_on_their_terrain() {
    let Some(mut host) = rebirth_host() else {
        return;
    };
    for map in TERRAIN_MAPS {
        let def = host.world.expansion.pack.as_ref().unwrap().maps[map].clone();
        let field = def
            .terrain_heightfield()
            .unwrap()
            .unwrap_or_else(|| panic!("{map} has a terrain"));
        let grid = def.tile_grid().unwrap();
        let walls = def.terrain_walls(&grid).unwrap();
        for spawn in &def.spawns {
            host.enter_expansion_map(map, &spawn.id)
                .unwrap_or_else(|e| panic!("{map}/{}: {e:#}", spawn.id));
            let [c, r] = spawn.tile.map(usize::from);
            let closed = grid.get(c, r).unwrap().walls | walls[r * grid.width + c];
            assert_ne!(closed, 0xF, "{map}/{}: the spawn tile is walled", spawn.id);
            for _ in 0..20 {
                host.tick().expect("tick");
            }
            // The sampler is the terrain at points all over the map.
            let w = &host.world;
            let mut checked = 0;
            for row in (0..grid.height as u16).step_by(3) {
                for col in (0..grid.width as u16).step_by(3) {
                    let (x, z) = def.tile_centre(col, row);
                    let (x, z) = (x + 17, z - 23);
                    let want = field.sample_world(x, z).unwrap();
                    assert_eq!(
                        w.sample_field_floor_height(x, z),
                        -want,
                        "{map}: floor at ({x}, {z})"
                    );
                    checked += 1;
                }
            }
            assert!(checked > 50);
        }
        // Somewhere the terrain floods is walled.
        if def.terrain.as_ref().unwrap().water.is_some() {
            assert!(walls.contains(&0xF), "{map}: deep water walls a tile");
        }
        let o = host.scene.as_ref().unwrap().overrides.clone().unwrap();
        eprintln!("[{map}] {} added meshes", o.added_tmds.len());
        assert_eq!(o.floor_lut, [0; 16]);
    }
}

/// Every added mesh of a terrain map draws all its triangles through the
/// hosts' VRAM-filtered mesh builder: no terrain prim samples unwritten
/// VRAM, so none is dropped as a hole.
#[test]
fn the_isles_terrain_meshes_draw_whole_against_the_maps_vram() {
    let Some(mut host) = rebirth_host() else {
        return;
    };
    for map in TERRAIN_MAPS {
        host.enter_expansion_map(map, "")
            .unwrap_or_else(|e| panic!("{map}: {e:#}"));
        let scene = host.scene.as_ref().unwrap();
        let o = scene.overrides.clone().unwrap();
        let res = legaia_engine_scene::expansion_map::detail::field_resources(&host.index, scene)
            .expect("the map's resources");
        let entry = o.geometry_entry.expect("a geometry entry");
        let pooled: Vec<_> = res.tmds.iter().filter(|t| t.entry_idx == entry).collect();
        let added = &pooled[pooled.len() - o.added_tmds.len()..];
        let (mut kept, mut all) = (0usize, 0usize);
        for t in added {
            kept += t.build_filtered_vram_mesh(&res.vram).indices.len();
            all += legaia_tmd::mesh::tmd_to_vram_mesh(&t.tmd, &t.raw)
                .indices
                .len();
        }
        eprintln!("[{map}] {kept} of {all} indices survive the VRAM filter");
        assert_eq!(kept, all, "{map}: prims dropped by the VRAM filter");
    }
}
