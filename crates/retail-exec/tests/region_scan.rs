//! The `.MAP` region table, every frame of field play: the resumable scan
//! `FUN_80017FBC` under the attribute refresh `FUN_800180EC`, and the camera
//! zone query `FUN_801DBA20` (PROT 0897) that reads what the refresh latched,
//! over every field scene's real region table at every tile class. Held
//! against `refresh_region_attributes` and `zone_query`; the zone records are
//! generated, every kind and box shape.

use crate::common::{Rng, scus_cpu};
use crate::walk_on::{index, map_image};
use legaia_engine_field::field_regions::{
    MAP_REGION_BLOCK_OFFSET, RegionAttributes, RegionTable, ZONE_RECORD_STRIDE,
    refresh_region_attributes, zone_query,
};
use legaia_engine_scene::scene::Scene;
use psx_retail_exec::{Stop, V0};

const REFRESH: u32 = 0x8001_80EC;
const ZONE_QUERY: u32 = 0x801D_BA20;
const OVERLAY_BASE: u32 = 0x801C_E818;
const MAP_PTR: u32 = 0x1F80_03EC;
const MAP: u32 = 0x8010_0000;
const MASK: u32 = 0x8007_B8F4;
const GAME_MODE: u32 = 0x8007_B83C;
const BOX: u32 = 0x1F80_0384;
const KIND: u32 = 0x1F80_037C;
const SCENE_BLOCK_PTR: u32 = 0x801C_6EA4;
const SCENE_BLOCK: u32 = 0x8016_0000;
const ZONES: u32 = 0x8016_1000;

#[test]
fn the_region_refresh_and_zone_query_match_over_every_scene() {
    differential!(scus(REFRESH), prot(897, ZONE_QUERY));
    let (Some(mut cpu), Some(d)) = (scus_cpu(), crate::common::disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &d.read_entry(897).expect("PROT 0897"));
    let index = index();
    let mut rng = Rng(0x5245_4749);
    let (mut scenes, mut hits, mut zone_hits, mut failures) = (0, 0, 0, Vec::new());
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
        let block = &image[MAP_REGION_BLOCK_OFFSET..];
        let Some(table) = RegionTable::parse(block) else {
            continue;
        };
        scenes += 1;
        cpu.mem.load(MAP, &image);
        cpu.mem.wr32(MAP_PTR, MAP);
        // Tiles inside and around every record, then anywhere.
        let mut tiles: Vec<(i32, i32)> = Vec::new();
        let body = i16::from_le_bytes([block[0xE], block[0xF]]) as usize;
        for i in 0..table.count() {
            let r = &block[body + i * 8..body + i * 8 + 4];
            for (dx, dz) in [(0, 0), (-1, 0), (0, -1), (1, 1), (2, 2), (-2, -2)] {
                tiles.push((i32::from(r[0]) + dx, i32::from(r[1]) + dz));
                tiles.push((i32::from(r[2]) + dx, i32::from(r[3]) + dz));
            }
        }
        for _ in 0..64 {
            tiles.push((rng.below(0x84) as i32 - 2, rng.below(0x84) as i32 - 2));
        }
        for (tx, tz) in tiles {
            let world_map = rng.below(8) == 0;
            cpu.mem.wr16(
                GAME_MODE,
                if world_map {
                    0xE + rng.below(2) as u16
                } else {
                    3
                },
            );
            cpu.mem.wr32(MASK, 0xDEAD_BEEF);
            for k in 0..4 {
                cpu.mem.wr8(BOX + k, 0xA5);
            }
            cpu.mem.wr8(KIND, 0xA5);
            assert_eq!(cpu.call(REFRESH, &[tx as u32, tz as u32]), Stop::Returned);
            let retail_mask = cpu.mem.rd32(MASK);
            let retail_attrs = RegionAttributes {
                box_bytes: std::array::from_fn(|k| cpu.mem.rd8(BOX + k as u32)),
                kind: cpu.mem.rd8(KIND),
            };
            let (mask, attrs) = refresh_region_attributes(Some(&table), tx, tz, world_map);
            hits += usize::from(mask != 0);
            if (retail_mask, retail_attrs) != (mask, attrs) && failures.len() < 16 {
                failures.push(format!(
                    "{name} tile ({tx}, {tz}) world map {world_map}: retail {retail_mask:#x} \
                     {retail_attrs:?} port {mask:#x} {attrs:?}"
                ));
            }

            // The zone query over the box the refresh just latched.
            let count = rng.below(6) as usize;
            let mut zones = vec![count as u8];
            for _ in 0..count {
                let mut rec = [0u8; ZONE_RECORD_STRIDE];
                rec[0] = match rng.below(4) {
                    0 => 0,
                    1 => 1,
                    _ => rng.below(32) as u8,
                };
                for b in &mut rec[1..5] {
                    *b = (tx.clamp(0, 0x7F) as u32 + rng.below(9)).saturating_sub(4) as u8;
                }
                for b in &mut rec[5..] {
                    *b = rng.below(256) as u8;
                }
                zones.extend_from_slice(&rec);
            }
            cpu.mem.load(ZONES, &zones);
            cpu.mem.wr32(SCENE_BLOCK_PTR, SCENE_BLOCK);
            cpu.mem.wr32(SCENE_BLOCK + 4, ZONES);
            cpu.regs.r[V0] = 0xFFFF_FFFF;
            assert_eq!(
                cpu.call(ZONE_QUERY, &[tx as u32, tz as u32]),
                Stop::Returned
            );
            let retail_rec = match cpu.regs.r[V0] {
                0 => None,
                p => Some(((p - ZONES - 1) / ZONE_RECORD_STRIDE as u32) as usize),
            };
            let retail_zone_mask = (count != 0).then(|| cpu.mem.rd32(MASK));
            let port = zone_query(&zones, Some(&table), &attrs, tx, tz);
            let port_rec = port.and_then(|r| r.record).map(|rec| {
                (rec.as_ptr() as usize - zones.as_ptr() as usize - 1) / ZONE_RECORD_STRIDE
            });
            zone_hits += usize::from(retail_rec.is_some());
            if (retail_rec, retail_zone_mask) != (port_rec, port.map(|r| r.region_mask))
                && failures.len() < 16
            {
                failures.push(format!(
                    "{name} zone tile ({tx}, {tz}): retail {retail_rec:?} {retail_zone_mask:?} \
                     port {port_rec:?} {:?}",
                    port.map(|r| r.region_mask)
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    eprintln!("{scenes} scenes, {hits} tiles in a region, {zone_hits} zone hits");
    assert!(
        scenes > 100 && hits > 1000 && zone_hits > 1000,
        "{scenes} {hits} {zone_hits}"
    );
}
