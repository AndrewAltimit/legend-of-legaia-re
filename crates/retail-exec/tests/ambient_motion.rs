//! The ambient motion VM `FUN_80038158` against `AmbientMotion`, tick by tick
//! over generated streams: every op of the table, the restart, move and anim
//! writes, the directional steps, both facing ramps, the wait and its touch
//! wake, the home step and the wander, the story-flag and SFX ops, the
//! translucency, tint, scale and angle ops, the model swap, the tile
//! teleport, the VRAM blit and the bit set / clear / wait (the null-pointer
//! selector included). The SCUS ramp pool `FUN_80036D80` ticks after the VM
//! on both sides, so the ramps the `0x0C`, `0x0D` and `0x14..0x16` installs
//! leave behind land on the fields they drive.
//!
//! A second test runs the preamble: the variant scan over the system-flag
//! bank, the variant switch that resets the actor's record, and the deferral
//! gate behind `+0x8A` bit 0, against `FieldNpcAmbient`'s `select_variant`
//! and `defer_gate` and `AmbientMotion::rebind_variant`.
//!
//! The two probes, the ground sampler, the model reload and `MoveImage` are
//! answered by hooks; both sides take the probes and the sampler from one
//! function of the actor's position.

use crate::common::{Rng, STACK_TOP, scus_cpu};
use legaia_engine_core::world::FieldNpcAmbient;
use legaia_engine_motion_vm::ambient_motion::{AmbientBlocking, AmbientMotion, Ramp, RampKind};
use legaia_engine_motion_vm::ambient_motion_ops::{AmbientEffect, ModelBank};
use psx_retail_exec::{A0, A1, A2, Cpu, Stop, V0};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

const ENTRY: u32 = 0x8003_8158;
const POOL_RESET: u32 = 0x8003_CDA8;
const POOL_INSTALL: u32 = 0x8003_C5F0;
const POOL_TICK: u32 = 0x8003_6D80;
const ALLOC: u32 = 0x8002_0DE0;
const POOL: u32 = 0x801C_66A0;
const OVERFLOW: u32 = 0x8007_3ED0;
const ACTOR: u32 = 0x8013_0000;
const PLAYER: u32 = 0x8013_8000;
const PLAYER_PTR: u32 = 0x8007_C364;
const STREAM: u32 = 0x8014_0000;
const DUMMY_DEST: u32 = 0x8015_0000;
const RECORDS: u32 = 0x801C_6470;
const FRAME_STEP: u32 = 0x1F80_0393;
const GLOBALS: u32 = 0x1F80_0394;
const MAILBOX: u32 = 0x8007_3F1C;
const FLAGS: u32 = 0x8008_5758;
const SFX_RING: u32 = 0x8007_B6D8;
const SCENE_BANK: u32 = 0x8007_B6F8;
const SPECIAL_BANK: u32 = 0x8007_B824;
const FAULT_CODE: u32 = 0x8007_B828;
const RAND: u32 = 0x8005_6798;
const STEP_PROBE: u32 = 0x801C_F8AC;
const WANDER_PROBE: u32 = 0x801D_5A68;
const GROUND: u32 = 0x8001_9278;
const MODEL_RELOAD: u32 = 0x8002_4E08;
const MOVE_IMAGE: u32 = 0x8005_8490;
const SLOT: u16 = 5;
const TICKS: usize = 80;

fn step_blocked(x: i16, z: i16) -> bool {
    (x as u16 ^ (z as u16).rotate_left(5)).wrapping_mul(0x9E37) >> 13 == 0
}

fn wander_blocked(x: i16, z: i16, dir4: u8) -> bool {
    (x as u16 ^ (z as u16).rotate_left(3) ^ u16::from(dir4) << 9).wrapping_mul(0x6A09) >> 14 == 0
}

