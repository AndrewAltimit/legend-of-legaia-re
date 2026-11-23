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
const VM: u32 = 0x801D_E840;
const SCRIPT: u32 = 0x8011_0000;
const CTX: u32 = 0x8012_0000;

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

/// Op `4C 83` (`0x801E20A8..0x801E2130`) stepped by the field VM on every field
/// scene's real `.MAP`, against the real `World`'s `fill_elevation_rect`:
/// rectangles grown around a scene's own records (hits, and the tiles beside
/// them that have none) and drawn anywhere on the grid, each tile resolved
/// through the real `FUN_801D5630`. Compared: both record blocks whole, re-read
/// off the image retail edited, against the port's override list.
#[test]
fn a_tile_rect_rewrites_the_elevation_records_retail_rewrites() {
    differential!(prot(897, VM));
    let Some(mut cpu) = retail_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.stub_outside = Some((OVERLAY_BASE, 0x8020_0000));
    cpu.step_budget = 2_000_000;
    cpu.mem.wr8(0x1F80_0393, 1);
    cpu.mem.wr32(CTX + 0x94, SCRIPT);
    let index = index();
    let mut rng = Rng(0x3443_3833);
    let (mut scenes, mut written, mut missed) = (0, 0u32, 0u32);
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
        let both = |img: &[u8]| {
            let mut v = parse_elevation_overrides(&img[PRIMARY..FALLBACK]);
            v.extend(parse_elevation_overrides(&img[FALLBACK..]));
            v
        };
        let records = both(&image);
        if records.is_empty() {
            continue;
        }
        scenes += 1;
        for _ in 0..12 {
            let (c0, r0, c1, r1) = if rng.below(4) == 0 {
                let (a, b) = (rng.below(0x80) as u8, rng.below(0x80) as u8);
                (
                    a,
                    b,
                    a.saturating_add(rng.below(6) as u8),
                    b.saturating_add(rng.below(6) as u8),
                )
            } else {
                let r = records[rng.below(records.len() as u32) as usize];
                let mut g = |t: u8| t.saturating_sub(rng.below(3) as u8);
                let (c0, r0) = (g(r.tile_x), g(r.tile_z));
                (
                    c0,
                    r0,
                    r.tile_x.saturating_add(rng.below(3) as u8),
                    r.tile_z.saturating_add(rng.below(3) as u8),
                )
            };
            let value = rng.next() as u8;
            let script = [0x4C, 0x83, c0, r0, c1, r1, value];
            cpu.mem.load(MAP, &image);
            cpu.mem.load(SCRIPT, &script);
            assert_eq!(cpu.call(VM, &[SCRIPT, 0, CTX]), Stop::Returned);
            assert_eq!(cpu.regs.r[V0], 7, "{name} {script:02x?}");
            let retail = both(&cpu.mem.read(MAP, image.len()));

            let mut w = World::new();
            w.terrain.elevation_overrides = records.clone();
            w.field_bytecode = script.to_vec();
            w.field_pc = 0;
            let _ = w.step_field();
            assert_eq!(
                w.terrain.elevation_overrides, retail,
                "{name} rect ({c0}, {r0})..=({c1}, {r1}) value {value:#x}"
            );
            let changed = records.iter().zip(&retail).filter(|(a, b)| a != b).count() as u32;
            written += changed;
            let tiles = (u32::from(c1.max(c0) - c0) + 1) * (u32::from(r1.max(r0) - r0) + 1);
            missed += u32::from(changed < tiles);
        }
    }
    assert!(
        scenes > 20 && written > 100 && missed > 100,
        "{scenes} {written} {missed}"
    );
}
