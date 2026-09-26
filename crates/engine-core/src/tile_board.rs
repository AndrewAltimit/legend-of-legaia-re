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
//! arm at `0x801EF334` inside `FUN_801EF2B0` seeds cells from BIOS
//! `rand`; `0x801E0B1C` is that arm printed `0xE818` low). This module
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

/// First cell value that draws a tile actor. Retail draws every cell whose
/// value indexes the tile-actor table above the player/floor slots
/// (`overlay_0897_801e0f3c` draws each cell `> 1`); the wall value `2` is
/// itself a drawn tile.
pub const CELL_DRAW_FIRST: u8 = 2;

/// Last cell value that draws a tile actor (top of the tile-actor table).
pub const CELL_DRAW_LAST: u8 = 0x0E;

/// Entry count of the per-cell-value tile-actor table (`DAT_801f35bc`,
/// `0x3c` bytes = 15 word-sized pointers). Index `0` is the player actor,
/// `2..=14` the per-value tile actors; index `1` (plain floor) is unused.
pub const TILE_ACTOR_TABLE_LEN: usize = 15;

/// Whether a cell value is drawn as a tile actor (`CELL_DRAW_FIRST..=CELL_DRAW_LAST`).
pub fn is_drawable_cell(value: u8) -> bool {
    (CELL_DRAW_FIRST..=CELL_DRAW_LAST).contains(&value)
}

/// The tile-actor template id for a drawable cell `value`: the header
/// `+0xc` base plus `value - 2` (`overlay_0897_801e0f3c`). Only meaningful
/// for [`is_drawable_cell`] values.
pub fn tile_template_for(base: u8, value: u8) -> u8 {
    base.wrapping_add(value - CELL_DRAW_FIRST)
}

/// One entry in the per-frame tile-board draw list: the tile actor to draw
/// and the world-centre coordinate to draw it at. The reposition pass
/// ([`crate::World::tick`]'s field arm) produces one per drawn cell from
/// the [`TileBoard::draw_cells`] set and the tile-actor table; the deferred
/// renderer draws the actor's mesh at each listed cell (retail's
/// `overlay_0897_801e0f3c` repositions the selected actor to each cell
/// centre and draws it in place).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TileDraw {
    /// Board column of this drawn cell.
    pub col: u8,
    /// Board row of this drawn cell.
    pub row: u8,
    /// Cell value (`2..=14`) selecting the tile actor from the table.
    pub cell_value: u8,
    /// Actor-pool slot of the selected tile actor.
    pub slot: u8,
    /// World X of the tile centre (`(origin_x + col) * 0x80 + 0x40`).
    pub world_x: i32,
    /// World Z of the tile centre (`(origin_z + row) * 0x80 + 0x40`).
    pub world_z: i32,
}

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
    /// Event-flag base A (`+7..+8`, read by `FUN_8003CE9C` as a
    /// sign-extended little-endian halfword): the SET base the event-cell
    /// arrival writes from.
    pub flag_base_set: i16,
    /// Event-flag base B (`+9..+0xA`, same reader): the TEST base the
    /// event-cell arrival gates on.
    pub flag_base_test: i16,
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
            flag_base_set: i16::from_le_bytes([window[7], window[8]]),
            flag_base_test: i16::from_le_bytes([window[9], window[0xa]]),
            player_template: window[0xb],
            tile_template_base: window[0xc],
        };
        if h.width == 0 || h.height == 0 {
            return None;
        }
        Some(h)
    }
}

