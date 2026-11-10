//! The field overlay's scripted-scene actor `FUN_801D4A60` and its spawner
//! `FUN_801D5A24` (PROT 0897) against
//! `legaia_engine_field::field_actor_program::{step_scene_program, spawn_program}`.
//!
//! The whole routine runs, over every state of the 38-entry table and beyond
//! it, with its callees hooked: the part stager is logged with the two
//! vectors it is handed, the flag, cue and stream calls with their arguments,
//! and the guard test is scripted.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_field::field_actor_program::{
    FLAG_RELEASE_GUARD, ProgramActor, ProgramEffect, ProgramEnv, ProgramPlayer, SPAWN_DESCRIPTOR,
    STAGE_SCALE, spawn_program, step_scene_program,
};
use psx_retail_exec::{A0, A1, A2, A3, Stop, V0};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

const OVERLAY_BASE: u32 = 0x801C_E818;
const PROGRAM: u32 = 0x801D_4A60;
const SPAWN_PROGRAM: u32 = 0x801D_5A24;

const STAGE: u32 = 0x8002_1B04;
const FLAG_SET: u32 = 0x8003_CE08;
const FLAG_CLEAR: u32 = 0x8003_CE34;
const FLAG_TEST: u32 = 0x8003_CE64;
const SFX: u32 = 0x8003_5B50;
const XA_CUE: u32 = 0x8003_D53C;
const XA_STREAM: u32 = 0x8001_9794;
const SPAWN: u32 = 0x8002_0DE0;

const FRAME_STEP: u32 = 0x1F80_0393;
const STORY_FLAGS: u32 = 0x1F80_0394;
const PLAYER_PTR: u32 = 0x8007_C364;
const EFFECT_LIST: u32 = 0x8007_C34C;
const BANK_REQUEST: u32 = 0x8007_BABC;
const BANK_CURRENT: u32 = 0x8007_BAA0;
const DEV_FLAGS: u32 = 0x8007_B868;
const XA_BUSY: u32 = 0x8007_BC20;

const ACTOR: u32 = 0x8012_0000;
const PLAYER: u32 = 0x8012_3000;

