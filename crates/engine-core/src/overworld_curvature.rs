//! The kingdom overworld's screen-Y curvature table - the `0x2000`-entry
//! `i16` table `FUN_800271A8` builds into the `_DAT_8007BB04` buffer.
//!
//! On the overworld retail pushes everything it draws down the screen by an
//! amount that grows with view depth, so the continent bends away over the
//! horizon. The table is indexed by `(SZ >> 5) + 1` (a halfword at
//! `base + (SZ >> 5) * 2 + 2`) and its entry is added to a vertex's `SY`. Two
//! consumers read it: the overworld mesh dispatch in `FUN_80043390` hands it
//! to the prim leaves at `0x800435E8..0x80043600` (scratchpad
//! `0x1F800314 - 0x2BC`, the world-map leaves' per-Z screen-Y term), and the
//! fog half-sheet emitter `FUN_8003F86C` adds it itself at
//! `0x8003F958..0x8003F9A0`. Both gate on the overworld bit
//! `_DAT_1F800394 & 1`, which `FUN_800271A8` sets.
//!
//! # How the table is built
//!
//! The MAN installer `FUN_8003AEB0` calls `FUN_800271A8(0x28, 0x2AB980)` when
//! the scene's overworld byte `_DAT_8007B6A8` is set (`0x8003AF90..9C`, the
//! second constant's low half in the call's delay slot). The routine:
//!
//! 1. fills the `_DAT_8007BB08` buffer with a quadratic drop,
//!    `ramp[k] = s0 >> 18` where `s0` starts at the second argument and grows
//!    by a running sum that grows by the first (`0x8002724C..0x80027268`);
//! 2. sets `H = 0x3C0` (`FUN_8003D254`, a `ctc2` to `H`) and loads the base
//!    matrix `_DAT_8007BF10` (`S * I`, zero translation) through
//!    `FUN_8003D1A4`;
//! 3. `RTPS`es `(0, ramp[j], 2000)` for `j = 3n / 2` and stores
//!    `SY - 0x78` per entry (`0x800272D0..0x80027338`). The store trails the
//!    transform by two iterations, so entries `0` and `1` hold the zero
//!    vector's `0`, and entry `i` holds the drop at `ramp[3 * (i - 2) / 2]`.
//!
//! With `H = 960` and both coordinates scaled by `S`, the drop is
//! `960 * ramp / 2000` pixels, saturated to the GTE's `SY` range
//! (`-1024..=1023` about `OFY = 120`). The running sum is a 32-bit register
//! and wraps, which only entries far past any reachable depth see.
//!
//! Pinned entry for entry against the `keikoku_chest_preload` capture by
//! `crates/engine-core/tests/fog_sheet_colour_retail_capture_disc.rs`.

use std::sync::OnceLock;

/// Entries in the table (`slti v0,a0,0x2000` at `0x80027330`).
pub const CURVATURE_ENTRIES: usize = 0x2000;
/// First argument `FUN_8003AEB0` passes: the ramp's second difference.
const RAMP_STEP: i32 = 0x28;
/// Second argument: the ramp's starting accumulator.
const RAMP_BASE: i32 = 0x2A_B980;
/// `H` the builder sets (`li a0,0x3c0`).
const BUILD_H: i32 = 0x3C0;
/// Depth of every transformed point (`li t0,0x7d0`).
const BUILD_Z: i32 = 0x7D0;
/// The screen centre the entries are measured from (`addiu v0,v0,-0x78`).
const BUILD_OFY: i32 = 0x78;

/// The table, built once.
///
/// PORT: FUN_800271A8
pub fn curvature_table() -> &'static [i16; CURVATURE_ENTRIES] {
    static TABLE: OnceLock<[i16; CURVATURE_ENTRIES]> = OnceLock::new();
    TABLE.get_or_init(|| {
        // 0x8002724C..0x80027268: the `0x4000`-entry drop.
        let mut ramp = [0i32; 0x4000];
        let (mut acc, mut step) = (RAMP_BASE, 0i32);
        for r in ramp.iter_mut() {
            *r = acc >> 18;
            step = step.wrapping_add(RAMP_STEP);
            acc = acc.wrapping_add(step);
        }
        let mut out = [0i16; CURVATURE_ENTRIES];
        for (i, o) in out.iter_mut().enumerate().skip(2) {
            let y = ramp[3 * (i - 2) / 2];
            // `S` scales numerator and depth alike; `SY` saturates.
            let sy = (BUILD_H * y).div_euclid(BUILD_Z);
            *o = (sy + BUILD_OFY).clamp(-1024, 1023) as i16 - BUILD_OFY as i16;
        }
        out
    })
}

/// The table entry for a view depth `sz` in retail's scaled space - the
/// halfword at `base + (sz >> 5) * 2 + 2` both consumers read.
pub fn curvature_at(sz: i32) -> i32 {
    let i = ((sz.max(0) >> 5) + 1) as usize;
    i32::from(curvature_table()[i.min(CURVATURE_ENTRIES - 1)])
}

