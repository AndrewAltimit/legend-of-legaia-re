//! Name-entry overlay - the menu screen the opening (`town01`) field script
//! opens so the player names the lead character.
//!
//! This is a port of the retail overlay's behaviour, not its bytes.
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
//! deltas are `-17` (up), `+17` (down), `+1` (right), `-1` (left), read off
//! the menu's auto-repeat word; a horizontal step wraps inside its own row.
//! The walk repeats while it stands on a separator and, from the control
//! row, while it stands on a cell of the button it left; a step left then
//! slides to the first cell of the button reached. A vertical step into the
//! control row keeps its column. The arms of a call are exclusive, in the
//! order cancel (takes the last glyph), Start alone (jumps to Select, and on
//! Select confirms), confirm, d-pad. The Select press leads to the confirm
//! prompt through one call that reads no pad, and the prompt has no cancel
//! arm. `crates/retail-exec/tests/name_entry.rs` holds the machine and the
//! renderer to the port call by call. A typed glyph is kept only under two bounds, both in the append arm
//! of `FUN_801F03F0`: the name before it holds fewer than nine glyphs
//! (`slti v0,s1,0x9` at `0x801F05F8`, [`NAME_MAX`]), and the name with it
//! measures under `0x39` px (`FUN_80035F04` then `slti v0,v0,0x39` at
//! `0x801F064C..0x801F0654`, [`NAME_WIDTH_CAP`]). The glyph is stored and
//! terminated first and taken back when it measures too wide, so a rejected
//! press changes nothing. The width bound applies when the entry carries the
//! disc's advance table ([`NameEntry::glyph_widths`]).

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
/// bytes NUL-padded ([`legaia_game_tables::new_game::NAME_LEN`]), so nine glyphs fit
/// with the terminator.
pub const NAME_MAX: usize = legaia_game_tables::new_game::NAME_LEN - 1;

/// The append arm keeps a glyph only while the name measures below this
/// many pixels (`slti v0,v0,0x39` at `0x801F0654`).
pub const NAME_WIDTH_CAP: u32 = 0x39;

/// A name's width in pixels as the append arm measures it: `FUN_80035F04`
/// over the name, each glyph advancing `widths[c] + DAT_800740E8 + 1`.
/// The pad word is `0` here - `FUN_80036888` clears it on every exit
/// (`sw zero,0x40e8(v1)` at `0x80036C18`) and nothing on the screen sets
/// it, so a glyph costs `widths[c] + 1`. A name holds no `0x7C` break or
/// escape (the grid selects neither), so the widest line is the whole name.
// REF: FUN_80035F04
pub fn name_width_px(widths: &[u8; 256], name: &str) -> u32 {
    name.bytes()
        .map(|b| u32::from(widths[usize::from(b)]) + 1)
        .sum()
}

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
    /// Cross - select the cell under the cursor (or confirm Yes/No).
    pub confirm: bool,
    /// The cancel button - takes the last glyph while editing; the confirm
    /// prompt has no arm for it.
    pub cancel: bool,
    /// Start pressed **alone** (the arm compares the whole edge word with
    /// `0x800`, `0x801F0588`): jumps to Select, and on Select confirms.
    pub start: bool,
}

/// A cue one frame of the screen queues.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NameEntryCue {
    /// `FUN_80035BD0(0x20)`: a glyph typed or taken, a button pressed, the
    /// name accepted.
    Accept,
    /// `FUN_80035B50(0x21)`: the hand moved.
    Move,
    /// `FUN_80035BD0(0x23)`: Select on an empty name.
    Buzz,
    /// `FUN_80035B50(0x37)`: No, back to the grid.
    Back,
}

impl NameEntryCue {
    /// Whether the cue goes through the push producer `FUN_80035B50`; the
    /// rest overwrite the last slot through `FUN_80035BD0`.
    pub fn pushes(self) -> bool {
        matches!(self, Self::Move | Self::Back)
    }

    /// The cue id handed to the ring.
    pub fn id(self) -> i16 {
        match self {
            Self::Accept => 0x20,
            Self::Move => 0x21,
            Self::Buzz => 0x23,
            Self::Back => 0x37,
        }
    }
}

/// What the machine draws above the grid on one call. The editing prompt is
/// drawn ahead of the arms of state `1`, so the frame of the Select press
/// still shows it; state `2` draws nothing; state `4` draws the question,
/// the two choices and their hand unless the call leaves the state.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum NameEntryLine {
    #[default]
    Editing,
    Hidden,
    Confirm,
}

