//! `FUN_80042558`, the stat aggregator, against the record-level port: the
//! ability words, the summon grant and revoke scans, the live window.
//!
//! Records are seeded off a generator with the shapes a game reaches (worn
//! Talismans and Ra-Seru forms, spells on the list, parked stashes, boosts at
//! the caps) and walked through the routine in RAM. The whole `0x414`-byte
//! record has to come back byte-equal, so a write the port does not know about
//! shows up as much as a wrong sum.

use crate::common::{Rng, diff_offsets, scus, scus_cpu};
use legaia_engine_battle::accessory_passives::AccessoryPassives;
use legaia_game_tables::accessory_passive::AccessoryPassiveTable;
use legaia_save::CharacterRecord;
use psx_retail_exec::{Cpu, Stop};

const AGGREGATOR: u32 = 0x8004_2558;
const RECORDS: u32 = 0x8008_4708;
const STRIDE: u32 = 0x414;
const MASK: u32 = 0x8007_4358;

/// Items a record plausibly wears: the grant list, passive accessories, gear.
fn item(rng: &mut Rng) -> u8 {
    match rng.below(5) {
        0 => [0x72u8, 0x73, 0x74, 0x75, 0x76, 0x09, 0x11, 0x19][rng.below(8) as usize],
        1 => 0,
        _ => rng.below(256) as u8,
    }
}

fn seed_record(rng: &mut Rng, r: &mut [u8]) {
    for b in r.iter_mut() {
        *b = rng.below(256) as u8;
    }
    let spells = rng.below(10) as usize;
    r[0x13C] = spells as u8;
    for i in 0..spells {
        r[0x13D + i] = match rng.below(3) {
            0 => 0x99 + rng.below(8) as u8,
            _ => 0x81 + rng.below(0x18) as u8,
        };
    }
    for slot in 0..8 {
        r[0x196 + slot] = item(rng);
    }
    let base: [(usize, u32); 9] = [
        (0x11C, 12000),
        (0x11E, 1300),
        (0x120, 130),
        (0x122, 400),
        (0x124, 1300),
        (0x126, 1300),
        (0x128, 1300),
        (0x12A, 1300),
        (0x12C, 1300),
    ];
    for (off, top) in base {
        let v = rng.below(top) as u16;
        r[off..off + 2].copy_from_slice(&v.to_le_bytes());
    }
    for off in [0x106usize, 0x10A, 0x10E] {
        let v = rng.below(12000) as u16;
        r[off..off + 2].copy_from_slice(&v.to_le_bytes());
    }
}

fn run_retail(cpu: &mut Cpu, records: &[Vec<u8>; 3]) -> ([Vec<u8>; 3], [u32; 4]) {
    for (i, r) in records.iter().enumerate() {
        cpu.mem.load(RECORDS + i as u32 * STRIDE, r);
    }
    assert_eq!(cpu.call(AGGREGATOR, &[]), Stop::Returned);
    let out = std::array::from_fn(|i| cpu.mem.read(RECORDS + i as u32 * STRIDE, STRIDE as usize));
    let mask = std::array::from_fn(|i| cpu.mem.rd32(MASK + i as u32 * 4));
    (out, mask)
}

/// What the port does to one record, in the aggregator's order.
fn run_port(raw: &[u8], passives: &AccessoryPassives) -> (Vec<u8>, [u32; 4]) {
    let mut rec = CharacterRecord::parse(raw).unwrap();
    let words = passives.bits_for_equipment(&rec.equipment().slots);
    let mut bytes = [0u8; legaia_save::ABILITY_BITS_LEN];
    for (w, word) in words.iter().enumerate() {
        bytes[w * 4..w * 4 + 4].copy_from_slice(&word.to_le_bytes());
    }
    rec.set_ability_bits(bytes);
    rec.sync_equipment_summons();
    rec.rebuild_live_window();
    (rec.write(), words)
}

#[test]
fn the_port_rebuilds_a_record_byte_for_byte() {
    differential!(scus(AGGREGATOR));
    let (Some(mut cpu), Some(scus)) = (scus_cpu(), scus()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let table = AccessoryPassiveTable::from_scus(&scus).expect("passive table");
    let passives = AccessoryPassives::from_disc(&table);
    let mut rng = Rng(0x4C45_4741);
    let (mut grants, mut revokes) = (0, 0);
    for round in 0..2000 {
        let records: [Vec<u8>; 3] = std::array::from_fn(|_| {
            let mut r = vec![0u8; STRIDE as usize];
            seed_record(&mut rng, &mut r);
            r
        });
        let (retail, mask) = run_retail(&mut cpu, &records);
        grants += cpu.calls.iter().filter(|c| **c == 0x8004_32BC).count();
        revokes += cpu.calls.iter().filter(|c| **c == 0x8004_2DBC).count();
        let mut global = [0u32; 4];
        for (i, seeded) in records.iter().enumerate() {
            let (port, words) = run_port(seeded, &passives);
            let d = diff_offsets(&retail[i], &port);
            assert!(
                d.is_empty(),
                "round {round} record {i} equip {:02x?}: {d:?}",
                &seeded[0x196..0x19E]
            );
            for (g, w) in global.iter_mut().zip(words) {
                *g |= w;
            }
        }
        assert_eq!(mask, global, "round {round}: the global ability mask");
    }
    assert!(
        grants > 1000 && revokes > 500,
        "{grants} grants, {revokes} revokes"
    );
}
