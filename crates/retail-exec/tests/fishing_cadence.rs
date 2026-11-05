//! The fishing cadence recogniser `FUN_801D3DB4` against `ReelCadence::feed`,
//! frame by frame over the disc's gesture templates: held-button scripts close
//! to a template, with taps thrown in, and frames of two steps.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_fishing::fishing::ReelCadence;
use legaia_minigame_formats::fishing_species as fs;
use legaia_retail_exec::{Stop, V0};

const RECOGNISE: u32 = 0x801D_3DB4;
const LAST: u32 = 0x801D_9064;
const INDEX: u32 = 0x801D_91DC;
const RING: u32 = 0x801D_91E4;
const HELD: u32 = 0x8007_B850;
const FRAME_STEP: u32 = 0x1F80_0393;

#[test]
fn a_reel_gesture_is_recognised_as_retail_recognises_it() {
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let overlay = d
        .read_entry(fs::FISHING_OVERLAY_PROT_INDEX)
        .expect("PROT 0972");
    cpu.mem.load(fs::FISHING_OVERLAY_BASE_VA, &overlay);
    let templates = fs::parse_cadence_templates(&overlay).expect("templates");
    let mut port = ReelCadence::new(templates.clone());
    let mut rng = Rng(0x4341_4445);
    let mut matched = [0u32; 4];
    cpu.mem.wr32(LAST, 0);
    for scenario in 0..4000 {
        // A template played near its timings, sometimes with a stray tap.
        let tpl = &templates[rng.below(templates.len() as u32) as usize];
        let mut script: Vec<(u32, i32)> = Vec::new();
        for step in &tpl.steps {
            let jitter = rng.below(25) as i32 - 12;
            script.push((u32::from(step.button), (step.duration + jitter).max(1)));
            if rng.below(6) == 0 {
                script.push((rng.below(3), 1 + rng.below(6) as i32));
            }
        }
        if rng.below(4) == 0 {
            script.push((rng.below(3), 1 + rng.below(90) as i32));
        }
        let step = if scenario % 2 == 0 { 1 } else { 2 };
        for (button, held) in script {
            let mask = match button {
                1 => 0x40,
                2 => 0x80,
                _ => 0,
            };
            for frame in 0..(held + step - 1) / step {
                cpu.mem.wr32(HELD, mask);
                cpu.mem.wr8(FRAME_STEP, step as u8);
                assert_eq!(cpu.call(RECOGNISE, &[]), Stop::Returned);
                let retail = cpu.regs.r[V0] as i32;
                let ours = port.feed(button as u8, step).map_or(-1, |t| t as i32);
                assert_eq!(retail, ours, "scenario {scenario} frame {frame}: match");
                if retail >= 0 {
                    matched[retail as usize] += 1;
                }
                let ring: Vec<(u8, i32)> = (0..16u32)
                    .map(|i| {
                        (
                            cpu.mem.rd32(RING + i * 8) as u8,
                            cpu.mem.rd32(RING + i * 8 + 4) as i32,
                        )
                    })
                    .collect();
                assert_eq!(
                    (
                        ring.as_slice(),
                        cpu.mem.rd32(INDEX) as usize,
                        cpu.mem.rd32(LAST) as u8
                    ),
                    port.state(),
                    "scenario {scenario} frame {frame}: ring"
                );
            }
        }
    }
    assert!(matched.iter().all(|&n| n > 10), "{matched:?}");
}

const CHECK: u32 = 0x801D_3198;
const CHECK_END: u32 = 0x801D_382C;
const LURE: u32 = 0x8014_0000;
const FISH: u32 = 0x8014_1000;
const SCENE: u32 = 0x8015_0000;
const COUNTDOWN: u32 = 0x801D_90EC;
const BAND: u32 = 0x801D_90E8;
const HOOKED: u32 = 0x801D_91B4;
const RECORD: u32 = 0x801D_927C;
const READOUT: u32 = 0x801D_9280;
const SPECIES: u32 = 0x801D_91CC;
const STRENGTH: u32 = 0x801D_91B8;
const FISH_SCALE: u32 = 0x801D_9108;
const SPAWN_PTR: u32 = 0x801D_9114;
const LURE_ROW: u32 = 0x8008_4450;
const ROD: u32 = 0x8008_4454;
const CASTS: u32 = 0x8008_4460;
const PRESSED: u32 = 0x8007_B874;
const VENUE: u32 = 0x801D_90D0;
const SPAWN_TABLE: u32 = 0x8016_0000;
const S4: usize = 20;
const S5: usize = 21;
const S1: usize = 17;
const RA: usize = 31;

