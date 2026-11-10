//! Four of the field overlay's panel actors (PROT 0897), each a
//! `PTR_FUN_801F33B4` slot, against `legaia_engine_field::world_map_panel_actors`:
//! the brightness fade / flash `FUN_801ED308`, the sub-list `FUN_801ED590`, the
//! records slide `FUN_801EDF00` and the fill fade `FUN_801EE5D4`.
//!
//! Each is a phase machine over globals and a handful of calls, so every call
//! is hooked and logged with its first argument, and the port's effect list is
//! unfolded into the calls and stores it stands for.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_field::world_map_panel::CursorPad;
use legaia_engine_field::world_map_panel_actors::{
    FadeFlashEffect, FadeFlashInput, FillFadeEffect, FillFadeInput, SoftResetEffect,
    SoftResetInput, SubListEffect, SubListInput, TextBoxEffect, TextBoxInput, fade_flash_tick,
    fill_fade_tick, soft_reset_tick, sub_list_tick, text_box_tick,
};
use psx_retail_exec::{A0, A1, Cpu, Stop, V0};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

const OVERLAY_BASE: u32 = 0x801C_E818;
const FADE_FLASH: u32 = 0x801E_D308;
const SUB_LIST: u32 = 0x801E_D590;
const SOFT_RESET: u32 = 0x801E_DF00;
const FILL_FADE: u32 = 0x801E_E5D4;
const TEXT_BOX: u32 = 0x801E_E90C;

const APPLY_BRIGHTNESS: u32 = 0x8003_479C;
const SAVE_SPAWN: u32 = 0x801D_841C;
const PANEL_SCRIPT: u32 = 0x801E_9B3C;
const REPLACE_CUE: u32 = 0x8003_5BD0;
const PUSH_CUE: u32 = 0x8003_5B50;
const DRAW_PUMP: u32 = 0x8003_1D00;
const HANDOFF_A: u32 = 0x8002_66E0;
const HANDOFF_B: u32 = 0x801D_84B4;
const AGGREGATE: u32 = 0x8004_2558;
const RECORDS: u32 = 0x801E_D710;
const FADE: u32 = 0x801D_58F0;
const RELOAD: u32 = 0x8001_7714;
const POST_PRIM: u32 = 0x8002_4E80;
const SPAWN: u32 = 0x8002_0DE0;
const LOAD_BUSY: u32 = 0x8003_CF04;
const QUEUE_DMA: u32 = 0x8003_CF40;

const FRAME_STEP: u32 = 0x1F80_0393;
const LEVEL: u32 = 0x8007_B440;
const COUNTER: u32 = 0x8007_B43C;
const TINT: u32 = 0x8007_BF30 + 0x2D;
const TINT_MIRROR: u32 = 0x8007_BF30 + 0xA1;
const TINT_SAVE: u32 = 0x8007_B634;
const GRID_PTR: u32 = 0x801C_6EA4;
const AUDIO_LEVEL: u32 = 0x8007_B910;
const ROW: u32 = 0x8007_BB88;
const BLOCKED: u32 = 0x8007_BB80;
const OPERAND_PTR: u32 = 0x8007_B450;
const SLIDE: u32 = 0x801F_35B8;
const RECORD_TICKS: u32 = 0x8007_B792;
const PAD_HELD: u32 = 0x8007_B850;
const PLAYER_PTR: u32 = 0x8007_C364;

const ACTOR: u32 = 0x8012_0000;
const GRID: u32 = 0x8012_1000;
const PLAYER: u32 = 0x8012_3000;

type Log = Rc<RefCell<Vec<(u32, u32)>>>;

fn hooked(image: &[u8], logged: &[u32]) -> (Cpu, Log) {
    let mut cpu = scus_cpu().expect("checked by the caller");
    cpu.mem.load(OVERLAY_BASE, image);
    let log: Log = Rc::new(RefCell::new(Vec::new()));
    for &va in logged {
        let l = log.clone();
        cpu.hook(va, move |r, _| l.borrow_mut().push((va, r.r[A0])));
    }
    cpu.mem.wr32(GRID_PTR, GRID);
    cpu.mem.wr32(PLAYER_PTR, PLAYER);
    (cpu, log)
}