#[test]
fn the_scene_program_matches_retail() {
    differential!(prot(897, PROGRAM));
    let Some(disc) = disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut cpu = scus_cpu().expect("disc present");
    cpu.mem
        .load(OVERLAY_BASE, &disc.read_entry(897).expect("PROT 0897"));
    let log = Rc::new(RefCell::new(Vec::<ProgramEffect>::new()));
    let guard = Rc::new(Cell::new(0u32));
    let l = log.clone();
    cpu.hook(STAGE, move |r, m| {
        assert_eq!(r.r[A3], STAGE_SCALE as u32, "the stage scale");
        let v = |m: &mut psx_retail_exec::Memory, at: u32| {
            (
                m.rd16(at) as i16,
                m.rd16(at + 2) as i16,
                m.rd16(at + 4) as i16,
            )
        };
        let (pos, rot) = (v(m, r.r[A0]), v(m, r.r[A1]));
        l.borrow_mut().push(ProgramEffect::StagePart {
            record: r.r[A2],
            pos,
            rot,
        });
    });
    let l = log.clone();
    cpu.hook(FLAG_SET, move |r, _| {
        l.borrow_mut().push(ProgramEffect::SetFlag(r.r[A0] as u8))
    });
    let l = log.clone();
    cpu.hook(FLAG_CLEAR, move |r, _| {
        l.borrow_mut().push(ProgramEffect::ClearFlag(r.r[A0] as u8))
    });
    let g = guard.clone();
    cpu.hook(FLAG_TEST, move |r, _| {
        assert_eq!(r.r[A0], u32::from(FLAG_RELEASE_GUARD));
        r.r[V0] = g.get();
    });
    let l = log.clone();
    cpu.hook(SFX, move |r, _| {
        l.borrow_mut().push(ProgramEffect::Sfx(r.r[A0] as u16))
    });
    let l = log.clone();
    cpu.hook(XA_CUE, move |r, _| {
        l.borrow_mut().push(ProgramEffect::XaCue {
            clip: r.r[A0] as u8,
            chan: r.r[A1] as u8,
            dur: r.r[A2] as u16,
        })
    });
    let l = log.clone();
    cpu.hook(XA_STREAM, move |r, _| {
        l.borrow_mut().push(ProgramEffect::XaStream {
            clip: r.r[A0] as u8,
        })
    });
    cpu.mem.wr32(PLAYER_PTR, PLAYER);

    let mut rng = Rng(0x5343_4E50);
    let mut seen = [0u32; 0x28];
    let (mut advanced, mut retired, mut released, mut requested, mut arrived, mut streamed) =
        (0, 0, 0, 0, 0, 0);
    for round in 0..120_000 {
        let state = match rng.below(40) {
            0 => 0x26 + rng.below(4) as u16,
            1 => rng.next() as u16 | 0x8000,
            _ => rng.below(0x26) as u16,
        };
        let near = [0x14i16, 0x20, 0x28, 0x30, 0x32, 0x40, 0x64][rng.below(7) as usize];
        let actor = ProgramActor {
            program: match rng.below(8) {
                0 => rng.next() as u16,
                _ => rng.below(4) as u16,
            },
            state,
            accum: match rng.below(4) {
                0 => rng.next() as u16,
                _ => (near - 5 + rng.below(8) as i16) as u16,
            },
            saved_speed: rng.next() as u16,
            angle: rng.below(0x600) as i16 - 0x300,
            flags: rng.next() & !8,
        };
        let mut player = ProgramPlayer {
            pos: (rng.next() as i16, rng.next() as i16, rng.next() as i16),
            rot: (rng.next() as i16, rng.next() as i16, rng.next() as i16),
            flags: rng.next(),
            speed: rng.next() as u16,
            lift: rng.next() as i16,
        };
        if rng.below(3) != 0 {
            // A lift within reach of its arrival, so the wind-down lands.
            player.lift = match rng.below(3) {
                0 => -actor.angle,
                1 => -actor.angle + rng.below(0x60) as i16,
                _ => -actor.angle + rng.below(0x400) as i16 - 0x80,
            };
        }
        let request = match rng.below(4) {
            0 => 0x7F3 + rng.below(3) as i32,
            1 => -1,
            _ => rng.below(4) as i32,
        };
        let env = ProgramEnv {
            frame_delta: rng.below(5) as u8,
            bgm_request: request,
            bgm_current: match rng.below(3) {
                0 => request,
                1 => -1,
                _ => 0x7F3 + rng.below(3) as i32,
            },
            dev_flags: rng.below(4) / 3 * rng.next(),
            xa_busy: rng.below(3) / 2,
            story_flags: rng.next(),
            release_guard_set: rng.below(2) == 0,
        };

        cpu.mem.wr16(ACTOR + 0x50, actor.program);
        cpu.mem.wr16(ACTOR + 0x54, actor.state);
        cpu.mem.wr16(ACTOR + 0x9E, actor.accum);
        cpu.mem.wr16(ACTOR + 0x72, actor.saved_speed);
        cpu.mem.wr16(ACTOR + 0x16, actor.angle as u16);
        cpu.mem.wr32(ACTOR + 0x10, actor.flags);
        for (i, v) in [player.pos.0, player.pos.1, player.pos.2, 0x7777]
            .into_iter()
            .enumerate()
        {
            cpu.mem.wr16(PLAYER + 0x14 + 2 * i as u32, v as u16);
        }
        for (i, v) in [player.rot.0, player.rot.1, player.rot.2, 0x7777]
            .into_iter()
            .enumerate()
        {
            cpu.mem.wr16(PLAYER + 0x24 + 2 * i as u32, v as u16);
        }
        cpu.mem.wr32(PLAYER + 0x10, player.flags);
        cpu.mem.wr16(PLAYER + 0x72, player.speed);
        cpu.mem.wr16(PLAYER + 0x8E, player.lift as u16);
        cpu.mem.wr8(FRAME_STEP, env.frame_delta);
        cpu.mem.wr32(STORY_FLAGS, env.story_flags);
        cpu.mem.wr32(BANK_REQUEST, env.bgm_request as u32);
        cpu.mem.wr32(BANK_CURRENT, env.bgm_current as u32);
        cpu.mem.wr32(DEV_FLAGS, env.dev_flags);
        cpu.mem.wr32(XA_BUSY, env.xa_busy);
        guard.set(u32::from(env.release_guard_set));
        log.borrow_mut().clear();
        assert_eq!(cpu.call(PROGRAM, &[ACTOR]), Stop::Returned, "round {round}");

        let step = step_scene_program(actor, player, env);
        let ctx = format!("round {round} {actor:x?} {player:x?} {env:x?}");
        let mut want_request = env.bgm_request;
        let effects: Vec<ProgramEffect> = step
            .effects
            .iter()
            .filter(|e| match **e {
                ProgramEffect::RequestBgm(t) => {
                    want_request = t;
                    requested += 1;
                    false
                }
                _ => true,
            })
            .copied()
            .collect();
        assert_eq!(*log.borrow(), effects, "effects: {ctx}");
        let got_actor = ProgramActor {
            program: cpu.mem.rd16(ACTOR + 0x50),
            state: cpu.mem.rd16(ACTOR + 0x54),
            accum: cpu.mem.rd16(ACTOR + 0x9E),
            saved_speed: cpu.mem.rd16(ACTOR + 0x72),
            angle: cpu.mem.rd16(ACTOR + 0x16) as i16,
            flags: cpu.mem.rd32(ACTOR + 0x10),
        };
        assert_eq!(got_actor, step.actor, "actor: {ctx}");
        let got_player = ProgramPlayer {
            pos: (
                cpu.mem.rd16(PLAYER + 0x14) as i16,
                cpu.mem.rd16(PLAYER + 0x16) as i16,
                cpu.mem.rd16(PLAYER + 0x18) as i16,
            ),
            rot: (
                cpu.mem.rd16(PLAYER + 0x24) as i16,
                cpu.mem.rd16(PLAYER + 0x26) as i16,
                cpu.mem.rd16(PLAYER + 0x28) as i16,
            ),
            flags: cpu.mem.rd32(PLAYER + 0x10),
            speed: cpu.mem.rd16(PLAYER + 0x72),
            lift: cpu.mem.rd16(PLAYER + 0x8E) as i16,
        };
        assert_eq!(got_player, step.player, "player: {ctx}");
        assert_eq!(cpu.mem.rd32(STORY_FLAGS), step.story_flags, "{ctx}");
        assert_eq!(cpu.mem.rd32(BANK_REQUEST) as i32, want_request, "{ctx}");
        assert_eq!(
            step.retired,
            step.actor.flags & 8 != 0,
            "the retire bit: {ctx}"
        );

        if let Some(n) = seen.get_mut(usize::from(state)) {
            *n += 1;
        }
        advanced += u32::from(step.actor.state == state.wrapping_add(1));
        retired += u32::from(step.retired);
        released += u32::from(
            step.retired && step.player.flags & 0x8_0000 == 0 && player.flags & 0x8_0000 != 0,
        );
        arrived += u32::from(state == 0x18 && step.actor.state == 0x19);
        streamed += u32::from(
            effects
                .iter()
                .any(|e| matches!(e, ProgramEffect::XaStream { .. })),
        );
    }
    assert!(
        seen[..0x26].iter().all(|&n| n > 1500),
        "every state of the table is reached: {seen:?}"
    );
    assert!(
        advanced > 10000
            && retired > 2000
            && released > 300
            && requested > 300
            && arrived > 300
            && streamed > 1000,
        "{advanced} {retired} {released} {requested} {arrived} {streamed}"
    );
}

