//! Where a Grid Duel played at a table sits on its pack map.
//!
//! A duel that names a map and a table (`map = "<id>"`, `[card_duels.table]`
//! in `modes.toml`) is played on a board laid on a table in the 3D field
//! scene. The session does not know: its rules, its AI and its state hash
//! are the 2D screen's. [`DuelTableFrame`] is the one fixed placement of the
//! table on the field - the board's centre, the direction the player looks
//! across it, the top's height and the seats - which the in-world stage
//! (`legaia_engine_expansion::duel_stage`) lays the cards out from.
//!
//! The table's axes: `across` is the unit ground vector from the player's
//! seat toward the opponent's ([`DuelTableFrame::facing`]), `right` the
//! player's right hand. Board row `0` is the far row (the opponent's side),
//! column `0` the player's left, as the 2D screen draws the grid.

use legaia_expansion_format::map::MapDef;
use legaia_expansion_format::modes::{CardDuel, DuelSfx, DuelSurface};
use serde::{Deserialize, Serialize};

use crate::fixed::ANGLE_TURN;
use crate::map_frame::MapFrame;

/// A compass sector (`0..8`, the map NPC facing table) as a mode angle
/// (`0` = world `+X`, a quarter turn = `+Z`). An NPC facing sector `f` takes
/// engine heading `0x800 + f * 0x200`, and a mode angle `a` is heading
/// `1024 - a`.
pub fn sector_to_mode_angle(sector: u8) -> u16 {
    (3072i32 - 512 * i32::from(sector & 7)).rem_euclid(i32::from(ANGLE_TURN)) as u16
}

/// A board cell's aspect: a card is taller than it is wide, so the board is
/// too (height over width).
pub const CELL_ASPECT: f32 = 1.36;

/// The table's placement on the field.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DuelTableFrame {
    /// The map and spawn; `origin` is the board's centre, world `(X, Z)`.
    pub frame: MapFrame,
    /// The direction the player looks across the board, a mode angle.
    pub facing: u16,
    /// Raise of the whole table above the floor, world units.
    pub lift: f32,
    /// The table top above the floor (before `lift`), world units.
    pub height: f32,
    /// The board's width, world units; its depth is `board * CELL_ASPECT`.
    pub board: f32,
    /// Each seat's distance from the board's centre, world units.
    pub seat_distance: f32,
    /// The stage draws a table under the board.
    pub draw_table: bool,
    pub surface: Option<DuelSurface>,
    pub sfx: DuelSfx,
    /// The map NPC across the board and the spectators, with their clips.
    pub opponent: Option<legaia_expansion_format::modes::DuelSeatNpc>,
    pub spectators: Vec<legaia_expansion_format::modes::DuelSeatNpc>,
    /// The seat height of a chair the stage draws for a seated NPC, world
    /// units above the floor (`lift` not included).
    #[serde(default)]
    pub chair_height: f32,
}

/// Why a duel's table could not be placed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TableError {
    NoMap(String),
    OffMap([u16; 2]),
}

impl DuelTableFrame {
    /// The table of `duel` on `def` (map `map_id`), or `None` when the duel
    /// is not played at a table.
    pub fn from_duel(
        duel: &CardDuel,
        def: Option<&MapDef>,
        map_id: &str,
    ) -> Result<Option<Self>, TableError> {
        let Some(t) = &duel.table else {
            return Ok(None);
        };
        let def = def.ok_or_else(|| TableError::NoMap(map_id.to_string()))?;
        let grid = def
            .tile_grid()
            .map_err(|_| TableError::NoMap(map_id.to_string()))?;
        if usize::from(t.tile[0]) >= grid.width || usize::from(t.tile[1]) >= grid.height {
            return Err(TableError::OffMap(t.tile));
        }
        let (cx, cz) = def.tile_centre(t.tile[0], t.tile[1]);
        Ok(Some(Self {
            frame: MapFrame {
                map_id: map_id.to_string(),
                spawn: duel.spawn.clone(),
                origin: [cx + t.offset[0], cz + t.offset[2]],
                unit: 1,
            },
            facing: sector_to_mode_angle(t.facing),
            lift: t.offset[1] as f32,
            height: f32::from(t.height),
            board: f32::from(t.board.max(48)),
            seat_distance: f32::from(t.seat_distance.max(40)),
            draw_table: t.draw_table,
            surface: t.surface,
            sfx: t.sfx.clone(),
            opponent: t.opponent.clone(),
            spectators: t.spectators.clone(),
            chair_height: f32::from(t.chair_height),
        }))
    }

    /// The board's centre, world `(X, Z)`.
    pub fn centre(&self) -> [f32; 2] {
        [self.frame.origin[0] as f32, self.frame.origin[1] as f32]
    }