/// The grid's hand-back cells and the actor's handler id and phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Seat {
    handback: i16,
    done_gate: i16,
    stashed: u16,
    handler: u16,
    phase: i16,
}

fn write_seat(cpu: &mut Cpu, s: Seat) {
    cpu.mem.wr16(GRID + 0x2E, s.handback as u16);
    cpu.mem.wr16(GRID + 0x3E, s.done_gate as u16);
    cpu.mem.wr16(GRID + 0x40, s.stashed);
    cpu.mem.wr16(ACTOR + 0x50, s.handler);
    cpu.mem.wr16(ACTOR + 0x54, s.phase as u16);
}

fn read_seat(cpu: &mut Cpu) -> Seat {
    Seat {
        handback: cpu.mem.rd16(GRID + 0x2E) as i16,
        done_gate: cpu.mem.rd16(GRID + 0x3E) as i16,
        stashed: cpu.mem.rd16(GRID + 0x40),
        handler: cpu.mem.rd16(ACTOR + 0x50),
        phase: cpu.mem.rd16(ACTOR + 0x54) as i16,
    }
}

fn seat(rng: &mut Rng, handler: u16, phases: i16) -> Seat {
    Seat {
        handback: rng.below(3) as i16,
        done_gate: 1 + rng.below(3) as i16,
        stashed: rng.below(0x34) as u16,
        handler,
        phase: rng.below(phases as u32 + 2) as i16 - 1,
    }
}

#[test]
fn the_brightness_flash_matches_retail() {
    differential!(prot(897, FADE_FLASH));
    let Some(disc) = disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let (mut cpu, log) = hooked(
        &disc.read_entry(897).expect("PROT 0897"),
        &[APPLY_BRIGHTNESS, SAVE_SPAWN],
    );
    let mut rng = Rng(0x464C_4153);
    let (mut spawned, mut restored, mut exits, mut decoded) = (0, 0, 0, 0);
    for round in 0..12000 {
        let before = seat(&mut rng, 0x30, 8);
        let input = FadeFlashInput {
            frame_delta: 1 + rng.below(4) as i32,
            level: match rng.below(4) {
                0 => 0xF2,
                1 => rng.below(0x20) as i32,
                _ => rng.below(0x100) as i32,
            },
            flash_counter: rng.below(10) as i32,
            handler_id: before.handler,
        };
        let live: [u8; 3] = std::array::from_fn(|_| rng.below(256) as u8);
        let saved: [u8; 3] = std::array::from_fn(|_| rng.below(256) as u8);
        write_seat(&mut cpu, before);
        cpu.mem.wr8(FRAME_STEP, input.frame_delta as u8);
        cpu.mem.wr32(LEVEL, input.level as u32);
        cpu.mem.wr32(COUNTER, input.flash_counter as u32);
        for i in 0..3u32 {
            cpu.mem.wr8(TINT + i, live[i as usize]);
            cpu.mem.wr8(TINT_MIRROR + i, 0x99);
            // The save slots run backwards: `+0x2D` lands at `0x8007B636`.
            cpu.mem.wr8(TINT_SAVE + 2 - i, saved[i as usize]);
        }
        log.borrow_mut().clear();
        assert_eq!(
            cpu.call(FADE_FLASH, &[ACTOR]),
            Stop::Returned,
            "round {round}"
        );

        let (phase, level, counter, effects) = fade_flash_tick(before.phase, input);
        let mut want = before;
        want.phase = phase;
        let (mut calls, mut tint, mut mirror, mut save) = (Vec::new(), live, [0x99u8; 3], saved);
        for e in &effects {
            match *e {
                FadeFlashEffect::ApplyBrightness(l) => calls.push((APPLY_BRIGHTNESS, l as u32)),
                FadeFlashEffect::CaptureAndClearTint => {
                    save = tint;
                    tint = [0; 3];
                    mirror = [0; 3];
                }
                FadeFlashEffect::SpawnSaveScreen => calls.push((SAVE_SPAWN, 0)),
                FadeFlashEffect::RestoreTint => {
                    tint = save;
                    mirror = save;
                }
                FadeFlashEffect::ClearSceneField3E => want.done_gate = 0,
                FadeFlashEffect::Exit(x) => {
                    let (mut ph, mut h) = (want.phase, want.handler);
                    x.apply(&mut want.handback, &mut want.stashed, &mut h, &mut ph);
                    want.handler = h;
                    want.phase = ph;
                    exits += 1;
                }
            }
        }
        let ctx = format!("round {round} phase {} {input:?}", before.phase);
        let got: Vec<(u32, u32)> = log
            .borrow()
            .iter()
            .map(|&(va, a0)| (va, if va == SAVE_SPAWN { 0 } else { a0 }))
            .collect();
        assert_eq!(got, calls, "calls: {ctx}");
        assert_eq!(read_seat(&mut cpu), want, "seat: {ctx}");
        assert_eq!(cpu.mem.rd32(LEVEL) as i32, level, "level: {ctx}");
        assert_eq!(cpu.mem.rd32(COUNTER) as i32, counter, "counter: {ctx}");
        for i in 0..3u32 {
            let k = i as usize;
            assert_eq!(cpu.mem.rd8(TINT + i), tint[k], "tint {i}: {ctx}");
            assert_eq!(cpu.mem.rd8(TINT_MIRROR + i), mirror[k], "mirror {i}: {ctx}");
            assert_eq!(cpu.mem.rd8(TINT_SAVE + 2 - i), save[k], "save {i}: {ctx}");
        }
        spawned += u32::from(effects.contains(&FadeFlashEffect::SpawnSaveScreen));
        restored += u32::from(effects.contains(&FadeFlashEffect::RestoreTint));
        decoded += u32::from(before.phase == 4 && phase != 4);
    }
    assert!(
        spawned > 300 && restored > 300 && exits > 1000 && decoded > 100,
        "{spawned} {restored} {exits} {decoded}"
    );
}

