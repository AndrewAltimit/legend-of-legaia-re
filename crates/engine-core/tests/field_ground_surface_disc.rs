//! Disc-gated: the town **ground surface** - the two layers that between them
//! have to leave no hole in a floor the player can stand on.
//!
//! 1. **The ground-quad layer** ([`legaia_asset::field_objects::build_walk_heightfield`])
//!    emits one quad per object-grid cell with the `0x1000` walk bit, textured
//!    from that cell's object record (`+0x14` atlas tile / `+0x15` tpage /
//!    `+0x16..+0x18` CLUT). That gate is retail's, measured from the live GPU
//!    prim pool of a Rim Elm field capture: recovering every ground `POLY_FT4`'s
//!    world `(col, row)` (camera fitted from the quads' own shared corners) gives
//!    **every** on-screen `0x1000` cell a quad, **no** on-screen `objcell == 0`
//!    cell a quad, and the `(tile, tpage, clut)` of every recovered quad equals
//!    its record's `+0x14`/`+0x15`/`+0x16` run. So a cell with no object record
//!    genuinely has no ground quad in retail either - widening the gate to the
//!    collision grid would emit quads retail does not draw, and they would sample
//!    empty atlas space and be discarded (an invisible ground).
//!
//! 2. **The env-mesh layer** surfaces those record-less cells: retail draws the
//!    scene's pack meshes over them (`FUN_8003A55C` placed objects + the
//!    `+0x10`-keyed per-cell terrain meshes). The mesh id is the record's `+0x10`
//!    for **every** object id - see [`legaia_asset::field_objects::pack_mesh_index`].
//!
//! The regression this guards: a positional "field-actor band" rule
//! (`pack_index = obj_idx - 5` for ids `93..=118`) used to override `+0x10` on ten
//! cells per Rim Elm map. One of them (`(30, 17)`, object id `99`, record
//! `+0x10 = 2`) anchors the terrain slab south-east of the player spawn, so the
//! slab was replaced by an unrelated mesh and its floor cells rendered as the
//! clear colour - a black wedge in the ground.
//!
//! Skips when `LEGAIA_DISC_BIN` / `extracted/` are missing (disc-gated convention).

use std::path::PathBuf;
use std::sync::Arc;

use legaia_asset::field_objects::{
    self, GRID_DIM, OBJECT_GRID_OFFSET, OBJECT_INDEX_MASK, ObjectRecord,
};
use legaia_engine_core::field_env;
use legaia_engine_core::scene::{ProtIndex, Scene};
use legaia_engine_core::scene_resources::{
    BuildOptions, FIELD_SHARED_BLOCKS, SceneLoadKind, SceneResources,
};

/// Rim Elm's two scene variants - the maps the retail prim-pool captures pin.
const TOWN_SCENES: &[&str] = &["town01", "town0c"];

/// Sub-cell sampling resolution of the mesh-coverage rasteriser (per axis).
const SUB: usize = 4;

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

fn gate() -> Option<(PathBuf, Arc<ProtIndex>)> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    let extracted = extracted_dir().or_else(|| {
        eprintln!("[skip] extracted/ missing");
        None
    })?;
    let index = Arc::new(ProtIndex::open_extracted(&extracted).expect("open prot index"));
    Some((extracted, index))
}

fn cell_at(map: &[u8], col: usize, row: usize) -> u16 {
    let o = OBJECT_GRID_OFFSET + (row * GRID_DIM + col) * 2;
    u16::from_le_bytes([map[o], map[o + 1]])
}

