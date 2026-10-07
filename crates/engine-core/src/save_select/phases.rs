//! Info-panel modes, select phases and their layout, and the session's input / event types.
//! Split out of `save_select.rs`.

use super::*;

/// What the bottom info panel shows for the focused grid cell.
///
/// Retail passes this to the panel renderer as a `view_mode` int; the
/// variants below carry the retail numbers. One retail mode has no port
/// equivalent and is deliberately absent: `100` (blank) is forced while the
/// "Now checking" dialog is up, which the port models as a separate
/// [`SelectPhase`] that does not draw the panel at all.
///
/// PORT: FUN_801E3F74 (selector) + FUN_801E08D8 (`view_mode` param).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotInfoMode {
    /// Retail `1`: kingdom name, play time, and the party's stats.
    Preview,
    /// Retail `2`: the block holds something this game cannot read.
    NotLegaiaSave,
    /// Retail `3`: the block is free.
    FreeBlock,
    /// Retail `4`: the "Return" caption, selected by cell index alone -
    /// `FUN_801E3F74`'s very first test (`li v0,0xf; bne a0,v0` at
    /// `0x801E3F74..0x801E3F84`), ahead of every content test.
    ///
    /// **Retail never reaches it.** See [`Self::for_grid_cell`].
    Return,
}

/// The grid cell index `FUN_801E3F74` answers [`SlotInfoMode::Return`] for.
///
/// Retail's own grid cursor cannot produce it - see
/// [`SlotInfoMode::for_grid_cell`] - so this is the constant the dead arm
/// tests, not a cell any host has to draw.
pub const SLOT_INFO_RETURN_CELL: u8 = 0x0F;

impl SlotInfoMode {
    /// Pick the mode for a slot. Mirrors FUN_801E3F74's branch order from
    /// its second test onward - the cell-index test that precedes it is
    /// [`Self::for_grid_cell`].
    pub fn for_slot(snap: &SlotSnapshot) -> Self {
        match snap.content {
            SlotContent::LegaiaSave => Self::Preview,
            // Retail reaches this via class `0`, and returns 2 from both
            // arms of its Save/Load branch - the distinction only matters
            // for a free block.
            SlotContent::Foreign => Self::NotLegaiaSave,
            SlotContent::Free => Self::FreeBlock,
        }
    }

    /// The whole of `FUN_801E3F74`: cell [`SLOT_INFO_RETURN_CELL`] captions
    /// [`Self::Return`] whatever the block holds, every other cell falls to
    /// [`Self::for_slot`].
    ///
    /// The `0xF` arm is **unreachable in retail**, and nothing in the port
    /// drives a cursor to it either. `FUN_801E3F74`'s only caller is the
    /// grid wrapper `FUN_801E06C0`, which is called once per frame as
    /// `FUN_801E06C0(state[+0x1F4], state[+0x1F8])` (`0x801DFD88`) and
    /// forms the cell as `col + row*5`. Both cursor words are clamped by
    /// the tick's shared stepper - `col` to `0..=4` (`slti v0,v0,0x5` at
    /// `0x801E017C`/`0x801E0190`) and `row` to `0..=2` (`slti v0,v0,0x3` at
    /// `0x801E01B0`/`0x801E01C0`) - so the cell tops out at `14`. The
    /// linear seed the two words are re-derived from (`_DAT_8007B7CC`) has
    /// exactly one writer on the whole disc, `sw s2,-0x4834(v0)` at
    /// `0x801DED2C`, and it stores that same `col + row*5`; a byte scan for
    /// the `0xB7CC` displacement over every extracted image finds three
    /// references, all three in PROT 0899 and all three in this tick.
    ///
    /// PORT: FUN_801E3F74
    pub fn for_grid_cell(cell: u8, snap: &SlotSnapshot) -> Self {
        if cell == SLOT_INFO_RETURN_CELL {
            return Self::Return;
        }
        Self::for_slot(snap)
    }

