//! Four fishing-overlay leaves of PROT 0972 against `engine-fishing`: the
//! cadence ring reset `FUN_801D746C`, the lure-ownership gate `FUN_801D712C`,
//! the lure's walk-grid probe `FUN_801D7030`, and the lead angler's tick
//! `FUN_801D2278` (facing, dwell roll, ripple spawn and camera publish) run
//! frame by frame against `FishWander`.

use crate::common::{Rng, diff_offsets, disc, scus_cpu};
use legaia_engine_fishing::fishing::{lure_item_id, select_owned_rod};
use legaia_engine_fishing::fishing_actors::{FishWander, walk_grid_overhead};
use legaia_engine_fishing::fishing_chrome::clear_slot_ring;
use legaia_minigame_formats::fishing_species as fs;
use psx_retail_exec::{A0, A1, A2, Cpu, Stop, V0};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

const RESET: u32 = 0x801D_746C;
const GATE: u32 = 0x801D_712C;
const PROBE: u32 = 0x801D_7030;
const WANDER: u32 = 0x801D_2278;
const CARD: u32 = 0x801D_5298;
const BAG_COUNT: u32 = 0x8004_2F4C;
const RAND: u32 = 0x8005_6798;
const SPAWN: u32 = 0x8002_1B04;

fn rig() -> Option<Cpu> {
    let (mut cpu, d) = (scus_cpu()?, disc()?);
    let overlay = d
        .read_entry(fs::FISHING_OVERLAY_PROT_INDEX)
        .expect("PROT 0972");
    cpu.mem.load(fs::FISHING_OVERLAY_BASE_VA, &overlay);
    Some(cpu)
}

#[test]
fn the_cadence_ring_clears_as_retail_clears_it() {
    differential!(prot(972, RESET));
    let Some(mut cpu) = rig() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    const INDEX: u32 = 0x801D_91DC;
    const WINDOW: (u32, usize) = (0x801D_9100, 0x200);
    let mut rng = Rng(0x5249_4E47);
    for round in 0..200 {
        let junk: Vec<u8> = (0..WINDOW.1).map(|_| rng.next() as u8).collect();
        cpu.mem.load(WINDOW.0, &junk);
        assert_eq!(cpu.call(RESET, &[]), Stop::Returned);
        let retail = cpu.mem.read(WINDOW.0, WINDOW.1);
        let mut port = junk.clone();
        let word = |b: &[u8], a: u32| {
            let o = (a - WINDOW.0) as usize;
            u32::from_le_bytes(b[o..o + 4].try_into().unwrap())
        };
        let mut index = word(&junk, INDEX);
        let mut slots: [(u32, u32); 16] =
            std::array::from_fn(|i| (word(&junk, 0x801D_91E4 + i as u32 * 8), 0));
        for (i, s) in slots.iter_mut().enumerate() {
            s.1 = word(&junk, 0x801D_91E8 + i as u32 * 8);
        }
        clear_slot_ring(&mut index, &mut slots);
        let mut put = |a: u32, v: u32| {
            let o = (a - WINDOW.0) as usize;
            port[o..o + 4].copy_from_slice(&v.to_le_bytes());
        };
        put(INDEX, index);
        for (i, s) in slots.iter().enumerate() {
            put(0x801D_91E4 + i as u32 * 8, s.0);
            put(0x801D_91E8 + i as u32 * 8, s.1);
        }
        assert!(
            retail == port,
            "round {round}: {:?}",
            diff_offsets(&retail, &port)
        );
    }
}

#[test]
fn the_lure_gate_re_points_as_retail_does() {
    differential!(prot(972, GATE));
    let Some(mut cpu) = rig() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    const LURE: u32 = 0x8008_4450;
    let counts = Rc::new(RefCell::new([0i32; 3]));
    let bag = counts.clone();
    cpu.hook(BAG_COUNT, move |r, _| {
        let id = r.r[A0];
        r.r[V0] = if (0x9D..=0x9F).contains(&id) {
            bag.borrow()[(id - 0x9D) as usize] as u32
        } else {
            0
        };
    });
    let mut rng = Rng(0x4C55_5245);
    let mut moved = 0;
    for round in 0..20_000 {
        let held: [i32; 3] = std::array::from_fn(|_| match rng.below(3) {
            0 => 0,
            1 => 1,
            _ => rng.below(99) as i32,
        });
        let index = rng.below(3);
        *counts.borrow_mut() = held;
        cpu.mem.wr32(LURE, index);
        assert_eq!(cpu.call(GATE, &[]), Stop::Returned);
        let mut port_index = index;
        let owned = select_owned_rod(&mut port_index, |id| {
            if (0x9D..=0x9F).contains(&id) {
                held[(id - 0x9D) as usize]
            } else {
                0
            }
        });
        assert_eq!(lure_item_id(0), 0x9D);
        assert_eq!(
            (cpu.regs.r[V0] != 0, cpu.mem.rd32(LURE)),
            (owned, port_index),
            "round {round} held {held:?} index {index}"
        );
        moved += u32::from(port_index != index);
    }
    assert!(moved > 1000, "{moved}");
}

