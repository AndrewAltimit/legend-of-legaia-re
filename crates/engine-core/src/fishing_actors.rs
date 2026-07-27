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
// NOT WIRED: the consumer is a **screen-space line primitive**, and the port
// emits none - this is the 3-D half of the clipper `clip_segment_2d` below
// names, and it is inert for exactly that reason. Neither `engine-ui`'s draw
// list (text + sprite + solid rect) nor `engine-render`'s VRAM pipeline carries
// a line kind to project *for*. Wiring wants a line draw kind first, not a
// fishing host.
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

/// The free-swimming fish actor as one advancing object: the `+0x14/+0x18`
/// world pair, the `+0x26` facing word, the dwell counter and the rolled
/// destination, stepped one call per idle/cast frame through the ported
/// kernels ([`step_facing`], [`roll_wander_target`], [`fish_camera`]).
///
/// The per-frame drift *rate* toward the rolled destination is host glue (the
/// retail chase constant is not pinned here); the roll, the facing step and
/// the camera publish are the ported arithmetic.
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
    /// The rolled destination the actor drifts toward.
    target: (i32, i32),
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
            target: (x as i32, z as i32),
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
            self.target = (t.x, t.z);
            rolled = Some(t);
        }
        // Host glue: drift toward the rolled destination a few units a frame.
        let step = |v: i16, t: i32| -> i16 { v + (t - v as i32).clamp(-4, 4) as i16 };
        self.x = step(self.x, self.target.0);
        self.z = step(self.z, self.target.1);
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
// NOT WIRED: its input is the `_DAT_8007B8F4` class word that retail reads
// *after* the walk-grid probe reports the `0x4000` water bit, and the engine's
// session carries no per-scene grid to probe (the same gap
// [`walk_grid_overhead`] names). Its sibling kernels on the same address are on
// the live path through [`crate::fishing::BandCheck::tick`]; this one is not,
// because the tick has no tile under the lure to classify.
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
// NOT WIRED: it counts raw held bits out of `_DAT_8007B874`, and the engine
// never sees that word - [`crate::fishing::BandCheck::tick`] takes an already
// abstracted `edge_bonus: i32` from the host instead, which is the same
// quantity arrived at from the browser / native input layers. Wiring means
// deciding the pad mask is the engine's representation, not adding a call.
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
// at the first stage frame, and the play window spawns them into its effect
// pool (offset from the wander actor's catch position) and fires each `cue`
// through the SFX scheduler (`window/minigames.rs`).
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

// NOT WIRED: the consumer is a **screen-space line primitive**, and the port
// emits none. `project_segment` above is this clipper's 3-D half and is inert
// for the same reason: the fishing line, the slot machine's paylines and the
// dance floor's guides are all two-point draws, and neither `engine-ui`'s draw
// list (text + sprite + solid rect) nor `engine-render`'s VRAM pipeline carries
// a line primitive to clip *for*. Wiring wants a line draw kind first, not a
// fishing host.
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
// NOT WIRED: the engine's fishing model ([`crate::fishing::PondSession`]) is
// venue *rules* - cast, band, strike, fight - and carries no per-scene grid at
// all, so there is no `*(_DAT_1F8003EC) + 0x4000` block to probe. The browser
// fishing venue does decode one (`legaia_asset::field_objects::WalkHeightfield`
// in `crates/web-viewer/src/minigames_fishing_scene.rs`), so the missing piece
// is a grid handle on the session rather than a decoder.
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
// NOT WIRED: both operands are *scene* positions - the angler and the lure -
// and the port's fishing session models the cast as a scalar power / line
// record, never as two points on a pond. The blocker is the same missing
// fishing scene host [`walk_grid_overhead`] names, plus a binding for the SCUS
// normalise helper `FUN_8005AF0C`, which the port takes as a closure rather
// than owning.
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
    fn the_wander_actor_rolls_on_dwell_expiry_and_drifts() {
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
        // The dwell now holds; no re-roll, and the actor drifts toward the
        // target (z gains its 0x400 bias, so it moves +4 a frame).
        let z0 = w.z;
        assert!(w.tick(0, || 0).is_none());
        assert_eq!(w.z, z0 + 4);
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
}
