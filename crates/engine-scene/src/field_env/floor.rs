//! Floor-following: Euler rotation of an offset, the floor anchor a placement rides, and the floor wave.

use super::*;

/// `Rx * Ry * Rz` of three authored PSX angles `[rot_x, rot_y, rot_z]`
/// (`4096` = full revolution) applied to a direction: the rotation half of
/// [`EnvDraw::place_point`], free of a draw so a caller holding only a
/// placement's three angles composes the same matrix.
// REF: FUN_80026988
pub fn rotate_euler(rot: [u16; 3], v: [f32; 3]) -> [f32; 3] {
    let a = |v: u16| f32::from(v & 0x0FFF) * (std::f32::consts::TAU / 4096.0);
    let (sx, cx) = a(rot[0]).sin_cos();
    let (sy, cy) = a(rot[1]).sin_cos();
    let (sz, cz) = a(rot[2]).sin_cos();
    let [x, y, z] = v;
    // Rz, then Ry, then Rx: the product `Rx * Ry * Rz` applied to `v`.
    let (x, y) = (x * cz - y * sz, x * sz + y * cz);
    let (x, z) = (x * cy + z * sy, -x * sy + z * cy);
    let (y, z) = (y * cx - z * sx, y * sx + z * cx);
    [x, y, z]
}

/// The rung(s) of the scene floor-height ladder one [`EnvDraw`]'s world Y was
/// resolved from.
///
/// The ladder is not static: field-VM op `0x4C` nibble-9 sub-`0xE` rewrites all
/// sixteen rungs and sub-`0..2` sets one rung **oscillating** every frame
/// (`FUN_801DDE34` -> `FUN_801DA930`, `legaia_engine_field::field_actor_timers`),
/// which is how `jou`'s organic interior floor undulates. Retail's per-cell
/// terrain emitters re-read the scratchpad ladder every frame, so the drawn
/// ground moves with it; the port bakes its draw list once at scene entry, so
/// it keeps the rungs each draw used and re-resolves the height per frame
/// through [`FloorWave`] instead of re-walking the map. The walk-ground
/// heightfield is the per-vertex half of the same wave
/// (`crate::field_ground::live_render_positions`).
///
/// This mirrors exactly what [`Placement::world_y`] consumes: a terrain /
/// decoration cell's four corner tiles, or a placed object's single
/// placement-tile nibble.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct FloorAnchor {
    /// A terrain / decoration cell's `2 x 2` corner-tile nibbles
    /// ([`Placement::floor_corner_nibbles`]).
    pub corners: Option<[u8; 4]>,
    /// A placed object's single placement-tile nibble
    /// ([`Placement::floor_nibble`]).
    pub nibble: Option<u8>,
}

impl FloorAnchor {
    /// The floor term of [`Placement::world_y`] (i.e. the world Y **without**
    /// the record's own `y_off`), against a floor LUT in the MAN-header frame.
    ///
    /// `None` for a draw with no floor grid at all - those sit at `0` whatever
    /// the ladder does, so a wave must not move them.
    pub fn height(&self, lut: &[i16; 16]) -> Option<i32> {
        let h = |nibble: u8| -(lut[(nibble & 0x0F) as usize] as i32);
        match (self.corners, self.nibble) {
            (Some(corners), _) => {
                let sum: i32 = corners.iter().map(|&n| h(n)).sum();
                Some(if sum < 0 { (sum + 3) >> 2 } else { sum >> 2 })
            }
            (None, Some(nib)) => Some(h(nib)),
            (None, None) => None,
        }
    }
}

/// How far the scene's floor-height ladder has moved since a draw list was
/// baked - the per-frame Y offset a host adds to each [`EnvDraw`] so the drawn
/// ground follows the live ladder.
///
/// Both LUTs are held in the **MAN-header frame** (the frame
/// [`crate::scene::Scene::field_floor_height_lut`] returns and
/// [`Placement::world_y`] consumes), which is the negation of the runtime
/// scratchpad frame `World::terrain.floor_height_lut` holds
/// (`0x1F80035C`; `FUN_8003AEB0` installs sixteen negated shorts).
/// [`Self::from_scene_and_world`] does that conversion, so a host never has to
/// pick a sign.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FloorWave {
    base: [i16; 16],
    live: [i16; 16],
}

impl FloorWave {
    /// The wave between the scene's baked ladder (`scene_lut`, MAN frame) and
    /// the world's live one (`world_lut`, scratchpad frame - i.e.
    /// `World::terrain.floor_height_lut`).
    ///
    /// `None` when the two agree, which is every frame of every scene whose
    /// script never touches the ladder: a host that short-circuits on `None`
    /// pays nothing on the 80-odd scenes without a wave.
    pub fn from_scene_and_world(
        scene_lut: Option<[i16; 16]>,
        world_lut: &[i16; 16],
    ) -> Option<Self> {
        let base = scene_lut?;
        let live = world_lut.map(i16::wrapping_neg);
        (base != live).then_some(FloorWave { base, live })
    }

    /// Build one from two MAN-frame LUTs directly (tests, and a host that
    /// already holds both in that frame).
    pub fn between(base: [i16; 16], live: [i16; 16]) -> Option<Self> {
        (base != live).then_some(FloorWave { base, live })
    }

    /// The Y offset to add to a draw resolved against the baked ladder.
    pub fn offset(&self, floor: &FloorAnchor) -> i32 {
        match (floor.height(&self.live), floor.height(&self.base)) {
            (Some(live), Some(base)) => live - base,
            _ => 0,
        }
    }

    /// The offsets for a whole draw list, in the list's own order.
    pub fn offsets(&self, draws: &[EnvDraw]) -> Vec<i32> {
        draws.iter().map(|d| self.offset(&d.floor)).collect()
    }
}
