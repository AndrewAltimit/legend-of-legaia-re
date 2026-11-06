//! The field floor sampler `FUN_80019278`, with the field overlay's kind-2
//! lookup `FUN_801D5630` live beneath it, against
//! `World::sample_field_floor_height`: generated tier grids, object cells with
//! and without the ramp bit, override records in both trigger blocks, and every
//! position class - tile interiors, tile and sub-cell edges, the map's borders
//! and negative coordinates.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_core::world::World;
use legaia_retail_exec::{Stop, V0};

const SAMPLER: u32 = 0x8001_9278;
const OVERLAY_BASE: u32 = 0x801C_E818;
const MAP_PTR: u32 = 0x1F80_03EC;
const LUT: u32 = 0x1F80_035C;
const MAP: u32 = 0x8010_0000;
const GRID: u32 = MAP + 0x4000;
const CELLS: u32 = MAP + 0x8000;
const PRIMARY: u32 = MAP + 0x10000;
const FALLBACK: u32 = MAP + 0x12000;
const BLOCK_LEN: usize = 0x2000;
const ENTITY: u32 = 0x8013_0000;

/// A trigger block holding `records` as its kind-2 sub-table (header word
/// `k` at `+4k`: offset `+2`, count `+4`).
fn trigger_block(records: &[[u8; 4]]) -> Vec<u8> {
    let mut b = vec![0u8; BLOCK_LEN];
    let off = 0x40i16;
    b[0xA..0xC].copy_from_slice(&off.to_le_bytes());
    b[0xC..0xE].copy_from_slice(&(records.len() as i16).to_le_bytes());
    for (i, r) in records.iter().enumerate() {
        let at = off as usize + i * 4;
        b[at..at + 4].copy_from_slice(r);
    }
    b
}

#[test]
fn the_floor_is_where_retail_puts_it() {
    let (Some(mut cpu), Some(disc)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &disc.read_entry(897).expect("PROT 0897"));
    cpu.mem.wr32(MAP_PTR, MAP);
    let mut rng = Rng(0x464C_4F4F);
    let (mut failures, mut ramps, mut blends) = (Vec::new(), 0, 0);
    for map_round in 0..30 {
        let mut w = World::new();
        // A grid of a few tiers in patches, so neighbours often agree.
        let tiers = 1 + rng.below(16);
        let grid: Vec<u8> = (0..0x4000)
            .map(|_| {
                let hi = if rng.below(8) == 0 {
                    rng.below(16) << 4
                } else {
                    0
                };
                (hi | rng.below(tiers)) as u8
            })
            .collect();
        let mut cells = vec![0u8; 0x8000];
        for c in cells.chunks_mut(2) {
            let mut v = rng.below(0x200) as u16;
            if rng.below(3) == 0 {
                v |= 0x800;
            }
            if rng.below(2) == 0 {
                v |= 0x1000;
            }
            c.copy_from_slice(&v.to_le_bytes());
        }
        let lut: Vec<i16> = (0..16).map(|_| rng.below(0x800) as i16 - 0x600).collect();
        let rec = |rng: &mut Rng| {
            [
                rng.below(128) as u8,
                rng.below(128) as u8,
                rng.below(256) as u8,
                rng.below(256) as u8,
            ]
        };
        let n = rng.below(200);
        let primary: Vec<[u8; 4]> = (0..n).map(|_| rec(&mut rng)).collect();
        let n = rng.below(200);
        let fallback: Vec<[u8; 4]> = (0..n).map(|_| rec(&mut rng)).collect();
        let (pb, fb) = (trigger_block(&primary), trigger_block(&fallback));
        // The descriptor table below the grid, which the corner block reads
        // into from a tile above the map, and nothing below the image.
        let descriptors: Vec<u8> = (0..0x4000).map(|_| rng.below(256) as u8).collect();
        cpu.mem.load(MAP, &descriptors);
        cpu.mem.load(MAP - 0x8000, &[0u8; 0x8000]);
        w.terrain.static_window.descriptors = descriptors;
        cpu.mem.load(GRID, &grid);
        cpu.mem.load(CELLS, &cells);
        cpu.mem.load(PRIMARY, &pb);
        cpu.mem.load(FALLBACK, &fb);
        for (i, v) in lut.iter().enumerate() {
            cpu.mem.wr16(LUT + i as u32 * 2, *v as u16);
        }
        w.load_field_collision_grid(&grid);
        w.load_field_object_cells(&cells);
        w.load_field_elevation_overrides(&pb, &fb);
        for (i, v) in lut.iter().enumerate() {
            w.terrain.floor_height_lut[i] = *v;
        }
        for _ in 0..4000 {
            let (x, z): (i16, i16) = match rng.below(6) {
                // Tile and sub-cell boundaries.
                0 => (
                    (rng.below(0x100) * 64 + rng.below(3)) as i16 - 1,
                    (rng.below(0x100) * 64 + rng.below(3)) as i16 - 1,
                ),
                // Near the origin, either side of zero.
                1 => (
                    rng.below(0x200) as i16 - 0x100,
                    rng.below(0x200) as i16 - 0x100,
                ),
                // Anywhere a halfword reaches.
                2 if rng.below(4) == 0 => (rng.next() as i16, rng.next() as i16),
                // The far border.
                2 => (
                    0x3F00 + rng.below(0x100) as i16,
                    0x3F00 + rng.below(0x100) as i16,
                ),
                // Tiles a record names.
                3 if !primary.is_empty() => {
                    let r = primary[rng.below(primary.len() as u32) as usize];
                    (
                        i16::from(r[0]) * 128 + rng.below(128) as i16,
                        i16::from(r[1]) * 128 + rng.below(128) as i16,
                    )
                }
                _ => (rng.below(0x3F80) as i16, rng.below(0x3F80) as i16),
            };
            cpu.mem.wr32(ENTITY + 0x10, 0);
            cpu.mem.wr16(ENTITY + 0x14, x as u16);
            cpu.mem.wr16(ENTITY + 0x18, z as u16);
            cpu.regs.r[V0] = 0;
            let stop = cpu.call(SAMPLER, &[ENTITY]);
            assert_eq!(stop, Stop::Returned, "({x}, {z})");
            let retail = cpu.regs.r[V0] as i32;
            let port = w.sample_field_floor_height(i32::from(x), i32::from(z));
            let (tx, tz) = (i32::from(x) >> 7, i32::from(z) >> 7);
            if w.field_tile_has_elevation_override(tx, tz) {
                ramps += 1;
            } else {
                blends += 1;
            }
            if retail != port && failures.len() < 12 {
                failures.push(format!(
                    "map {map_round} ({x}, {z}) tile ({tx}, {tz}): retail {retail} port {port}"
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(
        ramps > 10000 && blends > 10000,
        "ramps {ramps} blends {blends}"
    );
}
