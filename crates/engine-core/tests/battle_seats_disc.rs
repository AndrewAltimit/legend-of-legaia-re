//! Disc-gated: the `battle_seats` mirror byte-matches the SCUS placement
//! tables.
//!
//! `FUN_800513F0` seats the party from the 8-byte-entry table at
//! `0x800775C8` (rows by party count, stride `0x18`) and the monsters from
//! `0x80077608` (rows by monster count `+ 4` for the alternate family,
//! stride `0x20`). This oracle re-reads both tables off the user's
//! executable and asserts the committed engine mirror matches entry for
//! entry. Skips and passes when `LEGAIA_DISC_BIN` / `extracted/` is absent
//! (the workspace disc-gated convention).

use std::path::PathBuf;

use legaia_engine_core::battle_seats::{MONSTER_SEATS, MONSTER_SEATS_ALT, PARTY_SEATS, Seat};

const SCUS_LOAD_VA: u32 = 0x8001_0000;
const SCUS_FILE_HEADER: u32 = 0x800;
const PARTY_TABLE_VA: u32 = 0x8007_75C8;
const MONSTER_TABLE_VA: u32 = 0x8007_7608;

fn extracted_scus() -> Option<PathBuf> {
    for base in ["extracted", "../../extracted"] {
        let scus = PathBuf::from(base).join("SCUS_942.54");
        if scus.is_file() {
            return Some(scus);
        }
    }
    None
}

fn seat_at(exe: &[u8], va: u32) -> Seat {
    let off = (va - SCUS_LOAD_VA + SCUS_FILE_HEADER) as usize;
    let i16_at = |o: usize| i16::from_le_bytes([exe[o], exe[o + 1]]);
    Seat {
        x: i16_at(off),
        y: i16_at(off + 2),
        z: i16_at(off + 4),
    }
}

#[test]
fn battle_seat_tables_match_the_executable() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(scus) = extracted_scus() else {
        eprintln!("[skip] extracted/SCUS_942.54 missing");
        return;
    };
    let exe = std::fs::read(&scus).expect("read SCUS_942.54");

    // Party rows: index = party_count * 0x18 (1-based), 3 slots x 8 bytes.
    for (count0, row) in PARTY_SEATS.iter().enumerate() {
        let row_va = PARTY_TABLE_VA + ((count0 as u32 + 1) * 0x18);
        for (slot, expect) in row.iter().enumerate() {
            let got = seat_at(&exe, row_va + slot as u32 * 8);
            assert_eq!(got, *expect, "party count {} slot {slot}", count0 + 1);
        }
    }
    // Monster rows: normal family rows 1..=4, alternate rows 5..=8.
    for (family, table) in [(0u32, &MONSTER_SEATS), (4, &MONSTER_SEATS_ALT)] {
        for (count0, row) in table.iter().enumerate() {
            let row_va = MONSTER_TABLE_VA + ((count0 as u32 + 1 + family) * 0x20);
            for (slot, expect) in row.iter().enumerate() {
                let got = seat_at(&exe, row_va + slot as u32 * 8);
                assert_eq!(
                    got,
                    *expect,
                    "monster count {} family +{family} slot {slot}",
                    count0 + 1
                );
            }
        }
    }
    eprintln!("[ok] party + monster seat tables byte-match the executable");
}
