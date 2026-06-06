//! Disc-gated: the battle-action per-move power table (`0x801F4F5C`) parses out
//! of the real PROT 0898 (battle-action overlay) entry at the pinned offset.
//!
//! Pins, on real disc bytes, that the table `FUN_801dd0ac` reads for the
//! arts/physical attacker roll lives in the battle-action overlay image and
//! decodes to the power values observed in-RAM (byte-matched against two battle
//! save states). Skips and passes when `LEGAIA_DISC_BIN` / `extracted/` is
//! absent (the workspace disc-gated convention).

use std::path::PathBuf;

use legaia_asset::move_power::{self, BATTLE_ACTION_OVERLAY_PROT_INDEX};
use legaia_prot::archive::Archive;

fn extracted_prot() -> Option<PathBuf> {
    for base in ["extracted", "../../extracted"] {
        let prot = PathBuf::from(base).join("PROT.DAT");
        if prot.is_file() {
            return Some(prot);
        }
    }
    None
}

#[test]
fn move_power_table_parses_with_pinned_powers() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(prot) = extracted_prot() else {
        eprintln!("[skip] extracted/PROT.DAT missing");
        return;
    };
    let mut archive = Archive::open(&prot).expect("open PROT.DAT");
    let entry = archive
        .entries
        .get(BATTLE_ACTION_OVERLAY_PROT_INDEX)
        .cloned()
        .expect("PROT 0898 entry exists");
    let mut bytes = Vec::new();
    archive
        .read_entry(&entry, &mut bytes)
        .expect("read PROT 0898");

    let table = move_power::parse(&bytes).expect("move-power table parses at the pinned offset");
    assert_eq!(table.len(), move_power::MOVE_POWER_TABLE_LEN);

    // Move id 0 is the unused all-zero slot.
    assert!(table[0].is_empty(), "move id 0 should be the unused slot");

    // Pinned powers (`+0` field >> 2), byte-matched against the in-RAM table in
    // two battle save states. The three "special" lead records carry large
    // values (187 / 625 / 1500); the mid-table arts records are small.
    let expect: &[(usize, i32)] = &[(1, 187), (2, 625), (3, 1500), (9, 250), (16, 15), (43, 249)];
    for &(idx, pow) in expect {
        assert_eq!(table[idx].power(), pow, "power for move id {idx}");
        assert!(!table[idx].is_empty(), "move id {idx} should be populated");
    }

    // Every populated record's raw power is consistent with its decoded power.
    for r in &table {
        assert_eq!(r.power(), (r.power_raw as i32) >> 2);
    }

    // The id -> power-index map (0x80 bytes before the table) resolves battle
    // move ids to records (`power_table[map[move_id]]`). Pinned move ids ->
    // index, byte-matched against the in-RAM map across two battle save states.
    let map = move_power::parse_id_index_map(&bytes).expect("id->index map parses");
    let expect_map: &[(u8, u8)] = &[
        (0x04, 1),
        (0x05, 2),
        (0x06, 3),
        (0x07, 4),
        (0x19, 9),
        (0x25, 16),
        (0x46, 16), // a second id sharing record 16
        (0x74, 40),
    ];
    for &(move_id, idx) in expect_map {
        assert_eq!(
            move_power::index_for_move_id(&map, move_id),
            Some(idx),
            "move id {move_id:#04x} -> power index"
        );
    }
    // End-to-end: move id 0x04 -> record 1 -> power 187; 0x06 -> record 3 -> 1500.
    assert_eq!(
        move_power::record_for_move_id(&table, &map, 0x04).map(|r| r.power()),
        Some(187)
    );
    assert_eq!(
        move_power::record_for_move_id(&table, &map, 0x06).map(|r| r.power()),
        Some(1500)
    );
    // Unmapped ids resolve to no record.
    assert_eq!(move_power::index_for_move_id(&map, 0x00), None);
    assert_eq!(move_power::index_for_move_id(&map, 0x10), None);
}
