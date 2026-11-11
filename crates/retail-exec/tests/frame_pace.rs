//! The frame driver's pacing tail, `FUN_80016B6C` `0x8001703C..0x800171D8`,
//! against `engine_battle_loop::frame_clock::FrameCostRing::frame_end`: the
//! cost history `0x80084098`, its cursor `gp+0x440`, the step byte
//! `0x1F800393`, the step before it `0x1F800392` and the count handed to
//! `VSync`.
//!
//! The routine is entered at its head with the cue ring held off
//! (`gp+0x550`), so it falls to the ring loop's exit `0x80016EA4`; a hook
//! there steps over the draw-environment tail (GPU packets, two libgpu
//! calls) to the cost read `jal 0x800173BC` at `0x8001703C`, which is hooked
//! to return the scripted cost. `VSync` (`0x8005FB84`) and `DrawSync`
//! (`0x80058104`) are hooked, and the run stops at `0x800171DC`, past the
//! last store the law makes.

use crate::common::{Rng, scus_cpu};
use legaia_engine_battle_loop::frame_clock::{FrameCostRing, FramePace};
use psx_retail_exec::{A0, RA, RETURN_SENTINEL, Stop, V0};
use std::cell::RefCell;
use std::rc::Rc;

const DRAIN: u32 = 0x8001_6B6C;
const DRAIN_LOOP_EXIT: u32 = 0x8001_6EA4;
const COST_READ_SITE: u32 = 0x8001_703C;
const COST_READ: u32 = 0x8001_73BC;
const DRAW_SYNC: u32 = 0x8005_8104;
const VSYNC: u32 = 0x8005_FB84;
const LAW_END: u32 = 0x8001_71DC;

const GP: u32 = 0x8007_B318;
const HISTORY: u32 = 0x8008_4098;
const CURSOR: u32 = GP + 0x440;
const FORCED: u32 = GP + 0x5D8;
const MODE: u32 = GP + 0x4CE;
const RING_HOLD: u32 = GP + 0x550;
const FLOOR: u32 = 0x8007_B9D8;
const PREV_STEP: u32 = 0x1F80_0392;
const STEP: u32 = 0x1F80_0393;

/// Costs on and beside every edge the law tests.
const EDGES: [i32; 16] = [
    0, 0xF0, 0xF1, 0x1FE, 0x1FF, 0x2BB, 0x2BC, 0x2BD, 0x2CF, 0x2D0, 0x2D1, 0x2D2, 0x400, 0x7FFF,
    0x1_0000, -5,
];

#[test]
fn the_frame_step_law_matches_retail() {
    differential!(scus(DRAIN));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let cost = Rc::new(RefCell::new(0i32));
    let waits: Rc<RefCell<Vec<u32>>> = Rc::new(RefCell::new(Vec::new()));
    cpu.hook(DRAIN_LOOP_EXIT, |r, _| r.r[RA] = COST_READ_SITE);
    let c = cost.clone();
    cpu.hook(COST_READ, move |r, _| r.r[V0] = *c.borrow() as u32);
    cpu.hook(DRAW_SYNC, |_, _| {});
    let w = waits.clone();
    cpu.hook(VSYNC, move |r, _| w.borrow_mut().push(r.r[A0]));
    cpu.hook(LAW_END, |r, _| r.r[RA] = RETURN_SENTINEL);

    // forced, mode off 0x10, floor raised, raw past the clamp at step 3, at step 4
    let mut reached = [0u32; 5];
    let mut steps = std::collections::BTreeSet::new();
    for seed in 0..200u64 {
        let mut rng = Rng(0x5354_4550 ^ seed);
        // Every fourth run is a cheap one, so the history can read step 1.
        let cheap = seed % 4 == 0;
        let entries: [i16; 16] = std::array::from_fn(|_| match rng.below(8) {
            0 => -(rng.below(40) as i16),
            _ if cheap => rng.below(0x80) as i16,
            1 => 0x2BC,
            _ => rng.below(0x2D0) as i16,
        });
        let cursor = rng.below(17) as u8;
        let (step0, prev0) = (rng.below(5) as u8, rng.below(5) as u8);
        for (i, e) in entries.iter().enumerate() {
            cpu.mem.wr16(HISTORY + i as u32 * 2, *e as u16);
        }
        cpu.mem.wr32(CURSOR, u32::from(cursor));
        cpu.mem.wr8(STEP, step0);
        cpu.mem.wr8(PREV_STEP, prev0);
        cpu.mem.wr32(RING_HOLD, 1);
        let mut ring = FrameCostRing::from_state(entries, cursor, step0, prev0);
        for frame in 0..40 {
            let raw = if cheap {
                rng.below(0x120) as i32
            } else if rng.below(3) == 0 {
                EDGES[rng.below(EDGES.len() as u32) as usize]
            } else {
                rng.below(0x340) as i32
            };
            let pace = FramePace {
                forced: [0, 0, 0, 0, 0, 0, 1, 2, 3, 0x102][rng.below(10) as usize],
                mode: [0x10, 0x10, 0x10, 0x10, 0, 0x11][rng.below(6) as usize],
                floor: [0, 1, 1, 1, 2, 3, 4, -1, 0x103][rng.below(9) as usize],
            };
            *cost.borrow_mut() = raw;
            cpu.mem.wr32(FORCED, pace.forced);
            cpu.mem.wr16(MODE, pace.mode as u16);
            cpu.mem.wr32(FLOOR, pace.floor as u32);
            waits.borrow_mut().clear();
            assert_eq!(cpu.call(DRAIN, &[]), Stop::Returned);

            let end = ring.frame_end(raw, pace);
            let at = format!("seed {seed} frame {frame} raw {raw:#x} {pace:?}");
            let retail: [i16; 16] =
                std::array::from_fn(|i| cpu.mem.rd16(HISTORY + i as u32 * 2) as i16);
            assert_eq!(retail, ring.entries(), "history, {at}");
            assert_eq!(
                cpu.mem.rd32(CURSOR),
                u32::from(ring.cursor()),
                "cursor, {at}"
            );
            assert_eq!(cpu.mem.rd8(STEP), end.step, "step, {at}");
            assert_eq!(cpu.mem.rd8(STEP), ring.step(), "stored step, {at}");
            assert_eq!(
                cpu.mem.rd8(PREV_STEP),
                ring.prev_step(),
                "step before, {at}"
            );
            assert_eq!(*waits.borrow(), vec![end.vsync_wait], "VSync count, {at}");

            let adaptive = pace.forced == 0 && pace.mode == 0x10;
            reached[0] += u32::from(pace.forced != 0);
            reached[1] += u32::from(pace.forced == 0 && pace.mode != 0x10);
            reached[2] +=
                u32::from(adaptive && i32::from(end.step) == pace.floor && pace.floor > 1);
            reached[3] += u32::from(adaptive && raw == 0x2D0 && pace.floor <= 3);
            reached[4] += u32::from(adaptive && raw >= 0x2D1);
            if adaptive && pace.floor <= 1 {
                steps.insert(end.step);
            }
        }
    }
    assert!(reached.iter().all(|&n| n > 20), "{reached:?}");
    assert_eq!(steps.into_iter().collect::<Vec<_>>(), vec![1, 2, 3, 4]);
}
