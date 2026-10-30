//! Where a poker table played in the world sits on its pack map.
//!
//! A poker instance that names a map and a table (`map = "<id>"`,
//! `[poker.table]` in `modes.toml`) is dealt on a felt table in the 3D field
//! scene. The session does not know: [`PokerTableFrame`] is the fixed
//! placement the in-world stage (`legaia_engine_expansion::poker_stage`)
//! seats the players around and lays the cards and chips out on.
//!
//! The felt is an oval: [`PokerTableFrame::radius`] deep toward the
//! player, [`OVAL`] times that wide. Seats sit evenly round it starting
//! with the viewer's at the near edge, so every peer of a networked table
//! sits in the near chair and the others keep their order clockwise.

use legaia_expansion_format::map::MapDef;
use legaia_expansion_format::modes::{DuelSeatNpc, DuelSfx, DuelSurface, PokerTable};
use serde::{Deserialize, Serialize};

use crate::cards::table::sector_to_mode_angle;
use crate::fixed::ANGLE_TURN;
use crate::map_frame::MapFrame;

/// The felt's width over its depth.
pub const OVAL: f32 = 1.35;

/// The table's placement on the field.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PokerTableFrame {
    /// The map and spawn; `origin` is the felt's centre, world `(X, Z)`.
    pub frame: MapFrame,
    /// The direction the player looks across the table, a mode angle.
    pub facing: u16,
    pub lift: f32,
    pub height: f32,
    /// The felt's half depth, world units.
    pub radius: f32,
    /// Seats at the table (the session's).
    pub seats: usize,
    pub draw_table: bool,
    pub surface: Option<DuelSurface>,
    pub sfx: DuelSfx,
    /// The other seats' NPCs, in seat order with seat 0 (the player's in
    /// a single-player game) skipped.
    pub players: Vec<DuelSeatNpc>,
    pub spectators: Vec<DuelSeatNpc>,
}

/// Why a table could not be placed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PokerTableError {
    NoMap(String),
    OffMap([u16; 2]),
}

impl PokerTableFrame {
    /// The table of `t` on `def` (map `map_id`) for a session of `seats`
    /// seats, or `None` when the instance is not played at a table.
    pub fn from_table(
        t: &PokerTable,
        def: Option<&MapDef>,
        map_id: &str,
        seats: usize,
    ) -> Result<Option<Self>, PokerTableError> {
        let Some(l) = &t.table else {
            return Ok(None);
        };
        let def = def.ok_or_else(|| PokerTableError::NoMap(map_id.to_string()))?;
        let grid = def
            .tile_grid()
            .map_err(|_| PokerTableError::NoMap(map_id.to_string()))?;
        if usize::from(l.tile[0]) >= grid.width || usize::from(l.tile[1]) >= grid.height {
            return Err(PokerTableError::OffMap(l.tile));
        }
        let (cx, cz) = def.tile_centre(l.tile[0], l.tile[1]);
        Ok(Some(Self {
            frame: MapFrame {
                map_id: map_id.to_string(),
                spawn: t.spawn.clone(),
                origin: [cx + l.offset[0], cz + l.offset[2]],
                unit: 1,
            },
            facing: sector_to_mode_angle(l.facing),
            lift: l.offset[1] as f32,
            height: f32::from(l.height),
            radius: f32::from(l.radius.max(60)),
            seats: seats.max(2),
            draw_table: l.draw_table,
            surface: l.surface,
            sfx: l.sfx.clone(),
            players: l.players.clone(),
            spectators: l.spectators.clone(),
        }))
    }

    pub fn centre(&self) -> [f32; 2] {
        [self.frame.origin[0] as f32, self.frame.origin[1] as f32]
    }

    pub fn top(&self) -> f32 {
        self.lift + self.height
    }

    /// `across` and `right` as unit ground vectors `(X, Z)`.
    pub fn axes(&self) -> ([f32; 2], [f32; 2]) {
        let a = f32::from(self.facing) / f32::from(ANGLE_TURN) * std::f32::consts::TAU;
        let across = [a.cos(), a.sin()];
        (across, [across[1], -across[0]])
    }