/// The walk SM's state halfword (the controller actor's `+0x54`), for the
/// states the port keeps as their own phase. Values are retail's.
pub mod sm {
    /// Fade-in (`0x801EF680`): input ignored while the tiles scale up.
    pub const FADE_IN: u8 = 1;
    /// Walking - reading input (state 4) or interpolating (state 2); the
    /// port keeps the two apart with `TileBoardState::target`.
    pub const WALK: u8 = 4;
    /// The quit prompt (`0x801EFBD0`).
    pub const PROMPT: u8 = 5;
    /// Quit confirmed (`0x801EFC2C`, shared with `7`): `-> 9`.
    pub const QUIT: u8 = 6;
    /// Trigger-cell exit (`0x801EFC2C`): `-> 9`.
    pub const TRIGGER_EXIT: u8 = 7;
    /// One-tick step before the exit fade (`0x801EFCD0`): `+= 1`.
    pub const EXIT_STEP: u8 = 9;
    /// The exit fade-out (`0x801EFCE4`).
    pub const FADE_OUT: u8 = 0xA;
    /// One-tick step before the event exit's fade (`0x801EFCD0`).
    pub const EVENT_STEP: u8 = 0xB;
    /// The event exit's fade-out (`0x801EFCE4`, shared with `0xA`).
    pub const EVENT_FADE_OUT: u8 = 0xC;
    /// Park every tile actor off-board (`0x801EFDA8`): `-> 0xE`.
    pub const PARK: u8 = 0xD;
    /// Teardown (`0x801EFE64`).
    pub const TEARDOWN: u8 = 0xE;
}

/// Full tile scale, `+0x72 = 0x1000` (4.12 fixed point).
pub const FADE_FULL: i16 = 0x1000;

/// One fade-in tick (`0x801EF680..0x801EF6F0`): `+0x9C += (d * 3) << 5`,
/// clamped to [`FADE_FULL`]; `true` once it reached it (then `-> 2`, the
/// walk-in). `d` is `DAT_1F800393`.
pub fn fade_in_step(fade: i16, d: u8) -> (i16, bool) {
    let next = i32::from(fade) + ((i32::from(d) * 3) << 5);
    if next >= i32::from(FADE_FULL) {
        (FADE_FULL, true)
    } else {
        (next as i16, false)
    }
}

/// One fade-out tick (`0x801EFCE4..0x801EFD48`): `+0x9C -= d << 8`; below
/// zero it clamps to `0` and reports done (then `-> 0xD`).
pub fn fade_out_step(fade: i16, d: u8) -> (i16, bool) {
    let next = i32::from(fade) - (i32::from(d) << 8);
    if next < 0 {
        (0, true)
    } else {
        (next as i16, false)
    }
}

/// The tile value the exit fade and park leave alone: when the player
/// stands on an event cell (`8..=0xA`), its own tile actor keeps its scale
/// and position (`0x801EFD50..0x801EFD7C`, the same test in `0x801EFDA8`).
pub fn fade_exempt_value(cell_under_player: Option<u8>) -> Option<u8> {
    cell_under_player.filter(|c| (CELL_EVENT_FIRST..=CELL_EVENT_LAST).contains(c))
}

/// Where the quit prompt goes on a picker result (`FUN_801E9DC8`'s return
/// read at `0x801EFBF0..0x801EFC24`): confirm on row `0` quits, confirm on
/// row `1` and cancel go back to walking, anything else stays.
pub fn prompt_next(nav: crate::menu_input::CursorNav, cursor: u32) -> u8 {
    use crate::menu_input::CursorNav;
    match nav {
        CursorNav::Confirm if cursor & crate::menu_input::CURSOR_INDEX_MASK == 0 => sm::QUIT,
        CursorNav::Confirm | CursorNav::Cancel => sm::WALK,
        _ => sm::PROMPT,
    }
}

/// The quit prompt's panel (render tail `0x801EFED8..0x801EFFA8`): the
/// frame `FUN_8002C69C(100, 92, 120, 40)`, the title pen at `(100, 92)`,
/// the two rows at `(152, 105)` / `(152, 118)`, the hand cursor
/// (`FUN_8002B994`) at `x = 132` on the selected row. The strings are the
/// field overlay's own (`0x801CF650` title, `0x801CF10C` / `0x801CF110`
/// rows); the port draws the panel's geometry, not its text.
pub const PROMPT_FRAME: (i16, i16, i16, i16) = (100, 92, 120, 40);
/// The two rows' pen Y (`105`, `118`).
pub const PROMPT_ROW_Y: [i16; 2] = [105, 118];
/// Both rows' pen X.
pub const PROMPT_ROW_X: i16 = 152;
/// The hand cursor's X.
pub const PROMPT_CURSOR_X: i16 = 132;

