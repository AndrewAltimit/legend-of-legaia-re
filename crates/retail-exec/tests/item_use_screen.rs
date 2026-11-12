//! The Items screen's two apply sub-screens (PROT 0899) against the field
//! menu's Items session on a `World`: `FUN_801D8308` (one target, sub-screen
//! `8`) and `FUN_801D7FF8` (the whole party, sub-screen `9`, items with effect
//! flag `0x20`).
//!
//! Retail runs from the confirm on: the target check `FUN_8003FB10` (single
//! target only), the item applier `FUN_800402F4`, the stat aggregator, the
//! bag's by-slot consume `FUN_80043048`, the re-probe `FUN_8003043C` that
//! decides whether the apply screen stays up, and the exhaustion timer's exit
//! to the Use list or the command window. The window VM, the window tick, the
//! picker (answering "confirmed") and the cues are hooked; everything the
//! item does runs for real. The port takes the same bag, records and target
//! through `build_pause_items_session`, its pad, and `tick_open_subsession`,
//! which applies the use and resumes the screen. The three records, the whole
//! bag, the cues and where the screen lands compare.

use crate::common::{Rng, disc, scus, scus_cpu};
use crate::item_apply::seed;
use legaia_engine_battle::accessory_passives::AccessoryPassives;
use legaia_engine_core::world::World;
use legaia_engine_menu_runtime::field_menu_dispatch::{
    FieldMenuSubsession, build_pause_items_session, tick_open_subsession,
};
use legaia_engine_menus::inventory_use::InventoryUseState;
use legaia_engine_menus::item_bag::ItemBag;
use legaia_engine_menus::items::ItemCatalog;
use legaia_engine_menus::pause_screens::{PauseItemsFocus, special_use_route_for_item};
use legaia_engine_system::input::PadButton;
use legaia_game_tables::accessory_passive::AccessoryPassiveTable;
use legaia_game_tables::item_effect::ItemEffectTable;
use legaia_save::{CharacterRecord, Party};
use psx_retail_exec::{A0, Stop, V0};
use std::cell::RefCell;
use std::rc::Rc;

const OVERLAY_BASE: u32 = 0x801C_E818;
const SINGLE: u32 = 0x801D_8308;
const ALL: u32 = 0x801D_7FF8;
const AGGREGATOR: u32 = 0x8004_2558;
const VM: u32 = 0x801D_6628;
const WINDOW_TICK: u32 = 0x8003_1D00;
const PICKER: u32 = 0x801D_688C;
const PREVIEW: u32 = 0x801D_6A54;
const CUE: u32 = 0x8003_5BD0;
const SFX: u32 = 0x8003_5B50;
const RAND: u32 = 0x8005_6798;
const RECORDS: u32 = 0x8008_4708;
const STRIDE: u32 = 0x414;
const BAG: u32 = 0x8008_5958;
const WINDOW_LO: u32 = 0x8007_B5EA;
const WINDOW_HI: u32 = 0x8007_B5EC;
const SCREEN: u32 = 0x801E_46A4;
const STEP: u32 = 0x801E_46AC;
const TARGET: u32 = 0x801E_46C4;
const TIMER: u32 = 0x801E_46D0;
const BUSY: u32 = 0x8007_BB80;
const LIST_ROW: u32 = 0x8007_BB88;
const FRAME_STEP: u32 = 0x1F80_0393;

/// Where a use leaves the screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Landing {
    /// The confirm was refused; the target rows stay up.
    Refused,
    /// The apply screen stays up for another use of the same stack.
    Again,
    /// Back to the Use list.
    List,
    /// The bag is empty: back to the command window.
    Command,
}

fn items(sub: &FieldMenuSubsession) -> &legaia_engine_menus::pause_screens::PauseItemsSession {
    match sub {
        FieldMenuSubsession::Items(s) => s,
        _ => unreachable!(),
    }
}

