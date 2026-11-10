//! The field overlay's code-lock actor `FUN_801EED58` (PROT 0897, dispatcher
//! slot `0x21`) against `legaia_engine_field::field_submode_code_lock::code_lock_slot`.
//!
//! Every call is hooked and logged; the entered symbols are the five bytes at
//! the dispatcher context's `+0x54`, the cursor is `_DAT_8007BB88`, and the
//! code is the operand bytes `_DAT_8007B450 + 1..=5`.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_field::code_lock_actor::CodeLockActor;
use legaia_engine_field::field_submode_code_lock::{code_from_operand, code_lock_slot};
use legaia_engine_field::field_submode_flag_window::FlagWrite;
use legaia_engine_field::submode_hub_actors::{HubAction, HubActor, HubGrid};
use psx_retail_exec::{A0, Stop};
use std::cell::RefCell;
use std::rc::Rc;

const OVERLAY_BASE: u32 = 0x801C_E818;
const CODE_LOCK: u32 = 0x801E_ED58;

const INSTALL_PANEL: u32 = 0x801E_9B3C;
const PUSH_CUE: u32 = 0x8003_5B50;
const REPLACE_CUE: u32 = 0x8003_5BD0;
const FLAG_SET: u32 = 0x8003_CE08;
const FLAG_CLEAR: u32 = 0x8003_CE34;
const DRAW_PUMP: u32 = 0x8003_1D00;

const FRAME_STEP: u32 = 0x1F80_0393;
const BLOCKED: u32 = 0x8007_BB80;
const ROW: u32 = 0x8007_BB88;
const PAD_EDGE: u32 = 0x8007_B874;
const OPERAND_PTR: u32 = 0x8007_B450;
const GRID_PTR: u32 = 0x801C_6EA4;

const ACTOR: u32 = 0x8012_0000;
const GRID: u32 = 0x8012_1000;
const OPERAND: u32 = 0x8012_2000;

