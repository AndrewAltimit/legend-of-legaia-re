//! The sparring tutorial's overlay (PROT 0967) against `battle_tutorial`:
//!
//! - the hook `FUN_801F6B70` over every flow state of its table
//!   (`0x1E..=0x78`), every lesson and generated inputs (the Auto flag
//!   `ctx[+0x266]`, the two option words, the committed category and the
//!   command buffer of seat 0): the boxes it emits through `FUN_801F747C`
//!   (string and style, in order), whether it arms the countdown
//!   `FUN_801F7628`, and the completion tail's stores, against
//!   `BattleTutorial::tick`;
//! - the emitter `FUN_801F747C` over every style and generated text sizes:
//!   the registrar call `FUN_8003541C` it makes, against `BoxStyle::box_rect`;
//! - the countdown `FUN_801F7628` and the hook's run-down of it, pass by
//!   pass at every frame step, against `arm_countdown` / `tick_countdown`.
//!
//! Every call out of the image returns `0` but the ones named.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_battle::battle_tutorial::{
    BattleTutorial, BoxStyle, COMPLETION_COUNTDOWN_VSYNCS, CountdownTick, FLOW_STATE_BATTLE_OVER,
    TutorialInputs,
};
use psx_retail_exec::{A0, A1, A2, A3, Cpu, SP, Stop, V0};
use std::cell::RefCell;
use std::rc::Rc;

const BASE: u32 = 0x801F_69D8;
const TICK: u32 = 0x801F_6B70;
const EMIT: u32 = 0x801F_747C;
const COUNTDOWN: u32 = 0x801F_7628;
const CTX_PTR: u32 = 0x8007_BD24;
const CTX: u32 = 0x8012_0000;
const ACTORS: u32 = 0x801C_9370;
const ACTOR: u32 = 0x8013_0000;
const LINES: u32 = 0x8003_CBA8;
const WIDTH: u32 = 0x8003_5F04;
const REGISTER: u32 = 0x8003_541C;
const SCALAR: u32 = 0x1F80_037D;
const STEP: u32 = 0x1F80_0393;
const OPT_COMMAND: u32 = 0x8008_46C8;
const OPT_SELECT: u32 = 0x8008_46C4;
const CANCEL_MASK: u32 = 0x8008_46D4;

fn image_cpu() -> Option<Cpu> {
    let mut cpu = scus_cpu()?;
    let image = disc()?.read_entry(967).ok()?;
    cpu.mem.load(BASE, &image);
    cpu.mem.wr32(CTX_PTR, CTX);
    let mut targets: Vec<u32> = image
        .as_chunks::<4>()
        .0
        .iter()
        .map(|w| u32::from_le_bytes(*w))
        .filter(|w| w >> 26 == 3)
        .map(|w| ((w & 0x03FF_FFFF) << 2) | 0x8000_0000)
        .filter(|&t| !(BASE..BASE + 0x2000).contains(&t))
        .collect();
    targets.sort_unstable();
    targets.dedup();
    for t in targets {
        cpu.stub(t, 0);
    }
    Some(cpu)
}

fn clear_ctx(cpu: &mut Cpu) {
    for b in (0..0x1000).step_by(4) {
        cpu.mem.wr32(CTX + b, 0);
    }
    cpu.mem.wr32(ACTORS, ACTOR);
    for b in (0..0x400).step_by(4) {
        cpu.mem.wr32(ACTOR + b, 0);
    }
}

