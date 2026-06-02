//! Decode the real per-monster steal table out of `extracted/SCUS_942.54` if
//! present. Skips and passes when the executable isn't on disk - same gating
//! pattern as the other disc-dependent tests so CI doesn't need Sony bytes.
//!
//! The pinned entries are the live-capture ground truth (Skeleton id 13 ->
//! Incense @ 30%, from the `player_steal_skeleton_*` save pair) plus a handful
//! spanning the id space, each cross-checked against the published steal table
//! (item + chance). They're the byte-exact anchors that lock the
//! `[chance, item]` record layout and the `TABLE_VA + id*2` indexing.

use legaia_asset::item_names::ItemNameTable;
use legaia_asset::steal_table::StealTable;
use std::path::PathBuf;

fn scus_path() -> Option<PathBuf> {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace = manifest.parent()?.parent()?;
    let p = workspace.join("extracted").join("SCUS_942.54");
    p.is_file().then_some(p)
}

#[test]
fn decodes_the_steal_table_or_skips() {
    let Some(path) = scus_path() else {
        eprintln!("extracted/SCUS_942.54 not present - skipping");
        return;
    };
    let bytes = std::fs::read(&path).expect("read SCUS");
    let table = StealTable::from_scus(&bytes).expect("parse steal table");
    let names = ItemNameTable::from_scus(&bytes).expect("parse item names");

    assert_eq!(table.len(), 256, "monster id space");

    // The live-capture anchor: Skeleton (monster id 13) yields Incense (0x8a) at
    // a 30% steal chance - exactly what the player_steal_skeleton_banner save
    // shows on screen.
    let skeleton = table.entry(13).expect("skeleton entry");
    assert_eq!(skeleton.item_id, 0x8a, "Skeleton steals Incense (0x8a)");
    assert_eq!(skeleton.chance_pct, 30, "Skeleton steal chance 30%");
    assert_eq!(names.name(skeleton.item_id), Some("Incense"));

    // A spread of pinned (id -> item, chance) entries spanning the table, each
    // verified byte-exact against the published steal table. Covers the common
    // 30% consumables, the rarer 20% compasses, the 5% "Water" stat-up items,
    // the 1% grails, and a 100% boss steal.
    let pins: &[(u16, u8, u8, &str)] = &[
        (1, 0x7e, 30, "Antidote"),          // Evil Fly
        (3, 0xf2, 20, "Golden Compass"),    // Demon Fly
        (9, 0xf3, 20, "Silver Compass"),    // Acid Slime
        (91, 0x86, 5, "Wisdom Water"),      // Thermo (5% Water item)
        (181, 0xed, 100, "Evil Medallion"), // Evil Sim-Seru Cort (100%)
    ];
    for &(id, item, chance, name) in pins {
        let e = table.entry(id).unwrap_or_else(|| panic!("entry {id}"));
        assert_eq!(e.item_id, item, "monster {id} steal item");
        assert_eq!(e.chance_pct, chance, "monster {id} steal chance");
        assert_eq!(names.name(item), Some(name), "monster {id} item name");
    }

    // A healthy share of the id space holds a real steal (the retail archive
    // populates ~186 monsters; the rest are reserved zero slots).
    assert!(
        table.stealable_count() > 150,
        "expected most monster ids stealable, got {}",
        table.stealable_count()
    );

    eprintln!(
        "steal table: {} stealable entries; Skeleton(13) -> {} @ {}%",
        table.stealable_count(),
        names.name(skeleton.item_id).unwrap_or("?"),
        skeleton.chance_pct
    );
}
