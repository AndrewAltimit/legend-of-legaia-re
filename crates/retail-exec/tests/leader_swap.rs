//! The field party-leader swap controller `FUN_801D27E0` (PROT 0897) off the
//! disc, frame after frame against `LeaderSwap::step`: the six phases, the
//! arm gate's story-flag tests in retail's order, the fade counters at every
//! frame step, the leader search, and what each effect stands for in RAM -
//! the fades `FUN_80024E80`, the flag clears / set, the camera re-centre
//! calls, the leader bytes, the player's engaged bit and the fade object's
//! release. The called routines are hooked; the pose copies are the host's.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_field::leader_swap::{
    CAMERA_BUSY_BIT, LeaderSwap, LeaderSwapEffect, LeaderSwapWorld,
};
use psx_retail_exec::{A0, Stop, V0};
use std::cell::RefCell;
use std::rc::Rc;

const SWAP: u32 = 0x801D_27E0;
const OVERLAY_BASE: u32 = 0x801C_E818;
const FLAG_TEST: u32 = 0x8003_CE64;
const FLAG_SET: u32 = 0x8003_CE08;
const FLAG_CLEAR: u32 = 0x8003_CE34;
const FADE: u32 = 0x8002_4E80;
const REFRESH: u32 = 0x801D_E190;
const RECENTRE: u32 = 0x8001_7EC8;
const ZONE: u32 = 0x801D_E3E0;
const SNAP: u32 = 0x801D_B8EC;
const CLAMP: u32 = 0x801D_AA50;
const SPAWN_RECORD: u32 = 0x8003_BDE0;
const PLAYER_PTR: u32 = 0x8007_C364;
const LEADER: u32 = 0x8008_4597;
const LEADER_WORD: u32 = 0x8007_B8F8;
const SUPPRESS_A: u32 = 0x8007_B6B4;
const SUPPRESS_B: u32 = 0x8007_B6B0;
const SCRATCH: u32 = 0x1F80_0394;
const FRAME_STEP: u32 = 0x1F80_0393;
const REQUEST: u32 = 0x8007_B874;
const FADE_SLOT: u32 = 0x801F_3490;
const CTRL: u32 = 0x8013_0000;
const PLAYER: u32 = 0x8013_1000;
const MEMBERS: u32 = 0x8013_2000;
const FADE_OBJ: u32 = 0x8013_3000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Call {
    Test(u16),
    Set(u16),
    Clear(u16),
    Fade,
    Refresh,
    Recentre,
    Zone,
    Snap,
    Clamp,
    Record,
}

