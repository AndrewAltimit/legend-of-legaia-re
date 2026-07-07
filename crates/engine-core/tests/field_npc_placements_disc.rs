//! Disc-gated: the MAN partition-1 placement -> NPC model/animation
//! resolution the play-window field renderer uses.
//!
//! Runtime-pinned against the anchor-C town01 field save (53/53 animated
//! actors):
//!
//!  - the retail `0x8007C018` pool registers the party/savepoint head in
//!    slots `0..5` and the scene TMD list (in scene order) from slot 5, so a
//!    placement's model byte `< 0xF0` is a direct index into the engine's
//!    scene TMD list;
//!  - a model byte `>= 0xF0` selects global-pool head slot `model - 0xF0`;
//!  - the placement's `anim_id` byte (installed into actor `+0x5C`) is the
//!    scene-bundle ANM record index + 1 (`0` = none); special models index
//!    the PROT 0874 §1 locomotion bundle instead - the Noa/Gala placements
//!    carry exactly their locomotion idle records.
//!
//! Skips (passes) when `LEGAIA_DISC_BIN` is unset.

use std::path::PathBuf;
use std::sync::Arc;

use legaia_engine_core::scene::{ProtIndex, Scene};
use legaia_engine_core::scene_resources::{
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
fn town01_npc_placements_resolve_models_and_anim_records() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };
    let index = Arc::new(ProtIndex::open_extracted(&extracted).expect("open ProtIndex"));
    let scene = Scene::load(&index, "town01").expect("load town01");

    // Scene TMD list, built exactly as play-window builds it (Field kind).
    let mut shared_scenes: Vec<Scene> = Vec::new();
    for name in FIELD_SHARED_BLOCKS {
        if let Ok(sc) = Scene::load(&index, name) {
            shared_scenes.push(sc);
        }
    }
    let shared_refs: Vec<&Scene> = shared_scenes.iter().collect();
    let (res, _stats) = SceneResources::build_targeted_with_options(
        &scene,
        &shared_refs,
        BuildOptions {
            kind: SceneLoadKind::Field,
            upload_all_tims: false,
        },
    )
    .expect("build town01 scene resources");
    // The live pool table holds 119 populated slots = the 5-slot head +
    // 114 scene TMDs (anchor-C). With the shared `player_data` block
    // resolving to the retail character pack (extraction 0874), the head's
    // 5 player TMDs now arrive through the same sweep, so the resolved
    // pool equals the live populated-slot count exactly.
    assert_eq!(res.tmds.len(), 119, "town01 TMD pool incl. player head");

    // The per-scene ANM bundle in the player-ANM frame-stream layout (the
    // type-0x05 section of the scene's first PROT slot).
    let bundle = scene
        .entries
        .iter()
        .find_map(|e| {
            legaia_asset::player_anm::find_in_entry(&e.bytes, 3)
                .into_iter()
                .next()
        })
        .expect("town01 scene ANM bundle parses in the player-ANM layout");
    assert_eq!(bundle.record_count, 69, "town01 scene bundle record count");

    let man = scene
        .field_man_payload(&index)
        .expect("read MAN")
        .expect("town01 has a MAN");
    let mf = legaia_asset::man_section::parse(&man).expect("parse MAN");
    let placements = legaia_engine_core::man_field_scripts::classify_placements(&mf, &man);
    assert_eq!(placements.len(), 52, "town01 partition-1 placement count");

    let mut animated = 0usize;
    for (p, _kind) in &placements {
        if p.special_model {
            // Head slots only (Vahn/Noa/Gala/savepoint/aux).
            let head = (p.model_index - 0xF0) as usize;
            assert!(
                head < 5,
                "placement {} special model {head} out of head",
                p.index
            );
            continue;
        }
        // Model byte indexes the scene TMD list directly.
        let m = p.model_index as usize;
        assert!(
            m < res.tmds.len(),
            "placement {} model {m} outside the scene TMD list",
            p.index
        );
        if p.anim_id == 0 {
            continue;
        }
        // anim_id - 1 must name a real bundle record whose bone count fits
        // the model's object table (the FUN_8001B964 count contract, with
        // the trailing untracked objects truncated off).
        let rec_idx = (p.anim_id - 1) as usize;
        let rec = bundle.record(rec_idx).unwrap_or_else(|e| {
            panic!(
                "placement {} anim_id {} -> record {rec_idx} invalid: {e:#}",
                p.index, p.anim_id
            )
        });
        let nobj = res.tmds[m].tmd.objects.len();
        assert!(
            (rec.bone_count as usize) <= nobj,
            "placement {} record {rec_idx} bones {} > model {m} nobj {nobj}",
            p.index,
            rec.bone_count
        );
        animated += 1;
    }
    assert!(
        animated >= 30,
        "town01 should have >=30 animated placements, found {animated}"
    );

    // Cross-pin: the special-model Noa/Gala placements carry their PROT 0874
    // section-1 locomotion **idle** records (bank slot 1) as anim ids -
    // record 8 (Noa) and record 15 (Gala), i.e. ids 9 and 16.
    let noa = placements
        .iter()
        .find(|(p, _)| p.special_model && p.model_index == 0xF1)
        .map(|(p, _)| p.anim_id);
    let gala = placements
        .iter()
        .find(|(p, _)| p.special_model && p.model_index == 0xF2)
        .map(|(p, _)| p.anim_id);
    let idle = |slot: usize| {
        (legaia_asset::character_pack::locomotion_record_index(
            slot,
            legaia_asset::character_pack::LOCOMOTION_IDLE_SLOT,
        ) + 1) as u8
    };
    assert_eq!(noa, Some(idle(1)), "Noa placement anim = locomotion idle");
    assert_eq!(gala, Some(idle(2)), "Gala placement anim = locomotion idle");
}
