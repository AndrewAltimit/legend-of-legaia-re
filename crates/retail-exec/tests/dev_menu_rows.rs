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
