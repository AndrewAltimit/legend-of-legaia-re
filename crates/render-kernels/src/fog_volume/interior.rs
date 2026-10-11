//! `InteriorTracker`: deciding whether the player stands in an enclosed interior.

use super::*;

/// Walk areas (in 64-unit sub-cells) up to this size can be interiors; a
/// larger one is always open ground. House rooms, castle halls and the
/// corridors between them run from a few dozen sub-cells to about 750
/// across the scenes that raise a bank; the streets, plazas and forest
/// floors they hang off run past 1,300.
pub const INTERIOR_MAX_SUBCELLS: u32 = 800;

/// Sub-cells per side of the walk-area lattice (four per 128-unit tile).
pub const AREA_STRIDE: usize = 0x100;

/// Which walk areas of the current field scene are interiors, learned from
/// how the player moves between them.
///
/// Legaia lays a town's house rooms (and a castle's halls) out in the same
/// scene map as its streets, each an island of floor at its own tiles, and
/// a door is an **intra-scene warp** to it rather than a scene change
/// (`docs/formats/encounter.md`). The fog-region table bounds some scenes'
/// fog to their streets ([`region_weight`]), but many keep one region over
/// the whole map. So the bank also tracks the walk areas - the 4-connected
/// open floor components the locomotion collision leaves - and classifies
/// them:
///
/// - the area the player is first seen standing in after entering the scene
///   is **open ground** (an entrance from another scene, a picker seat);
/// - walking (no warp) into another area carries the current class over -
///   a stair whose tiles carry no floor bit splits one street into two
///   areas, and that is still the street;
/// - a **warp** (a jump past [`TELEPORT_UNITS`] in one tick) into an area
///   not yet classified makes it an **interior** when it is room-sized
///   ([`INTERIOR_MAX_SUBCELLS`]), open ground otherwise; a warp into a
///   classified area takes its class.
///
/// The bank fades out while the player is in an interior and snaps off on
/// the warp in (the door is a cut). Nothing here is retail: retail's pool
/// spawns wherever its region table lets it, interior or not.
#[derive(Debug, Clone, Default)]
pub struct InteriorTracker {
    /// Area id per sub-cell (`sz * 0x100 + sx`, `0` closed, ids 1-based);
    /// empty until seeded.
    labels: Vec<u16>,
    /// Sub-cell count per area (`sizes[id - 1]`).
    sizes: Vec<u32>,
    /// Per area: `0` unknown, `1` open ground, `2` interior.
    class: Vec<u8>,
    /// The player's last observed position and area.
    last: Option<([i32; 2], u16)>,
    /// Whether the player stands in an interior.
    pub indoors: bool,
}

impl InteriorTracker {
    /// Forget everything - a new scene.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Whether [`Self::seed`] has run for this scene.
    pub fn seeded(&self) -> bool {
        !self.labels.is_empty()
    }

    /// Install the scene's walk areas (`labels` over the `0x100 x 0x100`
    /// sub-cell lattice, `sizes` per 1-based id).
    pub fn seed(&mut self, labels: Vec<u16>, sizes: Vec<u32>) {
        self.class = vec![0; sizes.len()];
        self.labels = labels;
        self.sizes = sizes;
        self.last = None;
        self.indoors = false;
    }

    /// The area id at world `(x, z)`, `0` off the open floor.
    pub fn area_at(&self, x: i32, z: i32) -> u16 {
        if x < 0 || z < 0 {
            return 0;
        }
        let (sx, sz) = ((x >> 6) as usize, (z >> 6) as usize);
        if sx >= AREA_STRIDE || sz >= AREA_STRIDE {
            return 0;
        }
        self.labels.get(sz * AREA_STRIDE + sx).copied().unwrap_or(0)
    }

    /// Observe the player at world `(x, z)` this tick. Returns `true` on the
    /// tick a warp carries the player into an interior.
    pub fn observe(&mut self, x: i32, z: i32) -> bool {
        if !self.seeded() {
            return false;
        }
        let area = self.area_at(x, z);
        let Some((prev, prev_area)) = self.last else {
            // First sight in this scene: where the player came in.
            if area != 0 {
                self.set_class(area, 1);
                self.indoors = false;
                self.last = Some(([x, z], area));
            }
            return false;
        };
        let (dx, dz) = ((x - prev[0]) as f32, (z - prev[1]) as f32);
        let warped = (dx * dx + dz * dz).sqrt() > TELEPORT_UNITS;
        let mut entered = false;
        if area != 0 && area != prev_area {
            let known = self.class[usize::from(area) - 1];
            let class = match known {
                0 if warped => {
                    if self.sizes[usize::from(area) - 1] <= INTERIOR_MAX_SUBCELLS {
                        2
                    } else {
                        1
                    }
                }
                0 => {
                    if self.indoors {
                        2
                    } else {
                        1
                    }
                }
                k => k,
            };
            self.set_class(area, class);
            let indoors = class == 2;
            entered = warped && indoors && !self.indoors;
            self.indoors = indoors;
        }
        // A closed sub-cell (a doorway, a stair without a floor bit) keeps
        // the last area, so the next open one is compared against it.
        let keep = if area == 0 { prev_area } else { area };
        self.last = Some(([x, z], keep));
        entered
    }

    fn set_class(&mut self, area: u16, class: u8) {
        if let Some(c) = self.class.get_mut(usize::from(area).wrapping_sub(1)) {
            *c = class;
        }
    }
}
