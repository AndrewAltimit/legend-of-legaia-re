//! The field VM's camera-zone re-query `FUN_801DE3E0(tile_x, tile_z)` (PROT
//! 0897, reached from `4C 38` / `4C 39` / `4C C4`): the zone query
//! `FUN_801DBA20` over the box the attribute refresh latched, then the hit's
//! record load `FUN_801DBC20` or the fixed miss set. Held against
//! `zone_query` followed by `CameraZoneConfig::load_record` /
//! `load_zone_miss` over every field scene's region table, with generated
//! zone records of every mode around each tile.

use crate::common::{Rng, scus_cpu};
use crate::walk_on::{index, map_image};
use legaia_engine_field::camera_zone::CameraZoneConfig;
use legaia_engine_field::field_regions::{
    MAP_REGION_BLOCK_OFFSET, RegionTable, ZONE_RECORD_STRIDE, refresh_region_attributes, zone_query,
};
use legaia_engine_scene::scene::Scene;
use psx_retail_exec::{Cpu, Stop};

const REQUERY: u32 = 0x801D_E3E0;
const REFRESH: u32 = 0x8001_80EC;
const OVERLAY_BASE: u32 = 0x801C_E818;
const MAP_PTR: u32 = 0x1F80_03EC;
const MAP: u32 = 0x8010_0000;
const GAME_MODE: u32 = 0x8007_B83C;
const SCENE_BLOCK_PTR: u32 = 0x801C_6EA4;
const SCENE_BLOCK: u32 = 0x8016_0000;
const ZONES: u32 = 0x8016_1000;

fn block(cpu: &mut Cpu) -> CameraZoneConfig {
    let w = |cpu: &mut Cpu, a: u32| cpu.mem.rd32(a) as i32;
    CameraZoneConfig {
        mode: cpu.mem.rd8(0x8007_B607),
        b608: cpu.mem.rd8(0x8007_B608),
        b609: cpu.mem.rd8(0x8007_B609),
        b60a: cpu.mem.rd8(0x8007_B60A),
        b60b: cpu.mem.rd8(0x8007_B60B),
        pitch: w(cpu, 0x8007_B60C),
        yaw: w(cpu, 0x8007_B610),
        depth: w(cpu, 0x8007_B614),
        h: w(cpu, 0x8007_B618),
        anchor_x: w(cpu, 0x8007_B61C),
        anchor_h: w(cpu, 0x8007_B620),
        anchor_z: w(cpu, 0x8007_B624),
    }
}

#[test]
fn the_zone_requery_loads_what_retail_loads() {
    differential!(prot(897, REQUERY));
    let (Some(mut cpu), Some(d)) = (scus_cpu(), crate::common::disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &d.read_entry(897).expect("PROT 0897"));
    cpu.mem.wr16(GAME_MODE, 3);
    let index = index();
    let mut rng = Rng(0x5251_5259);
    let (mut scenes, mut hits, mut misses) = (0, 0, 0);
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
        let Some(table) = RegionTable::parse(&image[MAP_REGION_BLOCK_OFFSET..]) else {
            continue;
        };
        scenes += 1;
        cpu.mem.load(MAP, &image);
        cpu.mem.wr32(MAP_PTR, MAP);
        for _ in 0..40 {
            let (tx, tz) = (rng.below(0x80) as i32, rng.below(0x80) as i32);
            assert_eq!(cpu.call(REFRESH, &[tx as u32, tz as u32]), Stop::Returned);
            let (_, attrs) = refresh_region_attributes(Some(&table), tx, tz, false);
            let count = rng.below(6) as usize;
            let mut zones = vec![count as u8];
            for _ in 0..count {
                let mut rec = [0u8; ZONE_RECORD_STRIDE];
                rec[0] = [0u8, 1, 2, 5, 31][rng.below(5) as usize];
                for b in &mut rec[1..5] {
                    *b = (tx as u32 + rng.below(9)).saturating_sub(4) as u8;
                }
                for b in &mut rec[5..] {
                    *b = rng.below(256) as u8;
                }
                rec[5] = (rng.below(8) as u8) << 4 | rng.below(16) as u8;
                zones.extend_from_slice(&rec);
            }
            cpu.mem.load(ZONES, &zones);
            cpu.mem.wr32(SCENE_BLOCK_PTR, SCENE_BLOCK);
            cpu.mem.wr32(SCENE_BLOCK + 4, ZONES);
            // A block from before, which the miss arm keeps the anchors of.
            let mut port = CameraZoneConfig {
                mode: rng.next() as u8,
                b608: rng.next() as u8,
                b609: rng.next() as u8,
                b60a: rng.next() as u8,
                b60b: rng.next() as u8,
                pitch: rng.next() as i32,
                yaw: rng.next() as i32,
                depth: rng.next() as i32,
                h: rng.next() as i32,
                anchor_x: rng.next() as i32,
                anchor_h: rng.next() as i32,
                anchor_z: rng.next() as i32,
            };
            cpu.mem.wr8(0x8007_B607, port.mode);
            cpu.mem.wr8(0x8007_B608, port.b608);
            cpu.mem.wr8(0x8007_B609, port.b609);
            cpu.mem.wr8(0x8007_B60A, port.b60a);
            cpu.mem.wr8(0x8007_B60B, port.b60b);
            for (a, v) in [
                (0x8007_B60C, port.pitch),
                (0x8007_B610, port.yaw),
                (0x8007_B614, port.depth),
                (0x8007_B618, port.h),
                (0x8007_B61C, port.anchor_x),
                (0x8007_B620, port.anchor_h),
                (0x8007_B624, port.anchor_z),
            ] {
                cpu.mem.wr32(a, v as u32);
            }
            assert_eq!(cpu.call(REQUERY, &[tx as u32, tz as u32]), Stop::Returned);
            match zone_query(&zones, Some(&table), &attrs, tx, tz).and_then(|r| r.record) {
                Some(rec) => {
                    let rec: [u8; ZONE_RECORD_STRIDE] =
                        rec[..ZONE_RECORD_STRIDE].try_into().expect("record");
                    port.load_record(&rec);
                    hits += 1;
                }
                None => {
                    port.load_zone_miss();
                    misses += 1;
                }
            }
            assert_eq!(block(&mut cpu), port, "{name} tile ({tx}, {tz})");
        }
    }
    assert!(
        scenes > 50 && hits > 500 && misses > 500,
        "{scenes} {hits} {misses}"
    );
}
