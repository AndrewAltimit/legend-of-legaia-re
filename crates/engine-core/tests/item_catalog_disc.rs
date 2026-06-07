//! Disc-gated: the default consumable [`ItemCatalog`] is keyed by **real**
//! retail item ids - every entry's id resolves to its entry name in the
//! `SCUS_942.54` item table ([`legaia_asset::item_names`]).
//!
//! This is the guard against the prior bug where the catalog used fabricated
//! sequential ids (`0x01..`) that collide with the table's internal
//! `Ra-Seru Meta $N` placeholders, so a live granted id (e.g. Healing Leaf
//! `0x77`) never matched a catalog effect. Skips without `LEGAIA_DISC_BIN`.

use legaia_engine_core::Vfs;
use legaia_engine_core::items::ItemCatalog;
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
