//! The actor drop shadow off the disc: the grid transform `FUN_800460AC`
//! (three `RTPT`s over a 3x3 grid `0x20` apart round the actor, each point's
//! `SXY` / `SZ` stored to scratch `0x1F800020`) and the packet builder
//! `FUN_8001C394` that calls it and links four `POLY_FT4`s. Held against
//! `drop_shadow::shadow_grid` and `drop_shadow::shadow_quads`, the GTE side
//! answered by the interpreter's spec GTE under random cameras.

use crate::common::{Rng, scus_cpu};
use legaia_render_kernels::drop_shadow::{ShadowVertex, shadow_grid, shadow_quads};
use legaia_render_kernels::overworld_curvature::{REBUILD_BIAS, curvature_table};
use psx_retail_exec::gte::Gte;
use psx_retail_exec::{Cpu, Stop};

const GRID: u32 = 0x8004_60AC;
const QUADS: u32 = 0x8001_C394;
const SCRATCH_GRID: u32 = 0x1F80_0020;
const PACKET_PTR: u32 = 0x1F80_03A0;
const OT_SHIFT: u32 = 0x1F80_03A4;
const OT_PTR: u32 = 0x1F80_03F4;
const OVERWORLD_BIT: u32 = 0x1F80_0394;
const CURVE_PTR: u32 = 0x8007_BB04;
const ACTOR: u32 = 0x8010_0000;
const PACKETS: u32 = 0x8011_0000;
const OT: u32 = 0x8014_0000;
const OT_LEN: u32 = 0x4100;
const CURVE: u32 = 0x8018_0000;

/// A field-like camera: a rotation with the 6x scale, a translation that puts
/// the actor some way in front, `H` and the screen centre.
fn seed_camera(g: &mut Gte, rng: &mut Rng) {
    let a = |rng: &mut Rng| rng.below(0x1000) as u16;
    let m = legaia_render_kernels::gte::GteMat3::rot_x_psx(a(rng))
        .mul(&legaia_render_kernels::gte::GteMat3::rot_y_psx(a(rng)));
    let s = |v: i16| (i32::from(v) * 6).clamp(-0x8000, 0x7FFF);
    let e: Vec<i32> = m.m.iter().flatten().map(|v| s(*v)).collect();
    let p = |a: i32, b: i32| (a as u16 as u32) | (b as u16 as u32) << 16;
    for k in 0..4 {
        g.write_ctrl(k, p(e[2 * k], e[2 * k + 1]));
    }
    g.write_ctrl(4, e[8] as u16 as u32);
    g.write_ctrl(5, (rng.below(0x2000) as i32 - 0x1000) as u32);
    g.write_ctrl(6, (rng.below(0x2000) as i32 - 0x1000) as u32);
    g.write_ctrl(7, rng.below(0x8000));
    g.write_ctrl(24, 160 << 16);
    g.write_ctrl(25, 114 << 16);
    g.write_ctrl(26, 0x200 + rng.below(0x200));
}

fn random_pos(rng: &mut Rng, i: usize) -> [i32; 3] {
    if i.is_multiple_of(16) {
        // The halfword edges, where retail's grid wraps.
        let edge = |rng: &mut Rng| [-0x8000, 0x7FFF, -0x7FF0, 0x7FF0][rng.below(4) as usize];
        return [edge(rng), rng.below(0x200) as i32 - 0x100, edge(rng)];
    }
    let c = |rng: &mut Rng| rng.below(0x4000) as i32 - 0x2000;
    [c(rng), c(rng) / 8, c(rng)]
}

fn write_pos(cpu: &mut Cpu, va: u32, p: [i32; 3]) {
    for (k, v) in p.iter().enumerate() {
        cpu.mem.wr16(va + 2 * k as u32, *v as u16);
    }
}

/// The grid as the GTE wrote it: nine `(SXY, SZ)` words.
fn read_grid(cpu: &mut Cpu) -> [ShadowVertex; 9] {
    std::array::from_fn(|k| {
        let xy = cpu.mem.rd32(SCRATCH_GRID + 8 * k as u32);
        ShadowVertex {
            sx: xy as i16,
            sy: (xy >> 16) as i16,
            sz: cpu.mem.rd32(SCRATCH_GRID + 8 * k as u32 + 4) as u16,
            depth: None,
        }
    })
}

/// The spec GTE's RTPS of each port grid point, wrapped to the halfwords
/// the vertex registers hold.
fn project_port_grid(cam: &Gte, pos: [i32; 3]) -> [ShadowVertex; 9] {
    shadow_grid(pos).map(|p| {
        let mut g = cam.clone();
        g.write_data(0, (p[0] as u16 as u32) | (p[1] as u16 as u32) << 16);
        g.write_data(1, p[2] as u16 as u32);
        g.execute(0x0018_0001);
        let xy = g.read_data(14);
        ShadowVertex {
            sx: xy as i16,
            sy: (xy >> 16) as i16,
            sz: g.read_data(19) as u16,
            depth: None,
        }
    })
}

