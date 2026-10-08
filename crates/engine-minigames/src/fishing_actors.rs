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
//! ## Wiring status is per item, not per module
//!
//! This file carried a blanket `# NOT WIRED` heading, and it stopped being
//! true: the bite-roll trio ([`bite_interval`], [`bite_credit_override`],
//! [`roll_hit_type`]) is now on the live fight path through
//! [`crate::fishing::BandCheck::tick`]. A module blanket is read
//! unconditionally by every anchor in the file, so one wired item makes it
//! assert something false about that item and it cannot be narrowed in place.
//! Each genuinely inert item therefore carries its own `NOT WIRED:` line.
//!
//! `crate::fishing` models the minigame as *rules* (cast power, reel
//! tug-of-war, catch scoring); the actor-side kernels drive the retail
//! overlay's per-frame actor structs (`+0x14/+0x16/+0x18` position, `+0x22`
//! phase, `+0x26` facing) and the scene camera globals. [`FishWander`] and
//! [`LineActorSim`] carry those actors as advancing objects, hosted by the
//! play window's fishing frame (`window/minigames.rs`); the items still
//! inert (the line-draw pair, the walk-grid probes) name their own remaining
//! blocker in place.

use legaia_engine_vm::pad::{PACK_LEFT, PACK_RIGHT};

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
// REPLACED-BY: nothing is owed a port - **retail reaches this routine from
// nowhere.** A five-form reference sweep (literal LE word at every alignment,
// `lui`+`addiu` / `ori` materialisation, `jal`, `j`, PC-relative branch) over
// `SCUS_942.54`, every base-mapped overlay image and every raw PROT entry
// finds zero references to `0x801D5C2C`, and the fishing overlay holds exactly
// one literal pointer anywhere in the surrounding `0x801D5000..0x801D63FF`
// band, so it is not reached as `table_base + index` either. It is a real
// prologue entry point (`locate-entry-image.py` frames it in PROT 0972 and in
// no other image) that nothing calls - dead code the linker kept, and so a
// row the wiring worklist can never close (`docs/tooling/port-catalog.md` -
// What may carry it, fourth shape).
//
// So this is not the same row as [`clip_segment_2d`] below, which is live
// retail code with one caller. Naming a missing line primitive here would imply
// a call site could exist; none can. The port keeps the routine because the
// arithmetic is documented ground truth for the projection those overlays use,
// not because a host is owed.
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
// Wired: [`FishWander::tick`] re-rolls through this on dwell expiry, and the
// play window hosts a wander actor while the cast is idle
// (`window/minigames.rs`), spawning the rolled `ripple_variant`'s ripple into
// its effect pool.
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
// Wired: [`FishWander`] owns the `+0x26` facing word and steps it here each
// idle/cast frame; the play window feeds it the packed held mask built from
// its own pad state (`window/minigames.rs`).
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
// Wired: [`FishWander::camera`] publishes this each idle/cast frame, and the
// play window folds it into the engine camera's retail global trios
// (`Camera::globals` axes 1 / 4 / 6..8 - the same `_DAT_8007B792` /
// `_DAT_800840BC` / `_DAT_80089118..20` words retail writes).
pub fn fish_camera(x: i16, y: i16, z: i16, facing: i16) -> FishCamera {
    let yaw = ((facing as i32).wrapping_add(0x800) & 0xFFF).wrapping_neg() as i16;
    FishCamera {
        yaw,
        translation: (-(x as i32), 0, -(z as i32)),
        pitch_term: 0x400 - 6 * (y as i32),
    }
}

/// The lead angler's actor as one object: the `+0x14/+0x18` world pair, the
/// `+0x26` facing word and the dwell counter, stepped one call per idle/cast
/// frame through the ported kernels ([`step_facing`], [`roll_wander_target`],
/// [`fish_camera`]).
///
/// The name is historical: the tick (`FUN_801D2050` -> `FUN_801D2278`) was
/// read as a free-swimming fish, but its actor pointer `DAT_801D928C` is the
/// lead party member the setup spawns on the shore
/// (`legaia_engine_core::fishing_venue::party_placements`) - the library state
/// `minigame_fishing` holds the same pointer as the lead at `(4736, 10752)`.
/// The D-pad aims him within `0x700..=0x900`, the camera follows him, and the
/// dwell roll only places an ambient ripple out in the water; the actor
/// itself never moves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FishWander {
    /// World position (`+0x14` / `+0x16` / `+0x18`).
    pub x: i16,
    pub y: i16,
    pub z: i16,
    /// Facing word (`+0x26`).
    pub facing: i16,
    /// Frames left on the current dwell.
    dwell: i32,
}

impl FishWander {
    /// A fish parked at `(x, y, z)`, facing the middle of the steerable arc,
    /// due for a re-target on its first tick.
    pub fn new(x: i16, y: i16, z: i16) -> Self {
        FishWander {
            x,
            y,
            z,
            facing: 0x800,
            dwell: 0,
        }
    }

    /// One idle/cast frame: step + clamp the facing off the held pad, count
    /// the dwell down, and re-roll the destination when it expires. Returns
    /// the roll when one happened - its `ripple_variant` picks the ripple
    /// descriptor the host spawns at the retarget.
    pub fn tick<F: FnMut() -> u32>(&mut self, pad_held: u16, rand: F) -> Option<WanderTarget> {
        self.facing = step_facing(self.facing, pad_held);
        self.dwell -= 1;
        let mut rolled = None;
        if self.dwell <= 0 {
            let t = roll_wander_target(self.x as i32, self.z as i32, rand);
            self.dwell = t.dwell;
            rolled = Some(t);
        }
        // The roll offsets an on-stack *copy* of the position and spawns the
        // ripple there (`FUN_80021B04(sp+0x10, ..)`, `0x801D23FC`); nothing
        // in `FUN_801D2278` writes `+0x14` / `+0x18` back, so the actor -
        // the lead angler on the shore - holds its place.
        rolled
    }

    /// This frame's camera publish for the actor's live pose.
    pub fn camera(&self) -> FishCamera {
        fish_camera(self.x, self.y, self.z, self.facing)
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
// Wired: the play window's fishing HUD prints the wander actor's tile pair
// through this when the developer readout is up (`window/hud.rs`, gated by
// `debug_readout_visible` off the dev-menu print flag).
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
// Wired: the play window's fishing HUD computes this from the dev-menu
// session's presence (the engine's `_DAT_8007B9B0` stand-in) and the held pad
// modifier, and shows the `debug_tile` readout when it holds
// (`window/hud.rs`).
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

/// Strike credit the far band **replaces** the whole credit base with.
pub const BITE_FAR_CREDIT: i32 = -100;

/// Bite cadence for a cast metric of `distance` (`DAT_801D9280`).
///
/// This is the **modulus** of the per-frame strike roll: retail's
/// `(rand() % interval) < credit`, so a larger interval is a rarer bite.
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

/// The far band's strike-credit **override**, if it applies.
///
/// This rides on the same comparison as [`bite_interval`], and it is not a
/// bias: retail writes `li s1, -0x64` into the register that already holds
/// the credit base (`countdown + 2`, or `0x40` on a cadence match), so the
/// base is *replaced*, not offset. Everything added after the ladder - the
/// water-class bonus and the pad nudges - still lands on top of the `-100`,
/// which is why a shallow cast cannot strike at all: those add at most
/// `0x1E + 3`, far short of zero.
///
/// PORT: FUN_801d26cc (far-band credit override)
#[inline]
pub fn bite_credit_override(distance: i32) -> Option<i32> {
    (distance < BITE_LADDER_PIVOT).then_some(BITE_FAR_CREDIT)
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
// Wired through [`LureActor::probe`] on both fishing hosts. Its input is the
// `_DAT_8007B8F4` class word, and the previous note here had that word's
// producer wrong: it is **not** a read taken "after the walk-grid probe
// reports the `0x4000` water bit". The water gate is bit `0x4000` of the
// `+0x8000` per-tile cell word (`andi v1,v1,0x4000` at `0x801D3374`, over a
// halfword loaded through the scratchpad scene pointer at `0x801D3330`), and
// the class word is then rebuilt by `FUN_800180EC` - the region-kind mask
// [`legaia_engine_vm::field_regions::refresh_region_attributes`] already ports - called
// at `0x801D3384` with the same tile pair. `FUN_801D7030`'s probe is a
// separate read with a separate consequence.
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

/// Count this frame's pad nudges into the strike credit.
///
/// Retail loads the newly-pressed word `_DAT_8007B874` once and adds one to
/// the credit register per mask that hits (`0x801D343C..0x801D3468`): D-pad
/// left, D-pad right, and the two reel bits tested together - so both reel
/// buttons pressed on one frame are one nudge, and the cast press is none.
///
/// PORT: FUN_801d26cc (pad nudge)
///
/// Wired on all three hosts: `World::tick_fishing` counts the play hosts'
/// nudge through it off the engine pad's newly-pressed edges, rotated into
/// the packed layout, and the minigames page's `fishing_pond_tick` export
/// takes the packed pressed word and counts through it too.
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
    // Wired: [`LineActorSim`] owns the engine's `DAT_801D91C8` stand-in and
    // decodes it here every tick; the play window drives the sim across the
    // hook -> fight -> celebration phases (`window/minigames.rs`).
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
/// The address tag lives on [`celebration_bursts`], which reads this table -
/// a tag on the `const` resolves to no code anchor and the audit widens it to
/// the whole module.
///
/// REF: FUN_801d4948 (celebration tiers)
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
// Wired: [`LineActorSim::tick`]'s celebrate arm resolves the unlocked tiers
// at the first stage frame; `World::tick_fishing` queues each `cue` on the
// world's SFX channel at the session's catch edge (so all three hosts hear
// it), and the play window spawns the bursts into its effect pool (offset
// from the wander actor's catch position) in `window/minigames.rs`.
pub fn celebration_bursts(score: i32) -> impl Iterator<Item = &'static CelebrationBurst> {
    CELEBRATION_BURSTS.iter().filter(move |b| score > b.above)
}

/// Frame counts on the celebration actor's `+0x22` timer at which the
/// celebration advances stage: `(fire the bursts, fire the two flashes,
/// hand the actor back)`.
pub const CELEBRATION_STAGE_FRAMES: (i16, i16, i16) = (0x72, 0x87, 0xD2);

/// What one [`LineActorSim::tick`] asks the host to do.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LineActorFrame {
    /// SFX cue to raise this frame ([`HOOK_CUE`] on the arm phase,
    /// [`CELEBRATE_CUE`] at the celebration's first stage).
    pub cue: Option<u8>,
    /// Celebration bursts unlocked this frame (the first stage's
    /// [`celebration_bursts`] resolution) - the host spawns each at its
    /// offset from the catch position and fires its own `cue`.
    pub bursts: Vec<CelebrationBurst>,
    /// The celebration ran its `+0x22` timer out - the actor hands back.
    pub done: bool,
}

/// The reeling-line actor as one advancing object: the engine's stand-in for
/// the `DAT_801D91C8` sub-state word plus the celebration's `+0x22` timer,
/// decoded through [`LinePhase::from_raw`] every tick exactly as the retail
/// handler switches on the raw word.
///
/// A host arms it on the hook, ticks it while the fight runs, calls
/// [`LineActorSim::land`] with the catch score, and keeps ticking until
/// [`LineActorFrame::done`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LineActorSim {
    /// The raw sub-state word (`DAT_801D91C8`).
    pub raw: u32,
    /// The celebration timer (`actor + 0x22`).
    pub timer: i16,
    /// The landed catch score (`DAT_801D91B8`), set by [`LineActorSim::land`].
    pub score: i32,
}

