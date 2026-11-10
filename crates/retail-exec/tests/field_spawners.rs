//! Three field-overlay spawners (PROT 0897), each with the allocator
//! `FUN_80020DE0` answering a cleared actor, held against the port's spawn
//! for the record the per-frame tick then reads: the floor-ladder oscillator
//! `FUN_801DDE34` against `FloorTierBob::spawn`, the colour tween
//! `FUN_801DE2B0` against `tween_from_fade_template`, and the eased move
//! `FUN_801DE698` against `EasedMove::spawn`.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_field::actor_colour_tween::tween_from_fade_template;
use legaia_engine_field::field_actor_timers::{EasedMove, FloorTierBob};
use legaia_engine_system::fade::FadeTemplate;
use psx_retail_exec::{Cpu, Stop, V0};

const LADDER: u32 = 0x801D_DE34;
const TWEEN: u32 = 0x801D_E2B0;
const EASE: u32 = 0x801D_E698;
const ALLOC: u32 = 0x8002_0DE0;
const OVERLAY_BASE: u32 = 0x801C_E818;
const ACTOR: u32 = 0x8013_0000;
const BLOCK: u32 = 0x8013_1000;
const BLOCK2: u32 = 0x8013_1100;
const TARGET: u32 = 0x8013_2000;
const LUT: u32 = 0x1F80_035C;
const STACK_TOP: u32 = 0x801F_FF00;

fn retail_cpu() -> Option<Cpu> {
    let mut cpu = scus_cpu()?;
    cpu.mem
        .load(OVERLAY_BASE, &disc()?.read_entry(897).expect("PROT 0897"));
    cpu.stub(ALLOC, ACTOR);
    Some(cpu)
}

fn spawn(cpu: &mut Cpu, entry: u32, args: &[u32]) {
    cpu.mem.load(ACTOR, &[0u8; 0x100]);
    cpu.regs.r[V0] = 0;
    assert_eq!(cpu.call(entry, args), Stop::Returned);
    assert_eq!(cpu.regs.r[V0], ACTOR);
}

#[test]
fn the_floor_ladder_spawn_seeds_what_retail_seeds() {
    differential!(prot(897, LADDER));
    let Some(mut cpu) = retail_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x5350_4C44);
    for round in 0..20_000 {
        let slot = rng.below(16) as u16;
        let sub_op = rng.below(3) as u8;
        // The spawner divides by the period and by the period plus one, each
        // behind a `break`.
        let period = loop {
            let p = [1i16, 2, 8, 30, 0x78, rng.next() as i16][rng.below(6) as usize];
            if p != 0 && p != -1 {
                break p;
            }
        };
        let amplitude = [0i16, 1, 8, -8, 0x40, rng.next() as i16][rng.below(6) as usize];
        let arm = rng.next() as i16;
        let lut: Vec<u8> = (0..32).map(|_| rng.next() as u8).collect();
        cpu.mem.load(LUT, &lut);
        let seed = i16::from_le_bytes([lut[usize::from(slot) * 2], lut[usize::from(slot) * 2 + 1]]);
        cpu.mem.wr32(STACK_TOP + 0x10, arm as i32 as u32);
        spawn(
            &mut cpu,
            LADDER,
            &[
                u32::from(slot),
                u32::from(sub_op),
                period as i32 as u32,
                amplitude as i32 as u32,
            ],
        );
        let b = FloorTierBob::spawn(slot, sub_op, period, amplitude, arm, seed);
        let label = format!("round {round} slot {slot} period {period} amplitude {amplitude}");
        assert_eq!(cpu.mem.rd16(ACTOR + 0x50), b.slot, "{label}");
        assert_eq!(cpu.mem.rd16(ACTOR + 0x54) as i16, b.phase, "{label}");
        assert_eq!(cpu.mem.rd8(ACTOR + 0x6C), b.phase_seed, "{label}");
        assert_eq!(cpu.mem.rd32(ACTOR + 0x80) as i32, b.pos, "{label}");
        assert_eq!(cpu.mem.rd32(ACTOR + 0x84) as i32, b.vel, "{label}");
        assert_eq!(cpu.mem.rd32(ACTOR + 0x88) as i32, b.accel, "{label}");
        assert_eq!(cpu.mem.rd32(ACTOR + 0x8C) as i32, b.target, "{label}");
        assert_eq!(cpu.mem.rd16(ACTOR + 0x9E), b.arm, "{label}");
    }
}

#[test]
fn the_colour_tween_spawn_seeds_what_retail_seeds() {
    differential!(prot(897, TWEEN));
    let Some(mut cpu) = retail_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x5350_5457);
    for round in 0..5000 {
        let words: Vec<i16> = (0..13).map(|_| rng.next() as i16).collect();
        for (i, w) in words.iter().enumerate() {
            cpu.mem.wr16(BLOCK + i as u32 * 2, *w as u16);
        }
        let t = FadeTemplate {
            kind: words[0],
            duration: words[1],
            start_rgb: [words[3], words[4], words[5]],
            end_rgb: [words[7], words[8], words[9]],
            mode: [words[10], words[11], words[12]],
        };
        let kind = rng.next() as i16;
        spawn(&mut cpu, TWEEN, &[BLOCK, kind as i32 as u32]);
        let c = tween_from_fade_template(&t, kind);
        let label = format!("round {round}");
        for (o, v) in [
            (0xB8, c.from.0),
            (0xBA, c.from.1),
            (0xBC, c.from.2),
            (0xBE, c.to.0),
            (0xC0, c.to.1),
            (0xC2, c.to.2),
            (0xC4, c.delay as u16),
            (0xC6, c.hold as u16),
            (0xC8, c.clock),
            (0xD2, c.push_blend as u16),
            (0xD4, c.duration as u16),
            (0xD6, c.push_kind as u16),
        ] {
            assert_eq!(cpu.mem.rd16(ACTOR + o), v, "{label} +{o:#x}");
        }
    }
}

#[test]
fn the_eased_move_spawn_seeds_what_retail_seeds() {
    differential!(prot(897, EASE));
    let Some(mut cpu) = retail_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x5350_4541);
    for round in 0..5000 {
        let start: [i16; 3] = std::array::from_fn(|_| rng.next() as i16);
        let end: [i16; 3] = std::array::from_fn(|_| rng.next() as i16);
        let ticks = rng.next() as i16;
        for i in 0..3 {
            cpu.mem.wr16(BLOCK + i * 2, start[i as usize] as u16);
            cpu.mem.wr16(BLOCK2 + i * 2, end[i as usize] as u16);
        }
        spawn(
            &mut cpu,
            EASE,
            &[TARGET, BLOCK, BLOCK2, ticks as i32 as u32],
        );
        let m = EasedMove::spawn(start, end, ticks);
        let label = format!("round {round}");
        assert_eq!(cpu.mem.rd32(ACTOR + 0x90), TARGET, "{label}");
        assert_eq!(cpu.mem.rd16(ACTOR + 0x50), m.clock, "{label}");
        assert_eq!(cpu.mem.rd16(ACTOR + 0x9E) as i16, m.duration, "{label}");
        for i in 0..3u32 {
            assert_eq!(
                cpu.mem.rd16(ACTOR + 0x14 + i * 2) as i16,
                m.start[i as usize],
                "{label}"
            );
            assert_eq!(
                cpu.mem.rd16(ACTOR + 0x24 + i * 2) as i16,
                m.end[i as usize],
                "{label}"
            );
        }
        assert!(!m.retired);
    }
}
