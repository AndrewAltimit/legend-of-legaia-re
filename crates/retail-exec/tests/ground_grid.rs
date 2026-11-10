//! The battle floor's procedural grid `FUN_801D02C0` against the
//! `battle_ground_grid` kernels: the origin and probe points
//! (`grid_origin`, `probe_point`), the pass-1 depth classes
//! (`classify_cell`), the 3x3 lattice with the X word's implied `Y`
//! (`cell_lattice`, `implied_y`), the screen reject (`quad_on_screen`), the
//! sub-tiles' UVs and attributes (`sub_tile_uv`, `sub_tile_index`,
//! `sub_quad_lattice`, `CLUT_ATTR`, `TPAGE_ATTR`), the hand-loaded depth cue
//! (`grid_ir0_raw`) and the code byte the colour word carries
//! (`CODE_WHILE_EMITTING` / `CODE_AFTER_EMITTING`).
//!
//! The GTE side of the expected output is answered by the interpreter's spec
//! GTE run on the port's points, as in `drop_shadow`. Random battle-like
//! cameras over grids up to `12 x 12` cells; the visibility buffer, every
//! packet, the OT slot, the packet pointer and the colour word's code byte
//! are compared whole.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_battle_vm::battle_ground_grid::{
    CLUT_ATTR, CODE_AFTER_EMITTING, CODE_WHILE_EMITTING, CellVis, TPAGE_ATTR, cell_lattice,
    classify_cell, grid_ir0_raw, implied_y, probe_point, quad_on_screen, sub_quad_lattice,
    sub_tile_index, sub_tile_uv,
};
use psx_retail_exec::Stop;
use psx_retail_exec::gte::Gte;

const GRID: u32 = 0x801D_02C0;
const OVERLAY_BASE: u32 = 0x801C_E818;
const VIS_PTR: u32 = 0x8007_B814;
const VIS: u32 = 0x8015_0000;
const RGBC: u32 = 0x1F80_0398;
const PACKET_PTR: u32 = 0x1F80_03A0;
const OT_SHIFT: u32 = 0x1F80_03A4;
const OT_PTR: u32 = 0x1F80_03F4;
const DIMS: u32 = 0x1F80_03F8;
const OT_MASK: u32 = 0x1F80_03FC;
const PACKETS: u32 = 0x8011_0000;
const OT: u32 = 0x8014_0000;
const PACKET_WORDS: u32 = 13;

/// A battle-like camera: pitched down, any yaw, the 6x scale, a translation
/// that keeps much of the plane in front, `H`, the screen centre and a far
/// colour.
fn seed_camera(g: &mut Gte, rng: &mut Rng) {
    use legaia_render_kernels::gte::GteMat3;
    let pitch = 0x80 + rng.below(0x300) as u16;
    let m = GteMat3::rot_x_psx(pitch).mul(&GteMat3::rot_y_psx(rng.below(0x1000) as u16));
    let e: Vec<i32> = m.m.iter().flatten().map(|v| i32::from(*v)).collect();
    let p = |a: i32, b: i32| (a as u16 as u32) | (b as u16 as u32) << 16;
    for k in 0..4 {
        g.write_ctrl(k, p(e[2 * k], e[2 * k + 1]));
    }
    g.write_ctrl(4, e[8] as u16 as u32);
    g.write_ctrl(5, (rng.below(0x800) as i32 - 0x400) as u32);
    g.write_ctrl(6, (rng.below(0x800) as i32 - 0x200) as u32);
    g.write_ctrl(
        7,
        match rng.below(5) {
            0 => 0x5000 + rng.below(0x3000) as i32,
            1 => rng.below(0x1000) as i32 - 0x1400,
            _ => 0x800 + rng.below(0x2000) as i32,
        } as u32,
    );
    for k in 21..24 {
        g.write_ctrl(k, rng.below(0x1000));
    }
    g.write_ctrl(24, 160 << 16);
    g.write_ctrl(25, 120 << 16);
    g.write_ctrl(26, 0x100 + rng.below(0x200));
    g.write_ctrl(27, rng.next() & 0xFFFF);
    g.write_ctrl(28, rng.next());
}

