//! PROT 0898's cast colour wash `FUN_801F2410` against
//! `legaia_render_kernels::cast_colour_wash`: its scroll step over every mode
//! (`step_scroll`), and the band mode's `POLY_G4` chain (`band_prims`),
//! packet by packet in pool order - corners, the four corner colours, the
//! code byte and the far ordering-table bucket each links into - and the
//! scanline modes' `POLY_FT4` strips (`scanline_prims`), line by line.

use crate::common::{Rng, disc, scus_cpu};
use legaia_render_kernels::cast_colour_wash::{
    CastColourWash, SCANLINE_CLUT, SCANLINE_TPAGE, WASH_OT, band_prims, scanline_prims,
};
use legaia_render_kernels::screen_prim::ScreenPrim;
use psx_retail_exec::Stop;

const BASE: u32 = 0x801C_E818;
const WASH: u32 = 0x801F_2410;
const CTX_PTR: u32 = 0x8007_BD24;
const CTX: u32 = 0x8010_0000;
const POOL_PTR: u32 = 0x1F80_03A0;
const OT_PTR: u32 = 0x1F80_03F4;
const POOL: u32 = 0x8011_0000;
const OT: u32 = 0x8014_0000;
const PACKET: u32 = 0x24;

#[test]
fn the_cast_colour_wash_matches_the_overlay() {
    differential!(prot(898, WASH));
    let Some(mut cpu) = scus_cpu() else {
        return;
    };
    cpu.mem.load(
        BASE,
        &disc().expect("disc").read_entry(898).expect("PROT 0898"),
    );
    cpu.mem.wr32(CTX_PTR, CTX);
    let mut rng = Rng(0x5741_5348);
    let mut drawn = 0usize;
    for case in 0..3000 {
        let banded = case % 4 != 0;
        let mut w = CastColourWash {
            alpha: if rng.below(8) == 0 {
                0xFF
            } else {
                1 + rng.below(255) as u8
            },
            amplitude: rng.next() as u8,
            // A band mode with no height never leaves retail's loop.
            height: if banded {
                1 + rng.below(0x80) as u8
            } else {
                rng.below(0x40) as u8
            },
            mode: if banded { 0 } else { 3 + rng.below(250) as u8 },
            colour_a: std::array::from_fn(|_| rng.next() as u8),
            colour_b: std::array::from_fn(|_| rng.next() as u8),
            pulse: 0,
            u_base: 0,
            v_base: 0,
            scroll: 0,
            scroll_rate: rng.below(0x40) as i16 - 0x20,
        };
        // A module keeps the scroll inside a band or two; one far outside
        // would chain thousands of bands, in retail as in the port.
        let h = i32::from(w.height.max(1));
        w.scroll = (rng.below(3 * h as u32) as i32 - h) as i16;
        // A scroll a whole frame below the bands: the loop draws nothing.
        if banded && case % 16 == 1 {
            w.scroll = -0x200 - rng.below(0x100) as i16;
        }
        let step = 1 + rng.below(4) as u8;
        cpu.mem.load(CTX + 0x27A, &[0; 0x10]);
        for f in [
            0x27A, 0x27B, 0x27C, 0x27D, 0x27E, 0x27F, 0x280, 0x281, 0x282, 0x283,
        ] {
            cpu.mem.wr8(CTX + f, w.load(f as u16) as u8);
        }
        cpu.mem.wr16(CTX + 0x32A, w.scroll as u16);
        cpu.mem.wr16(CTX + 0x32C, w.scroll_rate as u16);
        cpu.mem.wr8(0x1F80_0393, step);
        cpu.mem.wr32(POOL_PTR, POOL);
        cpu.mem.wr32(OT_PTR, OT);
        cpu.mem.wr32(OT + 0x3FFC, 0x00FF_FFFF);
        assert_eq!(cpu.call(WASH, &[]), Stop::Returned, "case {case}");

        w.step_scroll(step);
        assert_eq!(
            cpu.mem.rd16(CTX + 0x32A) as i16,
            w.scroll,
            "case {case}: scroll"
        );
        let prims = band_prims(&w);
        let used = cpu.mem.rd32(POOL_PTR) - POOL;
        assert_eq!(used, PACKET * prims.len() as u32, "case {case}: packets");
        for (k, p) in prims.iter().enumerate() {
            let ScreenPrim::Flat(q) = p else {
                panic!("a band is a flat quad")
            };
            let at = POOL + PACKET * k as u32;
            assert_eq!(cpu.mem.rd8(at + 7), 0x38, "case {case} packet {k}: POLY_G4");
            assert_eq!(q.ot_index, WASH_OT);
            let colours = q.gouraud.expect("a gradient");
            for v in 0..4u32 {
                let rgb = cpu.mem.read(at + 4 + 8 * v, 3);
                let xy = (
                    cpu.mem.rd16(at + 8 + 8 * v) as i16,
                    cpu.mem.rd16(at + 10 + 8 * v) as i16,
                );
                assert_eq!(
                    rgb,
                    colours[v as usize][..3],
                    "case {case} packet {k} corner {v}"
                );
                assert_eq!(xy, q.xy[v as usize], "case {case} packet {k} corner {v}");
            }
        }
        // The chain hangs off the last bucket, newest first.
        if let Some(n) = prims.len().checked_sub(1) {
            assert_eq!(
                cpu.mem.rd32(OT + 0x3FFC) & 0x00FF_FFFF,
                (POOL + PACKET * n as u32) & 0x00FF_FFFF
            );
        }
        drawn += prims.len();
    }
    assert!(drawn > 10_000, "{drawn}");
}

