//! Disc-gated census: which PROT entry each placed actor's model comes from.
//!
//! A MAN partition-1 placement's model byte `< 0xF0` indexes the scene's TMD
//! list, `>= 0xF0` a global head slot (the player character pack). This walks
//! every CDNAME scene with a MAN and records, per placement, the PROT entry of
//! the resolved TMD, so a placement whose mesh comes from outside the scene's
//! own CDNAME block (a "borrowed" model) is named with its carrier.
//!
//! Skips (passes) when `LEGAIA_DISC_BIN` is unset.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use legaia_engine_scene::scene::{ProtIndex, Scene};
use legaia_engine_scene::scene_resources::{
    BuildOptions, FIELD_SHARED_BLOCKS, SceneLoadKind, SceneResources,
};

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

#[test]
fn placed_actor_models_by_carrier() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };
    let index = Arc::new(ProtIndex::open_extracted(&extracted).expect("open ProtIndex"));
    let cdname = std::fs::read_to_string(extracted.join("CDNAME.TXT")).expect("CDNAME");
    let names: Vec<String> = cdname
        .lines()
        .filter_map(|l| {
            let mut w = l.split_whitespace();
            (w.next() == Some("#define")).then(|| w.next().map(str::to_string))?
        })
        .collect();

    let mut shared_scenes: Vec<Scene> = Vec::new();
    for name in FIELD_SHARED_BLOCKS {
        if let Ok(sc) = Scene::load(&index, name) {
            shared_scenes.push(sc);
        }
    }
    let shared_refs: Vec<&Scene> = shared_scenes.iter().collect();
    let shared_idx: Vec<u32> = shared_scenes
        .iter()
        .flat_map(|s| s.entries.iter().map(|e| e.idx))
        .collect();

    let (mut scenes, mut placed, mut specials, mut unresolved) = (0, 0, 0, 0);
    // carrier class -> placements
    let mut borrowed: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for name in &names {
        let Ok(scene) = Scene::load(&index, name) else {
            continue;
        };
        let Ok(Some(man)) = scene.field_man_payload(&index) else {
            continue;
        };
        let Ok(mf) = legaia_scene_formats::man_section::parse(&man) else {
            continue;
        };
        let Ok((res, _)) = SceneResources::build_targeted_with_options(
            &scene,
            &shared_refs,
            BuildOptions {
                kind: SceneLoadKind::Field,
                upload_all_tims: false,
                ..Default::default()
            },
        ) else {
            continue;
        };
        scenes += 1;
        let own: Vec<u32> = scene.entries.iter().map(|e| e.idx).collect();
        let own_tmds = res
            .tmds
            .iter()
            .filter(|t| own.contains(&t.entry_idx))
            .count();
        for (p, _) in legaia_engine_field::man_field_scripts::classify_placements(&mf, &man) {
            placed += 1;
            if p.special_model {
                specials += 1;
                continue;
            }
            let Some(t) = res.tmds.get(p.model_index as usize) else {
                unresolved += 1;
                borrowed
                    .entry("unresolved".into())
                    .or_default()
                    .push(format!("{name}#{} m{}", p.index, p.model_index));
                continue;
            };
            if own.contains(&t.entry_idx) {
                continue;
            }
            assert!(
                p.model_index as usize >= own_tmds,
                "{name}#{}: model {} inside the scene's {own_tmds} TMDs yet sourced elsewhere",
                p.index,
                p.model_index
            );
            let class = if shared_idx.contains(&t.entry_idx) {
                format!("shared {}", t.entry_idx)
            } else {
                format!("other {}", t.entry_idx)
            };
            borrowed
                .entry(class)
                .or_default()
                .push(format!("{name}#{} m{}", p.index, p.model_index));
        }
    }
    eprintln!(
        "[npc-source] {scenes} scenes, {placed} placements, {specials} global-head specials, \
         {unresolved} unresolved"
    );
    for (k, v) in &borrowed {
        eprintln!(
            "[npc-source] {k}: {} e.g. {:?}",
            v.len(),
            &v[..v.len().min(6)]
        );
    }
    assert!(scenes >= 100, "scenes with a MAN: {scenes}");
    assert!(specials > 0, "the global-head specials are counted");
    // Every placement outside the scene's own block is in `other7`, the unused
    // koin3 draft: its model bytes run past its own TMD list.
    let offenders: Vec<&String> = borrowed.values().flatten().collect();
    assert_eq!(offenders.len(), 20, "{borrowed:?}");
    assert!(
        offenders.iter().all(|s| s.starts_with("other7#")),
        "a loadable scene borrows a model: {borrowed:?}"
    );
}
