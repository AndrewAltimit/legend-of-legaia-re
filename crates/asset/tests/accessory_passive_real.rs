//! Decode the real accessory passive-effect tables out of
//! `extracted/SCUS_942.54` if present. Skips and passes when the executable
//! isn't on disk - same gating pattern as the other disc-dependent tests so
//! CI doesn't need Sony bytes.
//!
//! Pins the descriptor-`+3` passive index per accessory, the `0x8007625C`
//! name/description text, the party-wide scope flags, and the retail
//! equipment `+5` sentinel invariant.

use legaia_asset::accessory_passive::{AccessoryPassiveTable, NO_PASSIVE, PASSIVE_COUNT, index};
use std::path::PathBuf;

fn scus_path() -> Option<PathBuf> {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace = manifest.parent()?.parent()?;
    let p = workspace.join("extracted").join("SCUS_942.54");
    p.is_file().then_some(p)
}

#[test]
fn decodes_the_accessory_passive_table_or_skips() {
    let Some(path) = scus_path() else {
        eprintln!("extracted/SCUS_942.54 not present - skipping");
        return;
    };
    let bytes = std::fs::read(&path).expect("read SCUS");
    let table = AccessoryPassiveTable::from_scus(&bytes).expect("parse accessory-passive table");

    assert_eq!(table.record_count(), PASSIVE_COUNT);

    // Every one of the 64 records resolves both text pointers - the table
    // extent is exactly 0x40 records (the bytes after row 0x3F are unrelated
    // data whose words are not in-segment string pointers).
    for idx in 0..PASSIVE_COUNT as u8 {
        let rec = table.record(idx).unwrap();
        assert!(rec.name.is_some(), "passive {idx:#04x} name");
        assert!(rec.description.is_some(), "passive {idx:#04x} description");
        assert!(rec.scope_raw <= 1, "passive {idx:#04x} scope is 0/1");
    }

    // Pinned (index, name) pairs across the effect families.
    let pin = |id: u8, idx: u8, name: &str| {
        let (got, rec) = table
            .passive(id)
            .unwrap_or_else(|| panic!("item {id:#04x} grants a passive"));
        assert_eq!(got, idx, "item {id:#04x} passive index");
        assert_eq!(rec.name.as_deref(), Some(name), "item {id:#04x} name");
    };
    pin(0xC0, index::HP_BOOST_1, "HP Boost 1"); // Life Ring
    pin(0xC1, index::HP_BOOST_2, "HP Boost 2"); // Life Armband
    pin(0xC2, index::MP_BOOST_1, "MP Boost 1"); // Magic Ring
    pin(0xC3, index::MP_BOOST_2, "MP Boost 2"); // Magic Armband
    pin(0xC4, index::MP_USED_DOWN_1, "MP Used Down 1"); // Spirit Jewel
    pin(0xC5, index::MP_USED_DOWN_2, "MP Used Down 2"); // Spirit Talisman
    pin(0xC6, 0x06, "ATK Boost"); // Power Ring
    pin(0xC9, 0x09, "UDF & LDF Boost"); // Guardian Ring
    pin(0xCC, 0x0C, "AGL Boost"); // Vitality Ring
    pin(0xCD, 0x0D, "Attack x2"); // War God Icon
    pin(0xD0, index::STEAL_ATTACK, "Steal Attack"); // Evil God Icon
    pin(0xD6, 0x16, "Poison Guard 1"); // Cure Amulet
    pin(0xDC, index::MASTER_GUARD, "Master Guard"); // Wonder Amulet
    pin(0xE4, index::ALL_GUARD, "All Guard"); // Rainbow Jewel
    pin(0xE7, 0x27, "Final Heal"); // Lost Grail
    pin(0xEA, 0x2A, "Maximum AP"); // Mettle Goblet
    pin(0xF0, 0x30, "Gold Boost"); // Golden Book
    pin(0xFC, 0x3C, "Low Encounter"); // Good Luck Bell

    // The seven elemental guards are contiguous: Earth..Dark = 0x1D..0x23
    // (Earth/Deep Sea/Burning/Tempest/Madlight/Luminous/Ebony Jewels).
    for (i, id) in (0xDD..=0xE3u8).enumerate() {
        let (idx, _) = table.passive(id).unwrap();
        assert_eq!(
            idx,
            index::ELEMENTAL_GUARD_FIRST + i as u8,
            "jewel {id:#04x}"
        );
    }

    // Quest items share passive indices with their purchasable twins:
    // Mei's Pendant == Life Ring, Minea's Ring == Life Armband, and each
    // elemental Talisman / Egg == the matching Jewel.
    assert_eq!(table.passive_index(0xAC), table.passive_index(0xC0)); // Mei's Pendant
    assert_eq!(table.passive_index(0x6C), table.passive_index(0xC1)); // Minea's Ring
    assert_eq!(table.passive_index(0x72), table.passive_index(0xDD)); // Earth Talisman = Earth Jewel
    assert_eq!(table.passive_index(0x73), table.passive_index(0xDE)); // Water Talisman = Deep Sea Jewel
    assert_eq!(table.passive_index(0x74), table.passive_index(0xE2)); // Light Talisman = Luminous Jewel
    assert_eq!(table.passive_index(0x75), table.passive_index(0xE3)); // Dark Talisman = Ebony Jewel
    assert_eq!(table.passive_index(0x76), table.passive_index(0xFC)); // Evil Talisman = Good Luck Bell

    // Party-wide scope (record +0 == 1): exactly the battle-end / encounter /
    // escape modifiers, indices 0x30..=0x37 and 0x3B..=0x3F.
    for idx in 0..PASSIVE_COUNT as u8 {
        let expect = matches!(idx, 0x30..=0x37 | 0x3B..=0x3F);
        assert_eq!(
            table.record(idx).unwrap().party_wide(),
            expect,
            "passive {idx:#04x} party-wide scope"
        );
    }

    // Retail equipment never grants a passive: every equip-bonus row carries
    // the +5 sentinel, so the kind==1 arm of FUN_80042558 is latent.
    assert!(!table.equip_passive_bytes().is_empty());
    for (row, b) in table.equip_passive_bytes().iter().enumerate() {
        assert_eq!(*b, NO_PASSIVE, "equip row {row:#x} +5 sentinel");
    }

    // Consumables resolve to no passive (descriptor +3 sentinel 0x41).
    for id in [0x77u8, 0x7C, 0x7F, 0x80, 0x8B] {
        assert_eq!(table.passive_index(id), None, "consumable {id:#04x}");
    }
}
