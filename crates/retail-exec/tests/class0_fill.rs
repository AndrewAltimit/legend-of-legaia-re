//! The class-0 window fill `FUN_8002BDC4` against
//! `legaia_engine_ui::battle_hud_chrome::class0_fill_quads`: random pens,
//! sizes and fill tiles, every `POLY_GT4` the routine writes held to the
//! port's quad - corners, the four vertex greys, texels, CLUT, page and the
//! semi-transparency bit.

use crate::common::{Rng, STACK_TOP, scus_cpu};
use legaia_engine_ui::battle_hud_chrome::class0_fill_quads;
use psx_retail_exec::Stop;

const FILL: u32 = 0x8002_BDC4;
const POOL_PTR: u32 = 0x1F80_03A0;
const OT_PTR: u32 = 0x1F80_03F4;
const POOL: u32 = 0x8012_0000;
const OT: u32 = 0x8014_0000;
const TILE: u32 = 0x8011_0000;

#[test]
fn the_class0_fill_matches_retail() {
    differential!(scus(FILL));
    let Some(mut cpu) = scus_cpu() else {
        return;
    };
    let mut rng = Rng(0x434C_3046);
    let mut many_bands = 0;
    for case in 0..3000 {
        let (x, y) = (rng.below(320) as i32 - 16, rng.below(240) as i32 - 16);
        let (w, h) = (1 + rng.below(300) as i32, 1 + rng.below(220) as i32);
        let (tu, tv) = (rng.below(0x80) as u8, rng.below(0x80) as u8);
        let (tw, th) = (8 + rng.below(25) as u8, 8 + rng.below(25) as u8);
        let flags = rng.below(0x100);
        cpu.mem.load(TILE, &[tu, tv, tw, th]);
        cpu.mem.wr32(POOL_PTR, POOL);
        cpu.mem.wr32(OT_PTR, OT);
        cpu.mem.wr32(OT + 4, 0x00FF_FFFF);
        cpu.mem.wr32(STACK_TOP + 0x10, w as u32);
        cpu.mem.wr32(STACK_TOP + 0x14, h as u32);
        assert_eq!(
            cpu.call(FILL, &[x as u32, y as u32, TILE, flags]),
            Stop::Returned,
            "case {case}"
        );
        let quads = class0_fill_quads((x - 4, y - 4, w + 8, h + 8), (i32::from(tw), i32::from(th)));
        many_bands += usize::from(quads.iter().any(|q| q.y > y + 28));
        let end = cpu.mem.rd32(POOL_PTR);
        assert_eq!(
            end - POOL,
            0x34 * quads.len() as u32,
            "case {case}: packets"
        );
        for (i, q) in quads.iter().enumerate() {
            let p = POOL + 0x34 * i as u32;
            let m = &mut cpu.mem;
            let code = if flags & 0x80 != 0 { 0x3E } else { 0x3C };
            assert_eq!(m.rd8(p + 3), 0x0C, "case {case} quad {i}: twelve words");
            assert_eq!(m.rd8(p + 7), code, "case {case} quad {i}: code");
            assert_eq!(m.rd16(p + 0xE), 0x7FC0 + (flags as u16 & 0x7F), "clut");
            assert_eq!(m.rd16(p + 0x1A), 0x1E, "case {case} quad {i}: page");
            let corners = [
                (q.x, q.y, 0, 0, q.top),
                (q.x + q.w, q.y, q.w, 0, q.top),
                (q.x, q.y + q.h, 0, q.h, q.bottom),
                (q.x + q.w, q.y + q.h, q.w, q.h, q.bottom),
            ];
            for (v, (cx, cy, du, dv, grey)) in corners.into_iter().enumerate() {
                let o = p + 4 + 12 * v as u32;
                let rgb = [m.rd8(o), m.rd8(o + 1), m.rd8(o + 2)];
                let xy = (m.rd16(o + 4) as i16, m.rd16(o + 6) as i16);
                let uv = (m.rd8(o + 8), m.rd8(o + 9));
                let want = (
                    [grey as u8; 3],
                    (cx as i16, cy as i16),
                    (tu.wrapping_add(du as u8), tv.wrapping_add(dv as u8)),
                );
                assert_eq!((rgb, xy, uv), want, "case {case} quad {i} vertex {v}");
            }
        }
    }
    assert!(many_bands > 500, "multi-band frames ran {many_bands} times");
}
