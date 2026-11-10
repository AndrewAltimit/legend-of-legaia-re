//! The fishing overlay's (PROT 0972) floor kernels against their ports in
//! `legaia_engine_field::minigame_floor`: the polar offset `FUN_801D7BB8`
//! over the SCUS quadrature tables (`polar_offset` over `polar_tables`), the
//! step-layer record scan `FUN_801D79E0` (`step_patch_in_layer`) and the
//! ground-height solver `FUN_801D6028` (`ground_height`), the latter two over
//! generated floor buffers laid out as retail installs them behind
//! `_DAT_1F8003EC`.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_field::minigame_floor::{
    FloorGrid, STEP_LAYER_KIND, STEP_RECORD_STRIDE, ground_height, polar_offset, polar_tables,
    step_patch_in_layer,
};
use psx_retail_exec::{Cpu, Stop, V0};

const POLAR: u32 = 0x801D_7BB8;
const STEP_PATCH: u32 = 0x801D_79E0;
const GROUND: u32 = 0x801D_6028;
const OVERLAY_BASE: u32 = 0x801C_E818;
const STACK_TOP: u32 = 0x801F_FF00;
const MAP_PTR: u32 = 0x1F80_03EC;
const RAMP: u32 = 0x1F80_035C;
const MAP: u32 = 0x8010_0000;
const ACTOR: u32 = 0x8013_0000;

fn cpu() -> Option<Cpu> {
    let mut cpu = scus_cpu()?;
    cpu.mem
        .load(OVERLAY_BASE, &disc()?.read_entry(972).expect("PROT 0972"));
    // The sine / cosine views the boot code installs, and the per-kind
    // record strides the library states carry (`04 04 04 08`).
    cpu.mem.wr32(0x8007_B81C, 0x8007_0A2C);
    cpu.mem.wr32(0x8007_B7F8, 0x8007_0A2C + 0x800);
    cpu.mem.load(0x8007_B318, &[4, 4, 4, 8]);
    Some(cpu)
}

#[test]
fn the_polar_offset_matches_retail() {
    differential!(prot(972, POLAR));
    let Some(mut cpu) = cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let (sin, cos) = polar_tables();
    const OUT_A: u32 = 0x8013_8000;
    const OUT_B: u32 = 0x8013_8004;
    let mut rng = Rng(0x504F_4C41);
    for run in 0..50_000 {
        let angle = rng.next();
        let radius = [0x14, 200, rng.next() as i32 >> rng.below(24)][rng.below(3) as usize];
        let scale = [1, 2, 3, rng.next() as i32 >> rng.below(24)][rng.below(4) as usize];
        cpu.mem.wr32(STACK_TOP + 0x10, scale as u32);
        assert_eq!(
            cpu.call(POLAR, &[angle, radius as u32, OUT_A, OUT_B]),
            Stop::Returned
        );
        let retail = (cpu.mem.rd32(OUT_A) as i32, cpu.mem.rd32(OUT_B) as i32);
        let port = polar_offset(angle, radius, scale, sin, cos).unwrap();
        assert_eq!(
            retail, port,
            "run {run}: angle {angle:#x} radius {radius} scale {scale}"
        );
    }
}

/// A floor buffer: random terrain and cell words, and a step layer at
/// `+0x10000` and `+0x12000` whose records sit on `cells`.
fn floor(rng: &mut Rng, cells: &[(u8, u8)]) -> Vec<u8> {
    let mut buf: Vec<u8> = (0..0x14000).map(|_| rng.next() as u8).collect();
    for base in [0x10000usize, 0x12000] {
        let hdr = base + STEP_LAYER_KIND * 4;
        let body = 0x40i16;
        let n = if rng.below(4) == 0 {
            0
        } else {
            cells.len() / 2
        };
        buf[hdr + 2..hdr + 4].copy_from_slice(&body.to_le_bytes());
        buf[hdr + 4..hdr + 6].copy_from_slice(&(n as i16).to_le_bytes());
        for i in 0..n {
            let (x, z) = cells[rng.below(cells.len() as u32) as usize];
            let at = base + body as usize + i * STEP_RECORD_STRIDE;
            buf[at] = x;
            buf[at + 1] = z;
        }
    }
    buf
}