/// `(frame rect, the two row pens, cursor x)`, in stage pixels.
pub type PromptLayout = ((i32, i32, i32, i32), [(i32, i32); 2], i32);

/// The prompt panel's geometry as the shared UI builder takes it
/// (`legaia_engine_ui::TileBoardPromptLayout`'s fields, in stage pixels):
/// `(frame, rows, cursor_x)`.
pub fn prompt_layout() -> PromptLayout {
    let (x, y, w, h) = PROMPT_FRAME;
    let row = |i: usize| (i32::from(PROMPT_ROW_X), i32::from(PROMPT_ROW_Y[i]));
    (
        (i32::from(x), i32::from(y), i32::from(w), i32::from(h)),
        [row(0), row(1)],
        i32::from(PROMPT_CURSOR_X),
    )
}

/// The field overlay's load base (PROT 0897, slot A).
const FIELD_OVERLAY_BASE: u32 = 0x801C_E818;
/// VAs of the prompt's three strings in the field overlay's data segment:
/// the title and the two rows (the `a0` of the render tail's three
/// `FUN_80036888` calls).
pub const PROMPT_STRING_VAS: [u32; 3] = [0x801C_F650, 0x801C_F10C, 0x801C_F110];

/// Read the prompt's three NUL-terminated strings out of the field overlay
/// image (PROT entry `0897`, as the disc holds it). `None` when the image
/// is too short to hold them.
pub fn prompt_strings(field_overlay: &[u8]) -> Option<[Vec<u8>; 3]> {
    let read = |va: u32| -> Option<Vec<u8>> {
        let off = va.checked_sub(FIELD_OVERLAY_BASE)? as usize;
        let tail = field_overlay.get(off..)?;
        let end = tail.iter().position(|&b| b == 0)?;
        Some(tail[..end].to_vec())
    };
    Some([
        read(PROMPT_STRING_VAS[0])?,
        read(PROMPT_STRING_VAS[1])?,
        read(PROMPT_STRING_VAS[2])?,
    ])
}

/// What the walk SM's arrival state (state 3) does with the cell the player
/// just reached.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArrivalAction {
    /// Cell `7`: leave the board with no flag write (states `7 -> 9 -> 0xA`
    /// fade, `0xD` park, `0xE` teardown).
    ExitTrigger,
    /// Cells `8..=0xA`: the event arm (state 8) writes the flags
    /// [`event_cell_flag_writes`] names, then leaves through the same fade
    /// (`0xB -> 0xC -> 0xD -> 0xE`).
    ExitEvent,
    /// Any other cell: advance every animated cell on the board one step
    /// ([`advance_animated_cells`]) and return to input (state 4).
    Continue,
}

/// Classify an arrival cell the way state 3 does (`0x801EF6FC..0x801EF760`):
/// `== 7` first, then `cell - 8 < 3` (unsigned).
pub fn arrival_action(cell: u8) -> ArrivalAction {
    if cell == CELL_TRIGGER {
        ArrivalAction::ExitTrigger
    } else if (CELL_EVENT_FIRST..=CELL_EVENT_LAST).contains(&cell) {
        ArrivalAction::ExitEvent
    } else {
        ArrivalAction::Continue
    }
}

/// The animated-cell pass of state 3 (`0x801EF764..0x801EF81C`): every cell
/// on the **whole board** whose value is in `0xB..=0xE` steps to the next
/// value, `0xE` wrapping to `0xB` (`v + 1`, then `0xB` when `v + 1 >= 0xF`).
/// Retail runs it on every arrival that is not a trigger or event cell, not
/// only on the arrived cell.
pub fn advance_animated_cells(cells: &mut [u8]) {
    for c in cells {
        if (CELL_ANIM_FIRST..=CELL_ANIM_LAST).contains(c) {
            let next = *c + 1;
            *c = if next < 0x0F { next } else { CELL_ANIM_FIRST };
        }
    }
}

