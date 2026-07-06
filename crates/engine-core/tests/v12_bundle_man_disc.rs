//! Disc-gated: the v12-family dungeon bundles (`rikuroa`, `dolk2`) carry their
//! MAN inside a `scene_asset_table` embedded at file offset `0x1000` of the
//! `Class::SceneV12Table` entry itself - the v12 runtime-fixup header wins the
//! classifier at offset 0, so these scenes have no first-class
//! `SceneAssetTable` / `SceneScriptedAssetTable` sibling. `find_bundle`'s
//! v12 fallback scans 0x800-aligned offsets for the MAN-bearing (type-3) table
//! and reports it as [`BundleSource::V12Embedded`] with `table_offset = 0x1000`;
//! [`extract_man_payload`] then resolves `table_offset + data_offset` against
//! the entry's extended footprint exactly as it does for the scripted variant.
//!
//! Pins:
//!   - dolk2 (PROT 76): MAN desc size `0x929`, data_off `0x1a89e` -> abs
//!     `0x1b89e`, decodes to 2345 B, `man_section::parse` OK, partitions
//!     `[10, 7, 3]`; its scene-destination table lists `map01` (the overworld
//!     return).
//!   - rikuroa (PROT 164): MAN desc size `0x9a54`, data_off `0x40927` -> abs
//!     `0x41927`, decodes to 39508 B, parse OK, partitions `[18, 70, 20]`; its
//!     MAN carries no named `0x3F` warp (the Ravine's exit / first-boss trigger
//!     is gated by the partition-2 cutscene timeline, not a scene-change op),
//!     so its scene-destination table decodes to an empty list without error.
//!
//! No-regression guard: `dolk` / `keikoku` (first-class scripted+bare table
//! pairs) and `map01` still load their MAN through the pre-existing detectors.
//!
//! Skip-pass when `LEGAIA_DISC_BIN` is unset / `extracted/` missing (CLAUDE.md
//! disc-gated convention).

use std::path::PathBuf;

use legaia_engine_core::man_field_scripts::scene_destinations;
use legaia_engine_core::scene::{ProtIndex, Scene};
use legaia_engine_core::scene_bundle::{BundleSource, extract_man_payload, find_bundle};

fn extracted_root() -> Option<PathBuf> {
    for p in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(p);
        if d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

/// Load a scene's MAN through the bundle path and parse it. Returns the parsed
/// MAN plus its raw bytes.
fn scene_man(index: &ProtIndex, name: &str) -> (legaia_asset::man_section::ManFile, Vec<u8>) {
    let scene = Scene::load(index, name).unwrap_or_else(|e| panic!("load {name}: {e:#}"));
    let bundle = find_bundle(&scene).unwrap_or_else(|| panic!("{name}: no bundle"));
    let entry_bytes = index
        .entry_bytes_extended(bundle.entry_idx())
        .expect("extended footprint");
    let man_bytes = extract_man_payload(&bundle, &entry_bytes)
        .unwrap_or_else(|e| panic!("{name}: extract MAN: {e:#}"))
        .unwrap_or_else(|| panic!("{name}: bundle carries no MAN"));
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

    // -- dolk2: embedded table at 0x1000, MAN partitions [10, 7, 3] --------
    {
        let scene = Scene::load(&index, "dolk2").expect("load dolk2");
        let bundle = find_bundle(&scene).expect("dolk2 bundle");
        match &bundle {
            BundleSource::V12Embedded {
                table_offset,
                entry,
                ..
            } => {
                assert_eq!(*table_offset, 0x1000, "dolk2 embedded table offset");
                assert_eq!(entry.idx, 76, "dolk2 v12 entry is PROT 76");
            }
            other => panic!("dolk2 expected V12Embedded, got {other:?}"),
        }
        assert_eq!(bundle.table_offset(), 0x1000);

        let (mf, man) = scene_man(&index, "dolk2");
        assert_eq!(man.len(), 2345, "dolk2 MAN decodes to 2345 B");
        assert_eq!(
            mf.header.partition_counts,
            [10, 7, 3],
            "dolk2 MAN partition counts"
        );

        let dests: Vec<String> = scene_destinations(&mf, &man)
            .iter()
            .map(|d| d.scene_name.clone())
            .collect();
        eprintln!("[dolk2] destinations: {dests:?}");
        assert!(
            dests.iter().any(|d| d == "map01"),
            "dolk2 lists its overworld return (map01); got {dests:?}"
        );
    }

    // -- rikuroa: embedded table at 0x1000, MAN partitions [18, 70, 20] ----
    {
        let scene = Scene::load(&index, "rikuroa").expect("load rikuroa");
        let bundle = find_bundle(&scene).expect("rikuroa bundle");
        match &bundle {
            BundleSource::V12Embedded {
                table_offset,
                entry,
                ..
            } => {
                assert_eq!(*table_offset, 0x1000, "rikuroa embedded table offset");
                assert_eq!(entry.idx, 164, "rikuroa v12 entry is PROT 164");
            }
            other => panic!("rikuroa expected V12Embedded, got {other:?}"),
        }

        let (mf, man) = scene_man(&index, "rikuroa");
        assert_eq!(man.len(), 39508, "rikuroa MAN decodes to 39508 B");
        assert_eq!(
            mf.header.partition_counts,
            [18, 70, 20],
            "rikuroa MAN partition counts"
        );
        // The Ravine's exit / first-boss trigger is not a named 0x3F warp;
        // its destination table decodes without error to an empty list.
        let dests = scene_destinations(&mf, &man);
        eprintln!("[rikuroa] {} named 0x3F destination(s)", dests.len());
    }

    eprintln!("[ok] v12-family embedded MAN bundles (rikuroa, dolk2) load");
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
