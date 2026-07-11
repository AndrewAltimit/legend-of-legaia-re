//! Disc-gated: the menu-overlay item-category / weapon-favor table
//! (PROT 0899 @ VA `0x801E4B88`) parses off the real disc and holds its
//! structural invariants against the `SCUS_942.54` equipment tables:
//! every key is an equippable **weapon** id, favor nibbles stay within
//! the three character bits, and every character has favored weapons in
//! both favor groups. Non-weapon equipment never resolves an entry, so
//! the ported check ([`legaia_engine_core::menu_item_category`],
//! `FUN_801DD0C0` - see the `// PORT:` tag in the module) scores it 0.
//! No Sony bytes are asserted, only structural facts. Skips + passes
//! when `LEGAIA_DISC_BIN` is absent.

use legaia_asset::equip_stats::{EquipSlot, EquipStatTable};
use legaia_engine_core::Vfs;
use legaia_engine_core::menu_item_category::{
    CATEGORY_MATCH_SCORE, CategoryEntry, category_check, parse_category_table,
};
use legaia_engine_core::scene::SceneHost;
use std::path::PathBuf;

fn from_disc() -> Option<(Vec<CategoryEntry>, EquipStatTable)> {
    let path = std::env::var_os("LEGAIA_DISC_BIN").map(PathBuf::from)?;
    if !path.is_file() {
        return None;
    }
    let host = SceneHost::open_disc(&path).expect("open disc");
    // The table sits past the entry's TOC size, like the window table -
    // read the extended footprint.
    let overlay = host
        .index
        .entry_bytes_extended(legaia_asset::menu_windows::MENU_OVERLAY_PROT_INDEX as u32)
        .expect("read PROT 0899 (extended)");
    let table = parse_category_table(&overlay).expect("category table parses");

    let scus = legaia_engine_core::DiscVfs::open(&path)
        .expect("open disc vfs")
        .read("SCUS_942.54")
        .expect("SCUS_942.54 present");
    let equip = EquipStatTable::from_scus(&scus).expect("equip stat table parses");
    Some((table, equip))
}

#[test]
fn disc_category_table_keys_weapons_and_scores_favor_bits() {
    let Some((table, equip)) = from_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or not a file");
        return;
    };

    assert!(!table.is_empty(), "retail category table is non-empty");

    // Every key is an equippable weapon; favor nibbles stay within the
    // three character bits.
    for e in &table {
        assert!(
            equip.is_equipment(e.item_id),
            "category key {:#04x} is not an equippable item",
            e.item_id
        );
        let bonus = equip
            .bonus(e.item_id)
            .expect("equippable key resolves a bonus record");
        assert_eq!(
            bonus.slot(),
            EquipSlot::Weapon,
            "category key {:#04x} is not a weapon",
            e.item_id
        );
        assert_eq!(e.nibble(0) & !0x7, 0, "group-0 nibble within char bits");
        assert_eq!(e.nibble(1) & !0x7, 0, "group-1 nibble within char bits");
    }

    // Keys are unique - the first-match-wins walk sees every entry.
    let mut ids: Vec<u8> = table.iter().map(|e| e.item_id).collect();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), table.len(), "category keys are unique");

    // Every character has favored weapons in both favor groups (the
    // Best-Equipment chooser calls with group 1).
    for char_index in 0..3u32 {
        for group in 0..2u32 {
            assert!(
                table
                    .iter()
                    .any(|e| category_check(&table, char_index, e.item_id, group)
                        == CATEGORY_MATCH_SCORE),
                "char {char_index} has a favored weapon in group {group}"
            );
        }
    }

    // Non-weapon equipment never keys an entry, so the check scores 0
    // for it (any character, either group).
    let mut saw_non_weapon = false;
    for id in 0..=255u8 {
        let Some(bonus) = equip.bonus(id) else {
            continue;
        };
        if bonus.slot() == EquipSlot::Weapon {
            continue;
        }
        saw_non_weapon = true;
        for char_index in 0..3u32 {
            for group in 0..2u32 {
                assert_eq!(
                    category_check(&table, char_index, id, group),
                    0,
                    "non-weapon {id:#04x} must not score a favor bonus"
                );
            }
        }
    }
    assert!(saw_non_weapon, "equip table exposes non-weapon equipment");
}
