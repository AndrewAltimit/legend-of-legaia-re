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
