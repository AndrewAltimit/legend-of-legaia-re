//! The fishing point exchange in PROT 0972 against `engine-fishing`: the prize
//! row's availability `FUN_801D6F90` (affordable, owned count not at the cap,
//! a one-time row not latched in the purchased mask) against
//! `PrizeExchange::is_available`, the list cursor `FUN_801D0C3C`, the
//! quantity cap `FUN_801D092C` and the confirm's grant `FUN_801D06C8`; and
//! the reel-button decoder `FUN_801D7450` against `ReelInput::from_pad_mask`.
//!
//! The availability reads the venue's row table through `0x801D90B8`, the
//! point pool `0x8008444C`, the purchased mask `0x8008446C` and the venue
//! `0x801D90D0`; the owned count comes from the bag counter `FUN_80042F4C`,
//! hooked to answer from the script.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_fishing::fishing::{PrizeExchange, PrizeRow, ReelInput};
use legaia_minigame_formats::fishing_species as fs;
use psx_retail_exec::{A0, Stop, V0};
use std::cell::Cell;
use std::rc::Rc;

const AVAILABLE: u32 = 0x801D_6F90;
const DECODE: u32 = 0x801D_7450;
const BAG_COUNT: u32 = 0x8004_2F4C;
const TABLE_PTR: u32 = 0x801D_90B8;
const VENUE: u32 = 0x801D_90D0;
const POINTS: u32 = 0x8008_444C;
const MASK: u32 = 0x8008_446C;
const TABLE: u32 = 0x8012_0000;

#[test]
fn prize_availability_matches_retail() {
    differential!(prot(972, AVAILABLE));
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let overlay = d
        .read_entry(fs::FISHING_OVERLAY_PROT_INDEX)
        .expect("PROT 0972");
    cpu.mem.load(fs::FISHING_OVERLAY_BASE_VA, &overlay);
    let owned = Rc::new(Cell::new(0u32));
    let asked = Rc::new(Cell::new(0u32));
    let (o, a) = (owned.clone(), asked.clone());
    cpu.hook(BAG_COUNT, move |r, _| {
        a.set(r.r[A0]);
        r.r[V0] = o.get();
    });
    cpu.mem.wr32(TABLE_PTR, TABLE);
    let mut rng = Rng(0x5052_495A);
    let (mut yes, mut no) = (0, 0);
    for run in 0..100_000 {
        let venue = rng.below(2) as usize;
        let rows: Vec<PrizeRow> = (0..6)
            .map(|row| PrizeRow {
                row,
                limit: [1, 99, 1 + rng.below(99)][rng.below(3) as usize],
                price: [0, 1, 50, 300, 1000, 5000, rng.below(10_000)][rng.below(7) as usize],
                item_id: rng.next() as u8,
                name: None,
            })
            .collect();
        for r in &rows {
            let at = TABLE + r.row as u32 * 12;
            cpu.mem.wr32(at, r.limit);
            cpu.mem.wr32(at + 4, r.price);
            cpu.mem.wr32(at + 8, u32::from(r.item_id));
        }
        let ex = PrizeExchange {
            venue,
            rows: rows.clone(),
            cursor: 0,
        };
        let row = rng.below(6) as usize;
        let points = [0, 49, 50, 999, 1000, 1001, rng.below(20_000) as i32][rng.below(7) as usize];
        let own = [0, 1, 98, 99, rng.below(100)][rng.below(5) as usize];
        let mask = rng.next() & rng.next();
        cpu.mem.wr32(VENUE, venue as u32);
        cpu.mem.wr32(POINTS, points as u32);
        cpu.mem.wr32(MASK, mask);
        owned.set(own);
        assert_eq!(cpu.call(AVAILABLE, &[row as u32]), Stop::Returned);
        let retail = cpu.regs.r[V0] != 0;
        let port = ex.is_available(row, points, own, mask);
        assert_eq!(
            retail, port,
            "run {run} venue {venue} row {row} price {} points {points} owned {own} mask {mask:#x}",
            rows[row].price
        );
        if rows[row].price <= points as u32 {
            assert_eq!(
                asked.get(),
                u32::from(rows[row].item_id),
                "the bag count asked"
            );
        }
        yes += u32::from(port);
        no += u32::from(!port);
    }
    assert!(yes > 15_000 && no > 15_000, "{yes} {no}");
}