/// The ground layer emits exactly the `0x1000` cells retail emits, with retail's
/// per-cell texture selector, and never on a page the scene has no texels for.
#[test]
fn ground_quads_match_the_retail_object_grid_gate() {
    let Some((_x, index)) = gate() else { return };
    let shared: Vec<Scene> = FIELD_SHARED_BLOCKS
        .iter()
        .filter_map(|n| Scene::load(&index, n).ok())
        .collect();
    let shared_refs: Vec<&Scene> = shared.iter().collect();

    for name in TOWN_SCENES {
        let scene = Scene::load(&index, name).expect("load scene");
        let map_idx = scene.field_map_index(&index).expect("field map");
        let map = index.entry_bytes_extended(map_idx).expect("map bytes");
        let hf = scene
            .walk_heightfield(&index)
            .expect("heightfield")
            .expect("scene has a field map");
        let (res, _) = SceneResources::build_targeted_with_options(
            &scene,
            &shared_refs,
            BuildOptions {
                kind: SceneLoadKind::Field,
                upload_all_tims: true,
                system_ui: None,
            },
        )
        .expect("scene resources");

        // One quad per walk-visible cell - no more (the false "collision grid is
        // the gate" reading), no fewer.
        let gated = (0..GRID_DIM)
            .flat_map(|r| (0..GRID_DIM).map(move |c| (c, r)))
            .filter(|&(c, r)| cell_at(&map, c, r) & field_objects::CELL_WALK_VISIBLE != 0)
            .count();
        assert!(
            gated > 1000,
            "{name}: only {gated} ground cells - map misread"
        );
        assert_eq!(
            hf.quad_count(),
            gated,
            "{name}: ground quads must be exactly the 0x1000 object-grid cells"
        );

        // Per-quad texture selector = the cell's record +0x14/+0x15/+0x16, and
        // the page it names has real texels in the scene VRAM (a quad sampling an
        // empty page decodes to 0x0000 and is discarded - invisible ground).
        let mut checked = 0usize;
        let mut vi = 0usize;
        for r in 0..GRID_DIM {
            for c in 0..GRID_DIM {
                let cell = cell_at(&map, c, r);
                if cell & field_objects::CELL_WALK_VISIBLE == 0 {
                    continue;
                }
                let rec = ObjectRecord::parse(&map, (cell & OBJECT_INDEX_MASK) as usize)
                    .expect("record in range");
                let [clut, tpage] = hf.cba_tsb[vi];
                if rec.terrain_tpage != 0 {
                    assert_eq!(
                        (hf.tile_ids[vi], tpage, clut),
                        (rec.terrain_tile, rec.terrain_tpage, rec.terrain_clut),
                        "{name}: cell ({c},{r}) ground texture must come from its record's \
                         +0x14/+0x15/+0x16 run"
                    );
                }
                assert_ne!(
                    tpage, 0,
                    "{name}: cell ({c},{r}) ground quad sampling tpage 0 (the framebuffer)"
                );
                let uvs: Vec<(u8, u8)> =
                    (vi..vi + 4).map(|i| (hf.uvs[i][0], hf.uvs[i][1])).collect();
                assert!(
                    res.vram.prim_has_texture_data(clut, tpage, &uvs),
                    "{name}: cell ({c},{r}) ground quad samples empty VRAM \
                     (clut 0x{clut:04X}, tpage 0x{tpage:04X}) - it would render as a hole"
                );
                checked += 1;
                vi += 4;
            }
        }
        eprintln!("[ground] {name}: {checked} ground quads, all record-textured + VRAM-backed");
    }
}

