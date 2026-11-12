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
//!
//! The master driver `FUN_80016444` itself runs against
//! `master_frame_plan`: its tick, render, flip, horizon and readout passes in
//! order, with the list head each pass is handed.

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

/// The master frame driver `FUN_80016444` over every combination of what
/// picks its passes - the argument, the tick hold `_DAT_8007B6E8`, the mirror
/// bit `_DAT_1F800394 & 0x10`, the STR mode and the world-map submode - with
/// the dev words raised or not, against `mode::master_frame_plan`. Each pass
/// is hooked and logged with the list head it was handed; the dev prints,
/// the `VSync(1)` timestamps and the heap queries are hooked and ignored.
/// The held-tick counter `_DAT_8007B8BC` is compared too.
#[test]
fn the_master_frame_driver_runs_the_plans_passes() {
    differential!(scus(MASTER));
    use legaia_engine_mode::mode::{
        ACTOR_LIST_HEADS, FramePass, MasterFrameInputs, master_frame_plan,
    };
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.stub_outside = Some((MASTER, 0x8001_698C));
    let log: Rc<RefCell<Vec<FramePass>>> = Rc::new(RefCell::new(Vec::new()));
    let head_of = |a: u32| {
        // The heads are seeded with their own addresses.
        assert!(ACTOR_LIST_HEADS.contains(&a), "{a:#x}");
        a
    };
    let simple = [
        (CHORD, FramePass::DevChord),
        (0x8001_72C0, FramePass::View),
        (0x8001_D058, FramePass::Flip),
        (0x8004_6978, FramePass::WashDrain),
        (0x801D_7EA0, FramePass::Horizon),
        (READOUT, FramePass::Readout),
    ];
    for (va, pass) in simple {
        let l = log.clone();
        cpu.hook(va, move |_, _| l.borrow_mut().push(pass));
    }
    {
        let l = log.clone();
        cpu.hook(0x8002_519C, move |r, _| {
            l.borrow_mut().push(FramePass::Tick(head_of(r.r[A0])))
        });
        let l = log.clone();
        cpu.hook(0x8001_D140, move |r, _| {
            l.borrow_mut().push(FramePass::Render(head_of(r.r[A0])))
        });
    }
    for va in [0x8001_A068, 0x8005_FB84, 0x8002_B688, 0x8002_B790] {
        cpu.hook(va, |r, _| r.r[V0] = 0);
    }
    for &h in &ACTOR_LIST_HEADS {
        cpu.mem.wr32(h, h);
    }
    let mut runs = 0;
    for bits in 0..256u32 {
        let i = MasterFrameInputs {
            param: (bits & 1) as i32,
            tick_hold: bits & 2 != 0,
            mirror: bits & 4 != 0,
            mode: if bits & 8 != 0 { 0x15 } else { 0x03 },
            submode: if bits & 16 != 0 { 2 } else { 1 },
        };
        let dev = bits >> 5;
        cpu.mem
            .wr32(0x8007_B6D0, if dev & 1 != 0 { 0xF } else { 0 });
        cpu.mem
            .wr32(0x8007_B98C, if dev & 2 != 0 { 0x8000_0000 } else { 0 });
        cpu.mem.wr16(0x8007_B86E, if dev & 4 != 0 { 1 } else { 0 });
        cpu.mem.wr32(0x8007_B6E8, u32::from(i.tick_hold));
        cpu.mem.wr32(0x1F80_0394, if i.mirror { 0x10 } else { 0 });
        cpu.mem.wr16(0x8007_B83C, i.mode);
        cpu.mem.wr32(0x8007_BC3C, i.submode);
        cpu.mem.wr16(0x8007_B8BC, 7);
        log.borrow_mut().clear();
        assert_eq!(cpu.call(MASTER, &[i.param as u32]), Stop::Returned);
        assert_eq!(cpu.regs.r[V0], 0);
        let plan = master_frame_plan(i);
        // The held tick is the counter, not a call.
        let calls: Vec<FramePass> = plan
            .iter()
            .copied()
            .filter(|p| *p != FramePass::HeldTick)
            .collect();
        assert_eq!(*log.borrow(), calls, "{i:?} dev {dev}");
        let held = plan.contains(&FramePass::HeldTick);
        assert_eq!(cpu.mem.rd16(0x8007_B8BC), 7 + u16::from(held), "{i:?}");
        runs += 1;
    }
    assert_eq!(runs, 256);
}
