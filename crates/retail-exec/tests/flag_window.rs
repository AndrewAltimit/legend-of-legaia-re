//! The field overlay's flag-window picker (PROT 0897), the screen `49 04`
//! opens at the Uru Mais warp pads: the picker `FUN_801EF014` against
//! `flag_window_tick`, with the shared list cursor live beneath it, and its
//! painter `FUN_801E6984` against `submode_panel_rows` and the lines
//! `flag_window_lines` lays out. Beside them the MAN loader's two spawns:
//! the submode open `FUN_801D9C3C` (`open_submode`) and the scene actor
//! `FUN_801DE478` (`scene_actor_initial_state`).

use crate::common::{Rng, diff_offsets, disc, scus_cpu};
use legaia_engine_field::field_submode::{
    PANEL_GLYPH_DX, PANEL_GLYPH2_DX, PANEL_HIGHLIGHT_DX, SCENE_ACTOR_TEMPLATE,
    SUBMODE_DRIVER_HANDLER, SUBMODE_DRIVER_TEMPLATE, SUBMODE_TAIL_OFFSET, SUBMODE_TAIL_WORDS,
    SubmodeOpen, open_submode, scene_actor_initial_state, submode_panel_rows,
};
use legaia_engine_field::field_submode_flag_window::{
    FLAG_WINDOW_LEGEND_VAS, FlagWindowState, flag_window_lines,
};
use legaia_engine_field::world_map_panel::CursorPad;
use legaia_engine_field::world_map_panel_actors::{
    FlagWindowDescriptor, FlagWindowEffect, FlagWindowInput, flag_window_tick,
};
use psx_retail_exec::{A0, A1, A2, A3, Cpu, SP, Stop, V0};
use std::cell::{Cell, RefCell};
use std::collections::BTreeSet;
use std::rc::Rc;

const OVERLAY_BASE: u32 = 0x801C_E818;
const PICKER: u32 = 0x801E_F014;
const PAINTER: u32 = 0x801E_6984;
const OPEN: u32 = 0x801D_9C3C;
const SCENE_ACTOR: u32 = 0x801D_E478;

const FLAG_TEST: u32 = 0x8003_CE64;
const FLAG_SET: u32 = 0x8003_CE08;
const FLAG_CLEAR: u32 = 0x8003_CE34;
const PANEL_SCRIPT: u32 = 0x801E_9B3C;
const DRAW_PUMP: u32 = 0x8003_1D00;
const CUE: u32 = 0x8003_5B50;
const SPRITE: u32 = 0x8002_B994;
const CELL: u32 = 0x8002_C488;
const TEXT: u32 = 0x8003_6888;
const FRAME: u32 = 0x8002_C69C;
const ALLOC: u32 = 0x8002_0DE0;
const FIND: u32 = 0x8003_CF04;

const OPERAND_PTR: u32 = 0x8007_B450;
const BLOCKED: u32 = 0x8007_BB80;
const ROW: u32 = 0x8007_BB88;
const MARKED: u32 = 0x8007_BB9C;
const GRID_PTR: u32 = 0x801C_6EA4;
const WINDOW_14: u32 = 0x801F_2B98 + 14 * 0x1C;
const MODE_FLAGS: u32 = 0x8007_B868;
const POOL_LISTS: u32 = 0x8007_C348;
const CONTEXT: u32 = 0x801F_2734;

const ACTOR: u32 = 0x8012_0000;
const GRID: u32 = 0x8012_1000;
const OPERAND: u32 = 0x8012_2000;
const SPAWNED: u32 = 0x8013_0000;

fn cpu() -> Option<Cpu> {
    let mut cpu = scus_cpu()?;
    cpu.mem
        .load(OVERLAY_BASE, &disc()?.read_entry(897).expect("PROT 0897"));
    Some(cpu)
}

fn write_desc(cpu: &mut Cpu, d: &FlagWindowDescriptor) {
    cpu.mem.wr8(OPERAND, 4);
    cpu.mem.wr8(OPERAND + 1, d.count);
    cpu.mem.wr8(OPERAND + 2, d.first_visible);
    cpu.mem.wr8(OPERAND + 3, d.rows);
    cpu.mem.wr16(OPERAND + 4, d.base_flag as u16);
    cpu.mem.wr32(OPERAND_PTR, OPERAND);
}

