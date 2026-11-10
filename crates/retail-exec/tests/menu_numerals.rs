//! The menu's number cells and ink tiers: the blank-padded and zero-padded
//! decimal writers `FUN_80034B78` / `FUN_80034E4C`, with the cell blit
//! `FUN_8003C11C` logged, against `decimal_field_cells` over values inside,
//! at and past their field, negative ones and every width; and the HP / MP
//! ink tiers `FUN_800349EC` / `FUN_80035EA8` over the record's live pairs and
//! status word against both hosts' readers (the menu ink and the battle
//! HUD's colour index).

use crate::common::{Rng, scus_cpu};
use legaia_engine_ui::menu::decimal_field::decimal_field_cells;
use psx_retail_exec::{A0, A1, Stop, V0};
use std::cell::RefCell;
use std::rc::Rc;

/// One logged cell: pen `(x, y)` and the cell `(u, v, w, h)`.
type Cell = (i16, i16, u16, u16, u16, u16);

const BLANK: u32 = 0x8003_4B78;
const ZERO: u32 = 0x8003_4E4C;
const CELL: u32 = 0x8003_C11C;
const HP_INK: u32 = 0x8003_49EC;
const MP_INK: u32 = 0x8003_5EA8;
/// `gp + 0x15C`, the writers' latch.
const LATCH: u32 = 0x8007_B474;
const RECORDS: u32 = 0x8008_4708;

fn value(rng: &mut Rng) -> i32 {
    match rng.below(6) {
        0 => rng.below(10) as i32,
        1 => rng.below(1000) as i32,
        2 => rng.below(100_000) as i32,
        3 => -(rng.below(100_000) as i32),
        4 => rng.next() as i32,
        _ => [0, 9, 10, 99, 100, 999, 1000, 9999, 10_203, 99_999_999][rng.below(10) as usize],
    }
}

#[test]
fn the_decimal_writers_draw_the_cells_retail_draws() {
    differential!(scus(BLANK), scus(ZERO));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let log: Rc<RefCell<Vec<Cell>>> = Rc::default();
    let l = log.clone();
    cpu.hook(CELL, move |r, m| {
        let (p, q) = (r.r[A0], r.r[A1]);
        l.borrow_mut().push((
            m.rd16(p) as i16,
            m.rd16(p + 2) as i16,
            m.rd16(q),
            m.rd16(q + 2),
            m.rd16(q + 4),
            m.rd16(q + 6),
        ));
    });
    let mut rng = Rng(0x4E55_4D53);
    let mut overflows = 0;
    for run in 0..40_000 {
        let v = value(&mut rng);
        let digits = rng.below(11) as i32;
        let zero_pad = rng.below(2) == 0;
        let (x, y) = (rng.below(0x140) as i16, rng.below(0xF0) as i16);
        log.borrow_mut().clear();
        cpu.mem.wr32(LATCH, 0);
        let entry = if zero_pad { ZERO } else { BLANK };
        assert_eq!(
            cpu.call(entry, &[v as u32, digits as u32, x as u32, y as u32]),
            Stop::Returned
        );
        assert_eq!(cpu.mem.rd32(LATCH), 0, "the latch is left clear");
        let retail: Vec<(i32, u8)> = log
            .borrow()
            .iter()
            .map(|&(cx, cy, u, v, w, h)| {
                assert_eq!((cy, v, w, h), (y, 0xD0, 8, 12), "run {run}");
                (i32::from(cx) - i32::from(x), (u / 8) as u8)
            })
            .collect();
        let port = decimal_field_cells(v, digits, zero_pad);
        assert_eq!(retail, port, "run {run}: {v} in {digits}, zero {zero_pad}");
        overflows += u32::from(digits > 0 && v > 0 && v.to_string().len() as i32 > digits);
    }
    assert!(overflows > 5000, "{overflows}");
}

#[test]
fn the_ink_tiers_match_both_readers() {
    differential!(scus(HP_INK), scus(MP_INK));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x494E_4B53);
    let mut seen = [0u32; 10];
    for run in 0..40_000 {
        let ch = rng.below(4);
        let rec = RECORDS + ch * 0x414;
        let max = [rng.below(10_000), rng.below(8), 0xFFFF, rng.next() & 0xFFFF]
            [rng.below(4) as usize] as u16;
        let cur = match rng.below(4) {
            0 => 0,
            1 => max,
            2 => max / [2u16, 4][rng.below(2) as usize] + rng.below(3) as u16,
            _ => rng.below(u32::from(max) + 2) as u16,
        };
        let status = [0u16, 0, 1, 0x8000, rng.next() as u16][rng.below(5) as usize];
        cpu.mem.wr16(rec + 0x104, max);
        cpu.mem.wr16(rec + 0x106, cur);
        cpu.mem.wr16(rec + 0x108, max);
        cpu.mem.wr16(rec + 0x10A, cur);
        cpu.mem.wr16(rec + 0x12E, status);
        assert_eq!(cpu.call(HP_INK, &[ch]), Stop::Returned);
        let hp = cpu.regs.r[V0] as u8;
        assert_eq!(cpu.call(MP_INK, &[ch]), Stop::Returned);
        let mp = cpu.regs.r[V0] as u8;
        let what = format!("run {run}: {cur}/{max} status {status:#x}");
        assert_eq!(
            hp,
            legaia_engine_ui::hp_bar_color_index(cur, max, status != 0),
            "{what}"
        );
        assert_eq!(mp, legaia_engine_ui::mp_bar_color_index(cur, max), "{what}");
        assert_eq!(
            legaia_engine_ui::menu_hp_ink_with_status(cur, max, status as i16),
            legaia_engine_ui::menu_ink_color(hp),
            "{what}"
        );
        assert_eq!(
            legaia_engine_ui::menu_mp_ink(cur, max),
            legaia_engine_ui::menu_ink_color(mp),
            "{what}"
        );
        seen[usize::from(hp)] += 1;
        seen[usize::from(mp)] += 1;
    }
    assert!([2, 6, 7, 9].iter().all(|&i| seen[i] > 1000), "{seen:?}");
}