/// The event-cell flag writes of walk-SM state 8 (`0x801EFC38..0x801EFCC8`).
///
/// With `v = cell - 8`, `a = header +7` and `b = header +9` (both through the
/// sign-extending halfword reader `FUN_8003CE9C`), retail first **sets**
/// system flag `a + v + 1` (`FUN_8003CE08`), then **tests** flag `b + v`
/// (`FUN_8003CE64`) and, when it is clear, also sets flag `a`. Returns the
/// indices to set in order; `test` answers the system-flag test.
pub fn event_cell_flag_writes(
    header: &TileBoardHeader,
    cell: u8,
    mut test: impl FnMut(u16) -> bool,
) -> Vec<u16> {
    let v = i32::from(cell) - i32::from(CELL_EVENT_FIRST);
    let a = i32::from(header.flag_base_set);
    let b = i32::from(header.flag_base_test);
    let mut out = vec![(a + v + 1) as u16];
    if !test((b + v) as u16) {
        out.push(a as u16);
    }
    out
}

/// The cell state 0 seats the player on: column `4`, row `0` (`0x801EF630`
/// `sw 4 -> DAT_801F35C8`, `0x801EF640` `sw 0 -> DAT_801F35CC`), with the
/// walk-in target `(hdr[1] * 128 + 0x240, hdr[2] * 128 + 0x40)` - that
/// cell's centre. The player walks there from wherever it stood (state `2`
/// after the fade-in), and the arrival pass (state `3`) then runs on it.
pub const START_COL: u8 = 4;
/// See [`START_COL`].
pub const START_ROW: u8 = 0;

/// The walker's octant store (`0x801EF8A4..0x801EF8CC`, state `4`, off the
/// cell under the player): the delay-slot clear always runs, terrain cells
/// `3..=6` then store `(cell - 3) * 2`, and the animated band `0xB..=0xE`
/// stores the **same** expression - `v1` is still `cell - 3` - so it writes
/// `16..=22`, which the remapper's `& 7` folds onto `0, 2, 4, 6`. Every
/// other cell leaves `0`. The raw stored value is returned.
pub fn walker_octant(cell: u8) -> u32 {
    let v1 = u32::from(cell).wrapping_sub(3);
    if (3..=6).contains(&cell) || (CELL_ANIM_FIRST..=CELL_ANIM_LAST).contains(&cell) {
        v1 << 1
    } else {
        0
    }
}

/// Decode one step from the octant-remapped pad mask (`0x801EF8D8..0x801EF918`),
/// in retail's priority order: `0x1000` row `+1`, `0x4000` row `-1`,
/// `0x2000` column `+1`, `0x8000` column `-1`. `None` for no direction.
pub fn step_for_mask(mask: u16) -> Option<TileStep> {
    if mask & 0x1000 != 0 {
        Some(TileStep::Down)
    } else if mask & 0x4000 != 0 {
        Some(TileStep::Up)
    } else if mask & 0x2000 != 0 {
        Some(TileStep::Right)
    } else if mask & 0x8000 != 0 {
        Some(TileStep::Left)
    } else {
        None
    }
}

/// The retail procedural board fill (the `0x801EF334` arm of `FUN_801EF2B0`), cells only
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

