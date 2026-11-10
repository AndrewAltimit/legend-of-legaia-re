//! Three per-frame field ticks of PROT 0897, run frame by frame against their
//! ports: the camera height ease `FUN_801DA390` against `ease_camera_offset`,
//! the floor-ladder oscillator `FUN_801DA930` against `FloorTierBob::step`
//! (with its burst arm), and the eased second-actor move `FUN_801DD4C4`
//! against `EasedMove::step`, every axis on and off.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_field::camera_ease::{CameraEaseInput, ease_camera_offset};
use legaia_engine_field::field_actor_timers::{EasedMove, FloorTierBob};
use psx_retail_exec::{Cpu, Stop};

const EASE: u32 = 0x801D_A390;
const LADDER: u32 = 0x801D_A930;
const MOVE: u32 = 0x801D_D4C4;
const OVERLAY_BASE: u32 = 0x801C_E818;
const FRAME_STEP: u32 = 0x1F80_0393;
const SCRATCH_FLAGS: u32 = 0x1F80_0394;
const LUT: u32 = 0x1F80_035C;
const SCENE_PTR: u32 = 0x801C_6EA4;
const SCENE: u32 = 0x8014_0000;
const PLAYER_PTR: u32 = 0x8007_C364;
const PAD_HELD: u32 = 0x8007_B850;
const CURRENT: u32 = 0x8007_BCAC;
const ACTOR: u32 = 0x8013_0000;
const TARGET: u32 = 0x8013_1000;

fn retail_cpu() -> Option<Cpu> {
    let mut cpu = scus_cpu()?;
    cpu.mem
        .load(OVERLAY_BASE, &disc()?.read_entry(897).expect("PROT 0897"));
    Some(cpu)
}

#[test]
fn the_camera_height_ease_matches_retail() {
    differential!(prot(897, EASE));
    let Some(mut cpu) = retail_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem.wr32(SCENE_PTR, SCENE);
    cpu.mem.wr32(PLAYER_PTR, ACTOR);
    let mut rng = Rng(0x4541_5345);
    let (mut locked, mut fast, mut settled, mut moved) = (0, 0, 0, 0);
    for run in 0..2000 {
        let mut current = [0x3C, 0, -40, 0x7FF0, rng.next() as i32 >> 8][rng.below(5) as usize];
        let scene_target = [0u16, 0x3C, 0x400, 0xFFC0, rng.next() as u16][rng.below(5) as usize];
        for frame in 0..40 {
            let footing = [0u16, 8, 0xFFF0, rng.next() as u16][rng.below(4) as usize];
            let input = CameraEaseInput {
                pad: [0, 0x0100_0000, 0x0002_0000, 0x0002_0000 | 0x10][rng.below(4) as usize]
                    | rng.next() & 0x00FC_FFFF,
                scene_target,
                player_footing: footing,
                footing_settled: if rng.below(2) == 0 {
                    footing as i16
                } else {
                    rng.next() as i16
                },
                z: rng.below(4) as i16,
                z_target: rng.below(4) as i16,
                fast_flags: if rng.below(2) == 0 { 0 } else { rng.next() },
                current,
            };
            cpu.mem.wr32(SCRATCH_FLAGS, input.pad);
            cpu.mem.wr16(SCENE + 0x4A, scene_target);
            cpu.mem.wr16(ACTOR + 0x16, footing);
            cpu.mem.wr16(ACTOR + 0x1E, input.footing_settled as u16);
            cpu.mem.wr16(ACTOR + 0x18, input.z as u16);
            cpu.mem.wr16(ACTOR + 0x20, input.z_target as u16);
            cpu.mem.wr32(PAD_HELD, input.fast_flags);
            cpu.mem.wr32(CURRENT, current as u32);
            assert_eq!(cpu.call(EASE, &[]), Stop::Returned);
            let port = ease_camera_offset(input);
            assert_eq!(
                cpu.mem.rd32(CURRENT) as i32,
                port,
                "run {run} frame {frame}: {input:?}"
            );
            if input.pad & 0x0100_0000 != 0 {
                locked += 1;
            } else if input.pad & 0x0002_0000 != 0 && input.fast_flags & 0xF000 != 0 {
                fast += 1;
            } else if input.footing_settled == footing as i16 && input.z == input.z_target {
                settled += 1;
            }
            moved += u32::from(port != current);
            current = port;
        }
    }
    assert!(
        locked > 5000 && fast > 3000 && settled > 3000 && moved > 20_000,
        "{locked} {fast} {settled} {moved}"
    );
}

