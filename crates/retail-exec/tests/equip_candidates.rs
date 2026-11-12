//! The Equip screen's candidate step `FUN_801D9C14` (sub-screen `0x14`,
//! PROT 0899) against `EquipSession`'s item picker and the window-25 model
//! `equip_screen_model` builds from it.
//!
//! Each episode enters the step (state `0`: the live block off
//! `FUN_801CF650`), arms it (state `1`) and runs state `2` once with the list
//! kernel's words seeded for one candidate row: the trial equip and the stat
//! aggregator `FUN_80042558` run for real, then the list's mode word decides
//! between browsing, cancel and confirm. The two stat blocks, the staged id,
//! and on a confirm or cancel the record, the bag, the cue and the browse row
//! all compare. The window VM and the window tick are stubbed; the list rows
//! the kernel would build are the port's `items_for_slot`.

use crate::common::{Rng, disc, scus, scus_cpu};
use crate::equip_party_compare::{Gear, gear, seed_record};
use legaia_engine_menus::equip_session::{
    EquipEvent, EquipInput, EquipSession, EquipState, REMOVE_ROW_ID, engine_equip_from_record,
    slot_for_browse_row,
};
use legaia_engine_menus::equipment::{DiscEquipInfo, equip_modifier_table_from_disc};
use legaia_engine_menus::item_bag::ItemBag;
use legaia_engine_menus::menu_item_category::parse_category_table;
use legaia_engine_menus::pause_screens::{
    EquipCompareCtx, equip_screen_model, stat_record_from_character,
};
use legaia_game_tables::equip_stats::EquipStatTable;
use legaia_save::CharacterRecord;
use psx_retail_exec::{A0, Stop};
use std::cell::RefCell;
use std::rc::Rc;

const OVERLAY_BASE: u32 = 0x801C_E818;
const CANDIDATES: u32 = 0x801D_9C14;
const AGGREGATOR: u32 = 0x8004_2558;
const VM: u32 = 0x801D_6628;
const WINDOW_TICK: u32 = 0x8003_1D00;
const CUE: u32 = 0x8003_5BD0;
const BUZZ: u32 = 0x8003_5B50;
const RECORDS: u32 = 0x8008_4708;
const STRIDE: u32 = 0x414;
const BAG: u32 = 0x8008_5958;
const WINDOW_LO: u32 = 0x8007_B5EA;
const WINDOW_HI: u32 = 0x8007_B5EC;
const PARTY_COUNT: u32 = 0x8008_4594;
const ROSTER: u32 = 0x8008_4598;
const SCREEN: u32 = 0x801E_46A4;
const STEP: u32 = 0x801E_46AC;
const STAGED: u32 = 0x801E_46B0;
const ROW: u32 = 0x801E_46C0;
const PICKED: u32 = 0x801E_46C4;
const BLOCK: u32 = 0x801E_F080;
const TRIAL_BLOCK: u32 = 0x801E_F0A0;
/// The list kernel's words: busy, selected row, mode, row class.
const BUSY: u32 = 0x8007_BB80;
const LIST_ROW: u32 = 0x8007_BB88;
const LIST_MODE: u32 = 0x8007_BB94;
const LIST_CLASS: u32 = 0x8007_BB9C;

fn block(m: &mut psx_retail_exec::Memory, at: u32) -> [i32; 8] {
    std::array::from_fn(|k| m.rd32(at + k as u32 * 4) as i32)
}

/// A record's bytes with the spell slots past its count blanked: a grant the
/// restore revokes again leaves its id, level and experience behind there,
/// where no reader looks; and a stash slot's level `0` reads as `1`, since
/// the head insert floors it there (`0x8004331C..0x80043324`) and the park
/// writes the floored level back.
fn live_bytes(r: &CharacterRecord) -> Vec<u8> {
    let mut b = r.write();
    for slot in 0..5 {
        let at = 0x2B5 + slot * 0x14;
        b[at] = b[at].max(1);
    }
    let n = usize::from(b[0x13C]).min(36);
    for i in n..36 {
        b[0x13D + i] = 0;
        b[0x161 + i] = 0;
        b[0x8 + i * 4..0xC + i * 4].fill(0);
    }
    b
}

