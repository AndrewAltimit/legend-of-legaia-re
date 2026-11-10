//! The battle side-band tick `FUN_80056208` against `battle_sideband_tick`:
//! every stage id (the sparring intro's four phases and the stray ones past
//! them, Cort's arrival before and after its module is resident, the form
//! transition's outro wait), every phase byte, the arm and ramp mode bytes
//! and their neighbours, timers either side of zero, pad edges, the loader's
//! step byte around `0xFF` and `0x12`, the opening counter's sign and a busy
//! or idle CD.
//!
//! Every call the tick makes is hooked and compared in order against the
//! port's effect list (the camera-aim call's three vectors and mode
//! included), and the written regions are compared whole: the battle context,
//! the camera block `0x800840B8..`, the caption pointer and the three pad
//! words.

use crate::common::{Rng, diff_offsets, scus_cpu};
use legaia_engine_battle::battle_sideband::{
    BattleSidebandEffect as E, BattleSidebandInputs, BattleSidebandState, INTRO_CAMERA_CONSTS,
    INTRO_CAMERA_MODE, INTRO_CAPTION_UI_ELEMENT, INTRO_CTX_STAMP, battle_sideband_tick,
};
use psx_retail_exec::{A0, A1, A2, A3, Stop, V0};
use std::cell::RefCell;
use std::rc::Rc;

const SIDEBAND: u32 = 0x8005_6208;
const UI_ELEMENT: u32 = 0x801D_8DE8;
const CAMERA_AIM: u32 = 0x801D_829C;
const DRAIN_FLOATING: u32 = 0x8003_55F0;
const SPARRING_HOOK: u32 = 0x801F_6B70;
const TEARDOWN_STAGING: u32 = 0x8002_5358;
const ARRIVAL: u32 = 0x801F_69F4;
const FORM_TRANSITION: u32 = 0x801F_69D8;
const CD_POLL: u32 = 0x8003_DE7C;

const CTX_PTR: u32 = 0x8007_BD24;
const CTX: u32 = 0x8012_0000;
const CTX_LEN: usize = 0x900;
const STAGE: u32 = 0x8007_B64A;
const OPENING: u32 = 0x8007_B648;
const LOADER: u32 = 0x8007_BD71;
const FRAME_STEP: u32 = 0x1F80_0393;
const PAD_A: u32 = 0x8007_B874;
const PAD_B: u32 = 0x8007_B938;
const PAD_C: u32 = 0x8007_B850;
const CAMERA: u32 = 0x8008_40B8;
const CAPTION_PTR: u32 = 0x8007_7494;
const CAPTION_TEXT: u32 = 0x8007_8CB4;
const ACTOR_SEAT3: u32 = 0x801C_937C;
const MONSTER: u32 = 0x8013_0000;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Call {
    Ui(u32, u32),
    Aim([i16; 3], [i16; 3], [i16; 3], u32),
    Drain,
    Sparring,
    Teardown,
    Arrival,
    FormTransition,
}