/// First cell of the Select button (`slti v0,v0,0x72`, `0x801F059C`).
pub const SELECT_FIRST_CELL: usize = 0x72;
/// The cell the screen opens on and Start jumps to (`li v0,0x74`).
pub const SELECT_OPEN_CELL: usize = 0x74;

/// Whether the entry caret shows on frame `frame`: retail blinks it at 75%
/// duty off the frame counter's `& 0x18` bits.
pub fn caret_on(frame: u64) -> bool {
    (frame & 0x18) != 0
}

impl NameEntryInput {
    /// Decode one frame's **raw** just-pressed pad word (the
    /// [`legaia_engine_system::input::PadButton`] layout both hosts feed `World::set_pad`)
    /// into the overlay's semantic edges: the d-pad, Cross to confirm and
    /// Circle to cancel. Both hosts route their naming-prompt edges through
    /// this one decode; each used to spell the six masks out locally.
    ///
    /// The machine tests the edge word against the two mask words at
    /// `0x800846D0` / `0x800846D4` (`lw v0,0x594(s2)` at `0x801F055C` with
    /// `s2 = 0x80084140`, and `0x590` at `0x801F05BC`), which the new-game
    /// init seeds to Cross | L1 and Circle | L2. Triangle is the engine's
    /// second cancel, as it is on the shop screens.
    pub fn from_pad_edge(edge: u16) -> Self {
        Self::from_pad_words(edge, edge)
    }

    /// Decode the two words the machine reads: the buttons off the edge word
    /// `_DAT_8007B874`, the d-pad off the auto-repeat word `_DAT_8007BB84`
    /// (`0x801F0754`, `0x801F0A0C`) - the edge word with the window kernel's
    /// cursor repeat folded in, so a held direction walks the grid.
    pub fn from_pad_words(edge: u16, repeat: u16) -> Self {
        use legaia_engine_system::input::PadButton;
        let pressed = |b: PadButton| edge & b.mask() != 0;
        let stepped = |b: PadButton| repeat & b.mask() != 0;
        Self {
            up: stepped(PadButton::Up),
            down: stepped(PadButton::Down),
            left: stepped(PadButton::Left),
            right: stepped(PadButton::Right),
            confirm: pressed(PadButton::Cross),
            cancel: pressed(PadButton::Circle) || pressed(PadButton::Triangle),
            start: edge == PadButton::Start.mask(),
        }
    }
}

/// Mutable name-entry session. Install on the world via
/// `legaia_engine_core::world::World::open_name_entry`; step it each frame with a
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
    /// Retail's state `2`: the one call between the Select press and the
    /// prompt, which reads no pad ([`NameEntryState::Confirm`] covers it).
    pub arming: bool,
    /// The line the machine drew on its last call ([`NameEntryLine`]).
    pub line: NameEntryLine,
    /// The screen's shape word `DAT_8007BB94` reads `4`: state `2` sets it
    /// and a No puts it back to `1`. The renderer draws neither the grid
    /// hand nor the caret under it (`0x801E6C34`, `0x801E6E48`).
    pub confirm_shape: bool,
    /// The frame-step floor (`DAT_8007B9D8`) the screen found on open. Retail
    /// state 0 saves it to `0x801F2A84` (`0x801F0468..0x801F0474`), state 1
    /// holds the floor at `1` every frame (`0x801F0488`) and the close arm
    /// writes the saved word back (`0x801F09A0`); see
    /// `legaia_engine_core::world::World::open_name_entry`.
    pub saved_frame_step_floor: Option<u8>,
    /// The disc's per-character advance table (`0x80073F1C`) the append
    /// arm measures a name with ([`name_width_px`]); `None` on a disc-free
    /// build, where only the nine-glyph bound applies.
    pub glyph_widths: Option<[u8; 256]>,
}

impl NameEntry {
    /// Open a fresh entry for `char_index`, seeded with `initial` (typically
    /// the template's default name, e.g. `Vahn`). The cursor starts on the
    /// Select button (retail initial cell `0x74`), so a bare confirm keeps
    /// the default name.
    ///
    /// PORT: FUN_801F03F0
    /// REF: FUN_801E6B34
    pub fn new(char_index: usize, initial: &str) -> Self {
        let name: String = initial.chars().take(NAME_MAX).collect();
        Self {
            char_index,
            default_name: name.clone(),
            name,
            cursor: SELECT_OPEN_CELL,
            state: NameEntryState::Editing,
            confirm_yes: false,
            arming: false,
            line: NameEntryLine::Editing,
            confirm_shape: false,
            saved_frame_step_floor: None,
            glyph_widths: None,
        }
    }

