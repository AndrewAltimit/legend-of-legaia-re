//! Fishing-overlay (PROT 0972) **actor-side** kernels: the free-swim wander,
//! the pre-hook tick's camera seeding and debug readout, the bite roll and
//! its interval ladder, the catch-celebration tiers, and the overlay's own
//! 3-D segment clip + projection.
//!
//! Every routine here is an entry of the *fishing* overlay, confirmed by
//! disassembling PROT 0972 at slot-A base `0x801CE818`
//! (`scripts/ghidra-analysis/locate-entry-image.py` frames each one in 0972
//! and in no other based image). The dumps also exist under an
//! `overlay_debug_menu_` prefix; that prefix names the **capture**, whose
//! slot A held these bytes above PROT 0971's much shorter footprint - it is
//! not a claim that the code is dev-menu code. See
//! `docs/tooling/dump-corpus-integrity.md`.
//!
//! Companion prose: `docs/subsystems/minigame-fishing.md`.
//!
//! # NOT WIRED
//!
//! `crate::fishing` models the fishing minigame as *rules* - cast power,
//! reel tug-of-war, catch scoring - with no actors, no camera and no
//! ordering table. Everything in this module drives the retail overlay's
//! per-frame actor structs (`+0x14/+0x16/+0x18` position, `+0x22` phase,
//! `+0x26` facing) and the scene camera globals, none of which the engine's
//! fishing session owns. Wiring needs a fishing *scene* host - an actor pool
//! plus the venue geometry - which is the same prerequisite the venue mesh
//! work carries.

use crate::dev_menu::{PACK_LEFT, PACK_RIGHT};

// --- 3-D segment clip + projection (FUN_801D5C2C) --------------------------

/// Screen-space centre the projector biases both outputs by (`0xA0`, `0x78`).
pub const SCREEN_CENTRE: (i16, i16) = (0xA0, 0x78);

/// A segment that survived the near-plane reject.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProjectedSegment {
    /// The view-space endpoints after clipping, as retail writes them back
    /// through the caller's two `i16[3]` buffers.
    pub view: [[i16; 3]; 2],
    /// The projected screen positions of those endpoints.
    pub screen: [(i16, i16); 2],
}

/// Fixed-point lerp helper: `(delta * t) >> 12`, biased so a negative
/// product truncates toward zero exactly as the retail `addiu ,0xfff` does.
#[inline]
fn lerp12(delta: i32, t: i32) -> i32 {
    let p = delta.wrapping_mul(t);
    let p = if p < 0 { p.wrapping_add(0xFFF) } else { p };
    p >> 12
}

/// Clip a view-space segment against the near bound and project both ends.
///
/// `a` and `b` are the endpoints **already transformed into view space** (in
/// retail, by the GTE wrapper `FUN_8003D344`, one `MVMVA`). `near` is the
/// depth bound at scratchpad `0x1F80037E` and `proj` the projection distance
/// at `0x8007B6F4`.
///
/// Returns `None` for the whole-segment reject - when *both* endpoints are
/// nearer than `near`, retail zeroes the two screen outputs and leaves the
/// endpoint buffers untouched.
///
/// The two clip arms are **not symmetric**, and the port keeps the
/// asymmetry. The `a`-side arm solves for the near crossing correctly:
/// `t = ((b.z - near) << 12) / (a.z - b.z)`, then `a = b - t * (a - b)`.
/// The `b`-side arm reuses the same numerator against the opposite
/// denominator - `t = ((b.z - near) << 12) / (b.z - a.z)`, then
/// `b = a + t * (b - a)` - which is the *complement* of the parameter that
/// would put `b` on the near plane. Only `b.z` is then forced to `near`, so
/// the far endpoint's x/y slide by `1 - t` instead of `t`.
///
/// One deliberate deviation: retail reaches the R3000 divide-by-zero trap
/// when a denominator or a post-clip `z` is zero. The port returns `None`
/// for those instead of trapping, which is the same "nothing to draw"
/// outcome the reject path produces.
///
/// PORT: FUN_801d5c2c
pub fn project_segment(a: [i32; 3], b: [i32; 3], near: i32, proj: i32) -> Option<ProjectedSegment> {
    if a[2] < near && b[2] < near {
        return None;
    }
    let mut p = a;
    let mut q = b;

    if p[2] < near {
        let denom = a[2] - b[2];
        if denom == 0 {
            return None;
        }
        let t = ((b[2] - near) << 12) / denom;
        p[0] = b[0] - lerp12(a[0] - b[0], t);
        p[1] = b[1] - lerp12(a[1] - b[1], t);
        p[2] = near;
        q = b;
    }
    if q[2] < near {
        let denom = b[2] - a[2];
        if denom == 0 {
            return None;
        }
        let t = ((b[2] - near) << 12) / denom;
        p = a;
        q[0] = a[0] + lerp12(b[0] - a[0], t);
        q[1] = a[1] + lerp12(b[1] - a[1], t);
        q[2] = near;
    }

    let view = [
        [p[0] as i16, p[1] as i16, p[2] as i16],
        [q[0] as i16, q[1] as i16, q[2] as i16],
    ];
    let scale = proj << 12;
    let project = |v: [i16; 3]| -> (i16, i16) {
        let k = scale / (v[2] as i32);
        (
            (lerp12(v[0] as i32, k) + SCREEN_CENTRE.0 as i32) as i16,
            (lerp12(v[1] as i32, k) + SCREEN_CENTRE.1 as i32) as i16,
        )
    };
    if view[0][2] == 0 || view[1][2] == 0 {
        return None;
    }
    Some(ProjectedSegment {
        view,
        screen: [project(view[0]), project(view[1])],
    })
}

