//! The scene-transition streaming actor `FUN_80021934` against
//! `SceneTransitionActor::tick`, tick by tick over whole transitions and from
//! generated states: the unconditional countdown, the five states, the index
//! arm and the path arm with the three scene-name buffers rotating beneath
//! it.
//!
//! The stream request `FUN_8001EEF0` and the CD-queue poll `FUN_8003DE7C` are
//! hooked, and the BIOS `strcpy` / `strcat` stubs the path build goes through
//! are answered here.

use crate::common::{Rng, scus_cpu};
use legaia_engine_field::scene_transition_actor::{
    SceneTransitionActor, SceneTransitionEffect as E, SceneTransitionInput, staged_byte_size,
};
use psx_retail_exec::{A0, A1, A2, A3, GP, Stop, V0};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

const TICK: u32 = 0x8002_1934;
const STREAM: u32 = 0x8001_EEF0;
const CD_BUSY: u32 = 0x8003_DE7C;
const STRCPY: u32 = 0x8005_6758;
const STRCAT: u32 = 0x8005_6728;
const FRAME_STEP: u32 = 0x1F80_0393;
const SCRATCH: u32 = 0x8007_BA24;
const SCREEN_BYTE: u32 = 0x8007_B648;
const START_GATE: u32 = 0x8007_BC20;
const INDEX_MODE: u32 = 0x8007_B8C2;
const STAGED_INDEX: u32 = 0x8007_B768;
const PENDING_INDEX: u32 = 0x8007_B9A0;
const BUFFER: u32 = 0x8007_B85C;
const STAGED: u32 = 0x8007_B9EC;
const GAME_MODE: u32 = 0x8007_B83C;
const NAME_PREV: u32 = 0x8008_4558;
const NAME_ACTIVE: u32 = 0x8008_4548;
const NAME_PENDING: u32 = 0x8009_15C8;
const ACTOR: u32 = 0x8012_0000;

fn cstr(mem: &mut psx_retail_exec::Memory, at: u32) -> String {
    (0..64)
        .map(|i| mem.rd8(at + i))
        .take_while(|b| *b != 0)
        .map(char::from)
        .collect()
}

