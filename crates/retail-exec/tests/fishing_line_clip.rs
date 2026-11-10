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
