//! The field's visible-tile crop off the disc: the decoration pass
//! `FUN_801F7088` (PROT 0900) run whole over random maps, focus words,
//! visible-tile windows and walk-region boxes, its per-cell draws (the
//! dispatcher `FUN_80043390`) and its ground emitter (`FUN_801F6D48` or
//! `FUN_801F69EC` on `_DAT_8007BB4C`) answered by hooks that log the cell and
//! the arguments. Held against `view_cells` and `ViewCells::decoration_visible`:
//! every cell drawn, in walk order; the emitter picked, its first cell, its
//! sub-tile origin and the clamped window it reads; and the scratchpad bytes
//! the pass restores on its way out.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_field::camera::FieldCullView;
use legaia_engine_scene::field_view_window::{ViewCells, view_cells};
use psx_retail_exec::{A0, A1, A2, A3, Stop};
use std::cell::RefCell;
use std::rc::Rc;

const PASS: u32 = 0x801F_7088;
const DISPATCH: u32 = 0x8004_3390;
const GROUND: u32 = 0x801F_6D48;
const GROUND_ALT: u32 = 0x801F_69EC;
const SLOT_B_BASE: u32 = 0x801F_69D8;
const MAP_PTR: u32 = 0x1F80_03EC;
const WINDOW: u32 = 0x1F80_03E8;
const REGION: u32 = 0x1F80_0384;
const FIRST: u32 = 0x1F80_02BC;
const FOCUS_X: u32 = 0x8008_9118;
const FOCUS_Z: u32 = 0x8008_9120;
const ALT_EMITTER: u32 = 0x8007_BB4C;
const MODEL_BIAS: u32 = 0x8007_B6F8;
const MODELS: u32 = 0x8007_C018;
const MAP: u32 = 0x8012_0000;
const DUMMY_MODEL: u32 = 0x8013_0000;

#[derive(Debug, Default)]
struct Log {
    drawn: Vec<(i32, i32)>,
    /// `(alt emitter, a0..a3, window bytes at the call)`.
    ground: Vec<(bool, [i32; 4], [i8; 4])>,
}

