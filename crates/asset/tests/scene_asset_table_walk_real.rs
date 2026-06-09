//! Disc-gated corpus test for the runtime-faithful slot-to-asset walk.
//!
//! Pins [`scene_asset_table::resolve`] + [`SceneAssetTable::slots`] /
//! [`SceneAssetTable::payload_range`] against every PROT entry the
//! categorizer classes `scene_asset_table` or `scene_scripted_asset_table`
//! (the ~5 % of the corpus the two detectors fire on).
//!
//! The runtime walk it mirrors is `descriptor_pair_walker` (`FUN_80020224`):
//! `count = *base`, then for slot `i` dispatch
//! `asset_type_dispatch(base + descriptor[i].data_offset, type_size, ...)`,
//! descriptors at `base + 8 + i*8`. This test asserts the static resolver
//! reproduces that walk for both the bare (table at offset 0) and the
//! prescript-prefixed (table at a 0x800-aligned offset) variants:
//!
//!  - `resolve` succeeds on every classified entry.
//!  - The first slot's payload anchors exactly at `table_base + header_end`
//!    (0x40 for count 7, 0x38 for count 6) - the runtime's `piVar1 +
//!    data_offset` invariant for descriptor 0.
//!  - Every slot's type byte is a legal dispatcher type (`< 0x15`).
//!  - Both variants are present (non-vacuous): >= 1 bare and >= 1 scripted.
//!
//! Skips silently when `extracted/PROT.DAT` or `LEGAIA_DISC_BIN` is missing.

use legaia_asset::AssetType;
use legaia_asset::categorize::{Class, classify};
use legaia_asset::scene_asset_table;
use legaia_prot::archive::Archive;
use std::path::PathBuf;

fn extracted_prot_dat() -> Option<PathBuf> {
    [
        PathBuf::from("extracted/PROT.DAT"),
        PathBuf::from("../../extracted/PROT.DAT"),
    ]
    .into_iter()
    .find(|p| p.is_file())
}

/// Header-end for a `count`-descriptor table (= the first descriptor's
/// data_offset): `8 + count*8`.
fn header_end(count: usize) -> u32 {
    (8 + count * 8) as u32
}

#[test]
fn scene_asset_table_walk_reproduces_runtime_dispatch() {
    let Some(prot_dat) = extracted_prot_dat() else {
        eprintln!("[skip] extracted/PROT.DAT missing");
        return;
    };
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }

    let mut archive = Archive::open(&prot_dat).expect("open PROT.DAT");
    let entries = archive.entries.clone();
    let mut buf = Vec::new();

    let mut bare = 0usize;
    let mut scripted = 0usize;
    let mut total_slots = 0usize;

    for entry in &entries {
        // Classify the full footprint - the scripted variant's table can sit
        // in trailing-overlay sectors past the indexed end.
        archive.read_entry(entry, &mut buf).expect("read entry");
        let class = classify(&buf).class;
        let is_table = matches!(
            class,
            Class::SceneAssetTable | Class::SceneScriptedAssetTable
        );
        if !is_table {
            continue;
        }

        let resolved = scene_asset_table::resolve(&buf).unwrap_or_else(|| {
            panic!("entry classed {} but resolve() returned None", class.name())
        });
        let table = &resolved.table;
        let base = resolved.table_base;

        // The bare variant resolves at offset 0; the scripted variant at a
        // 0x800-aligned offset past the event prescript.
        match class {
            Class::SceneAssetTable => {
                assert_eq!(base, 0, "bare table base must be 0");
                bare += 1;
            }
            Class::SceneScriptedAssetTable => {
                assert_ne!(base, 0, "scripted table base must be past offset 0");
                assert_eq!(base % 0x800, 0, "scripted table base is sector-aligned");
                scripted += 1;
            }
            _ => unreachable!(),
        }

        let slots: Vec<_> = table.slots().collect();
        assert!(
            slots.len() == 6 || slots.len() == 7,
            "table has 6 or 7 slots, got {}",
            slots.len()
        );

        // Descriptor 0 anchors at base + header_end - the runtime's
        // `(int)piVar1 + piVar5[3]` for the first slot.
        let first = table.payload_range(0, base).expect("slot 0 range");
        assert_eq!(
            first.start,
            base + header_end(slots.len()) as usize,
            "slot 0 payload must anchor at base + header_end"
        );

        for s in &slots {
            // Every type byte must be a legal dispatcher type (< 0x15).
            assert!(
                !matches!(s.asset_type, AssetType::Unknown(_)),
                "slot {} type 0x{:02X} is not a legal dispatcher type",
                s.slot,
                s.type_byte
            );
            // payload_range must agree with the raw descriptor fields.
            let r = table.payload_range(s.slot, base).expect("slot range");
            assert_eq!(r.start, base + s.data_offset as usize);
            assert_eq!(r.end - r.start, s.size as usize);
            total_slots += 1;
        }
    }

    eprintln!(
        "[scene-asset-walk] {} bare + {} scripted tables, {} slots walked",
        bare, scripted, total_slots
    );

    // Non-vacuous: both variants must appear in the retail corpus.
    assert!(bare > 0, "expected >= 1 bare scene_asset_table entry");
    assert!(
        scripted > 0,
        "expected >= 1 scripted scene_asset_table entry"
    );
}