// --- Free-swim wander (FUN_801D2278) ---------------------------------------

/// Facing-angle step one held D-pad frame applies.
pub const FACING_STEP: i16 = 0x40;

/// Inclusive facing clamp the idle/cast state holds the fish inside.
pub const FACING_RANGE: (i16, i16) = (0x700, 0x900);

/// Scene-mode value (`DAT_801D926C`) in which the D-pad steers the fish.
pub const MODE_IDLE_CAST: i32 = 0x0C;

/// Re-target dwell floor, in frames.
pub const RETARGET_MIN: i32 = 0x78;

/// Re-target dwell span above the floor (`rand % 200`).
pub const RETARGET_SPAN: i32 = 200;

/// Per-step of the randomised destination offset, along Z and X.
pub const WANDER_STEP: (i32, i32) = (0x20, 0x50);

/// Fixed Z bias applied to every re-target destination.
pub const WANDER_Z_BIAS: i32 = 0x400;

/// One re-rolled wander destination.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WanderTarget {
    /// New dwell timer, `rand % 200 + 0x78`.
    pub dwell: i32,
    /// Destination X, `x + (3 - rand % 6) * 0x50`.
    pub x: i32,
    /// Destination Z, `z + 0x400 + (3 - rand % 6) * 0x20`.
    pub z: i32,
    /// Which of the two ripple effect descriptors the roll picked
    /// (`rand & 1`).
    pub ripple_variant: u32,
}

/// Roll a new wander destination from four consecutive `rand()` draws.
///
/// The retail order is: one discarded draw that only seeds the on-stack
/// rotation word, the dwell draw, the Z draw, the X draw, then the ripple
/// pick. `rolls` must supply them in that order.
///
/// PORT: FUN_801d2278 (re-target roll)
pub fn roll_wander_target<F: FnMut() -> u32>(x: i32, z: i32, mut rolls: F) -> WanderTarget {
    let _rotation = rolls() & 0xFFF;
    let dwell = (rolls() as i32) % RETARGET_SPAN + RETARGET_MIN;
    let dz = 3 - ((rolls() as i32) % 6);
    let dx = 3 - ((rolls() as i32) % 6);
    let ripple_variant = rolls() & 1;
    WanderTarget {
        dwell,
        x: x + dx * WANDER_STEP.1,
        z: z + WANDER_Z_BIAS + dz * WANDER_STEP.0,
        ripple_variant,
    }
}

