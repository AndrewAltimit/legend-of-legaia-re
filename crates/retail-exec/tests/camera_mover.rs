//! The op-`0x45` camera mover's tick `FUN_801DC0BC` (PROT 0897) off the disc,
//! frame after frame from arming to arrival, against `CameraMover::tick` and
//! `store_frame`, the stores the field camera's `tick_globals` makes: every
//! curve, random spans, durations and frame steps, the shake amplitude at
//! rest and raised with `rand()` answered by the world stream on both sides.
//! The ten globals and the jitter pair are compared whole.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_camera::camera_mover::{AXIS_COUNT, CameraMover};
use legaia_engine_system::rand::world_rand;
use psx_retail_exec::{A0, Memory, Stop, V0};
use std::cell::Cell;
use std::rc::Rc;

const TICK: u32 = 0x801D_C0BC;
const FREE: u32 = 0x8001_7B94;
const RAND: u32 = 0x8005_6798;
const OVERLAY_BASE: u32 = 0x801C_E818;
const CAM_CTX_PTR: u32 = 0x801C_6EA4;
const CAM_CTX: u32 = 0x8013_0000;
const AMPLITUDE: u32 = 0x8007_B630;
const FRAME_STEP: u32 = 0x1F80_0393;
const ANGLES: u32 = 0x8007_B790;
const EYE: u32 = 0x8008_40B8;
const FOCUS: u32 = 0x8008_9118;
const GTE_H: u32 = 0x8007_B6F4;
const ACTOR: u32 = 0x8010_0000;
const BLOCK: u32 = 0x8010_0400;

/// The ten globals in the mover's axis order, each as the word a reader
/// sees: the halfword ones sign-extended.
fn read_globals(m: &mut Memory) -> [i32; AXIS_COUNT] {
    let mut g = [0; AXIS_COUNT];
    for k in 0..3 {
        g[k] = i32::from(m.rd16(ANGLES + 2 * k as u32) as i16);
        g[3 + k] = m.rd32(EYE + 4 * k as u32) as i32;
        g[6 + k] = m.rd32(FOCUS + 4 * k as u32) as i32;
    }
    g[9] = i32::from(m.rd16(GTE_H) as i16);
    g
}

fn write_globals(m: &mut Memory, g: &[i32; AXIS_COUNT]) {
    for k in 0..3 {
        m.wr16(ANGLES + 2 * k as u32, g[k] as u16);
        m.wr32(EYE + 4 * k as u32, g[3 + k] as u32);
        m.wr32(FOCUS + 4 * k as u32, g[6 + k] as u32);
    }
    m.wr16(GTE_H, g[9] as u16);
}

/// What the port leaves in the globals for a mover pose: the halfword
/// globals keep the low half.
fn as_globals(v: [i32; AXIS_COUNT]) -> [i32; AXIS_COUNT] {
    let mut g = v;
    for k in [0, 1, 2, 9] {
        g[k] = i32::from(v[k] as i16);
    }
    g
}

