//! The Triangle arts list's driver `FUN_801D3444` against
//! `ArtsListDriver::tick`: registered and not, open and closed, inside the
//! entry state `0x50` and out of it, slides across the whole byte (the
//! climb's wrap included), frame steps `0..=4` and wider, latch set and
//! clear, learned counts around the six and eleven thresholds and every row
//! offset.
//!
//! The two draw calls are hooked and compared (the Triangle sprite
//! `FUN_8002C488` and the text kernel `FUN_80036888`, their seats and string
//! pointers), with the three driver bytes and the window record's `x`.

use crate::common::{Rng, scus_cpu};
use legaia_engine_battle_vm::arts_list_driver::{
    ArtsListDriver, ArtsListDriverCaption as C, ArtsListDriverInputs,
};
use psx_retail_exec::{A0, A1, A2, A3, SP, Stop};
use std::cell::RefCell;
use std::rc::Rc;

const DRIVER: u32 = 0x801D_3444;
const SPRITE: u32 = 0x8002_C488;
const TEXT: u32 = 0x8003_6888;
const OVERLAY_BASE: u32 = 0x801C_E818;
const CTX_PTR: u32 = 0x8007_BD24;
const CTX: u32 = 0x8012_0000;
const CHAR_IDS: u32 = 0x8007_BD10;
const RECORDS: u32 = 0x8008_4140;
const FLAGS: u32 = 0x801F_4E09;
const SLIDE: u32 = 0x801F_4E0A;
const LATCH: u32 = 0x801F_4E08;
const WINDOW_PTR: u32 = 0x801F_4E0C;
const WINDOW: u32 = 0x8014_0000;
const ROW_OFFSET: u32 = 0x8007_B458;
const FRAME_STEP: u32 = 0x1F80_0393;
const HELP_TEXT: u32 = 0x801C_E840;
const NEXT_TEXT: u32 = 0x801C_E860;

#[test]
fn the_arts_list_driver_slides_and_captions_as_retail() {
    differential!(prot(898, DRIVER));
    let (Some(mut cpu), Some(disc)) = (scus_cpu(), crate::common::disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &disc.read_entry(898).expect("PROT 0898"));
    cpu.mem.wr32(CTX_PTR, CTX);
    cpu.mem.wr32(WINDOW_PTR, WINDOW);
    let calls = Rc::new(RefCell::new(Vec::new()));
    {
        let calls = calls.clone();
        cpu.hook(SPRITE, move |r, _| {
            calls
                .borrow_mut()
                .push((SPRITE, r.r[A0], r.r[A1], r.r[A2], 0));
        });
    }
    {
        let calls = calls.clone();
        cpu.hook(TEXT, move |r, m| {
            assert_eq!((r.r[A1], r.r[A2]), (0, 0));
            let y = m.rd32(r.r[SP] + 0x10);
            calls.borrow_mut().push((r.r[A0], r.r[A3], y, 0, 1));
        });
    }
    let mut rng = Rng(0x4152_5453);
    let mut arms = std::collections::HashMap::new();
    for n in 0..60_000 {
        let flags = match rng.below(4) {
            0 => rng.next() as u8,
            k => 0x80 | (k as u8 & 1) | (rng.next() as u8 & 0x7E & (rng.below(2) as u8 * 0xFF)),
        };
        let slide = match rng.below(3) {
            0 => rng.next() as u8,
            _ => rng.below(14) as u8,
        };
        let latch = match rng.below(3) {
            0 => rng.next() as u8,
            _ => 0,
        };
        let entering = rng.below(2) == 0;
        let state = if entering {
            0x50
        } else {
            [0x4F, 0x51, 0x5A, rng.next() as u8 | 1][rng.below(4) as usize]
        };
        let frame_step = match rng.below(4) {
            0 => rng.next() as u8,
            _ => rng.below(5) as u8,
        };
        let learned = match rng.below(3) {
            0 => rng.next() as u8,
            _ => rng.below(14) as u8,
        };
        let row_offset = match rng.below(4) {
            0 => rng.next(),
            k => (k - 1) * 5,
        };
        let seat = rng.below(4) as u8;
        let char_id = 1 + rng.below(3) as u8;
        cpu.mem.wr8(FLAGS, flags);
        cpu.mem.wr8(SLIDE, slide);
        cpu.mem.wr8(LATCH, latch);
        cpu.mem.wr8(CTX + 6, state);
        cpu.mem.wr8(CTX + 0x13, seat);
        cpu.mem.wr8(CHAR_IDS + u32::from(seat), char_id);
        cpu.mem
            .wr8(RECORDS + (u32::from(char_id) - 1) * 0x414 + 0x74D, learned);
        cpu.mem.wr32(ROW_OFFSET, row_offset);
        cpu.mem.wr8(FRAME_STEP, frame_step);
        cpu.mem.wr16(WINDOW + 0xA, 0xEEEE);
        calls.borrow_mut().clear();
        assert_eq!(cpu.call(DRIVER, &[]), Stop::Returned, "n {n}");

        let mut d = ArtsListDriver {
            flags,
            slide,
            latch,
        };
        let inputs = ArtsListDriverInputs {
            entering,
            frame_step,
            learned,
            row_offset,
        };
        let f = d.tick(&inputs);
        let ctx = format!("n {n} {inputs:?} flags {flags:#x} slide {slide} latch {latch}");
        assert_eq!(
            (cpu.mem.rd8(FLAGS), cpu.mem.rd8(SLIDE), cpu.mem.rd8(LATCH)),
            (d.flags, d.slide, d.latch),
            "{ctx}"
        );
        assert_eq!(
            cpu.mem.rd16(WINDOW + 0xA) as i16,
            f.map_or(-0x1112, |f| f.window_x),
            "{ctx}"
        );
        let want: Vec<_> = match f.and_then(|f| f.caption) {
            None => Vec::new(),
            Some(c) => {
                let (x, y) = c.glyph_seat();
                let text = if c == C::HyperArtsList {
                    HELP_TEXT
                } else {
                    NEXT_TEXT
                };
                vec![
                    (SPRITE, x as u32, y as u32, 0x3A, 0),
                    (text, x as u32 + 16, y as u32 + 2, 0, 1),
                ]
            }
        };
        assert_eq!(*calls.borrow(), want, "{ctx}");
        let key = (
            f.is_some(),
            f.and_then(|f| f.caption),
            d.flags == 0 && flags & 0x80 != 0,
        );
        *arms.entry(key).or_insert(0u32) += 1;
    }
    assert_eq!(arms.len(), 5, "{arms:?}");
    assert!(arms.values().all(|&c| c > 100), "{arms:?}");
}
