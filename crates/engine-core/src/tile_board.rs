//! Tile-board grid movement + collision (puzzle / board minigame mode).
//!
//! PORT: walk state machine `overlay_0897_801ef2b0`; grid header install
//! field-VM op `0x49` (`overlay_0897_801de840`, `_DAT_8007b450`).
//!
//! This is **not** general town/field locomotion (Legaia towns use free
//! movement). It is the discrete tile-board mode used by puzzle rooms /
//! board minigames within the field overlay: the board is a
//! `width × height` array of byte cells, the player occupies one
//! `(col, row)` cell, and each accepted d-pad press advances exactly one
//! cell. The cell array *is* the collision data - a destination cell
//! value of [`CELL_WALL`] (`2`) is a wall. Each cell value also indexes
//! a tile-actor table the board renderer draws. See
//! [`docs/subsystems/tile-board.md`](../../../docs/subsystems/tile-board.md).
//!
//! The board is installed inline in the field-VM event script by op
//! `0x49`; some instances are procedurally generated (the board filler
//! `overlay_0897_801e0b1c` seeds cells from BIOS `rand`). This module
//! models the runtime view the walk SM consumes, not the on-disc /
//! generated fill.

/// World units per tile (`0x80`). Retail multiplies the tile index by
/// this when mapping a cell to a world position.
pub const TILE: i32 = 0x80;

/// Half-tile offset placing the actor at the tile centre (`0x40`).
pub const TILE_CENTER: i32 = 0x40;

/// Cell value that blocks movement. The walk SM rejects a step whose
/// destination cell equals this (`overlay_0897_801ef2b0` case 4).
pub const CELL_WALL: u8 = 2;

/// Trigger cell - the arrival sub-state routes to the event handler.
pub const CELL_TRIGGER: u8 = 7;

/// First event / transition cell value (`8..=0xA`); arrival reads the
/// header `+7`/`+9` flag operands and leaves the board mode.
pub const CELL_EVENT_FIRST: u8 = 8;

/// Last event / transition cell value.
pub const CELL_EVENT_LAST: u8 = 0xA;

/// First animated-tile value; arrival cycles `0xB -> 0xE -> 0xB`.
pub const CELL_ANIM_FIRST: u8 = 0x0B;

/// Last animated-tile value.
pub const CELL_ANIM_LAST: u8 = 0x0E;

/// The inline board header a field-VM op `0x49` sub-op `5` points
/// `_DAT_8007b450` at. The window starts at the sub-op byte (`+0`); the
/// confirmed fields follow at `+1..+0xC` (13 bytes total - the Done arm
/// advances the script `sub-op + 13` bytes past the header). See
/// [`docs/subsystems/tile-board.md`](../../../docs/subsystems/tile-board.md).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TileBoardHeader {
    /// World tile origin X (`+1`).
    pub origin_x: u8,
    /// World tile origin Z (`+2`).
    pub origin_z: u8,
    /// Board width in columns (`+3`).
    pub width: u8,
    /// Board height in rows (`+4`).
    pub height: u8,
    /// Draw/scan radius around the player (`+5`).
    pub radius: u8,
    /// Mode flag: full-board draw vs. windowed draw (`+6`).
    pub mode_flag: u8,
    /// Player actor template id (`+0xb`).
    pub player_template: u8,
    /// Tile-actor template base id (`+0xc`), one per drawable cell value.
    pub tile_template_base: u8,
}

/// Byte length of the header window (`sub-op + 12 field bytes`); the op-49
/// Done arm advances `header_size + 13` past it.
pub const HEADER_LEN: usize = 13;

impl TileBoardHeader {
    /// Parse the header from the op-49 operand window (`window[0]` is the
    /// sub-op byte the retail pointer lands on). `None` when the window is
    /// short or the board dimensions are degenerate (`width * height == 0`).
    pub fn parse(window: &[u8]) -> Option<Self> {
        if window.len() < HEADER_LEN {
            return None;
        }
        let h = Self {
            origin_x: window[1],
            origin_z: window[2],
            width: window[3],
            height: window[4],
            radius: window[5],
            mode_flag: window[6],
            player_template: window[0xb],
            tile_template_base: window[0xc],
        };
        if h.width == 0 || h.height == 0 {
            return None;
        }
        Some(h)
    }
}

