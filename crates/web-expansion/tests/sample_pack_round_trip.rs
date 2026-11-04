//! The sample pack through the workbench session, disc-free: load the
//! `packs/sample/` tree, take the pack as the page's JSON, edit it the way
//! the editors do, export the `.lxpack` and read it back.

use std::path::Path;

use legaia_expansion_format::{ExpansionPack, PackFiles, bundle, read_dir_files};
use legaia_web_expansion::session::Core;
use serde_json::Value;

fn sample_pack_files() -> PackFiles {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs/sample");
    read_dir_files(&root).expect("packs/sample")
}

fn load(core: &mut Core) -> (PackFiles, Value) {
    let files = sample_pack_files();
    let loaded = core.load_files(files.clone());
    assert!(
        !loaded["pack"].is_null(),
        "the sample pack does not load: {}",
        loaded["issues"]
    );
    (files, loaded["pack"].clone())
}

#[test]
fn unedited_sample_pack_exports_its_own_bytes() {
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
    let map = &mut pack["maps"]["genesis_1f"];
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
        let edited_file = path == "battle.toml" || path == "maps/genesis_1f.toml";
        assert_eq!(changed, edited_file, "{path}");
    }

    // The re-read bundle loads into a fresh session as the same pack JSON.
    let mut again = Core::new();
    let reloaded = again.load_bundle(&bytes).unwrap();
    assert_eq!(reloaded["pack"], serde_json::to_value(&edited).unwrap());
}

/// The painter's preview answers one map the same however it got there: each
/// map of a run of tile edits, previewed cold, after its neighbours and again
/// past the terrain memos' reach, reads byte for byte the same, and the edits
/// do show. (Without the disc the rest checks measure nothing; the field and
/// the walls they stand on are the same.)
#[test]
fn a_map_preview_is_the_same_after_any_run_of_edits() {
    let mut core = Core::new();
    let (_, pack) = load(&mut core);
    for id in ["genesis_1f", "isles_beach", "isles_grotto"] {
        let map = &pack["maps"][id];
        let rows: Vec<String> = map["tiles"]["rows"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r.as_str().unwrap().to_string())
            .collect();
        let brush = |c: char| map["tiles"]["legend"].get(c.to_string()).is_some();
        let chars: Vec<char> = {
            let mut v: Vec<char> = rows
                .iter()
                .flat_map(|r| r.chars())
                .filter(|c| brush(*c))
                .collect();
            v.sort_unstable();
            v.dedup();
            v
        };
        assert!(chars.len() > 1, "{id}");
        // Twelve edits, past the memos' eight entries.
        let edits: Vec<String> = (0..12)
            .map(|i| {
                let (r, c) = (
                    1 + i * 3 % (rows.len() - 2),
                    1 + i * 5 % (rows[0].chars().count() - 2),
                );
                let mut line: Vec<char> = rows[r].chars().collect();
                let was = line[c];
                line[c] = *chars.iter().find(|x| **x != was).unwrap();
                let mut m = map.clone();
                m["tiles"]["rows"][r] = serde_json::json!(line.into_iter().collect::<String>());
                m.to_string()
            })
            .collect();
        let first: Vec<String> = edits
            .iter()
            .map(|e| core.map_preview(e).to_string())
            .collect();
        for (e, p) in edits.iter().zip(&first).rev() {
            assert_eq!(
                core.map_preview(e).to_string(),
                *p,
                "{id}: back over the run"
            );
        }
        for (e, p) in edits.iter().zip(&first) {
            assert_eq!(core.map_preview(e).to_string(), *p, "{id}: forward again");
        }
        assert!(
            first[0].contains("\"width\""),
            "{id}: {}",
            &first[0][..80.min(first[0].len())]
        );
        assert!(first.iter().any(|p| *p != first[0]), "{id}: the edits show");
    }
}