#[test]
fn the_tutorial_hook_emits_what_retail_does() {
    differential!(prot(967, TICK));
    let Some(mut cpu) = image_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let emitted: Rc<RefCell<Vec<(u32, u8)>>> = Rc::default();
    {
        let emitted = emitted.clone();
        cpu.hook(EMIT, move |r, _| {
            emitted.borrow_mut().push((r.r[A0], r.r[A1] as u8));
        });
    }
    let armed = Rc::new(std::cell::Cell::new(0u32));
    {
        let armed = armed.clone();
        cpu.hook(COUNTDOWN, move |_, _| armed.set(armed.get() + 1));
    }
    let mut rng = Rng(0x5455_544F);
    let mut failures = Vec::new();
    let (mut boxes, mut rewinds, mut closes) = (0, 0, 0);
    for flow in 0x1Eu8..=0x78 {
        for lesson in 0u8..=5 {
            for _ in 0..8 {
                let mut buf = [0u8; 5];
                for b in &mut buf {
                    *b = [0x0E, 0x0F, 0x00, 0x11][rng.below(4) as usize];
                }
                if rng.below(3) == 0 {
                    let at = rng.below(3) as usize;
                    buf[at..at + 3].copy_from_slice(&[0x0F, 0x0E, 0x0F]);
                }
                let inputs = TutorialInputs {
                    auto_attack: rng.below(2) == 0,
                    battle_command_cross: rng.below(2) == 0,
                    select_attack_automatic: rng.below(3) == 0,
                    action_category: rng.below(6) as u8,
                    command_buffer: buf,
                };
                clear_ctx(&mut cpu);
                cpu.mem.wr8(CTX + 0x06, flow);
                cpu.mem.wr8(CTX + 0x28A, lesson);
                cpu.mem.wr8(CTX + 0x266, u8::from(inputs.auto_attack));
                cpu.mem
                    .wr32(OPT_COMMAND, u32::from(inputs.battle_command_cross));
                cpu.mem
                    .wr32(OPT_SELECT, u32::from(inputs.select_attack_automatic));
                cpu.mem.wr8(ACTOR + 0x1DE, inputs.action_category);
                for (i, &b) in buf.iter().enumerate() {
                    cpu.mem.wr8(ACTOR + 0x1DF + i as u32, b);
                }
                emitted.borrow_mut().clear();
                armed.set(0);
                assert_eq!(cpu.call(TICK, &[]), Stop::Returned);
                let got = emitted.borrow().clone();

                let mut t = BattleTutorial::new();
                t.lesson = lesson;
                t.inputs = inputs;
                // A lesson already past the sign-off has run its tail.
                t.finished = lesson > 4;
                let tick = t.tick(flow);
                let want: Vec<(u32, u8)> = tick
                    .emission
                    .boxes
                    .iter()
                    .map(|b| (b.message, b.style))
                    .collect();
                let c = format!("flow {flow} lesson {lesson} {inputs:?}");
                if got != want {
                    failures.push(format!("{c}: boxes retail {got:x?} port {want:x?}"));
                }
                // Retail arms the countdown on a rewind and on the sign-off;
                // the port's rewind reopens the menu itself.
                let port_arms = tick.emission.rewind || (lesson == 4 && tick.battle_over);
                if (armed.get() != 0) != port_arms {
                    failures.push(format!(
                        "{c}: countdown armed retail {} port {port_arms}",
                        armed.get()
                    ));
                }
                let over = cpu.mem.rd8(CTX + 0x06) == FLOW_STATE_BATTLE_OVER
                    && cpu.mem.rd8(CTX + 0x07) == 0xFF;
                if over != tick.battle_over || cpu.mem.rd8(CTX + 0x28A) != t.lesson {
                    failures.push(format!(
                        "{c}: tail retail over {over} lesson {} port over {} lesson {}",
                        cpu.mem.rd8(CTX + 0x28A),
                        tick.battle_over,
                        t.lesson
                    ));
                }
                boxes += got.len();
                rewinds += u32::from(tick.emission.rewind);
                closes += u32::from(tick.battle_over);
            }
        }
    }
    let n = failures.len();
    failures.truncate(40);
    assert!(n == 0, "{n} mismatches:\n{}", failures.join("\n"));
    assert!(
        boxes > 500 && rewinds > 100 && closes > 100,
        "{boxes} {rewinds} {closes}"
    );
}

