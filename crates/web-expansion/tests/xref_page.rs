//! Rename and the flow graph over the page's own pack JSON: a rename in the
//! workbench exports a `.lxpack` whose files differ from the loaded ones
//! exactly where the rename plan says, and the graph comes back whole.

use std::collections::BTreeSet;

use legaia_expansion_format::bundle;
use legaia_web_expansion::session::Core;
use legaia_web_expansion::xref;
use serde_json::Value;

fn load(core: &mut Core) -> Value {
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../packs/rebirth");
    let files = legaia_expansion_format::read_dir_files(&dir).unwrap();
    core.load_files(files)["pack"].clone()
}

#[test]
fn a_rename_exports_only_the_files_it_touches() {
    let mut core = Core::new();
    let pack = load(&mut core);
    let before = bundle::read_bundle(&core.export_bundle(&pack.to_string()).unwrap()).unwrap();
    for (kind, old, new) in [
        ("monster", "azure_seraph", "sky_seraph"),
        ("quest", "tower_trial", "tower_test"),
        ("flag", "tower_heard", "tower_heeded"),
        ("map", "isles_village", "isles_hamlet"),
        ("card", "mist-wisp", "fog-wisp"),
        ("line", "guide_intro", "guide_hello"),
        ("mode", "tower-high-table", "tower-top-table"),
        ("npc", "isles_ferrywoman", "isles_boatwoman"),
        ("cutscene", "tower_intro", "tower_opening"),
    ] {
        let plan = xref::rename_plan(&pack.to_string(), kind, old, new).unwrap();
        assert_eq!(plan["ok"], true, "{kind} {old}: {plan}");
        let out = xref::rename(&pack.to_string(), kind, old, new).unwrap();
        assert_eq!(out["ok"], true, "{kind} {old}: {out}");
        assert_eq!(out["changes"], plan["changes"]);
        let edited = out["pack"].to_string();
        let errors: Vec<_> = core
            .validate(&edited)
            .into_iter()
            .filter(|i| i.severity == "error")
            .collect();
        assert!(errors.is_empty(), "{kind} {old}: {errors:#?}");
        let after = bundle::read_bundle(&core.export_bundle(&edited).unwrap()).unwrap();
        let mut changed: BTreeSet<String> = before
            .iter()
            .filter(|(p, b)| after.get(*p) != Some(b))
            .map(|(p, _)| p.clone())
            .collect();
        changed.extend(after.keys().filter(|p| !before.contains_key(*p)).cloned());
        let mut planned: BTreeSet<String> = plan["changes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["file"].as_str().unwrap().to_string())
            .collect();
        if kind == "map" {
            planned.insert(format!("maps/{new}.toml"));
        }
        assert_eq!(changed, planned, "{kind} {old}");
    }
}

#[test]
fn find_and_the_graph_read_the_page_json() {
    let mut core = Core::new();
    let pack = load(&mut core).to_string();
    let sites = xref::sites(&pack).unwrap();
    let hits: Vec<&Value> = sites["sites"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|s| s["kind"] == "boss" && s["id"] == "cinder_warden")
        .collect();
    assert!(
        hits.iter()
            .any(|s| s["role"] == "defines" && s["file"] == "battle.toml")
    );
    assert!(hits.iter().any(|s| s["role"] == "reads"));
    let g = xref::graph(&pack).unwrap();
    assert!(g["nodes"].as_array().unwrap().len() > 20);
    assert!(!g["edges"].as_array().unwrap().is_empty());
}
