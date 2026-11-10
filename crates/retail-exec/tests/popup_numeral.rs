//! The per-hit floating numeral `FUN_801DF6B8` against `popup_anchor_at_step`
//! and `popup_cells`: the 8-entry popup ring from any start index, live and
//! empty entries, values of one to five digits, ring ages across the whole
//! 37-frame life and past it, frame steps `1..=4`, display trios anywhere,
//! and projected squares from a pixel to past the 24-px cap, near every
//! clamp edge.
//!
//! The projection `FUN_800195A8` is hooked: its anchor and half-extent are
//! compared, and it answers a square the test draws (at an OT depth of
//! `0x80` or more - the routine's near arm under `0x80` has no port, see the
//! differential doc). `AddPrim` `FUN_8003D2C4` is hooked to collect the
//! packets in link order; each `POLY_FT4` is compared whole with the port's
//! cell, and the ring's timers and values after the walk.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_battle_vm::battle_value_readout::{
    CODE_COLOUR, GLYPH_CLUT, GLYPH_TPAGE, OT_TAG, POPUP_TIMER_LIMIT, popup_anchor_at_step,
    popup_cells, popup_timer_at_step,
};
use psx_retail_exec::{A0, A1, A2, A3, SP, Stop, V0};
use std::cell::RefCell;
use std::rc::Rc;

const POPUP: u32 = 0x801D_F6B8;
const PROJECT: u32 = 0x8001_95A8;
const ADD_PRIM: u32 = 0x8003_D2C4;
const OVERLAY_BASE: u32 = 0x801C_E818;
const CTX_PTR: u32 = 0x8007_BD24;
const CTX: u32 = 0x8012_0000;
const ACTORS: u32 = 0x801C_9370;
const ACTOR: u32 = 0x8013_0000;
const FRAME_STEP: u32 = 0x1F80_0393;
const PACKET_PTR: u32 = 0x1F80_03A0;
const OT_PTR: u32 = 0x1F80_03F4;
const OT: u32 = 0x8014_0000;
const PACKETS: u32 = 0x8015_0000;

