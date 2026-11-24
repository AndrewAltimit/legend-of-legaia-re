//! The party gauge colours of the battle frame `FUN_80046A20`
//! (`0x80046A80..0x80046D28`) against the HUD rows both hosts draw from
//! (`sync_battle_hud_rows` + `BattleHud::sync_status`, then
//! `BattleSlotHud::gauge_fill_indices`): readout HP at zero and at each band
//! edge, MP either side of its quarter and half, and a status word split
//! between the port's raw half and its tracker.

use crate::common::{Rng, scus_cpu};
use legaia_engine_battle::battle_hud_state::BattleHud;
use legaia_engine_battle_vm::status_effects::StatusKind;
use legaia_engine_core::world::World;
use legaia_save::Party;
use psx_retail_exec::Stop;

const FRAME: u32 = 0x8004_6A20;
/// The `jal` past the gauge loop.
const LOOP_END: u32 = 0x8004_6D2C;
const GAUGES: u32 = 0x801C_8FA0;
const ACTORS: u32 = 0x801C_9370;
const ACTOR: u32 = 0x8013_0000;
const ACTOR_STRIDE: u32 = 0x400;
const CELLS: u32 = 0x8014_0000;
/// `gp+0xA49`: the actor gauge `0` shows.
const SHOWN: u32 = 0x8007_BD61;

/// Tracked kinds and the word bit each packs to, for splitting a word.
const TRACKED: [(StatusKind, u16); 6] = [
    (StatusKind::Venom, 0x0001),
    (StatusKind::Toxic, 0x0002),
    (StatusKind::Stone, 0x0004),
    (StatusKind::Numb, 0x0400),
    (StatusKind::Sleep, 0x0800),
    (StatusKind::Curse, 0x1000),
];

#[test]
fn the_party_gauges_take_retail_colours() {
    differential!(scus(FRAME));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.hook(LOOP_END, |_, _| {});
    for g in 0..4u32 {
        for c in 0..4u32 {
            cpu.mem
                .wr32(GAUGES + g * 16 + c * 4, CELLS + (g * 4 + c) * 2);
        }
    }
    for s in 0..3 {
        cpu.mem.wr32(ACTORS + s * 4, ACTOR + s * ACTOR_STRIDE);
    }
    let mut rng = Rng(0x4741_5547);
    let (mut grey, mut raw_only) = (0, 0);
    for round in 0..20000 {
        let mut w = World::new();
        w.load_party(Party::zeroed(3));
        w.enter_battle(3, 2);
        w.party.party_count = 3;
        let mut words = [0u16; 3];
        for s in 0..3u32 {
            let max_hp = [1u16, 4, 7, 100, 999, 9999][rng.below(6) as usize];
            let hp = match rng.below(5) {
                0 => 0,
                1 => max_hp >> 1,
                2 => (max_hp >> 2) + 1,
                3 => max_hp >> 2,
                _ => 1 + rng.below(u32::from(max_hp)) as u16,
            };
            let max_mp = [0u16, 1, 9, 40, 999][rng.below(5) as usize];
            let mp =
                [0, max_mp >> 1, (max_mp >> 1) + 1, max_mp >> 2, max_mp][rng.below(5) as usize];
            let word: u16 = match rng.below(4) {
                0 => 0,
                1 => {
                    [0x1000, 0x0001, 0x0008, 0x0010, 0x0020, 0x0400, 0x0004][rng.below(7) as usize]
                }
                _ => (rng.next() as u16) & 0x1C3F,
            };
            words[s as usize] = word;

            let actor = ACTOR + s * ACTOR_STRIDE;
            cpu.mem.wr16(actor + 0x172, hp);
            cpu.mem.wr16(actor + 0x14E, max_hp);
            cpu.mem.wr16(actor + 0x174, mp);
            cpu.mem.wr16(actor + 0x152, max_mp);
            cpu.mem.wr16(actor + 0x16E, word);

            let a = &mut w.actors[s as usize].battle;
            a.hp = hp;
            a.hp_display = Some(hp);
            a.max_hp = max_hp;
            a.liveness = hp;
            a.mp = mp;
            a.mp_max = max_mp;
            // Half the time the tracked kinds sit in the tracker, as a hit's
            // status arm leaves them; the rest stays in the raw half.
            let mut raw = word;
            if rng.below(2) == 0 {
                for (kind, bit) in TRACKED {
                    if word & bit != 0 {
                        w.battle.status_effects.apply(s as u8, kind);
                        raw &= !bit;
                    }
                }
            }
            w.actors[s as usize].battle.field_flags = raw;
            if raw != 0 && raw == word {
                raw_only += 1;
            }
        }
        let shown = rng.below(3);
        cpu.mem.wr8(SHOWN, shown as u8);
        assert_eq!(cpu.call(FRAME, &[]), Stop::Returned, "round {round}");

        // What both hosts run each battle frame (`sync_battle_hud`).
        let mut hud = BattleHud::new();
        legaia_engine_battle_hud::sync_battle_hud_rows(&mut hud, &w);
        for slot in 0..hud.slots.len() as u8 {
            hud.sync_status(slot, &w.battle.status_effects);
        }
        let cell = |cpu: &mut psx_retail_exec::Cpu, g: u32, c: u32| {
            cpu.mem.rd16(CELLS + (g * 4 + c) * 2) as u8
        };
        for g in 0..4u32 {
            let seat = if g == 0 { shown } else { g - 1 };
            let retail = (cell(&mut cpu, g, 0), cell(&mut cpu, g, 2));
            // Each colour fills two cells: the bar and its readout.
            assert_eq!(retail.0, cell(&mut cpu, g, 1));
            assert_eq!(retail.1, cell(&mut cpu, g, 3));
            let port = hud.slots[seat as usize].gauge_fill_indices();
            assert_eq!(
                retail, port,
                "round {round} gauge {g} seat {seat} word {:#06x}",
                words[seat as usize]
            );
            grey += i32::from(retail.0 == 3);
        }
    }
    assert!(grey > 5000 && raw_only > 3000, "{grey} {raw_only}");
}