#[test]
fn the_floor_ladder_oscillates_as_retail_does() {
    differential!(prot(897, LADDER));
    let Some(mut cpu) = retail_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x4C41_4444);
    let (mut bursts, mut steps) = (0, 0);
    for run in 0..500 {
        let slot = rng.below(16) as u16;
        let period = [1i16, 8, 30, 120, rng.next() as i16 & 0x3FF][rng.below(5) as usize];
        let amplitude = [0i16, 4, 32, -16, rng.next() as i16][rng.below(5) as usize];
        // A seed past `0` sends retail's phase to `2` and its outer loop
        // never exits (the module note); the port stops there instead.
        let mut bob = FloorTierBob::spawn(slot, 0, period, amplitude, 0, rng.next() as i16);
        bob.phase = rng.below(2) as i16;
        let lut: Vec<u8> = (0..32).map(|_| rng.next() as u8).collect();
        cpu.mem.load(LUT, &lut);
        let mut port_lut = lut.clone();
        for frame in 0..60 {
            let delta = [0u8, 1, 1, 2, 3][rng.below(5) as usize];
            if rng.below(20) == 0 {
                bob.arm = 0x8000 | [0u16, 1, 5, 40][rng.below(4) as usize];
                bursts += 1;
            } else if rng.below(10) == 0 {
                bob.arm = rng.below(0x8000) as u16;
            }
            cpu.mem.wr16(ACTOR + 0x50, bob.slot);
            cpu.mem.wr16(ACTOR + 0x54, bob.phase as u16);
            cpu.mem.wr8(ACTOR + 0x6C, bob.phase_seed);
            cpu.mem.wr32(ACTOR + 0x80, bob.pos as u32);
            cpu.mem.wr32(ACTOR + 0x84, bob.vel as u32);
            cpu.mem.wr32(ACTOR + 0x88, bob.accel as u32);
            cpu.mem.wr32(ACTOR + 0x8C, bob.target as u32);
            cpu.mem.wr16(ACTOR + 0x9E, bob.arm);
            cpu.mem.wr8(FRAME_STEP, delta);
            assert_eq!(
                cpu.call(LADDER, &[ACTOR]),
                Stop::Returned,
                "run {run} frame {frame}"
            );
            if let Some(h) = bob.step(delta) {
                let at = usize::from(bob.slot) * 2;
                port_lut[at..at + 2].copy_from_slice(&h.to_le_bytes());
                steps += 1;
            }
            let label = format!("run {run} frame {frame} delta {delta}");
            assert_eq!(cpu.mem.rd16(ACTOR + 0x54) as i16, bob.phase, "{label}");
            assert_eq!(cpu.mem.rd32(ACTOR + 0x80) as i32, bob.pos, "{label}");
            assert_eq!(cpu.mem.rd32(ACTOR + 0x84) as i32, bob.vel, "{label}");
            assert_eq!(cpu.mem.rd16(ACTOR + 0x9E), bob.arm, "{label}");
            assert_eq!(cpu.mem.read(LUT, 32), port_lut, "{label}");
        }
    }
    assert!(bursts > 1000 && steps > 15_000, "{bursts} {steps}");
}

