//! The battle camera's tween builder and the field camera's shake off the
//! disc. `FUN_801D829C` (PROT 0898), with the glide spawner it hands the table
//! to stubbed, against `build_camera_angle_tween` over random globals, targets
//! and durations: the nine `{step, endpoint}` records at battle-context
//! `+0x118C`, the three target trios it rewrites and the rotation globals it
//! normalises in place. `FUN_801D9D30` (PROT 0897), `rand()` answered by the
//! world stream on both sides, against `apply_shake` at every amplitude: the
//! accumulator pair and the context's jitter pair, frame after frame.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_camera::battle_camera::{CameraAngles, apply_shake, build_camera_angle_tween};
use legaia_engine_system::rand::world_rand;
use psx_retail_exec::{Memory, Stop, V0};
use std::cell::Cell;
use std::rc::Rc;

const TWEEN: u32 = 0x801D_829C;
const SHAKE: u32 = 0x801D_9D30;
const GLIDE_SPAWN: u32 = 0x8002_1248;
const RAND: u32 = 0x8005_6798;
const OVERLAY_BASE: u32 = 0x801C_E818;
const CTX_PTR: u32 = 0x8007_BD24;
const CTX: u32 = 0x8012_0000;
const TABLE: u32 = CTX + 0x118C;
const ANGLES: u32 = 0x8007_B790;
const EYE: u32 = 0x8008_40B8;
const FOCUS: u32 = 0x8008_9118;
const TARGETS: [u32; 3] = [0x8010_0000, 0x8010_0010, 0x8010_0020];
const CAM_CTX_PTR: u32 = 0x801C_6EA4;
const CAM_CTX: u32 = 0x8013_0000;
const AMPLITUDE: u32 = 0x8007_B630;

fn trio(m: &mut Memory, a: u32, stride: u32) -> [i16; 3] {
    std::array::from_fn(|k| m.rd16(a + stride * k as u32) as i16)
}

#[test]
fn the_tween_table_matches_the_builder() {
    differential!(prot(898, TWEEN));
    let (Some(mut cpu), Some(disc)) = (scus_cpu(), disc()) else {
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &disc.read_entry(898).expect("PROT 0898"));
    cpu.mem.wr32(CTX_PTR, CTX);
    let spawned = Rc::new(Cell::new(0u32));
    {
        let spawned = spawned.clone();
        cpu.hook(GLIDE_SPAWN, move |r, _| {
            spawned.set(r.r[psx_retail_exec::A0])
        });
    }
    let mut rng = Rng(0x5457_4545);
    let (mut wrapped_up, mut wrapped_down, mut zero_frames) = (0, 0, 0);
    for round in 0..20000 {
        let wide = round % 4 == 0;
        let half = |rng: &mut Rng| {
            if wide {
                rng.next() as i16
            } else {
                (rng.below(0x2000) as i32 - 0x1000) as i16
            }
        };
        // The globals: three halfword angles, then two trios of words whose
        // low halfwords the builder reads.
        let mut current = CameraAngles::default();
        let mut words = [0u32; 6];
        for k in 0..3 {
            current.rotation[k] = rng.next() as i16;
            cpu.mem
                .wr16(ANGLES + 2 * k as u32, current.rotation[k] as u16);
            words[k] = (rng.next() & 0xFFFF_0000) | u32::from(half(&mut rng) as u16);
            words[3 + k] = (rng.next() & 0xFFFF_0000) | u32::from(half(&mut rng) as u16);
            current.shake[k] = words[k] as i16;
            current.focus[k] = words[3 + k] as i16;
            cpu.mem.wr32(EYE + 4 * k as u32, words[k]);
            cpu.mem.wr32(FOCUS + 4 * k as u32, words[3 + k]);
        }
        let mut target = CameraAngles::default();
        for k in 0..3 {
            target.rotation[k] = rng.next() as i16;
            target.shake[k] = half(&mut rng);
            target.focus[k] = half(&mut rng);
        }
        let trios = [target.rotation, target.shake, target.focus];
        for (a, t) in TARGETS.iter().zip(trios) {
            for (k, v) in t.iter().enumerate() {
                cpu.mem.wr16(a + 2 * k as u32, *v as u16);
            }
        }
        let frames = match round % 8 {
            0 => 0,
            1 => 1,
            _ => 1 + rng.below(0x40),
        };
        cpu.mem.load(TABLE, &[0xA5; 0x30]);
        spawned.set(0);
        let stop = cpu.call(TWEEN, &[TARGETS[0], TARGETS[1], TARGETS[2], frames]);
        assert_eq!(stop, Stop::Returned, "round {round}");
        assert_eq!(spawned.get(), TABLE, "the table goes to the glide spawner");

        let table = build_camera_angle_tween(&mut current, &mut target, frames as u16);
        let ctx = format!("round {round} frames {frames}");
        let mut want = Vec::new();
        for s in &table {
            want.extend(s.step.to_le_bytes());
            want.extend(s.target.to_le_bytes());
        }
        want.extend([0xA5; 0x30 - 36]);
        assert_eq!(cpu.mem.read(TABLE, 0x30), want, "table, {ctx}");
        assert_eq!(
            trio(&mut cpu.mem, TARGETS[0], 2),
            target.rotation,
            "rotation targets, {ctx}"
        );
        assert_eq!(
            trio(&mut cpu.mem, TARGETS[1], 2),
            target.shake,
            "shake targets, {ctx}"
        );
        assert_eq!(
            trio(&mut cpu.mem, TARGETS[2], 2),
            target.focus,
            "focus targets, {ctx}"
        );
        assert_eq!(
            trio(&mut cpu.mem, ANGLES, 2),
            current.rotation,
            "angle globals, {ctx}"
        );
        for k in 0..3 {
            assert_eq!(
                cpu.mem.rd32(EYE + 4 * k as u32),
                words[k],
                "eye word {k}, {ctx}"
            );
            assert_eq!(
                cpu.mem.rd32(FOCUS + 4 * k as u32),
                words[3 + k],
                "focus word {k}, {ctx}"
            );
        }
        wrapped_up += usize::from(target.rotation.iter().any(|v| *v >= 0x1000));
        wrapped_down += usize::from(current.rotation.iter().any(|v| *v >= 0x1000));
        zero_frames += usize::from(frames == 0);
    }
    assert!(
        wrapped_up > 0 && wrapped_down > 0 && zero_frames > 0,
        "up {wrapped_up} down {wrapped_down} zero {zero_frames}"
    );
}

