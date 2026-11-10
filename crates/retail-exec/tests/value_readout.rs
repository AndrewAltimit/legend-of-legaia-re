//! The multi-cast value readout pass `FUN_801E805C`, entered at its head:
//! the teardown against `readout_teardown` (the flag, the string total, the
//! value window and the queue the overlay image carries), and the value
//! window's render against `label_quad` and `multi_row_cluster`, with each
//! row's widget seated where the port seats it (`MULTI_ROW_X` plus a glide,
//! the value row) and values of one to five digits.
//!
//! `FUN_801D8DE8` and `AddPrim` `FUN_8003D2C4` are hooked; every packet is
//! compared whole in link order. The pass runs the combo fold and cluster
//! only with the window empty; here the cluster flag `ctx[+0x271]` is shut,
//! so a window-empty pass ends at the fold's gate (`combo_records` runs the
//! fold itself).

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_battle_vm::battle_value_readout::{
    CODE_COLOUR, DIGIT_CELL, GLYPH_CLUT, GLYPH_TPAGE, MULTI_ROW_VALUE_Y, MULTI_ROW_X, OT_TAG,
    decimal_digits, label_quad, multi_row_cluster, readout_teardown,
};
use psx_retail_exec::{A0, A1, Stop};
use std::cell::RefCell;
use std::rc::Rc;

const PASS: u32 = 0x801E_805C;
const UI_ELEMENT: u32 = 0x801D_8DE8;
const ADD_PRIM: u32 = 0x8003_D2C4;
const OVERLAY_BASE: u32 = 0x801C_E818;
const CTX_PTR: u32 = 0x8007_BD24;
const CTX: u32 = 0x8012_0000;
const FLAG: u32 = 0x8007_B64C;
const TOTAL: u32 = 0x8007_BD14;
const COUNT: u32 = 0x801F_6974;
const VALUES: u32 = 0x801F_6980;
const SLOT_IDS: u32 = 0x801F_6988;
const QUEUE: u32 = 0x801F_6834;
const PACKET_PTR: u32 = 0x1F80_03A0;
const OT_PTR: u32 = 0x1F80_03F4;
const OT: u32 = 0x8014_0000;
const PACKETS: u32 = 0x8015_0000;
const WIDGETS: u32 = 0x8016_0000;

fn packet(xy: [(i32, i32); 4], uv: [(u8, u8); 4]) -> Vec<u32> {
    let p = |(x, y): (i32, i32)| (x as u16 as u32) | (y as u16 as u32) << 16;
    let t = |(u, v): (u8, u8)| u32::from(u) | u32::from(v) << 8;
    vec![
        OT_TAG,
        CODE_COLOUR,
        p(xy[0]),
        t(uv[0]) | u32::from(GLYPH_CLUT) << 16,
        p(xy[1]),
        t(uv[1]) | u32::from(GLYPH_TPAGE) << 16,
        p(xy[2]),
        t(uv[2]),
        p(xy[3]),
        t(uv[3]),
    ]
}