#[test]
fn the_tower_map_previews_with_the_corner_rule() {
    let mut core = Core::new();
    let (_, pack) = load(&mut core);
    let map = &pack["maps"]["genesis_1f"];
    let p = core.map_preview(&map.to_string());
    assert!(p["error"].is_null(), "{p}");
    let (w, h) = (p["width"].as_u64().unwrap(), p["height"].as_u64().unwrap());
    // The Atrium (columns 0..=14) and the Hall of Trials east of it.
    assert_eq!((w, h), (29, 13));
    let at = |k: &str, c: u64, r: u64| p[k][(r * w + c) as usize].clone();
    // The wall ring is all four quarters; the dais stands 48 up, on a
    // rung of its own (its living-ground ripple swings that rung).
    assert_eq!(at("walls", 0, 0), 0xF);
    let dais = at("tier", 6, 5).as_u64().unwrap();
    assert_ne!(dais, 0);
    assert_eq!(p["ladder"][dais as usize], 48);
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
    // Banded from level 40: the same through 39, cheaper after.
    assert_eq!(retail[37], packed[37]);
    assert!(packed[38].as_u64() < retail[38].as_u64());
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
fn every_sample_pack_cutscene_previews_to_its_end() {
    let mut core = Core::new();
    let (_, pack) = load(&mut core);
    let ids: Vec<String> = pack["cutscenes"]["cutscenes"]
        .as_object()
        .expect("the sample pack has cutscenes")
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

/// The Arts tab's damage preview runs the sample pack's arts through the retail
/// melee kernel: one row per power byte, more ATK more damage, and the
/// finisher's `damage_pct` on top.
#[test]
fn the_arts_preview_rolls_sample_packs_arts() {
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

/// The workbench's `.lxpack` of an unedited sample pack is the CLI's
/// (`asset expansion bundle packs/sample`: the tree's layout files - all
/// of the sample pack's - through `write_bundle`), byte for byte.
#[test]
fn unedited_sample_pack_bundle_is_the_cli_bundle() {
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
    // One edit per section file, each to a field the sample pack sets.
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
        ("maps/genesis_1f.toml", &["maps", "genesis_1f", "name"]),
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
    assert!(rewrote > 0, "the sample pack has bare shop rows");
    assert_eq!(core.export_bundle(&pack.to_string()).unwrap(), cli);
}

/// The import path: `.lxpack` -> editable sources -> `.lxpack` is byte for
/// byte the bundle that came in, and so is the sources zip's tree.
#[test]
fn an_imported_bundle_exports_its_own_bytes() {
    let layout: PackFiles = sample_pack_files()
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
    // The sources zip holds the same files under `sample/`.
    let zip = core.sources_zip(&pack).unwrap();
    assert!(zip.starts_with(b"PK\x03\x04"));
    for path in layout.keys() {
        let name = format!("sample/{path}");
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
    // Still the sample pack session: it exports as before.
    assert!(core.export_bundle(&pack.to_string()).is_ok());
}

/// The diff panel: the sample pack against an edited copy of itself.
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
    assert_eq!(d["suggested_version"], "0.3.0");
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
    // The sample pack uses every list the forms replace, so the test is not vacuous.
    let maps = pack["maps"].as_object().unwrap();
    for k in ["decor", "scatter", "edges", "decals", "materials"] {
        assert!(
            maps.values()
                .any(|m| m[k].as_array().is_some_and(|a| !a.is_empty())),
            "the sample pack has no {k}"
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
    let map = &mut edited["maps"]["genesis_1f"];
    let decals = map["decals"].as_array_mut().unwrap();
    let copy = decals[0].clone();
    decals.insert(1, copy);
    let blockers = &mut edited["modes"]["prop_hunt"][0]["blockers"];
    let b = blockers.as_array_mut().unwrap();
    b.swap(0, 1);
    let bytes = core.export_bundle(&edited.to_string()).unwrap();
    let back = bundle::read_bundle(&bytes).unwrap();
    for (path, body) in &back {
        let touched = path == "maps/genesis_1f.toml" || path == "modes.toml";
        assert_eq!(files.get(path) != Some(body), touched, "{path}");
    }
    edited["maps"]["genesis_1f"]["decals"]
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
    let (scrolled, at) = pack["maps"]
        .as_object()
        .unwrap()
        .iter()
        .find_map(|(id, m)| {
            let i = m["materials"]
                .as_array()?
                .iter()
                .position(|x| x["kind"] == "scroll")?;
            Some((id.clone(), i))
        })
        .expect("a sample-pack map with a scroll material");
    let mut edited = pack.clone();
    let m = &mut edited["maps"][&scrolled]["materials"][at];
    let before = m.clone();
    *m = serde_json::json!({ "kind": "cycle", "clut": before["cell"]["clut"], "start": 0, "count": 16, "period": 8, "reverse": false });
    assert!(
        core.validate(&edited.to_string())
            .iter()
            .all(|i| i.severity != "error")
    );
    edited["maps"][&scrolled]["materials"][at] = before;
    assert_eq!(core.export_bundle(&edited.to_string()).unwrap(), cli);

    // The podium box and the boss cutscene pickers: unticked / cleared and
    // put back, the loaded bytes.
    let mut edited = pack.clone();
    let night = &mut edited["modes"]["party_nights"][0];
    let podium = night["podium"].clone();
    assert!(podium.is_object(), "the sample pack's night has a podium");
    night["podium"] = Value::Null;
    let key = pack["battle"]["bosses"]
        .as_object()
        .unwrap()
        .iter()
        .find(|(_, b)| b["intro"].is_string())
        .map(|(k, _)| k.clone())
        .expect("a sample-pack boss with an intro");
    let intro = edited["battle"]["bosses"][&key]["intro"].clone();
    edited["battle"]["bosses"][&key]["intro"] = Value::Null;
    let bytes = core.export_bundle(&edited.to_string()).unwrap();
    assert_ne!(bytes, cli);
    edited["modes"]["party_nights"][0]["podium"] = podium;
    edited["battle"]["bosses"][&key]["intro"] = intro;
    assert_eq!(core.export_bundle(&edited.to_string()).unwrap(), cli);
}

/// The battle stage editor and the tint material form, as the page drives
/// them: opened, ticked and unticked, re-tinted and put back, the loaded
/// bytes; a real edit touches only its own file.
#[test]
fn the_stage_and_tint_forms_round_trip() {
    let mut core = Core::new();
    let (files, pack) = load(&mut core);
    let cli = bundle::write_bundle(&files).unwrap();
    let bosses = pack["battle"]["bosses"].as_object().unwrap();
    // The sample pack uses both fields, so the test is not vacuous.
    let (staged, stage) = bosses
        .iter()
        .find(|(_, b)| b["stage"]["tint"].is_array())
        .map(|(k, b)| (k.clone(), b["stage"].clone()))
        .expect("a sample-pack boss on a tinted stage");
    assert!(stage["scene"].is_string());
    let plain = bosses
        .iter()
        .find(|(_, b)| b["stage"].is_null())
        .map(|(k, _)| k.clone())
        .expect("a sample-pack boss on the room's stage");
    let map = "genesis_2f";
    let tint_at = pack["maps"][map]["materials"]
        .as_array()
        .unwrap()
        .iter()
        .position(|m| m["kind"] == "tint")
        .expect("a sample-pack tint material");

    // Opened without an edit: the forms write nothing.
    assert_eq!(core.export_bundle(&pack.to_string()).unwrap(), cli);

    // Unticked and ticked back; the tint box off and on with its values.
    let mut edited = pack.clone();
    edited["battle"]["bosses"][&staged]["stage"] = Value::Null;
    assert_ne!(core.export_bundle(&edited.to_string()).unwrap(), cli);
    edited["battle"]["bosses"][&staged]["stage"] = stage.clone();
    edited["battle"]["bosses"][&staged]["stage"]["tint"] = Value::Null;
    assert_ne!(core.export_bundle(&edited.to_string()).unwrap(), cli);
    edited["battle"]["bosses"][&staged]["stage"]["tint"] = stage["tint"].clone();
    assert_eq!(core.export_bundle(&edited.to_string()).unwrap(), cli);

    // A fresh stage as the checkbox writes it, then unticked: the loaded bytes.
    let mut edited = pack.clone();
    edited["battle"]["bosses"][&plain]["stage"] =
        serde_json::json!({ "scene": "jou", "index": null, "keep_object_1": false, "tint": null });
    let bytes = core.export_bundle(&edited.to_string()).unwrap();
    let back = bundle::read_bundle(&bytes).unwrap();
    for (path, body) in &back {
        assert_eq!(
            files.get(path) != Some(body),
            path == "battle.toml",
            "{path}"
        );
    }
    let text = String::from_utf8(back["battle.toml"].clone()).unwrap();
    assert!(text.contains("scene = \"jou\""), "the new stage is written");
    let reread = core.pack_from_json(&edited.to_string()).unwrap();
    let st = reread.battle.bosses[&plain].stage.clone().unwrap();
    assert_eq!(
        (st.scene.as_str(), st.index, st.keep_object_1, st.tint),
        ("jou", None, false, None)
    );
    edited["battle"]["bosses"][&plain]["stage"] = Value::Null;
    assert_eq!(core.export_bundle(&edited.to_string()).unwrap(), cli);

    // A formation's stage, index and keep box: battle.toml only.
    let mut edited = pack.clone();
    let fkey = pack["battle"]["formations"]
        .as_object()
        .unwrap()
        .keys()
        .next()
        .unwrap()
        .clone();
    edited["battle"]["formations"][&fkey]["stage"] = serde_json::json!({ "scene": "town01", "index": 1, "keep_object_1": true, "tint": [200, 128, 90] });
    let reread = core.pack_from_json(&edited.to_string()).unwrap();
    let st = reread.battle.formations[&fkey].stage.clone().unwrap();
    assert_eq!(
        (st.index, st.keep_object_1, st.tint),
        (Some(1), true, Some([200, 128, 90]))
    );

    // The tint material: re-coloured and put back; switched to a cycle and
    // back to the loaded item.
    let mut edited = pack.clone();
    let m = &mut edited["maps"][map]["materials"][tint_at];
    let before = m.clone();
    m["color"] = serde_json::json!([128, 128, 128]);
    m["hue"] = serde_json::json!(0.0);
    let bytes = core.export_bundle(&edited.to_string()).unwrap();
    let back = bundle::read_bundle(&bytes).unwrap();
    for (path, body) in &back {
        let own = path == &format!("maps/{map}.toml");
        assert_eq!(files.get(path) != Some(body), own, "{path}");
    }
    // A tint that changes nothing is a warning, not an error.
    let issues = core.validate(&edited.to_string());
    assert!(issues.iter().all(|i| i.severity != "error"), "{issues:#?}");
    assert!(
        issues
            .iter()
            .any(|i| i.field == format!("materials[{tint_at}]") && i.severity == "warning"),
        "{issues:#?}"
    );
    edited["maps"][map]["materials"][tint_at] = serde_json::json!({ "kind": "cycle", "clut": before["clut"], "start": 0, "count": 16, "period": 8, "reverse": false });
    assert!(
        core.validate(&edited.to_string())
            .iter()
            .all(|i| i.severity != "error")
    );
    edited["maps"][map]["materials"][tint_at] = before;
    assert_eq!(core.export_bundle(&edited.to_string()).unwrap(), cli);
}

#[test]
fn beams_and_surround_regions_round_trip_through_the_json() {
    let mut core = Core::new();
    let (_, mut pack) = load(&mut core);
    let map = &mut pack["maps"]["genesis_1f"];
    map["beams"] = serde_json::json!([
        { "tile": [3, 3], "tilt": 12.0, "angle": 200.0, "motes": 8 },
        { "tile": [5, 5], "offset": [0, -600, 0], "profile": "rise", "pool": false }
    ]);
    map["surround"] = serde_json::json!({
        "ground": { "tpage": 5, "clut": 30720, "tile": 0 },
        "depth": 448,
        "fade": 20,
        "regions": [{ "rect": [0, 0, 2, 2], "depth": 1600, "fade": 4 }]
    });
    let json = pack.to_string();
    let edited = core.pack_from_json(&json).unwrap();
    let m = &edited.maps["genesis_1f"];
    assert_eq!(m.beams.len(), 2);
    assert_eq!(m.beams[0].motes, 8);
    assert_eq!(m.beams[0].height, 900, "the defaults fill in");
    assert_eq!(
        m.beams[1].profile,
        legaia_expansion_format::map_beams::BeamProfile::Rise
    );
    let s = m.surround.as_ref().unwrap();
    assert_eq!(s.fade, Some(20));
    assert_eq!(s.regions[0].depth, 1600);
    let bytes = core.export_bundle(&json).unwrap();
    let back = bundle::read_bundle(&bytes).unwrap();
    assert_eq!(ExpansionPack::from_files(&back).unwrap(), edited);
}