/// Step the fish facing for one frame of *held* pad input and clamp it.
///
/// The pad word is the packed held mask `_DAT_8007B850`; `PACK_LEFT`
/// (`0x8000`) turns the fish one way and `PACK_RIGHT` (`0x2000`) the other.
/// Both bits in the same frame cancel. The clamp runs whether or not the
/// pad moved, so an out-of-range facing is pulled in on the first frame.
///
/// PORT: FUN_801d2278 (facing arm)
pub fn step_facing(facing: i16, pad_held: u16) -> i16 {
    let mut f = facing;
    if pad_held & PACK_LEFT != 0 {
        f = f.wrapping_sub(FACING_STEP);
    }
    if pad_held & PACK_RIGHT != 0 {
        f = f.wrapping_add(FACING_STEP);
    }
    f.clamp(FACING_RANGE.0, FACING_RANGE.1)
}

/// The camera state the wander tick publishes each frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FishCamera {
    /// `_DAT_8007B792` - yaw, `-((facing + 0x800) & 0xFFF)`.
    pub yaw: i16,
    /// `_DAT_80089118 / 0x8008911C / 0x80089120` - translation, the negated
    /// fish position with a zero Y.
    pub translation: (i32, i32, i32),
    /// `_DAT_800840BC` - the pitch/height term, `0x400 - 6 * y`.
    pub pitch_term: i32,
}

/// Publish the camera for a fish at `(x, y, z)` facing `facing`.
///
/// PORT: FUN_801d2278 (camera publish)
pub fn fish_camera(x: i16, y: i16, z: i16, facing: i16) -> FishCamera {
    let yaw = ((facing as i32).wrapping_add(0x800) & 0xFFF).wrapping_neg() as i16;
    FishCamera {
        yaw,
        translation: (-(x as i32), 0, -(z as i32)),
        pitch_term: 0x400 - 6 * (y as i32),
    }
}

// --- Pre-hook tick (FUN_801D2050) ------------------------------------------

/// Camera globals the pre-hook tick's one-shot init seeds
/// (`_DAT_80084044`, `_DAT_80084046`).
pub const CAMERA_INIT: (i16, i16) = (-0x7FFF, 100);

/// Species id the fish-sprite spawn special-cases with a larger scale and
/// the extra draw flags.
pub const SPECIAL_SPECIES: u32 = 8;

/// Scale written into both scale fields for [`SPECIAL_SPECIES`].
pub const SPECIAL_SPECIES_SCALE: i16 = 0x88;

/// World-units-per-tile shift the debug readout applies to X and Z.
pub const DEBUG_TILE_SHIFT: u32 = 7;

/// Held-pad bit that, together with the global print flag, enables the
/// overlay's debug readouts (`_DAT_8007B850 & 2`).
pub const PACK_DEBUG_MODIFIER: u16 = 0x0002;

/// Convert a world coordinate to the tile index the debug readout prints.
///
/// Retail biases a negative value by `+0x7F` before the arithmetic shift, so
/// the division truncates toward zero rather than toward negative infinity.
///
/// PORT: FUN_801d2050 (debug readout)
#[inline]
pub fn debug_tile(v: i16) -> i32 {
    let v = v as i32;
    let biased = if v < 0 { v + 0x7F } else { v };
    biased >> DEBUG_TILE_SHIFT
}

/// Whether the overlay's debug readouts are showing this frame.
///
/// Both the global print flag `_DAT_8007B9B0` and the held modifier bit have
/// to be set; the same gate switches the bite interval to its debug value
/// (see [`bite_interval`]).
///
/// PORT: FUN_801d2050 (readout gate)
#[inline]
pub fn debug_readout_visible(print_flag: bool, pad_held: u16) -> bool {
    print_flag && pad_held & PACK_DEBUG_MODIFIER != 0
}

// --- Bite roll and interval ladder (FUN_801D26CC) --------------------------

/// Bite cadence in frames while the debug readouts are up - the override
/// that makes the fish bite almost immediately.
pub const BITE_INTERVAL_DEBUG: i32 = 0x20;

/// Bite cadence for a cast **above** the ladder's only live threshold.
pub const BITE_INTERVAL_NEAR: i32 = 1000;

/// Bite cadence at exactly [`BITE_LADDER_PIVOT`] - the ladder's untouched
/// initial value.
pub const BITE_INTERVAL_PIVOT: i32 = 0x200;

/// Bite cadence below the pivot.
pub const BITE_INTERVAL_FAR: i32 = 2000;

/// The single distance the interval ladder actually discriminates on.
pub const BITE_LADDER_PIVOT: i32 = 200;