#[test]
fn the_reel_decoder_matches_retail() {
    differential!(prot(972, DECODE));
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let overlay = d
        .read_entry(fs::FISHING_OVERLAY_PROT_INDEX)
        .expect("PROT 0972");
    cpu.mem.load(fs::FISHING_OVERLAY_BASE_VA, &overlay);
    for mask in (0..0x1_0000u32).chain([u32::MAX, 0xFFFF_0040, 0x8000_0080]) {
        assert_eq!(cpu.call(DECODE, &[mask]), Stop::Returned);
        let port = match ReelInput::from_pad_mask(mask) {
            ReelInput::Idle => 0,
            ReelInput::ReelA => 1,
            ReelInput::ReelB => 2,
        };
        assert_eq!(cpu.regs.r[V0], port, "mask {mask:#x}");
    }
}

/// The exchange screens' shared seat: the overlay in, the venue's six rows
/// at [`TABLE`], the text and box draws stubbed, the bag counter answering
/// `owned`, and the number draw `FUN_80034B78` recorded.
struct Screens {
    cpu: psx_retail_exec::Cpu,
    owned: Rc<Cell<u32>>,
    numbers: Rc<std::cell::RefCell<Vec<i32>>>,
    grants: Rc<std::cell::RefCell<Vec<(u32, u32)>>>,
}

const ROW: u32 = 0x801D_90D4;
const QTY: u32 = 0x801D_90B0;
const CONFIRM: u32 = 0x801D_90B4;
const CURSOR: u32 = 0x801D_90A8;
const SCREEN_PAD: u32 = 0x801D_90D8;
const PAD_EDGE: u32 = 0x8007_B874;
const LIST: u32 = 0x801D_0C3C;
const QUANTITY: u32 = 0x801D_092C;
const YES: u32 = 0x801D_06C8;

fn screens() -> Option<Screens> {
    let (mut cpu, d) = (scus_cpu()?, disc()?);
    let overlay = d
        .read_entry(fs::FISHING_OVERLAY_PROT_INDEX)
        .expect("PROT 0972");
    cpu.mem.load(fs::FISHING_OVERLAY_BASE_VA, &overlay);
    for va in [
        0x8003_6888,
        0x8002_C488,
        0x801D_74B0,
        0x8003_CBF8,
        0x8003_C1F8,
    ] {
        cpu.stub(va, 0);
    }
    let owned = Rc::new(Cell::new(0u32));
    let o = owned.clone();
    cpu.hook(BAG_COUNT, move |r, _| r.r[V0] = o.get());
    let numbers = Rc::new(std::cell::RefCell::new(Vec::new()));
    let n = numbers.clone();
    cpu.hook(0x8003_4B78, move |r, _| n.borrow_mut().push(r.r[A0] as i32));
    let grants = Rc::new(std::cell::RefCell::new(Vec::new()));
    let g = grants.clone();
    cpu.hook(0x8004_21D4, move |r, _| {
        g.borrow_mut().push((r.r[A0], r.r[psx_retail_exec::A1]))
    });
    cpu.mem.wr32(TABLE_PTR, TABLE);
    Some(Screens {
        cpu,
        owned,
        numbers,
        grants,
    })
}

fn seat_page(cpu: &mut psx_retail_exec::Cpu, rng: &mut Rng) -> PrizeExchange {
    let venue = rng.below(2) as usize;
    let rows: Vec<PrizeRow> = (0..6)
        .map(|row| PrizeRow {
            row,
            limit: [1, 99, 1 + rng.below(99)][rng.below(3) as usize],
            price: [1, 50, 300, 1000, 5000, 1 + rng.below(10_000)][rng.below(6) as usize],
            item_id: rng.next() as u8,
            name: None,
        })
        .collect();
    for r in &rows {
        let at = TABLE + r.row as u32 * 12;
        cpu.mem.wr32(at, r.limit);
        cpu.mem.wr32(at + 4, r.price);
        cpu.mem.wr32(at + 8, u32::from(r.item_id));
    }
    cpu.mem.wr32(VENUE, venue as u32);
    PrizeExchange {
        venue,
        rows,
        cursor: 0,
    }
}