fn desc(rng: &mut Rng) -> FlagWindowDescriptor {
    // The disc's two shapes, and generated ones around them.
    match rng.below(4) {
        0 => FlagWindowDescriptor {
            count: 8,
            first_visible: 0,
            rows: 8,
            base_flag: 0x138,
        },
        1 => FlagWindowDescriptor {
            count: 8,
            first_visible: 4,
            rows: 4,
            base_flag: 0x138,
        },
        _ => {
            let rows = 1 + rng.below(8) as u8;
            FlagWindowDescriptor {
                count: rng.below(10) as u8,
                first_visible: rng.below(5) as u8,
                rows,
                base_flag: 0x100 + rng.below(0x200) as i32,
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ev {
    Clear(i32),
    Set(i32),
    Script(u32),
    Pump,
}

#[test]
fn the_flag_window_picker_matches_retail() {
    differential!(prot(897, PICKER));
    let Some(mut cpu) = cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let flags: Rc<RefCell<BTreeSet<i32>>> = Rc::new(RefCell::new(BTreeSet::new()));
    let log: Rc<RefCell<Vec<Ev>>> = Rc::new(RefCell::new(Vec::new()));
    let f = flags.clone();
    cpu.hook(FLAG_TEST, move |r, _| {
        r.r[V0] = u32::from(f.borrow().contains(&(r.r[A0] as i32)));
    });
    for (va, make) in [
        (FLAG_CLEAR, (|a| Ev::Clear(a as i32)) as fn(u32) -> Ev),
        (FLAG_SET, |a| Ev::Set(a as i32)),
        (PANEL_SCRIPT, |a| Ev::Script(a)),
        (DRAW_PUMP, |_| Ev::Pump),
    ] {
        let l = log.clone();
        cpu.hook(va, move |r, _| l.borrow_mut().push(make(r.r[A0])));
    }
    cpu.stub(CUE, 0);
    cpu.mem.wr32(GRID_PTR, GRID);
    let mut rng = Rng(0x464C_4157);
    let (mut picked, mut same, mut cancelled, mut found, mut moved) = (0, 0, 0, 0, 0);
    for run in 0..3000 {
        let d = desc(&mut rng);
        write_desc(&mut cpu, &d);
        flags.borrow_mut().clear();
        // The pad the player stands on has its own flag set, as the lift's
        // scripts leave it; some runs set none or several.
        for _ in 0..rng.below(3) {
            let i = rng.below(u32::from(d.count).max(1) + 1) as i32;
            flags.borrow_mut().insert(d.base_flag + i);
        }
        let handler = 0x23u16;
        let mut phase = [0i16, 0, 0, 1, 2, 3, 4, -1][rng.below(8) as usize];
        let mut selection = rng.below(10) as i32;
        let mut remembered = rng.below(10) as i32;
        cpu.mem.wr16(ACTOR + 0x50, handler);
        cpu.mem.wr16(ACTOR + 0x54, phase as u16);
        cpu.mem.wr32(ROW, selection as u32);
        cpu.mem.wr32(MARKED, remembered as u32);
        cpu.mem.wr16(GRID + 0x2E, 0);
        cpu.mem.wr16(GRID + 0x40, 0);
        cpu.mem.wr16(WINDOW_14 + 0x0A, 0x1111);
        cpu.mem.wr16(WINDOW_14 + 0x0E, 0x2222);
        let (mut panel_y, mut panel_h, mut handback, mut stashed, mut state) =
            (0x1111u16, 0x2222u16, 0u16, 0u16, handler);
        for frame in 0..40 {
            let locked = rng.below(6) == 0;
            let pad = CursorPad {
                held: match rng.below(8) {
                    0 => 0x20,
                    1 => 0x40,
                    _ => 0,
                },
                pressed: [0, 0x1000, 0x4000, 0, 0x1000][rng.below(5) as usize],
                action_a_mask: 0x20,
                action_b_mask: 0x40,
            };
            cpu.mem.wr32(BLOCKED, u32::from(locked));
            cpu.mem.wr32(0x8007_B874, pad.held);
            cpu.mem.wr32(0x8007_BB84, pad.pressed);
            cpu.mem.wr32(0x8008_46D0, pad.action_a_mask);
            cpu.mem.wr32(0x8008_46D4, pad.action_b_mask);
            log.borrow_mut().clear();
            let before = flags.borrow().clone();
            assert_eq!(cpu.call(PICKER, &[ACTOR]), Stop::Returned, "run {run}");
            let input = FlagWindowInput {
                desc: d,
                input_locked: locked,
                selection,
                remembered,
                pad,
                handler_id: state,
            };
            let was = phase;
            let (p, s, r, effects) = flag_window_tick(phase, input, |id| before.contains(&id));
            (phase, selection, remembered) = (p, s, r);
            let mut want = Vec::new();
            for e in &effects {
                match *e {
                    FlagWindowEffect::ClearRange { base, count } => {
                        want.extend((0..i32::from(count)).map(|i| Ev::Clear(base + i)));
                    }
                    FlagWindowEffect::SizePanel { y, height } => {
                        panel_y = y as u16;
                        panel_h = height as u16;
                    }
                    FlagWindowEffect::RunPanelScript(va) => want.push(Ev::Script(va)),
                    FlagWindowEffect::SetFlag(id) => {
                        want.push(Ev::Set(id));
                        picked += 1;
                    }
                    FlagWindowEffect::Exit(x) => {
                        let (mut hb, mut st, mut h, mut ph) = (0i16, 0u16, state, phase);
                        x.apply(&mut hb, &mut st, &mut h, &mut ph);
                        (handback, stashed, state, phase) = (hb as u16, st, h, ph);
                    }
                    FlagWindowEffect::TickTextActors => want.push(Ev::Pump),
                }
            }
            let ctx = format!("run {run} frame {frame} phase {was} {d:?} {input:?}");
            assert_eq!(*log.borrow(), want, "calls: {ctx}");
            assert_eq!(cpu.mem.rd16(ACTOR + 0x54) as i16, phase, "phase: {ctx}");
            assert_eq!(cpu.mem.rd16(ACTOR + 0x50), state, "handler: {ctx}");
            assert_eq!(cpu.mem.rd32(ROW) as i32, selection, "selection: {ctx}");
            assert_eq!(cpu.mem.rd32(MARKED) as i32, remembered, "remembered: {ctx}");
            assert_eq!(cpu.mem.rd16(WINDOW_14 + 0x0A), panel_y, "panel y: {ctx}");
            assert_eq!(cpu.mem.rd16(WINDOW_14 + 0x0E), panel_h, "panel h: {ctx}");
            assert_eq!(cpu.mem.rd16(GRID + 0x2E), handback, "hand-back: {ctx}");
            assert_eq!(cpu.mem.rd16(GRID + 0x40), stashed, "stash: {ctx}");
            found += u32::from(was == 0 && remembered != i32::from(d.count));
            same += u32::from(was == 1 && phase == 3 && pad.held == 0x20);
            cancelled += u32::from(was == 1 && phase == 3 && pad.held == 0x40);
            moved += u32::from(was == 1 && phase == 1 && selection != input.selection);
            if state != handler {
                break;
            }
        }
    }
    assert!(
        picked > 200 && same > 50 && cancelled > 200 && found > 300 && moved > 500,
        "{picked} {same} {cancelled} {found} {moved}"
    );
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Draw {
    Sprite(i32, i32, i16, i16),
    Cell(i16, i16, i32),
    Text(u32, i16, i16),
    Frame(i16, i16, i16, i16),
}

#[test]
fn the_flag_window_painter_matches_retail() {
    differential!(prot(897, PAINTER));
    let Some(mut cpu) = cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let log: Rc<RefCell<Vec<Draw>>> = Rc::new(RefCell::new(Vec::new()));
    let l = log.clone();
    cpu.hook(SPRITE, move |r, _| {
        l.borrow_mut().push(Draw::Sprite(
            r.r[A0] as i32,
            r.r[A1] as i32,
            r.r[A2] as i16,
            r.r[A3] as i16,
        ));
    });
    let l = log.clone();
    cpu.hook(CELL, move |r, _| {
        l.borrow_mut()
            .push(Draw::Cell(r.r[A0] as i16, r.r[A1] as i16, r.r[A2] as i32));
    });
    let l = log.clone();
    cpu.hook(TEXT, move |r, m| {
        let y = m.rd32(r.r[SP] + 0x10) as i16;
        l.borrow_mut().push(Draw::Text(r.r[A0], r.r[A3] as i16, y));
    });
    let l = log.clone();
    cpu.hook(FRAME, move |r, _| {
        l.borrow_mut().push(Draw::Frame(
            r.r[A0] as i16,
            r.r[A1] as i16,
            r.r[A2] as i16,
            r.r[A3] as i16,
        ));
    });
    let mut rng = Rng(0x5041_4E54);
    let (mut cursors, mut marked_rows, mut basements) = (0, 0, 0);
    for run in 0..4000 {
        let d = desc(&mut rng);
        write_desc(&mut cpu, &d);
        let (x, w) = (rng.below(200) as i16, rng.below(120) as i16);
        let state = FlagWindowState {
            selection: [rng.below(10) as i32, 0x64][usize::from(rng.below(6) == 0)],
            remembered: rng.below(10) as i32,
            panel_y: rng.below(200) as i16,
            panel_h: i16::from(d.rows) * 16,
        };
        cpu.mem.wr16(ACTOR + 0x0A, x as u16);
        cpu.mem.wr16(ACTOR + 0x0C, state.panel_y as u16);
        cpu.mem.wr16(ACTOR + 0x0E, w as u16);
        cpu.mem.wr16(ACTOR + 0x10, state.panel_h as u16);
        cpu.mem.wr32(ROW, state.selection as u32);
        cpu.mem.wr32(MARKED, state.remembered as u32);
        log.borrow_mut().clear();
        assert_eq!(cpu.call(PAINTER, &[ACTOR]), Stop::Returned, "run {run}");

        // The row loop.
        let rows = submode_panel_rows(
            (x, state.panel_y),
            d.rows,
            d.first_visible,
            state.selection.clamp(0, 0xFF) as u8,
            state.remembered.clamp(0, 0xFF) as u8,
        );
        let mut want = Vec::new();
        for r in &rows {
            if r.highlighted {
                want.push(Draw::Sprite(0, 1, x + PANEL_HIGHLIGHT_DX, r.y));
                cursors += 1;
            }
            want.push(Draw::Cell(
                x + PANEL_GLYPH_DX,
                r.y,
                i32::from(r.entry) + i32::from(r.ink),
            ));
            if r.second_run {
                want.push(Draw::Cell(x + PANEL_GLYPH2_DX, r.y, i32::from(r.ink) + 8));
            } else {
                basements += 1;
            }
            marked_rows += u32::from(r.ink == 0x58);
        }
        // The legend beside the panel, and its frame.
        let (lx, ly) = (x + w, state.panel_y + state.panel_h);
        want.extend([
            Draw::Cell(lx + 0x20, ly - 0x20, 0x37),
            Draw::Cell(lx + 0x20, ly - 0x10, 0x38),
            Draw::Text(FLAG_WINDOW_LEGEND_VAS[0], lx + 0x34, ly - 0x1E),
            Draw::Text(FLAG_WINDOW_LEGEND_VAS[1], lx + 0x34, ly - 0x0E),
            Draw::Frame(lx + 0x10, ly - 0x20, 0x9C, 0x20),
        ]);
        let ctx = format!("run {run} {d:?} {state:?} x {x} w {w}");
        assert_eq!(*log.borrow(), want, "draws: {ctx}");

        // The lines the hosts draw sit on the same pens: record 14's own
        // geometry, which the window table holds.
        let record = FlagWindowState { ..state };
        let lines = flag_window_lines(&record, d, [b"a", b"b"]);
        let (rx, rw) = (
            cpu.mem.rd16(WINDOW_14 + 0x08) as i16,
            cpu.mem.rd16(WINDOW_14 + 0x0C) as i16,
        );
        let pens: Vec<(i16, i16)> = lines.iter().map(|l| (l.x, l.y)).collect();
        let shift = |d: &Draw| match *d {
            Draw::Sprite(_, _, px, py) | Draw::Cell(px, py, _) | Draw::Text(_, px, py) => {
                Some((px - x + rx, py))
            }
            Draw::Frame(..) => None,
        };
        // The legend hangs off the panel's width, so it compares only where
        // the generated width is the record's.
        let n = if w == rw {
            want.len() - 1
        } else {
            rows_draws(&want)
        };
        // The hosts lay the legend out icon and label by row; retail draws
        // both icons, then both labels. The pens are compared as a set.
        let mut retail_pens: Vec<(i16, i16)> = want.iter().take(n).filter_map(shift).collect();
        let mut pens = pens[..retail_pens.len()].to_vec();
        retail_pens.sort_unstable();
        pens.sort_unstable();
        assert_eq!(pens, retail_pens, "pens: {ctx}");
    }
    assert!(
        cursors > 1000 && marked_rows > 1000 && basements > 500,
        "{cursors} {marked_rows} {basements}"
    );
}

/// How many of `draws` belong to the row loop (everything before the legend).
fn rows_draws(draws: &[Draw]) -> usize {
    draws.len() - 5
}

#[test]
fn the_submode_open_matches_retail() {
    differential!(prot(897, OPEN));
    let Some(mut cpu) = cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let head = 0x8014_0000;
    cpu.mem.wr32(POOL_LISTS + 4, head);
    let present = Rc::new(Cell::new(false));
    let ok = Rc::new(Cell::new(true));
    let p = present.clone();
    cpu.hook(FIND, move |r, _| {
        assert_eq!((r.r[A0], r.r[A1]), (head, SUBMODE_DRIVER_HANDLER));
        r.r[V0] = u32::from(p.get()) * 0x8015_0000;
    });
    let o = ok.clone();
    cpu.hook(ALLOC, move |r, m| {
        assert_eq!((r.r[A0], r.r[A1]), (SUBMODE_DRIVER_TEMPLATE, head));
        if o.get() {
            m.load(SPAWNED, &[0x5A; 0x100]);
        }
        r.r[V0] = if o.get() { SPAWNED } else { 0 };
    });
    // The block from the state word to the end of the tail.
    let span = usize::from(SUBMODE_TAIL_OFFSET) + SUBMODE_TAIL_WORDS * 4;
    let mut rng = Rng(0x4F50_454E);
    let (mut spawned, mut already) = (0, 0);
    for run in 0..600 {
        present.set(rng.below(2) == 0);
        ok.set(rng.below(6) != 0);
        let junk: Vec<u8> = (0..span).map(|_| rng.next() as u8 | 1).collect();
        cpu.mem.load(CONTEXT, &junk);
        cpu.mem.load(SPAWNED, &[0xEE; 0x100]);
        assert_eq!(cpu.call(OPEN, &[]), Stop::Returned, "run {run}");

        let (seeds, outcome) = open_submode(present.get());
        let mut want = junk.clone();
        for (off, v) in seeds {
            want[usize::from(*off)..usize::from(*off) + 4].copy_from_slice(&v.to_le_bytes());
        }
        want[usize::from(SUBMODE_TAIL_OFFSET)..].fill(0);
        let got = cpu.mem.read(CONTEXT, span);
        assert!(got == want, "run {run}: {:?}", diff_offsets(&got, &want));
        let actor = cpu.mem.read(SPAWNED, 0x100);
        match outcome {
            SubmodeOpen::Spawned if ok.get() => {
                assert_eq!(cpu.regs.r[V0], SPAWNED, "run {run}");
                let mut rec = vec![0x5Au8; 0x100];
                rec[0x50..0x52].fill(0);
                rec[0x54..0x56].fill(0);
                assert_eq!(actor, rec, "run {run}");
                spawned += 1;
            }
            _ => {
                assert_eq!(cpu.regs.r[V0], 0, "run {run}");
                assert_eq!(actor, [0xEE; 0x100], "run {run}");
                already += u32::from(outcome == SubmodeOpen::AlreadyOpen);
            }
        }
    }
    assert!(spawned > 100 && already > 100, "{spawned} {already}");
}

#[test]
fn the_scene_actor_spawn_matches_retail() {
    differential!(prot(897, SCENE_ACTOR));
    let Some(mut cpu) = cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let head = 0x8014_0000;
    cpu.mem.wr32(POOL_LISTS + 4, head);
    cpu.hook(ALLOC, move |r, m| {
        assert_eq!((r.r[A0], r.r[A1]), (SCENE_ACTOR_TEMPLATE, head));
        m.load(SPAWNED, &[0x5A; 0x100]);
        r.r[V0] = SPAWNED;
    });
    let mut rng = Rng(0x5343_4143);
    let mut forced = 0;
    for run in 0..2000 {
        let requested = match rng.below(3) {
            0 => 0x0F,
            _ => rng.next() as u16,
        };
        let mode_flags = [0, 0, 1, rng.next()][rng.below(4) as usize];
        cpu.mem.wr32(MODE_FLAGS, mode_flags);
        assert_eq!(
            cpu.call(SCENE_ACTOR, &[u32::from(requested)]),
            Stop::Returned,
            "run {run}"
        );
        let state = scene_actor_initial_state(requested, mode_flags);
        let mut rec = vec![0x5Au8; 0x100];
        rec[0x54..0x56].copy_from_slice(&state.to_le_bytes());
        assert_eq!(cpu.mem.read(SPAWNED, 0x100), rec, "run {run}");
        forced += u32::from(mode_flags != 0);
    }
    assert!(forced > 500, "{forced}");
}
