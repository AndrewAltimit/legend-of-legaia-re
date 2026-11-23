//! PROT 0954 (Fatal Decision)'s outcome arm against `apply_outcome_to`.
//!
//! Arm 10 (`0x801F7CE4`) drains the module countdown `0x801F9018`, retires the
//! eight icons, steps the phase and jumps through the table at `0x801F6A18` on
//! the landed slot's value (`0x801F9020[*0x801F9080]`). The run seats the arm
//! one drain from passing on a victim with seeded stats - the halving floors'
//! edge values among them, Stone and not - and compares every halfword the
//! outcome arms write with the port's kernel, plus the gold tithe's word.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_battle_vm::battle_cam_script::ModuleShot;
use legaia_engine_cast::cast_arm_ticks::steal_party_draw;
use legaia_engine_cast::cast_fatal_decision::{
    FatalDecisionHost, FatalDecisionState, FatalDecisionView, FatalOutcome, FatalVictim,
    HIDDEN_RENDER_FLAG, NO_EFFECT_TEXT, OPEN_RECORD, PROMPT_TEXT, WHEEL_TICK_SPAN,
    apply_outcome_to, fatal_decision_tick, gold_tithe,
};
use legaia_engine_cast::cast_module_camera::ModuleSeat;
use legaia_engine_cast::cast_module_ticks::CastModuleCtx;
use psx_retail_exec::{A0, A1, A2, A3, Stop, V0};
use std::cell::RefCell;
use std::rc::Rc;

const BASE: u32 = 0x801F_69D8;
const OVERLAY_BASE: u32 = 0x801C_E818;
const CTX_PTR: u32 = 0x8007_BD24;
const ACTORS: u32 = 0x801C_9370;
const CTX: u32 = 0x8012_0000;
const ACTOR: u32 = 0x8013_0000;
const STRIDE: u32 = 0x400;
const PARTS: u32 = 0x8016_0000;
const SCALAR: u32 = 0x1F80_037D;
const STEP: u32 = 0x1F80_0393;
const RAND: u32 = 0x8005_6798;
const FATAL_DECISION: u32 = 0x801F_6A58;
const COUNTDOWN: u32 = 0x801F_9018;
const SLOTS: u32 = 0x801F_9020;
const ICONS: u32 = 0x801F_9040;
const LANDED: u32 = 0x801F_9080;
const GOLD: u32 = 0x8008_459C;