    /// The physical chair of `seat` seen from `local`: `0` is the near
    /// chair, then round the table to the player's left.
    pub fn chair(&self, seat: usize, local: usize) -> usize {
        (seat + self.seats - local % self.seats) % self.seats
    }

    /// Chair `chair`'s direction from the centre in table-local
    /// `(right, across)`, unit: chair 0 straight toward the player.
    pub fn chair_dir(&self, chair: usize) -> (f32, f32) {
        let t = chair as f32 / self.seats as f32 * std::f32::consts::TAU;
        // Chair 0 at -across; round counter-clockwise seen from above
        // (to the player's left first).
        (-t.sin(), -t.cos())
    }

    /// A table-local point `(right, across)` as world `(X, Z)`.
    pub fn at(&self, right: f32, across: f32) -> [f32; 2] {
        let (a, r) = self.axes();
        let c = self.centre();
        [
            c[0] + r[0] * right + a[0] * across,
            c[1] + r[1] * right + a[1] * across,
        ]
    }

    /// The point on the oval at chair `chair`'s direction, `inset` world
    /// units in from the felt's edge (negative: outside it), table-local.
    pub fn on_oval(&self, chair: usize, inset: f32) -> (f32, f32) {
        let (dr, da) = self.chair_dir(chair);
        let (w, d) = (self.radius * OVAL - inset, self.radius - inset);
        (dr * w, da * d)
    }

    /// A seat's name at the table: "You", its NPC's `name`, else
    /// "Seat N".
    pub fn seat_name(&self, seat: usize, local: usize) -> String {
        if seat == local {
            return "You".to_string();
        }
        self.player_for(seat)
            .filter(|n| !n.name.is_empty())
            .map(|n| n.name.clone())
            .unwrap_or_else(|| format!("Seat {}", seat + 1))
    }

    /// The NPC that plays session seat `seat`: [`Self::players`] in seat
    /// order with seat 0 skipped.
    pub fn player_for(&self, seat: usize) -> Option<&DuelSeatNpc> {
        if seat == 0 {
            return None;
        }
        self.players.get(seat - 1)
    }

    /// Where chair `chair`'s player stands, world `(X, Z)`.
    pub fn chair_pos(&self, chair: usize) -> [f32; 2] {
        let (r, a) = self.on_oval(chair, -58.0);
        self.at(r, a)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(seats: usize) -> PokerTableFrame {
        PokerTableFrame {
            frame: MapFrame {
                map_id: "m".into(),
                spawn: String::new(),
                origin: [1000, 1000],
                unit: 1,
            },
            facing: sector_to_mode_angle(6),
            lift: 0.0,
            height: 64.0,
            radius: 130.0,
            seats,
            draw_table: true,
            surface: None,
            sfx: DuelSfx::default(),
            players: Vec::new(),
            spectators: Vec::new(),
        }
    }

    #[test]
    fn the_viewer_sits_in_the_near_chair() {
        let t = table(4);
        for local in 0..4 {
            assert_eq!(t.chair(local, local), 0);
        }
        assert_eq!(t.chair(1, 0), 1);
        assert_eq!(t.chair(0, 1), 3);
        // Chair 0 is toward the player (facing +X: the near side is -X).
        let p = t.chair_pos(0);
        assert!(p[0] < 1000.0 - 130.0, "{p:?}");
        // Chair 2 of 4 is straight across.
        let o = t.chair_pos(2);
        assert!(o[0] > 1000.0 + 130.0, "{o:?}");
    }

    #[test]
    fn chairs_ring_the_table_evenly() {
        let t = table(6);
        let c = t.centre();
        let d: Vec<f32> = (0..6)
            .map(|k| {
                let p = t.chair_pos(k);
                (p[0] - c[0]).hypot(p[1] - c[1])
            })
            .collect();
        for x in &d {
            assert!(*x > 130.0 && *x < 130.0 * OVAL + 80.0, "{d:?}");
        }
    }
}
