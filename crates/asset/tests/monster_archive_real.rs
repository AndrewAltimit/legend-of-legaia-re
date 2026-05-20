//! Disc-gated regression test: decode the monster stat archive (PROT entry
//! `0867_battle_data`) and assert known monster ids decode to the expected
//! name + HP/MP. These values were byte-validated against live battle RAM
//! (Gimard id 10, Killer Bee id 62, Queen Bee id 63 match the actor stats a
//! PCSX-Redux watchpoint captured during the Rim Elm scripted fights).
//!
//! Skips silently when `extracted/PROT/` or `LEGAIA_DISC_BIN` is missing.
//!
//! What this catches:
//! - The `(id-1)*0x14000` slot stride or the `[u32 dec_size][LZS]` slot
//!   framing regresses.
//! - The record byte layout (name offset / HP / MP) drifts.
//! - PROT entry 867 stops being the monster archive (e.g. an extractor
//!   change truncates it).

use legaia_asset::monster_archive;
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

#[test]
fn known_monster_ids_decode_to_expected_records() {
    let Some(entry) = entry_867() else {
        eprintln!("[skip] extracted/PROT/0867_battle_data.BIN or LEGAIA_DISC_BIN missing");
        return;
    };

    // (id, name, hp, mp) - the Rim Elm scripted-battle monster set; HP/MP
    // for Gimard/Killer Bee/Queen Bee are byte-exact vs live battle RAM.
    let expected: &[(u16, &str, u16, u16)] = &[
        (4, "Gobu Gobu", 76, 15),
        (7, "Green Slime", 69, 24),
        (10, "Gimard", 99, 20),
        (61, "Hornet", 188, 88),
        (62, "Killer Bee", 288, 288),
        (63, "Queen Bee", 888, 888),
        (79, "Tetsu", 999, 999),
    ];

    for &(id, name, hp, mp) in expected {
        let rec = monster_archive::record(&entry, id)
            .unwrap_or_else(|e| panic!("id {id}: decode error {e:#}"))
            .unwrap_or_else(|| panic!("id {id}: expected a record, got None"));
        assert_eq!(rec.name, name, "id {id} name");
        assert_eq!(rec.hp, hp, "id {id} HP");
        assert_eq!(rec.mp, mp, "id {id} MP");
    }

    // The archive holds 194 fixed slots; a healthy fraction decode to real
    // records (the rest are filler / unused ids - e.g. index 78 "Comm").
    let all = monster_archive::records(&entry).expect("archive walk");
    assert!(
        all.len() > 100,
        "expected >100 populated monster records, got {}",
        all.len()
    );
}
