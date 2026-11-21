//! The fishing cadence recogniser `FUN_801D3DB4` against `ReelCadence::feed`,
//! frame by frame over the disc's gesture templates: held-button scripts close
//! to a template, with taps thrown in, and frames of two steps.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_fishing::fishing::ReelCadence;
use legaia_minigame_formats::fishing_species as fs;
use psx_retail_exec::{Stop, V0};

const RECOGNISE: u32 = 0x801D_3DB4;
const LAST: u32 = 0x801D_9064;
const INDEX: u32 = 0x801D_91DC;
const RING: u32 = 0x801D_91E4;
const HELD: u32 = 0x8007_B850;
const FRAME_STEP: u32 = 0x1F80_0393;

#[test]
fn a_reel_gesture_is_recognised_as_retail_recognises_it() {
    differential!(prot(972, RECOGNISE));
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
    differential!(prot(972, CHECK));
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
    cpu.hook(CHECK_END, |r, _| r.r[RA] = psx_retail_exec::RETURN_SENTINEL);
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
    differential!(prot(972, 0x801C_FB30));
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
        r.r[RA] = psx_retail_exec::RETURN_SENTINEL
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

/// The session machine `FUN_801CF3BC`, its body `0x801CF3BC..0x801D03B0`.
const SESSION: u32 = 0x801C_F3BC;
const SESSION_END: u32 = 0x801D_03B0;
/// Its state word (`-0x6d94` off `0x801E0000`).
const SESSION_STATE: u32 = 0x801D_926C;

/// The same meter, the machine entered at its head in state `0x14` once a
/// frame, every call out of its body stubbed: the head's pad latch and the
/// dispatch, the meter, the press that locks it and steps the state, and the
/// tail's fades left at rest.
#[test]
fn the_session_machine_sweeps_and_locks_the_meter() {
    differential!(prot(972, SESSION));
    use legaia_engine_fishing::fishing::{CAST_POWER_STEP, CastPower};
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let overlay = d
        .read_entry(fs::FISHING_OVERLAY_PROT_INDEX)
        .expect("PROT 0972");
    const METER: u32 = 0x801D_9274;
    const DIR: u32 = 0x801D_9278;
    const ROD_PTR: u32 = 0x801D_9284;
    cpu.stub_outside = Some((SESSION, SESSION_END));
    let mut rng = Rng(0x4D45_5452);
    let mut locks = 0;
    for round in 0..90 {
        cpu.mem.load(fs::FISHING_OVERLAY_BASE_VA, &overlay);
        // The rod actor the lock reads its facing off.
        cpu.mem.wr32(ROD_PTR, ROD_ACTOR);
        cpu.mem.wr32(SESSION_STATE, 0x14);
        cpu.mem.wr32(METER, 0);
        cpu.mem.wr32(DIR, 1);
        let step = 1 + round % 3;
        cpu.mem.wr8(FRAME_STEP, step as u8);
        let lock_at = rng.below(400);
        let mut port = CastPower::new();
        port.zero();
        for frame in 0..400u32 {
            let press = frame == lock_at;
            cpu.mem
                .wr32(PRESSED, if press { 0x40 << rng.below(2) } else { 0 });
            assert_eq!(cpu.call(SESSION, &[]), Stop::Returned, "frame {frame}");
            port.advance(CAST_POWER_STEP * step);
            let at = format!("round {round} step {step} frame {frame}");
            assert_eq!(cpu.mem.rd32(METER) as i32, port.value(), "meter, {at}");
            if press {
                assert_eq!(cpu.mem.rd32(SESSION_STATE), 0x15, "the press locks, {at}");
                assert_eq!(cpu.mem.rd32(METER) as i32, port.lock(), "{at}");
                locks += 1;
                break;
            }
            assert_eq!(cpu.mem.rd32(SESSION_STATE), 0x14, "{at}");
        }
    }
    assert!(locks > 60, "locks {locks}");
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
    differential!(
        prot(972, 0x801D_2784),
        prot(972, 0x801D_29E8),
        prot(972, CHECK_END),
        prot(972, 0x801D_39C8)
    );
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
    let end = |r: &mut psx_retail_exec::Regs, _: &mut psx_retail_exec::Memory| {
        r.r[RA] = psx_retail_exec::RETURN_SENTINEL
    };
    for a in [0x801D_39C4, 0x801D_2F44, 0x801D_3A44, 0x801D_395C] {
        cpu.hook(a, end);
    }
    // The GTE's leading-bit count.
    cpu.hook(0x8005_BA1C, |r, _| {
        r.r[V0] = r.r[psx_retail_exec::A0].leading_zeros()
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

/// The session machine's state 1, the venue select: the departure scene
/// `_DAT_8007BAC4` (`map02` -> Vidna's pond, `map03` -> Buma's) against
/// `venue_for_departure_scene`, over both venues the word held and scenes
/// either side of the two ids, every call out of the routine stubbed but the
/// actor spawn `FUN_80020DE0`, which hands out zeroed actors.
#[test]
fn the_venue_select_picks_the_pond_as_retail_does() {
    differential!(prot(972, SESSION));
    use legaia_engine_fishing::fishing::venue_for_departure_scene;
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let overlay = d
        .read_entry(fs::FISHING_OVERLAY_PROT_INDEX)
        .expect("PROT 0972");
    const SCENE_ID: u32 = 0x8007_BAC4;
    const ACTORS: u32 = 0x8014_8000;
    cpu.stub_outside = Some((SESSION, SESSION_END));
    let next = std::rc::Rc::new(std::cell::Cell::new(ACTORS));
    {
        let next = next.clone();
        cpu.hook(0x8002_0DE0, move |r, m| {
            let at = next.get();
            next.set(at + 0x100);
            for o in (0..0x100).step_by(4) {
                m.wr32(at + o, 0);
            }
            r.r[V0] = at;
        });
    }
    let mut seen = [0usize; 2];
    for scene in [0u32, 0xF3, 0xF4, 0xF5, 0x186, 0x187, 0x188, 0x3FF] {
        for current in 0..2usize {
            cpu.mem.load(fs::FISHING_OVERLAY_BASE_VA, &overlay);
            cpu.mem.wr32(SESSION_STATE, 1);
            cpu.mem.wr32(SCENE_ID, scene);
            cpu.mem.wr32(VENUE, current as u32);
            next.set(ACTORS);
            assert_eq!(cpu.call(SESSION, &[]), Stop::Returned, "scene {scene:#x}");
            let want = venue_for_departure_scene(scene, current);
            assert_eq!(
                cpu.mem.rd32(VENUE) as usize,
                want,
                "scene {scene:#x} venue {current}"
            );
            seen[want] += 1;
        }
    }
    assert!(seen.iter().all(|&n| n > 4), "{seen:?}");
}

/// The session machine's camera states against `VenueCamera`: the cast
/// wind-up `0xD` (`wind_up`: the TR floors, the pitch cap and the focus step
/// out along the lead's facing through the overlay's polar helper
/// `FUN_801D7BB8`, run live), the catch showcase `0x20` (`showcase`, both
/// venues) and its tilt `0x21` (`showcase_tilt`), each from random camera
/// globals at frame steps `1..=3`. Every other call out of the routine
/// returns `0`.
#[test]
fn the_camera_states_frame_the_pond_as_retail_does() {
    differential!(prot(972, SESSION));
    use legaia_engine_fishing::fishing::WINDUP_FRAMES;
    use legaia_engine_fishing::fishing_actors::VenueCamera;
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let overlay = d
        .read_entry(fs::FISHING_OVERLAY_PROT_INDEX)
        .expect("PROT 0972");
    const POLAR: u32 = 0x801D_7BB8;
    const LEAD_PTR: u32 = 0x801D_9284;
    const WIND_UP: u32 = 0x801D_9054;
    const PITCH: u32 = 0x8007_B790;
    const TR: u32 = 0x8008_40B8;
    const FOCUS: u32 = 0x8008_9118;
    let mut targets: Vec<u32> = overlay
        .as_chunks::<4>()
        .0
        .iter()
        .map(|w| u32::from_le_bytes(*w))
        .filter(|w| w >> 26 == 3)
        .map(|w| ((w & 0x03FF_FFFF) << 2) | 0x8000_0000)
        .filter(|&t| !(SESSION..SESSION_END).contains(&t) && t != POLAR)
        .collect();
    targets.sort_unstable();
    targets.dedup();
    for t in targets {
        cpu.stub(t, 0);
    }
    let mut rng = Rng(0x5645_4E55);
    let read = |cpu: &mut psx_retail_exec::Cpu| {
        let mut c = VenueCamera::default();
        c.rot = std::array::from_fn(|i| cpu.mem.rd16(PITCH + 2 * i as u32) as i16);
        c.tr = std::array::from_fn(|i| cpu.mem.rd32(TR + 4 * i as u32) as i32);
        c.focus = std::array::from_fn(|i| cpu.mem.rd32(FOCUS + 4 * i as u32) as i32);
        c
    };
    let (mut tilts_done, mut wound) = (0, 0);
    for case in 0..1500 {
        cpu.mem.load(fs::FISHING_OVERLAY_BASE_VA, &overlay);
        cpu.mem.wr32(0x8007_B81C, 0x8007_0A2C);
        cpu.mem.wr32(0x8007_B7F8, 0x8007_0A2C + 0x800);
        let step = 1 + rng.below(3);
        cpu.mem.wr8(FRAME_STEP, step as u8);
        cpu.mem.wr32(PRESSED, 0);
        let facing = rng.below(0x1000) as i16;
        for o in (0..0x100).step_by(4) {
            cpu.mem.wr32(ROD_ACTOR + o, 0);
        }
        cpu.mem.wr16(ROD_ACTOR + 0x26, facing as u16);
        cpu.mem.wr32(LEAD_PTR, ROD_ACTOR);
        for i in 0..3u32 {
            cpu.mem
                .wr16(PITCH + 2 * i, (rng.below(0x300) as i16 - 0x180) as u16);
            cpu.mem.wr32(TR + 4 * i, rng.below(0x1800) - 0x400);
            cpu.mem.wr32(FOCUS + 4 * i, rng.below(0x4000) - 0x2000);
        }
        let state = [0xD, 0x20, 0x21][case % 3];
        let venue = rng.below(2);
        cpu.mem.wr32(VENUE, venue);
        cpu.mem.wr32(SESSION_STATE, state);
        let counter = rng.below(16);
        cpu.mem.wr32(WIND_UP, counter);
        let mut port = read(&mut cpu);
        assert_eq!(cpu.call(SESSION, &[]), Stop::Returned, "case {case}");
        let at = format!("case {case} state {state:#x} step {step} venue {venue}");
        let next = match state {
            0xD => {
                port.wind_up(facing, step as i32);
                let total = counter as i32 + step as i32;
                if total >= WINDUP_FRAMES {
                    wound += 1;
                    0x14
                } else {
                    0xD
                }
            }
            0x20 => {
                port.showcase(venue as usize);
                0x21
            }
            _ => {
                let landed = port.showcase_tilt(step as i32);
                tilts_done += usize::from(landed);
                if landed { 0x22 } else { 0x21 }
            }
        };
        let got = read(&mut cpu);
        assert_eq!(
            (got.rot, got.tr, got.focus),
            (port.rot, port.tr, port.focus),
            "{at}: the camera"
        );
        // State 0xD falls into 0x14's meter, which leaves the state be on a
        // frame with no press.
        assert_eq!(cpu.mem.rd32(SESSION_STATE), next, "{at}: the state");
    }
    assert!(tilts_done > 20 && wound > 20, "{tilts_done} {wound}");
}

/// The hub: state `0x0C`'s open edge (`andi 0x110`, `0x801CF9EC`), the menu
/// state `0x64` through its routine `FUN_801D0474` and the help pages `0x65`
/// / `0x66` through `FUN_801D72A0` (both run live), frame by frame over
/// random pad edges against `FishingHub::open` / `FishingHub::step`: the
/// state, the menu cursor `0x801D912C` and the cue `0x8007B6D8`. A run ends
/// where the host takes over (back to the shore, the tackle and exchange
/// screens, the venue exit).
#[test]
fn the_hub_menu_and_help_pages_step_as_retail_does() {
    differential!(prot(972, SESSION));
    use legaia_engine_fishing::fishing_hub::{FishingHub, HUB_OPEN_MASK, HubExit, HubScreen};
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let overlay = d
        .read_entry(fs::FISHING_OVERLAY_PROT_INDEX)
        .expect("PROT 0972");
    const MENU: u32 = 0x801D_0474;
    const HELP: u32 = 0x801D_72A0;
    const CURSOR: u32 = 0x801D_912C;
    const CUE: u32 = 0x8007_B6D8;
    let mut targets: Vec<u32> = overlay
        .as_chunks::<4>()
        .0
        .iter()
        .map(|w| u32::from_le_bytes(*w))
        .filter(|w| w >> 26 == 3)
        .map(|w| ((w & 0x03FF_FFFF) << 2) | 0x8000_0000)
        .filter(|&t| !(SESSION..SESSION_END).contains(&t) && t != MENU && t != HELP)
        .collect();
    targets.sort_unstable();
    targets.dedup();
    for t in targets {
        cpu.stub(t, 0);
    }
    let mut rng = Rng(0x4855_4253);
    let edges = [
        0u32, 0, 0x1000, 0x4000, 0x20, 0x40, 0x04, 0x01, 0x10, 0x80, 0x100,
    ];
    let mut seen = std::collections::BTreeSet::new();
    for run in 0..300 {
        cpu.mem.load(fs::FISHING_OVERLAY_BASE_VA, &overlay);
        cpu.mem.wr32(SESSION_STATE, 0x0C);
        let mut hub: Option<FishingHub> = None;
        for frame in 0..60 {
            let edge = edges[rng.below(edges.len() as u32) as usize];
            // The shore's cast edge (`& 0xC0`) is the wind-up's, not the hub's.
            let edge = if hub.is_none() { edge & !0xC0 } else { edge };
            cpu.mem.wr32(PRESSED, edge);
            cpu.mem.wr16(CUE, 0);
            cpu.mem.wr8(FRAME_STEP, 1);
            assert_eq!(
                cpu.call(SESSION, &[]),
                Stop::Returned,
                "run {run} frame {frame}"
            );
            let at = format!("run {run} frame {frame} edge {edge:#x}");
            let (state, cue) = match hub.as_mut() {
                None if edge & HUB_OPEN_MASK != 0 => {
                    hub = Some(FishingHub::open());
                    (0x64, 0x21)
                }
                None => (0x0C, 0),
                Some(h) => {
                    let mut equip = (None, None);
                    let step = h.step(edge, 0, |_| 0, &mut equip);
                    let state = match (step.exit, h.screen) {
                        (Some(HubExit::Resume), _) => 0x0A,
                        (Some(HubExit::Leave), _) => 0xC8,
                        (Some(HubExit::OpenExchange), _) | (None, HubScreen::Exchange) => 0x78,
                        (None, HubScreen::Menu) => 0x64,
                        (None, HubScreen::Help(0)) => 0x65,
                        (None, HubScreen::Help(_)) => 0x66,
                        (None, HubScreen::Tackle) => 0x6E,
                    };
                    (state, step.sfx.unwrap_or(0))
                }
            };
            assert_eq!(cpu.mem.rd32(SESSION_STATE), state, "{at}: the state");
            assert_eq!(cpu.mem.rd16(CUE), cue, "{at}: the cue");
            if let Some(h) = hub.as_ref() {
                assert_eq!(
                    cpu.mem.rd32(CURSOR) as i32,
                    h.menu.cursor,
                    "{at}: the cursor"
                );
            }
            seen.insert(state);
            if matches!(state, 0x0A | 0x6E | 0x78 | 0xC8) {
                break;
            }
        }
    }
    assert!(
        [0x0A, 0x64, 0x65, 0x66, 0x6E, 0x78, 0xC8]
            .iter()
            .all(|s| seen.contains(s)),
        "{seen:x?}"
    );
}

/// The shore's lure gate: state `0xB`'s fade-in, whose end steps to `0xC`
/// and runs the gate `FUN_801D712C` (live, over a hooked bag count), parking
/// the session on the no-lure screen `0x96` when it fails; and state `0x96`,
/// which re-runs the gate every frame. Against `select_owned_rod` (the gate
/// `PondSession::shore_lure_gate` runs): the state and the re-pointed lure
/// word `0x80084450`.
#[test]
fn the_shore_lure_gate_parks_the_session_as_retail_does() {
    differential!(prot(972, SESSION));
    use legaia_engine_fishing::fishing::select_owned_rod;
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let overlay = d
        .read_entry(fs::FISHING_OVERLAY_PROT_INDEX)
        .expect("PROT 0972");
    const GATE: u32 = 0x801D_712C;
    const BAG_COUNT: u32 = 0x8004_2F4C;
    const FADE: u32 = 0x801D_905C;
    let mut targets: Vec<u32> = overlay
        .as_chunks::<4>()
        .0
        .iter()
        .map(|w| u32::from_le_bytes(*w))
        .filter(|w| w >> 26 == 3)
        .map(|w| ((w & 0x03FF_FFFF) << 2) | 0x8000_0000)
        .filter(|&t| !(SESSION..SESSION_END).contains(&t) && t != GATE)
        .collect();
    targets.sort_unstable();
    targets.dedup();
    for t in targets {
        cpu.stub(t, 0);
    }
    let bag = std::rc::Rc::new(std::cell::Cell::new([0i32; 6]));
    {
        let bag = bag.clone();
        cpu.hook(BAG_COUNT, move |r, _| {
            let id = r.r[psx_retail_exec::A0];
            r.r[V0] = if (0x9D..=0xA2).contains(&id) {
                bag.get()[(id - 0x9D) as usize] as u32
            } else {
                0
            };
        });
    }
    let mut rng = Rng(0x4741_5445);
    let mut parked = [0usize; 2];
    for case in 0..2000 {
        cpu.mem.load(fs::FISHING_OVERLAY_BASE_VA, &overlay);
        let counts: [i32; 6] = std::array::from_fn(|_| [0, 0, 1, 3][rng.below(4) as usize]);
        bag.set(counts);
        let lure = rng.below(3);
        let state = if case % 3 == 0 { 0x96 } else { 0x0B };
        let fade = rng.below(0x40);
        let step = 1 + rng.below(3);
        cpu.mem.wr32(SESSION_STATE, state);
        cpu.mem.wr32(LURE_ROW, lure);
        cpu.mem.wr32(FADE, fade);
        cpu.mem.wr8(FRAME_STEP, step as u8);
        cpu.mem.wr32(PRESSED, 0);
        assert_eq!(cpu.call(SESSION, &[]), Stop::Returned, "case {case}");
        let at = format!("case {case} state {state:#x} bag {counts:?} lure {lure} fade {fade}");
        let mut port_lure = lure;
        let count_of = |id: u32| counts[(id - 0x9D) as usize];
        let want = if state == 0x96 {
            select_owned_rod(&mut port_lure, count_of);
            0x96
        } else if fade as i32 - ((step as i32) << 4) >= 0 {
            0x0B
        } else if select_owned_rod(&mut port_lure, count_of) {
            parked[0] += 1;
            0x0C
        } else {
            parked[1] += 1;
            0x96
        };
        assert_eq!(cpu.mem.rd32(SESSION_STATE), want, "{at}: the state");
        assert_eq!(cpu.mem.rd32(LURE_ROW), port_lure, "{at}: the lure");
    }
    assert!(parked.iter().all(|&n| n > 50), "{parked:?}");
}

/// The line-break screen, state `0x28` (`0x801CFEE4..0x801CFF20`): the
/// converging pair `FUN_801D7528` (run live; its box draw stubbed) draws
/// while its timer `DAT_801D9164` is under `0xC8` and the session goes back
/// to the shore (state `0xA`) on the frame it stops; the timer gains the
/// frame step either way, and the leave edge does nothing (the arm leaves
/// `$s7` clear). Against `PondSession` seated on `PondPhase::Snapped`: the
/// state and, while it holds, the timer.
#[test]
fn the_line_break_screen_returns_to_the_shore_as_retail_does() {
    differential!(prot(972, SESSION));
    use legaia_engine_fishing::fishing::{
        CAST_POWER_STEP, FishingRecord, PondInput, PondPhase, PondSession,
    };
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let overlay = d
        .read_entry(fs::FISHING_OVERLAY_PROT_INDEX)
        .expect("PROT 0972");
    const PAIR: u32 = 0x801D_7528;
    const BANNER: u32 = 0x801D_9164;
    let mut targets: Vec<u32> = overlay
        .as_chunks::<4>()
        .0
        .iter()
        .map(|w| u32::from_le_bytes(*w))
        .filter(|w| w >> 26 == 3)
        .map(|w| ((w & 0x03FF_FFFF) << 2) | 0x8000_0000)
        .filter(|&t| !(SESSION..SESSION_END).contains(&t) && t != PAIR)
        .collect();
    targets.sort_unstable();
    targets.dedup();
    for t in targets {
        cpu.stub(t, 0);
    }
    let mut rng = Rng(0x4252_4B4E);
    let (mut held, mut back) = (0u32, 0u32);
    for case in 0..20_000u32 {
        cpu.mem.load(fs::FISHING_OVERLAY_BASE_VA, &overlay);
        let banner = match rng.below(3) {
            0 => 0xC8 - 4 + rng.below(8) as i32,
            1 => rng.below(0x120) as i32,
            _ => -(rng.below(8) as i32),
        };
        let step = 1 + rng.below(3) as i32;
        let edge = [0u32, 0x20, 0x01, 0x21, 0x40][rng.below(5) as usize];
        cpu.mem.wr32(SESSION_STATE, 0x28);
        cpu.mem.wr32(BANNER, banner as u32);
        cpu.mem.wr8(FRAME_STEP, step as u8);
        cpu.mem.wr32(PRESSED, edge);
        assert_eq!(cpu.call(SESSION, &[]), Stop::Returned, "case {case}");
        let retail = (cpu.mem.rd32(SESSION_STATE), cpu.mem.rd32(BANNER) as i32);
        let mut port = PondSession::new(
            Vec::new(),
            Vec::new(),
            Vec::new(),
            0,
            0,
            0,
            0,
            FishingRecord::default(),
            0,
            1,
        );
        port.seat_line_break(banner);
        let input = PondInput {
            leave_edge: edge & 0x21 != 0,
            cast_edge: edge & 0xC0 != 0,
            ..Default::default()
        };
        port.tick(input, step, CAST_POWER_STEP);
        let at = format!("case {case} banner {banner} step {step} edge {edge:#x}");
        match port.phase() {
            PondPhase::Snapped => {
                assert_eq!(retail, (0x28, port.line_break_banner()), "{at}");
                held += 1;
            }
            PondPhase::Idle => {
                assert_eq!(retail, (0x0A, banner + step), "{at}");
                back += 1;
            }
            p => panic!("{at}: the port left for {p:?}"),
        }
    }
    assert!(held > 5000 && back > 3000, "{held} {back}");
}

/// The venue exit, state `200` (`0x801D00F0..0x801D019C`): the lure gate
/// `FUN_801D712C` (live, over a hooked bag), the no-lure line when it fails
/// (`FUN_801D73B8`, recorded), the fade counter `DAT_801D90C0` gaining the
/// frame step, the grey push `FUN_80024EE4(0, 2, grey)` (recorded) at eight
/// times it, held at `0xFF` from `0x100`, and the leave (`FUN_80026018`,
/// recorded) once full with the XA drive word `0x8007BC20` idle. Against
/// `PondSession` seated on the fade (`seat_exit_fade`): the counter, the
/// push, the leave, the re-pointed lure and the line.
#[test]
fn the_venue_exit_fades_and_leaves_as_retail_does() {
    differential!(prot(972, SESSION));
    use legaia_engine_fishing::fishing::{
        CAST_POWER_STEP, FishingRecord, PondInput, PondSession, select_owned_rod,
    };
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let overlay = d
        .read_entry(fs::FISHING_OVERLAY_PROT_INDEX)
        .expect("PROT 0972");
    const GATE: u32 = 0x801D_712C;
    const BAG_COUNT: u32 = 0x8004_2F4C;
    const COUNTER: u32 = 0x801D_90C0;
    const XA: u32 = 0x8007_BC20;
    const PUSH: u32 = 0x8002_4EE4;
    const LEAVE: u32 = 0x8002_6018;
    const CAPTION: u32 = 0x801D_73B8;
    let mut targets: Vec<u32> = overlay
        .as_chunks::<4>()
        .0
        .iter()
        .map(|w| u32::from_le_bytes(*w))
        .filter(|w| w >> 26 == 3)
        .map(|w| ((w & 0x03FF_FFFF) << 2) | 0x8000_0000)
        .filter(|&t| !(SESSION..SESSION_END).contains(&t) && t != GATE)
        .collect();
    targets.sort_unstable();
    targets.dedup();
    for t in targets {
        cpu.stub(t, 0);
    }
    let bag = Rc::new(Cell::new([0i32; 6]));
    {
        let bag = bag.clone();
        cpu.hook(BAG_COUNT, move |r, _| {
            let id = r.r[psx_retail_exec::A0];
            r.r[V0] = if (0x9D..=0xA2).contains(&id) {
                bag.get()[(id - 0x9D) as usize] as u32
            } else {
                0
            };
        });
    }
    let calls = Rc::new(RefCell::new(Vec::<(u32, u32, u32, u32)>::new()));
    for va in [PUSH, LEAVE, CAPTION] {
        let c = calls.clone();
        cpu.hook(va, move |r, _| {
            c.borrow_mut().push((
                va,
                r.r[psx_retail_exec::A0],
                r.r[psx_retail_exec::A1],
                r.r[psx_retail_exec::A2],
            ))
        });
    }
    let mut rng = Rng(0x4558_4954);
    let (mut left, mut lined) = (0, 0);
    for case in 0..20_000u32 {
        cpu.mem.load(fs::FISHING_OVERLAY_BASE_VA, &overlay);
        let counts: [i32; 6] = std::array::from_fn(|_| [0, 0, 1, 3][rng.below(4) as usize]);
        bag.set(counts);
        let lure = rng.below(3);
        let counter = rng.below(0x28) as i32;
        let step = 1 + rng.below(3) as i32;
        cpu.mem.wr32(SESSION_STATE, 0xC8);
        cpu.mem.wr32(LURE_ROW, lure);
        cpu.mem.wr32(COUNTER, counter as u32);
        cpu.mem.wr32(XA, 0);
        cpu.mem.wr8(FRAME_STEP, step as u8);
        cpu.mem.wr32(PRESSED, 0);
        calls.borrow_mut().clear();
        assert_eq!(cpu.call(SESSION, &[]), Stop::Returned, "case {case}");
        let got = calls.borrow().clone();
        let pushes: Vec<u32> = got.iter().filter(|c| c.0 == PUSH).map(|c| c.3).collect();
        let retail = (
            cpu.mem.rd32(COUNTER) as i32,
            pushes,
            got.iter().any(|c| c.0 == LEAVE),
            cpu.mem.rd32(LURE_ROW),
            got.iter().any(|c| c.0 == CAPTION),
        );

        let mut port = PondSession::new(
            Vec::new(),
            Vec::new(),
            Vec::new(),
            0,
            lure,
            0,
            0,
            FishingRecord::default(),
            0,
            1,
        );
        port.seat_exit_fade(counter);
        let count_of = |id: u32| counts[(id - 0x9D) as usize];
        let owned = port.exit_fade_lure_gate(count_of);
        port.tick(PondInput::default(), step, CAST_POWER_STEP);
        let grey = u32::from(port.exit_fade_level().expect("fading"));
        let mut lure_port = lure;
        select_owned_rod(&mut lure_port, count_of);
        let p = (
            port.exit_fade_counter().expect("fading"),
            vec![grey * 0x01_01_01],
            port.take_exit_request(),
            port.persist().lure,
            !owned,
        );
        assert_eq!(
            retail, p,
            "case {case} counter {counter} step {step} bag {counts:?} lure {lure}"
        );
        assert_eq!(lure_port, p.3);
        left += u32::from(p.2);
        lined += u32::from(p.4);
    }
    assert!(left > 3000 && lined > 2000, "{left} {lined}");
}

/// State `0x1E`, the catch close-up (`0x801CFD30..0x801CFD7C`): it clears
/// the plate's accept word `DAT_801D90BC`, the surge `DAT_801D90F4` and the
/// catch-HUD word `DAT_801D9058`, then moves to the showcase `0x20` only
/// with the rod's sub-state `0x801D91AC` at `0x14` and the close-up flag
/// `0x801D91B0` clear. Against `catch_showcase_due`, the gate
/// `VenueCameraDriver` holds the restored camera on.
#[test]
fn the_catch_waits_for_the_rod_and_the_close_up_as_retail_does() {
    differential!(prot(972, SESSION));
    use legaia_engine_fishing::fishing_actors::catch_showcase_due;
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let overlay = d
        .read_entry(fs::FISHING_OVERLAY_PROT_INDEX)
        .expect("PROT 0972");
    const SWING: u32 = 0x801D_91AC;
    const CLOSE_UP: u32 = 0x801D_91B0;
    const CLEARED: [u32; 3] = [0x801D_90BC, 0x801D_90F4, 0x801D_9058];
    cpu.stub_outside = Some((SESSION, SESSION_END));
    let mut rng = Rng(0x5348_4F57);
    let mut shown = [0u32; 2];
    for case in 0..4000u32 {
        cpu.mem.load(fs::FISHING_OVERLAY_BASE_VA, &overlay);
        let swing = [0u32, 1, 2, 10, 0x14, 0x14][rng.below(6) as usize];
        let flag = [0u32, 0, 1, rng.next()][rng.below(4) as usize];
        cpu.mem.wr32(SESSION_STATE, 0x1E);
        cpu.mem.wr32(SWING, swing);
        cpu.mem.wr32(CLOSE_UP, flag);
        for w in CLEARED {
            cpu.mem.wr32(w, rng.next() | 1);
        }
        cpu.mem.wr8(FRAME_STEP, 1);
        cpu.mem.wr32(PRESSED, 0);
        assert_eq!(cpu.call(SESSION, &[]), Stop::Returned, "case {case}");
        let due = catch_showcase_due(swing == 0x14, flag != 0);
        let at = format!("case {case} swing {swing:#x} flag {flag:#x}");
        assert_eq!(
            cpu.mem.rd32(SESSION_STATE),
            if due { 0x20 } else { 0x1E },
            "{at}"
        );
        for w in CLEARED {
            assert_eq!(cpu.mem.rd32(w), 0, "{at}: {w:#x} cleared");
        }
        shown[usize::from(due)] += 1;
    }
    assert!(shown.iter().all(|&n| n > 500), "{shown:?}");
}
