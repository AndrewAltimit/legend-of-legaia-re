//! The move-VM extension's scanline strip emitter `FUN_801D31B0` (field
//! overlay, PROT 0897) against `legaia_engine_vm::move_ext_strip::emit_strip`,
//! at packet level: every `POLY_FT4` it links, in chain order, field for
//! field, and the trailing `SetDrawMode` packet.
//!
//! The two projections (`FUN_8005BA38`, `FUN_800195A8`) are hooked to hand
//! back generated screen points, which both sides take; the trig tables are
//! generated tables behind the two retail pointers.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_vm::move_ext_strip::{
    STRIP_QUAD_CODE, StripClip, StripProjection, StripScroll, StripSlab, emit_strip,
};
use psx_retail_exec::{A0, A1, A3, SP, Stop, V0};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

const OVERLAY_BASE: u32 = 0x801C_E818;
const STRIP: u32 = 0x801D_31B0;
const POINT: u32 = 0x8005_BA38;
const BOX: u32 = 0x8001_95A8;
const DRAW_MODE: u32 = 0x8005_9010;

const COS_PTR: u32 = 0x8007_B7F8;
const SIN_PTR: u32 = 0x8007_B81C;
const OT_SHIFT: u32 = 0x1F80_03A4;
const CLIP: u32 = 0x1F80_0388;
const CURSOR: u32 = 0x1F80_03A0;
const OT_BASE: u32 = 0x1F80_03F4;

const ACTOR: u32 = 0x8012_0000;
const COS: u32 = 0x8011_0000;
const SIN: u32 = 0x8011_4000;
const PACKETS: u32 = 0x8014_0000;
const OT: u32 = 0x8011_8000;
const OT_LEN: u32 = 0x40;
const OT_END: u32 = 0x00FF_FFFF;