/// Every floor cell the player can stand on is surfaced by *something*: a ground
/// quad, or env-mesh geometry over it. This is what the missing terrain slab
/// broke - the ground gate was already right, the mesh under it was not.
#[test]
fn open_floor_cells_are_surfaced_by_ground_or_mesh() {
    let Some((_x, index)) = gate() else { return };
    let shared: Vec<Scene> = FIELD_SHARED_BLOCKS
        .iter()
        .filter_map(|n| Scene::load(&index, n).ok())
        .collect();
    let shared_refs: Vec<&Scene> = shared.iter().collect();

    for name in TOWN_SCENES {
        let scene = Scene::load(&index, name).expect("load scene");
        let map_idx = scene.field_map_index(&index).expect("field map");
        let map = index.entry_bytes_extended(map_idx).expect("map bytes");
        let (res, _) = SceneResources::build_targeted_with_options(
            &scene,
            &shared_refs,
            BuildOptions {
                kind: SceneLoadKind::Field,
                upload_all_tims: true,
                system_ui: None,
            },
        )
        .expect("scene resources");
        let env = field_env::env_pack_tmd_indices(&scene, &res);
        assert!(!env.is_empty(), "{name}: no env mesh pack");
        let lut = scene.field_floor_height_lut(&index).expect("lut");

        // Rasterise every drawn env mesh's triangles into the XZ cell grid.
        let n = GRID_DIM * SUB;
        let mut cover = vec![false; n * n];
        let mut lists = Vec::new();
        if let Ok(Some(p)) = scene.field_object_placements(&index) {
            lists.push(p);
        }
        if let Ok(Some(t)) = scene.field_terrain_tiles(&index) {
            lists.push(t);
        }
        for list in &lists {
            let (draws, _dropped) = field_env::resolve_env_draws(&env, list, lut);
            for d in &draws {
                let rt = &res.tmds[d.res_tmd];
                let vm = rt.build_filtered_vram_mesh(&res.vram);
                let cm = legaia_tmd::mesh::tmd_to_color_mesh(&rt.tmd, &rt.raw);
                let ang = (d.rot_y & 0xFFF) as f32 * std::f32::consts::TAU / 4096.0;
                let (s, cs) = ang.sin_cos();
                let xz = |p: &[f32; 3]| -> [f32; 2] {
                    [
                        cs * p[0] + s * p[2] + d.world_x as f32,
                        -s * p[0] + cs * p[2] + d.world_z as f32,
                    ]
                };
                let mut pts: Vec<[f32; 2]> = vm.positions.iter().map(xz).collect();
                let mut idx: Vec<u32> = vm.indices.clone();
                let base = pts.len() as u32;
                pts.extend(cm.positions.iter().map(xz));
                idx.extend(cm.indices.iter().map(|i| i + base));
                for t in idx.chunks_exact(3) {
                    let (Some(a), Some(b), Some(c)) = (
                        pts.get(t[0] as usize),
                        pts.get(t[1] as usize),
                        pts.get(t[2] as usize),
                    ) else {
                        continue;
                    };
                    rasterise(a, b, c, &mut cover, n);
                }
            }
        }

        // A cell is "open floor" when its collision byte has a floor tier and no
        // wall sub-cell bits: the player stands there, so it must have a surface.
        let mut open = 0usize;
        let mut bare = Vec::new();
        for r in 0..GRID_DIM {
            for c in 0..GRID_DIM {
                let coll = map[field_objects::COLLISION_GRID_OFFSET + r * GRID_DIM + c];
                if coll == 0 || coll & 0xF0 != 0 {
                    continue; // no floor, or (partly) walled off
                }
                if cell_at(&map, c, r) & field_objects::CELL_WALK_VISIBLE != 0 {
                    continue; // the ground layer surfaces it
                }
                open += 1;
                let covered = (0..SUB * SUB)
                    .filter(|k| cover[(r * SUB + k / SUB) * n + c * SUB + k % SUB])
                    .count();
                if covered * 2 < SUB * SUB {
                    bare.push((c, r, coll));
                }
            }
        }
        eprintln!(
            "[ground] {name}: {open} record-less open-floor cells, {} not mesh-surfaced",
            bare.len()
        );
        // The retail wedge cells: the terrain slab anchored at cell (30,17)
        // (object id 99, record +0x10 = 2) covers them. Any regression of the
        // mesh-id rule drops the slab and they go bare.
        for r in 19..=20 {
            for c in 28..=33 {
                assert!(
                    !bare.contains(&(
                        c,
                        r,
                        map[field_objects::COLLISION_GRID_OFFSET + r * GRID_DIM + c]
                    )),
                    "{name}: cell ({c},{r}) has floor but neither ground quad nor mesh over it \
                     (the terrain-slab wedge)"
                );
            }
        }
        assert!(
            bare.len() * 20 <= open,
            "{name}: {} of {open} record-less open-floor cells have no surface at all \
             (>5% - a hole in the ground): {:?}",
            bare.len(),
            &bare[..bare.len().min(8)]
        );
    }
}