impl LineActorSim {
    /// A line actor at the arm phase (the hook just set).
    pub fn hooked() -> Self {
        LineActorSim::default()
    }

    /// The catch landed for `score` points: enter the celebration arm.
    pub fn land(&mut self, score: i32) {
        self.raw = 4;
        self.timer = 0;
        self.score = score;
    }

    /// One frame of the line actor's handler.
    pub fn tick(&mut self, frame_step: i16) -> LineActorFrame {
        let mut out = LineActorFrame::default();
        match LinePhase::from_raw(self.raw) {
            LinePhase::Arm => {
                out.cue = Some(HOOK_CUE);
                self.raw = 1;
            }
            LinePhase::Attach => {
                // The host copies the hooked fish's position; the sim only
                // steps the sub-state.
                self.raw = 2;
            }
            LinePhase::Track => {}
            LinePhase::Celebrate => {
                let before = self.timer;
                self.timer = self.timer.saturating_add(frame_step);
                let crossed = |at: i16| before < at && at <= self.timer;
                if crossed(CELEBRATION_STAGE_FRAMES.0) {
                    out.cue = Some(CELEBRATE_CUE);
                    out.bursts = celebration_bursts(self.score).copied().collect();
                }
                if crossed(CELEBRATION_STAGE_FRAMES.2) {
                    out.done = true;
                }
            }
            LinePhase::Idle(_) => {}
        }
        out
    }
}

// --- 2-D segment clip (FUN_801D56E4) ---------------------------------------

/// The four scratchpad halfwords the 2-D clipper reads, in the order it
/// reads them: `x_min` (`0x1F800388`, offset `+0x74` off the render block at
/// `0x1F800314`), `y_min` (`+0x76`), `x_max` (`+0x78`), `y_max` (`+0x7A`).
///
/// Retail sign-extends each bound on the *comparison* and reloads it
/// **zero-extended** (`lhu`) for the store, so a bound with the top bit set
/// compares negative and stores positive. The port keeps the bounds signed;
/// the retail window is a screen rectangle and never reaches that case.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClipRect {
    pub x_min: i16,
    pub y_min: i16,
    pub x_max: i16,
    pub y_max: i16,
}

// Wired through [`fishing_line`], the fishing line's per-frame build: retail's
// one caller (`jal` at `0x801D3D00`, the tail of the per-frame lure tick
// `FUN_801D26CC`) clips the line packet's two endpoints against
// [`LINE_CLIP_RECT`] before linking it. `crate::fishing::PondSession::line_frame`
// runs it every frame a line is out, on all three hosts (see
// `docs/subsystems/minigame-fishing.md` § "The fishing line").
// (`project_segment` above is a different case - retail never calls it at all.)
/// Clip a 2-D segment in place against [`ClipRect`].
///
/// `p` and `q` are the two `(x, y)` endpoints; both are edited. Retail runs
/// **eight** arms - each of the four bounds is applied to each endpoint in
/// turn, in the order `x_min(p)`, `x_min(q)`, `x_max(p)`, `x_max(q)`,
/// `y_min(p)`, `y_min(q)`, `y_max(p)`, `y_max(q)` - and each arm fires only
/// when the endpoint it moves is outside the bound **and the other endpoint
/// is strictly inside it**, so a segment wholly outside one bound is left
/// alone rather than collapsed.
///
/// Every arm has the same fixed-point form. For the `x_min` arm on `p`:
/// `t = ((q.x - bound) << 12) / (q.x - p.x)`, then
/// `p.y = q.y + (((p.y - q.y) * t) >> 12)` with the `+0xFFF` bias that
/// truncates a negative product toward zero, then `p.x = bound`. The `y`
/// arms are the same with the roles of the two components swapped.
///
/// The parameter is measured from the **other** endpoint, which is why the
/// blend is written against `q` rather than against `p`.
///
/// One deliberate deviation: retail reaches the R3000 divide-by-zero trap
/// when the two endpoints share the component being clipped. That cannot
/// happen on a firing arm - the arm requires one endpoint strictly below the
/// bound and the other strictly above it, so the difference is non-zero - but
/// the port returns without editing rather than trapping if it ever is.
///
/// PORT: FUN_801d56e4
pub fn clip_segment_2d(p: &mut (i16, i16), q: &mut (i16, i16), rect: ClipRect) {
    // `lo`: the endpoint sits below the bound and the other above it, so the
    // low side is clipped up onto it. `hi` is the mirror.
    fn arm_lo(a: &mut (i16, i16), b: (i16, i16), bound: i16, vertical: bool) {
        let (ac, bc) = if vertical { (a.1, b.1) } else { (a.0, b.0) };
        if !((ac as i32) < bound as i32 && (bound as i32) < bc as i32) {
            return;
        }
        blend(
            a,
            b,
            bound,
            vertical,
            (bc as i32 - bound as i32) << 12,
            bc,
            ac,
        );
    }
    fn arm_hi(a: &mut (i16, i16), b: (i16, i16), bound: i16, vertical: bool) {
        let (ac, bc) = if vertical { (a.1, b.1) } else { (a.0, b.0) };
        if !((bound as i32) < ac as i32 && (bc as i32) < bound as i32) {
            return;
        }
        blend(
            a,
            b,
            bound,
            vertical,
            (bound as i32 - bc as i32) << 12,
            ac,
            bc,
        );
    }
    fn blend(
        a: &mut (i16, i16),
        b: (i16, i16),
        bound: i16,
        vertical: bool,
        num: i32,
        den_hi: i16,
        den_lo: i16,
    ) {
        let den = den_hi as i32 - den_lo as i32;
        if den == 0 {
            return;
        }
        let t = num / den;
        if vertical {
            a.0 = (b.0 as i32 + lerp12(a.0 as i32 - b.0 as i32, t)) as i16;
            a.1 = bound;
        } else {
            a.1 = (b.1 as i32 + lerp12(a.1 as i32 - b.1 as i32, t)) as i16;
            a.0 = bound;
        }
    }

    arm_lo(p, *q, rect.x_min, false);
    arm_lo(q, *p, rect.x_min, false);
    arm_hi(p, *q, rect.x_max, false);
    arm_hi(q, *p, rect.x_max, false);
    arm_lo(p, *q, rect.y_min, true);
    arm_lo(q, *p, rect.y_min, true);
    arm_hi(p, *q, rect.y_max, true);
    arm_hi(q, *p, rect.y_max, true);
}

// --- The rod actor (FUN_801D1C5C) and the fishing line ---------------------

/// Scene-bank index of rod `0` in the venue scene (`other1`).
///
/// The cast arm spawns the rod actor from the template at `0x801D8FDC` after
/// writing its model word as `_DAT_8007B6F8 + _DAT_80084454 + 0x19`
/// (`0x801CFC34..0x801CFC4C`): the scene bank's base plus the persistent rod
/// index plus this constant, so rod `r` is scene model `0x19 + r`.
pub const ROD_MODEL_BASE: i16 = 0x19;

/// Vertex of the rod mesh's object 0 that is the line's rod end.
///
/// `FUN_801D1C5C` projects `+0x128` into the object's staged vertex array
/// (`addiu a0,s0,0x128` in the delay slot at `0x801D1FB8`), eight bytes a
/// vertex: vertex 37, the centre of the rod's last ring.
pub const ROD_TIP_VERTEX: usize = 0x128 / 8;

/// The VDF sub-entry the rod bends by: the actor's one morph slot names it
/// (`sb zero,0xb0(s1)` at `0x801D1EDC`, slot count `1` at `+0x6C`) and the
/// stager `FUN_8001C604` is asked for TMD group `0` (`clear a1` at
/// `0x801D1F98`).
pub const ROD_BEND_VDF_ENTRY: u8 = 0;