const FRAME_END: u32 = 0x8004_7430;
const CTX_PTR: u32 = 0x8007_BD24;
const CTX: u32 = 0x8012_0000;
const FRAME_STEP: u32 = 0x1F80_0393;
const SPECIAL_WORD: u32 = 0x8007_BAC0;
const RUMBLE_HOLD: u32 = 0x8007_BD34;
const PULSE: u32 = 0x8007_B92C;
const LEVEL: u32 = 0x8007_B930;
const PULSE_RATE: u32 = 0x8007_BD48;
const LEVEL_RATE: u32 = 0x8007_BD4C;
const KEEP_OBJECT_1: u32 = 0x8007_B64B;
const SLOT_1_YAW: u32 = 0x8008_91D2;
const MESSAGE_HOLD: u32 = 0x801F_6964;
const ELEMENT: u32 = 0x801D_8DE8;
const BATTLE_SIGNAL: u32 = 0x8007_BD71;

/// The frame driver past the gauges, run from its entry with every call
/// stubbed: the battle-exit pass's party loop (`battle_exit_party_reset`:
/// the status word cleared outside an arena leg, a fallen member stood at 1
/// HP), the countdown clamp at `0x42` while the exit is held, and the rumble
/// hold and decay of a frame that does not exit (`BattleRumbleDecay::step`),
/// the slot-1 backdrop spin (`World::tick_battle_backdrop_spin`, through
/// `BattleArmDrivers`) and the timed message's hold after the SM's pass
/// (`BattleState::tick_timed_message`, one engine tick a vsync of the step).
#[test]
fn the_frame_driver_exits_and_decays_as_retail_does() {
    use legaia_engine_battle_vm::battle_formulas::battle_exit_party_reset;
    use legaia_engine_system::rumble::{BattleRumbleDecay, RumbleMotors};

    differential!(scus(FRAME));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.stub_outside = Some((FRAME, FRAME_END));
    let unloads = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let sink = unloads.clone();
    cpu.hook(ELEMENT, move |r, _| {
        sink.borrow_mut().push((r.r[4], r.r[5]))
    });
    let clean = cpu.mem.clone();
    let mut rng = Rng(0x4558_4954);
    let (mut spun, mut timed_out) = (0, 0);
    let (mut exits, mut held, mut decayed) = (0, 0, 0);
    for round in 0..20000 {
        let party = 1 + rng.below(3);
        let countdown = [0x10i16, 0x42, 0x43, 0x60][rng.below(4) as usize];
        let hold_exit = rng.below(3) == 0;
        let special = [0u32, 0x100, 0x101][rng.below(3) as usize];
        let step = 1 + rng.below(3) as u8;
        let hold = [0i32, 0, 1, 2, 5, -3][rng.below(6) as usize];
        let pulse = [0i32, 0, 3, 0x40, 0xFF, 0x200, -5][rng.below(7) as usize];
        let level = [0i32, 0, 3, 0x40, 0xFF, 0x200, -5][rng.below(7) as usize];
        let pulse_rate = [8i32, 1, 0, -4, 0x40][rng.below(5) as usize];
        let level_rate = [8i32, 1, 0, -4, 0x40][rng.below(5) as usize];
        let status: [u16; 3] = std::array::from_fn(|_| rng.next() as u16);
        let hp: [u16; 3] = std::array::from_fn(|_| [0u16, 1, 77][rng.below(3) as usize]);
        let keep = rng.below(2) != 0;
        let yaw = rng.next() as u16;
        let message = [0i32, -3, 1, 2, 3, 0x78][rng.below(6) as usize];

        cpu.mem = clean.clone();
        unloads.borrow_mut().clear();
        cpu.mem.wr8(KEEP_OBJECT_1, u8::from(keep));
        cpu.mem.wr16(SLOT_1_YAW, yaw);
        cpu.mem.wr32(MESSAGE_HOLD, message as u32);
        // The action SM's pass runs, and the hold after it.
        cpu.mem.wr8(BATTLE_SIGNAL, 0xFF);
        cpu.mem.wr32(CTX_PTR, CTX);
        cpu.mem.wr8(CTX, party as u8);
        cpu.mem.wr8(CTX + 0xB, u8::from(hold_exit));
        cpu.mem.wr16(CTX + 0x6CE, countdown as u16);
        cpu.mem.wr8(CTX + 6, 0xFF);
        cpu.mem.wr8(FRAME_STEP, step);
        cpu.mem.wr32(SPECIAL_WORD, special);
        cpu.mem.wr32(RUMBLE_HOLD, hold as u32);
        cpu.mem.wr32(PULSE, pulse as u32);
        cpu.mem.wr32(LEVEL, level as u32);
        cpu.mem.wr32(PULSE_RATE, pulse_rate as u32);
        cpu.mem.wr32(LEVEL_RATE, level_rate as u32);
        for g in 0..4u32 {
            for c in 0..4u32 {
                cpu.mem
                    .wr32(GAUGES + g * 16 + c * 4, CELLS + (g * 4 + c) * 2);
            }
        }
        for s in 0..3u32 {
            let a = ACTOR + s * ACTOR_STRIDE;
            cpu.mem.wr32(ACTORS + s * 4, a);
            cpu.mem.wr16(a + 0x14C, hp[s as usize]);
            cpu.mem.wr16(a + 0x16E, status[s as usize]);
            cpu.mem.wr16(a + 0x14E, 100);
        }
        assert_eq!(cpu.call(FRAME, &[]), Stop::Returned, "round {round}");
        let at = format!(
            "round {round}: party {party} countdown {countdown:#x} hold {hold_exit} special \
             {special:#x} step {step} rumble {hold}/{pulse}/{level}/{pulse_rate}/{level_rate}"
        );
        // The spin runs ahead of the exit test, on every pass.
        let mut w = legaia_engine_core::world::World::new();
        w.battle.frame_clock.step = step;
        w.battle.backdrop_slot_1_yaw = yaw;
        w.encounters.region_setup =
            Some(legaia_engine_battle::region_encounter::RegionBattleSetup {
                stage_variant: 0,
                keep_backdrop_object_1: Some(keep),
                door_of_light_blocked: false,
                door_of_wind_blocked: false,
                world_map_return: None,
            });
        {
            use legaia_engine_core::world::arm_drivers::BattleArmDrivers;
            w.backdrop_spin();
        }
        assert_eq!(
            cpu.mem.rd16(SLOT_1_YAW),
            w.battle.backdrop_slot_1_yaw,
            "spin, {at}"
        );
        spun += i32::from(keep && step > 1);
        let exiting = countdown > 0x42 && !hold_exit;
        if exiting {
            for s in 0..3u32 {
                let a = ACTOR + s * ACTOR_STRIDE;
                let retail = (cpu.mem.rd16(a + 0x16E), cpu.mem.rd16(a + 0x14C));
                let want = if s < party {
                    battle_exit_party_reset(special, status[s as usize], hp[s as usize])
                } else {
                    (status[s as usize], hp[s as usize])
                };
                assert_eq!(retail, want, "seat {s}, {at}");
            }
            exits += 1;
            continue;
        }
        let clamped = if countdown > 0x42 { 0x42 } else { countdown };
        assert_eq!(cpu.mem.rd16(CTX + 0x6CE) as i16, clamped, "{at}");
        held += i32::from(countdown > 0x42);
        let mut decay = BattleRumbleDecay {
            hold,
            pulse_rate,
            level_rate,
        };
        let mut motors = RumbleMotors { pulse, level };
        decay.step(&mut motors, i32::from(step));
        let retail = (
            cpu.mem.rd32(RUMBLE_HOLD) as i32,
            cpu.mem.rd32(PULSE) as i32,
            cpu.mem.rd32(LEVEL) as i32,
        );
        assert_eq!(
            retail,
            (decay.hold, motors.pulse, motors.level),
            "rumble, {at}"
        );
        decayed += i32::from(retail.1 != pulse || retail.2 != level);
        // The timed message's hold.
        let mut state = legaia_engine_battle_loop::battle_state::BattleState::default();
        let mut banner = legaia_engine_battle_loop::message_banner::BattleMessageBanner::raised(
            legaia_engine_battle_loop::message_banner::TIMED_MESSAGE_ELEMENT,
            String::new(),
            message,
        );
        banner.glide = None;
        state.message_banner = Some(banner);
        for _ in 0..step {
            state.tick_timed_message();
        }
        let b = state.message_banner.as_ref().expect("banner");
        let retail_hold = cpu.mem.rd32(MESSAGE_HOLD) as i32;
        let port_hold = if message > 0 { b.hold.max(0) } else { b.hold };
        assert_eq!(retail_hold, port_hold, "message hold, {at}");
        let retail_unload = unloads.borrow().contains(&(0x66, 1));
        assert_eq!(retail_unload, b.leaving(), "message unload, {at}");
        timed_out += i32::from(retail_unload);
    }
    assert!(
        exits > 3000 && held > 2000 && decayed > 3000 && spun > 3000 && timed_out > 1000,
        "{exits} {held} {decayed} {spun} {timed_out}"
    );
}