fn jal_targets(module: &[u8]) -> Vec<u32> {
    let mut out: Vec<u32> = module
        .as_chunks::<4>()
        .0
        .iter()
        .map(|w| u32::from_le_bytes(*w))
        .filter(|w| w >> 26 == 3)
        .map(|w| 0x8000_0000 | ((w & 0x03FF_FFFF) << 2))
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

fn read_victim(cpu: &mut psx_retail_exec::Cpu, a: u32) -> FatalVictim {
    let h = |cpu: &mut psx_retail_exec::Cpu, o: u32| cpu.mem.rd16(a + o);
    FatalVictim {
        hp: h(cpu, 0x14C),
        max_hp: h(cpu, 0x14E),
        mp: h(cpu, 0x150),
        max_mp: h(cpu, 0x152),
        hp_bar_delta: cpu.mem.rd32(a + 0x10) as i32,
        mp_readout: h(cpu, 0x178),
        status: h(cpu, 0x16E),
        atk: h(cpu, 0x158),
        atk_base: h(cpu, 0x15A),
        udf: h(cpu, 0x15C),
        udf_base: h(cpu, 0x15E),
        ldf: h(cpu, 0x160),
        ldf_base: h(cpu, 0x162),
    }
}

#[test]
fn fatal_decisions_outcomes_land_as_retail_lands_them() {
    differential!(prot(954, FATAL_DECISION));
    let (Some(_), Some(disc)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let overlay = disc.read_entry(898).expect("PROT 0898");
    let module = disc.read_entry(954).expect("PROT 0954");
    let mut rng = Rng(0x4641_5441);
    let mut failures = Vec::new();
    let mut compared = 0usize;
    let edge = [0u16, 1, 2, 3];
    // Every outcome but the item theft, which reaches the bag.
    for value in (0..16u32).filter(|&v| v != FatalOutcome::StealItem as u32) {
        let outcome = FatalOutcome::from_slot(value).expect("an outcome");
        for round in 0..8usize {
            let mut cpu = scus_cpu().expect("SCUS");
            cpu.mem.load(OVERLAY_BASE, &overlay);
            cpu.mem.load(BASE, &module);
            for target in jal_targets(&module) {
                if target != RAND {
                    cpu.stub(target, 0);
                }
            }
            cpu.mem.wr32(CTX_PTR, CTX);
            for b in 0..0x1100 {
                cpu.mem.wr8(CTX + b, 0);
            }
            let (caster, victim) = (3u32, (round % 3) as u32);
            cpu.mem.wr8(CTX, 3);
            cpu.mem.wr8(CTX + 1, 1);
            cpu.mem.wr8(CTX + 0x13, caster as u8);
            cpu.mem.wr8(CTX + 0x279, 10);
            for s in 0..8u32 {
                let a = ACTOR + s * STRIDE;
                cpu.mem.wr32(ACTORS + s * 4, a);
                for o in (0..STRIDE).step_by(4) {
                    cpu.mem.wr32(a + o, 0);
                }
            }
            let va = ACTOR + victim * STRIDE;
            cpu.mem.wr8(ACTOR + caster * STRIDE + 0x1DD, victim as u8);
            let pick = |rng: &mut Rng, k: usize| {
                if (round + k).is_multiple_of(3) {
                    edge[(round + k) % 4]
                } else {
                    1 + rng.below(999) as u16
                }
            };
            let hp = pick(&mut rng, 0).max(1);
            cpu.mem.wr16(va + 0x14C, hp);
            cpu.mem.wr16(va + 0x14E, hp + rng.below(200) as u16);
            for (k, o) in [0x150u32, 0x152, 0x158, 0x15A, 0x15C, 0x15E, 0x160, 0x162]
                .into_iter()
                .enumerate()
            {
                cpu.mem.wr16(va + o, pick(&mut rng, k + 1));
            }
            cpu.mem.wr32(va + 0x10, rng.below(50));
            cpu.mem.wr16(va + 0x178, rng.below(50) as u16);
            // Stone on a seat in a quarter of the rounds; the HP and MP arms
            // test it (`0x801F7DC4..0x801F7DD0`).
            let status = if round % 4 == 3 {
                4
            } else {
                [0u16, 1, 0x1000][round % 3]
            };
            cpu.mem.wr16(va + 0x16E, status);
            cpu.mem.wr8(SCALAR, 8);
            cpu.mem.wr8(STEP, 1);
            cpu.mem.wr32(COUNTDOWN, 1);
            for i in 0..8u32 {
                cpu.mem.wr32(ICONS + 4 * i, PARTS + 0x80 * i);
                cpu.mem.wr32(SLOTS + 4 * i, 0);
            }
            let landed = (round % 8) as u32;
            cpu.mem.wr32(LANDED, landed);
            cpu.mem.wr32(SLOTS + 4 * landed, value);
            let gold = rng.below(100_000) as i32 - 10 * (round as i32 % 2);
            cpu.mem.wr32(GOLD, gold as u32);

            // The queued action Numb and Stone cancel: category `+0x1DE`,
            // initiative key `+0x16C`, item `+0x1DF`.
            let category = [0u8, 1, 1, 2][rng.below(4) as usize];
            let key = if rng.below(3) == 0 {
                0
            } else {
                1 + rng.below(50) as u16
            };
            let item = 1 + rng.below(200) as u8;
            cpu.mem.wr8(va + 0x1DE, category);
            cpu.mem.wr16(va + 0x16C, key);
            cpu.mem.wr8(va + 0x1DF, item);
            let refunds = Rc::new(RefCell::new(Vec::new()));
            {
                let refunds = refunds.clone();
                cpu.hook(0x8004_21D4, move |r, _| {
                    refunds.borrow_mut().push((r.r[A0] as u8, r.r[A1]));
                });
            }

            let mut want = read_victim(&mut cpu, va);
            let step = apply_outcome_to(&mut want, outcome);
            let stop = cpu.call(FATAL_DECISION, &[]);
            let c = format!("{outcome:?} round {round}");
            if stop != Stop::Returned {
                failures.push(format!("{c}: {stop:?}"));
                continue;
            }
            assert_ne!(cpu.mem.rd8(CTX + 0x279), 10, "{c}: the arm passed");
            let got = read_victim(&mut cpu, va);
            if got != want {
                failures.push(format!("{c}: retail {got:?}\n  port {want:?}"));
            }
            // `World::stone_cancels_queued_action`: an item action
            // (`ActionCategory::Item`) with its key set hands the item back,
            // and the category clears either way.
            let refund: Vec<(u8, u32)> = (step.cancels_action && category == 1 && key != 0)
                .then_some((item, 1))
                .into_iter()
                .collect();
            if *refunds.borrow() != refund {
                failures.push(format!(
                    "{c}: refund retail {:?} port {refund:?}",
                    refunds.borrow()
                ));
            }
            let left = cpu.mem.rd8(va + 0x1DE);
            let want_left = if step.cancels_action { 0 } else { category };
            if left != want_left {
                failures.push(format!("{c}: +0x1DE retail {left} port {want_left}"));
            }
            if outcome == FatalOutcome::GoldTithe {
                let g = cpu.mem.rd32(GOLD) as i32;
                if g != gold_tithe(gold) {
                    failures.push(format!("{c}: gold retail {g} port {}", gold_tithe(gold)));
                }
            }
            compared += 1;
        }
    }
    eprintln!("{compared} outcomes compared");
    assert!(compared >= 100, "{compared}");
    let n = failures.len();
    failures.truncate(40);
    assert!(n == 0, "{n} mismatches:\n{}", failures.join("\n"));
}

/// What one retail pass called out to, as the hooks logged it.
#[derive(Default)]
struct Calls {
    spawns: Vec<(u32, u16, u32)>,
    shots: Vec<ModuleShot>,
    cues: Vec<u16>,
    elements: Vec<(u32, u32)>,
    ticks: Vec<u32>,
    flashes: usize,
    frames: usize,
    draws: Vec<u32>,
    outcome: Option<u32>,
}

const SPAWN: u32 = 0x8002_1B04;
const BUILDER: u32 = 0x801D_829C;
const CUE: u32 = 0x8004_FCC8;
const ROUTE: u32 = 0x8004_FE5C;
const ELEMENT: u32 = 0x801D_8DE8;
const FLASH: u32 = 0x8002_4E80;
const FRAMING: u32 = 0x801D_5854;
const BEARING: u32 = 0x8001_9B28;
const TEXT_WIDTH: u32 = 0x8003_5F04;
const FORMATION: u32 = 0x8007_BD0C;
const ROT: u32 = 0x801F_9010;
const TICK_REM: u32 = 0x801F_9014;
const SPREAD: u32 = 0x801F_901C;
const BACKDROP: u32 = 0x801F_9060;
const PAD_A: u32 = 0x8007_B874;
const PAD_B: u32 = 0x8007_B938;
const CONFIRM_MASK: u32 = 0x8008_46D0;
const BANNER_TEXT: u32 = 0x8007_6C10 + 0x734;
const RADII: u32 = 0x8017_0000;

fn i16s(m: &mut psx_retail_exec::Memory, a: u32) -> [i16; 3] {
    [0, 2, 4].map(|o| m.rd16(a + o) as i16)
}

fn module_seat(m: &mut psx_retail_exec::Memory, a: u32) -> ModuleSeat {
    ModuleSeat {
        x: m.rd16(a + 0x34) as i16,
        y: m.rd16(a + 0x36) as i16,
        z: m.rd16(a + 0x38) as i16,
        facing: m.rd16(a + 0x46),
    }
}

/// One seat's words the arms store: tint `+0x04`, render flag `+0x21C`,
/// animation rate `+0x21D`, heading `+0x46`, reaction flag `+0x1DC`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SeatWords {
    tint: u32,
    flag: u8,
    rate: u8,
    facing: u16,
    react: u8,
}

fn seat_words(m: &mut psx_retail_exec::Memory, a: u32) -> SeatWords {
    SeatWords {
        tint: m.rd32(a + 4),
        flag: m.rd8(a + 0x21C),
        rate: m.rd8(a + 0x21D),
        facing: m.rd16(a + 0x46),
        react: m.rd8(a + 0x1DC),
    }
}

/// The body's whole phase chain, one pass per run: each run seats a phase
/// (`0..=12` and `0xFF`) with the module words, the seats and the pad drawn
/// around the arms' gates - the countdown either side of its drain, the angle
/// on and off a slot boundary and either sign, Stone and Rot victims, party
/// and monster victims, each listed caster and an unlisted one - runs
/// retail's pass and `fatal_decision_tick` on the same state, and compares
/// the phase and context bytes, the busy return, every module word
/// (`0x801F9010..0x801F9084`), the shot handed to the builder, the records
/// spawned and where their handles land, the cues, the message-bar and banner
/// raises, the flash, the retires, every icon's `+0x14` / `+0x16` / `+0x72`,
/// each seat's tint, flag, rate and heading (the port's pass applied the way
/// `World::apply_fatal_pass` applies it), the framing calls and the wheel
/// tick of the epilogue. The outcome arms past the dispatch are the test
/// above's; the steal (slot 14) is left out of the arm-10 runs.
#[test]
fn fatal_decisions_phase_chain_runs_as_retail_runs_it() {
    differential!(prot(954, FATAL_DECISION));
    let (Some(_), Some(disc)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let overlay = disc.read_entry(898).expect("PROT 0898");
    let module = disc.read_entry(954).expect("PROT 0954");
    let mut rng = Rng(0x4644_4348);
    let mut failures = Vec::new();
    let mut compared = 0usize;
    let mut passed_gate = [0usize; 13];
    let phases: Vec<u8> = (0..=12).chain([0xFF]).collect();
    let countdowns = [
        -9i32, -1, 0, 1, 7, 8, 9, 16, 0x1FF, 0x200, 0x201, 0x900, 9600,
    ];
    for round in 0..2800usize {
        let phase = phases[round % phases.len()];
        let mut cpu = scus_cpu().expect("SCUS");
        cpu.mem.load(OVERLAY_BASE, &overlay);
        cpu.mem.load(BASE, &module);
        cpu.mem.wr32(0x8007_B81C, 0x8007_0A2C);
        cpu.mem.wr32(0x8007_B7F8, 0x8007_0A2C + 0x800);
        let calls = Rc::new(RefCell::new(Calls::default()));
        for target in jal_targets(&module) {
            if ![RAND, BEARING].contains(&target) {
                cpu.stub(target, 0);
            }
        }
        {
            let calls = calls.clone();
            let next = Rc::new(std::cell::Cell::new(PARTS + 0x1000));
            cpu.hook(SPAWN, move |r, m| {
                let at = next.get();
                next.set(at + 0x80);
                for o in (0..0x80).step_by(4) {
                    m.wr32(at + o, 0);
                }
                calls
                    .borrow_mut()
                    .spawns
                    .push((r.r[A2], r.r[A3] as u16, at));
                r.r[V0] = at;
            });
        }
        {
            let calls = calls.clone();
            cpu.hook(BUILDER, move |r, m| {
                let shot = ModuleShot {
                    angles: i16s(m, r.r[A0]),
                    tr: i16s(m, r.r[A1]),
                    focus: i16s(m, r.r[A2]),
                    frames: r.r[A3] as u16,
                };
                calls.borrow_mut().shots.push(shot);
            });
        }
        for (va, kind) in [
            (CUE, 0u8),
            (ROUTE, 1),
            (ELEMENT, 2),
            (FLASH, 3),
            (FRAMING, 4),
        ] {
            let calls = calls.clone();
            cpu.hook(va, move |r, _| {
                let mut c = calls.borrow_mut();
                match kind {
                    0 => c.cues.push(r.r[A0] as u16),
                    1 => c.ticks.push(r.r[A0]),
                    2 => c.elements.push((r.r[A0], r.r[A1])),
                    3 => c.flashes += 1,
                    _ => c.frames += 1,
                }
                r.r[V0] = 0;
            });
        }
        cpu.stub(TEXT_WIDTH, 0x40);
        {
            let calls = calls.clone();
            let stream = Rc::new(std::cell::Cell::new(rng.next() | 1));
            cpu.hook(RAND, move |r, _| {
                let n = stream.get().wrapping_mul(0x41C6_4E6D).wrapping_add(0x3039);
                stream.set(n);
                r.r[V0] = (n >> 16) & 0x7FFF;
                calls.borrow_mut().draws.push(r.r[V0]);
            });
        }
        {
            let calls = calls.clone();
            cpu.probe(0x801F_7DBC, move |r, _, _| {
                calls.borrow_mut().outcome = Some(r.r[psx_retail_exec::V1]);
            });
        }

        // The battle: three party seats, one to three monsters.
        let monsters = 1 + rng.below(3);
        cpu.mem.wr32(CTX_PTR, CTX);
        for b in 0..0x800 {
            cpu.mem.wr8(CTX + b, 0);
        }
        cpu.mem.wr8(CTX, 3);
        cpu.mem.wr8(CTX + 1, monsters as u8);
        let caster = 3 + rng.below(monsters);
        let victim = if rng.below(4) == 0 {
            3 + rng.below(monsters)
        } else {
            rng.below(3)
        };
        cpu.mem.wr8(CTX + 0x13, caster as u8);
        cpu.mem.wr8(CTX + 0x279, phase);
        cpu.mem.wr8(CTX + 0x278, rng.below(4) as u8);
        cpu.mem.wr8(CTX + 0x0D, 1 + rng.below(5) as u8);
        cpu.mem.wr16(CTX + 0x6DA, 1 + rng.below(0x100) as u16);
        let depth = rng.below(0x2000) as i32 - 0x1000;
        cpu.mem.wr16(CTX + 0x6D0, depth as u16);
        let monster_id = [0x77u8, 0x78, 0x79, 0x5A, 0x90][rng.below(5) as usize];
        for i in 0..8u32 {
            cpu.mem.wr8(FORMATION + i, 0);
        }
        cpu.mem.wr8(FORMATION + caster - 3, monster_id);
        for s in 0..8u32 {
            let a = ACTOR + s * STRIDE;
            cpu.mem.wr32(ACTORS + s * 4, a);
            for o in (0..STRIDE).step_by(4) {
                cpu.mem.wr32(a + o, 0);
            }
            for o in [0x34u32, 0x38] {
                cpu.mem
                    .wr16(a + o, (rng.below(0x1000) as i32 - 0x800) as u16);
            }
            cpu.mem
                .wr16(a + 0x36, (rng.below(0x200) as i32 - 0x200) as u16);
            cpu.mem
                .wr16(a + 0x3E, (rng.below(0x300) as i32 - 0x280) as u16);
            cpu.mem.wr16(a + 0x46, rng.below(0x1000) as u16);
            cpu.mem
                .wr32(a + 4, [0, 0x2008_0200, 0x1234_5678][rng.below(3) as usize]);
            cpu.mem.wr8(a + 0x21C, rng.below(8) as u8);
            cpu.mem.wr8(a + 0x21D, rng.below(0x40) as u8);
            cpu.mem.wr16(
                a + 0x14C,
                if rng.below(3) == 0 {
                    0
                } else {
                    1 + rng.below(900) as u16
                },
            );
            cpu.mem.wr32(a + 0x22C, RADII + s * 0x80);
            cpu.mem
                .wr16(RADII + s * 0x80 + 0x58, rng.below(0x100) as u16);
        }
        let va = ACTOR + victim * STRIDE;
        cpu.mem.wr8(ACTOR + caster * STRIDE + 0x1DD, victim as u8);
        let status = [0u16, 0x0004, 0x0008, 0x0400, 0x0404, 0x0030, 0x1001][rng.below(7) as usize];
        cpu.mem.wr16(va + 0x16E, status);
        cpu.mem
            .wr8(va + 0x1D9, [0u8, 0, 8, 3][rng.below(4) as usize]);
        for o in [0x1EFu32, 0x1F0, 0x1F1] {
            cpu.mem.wr8(va + o, rng.below(3) as u8 * 7);
        }
        cpu.mem.wr8(va + 0x1F2, rng.below(2) as u8);
        cpu.mem.wr8(SCALAR, 8);
        cpu.mem.wr8(STEP, 1);
        let confirm = rng.below(3) == 0;
        cpu.mem.wr32(CONFIRM_MASK, 0x40);
        cpu.mem.wr32(PAD_A, if confirm { 0x40 } else { 0x10 });
        cpu.mem.wr32(PAD_B, 0);

        // The module words.
        let countdown = countdowns[rng.below(countdowns.len() as u32) as usize];
        let rot = match rng.below(4) {
            0 => (rng.below(16) as i32 - 8) << 9,
            1 => rng.below(0x4000) as i32 - 0x2000,
            2 => ((rng.below(16) as i32 - 8) << 9) + rng.below(8) as i32 - 4,
            _ => -(rng.below(0x200) as i32),
        };
        let spread = rng.below(0x100) as i32;
        let mut slots = [0u32; 8];
        for s in &mut slots {
            // A word past the table is never filled (arm 4 draws `% 8`,
            // `% 12` or `% 16`, and the image's words are zero); arms 4 and
            // 10 test it, and arm 9 would read a name past the table.
            let span = if phase != 9 && rng.below(16) == 0 {
                20
            } else {
                16
            };
            *s = rng.below(span);
        }
        let landed = rng.below(8);
        if phase == 10 && slots[landed as usize] == FatalOutcome::StealItem as u32 {
            slots[landed as usize] = FatalOutcome::Curse as u32;
        }
        let tick_rem = rng.below(0x400) as i32 - 0x200;
        cpu.mem.wr32(ROT, rot as u32);
        cpu.mem.wr32(TICK_REM, tick_rem as u32);
        cpu.mem.wr32(COUNTDOWN, countdown as u32);
        cpu.mem.wr32(SPREAD, spread as u32);
        for (i, s) in slots.iter().enumerate() {
            cpu.mem.wr32(SLOTS + 4 * i as u32, *s);
        }
        cpu.mem.wr32(LANDED, landed);
        cpu.mem.wr32(BACKDROP, PARTS + 0x800);
        cpu.mem.wr32(PARTS + 0x800 + 0x10, 0x20);
        for i in 0..8u32 {
            let p = PARTS + 0x80 * i;
            cpu.mem.wr32(ICONS + 4 * i, p);
            cpu.mem.wr32(p + 0x10, 0x20);
            cpu.mem.wr16(p + 0x14, rng.below(0x100) as u16);
            cpu.mem.wr16(p + 0x16, rng.below(0x100) as u16);
            cpu.mem.wr16(p + 0x72, 0x20 + rng.below(0x40) as u16);
        }

        // The port's view of the same state.
        let caster_a = ACTOR + caster * STRIDE;
        let mut st = FatalDecisionState {
            countdown,
            rot,
            spread,
            slots,
            landed: landed as usize,
            settle_ticks: 0,
            tick_rem,
        };
        let mut ctx = CastModuleCtx {
            party_count: 3,
            monster_count: monsters as u8,
            caster_seat: caster as u8,
            ctx_278: cpu.mem.rd8(CTX + 0x278),
            phase,
            ctx_0d: cpu.mem.rd8(CTX + 0x0D),
            ..Default::default()
        };
        let view = FatalDecisionView {
            caster: module_seat(&mut cpu.mem, caster_a),
            victim: module_seat(&mut cpu.mem, va),
            victim_seat_y: cpu.mem.rd16(va + 0x3E) as i16,
            victim_radius: cpu.mem.rd16(RADII + victim * 0x80 + 0x58) as i16,
            depth: depth as i16,
            caster_monster: monster_id,
            victim_is_party: victim < 3,
            confirm,
            victim_status: status,
            victim_hp: cpu.mem.rd16(va + 0x14C),
            victim_playing: cpu.mem.rd8(va + 0x1D9),
            victim_tint: cpu.mem.rd32(va + 4),
        };
        let seats_before: Vec<SeatWords> = (0..3 + monsters)
            .map(|s| seat_words(&mut cpu.mem, ACTOR + s * STRIDE))
            .collect();
        let hp: Vec<u16> = (0..3 + monsters)
            .map(|s| cpu.mem.rd16(ACTOR + s * STRIDE + 0x14C))
            .collect();
        let icons_before: Vec<[u16; 3]> = (0..8u32)
            .map(|i| [0x14, 0x16, 0x72].map(|o| cpu.mem.rd16(PARTS + 0x80 * i + o)))
            .collect();
        let victim_block = read_victim(&mut cpu, va);

        let stop = cpu.call(FATAL_DECISION, &[]);
        let c = format!("round {round} phase {phase:#x}");
        if stop != Stop::Returned {
            failures.push(format!("{c}: {stop:?}"));
            continue;
        }
        let busy = cpu.regs.r[V0] != 0;
        let calls = calls.borrow();
        struct Draws(Vec<u32>, usize, FatalVictim);
        impl FatalDecisionHost for Draws {
            fn rand(&mut self) -> u32 {
                let r = self.0.get(self.1).copied().unwrap_or(0);
                self.1 += 1;
                r
            }
            fn apply_outcome(&mut self, outcome: FatalOutcome) -> bool {
                match outcome {
                    FatalOutcome::GoldTithe | FatalOutcome::StealItem => true,
                    o => apply_outcome_to(&mut self.2, o).reaction,
                }
            }
        }
        let mut host = Draws(calls.draws.clone(), 0, victim_block);
        let p = fatal_decision_tick(&mut st, &mut ctx, &view, &mut host);
        let mut bad = Vec::new();
        let mut check = |what: &str, ok: bool, detail: String| {
            if !ok {
                bad.push(format!("{what}: {detail}"));
            }
        };
        let got_phase = cpu.mem.rd8(CTX + 0x279);
        check(
            "phase",
            got_phase == ctx.phase,
            format!("retail {got_phase:#x} port {:#x}", ctx.phase),
        );
        if phase <= 12 && got_phase != phase {
            passed_gate[phase as usize] += 1;
        }
        let b278 = cpu.mem.rd8(CTX + 0x278);
        check(
            "ctx+0x278",
            b278 == ctx.ctx_278,
            format!("retail {b278} port {}", ctx.ctx_278),
        );
        let b0d = cpu.mem.rd8(CTX + 0x0D);
        check(
            "ctx+0x0D",
            b0d == ctx.ctx_0d,
            format!("retail {b0d} port {}", ctx.ctx_0d),
        );
        let yaw = cpu.mem.rd16(CTX + 0x6DA);
        check(
            "ctx+0x6DA",
            (yaw == 0) == p.camera_reset,
            format!("retail {yaw} reset {}", p.camera_reset),
        );
        check(
            "busy",
            busy == !p.done,
            format!("retail {busy} port done {}", p.done),
        );
        check(
            "draws",
            host.1 == calls.draws.len(),
            format!("retail {} port {}", calls.draws.len(), host.1),
        );
        let words = [
            ("countdown", COUNTDOWN, st.countdown as u32),
            ("rot", ROT, st.rot as u32),
            ("spread", SPREAD, st.spread as u32),
            ("landed", LANDED, st.landed as u32),
        ];
        for (n, a, want) in words {
            let got = cpu.mem.rd32(a);
            check(
                n,
                got == want,
                format!("retail {:#x} port {want:#x}", got as i32),
            );
        }
        for i in 0..8u32 {
            let got = cpu.mem.rd32(SLOTS + 4 * i);
            check(
                "slot",
                got == st.slots[i as usize],
                format!("{i}: retail {got} port {}", st.slots[i as usize]),
            );
        }
        // The epilogue's wheel tick, as `World::run_fatal_decision` runs it.
        let rem = st.wheel_tick_rem(WHEEL_TICK_SPAN);
        let tick = st.tick_rem < rem;
        let got_rem = cpu.mem.rd32(TICK_REM) as i32;
        check(
            "tick rem",
            got_rem == rem,
            format!("retail {got_rem} port {rem}"),
        );
        check(
            "tick cue",
            calls.ticks == if tick { vec![0xFD] } else { vec![] },
            format!("retail {:?} port {tick}", calls.ticks),
        );
        check(
            "shot",
            calls.shots == p.shot.into_iter().collect::<Vec<_>>(),
            format!("retail {:?}\n    port {:?}", calls.shots, p.shot),
        );
        let want_spawns: Vec<(u32, u16)> = p.spawns.iter().map(|s| (s.record, s.scale)).collect();
        let got_spawns: Vec<(u32, u16)> = calls.spawns.iter().map(|s| (s.0, s.1)).collect();
        check(
            "spawns",
            got_spawns == want_spawns,
            format!("retail {got_spawns:x?}\n    port {want_spawns:x?}"),
        );
        for (k, s) in p.spawns.iter().enumerate() {
            let Some(&(_, _, handle)) = calls.spawns.get(k) else {
                break;
            };
            let at = match s.wheel_slot {
                Some(i) => ICONS + 4 * i as u32,
                None if s.record == OPEN_RECORD => BACKDROP,
                None => continue,
            };
            let got = cpu.mem.rd32(at);
            check(
                "handle",
                got == handle,
                format!("spawn {k} at {at:#x}: {got:#x} vs {handle:#x}"),
            );
        }
        check(
            "cues",
            calls.cues == p.cue.into_iter().collect::<Vec<_>>(),
            format!("retail {:x?} port {:x?}", calls.cues, p.cue),
        );
        let mut want_el = Vec::new();
        if let Some(open) = p.prompt {
            want_el.push((0x5B, u32::from(!open)));
        }
        if p.banner.is_some() {
            want_el.push((0x4C, 0));
        }
        // The gold outcome's message, the host's (`apply_fatal_outcome`).
        if p.applied == Some(FatalOutcome::GoldTithe) {
            want_el.push((0x5B, 0));
        }
        check(
            "elements",
            calls.elements == want_el,
            format!("retail {:x?} port {want_el:x?}", calls.elements),
        );
        if let Some(o) = p.banner {
            let t = cpu.mem.rd32(BANNER_TEXT);
            check(
                "banner",
                t == o.name_va(),
                format!("retail {t:#x} port {:#x}", o.name_va()),
            );
        }
        if p.prompt == Some(true) {
            let t = cpu.mem.rd32(0x8007_74AC);
            check("prompt", t == PROMPT_TEXT, format!("retail {t:#x}"));
        }
        check(
            "flash",
            (calls.flashes == 1) == p.flash,
            format!("retail {} port {}", calls.flashes, p.flash),
        );
        check(
            "framing",
            (calls.frames > 0) == p.end_frame,
            format!("retail {} port {}", calls.frames, p.end_frame),
        );
        let bd = cpu.mem.rd32(PARTS + 0x800 + 0x10) & 8 != 0;
        check(
            "backdrop retire",
            bd == p.retire_backdrop,
            format!("retail {bd} port {}", p.retire_backdrop),
        );
        let outcome_ran = calls.outcome.is_some();
        check(
            "outcome",
            outcome_ran == p.applied.is_some(),
            format!("retail {:?} port {:?}", calls.outcome, p.applied),
        );
        for i in 0..8u32 {
            let a = PARTS + 0x80 * i;
            let retired = cpu.mem.rd32(a + 0x10) & 8 != 0;
            check(
                "icon retire",
                retired == p.retire_wheel,
                format!("{i}: retail {retired}"),
            );
            let got = [0x14, 0x16, 0x72].map(|o| cpu.mem.rd16(a + o));
            let mut want = icons_before[i as usize];
            if let Some(xy) = p.place[i as usize] {
                want[0] = xy[0] as u16;
                want[1] = xy[1] as u16;
            }
            if let Some((g, d)) = p.grow
                && g == i as usize
            {
                want[1] = want[1].wrapping_add(d as u16);
                want[2] = want[2].wrapping_add(d as u16);
            }
            check(
                "icon",
                got == want,
                format!("{i}: retail {got:x?} port {want:x?}"),
            );
        }
        // The seats, the port's pass applied as the world applies it.
        let mut want_seats = seats_before.clone();
        if p.hide_all {
            for (s, w) in want_seats.iter_mut().enumerate() {
                if s < 3 || hp[s] != 0 {
                    w.tint = 0;
                    w.flag = HIDDEN_RENDER_FLAG;
                }
            }
        }
        for s in [caster as usize, victim as usize] {
            if p.halve_rates {
                want_seats[s].rate >>= 1;
            }
            if p.restore_rates {
                want_seats[s].rate <<= 1;
            }
        }
        for (s, r, f) in [
            (victim as usize, p.victim_render, p.victim_facing),
            (caster as usize, p.caster_render, p.caster_facing),
        ] {
            if let Some(r) = r {
                want_seats[s].tint = r.tint;
                want_seats[s].flag = r.flag;
            }
            if let Some(f) = f {
                want_seats[s].facing = f;
            }
        }
        if p.react {
            want_seats[victim as usize].react = 1;
        }
        for (s, want) in want_seats.iter().enumerate() {
            let got = seat_words(&mut cpu.mem, ACTOR + s as u32 * STRIDE);
            check(
                "seat",
                got == *want,
                format!("{s}: retail {got:x?}\n    port {want:x?}"),
            );
        }
        if !bad.is_empty() {
            failures.push(format!("{c} (caster {caster} victim {victim} id {monster_id:#x} cd {countdown} rot {rot:#x} confirm {confirm}):\n  {}", bad.join("\n  ")));
        }
        compared += 1;
    }
    eprintln!("{compared} passes compared, gates passed by phase {passed_gate:?}");
    assert!(compared >= 2000, "{compared}");
    assert!(passed_gate.iter().all(|&n| n > 0), "{passed_gate:?}");
    let n = failures.len();
    failures.truncate(12);
    assert!(n == 0, "{n} mismatches:\n{}", failures.join("\n"));
}

/// The steal outcome (`0x801F8164..0x801F8314`) against the port's draw,
/// `steal_party_draw` as `World::fatal_steal` runs it: the `rand()` gate, up
/// to `0x400` draws over the 256-slot bag (`0x80085958`) for an id, a count
/// and a price (`0x80074368 + id * 12 + 2`) all non-zero. Compares the item
/// consumed (`FUN_80042310(id, 1)`), the message the bar points at (the
/// built line `0x801F8FD0` or "no effect" `0x801F8D44`), whether the
/// reaction arm runs, the draws taken and the thief's cell cleared.
#[test]
fn fatal_decisions_steal_draws_as_retail_draws() {
    differential!(prot(954, FATAL_DECISION));
    let (Some(_), Some(disc)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    const BAG: u32 = 0x8008_5958;
    const ITEMS: u32 = 0x8007_4368;
    const THIEF: u32 = 0x801C_8FE0;
    const TEXT_PTR: u32 = 0x8007_74AC;
    let overlay = disc.read_entry(898).expect("PROT 0898");
    let module = disc.read_entry(954).expect("PROT 0954");
    let mut rng = Rng(0x5354_4541);
    let mut failures = Vec::new();
    let (mut hits, mut misses) = (0usize, 0usize);
    for round in 0..240usize {
        let mut cpu = scus_cpu().expect("SCUS");
        cpu.mem.load(OVERLAY_BASE, &overlay);
        cpu.mem.load(BASE, &module);
        let taken = Rc::new(RefCell::new(Vec::new()));
        let draws = Rc::new(RefCell::new(Vec::<u32>::new()));
        for target in jal_targets(&module) {
            if target != RAND {
                cpu.stub(target, 0);
            }
        }
        {
            let taken = taken.clone();
            cpu.hook(0x8004_2310, move |r, _| {
                taken.borrow_mut().push((r.r[A0] as u8, r.r[A1]))
            });
        }
        // A scripted stream: a zero gate in some rounds, then the LCG.
        let gate_zero = round % 8 == 7;
        {
            let draws = draws.clone();
            let stream = Rc::new(std::cell::Cell::new(rng.next() | 1));
            cpu.hook(RAND, move |r, _| {
                let n = stream.get().wrapping_mul(0x41C6_4E6D).wrapping_add(0x3039);
                stream.set(n);
                let v = if gate_zero && draws.borrow().is_empty() {
                    0
                } else {
                    (n >> 16) & 0x7FFF
                };
                r.r[V0] = v;
                draws.borrow_mut().push(v);
            });
        }
        cpu.mem.wr32(CTX_PTR, CTX);
        for b in 0..0x800 {
            cpu.mem.wr8(CTX + b, 0);
        }
        let caster = 3 + rng.below(3);
        cpu.mem.wr8(CTX, 3);
        cpu.mem.wr8(CTX + 1, 3);
        cpu.mem.wr8(CTX + 0x13, caster as u8);
        cpu.mem.wr8(CTX + 0x279, 10);
        for s in 0..8u32 {
            let a = ACTOR + s * STRIDE;
            cpu.mem.wr32(ACTORS + s * 4, a);
            for o in (0..STRIDE).step_by(4) {
                cpu.mem.wr32(a + o, 0);
            }
        }
        let victim = rng.below(3);
        cpu.mem.wr8(ACTOR + caster * STRIDE + 0x1DD, victim as u8);
        cpu.mem.wr8(SCALAR, 8);
        cpu.mem.wr8(STEP, 1);
        cpu.mem.wr32(COUNTDOWN, 1);
        for i in 0..8u32 {
            cpu.mem.wr32(ICONS + 4 * i, PARTS + 0x80 * i);
            cpu.mem.wr32(SLOTS + 4 * i, 0);
        }
        cpu.mem.wr32(LANDED, 0);
        cpu.mem.wr32(SLOTS, FatalOutcome::StealItem as u32);
        cpu.mem.wr32(THIEF + (caster - 3) * 4, 0x77);
        // The bag: empty, a few stacks, or many; some counts zero, some
        // items unpriced.
        let filled = [0u32, 1, 3, 40, 256][rng.below(5) as usize];
        let mut bag = vec![(0u8, 0u8); 256];
        for slot in bag.iter_mut().take(filled as usize) {
            *slot = (
                1 + rng.below(250) as u8,
                if rng.below(6) == 0 {
                    0
                } else {
                    1 + rng.below(99) as u8
                },
            );
        }
        for (i, (id, n)) in bag.iter().enumerate() {
            cpu.mem.wr8(BAG + 2 * i as u32, *id);
            cpu.mem.wr8(BAG + 2 * i as u32 + 1, *n);
        }
        let mut priced = [false; 256];
        for (id, p) in priced.iter_mut().enumerate() {
            *p = rng.below(4) != 0;
            cpu.mem.wr16(
                ITEMS + id as u32 * 12 + 2,
                if *p { 10 + id as u16 } else { 0 },
            );
        }
        let stop = cpu.call(FATAL_DECISION, &[]);
        let c = format!("round {round} ({filled} filled, gate zero {gate_zero})");
        if stop != Stop::Returned {
            failures.push(format!("{c}: {stop:?}"));
            continue;
        }
        let script = draws.borrow().clone();
        let mut k = 0usize;
        let pick = steal_party_draw(
            &bag,
            None,
            || {
                let r = script.get(k).copied().unwrap_or(0);
                k += 1;
                r
            },
            |id| priced[usize::from(id)],
        );
        let item = pick.and_then(|s| bag.get(usize::from(s)).map(|&(id, _)| id));
        if item.is_some() {
            hits += 1;
        } else {
            misses += 1;
        }
        let want_taken: Vec<(u8, u32)> = item.map(|id| (id, 1)).into_iter().collect();
        if *taken.borrow() != want_taken {
            failures.push(format!(
                "{c}: taken retail {:?} port {want_taken:?}",
                taken.borrow()
            ));
        }
        if k != script.len() {
            failures.push(format!("{c}: draws retail {} port {k}", script.len()));
        }
        let text = cpu.mem.rd32(TEXT_PTR);
        let want_text = if item.is_some() {
            0x801F_8FD0
        } else {
            NO_EFFECT_TEXT
        };
        if text != want_text {
            failures.push(format!("{c}: text retail {text:#x} port {want_text:#x}"));
        }
        // The reaction arm runs on a hit: the phase stops at 11.
        let phase = cpu.mem.rd8(CTX + 0x279);
        let want_phase = if item.is_some() { 11 } else { 12 };
        if phase != want_phase {
            failures.push(format!("{c}: phase retail {phase} port {want_phase}"));
        }
        let cell = cpu.mem.rd32(THIEF + (caster - 3) * 4);
        if item.is_some() && cell != 0 {
            failures.push(format!("{c}: thief cell {cell:#x}"));
        }
    }
    eprintln!("{hits} hits, {misses} misses");
    assert!(hits > 50 && misses > 30, "{hits} {misses}");
    let n = failures.len();
    failures.truncate(20);
    assert!(n == 0, "{n} mismatches:\n{}", failures.join("\n"));
}
