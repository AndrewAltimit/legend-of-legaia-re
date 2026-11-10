//! The menu overlay's (PROT 0899) party picker `FUN_801D98F0` (`0x12`), card
//! drivers `FUN_801DAE24` / `FUN_801DAEF4` (`0x18` / `0x19`), shop mode
//! select `FUN_801DAFD4` (`0x1A`) and quantity spinner `FUN_801DBC5C`
//! (`0x1E`), frame by frame against `SaveScreenMachine`, with the list kernel
//! `FUN_801D688C` live beneath the two lists and its port `menu_cursor_nav`
//! where the port's host calls it; and the shop buy list `FUN_801DB21C`
//! (`0x1B`) against `buy_list_confirm_route`, and the buy recipient picker
//! `FUN_801DB380` (`0x1C`) against `BuyRecipientSession`, its bag and equip
//! commits and Point Card accrual included.
//!
//! As in `save_subscreens`, the window VM and the window-list tick are logged
//! and stubbed, and the busy word, the pad words and the button masks are
//! scripted; the card op `FUN_801DD35C` is hooked to a scripted result, and
//! the spinner's list result word `0x8007BB94` (the stubbed list tick's
//! output) is scripted. Each frame compares the screen id, the step, the
//! cursor, the scripts run, both cue paths, the card ops, the card handle
//! install and the staging cells the screens write.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_menus::save_subscreen::{
    CardOp, SaveEntryContext, SaveScreenMachine, SaveSubScreen, SubScreenEffect, SubScreenInput,
};
use legaia_engine_menus::shop::{BuyListRoute, buy_list_confirm_route};
use legaia_engine_system::menu_input::{CursorNav, NavButtons, menu_cursor_nav};
use psx_retail_exec::{A0, A1, Cpu, Stop, V0};
use std::cell::RefCell;
use std::rc::Rc;

const OVERLAY_BASE: u32 = 0x801C_E818;
const PARTY_PICKER: u32 = 0x801D_98F0;
const CARD_LOAD: u32 = 0x801D_AE24;
const CARD_SAVE: u32 = 0x801D_AEF4;
const MODE_SELECT: u32 = 0x801D_AFD4;
const BUY_LIST: u32 = 0x801D_B21C;
const SPINNER: u32 = 0x801D_BC5C;
const LIST: u32 = 0x801D_688C;
const VM: u32 = 0x801D_6628;
const CUE: u32 = 0x8003_5BD0;
const CUE_88: u32 = 0x8004_2310;
const NAV_CUE: u32 = 0x8003_5B50;
const WINDOW_TICK: u32 = 0x8003_1D00;
const CARD: u32 = 0x801D_D35C;

const SCREEN: u32 = 0x801E_46A4;
const STEP: u32 = 0x801E_46AC;
const STAGED_ID: u32 = 0x801E_46B0;
const STAGED_COUNT: u32 = 0x801E_46B4;
const SHOP_CURSOR: u32 = 0x801E_46BC;
const PARTY_CURSOR: u32 = 0x801E_46C4;
const BUSY: u32 = 0x8007_BB80;
const EDGE: u32 = 0x8007_B874;
const REPEAT: u32 = 0x8007_BB84;
const CONFIRM_MASK: u32 = 0x8008_46D0;
const CANCEL_MASK: u32 = 0x8008_46D4;
const LIST_CONFIRM_MASK: u32 = 0x801E_F0F0;
const LIST_CANCEL_MASK: u32 = 0x801E_F0F4;
const PARTY_COUNT: u32 = 0x8008_4594;
const GOLD: u32 = 0x8008_459C;
const ITEM_TABLE: u32 = 0x8007_4368;
const BAG: u32 = 0x8008_5958;
const WINDOW_LO: u32 = 0x8007_B5EA;
const WINDOW_HI: u32 = 0x8007_B5EC;
/// The staged bag slot / item id, `0x8007BB88`.
const STAGED: u32 = 0x8007_BB88;
/// Two more staging words the mode select clears with it.
const STAGED_B: u32 = 0x8007_BB90;
const STAGED_C: u32 = 0x8007_BB98;
/// The list result word: `4` idle, `2` confirm, `3` cancel.
const LIST_RESULT: u32 = 0x8007_BB94;
const CARD_HANDLE: u32 = 0x8007_B44C;
const CARD_HANDLE_SRC: u32 = 0x801C_6EA0;
const ENTRY_CONTEXT: u32 = 0x8007_B450;