#[test]
fn the_step_patch_scan_matches_retail() {
    differential!(prot(972, STEP_PATCH));
    let Some(mut cpu) = cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x5354_4550);
    let mut hits = 0;
    for run in 0..300 {
        let cells: Vec<(u8, u8)> = (0..24)
            .map(|_| (rng.below(0x80) as u8, rng.below(0x80) as u8))
            .collect();
        let buf = floor(&mut rng, &cells);
        cpu.mem.load(MAP, &buf);
        for _ in 0..64 {
            let (gx, gz) = if rng.below(2) == 0 {
                let c = cells[rng.below(cells.len() as u32) as usize];
                (i32::from(c.0), i32::from(c.1))
            } else {
                (rng.below(0x84) as i32 - 2, rng.below(0x84) as i32 - 2)
            };
            for base in [0x10000u32, 0x12000] {
                assert_eq!(
                    cpu.call(
                        STEP_PATCH,
                        &[STEP_LAYER_KIND as u32, gx as u32, gz as u32, MAP + base]
                    ),
                    Stop::Returned
                );
                let retail = match cpu.regs.r[V0] {
                    0 => None,
                    p => {
                        let o = (p - MAP) as usize;
                        Some((buf[o + 2] as i8, buf[o + 3]))
                    }
                };
                let port = step_patch_in_layer(&buf[base as usize..], gx, gz)
                    .map(|p| (p.step, p.quadrants));
                hits += usize::from(port.is_some());
                assert_eq!(retail, port, "run {run} layer {base:#x} cell ({gx}, {gz})");
            }
        }
    }
    assert!(hits > 5000, "{hits}");
}

#[test]
fn the_ground_height_matches_retail() {
    differential!(prot(972, GROUND));
    let Some(mut cpu) = cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem.wr32(MAP_PTR, MAP);
    let mut rng = Rng(0x4752_4E44);
    let (mut stepped, mut flat, mut blended) = (0, 0, 0);
    for run in 0..200 {
        let cells: Vec<(u8, u8)> = (0..32)
            .map(|_| (rng.below(0x80) as u8, rng.below(0x80) as u8))
            .collect();
        let mut buf = floor(&mut rng, &cells);
        // Flatten some terrain so the equal-corner early return is reached.
        if rng.below(2) == 0 {
            let v = rng.next() as u8;
            for b in &mut buf[0x4000..0x8000] {
                if rng.below(2) == 0 {
                    *b = v;
                }
            }
        }
        cpu.mem.load(MAP, &buf);
        let ramp: Vec<i16> = (0..16).map(|_| rng.next() as i16 >> rng.below(6)).collect();
        for (i, h) in ramp.iter().enumerate() {
            cpu.mem.wr16(RAMP + 2 * i as u32, *h as u16);
        }
        let grid = FloorGrid::new(&buf);
        for sample in 0..200 {
            let (x, z) = if rng.below(2) == 0 {
                let c = cells[rng.below(cells.len() as u32) as usize];
                (
                    (i32::from(c.0) * 0x80 + rng.below(0x80) as i32) as i16,
                    (i32::from(c.1) * 0x80 + rng.below(0x80) as i32) as i16,
                )
            } else {
                // Inside the 128 x 128 grid: off it, retail wraps the cell
                // word's index (`& 0xFE`, `& 0x7F00`) where the port reads
                // an empty cell - a position no floor actor reaches.
                (rng.below(0x4000) as i16, rng.below(0x4000) as i16)
            };
            let flags = rng.next();
            cpu.mem.wr16(ACTOR + 0x14, x as u16);
            cpu.mem.wr16(ACTOR + 0x18, z as u16);
            cpu.mem.wr32(ACTOR + 0x10, flags);
            assert_eq!(cpu.call(GROUND, &[ACTOR]), Stop::Returned);
            let g = ground_height(grid, x, z, flags, &ramp, |gx, gz| grid.step_patch(gx, gz));
            let label = format!("run {run} sample {sample}: ({x}, {z}) cell {:#06x}", g.cell);
            assert_eq!(cpu.regs.r[V0] as i32, g.height, "{label}");
            assert_eq!(cpu.mem.rd32(ACTOR + 0x10), g.flags, "{label}");
            if g.cell & 0x800 != 0 {
                stepped += 1;
            } else if x & 0x7F == 0 && z & 0x7F == 0 {
                flat += 1;
            } else {
                blended += 1;
            }
        }
    }
    assert!(
        stepped > 5000 && blended > 5000,
        "{stepped} {flat} {blended}"
    );
}
