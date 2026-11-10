//! The field's text balloon: the SCUS spawner `FUN_8003C764` against
//! `TextBalloon::spawn_measured`, with the width measurer answered by a
//! scripted width on both sides (it has its own differential, `text_width`),
//! and the handler `FUN_801DA7F0` (PROT 0897) tick by tick against
//! `TextBalloon::tick` over the parent-link handshake, the player's engaged
//! bit and every frame step: the timer, the link, the kill bit, and on a
//! drawn frame the line's pen and the frame's rect.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_dialog::text_balloon::{BalloonTick, TextBalloon};
use psx_retail_exec::{A0, A1, A2, A3, SP, Stop, V0};
use std::cell::RefCell;
use std::rc::Rc;

const OVERLAY_BASE: u32 = 0x801C_E818;
const SPAWN: u32 = 0x8003_C764;
const HANDLER: u32 = 0x801D_A7F0;
const ALLOC: u32 = 0x8002_0DE0;
const MEASURE: u32 = 0x8003_5F04;
const FIND: u32 = 0x8003_CF04;
const PEN: u32 = 0x8003_C110;
const PRINT: u32 = 0x8003_6888;
const INK: u32 = 0x8003_4B6C;
const FRAME: u32 = 0x8002_C69C;
const PLAYER_PTR: u32 = 0x8007_C364;
const PLAYER: u32 = 0x8010_0000;
const ACTOR: u32 = 0x8011_0000;
const TEXT: u32 = 0x8012_0000;
const CADENCE: u32 = 0x1F80_0393;

#[derive(Default, Debug, Clone, PartialEq)]
struct Drawn {
    pen: Option<(i32, i32)>,
    frame: Option<(i32, i32, i32, i32)>,
}

#[test]
fn the_balloon_spawns_and_ticks_as_retail() {
    differential!(scus(SPAWN), prot(897, HANDLER));
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &d.read_entry(897).expect("PROT 0897"));
    let width = Rc::new(RefCell::new(0u32));
    let w = width.clone();
    cpu.hook(MEASURE, move |r, _| r.r[V0] = *w.borrow());
    cpu.hook(ALLOC, |r, m| {
        for i in (0..0xA0).step_by(4) {
            m.wr32(ACTOR + i, 0);
        }
        r.r[V0] = ACTOR;
    });
    cpu.stub(FIND, 0);
    cpu.stub(PEN, 0);
    cpu.stub(INK, 0);
    let drawn = Rc::new(RefCell::new(Drawn::default()));
    let dr = drawn.clone();
    cpu.hook(PRINT, move |r, m| {
        let y = m.rd32(r.r[SP] + 0x10) as i32;
        dr.borrow_mut().pen = Some((r.r[A3] as i32, y));
    });
    let dr = drawn.clone();
    cpu.hook(FRAME, move |r, _| {
        dr.borrow_mut().frame = Some((
            r.r[A0] as i32,
            r.r[A1] as i32,
            r.r[A2] as i32,
            r.r[A3] as i32,
        ));
    });
    cpu.mem.wr32(PLAYER_PTR, PLAYER);
    let mut rng = Rng(0x4241_4C4C);
    let (mut draws, mut forwards, mut kills) = (0, 0, 0);
    for run in 0..3000 {
        let w = [0, 0x40, 0x90, 0x140, 0x1A0, rng.below(0x200)][rng.below(6) as usize];
        *width.borrow_mut() = w;
        let parent = 0x8013_0000 + rng.below(0x100) * 4;
        assert_eq!(cpu.call(SPAWN, &[TEXT, parent]), Stop::Returned);
        assert_eq!(cpu.regs.r[V0], ACTOR);
        let mut b = TextBalloon::spawn_measured(b"x", w as i16);
        let what = format!("run {run} width {w:#x}");
        let m = &mut cpu.mem;
        assert_eq!(m.rd32(ACTOR + 0x90), TEXT, "{what}");
        assert_eq!(m.rd32(ACTOR + 0x94) != 0, b.parent_link, "{what}");
        assert_eq!(m.rd16(ACTOR + 0x54) as i16, b.timer, "{what}");
        assert_eq!(m.rd16(ACTOR + 0x9C) as i16, b.total, "{what}");
        assert_eq!(
            (m.rd16(ACTOR + 0x14) as i16, m.rd16(ACTOR + 0x16) as i16),
            (b.x.expect("measured"), b.y),
            "{what}"
        );
        for tick in 0..200 {
            let what = format!("{what} tick {tick}");
            let engaged = rng.below(5) == 0;
            let cadence = [1u8, 1, 2, 3, 4][rng.below(5) as usize];
            cpu.mem
                .wr32(PLAYER + 0x10, if engaged { 0x8_0000 } else { 0x10 });
            cpu.mem.wr8(CADENCE, cadence);
            *drawn.borrow_mut() = Drawn::default();
            assert_eq!(cpu.call(HANDLER, &[ACTOR]), Stop::Returned, "{what}");
            let t = b.tick(engaged, i16::from(cadence));
            let m = &mut cpu.mem;
            assert_eq!(m.rd16(ACTOR + 0x54) as i16, b.timer, "{what}: timer");
            assert_eq!(m.rd32(ACTOR + 0x94) != 0, b.parent_link, "{what}: link");
            assert_eq!(m.rd32(ACTOR + 0x10) & 8 != 0, b.killed, "{what}: kill");
            let port = match t {
                BalloonTick::Draw => Drawn {
                    pen: b.pen(),
                    frame: Some(b.frame_rect()),
                },
                _ => Drawn::default(),
            };
            assert_eq!(*drawn.borrow(), port, "{what}: draw");
            draws += u32::from(t == BalloonTick::Draw);
            forwards += u32::from(!b.parent_link && engaged);
            if b.killed {
                kills += 1;
                break;
            }
        }
    }
    assert!(
        draws > 10_000 && forwards > 500 && kills > 2000,
        "{draws} {forwards} {kills}"
    );
}
