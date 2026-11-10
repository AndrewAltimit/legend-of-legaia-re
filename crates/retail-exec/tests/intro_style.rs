//! The field-to-battle transition's style pick and quad builder off PROT
//! 0979 (base `0x801CE818`):
//!
//! - the init `FUN_801CE8CC` run up to the end of its style-selection block
//!   (the pad handler it calls next stops the run), over every battle-flag
//!   bit 7, every first-monster id and the scenes the overrides key on,
//!   against `select_intro_style`;
//! - the `POLY_GT4` builder `FUN_801CF1B0` over generated descriptors and
//!   arguments, against `build_intro_quad`: the whole packet, the ordering
//!   table link and the OT-depth reset.

use crate::common::{Rng, diff_offsets, disc, scus_cpu};
use legaia_render_kernels::battle_intro_styles::{IntroStyleInputs, select_intro_style};
use legaia_render_kernels::battle_intro_transition::{
    INTRO_QUAD_DEFAULT_OT_DEPTH, INTRO_QUAD_DESC_STRIDE, IntroQuadAnchor, IntroQuadDesc,
    IntroQuadRequest, build_intro_quad,
};
use psx_retail_exec::{Cpu, RA, RETURN_SENTINEL, Stop};

const BASE: u32 = 0x801C_E818;
const INIT: u32 = 0x801C_E8CC;
const QUAD: u32 = 0x801C_F1B0;
/// The pad handler the init calls right after the selection block.
const PAD_HANDLER: u32 = 0x8001_822C;
const STYLE: u32 = 0x801D_2460;
const SUB_STYLE: u32 = 0x801D_2464;
const OT_DEPTH: u32 = 0x801D_245C;
const TABLE: u32 = 0x801D_1EC4;
const PRIM_PTR: u32 = 0x1F80_03A0;
const OT_PTR: u32 = 0x1F80_03F4;
const PRIMS: u32 = 0x8010_0000;
const OT: u32 = 0x8011_0000;
const STACK_TOP: u32 = 0x801F_FF00;

fn intro_cpu() -> Option<Cpu> {
    let mut cpu = scus_cpu()?;
    cpu.mem.load(BASE, &disc()?.read_entry(979).ok()?);
    Some(cpu)
}

#[test]
fn the_intro_style_pick_matches_the_overlay() {
    differential!(prot(979, INIT));
    let Some(mut cpu) = intro_cpu() else {
        return;
    };
    for stub in [0x8005_67A8, 0x8001_DAF8, 0x8001_DCF8, 0x8006_53C8] {
        cpu.hook(stub, |_, _| {});
    }
    // Stop the run where the selection block ends.
    cpu.hook(PAD_HANDLER, |r, _| r.r[RA] = RETURN_SENTINEL);
    let mut seen = [0usize; 5];
    let mut rng = Rng(0x5354_594C);
    for slot0 in 0..=255u8 {
        for flags in [0u8, 0x80, 0x7F, 0xFF] {
            for scene in [3u32, 0x0C, 0x15, 0x9B, 0, 0x9A, rng.next()] {
                cpu.mem.wr8(0x8007_BD60, flags);
                cpu.mem.wr8(0x8007_BD0C, slot0);
                cpu.mem.wr32(0x8008_4540, scene);
                cpu.mem.wr32(STYLE, 0xEEEE);
                cpu.mem.wr32(SUB_STYLE, 0xEEEE);
                assert_eq!(cpu.call(INIT, &[]), Stop::Returned);
                let port = select_intro_style(&IntroStyleInputs {
                    battle_flags: flags,
                    formation_slot0: slot0,
                    scene_index: scene,
                });
                let at = format!("slot0 {slot0:#x} flags {flags:#x} scene {scene:#x}");
                assert_eq!(cpu.mem.rd32(STYLE) as i32, port.style.selector(), "{at}");
                assert_eq!(cpu.mem.rd32(SUB_STYLE) as i32, port.sub_style, "{at}");
                seen[port.style.selector() as usize] += 1;
            }
        }
    }
    assert!(seen.iter().all(|&n| n > 0), "{seen:?}");
}