#[test]
fn shadow_grid_matches_the_transform() {
    differential!(scus(GRID));
    let Some(mut cpu) = scus_cpu() else {
        return;
    };
    let mut rng = Rng(0x5348_4457);
    let mut wraps = 0;
    for i in 0..3000 {
        seed_camera(&mut cpu.gte, &mut rng);
        let cam = cpu.gte.clone();
        let pos = random_pos(&mut rng, i);
        write_pos(&mut cpu, ACTOR, pos);
        assert_eq!(cpu.call(GRID, &[ACTOR]), Stop::Returned);
        assert_eq!(cpu.gte_ops, 3, "three RTPTs");
        let retail = read_grid(&mut cpu);
        let port = project_port_grid(&cam, pos);
        assert_eq!(retail, port, "pos {pos:?}");
        if shadow_grid(pos)
            .iter()
            .flatten()
            .any(|v| i32::from(*v as i16) != *v)
        {
            wraps += 1;
        }
    }
    // The port's grid is i32; the GTE takes halfwords, so a grid point past
    // the edge wraps there - the same point the projector is handed.
    assert!(wraps > 0, "the edge seeds reached the wrap");
}

#[test]
fn shadow_packets_match_the_builder() {
    differential!(scus(QUADS));
    let Some(mut cpu) = scus_cpu() else {
        return;
    };
    let mut rng = Rng(0x5155_4144);
    let table = curvature_table();
    let (mut near_cases, mut bent, mut below_ot) = (0, 0, 0);
    for i in 0..3000 {
        seed_camera(&mut cpu.gte, &mut rng);
        let pos = random_pos(&mut rng, i + 1);
        let near = i % 3 == 0;
        let curve = match i % 4 {
            0 => Some(0),
            1 => Some(REBUILD_BIAS),
            _ => None,
        };
        let shift = (rng.below(3)) as u8;
        write_pos(&mut cpu, ACTOR + 0x14, pos);
        cpu.mem
            .wr32(ACTOR + 0x10, if near { 0x0180_0000 } else { 0x0100_0000 });
        cpu.mem.wr32(PACKET_PTR, PACKETS);
        cpu.mem.wr8(OT_SHIFT, shift);
        cpu.mem.wr32(OT_PTR, OT);
        cpu.mem.wr32(OVERWORLD_BIT, u32::from(curve.is_some()));
        cpu.mem.wr32(CURVE_PTR, CURVE);
        let bias = curve.unwrap_or(0);
        for (k, v) in table.iter().enumerate() {
            cpu.mem.wr16(CURVE + 2 * k as u32, (*v + bias) as u16);
        }
        let ot_base = OT - 0x40;
        for k in 0..(OT_LEN + 0x10) {
            cpu.mem.wr32(ot_base + 4 * k, 0x5A00_0000 | k);
        }
        assert_eq!(cpu.call(QUADS, &[ACTOR]), Stop::Returned);
        let grid = read_grid(&mut cpu);
        let quads = shadow_quads(&grid, curve, shift, near);

        // The packets, field by field.
        let mut ot: Vec<u32> = (0..(OT_LEN + 0x10)).map(|k| 0x5A00_0000 | k).collect();
        for (q, quad) in quads.iter().enumerate() {
            let pk = PACKETS + 0x28 * q as u32;
            assert_eq!(cpu.mem.rd32(pk + 4), 0x2E80_8080);
            for (c, (x, y)) in quad.xy.iter().enumerate() {
                let xy = cpu.mem.rd32(pk + 8 + 8 * c as u32);
                assert_eq!(
                    (xy as i16, (xy >> 16) as i16),
                    (*x, *y),
                    "corner {c} of {q}, pos {pos:?}"
                );
                let uv = cpu.mem.rd16(pk + 12 + 8 * c as u32);
                assert_eq!((uv as u8, (uv >> 8) as u8), quad.uv[c]);
            }
            assert_eq!(cpu.mem.rd16(pk + 0xE), quad.clut);
            assert_eq!(cpu.mem.rd16(pk + 0x16), quad.tpage);
            // Retail links at `slot - 10` for a near actor, with no floor.
            let sum: u32 = [0usize, 1, 3, 4]
                .iter()
                .map(|k| u32::from(grid[[0, 1, 3, 4][q] + k].sz))
                .sum();
            let raw = ((sum + 0xA0) >> 4) >> shift;
            let link = if near { raw as i64 - 10 } else { raw as i64 };
            if near && raw < 10 {
                below_ot += 1;
            } else {
                assert_eq!(i64::from(quad.ot_index), link, "OT slot of {q}");
            }
            let idx = (link + 0x10) as usize;
            let tag = cpu.mem.rd32(pk);
            assert_eq!(tag & 0xFF00_0000, 0x0900_0000);
            assert_eq!(tag & 0x00FF_FFFF, ot[idx] & 0x00FF_FFFF, "link of {q}");
            ot[idx] = (ot[idx] & 0xFF00_0000) | (pk & 0x00FF_FFFF);
        }
        for (k, want) in ot.iter().enumerate() {
            assert_eq!(cpu.mem.rd32(ot_base + 4 * k as u32), *want, "OT word {k}");
        }
        near_cases += usize::from(near);
        bent += usize::from(curve.is_some());
    }
    eprintln!("{near_cases} near, {bent} bent, {below_ot} near links below the OT base");
    assert!(near_cases > 0 && bent > 0);
}
