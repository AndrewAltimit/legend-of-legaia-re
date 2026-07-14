//! Town-identity regression: the site scene picker (`site/_gen.py`
//! `CDNAME_SCENES`) names each CDNAME block after the town it actually loads.
//! Two picker entries used to be mislabeled - `rayman`/"Ratayu" actually loads
//! Octam, and `town0d`/"Sol" actually loads a Rim Elm story-variant. This test
//! pins the ground truth the corrected labels rest on, read the same way the
//! browser does (the engine-decoded NPC dialogue surfaced by the field-NPC
//! catalog), so a future edit that reintroduces the wrong name fails here.
//!
//! Skipped (passes) when `LEGAIA_DISC_BIN` is unset, matching the rest of the
//! disc-dependent suite. CI runs without disc data.

#![cfg(not(target_arch = "wasm32"))]

use legaia_engine_core::scene::{ProtIndex, Scene};
use legaia_web_viewer::disc::{extract_cdname_txt, extract_prot_dat};
use legaia_web_viewer::field_npc::build_npc_catalog;
use legaia_web_viewer::field_scene::build_field_scene;
use std::env;
use std::fs;

fn index() -> Option<ProtIndex> {
    let disc_path = env::var_os("LEGAIA_DISC_BIN")?;
    let disc = fs::read(&disc_path).expect("disc image");
    let prot = extract_prot_dat(&disc).expect("PROT.DAT extraction");
    let cdname = extract_cdname_txt(&disc).expect("CDNAME.TXT extraction");
    Some(ProtIndex::from_bytes(prot, Some(&cdname)).expect("ProtIndex"))
}

/// All NPC dialog first-lines the scene's field-NPC catalog decodes, joined.
fn scene_dialog(index: &ProtIndex, name: &str) -> String {
    let pack =
        build_field_scene(index, name).unwrap_or_else(|e| panic!("{name}: build_field_scene: {e}"));
    let npcs = build_npc_catalog(index, name, &pack)
        .unwrap_or_else(|e| panic!("{name}: build_npc_catalog: {e}"));
    npcs.entries
        .iter()
        .filter_map(|e| e.dialog.clone())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Byte-fraction two scenes' slot-0 `.MAP` (the walkability grid) share. The
/// `town0b..0e` blocks are Rim Elm variants, so their `.MAP` is near-identical
/// to town01's; a genuinely different town would score far lower.
fn map_similarity(index: &ProtIndex, a: &str, b: &str) -> f64 {
    let load = |n: &str| {
        let scene = Scene::load(index, n).unwrap_or_else(|e| panic!("{n}: load: {e:#}"));
        (*scene
            .entries
            .first()
            .unwrap_or_else(|| panic!("{n}: no entries"))
            .bytes)
            .clone()
    };
    let (ma, mb) = (load(a), load(b));
    let n = ma.len().min(mb.len());
    if n == 0 {
        return 0.0;
    }
    let same = (0..n).filter(|&i| ma[i] == mb[i]).count();
    same as f64 / n as f64
}

#[test]
fn rayman_is_octam_not_ratayu() {
    let Some(index) = index() else {
        eprintln!("LEGAIA_DISC_BIN unset; skipping");
        return;
    };
    // Octam's ruler Hari greets you; the scene's dialogue is about Octam.
    let dlg = scene_dialog(&index, "rayman");
    assert!(
        dlg.contains("Hari"),
        "rayman (labeled Octam) should have Octam's ruler Hari in its NPC dialog"
    );
    // rayman2 is the Octam revisit - Hari again, not Ratayu.
    let dlg2 = scene_dialog(&index, "rayman2");
    assert!(
        dlg2.contains("Hari"),
        "rayman2 (labeled Octam revisit) should still feature Hari"
    );
}

#[test]
fn town0b_to_0e_are_rim_elm_variants() {
    let Some(index) = index() else {
        eprintln!("LEGAIA_DISC_BIN unset; skipping");
        return;
    };
    // Walkability `.MAP` near-identity to town01 (Rim Elm): the variants reuse
    // the village footprint. town0e (the epilogue) diverges most but town0b/0c
    // are byte-identical and town0d nearly so.
    for (variant, floor) in [("town0b", 0.99), ("town0c", 0.99), ("town0d", 0.95)] {
        let sim = map_similarity(&index, "town01", variant);
        assert!(
            sim >= floor,
            "{variant} .MAP should be >= {floor:.0}% identical to town01 (Rim Elm), got {:.3}",
            sim
        );
    }
    // The Rim Elm cast / landmarks appear in every variant - the strongest
    // single signal that these are Rim Elm, not the towns the old labels named.
    for variant in ["town0b", "town0c", "town0d", "town0e"] {
        let dlg = scene_dialog(&index, variant);
        assert!(
            ["Meta", "Ixis", "Maya", "Mei", "Wall", "Genesis"]
                .iter()
                .any(|m| dlg.contains(m)),
            "{variant} (a Rim Elm variant; town0d was mislabeled Sol) should \
             feature the Rim Elm cast / a Rim Elm landmark; dialog was: {dlg:?}"
        );
    }
}