#[test]
fn the_code_lock_matches_retail() {
    differential!(prot(897, CODE_LOCK));
    let Some(disc) = disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut cpu = scus_cpu().expect("disc present");
    cpu.mem
        .load(OVERLAY_BASE, &disc.read_entry(897).expect("PROT 0897"));
    let log = Rc::new(RefCell::new(Vec::<(u32, u32)>::new()));
    for va in [
        INSTALL_PANEL,
        PUSH_CUE,
        REPLACE_CUE,
        FLAG_SET,
        FLAG_CLEAR,
        DRAW_PUMP,
    ] {
        let l = log.clone();
        cpu.hook(va, move |r, _| l.borrow_mut().push((va, r.r[A0])));
    }
    cpu.mem.wr32(GRID_PTR, GRID);
    cpu.mem.wr32(OPERAND_PTR, OPERAND);
    let mut rng = Rng(0x434F_4445);
    let (mut opened, mut stored, mut filled, mut matched, mut missed, mut released, mut multi) =
        (0, 0, 0, 0, 0, 0, 0);
    for round in 0..40000 {
        let phase = rng.below(7) as i16 - 1;
        let handler = 0x21 + rng.below(3) as u16;
        let dwell = match rng.below(3) {
            0 => rng.below(0x18) as i16,
            1 => -(rng.below(6) as i16),
            _ => 0x7FFE + rng.below(2) as i16,
        };
        let index = rng.below(5);
        let entered: [u8; 5] = std::array::from_fn(|_| rng.below(4) as u8);
        // A code that matches about a third of the time, or misses in one
        // chosen place - each of the five compares gets its own miss.
        let mut code = entered;
        if rng.below(3) != 0 {
            let at = rng.below(5) as usize;
            code[at] = (code[at] + 1 + rng.below(3) as u8) & 3;
        }
        let pressed = match rng.below(4) {
            0 => 0,
            1 => 0x10 << rng.below(4),
            2 => rng.next() & 0xFFFF,
            _ => rng.next() & 0xFF0F,
        };
        let locked = rng.below(5) == 0;
        let delta = 1 + rng.below(4);
        let (handback, stashed) = (rng.below(3) as i16, rng.below(0x34) as u16);

        cpu.mem.wr16(ACTOR + 0x50, handler);
        cpu.mem.wr16(ACTOR + 0x54, phase as u16);
        cpu.mem.wr16(ACTOR + 0x9C, dwell as u16);
        cpu.mem.wr16(GRID + 0x2E, handback as u16);
        cpu.mem.wr16(GRID + 0x40, stashed);
        for (i, b) in entered.iter().enumerate() {
            cpu.mem.wr8(GRID + 0x54 + i as u32, *b);
        }
        // The bytes either side of the entry row, which no arm may touch.
        cpu.mem.wr8(GRID + 0x53, 0xA5);
        cpu.mem.wr8(GRID + 0x59, 0xA5);
        cpu.mem.wr8(OPERAND, 2);
        for (i, b) in code.iter().enumerate() {
            cpu.mem.wr8(OPERAND + 1 + i as u32, *b);
        }
        cpu.mem.wr32(ROW, index);
        cpu.mem.wr32(BLOCKED, u32::from(locked));
        cpu.mem.wr32(PAD_EDGE, pressed);
        cpu.mem.wr8(FRAME_STEP, delta as u8);
        log.borrow_mut().clear();
        assert_eq!(
            cpu.call(CODE_LOCK, &[ACTOR]),
            Stop::Returned,
            "round {round}"
        );

        let mut actor = HubActor {
            state: handler,
            sub: phase,
            ..HubActor::default()
        };
        let mut grid = HubGrid {
            handback,
            stashed_state: stashed,
            columns: entered.to_vec(),
            ..HubGrid::default()
        };
        let mut state = CodeLockActor {
            phase: 0,
            dwell,
            index: index as usize,
            entered,
        };
        let mut operand = vec![2u8];
        operand.extend_from_slice(&code);
        let mut writes = Vec::new();
        let frame = code_lock_slot(
            &mut actor,
            &mut grid,
            &mut state,
            &code_from_operand(&operand),
            pressed,
            locked,
            delta as i32,
            &mut writes,
        );
        let mut calls = Vec::new();
        let (pump, rest) = frame.actions.split_last().expect("the pump ends a frame");
        assert_eq!(*pump, HubAction::DrawPump);
        for a in rest {
            calls.push(match *a {
                HubAction::InstallPanel(va) => (INSTALL_PANEL, va),
                HubAction::EntryCue(c) => (PUSH_CUE, u32::from(c)),
                HubAction::ConfirmCue(c) => (REPLACE_CUE, u32::from(c)),
                ref other => panic!("the code lock asks for no {other:?}"),
            });
        }
        for w in &writes {
            calls.push(match *w {
                FlagWrite::Set(f) => (FLAG_SET, f as u32),
                FlagWrite::Clear(f) => (FLAG_CLEAR, f as u32),
            });
        }
        calls.push((DRAW_PUMP, 0));
        let got: Vec<(u32, u32)> = log
            .borrow()
            .iter()
            .map(|&(va, a0)| (va, if va == DRAW_PUMP { 0 } else { a0 }))
            .collect();
        let ctx = format!(
            "round {round} phase {phase} dwell {dwell} index {index} pressed {pressed:#x} \
             locked {locked} delta {delta} entered {entered:?} code {code:?}"
        );
        assert_eq!(got, calls, "calls: {ctx}");
        assert_eq!(cpu.mem.rd16(ACTOR + 0x50), actor.state, "handler: {ctx}");
        assert_eq!(cpu.mem.rd16(ACTOR + 0x54) as i16, actor.sub, "phase: {ctx}");
        assert_eq!(cpu.mem.rd16(ACTOR + 0x9C) as i16, state.dwell, "dwell: {ctx}");
        assert_eq!(cpu.mem.rd16(GRID + 0x2E) as i16, grid.handback, "{ctx}");
        assert_eq!(cpu.mem.rd16(GRID + 0x40), grid.stashed_state, "{ctx}");
        assert_eq!(cpu.mem.rd32(ROW) as usize, state.index, "cursor: {ctx}");
        let row: Vec<u8> = (0..5).map(|i| cpu.mem.rd8(GRID + 0x54 + i)).collect();
        assert_eq!(row, grid.columns, "entered: {ctx}");
        assert_eq!(cpu.mem.rd8(GRID + 0x53), 0xA5, "{ctx}");
        assert_eq!(cpu.mem.rd8(GRID + 0x59), 0xA5, "{ctx}");

        opened += u32::from(phase == 0);
        let took = phase == 1 && got.contains(&(PUSH_CUE, 0x36));
        stored += u32::from(took);
        multi += u32::from(took && (pressed & 0xF0).count_ones() > 1);
        filled += u32::from(took && actor.sub == 2);
        matched += u32::from(got.contains(&(FLAG_SET, 9)));
        missed += u32::from(got.contains(&(FLAG_CLEAR, 9)));
        released += u32::from(phase == 4 && actor.sub == 0);
    }
    assert!(
        opened > 3000
            && stored > 2000
            && multi > 500
            && filled > 300
            && matched > 1000
            && missed > 2000
            && released > 500,
        "{opened} {stored} {multi} {filled} {matched} {missed} {released}"
    );
}
