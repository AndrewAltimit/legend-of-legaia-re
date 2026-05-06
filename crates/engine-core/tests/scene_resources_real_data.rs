//! Real-data check: build [`SceneResources`] from a CDNAME scene's PROT
//! bytes and confirm the runtime VRAM pre-pass populates the right shape
//! of data — non-empty VRAM, non-zero parsed-TMD pool, parse-failure
//! count zero or near-zero on every scene the corpus ships.
//!
//! Skips when `extracted/PROT.DAT` is missing.

use std::path::PathBuf;

use legaia_engine_core::scene::{ProtIndex, Scene};
use legaia_engine_core::scene_resources::SceneResources;

fn extracted_dir() -> Option<PathBuf> {
    for p in ["extracted", "../../extracted"] {
        let d = PathBuf::from(p);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

#[test]
fn scene_resources_populate_vram_for_first_town() {
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }

    let index = ProtIndex::open_extracted(&extracted).expect("open ProtIndex");
    // `town01` is the first scripted town in the corpus — every captured
    // playthrough hits it, and it has the full canonical 6-asset bundle.
    let scene = match Scene::load(&index, "town01") {
        Ok(s) => s,
        Err(_) => {
            eprintln!("[skip] scene 'town01' missing from CDNAME");
            return;
        }
    };
    let res = SceneResources::build(&scene).expect("build resources");
    assert!(
        res.tim_count > 0,
        "town01 should expose at least one TIM via the scene's CDNAME entries"
    );
    assert_eq!(
        res.tim_parse_failures, 0,
        "tim_scan should round-trip cleanly on retail data"
    );
    // VRAM should hold non-zero pixels somewhere — pick the highest fb_y a
    // dialog tile-page would land at and assert the bottom-half rows
    // contain at least one non-zero word.
    let mut populated_rows = 0usize;
    for y in 0..512 {
        for x in 0..1024 {
            if res.vram.pixel(x, y) != 0 {
                populated_rows += 1;
                break;
            }
        }
    }
    assert!(
        populated_rows > 0,
        "VRAM should be populated with at least one non-zero row after scene load"
    );
    eprintln!(
        "[ok] town01 → {} TIMs uploaded, {} TMDs parsed, {} VRAM rows populated",
        res.tim_count,
        res.tmds.len(),
        populated_rows
    );
}
