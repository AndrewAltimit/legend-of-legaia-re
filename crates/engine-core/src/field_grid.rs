//! Field tile-grid movement + collision.
//!
//! PORT: overlay_0897_801ef2b0 (field-walk state machine) + field-VM op
//! `0x49` (`_DAT_8007b450` grid install).
//!
//! Player field movement is grid-based: the scene is a `width × height`
//! array of byte cells, the player occupies one `(col, row)` cell, and
//! each accepted d-pad press advances the player exactly one cell. The
//! cell array *is* the collision data - a destination cell value of
//! [`CELL_WALL`] (`2`) is a wall. See
//! [`docs/subsystems/field-walk.md`](../../../docs/subsystems/field-walk.md).
//!
//! The grid (header + cells) is installed inline in the field-VM event
//! script by op `0x49`; this module models the runtime view the walk SM
//! consumes (`overlay_0897_801ef2b0`), not the on-disc parse (the exact
//! inline cell-array offset is still open - see the doc).

/// World units per tile (`0x80`). Retail multiplies the tile index by
/// this when mapping a cell to a world position.
pub const TILE: i32 = 0x80;

/// Half-tile offset placing the actor at the tile centre (`0x40`).
pub const TILE_CENTER: i32 = 0x40;

/// Cell value that blocks movement. The walk SM rejects a step whose
/// destination cell equals this (`overlay_0897_801ef2b0` case 4).
pub const CELL_WALL: u8 = 2;

/// One of the four grid-step directions. The retail walk SM decodes
/// these from the camera-facing-remapped pad
/// (`func_0x800467e8` then mask bits `0x1000`/`0x2000`/`0x4000`/`0x8000`);
/// the engine maps screen d-pad directions to grid axes here. The
/// camera-relative remap is not yet ported (the facing transform is an
/// open RE item), so this is the camera-neutral mapping: screen-up
/// decrements the row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WalkDir {
    Up,
    Down,
    Left,
    Right,
}

/// Runtime field grid: dimensions, world-tile origin, the mutable cell
/// array, and the player's logical cell. Mirrors the runtime state the
/// walk SM reads (`DAT_801f35c0` cells, `_DAT_8007b450` header fields,
/// `DAT_801f35c8/cc` player cell).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FieldGrid {
    /// Grid width in columns (`_DAT_8007b450 + 3`).
    pub width: u8,
    /// Grid height in rows (`_DAT_8007b450 + 4`).
    pub height: u8,
    /// World tile origin X, added to `col` before the world mapping
    /// (`_DAT_8007b450 + 1`).
    pub origin_x: u8,
    /// World tile origin Z, added to `row` (`_DAT_8007b450 + 2`).
    pub origin_z: u8,
    /// `width * height` cell bytes, row-major (`DAT_801f35c0`).
    pub cells: Vec<u8>,
    /// Player column (`DAT_801f35c8`).
    pub player_col: u8,
    /// Player row (`DAT_801f35cc`).
    pub player_row: u8,
}

impl FieldGrid {
    /// Build a grid from raw parts. `cells` must be `width * height`
    /// bytes (row-major); shorter inputs read as walls past their end
    /// via [`Self::cell`] returning `None`.
    pub fn new(width: u8, height: u8, origin_x: u8, origin_z: u8, cells: Vec<u8>) -> Self {
        Self {
            width,
            height,
            origin_x,
            origin_z,
            cells,
            player_col: 0,
            player_row: 0,
        }
    }

    /// Cell value at `(col, row)`, or `None` when out of bounds or past
    /// the end of the `cells` buffer.
    pub fn cell(&self, col: i32, row: i32) -> Option<u8> {
        if col < 0 || row < 0 || col >= self.width as i32 || row >= self.height as i32 {
            return None;
        }
        let idx = row as usize * self.width as usize + col as usize;
        self.cells.get(idx).copied()
    }

    /// Retail collision rule: a move into `(col, row)` is blocked when
    /// the cell is out of bounds or its value is [`CELL_WALL`].
    pub fn is_blocked(&self, col: i32, row: i32) -> bool {
        match self.cell(col, row) {
            None => true,
            Some(v) => v == CELL_WALL,
        }
    }

    /// World `(x, z)` position of a tile centre:
    /// `world = (origin + idx) * TILE + TILE_CENTER`.
    pub fn tile_world(&self, col: i32, row: i32) -> (i32, i32) {
        (
            (self.origin_x as i32 + col) * TILE + TILE_CENTER,
            (self.origin_z as i32 + row) * TILE + TILE_CENTER,
        )
    }

