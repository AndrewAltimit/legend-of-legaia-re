//! The save screen (PROT 0899): the guarded box `FUN_801E4140`, the centred
//! caption `FUN_801E3EE0` off the live measurer, and the slot-grid dispatcher
//! `FUN_801E06C0`
//! against `slot_grid_quad_x`: every cell's x over the whole commit slide
//! (`t = *0x801EF1A0`, `0..=0x1000`), read off the per-cell drawer
//! `FUN_801E0FD0`'s pen, which is hooked; the info panel and its helper are
//! stubbed. And the info panel's mode selector `FUN_801E3F74` against
//! `SlotInfoMode::for_grid_cell`, the title strips behind the Load window
//! `FUN_801E0418` against `title_strip_rows`, and the record quad
//! `FUN_801E3FF0` against `save_ui_record_quad`, and the per-cell drawer
//! `FUN_801E0FD0` (live, its record quads hooked) against
//! `slot_preview_grid_draws_for`.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_menus::save_select::{SlotContent, SlotInfoMode, SlotSnapshot};
use legaia_engine_ui::{
    SLOT_GRID_COLS, SLOT_GRID_ROWS, save_ui_record_quad, slot_grid_quad_x, title_strip_rows,
};
use psx_retail_exec::{A0, A1, A2, A3, Stop, V0};
use std::cell::RefCell;
use std::rc::Rc;

const OVERLAY_BASE: u32 = 0x801C_E818;
const GRID: u32 = 0x801E_06C0;
const CELL: u32 = 0x801E_0FD0;
const SLIDE: u32 = 0x801E_F1A0;
const MODE: u32 = 0x801E_3F74;
const STRIPS: u32 = 0x801E_0418;
/// The strips' frame clock, advanced `0x20` per skipped frame.
const STRIP_CLOCK: u32 = 0x801F_3294;
const RECORD_QUAD: u32 = 0x801E_3FF0;
/// The save UI's sprite-record table (`[clut][u][v][w][h]`, 12-byte stride,
/// eight records).
const RECORDS: u32 = 0x801E_5048;

/// One `FUN_801E2EE4(mode, x, y, record, brightness, scale)` call.
type StripCall = (u32, i32, i32, u32, i32, u32);
/// One `FUN_801E3FF0(record, rgb, x, y)` call, with the face record's `u`.
type QuadCall = (u32, u32, i32, i32, u32);
/// The per-slot scanned flags and class bytes the card walk writes, and the
/// card op flag (`0` save, `1` load).
const SCANNED: u32 = 0x801F_2A68;
const CLASSES: u32 = 0x801F_2A48;
const OP_FLAG: u32 = 0x801F_0200;

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

const BOX: u32 = 0x801E_4140;
const CAPTION: u32 = 0x801E_3EE0;

