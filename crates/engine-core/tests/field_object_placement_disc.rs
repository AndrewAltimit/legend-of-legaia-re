//! Disc-gated: the field static-object placement table (`FUN_8003A55C`)
//! parsed from `town01`'s field map file reproduces the known building
//! anchors. Validates [`legaia_asset::field_objects`] + the
//! [`Scene::field_object_placements`] wiring against real disc bytes.
//!
//! Skips silently when `extracted/` or `LEGAIA_DISC_BIN` is missing.
use std::path::PathBuf;

use legaia_engine_core::scene::{Scene, SceneHost};
use legaia_engine_core::scene_resources::{
    BuildOptions, FIELD_SHARED_BLOCKS, SceneLoadKind, SceneResources,
};

fn extracted_dir() -> Option<PathBuf> {
    for d in ["extracted", "../../extracted"] {
        let p = PathBuf::from(d);
        if p.join("PROT.DAT").exists() && p.join("CDNAME.TXT").exists() {
            return Some(p);
        }
    }
    None
}

#[test]
fn town01_placements_reproduce_building_anchors() {
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }

    let host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    let index = host.index.clone();
    let scene = Scene::load(&index, "town01").expect("load town01");

    let placements = scene
        .field_object_placements(&index)
        .expect("read placements")
        .expect("town01 has a field map");

    // town01's environment is laid out by a few dozen placed objects (the
    // retail static-object actor count via FUN_8003A55C). A regression that
    // mis-reads the object grid / record table would collapse this to ~0 or
    // explode it across the whole 16384-tile grid.
    assert!(
        (20..=200).contains(&placements.len()),
        "town01 placed-object count out of plausible range: {} (expected 20..=200)",
        placements.len()
    );

    // Vahn's house: object id 137, anchor tile (col 38, row 25), world
    // (4864, _, 3208) - byte-validated against the live town01 actor.
    let vahns_house = placements
        .iter()
        .find(|p| p.obj_idx == 137)
        .unwrap_or_else(|| panic!("town01 placements missing object id 137 (Vahn's house)"));
    assert_eq!(
        (vahns_house.col, vahns_house.row),
        (38, 25),
        "Vahn's house anchor tile"
    );
    assert_eq!(
        (vahns_house.world_x, vahns_house.world_z),
        (4864, 3208),
        "Vahn's house world position"
    );
    // Mesh selection (byte-verified against the live actor's geometry): the
    // house (id 137, >=120) draws scene-pack mesh 36 via the record's +0x10
    // field; the windmill (id 96, in the 93..=118 band) draws mesh 91 (96-5).
    assert_eq!(vahns_house.pack_index, Some(36), "Vahn's house pack mesh");

    // Floor-height LUT: Vahn's house tile floor nibble is 6, lut[6] = 192, so
    // its world Y is -192 (matches the live actor). Validates the MAN-header
    // LUT accessor + the `-lut[nibble] + y_off` elevation formula.
    let lut = scene
        .field_floor_height_lut(&index)
        .expect("read floor LUT")
        .expect("town01 MAN carries a floor LUT");
    let nib = vahns_house
        .floor_nibble
        .expect("Vahn's house tile floor nibble");
    let world_y = -(lut[(nib & 0x0F) as usize] as i32) + vahns_house.y_off as i32;
    assert_eq!(world_y, -192, "Vahn's house world Y from floor LUT");
    if let Some(windmill) = placements.iter().find(|p| p.obj_idx == 96) {
        assert_eq!(windmill.pack_index, Some(91), "windmill pack mesh (96-5)");
    }
    if let Some(obj230) = placements.iter().find(|p| p.obj_idx == 230) {
        assert_eq!(obj230.pack_index, Some(15), "obj230 pack mesh (+0x10)");
    }

    // Every placement lands on-grid with the placed flag set, and within the
    // ~16384-unit town extent (128 tiles * 128 units).
    for p in &placements {
        assert!(p.flags & legaia_asset::field_objects::FLAG_PLACED != 0);
        assert!((0..128).contains(&p.col) && (0..128).contains(&p.row));
        assert!(
            (0..0x4040).contains(&p.world_x) && (0..0x4040).contains(&p.world_z),
            "placement off the town extent: ({}, {})",
            p.world_x,
            p.world_z
        );
    }

    eprintln!(
        "town01: {} placed static objects; Vahn's house @ tile (38,25) -> (4864, 3208)",
        placements.len()
    );
}

