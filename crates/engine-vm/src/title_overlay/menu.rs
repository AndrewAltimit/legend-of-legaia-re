//! The executable half: the title menu's input and attract law.
//! Split out of `title_overlay.rs`.

use super::*;

// ---------------------------------------------------------------------------
// The executable half: the title menu's input + attract law
// ---------------------------------------------------------------------------

/// D-pad bit that steps the title cursor **forward** one row - `Down` in the
/// repacked pad word (`andi v0,a2,0x4000` at `0x801DDBC0`).
pub const PADMASK_CURSOR_NEXT: u16 = 0x4000;

/// D-pad bit that steps it **back** one row - `Up` (`andi v0,a2,0x1000` at
/// `0x801DDB9C`).
pub const PADMASK_CURSOR_PREV: u16 = 0x1000;

/// SFX cue the tick stores on a cursor move (`li v1,0x21` /
/// `sh v1,-0x4928(a1)` at `0x801DDBB0`, i.e. `0x8007B6D8`).
pub const TITLE_SFX_CURSOR_MOVE: u16 = 0x21;

/// SFX cue it stores on a confirm (`li v1,0x20` at `0x801DDC20` /
/// `li v0,0x20` at `0x801DDC4C`, same halfword).
pub const TITLE_SFX_CONFIRM: u16 = 0x20;

/// Rows the title menu carries. Retail wraps the row counter with
/// `andi v1,v1,0x1` at `0x801DDC00`, which is a two-row space.
pub const TITLE_MENU_ROWS: u8 = 2;

/// Row `0` of the title menu - NEW GAME. Confirming it writes master game
/// mode [`MASTER_GAME_MODE_FIELD_LAUNCH`] further down the graph.
pub const TITLE_ROW_NEW_GAME: u8 = 0;

/// Row `1` - CONTINUE. Confirming it stashes `1` in `state[+0x200]`
/// (`0x801DDC64`) and routes to sub-mode `0x18`.
pub const TITLE_ROW_CONTINUE: u8 = 1;

/// Below this countdown value the tick stops reading the pad at all:
/// `slti v0,a0,0x11` / `bne v0,zero,0x801DDC94` at `0x801DDB84` jumps past
/// the whole cursor + confirm block. The last sixteen frames before the
/// attract fires accept no input.
pub const ATTRACT_INPUT_FREEZE_BELOW: i32 = 0x11;

/// What one [`TitleMenuState::step`] did, in the order retail does it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TitleMenuEvent {
    /// The cursor moved to `row`; retail cues [`TITLE_SFX_CURSOR_MOVE`].
    CursorMoved { row: u8 },
    /// `row` was confirmed off the `Start | L1 | Cross` mask
    /// ([`PADMASK_START_L1_CROSS`]); retail cues [`TITLE_SFX_CONFIRM`].
    Confirmed { row: u8 },
    /// The attract countdown underflowed. Retail's arm zeroes
    /// `0x8007BA78` and writes [`MASTER_GAME_MODE_STR_INIT`] to
    /// [`MASTER_GAME_MODE_ADDR`], i.e. hands the screen to the opening
    /// movie.
    AttractFired,
}

/// The mutable half of the title tick's `AttractIdle` (`0x10`) state - the
/// row counter, the attract countdown, and the row a confirm stashed.
///
/// The field names are retail's globals: `row_counter` is `_DAT_8007B820`
/// (`lw v0,-0x47e0(a0)` at `0x801DDBAC`), `countdown` is `0x801EF16C`
/// (`lw v0,-0xe94(a0)` at `0x801DDCBC`), `chosen_row` is `state[+0x200]`.
///
/// PORT: FUN_801DD35C (`0x801DDB74..0x801DDCF4`)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TitleMenuState {
    /// Free-running row counter. The displayed row is this value wrapped
    /// into the row space; retail writes the wrapped value straight back.
    pub row_counter: i32,
    /// Frames left before the attract movie takes the screen.
    pub countdown: i32,
    /// The row the last confirm stashed (`state[+0x200]`).
    pub chosen_row: u8,
    /// The SFX cue the last step stored, if any (`0x8007B6D8`).
    pub sfx: Option<u16>,
}