/// Entry `i` of [`curvature_table`] in closed form - the law the hosts' mesh
/// shaders evaluate per vertex (`OVERWORLD_CURVE_WGSL` in `engine-render`,
/// `overworldCurve` in the play page's GLSL) instead of sampling the table.
///
/// The drop's accumulator after `k` steps is `RAMP_BASE + RAMP_STEP *
/// k (k + 1) / 2`, so entry `i >= 2` is `H * ((RAMP_BASE + 20 k (k + 1)) >>
/// 18) / Z` at `k = 3 (i - 2) / 2`, saturated at `SY = 1023`. Exact for every
/// index a saturated `SZ` can reach (`(0xFFFF >> 5) + 1`); the 32-bit
/// accumulator only wraps far past it.
pub fn curvature_closed_form(i: usize) -> i32 {
    if i < 2 {
        return 0;
    }
    let k = (3 * (i as i64 - 2)) / 2;
    let y = (i64::from(RAMP_BASE) + i64::from(RAMP_STEP) / 2 * k * (k + 1)) >> 18;
    let sy = i64::from(BUILD_H) * y / i64::from(BUILD_Z);
    ((sy + i64::from(BUILD_OFY)).min(1023) - i64::from(BUILD_OFY)) as i32
}

/// The last table index a saturated `SZ` reaches: `(0xFFFF >> 5) + 1`.
pub const CURVATURE_REACHABLE: usize = (0xFFFF >> 5) + 1;

/// How the hosts' mesh shaders turn a draw's `clip.w` into retail's `SZ`
/// for the curvature index, for this frame - `0.0` switches the bend off.
///
/// On while retail's overworld bit is up ([`crate::world::World::overworld_bit`])
/// and the frame is one of retail's own cameras:
///
/// - the overworld **walk** camera composes the 6x world scale into its
///   matrix (`camera_view::world_map_walk_vp`), so its `clip.w` already is
///   `SZ` - factor `1`;
/// - a **scripted** shot (the `map01` aerial fly-in) and the field follow
///   pose are `1x` frames, so `SZ` is `clip.w` times the base matrix's scale
///   ([`crate::camera_view::CUTSCENE_WORLD_SCALE`]).
///
/// The top-view debug camera and the host's own orbit have no retail eye to
/// bend about, and keep the flat projection.
pub fn frame_curve_scale(overworld: bool, frame: &crate::camera_view::FieldCameraFrame) -> f32 {
    use crate::camera_view::FieldCameraFrame as F;
    if !overworld {
        return 0.0;
    }
    match frame {
        F::WorldMapWalk { .. } => 1.0,
        F::Cutscene(_) | F::Follow(_) => crate::camera_view::CUTSCENE_WORLD_SCALE,
        F::WorldMapTopView { .. } | F::HostDebugOrbit => 0.0,
    }
}

/// The screen-Y bend, in PSX stage rows, the curvature adds to a vertex whose
/// clip-space `w` is `clip_w` under a frame with `scale` from
/// [`frame_curve_scale`] - the CPU twin of the shaders' per-vertex term, for
/// the screen-space draws that must land on the bent ground (the overworld
/// markers). `0` when `scale` is `0`.
pub fn clip_bend_rows(clip_w: f32, scale: f32) -> f32 {
    if scale <= 0.0 || clip_w <= 0.0 {
        return 0.0;
    }
    let sz = ((clip_w * scale).round() as i32).clamp(0, 0xFFFF);
    curvature_closed_form(((sz >> 5) + 1) as usize) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_starts_flat_and_bends_down_with_depth() {
        let t = curvature_table();
        assert_eq!((t[0], t[1]), (0, 0), "the trailing store's zero vector");
        assert_eq!(t[2], 4, "960 * (0x2AB980 >> 18) / 2000");
        assert!(t.windows(2).take(3000).all(|w| w[1] >= w[0]), "monotone");
        assert_eq!(t[3306], 903, "saturates at SY = 1023");
        assert_eq!(curvature_at(5849), i32::from(t[183]));
    }

    #[test]
    fn the_closed_form_is_the_table_over_every_reachable_index() {
        let t = curvature_table();
        for (i, &e) in t.iter().enumerate().take(CURVATURE_REACHABLE + 1) {
            assert_eq!(curvature_closed_form(i), i32::from(e), "entry {i}");
        }
    }

    #[test]
    fn the_bend_scale_follows_the_frame() {
        use crate::camera_view::FieldCameraFrame as F;
        let top = F::WorldMapTopView {
            azimuth: 0,
            zoom: 0,
            pan: [0, 0],
        };
        assert_eq!(frame_curve_scale(true, &top), 0.0);
        assert_eq!(frame_curve_scale(false, &F::HostDebugOrbit), 0.0);
        assert_eq!(
            clip_bend_rows(0x1000 as f32, 1.0),
            curvature_at(0x1000) as f32
        );
        assert_eq!(clip_bend_rows(100.0, 0.0), 0.0);
    }
}