/// The list `FUN_801D0C3C`: the cursor floor (row 0 hidden until strictly
/// affordable) and the cursor's wrap at both ends, against
/// `PrizeExchange::first_visible` and `step_cursor`. No confirm is pressed.
#[test]
fn the_prize_list_cursor_moves_as_retails_does() {
    differential!(prot(972, LIST));
    let Some(mut s) = screens() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x4C49_5354);
    let mut wrapped = 0;
    for run in 0..50_000 {
        let ex = seat_page(&mut s.cpu, &mut rng);
        let price0 = ex.rows[0].price as i32;
        let points = [0, price0 - 1, price0, price0 + 1, rng.below(20_000) as i32]
            [rng.below(5) as usize]
            .max(0);
        // Now and then a cursor under the floor, as a purchase leaves row 0
        // when it takes the points below its price.
        let cursor = if rng.below(6) == 0 {
            0
        } else {
            ex.first_visible(points) + rng.below(6 - ex.first_visible(points) as u32) as usize
        };
        let pad = [0, 0x1000, 0x4000, 0x5000][rng.below(4) as usize];
        s.cpu.mem.wr32(POINTS, points as u32);
        s.cpu.mem.wr32(CURSOR, cursor as u32);
        s.cpu.mem.wr32(SCREEN_PAD, pad);
        s.cpu.mem.wr32(PAD_EDGE, 0);
        s.owned.set(rng.below(100));
        assert_eq!(s.cpu.call(LIST, &[1]), Stop::Returned);
        let retail = s.cpu.mem.rd32(CURSOR) as usize;
        let port = ex.step_cursor(cursor, points, pad & 0x1000 != 0, pad & 0x4000 != 0);
        assert_eq!(
            retail, port,
            "run {run} cursor {cursor} pad {pad:#x} points {points} price0 {price0}"
        );
        wrapped += u32::from(retail.abs_diff(cursor) > 1);
    }
    assert!(wrapped > 1000, "{wrapped}");
}

/// The quantity picker `FUN_801D092C` with input off: the cap it draws as the
/// second number (`FUN_80034B78(max, ..)`) against `PrizeExchange::max_qty`.
#[test]
fn the_quantity_cap_is_retails() {
    differential!(prot(972, QUANTITY));
    let Some(mut s) = screens() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x5154_5943);
    let (mut by_points, mut by_limit) = (0, 0);
    for run in 0..50_000 {
        let ex = seat_page(&mut s.cpu, &mut rng);
        let row = rng.below(6) as usize;
        let points = rng.below(50_000) as i32;
        let mask = rng.next() & rng.next();
        let own = [0, 1, 98, 99, rng.below(100)][rng.below(5) as usize];
        s.cpu.mem.wr32(POINTS, points as u32);
        s.cpu.mem.wr32(MASK, mask);
        s.cpu.mem.wr32(ROW, row as u32);
        s.cpu.mem.wr32(QTY, 1);
        s.owned.set(own);
        s.numbers.borrow_mut().clear();
        assert_eq!(s.cpu.call(QUANTITY, &[0]), Stop::Returned);
        // A latched one-time row whose item the bag holds more than once
        // caps at `limit - owned`, below zero; the list never opens the
        // picker on a latched row (`FUN_801D6F90` refuses it).
        let retail = s.numbers.borrow()[1].max(0) as u32;
        let port = ex.max_qty(row, points, own, mask);
        assert_eq!(
            retail, port,
            "run {run} row {row} points {points} owned {own} mask {mask:#x} {:?}",
            ex.rows[row]
        );
        let r = &ex.rows[row];
        by_points += u32::from(port > 0 && port == points as u32 / r.price);
        by_limit += u32::from(port > 0 && port < points as u32 / r.price);
    }
    assert!(
        by_points > 1000 && by_limit > 1000,
        "{by_points} {by_limit}"
    );
}