    /// The panel's centred caption, or `None` for [`Self::Preview`], which
    /// fills the panel with the save's stats instead.
    ///
    /// Only a free block's caption depends on `mode`: retail gates it on
    /// `_DAT_801f0200`, which is `0` on the Save path (the branch that goes
    /// on to stamp a product code into the chosen free block) and non-zero
    /// on the Load path.
    pub fn caption(self, mode: SaveSelectMode) -> Option<&'static str> {
        match self {
            Self::Preview => None,
            Self::NotLegaiaSave => Some("Not a Legend of Legaia save."),
            Self::FreeBlock => Some(match mode {
                SaveSelectMode::Save => "Able to save.",
                SaveSelectMode::Load => "No data",
            }),
            // `0x801CF384` in the menu overlay's rodata, drawn through the
            // same centred `FUN_801E3EE0(caption, 0xA0, panel_y + 0x18)`
            // tail as modes 2 and 3 (`0x801E0F70..0x801E0F88`).
            Self::Return => Some("Return"),
        }
    }
}

/// Phase of the SM.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectPhase {
    Browsing {
        cursor: u8,
    },
    /// Retail "Now checking. Do not remove MEMORY CARD" dialog frame.
    /// Auto-advances to [`SlotPreview`] when `frames_remaining`
    /// counts down to 0; no input is accepted while in this phase.
    NowChecking {
        slot: u8,
        frames_remaining: u16,
    },
    /// 5×3 portrait grid + bottom info panel preview of `slot`. X
    /// confirms the load (→ `Done(Loaded(slot))`); Circle returns to
    /// `Browsing { cursor: slot }`.
    SlotPreview {
        slot: u8,
    },
    ConfirmOverwrite {
        slot: u8,
        cursor: u8,
    },
    ConfirmDelete {
        slot: u8,
        cursor: u8,
    },
    /// A confirmed card Save or Load, between the confirm's "Yes" and the
    /// outcome: the write / read beat ("Saving to MEMORY CARD" / "Now
    /// Loading" over "Do not remove MEMORY CARD") for the first
    /// [`COMMIT_RESULT_FRAMES`]-exceeding stretch of `frames_remaining`, then
    /// the result line for the last [`COMMIT_RESULT_FRAMES`].
    ///
    /// The **write happens inside the beat**, as on retail: a Save holds the
    /// beat at its last write frame until the host has moved the bytes and
    /// answered through [`SaveSelectSession::report_commit`]
    /// (`SaveScreenFlow::save_request` hands it the request), so the result
    /// line is the write's real result - "Save successful." or retail's
    /// "Unable to save." - never a promise. A Load's bytes were already read
    /// for the grid, so its report is in from the start. A face button skips
    /// the result line, as retail's result arm adds a whole hold (`+0x5A`) on
    /// a press. Success ends in `Done(Saved)` / `Done(Loaded)`; a failed write
    /// returns to the block grid.
    ///
    /// Retail strings and the mode test that picks them live in PROT 0899:
    /// `0x801E2B50..0x801E2B7C` picks "Saving to MEMORY CARD" or "Now
    /// Loading" off the op flag `0x801F0200`, and `0x801DF920..0x801DF9B0`
    /// draws "Load successful." / "Save successful." off the result word.
    Committing {
        slot: u8,
        frames_remaining: u16,
        report: CommitReport,
    },
    Done(SelectOutcome),
}

/// The host's answer to a [`SelectPhase::Committing`] beat's card op.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommitReport {
    /// The host has not moved the bytes yet; the beat holds at its last
    /// write frame until it does.
    Pending,
    /// The op went through.
    Ok,
    /// The op failed: the result line reads retail's failure message.
    Failed,
}

/// Frames the result line of [`SelectPhase::Committing`] holds when no
/// button cuts it short. Retail's result arm accumulates the frame scalar
/// into its hold word and moves on at `0x5A` (`slti v0,v0,0x5B` at
/// `0x801DF9D8`): 90 sixtieth-second units.
pub const COMMIT_RESULT_FRAMES: u16 = 90;

