//! `main` (`FUN_80015E90`) whole, boot preamble and mode-table loop
//! (`0x8001615C..0x8001620C`), with every routine outside it stubbed and each
//! mode handler of the table at `0x8007078C` (`+0x10`) hooked to write a
//! scripted next word, against `ModeSeat` driven by the same script: which
//! mode each pass dispatches, and the transition edge the loop runs when a
//! handler changed the word - its `from` / `to`, the frame-begin skip clear
//! and the stores the port names.
//!
//! The run ends where retail's loop does, on a negative word
//! (`0x80016204 bgez`). The edge's calls (`FUN_8003DE7C`, `FUN_8003ED04`,
//! `FUN_80016230`, `FUN_80058104`, `FUN_8001822C`) are recorded, not run.

use crate::common::{Rng, scus_cpu};
use legaia_engine_mode::mode::{GameMode, ModeSeat, ModeWorld, SceneMode};
use psx_retail_exec::{GP, Stop};
use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

const MAIN: u32 = 0x8001_5E90;
const MAIN_END: u32 = 0x8001_6230;
const TABLE: u32 = 0x8007_078C;
const EDGE_RESET: u32 = 0x8001_6230;
const EDGE_TAIL: u32 = 0x8001_822C;
const PAD_LATCH: u32 = 0x8007_B938;

#[derive(Default)]
struct Probe {
    skip_clears: u32,
    pad_clears: u32,
}

impl ModeWorld for Probe {
    fn scene_mode(&self) -> SceneMode {
        SceneMode::Title
    }
    fn set_scene_mode(&mut self, _mode: SceneMode) {}
    fn take_frame_begin_skip(&mut self) -> bool {
        false
    }
    fn clear_frame_begin_skip(&mut self) {
        self.skip_clears += 1;
    }
    fn detach_sound(&mut self) {}
    fn battle_mode_word_held(&self) -> bool {
        false
    }
    fn clear_pad_edges(&mut self) {
        self.pad_clears += 1;
    }
    fn tick_frame(&mut self) {}
}

/// One pass of the loop as retail ran it: the word the handler saw, the word
/// it left.
type Pass = (i16, i16);
/// One edge: the word before and the word after.
type Edge = (i16, i16);

