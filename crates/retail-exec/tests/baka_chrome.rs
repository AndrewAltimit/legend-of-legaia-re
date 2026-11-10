//! Baka Fighter's screen chrome in PROT 0976, each timeline called as its
//! caller calls it with the widget emitter `FUN_801D5ED0`, the announcer
//! `FUN_8003D53C` and the screen tint `FUN_80024EE4` hooked: the round banner
//! `FUN_801D5C7C`, the intro title card `FUN_801D59D4` and the all-clear
//! announcer `FUN_801D21FC`, against `round_banner_frame`, `IntroTitle::frame`
//! and `countdown_frame`.

use crate::common::{Rng, STACK_TOP, disc, scus_cpu};
use legaia_engine_minigames::baka_fighter_chrome::{
    ChromeFrame, Countdown, GLYPH_WIDGET, IntroTitle, countdown_frame, glyph_u, round_banner_frame,
};
use psx_retail_exec::{A0, A1, A2, A3, Cpu, SP, Stop};
use std::cell::RefCell;
use std::rc::Rc;

const OVERLAY_BASE: u32 = 0x801C_E818;
const EMIT: u32 = 0x801D_5ED0;
const XA: u32 = 0x8003_D53C;
const TINT: u32 = 0x8002_4EE4;
const GLYPH_COLUMN: u32 = 0x801D_71CC;
const BANNER: u32 = 0x801D_5C7C;
const TITLE: u32 = 0x801D_59D4;
const ANNOUNCER: u32 = 0x801D_21FC;

/// `(widget, x, y, brightness, size, glyph column)` per quad.
type Quad = (u32, i32, i32, i32, i32, Option<u8>);

#[derive(Debug, Default, PartialEq, Eq)]
struct Frame {
    quads: Vec<Quad>,
    /// `(clip, channel, duration)` per announcer line.
    lines: Vec<(u32, u32, u32)>,
    /// The tint's colour word.
    tints: Vec<u32>,
}

fn rig() -> Option<(Cpu, Rc<RefCell<Frame>>)> {
    let (mut cpu, d) = (scus_cpu()?, disc()?);
    cpu.mem
        .load(OVERLAY_BASE, &d.read_entry(976).expect("PROT 0976"));
    let frame: Rc<RefCell<Frame>> = Rc::default();
    let f = frame.clone();
    cpu.hook(EMIT, move |r, m| {
        let widget = r.r[A2];
        let size = m.rd32(r.r[SP] + 0x10) as i32;
        let column = (widget == u32::from(GLYPH_WIDGET)).then(|| m.rd8(GLYPH_COLUMN));
        f.borrow_mut().quads.push((
            widget,
            r.r[A0] as i32,
            r.r[A1] as i32,
            r.r[A3] as i32,
            size,
            column,
        ));
    });
    let f = frame.clone();
    cpu.hook(XA, move |r, _| {
        f.borrow_mut().lines.push((r.r[A0], r.r[A1], r.r[A2]));
    });
    let f = frame.clone();
    cpu.hook(TINT, move |r, _| f.borrow_mut().tints.push(r.r[A2]));
    Some((cpu, frame))
}

fn expect(port: &ChromeFrame) -> Frame {
    Frame {
        quads: port
            .draws
            .iter()
            .map(|d| {
                (
                    u32::from(d.widget),
                    i32::from(d.x),
                    i32::from(d.y),
                    d.brightness,
                    d.size,
                    d.glyph.map(glyph_u),
                )
            })
            .collect(),
        lines: port
            .xa
            .iter()
            .map(|c| (u32::from(c.clip), u32::from(c.chan), u32::from(c.dur)))
            .collect(),
        tints: port.tint.iter().map(|t| t.colour).collect(),
    }
}

#[test]
fn the_round_banner_slides_as_retail_slides_it() {
    differential!(prot(976, BANNER));
    const ROUND: u32 = 0x801D_BF8C;
    let Some((mut cpu, frame)) = rig() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    for round in 0..4 {
        for t in 0..160 {
            cpu.mem.wr32(ROUND, round as u32);
            *frame.borrow_mut() = Frame::default();
            assert_eq!(cpu.call(BANNER, &[t as u32]), Stop::Returned);
            assert_eq!(
                *frame.borrow(),
                expect(&round_banner_frame(t, round)),
                "round {round} t {t}"
            );
        }
    }
}

