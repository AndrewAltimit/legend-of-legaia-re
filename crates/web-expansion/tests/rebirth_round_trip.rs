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
    // The painter draws the floor's decorations and decals where they bake.
    let decor = p["decor"].as_array().unwrap();
    assert!(decor.len() > 50, "{} decorations", decor.len());
    assert!(decor.iter().any(|d| d[2] == 1), "decals among them");
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

/// The Arts tab's damage preview runs Rebirth's arts through the retail
/// melee kernel: one row per power byte, more ATK more damage, and the
/// finisher's `damage_pct` on top.
#[test]
fn the_arts_preview_rolls_rebirths_arts() {
    let mut core = Core::new();
    let (_, pack) = load(&mut core);
    let ember = pack["arts"]["arts"]["ember_cascade"].to_string();
    let low = core.art_preview(&ember, 200, 150).unwrap();
    let high = core.art_preview(&ember, 400, 150).unwrap();
    assert_eq!(low["hits"].as_array().unwrap().len(), 3, "U22 L22 then L28");
    assert_eq!(low["hits"][2]["power"], "L28");
    assert!(high["mean"].as_u64() > low["mean"].as_u64());
    let mut boosted: Value = serde_json::from_str(&ember).unwrap();
    boosted["damage_pct"] = 200.into();
    let twice = core.art_preview(&boosted.to_string(), 200, 150).unwrap();
    let (a, b) = (
        low["mean"].as_u64().unwrap(),
        twice["mean"].as_u64().unwrap(),
    );
    assert!(b > a * 3 / 2, "damage_pct doubles the roll ({a} -> {b})");
    assert!(
        legaia_web_expansion::catalog::art_clips()
            .iter()
            .any(|c| c.character == "noa" && c.constant == 0x31 && c.name == "Love You")
    );
}

/// The workbench's `.lxpack` of an unedited Rebirth is the CLI's
/// (`asset expansion bundle packs/rebirth`: the tree's layout files - all
/// of Rebirth's - through `write_bundle`), byte for byte.
#[test]
fn unedited_rebirth_bundle_is_the_cli_bundle() {
    let mut core = Core::new();
    let (files, pack) = load(&mut core);
    let cli = bundle::write_bundle(&files).unwrap();
    assert_eq!(core.export_bundle(&pack.to_string()).unwrap(), cli);
}

/// An edit and its revert export the loaded bytes again, in every section
/// file and a map: the unchanged-file test is the canonical form, not the
/// JSON spelling, so the comments come back with the value.
#[test]
fn an_edit_then_its_revert_exports_the_loaded_bytes() {
    let mut core = Core::new();
    let (files, pack) = load(&mut core);
    let cli = bundle::write_bundle(&files).unwrap();
    // One edit per section file, each to a field Rebirth sets.
    let edits: &[(&str, &[&str])] = &[
        ("pack.toml", &["manifest", "name"]),
        (
            "battle.toml",
            &["battle", "monsters", "ironhide_ogre", "name"],
        ),
        ("dialog.toml", &["dialog", "lines", "guide_intro", "text"]),
        (
            "cutscenes.toml",
            &["cutscenes", "cutscenes", "tower_intro", "title"],
        ),
        (
            "maps/rebirth_tower_1f.toml",
            &["maps", "rebirth_tower_1f", "name"],
        ),
    ];
    for (file, path) in edits {
        let mut edited = pack.clone();
        let mut at = &mut edited;
        for k in *path {
            at = at.get_mut(*k).unwrap_or_else(|| panic!("{file}: no {k}"));
        }
        let before = at.clone();
        *at = Value::String("edited".into());
        let bytes = core.export_bundle(&edited.to_string()).unwrap();
        let back = bundle::read_bundle(&bytes).unwrap();
        assert_ne!(
            back.get(*file),
            files.get(*file),
            "{file}: the edit is exported"
        );
        let mut at = &mut edited;
        for k in *path {
            at = at.get_mut(*k).unwrap();
        }
        *at = before;
        assert_eq!(
            core.export_bundle(&edited.to_string()).unwrap(),
            cli,
            "{file}: reverted"
        );
    }
    // Every section the page edits is exported when edited (the audio and
    // the cutscenes files once kept their loaded bytes through an edit).
    for (file, section) in [
        ("audio.toml", "audio"),
        ("cutscenes.toml", "cutscenes"),
        ("arts.toml", "arts"),
        ("spells.toml", "spells"),
        ("items.toml", "items"),
        ("modes.toml", "modes"),
        ("cards.toml", "cards"),
        ("quests.toml", "quests"),
        ("progression.toml", "progression"),
    ] {
        if !files.contains_key(file) {
            continue;
        }
        let mut edited = pack.clone();
        edited[section] = serde_json::to_value(empty_section(section)).unwrap();
        // An emptied section may not validate (references into it); the
        // files view has no error gate.
        let view = core.files_view(&edited.to_string()).unwrap();
        let kept = view
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["path"] == file)
            .map(|f| f["text"].as_str().unwrap().as_bytes().to_vec());
        assert_ne!(
            kept.as_ref(),
            files.get(file),
            "{file}: an emptied section still exports its loaded bytes"
        );
    }
}