#[test]
fn the_mode_table_loop_dispatches_what_retail_dispatches() {
    differential!(scus(MAIN));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let gp = cpu.regs.r[GP];
    let (word, previous) = (gp + 0x524, gp + 0x494);
    cpu.stub_outside = Some((MAIN, MAIN_END));
    cpu.step_budget = 5_000_000;
    let script: Rc<RefCell<VecDeque<i16>>> = Rc::default();
    let passes: Rc<RefCell<Vec<Pass>>> = Rc::default();
    let edges: Rc<RefCell<Vec<Edge>>> = Rc::default();
    let handlers: std::collections::BTreeSet<u32> = (0..28)
        .map(|i| cpu.mem.rd32(TABLE + 0x18 * i + 0x10))
        .collect();
    for h in handlers {
        let (s, p) = (script.clone(), passes.clone());
        cpu.hook(h, move |_, m| {
            let seen = m.rd16(word) as i16;
            let next = s.borrow_mut().pop_front().unwrap_or(-1);
            m.wr16(word, next as u16);
            p.borrow_mut().push((seen, next));
        });
    }
    // The edge's own stores land after `FUN_8001822C`; the reset call sees
    // the word pair as the handler left it.
    let pending: Rc<RefCell<Option<(i16, i16)>>> = Rc::default();
    let pe = pending.clone();
    cpu.hook(EDGE_RESET, move |_, m| {
        *pe.borrow_mut() = Some((m.rd16(previous) as i16, m.rd16(word) as i16));
    });
    let (pe, ed) = (pending.clone(), edges.clone());
    cpu.hook(EDGE_TAIL, move |_, m| {
        let (from, to) = pe.borrow_mut().take().expect("the reset ran first");
        assert_eq!(m.rd16(word) as i16, to);
        ed.borrow_mut().push((from, to));
    });

    let mut rng = Rng(0x4D41_494E);
    let (mut total_passes, mut total_edges, mut init_passes) = (0usize, 0usize, 0usize);
    for round in 0..300 {
        let start = rng.below(28) as i16;
        let len = 1 + rng.below(60) as usize;
        let mut cur = start;
        let mut s = VecDeque::new();
        for _ in 0..len {
            // An INIT handler always stores (its successor, or anything); a
            // RUN handler mostly keeps the word.
            let keep = cur & 1 == 1 && rng.below(3) != 0;
            cur = if keep { cur } else { rng.below(28) as i16 };
            s.push_back(cur);
        }
        *script.borrow_mut() = s.clone();
        passes.borrow_mut().clear();
        edges.borrow_mut().clear();
        cpu.mem.wr16(word, start as u16);
        cpu.mem.wr16(previous, start as u16);
        for o in [0x3D8, 0x538, 0x55C] {
            cpu.mem.wr32(gp + o, rng.next() | 1);
        }
        cpu.mem.wr32(PAD_LATCH, rng.next() | 1);
        let stop = cpu.call(MAIN, &[]);
        assert_eq!(stop, Stop::Returned, "round {round}");
        assert!(script.borrow().is_empty(), "round {round}: every pass ran");
        // The stores of the last edge (the one to the negative word that ends
        // the loop) are what memory holds now.
        let last = *edges.borrow().last().expect("the ending edge");
        assert_eq!(last.1, -1);
        assert_eq!(cpu.mem.rd16(gp + 0x3D8), 0, "round {round}");
        assert_eq!(cpu.mem.rd32(gp + 0x538), 0, "round {round}");
        assert_eq!(cpu.mem.rd32(PAD_LATCH), 0, "round {round}");
        assert_eq!(cpu.mem.rd32(gp + 0x55C), 0, "round {round}");
        assert_eq!(cpu.mem.rd16(gp + 0x564) as i16, -1, "round {round}");
        assert_eq!(cpu.mem.rd16(previous) as i16, -1, "round {round}");

        // The port, on the same script.
        let mut seat = ModeSeat::new(GameMode::from_index(start as usize).unwrap());
        let mut world = Probe::default();
        let mut port_passes: Vec<Pass> = Vec::new();
        let mut port_edges: Vec<(i16, i16)> = Vec::new();
        for &next in &s {
            let frame = seat.frame(&mut world);
            if let Some(e) = frame.edge {
                assert!(e.cleared_frame_begin_skip);
                port_edges.push((e.from.as_index() as i16, e.to.as_index() as i16));
            }
            let seen = frame.game_mode.as_index() as i16;
            init_passes += usize::from(frame.init.is_some());
            port_passes.push((seen, next));
            seat.write_mode(GameMode::from_index(next as usize).unwrap());
        }
        // The edge into the last scripted word, which retail takes before it
        // loops back for the terminating pass.
        let frame = seat.frame(&mut world);
        if let Some(e) = frame.edge {
            port_edges.push((e.from.as_index() as i16, e.to.as_index() as i16));
        }
        let retail_edges: Vec<(i16, i16)> = edges
            .borrow()
            .iter()
            .filter(|e| e.1 >= 0)
            .copied()
            .collect();
        // Retail's pass list has one more entry: the terminating pass, whose
        // handler writes the negative word.
        let mut retail_passes = passes.borrow().clone();
        let end = retail_passes.pop().expect("the terminating pass");
        assert_eq!(end.1, -1);
        assert_eq!(end.0, *s.back().unwrap());
        assert_eq!(retail_passes, port_passes, "round {round}: dispatch");
        assert_eq!(retail_edges, port_edges, "round {round}: edges");
        assert_eq!(world.skip_clears as usize, port_edges.len());
        // `ModeSeat::frame` does not swallow the pad (its own doc gives the
        // ordering reason); retail's edge always clears both words.
        assert_eq!(world.pad_clears, 0);
        total_passes += port_passes.len();
        total_edges += port_edges.len();
    }
    assert!(total_passes > 5000 && total_edges > 3000 && init_passes > 1000);
}