#[test]
fn the_eased_move_lands_where_retail_lands_it() {
    differential!(prot(897, MOVE));
    let Some(mut cpu) = retail_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x4D4F_5645);
    let (mut eased, mut mirrored, mut arrived) = (0, 0, 0);
    for run in 0..3000 {
        let mut axis = || -> (i16, i16) {
            let start = rng.next() as i16 >> rng.below(8);
            let end = match rng.below(5) {
                0 => -1,
                1 => start,
                _ => rng.next() as i16 >> rng.below(8),
            };
            (start, end)
        };
        let (a, b, c) = (axis(), axis(), axis());
        let duration = [1i16, 2, 30, 0x200, 0, -3][rng.below(6) as usize];
        let mut m = EasedMove::spawn([a.0, b.0, c.0], [a.1, b.1, c.1], duration);
        let target_flags =
            rng.next() & !0x2000_0000 | if rng.below(2) == 0 { 0x2000_0000 } else { 0 };
        let flags = rng.next() & !8;
        cpu.mem.wr32(ACTOR + 0x10, flags);
        cpu.mem.wr16(ACTOR + 0x50, 0);
        cpu.mem.wr16(ACTOR + 0x9E, duration as u16);
        cpu.mem.wr32(ACTOR + 0x90, TARGET);
        for i in 0..3u32 {
            cpu.mem
                .wr16(ACTOR + 0x14 + i * 2, m.start[i as usize] as u16);
            cpu.mem.wr16(ACTOR + 0x24 + i * 2, m.end[i as usize] as u16);
        }
        cpu.mem.wr32(TARGET + 0x10, target_flags);
        let mut target = [0u16; 3];
        for (i, t) in target.iter_mut().enumerate() {
            *t = rng.next() as u16;
            cpu.mem.wr16(TARGET + 0x14 + i as u32 * 2, *t);
        }
        let mut mirror = rng.next() as u16;
        cpu.mem.wr16(TARGET + 0x8E, mirror);
        for frame in 0..80 {
            let delta = [0u8, 1, 1, 2, 4][rng.below(5) as usize];
            cpu.mem.wr8(FRAME_STEP, delta);
            assert_eq!(
                cpu.call(MOVE, &[ACTOR]),
                Stop::Returned,
                "run {run} frame {frame}"
            );
            let f = m.step(delta, target_flags);
            for (i, v) in f.axis.iter().enumerate() {
                if let Some(v) = v {
                    target[i] = *v as u16;
                    eased += 1;
                }
            }
            if let Some(y) = f.mirror_y {
                mirror = y as u16;
                mirrored += 1;
            }
            let label = format!("run {run} frame {frame}: {m:?}");
            for (i, t) in target.iter().enumerate() {
                assert_eq!(
                    cpu.mem.rd16(TARGET + 0x14 + i as u32 * 2),
                    *t,
                    "{label} axis {i}"
                );
            }
            assert_eq!(cpu.mem.rd16(TARGET + 0x8E), mirror, "{label}");
            assert_eq!(cpu.mem.rd16(ACTOR + 0x50), m.clock, "{label}");
            assert_eq!(cpu.mem.rd32(ACTOR + 0x10) & 8 != 0, m.retired, "{label}");
            assert_eq!(cpu.mem.rd32(ACTOR + 0x10) & !8, flags, "{label}");
            arrived += u32::from(f.retired);
        }
    }
    assert!(
        eased > 100_000 && mirrored > 10_000 && arrived > 10_000,
        "{eased} {mirrored} {arrived}"
    );
}

const SHUTTER: u32 = 0x801D_D784;
const OT_LINK: u32 = 0x8003_D2C4;
const PACKET_PTR: u32 = 0x1F80_03A0;
const OT_PTR: u32 = 0x1F80_03F4;
const PACKETS: u32 = 0x8015_0000;