fn empty_section(section: &str) -> Value {
    let pack = ExpansionPack::default();
    serde_json::to_value(&pack).unwrap()[section].clone()
}

/// A shop row the Items tab rewrote as `{ item }` (it edits every row in
/// that form) is the bare reference it was loaded as, so opening the tab
/// leaves `items.toml` as loaded.
#[test]
fn a_shop_row_in_either_spelling_is_unchanged() {
    let mut core = Core::new();
    let (files, mut pack) = load(&mut core);
    let cli = bundle::write_bundle(&files).unwrap();
    let mut rewrote = 0;
    for shop in pack["items"]["shops"].as_array_mut().unwrap() {
        for row in shop["stock"].as_array_mut().unwrap() {
            if !row.is_object() {
                *row = serde_json::json!({ "item": row.clone(), "requires": {} });
                rewrote += 1;
            }
        }
    }
    assert!(rewrote > 0, "Rebirth has bare shop rows");
    assert_eq!(core.export_bundle(&pack.to_string()).unwrap(), cli);
}

/// The import path: `.lxpack` -> editable sources -> `.lxpack` is byte for
/// byte the bundle that came in, and so is the sources zip's tree.
#[test]
fn an_imported_bundle_exports_its_own_bytes() {
    let layout: PackFiles = rebirth_files()
        .into_iter()
        .filter(|(p, _)| legaia_expansion_format::is_known_path(p))
        .collect();
    let bytes = bundle::write_bundle(&layout).unwrap();
    let mut core = Core::new();
    let r = core.import_bundle(&bytes);
    assert_eq!(r["ok"], true, "{}", r["error"]);
    assert_eq!(r["summary"]["cover"], "assets/cover.png");
    let pack = r["pack"].to_string();
    assert_eq!(core.export_bundle(&pack).unwrap(), bytes);
    // The sources zip holds the same files under `rebirth/`.
    let zip = core.sources_zip(&pack).unwrap();
    assert!(zip.starts_with(b"PK\x03\x04"));
    for path in layout.keys() {
        let name = format!("rebirth/{path}");
        assert!(
            zip.windows(name.len()).any(|w| w == name.as_bytes()),
            "{name}"
        );
    }
}

/// A bundle carrying an empty section file (its value the default) comes
/// back with that file, so the round trip holds for any tree, not only one
/// the workbench wrote.
#[test]
fn an_empty_section_file_survives_the_round_trip() {
    let mut files = PackFiles::new();
    files.insert(
        "pack.toml".into(),
        b"# hand written\nid = \"t\"\nname = \"T\"\nversion = \"0.1.0\"\n".to_vec(),
    );
    files.insert("battle.toml".into(), b"# nothing yet\n".to_vec());
    files.insert("CHANGELOG.md".into(), b"## 0.1.0\n".to_vec());
    let bytes = bundle::write_bundle(&files).unwrap();
    let mut core = Core::new();
    let r = core.import_bundle(&bytes);
    assert_eq!(r["ok"], true);
    assert_eq!(core.export_bundle(&r["pack"].to_string()).unwrap(), bytes);
    assert_eq!(core.doc_text("CHANGELOG.md"), "## 0.1.0\n");
}

