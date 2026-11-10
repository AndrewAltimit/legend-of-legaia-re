//! The L1 map display's two leaves in PROT 0981: the location stepper
//! `FUN_801CF5E8` against `step_cursor` over generated label tables, both
//! directions, and the survey camera clamp `FUN_801CF678` against
//! `clamp_camera` over the whole plane.
//!
//! The stepper is held to the inputs its caller hands it: forward from a
//! cursor `>= 0` (the tick passes `cursor + 1`), backward from below the count
//! over a table holding the kingdom (retail's backward walk spins for ever on
//! one that does not).

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_field::map_display::{MapLocation, clamp_camera, step_cursor};
use psx_retail_exec::{Cpu, Stop};

const STEP: u32 = 0x801C_F5E8;
const CLAMP: u32 = 0x801C_F678;
const OVERLAY_BASE: u32 = 0x801C_E818;
const TABLE_PTR: u32 = 0x8007_3EE0;
const TABLE: u32 = 0x8014_0000;
const KINGDOM: u32 = 0x8007_B970;
const CURSOR: u32 = 0x801C_F77C;
const CAM_X: u32 = 0x8008_9118;
const CAM_Z: u32 = 0x8008_9120;

fn retail_cpu() -> Option<Cpu> {
    let mut cpu = scus_cpu()?;
    cpu.mem
        .load(OVERLAY_BASE, &disc()?.read_entry(981).expect("PROT 0981"));
    Some(cpu)
}

#[test]
fn the_location_stepper_lands_where_retail_lands() {
    differential!(prot(981, STEP));
    let Some(mut cpu) = retail_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem.wr32(TABLE_PTR, TABLE);
    let mut rng = Rng(0x4D41_5053);
    let (mut parked, mut found, mut wrapped) = (0, 0, 0);
    for round in 0..20_000 {
        let count = 1 + rng.below(29) as usize;
        let kingdom = rng.below(4);
        let mut locations: Vec<MapLocation> = (0..count)
            .map(|_| MapLocation {
                region: rng.below(4) as u8,
                map_x: rng.next() as u8,
                map_y: rng.next() as u8,
                discovery_flag: 0,
                name: Vec::new(),
            })
            .collect();
        let backward = rng.below(2) == 0;
        if backward && !locations.iter().any(|l| u32::from(l.region) == kingdom) {
            let i = rng.below(count as u32) as usize;
            locations[i].region = kingdom as u8;
        }
        let mut table = vec![0u8; 1 + count * 0x20];
        table[0] = count as u8;
        for (i, l) in locations.iter().enumerate() {
            let r = 1 + i * 0x20;
            table[r] = l.region;
            table[r + 1] = l.map_x;
            table[r + 2] = l.map_y;
            for b in &mut table[r + 3..r + 0x20] {
                *b = rng.next() as u8;
            }
        }
        cpu.mem.load(TABLE, &table);
        cpu.mem.wr32(KINGDOM, kingdom);
        let cursor = if backward {
            rng.below(count as u32 + 1) as i32 - 1
        } else {
            rng.below(count as u32 + 2) as i32
        };
        cpu.mem.wr32(CURSOR, cursor as u32);
        assert_eq!(cpu.call(STEP, &[u32::from(backward)]), Stop::Returned);
        let port = step_cursor(cursor, backward, &locations, kingdom);
        assert_eq!(
            cpu.mem.rd32(CURSOR) as i32,
            port,
            "round {round} cursor {cursor} backward {backward}"
        );
        if port < 0 {
            parked += 1;
        } else {
            found += 1;
            wrapped += u32::from(backward && cursor < 0);
        }
    }
    assert!(
        parked > 1000 && found > 10_000 && wrapped > 500,
        "{parked} {found} {wrapped}"
    );
}

#[test]
fn the_survey_camera_clamp_matches_retail() {
    differential!(prot(981, CLAMP));
    let Some(mut cpu) = retail_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x434C_4D50);
    for _ in 0..20_000 {
        let pick = |rng: &mut Rng| match rng.below(3) {
            0 => rng.next() as i32,
            1 => -(rng.below(0x4000) as i32),
            _ => [
                -0xD00, -0xCFF, -0xC00, -0xBFF, -0x3380, -0x3381, -0x3580, -0x3581, 0,
            ][rng.below(9) as usize],
        };
        let (mut x, mut z) = (pick(&mut rng), pick(&mut rng));
        cpu.mem.wr32(CAM_X, x as u32);
        cpu.mem.wr32(CAM_Z, z as u32);
        assert_eq!(cpu.call(CLAMP, &[]), Stop::Returned);
        let (x0, z0) = (x, z);
        clamp_camera(&mut x, &mut z);
        assert_eq!(
            (cpu.mem.rd32(CAM_X) as i32, cpu.mem.rd32(CAM_Z) as i32),
            (x, z),
            "({x0}, {z0})"
        );
    }
}
