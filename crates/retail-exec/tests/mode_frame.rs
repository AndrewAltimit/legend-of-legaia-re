//! The per-frame mode handlers in `SCUS_942.54` against
//! `engine_mode::mode::per_frame_stage`: the default handler `FUN_80025EEC`
//! (twelve modes), MAPDISP's `FUN_80025F2C` and CARD's `FUN_80025F74`, each
//! reached through its mode-table row (`0x8007078C`, handler at `+0x10`),
//! with the frame-begin pass, the overlay hook, the body and the frame-end
//! pass hooked and their return values scripted, so both the frame-skip
//! early-out and the body's abort branch run. The call sequence and the
//! master driver's argument are compared.
//!
//! CARD's body `FUN_80017978` then runs on its own against
//! `CARD_FRAME_BODY`: the debug chord `FUN_800179C0`, the CARD actor's
//! `+0x0C` handler (the actor at `gp + 0x5C8`, `0x8007B8E0`), the readout
//! `FUN_800188C8` with the scratchpad byte `0x1F800391`, no master driver,
//! and a return of `0`.

use crate::common::{Rng, scus_cpu};
use legaia_engine_mode::mode::{CARD_FRAME_BODY, FrameBody, GameMode, per_frame_stage};
use psx_retail_exec::{A0, Stop, V0};
use std::cell::RefCell;
use std::rc::Rc;

const TABLE: u32 = 0x8007_078C;
const DEFAULT: u32 = 0x8002_5EEC;
const MAPDISP: u32 = 0x8002_5F2C;
const CARD: u32 = 0x8002_5F74;
const CARD_BODY: u32 = 0x8001_7978;
const BEGIN: u32 = 0x8001_698C;
const MASTER: u32 = 0x8001_6444;
const END: u32 = 0x8001_6B6C;
const CHORD: u32 = 0x8001_79C0;
const READOUT: u32 = 0x8001_88C8;
const CARD_ACTOR_PTR: u32 = 0x8007_B8E0;
const CARD_ACTOR: u32 = 0x8012_0000;
const CARD_TICK: u32 = 0x8FF0_0000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ev {
    Begin,
    Hook(u32),
    Master(i32),
    Card,
    End,
}

#[test]
fn every_per_frame_handler_runs_its_stage() {
    differential!(scus(DEFAULT), scus(MAPDISP), scus(CARD));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let log: Rc<RefCell<Vec<Ev>>> = Rc::new(RefCell::new(Vec::new()));
    let rets: Rc<RefCell<(u32, u32)>> = Rc::new(RefCell::new((0, 0)));
    {
        let (l, r) = (log.clone(), rets.clone());
        cpu.hook(BEGIN, move |regs, _| {
            l.borrow_mut().push(Ev::Begin);
            regs.r[V0] = r.borrow().0;
        });
        let (l, r) = (log.clone(), rets.clone());
        cpu.hook(MASTER, move |regs, _| {
            l.borrow_mut().push(Ev::Master(regs.r[A0] as i32));
            regs.r[V0] = r.borrow().1;
        });
        let (l, r) = (log.clone(), rets.clone());
        cpu.hook(CARD_BODY, move |regs, _| {
            l.borrow_mut().push(Ev::Card);
            regs.r[V0] = r.borrow().1;
        });
        let l = log.clone();
        cpu.hook(END, move |_, _| l.borrow_mut().push(Ev::End));
        let l = log.clone();
        cpu.hook(0x801C_E850, move |_, _| {
            l.borrow_mut().push(Ev::Hook(0x801C_E850))
        });
    }
    let mut rng = Rng(0x4D4F_4445);
    let (mut handlers, mut skipped, mut aborted) = (std::collections::BTreeSet::new(), 0, 0);
    for run in 0..2000 {
        let index = 1 + 2 * rng.below(14) as usize;
        let mode = GameMode::from_index(index).expect("mode");
        let handler = cpu.mem.rd32(TABLE + index as u32 * 24 + 0x10);
        let begin = [0, 0, 1][rng.below(3) as usize];
        let body = [0, 0, 1, 7][rng.below(4) as usize];
        *rets.borrow_mut() = (begin, body);
        log.borrow_mut().clear();
        assert_eq!(cpu.call(handler, &[]), Stop::Returned);
        handlers.insert(handler);
        let retail = log.borrow().clone();

        let stage = per_frame_stage(mode).expect("a per-frame mode");
        let mut port = vec![Ev::Begin];
        if begin == 0 {
            if let Some(h) = stage.overlay_hook {
                port.push(Ev::Hook(h));
            }
            port.push(match stage.body {
                FrameBody::Master { param } => Ev::Master(param),
                FrameBody::CardDriver => Ev::Card,
            });
            if !(stage.body_can_abort && body != 0) {
                port.push(Ev::End);
            }
        }
        skipped += u32::from(begin != 0);
        aborted += u32::from(begin == 0 && body != 0);
        assert_eq!(
            retail, port,
            "run {run} mode {index} begin {begin} body {body}"
        );
    }
    assert_eq!(handlers.len(), 3, "{handlers:x?}");
    assert!(skipped > 300 && aborted > 300, "{skipped} {aborted}");
}

#[test]
fn the_card_body_runs_the_card_actor_and_returns_zero() {
    differential!(scus(CARD_BODY));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let log: Rc<RefCell<Vec<(u32, u32)>>> = Rc::new(RefCell::new(Vec::new()));
    for at in [CHORD, READOUT, CARD_TICK, MASTER] {
        let l = log.clone();
        cpu.hook(at, move |regs, _| {
            l.borrow_mut().push((at, regs.r[A0]));
            regs.r[V0] = 0x55;
        });
    }
    cpu.mem.wr32(CARD_ACTOR_PTR, CARD_ACTOR);
    cpu.mem.wr32(CARD_ACTOR + 0xC, CARD_TICK);
    for step in [1u8, 2, 3] {
        cpu.mem.wr8(0x1F80_0391, step);
        cpu.mem.wr8(0x1F80_0393, 0xEE);
        log.borrow_mut().clear();
        assert_eq!(cpu.call(CARD_BODY, &[]), Stop::Returned);
        let calls = log.borrow().clone();
        assert_eq!(cpu.regs.r[V0] as i32, CARD_FRAME_BODY.returns);
        assert_eq!(
            calls.iter().any(|(a, _)| *a == MASTER),
            CARD_FRAME_BODY.runs_master_driver
        );
        assert_eq!(
            calls
                .iter()
                .any(|(a, x)| *a == CARD_TICK && *x == CARD_ACTOR),
            CARD_FRAME_BODY.ticks_card_actor
        );
        assert_eq!(calls[0].0, CHORD);
        assert_eq!(calls[2], (READOUT, u32::from(step)), "{calls:x?}");
    }
}
