//! Name-entry overlay - the menu screen the opening (`town01`) field script
//! opens so the player names the lead character.
//!
//! This is a clean-room port of the retail overlay's behaviour, not its bytes.
//! Retail reference (captured from the field/menu overlay at `0x801C0000` and
//! live-traced against the recomp oracle's GP0 draw stream):
//!
//! - render fn `FUN_801E6B34` - draws the character grid (15 px column pitch,
//!   14 px row pitch, origin `base + (4, 4)`), the working name + teal `_`
//!   caret in the name-field box, the three gold control-bar labels, and the
//!   name-field window (`FUN_8002C69C(base_x + 0xAC, base_y - 0x14, 0x48,
//!   0xC)`).
//! - charset grid at `0x801F29F0` - SEVEN rows of seventeen bytes: six glyph
//!   rows (`|` = 0x7C at columns 5 and 11 are non-selectable separators) and
//!   a control row `[00 00 66 x6 64 x6 65 x3]`. The renderer resolves a
//!   control-row cursor cell through a **+2 byte offset** into that row
//!   (`grid[cell + 2]`), so the three action groups land on the cursor
//!   anchor cells 102 / 108 / 114.
//! - control actions (live-verified against the record name at `+0x2A7`):
//!   `0x66` = backspace ("BS" label), `0x64` = **restore the default name**
//!   (labelled with the quoted template name, e.g. `` `Vahn' ``), `0x65` =
//!   end/confirm ("Select" label). There is no space button - the grid's
//!   blank cells type the space glyph.
//! - the cursor OPENS on the Select button (retail initial cell `0x74`), so
//!   a bare confirm accepts the default name.
//! - the "Is this name okay?" confirm shows Yes over No as two stacked teal
//!   rows with the hand on **No** (up/down moves it); the committed name is
//!   written into the live character record's name field at record offset
//!   `+0x2A7` (record base `0x80084708 + n*0x414`).
//!
//! The interactive handler moves a linear cursor over a 7-row x 17-column
//! navigation space (`0x77` = 119 cells, wrapped modulo 119): rows 0..5 hold
//! the 102 selectable glyph cells, and row 6 is the control bar. The d-pad
//! deltas are `-17` (up), `+17` (down), `+1` (right), `-1` (left); after each
//! move the cursor skips any non-selectable cell in the same direction, and a
//! move that lands in the control row snaps to the action group's anchor
//! cell. The retail name length is bounded by the proportional-font pixel
//! width (57 px); this port bounds it by glyph count ([`NAME_MAX`], derived
//! from the template's 10-byte name field).

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

/// One bottom-bar action (the retail row-6 sentinel bytes, resolved through
/// the renderer's `grid[cell + 2]` read).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Control {
    /// Delete the last glyph (retail `0x66`, "BS" label).
    Backspace,
    /// Restore the template's default name (retail `0x64`, labelled with the
    /// quoted default, e.g. `` `Vahn' ``). Live-verified: pressing it on an
    /// edited name rewrites the record name back to the template.
    Default,
    /// Finish - advance to the "Is this name okay?" confirm (retail `0x65`,
    /// "Select" label).
    End,
}

/// Cursor anchor cells for the three control-row buttons. A d-pad move that
/// lands anywhere in the control row snaps to its group's anchor (the cells
/// the retail cursor was observed to occupy: BS = 102, default = 108,
/// Select = 114; the SM's initial cell `0x74` = 116 also resolves to Select).
pub const CONTROL_ANCHORS: [usize; 3] = [CHAR_CELLS, CHAR_CELLS + 6, CHAR_CELLS + 12];

/// Sub-state of the name-entry SM (collapsed from the retail 5 substates: the
/// init + interactive handlers map to [`Editing`](NameEntryState::Editing);
/// the confirm handlers map to [`Confirm`](NameEntryState::Confirm) and its
/// commit / cancel exits).
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
    /// The template default name the `0x64` control restores (and the
    /// control bar displays quoted).
    pub default_name: String,
    /// Linear cursor over the `NAV_CELLS` navigation grid (`0..NAV_CELLS`).
    pub cursor: usize,
    /// Current sub-state.
    pub state: NameEntryState,
    /// Yes/No selection while in [`NameEntryState::Confirm`]. Retail opens
    /// the prompt with the hand on **No** (`false`); up/down moves it.
    pub confirm_yes: bool,
}