#[test]
fn the_sideband_tick_runs_as_retail_runs() {
    differential!(scus(SIDEBAND));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem.wr32(CTX_PTR, CTX);
    cpu.mem.wr32(ACTOR_SEAT3, MONSTER);
    let calls = Rc::new(RefCell::new(Vec::new()));
    let hook = |cpu: &mut psx_retail_exec::Cpu, va: u32, c: Call| {
        let calls = calls.clone();
        cpu.hook(va, move |_, _| calls.borrow_mut().push(c.clone()));
    };
    hook(&mut cpu, DRAIN_FLOATING, Call::Drain);
    hook(&mut cpu, SPARRING_HOOK, Call::Sparring);
    hook(&mut cpu, ARRIVAL, Call::Arrival);
    hook(&mut cpu, FORM_TRANSITION, Call::FormTransition);
    {
        let calls = calls.clone();
        cpu.hook(UI_ELEMENT, move |r, _| {
            calls.borrow_mut().push(Call::Ui(r.r[A0], r.r[A1]))
        });
    }
    {
        let calls = calls.clone();
        cpu.hook(CAMERA_AIM, move |r, m| {
            let v = |m: &mut psx_retail_exec::Memory, a: u32| {
                std::array::from_fn(|i| m.rd16(a + 2 * i as u32) as i16)
            };
            let c = Call::Aim(v(m, r.r[A0]), v(m, r.r[A1]), v(m, r.r[A2]), r.r[A3]);
            calls.borrow_mut().push(c);
        });
    }
    let staging_ret = Rc::new(RefCell::new(0u32));
    {
        let calls = calls.clone();
        let ret = staging_ret.clone();
        cpu.hook(TEARDOWN_STAGING, move |r, _| {
            calls.borrow_mut().push(Call::Teardown);
            r.r[V0] = *ret.borrow();
        });
    }
    let cd_busy = Rc::new(RefCell::new(0u32));
    {
        let busy = cd_busy.clone();
        cpu.hook(CD_POLL, move |r, _| {
            assert_eq!(r.r[A0], 1, "CD poll mode");
            r.r[V0] = *busy.borrow();
        });
    }

    let mut rng = Rng(0x5349_4442);
    let mut arms = std::collections::HashMap::new();
    for n in 0..40_000 {
        let stage = match rng.below(5) {
            0 => rng.next() as u8,
            k => k as u8 - 1 + u8::from(rng.below(4) == 0),
        };
        let phase = match rng.below(6) {
            0 => rng.next() as u8,
            _ => rng.below(5) as u8,
        };
        let ctx_mode = match rng.below(4) {
            0 => 0x14,
            1 => 0x0C,
            2 => [0x13, 0x15, 0x0B, 0x0D][rng.below(4) as usize],
            _ => rng.next() as u8,
        };
        let timer = |rng: &mut Rng| match rng.below(3) {
            0 => rng.below(0x20) as i16 - 0x10,
            1 => rng.next() as i16,
            _ => rng.below(0xC00) as i16,
        };
        let loader = match rng.below(4) {
            0 => 0xFF,
            1 => [0x11, 0x12, 0xFE, 0x13][rng.below(4) as usize],
            _ => rng.next() as u8,
        };
        let frame_step = rng.below(5) as u8;
        let (pad_a, pad_b) = match rng.below(3) {
            0 => (0, 0),
            1 => (rng.next() & 0xFFFF_0000, rng.next() & 0xFFFF_0000),
            _ => (rng.next(), rng.next()),
        };
        let opening = rng.next() as u8;
        let cam_a = match rng.below(2) {
            0 => 0xBF0 + rng.below(0x20) as i32,
            _ => rng.next() as i32,
        };
        let state = BattleSidebandState {
            phase,
            caption_timer: timer(&mut rng),
            phase3_accum: rng.next() as u16,
            ramp_delay: timer(&mut rng),
            outro_timer: timer(&mut rng),
            camera_a: cam_a,
            camera_b: rng.next() as i32,
            hold: rng.next() as u16,
        };
        let staging = rng.below(3);
        let busy = rng.below(2);
        let monster_x = rng.next() as i16;
        let monster_z = rng.next() as i16;

        // Seed.
        let mut ctx: Vec<u8> = (0..CTX_LEN).map(|_| rng.next() as u8).collect();
        ctx[6] = ctx_mode;
        ctx[0x289] = state.phase;
        ctx[0x6AE..0x6B0].copy_from_slice(&state.caption_timer.to_le_bytes());
        ctx[0x6B0..0x6B2].copy_from_slice(&state.hold.to_le_bytes());
        ctx[0x6CE..0x6D0].copy_from_slice(&state.phase3_accum.to_le_bytes());
        ctx[0x6D6..0x6D8].copy_from_slice(&state.ramp_delay.to_le_bytes());
        ctx[0x6D8..0x6DA].copy_from_slice(&state.outro_timer.to_le_bytes());
        cpu.mem.load(CTX, &ctx);
        cpu.mem.wr8(STAGE, stage);
        cpu.mem.wr8(OPENING, opening);
        cpu.mem.wr8(LOADER, loader);
        cpu.mem.wr8(FRAME_STEP, frame_step);
        cpu.mem.wr32(PAD_A, pad_a);
        cpu.mem.wr32(PAD_B, pad_b);
        cpu.mem.wr32(PAD_C, 0xDEAD_BEEF);
        cpu.mem.wr32(CAMERA, 0x1234_5678);
        cpu.mem.wr32(CAMERA + 4, state.camera_a as u32);
        cpu.mem.wr32(CAMERA + 8, state.camera_b as u32);
        cpu.mem.wr32(CAMERA + 12, 0x0BAD_F00D);
        cpu.mem.wr32(CAPTION_PTR, 0xEEEE_EEEE);
        cpu.mem.wr16(MONSTER + 0x34, monster_x as u16);
        cpu.mem.wr16(MONSTER + 0x38, monster_z as u16);
        *staging_ret.borrow_mut() = staging;
        *cd_busy.borrow_mut() = busy;
        calls.borrow_mut().clear();
        assert_eq!(cpu.call(SIDEBAND, &[]), Stop::Returned, "n {n}");

        // The port.
        let inputs = BattleSidebandInputs {
            submode: stage,
            frame_step,
            ctx_mode,
            loader_state: loader,
            pad_edge: (pad_a | pad_b) & 0xFFFF != 0,
            opening_negative: (opening as i8) < 0,
            cd_idle: busy == 0,
        };
        let mut s = state;
        let fx = battle_sideband_tick(&mut s, &inputs);

        // What the port's effects and state say retail wrote.
        let mut want_ctx = ctx.clone();
        let mut want_calls = Vec::new();
        let (mut cap, mut pads) = (0xEEEE_EEEEu32, (pad_a, pad_b, 0xDEAD_BEEFu32));
        for e in &fx.effects {
            match e {
                E::IntroCaption => {
                    want_ctx[0x1B] = INTRO_CTX_STAMP.0;
                    want_ctx[0x1C] = INTRO_CTX_STAMP.1;
                    cap = CAPTION_TEXT;
                    want_calls.push(Call::Ui(u32::from(INTRO_CAPTION_UI_ELEMENT), 0));
                }
                E::IntroCameraAim => want_calls.push(Call::Aim(
                    [0; 3],
                    [0, INTRO_CAMERA_CONSTS.0, INTRO_CAMERA_CONSTS.1],
                    [monster_x.wrapping_neg(), 0, monster_z.wrapping_neg()],
                    u32::from(INTRO_CAMERA_MODE),
                )),
                E::DrainFloatingElements => want_calls.push(Call::Drain),
                E::SparringHook => want_calls.push(Call::Sparring),
                E::TeardownStaging => {
                    want_ctx[0xB] = staging as u8;
                    want_calls.push(Call::Teardown);
                }
                E::ArrivalModule => want_calls.push(Call::Arrival),
                E::FormTransitionModule => want_calls.push(Call::FormTransition),
                E::ClearPadState => {
                    pads = (0, 0, 0);
                    want_ctx[0x884..0x888].fill(0);
                }
            }
        }
        want_ctx[0x289] = s.phase;
        want_ctx[0x6AE..0x6B0].copy_from_slice(&s.caption_timer.to_le_bytes());
        want_ctx[0x6B0..0x6B2].copy_from_slice(&s.hold.to_le_bytes());
        want_ctx[0x6CE..0x6D0].copy_from_slice(&s.phase3_accum.to_le_bytes());
        want_ctx[0x6D6..0x6D8].copy_from_slice(&s.ramp_delay.to_le_bytes());
        want_ctx[0x6D8..0x6DA].copy_from_slice(&s.outro_timer.to_le_bytes());

        let got_ctx = cpu.mem.read(CTX, CTX_LEN);
        assert!(
            got_ctx == want_ctx,
            "n {n} {inputs:?} {state:?}: ctx {:?}",
            diff_offsets(&got_ctx, &want_ctx)
        );
        assert_eq!(*calls.borrow(), want_calls, "n {n} {inputs:?} {state:?}");
        assert_eq!(
            (
                cpu.mem.rd32(CAMERA),
                cpu.mem.rd32(CAMERA + 4) as i32,
                cpu.mem.rd32(CAMERA + 8) as i32,
                cpu.mem.rd32(CAMERA + 12),
            ),
            (0x1234_5678, s.camera_a, s.camera_b, 0x0BAD_F00D),
            "n {n} {inputs:?} {state:?}"
        );
        assert_eq!(cpu.mem.rd32(CAPTION_PTR), cap, "n {n}");
        assert_eq!(
            (
                cpu.mem.rd32(PAD_A),
                cpu.mem.rd32(PAD_B),
                cpu.mem.rd32(PAD_C)
            ),
            pads,
            "n {n}"
        );
        let key = fx.effects.first().map(|e| format!("{e:?}"));
        *arms.entry(key).or_insert(0u32) += 1;
    }
    // Eight arms: idle, and each effect leading.
    assert!(
        arms.len() == 8 && arms.values().all(|&c| c > 100),
        "{arms:?}"
    );
}
