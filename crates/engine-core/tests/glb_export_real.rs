//! Disc-gated oracle for the VRChat/Unity world exporter
//! (`glb_export` + `scene_assembly` + `npc_catalog` - the kernels behind
//! `legaia-engine export-glb`): assemble town01, bake the three artifact
//! families, and structurally validate every `.glb` (header, JSON chunk,
//! meshes/nodes/animations) plus the manifest's cross-references.
//!
//! Skips silently when `extracted/` or `LEGAIA_DISC_BIN` is missing.

use legaia_engine_core::glb_export::{
    FloorSampler, GlbExportOptions, export_animated_prop_glbs, export_npc_glbs,
    export_scene_traversal, export_world_glb, world_manifest,
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
    let world = export_world_glb(&index, &scene, &a, &opts).expect("world glb");
    assert!(world.ground_quads > 0, "town01 has a walk floor grid");
    assert!(world.sky_hidden > 0, "town01's sky shells are filtered");
    // The shoreline morph bake: town01's populated VDF pack arms the
    // scene-entry pulse, so the world glb carries baked morph targets and a
    // looping `vdf_pulse` weights animation.
    assert!(
        world.morph_mesh_count > 0,
        "town01's shoreline mesh carries baked VDF morph targets"
    );
    assert!(
        world.morph_loop_seconds > 1.0,
        "the vdf_pulse loop has a real period, got {}",
        world.morph_loop_seconds
    );
    {
        let wj = glb_json(&world.glb);
        let anims = wj["animations"].as_array().expect("world animations");
        assert_eq!(anims.len(), 1);
        assert_eq!(anims[0]["name"], "vdf_pulse");
        assert!(
            !anims[0]["channels"].as_array().unwrap().is_empty(),
            "vdf_pulse animates at least one instance node"
        );
        let has_targets = wj["meshes"].as_array().unwrap().iter().any(|m| {
            m["primitives"][0]["targets"]
                .as_array()
                .is_some_and(|t| !t.is_empty())
        });
        assert!(has_targets, "a world mesh carries glTF morph targets");
    }
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
    let npcs = export_npc_glbs(&scene, &a, &catalog, &opts);
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
    // The loop/one-shot split is real data, not a constant: town01 carries
    // both a seamless loop (env 32's sway returns to frame 0 exactly) and
    // one-shot swings (the door/cupboard family ends 77..120 degrees out).
    assert!(
        props.iter().any(|p| p.cyclic) && props.iter().any(|p| !p.cyclic),
        "town01 has both cyclic and one-shot prop clips"
    );

    // --- Traversal: both doorway families resolve for Rim Elm, with
    // trigger boxes capsule-reach verified against the baked world glb. ---
    let floor = FloorSampler::build(&index, &scene);
    let traversal = export_scene_traversal(&index, &scene, &floor, &opts, Some(&world.glb));
    let kinds: Vec<&str> = traversal
        .teleports
        .iter()
        .filter_map(|t| t["kind"].as_str())
        .collect();
    assert!(
        kinds.contains(&"map"),
        "town01 has kind-0 map doors (Vahn's house exit)"
    );
    assert!(
        kinds.contains(&"script"),
        "town01 has script doors (the IN/OUT pairs)"
    );
    assert!(
        traversal
            .teleports
            .iter()
            .any(|t| t["kind"] == "script" && !t["facing_dir"].is_null()),
        "at least one script door carries an authored arrival facing"
    );
    assert!(
        traversal
            .portals
            .iter()
            .any(|p| p["target_scene"] == "map01"),
        "the south gate portal to map01 is exported"
    );

    // --- Manifest: files cross-reference the baked sets. ---
    let m = world_manifest(
        &a,
        &opts,
        &world,
        "town01.glb",
        Some(&catalog),
        &npcs,
        &props,
        &floor,
        &traversal,
    );
    assert_eq!(m["scene"], "town01");
    assert_eq!(
        m["teleports"].as_array().map(Vec::len),
        Some(traversal.teleports.len())
    );
    // Rim Elm's house doors stand on their own doorway-teleport triggers
    // (is_door), the gate leaves on the exit band (near_portal) - and the
    // windmills on neither, so they keep looping.
    let insts: Vec<&serde_json::Value> = m["animated_props"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|p| p["instances"].as_array().unwrap())
        .collect();
    let doors = insts.iter().filter(|i| i["is_door"] == true).count();
    let leaves = insts.iter().filter(|i| !i["near_portal"].is_null()).count();
    assert!(doors >= 4, "house doors tag is_door (got {doors})");
    assert!(leaves >= 1, "gate leaves tag near_portal (got {leaves})");
    assert!(
        doors + leaves < insts.len(),
        "windmills and other looping props stay untagged"
    );
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

#[test]
fn equipment_item_export_bakes_named_animated_glbs() {
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }
    let index = ProtIndex::open_extracted(&extracted).expect("open ProtIndex");
    let scus = std::fs::read(extracted.join("SCUS_942.54")).ok();

    let export =
        legaia_engine_core::glb_export::export_equipment_item_glbs(&index, scus.as_deref())
            .expect("items export");

    // The four player files together offer well over a hundred equippable
    // records, spread across all four characters.
    assert!(
        export.items.len() > 100,
        "expected >100 item records, got {}",
        export.items.len()
    );
    // Terra's player file (PLAYER4) offers no equippable records - she has
    // no changeable equipment in retail - so exactly three characters yield.
    for who in ["Vahn", "Noa", "Gala"] {
        assert!(
            export.items.iter().any(|i| i.character == who),
            "no items for {who}"
        );
    }
    assert!(
        !export.items.iter().any(|i| i.character == "Terra"),
        "Terra unexpectedly offers equippable records"
    );

    let mut alone = 0usize;
    let mut with_limb = 0usize;
    let mut named = 0usize;
    let mut animated = 0usize;
    for it in &export.items {
        if !it.glb_with_limb.is_empty() {
            with_limb += 1;
            let j = glb_json(&it.glb_with_limb);
            assert!(
                !j["meshes"].as_array().unwrap().is_empty(),
                "{}: with-limb glb has no meshes",
                it.file_stem
            );
        }
        if !it.glb_alone.is_empty() {
            alone += 1;
            let j = glb_json(&it.glb_alone);
            // Every item file bakes the loadout's clip bank (action bank +
            // weapon swings) so the piece moves with the limb it rides.
            let anims = j["animations"].as_array().map_or(0, Vec::len);
            if anims > 0 {
                animated += 1;
            }
            assert_eq!(
                anims, it.clip_count,
                "{}: baked animation count vs manifest clip count",
                it.file_stem
            );
            // The item's display name labels a node when SCUS was readable.
            if let Some(name) = &it.name {
                named += 1;
                let json_text = j.to_string();
                assert!(
                    json_text.contains(name.trim()),
                    "{}: item name {name:?} not in glb JSON",
                    it.file_stem
                );
            }
        }
    }
    // The vast majority of records survive the item-alone cut; every one
    // keeps the record-keeping with-limb export.
    assert!(alone > 80, "only {alone} item-alone glbs");
    assert!(with_limb > 100, "only {with_limb} with-limb glbs");
    assert!(animated > 80, "only {animated} alone glbs carry clips");
    if scus.is_some() {
        assert!(named > 80, "only {named} alone glbs carry a SCUS name");
    }

    // Manifest cross-references: every listed file name matches the stem
    // and flavour of the record it came from.
    let manifest = legaia_engine_core::glb_export::items_manifest(&export);
    let rows = manifest["items"].as_array().expect("items array");
    assert_eq!(rows.len(), export.items.len());
    for (row, it) in rows.iter().zip(&export.items) {
        if !it.glb_alone.is_empty() {
            assert_eq!(
                row["alone"].as_str().unwrap(),
                format!("{}_alone.glb", it.file_stem)
            );
        } else {
            assert!(row["alone"].is_null());
        }
    }
}
