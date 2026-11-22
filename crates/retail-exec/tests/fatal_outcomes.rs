//! PROT 0954 (Fatal Decision)'s outcome arm against `apply_outcome_to`.
//!
//! Arm 10 (`0x801F7CE4`) drains the module countdown `0x801F9018`, retires the
//! eight icons, steps the phase and jumps through the table at `0x801F6A18` on
//! the landed slot's value (`0x801F9020[*0x801F9080]`). The run seats the arm
//! one drain from passing on a victim with seeded stats - the halving floors'
//! edge values among them, Stone and not - and compares every halfword the
//! outcome arms write with the port's kernel, plus the gold tithe's word.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_cast::cast_fatal_decision::{
    FatalOutcome, FatalVictim, apply_outcome_to, gold_tithe,
};
use psx_retail_exec::Stop;

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
        .chunks_exact(4)
        .map(|w| u32::from_le_bytes(w.try_into().unwrap()))
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
                if (round + k) % 3 == 0 {
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

            let mut want = read_victim(&mut cpu, va);
            apply_outcome_to(&mut want, outcome);
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