fn ground(x: i16, z: i16) -> u16 {
    (x as u16).wrapping_mul(3) ^ (z as u16).rotate_left(7)
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

/// A short, mostly positive duration halfword: zero now and then, and a
/// negative one rarely.
fn duration(rng: &mut Rng) -> [u8; 2] {
    let d: i16 = match rng.below(10) {
        0 => 0,
        1 => -(1 + rng.below(40) as i16),
        _ => 1 + rng.below(40) as i16,
    };
    d.to_le_bytes()
}

/// One op. `tile` is the actor's tile pair, which the wander box is biased
/// round so the walk meets the box edges. `fault` lets one bit op per stream
/// take the null-pointer selector `0xC0`: retail scribbles physical `0`
/// there, which a later fault read in the same stream would see.
fn gen_op(rng: &mut Rng, tile: (u8, u8), fault: &mut bool) -> Vec<u8> {
    let ops = [
        0x02u8, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0x0C, 0x0D, 0x0E, 0x0F, 0x10,
        0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x20,
    ];
    let op = ops[rng.below(ops.len() as u32) as usize];
    let w = legaia_scene_formats::man_motion::op_width(op).unwrap();
    let mut v = vec![op];
    for _ in 1..w {
        v.push(rng.next() as u8);
    }
    match op {
        0x10..=0x12 => {
            if v[1] & 0xC0 == 0xC0 && (*fault || rng.below(4) != 0) {
                v[1] &= if rng.below(2) == 0 { 0xBF } else { 0x7F };
            }
            *fault |= v[1] & 0xC0 == 0xC0;
        }
        // Short waits and ramps, so a stream gets through several ops.
        0x04 | 0x05 => *v.last_mut().unwrap() &= 0x9F,
        // A flag index inside the bank.
        0x07 | 0x08 => v[2] &= 0x03,
        // A compass index past the eight entries overreads into the next
        // table in retail; the port's disclosed departure leaves it out.
        // A pace past `3` steps nothing.
        0x03 | 0x19 | 0x20 => v[1] &= 0x73,
        0x0C => {
            v[6..8].copy_from_slice(&duration(rng));
            if rng.below(3) == 0 {
                v[4..6].fill(0);
            }
        }
        0x0D => v[2..4].copy_from_slice(&duration(rng)),
        0x14..=0x16 => v[3..5].copy_from_slice(&duration(rng)),
        // Ids either side of the `0xF0` bank split.
        0x0E if rng.below(2) == 0 => {
            v[1] = 0xE8 + rng.below(0x10) as u8;
            v[2] = 0;
        }
        // A box round the actor's tile, so its edges stop the walk.
        0x18 if rng.below(2) == 0 => {
            let near = |rng: &mut Rng, t: u8, lo: bool| {
                let d = rng.below(2) as u8;
                if lo { t.saturating_sub(d) } else { t + d }
            };
            let pace = |b: u8| b & 0x80;
            v[1] = pace(v[1]) | near(rng, tile.0, true) & 0x7F;
            v[2] = pace(v[2]) | near(rng, tile.1, true) & 0x7F;
            v[3] = pace(v[3]) | near(rng, tile.0, false) & 0x7F;
            v[4] = pace(v[4]) | near(rng, tile.1, false) & 0x7F;
        }
        _ => {}
    }
    v
}

/// A variant's bytecode: a few ops, then a wait and a restart - retail loops
/// for ever on a stream with no op that ends the tick, and runs on past its
/// last byte.
fn gen_code(rng: &mut Rng, tile: (u8, u8)) -> Vec<u8> {
    let mut code = Vec::new();
    let mut fault = false;
    for _ in 0..1 + rng.below(6) {
        code.extend(gen_op(rng, tile, &mut fault));
    }
    code.extend([0x05, 1 + rng.below(8) as u8, 0x01]);
    code
}

#[derive(Default)]
struct Calls {
    models: Vec<u16>,
    blits: Vec<([u16; 4], i16, i16)>,
}

/// The CPU with its hooks in, the hooked `rand()` seed and the recorded calls.
type Rig = (Cpu, Rc<Cell<u32>>, Rc<RefCell<Calls>>);

fn setup() -> Option<Rig> {
    let mut cpu = scus_cpu()?;
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
    cpu.hook(GROUND, |r, m| {
        let a = r.r[A0];
        r.r[V0] = u32::from(ground(m.rd16(a + 0x14) as i16, m.rd16(a + 0x18) as i16));
    });
    let calls = Rc::new(RefCell::new(Calls::default()));
    let c = calls.clone();
    cpu.hook(MODEL_RELOAD, move |r, _| {
        c.borrow_mut().models.push(r.r[A1] as u16);
    });
    let c = calls.clone();
    cpu.hook(MOVE_IMAGE, move |r, m| {
        let a = r.r[A0];
        let rect = [m.rd16(a), m.rd16(a + 2), m.rd16(a + 4), m.rd16(a + 6)];
        c.borrow_mut()
            .blits
            .push((rect, r.r[A1] as i16, r.r[A2] as i16));
    });
    cpu.stub(ALLOC, 0);
    cpu.step_budget = 200_000;
    Some((cpu, seed, calls))
}

/// How a round's actor is driven.
struct Round {
    /// Header offset and selector of each variant, in table order, and the
    /// bytecode each runs. A single `0xFFFF` variant for the op test.
    variants: Vec<(usize, u16, Vec<u8>)>,
    /// `+0x8A` bit 0.
    defers: bool,
    /// Toggle the variants' flags, the player's engaged bit and the actor's
    /// `0x500` bits between ticks.
    churn: bool,
    slot: u16,
    /// The actor's starting `+0x14` / `+0x18`.
    at: (i16, i16),
}

fn run_round(
    cpu: &mut Cpu,
    seed: &Cell<u32>,
    calls: &RefCell<Calls>,
    rng: &mut Rng,
    round: usize,
    r: &Round,
) -> i32 {
    let (x, z) = r.at;
    let mut stream = Vec::new();
    for (_, sel, code) in &r.variants {
        let delta = if *sel == 0xFFFF { 0 } else { 4 + code.len() };
        stream.extend(sel.to_le_bytes());
        stream.extend((delta as u16).to_le_bytes());
        stream.extend(code);
    }
    let heading = rng.below(0x1000) as u16;
    let speed = 1 + rng.below(4) as u8;
    let mut flags = rng.next();
    if !r.churn {
        flags &= !0x500;
    }
    let clip = rng.next() as u16;
    let globals = rng.next();
    let rand0 = rng.next();
    let tint = rng.next() & 0x00FF_FFFF;
    let blend = if rng.below(2) == 0 {
        0
    } else {
        rng.next() as u16
    };
    let (pitch, roll) = (rng.next() as i16, rng.next() as i16);
    let banks = (rng.next() as u16, rng.next() as u16);
    let tile = (((x >> 7) & 0x7F) as u8, ((z >> 7) & 0x7F) as u8);

    // The pool, fresh, with some rounds nearly full of ramps another actor
    // owns so the installs overflow.
    assert_eq!(cpu.call(POOL_RESET, &[]), Stop::Returned);
    cpu.mem.wr32(OVERFLOW, 0);
    let mut port = AmbientMotion::new(u32::from(r.slot), heading).with_position(x, z);
    port.ramps.reset_pool();
    let fill = if rng.below(4) == 0 {
        58 + rng.below(6)
    } else {
        0
    };
    for _ in 0..fill {
        cpu.mem.wr32(STACK_TOP + 0x10, 0);
        cpu.mem.wr32(STACK_TOP + 0x14, 0x7FFF_FFFF);
        assert_eq!(
            cpu.call(POOL_INSTALL, &[0, DUMMY_DEST, 2, 0]),
            Stop::Returned
        );
        port.ramps.install(Ramp {
            dest: 0xFFFF,
            owner: u32::MAX,
            start: 0,
            end: 0,
            total: 0x7FFF_FFFF,
            remaining: 0x7FFF_FFFF,
            kind: RampKind::U16,
        });
    }

    // A fresh actor: the variant compare misses and the prologue installs
    // the stream, as a scene entry does.
    for o in (0..0x100).step_by(4) {
        cpu.mem.wr32(ACTOR + o, 0);
    }
    cpu.mem.load(STREAM, &stream);
    let m = &mut cpu.mem;
    m.wr32(ACTOR + 0x80, STREAM);
    m.wr16(ACTOR + 0x50, r.slot);
    m.wr8(ACTOR + 0x8A, u8::from(r.defers));
    m.wr32(ACTOR + 0x10, flags);
    m.wr16(ACTOR + 0x14, x as u16);
    m.wr16(ACTOR + 0x18, z as u16);
    m.wr16(ACTOR + 0x24, pitch as u16);
    m.wr16(ACTOR + 0x26, heading);
    m.wr16(ACTOR + 0x28, roll as u16);
    m.wr16(ACTOR + 0x62, clip);
    m.wr16(ACTOR + 0x72, 0x1000);
    m.wr32(ACTOR + 0x74, tint);
    m.wr16(ACTOR + 0x78, blend);
    m.wr8(ACTOR + 0x8C, tile.0);
    m.wr8(ACTOR + 0x8D, tile.1);
    m.wr8(FRAME_STEP, speed);
    m.wr32(GLOBALS, globals);
    m.wr32(MAILBOX, 0xFF);
    m.wr16(SCENE_BANK, banks.0);
    m.wr16(SPECIAL_BANK, banks.1);
    m.wr32(FAULT_CODE, 0);
    // The scene loader seeds every record unset (`0x8003B744..0x8003B750`).
    m.wr32(RECORDS + u32::from(r.slot) * 4, 0x8C8C);
    m.wr32(PLAYER_PTR, PLAYER);
    m.wr32(PLAYER + 0x10, 0);
    for i in 0..0x80 {
        m.wr8(FLAGS + i, 0);
    }
    for i in 0..4 {
        m.wr16(SFX_RING + i * 2, 0);
    }
    let gp = cpu.regs.r[psx_retail_exec::GP];
    cpu.mem.wr16(gp + 0x158, 0);
    seed.set(rand0);
    *calls.borrow_mut() = Calls::default();

    port.actor_flags = flags;
    port.clip_control = clip;
    port.globals = globals;
    port.rng = rand0;
    port.tint = tint;
    port.blend = blend;
    port.pitch = pitch;
    port.roll = roll;
    let mut chan = FieldNpcAmbient {
        variants: r
            .variants
            .iter()
            .map(|(_, sel, code)| (*sel, code.clone()))
            .collect(),
        live: None,
        vm: port,
        walks: false,
        defers: r.defers,
    };
    let mut flag_bank = [0u8; 0x80];
    let mut y = 0u16;
    let mut faulted = false;
    let mut moved = 0;
    let mut engaged = false;

    for tick in 0..TICKS {
        let at = format!("round {round} tick {tick} speed {speed} stream {stream:02x?}");
        let port = &mut chan.vm;
        // Now and then the player walks into this actor or another one: the
        // collision probe posts the id it touched to the mailbox.
        match rng.below(12) {
            0 | 1 => {
                cpu.mem.wr32(MAILBOX, u32::from(r.slot));
                port.pending_touch = Some(u32::from(r.slot));
            }
            2 => {
                cpu.mem.wr32(MAILBOX, u32::from(r.slot) + 1);
                port.pending_touch = Some(u32::from(r.slot) + 1);
            }
            _ => {}
        }
        // Something else flips a bit a `0x12` may be waiting on.
        if rng.below(4) == 0 {
            let bit = 1u32 << rng.below(32);
            match rng.below(3) {
                0 if r.churn || bit & 0x500 == 0 => port.actor_flags ^= bit,
                1 => port.clip_control ^= bit as u16,
                _ => port.globals ^= bit,
            }
            cpu.mem.wr32(ACTOR + 0x10, port.actor_flags);
            cpu.mem.wr16(ACTOR + 0x62, port.clip_control);
            cpu.mem.wr32(GLOBALS, port.globals);
        }
        if r.churn {
            if rng.below(4) == 0 {
                let gated: Vec<u16> = r
                    .variants
                    .iter()
                    .filter(|v| v.1 != 0xFFFF)
                    .map(|v| v.1)
                    .collect();
                if !gated.is_empty() {
                    let f = gated[rng.below(gated.len() as u32) as usize];
                    flag_bank[usize::from(f >> 3)] ^= 0x80 >> (f & 7);
                    cpu.mem
                        .wr8(FLAGS + u32::from(f >> 3), flag_bank[usize::from(f >> 3)]);
                }
            }
            if rng.below(6) == 0 {
                engaged = !engaged;
                let w = if engaged { 0x80000 } else { 0 } | (rng.next() & !0x80000);
                cpu.mem.wr32(PLAYER + 0x10, w);
            }
        }
        cpu.mem.wr16(0, 0);

        assert_eq!(cpu.call(ENTRY, &[ACTOR]), Stop::Returned, "{at}");
        assert_eq!(cpu.call(POOL_TICK, &[]), Stop::Returned, "{at}");

        let port = &mut chan.vm;
        let defer = FieldNpcAmbient::defer_gate(
            r.defers,
            engaged,
            port.actor_flags,
            Some((port.x, port.z)),
        );
        port.effects.clear();
        port.moved = false;
        if !defer {
            let pick = chan
                .select_variant(|f| flag_bank[usize::from(f >> 3)] & (0x80 >> (f & 7)) != 0)
                .expect("a default variant");
            if chan.live != Some(pick) {
                chan.live = Some(pick);
                chan.vm.rebind_variant();
            }
            let code = &chan.variants[pick].1;
            chan.vm.step_ops_with(code, speed, &Probes);
        }
        let port = &mut chan.vm;
        let mut models = Vec::new();
        let mut blits = Vec::new();
        for e in &port.effects {
            match *e {
                AmbientEffect::SystemFlagSet(i) => {
                    flag_bank[usize::from(i >> 3)] |= 0x80 >> (i & 7)
                }
                AmbientEffect::SystemFlagClear(i) => {
                    flag_bank[usize::from(i >> 3)] &= !(0x80 >> (i & 7))
                }
                AmbientEffect::ModelSwap { bank, offset } => {
                    let base = match bank {
                        ModelBank::Scene => banks.0,
                        ModelBank::Special => banks.1,
                    };
                    models.push(base.wrapping_add(offset as u16));
                }
                AmbientEffect::Teleport {
                    x,
                    z,
                    sample_ground,
                } => {
                    if sample_ground {
                        y = ground(x, z);
                    }
                }
                AmbientEffect::MoveImage { rect, dx, dy } => blits.push((rect, dx, dy)),
                AmbientEffect::BitTargetFault => faulted = true,
                _ => {}
            }
        }
        // The pool kills the ramps of an actor whose `+0x10 & 8` is up.
        if port.actor_flags & 8 != 0 {
            port.ramps.free_owner(port.owner);
        }
        port.tick_ramps(speed);

        let base = chan.live.map_or(0, |i| 4 + r.variants[i].0 as u16);
        let selector = chan.live.map_or(0, |i| r.variants[i].1 & 0xFFF);
        let port = &chan.vm;
        let rec = RECORDS + u32::from(r.slot) * 4;
        let m = &mut cpu.mem;
        let retail = (
            m.rd16(ACTOR + 0x14) as i16,
            m.rd16(ACTOR + 0x18) as i16,
            m.rd16(ACTOR + 0x26),
            u16::from(m.rd8(ACTOR + 0x8B)) | u16::from(m.rd8(ACTOR + 0xB7)) << 8,
            m.rd16(ACTOR + 0x84).wrapping_sub(base),
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
            "{at}\n(x, z, heading, cursor, pc, default_move, wander_phase, wander_dir, flags, clip, globals)"
        );
        assert_eq!(m.rd16(ACTOR + 0x86) & 0xFFF, selector, "{at} live variant");
        let retail = (
            m.rd16(ACTOR + 0x24) as i16,
            m.rd16(ACTOR + 0x28) as i16,
            m.rd16(ACTOR + 0x72),
            m.rd32(ACTOR + 0x74),
            m.rd16(ACTOR + 0x78),
            m.rd16(ACTOR + 0x16),
            (m.rd8(ACTOR + 0x8C), m.rd8(ACTOR + 0x8D)),
            m.rd32(FAULT_CODE),
        );
        let ported = (
            port.pitch,
            port.roll,
            port.scale.unwrap_or(0x1000),
            port.tint,
            port.blend,
            y,
            port.home_tile,
            if faulted { 0x3039 } else { 0 },
        );
        assert_eq!(
            retail, ported,
            "{at}\n(pitch, roll, scale, tint, blend, y, home tile, fault code)"
        );
        let busy = (1..64)
            .filter(|k| m.rd16(POOL + k * 0x20 + 0x1E) != 0)
            .count();
        assert_eq!(
            (busy, m.rd32(OVERFLOW)),
            (port.ramps.active(), port.ramps.overflow),
            "{at} pool"
        );
        let retail_move = (m.rd16(ACTOR + 0x88), m.rd16(ACTOR + 0x5C));
        let pair = port.move_pair.unwrap_or(0) as u16;
        assert_eq!(retail_move, (pair, pair), "{at} move pair");
        assert_eq!(
            port.requested_move.map_or(0, u16::from),
            retail_move.0 & 0xFF,
            "{at} requested move"
        );
        let mailbox = m.rd32(MAILBOX);
        assert_eq!(
            (mailbox != 0xFF).then_some(mailbox),
            port.pending_touch,
            "{at} touch mailbox"
        );
        let bank = m.read(FLAGS, 0x80);
        assert_eq!(bank, flag_bank, "{at} flag bank");
        let ring: Vec<i16> = (0..4).map(|i| m.rd16(SFX_RING + i * 2) as i16).collect();
        assert_eq!(ring, port.event_ring, "{at} SFX ring");
        let mut c = calls.borrow_mut();
        assert_eq!(c.models, models, "{at} model reloads");
        assert_eq!(c.blits, blits, "{at} MoveImage");
        *c = Calls::default();
        moved += i32::from(port.moved);
    }
    moved
}

fn start(rng: &mut Rng) -> (i16, i16) {
    (
        (0x400 + rng.below(0x3000)) as i16,
        (0x400 + rng.below(0x3000)) as i16,
    )
}

/// Every op of the table on a lone `0xFFFF` variant, the ramp pool ticking
/// after the VM: positions, heading, cursor, PC, the default-move record and
/// wander state, the flag words, the angles, scale, tint and draw mode, the
/// sampled height, the anchor tile, the fault code, the pool's busy count and
/// overflow counter, the move pair, the mailbox, the flag bank, the SFX ring
/// and the model-reload and `MoveImage` calls, after every tick.
#[test]
fn the_ambient_vm_runs_its_streams_as_retail_does() {
    differential!(scus(ENTRY));
    let Some((mut cpu, seed, calls)) = setup() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x414D_4249);
    let mut moved = 0;
    for round in 0..8000 {
        let (x, z) = start(&mut rng);
        let tile = (((x >> 7) & 0x7F) as u8, ((z >> 7) & 0x7F) as u8);
        let r = Round {
            variants: vec![(0, 0xFFFF, gen_code(&mut rng, tile))],
            defers: false,
            churn: false,
            slot: SLOT,
            at: (x, z),
        };
        moved += run_round(&mut cpu, &seed, &calls, &mut rng, round, &r);
    }
    assert!(moved > 2000, "{moved}");
}