#[test]
fn the_walk_grid_probe_reads_the_bit_retail_reads() {
    differential!(prot(972, PROBE));
    let Some(mut cpu) = rig() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    const MAP: u32 = 0x8012_0000;
    const GRID_LEN: usize = 0x4000;
    let mut rng = Rng(0x4752_4944);
    let grid: Vec<u8> = (0..GRID_LEN)
        .map(|_| {
            if rng.below(3) == 0 {
                rng.next() as u8
            } else {
                0
            }
        })
        .collect();
    cpu.mem.wr32(0x1F80_03EC, MAP);
    cpu.mem.load(MAP + 0x4000, &grid);
    let mut both = [0; 2];
    for round in 0..100_000 {
        let span = [0x2000, 0x8000, 0x10000][rng.below(3) as usize];
        let x = rng.below(span) as i32 - (span / 4) as i32;
        let z = rng.below(span) as i32 - (span / 4) as i32;
        assert_eq!(cpu.call(PROBE, &[x as u32, z as u32]), Stop::Returned);
        let retail = cpu.regs.r[V0] != 0;
        assert_eq!(
            retail,
            walk_grid_overhead(&grid, x, z),
            "round {round} x {x} z {z}"
        );
        both[usize::from(retail)] += 1;
    }
    assert!(both[0] > 10_000 && both[1] > 10_000, "{both:?}");
}

/// One `FUN_80021B04(position, rotation, descriptor, scale)` ripple spawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Ripple {
    at: [i16; 3],
    rotation: [i16; 3],
    descriptor: u32,
}

#[test]
fn the_lead_angler_ticks_as_retail_ticks_him() {
    differential!(prot(972, WANDER));
    let Some(mut cpu) = rig() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    const ACTOR: u32 = 0x8014_0000;
    const MODE: u32 = 0x801D_926C;
    const DWELL: u32 = 0x801D_9060;
    const HELD: u32 = 0x8007_B850;
    const YAW: u32 = 0x8007_B792;
    const PITCH: u32 = 0x8008_40BC;
    const TRANSLATION: u32 = 0x8008_9118;
    const FRAME_STEP: u32 = 0x1F80_0393;
    let state = Rc::new(Cell::new(0u32));
    let stream = state.clone();
    cpu.hook(RAND, move |r, _| {
        let next = stream.get().wrapping_mul(0x41C6_4E6D).wrapping_add(0x3039);
        stream.set(next);
        r.r[V0] = (next >> 16) & 0x7FFF;
    });
    let ripples = Rc::new(RefCell::new(Vec::new()));
    let sink = ripples.clone();
    cpu.hook(SPAWN, move |r, m| {
        let h = |m: &mut psx_retail_exec::Memory, a: u32| m.rd16(a) as i16;
        let (p, q) = (r.r[A0], r.r[A1]);
        sink.borrow_mut().push(Ripple {
            at: [h(m, p), h(m, p + 2), h(m, p + 4)],
            rotation: [h(m, q), h(m, q + 2), h(m, q + 4)],
            descriptor: r.r[A2],
        });
    });
    let mut rng = Rng(0x414E_474C);
    let mut rolls = 0;
    for run in 0..300 {
        let (x, y, z) = (
            rng.below(0x4000) as i16,
            rng.below(0x200) as i16 - 0x100,
            rng.below(0x4000) as i16,
        );
        let mut port = FishWander::new(x, y, z);
        for o in (0..0x80).step_by(4) {
            cpu.mem.wr32(ACTOR + o, 0);
        }
        cpu.mem.wr16(ACTOR + 0x14, x as u16);
        cpu.mem.wr16(ACTOR + 0x16, y as u16);
        cpu.mem.wr16(ACTOR + 0x18, z as u16);
        cpu.mem.wr16(ACTOR + 0x26, port.facing as u16);
        cpu.mem.wr32(MODE, 0xC);
        cpu.mem.wr32(DWELL, 0);
        let seed = rng.next();
        state.set(seed);
        let mut port_state = seed;
        for frame in 0..1500 {
            let dt = 1 + rng.below(3) as i32;
            let held: u16 = [0, 0x8000, 0x2000, 0xA000, 0x40][rng.below(5) as usize];
            cpu.mem.wr8(FRAME_STEP, dt as u8);
            cpu.mem.wr32(HELD, u32::from(held));
            ripples.borrow_mut().clear();
            assert_eq!(cpu.call(WANDER, &[ACTOR]), Stop::Returned);
            let rolled = port.tick(held, dt, || {
                port_state = port_state.wrapping_mul(0x41C6_4E6D).wrapping_add(0x3039);
                (port_state >> 16) & 0x7FFF
            });
            let cam = port.camera();
            let retail = (
                cpu.mem.rd16(ACTOR + 0x26) as i16,
                ripples.borrow().clone(),
                cpu.mem.rd16(YAW) as i16,
                [0, 4, 8].map(|o| cpu.mem.rd32(TRANSLATION + o) as i32),
                cpu.mem.rd32(PITCH) as i32,
                state.get(),
            );
            let ported = (
                port.facing,
                rolled
                    .map(|t| Ripple {
                        at: [t.x as i16, y, t.z as i16],
                        rotation: [0, t.rotation as i16, 0],
                        descriptor: if t.ripple_variant == 1 {
                            0x801D_8ED8
                        } else {
                            0x801D_8F44
                        },
                    })
                    .into_iter()
                    .collect::<Vec<_>>(),
                cam.yaw,
                [cam.translation.0, cam.translation.1, cam.translation.2],
                cam.pitch_term,
                port_state,
            );
            assert_eq!(retail, ported, "run {run} frame {frame} dt {dt}");
            rolls += u32::from(rolled.is_some());
        }
    }
    assert!(rolls > 2000, "{rolls}");
}