/// The rod actor's `+0x14 / +0x16 / +0x18` position, written at spawn
/// (`0x801CFC60..0x801CFC80`). It is a **view-space** point: the actor
/// composes its own matrix rather than the scene camera's.
pub const ROD_ACTOR_POS: [i16; 3] = [0, 0x46, 0x64];

/// The diagonal of the matrix the rod composes on: `FUN_801D1C5C` copies the
/// per-mode base matrix `0x8007BF10` to the scratchpad and stores `0x6000` into
/// its three diagonal halfwords (`0x801D1E28..0x801D1E38`). The copied base
/// is itself `0x6000 * I` with a zero translation in the fishing mode (read
/// off the `minigame_fishing` save state), so the rotation is a pure 6x scale.
pub const ROD_BASE_SCALE: i32 = 0x6000;

/// GTE `H` the rod projects under (`li a0,0xdc` into `FUN_8003D254` at
/// `0x801D1FA4`); the scene's own `_DAT_8007B6F4` is restored after the draw.
pub const ROD_PROJECTION_H: u16 = 0xDC;

/// The GTE screen centre `(OFX, OFY)` in whole pixels - global for the whole
/// game ([`legaia_engine_vm::battle_cam_script::GTE_OFY`]); the fishing save
/// state carries the same pair.
pub const GTE_SCREEN_CENTRE: (i32, i32) = (160, 114);

/// The draw-window bounds [`clip_segment_2d`] reads for the line: the
/// scratchpad halfwords `0x1F800388..0x1F80038E`, read off the
/// `minigame_fishing` save state. Retail's drawing area is `320 x 224` from
/// row `4`, which is where the `4` / `0xE4` come from.
pub const LINE_CLIP_RECT: ClipRect = ClipRect {
    x_min: 0,
    y_min: 4,
    x_max: 0x140,
    y_max: 0xE4,
};

/// The line packet's colour at its **fish** end (`+0x04`, the low three bytes
/// of `0x50303030` at `0x801D3A28`). The packet is `LINE_G2` (`0x50`): a
/// Gouraud line, opaque.
pub const LINE_FISH_RGB: [u8; 3] = [0x30, 0x30, 0x30];

/// The line packet's colour at its **rod** end (`+0x0C..+0x0E`, three `0x80`
/// byte stores at `0x801D3A50..0x801D3A58`).
pub const LINE_ROD_RGB: [u8; 3] = [0x80, 0x80, 0x80];

/// The ordering-table shift the line links at: the rod tip's projected depth
/// `>> (0x1F8003A4 + 2)` (`0x801D3D18..0x801D3D24`), the scratchpad byte
/// being `3` in the fishing save state.
pub const LINE_OT_SHIFT: u32 = 3 + 2;

/// Per-frame-delta bob step of the rod's cast / recover swing (`sll v1,v1,0x6`
/// at `0x801D1D00` / `0x801D1D40`).
pub const ROD_BOB_STEP: i32 = 0x40;

/// Where the cast swing stops (`slti v0,v0,-0x2bc` at `0x801D1D14`).
pub const ROD_BOB_FLOOR: i16 = -0x2BC;

/// Where the recover swing stops (`slti v0,v0,0x401` at `0x801D1D54`).
pub const ROD_BOB_CEILING: i16 = 0x400;

/// The bend the cast lock seeds (`li v0,0x1000` stored to `0x801D9150` at
/// `0x801CFC2C`).
pub const ROD_BEND_AT_CAST: i32 = 0x1000;

/// The rod actor's sub-state (`0x801D91AC`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RodSwing {
    /// `1`: the cast swing, dipping the rod to [`ROD_BOB_FLOOR`].
    Cast,
    /// `2`: holding the cast pose while the line is out.
    Hold,
    /// `10`: the recover swing back up to [`ROD_BOB_CEILING`] after a landed
    /// catch or a snapped line (`0x801D3CB4` / `0x801D3C44`).
    Recover,
    /// `0x14`: the recover finished; the actor retires (`ori v0,v0,0x8`).
    Done,
}

/// One primitive of a rod model's object 0: the untextured flat / Gouraud
/// triangles and quads the three rods are built from (TMD groups of kinds
/// `12..=15`, the `FUN_80043390` bank-0 handlers).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RodPrim {
    /// Vertex indices into object 0's (staged) vertex array; a triangle
    /// repeats its third index in the fourth slot.
    pub verts: [u16; 4],
    /// `true` for a quad (the group's `flags` bit 1).
    pub quad: bool,
    /// Per-vertex packet colour, in [`Self::verts`] order. A flat primitive
    /// carries its one colour word on every vertex.
    pub rgb: [[u8; 3]; 4],
}

/// The rod geometry the line's rod end is a point of, lifted off the venue
/// scene's bank.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RodMesh {
    /// Object 0's vertex bytes (eight per vertex) of rods `0..=2`, scene
    /// models `0x19..=0x1B`.
    pub rods: [Option<Vec<u8>>; 3],
    /// The bend: VDF sub-entry [`ROD_BEND_VDF_ENTRY`] of the scene's
    /// type-7 buffer.
    pub bend: Option<Vec<u8>>,
    /// Object 0's primitives of rods `0..=2`, in packet order - what
    /// `FUN_801D1C5C` hands the per-primitive dispatcher `FUN_80043390`
    /// (`jal` at `0x801D1FF4`). Empty for a rod whose model did not resolve
    /// or carried no untextured primitive.
    pub prims: [Vec<RodPrim>; 3],
}

/// The rod's primitives off a parsed model: object 0's untextured groups.
/// A textured group (none of the venue's three rods carries one) is skipped
/// rather than drawn without its texture.
fn rod_prims(bytes: &[u8], tmd: &legaia_tmd::Tmd) -> Vec<RodPrim> {
    let Some(obj) = tmd.objects.first() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for g in legaia_tmd::legaia_prims::iter_groups_lenient(
        bytes,
        obj.primitives_byte_offset,
        obj.primitives_byte_size,
    ) {
        let quad = g.header.n_vertices() == 4;
        for p in &g.prims {
            if !p.uvs.is_empty() {
                continue;
            }
            let idx = p.vertex_indices();
            let n = idx.len().min(4);
            if n < 3 || p.colors.len() < n {
                continue;
            }
            let mut verts = [0u16; 4];
            let mut rgb = [[0u8; 3]; 4];
            for i in 0..4 {
                let k = i.min(n - 1);
                verts[i] = idx[k];
                rgb[i] = p.colors[k];
            }
            out.push(RodPrim {
                verts,
                quad: quad && n == 4,
                rgb,
            });
        }
    }
    out
}

impl RodMesh {
    /// Lift the three rods out of their TMD bytes (scene models
    /// [`ROD_MODEL_BASE`]`..+3`, in rod order) and take the bend as given
    /// (VDF sub-entry [`ROD_BEND_VDF_ENTRY`] of the venue's type-7 buffer).
    /// `None` when not one rod model resolves. `engine-core`'s
    /// `fishing_actors::rod_mesh_from_scene` gathers both out of a loaded
    /// venue scene.
    pub fn from_models(models: [Option<Vec<u8>>; 3], bend: Option<Vec<u8>>) -> Option<Self> {
        let mut rods: [Option<Vec<u8>>; 3] = Default::default();
        let mut prims: [Vec<RodPrim>; 3] = Default::default();
        for (r, (slot, model)) in rods.iter_mut().zip(models).enumerate() {
            let Some(bytes) = model else {
                continue;
            };
            let Ok(tmd) = legaia_tmd::parse(&bytes) else {
                continue;
            };
            prims[r] = rod_prims(&bytes, &tmd);
            *slot = tmd.objects.first().map(|o| {
                o.vertices
                    .iter()
                    .flat_map(|v| {
                        [v.x, v.y, v.z, v._pad]
                            .into_iter()
                            .flat_map(i16::to_le_bytes)
                    })
                    .collect()
            });
        }
        if rods.iter().all(Option::is_none) {
            return None;
        }
        Some(Self { rods, bend, prims })
    }

    /// The tip vertex of rod `rod` bent by `weight` - the morph stager
    /// `FUN_8001C604` over group 0 at that slot weight, then vertex
    /// [`ROD_TIP_VERTEX`] of the staged array.
    pub fn tip(&self, rod: usize, weight: i16) -> Option<[i16; 3]> {
        self.staged(rod, weight)?.get(ROD_TIP_VERTEX).copied()
    }

    /// Every vertex of rod `rod`'s object 0 after the morph stager
    /// `FUN_8001C604` bent it by `weight` - the array the draw and the tip
    /// projection both read.
    pub fn staged(&self, rod: usize, weight: i16) -> Option<Vec<[i16; 3]>> {
        let rest = self.rods.get(rod)?.as_deref()?;
        let staged = match self.bend.as_deref() {
            Some(entry) => {
                legaia_engine_vm::vdf_morph::stage_group_morph(rest, 0, &[(entry, weight)])
            }
            None => rest.to_vec(),
        };
        let h = |v: &[u8; 8], i: usize| i16::from_le_bytes([v[i], v[i + 1]]);
        Some(
            staged
                .as_chunks::<8>()
                .0
                .iter()
                .map(|v| [h(v, 0), h(v, 2), h(v, 4)])
                .collect(),
        )
    }
}

/// The projected rod tip: the `0x801D9194` screen pair and the depth
/// `FUN_8003D368` returns (`IR3`, kept at `0x801D913C`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RodTip {
    pub sxy: (i16, i16),
    pub depth: i32,
}

