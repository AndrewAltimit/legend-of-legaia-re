//! Handler bank 3's tessellating quad leaf `FUN_80045BB4`, run through the
//! per-prim dispatcher `FUN_80043390` with the GTE seeded, against
//! `legaia_render_kernels::sprite_arm::{subdivide_quad, ColourWord}`.
//!
//! The rig: a hand-built object (vertex table, one kind-19 prim group of
//! gouraud-textured quads, a zero-count terminator), the dispatcher's
//! scratchpad state (packet cursor, ordering table, link mask, near cutoff,
//! OT shift) and a GTE holding an identity rotation, a translation in
//! front of the eye, a screen centre and a focal length. The colour word
//! carries the bank-3 bit and the depth-cue level is the call's `a2`.
//!
//! Every `POLY_GT3` linked into the ordering table is decoded and held to
//! the port: the sixteen sub-triangles `subdivide_quad` makes of each quad,
//! in emit order, their corners projected by the interpreter's own GTE
//! (`RTPS`, the hardware divide) - the port's positions are model space and
//! a host projects them - their texels, CLUT and page, and their colours,
//! which must be `ColourWord::cue` of the port's interpolated colour. Retail
//! lists each sub-triangle's corners in the reverse of the port's order;
//! the port keeps the parent quad's winding for the host's face cull, and a
//! `GT3` rasterises the same either way.

use crate::common::{Rng, scus_cpu};
use legaia_render_kernels::sprite_arm::{ColourWord, SplitVertex, subdivide_quad};
use psx_retail_exec::gte::Gte;
use psx_retail_exec::{Cpu, Stop};

const DISPATCH: u32 = 0x8004_3390;
const LEAF: u32 = 0x8004_5BB4;
const PRIM_PTR: u32 = 0x1F80_03A0;
const OT_SHIFT: u32 = 0x1F80_03A4;
const OT_PTR: u32 = 0x1F80_03F4;
const LINK_MASK: u32 = 0x1F80_03FC;
const NEAR_OTZ: u32 = 0x1F80_037E;
const SYSTEM_FLAGS: u32 = 0x1F80_0394;
const OBJECT: u32 = 0x8011_0000;
const VERTS: u32 = 0x8011_1000;
const PRIMS_IN: u32 = 0x8011_2000;
const PACKETS: u32 = 0x8012_0000;
const OT: u32 = 0x8014_0000;
const OT_WORDS: u32 = 0x1000;
const OT_END: u32 = 0x00FF_FFFF;
const OFX: i32 = 160;
const OFY: i32 = 120;
const H: u32 = 0x100;
const TZ: i32 = 0x400;

fn seat(cpu: &mut Cpu) {
    let g = &mut cpu.gte;
    // Identity rotation, translation (0, 0, TZ), screen centre, focal length.
    g.write_ctrl(0, 0x1000);
    g.write_ctrl(1, 0);
    g.write_ctrl(2, 0x1000);
    g.write_ctrl(3, 0);
    g.write_ctrl(4, 0x1000);
    g.write_ctrl(5, 0);
    g.write_ctrl(6, 0);
    g.write_ctrl(7, TZ as u32);
    g.write_ctrl(24, (OFX << 16) as u32);
    g.write_ctrl(25, (OFY << 16) as u32);
    g.write_ctrl(26, H);
    g.write_ctrl(27, 0);
    g.write_ctrl(28, 0);
    cpu.mem.wr32(PRIM_PTR, PACKETS);
    cpu.mem.wr32(OT_PTR, OT);
    cpu.mem.wr8(OT_SHIFT, 0);
    cpu.mem.wr16(NEAR_OTZ, 0);
    cpu.mem.wr32(SYSTEM_FLAGS, 0);
    cpu.mem.wr32(LINK_MASK, 0x00FF_FFFF);
    for w in 0..OT_WORDS {
        cpu.mem.wr32(OT + 4 * w, OT_END);
    }
}