#[test]
fn the_waiting_lines_band_and_strike_go_as_retails_do() {
    use legaia_engine_fishing::fishing::{
        BandCheck, ReelInput, band4_gate, hook_strength, spawn_species,
    };
    use legaia_engine_system::rand::BiosRand;
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let overlay = d
        .read_entry(fs::FISHING_OVERLAY_PROT_INDEX)
        .expect("PROT 0972");
    cpu.mem.load(fs::FISHING_OVERLAY_BASE_VA, &overlay);
    let templates = fs::parse_cadence_templates(&overlay).expect("templates");
    let state = std::rc::Rc::new(std::cell::Cell::new(0u32));
    let stream = state.clone();
    cpu.hook(0x8005_6798, move |r, _| {
        let next = stream.get().wrapping_mul(0x41C6_4E6D).wrapping_add(0x3039);
        stream.set(next);
        r.r[V0] = (next >> 16) & 0x7FFF;
    });
    cpu.hook(CHECK_END, |r, _| {
        r.r[RA] = legaia_retail_exec::RETURN_SENTINEL
    });
    cpu.stub(0x8005_67A8, 0); // the debug printf
    cpu.hook(0x8002_4C88, |r, _| r.r[V0] = FISH);
    for f in [
        0x8002_66E0,
        0x8002_6520,
        0x8001_E54C,
        0x8005_FB84,
        0x8002_6478,
    ] {
        cpu.stub(f, 0);
    }
    cpu.mem.wr32(0x1F80_03EC, SCENE);
    cpu.mem.wr32(SPAWN_PTR, SPAWN_TABLE);
    // A table of distinct ids, so the lookup shows.
    let mut table = vec![[0u32; 8]; 8];
    for (l, row) in table.iter_mut().enumerate() {
        for (b, id) in row.iter_mut().enumerate() {
            *id = ((l + b) % 10) as u32;
            cpu.mem
                .wr32(SPAWN_TABLE + (l as u32 * 8 + b as u32) * 4, *id);
        }
    }

    let mut rng = Rng(0x4241_4E44);
    let mut hooked = 0;
    let mut bands = [0u32; 5];
    for scenario in 0..3000 {
        let seed = rng.next();
        let (venue, lure, rod) = (rng.below(2), rng.below(3), rng.below(3));
        let casts = rng.below(120);
        let step = 1 + rng.below(2) as i32;
        cpu.mem.wr32(VENUE, venue);
        cpu.mem.wr32(LURE_ROW, lure);
        cpu.mem.wr32(ROD, rod);
        cpu.mem.wr32(CASTS, casts);
        cpu.mem.wr32(HOOKED, 0);
        cpu.mem.wr32(COUNTDOWN, 0);
        cpu.mem.wr32(BAND, 3);
        cpu.mem.wr32(INDEX, 0);
        cpu.mem.wr32(LAST, 0);
        for i in 0..32 {
            cpu.mem.wr32(RING + i * 4, 0);
        }
        state.set(seed);
        let mut port_rng = BiosRand::new(seed);
        let mut band = BandCheck::default();
        let mut cadence = ReelCadence::new(templates.clone());
        let mut record = 300 + rng.below(1600) as i32;
        for frame in 0..400 {
            let pick = rng.below(16);
            let held: u32 = match pick {
                0..=7 => 0,
                8..=11 => 0x40,
                _ => 0x80,
            };
            let pressed = if rng.below(8) == 0 { 0x8000u32 } else { 0 };
            let readout = (record - 300).max(0);
            cpu.mem.wr32(RECORD, record as u32);
            cpu.mem.wr32(READOUT, readout as u32);
            cpu.mem.wr32(HELD, held);
            cpu.mem.wr32(PRESSED, pressed);
            cpu.mem.wr8(FRAME_STEP, step as u8);
            cpu.regs.r[S1] = 0;
            cpu.regs.r[S4] = 10;
            cpu.regs.r[S5] = LURE;
            assert_eq!(cpu.call(CHECK, &[]), Stop::Returned, "{:x?}", cpu.calls);

            let button = match ReelInput::from_pad_mask(held) {
                ReelInput::ReelA => 1,
                ReelInput::ReelB => 2,
                ReelInput::Idle => 0,
            };
            let matched = band
                .band_due(record, step)
                .then(|| cadence.feed(button, step))
                .flatten();
            let struck = band.tick(
                &mut port_rng,
                matched,
                record,
                readout,
                legaia_engine_fishing::fishing_actors::bite_pad_nudge(pressed),
                0,
                held & 0xC0 != 0,
                step,
            );
            let at = format!("scenario {scenario} frame {frame} record {record}");
            assert_eq!(
                cpu.mem.rd32(COUNTDOWN) as i32,
                band.countdown,
                "{at}: countdown"
            );
            assert_eq!(cpu.mem.rd32(HOOKED) != 0, struck, "{at}: strike");
            if struck {
                let mut b = band.band;
                if band4_gate(
                    venue as usize,
                    lure,
                    rod as i32,
                    b,
                    casts as i32,
                    &mut port_rng,
                ) {
                    b = 4;
                }
                bands[b as usize] += 1;
                assert_eq!(cpu.mem.rd32(BAND), b, "{at}: band");
                let id = spawn_species(&table, lure, b).expect("species");
                assert_eq!(
                    cpu.mem.rd32(SPECIES) as usize,
                    id,
                    "{at}: species, band {b}"
                );
                let hs = hook_strength(&mut port_rng, 10, record, rod as i32);
                assert_eq!(cpu.mem.rd32(STRENGTH) as i32, hs.strength, "{at}: strength");
                assert_eq!(
                    cpu.mem.rd32(FISH_SCALE) as u16,
                    hs.fish_scale,
                    "{at}: scale"
                );
                hooked += 1;
                break;
            }
            assert_eq!(cpu.mem.rd32(BAND), band.band, "{at}: band");
            assert_eq!(state.get(), port_rng.state(), "{at}: draws");
            if held != 0 {
                record -= 4 * step;
            }
        }
    }
    assert!(hooked > 200, "hooked {hooked}: {bands:?}");
}