/// One `MVMVA` (`sf = 1`, `lm = 0`, `cv` = a zeroed `TR`): `m * v >> 12`,
/// each component saturated to `i16`.
fn mvmva(m: &[[i32; 3]; 3], v: [i32; 3]) -> [i32; 3] {
    let row = |r: usize| {
        let mac = (m[r][0] as i64 * v[0] as i64
            + m[r][1] as i64 * v[1] as i64
            + m[r][2] as i64 * v[2] as i64)
            >> 12;
        mac.clamp(-0x8000, 0x7FFF) as i32
    };
    [row(0), row(1), row(2)]
}

/// Post-multiply `m` by one axis rotation the way the SCUS builders
/// `FUN_800461A4` / `FUN_8004629C` / `FUN_8004638C` (`RotMatrixX / Y / Z`) do:
/// each new column is one `MVMVA` of the current matrix against that column
/// of the axis rotation, read out of the retail sine table.
fn rotate(m: &mut [[i32; 3]; 3], axis: usize, angle: i32) {
    use legaia_asset::minigame_slot_scene::{cos_4096, sin_4096};
    let a = angle & 0xFFF;
    let (s, c) = (sin_4096(a), cos_4096(a));
    let one = 0x1000;
    let cols: [[i32; 3]; 3] = match axis {
        0 => [[one, 0, 0], [0, c, s], [0, -s, c]],
        1 => [[c, 0, -s], [0, one, 0], [s, 0, c]],
        _ => [[c, s, 0], [-s, c, 0], [0, 0, one]],
    };
    let out = cols.map(|col| mvmva(m, col));
    for (ci, col) in out.iter().enumerate() {
        for r in 0..3 {
            m[r][ci] = col[r];
        }
    }
}

/// The rod's GTE state for one frame: the rotation `FUN_801D1C5C` loads
/// (`SetRotMatrix` at `0x801D1EB4`) and its translation.
///
/// The matrix is the 6x base rotated by `RotMatrixX(rot_x)`, `RotMatrixY(0)`,
/// `RotMatrixZ(rot_z)` and `RotMatrixY(-yaw)` in that order
/// (`0x801D1E80..0x801D1EA8`); the translation is [`ROD_ACTOR_POS`] pushed
/// through the base (`FUN_8003D344` at `0x801D1E40`, then `SetTransMatrix`).
fn rod_transform(rot_x: i16, rot_z: i16, yaw: i32) -> ([[i32; 3]; 3], [i32; 3]) {
    let s = ROD_BASE_SCALE;
    let base = [[s, 0, 0], [0, s, 0], [0, 0, s]];
    let tr = mvmva(&base, ROD_ACTOR_POS.map(i32::from));
    let mut m = base;
    rotate(&mut m, 0, rot_x as i32);
    rotate(&mut m, 1, 0);
    rotate(&mut m, 2, rot_z as i32);
    rotate(&mut m, 1, yaw.wrapping_neg());
    (m, tr)
}

/// One `RTPS` (`sf = 1`, `lm = 0`) under [`ROD_PROJECTION_H`] with the UNR
/// divide: the screen pair, `IR3` and `SZ3`.
fn rod_rtps(m: &[[i32; 3]; 3], tr: [i32; 3], v: [i16; 3]) -> ((i16, i16), i32, i32) {
    // MAC = (TR << 12 + R * V) >> 12.
    let mac = |r: usize| {
        ((tr[r] as i64) << 12)
            + m[r][0] as i64 * v[0] as i64
            + m[r][1] as i64 * v[1] as i64
            + m[r][2] as i64 * v[2] as i64
    };
    let ir = |r: usize| (mac(r) >> 12).clamp(-0x8000, 0x7FFF);
    let sz3 = (mac(2) >> 12).clamp(0, 0xFFFF) as u16;
    let (div, _) = legaia_engine_vm::gte_divide::gte_divide(ROD_PROJECTION_H, sz3);
    let screen =
        |ir: i64, of: i32| (((div * ir) + ((of as i64) << 16)) >> 16).clamp(-0x400, 0x3FF) as i16;
    (
        (
            screen(ir(0), GTE_SCREEN_CENTRE.0),
            screen(ir(1), GTE_SCREEN_CENTRE.1),
        ),
        ir(2) as i32,
        i32::from(sz3),
    )
}

/// Project the rod-tip vertex `v` the way `FUN_801D1C5C` does: the rod's
/// matrix ([`rod_transform`]), then `RTPS` under [`ROD_PROJECTION_H`] with
/// the UNR divide.
pub fn rod_tip_screen(v: [i16; 3], rot_x: i16, rot_z: i16, yaw: i32) -> RodTip {
    let (m, tr) = rod_transform(rot_x, rot_z, yaw);
    let (sxy, depth, _) = rod_rtps(&m, tr, v);
    RodTip { sxy, depth }
}

/// The pose one rod-actor tick drew with: the two rotation terms it built
/// this frame, the yaw it read, and the morph weight.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RodPose {
    /// `RotMatrixX`'s angle: the swing plus the lift / bend pitch.
    pub rot_x: i16,
    /// `RotMatrixZ`'s angle: twice the lean.
    pub rot_z: i16,
    /// `0x801D911C`, negated into the last `RotMatrixY`.
    pub yaw: i32,
    /// The morph slot's weight - the bend's low halfword.
    pub weight: i16,
}

/// `ZSF3` the rod's triangles average under: the dispatcher loads
/// `0x555 >> _DAT_1F8003A4` (`0x80043568..0x8004357C`), the scratch byte
/// being `3` in the `minigame_fishing` save state.
pub const ROD_ZSF3: i32 = 0x555 >> 3;

/// `ZSF4` the rod's quads average under (`0x400 >> _DAT_1F8003A4`, same
/// load). With it the ordering-table bucket `OTZ >> 2` is the mean depth
/// `/ 32` - the scale the line's `IR3 >> 5` links at.
pub const ROD_ZSF4: i32 = 0x400 >> 3;

/// The handlers' near cutoff: a primitive whose `OTZ` falls below the
/// scratch halfword `0x1F80037E` is dropped (`sub s1,s2,t4; bltz` at
/// `0x80043870` / `0x80043720`). `0x10` in the `minigame_fishing` state.
pub const ROD_NEAR_OTZ: i32 = 0x10;

/// One rod primitive as retail links it: screen corners, per-corner colour
/// and the ordering-table bucket. A triangle repeats its third corner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RodFace {
    pub xy: [(i16, i16); 4],
    pub rgb: [[u8; 3]; 4],
    pub quad: bool,
    /// `OTZ >> 2` - the word index `(OTZ & 0xFFFC)` addresses in the OT.
    pub ot: u32,
}

/// `NCLIP`: the signed area of the screen triangle `(a, b, c)`.
fn nclip(a: (i16, i16), b: (i16, i16), c: (i16, i16)) -> i64 {
    let (ax, ay) = (a.0 as i64, a.1 as i64);
    let (bx, by) = (b.0 as i64, b.1 as i64);
    let (cx, cy) = (c.0 as i64, c.1 as i64);
    ax * by + bx * cy + cx * ay - ax * cy - bx * ay - cx * by
}

/// The rod model drawn: object 0 of rod `rod`, bent by the pose's morph
/// weight, every vertex through the rod's matrix under `H = 0xDC`, then each
/// primitive through the bank-0 handler of its kind the way
/// `FUN_80043390` runs them for this call (the rod's `+0x74` is the
/// allocator's `0x00808080` - `sw v1,0x74(s0)` at `0x80020F3C` - and its
/// `+0x78` is `0`, so no blend bank, no depth cue and the single-sided
/// cull mask `0xFFFFFFFF`):
///
/// - a **triangle** (kinds 12 / 14, `0x80043658` / `0x80043B58`) is culled
///   when `NCLIP < 0` (`bltz` at `0x80043700`);
/// - a **quad** (kinds 13 / 15, `0x80043768` / `0x80043C6C`) is kept when
///   `NCLIP(v0, v1, v2) > 0`, else only when the second `NCLIP` over
///   `(v1, v2, v3)` after the fourth `RTPS` is negative
///   (`blez` at `0x80043818`, `bgez` at `0x8004384C`);
/// - `AVSZ3` / `AVSZ4` under [`ROD_ZSF3`] / [`ROD_ZSF4`] give `OTZ`, a
///   primitive below [`ROD_NEAR_OTZ`] is dropped, and the packet links at
///   `OTZ >> 2`.
///
/// Faces come back in packet order - the order the handlers `AddPrim` them.
///
/// PORT: FUN_801d1c5c (the model draw: the `FUN_80043390` call at
/// `0x801D1FF4` over the staged object 0)
pub fn rod_faces(mesh: &RodMesh, rod: usize, pose: RodPose) -> Vec<RodFace> {
    let Some(verts) = mesh.staged(rod, pose.weight) else {
        return Vec::new();
    };
    let Some(prims) = mesh.prims.get(rod) else {
        return Vec::new();
    };
    let (m, tr) = rod_transform(pose.rot_x, pose.rot_z, pose.yaw);
    let projected: Vec<((i16, i16), i32)> = verts
        .iter()
        .map(|&v| {
            let (sxy, _, sz) = rod_rtps(&m, tr, v);
            (sxy, sz)
        })
        .collect();
    let mut out = Vec::with_capacity(prims.len());
    for p in prims {
        let Some(c) = p
            .verts
            .iter()
            .map(|&i| projected.get(usize::from(i)).copied())
            .collect::<Option<Vec<_>>>()
        else {
            continue;
        };
        let xy = [c[0].0, c[1].0, c[2].0, c[3].0];
        let otz = if p.quad {
            let front = nclip(xy[0], xy[1], xy[2]) > 0 || nclip(xy[1], xy[2], xy[3]) < 0;
            if !front {
                continue;
            }
            ((c[0].1 + c[1].1 + c[2].1 + c[3].1) as i64 * ROD_ZSF4 as i64) >> 12
        } else {
            if nclip(xy[0], xy[1], xy[2]) < 0 {
                continue;
            }
            ((c[0].1 + c[1].1 + c[2].1) as i64 * ROD_ZSF3 as i64) >> 12
        };
        let otz = otz.clamp(0, 0xFFFF) as i32;
        if otz < ROD_NEAR_OTZ {
            continue;
        }
        out.push(RodFace {
            xy,
            rgb: p.rgb,
            quad: p.quad,
            ot: (otz >> 2) as u32,
        });
    }
    out
}