/// Refused imports name their gate; the session keeps the pack it had.
#[test]
fn refused_imports_name_the_gate() {
    let mut core = Core::new();
    let (_, pack) = load(&mut core);
    let r = core.import_bundle(b"PK\x03\x04 a zip, not a pack");
    assert_eq!(r["ok"], false);
    assert_eq!(r["error"]["kind"], "not_a_bundle");
    let mut newer = PackFiles::new();
    newer.insert(
        "pack.toml".into(),
        b"id = \"t\"\nname = \"T\"\nversion = \"3.0.0\"\nschema = 99\nshiny = 1\n".to_vec(),
    );
    let r = core.import_bundle(&bundle::write_bundle(&newer).unwrap());
    assert_eq!(r["error"]["kind"], "newer_format");
    assert!(
        r["error"]["message"]
            .as_str()
            .unwrap()
            .contains("update the engine")
    );
    // Still the Rebirth session: it exports as before.
    assert!(core.export_bundle(&pack.to_string()).is_ok());
}

/// The diff panel: Rebirth against an edited copy of itself.
#[test]
fn the_diff_panel_reads_an_older_bundle() {
    let mut core = Core::new();
    let (files, mut pack) = load(&mut core);
    let old: PackFiles = files
        .into_iter()
        .filter(|(p, _)| legaia_expansion_format::is_known_path(p))
        .collect();
    let old = bundle::write_bundle(&old).unwrap();
    let same = core.diff_against(&old, &pack.to_string()).unwrap();
    assert_eq!(same["counts"], serde_json::json!([0, 0, 0]));
    assert_eq!(same["suggested_version"], Value::Null);

    pack["battle"]["monsters"]["mist_hound"] = serde_json::json!({
        "base": { "kind": "monster", "id": 12 },
        "hp": 9000,
    });
    pack["battle"]["monsters"]["ironhide_ogre"]["hp"] = serde_json::json!({ "mul": 4.0 });
    let d = core.diff_against(&old, &pack.to_string()).unwrap();
    assert_eq!(d["same_pack"], true);
    assert_eq!(d["counts"], serde_json::json!([1, 0, 1]));
    assert_eq!(d["bump"], "minor");
    assert_eq!(d["suggested_version"], "0.2.0");
    let log = d["changelog"].as_str().unwrap();
    assert!(
        log.contains("- monster `mist_hound` (battle.toml)"),
        "{log}"
    );
    assert!(
        log.contains("- monster `ironhide_ogre`: hp (battle.toml)"),
        "{log}"
    );

    let s = core.share_status(&pack.to_string()).unwrap();
    assert_eq!(s["compatible"], true);
    assert!(s["cover_bytes"].as_u64().unwrap() > 0);
}

/// What the map detail forms and the modes point lists write when they only
/// draw: an absent list as `[]`, an absent prop `animate` as `[]`, a decal's
/// absent `cells` as `[]`, the lighting and fog boxes' defaults, every list
/// item's fields as the form reads them back. Opening every map and the
/// modes tab without an edit exports the loaded bytes.
fn as_the_forms_draw_it(pack: &mut Value) {
    let ensure = |o: &mut Value, k: &str, v: Value| {
        if o.get(k).is_none_or(Value::is_null) {
            o[k] = v;
        }
    };
    for (_, map) in pack["maps"].as_object_mut().unwrap() {
        for k in [
            "decor",
            "scatter",
            "edges",
            "decals",
            "materials",
            "emitters",
        ] {
            ensure(map, k, serde_json::json!([]));
        }
        ensure(
            map,
            "lighting",
            serde_json::json!({ "mood": null, "lamps": [] }),
        );
        ensure(map, "fog", serde_json::json!({ "regions": [] }));
        for prop in map["props"].as_array_mut().unwrap() {
            ensure(prop, "animate", serde_json::json!([]));
        }
        for d in map["decals"].as_array_mut().unwrap() {
            ensure(d, "cells", serde_json::json!([]));
        }
        for s in map["scatter"].as_array_mut().unwrap() {
            ensure(s, "meshes", serde_json::json!([]));
        }
    }
    for k in ["races", "prop_hunt"] {
        for it in pack["modes"][k].as_array_mut().into_iter().flatten() {
            for list in ["points", "waypoints", "blockers"] {
                if k == "races" && list == "blockers" || k == "prop_hunt" && list != "blockers" {
                    continue;
                }
                ensure(it, list, serde_json::json!([]));
            }
        }
    }
}