#[test]
fn the_sub_list_matches_retail() {
    differential!(prot(897, SUB_LIST));
    let Some(disc) = disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let (mut cpu, log) = hooked(
        &disc.read_entry(897).expect("PROT 0897"),
        &[
            PANEL_SCRIPT,
            REPLACE_CUE,
            DRAW_PUMP,
            HANDOFF_A,
            HANDOFF_B,
            PUSH_CUE,
        ],
    );
    let mut rng = Rng(0x5355_424C);
    let (mut confirmed, mut cancelled, mut handed, mut closed) = (0, 0, 0, 0);
    for round in 0..12000 {
        let before = seat(&mut rng, 0x31, 4);
        let pad = CursorPad {
            held: match rng.below(3) {
                0 => 0x20,
                1 => 0x40,
                _ => 0,
            } | (rng.next() & 0x0F00),
            pressed: [0, 0x1000, 0x4000][rng.below(3) as usize],
            action_a_mask: 0x20,
            action_b_mask: 0x40,
        };
        let input = SubListInput {
            input_locked: rng.below(5) == 0,
            cursor: rng.below(2) as i32,
            pad,
            handler_id: before.handler,
        };
        let audio = [0x7Fi32, 0x40, 1, 0, -0x30, 0x3FFF][rng.below(6) as usize];
        write_seat(&mut cpu, before);
        cpu.mem.wr32(BLOCKED, u32::from(input.input_locked));
        cpu.mem.wr32(ROW, input.cursor as u32);
        cpu.mem.wr32(0x8007_B874, pad.held);
        cpu.mem.wr32(0x8007_BB84, pad.pressed);
        cpu.mem.wr32(0x8008_46D0, pad.action_a_mask);
        cpu.mem.wr32(0x8008_46D4, pad.action_b_mask);
        cpu.mem.wr32(AUDIO_LEVEL, audio as u32);
        cpu.mem.wr32(OPERAND_PTR, 0x8012_2000);
        log.borrow_mut().clear();
        assert_eq!(
            cpu.call(SUB_LIST, &[ACTOR]),
            Stop::Returned,
            "round {round}"
        );

        let (phase, cursor, effects) = sub_list_tick(before.phase, input);
        let mut want = before;
        want.phase = phase;
        let (mut calls, mut level, mut operand) = (Vec::new(), audio, 0x8012_2000u32);
        for e in &effects {
            match *e {
                SubListEffect::RunPanelScript(va) => calls.push((PANEL_SCRIPT, va)),
                SubListEffect::ScaleAudioLevel { shift_right } => {
                    level = if shift_right { level >> 1 } else { level << 1 };
                }
                SubListEffect::PlaySfx(id) => calls.push((REPLACE_CUE, id)),
                SubListEffect::ClearWindowDescriptor => operand = 0,
                SubListEffect::HandOff => {
                    calls.push((HANDOFF_A, 0x8007_052C));
                    calls.push((HANDOFF_B, 0));
                    handed += 1;
                }
                SubListEffect::Exit(x) => {
                    let (mut ph, mut h) = (want.phase, want.handler);
                    x.apply(&mut want.handback, &mut want.stashed, &mut h, &mut ph);
                    want.handler = h;
                    want.phase = ph;
                    closed += 1;
                }
                SubListEffect::TickTextActors => calls.push((DRAW_PUMP, 0)),
            }
        }
        let ctx = format!("round {round} phase {} {input:?}", before.phase);
        // The cursor's own cues are the list cursor's, compared on their own;
        // the pump's and the hand-off's arguments are not theirs to read.
        let got: Vec<(u32, u32)> = log
            .borrow()
            .iter()
            .filter(|c| c.0 != PUSH_CUE)
            .map(|&(va, a0)| {
                (
                    va,
                    if va == DRAW_PUMP || va == HANDOFF_B {
                        0
                    } else {
                        a0
                    },
                )
            })
            .collect();
        assert_eq!(got, calls, "calls: {ctx}");
        assert_eq!(read_seat(&mut cpu), want, "seat: {ctx}");
        assert_eq!(cpu.mem.rd32(ROW) as i32, cursor, "cursor: {ctx}");
        assert_eq!(cpu.mem.rd32(AUDIO_LEVEL) as i32, level, "audio: {ctx}");
        assert_eq!(cpu.mem.rd32(OPERAND_PTR), operand, "operand: {ctx}");
        confirmed += u32::from(
            effects
                .iter()
                .any(|e| matches!(e, SubListEffect::PlaySfx(_))),
        );
        cancelled += u32::from(before.phase == 1 && phase == 2 && got.len() == 1);
    }
    assert!(
        confirmed > 300 && cancelled > 300 && handed > 1000 && closed > 1000,
        "{confirmed} {cancelled} {handed} {closed}"
    );
}