/// The confirm's Yes arm `FUN_801D06C8` on a purchase the list and the
/// picker allow: the grant, the points spent and the one-time latch, against
/// `PrizeExchange::buy`.
#[test]
fn a_confirmed_prize_is_granted_as_retail_grants_it() {
    differential!(prot(972, YES));
    let Some(mut s) = screens() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x5945_5321);
    let (mut bought, mut latched) = (0, 0);
    for run in 0..50_000 {
        let ex = seat_page(&mut s.cpu, &mut rng);
        let row = rng.below(6) as usize;
        let points = rng.below(100_000) as i32;
        let mask = rng.next() & rng.next();
        let own = [0, 1, 50, rng.below(99)][rng.below(4) as usize];
        let max = ex.max_qty(row, points, own, mask);
        if max == 0 || !ex.is_available(row, points, own, mask) {
            continue;
        }
        let qty = 1 + rng.below(max);
        s.cpu.mem.wr32(POINTS, points as u32);
        s.cpu.mem.wr32(MASK, mask);
        s.cpu.mem.wr32(ROW, row as u32);
        s.cpu.mem.wr32(QTY, qty);
        s.cpu.mem.wr32(CONFIRM, 0);
        s.cpu.mem.wr32(SCREEN_PAD, 0);
        s.cpu.mem.wr32(PAD_EDGE, 0x40);
        s.owned.set(own);
        s.grants.borrow_mut().clear();
        assert_eq!(s.cpu.call(YES, &[]), Stop::Returned);
        let p = ex.buy(row, qty, points, own, mask).expect("an allowed buy");
        let retail = (
            s.grants.borrow().clone(),
            s.cpu.mem.rd32(POINTS) as i32,
            s.cpu.mem.rd32(MASK),
        );
        let port = (
            vec![(u32::from(p.item_id), p.qty)],
            points - p.cost as i32,
            mask | p.latched_bit.map_or(0, |b| 1 << b),
        );
        assert_eq!(retail, port, "run {run} row {row} qty {qty}");
        bought += 1;
        latched += u32::from(p.latched_bit.is_some());
    }
    assert!(bought > 10_000 && latched > 1000, "{bought} {latched}");
}

/// What one exchange screen call drew: text by string and pen, decimal
/// fields `(value, places, x, y)`, separator glyphs `(glyph, x, y)`, icons
/// `(x, y, icon)` and panels `(cx, y, w, h)`.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct Drawn {
    texts: Vec<(u32, i32, i32)>,
    numbers: Vec<(i64, i32, i32, i32)>,
    glyphs: Vec<(u32, i32, i32)>,
    icons: Vec<(i32, i32, u32)>,
    panels: Vec<(i32, i32, i32, i32)>,
}

