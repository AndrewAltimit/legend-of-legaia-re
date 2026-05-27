//! Name-entry overlay - the menu screen the opening (`town01`) field script
//! opens so the player names the lead character.
//!
//! This is a clean-room port of the retail overlay's behaviour, not its bytes.
//! Retail reference (captured from the field/menu overlay at `0x801C0000`):
//!
//! - render fn `FUN_801E6B34` - draws the character grid, the working name,
//!   the blinking caret, and the box frames.
//! - state machine `FUN_801F03F0` - a `switch` over `struct+0x54` with a
//!   5-entry jump table at `0x801CF71C`: init (`0x801F0444`) -> interactive
//!   (`0x801F0480`) -> three confirm handlers (`0x801F095C`/`09C0`/`097C`).
//! - charset grid at `0x801F29F0` - six rows of seventeen bytes; `|` (0x7C)
//!   at columns 5 and 11 are non-selectable group separators.
//! - the committed name is written into the live character record's name
//!   field at record offset `+0x2A7` (record base `0x80084708 + n*0x414`),
//!   which surfaces at save-block offset `+0x86F` for slot 0.
//!
//! The interactive handler moves a linear cursor over a 7-row x 17-column
//! navigation space (`0x77` = 119 cells, wrapped modulo 119): rows 0..5 hold
//! the 102 selectable glyph cells, and row 6 is a control bar whose cells map
//! to Backspace / Space / End actions (the retail `0x66` / `0x64` / `0x65`
//! sentinel bytes). The d-pad deltas are `-17` (up), `+17` (down), `+1`
//! (right), `-1` (left); after each move the cursor skips any non-selectable
//! cell in the same direction. The retail name length is bounded by the
//! proportional-font pixel width (57 px); this port bounds it by glyph count
//! ([`NAME_MAX`], derived from the template's 10-byte name field).

/// Columns in the character grid (the retail grid is 17 wide).
pub const GRID_COLS: usize = 17;
/// Rows of selectable glyphs (rows 0..5 of the retail grid).
pub const CHAR_ROWS: usize = 6;
/// Total navigable rows, including the bottom control bar.
pub const NAV_ROWS: usize = 7;
/// Count of selectable glyph cells (`CHAR_ROWS * GRID_COLS`).
pub const CHAR_CELLS: usize = CHAR_ROWS * GRID_COLS;
/// Count of navigable cells (`NAV_ROWS * GRID_COLS`); the retail wrap modulus.
pub const NAV_CELLS: usize = NAV_ROWS * GRID_COLS;

/// Longest name the entry accepts, in glyphs. The template name field is 10
/// bytes NUL-padded ([`legaia_asset::new_game::NAME_LEN`]), so nine glyphs fit
/// with the terminator.
pub const NAME_MAX: usize = legaia_asset::new_game::NAME_LEN - 1;

/// The six selectable glyph rows, exactly as laid out in the retail charset
/// grid at `0x801F29F0`. `|` marks a non-selectable group separator (the
/// retail `0x7C` bytes at columns 5 and 11); blanks are selectable space cells.
pub const GRID: [&str; CHAR_ROWS] = [
    "ABCDE|abcde|12345",
    "FGHIJ|fghij|67890",
    "KLMNO|klmno|!?#%&",
    "PQRST|pqrst|.,'<>",
    "UVWXY|uvwxy|+-*/=",
    "Z    |z    |:;()~",
];

/// One bottom-bar action. The retail control row (grid row 6) tiles these
/// across its columns via the `0x66` / `0x64` / `0x65` sentinel bytes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Control {
    /// Non-selectable filler cell (the leading `0x00` columns).
    None,
    /// Delete the last glyph (retail `0x66`).
    Backspace,
    /// Append a space (retail `0x64`).
    Space,
    /// Finish - advance to the "Is this name okay?" confirm (retail `0x65`).
    End,
}

/// The control-bar row, column by column - the retail row-6 layout
/// (`00 00 | 66 x6 | 64 x6 | 65 x3`).
pub const CONTROL_ROW: [Control; GRID_COLS] = {
    use Control::*;
    [
        None, None, Backspace, Backspace, Backspace, Backspace, Backspace, Backspace, Space, Space,
        Space, Space, Space, Space, End, End, End,
    ]
};

/// Sub-state of the name-entry SM (collapsed from the retail 5 substates: the
/// init + interactive handlers map to [`Editing`](NameEntryState::Editing);
/// the three confirm handlers map to [`Confirm`](NameEntryState::Confirm) and
/// its commit / cancel exits).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NameEntryState {
    /// Interactive grid editing.
    Editing,
    /// "Is this name okay?" - Yes / No confirm prompt.
    Confirm,
    /// Player confirmed; the name is committed and the overlay should close.
    Done,
}

/// One frame of name-entry input. Engines map their pad to these semantic
/// edges (just-pressed, not held - the overlay steps one cell per press).
#[derive(Clone, Copy, Default, Debug)]
pub struct NameEntryInput {
    pub up: bool,
    pub down: bool,
    pub left: bool,
    pub right: bool,
    /// Cross / Circle - select the cell under the cursor (or confirm Yes/No).
    pub confirm: bool,
    /// Triangle - backspace shortcut while editing; cancel while confirming.
    pub cancel: bool,
}