/// The shutter-blackout actor `FUN_801DD784` frame by frame: the envelope
/// (`+0x54` phase, `+0x9E` clock, the `+0x10 & 8` retire) against
/// `ShutterBars::step`, and the two `POLY_F4` packets it emits from the
/// height against `shutter_bar_rects` at retail's 224-line bottom.
#[test]
fn the_shutter_bars_match_retail() {
    differential!(prot(897, SHUTTER));
    let Some(mut cpu) = retail_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    use legaia_engine_field::field_actor_timers::ShutterBars;
    use legaia_render_kernels::shutter_bars::{SHUTTER_RETAIL_SCREEN_H, shutter_bar_rects};
    let links = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    {
        let links = links.clone();
        cpu.hook(OT_LINK, move |r, _| {
            links
                .borrow_mut()
                .push((r.r[psx_retail_exec::A0], r.r[psx_retail_exec::A1]));
        });
    }
    let mut rng = Rng(0x5348_5554);
    let (mut traps, mut phases, mut retired) = (0, [0u32; 4], 0);
    'run: for run in 0..3000 {
        let durations: [i16; 3] = std::array::from_fn(|_| {
            if rng.below(12) == 0 {
                0
            } else {
                rng.below(0x100) as i16
            }
        });
        let mut bars = ShutterBars::spawn(durations[0], durations[1], durations[2]);
        let flags = rng.next() & !8;
        cpu.mem.wr32(ACTOR + 0x10, flags);
        cpu.mem.wr16(ACTOR + 0x54, 0);
        cpu.mem.wr16(ACTOR + 0x9E, 0);
        for (i, d) in durations.iter().enumerate() {
            cpu.mem.wr16(ACTOR + 0xB8 + 2 * i as u32, *d as u16);
        }
        cpu.mem.wr16(ACTOR + 0xBE, 0);
        let ot = 0x8016_0000 + rng.below(0x100) * 4;
        cpu.mem.wr32(OT_PTR, ot);
        for frame in 0..600 {
            let delta = [1u8, 1, 2, 3, 4][rng.below(5) as usize];
            cpu.mem.wr8(FRAME_STEP, delta);
            cpu.mem.wr32(PACKET_PTR, PACKETS);
            cpu.mem.load(PACKETS, &[0xA5; 0x30]);
            links.borrow_mut().clear();
            let stop = cpu.call(SHUTTER, &[ACTOR]);
            if let Stop::DivideByZero { .. } = stop {
                // A zero close / open duration on the frame its ramp runs:
                // retail traps, the port draws no ramp.
                traps += 1;
                continue 'run;
            }
            assert_eq!(stop, Stop::Returned, "run {run} frame {frame}");
            let bar = bars.step(delta);
            let label = format!("run {run} frame {frame} durations {durations:?} delta {delta}");
            assert_eq!(cpu.mem.rd16(ACTOR + 0x54) as i16, bars.phase, "{label}");
            assert_eq!(cpu.mem.rd16(ACTOR + 0x9E), bars.clock, "{label}");
            assert_eq!(
                cpu.mem.rd32(ACTOR + 0x10),
                flags | if bars.retired { 8 } else { 0 },
                "{label}"
            );
            let mut want = Vec::new();
            for r in shutter_bar_rects(bar, SHUTTER_RETAIL_SCREEN_H) {
                want.extend(0x0500_0000u32.to_le_bytes());
                want.extend(0x2800_0000u32.to_le_bytes());
                for (x, y) in [(r.x0, r.y0), (r.x1, r.y0), (r.x0, r.y1), (r.x1, r.y1)] {
                    want.extend(x.to_le_bytes());
                    want.extend(y.to_le_bytes());
                }
            }
            assert_eq!(cpu.mem.read(PACKETS, 0x30), want, "{label}: bar {bar}");
            assert_eq!(cpu.mem.rd32(PACKET_PTR), PACKETS + 0x30, "{label}");
            assert_eq!(
                *links.borrow(),
                vec![(ot + 8, PACKETS), (ot + 8, PACKETS + 0x18)],
                "{label}"
            );
            if (0..4).contains(&bars.phase) {
                phases[bars.phase as usize] += 1;
            }
            if bars.retired {
                retired += 1;
                continue 'run;
            }
        }
    }
    eprintln!("{phases:?} {retired} retired, {traps} traps");
    assert!(
        phases.iter().all(|&n| n > 1000) && retired > 2000,
        "{phases:?} {retired} {traps}"
    );
}