/// The rod-side inputs one in-water frame of the lure tick feeds the rod
/// (`FUN_801D26CC` state 2), off the held packed pad `_DAT_8007B850`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RodDrive {
    /// A fish is on (`_DAT_801D91B4`) as the frame starts.
    pub hooked: bool,
    /// The held packed pad word.
    pub held: u32,
}

/// The fishing rod as an advancing object: `FUN_801D1C5C` per frame, plus the
/// rod globals the lure tick writes. Spawned by the cast lock; retired when
/// the recover swing finishes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RodActor {
    /// `0x801D91AC`.
    pub swing: RodSwing,
    /// `0x801D9134`, the swing's pitch term.
    pub bob: i16,
    /// `0x801D9150`: the bend, which is both a pitch term and the morph
    /// slot's weight.
    pub bend: i32,
    /// `0x801D914C`: the D-pad-down pitch lift.
    pub lift: i32,
    /// `0x801D9140`: the roll toward the held D-pad side.
    pub lean: i32,
    /// `0x801D9144`: the roll's target.
    pub lean_target: i32,
    /// `0x801D911C`: the yaw toward the fish, `3 * (tip.x - fish.x)`.
    pub yaw: i32,
    /// This frame's projected tip, when the rod mesh resolved.
    pub tip: Option<RodTip>,
    /// The pose this frame's tick drew the model with ([`rod_faces`]);
    /// `None` until the first tick.
    pub pose: Option<RodPose>,
}

impl RodActor {
    /// The rod the cast lock spawns: the cast swing, the seeded bend, every
    /// other term at the run-loop init's zero.
    pub fn cast() -> Self {
        Self {
            swing: RodSwing::Cast,
            bob: 0,
            bend: ROD_BEND_AT_CAST,
            lift: 0,
            lean: 0,
            lean_target: 0,
            yaw: 0,
            tip: None,
            pose: None,
        }
    }

    /// Start the recover swing (a landed catch or a snapped line).
    pub fn recover(&mut self) {
        self.swing = RodSwing::Recover;
    }

    /// Whether the actor has retired.
    pub fn retired(&self) -> bool {
        self.swing == RodSwing::Done
    }

    /// One frame of the rod actor: the swing, the pose off this frame's
    /// globals, the tip projection, then the bend and lift bleeding off.
    ///
    /// PORT: FUN_801d1c5c
    pub fn tick(&mut self, mesh: Option<&RodMesh>, rod: usize, frame_step: i32) {
        let fs = frame_step.max(1);
        let step = (ROD_BOB_STEP * fs) as i16;
        match self.swing {
            RodSwing::Cast => {
                self.bob = self.bob.wrapping_sub(step);
                if self.bob < ROD_BOB_FLOOR {
                    self.bob = ROD_BOB_FLOOR;
                    self.swing = RodSwing::Hold;
                }
            }
            RodSwing::Recover => {
                self.bob = self.bob.wrapping_add(step);
                if self.bob > ROD_BOB_CEILING {
                    self.bob = ROD_BOB_CEILING;
                    self.swing = RodSwing::Done;
                }
            }
            RodSwing::Hold | RodSwing::Done => {}
        }
        // Pitch: the swing plus a sixteenth of (lift + bend / 2), both
        // divisions truncating toward zero (`0x801D1D7C..0x801D1DBC`).
        let half = (self.bend + ((self.bend as u32) >> 31) as i32) >> 1;
        let mut sum = self.lift.wrapping_add(half);
        if sum < 0 {
            sum += 0xF;
        }
        let rot_x = (self.bob as i32).wrapping_add(sum >> 4) as i16;
        let rot_z = (self.lean << 1) as i16;
        // The morph slot's weight is the bend's low halfword (`lhu` into
        // `+0xA0` at `0x801D1EE0`).
        let weight = self.bend as u16 as i16;
        self.pose = Some(RodPose {
            rot_x,
            rot_z,
            yaw: self.yaw,
            weight,
        });
        self.tip = mesh
            .and_then(|m| m.tip(rod, weight))
            .map(|v| rod_tip_screen(v, rot_x, rot_z, self.yaw));
        // The lift bleeds toward zero at `0x20` a frame delta, the bend down to
        // zero at `0x60` (`0x801D1EEC..0x801D1F7C`).
        let lift_step = 0x20 * fs;
        if self.lift < 0 {
            self.lift = (self.lift + lift_step).min(0);
        } else if self.lift > 0 {
            self.lift = (self.lift - lift_step).max(0);
        }
        if self.bend > 0 {
            self.bend = (self.bend - 0x60 * fs).max(0);
        }
    }

    /// The lure tick's rod writes for one in-water frame (`FUN_801D26CC`
    /// state 2): D-pad down lifts the rod (`0x801D2AB4..0x801D2AFC`), a held
    /// reel or a fish on bends it (`0x801D2B04..0x801D2C30`), and the D-pad
    /// sides roll it (`0x801D382C..0x801D395C`).
    ///
    /// The bend clamps read `_DAT_801D90F4`, which is `0` from the landing to
    /// the snap, so they are the bare `0x1000` / `0x1800`. Returns whether a
    /// hooked rod's bend ran past its `0x1800` cap this frame - the gate of
    /// the creak cue (`0x801D2B84..0x801D2BAC`), whose timer is the
    /// session's ([`crate::fishing::PondSession::take_rod_creaks`]).
    ///
    /// PORT: FUN_801d26cc (state 2: the rod lift, bend and roll writes)
    pub fn drive(&mut self, d: RodDrive, frame_step: i32) -> bool {
        let mut over_cap = false;
        let fs = frame_step.max(1);
        if d.held & 0x4000 != 0 {
            self.lift = (self.lift + 0x60 * fs).min(0x1000);
        }
        if d.held & 0xC0 != 0 || d.hooked {
            self.bend += 0x100 * fs;
            if d.held & 0x80 != 0 {
                self.bend += 0x80 * fs;
            }
            let cap = if d.hooked { 0x1800 } else { 0x1000 };
            if self.bend > cap {
                over_cap = d.hooked;
                self.bend = cap;
            }
        }
        let decay = 4 * fs;
        if self.lean_target > 0 {
            self.lean_target = (self.lean_target - decay).max(0);
        } else if self.lean_target < 0 {
            self.lean_target = (self.lean_target + decay).min(0);
        }
        if d.held & 0x2000 != 0 {
            self.lean_target = 0x100;
        }
        if d.held & 0x8000 != 0 {
            self.lean_target = -0x100;
        }
        let ease = 0x10 * fs;
        if self.lean < self.lean_target {
            self.lean = (self.lean + ease).min(self.lean_target);
        } else if self.lean > self.lean_target {
            self.lean = (self.lean - ease).max(self.lean_target);
        }
        over_cap
    }
}

/// One frame's fishing line, clipped: retail's `LINE_G2` packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FishingLine {
    /// The fish end (`+0x08`), in retail 320x240 screen space.
    pub fish: (i16, i16),
    /// The rod end (`+0x10`).
    pub rod: (i16, i16),
    /// [`LINE_FISH_RGB`].
    pub fish_rgb: [u8; 3],
    /// [`LINE_ROD_RGB`].
    pub rod_rgb: [u8; 3],
    /// The ordering-table bucket, off the rod tip's depth.
    pub ot: u32,
}

/// Build one frame's line from the fish's projected point and the rod tip:
/// the packet's two endpoints through [`clip_segment_2d`] against
/// [`LINE_CLIP_RECT`] (`jal 0x801D56E4` at `0x801D3D00`), linked at the tip's
/// depth bucket.
pub fn fishing_line(fish: (i16, i16), tip: RodTip) -> FishingLine {
    let (mut p, mut q) = (fish, tip.sxy);
    clip_segment_2d(&mut p, &mut q, LINE_CLIP_RECT);
    FishingLine {
        fish: p,
        rod: q,
        fish_rgb: LINE_FISH_RGB,
        rod_rgb: LINE_ROD_RGB,
        ot: (tip.depth.max(0) as u32) >> LINE_OT_SHIFT,
    }
}

// --- Walk-grid overhead probe (FUN_801D7030) -------------------------------

/// Bytes per row of the `+0x4000` sub-cell grid (`(x_cell / 2) & 0x7F`
/// column, `(z_cell / 2) & 0x7F` row, `row * 0x80 + column`).
pub const WALK_GRID_PITCH: usize = 0x80;

/// Rows in the same grid.
pub const WALK_GRID_ROWS: usize = 0x80;