#[test]
fn the_value_readout_tears_down_and_draws_as_retail() {
    differential!(prot(898, PASS));
    let (Some(mut cpu), Some(disc)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &disc.read_entry(898).expect("PROT 0898"));
    let queue: [u8; 16] = cpu.mem.read(QUEUE, 16).try_into().unwrap();
    cpu.mem.wr32(CTX_PTR, CTX);
    cpu.mem.wr32(OT_PTR, OT);
    let closed = Rc::new(RefCell::new(Vec::new()));
    {
        let closed = closed.clone();
        cpu.hook(UI_ELEMENT, move |r, _| {
            assert_eq!(r.r[A1], 0);
            closed.borrow_mut().push(r.r[A0] as i32);
        });
    }
    let linked = Rc::new(RefCell::new(Vec::new()));
    {
        let linked = linked.clone();
        cpu.hook(ADD_PRIM, move |r, _| {
            assert_eq!(r.r[A0], OT, "the OT head");
            linked.borrow_mut().push(r.r[A1]);
        });
    }
    let mut rng = Rng(0x5245_4144);
    let (mut torn, mut rows, mut wide) = (0u32, [0u32; 6], 0u32);
    for n in 0..20_000 {
        let flag = if rng.below(3) == 0 {
            0
        } else {
            1 + rng.below(0xFF) as u8
        };
        let total = if rng.below(2) == 0 { 0 } else { rng.next() };
        let count = rng.below(5);
        let values: [u16; 4] = std::array::from_fn(|k| {
            if rng.below(4) == 0 || k as u32 >= count.max(1) {
                0
            } else {
                match rng.below(5) {
                    0 => rng.below(10) as u16,
                    1 => rng.below(100) as u16,
                    2 => rng.below(1000) as u16,
                    3 => rng.below(10000) as u16,
                    _ => rng.next() as u16,
                }
            }
        });
        let slide = rng.below(160) as i32;
        cpu.mem.wr8(FLAG, flag);
        cpu.mem.wr32(TOTAL, total);
        cpu.mem.wr32(COUNT, count);
        for (k, v) in values.iter().enumerate() {
            cpu.mem.wr16(VALUES + 2 * k as u32, *v);
        }
        // Slot `s` holds the row of value `count - 1 - s`, its widget seated
        // where the port seats that row.
        for s in 0..4u32 {
            let live = s < count;
            let id = if live { 1 + s as u8 } else { 0 };
            cpu.mem.wr8(SLOT_IDS + s, id);
            let w = WIDGETS + 0x40 * s;
            cpu.mem
                .wr8(CTX + 0x11B6 + 0xC * u32::from(id), 0x10 + s as u8);
            cpu.mem.wr32(CTX + 0x1074 + 4 * (0x10 + s), w);
            cpu.mem
                .wr16(w + 0xA, (MULTI_ROW_X[s as usize] + slide) as u16);
            cpu.mem.wr16(w + 0xC, MULTI_ROW_VALUE_Y as u16);
        }
        cpu.mem.wr8(CTX + 0x271, 0);
        cpu.mem.wr32(PACKET_PTR, PACKETS);
        closed.borrow_mut().clear();
        linked.borrow_mut().clear();
        assert_eq!(cpu.call(PASS, &[]), Stop::Returned, "n {n}");

        // The teardown.
        let (want_closed, want_flag) = readout_teardown(flag, total, values, count, &queue);
        let ctx = format!("n {n} flag {flag} total {total} count {count} values {values:?}");
        assert_eq!(*closed.borrow(), want_closed, "{ctx}");
        assert_eq!(cpu.mem.rd8(FLAG), want_flag, "{ctx}");
        torn += u32::from(!want_closed.is_empty());

        // The render: rows of the window in the port's order, and the label
        // over slot 0's widget.
        let mut want: Vec<Vec<u32>> = Vec::new();
        if count > 0 {
            let row_values: Vec<u32> = (0..count as usize).map(|k| u32::from(values[k])).collect();
            let cluster = multi_row_cluster(&row_values, slide);
            let label = label_quad((MULTI_ROW_X[0] + slide) as u16, MULTI_ROW_VALUE_Y as u16);
            let lx = label.xy.map(|(x, y)| (i32::from(x), i32::from(y)));
            assert_eq!(cluster.labels.len(), 1);
            assert_eq!(
                (cluster.labels[0].x, cluster.labels[0].y),
                lx[0],
                "{ctx}: the cluster's label is label_quad's"
            );
            want.push(packet(lx, label.uv));
            // Retail walks slots 0.. (the last value first), each row's
            // units digit first.
            let per_row: Vec<(usize, usize)> = values[..count as usize]
                .iter()
                .enumerate()
                .map(|(k, v)| (k, decimal_digits(*v).len()))
                .collect();
            let mut starts = Vec::new();
            let mut s0 = 0;
            for (_, len) in &per_row {
                starts.push(s0);
                s0 += len;
            }
            for s in 0..count as usize {
                let k = count as usize - 1 - s;
                let cells = &cluster.cells[starts[k]..starts[k] + per_row[k].1];
                for c in cells.iter().rev() {
                    let (x0, y0) = (c.x, c.y);
                    let (x1, y1) = (c.x + c.w as i32 - 1, c.y + c.h as i32 - 1);
                    let (u0, v0) = (c.u, c.v);
                    let (u1, v1) = (c.u + DIGIT_CELL - 1, c.v + DIGIT_CELL - 1);
                    want.push(packet(
                        [(x0, y0), (x1, y0), (x0, y1), (x1, y1)],
                        [(u0, v0), (u1, v0), (u0, v1), (u1, v1)],
                    ));
                }
                rows[cells.len()] += 1;
                wide += u32::from(values[k] >= 10_000);
            }
        }
        let got: Vec<Vec<u32>> = linked
            .borrow()
            .iter()
            .map(|&p| (0..10).map(|k| cpu.mem.rd32(p + 4 * k)).collect())
            .collect();
        assert_eq!(got.len(), want.len(), "{ctx}");
        for (q, (a, b)) in got.iter().zip(&want).enumerate() {
            assert_eq!(a, b, "{ctx} packet {q}");
        }
    }
    eprintln!("{torn} teardowns, rows by digits {rows:?}, {wide} wide");
    assert!(
        torn > 5000 && rows[1..].iter().all(|&c| c > 300),
        "{rows:?}"
    );
}