#[derive(Default)]
struct Log {
    scripts: usize,
    cues: Vec<u32>,
    nav_cues: Vec<u32>,
    card_ops: Vec<(u32, u32)>,
}

struct Rig {
    cpu: Cpu,
    log: Rc<RefCell<Log>>,
    card_result: Rc<RefCell<u32>>,
}

fn rig() -> Option<Rig> {
    let (mut cpu, d) = (scus_cpu()?, disc()?);
    cpu.mem.load(
        OVERLAY_BASE,
        &d.read_entry_footprint(899).expect("PROT 0899"),
    );
    let log = Rc::new(RefCell::new(Log::default()));
    let l = log.clone();
    cpu.hook(VM, move |_, _| l.borrow_mut().scripts += 1);
    let l = log.clone();
    cpu.hook(CUE, move |r, _| l.borrow_mut().cues.push(r.r[A0]));
    let l = log.clone();
    cpu.hook(CUE_88, move |r, _| l.borrow_mut().cues.push(r.r[A0]));
    let l = log.clone();
    cpu.hook(NAV_CUE, move |r, _| l.borrow_mut().nav_cues.push(r.r[A0]));
    cpu.stub(WINDOW_TICK, 0);
    let card_result = Rc::new(RefCell::new(0));
    let (l, c) = (log.clone(), card_result.clone());
    cpu.hook(CARD, move |r, _| {
        l.borrow_mut().card_ops.push((r.r[A0], r.r[A1]));
        r.r[V0] = *c.borrow();
    });
    Some(Rig {
        cpu,
        log,
        card_result,
    })
}

fn dispatching() -> SaveScreenMachine {
    let mut m = SaveScreenMachine::new(SaveEntryContext::DebugParamEditor);
    m.tick(SubScreenInput::default(), 0xFF);
    m.tick(SubScreenInput::default(), 0xFF);
    m
}

fn edge_word(rng: &mut Rng) -> u32 {
    match rng.below(8) {
        0 | 1 => 0,
        2 => 0x40,
        3 => 0x20,
        4 => 0x60,
        5 => [0x1000, 0x4000, 0x2000, 0x8000, 0x10, 0x80][rng.below(6) as usize],
        _ => rng.next() & 0xF0F0,
    }
}

const SENTINEL: u32 = 0x00AB_CDEF;

