//! Small `SCUS_942.54` system leaves against their `legaia_engine_system`
//! ports: the boot SPU-base seeder `FUN_800265E8`, the staged character pair
//! `FUN_80035C00`, the prompt line counter `FUN_8003CBA8`, the DRAWENV seed
//! `FUN_80020038`, the fade ramp step `FUN_80020C14`, the rumble actuator
//! kernel `FUN_80018DB0` and the scene-name sync `FUN_8001D7F8` (its BIOS
//! `strcmp` / `strcpy` answered by the C library's definitions).
//! Each compares the bytes the routine writes, over a whole region where it
//! writes a table.

use crate::common::{Rng, diff_offsets, scus_cpu};
use legaia_engine_system::fade_ramp::{FadeRamp, RampFlags};
use legaia_engine_system::rumble::{ActuatorLayout, RumbleActuator, RumbleMotors};
use legaia_engine_system::scene_name_sync::{SceneNameEntry, sync_scene_name};
use legaia_engine_system::scus_leaf_kernels::{
    BOOT_ENABLE_FLAG_ADDRS, StagedCharacterSelector, seed_boot_offset_table, text_line_count,
};
use legaia_engine_system::sound_state::DRAW_ENV_INIT;
use psx_retail_exec::{A0, A1, Stop, V0};

const BOOT_SEED: u32 = 0x8002_65E8;
const STAGE_PAIR: u32 = 0x8003_5C00;
const LINE_COUNT: u32 = 0x8003_CBA8;
const DRAW_ENV: u32 = 0x8002_0038;
const FADE_STEP: u32 = 0x8002_0C14;
const RUMBLE: u32 = 0x8001_8DB0;
const NAME_SYNC: u32 = 0x8001_D7F8;

#[test]
fn the_boot_spu_base_seed_matches_retail() {
    differential!(scus(BOOT_SEED));
    let Some(mut cpu) = scus_cpu() else {
        return;
    };
    let mut rng = Rng(0x5350_5542);
    for _ in 0..4 {
        let mut table = [0u32; 12];
        for (i, w) in table.iter_mut().enumerate() {
            *w = rng.next();
            cpu.mem.wr32(0x8009_17B0 + 4 * i as u32, *w);
        }
        let flags_before = cpu.mem.read(0x8007_0518, 0xA0);
        assert_eq!(cpu.call(BOOT_SEED, &[]), Stop::Returned);
        seed_boot_offset_table(&mut table);
        let got: Vec<u32> = (0..12).map(|i| cpu.mem.rd32(0x8009_17B0 + 4 * i)).collect();
        assert_eq!(got, table);
        let mut want = flags_before;
        for a in BOOT_ENABLE_FLAG_ADDRS {
            let o = (a - 0x8007_0518) as usize;
            want[o..o + 2].copy_from_slice(&1u16.to_le_bytes());
        }
        assert_eq!(
            cpu.mem.read(0x8007_0518, 0xA0),
            want,
            "the enable halfwords"
        );
    }
}

#[test]
fn the_staged_character_pair_matches_retail() {
    differential!(scus(STAGE_PAIR));
    let Some(mut cpu) = scus_cpu() else {
        return;
    };
    let mut rng = Rng(0x5041_4952);
    let mut sel = StagedCharacterSelector::default();
    for _ in 0..64 {
        let fill: Vec<u8> = (0..0x20).map(|_| rng.next() as u8).collect();
        cpu.mem.load(0x8007_BB68, &fill);
        let (a, b) = (rng.next(), rng.next());
        assert_eq!(cpu.call(STAGE_PAIR, &[a, b]), Stop::Returned);
        sel.set_pair(a as u16, b as u16);
        let mut want = fill;
        want[8..10].copy_from_slice(&sel.primary.to_le_bytes());
        want[0x10..0x12].copy_from_slice(&sel.secondary.to_le_bytes());
        assert_eq!(cpu.mem.read(0x8007_BB68, 0x20), want);
    }
}

#[test]
fn the_prompt_line_count_matches_retail() {
    differential!(scus(LINE_COUNT));
    let Some(mut cpu) = scus_cpu() else {
        return;
    };
    let mut rng = Rng(0x4C49_4E45);
    let (mut breaks, mut escapes) = (0usize, 0usize);
    for case in 0..3000 {
        let len = rng.below(40) as usize;
        let mut s: Vec<u8> = (0..len)
            .map(|_| match rng.below(8) {
                0 => 0x7C,
                1 => 0xC0 | rng.below(16) as u8,
                _ => 1 + rng.below(0xFF) as u8,
            })
            .collect();
        // A lead byte never stands right before the terminator: retail's
        // skip would step past it, where the port stops at the slice's end.
        if s.last().is_some_and(|b| b & 0xF0 == 0xC0) {
            s.push(b'a');
        }
        s.push(0);
        s.push(0x7C);
        s.push(0);
        cpu.mem.load(0x8010_0000, &s);
        assert_eq!(cpu.call(LINE_COUNT, &[0x8010_0000]), Stop::Returned);
        assert_eq!(cpu.regs.r[V0], text_line_count(&s), "case {case}: {s:02X?}");
        breaks += s.iter().filter(|&&b| b == 0x7C).count();
        escapes += s.iter().filter(|&&b| b & 0xF0 == 0xC0).count();
    }
    assert!(breaks > 1000 && escapes > 1000);
}