#[test]
fn the_candidate_step_previews_and_commits_as_retail_does() {
    differential!(prot(899, CANDIDATES), scus(AGGREGATOR));
    let (Some(mut cpu), Some(disc), Some(scus)) = (scus_cpu(), disc(), scus()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let overlay = disc.read_entry(899).expect("PROT 0899");
    cpu.mem.load(OVERLAY_BASE, &overlay);
    cpu.stub(VM, 0);
    cpu.stub(WINDOW_TICK, 0);
    let cues: Rc<RefCell<Vec<u32>>> = Rc::default();
    for entry in [CUE, BUZZ] {
        let c = cues.clone();
        cpu.hook(entry, move |r, _| c.borrow_mut().push(entry << 8 | r.r[A0]));
    }
    let stats = EquipStatTable::from_scus(&scus).expect("equipment");
    let table = equip_modifier_table_from_disc(&stats);
    let effects =
        legaia_game_tables::item_effect::ItemEffectTable::from_scus(&scus).expect("item effects");
    let mut info = DiscEquipInfo::from_disc(&stats);
    info.install_goods(&effects);
    let category = parse_category_table(&overlay).expect("category");
    let passives = legaia_engine_battle::accessory_passives::AccessoryPassives::from_disc(
        &legaia_game_tables::accessory_passive::AccessoryPassiveTable::from_scus(&scus)
            .expect("passives"),
    );
    let Gear {
        gear: sellable,
        raseru,
        by_class,
    } = gear(&mut cpu.mem, &info);
    cpu.mem.wr8(PARTY_COUNT, 3);
    cpu.mem.load(ROSTER, &[0, 1, 2]);

    let mut rng = Rng(0x4341_4E44);
    let mut seen = [0u32; 6];
    let mut clipped = 0u32;
    for episode in 0..5_000 {
        let ch = rng.below(3);
        for c in 0..3u32 {
            let mut raw = vec![0u8; STRIDE as usize];
            seed_record(&mut rng, &mut raw, c as usize, &by_class, &raseru);
            cpu.mem.load(RECORDS + c * STRIDE, &raw);
        }
        // A bag with holes, one stack per id.
        let mut slots = vec![(0u8, 0u8); 256];
        for _ in 0..(1 + rng.below(40)) {
            let at = rng.below(64) as usize;
            let id = match rng.below(4) {
                0 => 1 + rng.below(255) as u8,
                _ => sellable[rng.below(sellable.len() as u32) as usize],
            };
            if id > 0x1A && !slots.iter().any(|s| s.0 == id) {
                slots[at] = (id, 1 + rng.below(3) as u8);
            }
        }
        for (i, (id, n)) in slots.iter().enumerate() {
            cpu.mem.wr8(BAG + i as u32 * 2, *id);
            cpu.mem.wr8(BAG + i as u32 * 2 + 1, *n);
        }
        cpu.mem.wr16(WINDOW_LO, 0);
        cpu.mem.wr16(WINDOW_HI, 0x100);
        assert_eq!(cpu.call(AGGREGATOR, &[]), Stop::Returned);
        let rec = RECORDS + ch * STRIDE;
        let character = CharacterRecord::parse(&cpu.mem.read(rec, STRIDE as usize)).unwrap();
        let names: Vec<String> = (0..3).map(|i| format!("{i}")).collect();

        // The port opens the same row's candidate list from the slot browse.
        let row = 1 + rng.below(7);
        let mut record = stat_record_from_character(&character);
        record.equip = engine_equip_from_record(record.equip, ch as u8);
        let mut session = EquipSession::new(
            record,
            ItemBag::from_slots(&slots),
            table.clone(),
            Default::default(),
            Vec::new(),
        )
        .with_active_party_slot(ch as u8)
        .with_weapon_category(category.clone())
        .with_restrictions(info.clone(), ch as u8);
        for _ in 0..row {
            session.input(EquipInput {
                down: true,
                ..Default::default()
            });
        }
        session.input(EquipInput {
            cross: true,
            ..Default::default()
        });
        let slot = slot_for_browse_row(row as u8);
        assert_eq!(
            session.state(),
            EquipState::ItemPicker {
                slot,
                cursor: 1.min(session.items_for_slot(slot).len() as u16 - 1)
            },
            "episode {episode}: row {row} opens its list"
        );
        let items = session.items_for_slot(slot);
        let target = rng.below(items.len() as u32) as u16;
        // Walk the hand there: page flips, then rows.
        for _ in 0..64 {
            let EquipState::ItemPicker { cursor, .. } = session.state() else {
                unreachable!()
            };
            if cursor == target {
                break;
            }
            let input = if cursor / 12 < target / 12 {
                EquipInput {
                    right: true,
                    ..Default::default()
                }
            } else if cursor / 12 > target / 12 {
                EquipInput {
                    left: true,
                    ..Default::default()
                }
            } else {
                EquipInput {
                    down: true,
                    ..Default::default()
                }
            };
            session.input(input);
        }
        assert_eq!(
            session.state(),
            EquipState::ItemPicker {
                slot,
                cursor: target
            }
        );
        session.drain_events();
        let item = items[usize::from(target)];

        // Retail: enter, arm, then one state-2 frame on that row.
        cpu.mem.wr32(PICKED, ch | 0x2000);
        cpu.mem.wr32(ROW, row);
        cpu.mem.wr32(SCREEN, 0x14);
        cpu.mem.wr32(STEP, 0);
        cpu.mem.wr32(BUSY, 0);
        assert_eq!(cpu.call(CANDIDATES, &[]), Stop::Returned);
        assert_eq!(cpu.call(CANDIDATES, &[]), Stop::Returned);
        assert_eq!(cpu.mem.rd32(STEP), 2);
        let (class, list_row) = if item.id == REMOVE_ROW_ID {
            (0x4000, 0)
        } else if item.equipped {
            (0x7000, rng.below(256))
        } else {
            let at = slots
                .iter()
                .position(|s| s.0 == item.id)
                .expect("a bag row");
            ([0x6000, 0x9000][rng.below(2) as usize], at as u32)
        };
        cpu.mem.wr32(LIST_CLASS, class);
        cpu.mem.wr32(LIST_ROW, list_row);
        let mode = [1u32, 1, 2, 2, 3][rng.below(5) as usize];
        cpu.mem.wr32(LIST_MODE, mode);
        cues.borrow_mut().clear();
        assert_eq!(cpu.call(CANDIDATES, &[]), Stop::Returned);

        let model = equip_screen_model(
            &session,
            ch as u8,
            &names,
            &[0, 1, 2],
            None,
            Some(EquipCompareCtx {
                record: &character,
                equip_info: Some(&info),
                item_effects: Some(&effects),
                passives: Some(&passives),
            }),
        );
        let cmp = model.compare.expect("window 25 on the candidate step");
        let what = format!(
            "episode {episode}: char {ch} row {row} item {item:?} class {class:#x} mode {mode}"
        );
        assert_eq!(cmp.staged_id, cpu.mem.rd32(STAGED) as i32, "{what}: staged");
        assert_eq!(
            cmp.current,
            block(&mut cpu.mem, BLOCK),
            "{what}: live block"
        );
        assert_eq!(
            cmp.candidate,
            block(&mut cpu.mem, TRIAL_BLOCK),
            "{what}: trial block"
        );
        seen[usize::from(item.id == REMOVE_ROW_ID)] += 1;

        let got = cues.borrow().clone();
        match mode {
            1 => {
                assert!(got.is_empty(), "{what}: {got:x?}");
                // The trial is undone, bar one retail side effect: the
                // aggregator it ran lowered a current HP / MP above the trial
                // maximum, and the restore does not raise it back.
                let after = CharacterRecord::parse(&cpu.mem.read(rec, STRIDE as usize)).unwrap();
                let (b, a) = (character.hp_mp_sp(), after.hp_mp_sp());
                assert!(a.hp_cur <= b.hp_cur && a.mp_cur <= b.mp_cur, "{what}");
                clipped += u32::from(a.hp_cur < b.hp_cur || a.mp_cur < b.mp_cur);
                let mut undone = after.clone();
                let mut hms = undone.hp_mp_sp();
                hms.hp_cur = b.hp_cur;
                hms.mp_cur = b.mp_cur;
                undone.set_hp_mp_sp(hms);
                let (x, y) = (live_bytes(&character), live_bytes(&undone));
                assert!(
                    x == y,
                    "{what}: restored {:?}",
                    crate::common::diff_offsets(&x, &y)
                );
                assert_eq!(cpu.mem.rd32(STEP), 2, "{what}");
                seen[2] += 1;
                continue;
            }
            3 => session.input(EquipInput {
                circle: true,
                ..Default::default()
            }),
            _ => session.input(EquipInput {
                cross: true,
                ..Default::default()
            }),
        }
        let events = session.drain_events();
        let EquipState::SlotPicker { cursor } = session.state() else {
            panic!("{what}: the port stays on the list: {:?}", session.state());
        };
        assert_eq!(cpu.mem.rd32(SCREEN), 0x13, "{what}: back to the browse");
        assert_eq!(
            cpu.mem.rd32(ROW) & 0xFFF,
            u32::from(cursor),
            "{what}: browse row"
        );
        let worn: [u8; 8] = std::array::from_fn(|i| cpu.mem.rd8(rec + 0x196 + i as u32));
        assert_eq!(
            engine_equip_from_record(worn, ch as u8),
            session.record().equip,
            "{what}: worn"
        );
        let bag: Vec<(u8, u8)> = (0..256u32)
            .map(|i| (cpu.mem.rd8(BAG + i * 2), cpu.mem.rd8(BAG + i * 2 + 1)))
            .collect();
        assert_eq!(bag.as_slice(), session.inventory().slots(), "{what}: bag");
        let committed = events
            .iter()
            .any(|e| matches!(e, EquipEvent::Committed { .. }));
        if mode == 3 {
            assert!(got.is_empty() && !committed, "{what}: {got:x?}");
            seen[3] += 1;
        } else if committed {
            assert_eq!(got, [CUE << 8 | 0x24], "{what}");
            seen[4] += 1;
        } else if events.contains(&EquipEvent::InvalidConfirm) {
            assert!(
                got == [BUZZ << 8 | 0x37] || got == [CUE << 8 | 0x24],
                "{what}: {got:x?}"
            );
            seen[5] += 1;
        } else {
            assert!(got.is_empty(), "{what}: {got:x?} {events:?}");
        }
    }
    assert!(seen.iter().all(|&n| n > 100), "{seen:?}");
    eprintln!("browsing frames that clipped a current HP / MP: {clipped}");
}