/// The render mapping is sound: every placement's `pack_index` lands inside
/// the scene_asset_table TMD pack the engine loads, so the play-window
/// static-draw pass resolves a real mesh for each placed object.
#[test]
fn town01_placement_pack_indices_resolve_in_loaded_pack() {
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }

    let host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    let index = host.index.clone();
    let scene = Scene::load(&index, "town01").expect("load town01");

    let placements = scene
        .field_object_placements(&index)
        .expect("read placements")
        .expect("field map");

    // The environment meshes are the scene bundle entry's TMD pack, in scan
    // order (the same indexing `pack_index` uses).
    let bundle =
        legaia_engine_core::scene_bundle::find_bundle(&scene).expect("town01 has a scene bundle");
    let bundle_entry = bundle.entry_idx();

    let mut shared: Vec<Scene> = Vec::new();
    for name in FIELD_SHARED_BLOCKS {
        if let Ok(s) = Scene::load(&index, name) {
            shared.push(s);
        }
    }
    let refs: Vec<&Scene> = shared.iter().collect();
    let (res, _) = SceneResources::build_targeted_with_options(
        &scene,
        &refs,
        BuildOptions {
            kind: SceneLoadKind::Field,
            upload_all_tims: true,
        },
    )
    .expect("build town01 field");

    let env_mesh_count = res
        .tmds
        .iter()
        .filter(|t| t.entry_idx == bundle_entry)
        .count();
    assert!(
        env_mesh_count >= 100,
        "town01 env mesh pack unexpectedly small: {env_mesh_count}"
    );

    let mut resolved = 0usize;
    for p in &placements {
        if let Some(pack_index) = p.pack_index {
            assert!(
                (pack_index as usize) < env_mesh_count,
                "object {} pack_index {} out of range (env pack has {})",
                p.obj_idx,
                pack_index,
                env_mesh_count
            );
            resolved += 1;
        }
    }
    assert!(
        resolved >= 10,
        "expected most placements to resolve a mesh; only {resolved} did"
    );
    eprintln!(
        "town01: {resolved}/{} placements resolve into the {env_mesh_count}-mesh pack",
        placements.len()
    );
}

