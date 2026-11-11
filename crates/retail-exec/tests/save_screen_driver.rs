//! The in-field save / load screen driver `FUN_80024190` (`SCUS_942.54`)
//! against `legaia_engine_field::field_save_screen_actor::FieldSaveScreenActor::tick`.
//!
//! Every callee is hooked and logged; the cover fill's `POLY_F4` is read off
//! the ordering-table bucket `AddPrim` linked it into, with the draw-mode
//! packet `SetDrawMode` was handed after it.

use crate::common::{Rng, scus_cpu};
use legaia_engine_field::field_save_screen_actor::{
    COVER_FILL_PRIM_BYTES, COVER_FILL_TAG, DRAW_MODE_PRIM_BYTES, FieldSaveScreenActor,
    SaveScreenEffect, SaveScreenInput,
};
use psx_retail_exec::{A0, A1, A2, A3, SP, Stop, V0};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

const DRIVER: u32 = 0x8002_4190;
const QUEUE: u32 = 0x8003_DE7C;
const LOAD_SLOT: u32 = 0x8003_EBE4;
const LOAD_SELECT: u32 = 0x801D_D35C;
const SAVE_WRITE: u32 = 0x801D_C6B4;
const SLOT_B: u32 = 0x8002_5BA0;
const DRAW_MODE: u32 = 0x8005_9010;

const CARD_ACTOR: u32 = 0x8007_B8E0;
const MODE: u32 = 0x8007_B83C;
const TRANSITION_BYTE: u32 = 0x8007_B648;
const SCREEN_W: u32 = 0x1F80_038C;
const SCREEN_H: u32 = 0x1F80_038E;
const OT_SIZE: u32 = 0x1F80_03A6;
const CURSOR: u32 = 0x1F80_03A0;
const OT_BASE: u32 = 0x1F80_03F4;

const ACTOR: u32 = 0x8012_0000;
const PACKETS: u32 = 0x8013_0000;
const OT: u32 = 0x8014_0000;
const OT_END: u32 = 0x00FF_FFFF;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Call {
    LoadSlot(u32, u32),
    LoadSelect(u32, u32),
    SaveWrite,
    SlotB,
    DrawMode(u32, [u32; 5]),
}