/// The retail procedural board fill (`overlay_0897_801e0b1c`), cells only
/// (the tile-actor spawns from the header template ids are host concerns).
/// `rand` supplies the BIOS `rand` draws (`func_0x80056798`, non-negative
/// 15-bit) in retail call order:
///
/// 1. every cell seeds `rand() % 6 + 2` (wall `2` + terrain `3..=6` +
///    trigger `7`),
/// 2. four animated tiles: `board[rand() % (w*h)] = 0xB + i`,
/// 3. three event tiles `8..=0xA` at `col = rand() % w`,
///    `row = rand() % ((h+1)>>1) + (h>>1)` (the bottom half-board).
///
/// Later scatters may land on earlier ones, exactly as retail's do.
pub fn procedural_fill(width: u8, height: u8, mut rand: impl FnMut() -> u32) -> Vec<u8> {
    let w = width as u32;
    let n = w * height as u32;
    let mut cells: Vec<u8> = (0..n).map(|_| (rand() % 6 + 2) as u8).collect();
    if n == 0 {
        return cells;
    }
    for i in 0..4u32 {
        let at = (rand() % n) as usize;
        cells[at] = (CELL_ANIM_FIRST as u32 + i) as u8;
    }
    let half = (height as u32 + 1) >> 1;
    for v in CELL_EVENT_FIRST..=CELL_EVENT_LAST {
        let col = rand() % w;
        let row = rand() % half + (height as u32 >> 1);
        cells[(row * w + col) as usize] = v;
    }
    cells
}

/// One of the four grid-step directions. The retail walk SM decodes
/// these from the camera-facing-remapped pad
/// (`func_0x800467e8` then mask bits `0x1000`/`0x2000`/`0x4000`/`0x8000`);
/// callers map screen d-pad directions to board axes here. The
/// camera-relative remap is not yet ported (the facing transform is an
/// open RE item), so this is the camera-neutral mapping: screen-up
/// decrements the row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TileStep {
    Up,
    Down,
    Left,
    Right,
}

/// Runtime tile board: dimensions, world-tile origin, the mutable cell
/// array, and the player's logical cell. Mirrors the runtime state the
/// walk SM reads (`DAT_801f35c0` cells, `_DAT_8007b450` header fields,
/// `DAT_801f35c8/cc` player cell).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TileBoard {
    /// Board width in columns (`_DAT_8007b450 + 3`).
    pub width: u8,
    /// Board height in rows (`_DAT_8007b450 + 4`).
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

impl TileBoard {
    /// Build a board from raw parts. `cells` must be `width * height`
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