/// The harness GTE's projection of a model point under [`seat`]'s state.
fn project(p: [i16; 3]) -> (i16, i16) {
    let mut g = Gte::default();
    g.write_ctrl(0, 0x1000);
    g.write_ctrl(2, 0x1000);
    g.write_ctrl(4, 0x1000);
    g.write_ctrl(7, TZ as u32);
    g.write_ctrl(24, (OFX << 16) as u32);
    g.write_ctrl(25, (OFY << 16) as u32);
    g.write_ctrl(26, H);
    g.write_data(0, (p[0] as u16 as u32) | (p[1] as u16 as u32) << 16);
    g.write_data(1, p[2] as u16 as u32);
    g.execute(0x0018_0001); // RTPS
    let sxy = g.read_data(14);
    (sxy as i16, (sxy >> 16) as i16)
}

#[derive(Debug, PartialEq, Eq)]
struct Gt3 {
    code: u8,
    rgb: [[u8; 3]; 3],
    xy: [(i16, i16); 3],
    uv: [(u8, u8); 3],
    clut: u16,
    tpage: u16,
}

fn read_gt3(cpu: &mut Cpu, p: u32) -> Gt3 {
    let m = &mut cpu.mem;
    assert_eq!(m.rd8(p + 3), 9, "a POLY_GT3 is nine words");
    let at = |k: u32| p + 4 + 12 * k;
    Gt3 {
        code: m.rd8(p + 7),
        rgb: [0, 1, 2].map(|k| [m.rd8(at(k)), m.rd8(at(k) + 1), m.rd8(at(k) + 2)]),
        xy: [0, 1, 2].map(|k| (m.rd16(at(k) + 4) as i16, m.rd16(at(k) + 6) as i16)),
        uv: [0, 1, 2].map(|k| (m.rd8(at(k) + 8), m.rd8(at(k) + 9))),
        clut: m.rd16(p + 0xE),
        tpage: m.rd16(p + 0x1A),
    }
}