#[test]
fn the_mover_tick_matches_the_glide() {
    differential!(prot(897, TICK));
    let (Some(mut cpu), Some(disc)) = (scus_cpu(), disc()) else {
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &disc.read_entry(897).expect("PROT 0897"));
    cpu.mem.wr32(CAM_CTX_PTR, CAM_CTX);
    let freed = Rc::new(Cell::new(0u32));
    {
        let freed = freed.clone();
        cpu.hook(FREE, move |r, _| freed.set(r.r[A0]));
    }
    let seed = Rc::new(Cell::new(0x0BAD_5EEDu32));
    {
        let seed = seed.clone();
        cpu.hook(RAND, move |r, _| {
            let mut s = seed.get();
            r.r[V0] = world_rand(&mut s);
            seed.set(s);
        });
    }
    let mut port_seed = seed.get();
    let mut shaken = 0usize;
    let mut rng = Rng(0x4D4F_5645);
    let (mut frames, mut arrivals) = (0usize, 0usize);
    let mut curves = [0usize; 5];
    for glide in 0..3000 {
        let curve = rng.below(6) as u16;
        let d = 2 + rng.below(if glide % 4 == 0 { 0x800 } else { 0x80 }) as u16;
        let mut pairs = [(0i16, 0i16); AXIS_COUNT];
        for (k, p) in pairs.iter_mut().enumerate() {
            let s = (rng.below(0x4000) as i32 - 0x2000) as i16;
            // A held axis now and then: start equals end.
            let e = if rng.below(6) == 0 {
                s
            } else {
                (rng.below(0x4000) as i32 - 0x2000) as i16
            };
            *p = (s, e);
            cpu.mem.wr16(BLOCK + 4 * k as u32, s as u16);
            cpu.mem.wr16(BLOCK + 4 * k as u32 + 2, e as u16);
        }
        // The live globals the beat started from: the block's starts, as
        // the arming routine read them.
        let mut live = [0i32; AXIS_COUNT];
        for k in 0..AXIS_COUNT {
            live[k] = i32::from(pairs[k].0);
        }
        write_globals(&mut cpu.mem, &live);
        let mut offset = [rng.below(0x40) as i32 - 0x20, -(rng.below(0x20) as i32)];
        cpu.mem.wr32(CAM_CTX + 0x18, offset[0] as u32);
        cpu.mem.wr32(CAM_CTX + 0x1C, offset[1] as u32);
        cpu.mem.wr16(ACTOR + 0x9C, 0);
        cpu.mem.wr16(ACTOR + 0x9E, d);
        cpu.mem.wr16(ACTOR + 0x50, curve);
        cpu.mem.wr32(ACTOR + 0x90, BLOCK);
        cpu.mem.wr32(ACTOR + 0x10, 0);
        freed.set(0);

        let mut mover = CameraMover::default();
        mover.arm(
            pairs.map(|p| i32::from(p.0)),
            pairs.map(|p| i32::from(p.1)),
            d,
            curve as u8,
        );
        for frame in 0.. {
            let step = 1 + rng.below(4) as u8;
            cpu.mem.wr8(FRAME_STEP, step);
            let amplitude = if glide % 2 == 0 { 0 } else { rng.below(0x16) };
            cpu.mem.wr32(AMPLITUDE, amplitude);
            assert_eq!(cpu.call(TICK, &[ACTOR]), Stop::Returned);
            let arrived = mover.tick(i32::from(step));
            let mut port = live;
            mover.store_frame(&mut port, &mut offset, amplitude, &mut port_seed);
            live = port;
            let want = as_globals(port);
            let ctx = format!(
                "glide {glide} frame {frame} curve {curve} d {d} t {}",
                mover.t
            );
            assert_eq!(read_globals(&mut cpu.mem), want, "globals, {ctx}");
            assert_eq!(
                cpu.mem.rd16(ACTOR + 0x9C) as i16 as i32,
                mover.t,
                "progress, {ctx}"
            );
            let got = [0, 4].map(|k| cpu.mem.rd32(CAM_CTX + 0x18 + k) as i32);
            assert_eq!(got, offset, "jitter pair, {ctx}");
            assert_eq!(seed.get(), port_seed, "rand stream, {ctx}");
            shaken += usize::from(amplitude != 0);
            let dead = cpu.mem.rd32(ACTOR + 0x10) & 8 != 0;
            assert_eq!(dead, arrived, "arrival, {ctx}");
            frames += 1;
            if arrived {
                assert_eq!(freed.get(), BLOCK, "the pair block is freed, {ctx}");
                arrivals += 1;
                break;
            }
        }
        curves[usize::from(curve.min(4))] += 1;
    }
    assert!(
        arrivals == 3000 && frames > 20000 && shaken > 0 && curves.iter().all(|c| *c > 0),
        "arrivals {arrivals} frames {frames} shaken {shaken} curves {curves:?}"
    );
}