#[test]
fn the_intro_title_card_plays_as_retail_plays_it() {
    differential!(prot(976, TITLE));
    const LATCH: u32 = 0x801D_BE8C;
    const BANNER_CLUT: u32 = 0x801D_740E;
    let Some((mut cpu, frame)) = rig() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x5449_544C);
    // Played through in order, then from generated frames with the latch
    // wherever a skipped frame leaves it.
    for pass in 0..40 {
        let mut title = IntroTitle::default();
        cpu.mem.wr32(LATCH, 0);
        for step in 0..260 {
            let t = if pass == 0 {
                step
            } else {
                rng.below(260) as i32
            };
            let clut_before = cpu.mem.rd16(BANNER_CLUT);
            *frame.borrow_mut() = Frame::default();
            assert_eq!(cpu.call(TITLE, &[t as u32]), Stop::Returned);
            let port = title.frame(t);
            assert_eq!(*frame.borrow(), expect(&port), "pass {pass} t {t}");
            assert_eq!(
                (cpu.mem.rd32(LATCH) as i32, cpu.mem.rd16(BANNER_CLUT)),
                (title.announced, port.banner_clut.unwrap_or(clut_before)),
                "pass {pass} t {t}"
            );
        }
    }
}

#[test]
fn the_all_clear_announcer_counts_as_retail_counts() {
    differential!(prot(976, ANNOUNCER));
    const STATE: u32 = 0x801D_C134;
    const TIMER: u32 = 0x801D_C138;
    const LOADING: u32 = 0x8007_BC20;
    const BANNER_LEVEL: u32 = 0x801D_BEB4;
    const TITLE_LEVEL: u32 = 0x801D_BEB0;
    const STAGE: u32 = 0x801D_C110;
    let Some((mut cpu, frame)) = rig() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x434C_4541);
    let mut finished = 0;
    for run in 0..300 {
        let mut st = Countdown::default();
        cpu.mem.wr32(STATE, 0);
        cpu.mem.wr32(TIMER, 0);
        let stage = if rng.below(3) == 0 {
            0x0E
        } else {
            rng.below(0x10) as i32
        };
        let load_frames = rng.below(20);
        let mut banner = 0;
        for f in 0..120 {
            let step = 1 + rng.below(3) as i32;
            let loading = f < load_frames;
            banner = (banner + rng.below(3) as i32).min(0xFF);
            let title = rng.below(0x200) as i32 - 0x80;
            cpu.mem.wr8(0x1F80_0393, step as u8);
            cpu.mem.wr32(LOADING, u32::from(loading));
            cpu.mem.wr32(BANNER_LEVEL, banner as u32);
            cpu.mem.wr32(TITLE_LEVEL, title as u32);
            cpu.mem.wr32(STAGE, stage as u32);
            *frame.borrow_mut() = Frame::default();
            cpu.mem.wr32(STACK_TOP + 0x10, 0);
            assert_eq!(cpu.call(ANNOUNCER, &[]), Stop::Returned);
            let port = countdown_frame(&mut st, step, loading, banner, title, stage);
            // A frame that both arms and finds the load done calls the
            // announcer twice; the drive takes the later clip, and the port
            // carries one line a frame.
            let mut retail = std::mem::take(&mut *frame.borrow_mut());
            if retail.lines.len() > 1 {
                retail.lines.drain(..retail.lines.len() - 1);
            }
            assert_eq!(retail, expect(&port), "run {run} frame {f}");
            assert_eq!(
                (cpu.mem.rd32(STATE) as i32, cpu.mem.rd32(TIMER) as i32),
                (st.state, st.timer),
                "run {run} frame {f}"
            );
        }
        finished += u32::from(st.state == 3);
    }
    assert!(finished > 100, "{finished}");
}