/// The preamble over a table of flag-gated variants ahead of the default,
/// with the flags, the player's engaged bit and the actor's own `0x500` bits
/// churning between ticks and some actors on the off-map park: which variant
/// runs, the switch that restarts it and resets the actor's record, and the
/// ticks the `+0x8A` gate skips.
#[test]
fn the_ambient_preamble_picks_and_defers_as_retail_does() {
    differential!(scus(ENTRY));
    let Some((mut cpu, seed, calls)) = setup() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x5052_4541);
    for round in 0..3000 {
        let (mut x, mut z) = start(&mut rng);
        match rng.below(8) {
            0 => {
                x = (0x3F81 + rng.below(0x60)) as i16;
                z = (0x3F81 + rng.below(0x60)) as i16;
            }
            1 => x = (0x3F81 + rng.below(0x60)) as i16,
            _ => {}
        }
        let tile = (((x >> 7) & 0x7F) as u8, ((z >> 7) & 0x7F) as u8);
        let mut variants = Vec::new();
        let mut at = 0;
        let mut used = Vec::new();
        for _ in 0..rng.below(4) {
            // A flag inside the bank; never `0`, which a fresh actor's `+0x86`
            // already names.
            let sel = 1 + rng.below(0x3FE) as u16;
            if used.contains(&sel) {
                continue;
            }
            used.push(sel);
            let code = gen_code(&mut rng, tile);
            let len = 4 + code.len();
            variants.push((at, sel, code));
            at += len;
        }
        variants.push((at, 0xFFFF, gen_code(&mut rng, tile)));
        let r = Round {
            variants,
            defers: rng.below(3) != 0,
            churn: true,
            slot: SLOT,
            at: (x, z),
        };
        run_round(&mut cpu, &seed, &calls, &mut rng, round, &r);
    }
}