const STRIP: u32 = 0x28;

/// Modes `1` and `2` (`0x801F2958..0x801F2D20`): the wobble's two halves,
/// the row's two- and three-strip wraps, mode `2`'s mirrored second strip.
/// The per-line arrays `ctx[+0x32E]` / `ctx[+0x4EE]` stay zero, as no
/// routine on the disc stores into them; the scroll stays where a module's
/// non-negative ramp keeps it (a negative `m` would index ahead of the sine
/// table in retail).
#[test]
fn the_scanline_wash_matches_the_overlay() {
    differential!(prot(898, WASH));
    let Some(mut cpu) = scus_cpu() else {
        return;
    };
    cpu.mem.load(
        BASE,
        &disc().expect("disc").read_entry(898).expect("PROT 0898"),
    );
    cpu.mem.wr32(CTX_PTR, CTX);
    // The sine / cosine table pointers as `FUN_80026BE0` installs them.
    cpu.mem.wr32(0x8007_B81C, 0x8007_0A2C);
    cpu.mem.wr32(0x8007_B7F8, 0x8007_0A2C + 0x800);
    let mut rng = Rng(0x5343_414E);
    let (mut three, mut mirrored) = (0usize, 0usize);
    for case in 0..400 {
        let h = 1 + rng.below(0xFF) as u8;
        let mut w = CastColourWash {
            alpha: 1 + rng.below(255) as u8,
            amplitude: rng.next() as u8,
            height: h,
            mode: 1 + (case % 2) as u8,
            colour_a: std::array::from_fn(|_| rng.next() as u8),
            colour_b: std::array::from_fn(|_| rng.next() as u8),
            pulse: 0,
            u_base: rng.next() as u8,
            v_base: rng.next() as u8,
            scroll: rng.below(u32::from(h)) as i16,
            scroll_rate: 0,
        };
        let step = 1 + rng.below(4) as u8;
        let floor = -(i32::from(h) / i32::from(step));
        w.scroll_rate = (floor + rng.below((0x20 - floor) as u32) as i32) as i16;
        cpu.mem.load(CTX + 0x27A, &[0; 0x10]);
        cpu.mem.load(CTX + 0x32A, &[0; 0x1C4 + 0x1C0]);
        for f in 0x27A..=0x286u32 {
            cpu.mem.wr8(CTX + f, w.load(f as u16) as u8);
        }
        cpu.mem.wr16(CTX + 0x32A, w.scroll as u16);
        cpu.mem.wr16(CTX + 0x32C, w.scroll_rate as u16);
        cpu.mem.wr8(0x1F80_0393, step);
        cpu.mem.wr32(POOL_PTR, POOL);
        cpu.mem.wr32(OT_PTR, OT);
        cpu.mem.wr32(OT + 0x3FFC, 0x00FF_FFFF);
        assert_eq!(cpu.call(WASH, &[]), Stop::Returned, "case {case}");

        w.step_scroll(step);
        assert_eq!(cpu.mem.rd16(CTX + 0x32A) as i16, w.scroll, "case {case}");
        assert!(w.scroll >= 0, "case {case}: the scroll stays a module's");
        let prims = scanline_prims(&w);
        let used = cpu.mem.rd32(POOL_PTR) - POOL;
        assert_eq!(used, STRIP * prims.len() as u32, "case {case}: packets");
        three += prims.len() - 2 * 0xE0;
        for (k, p) in prims.iter().enumerate() {
            let ScreenPrim::Textured(q) = p else {
                panic!("a scanline strip is a textured quad")
            };
            let at = POOL + STRIP * k as u32;
            let tag = format!("case {case} strip {k}");
            assert_eq!(cpu.mem.rd8(at + 3), 9, "{tag}: length");
            assert_eq!(cpu.mem.rd8(at + 7), 0x2C, "{tag}: POLY_FT4");
            let rgb = cpu.mem.read(at + 4, 3);
            assert_eq!(
                rgb,
                [q.color as u8, (q.color >> 8) as u8, (q.color >> 16) as u8],
                "{tag}: colour"
            );
            assert_eq!(cpu.mem.rd16(at + 0xE), q.clut, "{tag}: clut");
            assert_eq!(cpu.mem.rd16(at + 0x16), q.tpage, "{tag}: tpage");
            assert_eq!((q.clut, q.tpage), (SCANLINE_CLUT, SCANLINE_TPAGE));
            assert_eq!(q.ot_index, WASH_OT);
            for v in 0..4u32 {
                let xy = (
                    cpu.mem.rd16(at + 8 + 8 * v) as i16,
                    cpu.mem.rd16(at + 10 + 8 * v) as i16,
                );
                let uv = (cpu.mem.rd8(at + 0xC + 8 * v), cpu.mem.rd8(at + 0xD + 8 * v));
                assert_eq!(xy, q.xy[v as usize], "{tag} corner {v}: xy");
                assert_eq!(uv, q.uv[v as usize], "{tag} corner {v}: uv");
            }
            if w.mode == 2 && q.uv[1].0 == 0 && q.uv[0].0 != 0 {
                mirrored += 1;
            }
        }
        if let Some(n) = prims.len().checked_sub(1) {
            assert_eq!(
                cpu.mem.rd32(OT + 0x3FFC) & 0x00FF_FFFF,
                (POOL + STRIP * n as u32) & 0x00FF_FFFF
            );
        }
    }
    assert!(three > 1000 && mirrored > 1000, "{three} {mirrored}");
}
