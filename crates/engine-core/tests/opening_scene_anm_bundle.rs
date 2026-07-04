//! Disc-gated invariant behind the opening-cutscene vignette animation.
//!
//! The play-window render path builds a looping `FieldClipPlayer` for each
//! scene-actor placement from the scene's per-scene ANM bundle (the type-`0x05`
//! section `legaia_asset::player_anm::find_in_entry` walks). That lookup is
//! seeded with a descriptor count, and it turned out the count is NOT uniform:
//! `town01`'s bundle surfaces at count 3, but the opening prologue scenes
//! (`opdeene` / `opstati` / `opurud`) only surface theirs at count >= 5. The
//! render path used to hardcode 3, so those three scenes resolved NO bundle and
//! their vignette actors rendered as a frozen tableau under the narration crawl.
//! The fix searches counts `[3, 5, 6, 7]`; this test pins the disc reason it
//! must: prologue scenes need a count above 3, town01 is fine at 3.
//!
//! Skip-passes without disc data / extracted assets (CLAUDE.md convention).

use legaia_engine_core::scene::{ProtIndex, Scene};
use std::path::PathBuf;

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

/// First bundle any scene entry yields at descriptor count `desc`.
fn bundle_at(scene: &Scene, desc: usize) -> bool {
    scene
        .entries
        .iter()
        .any(|e| !legaia_asset::player_anm::find_in_entry(&e.bytes, desc).is_empty())
}

/// The render path's actual search: first bundle across `[3, 5, 6, 7]`.
fn bundle_found(scene: &Scene) -> bool {
    [3usize, 5, 6, 7].iter().any(|&d| bundle_at(scene, d))
}

#[test]
fn prologue_scene_anm_bundles_resolve() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(ex) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };
    let index = ProtIndex::open_extracted(&ex).expect("open ProtIndex");

    // town01 (the always-worked case): its bundle resolves at count 3.
    let town01 = Scene::load(&index, "town01").expect("load town01");
    assert!(
        bundle_at(&town01, 3),
        "town01's ANM bundle must still resolve at descriptor count 3"
    );

    // The opening prologue scenes: the count-3-only lookup found NOTHING here
    // (the frozen-tableau bug); the widened search must find a bundle so their
    // vignette actors get looping clip players and animate under the crawl.
    for scene_name in ["opdeene", "opstati", "opurud"] {
        let scene = Scene::load(&index, scene_name).expect("load prologue scene");
        assert!(
            !bundle_at(&scene, 3),
            "{scene_name} regression guard: if this now resolves at count 3 the \
             widened search is moot - re-pin the descriptor counts"
        );
        assert!(
            bundle_found(&scene),
            "{scene_name}: the widened [3,5,6,7] search MUST find the per-scene \
             ANM bundle (the vignette-actor animation source)"
        );
    }
    eprintln!("[opening] prologue scene ANM bundles resolve via the widened search");
}