/// The three exchange screens as the session runs them (`0x801D0028..
/// 0x801D0080`): the live one with its input on, the picker under the
/// confirm with it off; frame by frame over random pads from random picker
/// words, against `PrizeExchange::pad_step` and `dialog_draw`: the state
/// (`0x78` list, `0x79` picker, `0x7A` confirm, `0x64` back to the hub), the
/// list cursor, the picked row, the quantity, the confirm's row, the cue
/// `0x8007B6D8`, the grant, the points and the one-time latch, and every
/// text pen, number field, glyph, cursor icon and panel the picker and the
/// confirm draw.
#[test]
fn the_exchange_picker_and_confirm_step_and_draw_as_retail_does() {
    differential!(prot(972, LIST), prot(972, QUANTITY), prot(972, YES));
    use legaia_engine_fishing::fishing::{PrizeDialog, PrizeScreen, PrizeText};
    use psx_retail_exec::{A1, A2, A3, SP};
    use std::cell::RefCell;
    let Some(mut s) = screens() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    const STATE: u32 = 0x801D_926C;
    const CUE: u32 = 0x8007_B6D8;
    const NAME_BUF: u32 = 0x801D_7E58;
    let drawn = Rc::new(RefCell::new(Drawn::default()));
    {
        let d = drawn.clone();
        s.cpu.hook(0x8003_6888, move |r, m| {
            let y = m.rd32(r.r[SP] + 0x10) as i32;
            d.borrow_mut().texts.push((r.r[A0], r.r[A3] as i32, y));
        });
        let d = drawn.clone();
        s.cpu.hook(0x8003_4B78, move |r, _| {
            d.borrow_mut().numbers.push((
                i64::from(r.r[A0] as i32),
                r.r[A1] as i32,
                r.r[A2] as i32,
                r.r[A3] as i32,
            ))
        });
        let d = drawn.clone();
        s.cpu.hook(0x8003_C1F8, move |r, _| {
            d.borrow_mut()
                .glyphs
                .push((r.r[A0], r.r[A1] as i32, r.r[A2] as i32))
        });
        let d = drawn.clone();
        s.cpu.hook(0x8002_C488, move |r, _| {
            d.borrow_mut()
                .icons
                .push((r.r[A0] as i32, r.r[A1] as i32, r.r[A2]))
        });
        let d = drawn.clone();
        s.cpu.hook(0x801D_74B0, move |r, _| {
            d.borrow_mut().panels.push((
                r.r[A0] as i32,
                r.r[A1] as i32,
                r.r[A2] as i32,
                r.r[A3] as i32,
            ))
        });
    }
    let text_va = |t: PrizeText| match t {
        PrizeText::ItemName => NAME_BUF,
        PrizeText::QuantityLabel => 0x801C_EF88,
        PrizeText::Question => 0x801C_EF6C,
        PrizeText::Yes => 0x801C_EF7C,
        PrizeText::No => 0x801C_EF80,
    };
    let mut rng = Rng(0x5049_434B);
    let mut seen = std::collections::BTreeSet::new();
    let mut bought = 0;
    for run in 0..40_000u32 {
        let mut ex = seat_page(&mut s.cpu, &mut rng);
        // Disc-shaped limits: one-time rows and stacks of 99.
        for r in ex.rows.iter_mut() {
            r.limit = if rng.below(3) == 0 { 1 } else { 99 };
            s.cpu.mem.wr32(TABLE + r.row as u32 * 12, r.limit);
        }
        let points = [0, 50, 999, 5000, rng.below(100_000) as i32][rng.below(5) as usize];
        let mask = rng.next() & rng.next();
        let own = [0, 1, 50, 98, rng.below(99)][rng.below(5) as usize];
        let screen = [
            PrizeScreen::List,
            PrizeScreen::Quantity,
            PrizeScreen::Confirm,
        ][rng.below(3) as usize];
        let row = rng.below(6) as usize;
        // The picker and the confirm only open on an available row.
        if screen != PrizeScreen::List && !ex.is_available(row, points, own, mask) {
            continue;
        }
        let max = ex.max_qty(row, points, own, mask) as i32;
        let mut dlg = PrizeDialog {
            screen,
            row,
            // The picker holds its quantity to the cap before it opens the
            // confirm; it steps it from anywhere.
            qty: if screen == PrizeScreen::Confirm {
                1 + rng.below(max.max(1) as u32) as i32
            } else {
                rng.below(max.max(1) as u32 + 3) as i32 - 1
            },
            confirm_row: rng.below(2),
        };
        let floor = ex.first_visible(points);
        ex.cursor = floor + rng.below(6 - floor as u32) as usize;
        let keys = [0u32, 0x1000, 0x4000, 0x2000, 0x8000, 0xA000][rng.below(6) as usize];
        let pressed = [0u32, 0, 0x40, 0x04, 0x20, 0x01, 0x60][rng.below(7) as usize];
        let state = match screen {
            PrizeScreen::List => 0x78,
            PrizeScreen::Quantity => 0x79,
            PrizeScreen::Confirm => 0x7A,
        };
        let m = &mut s.cpu.mem;
        m.wr32(STATE, state);
        m.wr32(POINTS, points as u32);
        m.wr32(MASK, mask);
        m.wr32(CURSOR, ex.cursor as u32);
        m.wr32(ROW, row as u32);
        m.wr32(QTY, dlg.qty as u32);
        m.wr32(CONFIRM, dlg.confirm_row);
        m.wr32(SCREEN_PAD, keys);
        m.wr32(PAD_EDGE, pressed);
        m.wr16(CUE, 0);
        s.owned.set(own);
        s.grants.borrow_mut().clear();
        *drawn.borrow_mut() = Drawn::default();
        // The session's layering: the live screen, then the picker under
        // the confirm; only the picker's and the confirm's draws are kept.
        match screen {
            PrizeScreen::List => {
                assert_eq!(s.cpu.call(LIST, &[1]), Stop::Returned);
                *drawn.borrow_mut() = Drawn::default();
            }
            PrizeScreen::Quantity => {
                assert_eq!(s.cpu.call(QUANTITY, &[1]), Stop::Returned);
            }
            PrizeScreen::Confirm => {
                assert_eq!(s.cpu.call(YES, &[1]), Stop::Returned);
                assert_eq!(s.cpu.call(QUANTITY, &[0]), Stop::Returned);
            }
        }
        let m = &mut s.cpu.mem;
        let retail = (
            m.rd32(STATE),
            m.rd32(CURSOR) as usize,
            m.rd32(ROW) as usize,
            m.rd32(QTY) as i32,
            m.rd32(CONFIRM),
            m.rd16(CUE),
            s.grants.borrow().clone(),
            m.rd32(POINTS) as i32,
            m.rd32(MASK),
        );
        let before = dlg;
        let step = ex.pad_step(&mut dlg, keys, pressed, points, |_| own, mask);
        let p = step.purchase;
        let port_state = match (step.closed, dlg.screen) {
            (true, _) => 0x64,
            (_, PrizeScreen::List) => 0x78,
            (_, PrizeScreen::Quantity) => 0x79,
            (_, PrizeScreen::Confirm) => 0x7A,
        };
        let port = (
            port_state,
            ex.cursor,
            dlg.row,
            dlg.qty,
            dlg.confirm_row,
            step.sfx.unwrap_or(0),
            p.map(|p| vec![(u32::from(p.item_id), p.qty)])
                .unwrap_or_default(),
            points - p.map_or(0, |p| p.cost as i32),
            mask | p.and_then(|p| p.latched_bit).map_or(0, |b| 1 << b),
        );
        let at = format!(
            "run {run} {before:?} keys {keys:#x} pressed {pressed:#x} points {points} owned {own} row {:?}",
            ex.rows[row]
        );
        assert_eq!(retail, port, "{at}");
        // The draws: the screen the call drew from (the words as it
        // started, but the confirm's row and the quantity its own step
        // leaves).
        if screen != PrizeScreen::List {
            let mut shown = before;
            shown.qty = retail.3;
            shown.confirm_row = if screen == PrizeScreen::Confirm {
                // The confirm draws after its own row step, before its edges.
                let mut c = before.confirm_row;
                if keys & 0x1000 != 0 {
                    c = c.wrapping_sub(1);
                }
                if keys & 0x4000 != 0 {
                    c = c.wrapping_add(1);
                }
                c & 1
            } else {
                before.confirm_row
            };
            // The picker under the confirm draws after the confirm's grant.
            let d = ex.dialog_draw(&shown, retail.7, own, retail.8);
            let got = drawn.borrow().clone();
            let mut want_texts: Vec<(u32, i32, i32)> = d
                .texts
                .iter()
                .map(|&(t, x, y)| (text_va(t), i32::from(x), i32::from(y)))
                .collect();
            let mut got_texts = got.texts.clone();
            want_texts.sort_unstable();
            got_texts.sort_unstable();
            assert_eq!(got_texts, want_texts, "{at}: the text");
            let want_numbers: Vec<_> = d
                .numbers
                .iter()
                .map(|&(v, n, x, y)| (v, n, i32::from(x), i32::from(y)))
                .collect();
            assert_eq!(got.numbers, want_numbers, "{at}: the numbers");
            assert_eq!(
                got.glyphs,
                d.slash
                    .map(|(x, y)| vec![(6, i32::from(x), i32::from(y))])
                    .unwrap_or_default(),
                "{at}: the glyph"
            );
            let mut want_icons: Vec<_> = d
                .cursors
                .iter()
                .map(|&(x, y)| (i32::from(x), i32::from(y), 0x4E))
                .collect();
            let mut got_icons = got.icons.clone();
            want_icons.sort_unstable();
            got_icons.sort_unstable();
            assert_eq!(got_icons, want_icons, "{at}: the cursors");
            let mut want_panels: Vec<_> = d
                .panels
                .iter()
                .map(|&(a, b, c, e)| (i32::from(a), i32::from(b), i32::from(c), i32::from(e)))
                .collect();
            let mut got_panels = got.panels.clone();
            want_panels.sort_unstable();
            got_panels.sort_unstable();
            assert_eq!(got_panels, want_panels, "{at}: the panels");
        }
        seen.insert((state, port_state));
        bought += u32::from(p.is_some());
    }
    assert!(seen.len() >= 8 && bought > 500, "{seen:x?} {bought}");
}