#[test]
fn the_power_meter_sweeps_as_retails_does() {
    use legaia_engine_fishing::fishing::{CAST_POWER_STEP, CastPower};
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let overlay = d
        .read_entry(fs::FISHING_OVERLAY_PROT_INDEX)
        .expect("PROT 0972");
    cpu.mem.load(fs::FISHING_OVERLAY_BASE_VA, &overlay);
    const METER: u32 = 0x801D_9274;
    const DIR: u32 = 0x801D_9278;
    cpu.hook(0x801C_FB98, |r, _| {
        r.r[RA] = legaia_retail_exec::RETURN_SENTINEL
    });
    for step in [1i32, 2, 3] {
        cpu.mem.wr32(METER, 0);
        cpu.mem.wr32(DIR, 1);
        cpu.mem.wr8(FRAME_STEP, step as u8);
        let mut port = CastPower::new();
        for frame in 0..1000 {
            assert_eq!(cpu.call(0x801C_FB30, &[]), Stop::Returned);
            port.advance(CAST_POWER_STEP * step);
            assert_eq!(
                cpu.mem.rd32(METER) as i32,
                port.value(),
                "step {step} frame {frame}"
            );
        }
    }
}

const ANGLER: u32 = 0x8014_2000;
const ROD_ACTOR: u32 = 0x8014_3000;
const ACC_X: u32 = 0x801D_9174;
const ACC_Y: u32 = 0x801D_9178;
const ACC_Z: u32 = 0x801D_917C;
const TRACKED: u32 = 0x801D_9184;
const SPAWN: u32 = 0x801D_918C;
const POWER: u32 = 0x801D_9274;
const PULL: u32 = 0x801D_9148;
const SWAY: u32 = 0x801D_9140;
const SWAY_TARGET: u32 = 0x801D_9144;
const SWIM: u32 = 0x801D_9100;
const PRESS_WORD: u32 = 0x801D_90D8;

