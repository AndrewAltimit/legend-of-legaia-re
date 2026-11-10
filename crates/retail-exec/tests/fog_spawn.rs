//! The field fog's particle spawner `FUN_801D629C` (PROT 0897) against
//! `FogPool::spawn`, spawn by spawn over generated region tables: the
//! walk-box test, the first-region search with its disabled stop, the cap,
//! the five `rand()` draws, the free-stack pop and the record it fills, with
//! the trig LUT pair installed by `FUN_80026BE0` and compared against
//! `RetailRotationLut`. Off the overworld (`0x1F800394 & 1` clear): its depth
//! test runs through the GTE.
//!
//! The spawner also stores halfword `1` at record `+0x12`, which the port's
//! record does not carry; no reader of the pool (`FUN_8003F348`,
//! `FUN_8003F3FC`) loads it.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_field::fog_particles::{FOG_POOL_SLOTS, FogPool};
use legaia_engine_system::rotation_lut::RetailRotationLut;
use legaia_render_kernels::fog_volume::FogRegion;
use psx_retail_exec::{Cpu, Stop, V0};
use std::cell::RefCell;
use std::rc::Rc;

const SPAWN: u32 = 0x801D_629C;
const LUT_INSTALL: u32 = 0x8002_6BE0;
const RAND: u32 = 0x8005_6798;
const OVERLAY_BASE: u32 = 0x801C_E818;
const WINDOW: u32 = 0x1F80_0384;
const FLAGS: u32 = 0x1F80_0394;
const REGION_PTR: u32 = 0x8007_3ED8;
const REGION_COUNT: u32 = 0x8007_3EDC;
const REGIONS: u32 = 0x8014_0000;
const LIVE: u32 = 0x8007_BCA8;
const CAP: u32 = 0x8007_BCB0;
const POOL_PTR: u32 = 0x8007_B7E0;
const POOL: u32 = 0x8015_0000;
const SIN_PTR: u32 = 0x8007_B81C;
const COS_PTR: u32 = 0x8007_B7F8;
const RECORD_STRIDE: u32 = 0x18;

fn retail_cpu() -> Option<Cpu> {
    let mut cpu = scus_cpu()?;
    cpu.mem
        .load(OVERLAY_BASE, &disc()?.read_entry(897).expect("PROT 0897"));
    cpu.stub(0x8005_B818, 0);
    Some(cpu)
}

#[test]
fn the_trig_lut_install_points_at_the_retail_table() {
    differential!(scus(LUT_INSTALL));
    let Some(mut cpu) = retail_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    assert_eq!(cpu.call(LUT_INSTALL, &[]), Stop::Returned);
    let lut = RetailRotationLut::new();
    let (sin, cos) = (cpu.mem.rd32(SIN_PTR), cpu.mem.rd32(COS_PTR));
    let cos_table = lut.cos_table();
    for i in 0..4096u32 {
        assert_eq!(
            cpu.mem.rd16(sin + i * 2) as i16,
            lut.sin_table()[i as usize],
            "sin {i}"
        );
        assert_eq!(
            cpu.mem.rd16(cos + i * 2) as i16,
            cos_table[i as usize],
            "cos {i}"
        );
    }
}