/// The landed-catch card `FUN_801D5298`, run frame by frame until it has
/// credited the catch and settled the best-catch record, against
/// `FishingSpecies::score_for` and `FishingRecord::credit`. Its draws, the
/// clip driver and the sprite calls are stubbed; the record words are what it
/// owns.
#[test]
fn a_landed_catch_credits_what_retail_credits() {
    differential!(prot(972, CARD));
    let Some(mut cpu) = rig() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    use legaia_engine_fishing::fishing::{FISH_POINTS_CAP, FishingRecord};
    let overlay = disc()
        .unwrap()
        .read_entry(fs::FISHING_OVERLAY_PROT_INDEX)
        .unwrap();
    let species = fs::parse(&overlay).expect("species");
    for va in [
        0x8003_D2C4,
        0x8002_04F8,
        0x8001_72C0,
        0x8003_D344,
        0x801D_67BC,
        0x801D_63B0,
        0x801D_76E0,
        0x801D_73B8,
    ] {
        cpu.stub(va, 0);
    }
    const ACTOR: u32 = 0x8014_0000;
    const SPECIES: u32 = 0x801D_91CC;
    const STRENGTH: u32 = 0x801D_91B8;
    const LIFT: u32 = 0x801D_9294;
    const POINTS: u32 = 0x8008_444C;
    const BEST: u32 = 0x8008_4458;
    const BEST_FISH: u32 = 0x8008_445C;
    const PACKETS: u32 = 0x1F80_03A0;
    const FRAME_STEP: u32 = 0x1F80_0393;
    let mut rng = Rng(0x4341_5443);
    let (mut capped, mut bested) = (0, 0);
    for round in 0..20_000 {
        let sp = species[rng.below(species.len() as u32) as usize];
        // The hook-time sum stays under `0x800` (`hook_strength`: a class
        // roll below the water weight, at most 500, plus the line, spread and
        // rod terms); the card's 32-bit product only wraps past `0x8000` on
        // the dearest fish, which no hook reaches.
        let strength = match rng.below(2) {
            0 => rng.below(0x800) as i32,
            _ => rng.below(0x4000) as i32,
        };
        let points = match rng.below(3) {
            0 => FISH_POINTS_CAP - rng.below(2000) as i32,
            _ => rng.below(500_000) as i32,
        };
        let best = rng.below(4000) as i32;
        let best_fish = rng.below(10) as usize;
        for o in (0..0x80).step_by(4) {
            cpu.mem.wr32(ACTOR + o, 0);
        }
        cpu.mem.wr32(SPECIES, sp.index as u32);
        cpu.mem.wr32(STRENGTH, strength as u32);
        cpu.mem.wr32(LIFT, 1);
        cpu.mem.wr32(POINTS, points as u32);
        cpu.mem.wr32(BEST, best as u32);
        cpu.mem.wr32(BEST_FISH, best_fish as u32);
        let mut frames = 0;
        while cpu.mem.rd16(ACTOR + 0x2A) != 2 {
            cpu.mem.wr32(PACKETS, 0x8018_0000);
            cpu.mem.wr8(FRAME_STEP, 1 + rng.below(3) as u8);
            assert_eq!(cpu.call(CARD, &[ACTOR]), Stop::Returned);
            frames += 1;
            assert!(frames < 2000, "round {round}");
        }
        let mut port = FishingRecord {
            points,
            best_points: best,
            best_fish,
        };
        port.credit(sp.index, sp.score_for(strength));
        let retail = (
            cpu.mem.rd32(POINTS) as i32,
            cpu.mem.rd32(BEST) as i32,
            cpu.mem.rd32(BEST_FISH) as usize,
        );
        assert_eq!(
            retail,
            (port.points, port.best_points, port.best_fish),
            "round {round} species {} strength {strength:#x} points {points} best {best}",
            sp.index
        );
        capped += u32::from(retail.0 == FISH_POINTS_CAP);
        bested += u32::from(retail.1 != best);
    }
    assert!(capped > 100 && bested > 100, "{capped} {bested}");
}
