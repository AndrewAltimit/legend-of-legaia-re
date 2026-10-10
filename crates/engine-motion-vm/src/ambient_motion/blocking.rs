//! The heading LUT, the walk step and the `AmbientBlocking` collision seam.

/// The eight-point compass LUT at `0x80073F04`, in the **retail** heading
/// space (`0` = -Z, entry `i` = `i * 0x200`). Both rotate ops index it
/// `& 7`.
///
/// [`crate::motion_vm::heading_lut_engine`] is the same table carried into
/// the engine's `render_26` space (`+0x800`); this module stays in retail
/// space because the raw-wrapping write-back law is only meaningful there.
pub fn heading_lut_retail(idx: u8) -> u16 {
    u16::from(idx & 7) * 0x200
}

/// The per-direction **axis bitmask** table at `0x80073F14` - the sixteen
/// bytes immediately after the eight-entry compass LUT, and the table every
/// walk op reduces its heading index through before touching a coordinate.
///
/// Bit `1` = `+Z`, `2` = `-Z`, `4` = `+X`, `8` = `-X`, applied in that order,
/// so a diagonal entry moves both axes by the same per-tick step. Entries
/// `0..=7` are exactly the compass of [`heading_lut_retail`] in bit form
/// (`0` = `-Z`, walking clockwise); `8..=15` are the tail of the table and
/// carry no usable direction - `8` sets all four bits (whose four writes
/// cancel to zero net motion) and the rest are inert. No disc-authored walk
/// op indexes past `7`.
pub const WALK_DIR_BITS: [u8; 16] = [
    0x02, 0x0A, 0x08, 0x09, 0x01, 0x05, 0x04, 0x06, 0xFF, 0x00, 0x00, 0x00, 0x10, 0x00, 0x00, 0x00,
];

/// Apply one walk step of `step` units along the axes `WALK_DIR_BITS[idx]`
/// selects, in retail's write order. Returns the new `(x, z)`.
///
/// Retail loads and stores the coordinates as `lhu`/`sh` (16-bit wrapping),
/// which is what the `wrapping_*` here reproduces.
pub(crate) fn walk_apply(x: i16, z: i16, idx: u8, step: i16) -> (i16, i16) {
    let mask = WALK_DIR_BITS[usize::from(idx & 0x0F)];
    let (mut x, mut z) = (x, z);
    if mask & 1 != 0 {
        z = z.wrapping_add(step);
    }
    if mask & 2 != 0 {
        z = z.wrapping_sub(step);
    }
    if mask & 4 != 0 {
        x = x.wrapping_add(step);
    }
    if mask & 8 != 0 {
        x = x.wrapping_sub(step);
    }
    (x, z)
}

/// The collision service the **walk** ops need from the host.
///
/// Retail's two probes both resolve to `FUN_801cf8ac`, a box test of one
/// point against the **player actor** - not against the wall grid and not
/// against other NPCs. That is the single most surprising fact about ambient
/// wandering: an NPC's containment is its op's authored AABB, and the only
/// thing that can stop a step is the player standing in it.
pub trait AmbientBlocking {
    /// Directional steps `0x03` / `0x19` / `0x20`: retail probes the single
    /// `DAT_801F2254` compass point for the op's heading-LUT index
    /// (`radius 64` ahead) through `FUN_801cf8ac`.
    fn step_blocked(&self, x: i16, z: i16, lut_index: u8) -> bool;

    /// Wander `0x18` walk phase: retail probes the three-point fan of
    /// `DAT_801F21B4` row `dir4` (`0` = `Z-`, `1` = `X-`, `2` = `Z+`,
    /// `3` = `X+` - the `FUN_801cfe4c` direction space) through
    /// `FUN_801d5a68`, OR-ing the three results.
    fn wander_blocked(&self, x: i16, z: i16, dir4: u8) -> bool;
}

/// Nothing ever blocks - the standalone / facing-only reading, and what
/// [`AmbientMotion::tick`] uses.
#[derive(Debug, Clone, Copy, Default)]
pub struct NeverBlocks;

impl AmbientBlocking for NeverBlocks {
    fn step_blocked(&self, _x: i16, _z: i16, _lut_index: u8) -> bool {
        false
    }
    fn wander_blocked(&self, _x: i16, _z: i16, _dir4: u8) -> bool {
        false
    }
}

/// Every direction blocks - the fully-boxed-in reading.
///
/// Note what this does *not* buy: a blocked directional step re-runs its own
/// op forever without advancing the PC, so a stream held under this never
/// reaches whatever follows that op (a story-flag write included). A host
/// that wants an actor to hold its seat should let the ops run normally and
/// re-pin the position instead.
#[derive(Debug, Clone, Copy, Default)]
pub struct AlwaysBlocks;

impl AmbientBlocking for AlwaysBlocks {
    fn step_blocked(&self, _x: i16, _z: i16, _lut_index: u8) -> bool {
        true
    }
    fn wander_blocked(&self, _x: i16, _z: i16, _dir4: u8) -> bool {
        true
    }
}