#[test]
fn the_popup_numeral_lays_out_as_retail() {
    differential!(prot(898, POPUP));
    let (Some(mut cpu), Some(disc)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &disc.read_entry(898).expect("PROT 0898"));
    cpu.mem.wr32(CTX_PTR, CTX);
    cpu.mem.wr32(OT_PTR, OT);
    for s in 0..8 {
        cpu.mem.wr32(ACTORS + s * 4, ACTOR + s * 0x400);
    }
    // The projection answers the squares queued for it, in call order.
    let squares = Rc::new(RefCell::new(Vec::<(i16, i16, i16, i16)>::new()));
    let projected = Rc::new(RefCell::new(Vec::new()));
    {
        let squares = squares.clone();
        let projected = projected.clone();
        cpu.hook(PROJECT, move |r, m| {
            let v = r.r[A0];
            let anchor = [m.rd16(v) as i16, m.rd16(v + 2) as i16, m.rd16(v + 4) as i16];
            projected
                .borrow_mut()
                .push((anchor, r.r[A1] as i32, r.r[A2] as i32, r.r[A3]));
            let (x0, y0, x1, y1) = squares.borrow_mut().remove(0);
            let corners = [(x0, y0), (x1, y0), (x0, y1), (x1, y1)];
            for (k, (x, y)) in corners.iter().enumerate() {
                let p = m.rd32(r.r[SP] + 0x10 + 4 * k as u32);
                m.wr16(p, *x as u16);
                m.wr16(p + 2, *y as u16);
            }
            r.r[V0] = 0x80 + (v & 0x3FF);
        });
    }
    let linked = Rc::new(RefCell::new(Vec::new()));
    {
        let linked = linked.clone();
        cpu.hook(ADD_PRIM, move |r, _| {
            assert_eq!(r.r[A0], OT + 4);
            linked.borrow_mut().push(r.r[A1]);
        });
    }
    let mut rng = Rng(0x504F_5055);
    let (mut cells, mut expired, mut by_digits) = (0u32, 0u32, [0u32; 6]);
    for n in 0..6_000 {
        let start = rng.below(8) as u8;
        let step = 1 + rng.below(4) as u8;
        cpu.mem.wr8(CTX + 0x262, start);
        cpu.mem.wr8(FRAME_STEP, step);
        cpu.mem.wr32(PACKET_PTR, PACKETS);
        let mut entries = Vec::new();
        for i in 0..8u32 {
            let live = rng.below(3) != 0;
            let value: u16 = if !live {
                0
            } else {
                match rng.below(5) {
                    0 => rng.below(10) as u16 + 1,
                    1 => rng.below(100) as u16 + 1,
                    2 => rng.below(1000) as u16 + 1,
                    3 => rng.below(10000) as u16 + 1,
                    _ => rng.next() as u16 | 1,
                }
            };
            let age = rng.below(40) as u16;
            let seat = rng.below(7) as u8;
            cpu.mem.wr32(CTX + 0x83C + 4 * i, u32::from(value));
            cpu.mem.wr32(CTX + 0x85C + 4 * i, u32::from(age) * 16);
            cpu.mem.wr8(CTX + 0x318 + 2 * i, seat);
            entries.push((value, age, seat));
        }
        let mut trios = Vec::new();
        for s in 0..7u32 {
            let t: [i16; 3] = std::array::from_fn(|_| (rng.below(0x2000) as i32 - 0x1000) as i16);
            for (k, v) in t.iter().enumerate() {
                cpu.mem
                    .wr16(ACTOR + s * 0x400 + 0x3C + 2 * k as u32, *v as u16);
            }
            trios.push(t);
        }
        // One square per live entry, in ring order from the start index.
        let order: Vec<usize> = (0..8).map(|k| (k + usize::from(start)) & 7).collect();
        let mut want_squares = Vec::new();
        for &i in &order {
            if entries[i].0 == 0 {
                continue;
            }
            let x0 = rng.below(360) as i16 - 20;
            let y0 = rng.below(260) as i16 - 20;
            let w = match rng.below(3) {
                0 => rng.below(4) as i16,
                1 => rng.below(40) as i16,
                _ => 10 + rng.below(16) as i16,
            };
            want_squares.push((x0, y0, x0 + w, y0 + w));
        }
        *squares.borrow_mut() = want_squares.clone();
        projected.borrow_mut().clear();
        linked.borrow_mut().clear();
        assert_eq!(cpu.call(POPUP, &[]), Stop::Returned, "n {n}");

        // The port, entry by entry.
        let mut want_proj = Vec::new();
        let mut want_packets: Vec<Vec<u32>> = Vec::new();
        let mut sq = want_squares.iter();
        for &i in &order {
            let (value, age, seat) = entries[i];
            let timer = popup_timer_at_step(age, step);
            if value != 0 {
                let trio = trios[usize::from(seat)].map(i32::from);
                let (anchor, half) = popup_anchor_at_step(trio, age, step);
                want_proj.push((anchor.map(|v| v as i16), half, half, 0));
                let &(x0, y0, x1, y1) = sq.next().unwrap();
                let rect = (i32::from(x0), i32::from(y0), i32::from(x1), i32::from(y1));
                let laid = popup_cells(value, rect, timer);
                by_digits[laid.len()] += 1;
                // Retail links the units digit first, then each higher one.
                for c in laid.iter().rev() {
                    let (x0, y0) = (c.x, c.y);
                    let (x1, y1) = (c.x + c.w as i32 - 1, c.y + c.h as i32 - 1);
                    let xy = |x: i32, y: i32| (x as u16 as u32) | (y as u16 as u32) << 16;
                    let uv = |u: u8, v: u8| u32::from(u) | u32::from(v) << 8;
                    let (u0, v0) = (c.u, c.v);
                    let (u1, v1) = (c.u + c.cell - 1, c.v + c.cell - 1);
                    want_packets.push(vec![
                        OT_TAG,
                        CODE_COLOUR,
                        xy(x0, y0),
                        uv(u0, v0) | u32::from(GLYPH_CLUT) << 16,
                        xy(x1, y0),
                        uv(u1, v0) | u32::from(GLYPH_TPAGE) << 16,
                        xy(x0, y1),
                        uv(u0, v1),
                        xy(x1, y1),
                        uv(u1, v1),
                    ]);
                    cells += 1;
                }
            }
            // The ring after the walk: a live entry's timer stepped, its value
            // zeroed past the limit.
            let i = i as u32;
            let stepped = if value != 0 {
                timer
            } else {
                i32::from(age) * 16
            };
            assert_eq!(
                cpu.mem.rd32(CTX + 0x85C + 4 * i),
                stepped as u32,
                "n {n} {i}"
            );
            let keep = value != 0 && timer <= POPUP_TIMER_LIMIT;
            expired += u32::from(value != 0 && !keep);
            assert_eq!(
                cpu.mem.rd32(CTX + 0x83C + 4 * i),
                if keep { u32::from(value) } else { 0 },
                "n {n} entry {i} age {age} step {step}"
            );
        }
        assert_eq!(*projected.borrow(), want_proj, "n {n}");
        let got: Vec<Vec<u32>> = linked
            .borrow()
            .iter()
            .map(|&p| (0..10).map(|k| cpu.mem.rd32(p + 4 * k)).collect())
            .collect();
        assert_eq!(got.len(), want_packets.len(), "n {n}");
        for (q, (a, b)) in got.iter().zip(&want_packets).enumerate() {
            assert_eq!(a, b, "n {n} packet {q} {entries:?}");
        }
    }
    eprintln!("{cells} cells, {expired} expired, by digits {by_digits:?}");
    assert!(
        by_digits[1..].iter().all(|&c| c > 300) && expired > 300,
        "{by_digits:?}"
    );
}