#[test]
fn the_draw_env_seed_matches_retail() {
    differential!(scus(DRAW_ENV));
    let Some(mut cpu) = scus_cpu() else {
        return;
    };
    let base = 0x8010_0000;
    let fill = vec![0xA5u8; 0x40];
    cpu.mem.load(base, &fill);
    assert_eq!(cpu.call(DRAW_ENV, &[base]), Stop::Returned);
    let mut want = fill;
    for (off, value) in DRAW_ENV_INIT.stores() {
        let o = off as usize;
        if off == 0x28 {
            want[o..o + 2].copy_from_slice(&value.to_le_bytes());
        } else {
            want[o] = value as u8;
        }
    }
    assert_eq!(cpu.mem.read(base, 0x40), want);
}

#[test]
fn the_fade_ramp_step_matches_retail() {
    differential!(scus(FADE_STEP));
    let Some(mut cpu) = scus_cpu() else {
        return;
    };
    let actor = 0x8010_0000u32;
    let mut rng = Rng(0x4641_4445);
    let (mut drawn, mut finished, mut delayed) = (0usize, 0usize, 0usize);
    for case in 0..20_000 {
        let h = |rng: &mut Rng| rng.next() as i16;
        let ch = |rng: &mut Rng| -> i16 {
            match rng.below(4) {
                0 => rng.below(0x3FC1) as i16,
                1 => h(rng),
                _ => (rng.below(256) as i16) << 6,
            }
        };
        let mut ramp = FadeRamp {
            current_q6: [ch(&mut rng), ch(&mut rng), ch(&mut rng)],
            target_q6: [ch(&mut rng), ch(&mut rng), ch(&mut rng)],
            delta_q6: std::array::from_fn(|_| rng.below(0x400) as i16 - 0x200),
            kind: rng.next() as i32,
            delay: if rng.below(3) == 0 {
                rng.below(8) as i16 - 2
            } else {
                0
            },
            hold: rng.below(12) as i16 - 2,
            duration: rng.below(12) as i16 - 3,
            id: h(&mut rng),
            flags: RampFlags::default(),
        };
        let dt = [1u8, 1, 2, 3, 0, 255][rng.below(6) as usize];
        let mut bytes: Vec<u8> = (0..0xA4).map(|_| rng.next() as u8).collect();
        let put = |b: &mut Vec<u8>, o: usize, v: i16| b[o..o + 2].copy_from_slice(&v.to_le_bytes());
        for c in 0..3 {
            put(&mut bytes, 0x7C + 2 * c, ramp.current_q6[c]);
            put(&mut bytes, 0x84 + 2 * c, ramp.target_q6[c]);
            put(&mut bytes, 0x8C + 2 * c, ramp.delta_q6[c]);
        }
        bytes[0x94..0x98].copy_from_slice(&ramp.kind.to_le_bytes());
        put(&mut bytes, 0x98, ramp.delay);
        put(&mut bytes, 0x9A, ramp.hold);
        put(&mut bytes, 0x9C, ramp.duration);
        put(&mut bytes, 0x9E, ramp.id);
        cpu.mem.load(actor, &bytes);
        cpu.mem.wr8(0x1F80_0393, dt);
        assert_eq!(cpu.call(FADE_STEP, &[actor]), Stop::Returned);
        let got = cpu.regs.r[V0];
        let port = ramp.advance_fade_vsyncs(dt);
        assert_eq!(got, port.unwrap_or(u32::MAX), "case {case}");
        let mut want = bytes.clone();
        for c in 0..3 {
            put(&mut want, 0x7C + 2 * c, ramp.current_q6[c]);
        }
        put(&mut want, 0x98, ramp.delay);
        put(&mut want, 0x9A, ramp.hold);
        put(&mut want, 0x9C, ramp.duration);
        if ramp.flags.ramped {
            want[0x63] |= 1;
        }
        if ramp.flags.finished {
            want[0x10] |= 8;
        }
        let retail = cpu.mem.read(actor, 0xA4);
        assert!(
            retail == want,
            "case {case}: {:?}",
            diff_offsets(&retail, &want)
        );
        drawn += usize::from(port.is_some());
        finished += usize::from(ramp.flags.finished);
        delayed += usize::from(port.is_none() && !ramp.flags.finished);
    }
    assert!(
        drawn > 5000 && finished > 1000 && delayed > 500,
        "{drawn} {finished} {delayed}"
    );
}