/// The battle-entry sweep the frame driver runs ahead of the battle tick
/// (`0x80046EEC..0x8004700C`), pass by pass from the entry counter `gp+0x330
/// = 0x80` until it parks at `0xFF`, against `BattleCamera`'s entry sweep: the
/// counter after each pass (`+ step`, parked past `0xC0`), the drift of TR y /
/// z (`+0x30 * step` / `-0x40 * step` below `0xA2`, `0x80046FA4..0x80046FDC`)
/// and the passes that re-arm case 2 instead (`FUN_801D5854(0, 2)` from `0xA2`
/// on, logged), with retail fed the frame step the port's sweep runs each pass
/// at. The stage load on the first pass at `0x84` or more
/// (`0x80046F30..0x80046F74`) raises the context's `+0x11` once.
#[test]
fn the_entry_sweep_counts_and_drifts_as_retail_does() {
    use legaia_engine_battle_vm::battle_cam_script::{
        BattleCamPhase, BattleCamera, ENTRY_COUNTER_START, ENTRY_TR,
    };
    differential!(scus(FRAME));
    const COUNTER: u32 = 0x8007_B648;
    const TR: u32 = 0x8008_40B8;
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.stub_outside = Some((FRAME, FRAME_END));
    let case2 = std::rc::Rc::new(std::cell::RefCell::new(Vec::<(u32, u32)>::new()));
    let c2 = case2.clone();
    cpu.hook(0x801D_5854, move |r, _| {
        c2.borrow_mut().push((r.r[4], r.r[5]))
    });
    let clean = cpu.mem.clone();
    let mut loads = 0;
    for fs in 1..=4u8 {
        let mut cam = BattleCamera::new(BattleCamPhase::Menu, 0);
        cam.set_frame_step(fs);
        cam.start_entry_sweep();
        cpu.mem = clean.clone();
        cpu.mem.wr32(CTX_PTR, CTX);
        cpu.mem.wr8(CTX, 1);
        cpu.mem.wr8(CTX + 0x11, 0);
        cpu.mem.wr8(COUNTER, ENTRY_COUNTER_START as u8);
        for (k, v) in ENTRY_TR.iter().enumerate() {
            cpu.mem.wr32(TR + 4 * k as u32, *v as i32 as u32);
        }
        let mut frames = 0u64;
        for pass in 0..0x80 {
            let Some(counter) = cam.entry_sweep_counter() else {
                break;
            };
            assert_eq!(
                u32::from(cpu.mem.rd8(COUNTER)),
                counter,
                "fs {fs} pass {pass}"
            );
            // The step this pass runs at, as the port's sweep schedules it.
            let before = cam.entry_sweep_counter();
            let pose_before = cam.pose();
            frames += u64::from(fs);
            cam.advance_to(frames);
            let step = match cam.entry_sweep_counter() {
                Some(next) => next - before.unwrap(),
                None => 0,
            };
            // A pass that ends the sweep ran at the step the counter needed
            // to pass 0xC0; read it off the drift-free tail: the fight's own.
            let step = if step == 0 { u32::from(fs) } else { step };
            cpu.mem.wr8(FRAME_STEP, step as u8);
            case2.borrow_mut().clear();
            assert_eq!(cpu.call(FRAME, &[]), Stop::Returned, "fs {fs} pass {pass}");
            let ctx = format!("fs {fs} pass {pass} counter {counter:#x} step {step}");
            let after = cpu.mem.rd8(COUNTER);
            match cam.entry_sweep_counter() {
                Some(next) => assert_eq!(u32::from(after), next, "{ctx}"),
                None => assert_eq!(after, 0xFF, "{ctx}: parked"),
            }
            if counter < 0xA2 {
                assert!(case2.borrow().is_empty(), "{ctx}");
                let pose = cam.pose();
                for k in 1..3 {
                    let retail = cpu.mem.rd32(TR + 4 * k as u32) as i32;
                    let drift = pose.tr[k] - pose_before.tr[k];
                    let retail_drift = retail - (pose_before.tr[k] as i32);
                    assert_eq!(retail_drift, drift as i32, "{ctx}: TR {k}");
                    // Retail's TR runs on from the same seed as the port's.
                    cpu.mem.wr32(TR + 4 * k as u32, pose.tr[k] as i32 as u32);
                }
            } else {
                assert_eq!(*case2.borrow(), vec![(0, 2)], "{ctx}: case 2 re-armed");
            }
        }
        assert_eq!(cam.entry_sweep_counter(), None, "fs {fs}: the sweep ends");
        assert_eq!(cpu.mem.rd8(COUNTER), 0xFF, "fs {fs}");
        loads += u32::from(cpu.mem.rd8(CTX + 0x11));
    }
    assert_eq!(loads, 4, "one stage load a sweep");
}
