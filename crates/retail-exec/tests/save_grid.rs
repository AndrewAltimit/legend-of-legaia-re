//! The save screen's slot-grid dispatcher `FUN_801E06C0` (PROT 0899)
//! against `slot_grid_quad_x`: every cell's x over the whole commit slide
//! (`t = *0x801EF1A0`, `0..=0x1000`), read off the per-cell drawer
//! `FUN_801E0FD0`'s pen, which is hooked; the info panel and its helper are
//! stubbed.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_ui::{SLOT_GRID_COLS, SLOT_GRID_ROWS, slot_grid_quad_x};
use psx_retail_exec::{A0, A1, A2, Stop, V0};
use std::cell::RefCell;
use std::rc::Rc;

const OVERLAY_BASE: u32 = 0x801C_E818;
const GRID: u32 = 0x801E_06C0;
const CELL: u32 = 0x801E_0FD0;
const SLIDE: u32 = 0x801E_F1A0;

#[test]
fn the_grid_slides_every_cell_where_retail_does() {
    differential!(prot(899, GRID));
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem.load(
        OVERLAY_BASE,
        &d.read_entry_footprint(899).expect("PROT 0899"),
    );
    cpu.stub(0x801E_3F74, 0);
    cpu.stub(0x801E_08D8, 0);
    cpu.stub(0x8002_C488, 0);
    let cells: Rc<RefCell<Vec<(u32, i32, i32)>>> = Rc::default();
    let c = cells.clone();
    cpu.hook(CELL, move |r, _| {
        c.borrow_mut()
            .push((r.r[A0], r.r[A1] as i32, r.r[A2] as i32));
        r.r[V0] = 5;
    });
    let mut rng = Rng(0x534C_4F54);
    let mut ts: Vec<u32> = (0..=0x1000).step_by(0x40).collect();
    ts.extend((0..200).map(|_| rng.below(0x1001)));
    for t in ts {
        cpu.mem.wr32(SLIDE, t);
        cells.borrow_mut().clear();
        let (col, row) = (rng.below(5), rng.below(3));
        assert_eq!(cpu.call(GRID, &[col, row]), Stop::Returned, "t {t:#x}");
        let got = cells.borrow().clone();
        assert_eq!(got.len(), SLOT_GRID_COLS * SLOT_GRID_ROWS, "t {t:#x}");
        for (i, &(slot, x, y)) in got.iter().enumerate() {
            let (c, r) = (i as u32 % 5, i as u32 / 5);
            assert_eq!(slot, c + 5 * r, "t {t:#x} cell {i}");
            assert_eq!(y, 0x24 + 20 * r as i32, "t {t:#x} cell {i}");
            // The drawer adds its `+8` inset, which the port folds in.
            assert_eq!(
                x + 8,
                slot_grid_quad_x(c, r, slot, t as u16),
                "t {t:#x} cell {i} (col {c} row {r})"
            );
        }
    }
}