impl SelectPhase {
    /// Whether a save-select opened from the title is composed over the
    /// title art in this phase. Retail keeps the card up behind the pill
    /// row and pivots to black once a port is picked: the "Now checking"
    /// beat, the block grid and everything drawn on top of the grid - the
    /// confirm messagebox and the write / read beat - sit on black. Both
    /// hosts ask this rather than listing phases, which is how the confirm
    /// came to flash the title art back up on both.
    pub fn shows_title_backdrop(self) -> bool {
        matches!(self, SelectPhase::Browsing { .. } | SelectPhase::Done(_))
    }
}

/// Frames the write / read line of [`SelectPhase::Committing`] holds. The
/// port's own beat - retail sits on the card driver's result word, which the
/// port's synchronous backends answer at once - set long enough to read the
/// line.
pub const COMMIT_WORK_FRAMES: u16 = 45;

/// What a save-select phase puts on screen, for the host that composes the
/// draws.
///
/// The two shipped hosts each answered this from their own `match` over
/// [`SelectPhase`], and the answers disagreed on exactly one phase pair -
/// the overwrite / delete confirms. Retail raises that prompt **from the
/// preview**: under `SaveRack::CardPorts` a Save crosses the same
/// `NowChecking` beat a Load does and the Yes/No messagebox (mode 3 of
/// `FUN_801E1C1C`) slides up over the block grid the player picked the cell
/// on, not over the pill row (see `docs/subsystems/save-screen.md`). So a
/// confirm is a `SlotPreview` wearing a messagebox, which is what this says
/// and what both hosts now draw.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SaveSelectPhaseLayout {
    /// Draw only the committed card's pill, relocated up under the panel
    /// (retail's mode-2 slide), instead of the whole pill row.
    pub single_pill: bool,
    /// Emit the pointing-finger cursor on the pill row. Suppressed once a
    /// card is committed: the dialog covers the row and the grid emits its
    /// own cursor on the focused cell.
    pub pill_cursor: bool,
    /// Draw the picked card's 5x3 block grid + the sliding info panel.
    pub preview: bool,
    /// Draw the "Now checking" panel.
    pub now_checking: bool,
    /// Draw the Yes/No confirm messagebox on top of everything.
    pub confirm: bool,
    /// Draw the card-operation messagebox ([`SelectPhase::Committing`]).
    pub banner: bool,
}

/// The layout [`SelectPhase`] implies - see [`SaveSelectPhaseLayout`].
pub fn phase_layout(phase: SelectPhase) -> SaveSelectPhaseLayout {
    let base = SaveSelectPhaseLayout {
        single_pill: false,
        pill_cursor: true,
        preview: false,
        now_checking: false,
        confirm: false,
        banner: false,
    };
    match phase {
        SelectPhase::NowChecking { .. } => SaveSelectPhaseLayout {
            single_pill: true,
            pill_cursor: false,
            now_checking: true,
            ..base
        },
        SelectPhase::SlotPreview { .. } => SaveSelectPhaseLayout {
            single_pill: true,
            pill_cursor: false,
            preview: true,
            ..base
        },
        SelectPhase::Committing { .. } => SaveSelectPhaseLayout {
            single_pill: true,
            pill_cursor: false,
            preview: true,
            banner: true,
            ..base
        },
        SelectPhase::ConfirmOverwrite { .. } | SelectPhase::ConfirmDelete { .. } => {
            SaveSelectPhaseLayout {
                single_pill: true,
                pill_cursor: false,
                preview: true,
                confirm: true,
                ..base
            }
        }
        _ => base,
    }
}

/// Final outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectOutcome {
    Loaded(u8),
    Saved(u8),
    Deleted(u8),
    Cancelled,
}

/// Per-frame input bundle.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SelectInput {
    pub up: bool,
    pub down: bool,
    pub left: bool,
    pub right: bool,
    pub cross: bool,
    pub circle: bool,
    pub triangle: bool,
}

impl SelectInput {
    /// Unpack an edge-triggered PSX pad word, the shape every host already
    /// drives its menus with.
    pub fn from_pad_edge(pressed: u16) -> Self {
        use crate::input::PadButton;
        let is = |b: PadButton| pressed & b.mask() != 0;
        Self {
            up: is(PadButton::Up),
            down: is(PadButton::Down),
            left: is(PadButton::Left),
            right: is(PadButton::Right),
            cross: is(PadButton::Cross),
            circle: is(PadButton::Circle),
            triangle: is(PadButton::Triangle),
        }
    }
}

