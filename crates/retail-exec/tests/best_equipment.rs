//! The Best Equipment row: the candidate scan `FUN_801CF88C` and the applier
//! `FUN_801CF760` against `EquipSession::best_equipment_now` and
//! `slot_browse_confirm`, over the real tables and bags with holes.

use crate::common::{Rng, disc, scus, scus_cpu};
use legaia_engine_menus::equip_session::{
    EquipSession, SLOT_BROWSE_BEST_ROW, engine_equip_from_record,
};
use legaia_engine_menus::equipment::{DiscEquipInfo, equip_modifier_table_from_disc};
use legaia_engine_menus::item_bag::ItemBag;
use legaia_engine_menus::menu_item_category::parse_category_table;
use legaia_game_tables::equip_stats::EquipStatTable;
use psx_retail_exec::{Stop, V0};

const OVERLAY_BASE: u32 = 0x801C_E818;
const SCAN: u32 = 0x801C_F88C;
const APPLY: u32 = 0x801C_F760;
const AGGREGATE: u32 = 0x801C_F650;
const CANDIDATES: u32 = 0x801E_F0C0;
const RECORDS: u32 = 0x8008_4708;
const STRIDE: u32 = 0x414;
const EQUIP: u32 = 0x196;
const BAG: u32 = 0x8008_5958;
const WINDOW_LO: u32 = 0x8007_B5EA;
const WINDOW_HI: u32 = 0x8007_B5EC;

#[test]
fn the_best_equipment_row_picks_and_applies_what_retail_does() {
    differential!(prot(899, APPLY), prot(899, SCAN));
    let (Some(mut cpu), Some(disc), Some(scus)) = (scus_cpu(), disc(), scus()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let overlay = disc.read_entry(899).expect("PROT 0899");
    cpu.mem.load(OVERLAY_BASE, &overlay);
    cpu.stub(AGGREGATE, 0);
    let stats = EquipStatTable::from_scus(&scus).expect("equipment");
    let table = equip_modifier_table_from_disc(&stats);
    let info = DiscEquipInfo::from_disc(&stats);
    let category = parse_category_table(&overlay).expect("category");
    let gear: Vec<u8> = (1..=255u8).filter(|id| table.get(*id).is_some()).collect();

    let mut rng = Rng(0x4245_5354);
    let mut changed = 0;
    for round in 0..20000 {
        let ch = rng.below(3);
        // The worn bytes, retail's own order; only an armament byte is read.
        let mut worn = [0u8; 8];
        for b in worn.iter_mut().take(5) {
            if rng.below(3) != 0 {
                *b = gear[rng.below(gear.len() as u32) as usize];
            }
        }
        // A bag with holes, stacks and some items that are not equipment.
        let mut slots = vec![(0u8, 0u8); 256];
        for _ in 0..(1 + rng.below(30)) {
            let at = rng.below(64) as usize;
            let id = if rng.below(4) == 0 {
                1 + rng.below(255) as u8
            } else {
                gear[rng.below(gear.len() as u32) as usize]
            };
            slots[at] = (id, 1 + rng.below(3) as u8);
        }
        // An id stands in one slot, as normalize leaves the bag.
        for i in 0..slots.len() {
            if slots[i].0 != 0 && slots[..i].iter().any(|s| s.0 == slots[i].0) {
                slots[i] = (0, 0);
            }
        }
        for (i, (id, n)) in slots.iter().enumerate() {
            cpu.mem.wr8(BAG + i as u32 * 2, *id);
            cpu.mem.wr8(BAG + i as u32 * 2 + 1, *n);
        }
        cpu.mem.wr16(WINDOW_LO, 0);
        cpu.mem.wr16(WINDOW_HI, 0x100);
        let rec = RECORDS + ch * STRIDE;
        for (i, b) in worn.iter().enumerate() {
            cpu.mem.wr8(rec + EQUIP + i as u32, *b);
        }

        assert_eq!(cpu.call(SCAN, &[ch]), Stop::Returned, "{:x?}", cpu.calls);
        let picked: Vec<u8> = (0..4).map(|i| cpu.mem.rd8(CANDIDATES + i)).collect();
        // The scan restores the worn bytes.
        for (i, b) in worn.iter().enumerate() {
            assert_eq!(cpu.mem.rd8(rec + EQUIP + i as u32), *b, "restored");
        }

        let record = legaia_engine_battle::battle_stats::StatRecord {
            equip: engine_equip_from_record(worn, ch as u8),
            ..Default::default()
        };
        let mut session = EquipSession::new(
            record,
            ItemBag::from_slots(&slots),
            table.clone(),
            Default::default(),
            Vec::new(),
        )
        .with_weapon_category(category.clone())
        .with_restrictions(info.clone(), ch as u8);
        let port = session.best_equipment_now();
        assert_eq!(
            picked.as_slice(),
            port.as_slice(),
            "round {round}: char {ch} worn {worn:02x?} bag {:?}",
            slots.iter().filter(|s| s.0 != 0).collect::<Vec<_>>()
        );
        if picked[..] != [worn[if ch == 1 { 3 } else { 2 }], worn[1], worn[0], worn[4]] {
            changed += 1;
        }

        // The applier over what the scan parked.
        assert_eq!(cpu.call(APPLY, &[ch]), Stop::Returned, "{:x?}", cpu.calls);
        let swaps = cpu.regs.r[V0];
        session.slot_browse_confirm(SLOT_BROWSE_BEST_ROW, port);
        let bag: Vec<(u8, u8)> = (0..256u32)
            .map(|i| (cpu.mem.rd8(BAG + i * 2), cpu.mem.rd8(BAG + i * 2 + 1)))
            .collect();
        assert_eq!(
            bag.as_slice(),
            session.inventory().slots(),
            "bag: round {round}: char {ch} worn {worn:02x?} picks {picked:02x?}, {swaps} swaps"
        );
        let after: Vec<u8> = (0..8).map(|i| cpu.mem.rd8(rec + EQUIP + i)).collect();
        let mut raw = [0u8; 8];
        raw.copy_from_slice(&after);
        assert_eq!(
            engine_equip_from_record(raw, ch as u8),
            session.record().equip,
            "worn: round {round}: char {ch} worn {worn:02x?} picks {picked:02x?}"
        );
    }
    assert!(changed > 1000, "the scan moved {changed} times");
}