    /// The board's depth (along `across`), world units.
    pub fn board_depth(&self) -> f32 {
        self.board * CELL_ASPECT
    }

    /// The top above the floor, `lift` included.
    pub fn top(&self) -> f32 {
        self.lift + self.height
    }

    /// `across` and `right` as unit ground vectors `(X, Z)`, for the seat
    /// the view is from (`seat` 1 looks the other way).
    pub fn axes(&self, seat: usize) -> ([f32; 2], [f32; 2]) {
        let mut a = f32::from(self.facing) / f32::from(ANGLE_TURN) * std::f32::consts::TAU;
        if seat % 2 == 1 {
            a += std::f32::consts::PI;
        }
        let across = [a.cos(), a.sin()];
        // `across x up` with up `-Y` (the field frame is X right, Y down,
        // Z forward): looking along `+Z` the right hand is `+X`, along `+X`
        // it is `-Z`.
        let right = [across[1], -across[0]];
        (across, right)
    }

    /// A table-local point `(right, across)` from the board's centre as
    /// world `(X, Z)`, in seat 0's axes.
    pub fn at(&self, right: f32, across: f32) -> [f32; 2] {
        let (a, r) = self.axes(0);
        let c = self.centre();
        [
            c[0] + r[0] * right + a[0] * across,
            c[1] + r[1] * right + a[1] * across,
        ]
    }

    /// A board cell's centre, table-local `(right, across)`. Row `0` is
    /// the opponent's side (seat 1's), column `0` seat 0's left.
    pub fn cell_local(&self, cell: usize) -> (f32, f32) {
        let (col, row) = ((cell % 3) as f32, (cell / 3) as f32);
        let (w, d) = (self.board / 3.0, self.board_depth() / 3.0);
        ((col - 1.0) * w, (1.0 - row) * d)
    }

    /// A seat's position: behind its edge of the board, world `(X, Z)`.
    pub fn seat_pos(&self, seat: usize) -> [f32; 2] {
        let s = if seat.is_multiple_of(2) { -1.0 } else { 1.0 };
        self.at(0.0, s * self.seat_distance)
    }

    /// The mode angle seat `seat` faces (across the board).
    pub fn seat_facing(&self, seat: usize) -> u16 {
        if seat.is_multiple_of(2) {
            self.facing
        } else {
            (self.facing + ANGLE_TURN / 2) % ANGLE_TURN
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(facing: u8) -> DuelTableFrame {
        DuelTableFrame {
            frame: MapFrame {
                map_id: "m".into(),
                spawn: String::new(),
                origin: [1000, 2000],
                unit: 1,
            },
            facing: sector_to_mode_angle(facing),
            lift: 0.0,
            height: 64.0,
            board: 120.0,
            seat_distance: 150.0,
            draw_table: true,
            surface: None,
            sfx: DuelSfx::default(),
            opponent: None,
            spectators: Vec::new(),
            chair_height: 34.0,
        }
    }

    #[test]
    fn sectors_match_the_npc_heading_table() {
        // Sector 0 is heading 0x800 = mode angle 3072 (world -Z).
        assert_eq!(sector_to_mode_angle(0), 3072);
        assert_eq!(sector_to_mode_angle(2), 2048);
        assert_eq!(sector_to_mode_angle(4), 1024);
        assert_eq!(sector_to_mode_angle(6), 0);
    }

    #[test]
    fn seats_face_each_other_across_the_board() {
        // Sector 6: the player looks along world +X.
        let t = frame(6);
        let p = t.seat_pos(0);
        let o = t.seat_pos(1);
        assert!(
            (p[0] - 850.0).abs() < 1e-3 && (p[1] - 2000.0).abs() < 1e-3,
            "{p:?}"
        );
        assert!(
            (o[0] - 1150.0).abs() < 1e-3 && (o[1] - 2000.0).abs() < 1e-3,
            "{o:?}"
        );
        assert_eq!(t.seat_facing(0), 0);
        assert_eq!(t.seat_facing(1), 2048);
    }

    #[test]
    fn row_zero_is_the_far_row_and_column_zero_the_left() {
        let t = frame(6);
        // Looking along +X (up -Y) the right hand is world -Z.
        let (r, a) = t.cell_local(0);
        assert!(r < 0.0 && a > 0.0);
        let w = t.at(r, a);
        assert!(w[0] > 1000.0, "far row is toward the opponent: {w:?}");
        assert!(w[1] > 2000.0, "left column is toward +Z: {w:?}");
        let (r8, a8) = t.cell_local(8);
        assert!(r8 > 0.0 && a8 < 0.0);
        let (r4, a4) = t.cell_local(4);
        assert_eq!((r4, a4), (0.0, 0.0));
    }
}