#[test]
fn the_party_card_and_shop_screens_step_as_retail_steps() {
    differential!(
        prot(899, PARTY_PICKER),
        prot(899, CARD_LOAD),
        prot(899, CARD_SAVE),
        prot(899, MODE_SELECT),
        prot(899, SPINNER),
        prot(899, LIST)
    );
    let Some(Rig {
        mut cpu,
        log,
        card_result,
    }) = rig()
    else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem.wr32(PARTY_CURSOR, 0);
    assert_eq!(cpu.call(LIST, &[PARTY_CURSOR, 2, 1]), Stop::Returned);
    let screens = [
        (PARTY_PICKER, SaveSubScreen::PartyPicker),
        (CARD_LOAD, SaveSubScreen::CardLoad),
        (CARD_SAVE, SaveSubScreen::CardSave),
        (MODE_SELECT, SaveSubScreen::ShopModeSelect),
        (SPINNER, SaveSubScreen::QuantitySpinner),
    ];
    let mut rng = Rng(0x4D45_4E55);
    let mut ends = [0u32; 5];
    let (mut refused_sells, mut settles) = (0, 0);
    for episode in 0..8000 {
        let k = rng.below(5) as usize;
        let (entry, screen) = screens[k];
        let (confirm, cancel) = if rng.below(4) == 0 {
            (0x20, 0x40)
        } else {
            (0x40, 0x20)
        };
        for (a, v) in [
            (CONFIRM_MASK, confirm),
            (CANCEL_MASK, cancel),
            (LIST_CONFIRM_MASK, confirm),
            (LIST_CANCEL_MASK, cancel),
        ] {
            cpu.mem.wr32(a, v);
        }
        let party = 1 + rng.below(3);
        cpu.mem.wr8(PARTY_COUNT, party as u8);
        // A bag window that holds something to sell, or not.
        let (lo, hi) = [(0u16, 256u16), (0, 128), (128, 256)][rng.below(3) as usize];
        cpu.mem.wr16(WINDOW_LO, lo);
        cpu.mem.wr16(WINDOW_HI, hi);
        let stocked = rng.below(2) == 0;
        for i in 0..256u32 {
            let (id, n) = match rng.below(6) {
                0 if stocked => (1 + rng.below(255) as u8, 1 + rng.below(99) as u8),
                1 => (0, rng.next() as u8),
                2 => (rng.next() as u8, 0),
                _ => (0, 0),
            };
            cpu.mem.wr8(BAG + 2 * i, id);
            cpu.mem.wr8(BAG + 2 * i + 1, n);
        }
        // The host's input for the Sell row: any stack in the window.
        let sellable = (lo..hi).any(|i| {
            let a = BAG + 2 * u32::from(i);
            cpu.mem.rd8(a) != 0 && cpu.mem.rd8(a + 1) != 0
        });
        let staged_slot = rng.below(256);
        cpu.mem.wr32(CARD_HANDLE_SRC, 0x8012_3450 + rng.below(16));
        // The context the shared machine models: a save that returns to the
        // selector (`0x8007B450` clear; set, retail's save driver leaves for
        // the terminal screen instead, which the machine does not carry).
        cpu.mem.wr32(ENTRY_CONTEXT, 0);

        let count = if k == 0 { party } else { 3 };
        let cursor_at = if k == 0 { PARTY_CURSOR } else { SHOP_CURSOR };
        let mut cursor: u32 = rng.below(count);
        cpu.mem.wr32(PARTY_CURSOR, cursor);
        cpu.mem.wr32(SHOP_CURSOR, cursor);
        cpu.mem.wr32(STEP, 0);
        let mut port = dispatching();
        port.goto(screen);
        let mut port_log = Log::default();
        *log.borrow_mut() = Log::default();
        for frame in 0..40 {
            let what = format!("episode {episode} screen {:#x} frame {frame}", screen.id());
            let busy = u32::from(rng.below(3) == 0);
            let edge = edge_word(&mut rng);
            let repeat = [0, 0x1000, 0x4000, 0x2000, 0x8000][rng.below(5) as usize];
            let card_done = u32::from(rng.below(3) == 0);
            let spinner = [1u32, 2, 3, 4][rng.below(4) as usize];
            cpu.mem.wr32(BUSY, busy);
            cpu.mem.wr32(EDGE, edge);
            cpu.mem.wr32(REPEAT, repeat);
            cpu.mem.wr32(SCREEN, u32::from(screen.id()));
            cpu.mem.wr32(LIST_RESULT, spinner);
            cpu.mem.wr32(STAGED, staged_slot);
            cpu.mem.wr32(STAGED_B, SENTINEL);
            cpu.mem.wr32(STAGED_C, SENTINEL);
            cpu.mem.wr32(STAGED_ID, SENTINEL);
            cpu.mem.wr32(STAGED_COUNT, SENTINEL);
            cpu.mem.wr32(CARD_HANDLE, SENTINEL);
            *card_result.borrow_mut() = card_done;
            let step_before = cpu.mem.rd32(STEP);
            assert_eq!(cpu.call(entry, &[]), Stop::Returned, "{what}");

            let step = port.step();
            let mut nav = 0;
            let lists = matches!(
                screen,
                SaveSubScreen::PartyPicker | SaveSubScreen::ShopModeSelect
            );
            if lists && step == 1 && busy == 0 {
                let r = menu_cursor_nav(
                    &mut cursor,
                    count,
                    true,
                    NavButtons::new(
                        edge & confirm != 0,
                        edge & cancel != 0,
                        repeat & 0x1000 != 0,
                        repeat & 0x4000 != 0,
                    ),
                );
                nav = match r {
                    CursorNav::None => 0,
                    CursorNav::Confirm => 1,
                    CursorNav::Cancel => 2,
                    CursorNav::Moved => 3,
                };
                port_log.nav_cues.extend(r.sfx_cue().map(u32::from));
            }
            let fx = port.tick(
                SubScreenInput {
                    script_busy: busy != 0,
                    nav,
                    cursor: (cursor & 0xFFF) as u16,
                    card_done: card_done != 0,
                    sellable_items_available: sellable,
                    spinner_result: spinner as u8,
                    ..Default::default()
                },
                0,
            );
            let (mut cleared, mut read, mut installed) = (false, false, false);
            let mut quantity_cleared = false;
            for e in fx {
                match e {
                    SubScreenEffect::RunScript => port_log.scripts += 1,
                    // The exit row's two calls: the cue entry with `0`, then
                    // the second routine with `0x37` (the port names the one).
                    SubScreenEffect::Sfx(0x37) => {
                        port_log.cues.push(0);
                        port_log.nav_cues.push(0x37);
                    }
                    SubScreenEffect::Sfx(c) => port_log.cues.push(u32::from(c)),
                    SubScreenEffect::CardOp(op) => port_log
                        .card_ops
                        .push((1, if op == CardOp::Save { 1 } else { 2 })),
                    SubScreenEffect::ClearStaging => cleared = true,
                    SubScreenEffect::ClearQuantity => quantity_cleared = true,
                    SubScreenEffect::ReadInventoryEntry => read = true,
                    SubScreenEffect::InstallCardHandle => installed = true,
                    other => panic!("{what}: {other:?}"),
                }
            }

            let l = log.borrow();
            assert_eq!(l.scripts, port_log.scripts, "{what}: scripts");
            assert_eq!(l.cues, port_log.cues, "{what}: cues");
            assert_eq!(l.nav_cues, port_log.nav_cues, "{what}: list cues");
            assert_eq!(l.card_ops, port_log.card_ops, "{what}: card ops");
            assert_eq!(
                cpu.mem.rd32(cursor_at) & 0xFFF,
                cursor & 0xFFF,
                "{what}: cursor"
            );
            assert_eq!(
                cpu.mem.rd32(STAGED_B) == 0,
                cleared,
                "{what}: staging cleared"
            );
            if cleared {
                assert_eq!(cpu.mem.rd32(STAGED), 0, "{what}");
                assert_eq!(cpu.mem.rd32(STAGED_C), 0, "{what}");
                assert_eq!(cpu.mem.rd32(LIST_RESULT), 4, "{what}");
                assert_eq!(cpu.mem.rd32(STAGED_ID), 0, "{what}");
                assert_eq!(cpu.mem.rd32(STAGED_COUNT), 0, "{what}");
            }
            if quantity_cleared {
                assert_eq!(cpu.mem.rd32(STAGED), staged_slot, "{what}");
                assert_eq!(cpu.mem.rd32(LIST_RESULT), 4, "{what}");
                assert_eq!(cpu.mem.rd32(STAGED_ID), 0, "{what}");
                assert_eq!(cpu.mem.rd32(STAGED_COUNT), 0, "{what}");
            }
            if read {
                let a = BAG + 2 * staged_slot;
                assert_eq!(cpu.mem.rd32(STAGED_ID), u32::from(cpu.mem.rd8(a)), "{what}");
                assert_eq!(
                    cpu.mem.rd32(STAGED_COUNT),
                    u32::from(cpu.mem.rd8(a + 1)),
                    "{what}"
                );
                settles += u32::from(step_before == 1);
            } else if !cleared && !quantity_cleared {
                assert_eq!(cpu.mem.rd32(STAGED_ID), SENTINEL, "{what}: staged id");
            }
            assert_eq!(
                cpu.mem.rd32(CARD_HANDLE) != SENTINEL,
                installed,
                "{what}: card handle"
            );
            refused_sells += u32::from(l.cues.contains(&0x23));
            let left = cpu.mem.rd32(SCREEN);
            assert_eq!(left, u32::from(port.screen().id()), "{what}: screen");
            if left != u32::from(screen.id()) {
                ends[k] += 1;
                break;
            }
            assert_eq!(cpu.mem.rd32(STEP), u32::from(port.step()), "{what}: step");
            drop(l);
            *log.borrow_mut() = Log::default();
            port_log = Log::default();
        }
    }
    assert!(ends.iter().all(|&e| e > 500), "{ends:?}");
    assert!(
        refused_sells > 50 && settles > 200,
        "{refused_sells} {settles}"
    );
}

