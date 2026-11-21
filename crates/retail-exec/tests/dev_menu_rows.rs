//! The developer menu's row renderer `FUN_801EAD98` (PROT 0897) against
//! `dev_menu_row_y` and the label column: every row of every scroll window,
//! with the debug printer `FUN_8001AA68` logged and every other call outside
//! the overlay answered with zero. Each visible row's first print is its
//! label on the row's own line, at `x + 8` but for the equip pager's two
//! extra columns.

use crate::common::{Rng, STACK_TOP, disc, scus_cpu};
use legaia_engine_ui::{DEV_MENU_LABEL_COL, DEV_MENU_MAX_ROW, dev_menu_row_y};
use psx_retail_exec::{A1, A2, Stop};
use std::cell::RefCell;
use std::rc::Rc;

const OVERLAY_BASE: u32 = 0x801C_E818;
const ROWS: u32 = 0x801E_AD98;
const PRINT: u32 = 0x8001_AA68;

#[test]
fn every_row_lands_on_its_line() {
    differential!(prot(897, ROWS));
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &d.read_entry(897).expect("PROT 0897"));
    cpu.stub_outside = Some((OVERLAY_BASE, 0x8020_0000));
    let log: Rc<RefCell<Vec<(i32, i32)>>> = Rc::default();
    let l = log.clone();
    cpu.hook(PRINT, move |r, _| {
        l.borrow_mut().push((r.r[A1] as i32, r.r[A2] as i32))
    });
    cpu.mem.wr32(0x1F80_03A0, 0x8014_0000);
    cpu.mem.wr32(0x1F80_03F4, 0x8016_0000);
    let mut rng = Rng(0x4445_564D);
    let mut drawn = 0;
    for run in 0..400 {
        let (x, y) = (rng.below(0x100) as i32, rng.below(0xC0) as i32);
        let start = rng.below(0x18) as i32;
        let end = (start + rng.below(0x10) as i32).min(0x1A);
        cpu.mem.wr32(STACK_TOP + 0x10, end as u32);
        log.borrow_mut().clear();
        assert_eq!(
            cpu.call(ROWS, &[0x8013_0000, x as u32, y as u32, start as u32]),
            Stop::Returned,
            "run {run}"
        );
        let log = log.borrow();
        for row in start..=end {
            let line = dev_menu_row_y(y, row, start);
            let first = log.iter().find(|&&(_, ly)| ly == line);
            if row > DEV_MENU_MAX_ROW {
                assert!(
                    first.is_none(),
                    "run {run} row {row}: nothing past the list"
                );
                continue;
            }
            let &(lx, _) = first.unwrap_or_else(|| panic!("run {run} row {row}: no print"));
            // Rows 13 and 14 are the equip pager's second and third columns
            // (`x + 0x18` / `x + 0x38`, `0x801EC76C` / `0x801EC780`), which
            // the engine's menu has no row for.
            let col = match row {
                13 => 0x18,
                14 => 0x38,
                _ => DEV_MENU_LABEL_COL,
            };
            assert_eq!(lx, x + col, "run {run} row {row}");
            drawn += 1;
        }
        assert!(
            log.iter()
                .all(|&(_, ly)| ly >= y && ly <= dev_menu_row_y(y, end, start)),
            "run {run}: a print off the window"
        );
    }
    assert!(drawn > 2000, "{drawn}");
}

/// One print of `FUN_8001AA68`: the string's address and bytes, the pen,
/// and the digit width `_DAT_801F2B80` stood at.
#[derive(Debug, Clone)]
struct Print {
    at: u32,
    text: Vec<u8>,
    y: i32,
    width: u32,
}