#[test]
fn the_visible_tile_crop_matches_the_pass() {
    differential!(prot(900, PASS));
    let (Some(mut cpu), Some(disc)) = (scus_cpu(), disc()) else {
        return;
    };
    cpu.mem
        .load(SLOT_B_BASE, &disc.read_entry(900).expect("PROT 0900"));
    cpu.mem.wr32(0x8007_B81C, 0x8007_0A2C);
    cpu.mem.wr32(0x8007_B7F8, 0x8007_122C);
    cpu.mem.wr32(MAP_PTR, MAP);
    cpu.mem.wr32(MODEL_BIAS, 0);
    for k in 0..0x400 {
        cpu.mem.wr32(MODELS + 4 * k, DUMMY_MODEL);
    }
    cpu.mem.load(DUMMY_MODEL, &[0; 0x40]);
    let log = Rc::new(RefCell::new(Log::default()));
    {
        let log = log.clone();
        cpu.hook(DISPATCH, move |r, m| {
            // The loop's column and row counters sit in s2 / s3; the walk's
            // origin is the scratchpad pair the prologue left.
            let col = m.rd32(FIRST) as i32 + r.r[18] as i32;
            let row = m.rd32(FIRST + 4) as i32 + r.r[19] as i32;
            log.borrow_mut().drawn.push((col, row));
        });
    }
    for (va, alt) in [(GROUND, false), (GROUND_ALT, true)] {
        let log = log.clone();
        cpu.hook(va, move |r, m| {
            let w: [i8; 4] = std::array::from_fn(|k| m.rd8(WINDOW + k as u32) as i8);
            let args = [A0, A1, A2, A3].map(|i| r.r[i] as i32);
            log.borrow_mut().ground.push((alt, args, w));
        });
    }
    let mut rng = Rng(0x5649_4557);
    let (mut drawn, mut clamped, mut alt_runs) = (0usize, 0usize, 0usize);
    for round in 0..1500 {
        // A map: records with their placed flag and cull radius, a grid
        // with the `0x2000` decoration bit on some cells.
        let mut map = vec![0u8; 0x10000];
        for b in map.iter_mut() {
            *b = rng.next() as u8;
        }
        let deco_odds = 2 + rng.below(4);
        for cell in 0..0x4000usize {
            let at = 0x8000 + 2 * cell;
            let mut w = u16::from_le_bytes([map[at], map[at + 1]]) & !0x2000;
            if rng.below(deco_odds) == 0 {
                w |= 0x2000;
            }
            map[at..at + 2].copy_from_slice(&w.to_le_bytes());
        }
        for rec in 0..0x200usize {
            map[rec * 0x20 + 0x1E] = rng.below(4) as u8;
            // The model index stays inside the table the test seeds.
            map[rec * 0x20 + 0x11] = 0;
            map[rec * 0x20 + 0x10] &= 0x3F;
        }
        cpu.mem.load(MAP, &map);

        let region = {
            let x0 = rng.below(0x60) as u8;
            let z0 = rng.below(0x60) as u8;
            [
                x0,
                z0,
                x0 + 4 + rng.below(0x1C) as u8,
                z0 + 4 + rng.below(0x1C) as u8,
            ]
        };
        let window: [i8; 4] = [
            -(rng.below(10) as i8),
            -(rng.below(10) as i8),
            rng.below(10) as i8,
            rng.below(12) as i8,
        ];
        // The focus, stored negated, around the region (and now and then
        // well outside it, where the clamp moves the window).
        let spread = if round % 4 == 0 { 0x2000 } else { 0x800 };
        let fx =
            i32::from(region[0] + region[2]) * 64 + rng.below(2 * spread) as i32 - spread as i32;
        let fz =
            i32::from(region[1] + region[3]) * 64 + rng.below(2 * spread) as i32 - spread as i32;
        let focus_stored = [-fx, -fz];
        let alt = rng.below(4) == 0;
        cpu.mem.wr32(FOCUS_X, focus_stored[0] as u32);
        cpu.mem.wr32(FOCUS_Z, focus_stored[1] as u32);
        for k in 0..4 {
            cpu.mem.wr8(REGION + k as u32, region[k]);
            cpu.mem.wr8(WINDOW + k as u32, window[k] as u8);
        }
        cpu.mem.wr32(ALT_EMITTER, u32::from(alt));
        *log.borrow_mut() = Log::default();
        assert_eq!(cpu.call(PASS, &[]), Stop::Returned, "round {round}");

        let cells = view_cells(&FieldCullView {
            focus_stored,
            attr_box: region,
            window,
        });
        let mut want = Vec::new();
        for j in 0..cells.span[1] {
            for i in 0..cells.span[0] {
                let (col, row) = (cells.first[0] - 1 + i, cells.first[1] - 3 + j);
                // The draw list's own gates: the cell's decoration bit and
                // its record's placed flag.
                if !(0..0x80).contains(&col) || !(-1..0x80).contains(&row) {
                    continue;
                }
                let at = (0x8000 + row * 0x100 + col * 2) as usize;
                let word = u16::from_le_bytes([map[at], map[at + 1]]);
                if word & 0x2000 == 0 {
                    continue;
                }
                let rec = usize::from(word & 0x1FF) * 0x20;
                if map[rec + 0x12] & 4 != 0 {
                    continue;
                }
                if cells.decoration_visible(col, row, map[rec + 0x1E]) {
                    want.push((col, row));
                }
            }
        }
        let l = log.borrow();
        let ctx =
            format!("round {round} focus {focus_stored:?} window {window:?} region {region:?}");
        assert_eq!(l.drawn, want, "decoration cells, {ctx}");
        let ground = (
            alt,
            [
                cells.first[0],
                cells.first[1] - 1,
                cells.sub[0],
                cells.sub[1],
            ],
            cells.window,
        );
        assert_eq!(l.ground, vec![ground], "ground emitter, {ctx}");
        let back: [i8; 4] = std::array::from_fn(|k| cpu.mem.rd8(WINDOW + k as u32) as i8);
        assert_eq!(back, window, "window restored, {ctx}");
        let rback: [u8; 4] = std::array::from_fn(|k| cpu.mem.rd8(REGION + k as u32));
        assert_eq!(rback, region, "region restored, {ctx}");
        drawn += want.len();
        clamped += usize::from(cells.window != window);
        alt_runs += usize::from(alt);
    }
    assert!(
        drawn > 5000 && clamped > 100 && alt_runs > 0,
        "drawn {drawn} clamped {clamped} alt {alt_runs}"
    );
}