/// Events emitted per `tick`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectEvent {
    CursorMoved {
        slot: u8,
    },
    EnteredConfirm {
        slot: u8,
        kind: ConfirmKind,
    },
    /// User confirmed a destructive action.
    Confirmed {
        slot: u8,
        kind: ConfirmKind,
    },
    /// User cancelled out of a confirm prompt back to browsing.
    ConfirmCancelled {
        slot: u8,
        kind: ConfirmKind,
    },
    /// User picked an empty slot in Load mode (no-op blip).
    InvalidConfirm,
    /// User pressed X on a non-empty Load slot; "Now checking" dialog
    /// has been entered.
    EnteredNowChecking {
        slot: u8,
    },
    /// "Now checking" beat finished; slot-preview phase entered.
    EnteredSlotPreview {
        slot: u8,
    },
    /// The card-read beat failed: retail's status `3`, which prints
    /// "NOT CARD" and abandons the read. The session returns to
    /// browsing with the cursor still on the slot that was picked.
    CardReadFailed {
        slot: u8,
    },
    /// User confirmed the load from the slot-preview screen (X on
    /// SlotPreview).
    LoadConfirmed {
        slot: u8,
    },
    /// A Save's write failed; its failure line has been read and the session
    /// is back on the block grid.
    CommitFailed {
        slot: u8,
    },
    /// User cancelled out of the slot-preview screen back to browsing.
    SlotPreviewCancelled {
        slot: u8,
    },
    /// Whole session cancelled.
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfirmKind {
    Overwrite,
    Delete,
}

/// Default duration of the "Now checking" dialog in frames @ 60 Hz.
/// Two seconds of game time mirrors the retail memory-card scan beat.
pub const DEFAULT_NOW_CHECKING_FRAMES: u16 = 120;

/// Slide-in animation timer rate, in 12-bit fixed-point units per tick.
/// Mirrors retail's `DAT_801ef194 += DAT_1f800393 * 0x100` increment in
/// the save-UI dispatcher: 256/frame, clamped at 4096. See Ghidra
/// trace in `overlay_save_ui_select_801dd35c.txt` lines ~4246.
pub const SLIDE_ANIM_RATE: u16 = 256;
/// Fully-arrived sentinel for the slide-in timer (12-bit fixed-point
/// 1.0). Retail uses `0x1000` everywhere as the clamp ceiling and the
/// "at target" marker.
pub const SLIDE_ANIM_FULL: u16 = 0x1000;

/// Off-screen-below y-origin of the bottom info panel (retail
/// `0x18A = 394`). Drives the `t=0` end of the info-panel slide-in
/// interpolation. Pinned from `FUN_801E08D8`'s entry math:
/// `local_34 = (anim_t * -0x100) / 0xfff >> 12 + 0x18A`.
pub const INFO_PANEL_OFFSCREEN_Y: i32 = 394;
/// Parked y-origin of the bottom info panel (retail value, derived as
/// `0x18A - 0x100 = 138` when `anim_t = 0x1000`). The 9-slice chrome's
/// top gold border lands on this y; matches the existing
/// `SLOT_INFO_PANEL_POS.1` chrome scan.
pub const INFO_PANEL_PARKED_Y: i32 = 138;

/// Interpolate between `(start, target)` using a 12-bit fixed-point
/// `t` in `[0, SLIDE_ANIM_FULL]`. Mirrors retail's
/// `pos = start + (target - start) * t / 4096` math from
/// `FUN_801E1C1C`. Free function for callers without a session
/// handle; the [`SaveSelectSession::interpolate`] method forwards
/// here using `self.slide_anim_t()`.
pub fn interpolate_anim(start: (i32, i32), target: (i32, i32), t: u16) -> (i32, i32) {
    let t = t as i32;
    let denom = SLIDE_ANIM_FULL as i32;
    let dx = (target.0 - start.0) * t / denom;
    let dy = (target.1 - start.1) * t / denom;
    (start.0 + dx, start.1 + dy)
}