    /// Build the runtime board a parsed op-49 header describes, with the
    /// given cell fill (see [`procedural_fill`]).
    pub fn from_header(header: &TileBoardHeader, cells: Vec<u8>) -> Self {
        Self::new(
            header.width,
            header.height,
            header.origin_x,
            header.origin_z,
            cells,
        )
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
    pub fn neighbor(&self, dir: TileStep) -> (i32, i32) {
        let c = self.player_col as i32;
        let r = self.player_row as i32;
        match dir {
            TileStep::Up => (c, r - 1),
            TileStep::Down => (c, r + 1),
            TileStep::Left => (c - 1, r),
            TileStep::Right => (c + 1, r),
        }
    }

    /// Attempt a one-cell step in `dir`. On success, commit the player
    /// cell to the destination (matching retail's `DAT_801f35c8/cc =`
    /// at decision time) and return the destination's world-position
    /// target the actor interpolates toward. Returns `None` when the
    /// step is blocked - the player stays put.
    pub fn try_step(&mut self, dir: TileStep) -> Option<(i32, i32)> {
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

    /// 3x3 board, all floor (cell 1) except the centre column row 1 is a
    /// wall (cell 2). Player starts at (0,0).
    fn board_3x3() -> TileBoard {
        // row-major: (col,row)
        // row0: 1 1 1
        // row1: 1 2 1
        // row2: 1 1 1
        let cells = vec![1, 1, 1, 1, CELL_WALL, 1, 1, 1, 1];
        TileBoard::new(3, 3, 0, 0, cells)
    }

    #[test]
    fn cell_out_of_bounds_is_none() {
        let b = board_3x3();
        assert_eq!(b.cell(0, 0), Some(1));
        assert_eq!(b.cell(1, 1), Some(CELL_WALL));
        assert_eq!(b.cell(-1, 0), None);
        assert_eq!(b.cell(3, 0), None);
        assert_eq!(b.cell(0, 3), None);
    }

    #[test]
    fn wall_and_oob_block() {
        let b = board_3x3();
        assert!(b.is_blocked(1, 1)); // wall
        assert!(b.is_blocked(-1, 0)); // oob
        assert!(b.is_blocked(3, 3)); // oob
        assert!(!b.is_blocked(0, 0)); // floor
        assert!(!b.is_blocked(2, 2)); // floor
    }

    #[test]
    fn tile_world_centres_on_tile() {
        let b = TileBoard::new(4, 4, 2, 5, vec![1; 16]);
        // (origin + idx) * 0x80 + 0x40
        assert_eq!(b.tile_world(0, 0), (2 * 0x80 + 0x40, 5 * 0x80 + 0x40));
        assert_eq!(b.tile_world(1, 1), (3 * 0x80 + 0x40, 6 * 0x80 + 0x40));
    }

    #[test]
    fn step_into_floor_commits_and_returns_target() {
        let mut b = board_3x3();
        // (0,0) -> Right -> (1,0) floor
        let target = b.try_step(TileStep::Right);
        assert_eq!((b.player_col, b.player_row), (1, 0));
        assert_eq!(target, Some(b.tile_world(1, 0)));
    }

    #[test]
    fn step_into_wall_is_rejected() {
        let mut b = board_3x3();
        // move to (1,0) then Down into the (1,1) wall.
        b.try_step(TileStep::Right);
        assert_eq!((b.player_col, b.player_row), (1, 0));
        let blocked = b.try_step(TileStep::Down);
        assert_eq!(blocked, None);
        // stayed put
        assert_eq!((b.player_col, b.player_row), (1, 0));
    }

    #[test]
    fn header_parse_reads_confirmed_fields() {
        // [sub_op=5][ox][oz][w][h][radius][mode][flag lo][flag hi][flag lo]
        // [flag hi][player_tpl][tile_base]
        let window = [5u8, 3, 7, 6, 4, 2, 1, 0, 0, 0, 0, 0x21, 0x30];
        let h = TileBoardHeader::parse(&window).expect("13-byte header parses");
        assert_eq!((h.origin_x, h.origin_z), (3, 7));
        assert_eq!((h.width, h.height), (6, 4));
        assert_eq!((h.radius, h.mode_flag), (2, 1));
        assert_eq!((h.player_template, h.tile_template_base), (0x21, 0x30));
        // Short window / zero dims reject.
        assert_eq!(TileBoardHeader::parse(&window[..12]), None);
        let mut degenerate = window;
        degenerate[3] = 0;
        assert_eq!(TileBoardHeader::parse(&degenerate), None);
    }

    #[test]
    fn procedural_fill_matches_retail_value_classes() {
        // Deterministic 15-bit LCG standing in for BIOS rand.
        let mut seed = 0x1234u32;
        let mut rand = move || {
            seed = seed.wrapping_mul(0x41C6_4E6D).wrapping_add(0x3039);
            (seed >> 16) & 0x7FFF
        };
        let (w, h) = (8u8, 6u8);
        let cells = procedural_fill(w, h, &mut rand);
        assert_eq!(cells.len(), 48);
        // Every cell is a retail value class: base seed 2..=7, animated
        // 0xB..=0xE, or event 8..=0xA.
        assert!(cells.iter().all(|&c| (2..=0xE).contains(&c)));
        // The three event tiles land in the bottom half-board unless a later
        // event scatter collides; at least one always survives (they are the
        // final writes).
        let bottom_rows = (h as usize >> 1)..h as usize;
        let event_in_bottom = cells
            .iter()
            .enumerate()
            .filter(|&(_, &c)| (CELL_EVENT_FIRST..=CELL_EVENT_LAST).contains(&c))
            .all(|(i, _)| bottom_rows.contains(&(i / w as usize)));
        assert!(event_in_bottom, "event tiles scatter into the bottom half");
        assert!(
            cells
                .iter()
                .any(|&c| (CELL_EVENT_FIRST..=CELL_EVENT_LAST).contains(&c)),
            "at least the last event tile survives"
        );
    }

    #[test]
    fn step_off_edge_is_rejected() {
        let mut b = board_3x3();
        // (0,0) -> Up would be (0,-1), out of bounds.
        assert_eq!(b.try_step(TileStep::Up), None);
        assert_eq!((b.player_col, b.player_row), (0, 0));
        // Left also oob.
        assert_eq!(b.try_step(TileStep::Left), None);
        assert_eq!((b.player_col, b.player_row), (0, 0));
    }
}
