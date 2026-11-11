//! The shatter renderer `FUN_80029888` and its per-primitive half
//! `FUN_80029DD8` off the disc, over real Legaia TMDs registered by
//! `FUN_800268DC`, against `legaia_render_kernels::shatter::ShatterObject`.
//!
//! The GTE holds a random object rotation and a translation in front of the
//! eye, the scratchpad the burst offset (`0x1F800354`), the sweep front
//! (`0x1F80035A`) and the node's view translation (`0x1F8003BC`). A probe on
//! the per-triangle `RTPT` (`0x8002A328`) reads the three vertices the routine
//! hands the projector - the shattered positions - and each must be the
//! port's `ShatterObject::triangle` of the same triangle (quads as
//! `(v0, v1, v2)` then `(v1, v2, v3)`), in draw order, over every flag the
//! row word carries: the default spin-and-push, no spin, shrink, no push and
//! the three sweeps.

use crate::common::{Rng, disc, scus_cpu};
use legaia_container_formats::tmd_scan::scan_buffer;
use legaia_render_kernels::gte::GteMat3;
use legaia_render_kernels::shatter::{ShatterObject, ShatterRow, SweepFrame};
use psx_retail_exec::{Cpu, Stop};
use std::cell::RefCell;
use std::rc::Rc;

const REGISTER: u32 = 0x8002_68DC;
const SHATTER: u32 = 0x8002_9888;
const TRIANGLE: u32 = 0x8002_9DD8;
const PROJECT: u32 = 0x8002_A328;
const TMD: u32 = 0x8010_0000;
const OFF: u32 = 0x1F80_0354;
const NODE_T: u32 = 0x1F80_03BC;
const POOL_PTR: u32 = 0x1F80_03A0;
const OT_PTR: u32 = 0x1F80_03F4;
const POOL: u32 = 0x8018_0000;
const OT: u32 = 0x801A_0000;

type Tri = [[i16; 3]; 3];

/// The TMD's primitives as the renderer walks them: each object's triangles
/// over object-local vertices, a quad as two.
fn port_triangles(bytes: &[u8], tmd: &legaia_tmd::Tmd) -> Vec<Vec<Tri>> {
    tmd.objects
        .iter()
        .map(|o| {
            let groups = legaia_tmd::legaia_prims::iter_groups_lenient(
                bytes,
                o.primitives_byte_offset,
                o.primitives_byte_size,
            );
            let v = |i: u16| {
                let p = o.vertices.get(usize::from(i)).copied();
                p.map_or([0; 3], |p| [p.x, p.y, p.z])
            };
            let mut out = Vec::new();
            for g in groups {
                if !(0x10..=0x27).contains(&g.header.flags) {
                    continue;
                }
                for p in &g.prims {
                    let ix: Vec<u16> = p.vertex_indices().iter().copied().collect();
                    out.push([v(ix[0]), v(ix[1]), v(ix[2])]);
                    if ix.len() == 4 {
                        out.push([v(ix[1]), v(ix[2]), v(ix[3])]);
                    }
                }
            }
            out
        })
        .collect()
}

fn seat(cpu: &mut Cpu, rot: &GteMat3, tr: [i32; 3]) {
    let m = rot.m;
    let p = |a: i16, b: i16| (a as u16 as u32) | (b as u16 as u32) << 16;
    let g = &mut cpu.gte;
    g.write_ctrl(0, p(m[0][0], m[0][1]));
    g.write_ctrl(1, p(m[0][2], m[1][0]));
    g.write_ctrl(2, p(m[1][1], m[1][2]));
    g.write_ctrl(3, p(m[2][0], m[2][1]));
    g.write_ctrl(4, m[2][2] as u16 as u32);
    for (k, t) in tr.iter().enumerate() {
        g.write_ctrl(5 + k, *t as u32);
    }
    g.write_ctrl(24, 160 << 16);
    g.write_ctrl(25, 120 << 16);
    g.write_ctrl(26, 0x200);
    g.write_ctrl(29, 0x555);
    g.write_ctrl(30, 0x400);
    cpu.mem.wr32(POOL_PTR, POOL);
    cpu.mem.wr32(OT_PTR, OT);
}

