//! Disc-gated smoke test: locate an `izumi`-style scene bundle, walk its
//! 7 descriptors, and LZS-decompress descriptor 0 (the canonical TIM_LIST).
//!
//! Skips silently when `extracted/` or `LEGAIA_DISC_BIN` is missing.

use std::path::PathBuf;

use legaia_engine_core::scene::{ProtIndex, Scene};
use legaia_engine_core::scene_bundle;

fn extracted_dir() -> Option<PathBuf> {
    let d = PathBuf::from("extracted");
    if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
        Some(d)
    } else {
        let alt = PathBuf::from("../../extracted");
        if alt.join("PROT.DAT").exists() && alt.join("CDNAME.TXT").exists() {
            Some(alt)
        } else {
            None
        }
    }
}

#[test]
fn scene_bundle_lzs_extracts_descriptor_0_for_real_scene() {
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }

    let index = ProtIndex::open_extracted(&extracted).expect("open ProtIndex");

    // Sweep every scene name in CDNAME, look for the first that has a plain
    // or scripted asset-table entry, and LZS-extract descriptor 0.
    let cdname = legaia_prot::cdname::parse(&extracted.join("CDNAME.TXT")).expect("parse cdname");
    let mut scenes: Vec<String> = cdname.values().cloned().collect();
    scenes.sort();
    scenes.dedup();

    let mut bundle_found = 0u32;
    let mut lzs_ok = 0u32;
    for scene_name in &scenes {
        let Ok(scene) = Scene::load(&index, scene_name) else {
            continue;
        };
        let Some(bundle) = scene_bundle::find_bundle(&scene) else {
            continue;
        };
        bundle_found += 1;

        let descs = scene_bundle::walk_descriptors(&bundle);
        assert_eq!(descs.len(), 7);
        // First descriptor is canonical TIM_LIST (or near-equivalent) in
        // the standard variant.
        match scene_bundle::extract_descriptor_0_lzs(&bundle) {
            Ok((decoded, consumed)) => {
                assert_eq!(
                    decoded.len() as u32,
                    descs[0].descriptor.size,
                    "decoded len doesn't match descriptor size in scene '{scene_name}'"
                );
                assert!(
                    consumed > 0 && consumed < bundle.bytes().len(),
                    "consumed={consumed} bundle_len={}",
                    bundle.bytes().len()
                );
                lzs_ok += 1;
            }
            Err(e) => {
                eprintln!("[note] scene '{scene_name}' descriptor 0 LZS failed: {e}");
            }
        }
    }

    eprintln!(
        "[smoke] {} bundles found, {} LZS-extracted cleanly",
        bundle_found, lzs_ok
    );
    assert!(
        bundle_found > 0,
        "no scene bundles found in any CDNAME scene - find_bundle is broken"
    );
    // Most bundles should LZS-decode. Set a generous floor (>50%) so this is
    // robust to a few outlier scenes whose descriptor 0 isn't LZS.
    assert!(
        lzs_ok * 2 >= bundle_found,
        "only {} of {} bundles LZS-decoded - descriptor 0 extractor regressed",
        lzs_ok,
        bundle_found
    );
}
