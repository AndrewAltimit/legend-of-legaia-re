//! The camera-relative glide family off the disc: the spawner and normalizer
//! `FUN_80021248` against `normalize_camera_relative_params` and
//! `spawn_handshake`, its tick `FUN_8002149C` against `CameraRelGlide::tick`
//! and `finish_handshake`, frame after frame to retirement, and the field-entry
//! camera reset `FUN_80025C24` against `Camera::reset_globals_for_scene_entry`.
//! Random records, camera globals, frame steps and scratch words.

use crate::common::{Rng, scus_cpu};
use legaia_engine_camera::camera_rel_actor::{
    CameraSnapshot, normalize_camera_relative_params, spawn_handshake,
};
use legaia_engine_camera::camera_rel_glide::{CameraRelGlide, finish_handshake};
use legaia_engine_camera::retail_cam::RetailCamGlobals;
use psx_retail_exec::{Memory, Stop};

const SPAWN: u32 = 0x8002_1248;
const TICK: u32 = 0x8002_149C;
const RESET: u32 = 0x8002_5C24;
/// The actor allocator the spawner calls; hooked to hand back [`ACTOR`].
const ALLOC: u32 = 0x8002_0DE0;
const ACTOR: u32 = 0x8010_0000;
const RECORD: u32 = 0x8010_0400;
const PREV: u32 = 0x8010_0800;
/// `gp + 0x750`, the family's last actor.
const LAST_ACTOR: u32 = 0x8007_B318 + 0x750;
const SCRATCH: u32 = 0x1F80_0394;
const FRAME_STEP: u32 = 0x1F80_0393;
const ANGLES: u32 = 0x8007_B790;
const EYE: u32 = 0x8008_40B8;
const FOCUS: u32 = 0x8008_9118;
const GTE_H: u32 = 0x8007_B6F4;

fn write_globals(mem: &mut Memory, g: &RetailCamGlobals) {
    for i in 0..3 {
        mem.wr16(ANGLES + 2 * i as u32, g.0[i] as u16);
        mem.wr32(EYE + 4 * i as u32, g.0[3 + i] as u32);
        mem.wr32(FOCUS + 4 * i as u32, g.0[6 + i] as u32);
    }
    mem.wr16(GTE_H, g.0[9] as u16);
}

/// The globals as retail left them, the halfword ones sign-extended as the
/// port's mirror keeps them.
fn read_globals(mem: &mut Memory) -> RetailCamGlobals {
    let mut g = RetailCamGlobals([0; 10]);
    for i in 0..3 {
        g.0[i] = i32::from(mem.rd16(ANGLES + 2 * i as u32) as i16);
        g.0[3 + i] = mem.rd32(EYE + 4 * i as u32) as i32;
        g.0[6 + i] = mem.rd32(FOCUS + 4 * i as u32) as i32;
    }
    g.0[9] = i32::from(mem.rd16(GTE_H) as i16);
    g
}

fn random_globals(rng: &mut Rng) -> RetailCamGlobals {
    let mut g = RetailCamGlobals([0; 10]);
    for i in 0..3 {
        g.0[i] = i32::from(rng.next() as i16);
        g.0[3 + i] = rng.below(0x8000) as i32 - 0x4000;
        g.0[6 + i] = rng.below(0x8000) as i32 - 0x4000;
    }
    g.0[9] = 0x100 + rng.below(0x400) as i32;
    g
}

/// A record whose endpoints sit near the camera, so the glides arrive.
fn random_record(rng: &mut Rng, g: &RetailCamGlobals) -> [i16; 20] {
    let mut r = [0i16; 20];
    for i in 0..10 {
        r[2 * i] = match rng.below(6) {
            0 => 0,
            1 => rng.next() as i16,
            _ => rng.below(0x80) as i16 - 0x40,
        };
        r[2 * i + 1] = if rng.below(4) == 0 {
            rng.next() as i16
        } else {
            (g.0[i] + rng.below(0x400) as i32 - 0x200) as i16
        };
    }
    r
}