impl NameEntry {
    /// Open a fresh entry for `char_index`, seeded with `initial` (typically
    /// the template's default name, e.g. `Vahn`). The cursor starts on the
    /// Select button (retail initial cell `0x74`, normalised to the Select
    /// anchor), so a bare confirm keeps the default name.
    ///
    /// PORT: FUN_801F03F0
    /// REF: FUN_801E6B34
    pub fn new(char_index: usize, initial: &str) -> Self {
        let name: String = initial.chars().take(NAME_MAX).collect();
        Self {
            char_index,
            default_name: name.clone(),
            name,
            cursor: CONTROL_ANCHORS[2],
            state: NameEntryState::Editing,
            confirm_yes: false,
        }
    }

    /// Grid row / column of a navigation cell.
    fn row_col(cell: usize) -> (usize, usize) {
        (cell / GRID_COLS, cell % GRID_COLS)
    }

    /// `true` when a navigation cell can host the cursor: a glyph cell that is
    /// not a `|` separator, or any control-row cell (the `+2`-shifted retail
    /// read maps every control-row column onto one of the three buttons).
    pub fn is_selectable(&self, cell: usize) -> bool {
        let (row, col) = Self::row_col(cell);
        if row < CHAR_ROWS {
            GRID[row].as_bytes().get(col).copied() != Some(b'|')
        } else {
            true
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

    /// The control action under a control-row cell. Mirrors the retail
    /// renderer's `grid[cell + 2]` sentinel read: shifted columns 2..=7 are
    /// `0x66` (backspace), 8..=13 are `0x64` (default), 14.. are `0x65`
    /// (end) - i.e. raw columns 0..=5 / 6..=11 / 12..=16.
    pub fn control_at(&self, cell: usize) -> Option<Control> {
        let (row, col) = Self::row_col(cell);
        if row < CHAR_ROWS {
            return None;
        }
        Some(match col {
            0..=5 => Control::Backspace,
            6..=11 => Control::Default,
            _ => Control::End,
        })
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
    /// in the direction of travel. A landing in the control row snaps to the
    /// action group's anchor cell (the retail cursor cells 102 / 108 / 114).
    fn move_cursor(&mut self, delta: isize) {
        let step = if delta < 0 { -1 } else { 1 };
        // Horizontal movement that starts in the control row cycles the
        // three buttons, wrapping (retail-observed: Left steps Select ->
        // default -> BS; Right from Select wraps to BS). It never leaves
        // the row - only up/down do.
        if delta.abs() == 1 && self.cursor >= CHAR_CELLS {
            let from = match self.control_at(self.cursor) {
                Some(Control::Backspace) => 0isize,
                Some(Control::Default) => 1,
                _ => 2,
            };
            let next = (from + step).rem_euclid(3) as usize;
            self.cursor = CONTROL_ANCHORS[next];
            return;
        }
        let landed = (self.cursor as isize + delta).rem_euclid(NAV_CELLS as isize) as usize;
        let mut cell = self.skip_to_selectable(landed, step);
        if cell >= CHAR_CELLS {
            // A vertical landing in the control row snaps to the action
            // group's anchor (the retail cursor cells 102 / 108 / 114).
            cell = match self.control_at(cell) {
                Some(Control::Backspace) => CONTROL_ANCHORS[0],
                Some(Control::Default) => CONTROL_ANCHORS[1],
                _ => CONTROL_ANCHORS[2],
            };
        }
        self.cursor = cell;
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
                    Control::Default => {
                        self.name = self.default_name.clone();
                    }
                    Control::End => {
                        // Retail gates End on a non-empty name (the `blez`
                        // check); an empty name keeps editing.
                        if !self.name.trim().is_empty() {
                            // Retail opens the prompt with the hand on No.
                            self.confirm_yes = false;
                            self.state = NameEntryState::Confirm;
                        }
                    }
                }
            }
        }
    }

