//! The spin-up style's expanding ring.

use super::*;
use crate::gte::{GteMat3, GteVec3, avsz4_with_scale, psx_cos, psx_sin};
use crate::screen_prim::{FlatQuad, ScreenPrim};

// ---------------------------------------------------------------------------
// The spin-up style's expanding ring (FUN_801D1CFC)
// ---------------------------------------------------------------------------

/// Ring phase per clock frame: `FUN_801D0370`'s tail calls
/// `FUN_801D1CFC(elapsed * 0xA0)` with the pre-increment clock.
pub const SPINUP_RING_PHASE_STEP: i32 = 0xA0;
/// The ring draws while its phase is `1..=0x1000`
/// (`param_1 - 1 <u 0x1000`), i.e. for clock frames 1 through 25.
pub const SPINUP_RING_PHASE_MAX: i32 = 0x1000;
/// Segments around the ring (`FUN_80028158(dest, 0, 0x60, params)`).
pub const SPINUP_RING_SEGMENTS: usize = 0x60;
/// The mesh params' x/y scale pair (`params +0x20/+0x22 = 0xE00`).
pub const SPINUP_RING_SCALE: i32 = 0xE00;
/// The outer rim's z depth (`params +0x1E = 0xC8`); the inner rim sits at 0.
pub const SPINUP_RING_DEPTH: i16 = 0xC8;
/// The view translation the tail stages before the call
/// (`0x1F800348..0x350 = (0, 0, 0x800)`).
pub const SPINUP_RING_VIEW_Z: i32 = 0x800;
/// The ring's flat colour (`params +0x08 = 0x303030`).
pub const SPINUP_RING_RGB: u32 = 0x0030_3030;

/// The expanding shockwave ring `FUN_801D0370` draws behind its confetti:
/// `FUN_801D1CFC(phase)`, `phase = clock * 0xA0`.
///
/// The tail builds a procedural annulus via `FUN_80028158(scratch, 0, 0x60,
/// params)` - the shared SCUS multi-shape mesh generator - and dispatches it
/// through `FUN_80043390` with flag word `0x89000000` (bit 27: double-sided)
/// and `a2 = phase` (non-zero: the depth-cue alpha bank, ambient staged from
/// the flag word's zero RGB). The generator's case-0 arm with these params
/// makes a 96-column cone band: inner rim at radius `phase` and z `0`, outer
/// rim at radius `phase + 2` and z [`SPINUP_RING_DEPTH`], both scaled by
/// `0xE00 / 0x1000`, one quad per column pair, colour `0x303030`.
///
/// Two halves of this are modelled rather than instruction-ported, and both
/// are recorded here so the boundary is inspectable: the exact vertex walk of
/// `FUN_80028158` (1395 instructions; only its case-0 parameters and lane
/// assignment are taken - x from the `_DAT_8007B7F8` lane, y from
/// `_DAT_8007B81C`, per-column angle step `0x1000 / 96` with a half-step
/// offset and the generator's `-0x400` quarter-turn bias), and the fog bank's
/// blend (drawn here as the half-blend bank's `B/2 + F/2` with the colour
/// faded toward the staged black ambient as the phase grows - the ring
/// self-extinguishes as it expands, matching `a2` rising to `0x1000`).
/// The generator itself is instruction-ported as
/// `legaia_engine_effects::effect_default_arm`, which this leaf crate cannot
/// reach; its shape-`0` ring over these params is the band modelled here.
///
/// PORT: FUN_801D1CFC
/// REF: FUN_80028158 (case-0 annulus parameters only - see above)
pub fn emit_spinup_ring(phase: i32, prims: &mut Vec<ScreenPrim>) -> bool {
    if !(1..=SPINUP_RING_PHASE_MAX).contains(&phase) {
        return false;
    }
    let (r_in, r_out) = (phase, phase + 2);
    let seg_step = 0x1000 / SPINUP_RING_SEGMENTS as i32; // 0x2A
    let half_step = seg_step >> 1; // 0x15
    let tr = GteVec3::new(0, 0, SPINUP_RING_VIEW_Z);
    let vert = |k: usize, r: i32, z: i16| -> ProjCorner {
        let a = ((k as i32 % SPINUP_RING_SEGMENTS as i32) * seg_step - half_step - 0x400) & 0xFFF;
        let x = (((psx_cos(a as u16) * SPINUP_RING_SCALE) >> 12) * r) >> 12;
        let y = (((psx_sin(a as u16) * SPINUP_RING_SCALE) >> 12) * r) >> 12;
        project_intro_corner(&GteMat3::IDENTITY, tr, (x as i16, y as i16, z))
    };
    // The depth-cue fade toward the staged black ambient: `a2` is the phase,
    // full at 0x1000.
    let level = (((SPINUP_RING_RGB & 0xFF) as i32 * (0x1000 - phase)) >> 12) as u8;
    let mut drawn = false;
    for k in 0..SPINUP_RING_SEGMENTS {
        let pc = [
            vert(k, r_in, 0),
            vert(k, r_out, SPINUP_RING_DEPTH),
            vert(k + 1, r_in, 0),
            vert(k + 1, r_out, SPINUP_RING_DEPTH),
        ];
        let otz = avsz4_with_scale(pc[0].sz, pc[1].sz, pc[2].sz, pc[3].sz, INTRO_ZSF4);
        if otz < INTRO_NEAR_OTZ {
            continue;
        }
        prims.push(ScreenPrim::Flat(FlatQuad {
            xy: std::array::from_fn(|i| (pc[i].xy.x as i16, pc[i].xy.y as i16)),
            color: [level, level, level, 0xFF],
            gouraud: None,
            semi_transparent: true,
            abr_mode: 0,
            ot_index: otz as u32,
            depth: None,
        }));
        drawn = true;
    }
    drawn
}