#[test]
fn the_camera_relative_glide_matches_its_spawner_and_tick() {
    differential!(scus(SPAWN), scus(TICK));
    let Some(mut cpu) = scus_cpu() else {
        return;
    };
    cpu.hook(ALLOC, |r, _| r.r[psx_retail_exec::V0] = ACTOR);
    let mut rng = Rng(0x4341_4D52);
    let (mut finished, mut flagged, mut folded) = (0, 0, 0);
    for round in 0..600 {
        let start = random_globals(&mut rng);
        let record = random_record(&mut rng, &start);
        cpu.mem.load(ACTOR, &[0u8; 0x100]);
        for (k, h) in record.iter().enumerate() {
            cpu.mem.wr16(RECORD + 2 * k as u32, *h as u16);
        }
        // Seat halfwords 13 / 15 / 17 land on the actor's position.
        write_globals(&mut cpu.mem, &start);
        let scratch = rng.next() & 0x0000_03FF;
        cpu.mem.wr32(SCRATCH, scratch);
        cpu.mem.wr32(LAST_ACTOR, PREV);
        cpu.mem.wr32(PREV + 0x10, 0x20);
        assert_eq!(cpu.call(SPAWN, &[RECORD]), Stop::Returned);

        let cam = CameraSnapshot {
            angles: [0, 1, 2].map(|i| start.0[i] as u16),
            offsets: [start.0[3], start.0[4], start.0[5]],
            focus: [start.0[6], start.0[7], start.0[8]],
            gte_h: start.0[9] as i16,
        };
        let port = normalize_camera_relative_params(&record, &cam);
        let ctx = format!("round {round} record {record:?} cam {cam:?}");
        let params: Vec<i16> = (0..20)
            .map(|k| cpu.mem.rd16(ACTOR + 0x80 + 2 * k) as i16)
            .collect();
        assert_eq!(params, port.params.to_vec(), "params, {ctx}");
        let rot: Vec<i16> = (0..3)
            .map(|k| cpu.mem.rd16(ACTOR + 0x24 + 2 * k) as i16)
            .collect();
        assert_eq!(rot, port.rotation.to_vec(), "rotation, {ctx}");
        for (k, h) in [13usize, 15, 17].iter().enumerate() {
            assert_eq!(
                cpu.mem.rd16(ACTOR + 0x14 + 2 * k as u32),
                record[*h] as u16,
                "seat {k}, {ctx}"
            );
        }
        assert_eq!(cpu.mem.rd32(ACTOR + 0x4C), RECORD, "record link, {ctx}");
        assert_eq!(cpu.mem.rd32(LAST_ACTOR), ACTOR, "last actor, {ctx}");
        let hs = spawn_handshake(scratch);
        assert_eq!(cpu.mem.rd32(SCRATCH), hs.scratch, "scratch, {ctx}");
        let prev_flags = cpu.mem.rd32(PREV + 0x10);
        assert_eq!(
            prev_flags & 8 != 0,
            hs.flag_previous,
            "previous kill, {ctx}"
        );
        flagged += usize::from(hs.flag_previous);
        folded += port.rotation.iter().filter(|r| **r >= 0x400).count();

        // Tick to retirement.
        let mut glide = CameraRelGlide::from_normalized(&port);
        let mut globals = start;
        let mut sc = cpu.mem.rd32(SCRATCH);
        for frame in 0..400 {
            let dt = 1 + rng.below(4) as u8;
            cpu.mem.wr8(FRAME_STEP, dt);
            assert_eq!(cpu.call(TICK, &[ACTOR]), Stop::Returned);
            let t = glide.tick(&mut globals, dt);
            let mut want = globals;
            for i in [0usize, 1, 2, 9] {
                want.0[i] = i32::from(want.0[i] as i16);
            }
            let fctx = format!("{ctx} frame {frame} dt {dt}");
            assert_eq!(read_globals(&mut cpu.mem).0, want.0, "globals, {fctx}");
            let budget: Vec<u16> = (0..3).map(|k| cpu.mem.rd16(ACTOR + 0x24 + 2 * k)).collect();
            assert_eq!(budget, glide.angle_budget.to_vec(), "budget, {fctx}");
            if t.finished {
                sc = finish_handshake(sc);
            }
            assert_eq!(cpu.mem.rd32(SCRATCH), sc, "scratch, {fctx}");
            assert_eq!(
                cpu.mem.rd32(ACTOR + 0x10) & 8 != 0,
                t.finished,
                "kill, {fctx}"
            );
            if t.finished {
                finished += 1;
                break;
            }
        }
    }
    assert!(
        finished > 300 && flagged > 100 && folded > 100,
        "finished {finished} flagged {flagged} folded {folded}"
    );
}

#[test]
fn the_field_entry_reset_matches_the_camera_reset() {
    differential!(scus(RESET));
    let Some(mut cpu) = scus_cpu() else {
        return;
    };
    let mut rng = Rng(0x5253_4554);
    for _ in 0..200 {
        let g = random_globals(&mut rng);
        write_globals(&mut cpu.mem, &g);
        assert_eq!(cpu.call(RESET, &[]), Stop::Returned);
        let mut cam = legaia_engine_field::camera::Camera::new();
        cam.globals = g;
        cam.reset_globals_for_scene_entry();
        let mut want = cam.globals;
        for i in [0usize, 1, 2, 9] {
            want.0[i] = i32::from(want.0[i] as i16);
        }
        assert_eq!(read_globals(&mut cpu.mem).0, want.0, "from {:?}", g.0);
    }
}
