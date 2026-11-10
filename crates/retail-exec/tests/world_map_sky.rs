//! The overworld sky band `FUN_801F73E4` (PROT 0901) off the disc, the view
//! build `FUN_800172C0`, the `TR` load and the `RTPS` live beneath it and the
//! system-flag test answered by a hook, over random overworld cameras and
//! both flag states, against `sky_band_sprites`: every `SPRT` packet it links
//! (position, texel origin, CLUT, size, colour), the draw-mode packet's page,
//! the bucket both go into, and the yaw word it restores.

use crate::common::{Rng, disc, scus_cpu};
use legaia_render_kernels::world_map_sky::{SKY_CLUT, SKY_TPAGE, sky_band_sprites};
use psx_retail_exec::{A0, A1, Stop, V0};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

const SKY: u32 = 0x801F_73E4;
const SLOT_B_BASE: u32 = 0x801F_69D8;
const ADD_PRIM: u32 = 0x8003_D2C4;
const FLAG_TEST: u32 = 0x8003_CE64;
const ANGLES: u32 = 0x8007_B790;
const EYE: u32 = 0x8008_40B8;
const FOCUS: u32 = 0x8008_9118;
const H: u32 = 0x8007_B6F4;
const BASE: u32 = 0x8007_BF10;
const PRIM_PTR: u32 = 0x1F80_03A0;
const OT_DEPTH: u32 = 0x1F80_03A6;
const OT_PTR: u32 = 0x1F80_03F4;
const PRIMS: u32 = 0x8012_0000;
const OT: u32 = 0x8014_0000;

#[test]
fn the_sky_band_matches_sky_band_sprites() {
    differential!(prot(901, SKY));
    let (Some(mut cpu), Some(disc)) = (scus_cpu(), disc()) else {
        return;
    };
    cpu.mem
        .load(SLOT_B_BASE, &disc.read_entry(901).expect("PROT 0901"));
    cpu.mem.wr32(0x8007_B81C, 0x8007_0A2C);
    cpu.mem.wr32(0x8007_B7F8, 0x8007_122C);
    for r in 0..3 {
        for c in 0..3 {
            let v: u16 = if r == c { 0x6000 } else { 0 };
            cpu.mem.wr16(BASE + 2 * (3 * r + c) as u32, v);
        }
    }
    let links: Rc<RefCell<Vec<(u32, u32)>>> = Rc::default();
    {
        let links = links.clone();
        cpu.hook(ADD_PRIM, move |r, _| {
            links.borrow_mut().push((r.r[A0], r.r[A1]))
        });
    }
    let dim = Rc::new(Cell::new(false));
    {
        let dim = dim.clone();
        cpu.hook(FLAG_TEST, move |r, _| {
            assert_eq!(r.r[A0], 0x14C);
            r.r[V0] = u32::from(dim.get());
        });
    }
    let mut rng = Rng(0x534B_5942);
    let (mut sprites, mut clipped, mut dropped) = (0usize, 0usize, 0usize);
    for round in 0..3000 {
        let g: [i32; 10] = [
            rng.below(0x180) as i32 + 0x100,
            rng.next() as i16 as i32,
            rng.below(0x40) as i32 - 0x20,
            rng.below(0x400) as i32 - 0x200,
            rng.below(0x800) as i32,
            0x1000 + rng.below(0x4000) as i32,
            rng.below(0x20000) as i32 - 0x10000,
            rng.below(0x400) as i32 - 0x200,
            rng.below(0x20000) as i32 - 0x10000,
            0x100 + rng.below(0x200) as i32,
        ];
        for k in 0..3 {
            cpu.mem.wr16(ANGLES + 2 * k as u32, g[k] as u16);
            cpu.mem.wr32(EYE + 4 * k as u32, g[3 + k] as u32);
            cpu.mem.wr32(FOCUS + 4 * k as u32, g[6 + k] as u32);
        }
        cpu.mem.wr16(H, g[9] as u16);
        cpu.gte.write_ctrl(24, 160 << 16);
        cpu.gte.write_ctrl(25, 114 << 16);
        cpu.gte.write_ctrl(26, g[9] as u32);
        let depth = 0x100 + rng.below(0x100) as u16;
        cpu.mem.wr16(OT_DEPTH, depth);
        cpu.mem.wr32(OT_PTR, OT);
        cpu.mem.wr32(PRIM_PTR, PRIMS);
        dim.set(round % 3 == 0);
        links.borrow_mut().clear();
        assert_eq!(cpu.call(SKY, &[]), Stop::Returned, "round {round}");
        let ctx = format!("round {round} globals {g:?}");
        assert_eq!(
            cpu.mem.rd16(ANGLES + 2) as i16 as i32,
            g[1],
            "yaw restored, {ctx}"
        );

        let port = sky_band_sprites(&g, dim.get());
        let bucket = OT + u32::from(depth) * 4 - 8;
        let l = links.borrow();
        assert!(!l.is_empty(), "the draw-mode packet always links, {ctx}");
        let (mode_ot, mode) = *l.last().unwrap();
        assert_eq!(mode_ot, bucket, "draw-mode bucket, {ctx}");
        assert_eq!(
            cpu.mem.rd32(mode + 4) & 0x1FF,
            u32::from(SKY_TPAGE),
            "page, {ctx}"
        );
        let mut retail = Vec::new();
        for &(ot, p) in &l[..l.len() - 1] {
            assert_eq!(ot, bucket, "sprite bucket, {ctx}");
            let mut w = |k: u32| cpu.mem.rd32(p + 4 * k);
            let (w1, w2, w3, w4) = (w(1), w(2), w(3), w(4));
            assert_eq!(w1 >> 24, 0x64, "SPRT, {ctx}");
            assert_eq!((w3 >> 16) as u16, SKY_CLUT, "CLUT, {ctx}");
            retail.push((
                w2 as i16,
                (w2 >> 16) as i16,
                w3 as u8,
                (w3 >> 8) as u8,
                w4 as i16,
                (w4 >> 16) as i16,
                w1 & 0xFF_FFFF,
            ));
        }
        // The port drops a sprite the top clip left no rows; retail links it.
        let kept: Vec<_> = retail.iter().copied().filter(|s| s.5 > 0).collect();
        dropped += retail.len() - kept.len();
        let want: Vec<_> = port
            .iter()
            .map(|s| (s.x, s.y, s.u, s.v, s.w, s.h, s.color))
            .collect();
        assert_eq!(kept, want, "sprites, {ctx}");
        sprites += want.len();
        clipped += usize::from(want.iter().any(|s| s.4 != 0x80 || s.5 != 0x80));
    }
    assert!(
        sprites > 3000 && clipped > 0 && dropped > 0,
        "sprites {sprites} clipped {clipped} dropped {dropped}"
    );
}