/// `FUN_801EAD98`'s readouts against the port's kernels: the `CLOSED` gate
/// of rows 0 and 1 (`_DAT_8007B868`, `DevMenuRow::is_closed`), the camera
/// row's two box centres (`decode_camera_readout`), and the inline digit
/// kernel (`format_fixed_decimal` at the width the row stores in
/// `_DAT_801F2B80`) on every row whose value is one word: rows 7
/// (`_DAT_8007B6D0`), 0xA (`_DAT_801F2E8C`), 0x11 (`_DAT_801F2AA0`), 0x12..0x15
/// (the signed tile-window bytes `0x1F8003E8..EB`) and 0x16 (the camera
/// actor's `+0x14` / `+0x18` halves over 128), each seeded at random with
/// signs and overflowing magnitudes.
#[test]
fn the_row_readouts_print_what_the_port_formats() {
    differential!(prot(897, ROWS));
    use legaia_engine_dev_menu::dev_menu_leaves::{
        DevMenuRow, decode_camera_readout, format_fixed_decimal,
    };
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        return;
    };
    const GATE: u32 = 0x8007_B868;
    const CLOSED: u32 = 0x801C_F344;
    const FOLLOW: u32 = 0x8007_B606;
    const BOX: u32 = 0x1F80_0384;
    const DEBUG: u32 = 0x8007_B6D0;
    const RATE: u32 = 0x801F_2E8C;
    const FLAG: u32 = 0x801F_2AA0;
    const WINDOW: u32 = 0x1F80_03E8;
    const CAM_ACTOR: u32 = 0x8007_C364;
    const ACTOR: u32 = 0x8013_8000;
    cpu.mem
        .load(OVERLAY_BASE, &d.read_entry(897).expect("PROT 0897"));
    cpu.stub_outside = Some((OVERLAY_BASE, 0x8020_0000));
    let log: Rc<RefCell<Vec<Print>>> = Rc::default();
    let l = log.clone();
    cpu.hook(PRINT, move |r, m| {
        let at = r.r[psx_retail_exec::A0];
        let text = (0..32)
            .map(|k| m.rd8(at + k))
            .take_while(|&b| b != 0)
            .collect();
        l.borrow_mut().push(Print {
            at,
            text,
            y: r.r[A2] as i32,
            width: m.rd32(0x801F_2B80),
        })
    });
    cpu.mem.wr32(0x1F80_03A0, 0x8014_0000);
    cpu.mem.wr32(0x1F80_03F4, 0x8016_0000);
    cpu.mem.wr32(CAM_ACTOR, ACTOR);
    let mut rng = Rng(0x5245_4144);
    let mut checked = 0;
    let wide = |rng: &mut Rng| -> i32 {
        match rng.below(4) {
            0 => rng.below(10) as i32 - 5,
            1 => rng.next() as i32,
            _ => (rng.below(20000) as i32) - 10000,
        }
    };
    for run in 0..1500 {
        let gate = [0u32, 0, 1, 0x100][rng.below(4) as usize];
        let follow = [0u8, 1, 2][rng.below(3) as usize];
        let boxw = if rng.below(4) == 0 {
            0x7F7F_0000
        } else {
            rng.next()
        };
        let debug = wide(&mut rng);
        let cursor = rng.below(0x18);
        // Row 0xA under the cursor names item `rate` out of the item table,
        // so it holds an id there (the adjuster keeps it in `[1, 255]`).
        let rate = if cursor == 0xA {
            1 + rng.below(255) as i32
        } else {
            wide(&mut rng)
        };
        let flag = wide(&mut rng);
        let window: [i8; 4] = std::array::from_fn(|_| rng.next() as i8);
        let (h14, h18) = (rng.next() as i16, rng.next() as i16);
        cpu.mem.wr32(GATE, gate);
        cpu.mem.wr8(FOLLOW, follow);
        cpu.mem.wr32(BOX, boxw);
        cpu.mem.wr32(DEBUG, debug as u32);
        cpu.mem.wr32(RATE, rate as u32);
        cpu.mem.wr32(FLAG, flag as u32);
        for (k, b) in window.iter().enumerate() {
            cpu.mem.wr8(WINDOW + k as u32, *b as u8);
        }
        cpu.mem.wr16(ACTOR + 0x14, h14 as u16);
        cpu.mem.wr16(ACTOR + 0x18, h18 as u16);
        let y = rng.below(0x40) as i32;
        let start = 0;
        // Now and then an empty span, which draws nothing.
        let end = if rng.below(16) == 0 { -1 } else { 0x17 };
        cpu.mem.wr32(STACK_TOP + 0x10, end as u32);
        // The cursor row (`actor[+0x9e]`), which adds a row's extras.
        cpu.mem.wr16(0x8013_009E, cursor as u16);
        log.borrow_mut().clear();
        assert_eq!(
            cpu.call(ROWS, &[0x8013_0000, 0x10, y as u32, start as u32]),
            Stop::Returned,
            "run {run}"
        );
        let log = log.borrow();
        if end < start {
            assert!(log.is_empty(), "run {run}: an empty span prints {log:?}");
            continue;
        }
        let on_row = |row: i32| -> Vec<Print> {
            let line = dev_menu_row_y(y, row, start);
            log.iter().filter(|p| p.y == line).cloned().collect()
        };
        let digits = |row: i32| -> Vec<Print> {
            on_row(row)
                .into_iter()
                .skip(1)
                .filter(|p| {
                    p.text.iter().any(|b| b.is_ascii_digit())
                        && p.text.iter().all(|&b| b.is_ascii_digit() || b == b'-')
                })
                .collect()
        };
        let fmt = |p: &Print, v: i32| format_fixed_decimal(v, p.width as usize).into_bytes();
        let ctx = format!("run {run}");
        for row in [0u32, 1] {
            let label = &on_row(row as i32)[0];
            let closed = DevMenuRow::from_index(row).unwrap().is_closed(gate);
            assert_eq!(
                label.at == CLOSED,
                closed,
                "{ctx}: row {row} gate {gate:#x}"
            );
        }
        let cam = digits(3);
        match (follow, decode_camera_readout(boxw)) {
            (0, _) => assert!(cam.is_empty(), "{ctx}: camera {cam:?}"),
            // The whole-map box prints the overlay's fixed readout string,
            // not the digit kernel's stack buffer.
            (_, None) => {
                assert_eq!(cam.len(), 1, "{ctx}: camera {cam:?}");
                assert!(cam[0].at < STACK_TOP - 0x1000, "{ctx}: camera {cam:?}");
            }
            (_, Some((pitch, yaw))) => {
                assert_eq!(cam.len(), 2, "{ctx}: camera {boxw:#x} {cam:?}");
                assert_eq!(cam[0].text, fmt(&cam[0], pitch), "{ctx}: pitch");
                assert_eq!(cam[1].text, fmt(&cam[1], yaw), "{ctx}: yaw");
            }
        }
        let singles: [(i32, i32); 7] = [
            (7, debug),
            (0xA, rate),
            (0x11, flag),
            (0x12, i32::from(window[0])),
            (0x13, i32::from(window[1])),
            (0x14, i32::from(window[2])),
            (0x15, i32::from(window[3])),
        ];
        for (row, v) in singles {
            let p = &digits(row)[0];
            assert_eq!(p.text, fmt(p, v), "{ctx}: row {row:#x} value {v}");
            checked += 1;
        }
        let pos = digits(0x16);
        assert_eq!(
            pos[0].text,
            fmt(&pos[0], i32::from(h14 >> 7)),
            "{ctx}: +0x14"
        );
        assert_eq!(
            pos[1].text,
            fmt(&pos[1], i32::from(h18 >> 7)),
            "{ctx}: +0x18"
        );
    }
    assert!(checked > 5000, "{checked}");
}
