//! Disc-gated validation of the monster-drop re-pack primitive against the real
//! `battle_data` archive (PROT entry 867).
//!
//! For a sample of real monster slots, edit the drop fields, re-pack the LZS
//! slot, and confirm: (1) the slot stays exactly `0x14000` bytes, (2) the
//! re-parsed record shows the new drop, and (3) every other field (gold, exp,
//! hp, mp, stats, name, spells) is byte-identical - i.e. the edit is surgical.
//!
//! Reads the per-entry file the extractor writes under `extracted/PROT/`; skips
//! and passes when it is absent.

use legaia_asset::monster_archive::{self, SLOT_STRIDE};

fn entry_867() -> Option<Vec<u8>> {
    for p in ["extracted/PROT", "../../extracted/PROT"] {
        let f = std::path::Path::new(p).join("0867_battle_data.BIN");
        if f.is_file() {
            return std::fs::read(f).ok();
        }
    }
    None
}

fn slot_for(entry: &[u8], id: u16) -> &[u8] {
    let start = (id as usize - 1) * SLOT_STRIDE;
    let end = (start + SLOT_STRIDE).min(entry.len());
    &entry[start..end]
}

#[test]
fn set_drop_is_surgical_on_real_records() {
    let Some(entry) = entry_867() else {
        eprintln!("[skip] extracted/PROT/0867_battle_data.BIN missing");
        return;
    };

    let slots = entry.len() / SLOT_STRIDE;
    let mut tested = 0usize;
    for id in 1..=slots as u16 {
        let Ok(Some(orig)) = monster_archive::record(&entry, id) else {
            continue; // filler / non-record slot
        };

        // New drop values guaranteed to differ from the originals.
        let new_item = orig.drop_item.wrapping_add(1);
        let new_chance = (orig.drop_chance_pct % 100) + 1; // 1..=100

        let patched = legaia_patcher::monster::set_drop(slot_for(&entry, id), new_item, new_chance)
            .expect("re-pack slot");
        assert_eq!(patched.len(), SLOT_STRIDE, "slot id {id} changed size");

        // Re-parse the patched slot as a standalone single-slot archive.
        let reparsed = monster_archive::record(&patched, 1)
            .expect("parse patched")
            .expect("patched record present");

        assert_eq!(
            reparsed.drop_item, new_item,
            "id {id}: drop item not applied"
        );
        assert_eq!(
            reparsed.drop_chance_pct, new_chance,
            "id {id}: drop chance not applied"
        );
        // Everything else is untouched.
        assert_eq!(reparsed.gold, orig.gold, "id {id}: gold changed");
        assert_eq!(reparsed.exp, orig.exp, "id {id}: exp changed");
        assert_eq!(reparsed.hp, orig.hp, "id {id}: hp changed");
        assert_eq!(reparsed.mp, orig.mp, "id {id}: mp changed");
        assert_eq!(reparsed.stats, orig.stats, "id {id}: stats changed");
        assert_eq!(reparsed.name, orig.name, "id {id}: name changed");
        assert_eq!(
            reparsed.magic_count, orig.magic_count,
            "id {id}: spells changed"
        );

        tested += 1;
        if tested >= 40 {
            break;
        }
    }
    assert!(
        tested > 20,
        "expected to exercise many real records, got {tested}"
    );
    eprintln!("monster-drop re-pack: {tested} real records edited surgically");
}