#[test]
fn the_buy_list_routes_a_confirmed_row_as_retail_routes_it() {
    differential!(prot(899, BUY_LIST));
    let Some(Rig { mut cpu, log, .. }) = rig() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x4255_594C);
    let mut seen = [0u32; 4];
    for case in 0..20000 {
        let id = rng.below(256);
        let record = ITEM_TABLE + id * 12;
        let kind = [0u8, 1, 2, 3, 0xFF][rng.below(5) as usize];
        let price = [0u16, 1, 50, 999, 0x7FFF, 0xFFFF][rng.below(6) as usize]
            .wrapping_add(rng.below(3) as u16);
        cpu.mem.wr8(record, kind);
        cpu.mem.wr16(record + 2, price);
        let gold = match rng.below(3) {
            0 => u32::from(price),
            1 => u32::from(price).wrapping_sub(1),
            _ => rng.below(10_000_000),
        };
        cpu.mem.wr32(GOLD, gold);
        cpu.mem.wr32(STAGED, id);
        cpu.mem.wr32(LIST_RESULT, 2);
        cpu.mem.wr32(STEP, 2);
        cpu.mem.wr32(SCREEN, 0x1B);
        *log.borrow_mut() = Log::default();
        assert_eq!(cpu.call(BUY_LIST, &[]), Stop::Returned, "case {case}");
        let route = buy_list_confirm_route(kind, gold as i32, price);
        let screen = cpu.mem.rd32(SCREEN);
        let l = log.borrow();
        let what = format!("case {case} kind {kind} gold {gold} price {price}");
        match route {
            BuyListRoute::Refused => {
                assert_eq!(screen, 0x1B, "{what}");
                assert_eq!(l.cues, [0x23], "{what}");
                assert_eq!(cpu.mem.rd32(LIST_RESULT), 1, "{what}: list re-armed");
            }
            BuyListRoute::RecipientPicker => assert_eq!(screen, 0x1C, "{what}"),
            BuyListRoute::QuantityPicker => assert_eq!(screen, 0x1D, "{what}"),
            BuyListRoute::ModeSelect => assert_eq!(screen, 0x1A, "{what}"),
        }
        if route != BuyListRoute::Refused {
            assert!(l.cues.is_empty(), "{what}");
        }
        assert_eq!(cpu.mem.rd32(STAGED_ID), id, "{what}: staged item");
        assert_eq!(cpu.mem.rd32(STAGED_COUNT), 1, "{what}: quantity seeded");
        seen[route as usize] += 1;
    }
    assert!(seen.iter().all(|&n| n > 1000), "{seen:?}");
}

