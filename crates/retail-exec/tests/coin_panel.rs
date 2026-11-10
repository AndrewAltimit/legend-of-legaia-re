//! The casino coin counter's entry panel `FUN_801E6F70` (PROT 0897) against
//! `slot_machine::coin_entry_panel`: every label, number and caret draw in
//! order, each with the pen `_DAT_8007B454` held when it is called, over
//! generated digits, cursors, bank, gold, ceiling and play clock. That covers
//! the entry accumulator, the quote's two gates (the refused pen) and the
//! caret's blink and bound.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_minigames::slot_machine::{CoinPanelDraw, CoinPanelInput, coin_entry_panel};
use psx_retail_exec::{A0, A1, A2, A3, Stop};
use std::cell::RefCell;
use std::rc::Rc;

const OVERLAY_BASE: u32 = 0x801C_E818;
const PANEL: u32 = 0x801E_6F70;
const LABEL: u32 = 0x8003_CD00;
const NUMBER: u32 = 0x8003_4B78;
const CELL: u32 = 0x8002_C488;
const PEN: u32 = 0x8007_B454;
const DIGITS: u32 = 0x801F_35F0;
const CURSOR: u32 = 0x8007_BB9C;
const BANK: u32 = 0x8008_45A4;
const GOLD: u32 = 0x8008_459C;
const STOCK: u32 = 0x8007_BB90;
const CLOCK: u32 = 0x8008_4570;
const WINDOW: u32 = 0x8012_0000;

#[test]
fn the_entry_panel_draws_what_retail_draws() {
    differential!(prot(897, PANEL));
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &d.read_entry(897).expect("PROT 0897"));
    let log: Rc<RefCell<Vec<CoinPanelDraw>>> = Rc::new(RefCell::new(Vec::new()));
    let l = log.clone();
    cpu.hook(LABEL, move |r, m| {
        l.borrow_mut().push(CoinPanelDraw::Label {
            va: r.r[A0],
            x: r.r[A1] as i16,
            y: r.r[A2] as i16,
            pen: m.rd32(PEN) as u8,
        })
    });
    let l = log.clone();
    cpu.hook(NUMBER, move |r, m| {
        l.borrow_mut().push(CoinPanelDraw::Number {
            value: r.r[A0] as i32,
            digits: r.r[A1] as u8,
            x: r.r[A2] as i16,
            y: r.r[A3] as i16,
            pen: m.rd32(PEN) as u8,
        })
    });
    let l = log.clone();
    cpu.hook(CELL, move |r, _| {
        l.borrow_mut().push(CoinPanelDraw::Cell {
            x: r.r[A0] as i16,
            y: r.r[A1] as i16,
            cell: r.r[A2] as i32,
        })
    });
    let mut rng = Rng(0x434F_494E);
    let (mut refused, mut carets) = (0, 0);
    for run in 0..20_000 {
        let digits: Vec<i8> = (0..8)
            .map(|i| {
                if i < 6 || rng.below(4) == 0 {
                    rng.below(10) as i8
                } else {
                    0
                }
            })
            .collect();
        let cursor = [0, 3, 5, 9, 10, -1, rng.below(12) as i32][rng.below(7) as usize];
        let bank = rng.below(10_000_000) as i32;
        let gold = [0, 99, 100, rng.below(10_000_000) as i32][rng.below(4) as usize];
        let stock = [0, rng.below(1000) as i32, rng.below(100_000) as i32][rng.below(3) as usize];
        let clock = rng.next();
        let origin = (rng.below(0x100) as i16, rng.below(0xF0) as i16);
        for (i, v) in digits.iter().enumerate() {
            cpu.mem.wr8(DIGITS + i as u32, *v as u8);
        }
        cpu.mem.wr32(CURSOR, cursor as u32);
        cpu.mem.wr32(BANK, bank as u32);
        cpu.mem.wr32(GOLD, gold as u32);
        cpu.mem.wr32(STOCK, stock as u32);
        cpu.mem.wr32(CLOCK, clock);
        cpu.mem.wr16(WINDOW + 0xA, origin.0 as u16);
        cpu.mem.wr16(WINDOW + 0xC, origin.1 as u16);
        cpu.mem.wr32(PEN, 0x33);
        log.borrow_mut().clear();
        assert_eq!(cpu.call(PANEL, &[WINDOW]), Stop::Returned);
        let retail = log.borrow().clone();
        let port = coin_entry_panel(&CoinPanelInput {
            origin,
            digits: &digits,
            cursor,
            bank,
            gold,
            stock,
            clock,
        });
        assert_eq!(
            retail, port,
            "run {run} cursor {cursor} gold {gold} stock {stock}"
        );
        assert_eq!(cpu.mem.rd32(PEN) as u8, 7, "run {run}: the pen restored");
        refused += u32::from(
            port.iter()
                .any(|d| matches!(d, CoinPanelDraw::Number { pen: 9, .. })),
        );
        carets += u32::from(port.iter().any(|d| matches!(d, CoinPanelDraw::Cell { .. })));
    }
    assert!(refused > 3000 && carets > 3000, "{refused} {carets}");
}