#[test]
fn the_intro_quad_builder_matches_the_overlay() {
    differential!(prot(979, QUAD));
    let Some(mut cpu) = intro_cpu() else {
        return;
    };
    let mut rng = Rng(0x5155_4144);
    let mut modes = [0usize; 3];
    for case in 0..5000 {
        let records: Vec<u8> = (0..8 * INTRO_QUAD_DESC_STRIDE)
            .map(|_| rng.next() as u8)
            .collect();
        let mut records = records;
        for r in records.chunks_mut(INTRO_QUAD_DESC_STRIDE) {
            // Sizes a caller uses: q12 near one, now and then negative.
            let size = rng.below(0x3000) as i32 - if rng.below(8) == 0 { 0x1800 } else { 0 };
            r[..4].copy_from_slice(&size.to_le_bytes());
        }
        cpu.mem.load(TABLE, &records);
        let table: Vec<IntroQuadDesc> = records
            .chunks(INTRO_QUAD_DESC_STRIDE)
            .map(|r| IntroQuadDesc::parse(r).unwrap())
            .collect();
        let mode = rng.below(3) as i32;
        let index = rng.below(8) as i32;
        let key = if rng.below(10) == 0 {
            -(index + (mode << 10))
        } else {
            index + (mode << 10)
        };
        let req = IntroQuadRequest {
            anchor: if rng.below(2) == 0 {
                IntroQuadAnchor::Centre
            } else {
                IntroQuadAnchor::TopLeft
            },
            x: rng.next() as i16 >> 4,
            y: rng.next() as i16 >> 4,
            key,
            intensity: rng.below(0x200) as i32 - if rng.below(10) == 0 { 0x100 } else { 0 },
            scale_x: rng.below(0x2000) as i32 - 0x400,
            scale_y: rng.below(0x2000) as i32 - 0x400,
            ot_depth: rng.below(0x20),
        };
        let Some(q) = build_intro_quad(&req, &table) else {
            continue;
        };
        modes[(key.unsigned_abs() >> 10).min(2) as usize] += 1;
        cpu.mem.load(PRIMS, &[0xEE; 0x40]);
        cpu.mem.load(OT, &[0; 0x80]);
        for i in 0..0x20 {
            cpu.mem.wr32(OT + 4 * i, 0x00FF_0000 | i);
        }
        cpu.mem.wr32(PRIM_PTR, PRIMS);
        cpu.mem.wr32(OT_PTR, OT);
        cpu.mem.wr32(OT_DEPTH, req.ot_depth);
        cpu.mem.wr32(STACK_TOP + 0x10, req.intensity as u32);
        cpu.mem.wr32(STACK_TOP + 0x14, req.scale_x as u32);
        cpu.mem.wr32(STACK_TOP + 0x18, req.scale_y as u32);
        let anchor = u32::from(req.anchor == IntroQuadAnchor::TopLeft) * (1 + rng.below(4));
        assert_eq!(
            cpu.call(QUAD, &[anchor, req.x as u32, req.y as u32, key as u32]),
            Stop::Returned,
            "case {case}"
        );
        // The packet the port's quad lays out, over the sentinel.
        let mut want = vec![0xEEu8; 0x40];
        let ot_slot = OT + 4 * req.ot_depth;
        let link = 0x0C00_0000 | (0x00FF_0000 | req.ot_depth) & 0x00FF_FFFF;
        want[0..4].copy_from_slice(&link.to_le_bytes());
        for (i, v) in q.verts.iter().enumerate() {
            let at = 4 + 12 * i;
            want[at..at + 3].copy_from_slice(&v.rgb);
            want[at + 4..at + 6].copy_from_slice(&v.x.to_le_bytes());
            want[at + 6..at + 8].copy_from_slice(&v.y.to_le_bytes());
            want[at + 8] = v.u;
            want[at + 9] = v.v;
        }
        want[7] = q.code;
        want[0x0E..0x10].copy_from_slice(&q.clut.to_le_bytes());
        want[0x1A..0x1C].copy_from_slice(&q.tpage.to_le_bytes());
        let retail = cpu.mem.read(PRIMS, 0x40);
        assert!(
            retail == want,
            "case {case} {req:?}: {:?}",
            diff_offsets(&retail, &want)
        );
        assert_eq!(
            cpu.mem.rd32(PRIM_PTR),
            PRIMS + 0x34,
            "case {case}: the pool advance"
        );
        assert_eq!(cpu.mem.rd32(ot_slot) & 0x00FF_FFFF, PRIMS & 0x00FF_FFFF);
        assert_eq!(q.ot_depth, req.ot_depth);
        assert_eq!(cpu.mem.rd32(OT_DEPTH), INTRO_QUAD_DEFAULT_OT_DEPTH);
    }
    assert!(modes.iter().all(|&n| n > 500), "{modes:?}");
}
