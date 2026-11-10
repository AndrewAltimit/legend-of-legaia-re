//! The per-actor motion VM `FUN_8003774C` against `motion_vm::step`, one leg
//! at a time frame by frame: the compass walks `0x37` / `0x41`, the compass
//! ramp `0x38`, the hold `0x43`, the tile walk `0x47` and the face-target ramp
//! `0x4C`, over generated operands, start positions, headings and frame steps.
//!
//! The port's headings sit a half-turn from retail's (`engine = retail +
//! 0x800`, `heading_lut_engine`), so headings compare modulo `0x1000` across
//! that offset.

use crate::common::{Rng, scus_cpu};
use legaia_engine_motion_vm::motion_vm::{MotionState, MotionTarget, StepResult, step};
use psx_retail_exec::Stop;

const ENTRY: u32 = 0x8003_774C;
const ACTOR: u32 = 0x8013_0000;
const TARGET: u32 = 0x8013_1000;
const SCRIPT: u32 = 0x8014_0000;
const PLAYER_PTR: u32 = 0x8007_C364;
const FRAME_STEP: u32 = 0x1F80_0393;
const MAX_FRAMES: usize = 400;

/// One leg's script and the inputs the port's caller derives from it.
struct Leg {
    bytes: Vec<u8>,
    s2: u8,
    speed: u16,
    approach: u8,
    target: (i16, i16),
}

fn tile(b: u8) -> i16 {
    i16::from(b & 0x7F) * 0x80 + if b & 0x80 != 0 { 0x80 } else { 0x40 }
}

fn gen_leg(rng: &mut Rng, op: u8, actor_xz: (i16, i16)) -> Leg {
    let s2 = 1 + rng.below(4) as u8;
    let b = |rng: &mut Rng| rng.next() as u8;
    match op {
        0x37 | 0x41 => Leg {
            bytes: vec![op, b(rng), b(rng)],
            s2,
            speed: u16::from(s2),
            approach: 0,
            target: (0, 0),
        },
        0x38 => {
            let b1 = (b(rng) & 0x80) | (rng.below(40) as u8);
            Leg {
                bytes: vec![op, (b(rng) & 0x80) | rng.below(8) as u8, b1],
                s2,
                speed: u16::from(s2),
                approach: 0,
                target: (0, 0),
            }
        }
        0x43 => Leg {
            bytes: vec![op],
            s2,
            speed: u16::from(s2),
            approach: 0,
            target: (0, 0),
        },
        0x47 => {
            // The tile near the actor, so legs end inside the frame budget.
            let near = |c: i16, rng: &mut Rng| -> u8 {
                let t = (i32::from(c) / 0x80 + rng.below(9) as i32 - 4).clamp(0, 0x7E);
                t as u8 | if rng.below(2) == 0 { 0x80 } else { 0 }
            };
            let (b0, b1) = (near(actor_xz.0, rng), near(actor_xz.1, rng));
            let approach = [0u8, 0, 1, 2, 3][rng.below(5) as usize];
            let bits = rng.below(6) as u8;
            let b2 = (approach << 4) | bits;
            Leg {
                bytes: vec![op, b0, b1, b2],
                s2,
                speed: 0x80 >> (2 + bits),
                approach,
                target: (tile(b0), tile(b1)),
            }
        }
        _ => {
            // A sub-mode the arm does not take parks retail's script for
            // ever; the port ends the leg instead, a disclosed departure
            // (`docs/subsystems/motion-vm.md`), so the generator leaves it out.
            let sub = [0x85u8, 0x8E, 0x8F][rng.below(3) as usize];
            let budget = rng.below(60) as u16;
            Leg {
                bytes: vec![op, sub, budget as u8, (budget >> 8) as u8, 0xF8],
                s2,
                speed: u16::from(s2),
                approach: 0,
                target: (
                    actor_xz.0.wrapping_add(rng.below(2001) as i16 - 1000),
                    actor_xz.1.wrapping_add(rng.below(2001) as i16 - 1000),
                ),
            }
        }
    }
}

#[test]
fn the_motion_vm_walks_and_turns_as_retail_does() {
    differential!(scus(ENTRY));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem.wr32(PLAYER_PTR, TARGET);
    let mut rng = Rng(0x4D4F_5449);
    let mut done = [0u32; 6];
    for round in 0..6000 {
        let op = [0x37u8, 0x41, 0x38, 0x43, 0x47, 0x4C][round % 6];
        let x = rng.below(0x3000) as i16;
        let z = rng.below(0x3000) as i16;
        let yaw = rng.below(0x1000) as u16;
        let leg = gen_leg(&mut rng, op, (x, z));

        cpu.mem.load(SCRIPT, &leg.bytes);
        cpu.mem.wr32(ACTOR + 0x94, SCRIPT);
        cpu.mem.wr16(ACTOR + 0x50, 0x10);
        cpu.mem.wr16(ACTOR + 0x14, x as u16);
        cpu.mem.wr16(ACTOR + 0x18, z as u16);
        cpu.mem.wr16(ACTOR + 0x26, yaw);
        cpu.mem.wr16(ACTOR + 0x54, 0);
        cpu.mem.wr16(TARGET + 0x14, leg.target.0 as u16);
        cpu.mem.wr16(TARGET + 0x18, leg.target.1 as u16);
        cpu.mem.wr8(FRAME_STEP, leg.s2);

        let mut st = MotionState {
            world_x: x,
            world_z: z,
            yaw: (yaw + 0x800) & 0xFFF,
            speed: leg.speed,
            approach: leg.approach,
            walk_iterations: u16::from(leg.s2),
            ..MotionState::default()
        };
        let target = MotionTarget {
            x: leg.target.0,
            z: leg.target.1,
            ..MotionTarget::default()
        };
        for frame in 0..MAX_FRAMES {
            assert_eq!(cpu.call(ENTRY, &[ACTOR]), Stop::Returned, "round {round}");
            let retail_done = cpu.regs.r[psx_retail_exec::V0] != 0;
            let retail = (
                cpu.mem.rd16(ACTOR + 0x14) as i16,
                cpu.mem.rd16(ACTOR + 0x18) as i16,
                cpu.mem.rd16(ACTOR + 0x26),
                cpu.mem.rd16(ACTOR + 0x54),
            );
            let port_done = step(&mut st, target, &leg.bytes) == StepResult::Done;
            let ctx = format!(
                "round {round} frame {frame} op {op:#x} bytes {:02x?} s2 {} start ({x}, {z}) yaw {yaw:#x} target {:?}",
                leg.bytes, leg.s2, leg.target
            );
            assert_eq!(
                (retail.0, retail.1, retail.3, retail_done),
                (st.world_x, st.world_z, st.op_accum, port_done),
                "{ctx}: x, z, accum, done"
            );
            assert_eq!(
                retail.2.wrapping_add(0x800) & 0xFFF,
                st.yaw & 0xFFF,
                "{ctx}: heading retail {:#x} port {:#x}",
                retail.2,
                st.yaw
            );
            if retail_done {
                done[round % 6] += 1;
                break;
            }
        }
    }
    // Every op but the hold ends its legs.
    assert!(
        done.iter()
            .enumerate()
            .all(|(i, &n)| (i == 3) == (n == 0) && (i == 3 || n > 300)),
        "{done:?}"
    );
}
