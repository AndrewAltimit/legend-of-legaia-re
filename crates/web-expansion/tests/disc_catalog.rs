//! Disc-gated: the picker catalog and the disc half of validation over the
//! real NA disc. Skips (and passes) when `LEGAIA_DISC_BIN` is unset.

use std::path::Path;

use legaia_expansion_format::read_dir_files;
use legaia_web_expansion::session::Core;

fn disc() -> Option<Vec<u8>> {
    let path = std::env::var_os("LEGAIA_DISC_BIN")?;
    match std::fs::read(&path) {
        Ok(b) => Some(b),
        Err(e) => {
            eprintln!("LEGAIA_DISC_BIN unreadable ({e}); skipping");
            None
        }
    }
}

#[test]
fn catalog_and_rebirth_against_the_disc() {
    let Some(image) = disc() else {
        eprintln!("LEGAIA_DISC_BIN unset; skipping");
        return;
    };
    let mut core = Core::new();
    core.open_disc(&image).unwrap();
    let cat: serde_json::Value = serde_json::from_str(&core.catalog_json()).unwrap();

    let monsters = cat["monsters"].as_array().unwrap();
    assert!(monsters.len() > 150, "{} monsters", monsters.len());
    assert!(monsters.iter().all(|m| m["id"].as_u64().unwrap() >= 1));
    assert!(cat["items"].as_array().unwrap().len() > 100);
    let scenes = cat["scenes"].as_array().unwrap();
    for s in ["town01", "tower", "chitei2"] {
        assert!(scenes.iter().any(|r| r["name"] == s), "{s}");
    }
    assert_eq!(cat["bgm"].as_array().unwrap().len(), 81);

    // The stage picker's lists: each block's stage streams as the engine's
    // own scan finds them, and the pick a fight there takes by default.
    let prot = legaia_iso::image::extract_prot_dat(&image).unwrap();
    let cdname = legaia_iso::image::extract_cdname_txt(&image);
    let index = legaia_engine_scene::scene::ProtIndex::from_bytes(prot, cdname.as_deref()).unwrap();
    let mut staged = 0;
    for s in scenes {
        let name = s["name"].as_str().unwrap();
        let stages: Vec<u32> = s["stages"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_u64().unwrap() as u32)
            .collect();
        assert_eq!(stages, index.battle_stage_entries(name), "{name}");
        if let Some(e) = index.battle_stage_entry_for_scene(name) {
            staged += 1;
            assert_eq!(
                stages[s["stage_pick"].as_u64().unwrap() as usize],
                e,
                "{name}"
            );
        }
    }
    assert!(staged > 40, "{staged} blocks with a battle stage");

    // Rebirth resolves on the disc: no coordinate error.
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs/rebirth");
    let loaded = core.load_files(read_dir_files(&root).unwrap());
    let errors: Vec<_> = loaded["issues"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|i| i["severity"] == "error")
        .collect();
    assert!(errors.is_empty(), "{errors:#?}");

    // The tower map's base scene has a geometry pack holding the props'
    // meshes, and the preview reads its own ladder.
    let objs: serde_json::Value =
        serde_json::from_str(&core.scene_objects_json("tower").unwrap()).unwrap();
    let n = objs["objects"].as_array().unwrap().len();
    assert!(n > 19, "tower has {n} pack meshes");
    assert!(objs["entry"].is_u64());
    // The texture-cell picker's list: the cells the pack's meshes sample,
    // most used first, each a valid atlas cell.
    let cells = objs["cells"].as_array().unwrap();
    assert!(cells.len() > 4, "tower samples {} cells", cells.len());
    assert!(cells.iter().all(|c| c["tile"].as_u64().unwrap() < 64));
    assert!(cells[0]["prims"].as_u64() >= cells[cells.len() - 1]["prims"].as_u64());
    let map = &loaded["pack"]["maps"]["rebirth_tower_1f"];
    let p = core.map_preview(&map.to_string());
    assert!(p["error"].is_null(), "{p}");
}

/// The resting check in the workbench: the catalog measures the meshes off
/// the disc, so a raised torch is a warning in the Checks list (at its field
/// path, for click-to-field) and the painter's preview says where each item
/// stands and what is under it.
#[test]
fn the_workbench_measures_where_things_rest() {
    let Some(image) = disc() else {
        eprintln!("LEGAIA_DISC_BIN unset; skipping");
        return;
    };
    let mut core = Core::new();
    core.open_disc(&image).unwrap();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs/rebirth");
    let loaded = core.load_files(read_dir_files(&root).unwrap());
    let mut pack = loaded["pack"].clone();
    let rest_warnings = |issues: &[legaia_web_expansion::session::IssueRow]| {
        issues
            .iter()
            .filter(|i| {
                i.path == "maps/isles_village.toml" && i.message.contains("over the ground")
            })
            .map(|i| i.field.clone())
            .collect::<Vec<_>>()
    };
    assert!(rest_warnings(&core.validate(&pack.to_string())).is_empty());
    // Lift a corner torch of the lantern table off the ground.
    let props = pack["maps"]["isles_village"]["props"]
        .as_array_mut()
        .unwrap();
    let k = props
        .iter()
        .position(|p| p["id"] == "lantern_table_nw")
        .unwrap();
    props[k]["offset"] = serde_json::json!([0, 40, 0]);
    let at = format!("props[{k}]");
    assert_eq!(
        rest_warnings(&core.validate(&pack.to_string())),
        vec![at.clone()]
    );

    let map = &pack["maps"]["isles_village"];
    let p = core.map_preview(&map.to_string());
    assert!(p["error"].is_null(), "{p}");
    assert!(
        p["rest_issues"]
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["at"] == at)
    );
    let rest = p["rest"].as_array().unwrap();
    let torch = rest.iter().find(|r| r["at"] == at).unwrap();
    assert!(torch["base"].is_number(), "{torch}");
    // The table stands on the floor with its top measured.
    let table = rest.iter().find(|r| r["id"] == "lantern_table").unwrap();
    assert!(table["top"].as_f64().unwrap() > 40.0, "{table}");
}
