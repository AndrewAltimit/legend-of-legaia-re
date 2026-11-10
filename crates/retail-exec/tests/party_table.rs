//! The field overlay's party-table rebuild `FUN_801DE190` (PROT 0897), with
//! the system flag test `FUN_8003CE64` live beneath it over a random flag
//! bank, against `rebuild_battle_party_table` fed the `World`'s own flag
//! read. Every present count `0..=255` (four and up read as three), random
//! ids and a stale table: the four table bytes at `DAT_8007BD10` have to
//! match.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_core::world::{World, rebuild_battle_party_table};
use psx_retail_exec::Stop;

const REBUILD: u32 = 0x801D_E190;
const OVERLAY_BASE: u32 = 0x801C_E818;
const FLAG_BANK: u32 = 0x8008_5758;
const FLAG_BANK_BYTES: usize = 0x200;
const PRESENT_COUNT: u32 = 0x8008_4594;
const PRESENT_LIST: u32 = 0x8008_4598;
const TABLE: u32 = 0x8007_BD10;

#[test]
fn the_party_table_rebuild_matches_retail() {
    differential!(prot(897, REBUILD));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem.load(
        OVERLAY_BASE,
        &disc().unwrap().read_entry(897).expect("PROT 0897"),
    );
    let mut rng = Rng(0x5041_5254);
    let mut w = World::new();
    let (mut companions, mut capped, mut empty) = (0, 0, 0);
    for run in 0..20_000 {
        let bank: Vec<u8> = (0..FLAG_BANK_BYTES).map(|_| rng.next() as u8).collect();
        cpu.mem.load(FLAG_BANK, &bank);
        w.flags.system_flags = bank;
        let count = [0u8, 1, 2, 3, 4, rng.next() as u8][rng.below(6) as usize];
        let list: [u8; 4] = std::array::from_fn(|_| {
            if rng.below(4) == 0 {
                rng.next() as u8
            } else {
                rng.below(4) as u8
            }
        });
        let stale: [u8; 4] = std::array::from_fn(|_| rng.next() as u8);
        cpu.mem.wr8(PRESENT_COUNT, count);
        cpu.mem.load(PRESENT_LIST, &list);
        cpu.mem.load(TABLE, &stale);
        assert_eq!(cpu.call(REBUILD, &[]), Stop::Returned, "run {run}");
        let companion = w.system_flag_test(0x13);
        let mut port = stale;
        rebuild_battle_party_table(count, &list, companion, &mut port);
        assert_eq!(
            cpu.mem.read(TABLE, 4),
            port,
            "run {run}: count {count} list {list:?} companion {companion}"
        );
        companions += u32::from(companion);
        capped += u32::from(count >= 3 && companion);
        empty += u32::from(count == 0);
    }
    assert!(
        companions > 5000 && capped > 1000 && empty > 1000,
        "{companions} {capped} {empty}"
    );
}