/// Probe the per-scene walkability grid's **high** nibble.
///
/// `grid` is the byte block at `*(_DAT_1F8003EC) + 0x4000` - the same block
/// the field overlay's per-axis collision reads (`FUN_801CFE4C`, see
/// `docs/subsystems/field-locomotion.md`), which takes the byte's *low*
/// nibble. This probe takes the high one, so the two read the two 4-bit
/// masks packed into each grid byte independently.
///
/// The two coordinate conversions are **not** the same ladder, which is the
/// thing to keep when re-deriving this:
///
/// - `z` truncates toward zero (`z < 0` is biased `+0x3F` before the
///   arithmetic shift) and is then biased **`+2` sub-cells**.
/// - `x` rounds up (`(x + 0x3F) >> 6`, no sign test - the bias is
///   unconditional) and is then biased **`-1` sub-cell**.
///
/// The byte is `row * 0x80 + column` with `column` from **x** and `row` from
/// **z**; the sub-cell bit is `1 << ((x_cell & 1) + 2 * (z_cell & 1))`.
///
/// PORT: FUN_801d7030
// Wired through [`LureActor::probe`], which both fishing hosts reach: the
// browser minigames page drives it inside the venue-faithful
// [`crate::fishing::PondSession`], and the play window's
// `window/minigames.rs::tick_fishing_actors` drifts its own lure actor with
// it. The probe's consequence is the one retail applies at `0x801D2E18` - a
// `frame_delta << 11` push of the lure's `x` accumulator, signed by the low
// bit of the persistent cast counter `_DAT_80084460` - not a water test.
pub fn walk_grid_overhead(grid: &[u8], x: i32, z: i32) -> bool {
    let zc = (if z < 0 { z + 0x3F } else { z } >> 6) + 2;
    let xc = ((x + 0x3F) >> 6) - 1;
    // `srl 31; addu; sra 1` - divide by two truncating toward zero, which is
    // what Rust's `/` already does on a signed integer.
    let column = (xc / 2) & 0x7F;
    let row = (zc / 2) & 0x7F;
    let idx = row as usize * WALK_GRID_PITCH + column as usize;
    let Some(byte) = grid.get(idx) else {
        return false;
    };
    let bit = 1u8 << ((xc & 1) + 2 * (zc & 1)) as u32;
    (byte >> 4) & bit != 0
}

// --- The cast lure (FUN_801CF3BC case 0x14 + FUN_801D26CC's probe pair) ----

/// Radius the cast lure spawns at, ahead of the angler along its facing
/// (`li a1,0xc8` feeding the polar helper at `0x801CFC50`).
pub const LURE_CAST_RADIUS: i32 = 200;

/// The venue anchor both fishing hosts stand the angler on.
///
/// This is port glue, not a retail constant: retail reads the angler actor's
/// own `+0x14`/`+0x18`, and neither host spawns a field actor for the angler.
/// The play window already seeds its wander actor and its tracked-point
/// readout from this pair, so the lure casts from the same place the rest of
/// the venue is measured against.
pub const VENUE_ANCHOR: (i16, i16) = (0x400, 0x400);

/// Shift the walk-grid probe pushes the lure's `x` accumulator by per frame
/// delta (`sll v0,v0,0xb` at `0x801D2E3C` / `0x801D2E50`), in the
/// accumulator's 24.8 fixed point - eight world units per frame delta.
pub const LURE_DRIFT_SHIFT: u32 = 11;

/// World units per `.MAP` tile - the shift both of the lure's probes take
/// their tile coordinate with (`sra a2,v1,0x17` over a sign-extended `i16`).
pub const LURE_TILE_SHIFT: u32 = 7;

/// The `+0x8000` cell word's **water** bit (`andi v1,v1,0x4000` at
/// `0x801D3374`).
///
/// This is a bit of the per-tile object-index halfword, not an offset: the
/// same word's low nine bits are the object index the bite tick keeps
/// (`andi s3,v0,0x1ff` at `0x801D2E14`). It is not the `+0x4000` walk grid
/// [`walk_grid_overhead`] reads - the two share only the hex digits.
pub const CELL_WATER_BIT: u16 = 0x4000;

/// What one lure frame resolved out of the venue map.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LureProbe {
    /// The `+0x8000` cell word said this tile is water.
    pub water: bool,
    /// Strike-credit addend from the tile's water class (`s2`; `0` outside
    /// every class).
    pub countdown_bonus: i32,
    /// The class's fish weight (`s4`; [`WATER_TILE_DEFAULT`]'s `10` outside
    /// every class).
    pub weight: i32,
    /// This frame's walk-grid drift of the lure's `x`, in world units
    /// (signed; `0` when the probe found no overhead bit).
    pub drift: i32,
}

/// The cast lure as an advancing object.
///
/// Retail spawns it in the cast arm of the fishing SM (`FUN_801CF3BC` case
/// `0x14`): the angler actor's `xz` minus the polar offset of its `+0x26`
/// facing at radius [`LURE_CAST_RADIUS`], latched into the tracked-point
/// pair at `0x801D918C` and into the 24.8 `x` accumulator at `0x801D9174`.
/// The bite tick `FUN_801D26CC` then probes the tile under it every frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LureActor {
    /// `0x801D9174` - the lure's world `x` in 24.8 fixed point, the form the
    /// drift is applied in.
    pub x_fixed: i32,
    /// `0x801D9190` - the lure's world `z`, the tracked triple's third
    /// halfword. `0x801D918E`, between it and the `x`, is the lure's
    /// **height** (`actor + 0x16` less `0x80`, stored by `sh $a0, 2($a2)`
    /// at `0x801CFCEC`) - the same `+0x14 / +0x16 / +0x18` = x / footing /
    /// z layout the field player record uses. The 24.8 accumulator for this
    /// `z` is `0x801D917C`, the third of the `<< 8` triple.
    pub z: i16,
}

impl LureActor {
    /// Spawn the lure for a cast from `(anchor_x, anchor_z)` facing `facing`.
    ///
    /// `frame_step` is the scratchpad frame delta (`0x1F800393`), which retail
    /// passes as the polar helper's **scale** on this arm as well as on the
    /// per-frame ones - so a 60 Hz frame (`1`) puts the lure exactly
    /// [`LURE_CAST_RADIUS`] units out - and the spawn is a subtraction:
    /// `lure.xz = anchor.xz - polar(facing, 200)` (`subu v1,v1,v0` at
    /// `0x801CFCD4`).
    ///
    /// Returns `None` only when the quadrature tables are too short for the
    /// masked angle, which the materialised pair never is.
    ///
    /// PORT: FUN_801cf3bc (case 0x14 - the cast lure spawn)
    pub fn cast(anchor_x: i16, anchor_z: i16, facing: i16, frame_step: i32) -> Option<Self> {
        let (sin, cos) = crate::minigame_floor::polar_tables();
        let (dx, dz) = crate::minigame_floor::polar_offset(
            facing as u32,
            LURE_CAST_RADIUS,
            frame_step.max(1),
            sin,
            cos,
        )?;
        let x = (anchor_x as i32 - dx) as i16;
        let z = (anchor_z as i32 - dz) as i16;
        Some(Self {
            x_fixed: (x as i32) << 8,
            z,
        })
    }

    /// The lure's world `x` (the accumulator's integer part).
    pub fn x(&self) -> i16 {
        (self.x_fixed >> 8) as i16
    }

    /// Probe the venue map under the lure and apply this frame's drift.
    ///
    /// `map` is the scene's `.MAP` buffer (`*_DAT_1F8003EC`), `region` its
    /// parsed `+0x10000` region table, `cast_counter` the persistent lifetime
    /// cast count (`_DAT_80084460`, whose low bit picks the drift's sign at
    /// `0x801D2E28`) and `frame_step` the scratchpad frame delta.
    ///
    /// Two independent reads, in retail's order:
    ///
    /// 1. [`walk_grid_overhead`] over the `+0x4000` grid's **high** nibble -
    ///    a hit drifts the `x` accumulator by `frame_step << 11`.
    /// 2. the `+0x8000` cell word's [`CELL_WATER_BIT`]; on water, the region
    ///    walk ([`legaia_engine_vm::field_regions::refresh_region_attributes`] - retail's
    ///    `FUN_800180EC`, called at `0x801D3384`, writing `_DAT_8007B8F4`)
    ///    yields the region-kind mask [`water_tile_class`] classifies.
    pub fn probe(
        &mut self,
        map: &[u8],
        region: Option<&legaia_engine_vm::field_regions::RegionTable<'_>>,
        cast_counter: i32,
        frame_step: i32,
    ) -> LureProbe {
        let fs = frame_step.max(1);
        let x = self.x();
        let mut out = LureProbe {
            weight: WATER_TILE_DEFAULT.1,
            ..Default::default()
        };
        // `walk_grid_overhead` indexes from the grid's own base, so it takes
        // the `+0x4000` block rather than the whole map.
        let walk = map
            .get(legaia_engine_vm::field_regions::MAP_WALK_GRID_OFFSET..)
            .unwrap_or(&[]);
        if walk_grid_overhead(walk, x as i32, self.z as i32) {
            let push = fs << LURE_DRIFT_SHIFT;
            let signed = if cast_counter & 1 != 0 { push } else { -push };
            self.x_fixed = self.x_fixed.saturating_add(signed);
            out.drift = signed >> 8;
        }
        let tile_x = (x as i32) >> LURE_TILE_SHIFT;
        let tile_z = (self.z as i32) >> LURE_TILE_SHIFT;
        let grid = crate::minigame_floor::FloorGrid::new(map);
        if grid.cell(tile_x, tile_z) & CELL_WATER_BIT == 0 {
            return out;
        }
        out.water = true;
        let (mask, _attrs) = legaia_engine_vm::field_regions::refresh_region_attributes(
            region, tile_x, tile_z, false,
        );
        if let Some((bonus, weight)) = water_tile_class(mask) {
            out.countdown_bonus = bonus;
            out.weight = weight;
        }
        out
    }
}

// --- Tracked-point separation (FUN_801D765C) -------------------------------