#[test]
fn the_leader_swap_runs_as_retail_runs_it() {
    differential!(prot(897, SWAP));
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &d.read_entry(897).expect("PROT 0897"));
    cpu.mem.wr32(PLAYER_PTR, PLAYER);
    let calls: Rc<RefCell<Vec<Call>>> = Rc::default();
    let flags: Rc<RefCell<[bool; 0x40]>> = Rc::new(RefCell::new([false; 0x40]));
    {
        let (calls, flags) = (calls.clone(), flags.clone());
        cpu.hook(FLAG_TEST, move |r, _| {
            let i = r.r[A0] as u16;
            calls.borrow_mut().push(Call::Test(i));
            r.r[V0] = u32::from(flags.borrow()[usize::from(i) & 0x3F]);
        });
    }
    for (va, mk) in [
        (FLAG_SET, Call::Set as fn(u16) -> Call),
        (FLAG_CLEAR, Call::Clear as fn(u16) -> Call),
    ] {
        let calls = calls.clone();
        cpu.hook(va, move |r, _| calls.borrow_mut().push(mk(r.r[A0] as u16)));
    }
    for (va, call) in [
        (FADE, Call::Fade),
        (REFRESH, Call::Refresh),
        (RECENTRE, Call::Recentre),
        (ZONE, Call::Zone),
        (SNAP, Call::Snap),
        (CLAMP, Call::Clamp),
        (SPAWN_RECORD, Call::Record),
    ] {
        let calls = calls.clone();
        cpu.hook(va, move |r, _| {
            calls.borrow_mut().push(call);
            r.r[V0] = if call == Call::Fade { FADE_OBJ } else { 0 };
        });
    }
    for k in 0..3u32 {
        cpu.mem.wr32(CTRL + 0x80 + 4 * k, MEMBERS + 0x100 * k);
    }
    let mut rng = Rng(0x4C45_4144);
    let mut reached = [0u32; 6];
    let mut swaps = 0;
    for run in 0..3000 {
        let base = 0x10 + rng.below(0x20) as u16;
        let mut port = LeaderSwap {
            phase: [0i16, 0, 0, 1, 2, 3, 4, 5, 6][rng.below(9) as usize],
            counter: rng.below(0x20) as u16,
            flag_base: base,
        };
        cpu.mem.wr16(CTRL + 0x54, port.phase as u16);
        cpu.mem.wr16(CTRL + 0x9E, port.counter);
        cpu.mem.wr16(CTRL + 0x50, base);
        let mut ctrl_flags = rng.next();
        cpu.mem.wr32(CTRL + 0x10, ctrl_flags);
        let mut fade_flags = rng.next();
        cpu.mem.wr32(FADE_OBJ + 0x10, fade_flags);
        cpu.mem.wr32(FADE_SLOT, FADE_OBJ);
        let mut leader = rng.below(3) as u8;
        cpu.mem.wr8(LEADER, leader);
        for frame in 0..80 {
            {
                let mut f = flags.borrow_mut();
                for (i, v) in f.iter_mut().enumerate() {
                    *v = match i {
                        0x0D => rng.below(16) != 0,
                        _ => rng.below(2) == 0,
                    };
                }
                // Keep the leader search finite: one slot is always clear.
                f[usize::from(base) + rng.below(3) as usize] = false;
            }
            let suppress_a = [0u32, 0, 0, 1][rng.below(4) as usize];
            let suppress_b = [0i32, 0, -1, 1][rng.below(4) as usize];
            let mut player_flags = rng.next()
                & !(if rng.below(2) == 0 {
                    CAMERA_BUSY_BIT
                } else {
                    0
                });
            let scratch = rng.next() & !(if rng.below(2) == 0 { 0x400 } else { 0 });
            let request = rng.next();
            let step = [1u8, 1, 2, 3, 4][rng.below(5) as usize];
            cpu.mem.wr32(SUPPRESS_A, suppress_a);
            cpu.mem.wr32(SUPPRESS_B, suppress_b as u32);
            cpu.mem.wr32(PLAYER + 0x10, player_flags);
            cpu.mem.wr32(SCRATCH, scratch);
            cpu.mem.wr32(REQUEST, request);
            cpu.mem.wr8(FRAME_STEP, step);
            calls.borrow_mut().clear();
            assert_eq!(cpu.call(SWAP, &[CTRL]), Stop::Returned, "run {run}");

            let world = LeaderSwapWorld {
                leader,
                suppress_a: suppress_a as i32,
                suppress_b,
                camera_flags: player_flags,
                pad: scratch,
                request_byte: request,
            };
            let phase_in = port.phase;
            let mut tested = Vec::new();
            let tick = {
                let f = flags.borrow();
                port.step(&world, step, |i| {
                    tested.push(i);
                    f[usize::from(i) & 0x3F]
                })
            };
            let mut want: Vec<Call> = Vec::new();
            let mut tests = tested.into_iter();
            for e in &tick.effects {
                match *e {
                    LeaderSwapEffect::CachePartyPoses | LeaderSwapEffect::RetireController => {}
                    LeaderSwapEffect::SpawnFadeOut => {
                        player_flags |= CAMERA_BUSY_BIT;
                        want.push(Call::Fade);
                    }
                    LeaderSwapEffect::StoreOutgoingPose { .. } => {}
                    LeaderSwapEffect::CommitLeader { slot } => {
                        want.extend(tests.by_ref().map(Call::Test));
                        want.extend([
                            Call::Clear(0x10),
                            Call::Clear(0x11),
                            Call::Clear(0x12),
                            Call::Set(0x10 + u16::from(slot)),
                        ]);
                        leader = slot;
                    }
                    LeaderSwapEffect::RefreshParty => want.push(Call::Refresh),
                    LeaderSwapEffect::RecentreCamera { .. } => {
                        want.extend([Call::Recentre, Call::Zone, Call::Snap, Call::Clamp]);
                    }
                    LeaderSwapEffect::ClearIncomingPose { .. } => {}
                    LeaderSwapEffect::SpawnFadeIn => want.extend([Call::Fade, Call::Record]),
                    LeaderSwapEffect::ReleaseFadeObject => fade_flags |= 8,
                    LeaderSwapEffect::ClearCameraBusy => player_flags &= !CAMERA_BUSY_BIT,
                }
            }
            if phase_in == 5 {
                ctrl_flags &= !8;
            }
            // Phase 0's tests run before any effect but the pose cache.
            let leftover: Vec<Call> = tests.map(Call::Test).collect();
            if phase_in == 0 {
                want.splice(0..0, leftover);
            } else {
                assert!(leftover.is_empty());
            }
            let label = format!("run {run} frame {frame} phase {phase_in} step {step} {world:?}");
            assert_eq!(*calls.borrow(), want, "{label}");
            assert_eq!(cpu.mem.rd16(CTRL + 0x54) as i16, port.phase, "{label}");
            assert_eq!(cpu.mem.rd16(CTRL + 0x9E), port.counter, "{label}");
            assert_eq!(cpu.mem.rd32(PLAYER + 0x10), player_flags, "{label}");
            assert_eq!(cpu.mem.rd32(CTRL + 0x10), ctrl_flags, "{label}");
            assert_eq!(cpu.mem.rd32(FADE_OBJ + 0x10), fade_flags, "{label}");
            assert_eq!(cpu.mem.rd8(LEADER), leader, "{label}");
            if let Some(n) = tick.new_leader {
                assert_eq!(cpu.mem.rd16(LEADER_WORD), u16::from(n), "{label}");
                assert_eq!(cpu.mem.rd8(LEADER + 1), n, "{label}");
                swaps += 1;
            }
            if (0..6).contains(&phase_in) {
                reached[phase_in as usize] += 1;
            }
        }
    }
    assert!(
        reached.iter().all(|&n| n > 200) && swaps > 200,
        "{reached:?} {swaps}"
    );
}