#[test]
fn the_shake_matches_the_jitter() {
    differential!(prot(897, SHAKE));
    let (Some(mut cpu), Some(disc)) = (scus_cpu(), disc()) else {
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &disc.read_entry(897).expect("PROT 0897"));
    cpu.mem.wr32(CAM_CTX_PTR, CAM_CTX);
    let seed = Rc::new(Cell::new(0x1234_5678u32));
    {
        let seed = seed.clone();
        cpu.hook(RAND, move |r, _| {
            let mut s = seed.get();
            r.r[V0] = world_rand(&mut s);
            seed.set(s);
        });
    }
    let mut rng = Rng(0x5348_414B);
    let mut port_seed = seed.get();
    let (mut drawn, mut rest) = (0, 0);
    for run in 0..400 {
        let mut accum = [0, 1].map(|_| rng.below(0x10_0000) as i32 - 0x8_0000);
        let mut offset = [rng.below(0x100) as i32 - 0x80, -(rng.below(0x80) as i32)];
        for k in 0..2 {
            cpu.mem.wr32(EYE + 4 * k as u32, accum[k] as u32);
            cpu.mem
                .wr32(CAM_CTX + 0x18 + 4 * k as u32, offset[k] as u32);
        }
        for frame in 0..24 {
            // Mostly the amplitudes the op stages; now and then a word past
            // `0x15`, where the shift wraps through its five bits.
            let amplitude = match rng.below(10) {
                0 => 0,
                1 => rng.next(),
                _ => rng.below(0x16),
            };
            cpu.mem.wr32(AMPLITUDE, amplitude);
            assert_eq!(cpu.call(SHAKE, &[]), Stop::Returned);
            apply_shake(&mut accum, &mut offset, amplitude, &mut port_seed);
            let ctx = format!("run {run} frame {frame} amplitude {amplitude:#x}");
            for k in 0..2 {
                assert_eq!(
                    cpu.mem.rd32(EYE + 4 * k as u32) as i32,
                    accum[k],
                    "accum {k}, {ctx}"
                );
                assert_eq!(
                    cpu.mem.rd32(CAM_CTX + 0x18 + 4 * k as u32) as i32,
                    offset[k],
                    "offset {k}, {ctx}"
                );
            }
            assert_eq!(seed.get(), port_seed, "rand stream, {ctx}");
            drawn += usize::from(amplitude != 0);
            rest += usize::from(amplitude == 0);
        }
    }
    assert!(drawn > 0 && rest > 0);
}