#[test]
fn the_records_slide_matches_retail() {
    differential!(prot(897, SOFT_RESET));
    let Some(disc) = disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let (mut cpu, log) = hooked(
        &disc.read_entry(897).expect("PROT 0897"),
        &[AGGREGATE, FADE, RELOAD, DRAW_PUMP],
    );
    let l = log.clone();
    cpu.hook(RECORDS, move |r, _| {
        assert_eq!(r.r[A0], 0x20, "the records renderer's x");
        l.borrow_mut().push((RECORDS, r.r[A1]));
    });
    let mut rng = Rng(0x534C_4944);
    let (mut slid, mut accepted, mut reloaded) = (0, 0, 0);
    for round in 0..10000 {
        let before = seat(&mut rng, 0x33, 4);
        let input = SoftResetInput {
            frame_delta: 1 + rng.below(4) as i32,
            slide: match rng.below(5) {
                0 => 0xE,
                1 => 0xF,
                2 => 0x74 + rng.below(6) as i32,
                _ => rng.below(0xE8) as i32,
            },
            pad: match rng.below(3) {
                0 => 0,
                1 => 1 << rng.below(16),
                _ => rng.next() & 0xFFFF,
            },
        };
        let ticks = rng.below(0x1_0000) as u16;
        write_seat(&mut cpu, before);
        cpu.mem.wr8(FRAME_STEP, input.frame_delta as u8);
        cpu.mem.wr32(SLIDE, input.slide as u32);
        cpu.mem.wr32(PAD_HELD, input.pad);
        cpu.mem.wr16(RECORD_TICKS, ticks);
        log.borrow_mut().clear();
        assert_eq!(
            cpu.call(SOFT_RESET, &[ACTOR]),
            Stop::Returned,
            "round {round}"
        );

        let (phase, slide, effects) = soft_reset_tick(before.phase, input);
        let mut want = before;
        want.phase = phase;
        let (mut calls, mut drawn) = (Vec::new(), 0u16);
        for e in &effects {
            match *e {
                SoftResetEffect::ArmReset => calls.push((AGGREGATE, 0)),
                SoftResetEffect::DrawRecords { y } => {
                    calls.push((RECORDS, y as u32));
                    drawn += 1;
                }
                SoftResetEffect::WhiteFade { .. } => calls.push((FADE, 2)),
                SoftResetEffect::ReloadExecutable => calls.push((RELOAD, 0x801C_F5A0)),
                SoftResetEffect::TickTextActors => calls.push((DRAW_PUMP, 0)),
            }
        }
        let ctx = format!("round {round} phase {} {input:?}", before.phase);
        let got: Vec<(u32, u32)> = log
            .borrow()
            .iter()
            .map(|&(va, a0)| {
                (
                    va,
                    if va == DRAW_PUMP || va == AGGREGATE {
                        0
                    } else {
                        a0
                    },
                )
            })
            .collect();
        assert_eq!(got, calls, "calls: {ctx}");
        assert_eq!(read_seat(&mut cpu), want, "seat: {ctx}");
        assert_eq!(cpu.mem.rd32(SLIDE) as i32, slide, "slide: {ctx}");
        assert_eq!(
            cpu.mem.rd16(RECORD_TICKS),
            ticks.wrapping_add(drawn),
            "ticks: {ctx}"
        );
        slid += u32::from(before.phase == 1 && slide < input.slide);
        accepted += u32::from(before.phase == 1 && phase == 2);
        reloaded += u32::from(effects.contains(&SoftResetEffect::ReloadExecutable));
    }
    assert!(
        slid > 500 && accepted > 100 && reloaded > 100,
        "{slid} {accepted} {reloaded}"
    );
}