    /// Install the advance table read from `scus` (`SCUS_942.54`), so the
    /// append arm applies retail's width cap. A `None` or unreadable
    /// executable leaves the entry on the glyph-count bound alone.
    pub fn with_scus_widths(mut self, scus: Option<&[u8]>) -> Self {
        self.glyph_widths = scus.and_then(legaia_font::scus_width_table);
        self
    }

    /// Whether appending `g` keeps the name inside both of the append arm's
    /// bounds (`0x801F05F8` and `0x801F0654`).
    fn accepts(&self, g: char) -> bool {
        if self.name.chars().count() >= NAME_MAX {
            return false;
        }
        match &self.glyph_widths {
            Some(widths) => {
                let mut trial = self.name.clone();
                trial.push(g);
                name_width_px(widths, &trial) < NAME_WIDTH_CAP
            }
            None => true,
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

    /// Where the hand cursor sits, as the draw builders take it: a
    /// `(row, column)` on the character grid, or the index of the control
    /// button (`0` backspace, `1` default, `2` end) on the control row.
    ///
    /// Both hosts project the screen through this and [`caret_on`]; each used
    /// to carry its own copy of the mapping.
    pub fn cursor_cells(&self) -> (Option<(usize, usize)>, Option<usize>) {
        if self.cursor < CHAR_CELLS {
            return (
                Some((self.cursor / GRID_COLS, self.cursor % GRID_COLS)),
                None,
            );
        }
        let idx = self.control_at(self.cursor).map(|c| match c {
            Control::Backspace => 0,
            Control::Default => 1,
            Control::End => 2,
        });
        (None, idx)
    }

    /// Advance the SM by one input frame; the cue the frame queues, if any.
    pub fn step(&mut self, input: NameEntryInput) -> Option<NameEntryCue> {
        match self.state {
            NameEntryState::Editing => self.step_editing(input),
            NameEntryState::Confirm => self.step_confirm(input),
            NameEntryState::Done => None,
        }
    }

    /// The grid byte under a glyph-row cell (`0x801F29F0 + cell`).
    fn grid_byte(cell: usize) -> u8 {
        GRID[cell / GRID_COLS].as_bytes()[cell % GRID_COLS]
    }

    /// Move the cursor one step of `delta` (`-17` up, `+17` down, `+1` right,
    /// `-1` left), the walk at `0x801F07AC..0x801F0958`.
    ///
    /// A horizontal step wraps inside its own row. The walk repeats while it
    /// stands on a `|` separator, and from the control row while it stands on
    /// a cell of the button it left. A step left then slides to the first
    /// cell of the button it reached. A vertical step into the control row
    /// keeps its column: the button is whichever one owns that column.
    fn move_cursor(&mut self, delta: isize) {
        let (n, cols) = (NAV_CELLS as isize, GRID_COLS as isize);
        let old = self.cursor;
        let old_button = self.control_at(old);
        let mut c = old as isize;
        // Retail's loop has no bound; no reachable cell spins it, and the
        // bound here only keeps a hand-set cursor from hanging the port.
        for _ in 0..2 * NAV_CELLS {
            if delta == 1 && c % cols == cols - 1 {
                c -= cols - 1;
            } else if delta == -1 && c % cols == 0 {
                c += cols - 1;
            } else {
                c += delta;
            }
            if c < 0 {
                c += n;
            }
            if c >= n {
                c -= n;
            }
            let cell = c as usize;
            if cell >= CHAR_CELLS {
                if old >= CHAR_CELLS && self.control_at(cell) == old_button {
                    continue;
                }
                break;
            }
            if Self::grid_byte(cell) != b'|' {
                break;
            }
        }
        let mut cell = c as usize;
        if delta == -1 {
            let button = self.control_at(cell);
            while cell > CHAR_CELLS && self.control_at(cell - 1) == button {
                cell -= 1;
            }
        }
        self.cursor = cell;
    }

    /// One call of state `1` (`0x801F0480..0x801F0958`). The arms are
    /// exclusive and tested in this order: cancel, Start, confirm, d-pad.
    fn step_editing(&mut self, input: NameEntryInput) -> Option<NameEntryCue> {
        self.line = NameEntryLine::Editing;
        // A cursor on a separator backs up a row (`0x801F0494..0x801F04D4`).
        while self.cursor < CHAR_CELLS
            && self.cursor >= GRID_COLS
            && Self::grid_byte(self.cursor) == b'|'
        {
            self.cursor -= GRID_COLS;
        }
        if input.cancel {
            // The cancel button takes the last glyph (`0x801F0570..0x801F0580`).
            return self.name.pop().map(|_| NameEntryCue::Accept);
        }
        let mut confirm = input.confirm;
        if input.start {
            // Start alone jumps to Select, and on Select is a confirm
            // (`0x801F0588..0x801F05C4`).
            if self.cursor < SELECT_FIRST_CELL {
                self.cursor = SELECT_OPEN_CELL;
                return Some(NameEntryCue::Accept);
            }
            confirm = true;
        }
        if confirm {
            if self.cursor < CHAR_CELLS {
                let g = Self::grid_byte(self.cursor) as char;
                if self.accepts(g) {
                    self.name.push(g);
                    return Some(NameEntryCue::Accept);
                }
                return None;
            }
            return match self.control_at(self.cursor) {
                Some(Control::End) => {
                    // The gate is the name's length (`blez s1`,
                    // `0x801F0694`): a name of blanks passes.
                    if self.name.is_empty() {
                        Some(NameEntryCue::Buzz)
                    } else {
                        self.state = NameEntryState::Confirm;
                        self.arming = true;
                        self.confirm_yes = false;
                        Some(NameEntryCue::Accept)
                    }
                }
                Some(Control::Default) => {
                    self.name = self.default_name.clone();
                    Some(NameEntryCue::Accept)
                }
                _ => self.name.pop().map(|_| NameEntryCue::Accept),
            };
        }
        // Up, down, right, left, in the order the arm tests the repeat word
        // (`0x801F0778..0x801F07A8`).
        let delta = if input.up {
            -(GRID_COLS as isize)
        } else if input.down {
            GRID_COLS as isize
        } else if input.right {
            1
        } else if input.left {
            -1
        } else {
            return None;
        };
        self.move_cursor(delta);
        Some(NameEntryCue::Move)
    }

    /// One call of states `2` and `4` (`0x801F095C..0x801F0A3C`).
    fn step_confirm(&mut self, input: NameEntryInput) -> Option<NameEntryCue> {
        if self.arming {
            // State 2: the call after the press seats the hand on No and
            // reads no pad.
            self.arming = false;
            self.confirm_yes = false;
            self.confirm_shape = true;
            self.line = NameEntryLine::Hidden;
            return None;
        }
        self.line = NameEntryLine::Confirm;
        if input.confirm {
            // Either way out skips the prompt's draws (`0x801F09E4`,
            // `0x801F0A00`).
            self.line = NameEntryLine::Hidden;
            return if self.confirm_yes {
                self.state = NameEntryState::Done;
                Some(NameEntryCue::Accept)
            } else {
                self.state = NameEntryState::Editing;
                self.confirm_shape = false;
                Some(NameEntryCue::Back)
            };
        }
        // Yes sits above No; up or down moves the hand between the two.
        // There is no cancel arm: the cancel button does nothing here.
        if input.up || input.down {
            self.confirm_yes = !self.confirm_yes;
            return Some(NameEntryCue::Move);
        }
        None
    }

    /// The name as the record holds it. Retail edits the record's own name
    /// field in place, so what was typed is what is kept, blanks included.
    pub fn committed_name(&self) -> String {
        self.name.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shared decode reads the raw layout: the masks the two hosts used
    /// to spell out (`0x0010` up, `0x0040` down, `0x0080` left, `0x0020`
    /// right, `0x4000` Cross, `0x2000` Circle and `0x1000` Triangle for the
    /// cancel), and nothing else.
    #[test]
    fn pad_edge_decode_matches_the_raw_masks() {
        let i = NameEntryInput::from_pad_edge(0x0010 | 0x4000);
        assert!(i.up && i.confirm && !i.down && !i.cancel && !i.left && !i.right);
        let i = NameEntryInput::from_pad_edge(0x0040 | 0x0080 | 0x0020 | 0x1000);
        assert!(i.down && i.left && i.right && i.cancel && !i.up && !i.confirm);
        // Circle is retail's cancel (the mask word `0x800846D4`).
        let i = NameEntryInput::from_pad_edge(0x2000);
        assert!(i.cancel && !i.confirm);
        // Start with another button is not a naming edge, and Square is none.
        let i = NameEntryInput::from_pad_edge(0x8000 | 0x0008);
        assert!(!(i.up || i.down || i.left || i.right || i.confirm || i.cancel || i.start));
    }

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

    /// A vertical step into the control row keeps its column, so the way
    /// back up lands on the glyph it left; a step left from there slides to
    /// the button's first cell.
    #[test]
    fn a_vertical_step_onto_a_button_keeps_its_column() {
        let mut e = NameEntry::new(0, "Vahn");
        e.cursor = 5 * GRID_COLS + 3; // row 5, column 3
        let press = |e: &mut NameEntry, i: NameEntryInput| e.step(i);
        press(
            &mut e,
            NameEntryInput {
                down: true,
                ..Default::default()
            },
        );
        assert_eq!(e.cursor, CHAR_CELLS + 3);
        assert_eq!(e.control_at(e.cursor), Some(Control::Backspace));
        press(
            &mut e,
            NameEntryInput {
                up: true,
                ..Default::default()
            },
        );
        assert_eq!(e.cursor, 5 * GRID_COLS + 3);
        e.cursor = CHAR_CELLS + 9;
        press(
            &mut e,
            NameEntryInput {
                left: true,
                ..Default::default()
            },
        );
        assert_eq!(e.cursor, CONTROL_ANCHORS[0]);
    }

    /// Right off a glyph row's last column wraps to that row's first.
    #[test]
    fn a_horizontal_step_wraps_inside_its_row() {
        let mut e = NameEntry::new(0, "Vahn");
        e.cursor = 2 * GRID_COLS + 16;
        e.step(NameEntryInput {
            right: true,
            ..Default::default()
        });
        assert_eq!(e.cursor, 2 * GRID_COLS);
        e.step(NameEntryInput {
            left: true,
            ..Default::default()
        });
        assert_eq!(e.cursor, 2 * GRID_COLS + 16);
    }

    /// Start alone jumps to Select, and on Select confirms; the confirm
    /// prompt has no cancel arm.
    #[test]
    fn start_jumps_to_select_and_then_confirms() {
        let mut e = NameEntry::new(0, "Vahn");
        e.cursor = 0;
        let start = NameEntryInput {
            start: true,
            ..Default::default()
        };
        assert_eq!(e.step(start), Some(NameEntryCue::Accept));
        assert_eq!(
            (e.cursor, e.state),
            (SELECT_OPEN_CELL, NameEntryState::Editing)
        );
        e.step(start);
        assert_eq!(e.state, NameEntryState::Confirm);
        e.step(NameEntryInput::default());
        e.step(NameEntryInput {
            cancel: true,
            ..Default::default()
        });
        assert_eq!(e.state, NameEntryState::Confirm);
        assert!(NameEntryInput::from_pad_edge(0x0008).start);
    }

    #[test]
    fn down_from_grid_lands_on_the_button_under_it() {
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
    fn cancel_takes_the_last_glyph() {
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
        // The call after the press reads no pad (retail's state 2).
        assert!(e.arming);
        e.step(NameEntryInput {
            up: true,
            ..Default::default()
        });
        assert!(!e.arming && !e.confirm_yes);
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

    /// Press confirm `n` times on the grid cell holding `g`.
    fn type_glyph(e: &mut NameEntry, g: char, n: usize) {
        e.cursor = (0..CHAR_CELLS)
            .find(|&c| e.glyph_at(c) == Some(g))
            .expect("glyph on the grid");
        for _ in 0..n {
            e.step(NameEntryInput {
                confirm: true,
                ..Default::default()
            });
        }
    }

    /// With the advance table installed, the width cap decides for wide
    /// glyphs and the nine-glyph bound for narrow ones - the two tests of
    /// the append arm (`0x801F05F8`, `0x801F0654`).
    #[test]
    fn width_cap_and_glyph_bound_both_apply() {
        let mut widths = [6u8; 256];
        widths[usize::from(b'W')] = 11; // advance 12: four fit in 48, five need 60
        widths[usize::from(b'i')] = 3; // advance 4: nine need only 36
        let mut wide = NameEntry::new(0, "");
        wide.glyph_widths = Some(widths);
        type_glyph(&mut wide, 'W', 8);
        assert_eq!(wide.name, "WWWW");
        assert!(name_width_px(&widths, &wide.name) < NAME_WIDTH_CAP);

        let mut narrow = NameEntry::new(0, "");
        narrow.glyph_widths = Some(widths);
        type_glyph(&mut narrow, 'i', 12);
        assert_eq!(narrow.name.chars().count(), NAME_MAX);

        // A rejected glyph leaves room for a narrower one: retail takes the
        // too-wide glyph back and keeps editing.
        type_glyph(&mut wide, 'i', 1);
        assert_eq!(wide.name, "WWWWi");
    }

    /// Without a table (a disc-free build) only the glyph bound applies.
    #[test]
    fn no_table_keeps_the_glyph_bound_alone() {
        let mut e = NameEntry::new(0, "").with_scus_widths(None);
        assert!(e.glyph_widths.is_none());
        type_glyph(&mut e, 'W', 12);
        assert_eq!(e.name.chars().count(), NAME_MAX);
    }
}
