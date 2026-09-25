//! Disc-gated: the translation workbench session (`translate_workbench::Core`)
//! over the real disc - the rows the page lists, the live line check, the
//! scene / name fast paths, the preview, and a shipped pack loaded, saved,
//! restored and reported.
//!
//! Skips + passes when `LEGAIA_DISC_BIN` is unset. Asserts on keys, lengths
//! and counts only - no game text.

#![cfg(not(target_arch = "wasm32"))]

use std::collections::BTreeMap;

use legaia_web_viewer::translate_workbench::Core;
use serde_json::Value;

fn open() -> Option<Core> {
    let Some(disc) = std::env::var("LEGAIA_DISC_BIN")
        .ok()
        .filter(|s| !s.is_empty())
    else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return None;
    };
    let bytes = std::fs::read(&disc).ok()?;
    Some(Core::open(bytes).expect("open the disc"))
}

#[test]
fn workbench_session_over_the_real_disc() {
    let Some(mut core) = open() else { return };
    let v: Value = serde_json::from_str(&core.entries_json()).unwrap();
    assert_eq!(v["font"], true, "the disc font decodes");
    let rows = v["entries"].as_array().unwrap();
    assert!(rows.len() > 30_000, "{} rows", rows.len());

    // Dialog rows carry box numbers; a box never holds more than three rows,
    // and three-row boxes exist.
    let mut per_box: BTreeMap<u64, u64> = BTreeMap::new();
    let (mut dialog, mut fits) = (0usize, 0usize);
    for r in rows {
        let s = r["s"].as_str().unwrap();
        if s == "scene_dialog" || s == "inline_text" {
            let b = r["box"].as_u64().expect("dialog row has a box");
            let row = r["row"].as_u64().unwrap();
            assert!(row < 3);
            *per_box.entry(b).or_default() += 1;
            assert!(r["lim"].as_str().unwrap().starts_with("field_dialog_row"));
            dialog += 1;
            let limit = if row == 0 { 244 } else { 228 };
            if r["spx"].as_u64().is_some_and(|px| px <= limit) {
                fits += 1;
            }
        }
        if s == "monster_names" {
            assert!(r["cap"].as_u64().unwrap() >= r["room"].as_u64().unwrap());
        }
        assert_ne!(r["rk"], "unknown", "{}", r["k"]);
    }
    assert!(per_box.values().all(|&n| n <= 3));
    assert!(per_box.values().any(|&n| n == 3));
    eprintln!("English dialog rows inside their width: {fits}/{dialog}");
    assert!(fits * 10 > dialog * 9, "{fits}/{dialog}");

    // The English source checks at exactly its room, and a line naming an
    // item through a substitution token measures with the name resolved.
    let man = rows
        .iter()
        .find(|r| r["s"] == "scene_dialog" && r["src"].as_str().unwrap().contains("{c2:"))
        .expect("a dialog line with an item token");
    let key = man["k"].as_str().unwrap();
    let c = core.check(key, man["src"].as_str().unwrap());
    assert_eq!(c.len, man["room"].as_u64().map(|n| n as usize));
    assert_eq!(c.unresolved, 0);
    assert!(c.px.unwrap() > 0);

    // Scene fast path: a short line in one scene lands.
    let prot: usize = key.split(':').nth(1).unwrap().parse().unwrap();
    assert!(core.set_translation(key, "Ok."));
    let fit: Value = serde_json::from_str(&core.scene_fit(prot, false)).unwrap();
    let row = fit["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["key"] == key)
        .unwrap();
    assert!(
        ["in_place", "relocated"].contains(&row["outcome"].as_str().unwrap()),
        "{row}"
    );

    // Name fast path: a name longer than its room is planned by the
    // importer's SCUS pass (moved or refused, never written in place).
    let item = rows
        .iter()
        .find(|r| r["rk"] == "name_movable")
        .expect("a movable name");
    let ikey = item["k"].as_str().unwrap();
    let long = "W".repeat(item["room"].as_u64().unwrap() as usize + 3);
    core.set_translation(ikey, &long);
    let names: Value = serde_json::from_str(&core.names_fit()).unwrap();
    let nrow = names["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["key"] == ikey)
        .unwrap();
    assert!(
        ["moved", "no_free_run"].contains(&nrow["outcome"].as_str().unwrap()),
        "{nrow}"
    );

    // The preview is as wide as the measure says, and at least the limit.
    let r = core.render(key, &["Ok.", "Second row"]).expect("font");
    assert_eq!(r.rgba.len(), (r.w * r.h * 4) as usize);
    assert_eq!(r.limit_px, 244);
    assert!(r.w >= r.limit_px);

    // A shipped pack loads onto the disc's keys, survives the autosave
    // round trip and strips to its filled keys.
    let fr = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../site/lang/fr.yaml");
    let yaml = std::fs::read_to_string(fr).unwrap();
    let (filled, merged, unknown) = core.load_pack(&yaml).unwrap();
    assert!(merged > 0 && unknown == 0, "{filled} {merged} {unknown}");
    let saved = core.translations_json();
    let restored = core.load_translations_json(&saved).unwrap();
    assert_eq!(restored, merged);
    let (_, kept) = core.shareable_yaml().unwrap();
    assert_eq!(kept, merged);

    let report: Value = serde_json::from_str(&core.space_report(false).unwrap()).unwrap();
    assert_eq!(report["schema"], "legaia-space-v1");
    assert!(report["reasons"].is_array());
    assert_eq!(
        report["summary"]["filled"].as_u64().unwrap() as usize,
        merged
    );
    let disc: Value = serde_json::from_str(&core.disc_report_json()).unwrap();
    assert!(disc["entries"].is_null() && disc["scenes"].as_array().unwrap().len() > 10);
}