/// The ground emitter's cell loop (`0x801F6DE0..0x801F7058`): the cell each
/// pass of the inner loop reads, as the column `a0` and row `a1` stand at the
/// grid read `0x801F6DF8`.
const GROUND_VISIT: u32 = 0x801F_6DF8;
const PACKET_CURSOR: u32 = 0x1F80_03A0;
const OT_PTR: u32 = 0x1F80_03F4;
const PACKETS: u32 = 0x8015_0000;
const OT: u32 = 0x8018_0000;

#[test]
fn the_ground_emitter_visits_the_cells_the_gate_names() {
    differential!(prot(900, GROUND));
    let (Some(mut cpu), Some(disc)) = (scus_cpu(), disc()) else {
        return;
    };
    cpu.mem
        .load(SLOT_B_BASE, &disc.read_entry(900).expect("PROT 0900"));
    cpu.mem.wr32(MAP_PTR, MAP);
    // Identity rotation, the scene well in front: most cells pass the depth
    // gate (`SZ - 0x40 >= 0`) and write their packet.
    for (r, v) in [
        (0, 0x1000),
        (2, 0x1000),
        (4, 0x1000),
        (7, 0x4000),
        (26, 0x100),
    ] {
        cpu.gte.write_ctrl(r, v);
    }
    let visits = Rc::new(RefCell::new(Vec::<(i32, i32)>::new()));
    {
        let visits = visits.clone();
        cpu.probe(GROUND_VISIT, move |r, _, _| {
            visits.borrow_mut().push((r.r[A0] as i32, r.r[A1] as i32));
        });
    }
    let mut rng = Rng(0x4752_4E44);
    let (mut visited, mut drew, mut short) = (0usize, 0u32, 0usize);
    for round in 0..3000 {
        // A grid whose `0x1000` cells take the draw path, so the loop is run
        // with its packet writes in between, and random heights and records.
        let mut map = vec![0u8; 0x10000];
        for b in map.iter_mut() {
            *b = rng.next() as u8;
        }
        cpu.mem.load(MAP, &map);
        cpu.mem.wr32(PACKET_CURSOR, PACKETS);
        cpu.mem.wr32(OT_PTR, OT);
        cpu.mem.wr8(0x1F80_03A4, 0);
        // The window as the decoration pass writes it back: the clamp leaves
        // `E8 <= EA`, `E9 <= EB` mostly, and now and then an empty or
        // inverted span, which the `do`-`while` loops still run once.
        let window: [i8; 4] = if round % 8 == 0 {
            std::array::from_fn(|_| rng.below(20) as i8 - 10)
        } else {
            let (c0, r0) = (-(rng.below(10) as i8), -(rng.below(10) as i8));
            [c0, r0, c0 + rng.below(20) as i8, r0 + rng.below(22) as i8]
        };
        for k in 0..4 {
            cpu.mem.wr8(WINDOW + k as u32, window[k] as u8);
        }
        let first = [rng.below(0x80) as i32, rng.below(0x81) as i32];
        let sub = [rng.below(0x80), rng.below(0x80)];
        let cells = ViewCells {
            first,
            window,
            sub: sub.map(|s| s as i32),
            span: [0, 0],
            region: [0, 0, 0x80, 0x80],
            window_in: window,
        };
        visits.borrow_mut().clear();
        let a1 = (first[1] - 1) as u32;
        let stop = cpu.call(GROUND, &[first[0] as u32, a1, sub[0], sub[1]]);
        assert_eq!(stop, Stop::Returned, "round {round}");
        let got = visits.borrow().clone();
        // Every cell the gate admits over a band wider than any window, in
        // the loop's order: rows, then columns.
        let mut want = Vec::new();
        for dr in 0..0x80 {
            let row = ((first[1] - 1) + dr) & 0x7F;
            for col in first[0] - 2..first[0] + 0x30 {
                if cells.ground_visible(col, row) {
                    want.push((col, row));
                }
            }
        }
        let ctx = format!("round {round} first {first:?} window {window:?}");
        let mut sorted = got.clone();
        sorted.sort_by_key(|&(c, r)| (((r - (first[1] - 1)) & 0x7F), c));
        assert_eq!(sorted, got, "the walk order, {ctx}");
        assert_eq!(got, want, "cells visited, {ctx}");
        visited += got.len();
        drew += (cpu.mem.rd32(PACKET_CURSOR) - PACKETS) / 0x28;
        short += usize::from(window[2] <= window[0] || window[3] <= window[1]);
    }
    assert!(
        visited > 50_000 && drew > 1000 && short > 50,
        "visited {visited} drew {drew} short {short}"
    );
}
