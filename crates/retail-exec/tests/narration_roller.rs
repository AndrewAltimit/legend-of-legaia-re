//! The opening narration roller `FUN_80037174` (`SCUS_942.54`) against
//! `legaia_engine_field::cutscene_narration::CutsceneNarration`, frame for
//! frame over generated page blocks and config seeds.
//!
//! The text draw `FUN_80036888` and its width `FUN_80035F04` are hooked: the
//! draw logs which page it was handed and the row it was put on, which is
//! compared with the port's line list. The two `DR_AREA` packets are read
//! back: the second carries the clip window.

use crate::common::{Rng, scus_cpu};
use legaia_engine_field::cutscene_narration::{CutsceneNarration, RollerSeed};
use psx_retail_exec::{A0, SP, Stop, V0};
use std::cell::RefCell;
use std::rc::Rc;

const ROLLER: u32 = 0x8003_7174;
const WIDTH: u32 = 0x8003_5F04;
const DRAW: u32 = 0x8003_6888;

const CTX_PTR: u32 = 0x801C_6EA4;
const FRAME_STEP: u32 = 0x1F80_0393;
const CURSOR: u32 = 0x1F80_03A0;
const OT_BASE: u32 = 0x1F80_03F4;
const BUFFER: u32 = 0x1F80_0390;

const ACTOR: u32 = 0x8012_0000;
const PARENT: u32 = 0x8012_0400;
const CTX: u32 = 0x8012_1000;
const BLOCK: u32 = 0x8012_2000;
const PACKETS: u32 = 0x8013_0000;
const OT: u32 = 0x8014_0000;

#[test]
fn the_narration_roller_matches_retail() {
    differential!(scus(ROLLER));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let draws = Rc::new(RefCell::new(Vec::<(u32, i32)>::new()));
    cpu.hook(WIDTH, |r, _| r.r[V0] = 0x60);
    let d = draws.clone();
    cpu.hook(DRAW, move |r, m| {
        let y = m.rd32(r.r[SP] + 0x10) as i32;
        d.borrow_mut().push((r.r[A0], y));
    });
    cpu.mem.wr32(CTX_PTR, CTX);
    cpu.mem.wr32(OT_BASE, OT);

    let mut rng = Rng(0x524F_4C4C);
    let (mut frames_run, mut completed, mut released, mut drawn) = (0, 0, 0, 0);
    for block in 0..300 {
        // Pages: `0x1F text 0x00`, some with no text.
        let n_pages = rng.below(14) as usize;
        let pages: Vec<String> = (0..n_pages)
            .map(|_| {
                if rng.below(5) == 0 {
                    String::new()
                } else {
                    (0..1 + rng.below(20))
                        .map(|_| (0x20 + rng.below(0x5F)) as u8 as char)
                        .collect()
                }
            })
            .collect();
        let mut bytes = vec![n_pages as u8];
        let mut starts = Vec::new();
        for p in &pages {
            starts.push(BLOCK + bytes.len() as u32 + 1);
            bytes.push(0x1F);
            bytes.extend_from_slice(p.as_bytes());
            bytes.push(0);
        }
        for (i, b) in bytes.iter().enumerate() {
            cpu.mem.wr8(BLOCK + i as u32, *b);
        }
        let seed = RollerSeed {
            top: [0x40, 0x20, 0x80, 0xB0][rng.below(4) as usize],
            slots: [8, 1, 2, 4, 6, 12][rng.below(6) as usize],
            divisor: [4, 1, 2, 3, 6, 8][rng.below(6) as usize],
            release: if rng.below(3) == 0 {
                1 + rng.below(n_pages as u32 + 2) as i16
            } else {
                0
            },
        };
        let step = 1 + rng.below(4) as u16;
        cpu.mem.wr16(CTX + 0x4C, seed.top as u16);
        cpu.mem.wr16(CTX + 0x4E, seed.slots as u16);
        cpu.mem.wr16(CTX + 0x50, seed.divisor as u16);
        cpu.mem.wr16(CTX + 0x52, seed.release as u16);
        cpu.mem.wr16(ACTOR + 0x54, 0);
        cpu.mem.wr32(ACTOR + 0x10, rng.next() & !0x8_0008);
        cpu.mem.wr32(ACTOR + 0x90, BLOCK);
        cpu.mem.wr32(ACTOR + 0x94, PARENT);
        cpu.mem.wr32(PARENT + 0x10, 0xFFFF_FFFF);
        cpu.mem.wr8(FRAME_STEP, step as u8);
        let mut port = CutsceneNarration::with_seed(pages.clone(), seed, step);

        for frame in 0..600 {
            let buf = rng.below(2);
            cpu.mem.wr8(BUFFER, buf as u8);
            cpu.mem.wr32(CURSOR, PACKETS);
            cpu.mem.wr32(OT + 4, 0x00FF_FFFF);
            // A resume now and then, the config op's `w3 == 2`.
            if port.is_paused() && rng.below(40) == 0 {
                port.resume();
                let f = cpu.mem.rd32(ACTOR + 0x10);
                cpu.mem.wr32(ACTOR + 0x10, f & !0x8_0000);
            }
            draws.borrow_mut().clear();
            assert_eq!(cpu.call(ROLLER, &[ACTOR]), Stop::Returned, "block {block} frame {frame}");
            port.frame();
            frames_run += 1;
            let ctx = format!("block {block} frame {frame} {seed:?} step {step} pages {n_pages}");

            let flags = cpu.mem.rd32(ACTOR + 0x10);
            assert_eq!(flags & 8 != 0, port.is_complete(), "complete: {ctx}");
            assert_eq!(flags & 0x8_0000 != 0, port.is_paused(), "paused: {ctx}");
            assert_eq!(cpu.mem.rd16(CTX + 0x52) as i16, port.seed().release, "release: {ctx}");
            if port.is_complete() {
                assert_eq!(cpu.mem.rd32(PARENT + 0x10), !0x400, "parent: {ctx}");
                completed += 1;
                break;
            }
            assert_eq!(cpu.mem.rd16(ACTOR + 0x6A) as usize, port.retired(), "retired: {ctx}");
            assert_eq!(cpu.mem.rd16(ACTOR + 0x9C) as usize, port.current_index(), "admitted: {ctx}");
            // Retail draws an empty page as an empty string; the port keeps
            // it out of the line list. Neither puts anything on screen.
            let got: Vec<(String, i32)> = draws
                .borrow()
                .iter()
                .map(|&(a0, y)| {
                    let p = starts.iter().position(|&s| s == a0).expect("a page start");
                    (pages[p].clone(), y)
                })
                .filter(|(p, _)| !p.is_empty())
                .collect();
            let want: Vec<(String, i32)> = port
                .all_lines()
                .iter()
                .map(|l| (l.text.to_string(), l.y))
                .collect();
            assert_eq!(got, want, "lines: {ctx}");
            drawn += want.len();
            // The clip window: the second `DR_AREA`.
            let (top, bottom) = port.clip_window();
            let clip = PACKETS + 0xC;
            let off = (buf * 0xF0) as i32;
            assert_eq!(cpu.mem.rd32(clip + 4), 0xE300_0000 | (((top + off) as u32) << 10), "{ctx}");
            assert_eq!(
                cpu.mem.rd32(clip + 8),
                0xE400_0000 | (((bottom + off) as u32) << 10) | 0x13F,
                "{ctx}"
            );
            released += u32::from(port.is_paused());
        }
    }
    assert!(
        frames_run > 30000 && completed > 150 && released > 500 && drawn > 50000,
        "{frames_run} {completed} {released} {drawn}"
    );
}