#[test]
fn the_strip_emitter_matches_retail_at_packet_level() {
    differential!(prot(897, STRIP));
    let Some(disc) = disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut cpu = scus_cpu().expect("disc present");
    cpu.mem
        .load(OVERLAY_BASE, &disc.read_entry(897).expect("PROT 0897"));
    let mut rng = Rng(0x5354_5250);
    let cos: Vec<i16> = (0..0x1000).map(|_| (rng.below(0x2001) as i32 - 0x1000) as i16).collect();
    let sin: Vec<i16> = (0..0x1000).map(|_| (rng.below(0x2001) as i32 - 0x1000) as i16).collect();
    for i in 0..0x1000u32 {
        cpu.mem.wr16(COS + 2 * i, cos[i as usize] as u16);
        cpu.mem.wr16(SIN + 2 * i, sin[i as usize] as u16);
    }
    cpu.mem.wr32(COS_PTR, COS);
    cpu.mem.wr32(SIN_PTR, SIN);
    cpu.mem.wr32(OT_BASE, OT);

    let proj = Rc::new(Cell::new(StripProjection::default()));
    let modes = Rc::new(RefCell::new(Vec::<(u32, u32)>::new()));
    let p = proj.clone();
    cpu.hook(POINT, move |r, m| {
        let pr = p.get();
        m.wr16(r.r[A1], pr.centre.0 as u16);
        m.wr16(r.r[A1] + 2, pr.centre.1 as u16);
        r.r[V0] = pr.otz;
    });
    let p = proj.clone();
    cpu.hook(BOX, move |r, m| {
        let pr = p.get();
        for (k, c) in pr.corners.iter().enumerate() {
            let at = m.rd32(r.r[SP] + 0x10 + 4 * k as u32);
            m.wr16(at, c.0 as u16);
            m.wr16(at + 2, c.1 as u16);
        }
    });
    let md = modes.clone();
    cpu.hook(DRAW_MODE, move |r, m| {
        md.borrow_mut().push((r.r[A0], r.r[A3]));
        m.wr32(r.r[A0] + 4, 0xE100_0000 | r.r[A3]);
    });

    let (mut quads_seen, mut wobbled, mut clipped, mut skipped) = (0usize, 0, 0, 0);
    for round in 0..3000 {
        let u0 = rng.below(0xC0) as u16;
        let v0 = rng.below(0xC0) as u16;
        let slab = StripSlab {
            u0,
            v0,
            u1: u0 + 3 + rng.below(0x3D) as u16,
            v1: v0 + rng.below(0x40) as u16,
            tpage: rng.next() as u16,
            clut: rng.next() as u16,
            half_w: rng.next() as i16,
            half_h: rng.next() as i16,
            wobble_amp: (rng.below(0x80) as i16) - 0x40,
            wobble_freq: if rng.below(2) == 0 { 0 } else { rng.below(0x100) as i16 - 0x80 },
        };
        let scroll = StripScroll {
            u_phase: rng.next() as u16,
            wobble_phase: rng.next() as i16,
            v_phase: rng.next() as u16,
        };
        let (sx, sy) = (rng.below(0x200) as i16 - 0x60, rng.below(0x180) as i16 - 0x40);
        let w = rng.below(0x80) as i16;
        let (top, bottom) = (sy - rng.below(0x50) as i16, sy + rng.below(0x50) as i16);
        let shift = rng.below(4) as u8 + 6;
        let pr = StripProjection {
            centre: (sx, sy),
            otz: if rng.below(10) == 0 {
                0
            } else {
                1 + rng.below((OT_LEN << shift) - 1)
            },
            corners: [
                (sx - w + rng.below(9) as i16 - 4, top),
                (sx + w + rng.below(9) as i16 - 4, top),
                (sx - w, bottom),
                (sx + w, bottom),
            ],
        };
        let clip = if rng.below(3) == 0 {
            StripClip {
                left: rng.below(0x60) as u16,
                top: rng.below(0x60) as u16,
                right: 0xA0 + rng.below(0xA0) as u16,
                bottom: 0x80 + rng.below(0x80) as u16,
            }
        } else {
            StripClip::default()
        };
        proj.set(pr);
        for (o, v) in [
            (0xA8u32, slab.u0),
            (0xAA, slab.v0),
            (0xAC, slab.u1),
            (0xAE, slab.v1),
            (0xB0, slab.tpage),
            (0xB2, slab.clut),
            (0xB4, slab.half_w as u16),
            (0xB6, slab.half_h as u16),
            (0xB8, slab.wobble_amp as u16),
            (0xBA, slab.wobble_freq as u16),
            (0x24, scroll.u_phase),
            (0x26, scroll.wobble_phase as u16),
            (0x28, scroll.v_phase),
        ] {
            cpu.mem.wr16(ACTOR + o, v);
        }
        for (i, v) in [clip.left, clip.top, clip.right, clip.bottom].iter().enumerate() {
            cpu.mem.wr16(CLIP + 2 * i as u32, *v);
        }
        cpu.mem.wr8(OT_SHIFT, shift);
        cpu.mem.wr32(CURSOR, PACKETS);
        for i in 0..OT_LEN {
            cpu.mem.wr32(OT + 4 * i, OT_END);
        }
        modes.borrow_mut().clear();
        assert_eq!(cpu.call(STRIP, &[ACTOR]), Stop::Returned, "round {round}");

        let want = emit_strip(&slab, &scroll, &pr, &clip, shift, |a| {
            (sin[usize::from(a)], cos[usize::from(a)])
        });
        let ctx = format!("round {round} {slab:?} {scroll:?} {pr:?} {clip:?} shift {shift}");
        let Some(want) = want else {
            assert_eq!(pr.otz, 0, "the port declined a slab retail drew: {ctx}");
            assert_eq!(cpu.mem.rd32(CURSOR), PACKETS, "{ctx}");
            assert!(modes.borrow().is_empty(), "{ctx}");
            skipped += 1;
            continue;
        };
        let n = want.quads.len() as u32;
        let mode_pkt = PACKETS + 0x28 * n;
        assert_eq!(cpu.mem.rd32(CURSOR), mode_pkt + 0xC, "cursor: {ctx}");
        assert_eq!(*modes.borrow(), [(mode_pkt, u32::from(want.mode_tpage))], "{ctx}");
        // The chain, newest first: the mode packet, then the quads backwards.
        let bucket = OT + 4 * want.ot_index;
        let mut chain = Vec::new();
        let mut link = cpu.mem.rd32(bucket) & 0xFF_FFFF;
        while link != OT_END {
            chain.push(0x8000_0000 | link);
            link = cpu.mem.rd32(0x8000_0000 | link) & 0xFF_FFFF;
            assert!(chain.len() <= n as usize + 1, "a loop: {ctx}");
        }
        let mut expect = vec![mode_pkt];
        expect.extend((0..n).rev().map(|i| PACKETS + 0x28 * i));
        assert_eq!(chain, expect, "chain: {ctx}");
        for (i, q) in want.quads.iter().enumerate() {
            let at = PACKETS + 0x28 * i as u32;
            assert_eq!(cpu.mem.rd32(at + 4), STRIP_QUAD_CODE, "quad {i}: {ctx}");
            let xy: Vec<(i16, i16)> = (0..4)
                .map(|v| {
                    let b = at + 8 + 8 * v;
                    (cpu.mem.rd16(b) as i16, cpu.mem.rd16(b + 2) as i16)
                })
                .collect();
            let uv: Vec<(u8, u8)> = (0..4)
                .map(|v| {
                    let b = at + 0xC + 8 * v;
                    (cpu.mem.rd8(b), cpu.mem.rd8(b + 1))
                })
                .collect();
            assert_eq!(xy, q.xy(), "quad {i} xy: {ctx}");
            assert_eq!(uv, q.uv(), "quad {i} uv: {ctx}");
            assert_eq!(cpu.mem.rd16(at + 0xE), q.clut, "quad {i} clut: {ctx}");
            assert_eq!(cpu.mem.rd16(at + 0x16), q.tpage, "quad {i} tpage: {ctx}");
        }
        quads_seen += want.quads.len();
        wobbled += u32::from(slab.wobble_freq != 0 && !want.quads.is_empty());
        clipped += u32::from(clip != StripClip::default() && !want.quads.is_empty());
    }
    assert!(
        quads_seen > 100_000 && wobbled > 800 && clipped > 300 && skipped > 150,
        "{quads_seen} {wobbled} {clipped} {skipped}"
    );
}
