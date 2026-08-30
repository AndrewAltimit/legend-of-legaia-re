//! Disc-gated oracle for the VRChat/Unity world exporter
//! (`glb_export` + `scene_assembly` + `npc_catalog` - the kernels behind
//! `legaia-engine export-glb`): assemble town01, bake the three artifact
//! families, and structurally validate every `.glb` (header, JSON chunk,
//! meshes/nodes/animations) plus the manifest's cross-references.
//!
//! Skips silently when `extracted/` or `LEGAIA_DISC_BIN` is missing.

use legaia_engine_core::glb_export::{
    FloorSampler, GlbExportOptions, export_animated_prop_glbs, export_npc_glbs, export_world_glb,
    world_manifest,
};
use legaia_engine_core::npc_catalog::catalog_scene_npcs;
use legaia_engine_core::scene::{ProtIndex, Scene};
use legaia_engine_core::scene_assembly::assemble_field_scene;
use std::path::PathBuf;

fn extracted_dir() -> Option<PathBuf> {
    for p in ["extracted", "../../extracted"] {
        let d = PathBuf::from(p);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

/// Parse a `.glb`'s JSON chunk, asserting the container structure.
fn glb_json(glb: &[u8]) -> serde_json::Value {
    assert_eq!(&glb[0..4], b"glTF", "glb magic");
    let total = u32::from_le_bytes(glb[8..12].try_into().unwrap()) as usize;
    assert_eq!(total, glb.len(), "glb length field");
    let json_len = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
    assert_eq!(&glb[16..20], b"JSON");
    serde_json::from_slice(&glb[20..20 + json_len]).expect("glb JSON chunk parses")
}

#[test]
fn town01_world_export_bakes_all_three_artifact_families() {
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }

    let index = ProtIndex::open_extracted(&extracted).expect("open ProtIndex");
    let a = assemble_field_scene(&index, "town01").expect("assemble town01");
    let scene = Scene::load(&index, "town01").expect("load town01");
    let opts = GlbExportOptions {
        scale: 1.0 / 64.0,
        include_sky: false,
    };

    // --- World glb: ground + terrain + placements, sky shells dropped. ---
    let world = export_world_glb(&scene, &a, &opts).expect("world glb");
    assert!(world.ground_quads > 0, "town01 has a walk floor grid");
    assert!(world.sky_hidden > 0, "town01's sky shells are filtered");
    assert!(
        world.instance_count > 100,
        "town01 draws >100 instances, got {}",
        world.instance_count
    );
    let wj = glb_json(&world.glb);
    let meshes = wj["meshes"].as_array().expect("meshes array");
    assert_eq!(meshes.len(), world.mesh_count);
    assert_eq!(meshes[0]["name"], "ground");
    // One baked atlas material for the whole scene.
    assert_eq!(wj["images"].as_array().map(Vec::len), Some(1));
    // Every instance became a node under the root.
    assert!(wj["nodes"].as_array().expect("nodes").len() > world.instance_count);

    // The bind resolve carries clips onto placed draws (the windmill class);
    // without it every placement reads anim 0 and the prop split is empty.
    assert!(
        a.placements.iter().any(|d| d.anim_id != 0),
        "some town01 placements are clip-bound"
    );

    // --- NPC glbs: animated, with the spawn clip leading. ---
    let catalog = catalog_scene_npcs(&index, "town01", &a.res, None).expect("npc catalog");
    assert!(
        catalog.entries.len() > 20,
        "town01 catalogs a real NPC roster, got {}",
        catalog.entries.len()
    );
    let npcs = export_npc_glbs(&scene, &a, &catalog);
    assert!(npcs.len() > 20, "most entries bake, got {}", npcs.len());
    let animated = npcs
        .iter()
        .find(|n| !n.clips.is_empty())
        .expect("at least one NPC carries clips");
    let nj = glb_json(&animated.glb);
    assert_eq!(
        nj["animations"].as_array().map(Vec::len),
        Some(animated.clips.len()),
        "every listed clip baked as a glTF animation"
    );

    // --- Animated props (windmill sails etc.): clip frames > 1, instanced. ---
    let props = export_animated_prop_glbs(&scene, &a, &opts);
    assert!(!props.is_empty(), "town01 has animated props");
    for p in &props {
        assert!(p.frame_count > 1);
        assert!(!p.instances.is_empty());
        let pj = glb_json(&p.glb);
        assert_eq!(pj["animations"].as_array().map(Vec::len), Some(1));
    }

    // --- Manifest: files cross-reference the baked sets. ---
    let floor = FloorSampler::build(&index, &scene);
    let m = world_manifest(
        &a,
        &opts,
        &world,
        "town01.glb",
        Some(&catalog),
        &npcs,
        &props,
        &floor,
    );
    assert_eq!(m["scene"], "town01");
    assert_eq!(m["npcs"].as_array().map(Vec::len), Some(npcs.len()));
    assert_eq!(
        m["animated_props"].as_array().map(Vec::len),
        Some(props.len())
    );
    for n in m["npcs"].as_array().unwrap() {
        let f = n["file"].as_str().unwrap();
        assert!(f.starts_with("npcs/") && f.ends_with(".glb"));
        assert_eq!(n["position"].as_array().map(Vec::len), Some(3));
    }
}
