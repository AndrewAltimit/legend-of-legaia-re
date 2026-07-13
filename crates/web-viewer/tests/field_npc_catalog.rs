//! Verify the web viewer's field-NPC catalog
//! ([`legaia_web_viewer::field_npc::build_npc_catalog`]) resolves real actors
//! for representative scenes: the MAN's partition-1 placements decode, every
//! catalogued entry's model byte lands in the scene's TMD pool, and every
//! catalogued entry is **assemblable** - a multi-object TMD's vertices are
//! object-local, so listing one without a pose source would put a pile of
//! disconnected parts on the page.
//!
//! Skipped (passes) when `LEGAIA_DISC_BIN` is unset, matching the rest of the
//! disc-dependent test suite. CI runs without disc data.

#![cfg(not(target_arch = "wasm32"))]

use legaia_engine_core::scene::ProtIndex;
use legaia_web_viewer::disc::{extract_cdname_txt, extract_prot_dat};
use legaia_web_viewer::field_npc::build_npc_catalog;
use legaia_web_viewer::field_scene::build_field_scene;
use std::env;
use std::fs;

/// Rim Elm (the catalog's default, dialog-densest scene), a spring, a cave, and
/// Mt. Rikuroa - the last ships **no ANM bundle at all**, which is what the
/// unposable guard exists for.
const SCENES: &[&str] = &["town01", "izumi", "cave01", "rikuroa"];

fn index() -> Option<ProtIndex> {
    let disc_path = env::var_os("LEGAIA_DISC_BIN")?;
    let disc = fs::read(&disc_path).expect("disc image");
    let prot = extract_prot_dat(&disc).expect("PROT.DAT extraction");
    let cdname = extract_cdname_txt(&disc).expect("CDNAME.TXT extraction");
    Some(ProtIndex::from_bytes(prot, Some(&cdname)).expect("ProtIndex from in-memory PROT"))
}

#[test]
fn npc_catalog_only_lists_assemblable_actors() {
    let Some(index) = index() else {
        eprintln!("LEGAIA_DISC_BIN unset; skipping NPC-catalog test");
        return;
    };

    for &name in SCENES {
        let pack = build_field_scene(&index, name)
            .unwrap_or_else(|e| panic!("{name}: build_field_scene failed: {e}"));
        let npcs = build_npc_catalog(&index, name, &pack)
            .unwrap_or_else(|e| panic!("{name}: build_npc_catalog failed: {e}"));

        for e in &npcs.entries {
            // The model byte indexes the scene TMD pool directly - NOT the
            // env-pack subset the placement draws use. That index-space
            // equality is the whole binding.
            assert!(
                (e.placement.model_index as usize) < pack.res.tmds.len(),
                "{name}: actor {} model {} out of the {}-entry scene TMD pool",
                e.placement.index,
                e.placement.model_index,
                pack.res.tmds.len()
            );
            assert!(
                e.nobj > 0,
                "{name}: actor {} resolved to a 0-object TMD",
                e.placement.index
            );
            // The invariant the page depends on: anything multi-object that we
            // list must have a clip to pose it, and a bundle to read that clip
            // from. Otherwise it renders as unassembled parts.
            if e.nobj > 1 {
                assert!(
                    e.placement.anim_id > 0 && npcs.anm_prot.is_some(),
                    "{name}: actor {} has {} objects but no pose source \
                     (anim {}, bundle {:?}) - it should have been counted \
                     unposable, not catalogued",
                    e.placement.index,
                    e.nobj,
                    e.placement.anim_id,
                    npcs.anm_prot
                );
            }
        }
    }
}

/// Mt. Rikuroa is the scene that ships **no** ANM bundle while still placing
/// multi-object story actors. It's the reason the unposable guard exists, so
/// pin it: the guard must fire (the loop above asserts nothing multi-object
/// survived into the catalog).
#[test]
fn rikuroa_unposable_actors_are_withheld() {
    let Some(index) = index() else {
        eprintln!("LEGAIA_DISC_BIN unset; skipping rikuroa unposable pin");
        return;
    };
    let pack = build_field_scene(&index, "rikuroa").expect("rikuroa field scene");
    let npcs = build_npc_catalog(&index, "rikuroa", &pack).expect("rikuroa NPC catalog");

    assert!(
        npcs.anm_prot.is_none(),
        "rikuroa: expected no scene ANM bundle (the premise of this pin)"
    );
    assert!(
        npcs.unposable_count > 0,
        "rikuroa: expected its multi-object story actors to be withheld as unposable"
    );
}

/// Rim Elm is the page's default and the scene pinned against the engine-side
/// ground truth (`engine-core`'s `field_npc_placements_disc`): 52 partition-1
/// placements, the party/savepoint heads routed out, and a dense population of
/// talk-NPCs carrying the inline dialog the page uses as their name.
#[test]
fn town01_catalog_matches_engine_ground_truth() {
    let Some(index) = index() else {
        eprintln!("LEGAIA_DISC_BIN unset; skipping town01 NPC-catalog pin");
        return;
    };
    let pack = build_field_scene(&index, "town01").expect("town01 field scene");
    let npcs = build_npc_catalog(&index, "town01", &pack).expect("town01 NPC catalog");

    // Catalogued + party/savepoint + unposable = every partition-1 placement.
    let total = npcs.entries.len() as u32 + npcs.special_count + npcs.unposable_count;
    assert_eq!(
        total,
        52,
        "town01: expected 52 partition-1 placements, got {total} \
         ({} catalogued, {} party/savepoint, {} unposable)",
        npcs.entries.len(),
        npcs.special_count,
        npcs.unposable_count
    );
    // town01's actors all carry clips, so none are withheld.
    assert_eq!(
        npcs.unposable_count, 0,
        "town01: every actor names a clip; none should be unposable"
    );
    // Vahn / Noa / Gala / the savepoint are placed here and must route to the
    // characters page, not this catalog.
    assert!(
        npcs.special_count > 0,
        "town01: expected party/savepoint heads to be excluded from the catalog"
    );
    // Script-gated spawns (story NPCs not in town yet) are listed, flagged -
    // they're a big slice of Rim Elm's cast and dropping them would gut the
    // catalog.
    let conditional = npcs.entries.iter().filter(|e| e.conditional).count();
    assert!(
        conditional >= 10,
        "town01: expected the script-gated spawns to be catalogued, got {conditional}"
    );
    // The town is the game's dialog-densest scene; the inline-text label is
    // what the page shows as an NPC's name.
    let labelled = npcs.entries.iter().filter(|e| e.dialog.is_some()).count();
    assert!(
        labelled >= 5,
        "town01: expected >= 5 actors with inline-dialog labels, got {labelled}"
    );
}
