//! The fishing line's 2-D clip `FUN_801D56E4` against `clip_segment_2d`:
//! random segments against the line's own rectangle and random ones, every
//! endpoint compared after the eight arms run.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_fishing::fishing_actors::{ClipRect, LINE_CLIP_RECT, clip_segment_2d};
use legaia_minigame_formats::fishing_species as fs;
use psx_retail_exec::Stop;

const CLIP: u32 = 0x801D_56E4;
/// The rectangle's four halfwords, `x_min, y_min, x_max, y_max`
/// (`0x1F800314 + 0x74..0x7A`).
const RECT: u32 = 0x1F80_0388;
const P: u32 = 0x8010_0000;
const Q: u32 = 0x8010_0008;

#[test]
fn the_line_is_clipped_as_retail_clips_it() {
    differential!(prot(972, CLIP));
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let overlay = d
        .read_entry(fs::FISHING_OVERLAY_PROT_INDEX)
        .expect("PROT 0972");
    cpu.mem.load(fs::FISHING_OVERLAY_BASE_VA, &overlay);
    let mut rng = Rng(0x434C_4950);
    let coord = |rng: &mut Rng| rng.below(0x700) as i16 - 0x300;
    let mut moved = 0u32;
    for case in 0..20_000 {
        let rect = if case % 2 == 0 {
            LINE_CLIP_RECT
        } else {
            let (a, b) = (rng.below(0x200) as i16, rng.below(0x200) as i16);
            let (c, e) = (rng.below(0x200) as i16, rng.below(0x200) as i16);
            ClipRect {
                x_min: a.min(b),
                y_min: c.min(e),
                x_max: a.max(b),
                y_max: c.max(e),
            }
        };
        let mut p = (coord(&mut rng), coord(&mut rng));
        let mut q = (coord(&mut rng), coord(&mut rng));
        for (i, v) in [rect.x_min, rect.y_min, rect.x_max, rect.y_max]
            .into_iter()
            .enumerate()
        {
            cpu.mem.wr16(RECT + 2 * i as u32, v as u16);
        }
        cpu.mem.wr16(P, p.0 as u16);
        cpu.mem.wr16(P + 2, p.1 as u16);
        cpu.mem.wr16(Q, q.0 as u16);
        cpu.mem.wr16(Q + 2, q.1 as u16);
        assert_eq!(cpu.call(CLIP, &[P, Q]), Stop::Returned, "case {case}");
        let (p0, q0) = (p, q);
        clip_segment_2d(&mut p, &mut q, rect);
        let rd = |cpu: &mut psx_retail_exec::Cpu, a: u32| {
            (cpu.mem.rd16(a) as i16, cpu.mem.rd16(a + 2) as i16)
        };
        let retail = (rd(&mut cpu, P), rd(&mut cpu, Q));
        assert_eq!(retail, (p, q), "case {case}: {p0:?}-{q0:?} in {rect:?}");
        moved += u32::from((p, q) != (p0, q0));
    }
    assert!(moved > 2_000, "only {moved} cases clipped anything");
}

/// The 3-D segment clip + projection `FUN_801D5C2C` (PROT 0972): its two view
/// transforms `FUN_8003D344` run live on the GTE with an identity rotation and
/// a generated translation, so the view-space endpoints the port takes are the
/// model points plus that translation.
const PROJECT: u32 = 0x801D_5C2C;
/// The near bound (`lhu 0x1F80037E`) and the projection distance (`lh 0x8007B6F4`).
const NEAR: u32 = 0x1F80_037E;
const PROJ: u32 = 0x8007_B6F4;
const SA: u32 = 0x8010_0010;
const SB: u32 = 0x8010_0018;

#[test]
fn the_segment_is_clipped_and_projected_as_retail_does() {
    use legaia_engine_fishing::fishing_actors::project_segment;
    differential!(prot(972, PROJECT));
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let overlay = d
        .read_entry(fs::FISHING_OVERLAY_PROT_INDEX)
        .expect("PROT 0972");
    cpu.mem.load(fs::FISHING_OVERLAY_BASE_VA, &overlay);
    // Identity rotation: RT11 / RT22 / RT33 = 1.0.
    for (r, v) in [(0, 0x1000), (1, 0), (2, 0x1000), (3, 0), (4, 0x1000)] {
        cpu.gte.write_ctrl(r, v);
    }
    let mut rng = Rng(0x5345_474D);
    let (mut rejected, mut clipped_a, mut clipped_b, mut whole) = (0u32, 0u32, 0u32, 0u32);
    for case in 0..20_000 {
        let small = |rng: &mut Rng| rng.below(0x6000) as i32 - 0x3000;
        let tr = [small(&mut rng), small(&mut rng), small(&mut rng)];
        let a = [small(&mut rng), small(&mut rng), small(&mut rng)];
        let b = [small(&mut rng), small(&mut rng), small(&mut rng)];
        let near = 1 + rng.below(0x800) as i32;
        let proj = 0x80 + rng.below(0x380) as i32;
        for (i, t) in tr.iter().enumerate() {
            cpu.gte.write_ctrl(5 + i, *t as u32);
        }
        cpu.mem.wr16(NEAR, near as u16);
        cpu.mem.wr16(PROJ, proj as u16);
        for (at, v) in [(P, a), (Q, b)] {
            for (i, c) in v.iter().enumerate() {
                cpu.mem.wr16(at + 2 * i as u32, *c as u16);
            }
            cpu.mem.wr16(at + 6, 0);
        }
        for at in [SA, SB] {
            cpu.mem.wr32(at, 0xDEAD_BEEF);
        }
        assert_eq!(
            cpu.call(PROJECT, &[P, Q, SA, SB]),
            Stop::Returned,
            "case {case}"
        );
        let va = [a[0] + tr[0], a[1] + tr[1], a[2] + tr[2]];
        let vb = [b[0] + tr[0], b[1] + tr[1], b[2] + tr[2]];
        let rd3 = |cpu: &mut psx_retail_exec::Cpu, at: u32| {
            [0, 2, 4].map(|o| cpu.mem.rd16(at + o) as i16)
        };
        let rd2 = |cpu: &mut psx_retail_exec::Cpu, at: u32| {
            (cpu.mem.rd16(at) as i16, cpu.mem.rd16(at + 2) as i16)
        };
        let (ra, rb) = (rd3(&mut cpu, P), rd3(&mut cpu, Q));
        let (sa, sb) = (rd2(&mut cpu, SA), rd2(&mut cpu, SB));
        match project_segment(va, vb, near, proj) {
            None => {
                assert_eq!((sa, sb), ((0, 0), (0, 0)), "case {case}: reject screens");
                let src = |v: [i32; 3]| v.map(|c| c as i16);
                assert_eq!(
                    (ra, rb),
                    (src(a), src(b)),
                    "case {case}: reject leaves the buffers"
                );
                rejected += 1;
            }
            Some(seg) => {
                assert_eq!([ra, rb], seg.view, "case {case}: {va:?} {vb:?} near {near}");
                assert_eq!([sa, sb], seg.screen, "case {case}: screens");
                if va[2] < near {
                    clipped_a += 1;
                } else if vb[2] < near {
                    clipped_b += 1;
                } else {
                    whole += 1;
                }
            }
        }
    }
    assert!(
        rejected > 1000 && clipped_a > 1000 && clipped_b > 1000 && whole > 1000,
        "arms: reject {rejected}, a {clipped_a}, b {clipped_b}, whole {whole}"
    );
}
