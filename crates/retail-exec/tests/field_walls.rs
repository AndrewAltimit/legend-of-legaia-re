//! `FUN_801CFE4C`'s static-wall arm (result bit `2`), the walk step's collision,
//! against `World::field_dir_blocked`, over generated grids and every position
//! class: the routine's three probes, its signed cell derivation and its
//! four-way bit select.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_core::world::World;
use psx_retail_exec::{Stop, V0};

const COLLIDE: u32 = 0x801C_FE4C;
const OVERLAY_BASE: u32 = 0x801C_E818;
const MAP_PTR: u32 = 0x1F80_03EC;
const MAP: u32 = 0x8010_0000;
const GRID: u32 = MAP + 0x4000;
const GRID_LEN: usize = 0x4000;
const ACTOR: u32 = 0x8013_0000;
const ACTOR_COUNT: u32 = 0x8007_B6B8;

#[test]
fn the_walk_step_hits_the_walls_retail_hits() {
    let (Some(mut cpu), Some(disc)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &disc.read_entry(897).expect("PROT 0897"));
    cpu.mem.wr32(MAP_PTR, MAP);
    cpu.mem.wr32(ACTOR_COUNT, 0);
    let mut world = World::new();
    let mut rng = Rng(0x5741_4C4C);
    let (mut failures, mut blocked) = (Vec::new(), 0);
    for grid_round in 0..40 {
        let density = [1, 4, 16, 64][rng.below(4) as usize];
        let grid: Vec<u8> = (0..GRID_LEN)
            .map(|_| {
                if rng.below(density) == 0 {
                    rng.below(256) as u8
                } else {
                    0
                }
            })
            .collect();
        cpu.mem.load(GRID, &grid);
        world.terrain.collision_grid = grid;
        for _ in 0..2000 {
            // The grid is 128 x 128 tiles of 128 units; cell boundaries fall on
            // the multiples of 64, and the walk reaches them all.
            let (x, z) = match rng.below(4) {
                0 => (
                    (rng.below(0x100) * 64 + rng.below(3)) as i16,
                    (rng.below(0x100) * 64 + rng.below(3)) as i16,
                ),
                1 => (rng.below(0x80) as i16 - 0x40, rng.below(0x80) as i16 - 0x40),
                _ => (60 + rng.below(0x3F00) as i16, 60 + rng.below(0x3F00) as i16),
            };
            let dir = rng.below(4);
            cpu.mem.wr16(ACTOR + 0x14, x as u16);
            cpu.mem.wr16(ACTOR + 0x18, z as u16);
            cpu.regs.r[V0] = 0;
            let stop = cpu.call(COLLIDE, &[ACTOR, 0, dir]);
            assert_eq!(stop, Stop::Returned);
            let retail = cpu.regs.r[V0] & 2 != 0;
            let port = world.field_dir_blocked(x, z, dir as usize);
            blocked += u32::from(retail);
            if retail != port && failures.len() < 8 {
                failures.push(format!(
                    "grid {grid_round} ({x}, {z}) dir {dir}: retail {retail} port {port}"
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(blocked > 1000, "{blocked} blocked");
}
