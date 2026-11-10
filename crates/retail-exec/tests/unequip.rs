//! The take-item fallback `FUN_800430AC` against
//! `party_unequip_accessory_by_id`: present-party lists of every length and
//! order over a four-record roster, accessories drawn from a small id pool so
//! the id is worn by several members, worn by an absent one, or by nobody.
//! Every record compares whole, with the return.

use crate::common::{Rng, diff_offsets, scus_cpu};
use legaia_engine_menus::equipment::party_unequip_accessory_by_id;
use legaia_save::Party;
use psx_retail_exec::{Stop, V0};

const UNEQUIP: u32 = 0x8004_30AC;
const COUNT: u32 = 0x8008_4594;
const LIST: u32 = 0x8008_4598;
const RECORDS: u32 = 0x8008_4708;
const STRIDE: usize = 0x414;

#[test]
fn the_unequip_walks_the_party_retail_walks() {
    differential!(scus(UNEQUIP));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x554E_4551);
    let (mut hits, mut misses, mut absent_wearer) = (0, 0, 0);
    for _ in 0..4_000 {
        let mut party = Party::zeroed(4);
        for m in &mut party.members {
            for b in m.raw.iter_mut() {
                *b = rng.next() as u8;
            }
            let mut eq = m.equipment();
            for s in 5..8 {
                eq.slots[s] = [0, 0x90, 0x91, 0x92][rng.below(4) as usize];
            }
            m.set_equipment(eq);
        }
        let mut ids = vec![0u8, 1, 2, 3];
        for i in (1..4).rev() {
            ids.swap(i, rng.below(i as u32 + 1) as usize);
        }
        ids.truncate(rng.below(5) as usize);
        let item = [0u8, 0x90, 0x91, 0x92, 0x93][rng.below(5) as usize];

        for (i, m) in party.members.iter().enumerate() {
            cpu.mem.load(RECORDS + (i * STRIDE) as u32, &m.raw);
        }
        cpu.mem.wr8(COUNT, ids.len() as u8);
        for i in 0..4 {
            cpu.mem
                .wr8(LIST + i, ids.get(i as usize).copied().unwrap_or(0xEE));
        }
        assert_eq!(cpu.call(UNEQUIP, &[u32::from(item)]), Stop::Returned);
        let retail_hit = cpu.regs.r[V0] == 0;

        let worn_by_absent = (0..4u8).any(|i| {
            !ids.contains(&i)
                && party.members[usize::from(i)].equipment().slots[5..8].contains(&item)
        });
        let port_hit = party_unequip_accessory_by_id(&mut party, &ids, item);
        assert_eq!(retail_hit, port_hit, "list {ids:?} item {item:#x}");
        for (i, m) in party.members.iter().enumerate() {
            let retail = cpu.mem.read(RECORDS + (i * STRIDE) as u32, STRIDE);
            assert!(
                retail == m.raw,
                "record {i}, list {ids:?} item {item:#x}: {:?}",
                diff_offsets(&retail, &m.raw)
            );
        }
        if port_hit {
            hits += 1;
        } else {
            misses += 1;
        }
        if worn_by_absent {
            absent_wearer += 1;
        }
    }
    assert!(
        hits > 500 && misses > 500 && absent_wearer > 500,
        "{hits} {misses} {absent_wearer}"
    );
}
