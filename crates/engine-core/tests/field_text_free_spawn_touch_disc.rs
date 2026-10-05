//! Disc-gated: a text-free placement whose interaction section spawns a
//! partition-2 record is touchable.
//!
//! Retail's touch post (`FUN_801D5B5C`) engages whatever placement the
//! interaction probe hits, and the dialog SM (`FUN_80039B7C`) runs its
//! interaction section from the PC whether or not it carries text. `retock`
//! `P1[32]` is the case the story depends on: the stand-in at `(121, 14)`
//! while `0x357` is set and `0x33B` clear, whose whole interaction is
//! `76 3C 08 00 44 65 21` - a spawn of `P2[16]`, or with `0x63C` set of
//! `P2[33]`, Eliza's Seru-bride scene, which raises `0x33C` and opens Lord
//! Saryu's room (`jagaroom`). The engine keyed interactions on text, so the
//! press found the stand-in and ran nothing.
//!
//! The test pins the record on the real disc and prints the disc-wide set of
//! text-free placements the spawn arm makes touchable. No Sony bytes are
//! asserted - only placement indices and an opcode class. Skip-passes
//! without disc data / extracted assets (CLAUDE.md convention).

use std::path::PathBuf;
use std::sync::Arc;

use legaia_asset::man_section::parse as parse_man;
use legaia_engine_core::man_field_scripts::{
    classify_placements, placement_inline_prologue, placement_scripted_menu_record,
};
use legaia_engine_core::scene::{ProtIndex, Scene};

fn extracted_dir() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("LEGAIA_EXTRACTED_DIR").map(PathBuf::from)
        && d.join("PROT.DAT").exists()
    {
        return Some(d);
    }
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

#[test]
fn text_free_spawn_interactions_are_touchable() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };
    let index = Arc::new(ProtIndex::open_extracted(&extracted).expect("open ProtIndex"));
    let mut names = index.cdname_scene_names();
    names.sort();
    names.dedup();

    let mut touchable: Vec<(String, usize)> = Vec::new();
    for name in &names {
        let Ok(scene) = Scene::load(&index, name) else {
            continue;
        };
        let Ok(Some(man)) = scene.field_man_payload(&index) else {
            continue;
        };
        let Ok(mf) = parse_man(&man) else {
            continue;
        };
        for (p, _) in classify_placements(&mf, &man) {
            if placement_inline_prologue(&mf, &man, &p).is_some() {
                continue;
            }
            if let Some(rec) = placement_scripted_menu_record(&mf, &man, &p) {
                // No text to page: the whole body is the interaction.
                assert_eq!(rec.first_segment, rec.body.len());
                touchable.push((name.clone(), p.index));
            }
        }
    }
    eprintln!("[ran] text-free touchable placements: {touchable:?}");
    assert!(
        touchable.iter().any(|(s, i)| s == "retock" && *i == 32),
        "retock P1[32] (Eliza's stand-in) must be touchable: {touchable:?}"
    );
}