#[test]
fn the_apply_screens_use_what_retail_uses_and_land_where_it_lands() {
    differential!(prot(899, SINGLE), prot(899, ALL));
    let (Some(mut cpu), Some(disc), Some(scus)) = (scus_cpu(), disc(), scus()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &disc.read_entry(899).expect("PROT 0899"));
    cpu.stub(RAND, 0);
    for va in [VM, WINDOW_TICK, PREVIEW] {
        cpu.stub(va, 0);
    }
    cpu.hook(PICKER, |r, _| r.r[V0] = 1);
    let cues: Rc<RefCell<Vec<(u32, u32)>>> = Rc::default();
    for entry in [CUE, SFX] {
        let c = cues.clone();
        cpu.hook(entry, move |r, _| c.borrow_mut().push((entry, r.r[A0])));
    }
    cpu.mem.wr32(0x8008_4594, 3);
    cpu.mem.load(0x8008_4598, &[0, 1, 2]);

    let table = ItemEffectTable::from_scus(&scus).expect("item effects");
    let mut world = World::default();
    world.set_item_catalog(ItemCatalog::vanilla());
    world.set_item_effects(table.clone());
    world.set_accessory_passives(AccessoryPassives::from_disc(
        &AccessoryPassiveTable::from_scus(&scus).expect("passives"),
    ));
    let usable: Vec<u8> = (1..=255u8)
        .filter(|&id| {
            table.kind(id) == 2
                && table.effect(id).is_some_and(|e| e.flags & 0x02 != 0)
                && special_use_route_for_item(id).is_none()
        })
        .collect();
    assert!(usable.len() > 10, "{usable:x?}");

    let mut rng = Rng(0x4954_5553);
    let mut seen = std::collections::BTreeMap::<(bool, Landing), u32>::new();
    let mut failures = Vec::new();
    for round in 0..3_000 {
        let id = usable[rng.below(usable.len() as u32) as usize];
        let e = table.effect(id).unwrap();
        let all = e.flags & 0x20 != 0;
        let records: Vec<Vec<u8>> = (0..3).map(|_| seed(&mut rng)).collect();
        for (i, rec) in records.iter().enumerate() {
            cpu.mem.load(RECORDS + i as u32 * STRIDE, rec);
        }
        // A bag with the item at one slot, a stack of one to three, and a
        // few other stacks (or none, so the last use empties the bag).
        let mut slots = vec![(0u8, 0u8); 256];
        let at = rng.below(40) as usize;
        slots[at] = (id, 1 + rng.below(3) as u8);
        if rng.below(4) != 0 {
            for _ in 0..1 + rng.below(6) {
                let s = rng.below(40) as usize;
                let other = 1 + rng.below(255) as u8;
                if slots[s].0 == 0 && !slots.iter().any(|x| x.0 == other) {
                    slots[s] = (other, 1 + rng.below(9) as u8);
                }
            }
        }
        for (i, (sid, n)) in slots.iter().enumerate() {
            cpu.mem.wr8(BAG + i as u32 * 2, *sid);
            cpu.mem.wr8(BAG + i as u32 * 2 + 1, *n);
        }
        let bag = ItemBag::from_slots(&slots);
        let (lo, hi) = bag.window_bounds();
        cpu.mem.wr16(WINDOW_LO, lo as u16);
        cpu.mem.wr16(WINDOW_HI, hi as u16);
        assert_eq!(cpu.call(AGGREGATOR, &[]), Stop::Returned);
        let settled: Vec<Vec<u8>> = (0..3)
            .map(|i| cpu.mem.read(RECORDS + i * STRIDE, STRIDE as usize))
            .collect();

        // The port: the same party and bag, the Items screen opened over it.
        let mut party = Party::zeroed(3);
        for (i, rec) in settled.iter().enumerate() {
            party.members[i] = CharacterRecord::parse(rec).unwrap();
        }
        world.party.roster = party;
        world.party.party_count = 3;
        world.party.inventory = bag;
        world.refresh_party_ability_bits();
        let mut sub = FieldMenuSubsession::Items(build_pause_items_session(&world));
        let press = |sub: &mut FieldMenuSubsession, world: &mut World, b: PadButton| {
            let _ = tick_open_subsession(sub, b.mask(), None, world);
        };
        press(&mut sub, &mut world, PadButton::Cross); // Use
        let row = items(&sub).rows.iter().position(|r| r.slot as usize == at);
        let Some(row) = row else {
            failures.push(format!("round {round}: item {id:#04x} has no Use row"));
            continue;
        };
        for _ in 0..64 {
            let c = items(&sub).list_cursor();
            if c == row {
                break;
            }
            let b = if c / 12 < row / 12 {
                PadButton::Right
            } else if c / 12 > row / 12 {
                PadButton::Left
            } else {
                PadButton::Down
            };
            press(&mut sub, &mut world, b);
        }
        assert_eq!(items(&sub).list_cursor(), row, "round {round}");
        press(&mut sub, &mut world, PadButton::Cross);
        // The target the single-target screen confirms on; the all-party one
        // has no row and applies off the first member.
        let target = if all { 0 } else { rng.below(3) };
        match items(&sub).inner.state {
            InventoryUseState::TargetSelect { .. } => {
                for _ in 0..8 {
                    let InventoryUseState::TargetSelect { cursor, .. } = items(&sub).inner.state
                    else {
                        break;
                    };
                    if items(&sub)
                        .inner
                        .targets
                        .get(cursor)
                        .map(|t| u32::from(t.slot))
                        == Some(target)
                    {
                        break;
                    }
                    press(&mut sub, &mut world, PadButton::Down);
                }
            }
            InventoryUseState::AllPartyConfirm { .. } => {}
            // The list refused the row (a dimmed row is the list kernel's
            // business, not these screens').
            _ => continue,
        }
        let on_target = match items(&sub).inner.state {
            InventoryUseState::TargetSelect { cursor, .. } => items(&sub)
                .inner
                .targets
                .get(cursor)
                .map(|t| u32::from(t.slot)),
            _ => Some(0),
        };
        if on_target != Some(target) {
            continue;
        }
        press(&mut sub, &mut world, PadButton::Cross);
        let port_landing = {
            let s = items(&sub);
            if s.target_select() {
                Landing::Again
            } else if s.focus == PauseItemsFocus::Command || s.bag_empty() {
                Landing::Command
            } else {
                Landing::List
            }
        };

        // Retail, from the confirm on.
        cues.borrow_mut().clear();
        cpu.mem.wr32(LIST_ROW, at as u32);
        cpu.mem.wr32(TARGET, target);
        cpu.mem.wr32(BUSY, 0);
        cpu.mem.wr32(SCREEN, if all { 9 } else { 8 });
        cpu.mem.wr32(STEP, 1);
        let routine = if all { ALL } else { SINGLE };
        let mut retail = None;
        for _ in 0..6 {
            let step = cpu.mem.rd32(STEP);
            // The level-up notice waits for a press; it is the notice's own
            // window, and the use behind it is what is compared.
            if !all && step == 4 {
                cpu.mem.wr32(STEP, 5);
                continue;
            }
            if (all && step == 5) || (!all && step == 6) {
                cpu.mem.wr8(FRAME_STEP, 0x14);
                cpu.mem.wr32(TIMER, 0);
            }
            assert_eq!(cpu.call(routine, &[]), Stop::Returned, "round {round}");
            let (step2, screen) = (cpu.mem.rd32(STEP), cpu.mem.rd32(SCREEN));
            if screen == 6 {
                retail = Some(Landing::List);
                break;
            }
            if screen == 5 {
                retail = Some(Landing::Command);
                break;
            }
            if step == 1 && step2 == 1 {
                retail = Some(Landing::Refused);
                break;
            }
            // Back on the target rows (single) or the confirm (all).
            if step > 1 && step2 == 1 {
                retail = Some(Landing::Again);
                break;
            }
        }
        let retail = retail.expect("the screen lands");
        let what = format!(
            "round {round}: item {id:#04x} class {:#x} tier {} all {all} target {target} stack {} port {:?} {:?} left {:?} retail left {:?}",
            e.class,
            e.tier,
            slots[at].1,
            items(&sub).focus,
            items(&sub).inner.state,
            world.party.inventory.slots()[at],
            (
                cpu.mem.rd8(BAG + at as u32 * 2),
                cpu.mem.rd8(BAG + at as u32 * 2 + 1)
            ),
        );
        if retail == Landing::Refused {
            // The port's target rows also stay up.
            if !items(&sub).target_select() {
                failures.push(format!("{what}: retail refuses, the port applied"));
            }
            *seen.entry((all, retail)).or_default() += 1;
            continue;
        }
        if retail != port_landing {
            failures.push(format!(
                "{what}: lands retail {retail:?} port {port_landing:?}"
            ));
            continue;
        }
        let rbag: Vec<(u8, u8)> = (0..256u32)
            .map(|i| (cpu.mem.rd8(BAG + i * 2), cpu.mem.rd8(BAG + i * 2 + 1)))
            .collect();
        if rbag.as_slice() != world.party.inventory.slots() {
            failures.push(format!("{what}: bag differs"));
            continue;
        }
        world.refresh_party_ability_bits();
        for i in 0..3 {
            let r = cpu.mem.read(RECORDS + i as u32 * STRIDE, STRIDE as usize);
            let p = world.party.roster.members[i].write();
            let d = crate::common::diff_offsets(&r, &p);
            if !d.is_empty() {
                failures.push(format!("{what}: record {i}: {d:?}"));
                break;
            }
        }
        let got = cues.borrow().clone();
        let want_sfx = (SFX, 0x25);
        if !got.contains(&want_sfx) {
            failures.push(format!("{what}: cues {got:x?}"));
        }
        *seen.entry((all, retail)).or_default() += 1;
    }
    assert!(
        failures.is_empty(),
        "{} failures, first:\n{}",
        failures.len(),
        failures
            .iter()
            .take(12)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
    eprintln!("{seen:?}");
    for key in [
        (false, Landing::Again),
        (false, Landing::List),
        (true, Landing::Again),
        (true, Landing::List),
    ] {
        assert!(
            seen.get(&key).copied().unwrap_or(0) > 20,
            "{key:?}: {seen:?}"
        );
    }
}
