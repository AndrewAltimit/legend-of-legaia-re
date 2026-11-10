//! PROT 0898's cast colour wash `FUN_801F2410` against
//! `legaia_render_kernels::cast_colour_wash`: its scroll step over every mode
//! (`step_scroll`), and the band mode's `POLY_G4` chain (`band_prims`),
//! packet by packet in pool order - corners, the four corner colours, the
//! code byte and the far ordering-table bucket each links into.

use crate::common::{Rng, disc, scus_cpu};
use legaia_render_kernels::cast_colour_wash::{CastColourWash, WASH_OT, band_prims};
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