/// The master frame driver `FUN_80016444`, run with every call stubbed and
/// its passes recorded. It is not compared: the port has no list of actor
/// lists to hold against it - `World::tick` runs one actor pool with per-kind
/// loops, and the render passes and the flip are the host renderer's. What
/// is checked is the order its doc states: five tick passes `FUN_8002519C`,
/// the flip `FUN_8001D058` ahead of the five render passes `FUN_8001D140` in
/// STR mode (`0x15`) and after them otherwise, and the passes' third and
/// fourth lists chosen by the scratchpad mirror bit `0x1F800394 & 0x10`:
/// `0x8007C358` / `0x8007C354` with it clear, `0x8007C360` / `0x8007C35C`
/// with it set. A zero argument, the map display's, runs no tick pass.
#[test]
fn the_frame_driver_runs_its_passes_in_the_documented_order() {
    not_a_differential!(
        "FUN_80016444's passes walk seven actor-list heads the port's single actor pool has no counterpart for; drives it and checks the pass order World::tick's doc states"
    );
    const DRIVER: u32 = 0x8001_6444;
    const DRIVER_END: u32 = 0x8001_698C;
    const TICK_PASS: u32 = 0x8002_519C;
    const RENDER_PASS: u32 = 0x8001_D140;
    const FLIP: u32 = 0x8001_D058;
    const HEADS: u32 = 0x8007_C34C;
    const MIRROR: u32 = 0x1F80_0394;
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let gp = cpu.regs.r[GP];
    cpu.stub_outside = Some((DRIVER, DRIVER_END));
    let calls: Rc<RefCell<Vec<(char, u32)>>> = Rc::default();
    for (va, tag) in [(TICK_PASS, 't'), (RENDER_PASS, 'r'), (FLIP, 'f')] {
        let c = calls.clone();
        cpu.hook(va, move |r, _| {
            c.borrow_mut().push((tag, r.r[psx_retail_exec::A0]))
        });
    }
    for i in 0..9 {
        cpu.mem.wr32(HEADS + 4 * i, 0x100 + i);
    }
    for (param, mirror, mode) in [(1, false, 3), (1, true, 3), (1, false, 0x15), (0, false, 3)] {
        cpu.mem.wr32(gp + 0x3B8, 0);
        cpu.mem.wr32(gp + 0x3D0, 0);
        cpu.mem.wr16(gp + 0x524, mode);
        cpu.mem.wr32(MIRROR, if mirror { 0x10 } else { 0 });
        calls.borrow_mut().clear();
        assert_eq!(cpu.call(DRIVER, &[param]), Stop::Returned);
        let lists = |t: char| -> Vec<u32> {
            calls
                .borrow()
                .iter()
                .filter(|c| c.0 == t)
                .map(|c| c.1 - 0x100)
                .collect()
        };
        // List `k` is the head at `0x8007C34C + 4 * k`.
        let passes: Vec<u32> = if mirror {
            vec![0, 1, 5, 4, 8]
        } else {
            vec![0, 1, 3, 2, 8]
        };
        assert_eq!(
            lists('t'),
            if param == 0 { vec![] } else { passes.clone() },
            "param {param} mirror {mirror}"
        );
        assert_eq!(lists('r'), passes, "mirror {mirror}");
        let order: Vec<char> = calls
            .borrow()
            .iter()
            .map(|c| c.0)
            .filter(|&t| t != 't')
            .collect();
        let flip_first = order.first() == Some(&'f');
        assert_eq!(flip_first, mode == 0x15, "mode {mode:#x}: {order:?}");
        assert_eq!(order.iter().filter(|&&t| t == 'f').count(), 1);
    }
}
