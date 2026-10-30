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

const TERRAIN_MAPS: [&str; 4] = [
    "isles_beach",
    "isles_village",
    "isles_grotto",
    "isles_thornwood",
];

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
    let mut bad = Vec::new();
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
        let mut why: std::collections::BTreeMap<(u16, u16), usize> = Default::default();
        for t in added {
            let _ = legaia_tmd::mesh::tmd_to_vram_mesh_filtered(&t.tmd, &t.raw, |cba, tsb, uvs| {
                let ok = res.vram.prim_has_texture_data(cba, tsb, uvs);
                if !ok {
                    *why.entry((cba, tsb)).or_default() += 1;
                }
                ok
            });
            kept += t.build_filtered_vram_mesh(&res.vram).indices.len();
            all += legaia_tmd::mesh::tmd_to_vram_mesh(&t.tmd, &t.raw)
                .indices
                .len();
        }
        eprintln!(
            "[{map}] {kept} of {all} indices survive the VRAM filter; dropped by (cba, tsb): {why:?}"
        );
        if kept != all {
            bad.push(format!("{map}: {} of {all} indices dropped", all - kept));
        }
    }
    assert!(bad.is_empty(), "prims dropped by the VRAM filter: {bad:?}");
}

/// The grotto's rock names the West Voz Forest's VRAM (`from = "vell"`):
/// the library rewrites every borrowed cell to a copy in the map's free
/// VRAM, and the copy holds the source scene's texels and palette word for
/// word.
#[test]
fn borrowed_terrain_cells_copy_the_source_scenes_vram() {
    use legaia_engine_scene::expansion_map::detail::{field_resources, resolve_mesh_library};
    use legaia_engine_scene::scene::Scene;
    let Some(host) = rebirth_host() else {
        return;
    };
    let pack = host.world.expansion.pack.as_ref().unwrap();
    let def = pack.maps["isles_grotto"].clone();
    let base = Scene::load(&host.index, "cave01").unwrap();
    let geometry = legaia_engine_scene::scene_resources::field_geometry_pack(&base);
    let lib = resolve_mesh_library(
        &host.index,
        &def,
        &pack.assets,
        &base,
        geometry.map(|(e, _)| e),
    )
    .expect("the grotto's library");
    let t = lib.terrain.as_ref().expect("the grotto borrows");
    assert!(t.borrowed().is_empty(), "every from is resolved");
    let vell = field_resources(&host.index, &Scene::load(&host.index, "vell").unwrap()).unwrap();
    let mut vram = field_resources(&host.index, &base).unwrap().vram;
    legaia_engine_scene::expansion_map::detail::apply_vram_patches(&mut vram, &lib.vram_patches);
    let src = def.terrain.as_ref().unwrap();
    let texel = |v: &legaia_tim::Vram, tpage: u8, clut: u16, tile: u8, u: usize, w: usize| {
        let (px, py) = (
            (usize::from(tpage) & 0xF) * 64,
            (usize::from(tpage) >> 4 & 1) * 256,
        );
        let (cu, cv) = (
            usize::from(tile) % 8 * 32 + u,
            usize::from(tile) / 8 * 32 + w,
        );
        let word = v.pixel(px + cu / 4, py + cv);
        let idx = usize::from((word >> ((cu % 4) * 4)) & 0xF);
        let (cx, cy) = (usize::from(clut & 0x3F) * 16, usize::from(clut >> 6));
        v.pixel(cx + idx, cy)
    };
    let mut checked = 0;
    for (from, to) in src.materials.iter().zip(&t.materials) {
        if from.from.as_deref() != Some("vell") {
            continue;
        }
        for (a, b) in from.cells.iter().zip(&to.cells) {
            assert_ne!(
                (a.tpage, a.tile, a.clut),
                (b.tpage, b.tile, b.clut),
                "a borrowed cell moves"
            );
            for (u, w) in [(0, 0), (5, 9), (17, 3), (31, 31), (12, 22)] {
                assert_eq!(
                    texel(&vell.vram, a.tpage, a.clut, a.tile, u, w),
                    texel(&vram, b.tpage, b.clut, b.tile, u, w),
                    "{} cell {a:?} texel ({u}, {w})",
                    from.name
                );
                checked += 1;
            }
        }
    }
    assert!(checked > 100, "{checked}");
}
