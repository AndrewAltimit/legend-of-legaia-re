//! Disc-gated: the v12-family dungeon scenes (`rikuroa`, `dolk2`) resolve
//! their MAN from the block's **streaming variant carrier** - the type-3
//! chunk of a `data_field_streaming` entry - not from an asset-table bundle.
//! Their own v12 sidecar (the block's 2nd retail entry) embeds only the
//! count-4 MAN-less sibling table, so `find_bundle`'s v12 fallback yields no
//! MAN-bearing table and [`Scene::field_man_payload`] falls back to
//! [`streaming_man_payloads`]. The live script heap at the Mt. Rikuroa
//! Caruban beat byte-matches the streaming chunk of PROT `0157` - the
//! streaming carrier IS the resident MAN.
//!
//! (The earlier reading - "rikuroa's MAN is v12-embedded at 0x1000 of PROT
//! 164" - decoded the NEXT block's sidecar: the unshifted CDNAME window bled
//! two entries into `geremi`, whose v12 copy of Jeremi's MAN sat at ext 164.
//! Same story for "dolk2's PROT 76", which is `suimon`'s sidecar.)
//!
//! Pins:
//!   - dolk2: streaming carrier ext `70`, MAN `0xAC04` B, partitions
//!     `[29, 73, 17]`.
//!   - rikuroa: streaming carrier ext `157`, MAN `0x74F0` B, partitions
//!     `[13, 29, 64]` (carries the post-Caruban story-flag `0x142` SETs).
//!
//! No-regression guard: `dolk` / `keikoku` (first-class scripted+bare table
//! pairs) and `map01` still load their MAN through the pre-existing
//! detectors.
//!
//! Skip-pass when `LEGAIA_DISC_BIN` is unset / `extracted/` missing (CLAUDE.md
//! disc-gated convention).

use std::path::PathBuf;

use legaia_engine_core::man_field_scripts::scene_destinations;
use legaia_engine_core::scene::{ProtIndex, Scene};
use legaia_engine_core::scene_bundle::{BundleSource, find_bundle, streaming_man_payloads};

fn extracted_root() -> Option<PathBuf> {
    for p in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(p);
        if d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

/// Load a scene's MAN through the engine's resolution order (bundle first,
/// streaming variant fallback) and parse it.
fn scene_man(index: &ProtIndex, name: &str) -> (legaia_asset::man_section::ManFile, Vec<u8>) {
    let scene = Scene::load(index, name).unwrap_or_else(|e| panic!("load {name}: {e:#}"));
    let man_bytes = scene
        .field_man_payload(index)
        .unwrap_or_else(|e| panic!("{name}: field_man_payload: {e:#}"))
        .unwrap_or_else(|| panic!("{name}: no MAN resolves"));
    let mf = legaia_asset::man_section::parse(&man_bytes)
        .unwrap_or_else(|e| panic!("{name}: parse MAN: {e:#}"));
    (mf, man_bytes)
}

#[test]
fn v12_embedded_bundles_yield_a_man() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(root) = extracted_root() else {
        eprintln!("[skip] extracted/ (CDNAME.TXT) missing");
        return;
    };
    let index = ProtIndex::open_extracted(&root).expect("prot index");

    // -- dolk2: streaming carrier ext 70, MAN partitions [29, 73, 17] -----
    {
        let scene = Scene::load(&index, "dolk2").expect("load dolk2");
        let streams = streaming_man_payloads(&scene);
        assert_eq!(streams.len(), 1, "dolk2 has one streaming MAN carrier");
        assert_eq!(streams[0].0, 70, "dolk2 streaming carrier is ext 70");

        let (mf, man) = scene_man(&index, "dolk2");
        assert_eq!(man.len(), 0xAC04, "dolk2 MAN size");
        assert_eq!(
            mf.header.partition_counts,
            [29, 73, 17],
            "dolk2 MAN partition counts"
        );

        let dests: Vec<String> = scene_destinations(&mf, &man)
            .iter()
            .map(|d| d.scene_name.clone())
            .collect();
        eprintln!("[dolk2] destinations: {dests:?}");
    }

    // -- rikuroa: streaming carrier ext 157, MAN partitions [13, 29, 64] ---
    {
        let scene = Scene::load(&index, "rikuroa").expect("load rikuroa");
        let streams = streaming_man_payloads(&scene);
        assert_eq!(streams.len(), 1, "rikuroa has one streaming MAN carrier");
        assert_eq!(streams[0].0, 157, "rikuroa streaming carrier is ext 157");

        let (mf, man) = scene_man(&index, "rikuroa");
        assert_eq!(man.len(), 0x74F0, "rikuroa MAN size");
        assert_eq!(
            mf.header.partition_counts,
            [13, 29, 64],
            "rikuroa MAN partition counts"
        );
        let dests = scene_destinations(&mf, &man);
        eprintln!("[rikuroa] {} named 0x3F destination(s)", dests.len());
    }

    eprintln!("[ok] v12-family streaming-carrier MANs (rikuroa, dolk2) load");
}

#[test]
fn first_class_bundles_still_load_no_regression() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(root) = extracted_root() else {
        eprintln!("[skip] extracted/ (CDNAME.TXT) missing");
        return;
    };
    let index = ProtIndex::open_extracted(&root).expect("prot index");

    // dolk / keikoku have a first-class scripted+bare table pair; the v12
    // fallback must NOT take over and they keep their scripted bundle.
    for (name, want_off) in [("dolk", 0x800usize), ("keikoku", 0x800)] {
        let scene = Scene::load(&index, name).expect("load");
        let bundle = find_bundle(&scene).unwrap_or_else(|| panic!("{name}: no bundle"));
        assert!(
            matches!(bundle, BundleSource::Scripted { .. }),
            "{name} keeps its scripted bundle, not the v12 fallback"
        );
        assert_eq!(bundle.table_offset(), want_off, "{name} table offset");
        let (mf, _man) = scene_man(&index, name);
        assert!(
            mf.header.total_records() > 0,
            "{name} MAN still parses with records"
        );
    }

    // map01 (the Drake overworld) still resolves its MAN + destination table.
    let (mf, man) = scene_man(&index, "map01");
    let dests: Vec<String> = scene_destinations(&mf, &man)
        .iter()
        .map(|d| d.scene_name.clone())
        .collect();
    for expected in ["rikuroa", "dolk", "dolk2"] {
        assert!(
            dests.iter().any(|d| d == expected),
            "map01 still lists {expected}; got {dests:?}"
        );
    }
    eprintln!("[ok] first-class bundles (dolk/keikoku/map01) unaffected");
}