#[test]
fn the_shatter_renderer_moves_each_triangle_where_the_port_does() {
    differential!(scus(SHATTER), scus(TRIANGLE));
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        return;
    };
    // The trig table pointers `RotMatrix` reads, as the boot sets them.
    cpu.mem.wr32(0x8007_B81C, 0x8007_0A2C);
    cpu.mem.wr32(0x8007_B7F8, 0x8007_122C);
    let seen = Rc::new(RefCell::new(Vec::<Tri>::new()));
    let s = seen.clone();
    cpu.probe(PROJECT, move |_, _, g| {
        let v = |k: usize| {
            let xy = g.read_data(2 * k);
            let z = g.read_data(2 * k + 1);
            [xy as i16, (xy >> 16) as i16, z as i16]
        };
        s.borrow_mut().push([v(0), v(1), v(2)]);
    });
    // The per-primitive half is a `jal` from the renderer.
    cpu.watched.insert(TRIANGLE);
    let mut rng = Rng(0x5348_4154);
    let (mut tmds, mut tris) = (0, 0);
    for entry in 0..1233 {
        let Ok(buf) = d.read_entry(entry) else {
            continue;
        };
        for hit in scan_buffer(&buf).into_iter().take(2) {
            let bytes = &buf[hit.offset..hit.offset + hit.byte_len];
            if bytes.len() > 0x4_0000 {
                continue;
            }
            let Ok(tmd) = legaia_tmd::parse(bytes) else {
                continue;
            };
            let objects = port_triangles(bytes, &tmd);
            cpu.mem.load(TMD, bytes);
            assert_eq!(cpu.call(REGISTER, &[TMD]), Stop::Returned);
            tmds += 1;
            for (k, want_tris) in objects.iter().enumerate() {
                let value = 1 + rng.below(0x1000) as u16;
                let flags =
                    [0u32, 1, 2, 4, 5, 6, 8, 0x10, 0x20, 0x0C, 0x1C][rng.below(11) as usize];
                let row = ShatterRow {
                    off: std::array::from_fn(|_| rng.below(0x100) as i16 - 0x80),
                    h3: rng.below(0x800) as i16 - 0x400,
                    flags,
                };
                let word = u32::from(value) | flags << 16;
                let angles = (rng.next() as i16, rng.next() as i16, rng.next() as i16);
                let rot = legaia_render_kernels::battle_intro::euler_rot_psx(angles);
                let tr = [
                    rng.below(0x200) as i32 - 0x100,
                    rng.below(0x200) as i32 - 0x100,
                    0x1000 + rng.below(0x2000) as i32,
                ];
                let node_t: [i32; 3] = std::array::from_fn(|_| rng.below(0x400) as i32 - 0x200);
                seat(&mut cpu, &rot, tr);
                for (a, o) in row.off.iter().enumerate() {
                    cpu.mem.wr16(OFF + 2 * a as u32, *o as u16);
                }
                cpu.mem.wr16(OFF + 6, row.h3 as u16);
                for (a, t) in node_t.iter().enumerate() {
                    cpu.mem.wr32(NODE_T + 4 * a as u32, *t as u32);
                }
                for i in 0..0x1000 {
                    cpu.mem.wr32(OT + 4 * i, 0x00FF_FFFF);
                }
                seen.borrow_mut().clear();
                let obj = TMD + 12 + 0x1C * k as u32;
                assert_eq!(
                    cpu.call(SHATTER, &[obj, 0x0080_8080, 0x800, word]),
                    Stop::Returned,
                    "entry {entry} object {k}"
                );
                let mut port = ShatterObject::new(word, &row, SweepFrame { rot, tr, node_t });
                let want: Vec<Tri> = want_tris
                    .iter()
                    .map(|t| {
                        port.triangle(*t)
                            .map(|v| std::array::from_fn(|a| (v[a] + i32::from(row.off[a])) as i16))
                    })
                    .collect();
                let got = seen.borrow();
                let ctx = format!("entry {entry} object {k} word {word:#x} row {row:?}");
                assert_eq!(got.len(), want.len(), "{ctx}: triangles");
                for (n, (g, w)) in got.iter().zip(&want).enumerate() {
                    assert_eq!(g, w, "{ctx}: triangle {n}");
                }
                tris += got.len();
            }
            if tmds >= 120 {
                break;
            }
        }
        if tmds >= 120 {
            break;
        }
    }
    assert!(tmds >= 120 && tris > 10_000, "{tmds} {tris}");
}