#[test]
fn the_detail_forms_open_without_an_edit() {
    let mut core = Core::new();
    let (files, mut pack) = load(&mut core);
    let cli = bundle::write_bundle(&files).unwrap();
    // Rebirth uses every list the forms replace, so the test is not vacuous.
    let maps = pack["maps"].as_object().unwrap();
    for k in ["decor", "scatter", "edges", "decals", "materials"] {
        assert!(
            maps.values()
                .any(|m| m[k].as_array().is_some_and(|a| !a.is_empty())),
            "Rebirth has no {k}"
        );
    }
    assert!(maps.values().any(|m| {
        m["props"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["animate"].as_array().is_some_and(|a| !a.is_empty()))
    }));
    assert!(pack["modes"]["races"][0]["points"].as_array().is_some());
    as_the_forms_draw_it(&mut pack);
    assert_eq!(core.export_bundle(&pack.to_string()).unwrap(), cli);

    // A Duplicate and its Remove, a reorder and its reverse: the loaded bytes.
    let mut edited = pack.clone();
    let map = &mut edited["maps"]["rebirth_tower_1f"];
    let decals = map["decals"].as_array_mut().unwrap();
    let copy = decals[0].clone();
    decals.insert(1, copy);
    let blockers = &mut edited["modes"]["prop_hunt"][0]["blockers"];
    let b = blockers.as_array_mut().unwrap();
    b.swap(0, 1);
    let bytes = core.export_bundle(&edited.to_string()).unwrap();
    let back = bundle::read_bundle(&bytes).unwrap();
    for (path, body) in &back {
        let touched = path == "maps/rebirth_tower_1f.toml" || path == "modes.toml";
        assert_eq!(files.get(path) != Some(body), touched, "{path}");
    }
    edited["maps"]["rebirth_tower_1f"]["decals"]
        .as_array_mut()
        .unwrap()
        .remove(1);
    edited["modes"]["prop_hunt"][0]["blockers"]
        .as_array_mut()
        .unwrap()
        .swap(0, 1);
    assert_eq!(core.export_bundle(&edited.to_string()).unwrap(), cli);

    // The material kind switch rebuilds the item: a scroll turned into a
    // cycle and back keeps the cell only if the form carries it, so the
    // revert is the loaded scroll exactly.
    let mut edited = pack.clone();
    let m = &mut edited["maps"]["rebirth_tower_1f"]["materials"][0];
    let before = m.clone();
    assert_eq!(before["kind"], "scroll");
    *m = serde_json::json!({ "kind": "cycle", "clut": before["cell"]["clut"], "start": 0, "count": 16, "period": 8, "reverse": false });
    assert!(
        core.validate(&edited.to_string())
            .iter()
            .all(|i| i.severity != "error")
    );
    edited["maps"]["rebirth_tower_1f"]["materials"][0] = before;
    assert_eq!(core.export_bundle(&edited.to_string()).unwrap(), cli);

    // The podium box and the boss cutscene pickers: unticked / cleared and
    // put back, the loaded bytes.
    let mut edited = pack.clone();
    let night = &mut edited["modes"]["party_nights"][0];
    let podium = night["podium"].clone();
    assert!(podium.is_object(), "Rebirth's night has a podium");
    night["podium"] = Value::Null;
    let key = pack["battle"]["bosses"]
        .as_object()
        .unwrap()
        .iter()
        .find(|(_, b)| b["intro"].is_string())
        .map(|(k, _)| k.clone())
        .expect("a Rebirth boss with an intro");
    let intro = edited["battle"]["bosses"][&key]["intro"].clone();
    edited["battle"]["bosses"][&key]["intro"] = Value::Null;
    let bytes = core.export_bundle(&edited.to_string()).unwrap();
    assert_ne!(bytes, cli);
    edited["modes"]["party_nights"][0]["podium"] = podium;
    edited["battle"]["bosses"][&key]["intro"] = intro;
    assert_eq!(core.export_bundle(&edited.to_string()).unwrap(), cli);
}