/// Bias added to the bite countdown in the far band.
pub const BITE_FAR_BIAS: i32 = -100;

/// Bite cadence for a cast metric of `distance` (`DAT_801D9280`).
///
/// The retail ladder is six `slti`/`bne` pairs writing the same register in
/// **ascending** threshold order, so every earlier arm is overwritten by a
/// later one that is true whenever it is. The four intermediate cadences
/// (`200`, `350`, `400`, `500`) are therefore unreachable: only the
/// `>= 201` arm, the `<= 199` arm and the untouched initial value survive.
/// The port reproduces the reachable behaviour and names the dead arms in
/// [`BITE_LADDER_DEAD_ARMS`] rather than pretending they run.
///
/// PORT: FUN_801d26cc (bite-interval ladder)
pub fn bite_interval(distance: i32, debug: bool) -> i32 {
    if debug {
        return BITE_INTERVAL_DEBUG;
    }
    if distance > BITE_LADDER_PIVOT {
        BITE_INTERVAL_NEAR
    } else if distance < BITE_LADDER_PIVOT {
        BITE_INTERVAL_FAR
    } else {
        BITE_INTERVAL_PIVOT
    }
}

/// The four `(threshold, cadence)` arms the ladder's write order makes
/// unreachable, kept so the dead range is documented rather than lost.
pub const BITE_LADDER_DEAD_ARMS: [(i32, i32); 4] = [(401, 200), (351, 350), (301, 400), (251, 500)];

/// Bite cadence bias for a cast metric (`-100` in the far band, `0`
/// otherwise). It rides on the same comparison as [`bite_interval`].
///
/// PORT: FUN_801d26cc (bite-interval bias)
#[inline]
pub fn bite_interval_bias(distance: i32) -> i32 {
    if distance < BITE_LADDER_PIVOT {
        BITE_FAR_BIAS
    } else {
        0
    }
}

/// Upper bound (inclusive) of each random band, most common first. A draw of
/// `rand() & 0xFFF` picks the **last** band whose bound it exceeds.
pub const HIT_TYPE_BANDS: [(u32, u8); 4] = [(0x0C00, 3), (0x0E70, 2), (0x0F38, 1), (0x0FFF, 0)];

/// Cast-band roll used when the scripted picker declines.
///
/// Retail seeds `3`, then overwrites with `2`, `1`, `0` as the draw passes
/// `0xC00`, `0xE70` and `0xF38`, so the bands are heavily skewed toward `3`
/// (3073/4096) and `0` is a 199-in-4096 tail.
///
/// PORT: FUN_801d26cc (hit-type roll)
pub fn roll_hit_type(draw: u32) -> u8 {
    let d = draw & 0xFFF;
    let mut band = 3;
    if d > 0x0C00 {
        band = 2;
    }
    if d > 0x0E70 {
        band = 1;
    }
    if d > 0x0F38 {
        band = 0;
    }
    band
}

/// Minimum cast metric below which the bite countdown is forced to zero
/// (`DAT_801D9280 < 100`).
pub const BITE_SUPPRESS_BELOW: i32 = 100;

/// Water-tile class flags read out of `_DAT_8007B8F4` after the walk-grid
/// probe reports the `0x4000` water bit, with the `(countdown bonus, weight)`
/// pair each one installs. Retail tests them in this order without `else`,
/// so the highest set bit wins.
pub const WATER_TILE_CLASSES: [(u32, i32, i32); 3] =
    [(0x04, 0x1E, 100), (0x08, 0x14, 300), (0x10, 0x14, 500)];

/// Resolve the water-tile class bonus for a probe result.
///
/// Returns `None` when no class bit is set, which leaves the countdown bonus
/// and the fish weight at their defaults (`0` and `10`).
///
/// PORT: FUN_801d26cc (water-tile class)
pub fn water_tile_class(flags: u32) -> Option<(i32, i32)> {
    let mut got = None;
    for (bit, bonus, weight) in WATER_TILE_CLASSES {
        if flags & bit != 0 {
            got = Some((bonus, weight));
        }
    }
    got
}

/// Default `(countdown bonus, weight)` outside every water class.
pub const WATER_TILE_DEFAULT: (i32, i32) = (0, 10);