#[test]
fn the_bank3_quad_split_matches_subdivide_quad() {
    differential!(scus(DISPATCH), scus(LEAF));
    let Some(mut cpu) = scus_cpu() else {
        return;
    };
    // The leaf is reached through the dispatcher's jump table.
    cpu.watched.insert(LEAF);
    let mut rng = Rng(0x5350_4C54);
    let mut drawn = 0;
    for case in 0..1500 {
        seat(&mut cpu);
        // One quad in front of the eye, wound so it faces it, small enough
        // to stay on screen.
        let (cx, cy) = (
            rng.below(0x100) as i16 - 0x80,
            rng.below(0xC0) as i16 - 0x60,
        );
        let (hw, hh) = (8 + rng.below(0x30) as i16, 8 + rng.below(0x30) as i16);
        let mut rz = Rng(rng.next() as u64);
        let mut zr = || rz.below(0x40) as i16 - 0x20;
        let corners_pos = [
            [cx - hw, cy - hh, zr()],
            [cx + hw, cy - hh, zr()],
            [cx - hw, cy + hh, zr()],
            [cx + hw, cy + hh, zr()],
        ];
        let rgb: [[u8; 3]; 4] = std::array::from_fn(|_| {
            let w = rng.next();
            [w as u8, (w >> 8) as u8, (w >> 16) as u8]
        });
        let uv: [[u8; 2]; 4] = std::array::from_fn(|_| [rng.next() as u8, rng.next() as u8]);
        let clut = rng.next() as u16;
        let tpage = rng.next() as u16 & 0xFF9F;
        // The colour word: bank 3 (`0x20`), both sides (`0x08`), a random
        // ABR and ABE, and a far colour; `a2` the depth-cue level.
        let abr = rng.below(4);
        let semi = rng.below(2);
        let far = rng.next() & 0x00FF_FFFF;
        let word = semi << 31 | (0x28 | abr) << 24 | far;
        let level = 1 + rng.below(0x1000);

        for (k, p) in corners_pos.iter().enumerate() {
            let at = VERTS + 8 * k as u32;
            cpu.mem.wr16(at, p[0] as u16);
            cpu.mem.wr16(at + 2, p[1] as u16);
            cpu.mem.wr16(at + 4, p[2] as u16);
            cpu.mem.wr16(at + 6, 0);
        }
        // Kind 19, one prim, then a zero-count terminator.
        cpu.mem.wr32(PRIMS_IN, 19 << 17 | 1);
        cpu.mem.wr32(PRIMS_IN + 4, 0);
        let prim = PRIMS_IN + 8;
        let col = |c: [u8; 3]| u32::from(c[0]) | u32::from(c[1]) << 8 | u32::from(c[2]) << 16;
        let uvw = |t: [u8; 2]| u32::from(t[0]) | u32::from(t[1]) << 8;
        cpu.mem.wr32(prim, col(rgb[0]));
        cpu.mem.wr32(prim + 4, col(rgb[1]));
        cpu.mem.wr32(prim + 8, col(rgb[2]));
        cpu.mem.wr32(prim + 0xC, col(rgb[3]));
        cpu.mem
            .wr32(prim + 0x10, uvw(uv[0]) | u32::from(clut) << 16);
        cpu.mem
            .wr32(prim + 0x14, uvw(uv[1]) | u32::from(tpage) << 16);
        cpu.mem.wr32(prim + 0x18, uvw(uv[2]) | uvw(uv[3]) << 16);
        cpu.mem.wr32(prim + 0x1C, 8 << 16);
        cpu.mem.wr32(prim + 0x20, (2 * 8) | ((3 * 8) << 16));
        cpu.mem.wr32(prim + 0x24, 0);
        cpu.mem.wr32(prim + 0x28, 0);
        cpu.mem.wr32(OBJECT, VERTS);
        cpu.mem.wr32(OBJECT + 8, 0);
        cpu.mem.wr32(OBJECT + 0xC, 0);
        cpu.mem.wr32(OBJECT + 0x10, PRIMS_IN);
        cpu.mem.wr32(OBJECT + 0x14, 1);
        cpu.mem.wr32(OBJECT + 0x18, 0);
        assert_eq!(
            cpu.call(DISPATCH, &[OBJECT, word, level]),
            Stop::Returned,
            "case {case}"
        );

        // Every packet linked, in emission order (each bucket's chain runs
        // newest first; emission order is packet address order).
        let mut linked = Vec::new();
        for w in 0..OT_WORDS {
            let mut p = cpu.mem.rd32(OT + 4 * w) & 0x00FF_FFFF;
            while p != OT_END {
                linked.push(0x8000_0000 | p);
                p = cpu.mem.rd32(0x8000_0000 | p) & 0x00FF_FFFF;
            }
        }
        linked.sort_unstable();
        let got: Vec<Gt3> = linked.iter().map(|&p| read_gt3(&mut cpu, p)).collect();

        let colour = ColourWord::from_words(word, level as u16);
        let tris = subdivide_quad(std::array::from_fn(|c| SplitVertex {
            pos: corners_pos[c],
            rgb: rgb[c],
            uv: uv[c],
        }));
        let want: Vec<Gt3> = tris
            .iter()
            .map(|t| {
                // Retail's corner order is the port's reversed.
                let t = [t[2], t[1], t[0]];
                Gt3 {
                    code: 0x34 | (semi as u8) << 1,
                    rgb: t.map(|v| colour.cue(v.rgb)),
                    xy: t.map(|v| project(v.pos)),
                    uv: t.map(|v| (v.uv[0], v.uv[1])),
                    clut,
                    tpage: tpage | (abr as u16) << 5,
                }
            })
            .collect();
        assert_eq!(got.len(), 16, "case {case}: sixteen GT3s");
        for (k, (g, w)) in got.iter().zip(&want).enumerate() {
            assert_eq!(
                g, w,
                "case {case} triangle {k} word {word:#x} level {level:#x}"
            );
        }
        drawn += got.len();
    }
    assert!(drawn > 10_000, "{drawn}");
}