/// The mesh id of a field object is the record's `+0x10`, for every object id -
/// no positional band. Pinned against retail's prim pool at town0c cell (30,17).
#[test]
fn object_mesh_id_is_the_record_field_for_every_id() {
    let Some((_x, index)) = gate() else { return };
    for name in TOWN_SCENES {
        let scene = Scene::load(&index, name).expect("load scene");
        let map_idx = scene.field_map_index(&index).expect("field map");
        let map = index.entry_bytes_extended(map_idx).expect("map bytes");
        let mut band_cells = 0usize;
        for r in 0..GRID_DIM {
            for c in 0..GRID_DIM {
                let cell = cell_at(&map, c, r);
                if cell == 0 {
                    continue;
                }
                let oi = cell & OBJECT_INDEX_MASK;
                let Some(rec) = ObjectRecord::parse(&map, oi as usize) else {
                    continue;
                };
                if (93..=118).contains(&oi) {
                    band_cells += 1;
                }
                if oi > 3 {
                    assert_eq!(
                        field_objects::pack_mesh_index(oi, &rec),
                        Some(rec.pack_index_field),
                        "{name}: cell ({c},{r}) object id {oi} must take its mesh from +0x10"
                    );
                }
            }
        }
        // Non-vacuous: Rim Elm really does place objects in the old "band".
        assert!(
            band_cells > 0,
            "{name}: no object ids in 93..=118 - the band-rule guard is vacuous"
        );
        // The slab that the band rule used to steal: id 99 -> +0x10 = 2.
        let cell = cell_at(&map, 30, 17);
        let rec = ObjectRecord::parse(&map, (cell & OBJECT_INDEX_MASK) as usize).expect("record");
        assert_eq!(
            cell & OBJECT_INDEX_MASK,
            99,
            "{name}: cell (30,17) object id"
        );
        assert_eq!(
            rec.pack_index_field, 2,
            "{name}: cell (30,17) record +0x10 (the terrain slab's env-pack mesh)"
        );
    }
}

/// Point-sample a triangle into the sub-cell coverage grid.
fn rasterise(a: &[f32; 2], b: &[f32; 2], c: &[f32; 2], cover: &mut [bool], n: usize) {
    let step = 128.0 / SUB as f32;
    let lo = |v: [f32; 3]| (v[0].min(v[1]).min(v[2]) / step).floor() as i32;
    let hi = |v: [f32; 3]| (v[0].max(v[1]).max(v[2]) / step).ceil() as i32;
    let (x0, x1) = (lo([a[0], b[0], c[0]]), hi([a[0], b[0], c[0]]));
    let (z0, z1) = (lo([a[1], b[1], c[1]]), hi([a[1], b[1], c[1]]));
    for gz in z0.max(0)..=z1.min(n as i32 - 1) {
        for gx in x0.max(0)..=x1.min(n as i32 - 1) {
            let px = (gx as f32 + 0.5) * step;
            let pz = (gz as f32 + 0.5) * step;
            let d1 = (px - b[0]) * (a[1] - b[1]) - (a[0] - b[0]) * (pz - b[1]);
            let d2 = (px - c[0]) * (b[1] - c[1]) - (b[0] - c[0]) * (pz - c[1]);
            let d3 = (px - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (pz - a[1]);
            let neg = d1 < 0.0 || d2 < 0.0 || d3 < 0.0;
            let pos = d1 > 0.0 || d2 > 0.0 || d3 > 0.0;
            if !(neg && pos) {
                cover[gz as usize * n + gx as usize] = true;
            }
        }
    }
}
