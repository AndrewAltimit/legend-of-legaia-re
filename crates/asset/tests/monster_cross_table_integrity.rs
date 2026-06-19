//! Disc-gated cross-table integrity sweep over the whole monster roster.
//!
//! Four independent `SCUS_942.54` / PROT parsers describe the same id spaces:
//! the monster archive (PROT 867) carries each monster's **drop item** id, its
//! **global magic-attack** ids, and (via the steal table) a **steal item** id;
//! those ids index the item-name table and the spell-name table. This test
//! decodes the full roster and asserts every non-zero id resolves to a real
//! named entry - so a layout drift in ANY of the four parsers (a shifted record
//! field, a mis-strided table) surfaces as an out-of-range id here, not as a
//! silently wrong name in-game.
//!
//! Finding it pins: across all populated monsters, every drop / magic-attack /
//! steal id resolves cleanly (zero dangling references); the only "missing"
//! drops are the explicit `0` = no-drop sentinel.
//!
//! Skips silently when `extracted/` or `LEGAIA_DISC_BIN` is missing.

use legaia_asset::{
    item_names::ItemNameTable, monster_archive, spell_names::SpellNameTable,
    steal_table::StealTable,
};
use std::path::PathBuf;

fn entry_867() -> Option<Vec<u8>> {
    std::env::var_os("LEGAIA_DISC_BIN")?;
    for p in ["extracted/PROT", "../../extracted/PROT"] {
        let f = PathBuf::from(p).join("0867_battle_data.BIN");
        if f.is_file() {
            return std::fs::read(f).ok();
        }
    }
    None
}

fn read_scus() -> Option<Vec<u8>> {
    for base in ["extracted", "../../extracted"] {
        let p = PathBuf::from(base).join("SCUS_942.54");
        if let Ok(b) = std::fs::read(&p) {
            return Some(b);
        }
    }
    None
}

#[test]
fn every_monster_drop_magic_and_steal_id_resolves() {
    let (Some(entry), Some(scus)) = (entry_867(), read_scus()) else {
        eprintln!("[skip] extracted/ (PROT/0867 + SCUS_942.54) or LEGAIA_DISC_BIN missing");
        return;
    };

    let items = ItemNameTable::from_scus(&scus).expect("parse item-name table");
    let spells = SpellNameTable::from_scus(&scus).expect("parse spell-name table");
    let steal = StealTable::from_scus(&scus).expect("parse steal table");
    let records = monster_archive::records(&entry).expect("decode monster archive");

    let named = |t: &ItemNameTable, id: u8| t.name(id).is_some_and(|s| !s.is_empty());

    let mut monsters = 0usize;
    let mut no_drop = 0usize;
    let mut magic_checked = 0usize;
    let mut steal_checked = 0usize;
    let mut dangling: Vec<String> = Vec::new();

    for r in &records {
        monsters += 1;

        // Drop item: 0 = no drop (an explicit sentinel), else must be a named item.
        if r.drop_item == 0 {
            no_drop += 1;
        } else if !named(&items, r.drop_item) {
            dangling.push(format!(
                "monster {} ({}) drop_item 0x{:02X} has no item name",
                r.id, r.name, r.drop_item
            ));
        }

        // Global magic-attack ids must resolve in the spell table.
        for &m in &r.magic_attacks {
            if m == 0 {
                continue;
            }
            magic_checked += 1;
            if spells.name(m).is_none_or(|s| s.is_empty()) {
                dangling.push(format!(
                    "monster {} ({}) magic id 0x{:02X} has no spell name",
                    r.id, r.name, m
                ));
            }
        }

        // Steal item (the static steal table, keyed by 1-based monster id).
        if let Some(it) = steal.steal_item(r.id)
            && it != 0
        {
            steal_checked += 1;
            if !named(&items, it) {
                dangling.push(format!(
                    "monster {} ({}) steal item 0x{:02X} has no item name",
                    r.id, r.name, it
                ));
            }
        }
    }

    eprintln!(
        "[xtable] monsters={monsters} no_drop={no_drop} magic_ids_checked={magic_checked} \
         steal_ids_checked={steal_checked} dangling={}",
        dangling.len()
    );

    assert!(
        monsters > 150,
        "expected the full monster roster, got {monsters}"
    );
    // Non-vacuous: the sweep actually exercised cross-table references.
    assert!(magic_checked > 100, "too few magic ids checked");
    assert!(steal_checked > 50, "too few steal ids checked");
    assert!(
        dangling.is_empty(),
        "dangling cross-table references (parser drift?):\n{}",
        dangling.join("\n")
    );
}
