//! The two SCUS stages the field pad controller `FUN_801D01B0` runs its held
//! mask through, each called on its own: the camera ring remap
//! `FUN_800467E8` against `World::remap_pad_direction`, and the wall-slide
//! resolver `FUN_80046494` (with the field overlay's walkability probe
//! `FUN_801D56C4` live beneath it) against `World::resolve_field_slide`, over
//! generated grids and every mask shape the pad can hold.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_core::world::{Actor, World};
use psx_retail_exec::{Cpu, GP, Stop, V0};

const REMAP: u32 = 0x8004_67E8;
const RESOLVE: u32 = 0x8004_6494;
/// The debug print the resolver calls behind `gp+0x3B8` bit `0`.
const PRINT: u32 = 0x8001_A068;
const OVERLAY_BASE: u32 = 0x801C_E818;
const MAP_PTR: u32 = 0x1F80_03EC;
const MAP: u32 = 0x8010_0000;
const GRID: u32 = MAP + 0x4000;
const GRID_LEN: usize = 0x4000;
const ACTOR: u32 = 0x8013_0000;
const PAD: u32 = 0x8014_0000;
const OCTANT: u32 = 0x2D8;
const HELD: u32 = 0x538;
const RESOLVED: u32 = 0x9C4;
const DEBUG: u32 = 0x3B8;

#[test]
fn the_camera_ring_remap_matches_retail() {
    differential!(scus(REMAP));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let gp = cpu.regs.r[GP];
    let mut rng = Rng(0x5249_4E47);
    let mut turned = 0;
    for _ in 0..20_000 {
        // Every nibble, not only the eight ring entries: a mask off the ring
        // takes the ring's slot `rot`.
        let held = rng.next() & 0xFFFF;
        let rot = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 0xFFFF_FFFF][rng.below(11) as usize];
        let high = rng.next() & 0xFFFF_0000;
        cpu.mem.wr32(gp + OCTANT, rot);
        cpu.mem.wr32(PAD, high | held);
        assert_eq!(cpu.call(REMAP, &[PAD]), Stop::Returned);
        let word = cpu.mem.rd32(PAD);
        let port = World::remap_pad_direction(held as u16, rot);
        assert_eq!(word, high | u32::from(port), "held {held:#x} rot {rot}");
        turned += u32::from(port != held as u16);
    }
    assert!(turned > 5000, "{turned}");
}

fn retail_cpu() -> Option<Cpu> {
    let mut cpu = scus_cpu()?;
    cpu.mem
        .load(OVERLAY_BASE, &disc()?.read_entry(897).expect("PROT 0897"));
    cpu.stub(PRINT, 0);
    Some(cpu)
}

#[test]
fn the_wall_slide_resolves_where_retail_resolves() {
    differential!(scus(RESOLVE));
    let Some(mut cpu) = retail_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let gp = cpu.regs.r[GP];
    cpu.mem.wr32(MAP_PTR, MAP);
    let mut w = World::new();
    w.actors = vec![Actor::default(); 2];
    w.actors[0].active = true;
    w.player_actor_slot = Some(0);
    let mut rng = Rng(0x534C_4944);
    let (mut clear, mut slid, mut stuck, mut passed) = (0, 0, 0, 0);
    for grid_round in 0..30 {
        let density = [2, 8, 32, 128][rng.below(4) as usize];
        let grid: Vec<u8> = (0..GRID_LEN)
            .map(|_| {
                if rng.below(density) == 0 {
                    (rng.below(16) as u8) << 4
                } else {
                    0
                }
            })
            .collect();
        cpu.mem.load(GRID, &grid);
        w.terrain.collision_grid = grid;
        for round in 0..3000 {
            let (x, z) = (
                (0x200 + rng.below(0x3800)) as i16,
                (0x200 + rng.below(0x3800)) as i16,
            );
            let (x, z) = match rng.below(3) {
                0 => (x & !63, z & !63),
                1 => (x & !1, z & !1),
                _ => (x, z),
            };
            // Cardinals, the four diagonals the resolver passes by, opposing
            // pairs and triples it walks, and the no-clip bit.
            let dir = (rng.below(16) as u32) << 12;
            let held = dir
                | if rng.below(8) == 0 { 0x2 } else { 0 }
                | if rng.below(4) == 0 { 0x80 } else { 0 };
            cpu.mem.wr16(ACTOR + 0x14, x as u16);
            cpu.mem.wr16(ACTOR + 0x18, z as u16);
            cpu.mem.wr32(gp + HELD, held);
            cpu.mem.wr32(gp + DEBUG, 0);
            cpu.mem.wr32(gp + RESOLVED, 0xDEAD_BEEF);
            cpu.regs.r[V0] = 0xDEAD_BEEF;
            let stop = cpu.call(RESOLVE, &[ACTOR]);
            assert_eq!(stop, Stop::Returned, "grid {grid_round} round {round}");
            let r = cpu.regs.r[V0];
            let port = u32::from(w.resolve_field_slide(held as u16, x, z));
            assert_eq!(
                r, port,
                "grid {grid_round} round {round}: at ({x}, {z}) held {held:#x}"
            );
            let early = held & 0x2 != 0 || matches!(dir, 0x9000 | 0xC000 | 0x3000 | 0x6000);
            if early {
                passed += 1;
            } else {
                // The resolved word is stored only past the early exits.
                assert_eq!(
                    cpu.mem.rd32(gp + RESOLVED),
                    r,
                    "grid {grid_round} round {round}"
                );
                if r & !dir & 0xF000 != 0 {
                    slid += 1;
                } else if r == dir && dir != 0 {
                    clear += 1;
                } else {
                    stuck += 1;
                }
            }
        }
    }
    assert!(
        clear > 5000 && slid > 1000 && passed > 5000,
        "{clear} {slid} {stuck} {passed}"
    );
}
