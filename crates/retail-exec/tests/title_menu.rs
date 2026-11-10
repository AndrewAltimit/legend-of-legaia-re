//! The title overlay tick `FUN_801DD35C` (PROT 0899)'s attract-idle arm
//! (sub-mode `0x10`, `0x801DDB74..0x801DDCF4`) frame by frame against
//! `legaia_engine_title::title_overlay::TitleMenuState::step`: random pad
//! edges and holds over random countdowns, every call outside the routine
//! stubbed. Compared each frame: the row counter `_DAT_8007B820`, the
//! countdown `0x801EF16C`, the cue `0x8007B6D8`, and the arm's exits - the
//! confirmed row's sub-mode (`0x16` for row 0, `0x18` for row 1) and the
//! attract hand-off (game mode `0x1A`).

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_title::title_overlay::{TitleMenuEvent, TitleMenuState};
use psx_retail_exec::Stop;

const OVERLAY_BASE: u32 = 0x801C_E818;
const TICK: u32 = 0x801D_D35C;
const TICK_END: u32 = TICK + 12104;
const SUB_MODE: u32 = 0x801F_0204;
const ROW_COUNTER: u32 = 0x8007_B820;
const COUNTDOWN: u32 = 0x801E_F16C;
const CURSOR_PAD: u32 = 0x801E_F114;
const PRESS: u32 = 0x8007_B874;
const HELD: u32 = 0x8007_B850;
const CUE: u32 = 0x8007_B6D8;
const GAME_MODE: u32 = 0x8007_B83C;
const FRAME_STEP: u32 = 0x1F80_0393;
/// The tick's once-only setup latch: set, the per-frame body runs.
const SET_UP: u32 = 0x801F_0230;
/// The word OR-ed into the pad edge the cursor reads.
const PAD_EXTRA: u32 = 0x8007_B93C;
const POOL_PTR: u32 = 0x1F80_03A0;
const OT_PTR: u32 = 0x1F80_03F4;

#[test]
fn the_title_menu_steps_as_retail_does() {
    differential!(prot(899, TICK));
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        return;
    };
    cpu.mem.load(
        OVERLAY_BASE,
        &d.read_entry_footprint(899).expect("PROT 0899"),
    );
    cpu.stub_outside = Some((TICK, TICK_END));
    // The sub-mode -> case identity `FUN_801E38D0` sits past the routine.
    cpu.hook(0x801E_38D0, |r, _| {
        r.r[psx_retail_exec::V0] = r.r[psx_retail_exec::A0]
    });
    cpu.mem.wr32(SET_UP, 1);
    cpu.mem.wr32(PAD_EXTRA, 0);
    let mut rng = Rng(0x5449_544C);
    let (mut confirmed, mut fired, mut moved) = (0, 0, 0);
    for run in 0..400 {
        let mut port = TitleMenuState::new();
        port.countdown = [0x5DC, 0x20, 0x12, 0x10, 3][rng.below(5) as usize];
        port.row_counter = rng.below(2) as i32;
        cpu.mem.wr32(COUNTDOWN, port.countdown as u32);
        cpu.mem.wr32(ROW_COUNTER, port.row_counter as u32);
        for frame in 0..30 {
            let edge: u16 = match rng.below(8) {
                0..=3 => 0,
                4 => 0x4000,
                5 => 0x1000,
                6 => [0x0800, 0x0004, 0x0040][rng.below(3) as usize],
                _ => rng.next() as u16,
            };
            let held: u16 = if rng.below(6) == 0 {
                rng.next() as u16
            } else {
                0
            };
            let step = 1 + rng.below(3) as u8;
            cpu.mem.wr32(SUB_MODE, 0x10);
            cpu.mem.wr32(CURSOR_PAD, u32::from(edge));
            cpu.mem.wr32(PRESS, u32::from(edge));
            cpu.mem.wr32(HELD, u32::from(held));
            cpu.mem.wr8(FRAME_STEP, step);
            cpu.mem.wr16(CUE, 0xAAAA);
            cpu.mem.wr16(GAME_MODE, 0);
            cpu.mem.wr32(POOL_PTR, 0x8014_0000);
            cpu.mem.wr32(OT_PTR, 0x8016_0000);
            assert_eq!(cpu.call(TICK, &[0, 0]), Stop::Returned);
            let events = port.step(edge, held, step, 2);
            let ctx = format!("run {run} frame {frame} edge {edge:#x} held {held:#x}");
            assert_eq!(
                cpu.mem.rd32(ROW_COUNTER) as i32,
                port.row_counter,
                "{ctx}: the row"
            );
            let cue = cpu.mem.rd16(CUE);
            assert_eq!((cue != 0xAAAA).then_some(cue), port.sfx, "{ctx}: the cue");
            let row_pick = events.iter().find_map(|e| match e {
                TitleMenuEvent::Confirmed { row } => Some(*row),
                _ => None,
            });
            let fires = events.contains(&TitleMenuEvent::AttractFired);
            let sub = cpu.mem.rd32(SUB_MODE);
            match row_pick {
                Some(0) => assert_eq!(sub, 0x16, "{ctx}"),
                Some(_) => assert_eq!(sub, 0x18, "{ctx}"),
                None if !fires => assert_eq!(sub, 0x10, "{ctx}"),
                None => {}
            }
            if row_pick == Some(0) {
                confirmed += 1;
                break;
            }
            assert_eq!(
                cpu.mem.rd32(COUNTDOWN) as i32,
                port.countdown,
                "{ctx}: the countdown"
            );
            assert_eq!(cpu.mem.rd16(GAME_MODE) == 0x1A, fires, "{ctx}: the attract");
            moved += usize::from(
                events
                    .iter()
                    .any(|e| matches!(e, TitleMenuEvent::CursorMoved { .. })),
            );
            if row_pick.is_some() {
                confirmed += 1;
                break;
            }
            if fires {
                fired += 1;
                break;
            }
        }
    }
    assert!(
        confirmed > 50 && fired > 50 && moved > 200,
        "{confirmed} {fired} {moved}"
    );
}
