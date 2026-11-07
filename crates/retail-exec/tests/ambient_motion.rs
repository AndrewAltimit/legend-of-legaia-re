//! The ambient motion VM `FUN_80038158` against `AmbientMotion::step_ops_with`,
//! tick by tick over generated streams: the restart, move and anim writes, the
//! directional steps, the facing ramp, the wait, the home step and the wander,
//! the story-flag and SFX ops, the translucency ops and the bit set / clear /
//! wait.
//!
//! Left out: the ops that reach the GPU, a model reload or the floor sampler
//! (`0x0E`, `0x0F`, `0x13`), and the ramp-scheduler ops (`0x0C`, `0x0D`,
//! `0x14..0x16`), whose pool runs in its own actor template.
//!
//! The two probes are overlay routines; both sides answer them from one
//! function of the actor's position and the probe's direction.

use crate::common::{Rng, scus_cpu};
use legaia_engine_motion_vm::ambient_motion::{AmbientBlocking, AmbientMotion};
use legaia_engine_motion_vm::ambient_motion_ops::AmbientEffect;
use psx_retail_exec::{A0, A1, Stop, V0};
use std::cell::Cell;
use std::rc::Rc;

const ENTRY: u32 = 0x8003_8158;
const ACTOR: u32 = 0x8013_0000;
const STREAM: u32 = 0x8014_0000;
const RECORDS: u32 = 0x801C_6470;
const FRAME_STEP: u32 = 0x1F80_0393;
const GLOBALS: u32 = 0x1F80_0394;
const MAILBOX: u32 = 0x8007_3F1C;
const FLAGS: u32 = 0x8008_5758;
const SFX_RING: u32 = 0x8007_B6D8;
const RAND: u32 = 0x8005_6798;
const STEP_PROBE: u32 = 0x801C_F8AC;
const WANDER_PROBE: u32 = 0x801D_5A68;
const SLOT: u16 = 5;
const TICKS: usize = 80;

fn step_blocked(x: i16, z: i16) -> bool {
    (x as u16 ^ (z as u16).rotate_left(5)).wrapping_mul(0x9E37) >> 13 == 0
}

fn wander_blocked(x: i16, z: i16, dir4: u8) -> bool {
    (x as u16 ^ (z as u16).rotate_left(3) ^ u16::from(dir4) << 9).wrapping_mul(0x6A09) >> 14 == 0
}

struct Probes;

impl AmbientBlocking for Probes {
    fn step_blocked(&self, x: i16, z: i16, _lut_index: u8) -> bool {
        step_blocked(x, z)
    }
    fn wander_blocked(&self, x: i16, z: i16, dir4: u8) -> bool {
        wander_blocked(x, z, dir4)
    }
}

fn gen_op(rng: &mut Rng) -> Vec<u8> {
    let ops = [
        0x02u8, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0x10, 0x11, 0x12, 0x17, 0x18,
        0x19, 0x20,
    ];
    let op = ops[rng.below(ops.len() as u32) as usize];
    let w = legaia_scene_formats::man_motion::op_width(op).unwrap();
    let mut v = vec![op];
    for _ in 1..w {
        v.push(rng.next() as u8);
    }
    match op {
        // A selector of `0xC0` reads through a null pointer in retail.
        0x10..=0x12 => v[1] &= 0xBF,
        // Short waits and ramps, so a stream gets through several ops.
        0x04 | 0x05 => *v.last_mut().unwrap() &= 0x9F,
        // A flag index inside the bank.
        0x07 | 0x08 => v[2] &= 0x03,
        // A compass index past the eight entries overreads into the next
        // table in retail; the port's disclosed departure leaves it out.
        // A pace past `3` steps nothing.
        0x03 | 0x19 | 0x20 => v[1] &= 0x73,
        _ => {}
    }
    v
}