/// Runtime VA of the first tracked 2-D point (`+0` = x, `+4` = y).
pub const TRACKED_POINT_A_VA: u32 = 0x801D_9184;

/// Runtime VA of the second (`+0` = x, `+4` = y).
pub const TRACKED_POINT_B_VA: u32 = 0x801D_918C;

/// World units per sub-cell - the shift the separation is reported in.
pub const SUBCELL_SHIFT: u32 = 6;

/// Separation of the overlay's two tracked 2-D points, in sub-cells.
///
/// Retail takes no arguments: it reads `(i16 x, i16 y)` out of
/// [`TRACKED_POINT_A_VA`] / [`TRACKED_POINT_B_VA`] - the same pair
/// `FUN_801D26CC` feeds to the bearing helper `FUN_80019B28` - takes the
/// absolute difference of each component, squares and sums them, and hands
/// the sum to the SCUS normalise helper `FUN_8005AF0C` (`sqrt`).
///
/// The result is arithmetic-shifted right by [`SUBCELL_SHIFT`] and a negative
/// result is clamped to zero. `>> 6` is the **sub-cell** step (64 units), not
/// the 128-unit tile: the same shift `FUN_801D7030` uses to index the grid.
///
/// PORT: FUN_801d765c
// Wired: the play window's fishing developer readout computes the wander
// actor's separation from the venue anchor through this (with an integer
// square root standing in for the SCUS normalise helper `FUN_8005AF0C`,
// which the port takes as a closure rather than owning) - see
// `window/hud.rs`.
pub fn tracked_point_separation(
    a: (i16, i16),
    b: (i16, i16),
    sqrt: impl FnOnce(i32) -> i32,
) -> i32 {
    let dx = (a.0 as i32 - b.0 as i32).abs();
    let dy = (a.1 as i32 - b.1 as i32).abs();
    (sqrt(dx * dx + dy * dy) >> SUBCELL_SHIFT).max(0)
}

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
    fn only_the_far_band_overrides_the_credit() {
        assert_eq!(bite_credit_override(199), Some(BITE_FAR_CREDIT));
        assert_eq!(bite_credit_override(200), None);
        assert_eq!(bite_credit_override(1000), None);
        // The override and the modulus flip on the same comparison.
        for d in 0..400 {
            assert_eq!(
                bite_credit_override(d).is_some(),
                bite_interval(d, false) == BITE_INTERVAL_FAR
            );
        }
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

    const SCREEN: ClipRect = ClipRect {
        x_min: 0,
        y_min: 0,
        x_max: 0x140,
        y_max: 0xF0,
    };

    #[test]
    fn a_segment_inside_the_window_is_untouched() {
        let (mut p, mut q) = ((10, 20), (300, 200));
        clip_segment_2d(&mut p, &mut q, SCREEN);
        assert_eq!((p, q), ((10, 20), (300, 200)));
    }

    #[test]
    fn a_crossing_endpoint_lands_on_the_bound_at_the_true_intersection() {
        // p is left of x_min = 0; the segment crosses at x = 0, y = 50.
        let (mut p, mut q) = ((-100, 0), (100, 100));
        clip_segment_2d(&mut p, &mut q, SCREEN);
        assert_eq!(p, (0, 50));
        assert_eq!(q, (100, 100));
    }

    #[test]
    fn each_bound_moves_the_endpoint_that_is_outside_it() {
        // q is past x_max; the crossing sits halfway.
        let (mut p, mut q) = ((0x100, 0), (0x180, 0x40));
        clip_segment_2d(&mut p, &mut q, SCREEN);
        assert_eq!(p, (0x100, 0));
        assert_eq!(q, (0x140, 0x20));
    }

    #[test]
    fn a_segment_wholly_outside_one_bound_is_left_alone() {
        // Retail's arm needs one endpoint strictly below the bound and the
        // other strictly above it, so a segment entirely left of x_min is
        // untouched rather than collapsed onto the edge.
        let (mut p, mut q) = ((-200, 10), (-100, 20));
        clip_segment_2d(&mut p, &mut q, SCREEN);
        assert_eq!((p, q), ((-200, 10), (-100, 20)));
    }

    #[test]
    fn the_vertical_arms_swap_the_components() {
        let rect = ClipRect {
            x_min: -1000,
            y_min: 0,
            x_max: 1000,
            y_max: 100,
        };
        let (mut p, mut q) = ((0, -100), (100, 100));
        clip_segment_2d(&mut p, &mut q, rect);
        // p rides up to y_min = 0 (halfway, x = 50); q rides down to y_max.
        assert_eq!(p, (50, 0));
        assert_eq!(q.1, 100);
    }

    #[test]
    fn the_overhead_probe_reads_the_high_nibble_only() {
        let mut grid = vec![0u8; WALK_GRID_PITCH * WALK_GRID_ROWS];
        // x = 64 -> xc = ((64 + 63) >> 6) - 1 = 0; z = 0 -> zc = 0 + 2 = 2.
        // column = 0, row = 1, bit = 1 << (0 + 2*0) = 1.
        let idx = WALK_GRID_PITCH;
        grid[idx] = 0x01; // low nibble only - the field collision's mask
        assert!(!walk_grid_overhead(&grid, 64, 0));
        grid[idx] = 0x10; // the same sub-cell in the high nibble
        assert!(walk_grid_overhead(&grid, 64, 0));
    }

    #[test]
    fn the_overhead_probe_selects_the_sub_cell_by_parity() {
        let mut grid = vec![0u8; WALK_GRID_PITCH * WALK_GRID_ROWS];
        // xc = 1 (x = 128), zc = 3 (z = 64) -> both odd -> bit 8.
        // column = 0, row = 1.
        grid[WALK_GRID_PITCH] = 0x80;
        assert!(walk_grid_overhead(&grid, 128, 64));
        // Same byte, wrong parity pair (xc = 0, zc = 2 -> bit 1).
        assert!(!walk_grid_overhead(&grid, 64, 0));
    }

    #[test]
    fn an_out_of_range_probe_reports_clear_instead_of_panicking() {
        assert!(!walk_grid_overhead(&[], 0, 0));
    }

    #[test]
    fn the_separation_is_a_sub_cell_count() {
        let sqrt = |v: i32| (v as f64).sqrt() as i32;
        // 64 units apart on one axis is exactly one sub-cell.
        assert_eq!(tracked_point_separation((0, 0), (64, 0), sqrt), 1);
        assert_eq!(tracked_point_separation((0, 0), (63, 0), sqrt), 0);
        // The sign of each component is dropped before the square.
        assert_eq!(
            tracked_point_separation((0, 0), (-640, 0), sqrt),
            tracked_point_separation((0, 0), (640, 0), sqrt)
        );
        // A negative normalise result clamps to zero rather than wrapping.
        assert_eq!(tracked_point_separation((0, 0), (100, 100), |_| -1), 0);
    }

    #[test]
    fn the_wander_actor_rolls_on_dwell_expiry_and_holds_its_place() {
        let mut w = FishWander::new(0x400, 0, 0x400);
        // First tick: the dwell is due, so the roll happens immediately.
        let draws = [0u32, 50, 0, 6, 1];
        let mut i = 0;
        let rolled = w
            .tick(0, || {
                let v = draws[i % draws.len()];
                i += 1;
                v
            })
            .expect("first tick re-rolls");
        assert_eq!(rolled.ripple_variant, 1);
        // The dwell now holds; no re-roll, and the actor stays put - the
        // roll names a ripple point, it does not move the angler.
        let (x0, z0) = (w.x, w.z);
        assert!(w.tick(0, || 0).is_none());
        assert_eq!((w.x, w.z), (x0, z0));
        // The facing steps + clamps off the held packed pad.
        let f0 = w.facing;
        w.tick(PACK_RIGHT, || 0);
        assert_eq!(w.facing, f0 + FACING_STEP);
    }

    #[test]
    fn the_line_actor_arms_tracks_and_celebrates() {
        let mut line = LineActorSim::hooked();
        // Arm fires the hook cue and steps to Attach.
        let f = line.tick(1);
        assert_eq!(f.cue, Some(HOOK_CUE));
        assert_eq!(line.raw, 1);
        // Attach copies (host-side) and steps to Track; Track holds.
        line.tick(1);
        assert_eq!(line.raw, 2);
        assert_eq!(line.tick(1), LineActorFrame::default());
        // Landing enters the celebrate arm; the first stage frame fires the
        // celebrate cue + every unlocked burst, and the last hands back.
        line.land(700);
        let mut fired = None;
        let mut done = false;
        for _ in 0..CELEBRATION_STAGE_FRAMES.2 + 2 {
            let f = line.tick(1);
            if f.cue.is_some() {
                assert!(fired.is_none(), "the stage cue fires once");
                fired = Some(f.bursts.len());
            }
            done |= f.done;
        }
        // Score 700 clears the 200 + 600 tiers only.
        assert_eq!(fired, Some(2));
        assert!(done);
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

    /// A 42-vertex rod whose vertex [`ROD_TIP_VERTEX`] sits at the disc
    /// rod's tip, `(0, -138, 0)`, and a one-record bend moving that vertex by
    /// `(0, 6, 45)` at full weight - the shape of `other1`'s rods and VDF
    /// sub-entry 0, synthesised so no disc bytes are needed.
    fn synthetic_rod_mesh() -> RodMesh {
        let mut rest = vec![0u8; 42 * 8];
        let o = ROD_TIP_VERTEX * 8;
        rest[o + 2..o + 4].copy_from_slice(&(-138i16).to_le_bytes());
        let mut bend = Vec::new();
        for w in [1u32, 0, ROD_TIP_VERTEX as u32, 1] {
            bend.extend_from_slice(&w.to_le_bytes());
        }
        for c in [0i16, 6, 45, 0] {
            bend.extend_from_slice(&c.to_le_bytes());
        }
        RodMesh {
            rods: [Some(rest.clone()), Some(rest.clone()), Some(rest)],
            bend: Some(bend),
            ..Default::default()
        }
    }

    #[test]
    fn the_rod_tip_bends_by_the_weighted_vdf_delta() {
        let m = synthetic_rod_mesh();
        assert_eq!(m.tip(0, 0), Some([0, -138, 0]));
        assert_eq!(m.tip(1, 0x1000), Some([0, -132, 45]));
        // `(0x800 * 45) >> 12` = 22: the GPF blend truncates.
        assert_eq!(m.tip(2, 0x800), Some([0, -135, 22]));
        assert_eq!(m.tip(3, 0), None, "three rods");
    }

    #[test]
    fn an_unrotated_rod_projects_its_tip_straight_up_the_centre_line() {
        // R = 6I, TR = 6 * (0, 0x46, 0x64) = (0, 420, 600): the tip lands at
        // view (0, 420 - 828, 600), i.e. screen y = 114 + 220 * -408 / 600.
        let t = rod_tip_screen([0, -138, 0], 0, 0, 0);
        assert_eq!(t.sxy, (160, -36));
        assert_eq!(t.depth, 600);
    }

    #[test]
    fn the_cast_pose_brings_the_tip_into_the_clip_window() {
        let t = rod_tip_screen([0, -138, 0], ROD_BOB_FLOOR, 0, 0);
        let r = LINE_CLIP_RECT;
        assert!(
            (r.x_min..=r.x_max).contains(&t.sxy.0) && (r.y_min..=r.y_max).contains(&t.sxy.1),
            "{t:?}"
        );
    }

    #[test]
    fn the_yaw_swings_the_tip_to_mirrored_sides() {
        // `RotMatrixY(-yaw)` is the last factor, so it acts on the vertex
        // first - about the rod's own axis. A straight rod's tip sits on that
        // axis and does not move; the bend's `z` lean is what the yaw swings.
        let straight = rod_tip_screen([0, -138, 0], ROD_BOB_FLOOR, 0, 0x100);
        assert_eq!(straight, rod_tip_screen([0, -138, 0], ROD_BOB_FLOOR, 0, 0));
        let l = rod_tip_screen([0, -132, 45], ROD_BOB_FLOOR, 0, 0x100);
        let r = rod_tip_screen([0, -132, 45], ROD_BOB_FLOOR, 0, -0x100);
        assert!(l.sxy.0 != 160 && r.sxy.0 != 160);
        assert!((l.sxy.0 - 160).signum() == -(r.sxy.0 - 160).signum());
        assert!(((l.sxy.0 - 160).abs() - (r.sxy.0 - 160).abs()).abs() <= 1);
        assert_eq!(l.sxy.1, r.sxy.1);
    }

    #[test]
    fn the_line_clips_the_fish_end_and_links_at_the_tip_depth() {
        let tip = RodTip {
            sxy: (160, 100),
            depth: 640,
        };
        // The fish is off the left edge: its end rides onto x = 0 along the
        // segment, the rod end stays put.
        let l = fishing_line((-160, 200), tip);
        assert_eq!(l.rod, (160, 100));
        assert_eq!(l.fish, (0, 150));
        assert_eq!((l.fish_rgb, l.rod_rgb), (LINE_FISH_RGB, LINE_ROD_RGB));
        assert_eq!(l.ot, 640 >> 5);
        // An on-screen pair passes through untouched.
        let l = fishing_line((40, 180), tip);
        assert_eq!((l.fish, l.rod), ((40, 180), (160, 100)));
    }

    /// A four-vertex square in the rod's local `xy` plane with three
    /// primitives over it: a quad wound toward the camera, the same quad
    /// wound away, and a triangle.
    fn square_rod_mesh() -> RodMesh {
        let mut rest = Vec::new();
        for (x, y) in [(-10i16, -10i16), (10, -10), (-10, 10), (10, 10)] {
            for c in [x, y, 0, 0] {
                rest.extend_from_slice(&c.to_le_bytes());
            }
        }
        let red = [[0x80, 0x28, 0x28]; 4];
        RodMesh {
            rods: [Some(rest), None, None],
            bend: None,
            prims: [
                vec![
                    RodPrim {
                        verts: [0, 1, 2, 3],
                        quad: true,
                        rgb: red,
                    },
                    RodPrim {
                        verts: [1, 0, 3, 2],
                        quad: true,
                        rgb: red,
                    },
                    RodPrim {
                        verts: [0, 1, 2, 2],
                        quad: false,
                        rgb: red,
                    },
                ],
                Vec::new(),
                Vec::new(),
            ],
        }
    }

    #[test]
    fn the_rod_model_culls_its_back_faces_and_buckets_by_mean_depth() {
        let pose = RodPose {
            rot_x: 0,
            rot_z: 0,
            yaw: 0,
            weight: 0,
        };
        let faces = rod_faces(&square_rod_mesh(), 0, pose);
        // The away-wound quad fails both NCLIP halves; the other two draw.
        assert_eq!(faces.len(), 2, "{faces:?}");
        assert!(faces[0].quad && !faces[1].quad, "packet order is kept");
        // Every corner is the tip projection of its vertex.
        for (i, v) in [(-10i16, -10i16), (10, -10), (-10, 10), (10, 10)]
            .into_iter()
            .enumerate()
        {
            assert_eq!(faces[0].xy[i], rod_tip_screen([v.0, v.1, 0], 0, 0, 0).sxy);
        }
        // Depth 600 at every corner: AVSZ4 = 4 * 600 * 0x80 >> 12 = 75,
        // AVSZ3 = 3 * 600 * 0xAA >> 12 = 74; both link at `OTZ >> 2` = 18 -
        // the line's `600 >> 5`.
        assert_eq!((faces[0].ot, faces[1].ot), (18, 18));
        assert_eq!(faces[0].ot, 600 >> 5);
        // A missing rod draws nothing.
        assert!(rod_faces(&square_rod_mesh(), 1, pose).is_empty());
    }

    #[test]
    fn the_rod_actor_records_the_pose_it_drew_with() {
        let mesh = synthetic_rod_mesh();
        let mut rod = RodActor::cast();
        assert_eq!(rod.pose, None);
        rod.tick(Some(&mesh), 0, 1);
        let pose = rod.pose.expect("a pose after the first tick");
        // First cast frame: bob -0x40, pitch (0 + 0x1000 / 2) / 16 = 0x80.
        assert_eq!(pose.rot_x, -0x40 + 0x80);
        assert_eq!(pose.weight, ROD_BEND_AT_CAST as i16, "the pre-bleed bend");
        let tip = mesh.tip(0, pose.weight).unwrap();
        assert_eq!(
            rod.tip,
            Some(rod_tip_screen(tip, pose.rot_x, pose.rot_z, pose.yaw))
        );
    }

    #[test]
    fn the_rod_swings_down_on_the_cast_and_retires_after_the_recover() {
        let mesh = synthetic_rod_mesh();
        let mut rod = RodActor::cast();
        let mut frames = 0;
        while rod.swing == RodSwing::Cast {
            rod.tick(Some(&mesh), 0, 1);
            frames += 1;
            assert!(frames < 100);
        }
        // 700 / 0x40 rounds up to eleven frames.
        assert_eq!(frames, 11);
        assert_eq!((rod.swing, rod.bob), (RodSwing::Hold, ROD_BOB_FLOOR));
        assert!(rod.tip.is_some());
        // The cast's seeded bend bleeds off at 0x60 a frame.
        assert_eq!(rod.bend, ROD_BEND_AT_CAST - 11 * 0x60);
        rod.recover();
        let mut frames = 0;
        while !rod.retired() {
            rod.tick(Some(&mesh), 0, 1);
            frames += 1;
            assert!(frames < 100);
        }
        assert_eq!(rod.bob, ROD_BOB_CEILING);
        assert_eq!(frames, 27, "(700 + 0x400) / 0x40 rounds up to 27");
    }

    #[test]
    fn the_lure_tick_bends_lifts_and_rolls_the_rod() {
        let mut rod = RodActor::cast();
        rod.bend = 0;
        // A held reel bends toward 0x1000; a fish on caps it at 0x1800.
        for _ in 0..64 {
            rod.drive(
                RodDrive {
                    hooked: false,
                    held: 0x80,
                },
                1,
            );
        }
        assert_eq!(rod.bend, 0x1000);
        for _ in 0..64 {
            rod.drive(
                RodDrive {
                    hooked: true,
                    held: 0,
                },
                1,
            );
        }
        assert_eq!(rod.bend, 0x1800);
        // D-pad down lifts, capped at 0x1000.
        for _ in 0..64 {
            rod.drive(
                RodDrive {
                    hooked: false,
                    held: 0x4000,
                },
                1,
            );
        }
        assert_eq!(rod.lift, 0x1000);
        // D-pad right rolls toward 0x100 at 0x10 a frame.
        rod.drive(
            RodDrive {
                hooked: false,
                held: 0x2000,
            },
            1,
        );
        assert_eq!((rod.lean_target, rod.lean), (0x100, 0x10));
        for _ in 0..32 {
            rod.drive(
                RodDrive {
                    hooked: false,
                    held: 0x2000,
                },
                1,
            );
        }
        assert_eq!(rod.lean, 0x100);
        // Released, the target bleeds back and the roll follows it down.
        for _ in 0..200 {
            rod.drive(RodDrive::default(), 1);
        }
        assert_eq!((rod.lean_target, rod.lean), (0, 0));
    }
}