const RECIPIENT: u32 = 0x801D_B380;
const GIVE: u32 = 0x8004_21D4;
const HELD_COUNT: u32 = 0x8004_2F4C;
const AGGREGATE: u32 = 0x8004_2558;
const ROSTER: u32 = 0x8008_4598;
const RECORDS: u32 = 0x8008_4708;
const POINTS: u32 = 0x8008_45B4;
const RECIPIENT_CURSOR: u32 = 0x801E_46C0;
const PAD_CONFIRM: u32 = 0x8008_46D0;
const PAD_CANCEL: u32 = 0x8008_46D4;

#[test]
fn the_buy_recipient_picker_steps_as_retail_steps() {
    use legaia_engine_menus::shop::{BuyRecipientEvent, BuyRecipientSession, apply_point_card};
    differential!(prot(899, RECIPIENT));
    let Some(Rig { mut cpu, log, .. }) = rig() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let scus = crate::common::scus().unwrap();
    let stats = legaia_game_tables::equip_stats::EquipStatTable::from_scus(&scus).expect("equip");
    let info = legaia_engine_menus::equipment::DiscEquipInfo::from_disc(&stats);
    let given: Rc<RefCell<Vec<(u32, u32)>>> = Rc::default();
    let g = given.clone();
    cpu.hook(GIVE, move |r, _| g.borrow_mut().push((r.r[A0], r.r[A1])));
    let held = Rc::new(RefCell::new(0u32));
    let h = held.clone();
    cpu.hook(HELD_COUNT, move |r, _| {
        r.r[V0] = if r.r[A0] == 0xFE { *h.borrow() } else { 0 }
    });
    cpu.stub(AGGREGATE, 0);
    let gear: Vec<u32> = (1..256u32)
        .filter(|&id| cpu.mem.rd8(ITEM_TABLE + id * 12) == 1)
        .collect();
    let mut rng = Rng(0x5243_5054);
    let (mut bags, mut equips, mut buzzes, mut toasts, mut cancels) = (0, 0, 0, 0, 0);
    for episode in 0..6000 {
        let n = 1 + rng.below(3);
        let mut pool = vec![0u32, 1, 2];
        let ids: Vec<u32> = (0..n)
            .map(|_| pool.remove(rng.below(pool.len() as u32) as usize))
            .collect();
        cpu.mem.wr8(PARTY_COUNT, n as u8);
        for (i, &id) in ids.iter().enumerate() {
            cpu.mem.wr8(ROSTER + i as u32, id as u8);
            for s in 0..8 {
                let worn =
                    [0u8, gear[rng.below(gear.len() as u32) as usize] as u8][rng.below(2) as usize];
                cpu.mem.wr8(RECORDS + id * 0x414 + 0x196 + s, worn);
            }
        }
        let item = gear[rng.below(gear.len() as u32) as usize];
        let price = u32::from(cpu.mem.rd16(ITEM_TABLE + item * 12 + 2));
        cpu.mem.wr32(STAGED_ID, item);
        let gold = price + rng.below(100_000);
        cpu.mem.wr32(GOLD, gold);
        let points = [0, 9_999_990, rng.below(1_000_000)][rng.below(3) as usize];
        cpu.mem.wr32(POINTS, points);
        *held.borrow_mut() = [0, 0, 1][rng.below(3) as usize];
        let card = *held.borrow() != 0;
        cpu.mem.wr32(PAD_CONFIRM, 0x40);
        cpu.mem.wr32(PAD_CANCEL, 0x20);
        cpu.mem.wr32(LIST_CONFIRM_MASK, 0x40);
        cpu.mem.wr32(LIST_CANCEL_MASK, 0x20);
        let can_equip: Vec<bool> = ids
            .iter()
            .map(|&ch| info.can_equip(item as u8, ch as u8))
            .collect();
        let mut port = BuyRecipientSession::new(item as u8, price as u16, can_equip.clone(), card);
        cpu.mem.wr32(SCREEN, 0x1C);
        cpu.mem.wr32(STEP, 0);
        cpu.mem.wr32(BUSY, 0);
        cpu.mem.wr32(RECIPIENT_CURSOR, 0xABC);
        assert_eq!(cpu.call(RECIPIENT, &[]), Stop::Returned);
        port.tick(NavButtons::new(false, false, false, false));
        assert_eq!(
            cpu.mem.rd32(RECIPIENT_CURSOR),
            0,
            "episode {episode}: the hand opens on the bag"
        );
        let mut points_now = points;
        for frame in 0..30 {
            let edge = edge_word(&mut rng);
            let repeat = [0, 0x1000, 0x4000][rng.below(3) as usize];
            cpu.mem.wr32(EDGE, edge);
            cpu.mem.wr32(REPEAT, repeat);
            cpu.mem.wr32(BUSY, 0);
            *log.borrow_mut() = Log::default();
            given.borrow_mut().clear();
            let before: Vec<Vec<u8>> = ids
                .iter()
                .map(|&id| cpu.mem.read(RECORDS + id * 0x414 + 0x196, 8))
                .collect();
            let gold_before = cpu.mem.rd32(GOLD);
            assert_eq!(cpu.call(RECIPIENT, &[]), Stop::Returned);
            let ev = port.tick(NavButtons::new(
                edge & 0x40 != 0,
                edge & 0x20 != 0,
                repeat & 0x1000 != 0,
                repeat & 0x4000 != 0,
            ));
            let what =
                format!("episode {episode} frame {frame} item {item:#x} card {card} ev {ev:?}");
            let cues: Vec<u32> = log
                .borrow()
                .cues
                .iter()
                .copied()
                .filter(|&c| c != 0)
                .collect();
            let want_cue = legaia_engine_menus::shop_cues::recipient_ui_cue(&ev)
                .filter(|c| ![0x21, 0x37].contains(c))
                .map(u32::from);
            assert_eq!(
                cues,
                want_cue.into_iter().collect::<Vec<_>>(),
                "{what}: cues"
            );
            let gold_now = cpu.mem.rd32(GOLD);
            match ev {
                BuyRecipientEvent::BoughtToBag {
                    cost, point_credit, ..
                } => {
                    assert_eq!(*given.borrow(), [(item, 1)], "{what}");
                    assert_eq!(gold_before - gold_now, cost as u32, "{what}");
                    points_now = apply_point_card(points_now as i32, point_credit) as u32;
                    bags += 1;
                }
                BuyRecipientEvent::BoughtAndEquipped {
                    party_index,
                    cost,
                    point_credit,
                    ..
                } => {
                    let id = ids[usize::from(party_index)];
                    let after = cpu.mem.read(RECORDS + id * 0x414 + 0x196, 8);
                    let changed: Vec<usize> = (0..8)
                        .filter(|&s| after[s] != before[usize::from(party_index)][s])
                        .collect();
                    assert!(after.contains(&(item as u8)), "{what}: equipped");
                    assert!(changed.len() <= 1, "{what}");
                    assert_eq!(gold_before - gold_now, cost as u32, "{what}");
                    points_now = apply_point_card(points_now as i32, point_credit) as u32;
                    equips += 1;
                }
                BuyRecipientEvent::Buzz => buzzes += 1,
                _ => assert_eq!(gold_now, gold_before, "{what}"),
            }
            assert_eq!(cpu.mem.rd32(POINTS), points_now, "{what}: points");
            let screen = cpu.mem.rd32(SCREEN);
            if port.is_done() {
                assert_eq!(screen, 0x1B, "{what}: back to the buy list");
                if ev == BuyRecipientEvent::Cancelled {
                    cancels += 1;
                } else if ev == BuyRecipientEvent::ExitToBuyList {
                    toasts += 1;
                }
                break;
            }
            assert_eq!(screen, 0x1C, "{what}: still picking");
            assert_eq!(
                cpu.mem.rd32(RECIPIENT_CURSOR) & 0xFFF,
                port.cursor & 0xFFF,
                "{what}"
            );
        }
    }
    assert!(
        bags > 300 && equips > 300 && buzzes > 300 && toasts > 100 && cancels > 300,
        "{bags} {equips} {buzzes} {toasts} {cancels}"
    );
}