/// Pin *why* a handful of town01 placements don't render in the field
/// pass, so the "≈8/46 placements drop" gap stays characterised by cause
/// (not just counted). The placement render path builds each mesh with the
/// textured-only VRAM filter; an empty build drops the placement. There are
/// two distinct causes, and they are NOT the same fix:
///
///   * **untextured props** - the mesh is all flat / gouraud (per-vertex RGB,
///     no UVs), so the textured builder skips every prim. These are now
///     recovered by the engine's vertex-colour path: `tmd_to_color_mesh` builds
///     a [`legaia_tmd::mesh::ColorMesh`] from the per-prim colour blocks, which
///     play-window uploads + draws on the colour pipeline (asserted below).
///   * **missing-CLUT props** - the mesh IS textured, but its prims sample a
///     CLUT row the field VRAM pre-pass didn't upload, so the coverage filter
///     correctly drops them (rendering them would show flat `CLUT[0]`).
///     Recovering these is a VRAM-coverage question, not a shading one.
///
/// This corrects the earlier "all ≈8 are fully-untextured props" reading: on
/// town01 only 2 of the 8 dropped placements are untextured; the other 6 are
/// textured prims missing their CLUT. Numbers are exact disc invariants.
#[test]
fn town01_dropped_placements_split_untextured_vs_missing_clut() {
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }
    let host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    let index = host.index.clone();
    let scene = Scene::load(&index, "town01").expect("load town01");
    let placements = scene
        .field_object_placements(&index)
        .expect("read placements")
        .expect("field map");
    let bundle =
        legaia_engine_core::scene_bundle::find_bundle(&scene).expect("town01 has a scene bundle");
    let bundle_entry = bundle.entry_idx();
    let mut shared: Vec<Scene> = Vec::new();
    for name in FIELD_SHARED_BLOCKS {
        if let Ok(s) = Scene::load(&index, name) {
            shared.push(s);
        }
    }
    let refs: Vec<&Scene> = shared.iter().collect();
    let (res, _) = SceneResources::build_targeted_with_options(
        &scene,
        &refs,
        BuildOptions {
            kind: SceneLoadKind::Field,
            upload_all_tims: true,
        },
    )
    .expect("build town01 field");
    let env_tmds: Vec<_> = res
        .tmds
        .iter()
        .filter(|t| t.entry_idx == bundle_entry)
        .collect();

    let mut placements_drawn = 0usize;
    let mut placements_dropped = 0usize;
    let mut dropped_untextured = 0usize;
    let mut dropped_missing_clut = 0usize;
    // pack_index -> (obj_idx, is_untextured)
    let mut dropped_meshes = std::collections::BTreeMap::new();
    for p in &placements {
        let Some(pi) = p.pack_index else { continue };
        let Some(rtmd) = env_tmds.get(pi as usize) else {
            continue;
        };
        let (vmesh, stats) = rtmd.build_filtered_vram_mesh_stats(&res.vram);
        if !vmesh.indices.is_empty() {
            placements_drawn += 1;
            continue;
        }
        placements_dropped += 1;
        let untextured = stats.dropped_by_filter == 0 && stats.skipped_untextured > 0;
        if untextured {
            dropped_untextured += 1;
        } else {
            dropped_missing_clut += 1;
        }
        dropped_meshes.entry(pi).or_insert((p.obj_idx, untextured));
    }
    eprintln!(
        "town01 placement render: {placements_drawn} drawn, {placements_dropped} dropped \
         ({dropped_untextured} untextured-prop, {dropped_missing_clut} missing-CLUT) \
         across {} meshes",
        dropped_meshes.len()
    );

    // 38 of 46 placements draw; the 8 that don't split 2 untextured + 6
    // missing-CLUT across exactly three distinct env-pack meshes.
    assert_eq!(placements_drawn, 38, "town01 placements that draw");
    assert_eq!(placements_dropped, 8, "town01 placements dropped");
    assert_eq!(dropped_untextured, 2, "dropped because all-untextured");
    assert_eq!(dropped_missing_clut, 6, "dropped because CLUT not resident");

    // The three distinct dropped meshes (pack index -> obj id, untextured?).
    let expect: &[(u16, u16, bool)] = &[
        (31, 315, true),  // untextured prop
        (74, 347, false), // textured, CLUT not uploaded
        (109, 114, true), // untextured prop
    ];
    assert_eq!(
        dropped_meshes.len(),
        expect.len(),
        "distinct dropped env-pack meshes"
    );
    for &(pi, obj, untextured) in expect {
        let got = dropped_meshes
            .get(&pi)
            .unwrap_or_else(|| panic!("expected dropped mesh pack[{pi}]"));
        assert_eq!(got.0, obj, "obj id for dropped mesh pack[{pi}]");
        assert_eq!(got.1, untextured, "untextured? for dropped mesh pack[{pi}]");
    }

    // The textured-but-dropped mesh (pack[74], obj 347) is dropped purely
    // because its 4 prims sample an un-uploaded CLUT row - it is NOT an
    // untextured prop, so a per-vertex-RGB fallback would NOT recover it.
    let (_m, reasons) = env_tmds[74].build_filtered_vram_mesh_reasoned(&res.vram);
    assert_eq!(reasons.kept, 0, "pack[74] keeps no prims");
    assert_eq!(reasons.missing_clut, 4, "pack[74] drops = missing CLUT");
    assert_eq!(reasons.missing_texture_page, 0);
    assert_eq!(reasons.clut_depth_mismatch, 0);
    assert_eq!(reasons.skipped_untextured, 0, "pack[74] IS textured");

    // ROOT CAUSE (pinned): pack[74]'s 4 prims all sample the same texture page
    // (960, 256) + CLUT row 510 - i.e. the bottom-right VRAM band x in
    // [896, 1024], y = 256 that the Field pre-pass excludes by design (the
    // character / party-texture region; this is the same band that shows up as
    // the documented town01 static-VRAM residue at x=896..1024,y=256). Even
    // `upload_all_tims: true` doesn't fill CLUT row 510, so the texture isn't a
    // static scene TIM at all - it's a runtime targeted upload the field
    // pre-pass doesn't model. So this mesh is NOT recoverable by a render-filter
    // tweak; recovering it needs that runtime texture-band upload reproduced.
    for o in &env_tmds[74].tmd.objects {
        let groups = legaia_tmd::legaia_prims::iter_groups_lenient(
            &env_tmds[74].raw,
            o.primitives_byte_offset,
            o.primitives_byte_size,
        );
        for g in &groups {
            for prim in g.prims.iter().filter(|p| !p.uvs.is_empty()) {
                let (cx, cy) = prim.cba_xy();
                let (tx, ty, _depth, _abr) = prim.tpage_xy();
                assert!(
                    (896..=1024).contains(&tx) && ty == 256,
                    "pack[74] prim tpage ({tx},{ty}) should sit in the excluded \
                     character/party-texture band x=[896,1024], y=256"
                );
                assert_eq!(cy, 510, "pack[74] prim CLUT row");
                let _ = cx;
            }
        }
    }

    // Vertex-colour recovery: the two untextured props (pack 31 / 109) now
    // build a non-empty `ColorMesh` from their per-prim colour blocks, so the
    // engine's colour pipeline renders them (the +2 placements the field render
    // recovers). The missing-CLUT textured mesh (pack 74) yields an EMPTY colour
    // mesh - it is textured, so the colour path correctly does NOT recover it.
    let cm31 = legaia_tmd::mesh::tmd_to_color_mesh(&env_tmds[31].tmd, &env_tmds[31].raw);
    let cm109 = legaia_tmd::mesh::tmd_to_color_mesh(&env_tmds[109].tmd, &env_tmds[109].raw);
    let cm74 = legaia_tmd::mesh::tmd_to_color_mesh(&env_tmds[74].tmd, &env_tmds[74].raw);
    assert!(
        !cm31.is_empty() && cm31.colors.len() == cm31.positions.len(),
        "pack[31] untextured prop recovers a colour mesh ({} verts)",
        cm31.positions.len()
    );
    assert!(
        !cm109.is_empty() && cm109.colors.len() == cm109.positions.len(),
        "pack[109] untextured prop recovers a colour mesh ({} verts)",
        cm109.positions.len()
    );
    assert!(
        cm74.is_empty(),
        "pack[74] is textured (missing CLUT) - colour path must not recover it"
    );
    // At least one decoded colour is non-black (the props aren't all (0,0,0)).
    assert!(
        cm31.colors
            .iter()
            .chain(cm109.colors.iter())
            .any(|&c| c != [0, 0, 0]),
        "recovered props carry real (non-black) per-vertex colours"
    );
}