#[test]
fn the_save_screen_driver_matches_retail() {
    differential!(scus(DRIVER));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let log = Rc::new(RefCell::new(Vec::<Call>::new()));
    let busy = Rc::new(Cell::new(0u32));
    let done = Rc::new(Cell::new(0u32));
    let b = busy.clone();
    cpu.hook(QUEUE, move |r, _| {
        assert_eq!(r.r[A0], 1);
        r.r[V0] = b.get();
    });
    let l = log.clone();
    cpu.hook(LOAD_SLOT, move |r, _| {
        l.borrow_mut().push(Call::LoadSlot(r.r[A0], r.r[A1]))
    });
    let (l, d) = (log.clone(), done.clone());
    cpu.hook(LOAD_SELECT, move |r, _| {
        l.borrow_mut().push(Call::LoadSelect(r.r[A0], r.r[A1]));
        r.r[V0] = d.get();
    });
    let (l, d) = (log.clone(), done.clone());
    cpu.hook(SAVE_WRITE, move |r, _| {
        l.borrow_mut().push(Call::SaveWrite);
        r.r[V0] = d.get();
    });
    let l = log.clone();
    cpu.hook(SLOT_B, move |_, _| l.borrow_mut().push(Call::SlotB));
    let l = log.clone();
    cpu.hook(DRAW_MODE, move |r, m| {
        let fifth = m.rd32(r.r[SP] + 0x10);
        l.borrow_mut().push(Call::DrawMode(
            r.r[A0],
            [r.r[A1], r.r[A2], r.r[A3], fifth, 0],
        ));
        // Fill the packet the way libgpu does, with a marker the test reads.
        m.wr32(r.r[A0], 0x0200_0000);
        m.wr32(r.r[A0] + 4, 0xE100_0000 | r.r[A3]);
        m.wr32(r.r[A0] + 8, 0xE200_0000);
    });

    let mut rng = Rng(0x5341_5645);
    let mut reached = [0u32; 12];
    let mut covers = 0;
    for round in 0..20000 {
        let state = match rng.below(14) {
            11 => 11 + rng.below(4) as u16,
            12 => rng.next() as u16 | 0x8000,
            n => n as u16,
        };
        let input = SaveScreenInput {
            queue_busy: rng.below(2) == 0,
            is_save: rng.below(2) == 0,
            ui_done: rng.below(2) == 0,
            screen_width: [320, 256, 368, 512, 640][rng.below(5) as usize],
            screen_height: [240, 224, 256, 480][rng.below(4) as usize],
            ot_size: match rng.below(4) {
                0 => 0,
                1 => 1,
                _ => 1 + rng.below(0x400) as u16,
            },
        };
        let save_word = if input.is_save {
            1 + rng.below(0x7FFF) as u16
        } else {
            0
        };
        let flags = rng.next() & !8;
        cpu.mem.wr16(ACTOR + 0x1A, state);
        cpu.mem.wr16(ACTOR + 0x5C, save_word);
        cpu.mem.wr32(ACTOR + 0x10, flags);
        cpu.mem.wr32(CARD_ACTOR, 0x5A5A_5A5A);
        cpu.mem.wr16(MODE, 0xA5A5);
        cpu.mem.wr8(TRANSITION_BYTE, 0xA5);
        cpu.mem.wr16(SCREEN_W, input.screen_width);
        cpu.mem.wr16(SCREEN_H, input.screen_height);
        cpu.mem.wr16(OT_SIZE, input.ot_size);
        cpu.mem.wr32(CURSOR, PACKETS);
        // The ordering table, with a guard word below bucket 0 for the
        // degenerate empty table's index -1.
        cpu.mem.wr32(OT_BASE, OT + 4);
        for i in 0..4 {
            cpu.mem.wr32(OT + 4 * i, OT_END);
        }
        busy.set(u32::from(input.queue_busy));
        done.set(if input.ui_done { 1 + rng.below(3) } else { 0 });
        log.borrow_mut().clear();
        assert_eq!(cpu.call(DRIVER, &[ACTOR]), Stop::Returned, "round {round}");

        let mut actor = FieldSaveScreenActor { state };
        let effects = actor.tick(input);
        let ctx = format!("round {round} state {state} {input:?}");
        let (mut calls, mut card, mut mode, mut byte, mut want_flags) =
            (Vec::new(), 0x5A5A_5A5A, 0xA5A5u16, 0xA5u8, flags);
        let mut cover = None;
        for e in &effects {
            match *e {
                SaveScreenEffect::RegisterCardActor => card = ACTOR,
                SaveScreenEffect::EnterGameMode(m) => mode = m,
                SaveScreenEffect::LoadOverlaySlot(s) => calls.push(Call::LoadSlot(s as u32, 0)),
                SaveScreenEffect::RunLoadSelect => calls.push(Call::LoadSelect(0, 0)),
                SaveScreenEffect::RunSaveWrite => calls.push(Call::SaveWrite),
                SaveScreenEffect::RestoreSlotBPair => calls.push(Call::SlotB),
                SaveScreenEffect::EmitCoverFill(c) => {
                    calls.push(Call::DrawMode(
                        PACKETS + COVER_FILL_PRIM_BYTES as u32,
                        [0, 0, c.draw_mode_param as u32, 0, 0],
                    ));
                    cover = Some(c);
                }
                SaveScreenEffect::ClearTransitionScreenByte => byte = 0,
                SaveScreenEffect::Retire => want_flags |= 8,
            }
        }
        assert_eq!(*log.borrow(), calls, "calls: {ctx}");
        assert_eq!(cpu.mem.rd16(ACTOR + 0x1A), actor.state, "state: {ctx}");
        assert_eq!(cpu.mem.rd32(CARD_ACTOR), card, "card actor: {ctx}");
        assert_eq!(cpu.mem.rd16(MODE), mode, "mode: {ctx}");
        assert_eq!(cpu.mem.rd8(TRANSITION_BYTE), byte, "byte: {ctx}");
        assert_eq!(cpu.mem.rd32(ACTOR + 0x10), want_flags, "flags: {ctx}");
        let used = cpu.mem.rd32(CURSOR) - PACKETS;
        match cover {
            None => {
                assert_eq!(used, 0, "{ctx}");
                for i in 0..4 {
                    assert_eq!(cpu.mem.rd32(OT + 4 * i), OT_END, "{ctx}");
                }
            }
            Some(c) => {
                covers += 1;
                assert_eq!(
                    used as usize,
                    COVER_FILL_PRIM_BYTES + DRAW_MODE_PRIM_BYTES,
                    "{ctx}"
                );
                let bucket = OT + 4 + (4 * c.ot_index) as u32;
                // Newest first: the draw-mode packet, then the quad.
                let dm = PACKETS + COVER_FILL_PRIM_BYTES as u32;
                assert_eq!(cpu.mem.rd32(bucket) & 0xFF_FFFF, dm & 0xFF_FFFF, "{ctx}");
                assert_eq!(cpu.mem.rd32(dm) & 0xFF_FFFF, PACKETS & 0xFF_FFFF, "{ctx}");
                assert_eq!(cpu.mem.rd32(PACKETS) & 0xFF_FFFF, OT_END, "{ctx}");
                assert_eq!(cpu.mem.rd32(PACKETS) & 0xFF00_0000, COVER_FILL_TAG, "{ctx}");
                assert_eq!(cpu.mem.rd32(PACKETS + 4), c.code, "{ctx}");
                let corners: Vec<(i16, i16)> = (0..4)
                    .map(|v| {
                        let at = PACKETS + 8 + 4 * v;
                        (cpu.mem.rd16(at) as i16, cpu.mem.rd16(at + 2) as i16)
                    })
                    .collect();
                assert_eq!(corners, c.corners, "corners: {ctx}");
                for i in 0..4u32 {
                    if OT + 4 * i != bucket {
                        assert_eq!(cpu.mem.rd32(OT + 4 * i), OT_END, "bucket {i}: {ctx}");
                    }
                }
            }
        }
        if let Some(n) = reached.get_mut(usize::from(state.min(11))) {
            *n += u32::from(actor.state != state || effects.contains(&SaveScreenEffect::Retire));
        }
    }
    assert!(
        reached[..11].iter().all(|&n| n > 200) && covers > 2000,
        "{reached:?} {covers}"
    );
}