    fn step_confirm(&mut self, input: NameEntryInput) {
        // Yes sits above No; up/down moves the hand between the two rows.
        if input.up || input.down {
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
    fn control_row_maps_backspace_default_end() {
        let e = NameEntry::new(0, "Vahn");
        assert_eq!(e.control_at(CHAR_CELLS), Some(Control::Backspace));
        assert_eq!(e.control_at(CHAR_CELLS + 6), Some(Control::Default));
        assert_eq!(e.control_at(CHAR_CELLS + 12), Some(Control::End));
        // The retail initial cell (0x74 = 116) also resolves to End.
        assert_eq!(e.control_at(116), Some(Control::End));
    }

    #[test]
    fn cursor_opens_on_select_button() {
        let e = NameEntry::new(0, "Vahn");
        assert_eq!(e.control_at(e.cursor), Some(Control::End));
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
    fn control_row_left_right_cycles_the_three_buttons() {
        let mut e = NameEntry::new(0, "Vahn");
        // Opens on Select; Left -> Default anchor (108), Left -> BS (102),
        // Left wraps -> Select (114). Mirrors the live-recorded cursor cells.
        e.step(NameEntryInput {
            left: true,
            ..Default::default()
        });
        assert_eq!(e.cursor, CONTROL_ANCHORS[1]);
        e.step(NameEntryInput {
            left: true,
            ..Default::default()
        });
        assert_eq!(e.cursor, CONTROL_ANCHORS[0]);
        e.step(NameEntryInput {
            left: true,
            ..Default::default()
        });
        assert_eq!(e.cursor, CONTROL_ANCHORS[2]);
        // Right from Select wraps to BS (observed live).
        e.step(NameEntryInput {
            right: true,
            ..Default::default()
        });
        assert_eq!(e.cursor, CONTROL_ANCHORS[0]);
    }

    #[test]
    fn down_from_grid_snaps_to_group_anchor() {
        let mut e = NameEntry::new(0, "Vahn");
        // Row 5 col 6 ('z', cell 91) + down = cell 108 = the Default anchor
        // (the live-recorded 91 <-> 108 hop).
        e.cursor = 91;
        e.step(NameEntryInput {
            down: true,
            ..Default::default()
        });
        assert_eq!(e.cursor, CONTROL_ANCHORS[1]);
        // And back up onto 'z'.
        e.step(NameEntryInput {
            up: true,
            ..Default::default()
        });
        assert_eq!(e.cursor, 91);
        assert_eq!(e.glyph_at(e.cursor), Some('z'));
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
        e.cursor = CONTROL_ANCHORS[0];
        e.step(NameEntryInput {
            confirm: true,
            ..Default::default()
        });
        assert_eq!(e.name, "Vah");
    }

    #[test]
    fn default_control_restores_the_template_name() {
        let mut e = NameEntry::new(0, "Vahn");
        // Type an extra glyph then restore: "Vahnz" -> "Vahn" (the live
        // sequence recorded against the record name at +0x2A7).
        e.cursor = 91; // 'z'
        e.step(NameEntryInput {
            confirm: true,
            ..Default::default()
        });
        assert_eq!(e.name, "Vahnz");
        e.cursor = CONTROL_ANCHORS[1];
        e.step(NameEntryInput {
            confirm: true,
            ..Default::default()
        });
        assert_eq!(e.name, "Vahn");
        // A second press is a no-op.
        e.step(NameEntryInput {
            confirm: true,
            ..Default::default()
        });
        assert_eq!(e.name, "Vahn");
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
    fn end_with_a_name_enters_confirm_on_no_then_commits_via_up() {
        let mut e = NameEntry::new(0, "Vahn");
        e.cursor = CONTROL_ANCHORS[2]; // Select
        e.step(NameEntryInput {
            confirm: true,
            ..Default::default()
        });
        assert_eq!(e.state, NameEntryState::Confirm);
        assert!(
            !e.confirm_yes,
            "retail opens the prompt with the hand on No"
        );
        // Up moves the hand to Yes; confirm commits.
        e.step(NameEntryInput {
            up: true,
            ..Default::default()
        });
        assert!(e.confirm_yes);
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
        e.cursor = CONTROL_ANCHORS[2];
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
        e.confirm_yes = false;
        // Confirm with the hand on No -> back to editing.
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