/// One of the four grid-step directions, named for the row / column delta
/// ([`TileBoard::neighbor`]): `Up` decrements the row, `Down` increments
/// it. The walk SM reaches them from the octant-remapped pad mask through
/// [`step_for_mask`] - which is where screen directions meet board axes.
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

    /// The `(col, row)` cells the board scans/draws this frame, in
    /// row-major order (`overlay_0897_801e0f3c`; header `+6` picks the pass,
    /// `+5` the radius):
    ///
    /// - `mode_flag == 0` (**full-board**): every in-bounds cell.
    /// - `mode_flag != 0` (**windowed**): the square window of Chebyshev
    ///   `radius` cells around the player, clamped to the board edges.
    ///
    /// Purely geometric - the caller filters each cell by
    /// [`is_drawable_cell`] and looks up its tile actor.
    pub fn draw_cells(&self, mode_flag: u8, radius: u8) -> Vec<(i32, i32)> {
        let (w, h) = (self.width as i32, self.height as i32);
        let (c0, c1, r0, r1) = if mode_flag == 0 {
            (0, w - 1, 0, h - 1)
        } else {
            let rad = radius as i32;
            let (pc, pr) = (self.player_col as i32, self.player_row as i32);
            (
                (pc - rad).max(0),
                (pc + rad).min(w - 1),
                (pr - rad).max(0),
                (pr + rad).min(h - 1),
            )
        };
        let mut out = Vec::new();
        for row in r0..=r1 {
            for col in c0..=c1 {
                out.push((col, row));
            }
        }
        out
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

// ---------------------------------------------------------------------
// Per-frame draw assembly
// ---------------------------------------------------------------------
//
// These three lived in `legaia-engine-shell`, which pulls winit and cpal and
// therefore does not build for wasm32 - so the browser play page had no way
// to reach them and drew no board at all. `World` maintains
// `tile_board_draw_list` for both hosts (the field VM's op `0x49` installs
// the board regardless of who is rendering), so a scene that installs one on
// the play page produced an invisible board: the walk SM still refuses the
// wall cells, which is a walk into nothing rather than a cosmetic gap.
//
// They are pure `&World -> Vec<..>` with no GPU, glam or serde in sight, and
// they sit here rather than in `engine-ui` because `engine-ui` deliberately
// does not depend on `engine-core` - its contract is view-struct in,
// `TextDraw` out, and these take a `World`.

/// One tile-actor mesh instance for this frame: the actor at `slot` draws
/// at `world` (a drawable cell's tile centre, floor-snapped like the field
/// NPC draws). A cell value repeated across cells yields multiple draws
/// sharing one `slot` - the per-cell instancing the shared actor can't
/// carry in its own transform.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TileActorDraw {
    /// Actor-pool slot of the cell value's tile actor.
    pub slot: u8,
    /// The drawn cell's value (`2..=14`).
    pub cell_value: u8,
    /// World-space draw position `(x, y, z)` in the retail Y-down field
    /// frame (the same convention the field NPC / placement draws use).
    pub world: [f32; 3],
    /// Uniform scale, the tile actor's `+0x72` over `0x1000`: the board's
    /// fade grows the tiles in at install and shrinks them away at exit.
    pub scale: f32,
}

/// Assemble the per-cell tile-actor draw set from the world's per-frame
/// tile-board draw list. Empty when no board is installed. Slots whose
/// actor is gone (despawned mid-frame) are skipped - unresolved templates
/// degrade to "no draw", never a panic.
///
/// This is the port site for the board's draw pass, and the only one: both
/// hosts call it (`play-window`'s redraw and the browser play page's
/// `play_tile_board`). The `engine-shell` module of the same name is a bare
/// re-export kept for the native bin's import path.
///
/// Provenance caveat - the dump this cites is a **wrong-base print**. The
/// board renderer is not a function: it is the tail block of the walk SM
/// `FUN_801EF2B0` at `0x801EFEA0`, entered by nineteen in-body branches and
/// running to that routine's epilogue, so there is no render entry to name
/// (`overlay_0897_801efea0.txt`, and
/// [`docs/subsystems/tile-board.md`](../../../docs/subsystems/tile-board.md)
/// "The render tail"). `overlay_0897_801e0f3c.txt` is a second print of the
/// same instructions under a different load base; it is cited here because
/// it is the dump file the corpus carries for this pass, not because a
/// function begins at that address.
///
/// PORT: overlay_0897_801e0f3c (per-cell tile-actor draw pass; the select +
/// reposition halves live in `World::refresh_tile_board_draw_list`)
pub fn tile_board_actor_draws(world: &crate::world::World) -> Vec<TileActorDraw> {
    world
        .board
        .draw_list
        .iter()
        .filter(|d| world.actors.get(d.slot as usize).is_some_and(|a| a.active))
        .map(|d| {
            let y = world.sample_field_floor_height(d.world_x, d.world_z) as f32;
            TileActorDraw {
                slot: d.slot,
                cell_value: d.cell_value,
                world: [d.world_x as f32, y, d.world_z as f32],
                scale: world.tile_board_cell_scale(d.cell_value),
            }
        })
        .collect()
}

