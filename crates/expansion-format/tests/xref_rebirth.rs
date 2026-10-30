//! The reference model over the flagship pack: it finds every reference the
//! validator resolves, and renaming any id of any kind leaves the pack
//! loading and validating as before, changes exactly the files the rename
//! plan names, and renames back to the same pack.

use std::collections::BTreeSet;
use std::path::PathBuf;

use legaia_expansion_format::validate::{Severity, declarations_and_references, validate};
use legaia_expansion_format::xref::{self, Kind, Role};
use legaia_expansion_format::{ExpansionPack, flow};

fn rebirth() -> ExpansionPack {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../packs/rebirth");
    ExpansionPack::from_dir(&dir).unwrap()
}

#[test]
fn the_model_holds_every_validator_reference() {
    let pack = rebirth();
    let ix = xref::index(&pack);
    let mut missing = Vec::new();
    for (kind, id, file, field) in declarations_and_references(&pack) {
        let k = Kind::parse(kind).unwrap_or_else(|| panic!("kind {kind}"));
        let local = id.split_once(':').map_or(id.as_str(), |(_, l)| l);
        let found = ix.of(k, local).any(|s| {
            s.file == file
                && (s.field == field
                    || s.field.starts_with(&format!("{field}."))
                    || s.field.starts_with(&format!("{field}[")))
        });
        if !found {
            missing.push(format!("{kind} {id} at {file} [{field}]"));
        }
    }
    assert!(missing.is_empty(), "{missing:#?}");
}

#[test]
fn the_model_covers_every_kind_rebirth_uses() {
    let ix = xref::index(&rebirth());
    for kind in [
        Kind::Monster,
        Kind::Formation,
        Kind::Boss,
        Kind::Quest,
        Kind::Challenge,
        Kind::Flag,
        Kind::Counter,
        Kind::Card,
        Kind::Booster,
        Kind::Item,
        Kind::Art,
        Kind::Spell,
        Kind::Map,
        Kind::Npc,
        Kind::Cutscene,
        Kind::Line,
        Kind::Mode,
    ] {
        assert!(!ix.ids(kind).is_empty(), "{kind:?}");
    }
}

fn counts(pack: &ExpansionPack) -> (usize, usize) {
    let issues = validate(pack);
    let errors = issues
        .iter()
        .filter(|i| i.severity == Severity::Error)
        .count();
    (errors, issues.len() - errors)
}

#[test]
fn renaming_any_id_keeps_the_pack_whole() {
    let pack = rebirth();
    let base_counts = counts(&pack);
    assert_eq!(base_counts.0, 0);
    let base_files = pack.to_files().unwrap();
    let ix = xref::index(&pack);
    let value = serde_json::to_value(&pack).unwrap();
    let mut renamed = 0;
    for &kind in Kind::ALL {
        // Up to eight ids of each kind, spread across the pack.
        let ids: Vec<String> = ix.ids(kind).into_iter().collect();
        let step = ids.len().div_ceil(8).max(1);
        for id in ids.into_iter().step_by(step) {
            // Engine flags follow their owner; a rename starts at the owner.
            if kind == Kind::Flag && (id.contains('.')) {
                continue;
            }
            let mut new = format!("rn_{id}");
            new.truncate(48);
            let mut v = value.clone();
            let changes = xref::rename(&mut v, kind, &id, &new)
                .unwrap_or_else(|e| panic!("{kind:?} {id}: {e}"));
            assert!(
                changes.iter().any(|c| c.role == Role::Defines)
                    || matches!(kind, Kind::Flag | Kind::Counter)
            );
            let mut edited: ExpansionPack =
                serde_json::from_value(v.clone()).unwrap_or_else(|e| panic!("{kind:?} {id}: {e}"));
            // The JSON form carries no files; the workbench session keeps them.
            edited.assets.clone_from(&pack.assets);
            edited.docs.clone_from(&pack.docs);
            // Validates as before: no new error, the same number of warnings.
            if counts(&edited) != base_counts {
                let errs: Vec<_> = validate(&edited)
                    .into_iter()
                    .filter(|i| i.severity == Severity::Error)
                    .take(5)
                    .collect();
                panic!("{kind:?} {id}: {errs:#?}");
            }
            // No site of the old id is left.
            let after = xref::index(&edited);
            assert!(
                after.of(kind, &id).next().is_none(),
                "{kind:?} {id} left behind"
            );
            // The exported files that changed are exactly the planned ones.
            let mut files = edited.to_files().unwrap();
            if kind == Kind::Map {
                let moved = files.remove(&format!("maps/{new}.toml")).unwrap();
                files.insert(format!("maps/{id}.toml"), moved);
            }
            let mut changed: BTreeSet<&str> = base_files
                .iter()
                .filter(|(p, b)| files.get(*p) != Some(b))
                .map(|(p, _)| p.as_str())
                .chain(
                    files
                        .keys()
                        .filter(|p| !base_files.contains_key(*p))
                        .map(String::as_str),
                )
                .collect();
            if kind == Kind::Map {
                // The map's own file moves to its new name.
                changed.insert(
                    base_files
                        .keys()
                        .find(|p| **p == format!("maps/{id}.toml"))
                        .unwrap(),
                );
            }
            let planned: BTreeSet<&str> = changes.iter().map(|c| c.file.as_str()).collect();
            assert_eq!(changed, planned, "{kind:?} {id}");
            // Renaming back is the pack it came from.
            let mut back = v;
            xref::rename(&mut back, kind, &new, &id).unwrap();
            let mut back: ExpansionPack = serde_json::from_value(back).unwrap();
            back.assets.clone_from(&pack.assets);
            back.docs.clone_from(&pack.docs);
            assert_eq!(back, pack, "{kind:?} {id} does not rename back");
            renamed += 1;
        }
    }
    assert!(renamed > 60, "{renamed}");
}

#[test]
fn rename_refuses_a_taken_or_bad_id() {
    let value = serde_json::to_value(rebirth()).unwrap();
    let ix = xref::index_value(&value);
    let quests: Vec<String> = ix.ids(Kind::Quest).into_iter().collect();
    let mut v = value.clone();
    assert!(xref::rename(&mut v, Kind::Quest, &quests[0], &quests[1]).is_err());
    assert!(xref::rename(&mut v, Kind::Quest, &quests[0], "Has Space").is_err());
    assert!(xref::rename(&mut v, Kind::Quest, "no_such_quest", "fresh").is_err());
    assert_eq!(v, value, "a refused rename changes nothing");
}

#[test]
fn rebirth_flow_graph_draws_its_progression() {
    let pack = rebirth();
    let g = flow::graph(&pack);
    let kinds: BTreeSet<&str> = g.nodes.iter().map(|n| n.kind.as_str()).collect();
    for k in [
        "entry", "map", "quest", "boss", "cutscene", "flag", "counter",
    ] {
        assert!(kinds.contains(k), "{k}: {kinds:?}");
    }
    let keys: BTreeSet<&str> = g.nodes.iter().map(|n| n.key.as_str()).collect();
    for e in &g.edges {
        assert!(
            keys.contains(e.from.as_str()) && keys.contains(e.to.as_str()),
            "{e:?}"
        );
    }
    // The entrance map is reached from the entry node.
    assert!(
        g.edges
            .iter()
            .any(|e| e.from == "entry:" && e.to == "map:rebirth_tower_1f" && e.role == "enters")
    );
    // Every lint finding is on a node or listed on its own.
    let on_nodes: usize = g.nodes.iter().map(|n| n.problems.len()).sum();
    assert_eq!(on_nodes + g.problems.len(), flow::lint(&pack).len());
}