#[test]
fn the_ambient_vm_runs_its_streams_as_retail_does() {
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let seed = Rc::new(Cell::new(0u32));
    let s = seed.clone();
    cpu.hook(RAND, move |r, _| {
        let mut v = s.get();
        r.r[V0] = u32::from(legaia_engine_system::rand::psyq_rand_step(&mut v));
        s.set(v);
    });
    cpu.hook(STEP_PROBE, |r, m| {
        let a = r.r[A0];
        r.r[V0] = u32::from(step_blocked(
            m.rd16(a + 0x14) as i16,
            m.rd16(a + 0x18) as i16,
        ));
    });
    cpu.hook(WANDER_PROBE, |r, m| {
        let a = r.r[A0];
        let (x, z) = (m.rd16(a + 0x14) as i16, m.rd16(a + 0x18) as i16);
        r.r[V0] = u32::from(wander_blocked(x, z, r.r[A1] as u8));
    });
    cpu.step_budget = 200_000;
    let mut rng = Rng(0x414D_4249);
    let mut moved = 0;
    for round in 0..10000 {
        let mut code = Vec::new();
        for _ in 0..1 + rng.below(6) {
            code.extend(gen_op(&mut rng));
        }
        // A wait and a restart at the end: retail loops for ever on a stream
        // with no op that ends the tick, and runs on past its last byte.
        code.extend([0x05, 1 + rng.below(8) as u8, 0x01]);
        let mut stream = vec![0xFF, 0xFF, 0, 0];
        stream.extend(&code);

        let x = (0x400 + rng.below(0x3000)) as i16;
        let z = (0x400 + rng.below(0x3000)) as i16;
        let heading = rng.below(0x1000) as u16;
        let speed = 1 + rng.below(4) as u8;
        let flags = rng.next();
        let clip = rng.next() as u16;
        let globals = rng.next();
        let rand0 = rng.next();

        // A fresh actor: the variant compare misses and the prologue installs
        // the stream, as a scene entry does.
        for o in (0..0x100).step_by(4) {
            cpu.mem.wr32(ACTOR + o, 0);
        }
        cpu.mem.load(STREAM, &stream);
        cpu.mem.wr32(ACTOR + 0x80, STREAM);
        cpu.mem.wr16(ACTOR + 0x50, SLOT);
        cpu.mem.wr32(ACTOR + 0x10, flags);
        cpu.mem.wr16(ACTOR + 0x14, x as u16);
        cpu.mem.wr16(ACTOR + 0x18, z as u16);
        cpu.mem.wr16(ACTOR + 0x26, heading);
        cpu.mem.wr16(ACTOR + 0x62, clip);
        cpu.mem.wr8(ACTOR + 0x8C, ((x >> 7) & 0x7F) as u8);
        cpu.mem.wr8(ACTOR + 0x8D, ((z >> 7) & 0x7F) as u8);
        cpu.mem.wr8(FRAME_STEP, speed);
        cpu.mem.wr32(GLOBALS, globals);
        cpu.mem.wr8(MAILBOX, 0xFF);
        for i in 0..0x80 {
            cpu.mem.wr8(FLAGS + i, 0);
        }
        for i in 0..4 {
            cpu.mem.wr16(SFX_RING + i * 2, 0);
        }
        let gp = cpu.regs.r[psx_retail_exec::GP];
        cpu.mem.wr16(gp + 0x158, 0);
        seed.set(rand0);

        let mut port = AmbientMotion::new(u32::from(SLOT), heading).with_position(x, z);
        port.actor_flags = flags;
        port.clip_control = clip;
        port.globals = globals;
        port.rng = rand0;
        let mut flag_bank = [0u8; 0x80];

        for tick in 0..TICKS {
            // Now and then the player walks into the actor: the collision
            // probe posts its id to the touch mailbox.
            if rng.below(6) == 0 {
                cpu.mem.wr8(MAILBOX, SLOT as u8);
                port.pending_touch = Some(u32::from(SLOT));
            }
            assert_eq!(
                cpu.call(ENTRY, &[ACTOR]),
                Stop::Returned,
                "round {round} tick {tick} code {code:02x?}"
            );
            port.step_ops_with(&code, speed, &Probes);
            for e in &port.effects {
                match *e {
                    AmbientEffect::SystemFlagSet(i) => {
                        flag_bank[usize::from(i >> 3)] |= 0x80 >> (i & 7)
                    }
                    AmbientEffect::SystemFlagClear(i) => {
                        flag_bank[usize::from(i >> 3)] &= !(0x80 >> (i & 7))
                    }
                    _ => {}
                }
            }
            let rec = RECORDS + u32::from(SLOT) * 4;
            let m = &mut cpu.mem;
            let retail = (
                m.rd16(ACTOR + 0x14) as i16,
                m.rd16(ACTOR + 0x18) as i16,
                m.rd16(ACTOR + 0x26),
                u16::from(m.rd8(ACTOR + 0x8B)) | u16::from(m.rd8(ACTOR + 0xB7)) << 8,
                m.rd16(ACTOR + 0x84).wrapping_sub(4),
                [m.rd8(rec), m.rd8(rec + 1)],
                m.rd8(rec + 2),
                ((m.rd16(ACTOR + 0x86) & 0x3000) >> 11) as u8,
                m.rd32(ACTOR + 0x10),
                m.rd16(ACTOR + 0x62),
                m.rd32(GLOBALS),
            );
            let ported = (
                port.x,
                port.z,
                port.heading,
                port.cursor,
                port.pc,
                port.default_move,
                port.wander_phase,
                port.wander_dir,
                port.actor_flags,
                port.clip_control,
                port.globals,
            );
            assert_eq!(
                retail, ported,
                "round {round} tick {tick} speed {speed} code {code:02x?}\n\
                 (x, z, heading, cursor, pc, default_move, wander_phase, wander_dir, flags, clip, globals)"
            );
            let retail_move = (m.rd16(ACTOR + 0x88), m.rd16(ACTOR + 0x5C));
            let pair = port.move_pair.unwrap_or(0) as u16;
            assert_eq!(
                retail_move,
                (pair, pair),
                "round {round} tick {tick} move pair code {code:02x?}"
            );
            assert_eq!(
                port.requested_move.map_or(0, u16::from),
                retail_move.0 & 0xFF,
                "round {round} tick {tick} requested move"
            );
            let mailbox = m.rd8(MAILBOX);
            assert_eq!(
                (mailbox != 0xFF).then_some(u32::from(mailbox)),
                port.pending_touch,
                "round {round} tick {tick} touch mailbox"
            );
            let bank = m.read(FLAGS, 0x80);
            assert_eq!(bank, flag_bank, "round {round} tick {tick} flag bank");
            let ring: Vec<i16> = (0..4).map(|i| m.rd16(SFX_RING + i * 2) as i16).collect();
            assert_eq!(ring, port.event_ring, "round {round} tick {tick} SFX ring");
            moved += i32::from(port.moved);
        }
    }
    assert!(moved > 2000, "{moved}");
}
