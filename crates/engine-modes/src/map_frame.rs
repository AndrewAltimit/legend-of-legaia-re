//! Where a mode played on a pack map sits in the field scene.
//!
//! A golf hole or a race track that names a pack map (`map = "<id>"` in
//! `modes.toml`) is laid out in the map's tiles, and the session runs in
//! its own **mode units** exactly as it does for a tile-string course: the
//! physics constants, the AI and the state hash do not know a map exists.
//! [`MapFrame`] is the one fixed placement of those units on the field: one
//! mode unit is [`MapFrame::unit`] field world units, and mode `(0, 0)` sits
//! at [`MapFrame::origin`]. Mode `+x` is world `+X` and mode `+y` world `+Z`,
//! so a 12-bit mode angle keeps its meaning on the ground.
//!
//! The scale is per mode. A golf tile is [`crate::golf::course::TILE`]
//! (`32`) mode units and a map tile `128` world units, so at
//! [`GOLF_UNIT`] one golf tile is one map tile and a hole keeps the length
//! its physics was tuned for. A race runs at [`RACE_UNIT`], where the
//! racers' top speed is about the field player's run.

use serde::{Deserialize, Serialize};

use crate::fixed::{FX_ONE, Vec2};

/// Field world units per mode unit on a golf hole: one golf tile per map
/// tile.
pub const GOLF_UNIT: i32 = 4;
/// Field world units per mode unit on a race track.
pub const RACE_UNIT: i32 = 2;
/// Field world units per map tile.
pub const MAP_TILE: i32 = 128;

/// The placement of a session's mode units on a pack map's field.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MapFrame {
    /// The pack map (`maps/<id>.toml` stem) the session is played on.
    pub map_id: String,
    /// The spawn the host enters the map at (the map's first when empty).
    pub spawn: String,
    /// World `(X, Z)` of mode `(0, 0)`.
    pub origin: [i32; 2],
    /// Field world units per mode unit ([`GOLF_UNIT`], [`RACE_UNIT`]).
    pub unit: i32,
}

impl MapFrame {
    /// A mode position as world `(X, Z)`, truncated to whole units.
    pub fn to_world(&self, p: Vec2) -> [i32; 2] {
        [
            self.origin[0] + (p.x as i64 * self.unit as i64 / FX_ONE as i64) as i32,
            self.origin[1] + (p.y as i64 * self.unit as i64 / FX_ONE as i64) as i32,
        ]
    }

    /// A mode position as world `(X, Z)` with the fraction kept, for drawing.
    pub fn to_world_f(&self, p: Vec2) -> [f32; 2] {
        let s = self.unit as f32 / FX_ONE as f32;
        [
            self.origin[0] as f32 + p.x as f32 * s,
            self.origin[1] as f32 + p.y as f32 * s,
        ]
    }

    /// A world `(X, Z)` as a mode position.
    pub fn from_world(&self, x: i32, z: i32) -> Vec2 {
        Vec2::new(
            (x - self.origin[0]) * FX_ONE / self.unit,
            (z - self.origin[1]) * FX_ONE / self.unit,
        )
    }

    /// A length in mode [`crate::fixed::Fx`] as world units.
    pub fn len_to_world(&self, len: crate::fixed::Fx) -> f32 {
        len as f32 * self.unit as f32 / FX_ONE as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_units_are_two_world_units_from_the_origin() {
        let f = MapFrame {
            map_id: "m".into(),
            spawn: String::new(),
            origin: [2048, 1024],
            unit: RACE_UNIT,
        };
        let p = Vec2::new(32 * FX_ONE, 16 * FX_ONE);
        assert_eq!(f.to_world(p), [2048 + 64, 1024 + 32]);
        assert_eq!(f.from_world(2048 + 64, 1024 + 32), p);
        assert_eq!(f.to_world_f(Vec2::new(FX_ONE / 2, 0)), [2049.0, 1024.0]);
    }
}