#[test]
fn the_fill_fade_matches_retail() {
    differential!(prot(897, FILL_FADE));
    let Some(disc) = disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let (mut cpu, log) = hooked(
        &disc.read_entry(897).expect("PROT 0897"),
        &[PANEL_SCRIPT, POST_PRIM, DRAW_PUMP, SPAWN, QUEUE_DMA],
    );
    let busy = Rc::new(Cell::new(0u32));
    let b = busy.clone();
    cpu.hook(LOAD_BUSY, move |r, _| r.r[V0] = b.get());
    // The second fill of case 2 is posted only while the scene's own fill
    // handle `0x8007B62C` is live; the port's effect names the first.
    const SCENE_FILL: u32 = 0x8007_B62C;
    let mut rng = Rng(0x4649_4C4C);
    let (mut spawned, mut queued, mut exits, mut held) = (0, 0, 0, 0);
    for round in 0..10000 {
        let before = seat(&mut rng, 0x2E, 5);
        let input = FillFadeInput {
            frame_delta: 1 + rng.below(4) as i16,
            timer: match rng.below(4) {
                0 => 0x10 - rng.below(5) as i16,
                1 => -(rng.below(4) as i16),
                _ => rng.below(0x14) as i16,
            },
            input_locked: rng.below(4) == 0,
            load_pending: rng.below(2) == 0,
            handler_id: before.handler,
        };
        let live: [u8; 3] = std::array::from_fn(|_| rng.below(256) as u8);
        let saved: [u8; 3] = std::array::from_fn(|_| rng.below(256) as u8);
        let player_flags = rng.next() & !0x8_0000;
        let scene_fill = rng.below(2) * 0x8012_5000;
        write_seat(&mut cpu, before);
        cpu.mem.wr8(FRAME_STEP, input.frame_delta as u8);
        cpu.mem.wr16(ACTOR + 0x9E, input.timer as u16);
        cpu.mem.wr32(BLOCKED, u32::from(input.input_locked));
        cpu.mem.wr32(PLAYER + 0x10, player_flags);
        cpu.mem.wr32(SCENE_FILL, scene_fill);
        busy.set(u32::from(input.load_pending));
        for i in 0..3u32 {
            cpu.mem.wr8(TINT + i, live[i as usize]);
            cpu.mem.wr8(TINT_MIRROR + i, 0x99);
            cpu.mem.wr8(TINT_SAVE + 2 - i, saved[i as usize]);
        }
        log.borrow_mut().clear();
        assert_eq!(
            cpu.call(FILL_FADE, &[ACTOR]),
            Stop::Returned,
            "round {round}"
        );

        let (phase, timer, effects) = fill_fade_tick(before.phase, input);
        let mut want = before;
        want.phase = phase;
        let (mut calls, mut tint, mut mirror, mut save, mut flags) =
            (Vec::new(), live, [0x99u8; 3], saved, player_flags);
        for e in &effects {
            match *e {
                FillFadeEffect::RunPanelScript(va) => calls.push((PANEL_SCRIPT, va)),
                FillFadeEffect::PostFillPrim => calls.push((POST_PRIM, 0)),
                FillFadeEffect::SpawnSubActorAndCaptureTint => {
                    calls.push((SPAWN, 0x8007_06BC));
                    save = tint;
                    tint = [0; 3];
                    mirror = [0; 3];
                    spawned += 1;
                }
                FillFadeEffect::QueueDmaAndRestoreTint => {
                    calls.push((QUEUE_DMA, 0));
                    calls.push((QUEUE_DMA, 0));
                    if scene_fill != 0 {
                        calls.push((POST_PRIM, 0));
                    }
                    calls.push((POST_PRIM, 0));
                    tint = save;
                    mirror = save;
                    queued += 1;
                }
                FillFadeEffect::SetSceneFlagBit => flags |= 0x8_0000,
                FillFadeEffect::TickTextActors => calls.push((DRAW_PUMP, 0)),
                FillFadeEffect::Exit(x) => {
                    let (mut ph, mut h) = (want.phase, want.handler);
                    x.apply(&mut want.handback, &mut want.stashed, &mut h, &mut ph);
                    want.handler = h;
                    want.phase = ph;
                    exits += 1;
                }
                FillFadeEffect::ClearSceneField3E => want.done_gate = 0,
            }
        }
        let ctx = format!("round {round} phase {} {input:?}", before.phase);
        let got: Vec<(u32, u32)> = log
            .borrow()
            .iter()
            .map(|&(va, a0)| {
                let keep = va == PANEL_SCRIPT || va == SPAWN;
                (va, if keep { a0 } else { 0 })
            })
            .collect();
        assert_eq!(got, calls, "calls: {ctx}");
        assert_eq!(read_seat(&mut cpu), want, "seat: {ctx}");
        assert_eq!(cpu.mem.rd16(ACTOR + 0x9E) as i16, timer, "timer: {ctx}");
        assert_eq!(cpu.mem.rd32(PLAYER + 0x10), flags, "player flags: {ctx}");
        for i in 0..3u32 {
            let k = i as usize;
            assert_eq!(cpu.mem.rd8(TINT + i), tint[k], "tint {i}: {ctx}");
            assert_eq!(cpu.mem.rd8(TINT_MIRROR + i), mirror[k], "mirror {i}: {ctx}");
            assert_eq!(cpu.mem.rd8(TINT_SAVE + 2 - i), save[k], "save {i}: {ctx}");
        }
        held += u32::from(before.phase == 1 && phase == 1);
    }
    assert!(
        spawned > 300 && queued > 300 && exits > 500 && held > 500,
        "{spawned} {queued} {exits} {held}"
    );
}