/// The flight arm, its tail, and the in-water head, as `LureActor` runs them.
#[test]
fn the_lure_flies_and_comes_in_as_retails_does() {
    use legaia_engine_fishing::fishing_actors::LureActor;
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let overlay = d
        .read_entry(fs::FISHING_OVERLAY_PROT_INDEX)
        .expect("PROT 0972");
    cpu.mem.load(fs::FISHING_OVERLAY_BASE_VA, &overlay);
    let scus = crate::common::scus().expect("scus");
    // The real quadrature tables the boot installs (`FUN_80026BE0`).
    cpu.mem.wr32(0x8007_B81C, 0x8007_0A2C);
    cpu.mem.wr32(0x8007_B7F8, 0x8007_122C);
    drop(scus);
    let end = |r: &mut legaia_retail_exec::Regs, _: &mut legaia_retail_exec::Memory| {
        r.r[RA] = legaia_retail_exec::RETURN_SENTINEL
    };
    for a in [0x801D_39C4, 0x801D_2F44, 0x801D_3A44, 0x801D_395C] {
        cpu.hook(a, end);
    }
    // The GTE's leading-bit count.
    cpu.hook(0x8005_BA1C, |r, _| {
        r.r[V0] = r.r[legaia_retail_exec::A0].leading_zeros()
    });
    cpu.stub(0x8002_1B04, FISH);
    cpu.stub(0x8006_5034, 0);
    cpu.stub(0x8006_53C8, 0);
    cpu.stub(0x801D_7030, 0);
    cpu.stub(0x8001_A068, 0);
    cpu.stub(0x8005_6798, 0);
    cpu.mem.wr32(0x801D_9284, ANGLER);
    cpu.mem.wr32(0x801D_928C, ROD_ACTOR);
    cpu.mem.wr32(0x1F80_03EC, SCENE);
    let mut rng = Rng(0x4C55_5245);
    let mut landed = 0;
    let mut pulled = 0;
    for scenario in 0..1500 {
        let facing = rng.below(0x1000) as i16;
        let (ax, ay, az) = (
            rng.below(0x2000) as i16,
            rng.below(0x100) as i16 - 0x40,
            rng.below(0x2000) as i16,
        );
        let power = 0x20 + rng.below(0xFE0) as i32;
        let step = 1 + rng.below(3) as i32;
        let mut port = LureActor::cast(ax, ay, az, facing, power, step).expect("cast");
        cpu.mem.wr8(FRAME_STEP, step as u8);
        cpu.mem.wr16(ANGLER + 0x26, facing as u16);
        cpu.mem.wr16(ROD_ACTOR + 0x26, facing as u16);
        cpu.mem.wr32(ACC_X, port.x_fixed as u32);
        cpu.mem.wr32(ACC_Y, port.y_fixed as u32);
        cpu.mem.wr32(ACC_Z, port.z_fixed as u32);
        cpu.mem.wr16(TRACKED, port.tracked.0 as u16);
        cpu.mem.wr16(TRACKED + 2, port.tracked.1 as u16);
        cpu.mem.wr16(TRACKED + 4, port.tracked.2 as u16);
        cpu.mem.wr16(SPAWN, port.spawn.0 as u16);
        cpu.mem.wr16(SPAWN + 4, port.spawn.1 as u16);
        cpu.mem.wr32(POWER, power as u32);
        cpu.mem.wr16(LURE + 0x3E, 0x40);
        cpu.mem.wr32(PULL, 0);
        cpu.mem.wr32(SWAY, 0);
        cpu.mem.wr32(SWAY_TARGET, 0);
        cpu.mem.wr32(SWIM, 0);
        cpu.mem.wr32(SWIM + 4, 0);
        cpu.mem.wr32(HOOKED, 0);
        let mut record = port.tail();
        cpu.mem.wr32(RECORD, record as u32);
        // Flight.
        for frame in 0..200 {
            cpu.regs.r[S5] = LURE;
            cpu.regs.r[16] = 0x1F80_0314;
            cpu.mem.wr32(0x801D_9150, 0);
            assert_eq!(
                cpu.call(0x801D_2784, &[]),
                Stop::Returned,
                "{:x?}",
                cpu.calls
            );
            let down = port.flight_step(facing, step);
            let at =
                format!("scenario {scenario} flight {frame} facing {facing:#x} power {power:#x}");
            assert_eq!(cpu.mem.rd32(ACC_X) as i32, port.x_fixed, "{at}: x");
            assert_eq!(cpu.mem.rd32(ACC_Y) as i32, port.y_fixed, "{at}: y");
            assert_eq!(cpu.mem.rd32(ACC_Z) as i32, port.z_fixed, "{at}: z");
            assert_eq!(cpu.mem.rd32(POWER) as i32, port.power, "{at}: power");
            assert_eq!(cpu.mem.rd16(LURE + 0x3E) as i16, port.fall, "{at}: fall");
            cpu.regs.r[S5] = LURE;
            cpu.regs.r[3] = 0x801E_0000;
            assert_eq!(
                cpu.call(0x801D_39C8, &[]),
                Stop::Returned,
                "{:x?}",
                cpu.calls
            );
            record = port.tail();
            assert_eq!(cpu.mem.rd32(RECORD) as i32, record, "{at}: record");
            if down {
                landed += 1;
                break;
            }
        }
        // In the water, with presses, D-pad and a fish's swim.
        let hooked = rng.below(2) == 0;
        for frame in 0..120 {
            let edge = if rng.below(10) == 0 {
                [0x40u32, 0x80, 0xC0][rng.below(3) as usize]
            } else {
                0
            };
            let held = [0u32, 0, 0x2000, 0x8000][rng.below(4) as usize];
            let swim = (
                rng.below(0x4000) as i32 - 0x2000,
                rng.below(0x4000) as i32 - 0x2000,
            );
            cpu.mem.wr32(PRESS_WORD, edge);
            cpu.mem.wr32(HELD, held);
            cpu.mem.wr32(HOOKED, u32::from(hooked));
            cpu.mem.wr32(SWIM, swim.0 as u32);
            cpu.mem.wr32(SWIM + 4, swim.1 as u32);
            cpu.mem.wr32(RECORD, record as u32);
            cpu.regs.r[S5] = LURE;
            assert_eq!(
                cpu.call(0x801D_29E8, &[]),
                Stop::Returned,
                "{:x?}",
                cpu.calls
            );
            port.water_step(port.heading(), facing, record, hooked.then_some(swim), step);
            port.pull_step(edge, step);
            let at = format!("scenario {scenario} water {frame} hooked {hooked} record {record}");
            assert_eq!(cpu.mem.rd32(ACC_X) as i32, port.x_fixed, "{at}: x");
            assert_eq!(cpu.mem.rd32(ACC_Z) as i32, port.z_fixed, "{at}: z");
            assert_eq!(cpu.mem.rd32(PULL) as i32, port.pull, "{at}: pull");
            cpu.regs.r[S5] = LURE;
            assert_eq!(
                cpu.call(0x801D_382C, &[]),
                Stop::Returned,
                "{:x?}",
                cpu.calls
            );
            port.sway_step(held, step);
            assert_eq!(cpu.mem.rd32(SWAY) as i32, port.sway.0, "{at}: sway");
            assert_eq!(
                cpu.mem.rd32(SWAY_TARGET) as i32,
                port.sway.1,
                "{at}: target"
            );
            cpu.regs.r[S5] = LURE;
            cpu.regs.r[3] = 0x801E_0000;
            assert_eq!(cpu.call(0x801D_39C8, &[]), Stop::Returned);
            record = port.tail();
            assert_eq!(cpu.mem.rd32(RECORD) as i32, record, "{at}: record");
            pulled += usize::from(port.pull != 0);
        }
    }
    assert!(landed > 1400 && pulled > 1000, "{landed} {pulled}");
}