#[test]
fn the_guarded_box_and_the_caption_match() {
    differential!(prot(899, BOX), prot(899, CAPTION));
    let (Some(mut cpu), Some(d), Some(scus)) = (scus_cpu(), disc(), crate::common::scus()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem.load(
        OVERLAY_BASE,
        &d.read_entry_footprint(899).expect("PROT 0899"),
    );
    let tim = d
        .read_prot_bytes(
            legaia_font::FONT_TIM_PROT_DAT_OFFSET,
            legaia_font::FONT_TIM_LEN,
        )
        .expect("font TIM");
    let font = legaia_font::Font::from_disc_tim_and_scus(&tim, &scus).expect("font");
    type Log = Rc<RefCell<Vec<(u32, [i32; 4])>>>;
    let log: Log = Rc::default();
    let l = log.clone();
    cpu.hook(0x8002_C69C, move |r, _| {
        l.borrow_mut().push((
            0x8002_C69C,
            [r.r[A0], r.r[A1], r.r[A2], r.r[psx_retail_exec::A3]].map(|v| v as i32),
        ))
    });
    let l = log.clone();
    cpu.hook(0x8003_6888, move |r, m| {
        let y = m.rd32(r.r[psx_retail_exec::SP] + 0x10) as i32;
        l.borrow_mut()
            .push((r.r[A0], [r.r[psx_retail_exec::A3] as i32, y, 0, 0]))
    });
    cpu.stub(0x8003_4B6C, 0);
    cpu.stub(0x8003_CA38, 1);
    // BIOS A(0x2B) `memset`, the measurer's scratch-line clear.
    cpu.hook(0x8005_6788, |r, m| {
        for i in 0..r.r[A2] {
            m.wr8(r.r[A0] + i, r.r[A1] as u8);
        }
    });
    let mut rng = Rng(0x424F_5843);
    for run in 0..4_000 {
        let (x, y, w, h) = (
            rng.below(0x140) as i32,
            [rng.below(0x100) as i32, 0xF0, 0xF1][rng.below(3) as usize],
            rng.below(0x100) as i32,
            rng.below(0x80) as i32,
        );
        for (i, v) in [w, h].iter().enumerate() {
            cpu.mem
                .wr32(crate::common::STACK_TOP + 0x10 + 4 * i as u32, *v as u32);
        }
        log.borrow_mut().clear();
        assert_eq!(
            cpu.call(BOX, &[0x44, 0x0220_2020, x as u32, y as u32]),
            Stop::Returned
        );
        let want: Vec<(u32, [i32; 4])> =
            legaia_engine_ui::menu::window_painters::guarded_box_rect(x, y, w, h)
                .map(|(a, b, c, d)| (0x8002_C69C, [a, b, c, d]))
                .into_iter()
                .collect();
        assert_eq!(*log.borrow(), want, "run {run}");
    }
    // The caption, centred on 0xA0 off the retail measurer.
    for (run, caption) in [
        "Not a Legend of Legaia save.",
        "Able to save.",
        "No data",
        "Return",
        "A",
        "Wide WWW caption",
    ]
    .iter()
    .enumerate()
    {
        let text = 0x8013_0000;
        cpu.mem.load(text, caption.as_bytes());
        cpu.mem.wr8(text + caption.len() as u32, 0);
        let offset = rng.below(0x40) as i32 - 0x20;
        let y = legaia_engine_ui::SLOT_INFO_PANEL_PARKED_Y + offset + 0x18;
        log.borrow_mut().clear();
        assert_eq!(cpu.call(CAPTION, &[text, 0xA0, y as u32]), Stop::Returned);
        let (_, [rx, ry, ..]) = log.borrow()[0];
        let port = legaia_engine_ui::slot_info_caption_draws_for(&font, caption, offset, (0, 0), 1);
        let want = legaia_engine_ui::text_draws_for(
            &font.layout(caption.as_bytes()),
            (rx, ry),
            legaia_engine_ui::SAVE_SELECT_TITLE_COLOR,
        );
        let key = |d: &[legaia_engine_ui::TextDraw]| -> Vec<_> {
            d.iter().map(|t| (t.dst, t.src)).collect()
        };
        assert_eq!(key(&port), key(&want), "caption {run} {caption:?}");
    }
}

#[test]
fn the_info_panel_mode_is_the_one_retail_picks() {
    differential!(prot(899, MODE));
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem.load(
        OVERLAY_BASE,
        &d.read_entry_footprint(899).expect("PROT 0899"),
    );
    let mut rng = Rng(0x4D4F_4445);
    let mut seen = [0u32; 5];
    for case in 0..20_000 {
        let cell = rng.below(16);
        // The walk has run: every slot scanned (the port's grid is never up
        // before it - the scan is its own phase).
        cpu.mem.wr8(SCANNED + cell, 1);
        let class = [0u8, 1, 2, 3, rng.next() as u8][rng.below(5) as usize];
        cpu.mem.wr8(CLASSES + cell, class);
        let op = rng.below(2);
        cpu.mem.wr32(OP_FLAG, op);
        assert_eq!(cpu.call(MODE, &[cell]), Stop::Returned);
        let mode = cpu.regs.r[V0];
        // The class byte's meaning, as the directory walk stamps it.
        let content = match class {
            1 => SlotContent::LegaiaSave,
            0 => SlotContent::Foreign,
            _ => SlotContent::Free,
        };
        let snap = SlotSnapshot {
            content,
            present: class == 1,
            ..SlotSnapshot::empty(cell as u8)
        };
        let port = match SlotInfoMode::for_grid_cell(cell as u8, &snap) {
            SlotInfoMode::Preview => 1,
            SlotInfoMode::NotLegaiaSave => 2,
            SlotInfoMode::FreeBlock => 3,
            SlotInfoMode::Return => 4,
        };
        assert_eq!(mode, port, "case {case}: cell {cell} class {class} op {op}");
        seen[mode as usize] += 1;
        // Not yet scanned: retail answers `2`, whatever the class.
        cpu.mem.wr8(SCANNED + cell, 0);
        assert_eq!(cpu.call(MODE, &[cell]), Stop::Returned);
        assert_eq!(cpu.regs.r[V0], if cell == 15 { 4 } else { 2 });
    }
    assert!(seen[1..].iter().all(|&n| n > 500), "{seen:?}");
}

#[test]
fn the_title_strips_behind_the_load_window_are_retails() {
    differential!(prot(899, STRIPS));
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem.load(
        OVERLAY_BASE,
        &d.read_entry_footprint(899).expect("PROT 0899"),
    );
    // The record drawer `FUN_801E2EE4(mode, x, y, record, brightness, scale)`.
    let drawn: Rc<RefCell<Vec<StripCall>>> = Rc::default();
    let dr = drawn.clone();
    cpu.hook(0x801E_2EE4, move |r, m| {
        let sp = r.r[psx_retail_exec::SP];
        dr.borrow_mut().push((
            r.r[A0],
            r.r[A1] as i32,
            r.r[A2] as i32,
            r.r[A3],
            m.rd32(sp + 0x10) as i32,
            m.rd32(sp + 0x14),
        ))
    });
    let mut rng = Rng(0x5354_5250);
    for case in 0..2_000 {
        let selected = [0u32, 1, rng.next()][rng.below(3) as usize];
        cpu.mem.wr32(0x8007_B820, selected);
        let brightness = rng.below(0x100) as i32;
        let counter = rng.next();
        let skip = rng.below(4) as u8;
        cpu.mem.wr32(STRIP_CLOCK, counter);
        cpu.mem.wr8(0x1F80_0393, skip);
        drawn.borrow_mut().clear();
        assert_eq!(
            cpu.call(STRIPS, &[brightness as u32]),
            Stop::Returned,
            "case {case}"
        );
        let want: Vec<StripCall> = title_strip_rows(selected != 0)
            .iter()
            .map(|row| {
                let b = if row.half_bright {
                    brightness / 2
                } else {
                    brightness
                };
                (2, 0xA0, row.y, u32::from(row.record), b, 0x1000)
            })
            .collect();
        assert_eq!(*drawn.borrow(), want, "case {case}: selected {selected:#x}");
        assert_eq!(
            cpu.mem.rd32(STRIP_CLOCK),
            counter.wrapping_add(u32::from(skip) * 0x20),
            "case {case}: the frame clock"
        );
    }
}

#[test]
fn a_save_ui_record_quad_lands_where_retail_puts_it() {
    differential!(prot(899, RECORD_QUAD));
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem.load(
        OVERLAY_BASE,
        &d.read_entry_footprint(899).expect("PROT 0899"),
    );
    let linked: Rc<RefCell<Vec<u32>>> = Rc::default();
    let l = linked.clone();
    cpu.hook(0x8003_D2C4, move |r, _| l.borrow_mut().push(r.r[A1]));
    let mut rng = Rng(0x5155_4144);
    for case in 0..4_000 {
        let index = rng.below(8);
        let rgb = (rng.next() as u8, rng.next() as u8, rng.next() as u8);
        let word = u32::from(rgb.0) | u32::from(rgb.1) << 8 | u32::from(rgb.2) << 16;
        let (x, y) = (rng.below(320) as i32 - 16, rng.below(240) as i32 - 16);
        cpu.mem.wr32(0x1F80_03A0, 0x8010_0000);
        cpu.mem.wr32(0x1F80_03F4, 0x8018_0000);
        linked.borrow_mut().clear();
        assert_eq!(
            cpu.call(RECORD_QUAD, &[index, word, x as u32, y as u32]),
            Stop::Returned
        );
        let p = 0x8010_0000;
        assert_eq!(*linked.borrow(), [p], "case {case}");
        let rec = RECORDS + index * 12;
        let record = (
            u32::from(cpu.mem.rd8(rec + 2)),
            u32::from(cpu.mem.rd8(rec + 4)),
            u32::from(cpu.mem.rd16(rec + 6)),
            u32::from(cpu.mem.rd16(rec + 8)),
        );
        let m = &mut cpu.mem;
        assert_eq!(m.rd8(p + 7), 0x2C, "case {case}: a textured quad");
        let rgb_got = (m.rd8(p + 4), m.rd8(p + 5), m.rd8(p + 6));
        let xy: Vec<(i32, i32)> = (0..4)
            .map(|k| {
                (
                    m.rd16(p + 8 + k * 8) as i16 as i32,
                    m.rd16(p + 10 + k * 8) as i16 as i32,
                )
            })
            .collect();
        let uv: Vec<(u32, u32)> = (0..4)
            .map(|k| {
                (
                    u32::from(m.rd8(p + 12 + k * 8)),
                    u32::from(m.rd8(p + 13 + k * 8)),
                )
            })
            .collect();
        let q = save_ui_record_quad(record, rgb, (x, y), (0, 0), 1);
        let what = format!("case {case}: record {index} {record:?}");
        assert_eq!(rgb_got, rgb, "{what}");
        let (dx, dy, dw, dh) = q.dst;
        assert_eq!(
            xy,
            vec![
                (dx, dy),
                (dx + dw as i32, dy),
                (dx, dy + dh as i32),
                (dx + dw as i32, dy + dh as i32)
            ],
            "{what}: corners"
        );
        let (su, sv, sw, sh) = q.src;
        let wrap = |v: u32| v & 0xFF;
        assert_eq!(
            uv,
            vec![
                (su, sv),
                (wrap(su + sw), sv),
                (su, wrap(sv + sh)),
                (wrap(su + sw), wrap(sv + sh))
            ],
            "{what}: texels"
        );
        assert_eq!(
            q.color,
            [
                f32::from(rgb.0) / 128.0,
                f32::from(rgb.1) / 128.0,
                f32::from(rgb.2) / 128.0,
                1.0
            ],
            "{what}"
        );
    }
}

#[test]
fn every_grid_cell_draws_what_retail_draws() {
    differential!(prot(899, CELL));
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem.load(
        OVERLAY_BASE,
        &d.read_entry_footprint(899).expect("PROT 0899"),
    );
    // The record quad: `(record, rgb word, x, y)`, and the face record's
    // per-cell `u` and CLUT patch.
    let quads: Rc<RefCell<Vec<QuadCall>>> = Rc::default();
    let q = quads.clone();
    cpu.hook(RECORD_QUAD, move |r, m| {
        q.borrow_mut().push((
            r.r[A0],
            r.r[A1],
            r.r[A2] as i32,
            r.r[A3] as i32,
            u32::from(m.rd16(RECORDS + 2)),
        ))
    });
    let face = |i: u32| (0x100 + i * 16, 0x80, 16, 16);
    let mut rects = legaia_engine_ui::SaveMenuAtlasRects {
        load_empty_frame: Some((0, 0, 32, 32)),
        load_used_frame: Some((32, 0, 32, 32)),
        ..Default::default()
    };
    for i in 0..16 {
        rects.save_icons[i] = Some(face(i as u32));
    }
    let mut rng = Rng(0x4345_4C4C);
    let mut seen = [0u32; 4];
    for case in 0..2_000 {
        // A card walk's result over the fifteen cells.
        let mut cells = Vec::new();
        for slot in 0..15u32 {
            let scanned = u8::from(rng.below(6) != 0);
            let class = [0u8, 1, 1, 2, 3][rng.below(5) as usize];
            cpu.mem.wr8(SCANNED + slot, scanned);
            cpu.mem.wr8(CLASSES + slot, class);
            let mode = if scanned == 0 || class == 0 {
                2
            } else if class == 1 {
                1
            } else {
                3
            };
            cells.push(legaia_engine_ui::SlotGridCell {
                present: mode == 1,
                icon_tile: (mode == 1).then_some(slot as u8),
                foreign: mode == 2,
            });
        }
        let cursor = rng.below(15);
        let port =
            legaia_engine_ui::slot_preview_grid_draws_for(&rects, &cells, cursor as u8, (0, 0), 1);
        let mut want = Vec::new();
        for slot in 0..15u32 {
            let (c, r) = (slot % 5, slot / 5);
            let x = slot_grid_quad_x(c, r, slot, 0x1000) - 8;
            let y = 0x24 + 20 * r as i32;
            quads.borrow_mut().clear();
            assert_eq!(
                // The grid loop passes `0` (full) on the focused cell, `3`
                // (dimmed) on the rest.
                cpu.call(
                    CELL,
                    &[slot, x as u32, y as u32, if slot == cursor { 0 } else { 3 }]
                ),
                Stop::Returned
            );
            let mode = cpu.regs.r[V0];
            seen[mode as usize] += 1;
            for &(record, rgb, px, py, u) in quads.borrow().iter() {
                let src = match record {
                    0 => {
                        assert_eq!(u, slot * 16, "case {case} slot {slot}: the face's column");
                        face(slot)
                    }
                    1 => rects.load_empty_frame.unwrap(),
                    5 => rects.load_used_frame.unwrap(),
                    other => panic!("case {case} slot {slot}: record {other}"),
                };
                let tint = f32::from((rgb & 0xFF) as u8) / 128.0;
                want.push((src, (px, py), [tint, tint, tint, 1.0]));
            }
        }
        // The port's cell draws in the grid's order (the frame, then the
        // face over it; retail links the face first, which the ordering
        // table draws last), the hand after them.
        let got: Vec<_> = port
            .iter()
            .filter(|s| s.src != rects.cursor)
            .map(|s| (s.src, (s.dst.0, s.dst.1), s.color))
            .collect();
        let mut want_sorted = Vec::new();
        // Retail per cell: [face, frame] or [frame]; the port: [frame, face].
        let mut i = 0;
        while i < want.len() {
            if i + 1 < want.len() && want[i].0.2 == 16 {
                want_sorted.push(want[i + 1]);
                want_sorted.push(want[i]);
                i += 2;
            } else {
                want_sorted.push(want[i]);
                i += 1;
            }
        }
        assert_eq!(got, want_sorted, "case {case} cursor {cursor}");
    }
    assert!(seen[1..].iter().all(|&n| n > 1000), "{seen:?}");
}
