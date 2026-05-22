//! Decode the real item-name table out of `extracted/SCUS_942.54` if present.
//! Skips and passes when the executable isn't on disk - same gating pattern as
//! the other disc-dependent tests so CI doesn't need Sony bytes.

use legaia_asset::item_names::ItemNameTable;
use std::path::PathBuf;

fn scus_path() -> Option<PathBuf> {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace = manifest.parent()?.parent()?;
    let p = workspace.join("extracted").join("SCUS_942.54");
    p.is_file().then_some(p)
}

#[test]
fn decodes_the_item_name_table_or_skips() {
    let Some(path) = scus_path() else {
        eprintln!("extracted/SCUS_942.54 not present - skipping");
        return;
    };
    let bytes = std::fs::read(&path).expect("read SCUS");
    let table = ItemNameTable::from_scus(&bytes).expect("parse item-name table");

    assert_eq!(table.len(), 256, "item id space");
    // The overwhelming majority of the 256 ids are real items; only a handful
    // of reserved gap slots are empty.
    assert!(
        table.named_count() > 240,
        "expected most ids named, got {}",
        table.named_count()
    );

    // Pinned names spanning the id space: a common consumable drop, a steal
    // accessory, an early weapon, and a late-game armor. These are exactly the
    // ids a monster record's `drop_item` byte resolves through.
    assert_eq!(table.name(0x79), Some("Healing Berry"));
    assert_eq!(table.name(0xF3), Some("Silver Compass"));
    assert_eq!(table.name(0x22), Some("Survival Knife"));
    assert_eq!(table.name(0x47), Some("Master Armor"));

    // id 0 is "no item" (empty slot).
    assert_eq!(table.name(0), None);
}
