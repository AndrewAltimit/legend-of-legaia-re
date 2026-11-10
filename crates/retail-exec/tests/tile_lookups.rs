//! The field overlay's two tile readers, called on their own: the
//! walkability probe `FUN_801D56C4` against `World::field_tile_is_wall` over
//! generated collision grids and the whole `i16` plane, and the trigger-block
//! record lookup `FUN_801D5AE0` with its primary-then-fallback caller
//! `FUN_801D5630`, kind `2`, against `parse_elevation_overrides` and
//! `lookup_elevation_override` over every field scene's real `.MAP`.

use crate::common::{Rng, disc, scus_cpu};
use crate::walk_on::{index, map_image};
use legaia_engine_core::world::World;
use legaia_engine_field::field_elevation::{lookup_elevation_override, parse_elevation_overrides};
use legaia_engine_scene::scene::Scene;
use psx_retail_exec::{Cpu, Stop, V0};

const PROBE: u32 = 0x801D_56C4;
const LOOKUP: u32 = 0x801D_5630;
const RECORD: u32 = 0x801D_5AE0;
const OVERLAY_BASE: u32 = 0x801C_E818;
const MAP_PTR: u32 = 0x1F80_03EC;
const MAP: u32 = 0x8010_0000;
const GRID: u32 = MAP + 0x4000;
const GRID_LEN: usize = 0x4000;
const PRIMARY: usize = 0x1_0000;
const FALLBACK: usize = 0x1_2000;

fn retail_cpu() -> Option<Cpu> {
    let mut cpu = scus_cpu()?;
    cpu.mem
        .load(OVERLAY_BASE, &disc()?.read_entry(897).expect("PROT 0897"));
    cpu.mem.wr32(MAP_PTR, MAP);
    Some(cpu)
}

fn call(cpu: &mut Cpu, entry: u32, args: &[u32]) -> u32 {
    cpu.regs.r[V0] = 0xDEAD_BEEF;
    assert_eq!(
        cpu.call(entry, args),
        Stop::Returned,
        "{entry:#x} {args:x?}"
    );
    cpu.regs.r[V0]
}

#[test]
fn the_walkability_probe_matches_retail() {
    differential!(prot(897, PROBE));
    let Some(mut cpu) = retail_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut w = World::new();
    let mut rng = Rng(0x5052_4F42);
    let mut walls = 0;
    for _ in 0..20 {
        let density = [2, 4, 16][rng.below(3) as usize];
        let grid: Vec<u8> = (0..GRID_LEN)
            .map(|_| {
                if rng.below(density) == 0 {
                    rng.next() as u8
                } else {
                    rng.next() as u8 & 0x0F
                }
            })
            .collect();
        cpu.mem.load(GRID, &grid);
        w.terrain.collision_grid = grid;
        for _ in 0..5000 {
            let (x, z) = match rng.below(3) {
                // The whole plane: negative and past-the-grid coordinates wrap.
                0 => (rng.next() as i16, rng.next() as i16),
                // Around the sub-cell edges, where the biases decide.
                1 => (
                    (rng.below(0x80) as i16 * 64 + rng.below(5) as i16 - 2),
                    (rng.below(0x80) as i16 * 64 + rng.below(5) as i16 - 2),
                ),
                _ => (rng.below(0x4000) as i16, rng.below(0x4000) as i16),
            };
            let r = call(&mut cpu, PROBE, &[x as i32 as u32, z as i32 as u32]);
            let p = w.field_tile_is_wall(x, z);
            assert_eq!(r != 0, p, "({x}, {z}) -> {r:#x}");
            walls += u32::from(p);
        }
    }
    assert!(walls > 10_000, "{walls}");
}

#[test]
fn every_elevation_record_is_found_where_retail_finds_it() {
    differential!(prot(897, LOOKUP), prot(897, RECORD));
    let Some(mut cpu) = retail_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let index = index();
    let (mut scenes, mut hits, mut fallback_hits) = (0, 0, 0);
    for name in index.cdname_scene_names() {
        let Ok(scene) = Scene::load(&index, &name) else {
            continue;
        };
        if scene.field_map_index(&index).is_none() {
            continue;
        }
        let Some(image) = map_image(&index, &scene) else {
            continue;
        };
        scenes += 1;
        cpu.mem.load(MAP, &image);
        let primary = parse_elevation_overrides(&image[PRIMARY..FALLBACK]);
        let fallback = parse_elevation_overrides(&image[FALLBACK..]);
        let both: Vec<_> = primary.iter().chain(fallback.iter()).copied().collect();
        // Every tile, and a few rows and columns past the grid.
        for z in 0..0x84u32 {
            for x in 0..0x84u32 {
                let r = call(&mut cpu, LOOKUP, &[2, x, z]);
                let p = lookup_elevation_override(&both, x as u8, z as u8);
                let label = format!("{name} ({x}, {z})");
                match p {
                    None => assert_eq!(r, 0, "{label}"),
                    Some(rec) => {
                        assert_ne!(r, 0, "{label}");
                        assert_eq!(
                            cpu.mem.read(r, 4),
                            [rec.tile_x, rec.tile_z, rec.coarse as u8, rec.quads],
                            "{label}"
                        );
                        hits += 1;
                        // Which block answered.
                        let in_primary = lookup_elevation_override(&primary, x as u8, z as u8);
                        let rp = call(&mut cpu, RECORD, &[2, x, z, MAP + PRIMARY as u32]);
                        assert_eq!(rp != 0, in_primary.is_some(), "{label}");
                        if in_primary.is_none() {
                            assert_eq!(rp, 0, "{label}");
                            let rf = call(&mut cpu, RECORD, &[2, x, z, MAP + FALLBACK as u32]);
                            assert_eq!(rf, r, "{label}");
                            fallback_hits += 1;
                        } else {
                            assert_eq!(rp, r, "{label}");
                        }
                    }
                }
            }
        }
    }
    assert!(scenes > 50 && hits > 100, "{scenes} {hits} {fallback_hits}");
}