/// Mutable name-entry session. Install on the world via
/// [`crate::world::World::open_name_entry`]; step it each frame with a
/// [`NameEntryInput`]; read [`NameEntry::state`] for `Done` to commit + close.
#[derive(Clone, Debug)]
pub struct NameEntry {
    /// Party slot whose record receives the committed name.
    pub char_index: usize,
    /// Working name buffer (at most [`NAME_MAX`] glyphs).
    pub name: String,
    /// Linear cursor over the `NAV_CELLS` navigation grid (`0..NAV_CELLS`).
    pub cursor: usize,
    /// Current sub-state.
    pub state: NameEntryState,
    /// Yes/No selection while in [`NameEntryState::Confirm`] (`true` = Yes).
    pub confirm_yes: bool,
}

impl NameEntry {
    /// Open a fresh entry for `char_index`, seeded with `initial` (typically
    /// the template's default name, e.g. `Vahn`). The cursor starts on the
    /// first selectable cell.
    ///
    /// PORT: FUN_801F03F0
    /// REF: FUN_801E6B34
    pub fn new(char_index: usize, initial: &str) -> Self {
        let mut e = Self {
            char_index,
            name: initial.chars().take(NAME_MAX).collect(),
            cursor: 0,
            state: NameEntryState::Editing,
            confirm_yes: true,
        };
        // Land on a selectable cell (cell 0 = 'A' already is, but stay robust).
        if !e.is_selectable(e.cursor) {
            e.cursor = e.skip_to_selectable(e.cursor, 1);
        }
        e
    }

    /// Grid row / column of a navigation cell.
    fn row_col(cell: usize) -> (usize, usize) {
        (cell / GRID_COLS, cell % GRID_COLS)
    }

    /// `true` when a navigation cell can host the cursor: a glyph cell that is
    /// not a `|` separator, or a non-`None` control cell.
    pub fn is_selectable(&self, cell: usize) -> bool {
        let (row, col) = Self::row_col(cell);
        if row < CHAR_ROWS {
            GRID[row].as_bytes().get(col).copied() != Some(b'|')
        } else {
            CONTROL_ROW.get(col).copied().unwrap_or(Control::None) != Control::None
        }
    }

    /// Walk from `cell` in `step` direction (+1 / -1), wrapping modulo
    /// [`NAV_CELLS`], until a selectable cell is found. Bounded by the grid
    /// size so a degenerate all-blank row can't loop forever.
    fn skip_to_selectable(&self, cell: usize, step: isize) -> usize {
        let mut c = cell;
        for _ in 0..NAV_CELLS {
            if self.is_selectable(c) {
                return c;
            }
            c = (c as isize + step).rem_euclid(NAV_CELLS as isize) as usize;
        }
        cell
    }

    /// The glyph under a char-row cell (`None` for separators / control rows).
    pub fn glyph_at(&self, cell: usize) -> Option<char> {
        let (row, col) = Self::row_col(cell);
        if row >= CHAR_ROWS {
            return None;
        }
        let b = *GRID[row].as_bytes().get(col)?;
        if b == b'|' { None } else { Some(b as char) }
    }

    /// The control action under a control-row cell.
    pub fn control_at(&self, cell: usize) -> Option<Control> {
        let (row, col) = Self::row_col(cell);
        if row < CHAR_ROWS {
            return None;
        }
        match CONTROL_ROW.get(col).copied().unwrap_or(Control::None) {
            Control::None => None,
            c => Some(c),
        }
    }

    /// Advance the SM by one input frame.
    pub fn step(&mut self, input: NameEntryInput) {
        match self.state {
            NameEntryState::Editing => self.step_editing(input),
            NameEntryState::Confirm => self.step_confirm(input),
            NameEntryState::Done => {}
        }
    }

    /// Move the cursor by `delta`, wrap, then skip to the next selectable cell
    /// in the direction of travel.
    fn move_cursor(&mut self, delta: isize) {
        let step = if delta < 0 { -1 } else { 1 };
        let landed = (self.cursor as isize + delta).rem_euclid(NAV_CELLS as isize) as usize;
        self.cursor = self.skip_to_selectable(landed, step);
    }

    fn step_editing(&mut self, input: NameEntryInput) {
        // Retail d-pad deltas: up = -17, down = +17, right = +1, left = -1.
        if input.up {
            self.move_cursor(-(GRID_COLS as isize));
        } else if input.down {
            self.move_cursor(GRID_COLS as isize);
        } else if input.left {
            self.move_cursor(-1);
        } else if input.right {
            self.move_cursor(1);
        }

        // Triangle is a backspace shortcut while editing.
        if input.cancel {
            self.name.pop();
        }

        if input.confirm {
            if let Some(g) = self.glyph_at(self.cursor) {
                if self.name.chars().count() < NAME_MAX {
                    self.name.push(g);
                }
            } else if let Some(c) = self.control_at(self.cursor) {
                match c {
                    Control::Backspace => {
                        self.name.pop();
                    }
                    Control::Space => {
                        if self.name.chars().count() < NAME_MAX {
                            self.name.push(' ');
                        }
                    }
                    Control::End => {
                        // Retail gates End on a non-empty name (the `blez`
                        // check); an empty name keeps editing.
                        if !self.name.trim().is_empty() {
                            self.confirm_yes = true;
                            self.state = NameEntryState::Confirm;
                        }
                    }
                    Control::None => {}
                }
            }
        }
    }