#[test]
fn the_ground_grid_emits_what_the_port_kernels_say() {
    differential!(prot(898, GRID));
    let (Some(mut cpu), Some(disc)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &disc.read_entry(898).expect("PROT 0898"));
    let mut rng = Rng(0x4752_4944);
    let mut classes = [0u32; 3];
    let (mut emitted, mut rejected) = (0u32, 0u32);
    for n in 0..1_500 {
        seed_camera(&mut cpu.gte, &mut rng);
        let cam = cpu.gte.clone();
        let w = 1 + rng.below(12) as i16;
        let h = 1 + rng.below(12) as i16;
        let shift = rng.below(3) as u8;
        let rgb = rng.next() & 0x00FF_FFFF;
        let code = rng.next() as u8;
        cpu.mem.wr32(RGBC, rgb | u32::from(code) << 24);
        cpu.mem.wr32(VIS_PTR, VIS);
        cpu.mem.load(VIS, &[0x55; 0x100]);
        cpu.mem.wr32(PACKET_PTR, PACKETS);
        cpu.mem.wr8(OT_SHIFT, shift);
        cpu.mem.wr32(OT_PTR, OT);
        cpu.mem.wr16(DIMS, w as u16);
        cpu.mem.wr16(DIMS + 2, h as u16);
        cpu.mem.wr32(OT_MASK, 0x00FF_FFFF);
        let slot = OT + ((0x3FEC >> shift) << 2);
        let ot_seed = 0xA500_0000 | (rng.next() & 0x00FF_FFFF);
        cpu.mem.wr32(slot, ot_seed);
        assert_eq!(cpu.call(GRID, &[]), Stop::Returned, "n {n}");

        // Pass 1: the probe's view depth through the rotation and translation.
        let mut vis = Vec::new();
        let mut g = cam.clone();
        for row in 0..i32::from(h) {
            for col in 0..i32::from(w) {
                let (x, z) = probe_point(w, h, col, row);
                g.write_data(0, x as u32);
                g.write_data(1, z as u32);
                g.execute(0x0048_0012);
                let ir3 = g.read_data(11) as i16 as i32;
                let c = classify_cell(ir3);
                classes[match c {
                    CellVis::Behind => 0,
                    CellVis::Far => 1,
                    CellVis::Visible => 2,
                }] += 1;
                vis.push(c);
            }
        }
        let got_vis = cpu.mem.read(VIS, vis.len() + 1);
        let want_vis: Vec<u8> = vis
            .iter()
            .map(|c| c.as_byte() as u8)
            .chain([0x55])
            .collect();
        assert_eq!(got_vis, want_vis, "n {n} {w}x{h}");

        // Pass 2: the lattice of each visible cell, four quads a cell.
        let mut packets: Vec<u32> = Vec::new();
        let mut ot = ot_seed;
        let mut code_now = code;
        for (k, c) in vis.iter().enumerate() {
            if *c != CellVis::Visible {
                continue;
            }
            let (row, col) = (k as i32 / i32::from(w), k as i32 % i32::from(w));
            let lattice = cell_lattice(w, h, col, row);
            let mut sxy = [[0u32; 3]; 3];
            let mut sz = [[0u32; 3]; 3];
            for (r, lrow) in lattice.iter().enumerate() {
                for (i, (x, z)) in lrow.iter().enumerate() {
                    assert_eq!(implied_y(*x), ((*x as u32) >> 16) as i16);
                    g.write_data(2 * i, *x as u32);
                    g.write_data(2 * i + 1, *z as u32);
                }
                g.execute(0x0028_0030);
                for i in 0..3 {
                    sxy[r][i] = g.read_data(12 + i);
                    sz[r][i] = g.read_data(17 + i);
                }
            }
            // The code byte reads `CODE_WHILE_EMITTING` from the projection
            // on, and `CODE_AFTER_EMITTING` once the cell is done.
            code_now = CODE_AFTER_EMITTING;
            let corner = |r: usize, i: usize| (sxy[r][i] as i16, (sxy[r][i] >> 16) as i16);
            if !quad_on_screen([corner(0, 0), corner(0, 2), corner(2, 0), corner(2, 2)]) {
                rejected += 1;
                continue;
            }
            g.write_data(6, rgb | u32::from(CODE_WHILE_EMITTING) << 24);
            for sub_row in 0..2 {
                for sub_col in 0..2 {
                    let uv = sub_tile_uv(sub_tile_index(sub_row, sub_col));
                    let idx = sub_quad_lattice(sub_row, sub_col);
                    let pk = PACKETS + 4 * packets.len() as u32;
                    let mut words = vec![(ot & 0x00FF_FFFF) | 0x0C00_0000];
                    for (v, (r, i)) in idx.iter().enumerate() {
                        g.write_data(8, grid_ir0_raw(sz[*r][*i] as i32) as u32);
                        g.execute(0x0078_0010);
                        let uvw = u32::from(uv[v].0) | u32::from(uv[v].1) << 8;
                        let attr = match v {
                            0 => u32::from(CLUT_ATTR) << 16,
                            1 => u32::from(TPAGE_ATTR) << 16,
                            _ => 0,
                        };
                        words.extend([g.read_data(22), sxy[*r][*i], attr | uvw]);
                    }
                    ot = (pk & 0x00FF_FFFF) | (ot & 0xFF00_0000);
                    packets.extend(words);
                    emitted += 1;
                }
            }
        }
        let got: Vec<u32> = (0..packets.len() as u32)
            .map(|k| cpu.mem.rd32(PACKETS + 4 * k))
            .collect();
        for (q, (a, b)) in got
            .chunks(PACKET_WORDS as usize)
            .zip(packets.chunks(PACKET_WORDS as usize))
            .enumerate()
        {
            assert_eq!(a, b, "n {n} {w}x{h} packet {q}");
        }
        assert_eq!(cpu.mem.rd32(slot), ot, "n {n} OT slot");
        assert_eq!(
            cpu.mem.rd32(PACKET_PTR),
            PACKETS + 4 * packets.len() as u32,
            "n {n}"
        );
        assert_eq!(
            cpu.mem.rd32(RGBC),
            rgb | u32::from(code_now) << 24,
            "n {n} colour word"
        );
    }
    eprintln!("classes {classes:?}, {emitted} quads, {rejected} cells off screen");
    assert!(classes.iter().all(|&c| c > 3000), "{classes:?}");
    assert!(emitted > 10_000 && rejected > 500, "{emitted} {rejected}");
}
