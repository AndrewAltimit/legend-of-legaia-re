//! Disc-gated: the default consumable [`ItemCatalog`] is keyed by **real**
//! retail item ids - every entry's id resolves to its entry name in the
//! `SCUS_942.54` item table ([`legaia_asset::item_names`]).
//!
//! This is the guard against the prior bug where the catalog used fabricated
//! sequential ids (`0x01..`) that collide with the table's internal
//! `Ra-Seru Meta $N` placeholders, so a live granted id (e.g. Healing Leaf
//! `0x77`) never matched a catalog effect. Skips without `LEGAIA_DISC_BIN`.

use legaia_asset::item_effect::{ItemEffectTable, RestoreAmount};
use legaia_engine_core::Vfs;
use legaia_engine_core::items::{ItemCatalog, ItemEffect};
use std::path::PathBuf;

#[test]
fn vanilla_catalog_ids_match_the_real_item_table() {
    let Some(path) = std::env::var_os("LEGAIA_DISC_BIN").map(PathBuf::from) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    if !path.is_file() {
        eprintln!("[skip] LEGAIA_DISC_BIN is not a file");
        return;
    }
    let scus = legaia_engine_core::DiscVfs::open(&path)
        .expect("open disc")
        .read("SCUS_942.54")
        .expect("SCUS_942.54 present");
    let names =
        legaia_asset::item_names::ItemNameTable::from_scus(&scus).expect("item table parses");

    let catalog = ItemCatalog::vanilla();
    assert!(catalog.len() >= 10, "catalog is populated");
    for entry in catalog.iter() {
        let real = names
            .name(entry.id)
            .unwrap_or_else(|| panic!("catalog id {:#04x} names no real item", entry.id));
        assert_eq!(
            real, entry.name,
            "catalog id {:#04x} should be {:?}, the item table says {:?}",
            entry.id, entry.name, real
        );
    }

    // Anchor: the real Healing Leaf is id 0x77 (not the old fabricated 0x01).
    assert_eq!(names.name(0x77), Some("Healing Leaf"));
    assert!(
        catalog.get(0x77).is_some(),
        "the catalog keys Healing Leaf at its real id 0x77"
    );
    assert!(
        catalog.get(0x01).is_none(),
        "0x01 (Ra-Seru Meta $1 placeholder) is not a consumable in the catalog"
    );
}

/// The curated HP/MP restore amounts are byte-confirmed against the on-disc
/// heal-amount table (`0x8007655C`) the apply handler `FUN_800402F4` reads -
/// so the engine's numbers are disc-faithful, not just walkthrough-sourced.
/// Skips without `LEGAIA_DISC_BIN`.
#[test]
fn curated_heal_amounts_match_the_disc_heal_table() {
    let Some(path) = std::env::var_os("LEGAIA_DISC_BIN").map(PathBuf::from) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    if !path.is_file() {
        eprintln!("[skip] LEGAIA_DISC_BIN is not a file");
        return;
    }
    let scus = legaia_engine_core::DiscVfs::open(&path)
        .expect("open disc")
        .read("SCUS_942.54")
        .expect("SCUS_942.54 present");
    let table = ItemEffectTable::from_scus(&scus).expect("item-effect table parses");

    let catalog = ItemCatalog::vanilla();
    // For every flat-heal catalog entry, the curated amount must equal what the
    // disc apply handler would restore for that item id.
    let mut checked = 0;
    for entry in catalog.iter() {
        match (entry.effect, table.restore_amount(entry.id)) {
            (ItemEffect::Heal { amount }, Some(RestoreAmount::Hp(disc))) => {
                assert_eq!(
                    amount, disc,
                    "{:#04x} {:?}: curated HP heal {} != disc {}",
                    entry.id, entry.name, amount, disc
                );
                checked += 1;
            }
            (ItemEffect::HealMp { amount }, Some(RestoreAmount::Mp(disc))) => {
                assert_eq!(
                    amount, disc,
                    "{:#04x} {:?}: curated MP heal {} != disc {}",
                    entry.id, entry.name, amount, disc
                );
                checked += 1;
            }
            // HealAll is the disc's tier-2 full restore (9999); other effect
            // shapes (cure/revive/buff/spirit/escape) aren't flat amounts.
            (ItemEffect::HealAll, Some(RestoreAmount::Hp(disc))) => {
                assert_eq!(
                    disc, 9999,
                    "{:#04x} HealAll should be the 9999 tier",
                    entry.id
                );
                checked += 1;
            }
            _ => {}
        }
    }
    assert!(checked >= 5, "cross-checked too few heal items ({checked})");

    // Spot anchors keyed by real id.
    assert_eq!(table.restore_amount(0x77), Some(RestoreAmount::Hp(200))); // Healing Leaf
    assert_eq!(table.restore_amount(0x7C), Some(RestoreAmount::Mp(50))); // Magic Leaf
}
