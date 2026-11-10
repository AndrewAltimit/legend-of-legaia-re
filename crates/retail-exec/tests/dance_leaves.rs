//! The dance hall's leaves in PROT 0980 outside the dancer tick: the CPU
//! dancer's pad bit `FUN_801D4040`, the clip-driver gate `FUN_801D4098`, the
//! banner's thousands glyph `FUN_801D3E28` and the number renderer
//! `FUN_801D32F8`, against `DanceDir::pad_bit`, `dance_clip_driver_gate`,
//! `score_thousands_glyph_u` and `dance_number_digits` with its two glyph maps.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_minigames::dance::{
    DanceDir, dance_clip_driver_gate, dance_level_digit_u, dance_number_digits,
    dance_score_digit_u, score_thousands_glyph_u,
};
use psx_retail_exec::{A0, A1, A2, A3, Cpu, SP, Stop, V0};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

const OVERLAY_BASE: u32 = 0x801C_E818;
const PAD_BIT: u32 = 0x801D_4040;
const AUTO_FEED: u32 = 0x801D_1820;
const GATE: u32 = 0x801D_4098;
const CLIP_DRIVER: u32 = 0x8002_04F8;
const THOUSANDS: u32 = 0x801D_3E28;
const NUMBER: u32 = 0x801D_32F8;
const QUAD: u32 = 0x801D_2F38;
const BANNER_U: u32 = 0x801D_4760;
const SCORE_U: u32 = 0x801D_46E8;
const LEVEL_U: u32 = 0x801D_4968;
const STACK_TOP: u32 = 0x801F_FF00;

/// One `FUN_801D2F38(x, y, widget, brightness, scale)` draw, with the glyph
/// byte the caller patched in before it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Quad {
    x: i32,
    y: i32,
    widget: u32,
    brightness: u32,
    scale: u32,
    u: u8,
}

type Quads = Rc<RefCell<Vec<Quad>>>;

fn rig() -> Option<(Cpu, Quads, Rc<Cell<u32>>)> {
    let (mut cpu, d) = (scus_cpu()?, disc()?);
    cpu.mem
        .load(OVERLAY_BASE, &d.read_entry(980).expect("PROT 0980"));
    let quads = Rc::new(RefCell::new(Vec::new()));
    let sink = quads.clone();
    cpu.hook(QUAD, move |r, m| {
        let widget = r.r[A2];
        let u = match widget {
            1 => m.rd8(SCORE_U),
            0x21 => m.rd8(LEVEL_U),
            _ => m.rd8(BANNER_U),
        };
        sink.borrow_mut().push(Quad {
            x: r.r[A0] as i32,
            y: r.r[A1] as i32,
            widget,
            brightness: r.r[A3],
            scale: m.rd32(r.r[SP] + 0x10),
            u,
        });
    });
    let driven = Rc::new(Cell::new(0));
    let count = driven.clone();
    cpu.hook(CLIP_DRIVER, move |_, _| count.set(count.get() + 1));
    Some((cpu, quads, driven))
}

#[test]
fn a_cpu_dancer_presses_the_bit_retail_presses() {
    differential!(prot(980, PAD_BIT));
    let Some((mut cpu, _, _)) = rig() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let symbol = Rc::new(Cell::new(0u32));
    let feed = symbol.clone();
    cpu.hook(AUTO_FEED, move |r, _| r.r[V0] = feed.get());
    for s in [0u32, 1, 2, 3, 4, 5, 0xFF, 0xFFFF_FFFF] {
        symbol.set(s);
        assert_eq!(cpu.call(PAD_BIT, &[0]), Stop::Returned);
        let port = match s {
            1 => DanceDir::A.pad_bit(),
            2 => DanceDir::B.pad_bit(),
            3 => DanceDir::C.pad_bit(),
            _ => 0,
        };
        assert_eq!(cpu.regs.r[V0], u32::from(port), "symbol {s}");
    }
}