    /// World position of the player's current cell centre.
    pub fn player_world(&self) -> (i32, i32) {
        self.tile_world(self.player_col as i32, self.player_row as i32)
    }

    /// Candidate `(col, row)` one step from the player in `dir`. May be
    /// out of bounds (negative or past the edge); callers gate it
    /// through [`Self::is_blocked`].
    pub fn neighbor(&self, dir: WalkDir) -> (i32, i32) {
        let c = self.player_col as i32;
        let r = self.player_row as i32;
        match dir {
            WalkDir::Up => (c, r - 1),
            WalkDir::Down => (c, r + 1),
            WalkDir::Left => (c - 1, r),
            WalkDir::Right => (c + 1, r),
        }
    }

    /// Attempt a one-cell step in `dir`. On success, commit the player
    /// cell to the destination (matching retail's `DAT_801f35c8/cc =`
    /// at decision time) and return the destination's world-position
    /// target the actor interpolates toward. Returns `None` when the
    /// step is blocked - the player stays put.
    pub fn try_step(&mut self, dir: WalkDir) -> Option<(i32, i32)> {
        let (col, row) = self.neighbor(dir);
        if self.is_blocked(col, row) {
            return None;
        }
        self.player_col = col as u8;
        self.player_row = row as u8;
        Some(self.tile_world(col, row))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 3x3 grid, all floor (cell 1) except the centre column row 1 is a
    /// wall (cell 2). Player starts at (0,0).
    fn grid_3x3() -> FieldGrid {
        // row-major: (col,row)
        // row0: 1 1 1
        // row1: 1 2 1
        // row2: 1 1 1
        let cells = vec![1, 1, 1, 1, CELL_WALL, 1, 1, 1, 1];
        FieldGrid::new(3, 3, 0, 0, cells)
    }

    #[test]
    fn cell_out_of_bounds_is_none() {
        let g = grid_3x3();
        assert_eq!(g.cell(0, 0), Some(1));
        assert_eq!(g.cell(1, 1), Some(CELL_WALL));
        assert_eq!(g.cell(-1, 0), None);
        assert_eq!(g.cell(3, 0), None);
        assert_eq!(g.cell(0, 3), None);
    }

    #[test]
    fn wall_and_oob_block() {
        let g = grid_3x3();
        assert!(g.is_blocked(1, 1)); // wall
        assert!(g.is_blocked(-1, 0)); // oob
        assert!(g.is_blocked(3, 3)); // oob
        assert!(!g.is_blocked(0, 0)); // floor
        assert!(!g.is_blocked(2, 2)); // floor
    }

    #[test]
    fn tile_world_centres_on_tile() {
        let g = FieldGrid::new(4, 4, 2, 5, vec![1; 16]);
        // (origin + idx) * 0x80 + 0x40
        assert_eq!(g.tile_world(0, 0), (2 * 0x80 + 0x40, 5 * 0x80 + 0x40));
        assert_eq!(g.tile_world(1, 1), (3 * 0x80 + 0x40, 6 * 0x80 + 0x40));
    }

    #[test]
    fn step_into_floor_commits_and_returns_target() {
        let mut g = grid_3x3();
        // (0,0) -> Right -> (1,0) floor
        let target = g.try_step(WalkDir::Right);
        assert_eq!((g.player_col, g.player_row), (1, 0));
        assert_eq!(target, Some(g.tile_world(1, 0)));
    }

    #[test]
    fn step_into_wall_is_rejected() {
        let mut g = grid_3x3();
        // move to (1,0) then Down into the (1,1) wall.
        g.try_step(WalkDir::Right);
        assert_eq!((g.player_col, g.player_row), (1, 0));
        let blocked = g.try_step(WalkDir::Down);
        assert_eq!(blocked, None);
        // stayed put
        assert_eq!((g.player_col, g.player_row), (1, 0));
    }

    #[test]
    fn step_off_edge_is_rejected() {
        let mut g = grid_3x3();
        // (0,0) -> Up would be (0,-1), out of bounds.
        assert_eq!(g.try_step(WalkDir::Up), None);
        assert_eq!((g.player_col, g.player_row), (0, 0));
        // Left also oob.
        assert_eq!(g.try_step(WalkDir::Left), None);
        assert_eq!((g.player_col, g.player_row), (0, 0));
    }
}
