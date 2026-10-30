//! The flagship pack through the workbench session, disc-free: load the
//! `packs/rebirth/` tree, take the pack as the page's JSON, edit it the way
//! the editors do, export the `.lxpack` and read it back.

use std::path::Path;

use legaia_expansion_format::{ExpansionPack, PackFiles, bundle, read_dir_files};
use legaia_web_expansion::session::Core;
use serde_json::Value;

fn rebirth_files() -> PackFiles {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs/rebirth");
    read_dir_files(&root).expect("packs/rebirth")
}

fn load(core: &mut Core) -> (PackFiles, Value) {
    let files = rebirth_files();
    let loaded = core.load_files(files.clone());
    assert!(
        !loaded["pack"].is_null(),
        "Rebirth does not load: {}",
        loaded["issues"]
    );
    (files, loaded["pack"].clone())
}

#[test]
fn unedited_rebirth_exports_its_own_bytes() {
    let mut core = Core::new();
    let (files, pack) = load(&mut core);
    let errors: Vec<_> = core
        .validate(&pack.to_string())
        .into_iter()
        .filter(|i| i.severity == "error")
        .collect();
    assert!(errors.is_empty(), "{errors:#?}");

    let bytes = core.export_bundle(&pack.to_string()).unwrap();
    let back = bundle::read_bundle(&bytes).unwrap();
    // Every layout file comes back byte for byte (comments and all).
    let layout: PackFiles = files
        .into_iter()
        .filter(|(p, _)| legaia_expansion_format::is_known_path(p))
        .collect();
    assert_eq!(back, layout);
    // Deterministic.
    assert_eq!(core.export_bundle(&pack.to_string()).unwrap(), bytes);
}

#[test]
fn an_edit_round_trips_and_touches_only_its_file() {
    let mut core = Core::new();
    let (files, mut pack) = load(&mut core);

    // The battle editor: a new derived monster and a scaled stat.
    pack["battle"]["monsters"]["ironhide_ogre"]["hp"] = serde_json::json!({ "mul": 4.0 });
    pack["battle"]["monsters"]["mist_hound"] = serde_json::json!({
        "base": { "kind": "monster", "id": 12 },
        "hp": 9000,
    });
    // The painter: one more spawn and a repainted tile.
    let map = &mut pack["maps"]["rebirth_tower_1f"];
    map["spawns"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({ "id": "corner", "tile": [1, 1], "facing": 2 }));
    let row = map["tiles"]["rows"][1].as_str().unwrap().to_string();
    let mut chars: Vec<char> = row.chars().collect();
    chars[2] = ',';
    map["tiles"]["rows"][1] = Value::String(chars.into_iter().collect());

    let json = pack.to_string();
    let edited = core.pack_from_json(&json).unwrap();
    let bytes = core.export_bundle(&json).unwrap();
    let back = bundle::read_bundle(&bytes).unwrap();
    assert_eq!(ExpansionPack::from_files(&back).unwrap(), edited);

    for (path, b) in &back {
        let changed = files.get(path) != Some(b);
        let edited_file = path == "battle.toml" || path == "maps/rebirth_tower_1f.toml";
        assert_eq!(changed, edited_file, "{path}");
    }

    // The re-read bundle loads into a fresh session as the same pack JSON.
    let mut again = Core::new();
    let reloaded = again.load_bundle(&bytes).unwrap();
    assert_eq!(reloaded["pack"], serde_json::to_value(&edited).unwrap());
}

#[test]
fn the_tower_map_previews_with_the_corner_rule() {
    let mut core = Core::new();
    let (_, pack) = load(&mut core);
    let map = &pack["maps"]["rebirth_tower_1f"];
    let p = core.map_preview(&map.to_string());
    assert!(p["error"].is_null(), "{p}");
    let (w, h) = (p["width"].as_u64().unwrap(), p["height"].as_u64().unwrap());
    assert_eq!((w, h), (15, 13));
    let at = |k: &str, c: u64, r: u64| p[k][(r * w + c) as usize].clone();
    // The wall ring is all four quarters; the dais is tier 1 at 48 up.
    assert_eq!(at("walls", 0, 0), 0xF);
    assert_eq!(at("tier", 6, 5), 1);
    assert_eq!(p["ladder"][1], 48);
    // The dais spans columns 5..=9, rows 4..=8: flat inside, and its last
    // column and row slope down to the arena floor.
    assert_eq!(at("flat", 6, 5), true);
    assert_eq!(at("flat", 9, 5), false);
    assert_eq!(at("flat", 6, 8), false);
    assert_eq!(at("corners", 9, 5), serde_json::json!([48, 0, 48, 0]));
}

#[test]
fn xp_chart_and_dialog_check() {
    let mut core = Core::new();
    let (_, pack) = load(&mut core);
    let chart = core.xp_chart(&pack["progression"].to_string()).unwrap();
    let retail = chart["retail"].as_array().unwrap();
    let packed = chart["pack"].as_array().unwrap();
    assert_eq!(retail.len(), 98);
    assert_eq!(packed.len(), 98);
    // Banded from level 60: the same through 59, cheaper after.
    assert_eq!(retail[57], packed[57]);
    assert!(packed[97].as_u64() < retail[97].as_u64());

    let line = &pack["dialog"]["lines"]["guide_intro"];
    let d = core.check_dialog(&line.to_string()).unwrap();
    assert!(d["error"].is_null(), "{d}");
    // One page per blank-line-separated block of the line's text.
    let pages = line["text"].as_str().unwrap().split("\n\n").count();
    assert_eq!(d["pages"].as_array().unwrap().len(), pages);
    let long = serde_json::json!({ "text": "x".repeat(40) });
    let d = core.check_dialog(&long.to_string()).unwrap();
    assert_eq!(d["pages"][0][0]["long"], true);
}

#[test]
fn every_rebirth_cutscene_previews_to_its_end() {
    let mut core = Core::new();
    let (_, pack) = load(&mut core);
    let ids: Vec<String> = pack["cutscenes"]["cutscenes"]
        .as_object()
        .expect("Rebirth has cutscenes")
        .keys()
        .cloned()
        .collect();
    assert!(ids.contains(&"tower_intro".to_string()), "{ids:?}");
    for id in &ids {
        let t = core.cutscene_timeline(&pack.to_string(), id, &[]).unwrap();
        assert_eq!(t["ended"], true, "{id} never ends: {t}");
        let rows = t["rows"].as_array().unwrap();
        assert!(!rows.is_empty(), "{id}");
        // Frames never run backwards on a track.
        let mut last: std::collections::BTreeMap<String, u64> = Default::default();
        for r in rows {
            let track = r["track"].as_str().unwrap().to_string();
            let f = r["frame"].as_u64().unwrap();
            assert!(last.get(&track).is_none_or(|&p| p <= f), "{id}: {r}");
            last.insert(track, f);
        }
    }
    // The intro's camera crane runs beside its main track.
    let t = core
        .cutscene_timeline(&pack.to_string(), "tower_intro", &[])
        .unwrap();
    assert!(
        t["rows"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["track"] == "crane" && r["kind"] == "camera")
    );
    assert!(
        core.cutscene_timeline(&pack.to_string(), "nope", &[])
            .is_err()
    );
}
