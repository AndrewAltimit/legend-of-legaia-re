//! The far arm's slab clip `FUN_80027F00` and its corner lerp `FUN_80029724`
//! off the disc, against `legaia_engine_effects::object_effect::far_arm_mesh`
//! with a `MeshClip` measuring the mesh's own `y`.
//!
//! The routine is called directly on a ring of `0x1C`-byte corner records
//! (the effect-space measure at `+4`, the position at `+0xC`, the colour word
//! at `+0x14`, the texel at `+0x18`) with the last record copied in front of
//! the ring and the first behind it, as the arm lays them out, and the bounds
//! at `0x1F800380` / `0x1F800382`. Triangles and quads (the quad's ring is
//! `v0, v1, v3, v2`), integer corners, random slabs; gouraud-textured shape.
//!
//! The port clips in `f32` and its corners are mesh-space floats (the hosts
//! hand it meshes already scaled by the node's render scale, so an integer
//! clip would round a scaled mesh). Compared: the corner count and order, and
//! each corner's position to within the unit and a half retail's truncated
//! q12 fraction and `>> 12` floor take, its texel and colour to within two
//! (retail floors, the port rounds to the byte).

use crate::common::{Rng, scus_cpu};
use legaia_engine_effects::object_effect::{MeshClip, far_arm_mesh};
use psx_retail_exec::{Cpu, Stop, V0};

const CLIP: u32 = 0x8002_7F00;
const BOUNDS: u32 = 0x1F80_0380;
const RECORDS: u32 = 0x8011_0000;
const OUT: u32 = 0x8012_0000;
const DESC: u32 = 0x8013_0000;

#[derive(Debug, Clone, Copy)]
struct Corner {
    pos: [i16; 3],
    rgb: [u8; 3],
    uv: [u8; 2],
}

fn write_record(cpu: &mut Cpu, at: u32, c: &Corner) {
    cpu.mem.load(at, &[0u8; 0x1C]);
    cpu.mem.wr32(at + 4, c.pos[1] as i32 as u32);
    for (k, v) in c.pos.iter().enumerate() {
        cpu.mem.wr16(at + 0xC + 2 * k as u32, *v as u16);
    }
    cpu.mem.wr32(
        at + 0x14,
        u32::from(c.rgb[0]) | u32::from(c.rgb[1]) << 8 | u32::from(c.rgb[2]) << 16,
    );
    cpu.mem.wr8(at + 0x18, c.uv[0]);
    cpu.mem.wr8(at + 0x19, c.uv[1]);
}

#[test]
fn the_far_arm_clip_keeps_what_retail_keeps() {
    differential!(scus(CLIP), scus(0x8002_9724));
    let Some(mut cpu) = scus_cpu() else {
        return;
    };
    // The corner lerp is a `jal` from the clip loop.
    cpu.watched.insert(0x8002_9724);
    // Gouraud-textured: the shape byte's two bits.
    cpu.mem.wr32(DESC, 0x0300_0000);
    cpu.mem.wr32(DESC + 4, 0);
    let mut rng = Rng(0x434C_4950);
    let (mut clipped, mut empty) = (0, 0);
    for case in 0..6000 {
        let quad = rng.below(2) == 0;
        let n = if quad { 4 } else { 3 };
        let corners: Vec<Corner> = (0..n)
            .map(|_| Corner {
                pos: std::array::from_fn(|_| rng.below(0x400) as i16 - 0x200),
                rgb: std::array::from_fn(|_| rng.next() as u8),
                uv: std::array::from_fn(|_| rng.next() as u8),
            })
            .collect();
        let lo = rng.below(0x400) as i16 - 0x200;
        let hi = lo + rng.below(0x300) as i16;
        // The ring the arm walks: a quad's is `v0, v1, v3, v2`.
        let ring: Vec<Corner> = if quad {
            vec![corners[0], corners[1], corners[3], corners[2]]
        } else {
            corners.clone()
        };
        write_record(&mut cpu, RECORDS - 0x1C, &ring[n - 1]);
        for (k, c) in ring.iter().enumerate() {
            write_record(&mut cpu, RECORDS + 0x1C * k as u32, c);
        }
        write_record(&mut cpu, RECORDS + 0x1C * n as u32, &ring[0]);
        cpu.mem.wr16(BOUNDS, lo as u16);
        cpu.mem.wr16(BOUNDS + 2, hi as u16);
        assert_eq!(
            cpu.call(CLIP, &[n as u32, RECORDS, OUT, DESC]),
            Stop::Returned,
            "case {case}"
        );
        let count = cpu.regs.r[V0] as usize;
        let got: Vec<Corner> = (0..count as u32)
            .map(|k| {
                let at = OUT + 0x10 * k;
                let w = cpu.mem.rd32(at + 8);
                Corner {
                    pos: std::array::from_fn(|a| cpu.mem.rd16(at + 2 * a as u32) as i16),
                    rgb: [w as u8, (w >> 8) as u8, (w >> 16) as u8],
                    uv: [cpu.mem.rd8(at + 0xC), cpu.mem.rd8(at + 0xD)],
                }
            })
            .collect();

        let mesh = legaia_tmd::mesh::VramMesh {
            positions: corners.iter().map(|c| c.pos.map(f32::from)).collect(),
            uvs: corners.iter().map(|c| c.uv).collect(),
            cba_tsb: vec![[0, 0]; n],
            indices: if quad {
                vec![0, 1, 2, 2, 1, 3]
            } else {
                vec![0, 1, 2]
            },
            normals: vec![[0.0; 3]; n],
            colors: corners.iter().map(|c| c.rgb).collect(),
        };
        let clip = MeshClip {
            m: [0.0, 1.0, 0.0],
            lo: f32::from(lo),
            hi: f32::from(hi),
        };
        let out = far_arm_mesh(&mesh, Some(&clip), |_| true);
        let ctx = format!("case {case}: lo {lo} hi {hi} ring {ring:?}");
        if count < 3 {
            assert!(out.positions.is_empty(), "{ctx}: retail draws nothing");
            empty += 1;
            continue;
        }
        assert_eq!(out.positions.len(), count, "{ctx}: corners {got:?}");
        for (k, g) in got.iter().enumerate() {
            let p = out.positions[k];
            for a in 0..3 {
                assert!(
                    (f32::from(g.pos[a]) - p[a]).abs() < 1.5,
                    "{ctx}: corner {k} position {:?} vs {p:?}",
                    g.pos
                );
            }
            for a in 0..2 {
                assert!(
                    (i32::from(g.uv[a]) - i32::from(out.uvs[k][a])).abs() <= 2,
                    "{ctx}: corner {k} texel {:?} vs {:?}",
                    g.uv,
                    out.uvs[k]
                );
            }
            for a in 0..3 {
                assert!(
                    (i32::from(g.rgb[a]) - i32::from(out.colors[k][a])).abs() <= 2,
                    "{ctx}: corner {k} colour {:?} vs {:?}",
                    g.rgb,
                    out.colors[k]
                );
            }
        }
        clipped += usize::from(count != n);
    }
    assert!(clipped > 1000 && empty > 100, "{clipped} {empty}");
}