#[test]
fn the_rumble_actuator_kernel_matches_retail() {
    differential!(scus(RUMBLE));
    let Some(mut cpu) = scus_cpu() else {
        return;
    };
    let mut rng = Rng(0x5255_4D42);
    let mut port = RumbleActuator::default();
    let mut pulses = 0usize;
    // The CD watchdog in front of the kernel stays idle.
    cpu.mem.wr32(0x8007_BC70, 0);
    cpu.mem.wr32(0x8007_B8A4, port.cadence as u32);
    let mut motors = RumbleMotors::default();
    for frame in 0..20_000 {
        if rng.below(12) == 0 {
            let w = |rng: &mut Rng| match rng.below(4) {
                0 => 0,
                1 => rng.below(0x300) as i32 - 0x100,
                _ => rng.below(0x100) as i32,
            };
            motors = RumbleMotors {
                pulse: w(&mut rng),
                level: w(&mut rng),
            };
        }
        let fixed = frame / 5000 % 2 == 1;
        let layout = if fixed {
            ActuatorLayout::Fixed
        } else {
            ActuatorLayout::DualShock
        };
        cpu.mem.wr32(0x8007_B79C, u32::from(fixed));
        cpu.mem.wr32(0x8007_B92C, motors.pulse as u32);
        cpu.mem.wr32(0x8007_B930, motors.level as u32);
        let counter = cpu.mem.rd32(0x8007_B8AC);
        assert_eq!(cpu.call(RUMBLE, &[]), Stop::Returned);
        let bytes = port.step(motors, layout);
        let got = [cpu.mem.rd8(0x8009_15DA), cpu.mem.rd8(0x8009_15DB)];
        assert_eq!(got, bytes, "frame {frame} {motors:?} {layout:?}");
        assert_eq!(
            cpu.mem.rd32(0x8007_B8A4) as i32,
            port.cadence,
            "frame {frame}"
        );
        assert_eq!(cpu.mem.rd32(0x8007_B8AC), counter.wrapping_add(1));
        pulses += usize::from(bytes[usize::from(fixed)] == 1);
    }
    assert!(pulses > 500, "{pulses}");
}

#[test]
fn the_scene_name_sync_matches_retail() {
    differential!(scus(NAME_SYNC));
    let Some(mut cpu) = scus_cpu() else {
        return;
    };
    let c_str = |m: &mut psx_retail_exec::Memory, mut a: u32| {
        let mut s = Vec::new();
        loop {
            let b = m.rd8(a);
            if b == 0 {
                return s;
            }
            s.push(b);
            a += 1;
        }
    };
    cpu.hook(0x8005_6738, move |r, m| {
        let (a, b) = (c_str(m, r.r[A0]), c_str(m, r.r[A1]));
        r.r[V0] = match a.cmp(&b) {
            std::cmp::Ordering::Less => -1i32 as u32,
            std::cmp::Ordering::Equal => 0,
            std::cmp::Ordering::Greater => 1,
        };
    });
    cpu.hook(0x8005_6758, move |r, m| {
        let mut s = c_str(m, r.r[A1]);
        s.push(0);
        m.load(r.r[A0], &s);
        r.r[V0] = r.r[A0];
    });
    let mut rng = Rng(0x4E41_4D45);
    let names = [
        "town01", "map01", "jou", "opdeene", "dolk", "rugi", "TOWN01", "Map01",
    ];
    let (mut hits, mut misses) = (0usize, 0usize);
    for case in 0..300 {
        let table: Vec<SceneNameEntry> = (0..150)
            .map(|_| {
                let mut name = [0u8; 12];
                let n = names[rng.below(names.len() as u32) as usize].to_lowercase();
                name[..n.len()].copy_from_slice(n.as_bytes());
                if rng.below(4) == 0 {
                    name[rng.below(n.len() as u32) as usize] ^= 1;
                }
                SceneNameEntry {
                    name,
                    index: rng.next() as u16,
                }
            })
            .collect();
        for (i, e) in table.iter().enumerate() {
            let mut rec = e.name.to_vec();
            rec.extend_from_slice(&e.index.to_le_bytes());
            rec.extend_from_slice(&[rng.next() as u8, rng.next() as u8]);
            cpu.mem.load(0x8008_8758 + 16 * i as u32, &rec);
        }
        let mut staged = [0u8; 8];
        let pick = if rng.below(6) == 0 {
            "nowhere"
        } else {
            names[rng.below(names.len() as u32) as usize]
        };
        staged[..pick.len()].copy_from_slice(pick.as_bytes());
        if rng.below(5) == 0 {
            staged[7] = b'Q';
        }
        let mut active: [u8; 8] = std::array::from_fn(|_| rng.next() as u8);
        let mut index = rng.next();
        cpu.mem.load(0x8007_050C, &staged);
        cpu.mem.load(0x8008_4548, &active);
        cpu.mem.wr32(0x8008_4540, index);
        assert_eq!(cpu.call(NAME_SYNC, &[]), Stop::Returned);
        let before = index;
        sync_scene_name(&mut staged, &table, &mut active, &mut index);
        assert_eq!(cpu.mem.read(0x8007_050C, 8), staged, "case {case} staged");
        assert_eq!(cpu.mem.read(0x8008_4548, 8), active, "case {case} active");
        assert_eq!(cpu.mem.rd32(0x8008_4540), index, "case {case} index");
        if index == before {
            misses += 1;
        } else {
            hits += 1;
        }
    }
    assert!(hits > 100 && misses > 10, "{hits} {misses}");
}