#[test]
fn the_scene_program_spawner_matches_retail() {
    differential!(prot(897, SPAWN_PROGRAM));
    let Some(disc) = disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut cpu = scus_cpu().expect("disc present");
    cpu.mem
        .load(OVERLAY_BASE, &disc.read_entry(897).expect("PROT 0897"));
    let log = Rc::new(RefCell::new(Vec::<(u32, u32)>::new()));
    let ret = Rc::new(Cell::new(ACTOR));
    let (l, v) = (log.clone(), ret.clone());
    cpu.hook(SPAWN, move |r, _| {
        l.borrow_mut().push((r.r[A0], r.r[A1]));
        r.r[V0] = v.get();
    });
    cpu.mem.wr32(EFFECT_LIST, 0x8012_7000);
    for program in [0u16, 1, 2, 3, 0x1234, 0xFFFF] {
        for full in [false, true] {
            // A full pool returns null, and the spawner writes nothing.
            ret.set(if full { 0 } else { ACTOR });
            cpu.mem.wr16(ACTOR + 0x50, 0xA5A5);
            cpu.mem.wr16(ACTOR + 0x54, 0xA5A5);
            cpu.mem.wr16(0x8000_0050, 0xA5A5);
            cpu.mem.wr16(0x8000_0054, 0xA5A5);
            log.borrow_mut().clear();
            assert_eq!(
                cpu.call(SPAWN_PROGRAM, &[u32::from(program)]),
                Stop::Returned
            );
            assert_eq!(*log.borrow(), [(SPAWN_DESCRIPTOR, 0x8012_7000)]);
            let want = spawn_program(program);
            let (p, s) = if full {
                (0xA5A5, 0xA5A5)
            } else {
                (want.program, want.state)
            };
            assert_eq!(cpu.mem.rd16(ACTOR + 0x50), p, "program {program:#x}");
            assert_eq!(cpu.mem.rd16(ACTOR + 0x54), s, "program {program:#x}");
            assert_eq!(cpu.mem.rd16(0x8000_0050), 0xA5A5, "null is not written");
            assert_eq!(cpu.mem.rd16(0x8000_0054), 0xA5A5, "null is not written");
        }
    }
}