/// Pad bits that each shorten the bite countdown by one frame while held
/// (`_DAT_8007B874`): the two D-pad bits and the two shoulder bits, the
/// latter pair tested as one mask.
pub const BITE_NUDGE_MASKS: [u32; 3] = [0x8000, 0x2000, 0x00C0];

/// Count this frame's pad nudges into the bite countdown.
///
/// PORT: FUN_801d26cc (pad nudge)
pub fn bite_pad_nudge(pad: u32) -> i32 {
    BITE_NUDGE_MASKS.iter().filter(|&&m| pad & m != 0).count() as i32
}

// --- Catch celebration and line sub-state (FUN_801D4948) -------------------

/// The reeling-line actor's sub-state (`DAT_801D91C8`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinePhase {
    /// `0` - arm: seed the hook cue and step to [`LinePhase::Attach`].
    Arm,
    /// `1` - copy the hooked fish's position out of `actor[+0x48]`, then
    /// step to [`LinePhase::Track`].
    Attach,
    /// `2` - track the hooked fish each frame.
    Track,
    /// `4` - the catch celebration. The published sub-state list omits this
    /// arm; it is the bulk of the routine.
    Celebrate,
    /// Any other value: the routine leaves the actor alone.
    Idle(u32),
}

impl LinePhase {
    /// Decode the raw sub-state word.
    ///
    /// PORT: FUN_801d4948 (sub-state decode)
    pub fn from_raw(v: u32) -> LinePhase {
        match v {
            0 => LinePhase::Arm,
            1 => LinePhase::Attach,
            2 => LinePhase::Track,
            4 => LinePhase::Celebrate,
            other => LinePhase::Idle(other),
        }
    }
}

/// SFX cue the arm phase raises (`_DAT_8007B6DA = 0x3A`).
pub const HOOK_CUE: u8 = 0x3A;

/// SFX cue the celebration's first stage raises.
pub const CELEBRATE_CUE: u8 = 0x2B;

/// One firework burst of the catch celebration: the score threshold that
/// unlocks it, its spawn offset and the SFX cue it raises (`None` for the
/// top tier, which is silent).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CelebrationBurst {
    /// Exclusive lower bound on the catch score `DAT_801D91B8`.
    pub above: i32,
    /// Spawn offset `(x, y, z)`.
    pub offset: (i16, i16, i16),
    /// Cue raised alongside the burst.
    pub cue: Option<u8>,
}

/// The four bursts, in the order retail evaluates them. Every tier whose
/// threshold the score clears fires, so a big catch plays all four.
///
/// PORT: FUN_801d4948 (celebration tiers)
pub const CELEBRATION_BURSTS: [CelebrationBurst; 4] = [
    CelebrationBurst {
        above: 200,
        offset: (0x190, 0x190, 1000),
        cue: Some(0x25),
    },
    CelebrationBurst {
        above: 600,
        offset: (0x190, -0x190, 800),
        cue: Some(0x26),
    },
    CelebrationBurst {
        above: 800,
        offset: (-0x190, 0, 800),
        cue: Some(0x27),
    },
    CelebrationBurst {
        above: 0x4B0,
        offset: (0, 0, 1000),
        cue: None,
    },
];

/// The bursts a catch score unlocks.
///
/// PORT: FUN_801d4948 (celebration gate)
pub fn celebration_bursts(score: i32) -> impl Iterator<Item = &'static CelebrationBurst> {
    CELEBRATION_BURSTS.iter().filter(move |b| score > b.above)
}

