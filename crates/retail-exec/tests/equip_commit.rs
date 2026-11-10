//! The equip applier `FUN_801E5A08` (field overlay, PROT 0897; the pause
//! menu's Equip screen and the dev menu's `EQUIP` row both commit through
//! its port) against `commit_equip`: the bag scan, take and refund running
//! for real in `SCUS_942.54` on one side and through `RetailInventory` on the
//! other, over every character, slot argument and class byte. The bag, the
//! character's whole record, the cue and the return compare.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_dev_menu::dev_equip_commit::{EquipCommitHost, commit_equip};
use legaia_save::retail_inventory::RetailInventory;
use psx_retail_exec::{A0, GP, Stop, V0};
use std::cell::RefCell;
use std::rc::Rc;

const OVERLAY_BASE: u32 = 0x801C_E818;
const COMMIT: u32 = 0x801E_5A08;
const CUE: u32 = 0x8003_5BD0;
const BAG: u32 = 0x8008_5958;
const RECORDS: u32 = 0x8008_4708;
const STRIDE: u32 = 0x414;
const ITEMS: u32 = 0x8007_4368;
const EQUIP: u32 = 0x8007_4F68;
const WEAPON_SLOTS: u32 = 0x8007_B42C;

struct Host<'a> {
    bag: &'a mut RetailInventory,
    cues: Vec<u32>,
}

impl EquipCommitHost for Host<'_> {
    fn find_in_bag(&self, item_id: u8) -> u16 {
        self.bag.find_slot(item_id).map_or(0x100, |s| s as u16)
    }
    fn take_from_bag(&mut self, bag_index: u16, qty: u8) {
        self.bag.consume_slot(bag_index as i16, qty, 0);
    }
    fn give_to_bag(&mut self, item_id: u8, qty: u8) {
        self.bag.add(item_id, qty);
    }
    fn play_sfx(&mut self, cue: u8) {
        self.cues.push(u32::from(cue));
    }
}

#[test]
fn the_equip_commit_moves_what_retail_moves() {
    differential!(prot(897, COMMIT));
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem.load(
        OVERLAY_BASE,
        &d.read_entry_footprint(897).expect("PROT 0897"),
    );
    let cues = Rc::new(RefCell::new(Vec::new()));
    let c = cues.clone();
    cpu.hook(CUE, move |r, _| c.borrow_mut().push(r.r[A0]));
    // The whole bag is the window.
    let gp = cpu.regs.r[GP];
    cpu.mem.wr16(gp + 0x2D2, 0);
    cpu.mem.wr16(gp + 0x2D4, 0x100);
    cpu.mem.wr16(gp + 0x2D6, 0x100);
    let weapon_slots: Vec<i16> = (0..4)
        .map(|i| cpu.mem.rd16(WEAPON_SLOTS + 2 * i) as i16)
        .collect();
    let mut rng = Rng(0x4551_4950);
    let (mut committed, mut missed, mut refunds) = (0, 0, 0);
    for case in 0..5000 {
        let ids: Vec<u8> = (0..12).map(|_| 1 + rng.below(255) as u8).collect();
        let mut slots = vec![(0u8, 0u8); 256];
        for s in slots.iter_mut() {
            if rng.below(3) == 0 {
                *s = (ids[rng.below(12) as usize], 1 + rng.below(99) as u8);
            }
        }
        for (i, &(id, n)) in slots.iter().enumerate() {
            cpu.mem.wr8(BAG + 2 * i as u32, id);
            cpu.mem.wr8(BAG + 2 * i as u32 + 1, n);
        }
        let ch = rng.below(4);
        let rec = RECORDS + ch * STRIDE;
        let mut record: Vec<u8> = (0..STRIDE).map(|_| rng.next() as u8).collect();
        for b in &mut record[0x196..0x19E] {
            *b = if rng.below(3) == 0 {
                0
            } else {
                ids[rng.below(12) as usize]
            };
        }
        cpu.mem.load(rec, &record);
        // Now and then an item the bag does not hold: the scan's miss arm.
        let item = if rng.below(4) == 0 {
            loop {
                let id = 1 + rng.below(255) as u8;
                if !ids.contains(&id) {
                    break id;
                }
            }
        } else {
            ids[rng.below(12) as usize]
        };
        let slot_arg = [-1i32, 0, 1, 2, 3, 4, 5, 6][rng.below(8) as usize];
        let sub = u32::from(cpu.mem.rd8(ITEMS + 12 * u32::from(item) + 1));
        let class_byte = cpu.mem.rd8(EQUIP + 8 * sub + 7);
        cues.borrow_mut().clear();
        cpu.regs.r[V0] = 0xDEAD;
        assert_eq!(
            cpu.call(COMMIT, &[u32::from(item), ch, slot_arg as u32]),
            Stop::Returned,
            "case {case}"
        );
        let r = cpu.regs.r[V0];

        let mut bag = RetailInventory::from_slots(BAG, slots);
        let mut host = Host {
            bag: &mut bag,
            cues: Vec::new(),
        };
        let p = commit_equip(
            &mut host,
            &mut record,
            item,
            ch as usize,
            slot_arg,
            class_byte,
            &weapon_slots,
        );
        let port_cues = host.cues;
        let what =
            format!("case {case} item {item:#x} char {ch} slot {slot_arg} class {class_byte:#x}");
        assert_eq!(r, u32::from(p.is_some()), "{what}");
        assert_eq!(*cues.borrow(), port_cues, "{what}");
        let after: Vec<(u8, u8)> = (0..256u32)
            .map(|i| (cpu.mem.rd8(BAG + 2 * i), cpu.mem.rd8(BAG + 2 * i + 1)))
            .collect();
        assert_eq!(after, bag.slots(), "{what}: bag");
        let got = cpu.mem.read(rec, STRIDE as usize);
        assert!(
            got == record,
            "{what}: {:?}",
            crate::common::diff_offsets(&got, &record)
        );
        match p {
            Some(c) => {
                committed += 1;
                refunds += u32::from(c.refunded.is_some());
            }
            None => missed += 1,
        }
    }
    assert!(
        committed > 1000 && missed > 300 && refunds > 500,
        "{committed} {missed} {refunds}"
    );
}