#[test]
fn the_clip_driver_runs_when_retail_runs_it() {
    differential!(prot(980, GATE));
    let Some((mut cpu, _, driven)) = rig() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    const ACTOR: u32 = 0x8014_0000;
    let mut rng = Rng(0x4741_5445);
    let mut both = [0; 2];
    for round in 0..20_000 {
        let clip = [0i16, 1, -1, i16::MIN, i16::MAX, rng.next() as i16][rng.below(6) as usize];
        let flags = [0, 0x1000, 0xFFFF_EFFF, rng.next()][rng.below(4) as usize];
        cpu.mem.wr16(ACTOR + 0x5C, clip as u16);
        cpu.mem.wr32(ACTOR + 0x10, flags);
        driven.set(0);
        assert_eq!(cpu.call(GATE, &[ACTOR]), Stop::Returned);
        let retail = driven.get() == 1;
        let port = dance_clip_driver_gate(clip, flags);
        assert_eq!(retail, port, "round {round} clip {clip} flags {flags:#x}");
        both[usize::from(retail)] += 1;
    }
    assert!(both[0] > 1000 && both[1] > 1000, "{both:?}");
}

#[test]
fn the_banner_glyph_is_the_one_retail_patches() {
    differential!(prot(980, THOUSANDS));
    let Some((mut cpu, quads, _)) = rig() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x5448_4F55);
    for round in 0..20_000 {
        let score = match rng.below(3) {
            0 => rng.below(10_000) as i32,
            1 => rng.below(100_000_000) as i32 - 50_000_000,
            _ => rng.next() as i32,
        };
        let (x, y) = (rng.below(320) as i32, rng.below(240) as i32);
        quads.borrow_mut().clear();
        assert_eq!(
            cpu.call(THOUSANDS, &[score as u32, x as u32, y as u32]),
            Stop::Returned
        );
        let u = score_thousands_glyph_u(score) as u8;
        // The two banner halves, 8 px apart, both drawn after the patch.
        let port = [6, 7].map(|widget| Quad {
            x: x + (widget as i32 - 6) * 8,
            y,
            widget,
            brightness: 0x80,
            scale: 0x1000,
            u,
        });
        assert_eq!(
            quads.borrow().as_slice(),
            &port,
            "round {round} score {score}"
        );
    }
}

#[test]
fn a_number_draws_the_digits_retail_draws() {
    differential!(prot(980, NUMBER));
    let Some((mut cpu, quads, _)) = rig() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x4E55_4D42);
    for round in 0..30_000 {
        let value = match rng.below(4) {
            0 => rng.below(10),
            1 => rng.below(100_000),
            2 => rng.below(100_000_000),
            _ => rng.next() & 0x7FFF_FFFF,
        };
        let style_b = rng.below(2) == 1;
        let (x, y) = (rng.below(320) as i32, rng.below(240) as i32);
        let brightness = rng.below(0x100);
        cpu.mem.wr32(STACK_TOP + 0x10, brightness);
        quads.borrow_mut().clear();
        let args = [u32::from(style_b), x as u32, y as u32, value];
        assert_eq!(cpu.call(NUMBER, &args), Stop::Returned);
        let port: Vec<Quad> = dance_number_digits(value)
            .iter()
            .enumerate()
            .filter_map(|(slot, d)| {
                let d = (*d)?;
                Some(if style_b {
                    Quad {
                        x: x + slot as i32 * 8,
                        y,
                        widget: 0x21,
                        brightness,
                        scale: 0x1000,
                        u: dance_level_digit_u(d),
                    }
                } else {
                    Quad {
                        x: x + slot as i32 * 0x10,
                        y,
                        widget: 1,
                        brightness,
                        scale: 0x1000,
                        u: dance_score_digit_u(d),
                    }
                })
            })
            .collect();
        assert_eq!(
            quads.borrow().as_slice(),
            port.as_slice(),
            "round {round} value {value} style_b {style_b}"
        );
    }
}
