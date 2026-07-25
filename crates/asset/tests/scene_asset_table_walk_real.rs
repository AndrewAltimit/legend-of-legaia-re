//! Disc-gated corpus test for the runtime-faithful slot-to-asset walk.
//!
//! Pins [`scene_asset_table::resolve`] + [`SceneAssetTable::slots`] /
//! [`SceneAssetTable::payload_range`] against every PROT entry the
//! categorizer classes `scene_asset_table` or `scene_scripted_asset_table`.
//!
//! The runtime walk it mirrors is `descriptor_pair_walker` (`FUN_80020224`):
//! `count = *base`, then for slot `i` dispatch
//! `asset_type_dispatch(base + descriptor[i].data_offset, type_size, ...)`,
//! descriptors at `base + 8 + i*8`. This test asserts the static resolver
//! reproduces that walk:
//!
//!  - `resolve` succeeds on every classified entry.
//!  - The first slot's payload anchors exactly at `table_base + header_end`
//!    (0x40 for count 7, 0x38 for count 6) - the runtime's `piVar1 +
//!    data_offset` invariant for descriptor 0.
//!  - Every slot's type byte is a legal dispatcher type (`< 0x15`).
//!  - Every table sits at offset **0** of its entry, and every descriptor
//!    payload fits inside that entry.
//!
//! ## The prescript-prefixed variant does not exist
//!
//! `scene_scripted_asset_table` - a table at a 0x800-aligned offset past an
//! event prescript - was an artifact of the old entry size, which ran past the
//! entry into its neighbours. Every such hit sat at a **sector boundary that
//! is the next entry's start LBA**: the "table at +0x800 of entry `p`" is
//! entry `p+1`'s table at offset 0, and "+0x1000" is entry `p+2`'s. Reading
//! each entry's own sectors leaves the same 88 bare tables and no scripted
//! ones, so the assertion below pins the count at zero rather than deleting
//! the class - a detector or reader regression that resurrects the phantom
//! fails here. See `docs/formats/prot.md`.
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
        // The entry's own sectors, and nothing a neighbour owns.
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

        // Every real table is at offset 0 of its own entry.
        match class {
            Class::SceneAssetTable => {
                assert_eq!(base, 0, "bare table base must be 0");
                bare += 1;
            }
            Class::SceneScriptedAssetTable => {
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
            // Every descriptor's payload starts inside the entry that carries
            // the table - the property the old reading broke.
            assert!(
                r.start < buf.len(),
                "PROT {}: slot {} payload starts at {} past the {}-byte entry",
                entry.index,
                s.slot,
                r.start,
                buf.len()
            );
            total_slots += 1;
        }
    }

    eprintln!(
        "[scene-asset-walk] {} bare + {} scripted tables, {} slots walked",
        bare, scripted, total_slots
    );

    // Non-vacuous on the bare side...
    assert!(bare > 0, "expected >= 1 bare scene_asset_table entry");
    // ...and pinned at zero on the other: the prescript-prefixed variant was
    // an over-read phantom (module docs above). A reader or detector change
    // that brings it back lands here.
    assert_eq!(
        scripted, 0,
        "scene_scripted_asset_table matched {scripted} entries - the phantom is back"
    );
}
