//! The software clip's crossing-vertex interpolator `FUN_80029724` against
//! `interp_clip_vertex`: every flag combination (UV, RGB, trailing
//! neighbour), crossing fractions across the q12 unit and past it, and
//! random projected-vertex records. The 16-byte output slot is compared
//! whole, with the bytes the routine leaves alone.

use crate::common::{Rng, diff_offsets, scus_cpu};
use legaia_render_kernels::gte::raster::{CLIP_VERT_STRIDE, interp_clip_vertex};
use psx_retail_exec::Stop;

const INTERP: u32 = 0x8002_9724;
const VERTS: u32 = 0x8011_0000;
const OUT: u32 = 0x8012_0000;

#[test]
fn the_crossing_vertex_is_retails() {
    differential!(scus(INTERP));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x434C_4950);
    for n in 0..40_000 {
        let verts: Vec<u8> = (0..CLIP_VERT_STRIDE * 3)
            .map(|_| rng.next() as u8)
            .collect();
        let out: Vec<u8> = (0..16).map(|_| rng.next() as u8).collect();
        cpu.mem.load(VERTS, &verts);
        cpu.mem.load(OUT, &out);
        let flags =
            rng.below(4) | if rng.below(2) == 0 { 0x800 } else { 0 } | (rng.next() & 0xF000);
        let frac = match rng.below(4) {
            0 => rng.below(0x8000) as i32 - 0x4000,
            _ => rng.below(0x1001) as i32,
        };
        let cur = CLIP_VERT_STRIDE;
        assert_eq!(
            cpu.call(INTERP, &[OUT, VERTS + cur as u32, flags, frac as u32]),
            Stop::Returned
        );
        let retail = cpu.mem.read(OUT, 16);
        let mut port = out.clone();
        interp_clip_vertex(&mut port, &verts, cur, flags, frac);
        assert!(
            retail == port,
            "n {n} flags {flags:#x} frac {frac}: {:?}",
            diff_offsets(&retail, &port)
        );
    }
}