/// The text box `FUN_801EE90C`: the confirm / decline cursor, the party
/// restore over the three records of the save block, and the fill fade it
/// replays at phases 10..13 with the place-name latch in front.
#[test]
fn the_text_box_matches_retail() {
    differential!(prot(897, TEXT_BOX));
    let Some(disc) = disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let (mut cpu, log) = hooked(
        &disc.read_entry(897).expect("PROT 0897"),
        &[
            PANEL_SCRIPT,
            REPLACE_CUE,
            PUSH_CUE,
            POST_PRIM,
            DRAW_PUMP,
            SPAWN,
            QUEUE_DMA,
        ],
    );
    let busy = Rc::new(Cell::new(0u32));
    let b = busy.clone();
    cpu.hook(LOAD_BUSY, move |r, _| r.r[V0] = b.get());
    const SCENE_FILL: u32 = 0x8007_B62C;
    const NAME_SRC: u32 = 0x801C_6EA0;
    const NAME_LATCH: u32 = 0x8007_B44C;
    const SAVE_BLOCK: u32 = 0x8008_4140;
    let mut rng = Rng(0x5445_5854);
    let (mut restored, mut declined, mut exits, mut spawned, mut queued, mut idle) =
        (0, 0, 0, 0, 0, 0);
    for round in 0..20000 {
        let before = seat(&mut rng, 0x2F, 15);
        let pad = CursorPad {
            held: match rng.below(4) {
                0 => 0x20,
                1 => 0x40,
                _ => 0,
            } | (rng.next() & 0x0F00),
            pressed: [0, 0x1000, 0x4000, 0x20, 0x40][rng.below(5) as usize],
            action_a_mask: 0x20,
            action_b_mask: 0x40,
        };
        let fade = FillFadeInput {
            frame_delta: 1 + rng.below(4) as i16,
            timer: match rng.below(4) {
                0 => 0x10 - rng.below(5) as i16,
                1 => -(rng.below(4) as i16),
                _ => rng.below(0x14) as i16,
            },
            input_locked: rng.below(4) == 0,
            load_pending: rng.below(2) == 0,
            handler_id: before.handler,
        };
        let input = TextBoxInput {
            input_locked: fade.input_locked,
            cursor: rng.below(2) as i32,
            pad,
            dismiss_pressed: pad.pressed & (pad.action_a_mask | pad.action_b_mask) != 0,
            fade,
            handler_id: before.handler,
        };
        let live: [u8; 3] = std::array::from_fn(|_| rng.below(256) as u8);
        let saved: [u8; 3] = std::array::from_fn(|_| rng.below(256) as u8);
        let player_flags = rng.next() & !0x8_0000;
        let scene_fill = rng.below(2) * 0x8012_5000;
        let name = 0x8012_6000 + rng.below(0x100);
        // Three party records' HP / MP quartets, and the fourth the loop
        // must leave alone.
        let party: [[u16; 4]; 4] =
            std::array::from_fn(|_| std::array::from_fn(|_| rng.next() as u16));
        write_seat(&mut cpu, before);
        cpu.mem.wr8(FRAME_STEP, fade.frame_delta as u8);
        cpu.mem.wr16(ACTOR + 0x9E, fade.timer as u16);
        cpu.mem.wr32(BLOCKED, u32::from(input.input_locked));
        cpu.mem.wr32(ROW, input.cursor as u32);
        cpu.mem.wr32(0x8007_B874, pad.held);
        cpu.mem.wr32(0x8007_BB84, pad.pressed);
        cpu.mem.wr32(0x8008_46D0, pad.action_a_mask);
        cpu.mem.wr32(0x8008_46D4, pad.action_b_mask);
        cpu.mem.wr32(PLAYER + 0x10, player_flags);
        cpu.mem.wr32(SCENE_FILL, scene_fill);
        cpu.mem.wr32(NAME_SRC, name);
        cpu.mem.wr32(NAME_LATCH, 0x5A5A_5A5A);
        for (i, q) in party.iter().enumerate() {
            for (k, v) in q.iter().enumerate() {
                cpu.mem
                    .wr16(SAVE_BLOCK + 0x6CC + 0x414 * i as u32 + 2 * k as u32, *v);
            }
        }
        busy.set(u32::from(fade.load_pending));
        for i in 0..3u32 {
            cpu.mem.wr8(TINT + i, live[i as usize]);
            cpu.mem.wr8(TINT_MIRROR + i, 0x99);
            cpu.mem.wr8(TINT_SAVE + 2 - i, saved[i as usize]);
        }
        log.borrow_mut().clear();
        assert_eq!(
            cpu.call(TEXT_BOX, &[ACTOR]),
            Stop::Returned,
            "round {round}"
        );

        let (phase, cursor, timer, effects) = text_box_tick(before.phase, input);
        let mut want = before;
        want.phase = phase;
        let (mut calls, mut tint, mut mirror, mut save, mut flags) =
            (Vec::new(), live, [0x99u8; 3], saved, player_flags);
        let (mut want_party, mut latch) = (party, 0x5A5A_5A5Au32);
        for e in &effects {
            match *e {
                TextBoxEffect::PlaySfx(0) => calls.push((REPLACE_CUE, 0)),
                TextBoxEffect::PlaySfx(id) => calls.push((PUSH_CUE, id)),
                TextBoxEffect::RestoreParty => {
                    for q in want_party.iter_mut().take(3) {
                        q[1] = q[0];
                        q[3] = q[2];
                    }
                    restored += 1;
                }
                TextBoxEffect::RunPanelScript(va) => calls.push((PANEL_SCRIPT, va)),
                TextBoxEffect::Exit(x) => {
                    let (mut ph, mut h) = (want.phase, want.handler);
                    x.apply(&mut want.handback, &mut want.stashed, &mut h, &mut ph);
                    want.handler = h;
                    want.phase = ph;
                    exits += 1;
                }
                TextBoxEffect::Fade(f) => match f {
                    FillFadeEffect::PostFillPrim => {
                        calls.push((POST_PRIM, 0));
                        latch = name;
                    }
                    FillFadeEffect::SpawnSubActorAndCaptureTint => {
                        calls.push((SPAWN, 0x8007_06BC));
                        save = tint;
                        tint = [0; 3];
                        mirror = [0; 3];
                        spawned += 1;
                    }
                    FillFadeEffect::QueueDmaAndRestoreTint => {
                        calls.push((QUEUE_DMA, 0));
                        calls.push((QUEUE_DMA, 0));
                        if scene_fill != 0 {
                            calls.push((POST_PRIM, 0));
                        }
                        calls.push((POST_PRIM, 0));
                        tint = save;
                        mirror = save;
                        queued += 1;
                    }
                    FillFadeEffect::SetSceneFlagBit => flags |= 0x8_0000,
                    FillFadeEffect::TickTextActors => calls.push((DRAW_PUMP, 0)),
                    other => panic!("the text box replays no {other:?}"),
                },
                TextBoxEffect::ClearSceneField3E => want.done_gate = 0,
                TextBoxEffect::TickTextActors => calls.push((DRAW_PUMP, 0)),
            }
        }
        let ctx = format!("round {round} phase {} {input:?}", before.phase);
        // The cursor's own cues are the list cursor's, compared on their own.
        let got: Vec<(u32, u32)> = log
            .borrow()
            .iter()
            .filter(|c| !(c.0 == PUSH_CUE && c.1 != 0x25))
            .map(|&(va, a0)| {
                let keep = matches!(va, PANEL_SCRIPT | SPAWN | REPLACE_CUE | PUSH_CUE);
                (va, if keep { a0 } else { 0 })
            })
            .collect();
        assert_eq!(got, calls, "calls: {ctx}");
        assert_eq!(read_seat(&mut cpu), want, "seat: {ctx}");
        assert_eq!(cpu.mem.rd32(ROW) as i32, cursor, "cursor: {ctx}");
        assert_eq!(cpu.mem.rd16(ACTOR + 0x9E) as i16, timer, "timer: {ctx}");
        assert_eq!(cpu.mem.rd32(PLAYER + 0x10), flags, "player flags: {ctx}");
        assert_eq!(cpu.mem.rd32(NAME_LATCH), latch, "name latch: {ctx}");
        for (i, q) in want_party.iter().enumerate() {
            for (k, v) in q.iter().enumerate() {
                let at = SAVE_BLOCK + 0x6CC + 0x414 * i as u32 + 2 * k as u32;
                assert_eq!(cpu.mem.rd16(at), *v, "party {i} word {k}: {ctx}");
            }
        }
        for i in 0..3u32 {
            let k = i as usize;
            assert_eq!(cpu.mem.rd8(TINT + i), tint[k], "tint {i}: {ctx}");
            assert_eq!(cpu.mem.rd8(TINT_MIRROR + i), mirror[k], "mirror {i}: {ctx}");
            assert_eq!(cpu.mem.rd8(TINT_SAVE + 2 - i), save[k], "save {i}: {ctx}");
        }
        declined += u32::from(before.phase == 1 && phase == 3);
        idle += u32::from((4..10).contains(&before.phase) && got == [(DRAW_PUMP, 0)]);
    }
    assert!(
        restored > 50
            && declined > 50
            && exits > 300
            && spawned > 300
            && queued > 300
            && idle > 3000,
        "{restored} {declined} {exits} {spawned} {queued} {idle}"
    );
}