/// Frame counts on the celebration actor's `+0x22` timer at which the
/// celebration advances stage: `(fire the bursts, fire the two flashes,
/// hand the actor back)`.
pub const CELEBRATION_STAGE_FRAMES: (i16, i16, i16) = (0x72, 0x87, 0xD2);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_segment_wholly_behind_the_near_bound_is_rejected() {
        assert_eq!(project_segment([0, 0, 10], [10, 0, 20], 100, 0x100), None);
    }

    #[test]
    fn a_segment_wholly_in_front_projects_both_ends_unclipped() {
        let s = project_segment([0, 0, 1000], [100, 50, 1000], 100, 0x100).unwrap();
        assert_eq!(s.view[0], [0, 0, 1000]);
        assert_eq!(s.view[1], [100, 50, 1000]);
        // Centred x = 0 lands on the screen centre.
        assert_eq!(s.screen[0], SCREEN_CENTRE);
        assert!(s.screen[1].0 > SCREEN_CENTRE.0);
    }

    #[test]
    fn the_near_endpoint_is_pulled_onto_the_bound() {
        // a is behind the bound, b is in front.
        let s = project_segment([0, 0, 50], [400, 0, 450], 100, 0x100).unwrap();
        assert_eq!(s.view[0][2], 100, "clipped to the near bound");
        assert_eq!(s.view[1], [400, 0, 450], "the far end is untouched");
        // The correct crossing sits 1/8 along a->b: x = 50.
        assert_eq!(s.view[0][0], 50);
    }

    #[test]
    fn the_far_arm_uses_the_complementary_parameter() {
        // Mirror of the case above: b is the one behind the bound. The
        // retail arm slides b by 1 - t instead of t, so it does NOT land on
        // the geometric crossing - only its z is forced to the bound.
        let s = project_segment([400, 0, 450], [0, 0, 50], 100, 0x100).unwrap();
        assert_eq!(s.view[0], [400, 0, 450]);
        assert_eq!(s.view[1][2], 100);
        // The geometric crossing is x = 50; retail's complementary
        // parameter puts the endpoint at 350 instead.
        assert_eq!(s.view[1][0], 350);
    }

    #[test]
    fn the_facing_clamp_holds_the_arc() {
        assert_eq!(step_facing(0x800, 0), 0x800);
        assert_eq!(step_facing(0x800, PACK_RIGHT), 0x840);
        assert_eq!(step_facing(0x800, PACK_LEFT), 0x7C0);
        // Both directions in one frame cancel.
        assert_eq!(step_facing(0x800, PACK_LEFT | PACK_RIGHT), 0x800);
        // The clamp catches the ends, and pulls an out-of-range seed in.
        assert_eq!(step_facing(0x700, PACK_LEFT), 0x700);
        assert_eq!(step_facing(0x900, PACK_RIGHT), 0x900);
        assert_eq!(step_facing(0x100, 0), 0x700);
    }

    #[test]
    fn the_camera_negates_the_position_and_wraps_the_yaw() {
        let c = fish_camera(0x200, 0x40, -0x300, 0x800);
        assert_eq!(c.translation, (-0x200, 0, 0x300));
        // facing 0x800 -> (0x800 + 0x800) & 0xFFF = 0 -> yaw 0.
        assert_eq!(c.yaw, 0);
        assert_eq!(c.pitch_term, 0x400 - 6 * 0x40);
        // A quarter turn wraps inside the 12-bit angle space.
        assert_eq!(fish_camera(0, 0, 0, 0).yaw, -0x800);
    }

    #[test]
    fn the_wander_roll_consumes_five_draws_in_order() {
        let draws = [0, 50, 0, 6, 1];
        let mut i = 0;
        let t = roll_wander_target(1000, 2000, || {
            let v = draws[i];
            i += 1;
            v
        });
        assert_eq!(i, 5);
        assert_eq!(t.dwell, 50 + RETARGET_MIN);
        // rand % 6 == 0 -> step 3; rand 6 % 6 == 0 -> step 3 as well.
        assert_eq!(t.z, 2000 + WANDER_Z_BIAS + 3 * 0x20);
        assert_eq!(t.x, 1000 + 3 * 0x50);
        assert_eq!(t.ripple_variant, 1);
    }

    #[test]
    fn debug_tiles_truncate_toward_zero() {
        assert_eq!(debug_tile(0), 0);
        assert_eq!(debug_tile(128), 1);
        assert_eq!(debug_tile(127), 0);
        assert_eq!(debug_tile(-1), 0);
        assert_eq!(debug_tile(-128), -1);
        assert_eq!(debug_tile(-129), -1);
    }

    #[test]
    fn the_debug_readout_needs_both_the_flag_and_the_modifier() {
        assert!(!debug_readout_visible(false, PACK_DEBUG_MODIFIER));
        assert!(!debug_readout_visible(true, 0));
        assert!(debug_readout_visible(true, PACK_DEBUG_MODIFIER));
    }

    #[test]
    fn the_bite_ladder_only_discriminates_at_two_hundred() {
        assert_eq!(bite_interval(201, false), BITE_INTERVAL_NEAR);
        assert_eq!(bite_interval(100_000, false), BITE_INTERVAL_NEAR);
        assert_eq!(bite_interval(200, false), BITE_INTERVAL_PIVOT);
        assert_eq!(bite_interval(199, false), BITE_INTERVAL_FAR);
        assert_eq!(bite_interval(0, false), BITE_INTERVAL_FAR);
        // None of the dead arms' cadences is ever produced.
        for (threshold, cadence) in BITE_LADDER_DEAD_ARMS {
            assert_ne!(bite_interval(threshold, false), cadence);
        }
    }

    #[test]
    fn the_debug_gate_overrides_the_whole_ladder() {
        assert_eq!(bite_interval(0, true), BITE_INTERVAL_DEBUG);
        assert_eq!(bite_interval(5000, true), BITE_INTERVAL_DEBUG);
    }

    #[test]
    fn only_the_far_band_carries_the_bias() {
        assert_eq!(bite_interval_bias(199), BITE_FAR_BIAS);
        assert_eq!(bite_interval_bias(200), 0);
        assert_eq!(bite_interval_bias(1000), 0);
    }

    #[test]
    fn the_hit_type_roll_is_skewed_to_band_three() {
        assert_eq!(roll_hit_type(0), 3);
        assert_eq!(roll_hit_type(0x0C00), 3);
        assert_eq!(roll_hit_type(0x0C01), 2);
        assert_eq!(roll_hit_type(0x0E70), 2);
        assert_eq!(roll_hit_type(0x0E71), 1);
        assert_eq!(roll_hit_type(0x0F38), 1);
        assert_eq!(roll_hit_type(0x0F39), 0);
        assert_eq!(roll_hit_type(0x0FFF), 0);
        // The draw is masked, so high bits never change the band.
        assert_eq!(roll_hit_type(0xFFFF_F000), 3);
    }

    #[test]
    fn the_highest_water_class_bit_wins() {
        assert_eq!(water_tile_class(0), None);
        assert_eq!(water_tile_class(0x04), Some((0x1E, 100)));
        assert_eq!(water_tile_class(0x08), Some((0x14, 300)));
        assert_eq!(water_tile_class(0x10), Some((0x14, 500)));
        // Several bits set: the last arm tested wins, as in retail.
        assert_eq!(water_tile_class(0x1C), Some((0x14, 500)));
    }

    #[test]
    fn each_nudge_mask_counts_once() {
        assert_eq!(bite_pad_nudge(0), 0);
        assert_eq!(bite_pad_nudge(0x8000), 1);
        assert_eq!(bite_pad_nudge(0x8000 | 0x2000), 2);
        // 0x40 and 0x80 are one mask, so holding both still counts one.
        assert_eq!(bite_pad_nudge(0x40 | 0x80), 1);
        assert_eq!(bite_pad_nudge(0xA0C0), 3);
    }

    #[test]
    fn the_line_sub_state_has_a_fourth_arm() {
        assert_eq!(LinePhase::from_raw(0), LinePhase::Arm);
        assert_eq!(LinePhase::from_raw(1), LinePhase::Attach);
        assert_eq!(LinePhase::from_raw(2), LinePhase::Track);
        assert_eq!(LinePhase::from_raw(4), LinePhase::Celebrate);
        assert_eq!(LinePhase::from_raw(3), LinePhase::Idle(3));
    }

    #[test]
    fn celebration_tiers_accumulate_with_the_score() {
        assert_eq!(celebration_bursts(0).count(), 0);
        assert_eq!(celebration_bursts(201).count(), 1);
        assert_eq!(celebration_bursts(601).count(), 2);
        assert_eq!(celebration_bursts(801).count(), 3);
        assert_eq!(celebration_bursts(0x4B1).count(), 4);
        // The cues fire bottom-up, and the top tier is silent.
        let cues: Vec<Option<u8>> = celebration_bursts(2000).map(|b| b.cue).collect();
        assert_eq!(cues, vec![Some(0x25), Some(0x26), Some(0x27), None]);
    }
}
