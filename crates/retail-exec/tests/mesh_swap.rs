//! The Ra-Seru mesh swap `FUN_8001EBEC` against `equipment_swap::apply` over
//! `ACTIVE_PARTY_SLOTS`: each of the three party TMDs the pool holds at
//! `DAT_8007C018[_DAT_8007B824 + slot]` takes the `+0x124` or `+0x140`
//! template over its patched group, by the record byte the slot names.

use crate::common::{Rng, scus_cpu};
use legaia_actor_formats::character_pack::equipment_swap::{ACTIVE_PARTY_SLOTS, apply};
use psx_retail_exec::Stop;

const SWAP: u32 = 0x8001_EBEC;
const POOL: u32 = 0x8007_C018;
const POOL_BASE: u32 = 0x8007_B824;
const RECORDS: u32 = 0x8008_4708;
const STRIDE: u32 = 0x414;
const TMDS: u32 = 0x8014_0000;
const TMD_LEN: usize = 0x160;

#[test]
fn the_mesh_swap_patches_what_retail_patches() {
    differential!(scus(SWAP));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x5357_4150);
    let (mut held, mut bare) = (0, 0);
    for round in 0..3000 {
        let base = rng.below(8);
        cpu.mem.wr32(POOL_BASE, base);
        let mut tmds = Vec::new();
        for slot in 0..3u32 {
            let tmd: Vec<u8> = (0..TMD_LEN).map(|_| rng.next() as u8).collect();
            let at = TMDS + slot * 0x200;
            cpu.mem.load(at, &tmd);
            cpu.mem.wr32(POOL + (base + slot) * 4, at);
            tmds.push(tmd);
        }
        let mut bytes = Vec::new();
        for slot in 0..3u32 {
            let record: Vec<u8> = (0..8)
                .map(|_| {
                    if rng.below(2) == 0 {
                        0
                    } else {
                        rng.next() as u8
                    }
                })
                .collect();
            cpu.mem.load(RECORDS + slot * STRIDE + 0x196, &record);
            bytes.push(record);
        }
        assert_eq!(cpu.call(SWAP, &[]), Stop::Returned);
        for patch in ACTIVE_PARTY_SLOTS {
            let slot = usize::from(patch.slot);
            let byte = bytes[slot][usize::from(patch.equip_byte_record_offset - 0x196)];
            let port = apply(&tmds[slot], patch, byte);
            let retail = cpu.mem.read(TMDS + slot as u32 * 0x200, TMD_LEN);
            assert_eq!(retail, port, "round {round} slot {slot} byte {byte:#x}");
            if byte == 0 {
                bare += 1;
            } else {
                held += 1;
            }
        }
    }
    assert!(held > 2000 && bare > 2000, "{held} {bare}");
}
