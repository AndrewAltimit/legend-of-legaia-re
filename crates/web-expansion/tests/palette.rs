//! The palette's exports: the mesh wire form, the catalog JSON and the
//! starter map behind **+ New map**, validated.

use legaia_web_expansion::catalog::pack_scene_meshes;
use legaia_web_expansion::session::Core;
use legaia_web_expansion::starter::starter_map;

/// A pack with only a manifest and the starter map.
fn pack_with_starter(core: &mut Core) -> String {
    let mut pack = core.start_fresh("starter_test", "Starter");
    pack["maps"] = serde_json::json!({ "first": starter_map("First") });
    pack.to_string()
}

#[test]
fn starter_map_validates_and_exports_with_no_disc_bytes() {
    let mut core = Core::new();
    let json = pack_with_starter(&mut core);
    let errors: Vec<_> = core
        .validate(&json)
        .into_iter()
        .filter(|i| i.severity == "error")
        .collect();
    assert!(errors.is_empty(), "{errors:?}");
    // The export's no-disc-bytes rule runs on it too, and it reads back.
    assert!(core.export_bundle(&json).is_ok());
}

#[test]
fn starter_map_has_ground_objects_and_a_start() {
    let m = starter_map("First");
    assert!(m["terrain"]["shapes"].as_array().unwrap().len() >= 2);
    assert!(m["props"].as_array().unwrap().len() >= 2);
    assert_eq!(m["spawns"].as_array().unwrap().len(), 1);
    assert_eq!(m["name"], "First");
}

#[test]
fn starter_map_meshes_are_catalogued() {
    let cat = legaia_expansion_format::mesh_catalog::catalog();
    let m = starter_map("First");
    for p in m["props"].as_array().unwrap() {
        let object = p["mesh"]["object"].as_u64().unwrap() as u16;
        let e = cat
            .get(legaia_web_expansion::starter::STARTER_SCENE, object)
            .unwrap_or_else(|| panic!("{p} is not catalogued"));
        // A starter prop is a thing, not a ground shell or a wall run.
        assert!(!format!("{:?}", e.kind).contains("Terrain"), "{p}");
    }
}

#[test]
fn mesh_catalog_export_lists_names_and_kinds() {
    let core = Core::new();
    let rows: serde_json::Value = serde_json::from_str(&core.mesh_catalog_json()).unwrap();
    let rows = rows.as_array().unwrap();
    assert!(!rows.is_empty());
    assert!(
        rows.iter()
            .all(|r| r["label"].is_string() && r["kind"].is_string())
    );
    // No disc, no key.
    assert_eq!(core.disc_key(), "");
}

#[test]
fn mesh_wire_form_is_padded_and_counted() {
    let mesh = legaia_tmd::mesh::VramMesh {
        positions: vec![[1.0, 2.0, 3.0]; 3],
        uvs: vec![[1, 2]; 3],
        cba_tsb: vec![[7, 8]; 3],
        indices: vec![0, 1, 2],
        normals: Vec::new(),
        colors: Vec::new(),
    };
    let flat = vec![9u8; 12];
    let out = pack_scene_meshes(&[0u8; 6], &[(mesh, flat)]);
    assert_eq!(&out[..4], b"LXMS");
    assert_eq!(u32::from_le_bytes(out[4..8].try_into().unwrap()), 1);
    assert_eq!(u32::from_le_bytes(out[8..12].try_into().unwrap()), 6);
    assert_eq!(u32::from_le_bytes(out[12..16].try_into().unwrap()), 3);
    assert_eq!(u32::from_le_bytes(out[16..20].try_into().unwrap()), 3);
    assert_eq!(u32::from_le_bytes(out[20..24].try_into().unwrap()), 12);
    // Header 24, VRAM 6 + 2 pad, positions 36, UVs 6 + 2 pad, words 12,
    // indices 12, flat 12.
    assert_eq!(out.len(), 24 + 8 + 36 + 8 + 12 + 12 + 12);
}

/// With the disc: the starter's coordinates resolve (no error against the
/// resolver), and the wire form carries the VRAM and town01's pack.
#[test]
fn starter_map_and_meshes_on_the_disc() {
    let Ok(disc) = std::env::var("LEGAIA_DISC_BIN") else {
        return;
    };
    let Ok(image) = std::fs::read(&disc) else {
        return;
    };
    let mut core = Core::new();
    core.open_disc(&image).unwrap();
    let json = pack_with_starter(&mut core);
    let errors: Vec<_> = core
        .validate(&json)
        .into_iter()
        .filter(|i| i.severity == "error")
        .collect();
    assert!(errors.is_empty(), "{errors:?}");
    assert_eq!(core.disc_key().len(), 48);
    let wire = core.scene_meshes("town01").unwrap();
    assert_eq!(&wire[..4], b"LXMS");
    assert_eq!(u32::from_le_bytes(wire[8..12].try_into().unwrap()), 1 << 20);
    assert!(u32::from_le_bytes(wire[4..8].try_into().unwrap()) > 75);
}