    fn step_confirm(&mut self, input: NameEntryInput) {
        if input.left || input.right {
            self.confirm_yes = !self.confirm_yes;
        }
        if input.cancel {
            self.state = NameEntryState::Editing;
            return;
        }
        if input.confirm {
            if self.confirm_yes {
                self.state = NameEntryState::Done;
            } else {
                self.state = NameEntryState::Editing;
            }
        }
    }

    /// The name as it would be written into the record, trimmed of trailing
    /// whitespace (the retail field is NUL-padded; leading content is kept).
    pub fn committed_name(&self) -> String {
        self.name.trim_end().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_is_six_by_seventeen() {
        assert_eq!(GRID.len(), CHAR_ROWS);
        for row in GRID {
            assert_eq!(row.len(), GRID_COLS, "row {row:?} is not 17 wide");
        }
        // Separators sit at columns 5 and 11 in every glyph row.
        for row in GRID {
            assert_eq!(row.as_bytes()[5], b'|');
            assert_eq!(row.as_bytes()[11], b'|');
        }
    }

    #[test]
    fn control_row_tiles_backspace_space_end() {
        assert_eq!(CONTROL_ROW[0], Control::None);
        assert_eq!(CONTROL_ROW[2], Control::Backspace);
        assert_eq!(CONTROL_ROW[8], Control::Space);
        assert_eq!(CONTROL_ROW[16], Control::End);
    }

    #[test]
    fn cursor_skips_separators_when_moving_right() {
        let mut e = NameEntry::new(0, "");
        // 'E' is column 4; one right should skip the '|' at column 5 onto 'a'.
        e.cursor = 4;
        e.step(NameEntryInput {
            right: true,
            ..Default::default()
        });
        assert_eq!(e.glyph_at(e.cursor), Some('a'));
    }

    #[test]
    fn confirm_on_glyph_appends_it() {
        let mut e = NameEntry::new(0, "");
        e.cursor = 0; // 'A'
        e.step(NameEntryInput {
            confirm: true,
            ..Default::default()
        });
        assert_eq!(e.name, "A");
    }

    #[test]
    fn backspace_control_deletes_last_glyph() {
        let mut e = NameEntry::new(0, "Vahn");
        // Column 2 of the control row = Backspace.
        e.cursor = CHAR_CELLS + 2;
        e.step(NameEntryInput {
            confirm: true,
            ..Default::default()
        });
        assert_eq!(e.name, "Vah");
    }

    #[test]
    fn triangle_is_a_backspace_shortcut() {
        let mut e = NameEntry::new(0, "Vahn");
        e.step(NameEntryInput {
            cancel: true,
            ..Default::default()
        });
        assert_eq!(e.name, "Vah");
    }

    #[test]
    fn end_with_a_name_enters_confirm_then_commits() {
        let mut e = NameEntry::new(0, "Vahn");
        e.cursor = CHAR_CELLS + 16; // End cell
        e.step(NameEntryInput {
            confirm: true,
            ..Default::default()
        });
        assert_eq!(e.state, NameEntryState::Confirm);
        assert!(e.confirm_yes);
        // Confirm Yes -> Done.
        e.step(NameEntryInput {
            confirm: true,
            ..Default::default()
        });
        assert_eq!(e.state, NameEntryState::Done);
        assert_eq!(e.committed_name(), "Vahn");
    }

    #[test]
    fn end_with_empty_name_stays_editing() {
        let mut e = NameEntry::new(0, "");
        e.cursor = CHAR_CELLS + 16; // End cell
        e.step(NameEntryInput {
            confirm: true,
            ..Default::default()
        });
        assert_eq!(e.state, NameEntryState::Editing);
    }

    #[test]
    fn confirm_no_returns_to_editing() {
        let mut e = NameEntry::new(0, "Vahn");
        e.state = NameEntryState::Confirm;
        e.confirm_yes = true;
        // Toggle to No, then confirm.
        e.step(NameEntryInput {
            left: true,
            ..Default::default()
        });
        assert!(!e.confirm_yes);
        e.step(NameEntryInput {
            confirm: true,
            ..Default::default()
        });
        assert_eq!(e.state, NameEntryState::Editing);
    }

    #[test]
    fn name_is_capped_at_max_glyphs() {
        let mut e = NameEntry::new(0, "");
        e.cursor = 0; // 'A'
        for _ in 0..(NAME_MAX + 5) {
            e.step(NameEntryInput {
                confirm: true,
                ..Default::default()
            });
        }
        assert_eq!(e.name.chars().count(), NAME_MAX);
    }
}