#[test]
fn the_tutorial_box_is_sized_as_retail_sizes_it() {
    differential!(prot(967, EMIT));
    let Some(mut cpu) = image_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let size = Rc::new(std::cell::Cell::new((0u32, 0u32)));
    {
        let size = size.clone();
        cpu.hook(LINES, move |r, _| r.r[V0] = size.get().0);
    }
    {
        let size = size.clone();
        cpu.hook(WIDTH, move |r, _| r.r[V0] = size.get().1);
    }
    let reg: Rc<RefCell<Vec<[i32; 8]>>> = Rc::default();
    {
        let reg = reg.clone();
        cpu.hook(REGISTER, move |r, m| {
            let sp = r.r[SP];
            let mut arg = |o: u32| m.rd32(sp + o) as i32;
            reg.borrow_mut().push([
                r.r[A0] as i32,
                r.r[A1] as i32,
                r.r[A2] as i32,
                r.r[A3] as i32,
                arg(0x10),
                arg(0x14),
                arg(0x18),
                arg(0x1C),
            ]);
        });
    }
    let mut rng = Rng(0x424F_5853);
    let mut failures = Vec::new();
    for style in 0u8..=9 {
        for _ in 0..40 {
            let lines = 1 + rng.below(4);
            let width = 8 + rng.below(0x110);
            size.set((lines, width));
            reg.borrow_mut().clear();
            let text = 0x801F_8000 + rng.below(0x100) * 4;
            assert_eq!(cpu.call(EMIT, &[text, u32::from(style)]), Stop::Returned);
            let b = BoxStyle::from_raw(style).expect("style");
            let (x, y, w, h) = b.box_rect(width as i16, lines as i16);
            let waits = i32::from(b.waits_for_input);
            let want = [
                1 + waits,
                0xD,
                text as i32,
                i32::from(x),
                i32::from(y),
                i32::from(w),
                i32::from(h),
                0x44 - waits,
            ];
            match reg.borrow().as_slice() {
                [got] if *got == want => {}
                got => failures.push(format!(
                    "style {style} lines {lines} width {width}: retail {got:x?} port {want:x?}"
                )),
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn the_tutorial_countdown_runs_down_as_retails() {
    differential!(prot(967, COUNTDOWN), prot(967, TICK));
    let Some(mut cpu) = image_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.stub(EMIT, 0);
    let mut failures = Vec::new();
    for step in 1u8..=4 {
        // A lesson of `4` runs the sign-off tail on the hook's first call,
        // which arms the countdown afresh; past it (`5`) the run-down closes.
        for lesson in [0u8, 2, 3, 5] {
            for scalar in [8u8, 16] {
                clear_ctx(&mut cpu);
                cpu.mem.wr8(SCALAR, scalar);
                cpu.mem.wr8(STEP, step);
                cpu.mem.wr32(CANCEL_MASK, 0x40);
                cpu.mem.wr8(CTX + 0x28A, lesson);
                assert_eq!(cpu.call(COUNTDOWN, &[]), Stop::Returned);
                let mut t = BattleTutorial::new();
                t.lesson = lesson;
                t.arm_countdown();
                let c = format!("step {step} lesson {lesson} scalar {scalar}");
                let seeded = i32::from(cpu.mem.rd16(CTX + 0x6B4) as i16);
                if seeded != i32::from(COMPLETION_COUNTDOWN_VSYNCS) * i32::from(scalar)
                    || cpu.mem.rd16(CTX + 0x6B0) != 1
                    || cpu.mem.rd32(CTX + 0x88C) != 0x40
                {
                    failures.push(format!("{c}: seeded {seeded}"));
                }
                // The hook runs the section on every call; the latch keeps
                // its dispatch from running.
                cpu.mem.wr16(CTX + 0x6AE, 1);
                cpu.mem.wr8(CTX + 0x06, 0x1E);
                let mut retail_passes = 0;
                let mut retail_end = None;
                for pass in 1..=400 {
                    let phase = cpu.mem.rd8(CTX + 0x289);
                    assert_eq!(cpu.call(TICK, &[]), Stop::Returned);
                    if cpu.mem.rd16(CTX + 0x6B4) as i16 <= 0 {
                        retail_passes = pass;
                        retail_end = Some(if cpu.mem.rd8(CTX + 0x289) != phase {
                            "close"
                        } else {
                            "rewind"
                        });
                        break;
                    }
                }
                let mut port_passes = 0;
                let mut port_end = None;
                for pass in 1..=400 {
                    match t.tick_countdown(false, false, step) {
                        CountdownTick::Holding => {}
                        CountdownTick::Rewind => {
                            port_passes = pass;
                            port_end = Some("rewind");
                            break;
                        }
                        CountdownTick::Close => {
                            port_passes = pass;
                            port_end = Some("close");
                            break;
                        }
                        CountdownTick::Idle => break,
                    }
                }
                if (retail_passes, retail_end) != (port_passes, port_end) {
                    failures.push(format!(
                        "{c}: retail {retail_end:?} after {retail_passes} passes, port {port_end:?} after {port_passes}"
                    ));
                }
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