#[test]
fn the_transition_actor_matches_retail() {
    differential!(scus(TICK));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let gp = cpu.regs.r[GP];
    let (countdown_at, size_at) = (gp + 0x710, gp + 0x73C);
    // (path or scratch pointer's string, chunk, buffer, a3)
    type Call = (String, i16, u32, u32);
    let streams: Rc<RefCell<Vec<Call>>> = Rc::new(RefCell::new(Vec::new()));
    let sectors = Rc::new(Cell::new(0u32));
    let (s, n) = (streams.clone(), sectors.clone());
    cpu.hook(STREAM, move |r, m| {
        let path = cstr(m, r.r[A0]);
        s.borrow_mut()
            .push((path, r.r[A1] as i16, r.r[A2], r.r[A3]));
        r.r[V0] = n.get();
    });
    let busy = Rc::new(Cell::new(0u32));
    let b = busy.clone();
    cpu.hook(CD_BUSY, move |r, _| {
        assert_eq!(r.r[A0], 1, "the CD-queue poll's argument");
        r.r[V0] = b.get();
    });

    for (va, append) in [(STRCPY, false), (STRCAT, true)] {
        cpu.hook(va, move |r, m| {
            let (dst, src) = (r.r[A0], r.r[A1]);
            let text = cstr(m, src);
            let at = if append {
                dst + cstr(m, dst).len() as u32
            } else {
                dst
            };
            m.load(at, text.as_bytes());
            m.wr8(at + text.len() as u32, 0);
            r.r[V0] = dst;
        });
    }

    let names = ["town01", "map01", "kor3", "a", "rikuroa"];
    let mut rng = Rng(0x5343_4E54);
    let (mut handed, mut indexed, mut held, mut gated, mut stale) = (0, 0, 0, 0, 0);
    for run in 0..6000 {
        // A third of the runs play a whole transition from state 0; the rest
        // start from a generated state.
        let whole = run % 3 == 0;
        let mut actor = SceneTransitionActor {
            state: if whole { 0 } else { rng.below(7) as u16 },
            countdown: match rng.below(4) {
                0 => -(rng.below(4) as i32),
                1 => rng.below(3) as i32,
                _ => rng.below(0x50) as i32,
            },
        };
        let index_mode = rng.below(4) == 0;
        let name = names[rng.below(names.len() as u32) as usize];
        let (prev, pending) = ("prev01", "next02");
        let buffer = 0x8010_0000 + rng.below(0x100) * 4;
        let mut staged_index = rng.below(0x400) as i16;
        cpu.mem.wr16(ACTOR + 0x1A, actor.state);
        cpu.mem.wr32(countdown_at, actor.countdown as u32);
        cpu.mem.wr16(INDEX_MODE, u16::from(index_mode));
        cpu.mem.wr16(STAGED_INDEX, staged_index as u16);
        cpu.mem.wr32(BUFFER, buffer);
        for (at, text) in [
            (NAME_PREV, prev),
            (NAME_ACTIVE, name),
            (NAME_PENDING, pending),
        ] {
            cpu.mem.load(at, text.as_bytes());
            cpu.mem.wr8(at + text.len() as u32, 0);
        }
        let mut active = name.to_string();
        let mut previous = prev.to_string();
        for tick in 0..if whole { 120 } else { 1 } {
            let input = SceneTransitionInput {
                dt: 1 + rng.below(3) as u8,
                start_gate: i32::from(rng.below(5) == 0),
                index_mode,
                stream_busy: rng.below(3) == 0,
                staged_index,
                pending_index: rng.below(0x400) as u16,
                // Retail builds the path after the rotation, from the name
                // the pending buffer handed over.
                scene_name: pending,
            };
            cpu.mem.wr8(FRAME_STEP, input.dt);
            cpu.mem.wr32(START_GATE, input.start_gate as u32);
            cpu.mem.wr16(PENDING_INDEX, input.pending_index);
            cpu.mem.wr32(SCRATCH, 0xEEEE_EEEE);
            cpu.mem.wr8(SCREEN_BYTE, 0x11);
            cpu.mem.wr32(STAGED, 0);
            cpu.mem.wr16(GAME_MODE, 3);
            cpu.mem.wr32(size_at, 0x7777_7777);
            busy.set(u32::from(input.stream_busy));
            sectors.set(rng.below(0x200));
            streams.borrow_mut().clear();
            let before = actor;
            assert_eq!(cpu.call(TICK, &[ACTOR]), Stop::Returned, "run {run}");
            let effects = actor.tick(input);
            let ctx = format!("run {run} tick {tick} {before:?} {input:?}");

            let (mut screen, mut size, mut staged, mut mode) = (0x11u8, 0x7777_7777u32, 0, 3);
            let mut want: Vec<Call> = Vec::new();
            assert_eq!(effects[0], E::ClearTransitionScratch, "{ctx}");
            for e in &effects[1..] {
                match e {
                    E::ClearTransitionScratch => panic!("cleared twice: {ctx}"),
                    E::SetTransitionScreenByte(v) => screen = *v,
                    E::SeedCountdown(v) => assert_eq!(actor.countdown, *v, "{ctx}"),
                    E::StreamChunkByIndex { chunk } => {
                        // The index arm hands the stream its stack scratch,
                        // which it never wrote: the string is not compared.
                        want.push((String::new(), *chunk, buffer, 0));
                        size = staged_byte_size(sectors.get() as i32) as u32;
                        indexed += 1;
                    }
                    E::RotateSceneNameBuffers => {
                        previous = active.clone();
                        active = pending.to_string();
                    }
                    E::SetStagedIndex(v) => staged_index = *v as i16,
                    E::StreamBundleByPath { path, chunk } => {
                        want.push((path.clone(), *chunk, buffer, 0));
                    }
                    E::MarkBundleStaged => staged = 1,
                    E::EnterGameMode(m) => {
                        mode = *m;
                        handed += 1;
                    }
                }
            }
            let mut got = streams.borrow().clone();
            if matches!(effects.get(1), Some(E::StreamChunkByIndex { .. })) {
                got[0].0.clear();
            }
            assert_eq!(got, want, "streams: {ctx}");
            assert_eq!(cpu.mem.rd16(ACTOR + 0x1A), actor.state, "state: {ctx}");
            assert_eq!(
                cpu.mem.rd32(countdown_at) as i32,
                actor.countdown,
                "countdown: {ctx}"
            );
            assert_eq!(cpu.mem.rd32(SCRATCH), 0, "scratch: {ctx}");
            assert_eq!(cpu.mem.rd8(SCREEN_BYTE), screen, "screen byte: {ctx}");
            assert_eq!(cpu.mem.rd32(size_at), size, "staged size: {ctx}");
            assert_eq!(cpu.mem.rd32(STAGED), staged, "staged: {ctx}");
            assert_eq!(cpu.mem.rd16(GAME_MODE), mode, "mode: {ctx}");
            assert_eq!(
                cpu.mem.rd16(STAGED_INDEX) as i16,
                staged_index,
                "staged index: {ctx}"
            );
            assert_eq!(cstr(&mut cpu.mem, NAME_ACTIVE), active, "active: {ctx}");
            assert_eq!(cstr(&mut cpu.mem, NAME_PREV), previous, "previous: {ctx}");
            held += u32::from(before.state == 4 && mode == 3);
            gated += u32::from(before.state == 0 && actor.state == 0);
            stale += u32::from(before.state >= 5);
            if mode == 2 {
                break;
            }
        }
    }
    assert!(
        handed > 2000 && indexed > 100 && held > 2000 && gated > 300 && stale > 500,
        "{handed} {indexed} {held} {gated} {stale}"
    );
}