#[test]
fn a_fog_spawn_fills_what_retail_fills() {
    differential!(prot(897, SPAWN));
    let Some(mut cpu) = retail_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    assert_eq!(cpu.call(LUT_INSTALL, &[]), Stop::Returned);
    let lut = RetailRotationLut::new();
    let stream: Rc<RefCell<Vec<u32>>> = Rc::default();
    let s = stream.clone();
    cpu.hook(RAND, move |r, _| r.r[V0] = s.borrow_mut().remove(0));
    cpu.mem.wr32(FLAGS, 0);
    cpu.mem.wr32(REGION_PTR, REGIONS);
    cpu.mem.wr32(POOL_PTR, POOL);
    let mut rng = Rng(0x464F_4753);
    let (mut spawned, mut refused, mut exhausted) = (0, 0, 0);
    for scene in 0..200 {
        let window = [
            rng.below(0x20) as u8,
            rng.below(0x20) as u8,
            0x20 + rng.below(0x60) as u8,
            0x20 + rng.below(0x60) as u8,
        ];
        cpu.mem.load(WINDOW, &window);
        let n = rng.below(6) as usize;
        let mut bytes = Vec::new();
        for _ in 0..n {
            let (x0, z0) = (rng.below(0x60) as u8, rng.below(0x60) as u8);
            bytes.extend_from_slice(&[
                u8::from(rng.below(5) != 0),
                x0,
                z0,
                x0 + rng.below(0x30) as u8,
                z0 + rng.below(0x30) as u8,
                rng.next() as u8,
                rng.next() as u8,
                rng.next() as u8,
                rng.next() as u8,
                rng.next() as u8,
                rng.next() as u8,
            ]);
        }
        cpu.mem.load(REGIONS, &bytes);
        cpu.mem.wr8(REGION_COUNT, n as u8);
        let mut pool = FogPool::new();
        pool.regions = bytes.chunks(0xB).filter_map(FogRegion::parse).collect();
        pool.cap = [0x18u16, 0x50, 4][rng.below(3) as usize];
        cpu.mem.wr32(CAP, u32::from(pool.cap));
        // The retail pool as MAIN_INIT leaves it: the stack seeded `0..79`.
        cpu.mem.load(
            POOL,
            &vec![0u8; 0xA4 + FOG_POOL_SLOTS * RECORD_STRIDE as usize],
        );
        cpu.mem.wr16(POOL, (FOG_POOL_SLOTS - 1) as u16);
        for i in 0..FOG_POOL_SLOTS as u32 {
            cpu.mem.wr16(POOL + 4 + i * 2, i as u16);
            cpu.mem.wr8(POOL + 0xA4 + i * RECORD_STRIDE + 4, i as u8);
        }
        for op in 0..120 {
            pool.live = [0u16, 3, 0x17, 0x18, 0x50][rng.below(5) as usize];
            cpu.mem.wr32(LIVE, u32::from(pool.live));
            // Mostly tiles inside a region, so the draws are reached.
            let (tx, tz) = match pool.regions.get(rng.below(n as u32 + 1) as usize) {
                Some(r) if rng.below(4) != 0 => (
                    i32::from(r.x0) + rng.below(u32::from(r.x1 - r.x0) + 2) as i32,
                    i32::from(r.z0) + rng.below(u32::from(r.z1 - r.z0) + 2) as i32,
                ),
                _ => (rng.below(0x80) as i32, rng.below(0x80) as i32),
            };
            let draws: Vec<u32> = (0..5).map(|_| rng.next() & 0x7FFF).collect();
            *stream.borrow_mut() = draws.clone();
            assert_eq!(cpu.call(SPAWN, &[tx as u32, tz as u32]), Stop::Returned);
            let left = stream.borrow().len();
            let mut port_draws = draws.into_iter();
            let mut used = 0;
            let ok = pool.spawn(tx, tz, window, &lut, &mut || {
                used += 1;
                port_draws.next().expect("a sixth draw")
            });
            let label = format!("scene {scene} op {op} tile ({tx}, {tz})");
            assert_eq!(5 - left, used, "{label}: draws");
            if ok {
                spawned += 1;
            } else if used == 2 {
                exhausted += 1;
            } else {
                refused += 1;
            }
            let top = FOG_POOL_SLOTS as i16 - 1 - pool.allocated() as i16;
            assert_eq!(cpu.mem.rd16(POOL) as i16, top, "{label}");
            for (i, r) in pool.records.iter().enumerate() {
                let at = POOL + 0xA4 + i as u32 * RECORD_STRIDE;
                let retail = (
                    cpu.mem.rd16(at),
                    cpu.mem.rd16(at + 2),
                    cpu.mem.rd8(at + 4),
                    cpu.mem.rd8(at + 5) != 0,
                    cpu.mem.rd8(at + 6) as i8,
                    cpu.mem.rd8(at + 7) as i8,
                    cpu.mem.rd32(at + 8) as i32,
                    cpu.mem.rd32(at + 0xC) as i32,
                    cpu.mem.rd16(at + 0x10) as i16,
                    [
                        cpu.mem.rd8(at + 0x14),
                        cpu.mem.rd8(at + 0x15),
                        cpu.mem.rd8(at + 0x16),
                    ],
                );
                let port = (
                    r.age,
                    r.rate,
                    r.slot,
                    r.alive,
                    r.vx,
                    r.vz,
                    r.x,
                    r.z,
                    r.y,
                    [r.grey; 3],
                );
                assert_eq!(retail, port, "{label} record {i}");
            }
        }
    }
    assert!(
        spawned > 1000 && refused > 3000,
        "{spawned} {refused} {exhausted}"
    );
}