/// Op `0x17` on an actor id past the `0x8C`-entry record table: retail's
/// arena guard leaves the bytes past the table (the ramp pool's) alone, and
/// the port leaves the default move unset.
#[test]
fn the_default_move_write_stays_inside_the_record_table() {
    differential!(scus(ENTRY));
    let Some((mut cpu, _seed, _calls)) = setup() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x4152_454E);
    for slot in 0x8Cu16..0xF8 {
        let (a, b) = (rng.next() as u8, rng.next() as u8);
        let stream = [0xFF, 0xFF, 0, 0, 0x17, a, b, 0x05, 0x10, 0x01];
        for o in (0..0x100).step_by(4) {
            cpu.mem.wr32(ACTOR + o, 0);
        }
        cpu.mem.load(STREAM, &stream);
        cpu.mem.wr32(ACTOR + 0x80, STREAM);
        cpu.mem.wr16(ACTOR + 0x50, slot);
        cpu.mem.wr8(FRAME_STEP, 1);
        cpu.mem.wr32(MAILBOX, 0xFF);
        let rec = RECORDS + u32::from(slot) * 4;
        // The preamble's own variant switch stamps `[0x8C, 0x8C, 0, 0]`
        // first; the op must leave that standing.
        assert_eq!(cpu.call(ENTRY, &[ACTOR]), Stop::Returned);
        let mut port = AmbientMotion::new(u32::from(slot), 0);
        port.rebind_variant();
        port.step_ops_with(&stream[4..], 1, &Probes);
        assert_eq!(
            [cpu.mem.rd8(rec), cpu.mem.rd8(rec + 1)],
            port.default_move,
            "slot {slot:#x}"
        );
        assert_eq!(cpu.mem.rd16(ACTOR + 0x84) - 4, port.pc, "slot {slot:#x}");
    }
}