impl Default for TitleMenuState {
    fn default() -> Self {
        Self::new()
    }
}

impl TitleMenuState {
    /// A freshly entered title menu: cursor on row 0, countdown at
    /// [`COUNTDOWN_RESET_VALUE`] (`li v0,0x5dc` at `0x801DDACC`).
    pub fn new() -> Self {
        Self {
            row_counter: 0,
            countdown: COUNTDOWN_RESET_VALUE as i32,
            chosen_row: 0,
            sfx: None,
        }
    }

    /// The row the cursor is on - retail's `andi v1,v1,0x1` at
    /// `0x801DDC00` generalised to `rows` (which is `2` in retail, where
    /// the mask and the modulo agree).
    pub fn row(&self, rows: u8) -> u8 {
        let n = rows.max(1) as i32;
        self.row_counter.rem_euclid(n) as u8
    }

    /// One frame of the menu state, in retail's order:
    ///
    /// 1. Skip the whole input block while the countdown is below
    ///    [`ATTRACT_INPUT_FREEZE_BELOW`] (`0x801DDB84`).
    /// 2. `Down` / `Up` step the row counter and cue
    ///    [`TITLE_SFX_CURSOR_MOVE`] (`0x801DDB9C..0x801DDBE0`).
    /// 3. Wrap the counter and store it back (`0x801DDC00`).
    /// 4. `Start | L1 | Cross` confirms the wrapped row, cues
    ///    [`TITLE_SFX_CONFIRM`], and stashes it (`0x801DDC04..0x801DDC70`).
    /// 5. Any held pad bit re-arms the countdown (`0x801DDC74`), then the
    ///    countdown drops by the frame scalar and fires on underflow
    ///    (`0x801DDCB0..0x801DDCF4`).
    ///
    /// `pad_edge` is the just-pressed word the cursor and confirm read;
    /// `pad_held` is the held word retail re-arms the countdown from
    /// (`_DAT_8007B850`). `frame_scalar` is the scratchpad byte at
    /// `0x1F800393`, `1` on a normal frame.
    ///
    /// PORT: FUN_801DD35C (`0x801DDB74..0x801DDCF4`)
    pub fn step(
        &mut self,
        pad_edge: u16,
        pad_held: u16,
        frame_scalar: u8,
        rows: u8,
    ) -> Vec<TitleMenuEvent> {
        let mut events = Vec::new();
        self.sfx = None;
        let n = rows.max(1) as i32;
        if self.countdown >= ATTRACT_INPUT_FREEZE_BELOW {
            let before = self.row_counter.rem_euclid(n);
            if pad_edge & PADMASK_CURSOR_NEXT != 0 {
                self.row_counter = self.row_counter.wrapping_add(1);
                self.sfx = Some(TITLE_SFX_CURSOR_MOVE);
            }
            if pad_edge & PADMASK_CURSOR_PREV != 0 {
                self.row_counter = self.row_counter.wrapping_sub(1);
                self.sfx = Some(TITLE_SFX_CURSOR_MOVE);
            }
            // Retail wraps the counter and writes the wrapped value back,
            // so the counter never runs away from the row space.
            self.row_counter = self.row_counter.rem_euclid(n);
            if self.row_counter != before {
                events.push(TitleMenuEvent::CursorMoved {
                    row: self.row_counter as u8,
                });
            }
            if pad_edge & PADMASK_START_L1_CROSS != 0 {
                let row = self.row_counter as u8;
                self.chosen_row = row;
                self.sfx = Some(TITLE_SFX_CONFIRM);
                events.push(TitleMenuEvent::Confirmed { row });
                return events;
            }
        }
        if pad_held != 0 {
            self.countdown = COUNTDOWN_RESET_VALUE as i32;
        }
        self.countdown -= frame_scalar as i32;
        if self.countdown < 0 {
            events.push(TitleMenuEvent::AttractFired);
        }
        events
    }
}