/// The distinct tile-actor slots in the active draw set whose actor carries
/// a resolved template mesh (`tmd_ref`), in first-seen order - the set the
/// renderer must upload before the per-cell draws can land. Unresolved
/// templates (empty `tmd_ref`) are excluded: they allocated a slot but have
/// nothing to upload.
pub fn tile_actor_slots_needing_mesh(world: &crate::world::World) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    for d in &world.board.draw_list {
        if out.contains(&d.slot) {
            continue;
        }
        if world
            .actors
            .get(d.slot as usize)
            .is_some_and(|a| a.active && a.tmd_ref.is_some())
        {
            out.push(d.slot);
        }
    }
    out
}

/// Whether actor-pool `slot` is a board-owned tile actor (a `2..=14` entry
/// of the tile-actor table). The generic per-actor draw loop skips these -
/// a tile actor draws once per cell through the deferred draw list, and its
/// own transform only holds the *last* repositioned cell. Table slot 0 (the
/// player) is not board-owned: the normal field path draws it.
pub fn is_tile_actor_slot(world: &crate::world::World, slot: usize) -> bool {
    (CELL_DRAW_FIRST..=CELL_DRAW_LAST)
        .any(|v| world.board.actor_slots[v as usize].is_some_and(|s| s as usize == slot))
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
    fn walker_octant_bands_alias_the_animated_cells() {
        assert_eq!(walker_octant(2), 0);
        assert_eq!(walker_octant(3), 0);
        assert_eq!(walker_octant(6), 6);
        assert_eq!(walker_octant(7), 0);
        assert_eq!(walker_octant(0x0B), 16);
        assert_eq!(walker_octant(0x0E) & 7, 6);
    }

    #[test]
    fn step_for_mask_takes_retail_priority() {
        assert_eq!(step_for_mask(0x1000), Some(TileStep::Down));
        assert_eq!(step_for_mask(0x3000), Some(TileStep::Down));
        assert_eq!(step_for_mask(0x6000), Some(TileStep::Up));
        assert_eq!(step_for_mask(0xA000), Some(TileStep::Right));
        assert_eq!(step_for_mask(0x8000), Some(TileStep::Left));
        assert_eq!(step_for_mask(0x0010), None);
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
        // The two flag bases are sign-extended LE halfwords (`FUN_8003CE9C`).
        let mut flagged = window;
        flagged[7..11].copy_from_slice(&[0x34, 0x12, 0xFE, 0xFF]);
        let hf = TileBoardHeader::parse(&flagged).unwrap();
        assert_eq!((hf.flag_base_set, hf.flag_base_test), (0x1234, -2));
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
    fn drawable_cells_and_template_mapping() {
        // Only 2..=14 draw a tile actor; floor (0/1) and out-of-table (>14) don't.
        assert!(!is_drawable_cell(0));
        assert!(!is_drawable_cell(1));
        assert!(is_drawable_cell(2));
        assert!(is_drawable_cell(0x0E));
        assert!(!is_drawable_cell(0x0F));
        // Template id = base + (value - 2).
        assert_eq!(tile_template_for(0x30, 2), 0x30);
        assert_eq!(tile_template_for(0x30, 3), 0x31);
        assert_eq!(tile_template_for(0x30, 0x0E), 0x30 + 12);
    }

    #[test]
    fn full_board_draw_set_covers_every_cell() {
        let b = TileBoard::new(3, 2, 0, 0, vec![1; 6]);
        let cells = b.draw_cells(0, 5);
        assert_eq!(cells.len(), 6);
        assert_eq!(cells[0], (0, 0));
        assert_eq!(cells[5], (2, 1));
    }

    #[test]
    fn windowed_draw_set_restricts_to_radius() {
        // 5x5 board, player at centre (2,2), radius 1 -> a 3x3 window.
        let mut b = TileBoard::new(5, 5, 0, 0, vec![1; 25]);
        b.player_col = 2;
        b.player_row = 2;
        let cells = b.draw_cells(1, 1);
        assert_eq!(cells.len(), 9);
        assert!(cells.contains(&(1, 1)));
        assert!(cells.contains(&(3, 3)));
        assert!(!cells.contains(&(0, 2)));
        assert!(!cells.contains(&(4, 2)));
        // The window clamps at the edge: player in the corner -> 2x2.
        b.player_col = 0;
        b.player_row = 0;
        assert_eq!(b.draw_cells(1, 1).len(), 4);
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
