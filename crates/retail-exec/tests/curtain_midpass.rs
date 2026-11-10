//! The curtain style's mid-pass emitter `FUN_801D1D9C` (PROT 0979) against
//! the constants `legaia_render_kernels::battle_intro` decays the curtain's
//! intermediate by: called as `FUN_801D11D0` calls it
//! (`CURTAIN_MIDPASS_OT`, `CURTAIN_MIDPASS_ABR`, `CURTAIN_MIDPASS_RGB`) with
//! the transition's 320x240 draw size, and over random OT layers. The
//! `POLY_F4` it writes is a semi-transparent flat quad over exactly
//! `FIELD_CAPTURE_ROWS` (once its `-4` top is clipped by the draw area),
//! its colour a `CURTAIN_MIDPASS_DECAY_5` step per 5-bit channel, and the
//! draw-mode packet linked ahead of it selects ABR `CURTAIN_MIDPASS_ABR`;
//! both link at the OT layer, clamped to the table.

use crate::common::{Rng, disc, scus_cpu};
use legaia_render_kernels::battle_intro::{
    CURTAIN_MIDPASS_ABR, CURTAIN_MIDPASS_DECAY_5, CURTAIN_MIDPASS_OT, CURTAIN_MIDPASS_RGB,
};
use legaia_render_kernels::vram_capture::FIELD_CAPTURE_ROWS;
use psx_retail_exec::Stop;

const OVERLAY_BASE: u32 = 0x801C_E818;
const MIDPASS: u32 = 0x801D_1D9C;
const POOL_PTR: u32 = 0x1F80_03A0;
const OT_PTR: u32 = 0x1F80_03F4;
const OT_LEN: u32 = 0x1F80_03A6;
const DRAW_W: u32 = 0x1F80_038C;
const DRAW_H: u32 = 0x1F80_038E;
const POOL: u32 = 0x8012_0000;
const OT: u32 = 0x8014_0000;
const OT_END: u32 = 0x00FF_FFFF;

#[test]
fn the_curtain_midpass_emits_what_the_port_decays_by() {
    differential!(prot(979, MIDPASS));
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &d.read_entry(979).expect("PROT 0979"));
    let mut rng = Rng(0x4D49_4450);
    for case in 0..500 {
        let len = 0x200u32;
        let layer = if case == 0 {
            CURTAIN_MIDPASS_OT as i32
        } else {
            rng.below(0x300) as i32 - 0x40
        };
        cpu.mem.wr32(POOL_PTR, POOL);
        cpu.mem.wr32(OT_PTR, OT);
        cpu.mem.wr16(OT_LEN, len as u16);
        cpu.mem.wr16(DRAW_W, 320);
        cpu.mem.wr16(DRAW_H, 240);
        for i in 0..len {
            cpu.mem.wr32(OT + 4 * i, OT_END);
        }
        let args = [
            layer as u32,
            u32::from(CURTAIN_MIDPASS_ABR),
            CURTAIN_MIDPASS_RGB,
        ];
        assert_eq!(cpu.call(MIDPASS, &args), Stop::Returned, "case {case}");
        let m = &mut cpu.mem;
        // The quad.
        let quad = POOL;
        assert_eq!(m.rd8(quad + 3), 5, "a POLY_F4 is five words");
        assert_eq!(m.rd8(quad + 7), 0x2B, "flat, four-point, semi-transparent");
        let rgb = m.rd32(quad + 4) & 0xFF_FFFF;
        assert_eq!(rgb, CURTAIN_MIDPASS_RGB);
        for k in 0..3 {
            assert_eq!(
                ((rgb >> (8 * k)) & 0xFF) as u16 >> 3,
                CURTAIN_MIDPASS_DECAY_5
            );
        }
        let xy: Vec<(i16, i16)> = (0..4)
            .map(|v| {
                (
                    m.rd16(quad + 8 + 4 * v) as i16,
                    m.rd16(quad + 0xA + 4 * v) as i16,
                )
            })
            .collect();
        let (x0, x1) = (
            xy.iter().map(|p| p.0).min().unwrap(),
            xy.iter().map(|p| p.0).max().unwrap(),
        );
        let (y0, y1) = (
            xy.iter().map(|p| p.1).min().unwrap(),
            xy.iter().map(|p| p.1).max().unwrap(),
        );
        let r = FIELD_CAPTURE_ROWS;
        assert_eq!(
            (x0, y0.max(0), x1 - x0, y1 - y0.max(0)),
            (r.x as i16, r.y as i16, r.w as i16, r.h as i16),
            "case {case}: the quad covers the intermediate"
        );
        // The draw-mode packet after it: E1 with the ABR bits.
        let mode = POOL + 0x18;
        let e1 = m.rd32(mode + 4);
        assert_eq!(e1 >> 24, 0xE1, "a draw-mode packet");
        assert_eq!(((e1 >> 5) & 3) as u8, CURTAIN_MIDPASS_ABR, "the blend");
        // Both on the clamped layer, the mode packet ahead of the quad.
        let at = layer.clamp(0, len as i32 - 1) as u32;
        assert_eq!(
            m.rd32(OT + 4 * at) & 0xFF_FFFF,
            mode & 0xFF_FFFF,
            "case {case}"
        );
        assert_eq!(m.rd32(mode) & 0xFF_FFFF, quad & 0xFF_FFFF, "case {case}");
    }
}
