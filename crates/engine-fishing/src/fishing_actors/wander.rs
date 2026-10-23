//! The free-swim wander: facing step, retarget roll, the fish camera and the wander actor.

use legaia_engine_system::pad::{PACK_LEFT, PACK_RIGHT};

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
    /// (`rand & 1`): `1` spawns record `0x801D8ED8`, `0` record `0x801D8F44`
    /// (`0x801D23D4..0x801D23F8`).
    pub ripple_variant: u32,
    /// The first draw, `rand() & 0xFFF`: the on-stack rotation trio's `y`
    /// (`sh v0,0x1a(sp)` at `0x801D2300`) the ripple spawns with, `x` / `z`
    /// zero.
    pub rotation: u32,
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
    let rotation = rolls() & 0xFFF;
    let dwell = (rolls() as i32) % RETARGET_SPAN + RETARGET_MIN;
    let dz = 3 - ((rolls() as i32) % 6);
    let dx = 3 - ((rolls() as i32) % 6);
    let ripple_variant = rolls() & 1;
    WanderTarget {
        dwell,
        x: x + dx * WANDER_STEP.1,
        z: z + WANDER_Z_BIAS + dz * WANDER_STEP.0,
        ripple_variant,
        rotation,
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
/// (`legaia_engine_minigame_scenes::fishing_venue::party_placements`) - the library state
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
