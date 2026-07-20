//! Save-slot select session.
//!
//! PORT: FUN_801DD35C (save-UI dispatcher), FUN_801E08D8 (info-panel renderer), FUN_801E1C1C (slide-in animator)
//!
//! Drives the slot-list UI (read save metadata, browse, Load/Save/Delete
//! confirmations). Renderer-agnostic - engines render the slot list
//! against the existing text overlay; the session emits typed events for
//! engines to react to (cursor blip, confirm chime, etc.).
//!
//! Two operating modes:
//!
//! - [`SaveSelectMode::Load`] - pick a non-empty slot to load.
//! - [`SaveSelectMode::Save`] - pick any slot (empty or full) to write
//!   into. Picking a non-empty slot enters the Overwrite confirm prompt.
//!
//! ## States
//!
//! `Browsing → ConfirmLoad / ConfirmOverwrite / ConfirmDelete → Done`
//!
//! ## Slot list = save blocks, or memory-card slots
//!
//! By default the slot list is the **save blocks** themselves: the pills
//! show the first two, the preview grid shows all fifteen, and Save mode
//! picks a block straight off the pill row. That is the flat model the
//! native shell drives against its on-disk LGSF slots.
//!
//! Retail is two-stage: the pills are the console's **two memory-card
//! slots** (the libcd channel's `port`, see
//! `docs/subsystems/save-screen.md`), and the 5x3 preview grid is the
//! chosen card's fifteen blocks. Hosts that model it that way opt in via
//! [`SaveSelectSession::set_card_slots_mode`], which routes Save-mode
//! confirmation through the same `NowChecking` card-read beat Load mode
//! already uses and lands the overwrite prompt after the grid rather than
//! before it. The flag is off by default, so the flat model is unchanged.
//!
//! Engines call [`SaveSelectSession::tick`] each frame and react to
//! returned [`SelectEvent`]s. The session never reads the save data
//! itself - engines pre-load slot metadata into [`SlotSnapshot`] entries
//! and feed them through `set_slots`.

use crate::menu_input::{CURSOR_INDEX_MASK, CursorNav, NavButtons, menu_cursor_nav};

/// What occupies a card block, in the terms retail's info panel branches
/// on - its per-slot class byte at `0x801F2A48`.
///
/// `present` answers "can this be loaded"; this answers "why not", which is
/// what decides the caption an unloadable block shows.
///
/// REF: FUN_801E3F74 (the class byte is the `-0x7fe0d5b8` array it reads).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SlotContent {
    /// A readable Legend of Legaia save. Retail class `1`.
    LegaiaSave,
    /// A free block. Retail class `>= 2`.
    ///
    /// The default: a session over disk saves (rather than a card) has no
    /// foreign saves, so every absent slot is simply free.
    #[default]
    Free,
    /// Occupied by a save this game cannot read - another game's, or a
    /// Legaia block whose payload does not parse. Retail class `0`.
    Foreign,
}

/// Per-slot metadata. Engines build these from disc/disk save scans
/// (the `legaia-save` crate provides the parsers). Pure data - the
/// session never touches the filesystem.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotSnapshot {
    pub slot: u8,
    pub present: bool,
    /// What occupies the block. `present == true` implies
    /// [`SlotContent::LegaiaSave`]; the other variants distinguish the two
    /// ways a slot can be unloadable.
    pub content: SlotContent,
    /// Display label engines render. `"<empty>"` for empty slots.
    pub label: String,
    /// Game time in seconds (for the "Play time: 12:34:56" line).
    pub play_time_seconds: u32,
    /// Party leader's level (for the "Lv. 23" badge).
    pub party_lv: u8,
    /// Map name where the save was written.
    pub location: String,
    /// In-game gold.
    pub money: u32,
    /// Lead character's roster index (0=Vahn, 1=Noa, 2=Gala). Used by
    /// the load-screen slot-preview to pick which 16×16 portrait
    /// sprite to render for this slot. Defaults to 0 (Vahn) which
    /// matches every retail Legaia save (the lead slot is always
    /// Vahn).
    pub leader_char_id: u8,
    /// Lead character's display name ("Vahn" / "Noa" / "Gala"...).
    pub leader_name: String,
    /// Lead character's current/max HP for the slot-preview info
    /// panel.
    pub leader_hp: (u16, u16),
    /// Lead character's current/max MP (a.k.a. WP) for the info
    /// panel.
    pub leader_mp: (u16, u16),
}

impl SlotSnapshot {
    pub fn empty(slot: u8) -> Self {
        Self {
            slot,
            present: false,
            content: SlotContent::Free,
            label: format!("Slot {slot}: <empty>"),
            play_time_seconds: 0,
            party_lv: 0,
            location: String::new(),
            money: 0,
            leader_char_id: 0,
            leader_name: String::new(),
            leader_hp: (0, 0),
            leader_mp: (0, 0),
        }
    }

    /// A slot occupied by something this game cannot read. Carries no
    /// preview data (there is none to read), so it differs from
    /// [`Self::empty`] only in [`SlotContent`] and the label - but that
    /// difference is what picks the info panel's caption.
    pub fn foreign(slot: u8) -> Self {
        Self {
            content: SlotContent::Foreign,
            label: format!("Slot {slot}: <unreadable>"),
            ..Self::empty(slot)
        }
    }

    /// Format play time as `HH:MM:SS`.
    pub fn play_time_string(&self) -> String {
        let secs = self.play_time_seconds;
        let h = secs / 3600;
        let m = (secs % 3600) / 60;
        let s = secs % 60;
        format!("{h:02}:{m:02}:{s:02}")
    }
}

/// Save-select operating mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveSelectMode {
    Load,
    Save,
}

/// Number of memory-card directory frames retail's card walk visits.
///
/// The card holds 15 usable blocks; the class array retail writes is 16
/// entries wide because a parsed slot number is a two-digit field.
pub const CARD_DIR_FRAMES: usize = 15;

/// Width of the per-slot class array retail clears before each walk.
pub const CARD_SLOT_CLASSES: usize = 16;

/// Save-filename prefixes retail matches a directory frame against, one
/// per region. Both are 16 bytes - the exact `strncmp` length retail
/// passes - and the two digits that follow are the slot number.
pub const CARD_SAVE_PREFIXES: [&[u8]; 2] = [b"BASCUS-94254PRO_", b"BISCPS-10059PRO_"];

/// Length retail compares a directory filename over.
const CARD_PREFIX_LEN: usize = 16;

/// Classify a memory card's directory into per-slot [`SlotContent`].
///
/// This is the producer for the class byte [`SlotContent`] models: retail
/// clears the array, walks the 15 directory frames matching each filename
/// against either regional save prefix, and stamps class `1` on every slot
/// a matched filename names. Only *after* that does it spend the card's
/// reported free-block count marking still-unclassified slots class `2`
/// (free) - so a block the walk neither matched nor could afford to call
/// free stays class `0`, which is [`SlotContent::Foreign`].
///
/// That ordering is the whole point: absence of a match is not evidence a
/// block is free. Retail only calls a block free when the card's own
/// free-block count pays for it, which is why an unreadable foreign save
/// captions as "not a Legend of Legaia save" rather than inviting an
/// overwrite.
///
/// `frames` are the raw directory frames (retail reads a `0x28`-byte
/// stride; only the leading filename field matters here). `avail_blocks`
/// is the card's free-block count, which retail queries separately before
/// the walk.
///
/// One deliberate departure: retail's budget loop is bounded only by the
/// budget (`bgtz` on the counter, no slot bound) and its match loop
/// stamps `class[slot]` with no range check, so a malformed card can walk
/// off the end of the 16-byte array. Both loops are bounded here.
///
/// NOT WIRED: no caller outside this module's tests. The engine's save
/// UI builds its slot list from [`SaveSelectSession`], which
/// `engine-shell`'s window driver does drive; this classifier is the
/// retail card-directory mirror and no host reads a physical card
/// directory through it yet.
///
/// PORT: FUN_801E1208
pub fn classify_card_directory(
    frames: &[&[u8]],
    avail_blocks: u32,
) -> [SlotContent; CARD_SLOT_CLASSES] {
    // Retail clears the class array (and its sibling scanned-flag array)
    // before every walk. Class 0 is Foreign - "occupied by something
    // unreadable" - which is what an untouched entry means here.
    let mut classes = [SlotContent::Foreign; CARD_SLOT_CLASSES];

    for frame in frames.iter().take(CARD_DIR_FRAMES) {
        let Some(slot) = card_dir_slot_of(frame) else {
            continue;
        };
        if let Some(cell) = classes.get_mut(slot) {
            *cell = SlotContent::LegaiaSave;
        }
    }

    // Spend the card's free-block budget on slots the walk left unclaimed.
    // Retail decrements a counter rather than testing a bound, so a card
    // reporting more free blocks than there are unclaimed slots simply
    // runs out of slots first.
    let mut avail = avail_blocks;
    for cell in classes.iter_mut() {
        if avail == 0 {
            break;
        }
        if *cell == SlotContent::Foreign {
            *cell = SlotContent::Free;
            avail -= 1;
        }
    }

    classes
}

/// Parse the slot number a directory frame's filename encodes, or `None`
/// when the filename is not one of this game's saves.
///
/// Retail reads the two bytes straight after the 16-byte prefix as ASCII
/// digits. The second digit is optional: it only folds into the number
/// when it actually is a digit, so a one-digit name parses as its single
/// digit rather than being rejected.
///
/// REF: FUN_801E1208 (the filename match + digit parse it inlines).
fn card_dir_slot_of(frame: &[u8]) -> Option<usize> {
    if frame.len() < CARD_PREFIX_LEN + 2 {
        return None;
    }
    let matched = CARD_SAVE_PREFIXES
        .iter()
        .any(|p| &frame[..CARD_PREFIX_LEN] == *p);
    if !matched {
        return None;
    }
    let hi = frame[CARD_PREFIX_LEN].wrapping_sub(b'0');
    let lo = frame[CARD_PREFIX_LEN + 1].wrapping_sub(b'0');
    let slot = if lo < 10 {
        hi as usize * 10 + lo as usize
    } else {
        hi as usize
    };
    Some(slot)
}

/// Build the session's slot list straight off a card directory.
///
/// Pairs [`classify_card_directory`] with the two content-keyed
/// [`SlotSnapshot`] constructors so a card-backed host gets a slot list
/// whose captions already follow the retail class byte. Slots the walk
/// classified as [`SlotContent::LegaiaSave`] come back marked `present`
/// but without preview data - a host fills those in by reading the block
/// itself, which is a separate card read in retail too.
pub fn card_directory_slots(frames: &[&[u8]], avail_blocks: u32) -> Vec<SlotSnapshot> {
    classify_card_directory(frames, avail_blocks)
        .into_iter()
        .take(CARD_DIR_FRAMES)
        .enumerate()
        .map(|(i, content)| {
            let slot = i as u8;
            match content {
                SlotContent::Free => SlotSnapshot::empty(slot),
                SlotContent::Foreign => SlotSnapshot::foreign(slot),
                SlotContent::LegaiaSave => SlotSnapshot {
                    present: true,
                    content,
                    label: format!("Slot {slot}"),
                    ..SlotSnapshot::empty(slot)
                },
            }
        })
        .collect()
}

/// What the bottom info panel shows for the focused grid cell.
///
/// Retail passes this to the panel renderer as a `view_mode` int; the
/// variants below carry the retail numbers. Two retail modes have no port
/// equivalent and are deliberately absent: `4` ("Return") belongs to a
/// sixteenth cell the 5x3 block grid does not have, and `100` (blank) is
/// forced while the "Now checking" dialog is up, which the port models as a
/// separate [`SelectPhase`] that does not draw the panel at all.
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
}

impl SlotInfoMode {
    /// Pick the mode for a slot. Mirrors FUN_801E3F74's branch order.
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
    Done(SelectOutcome),
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
    /// "Now checking" timer expired; slot-preview phase entered.
    EnteredSlotPreview {
        slot: u8,
    },
    /// User confirmed the load from the slot-preview screen (X on
    /// SlotPreview).
    LoadConfirmed {
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

/// Save-select session state machine.
#[derive(Debug, Clone)]
pub struct SaveSelectSession {
    mode: SaveSelectMode,
    slots: Vec<SlotSnapshot>,
    phase: SelectPhase,
    /// Frames the "Now checking" dialog stays visible after the user
    /// confirms a Load on a non-empty slot. Public knob so tests can
    /// fast-forward through the dialog without ticking 120 times.
    now_checking_frames: u16,
    /// 12-bit fixed-point slide-in animation timer (0..=4096). Holds
    /// at 0 while Browsing/Done. On entry to `NowChecking` it resets
    /// to 0 and ramps `+SLIDE_ANIM_RATE` per tick, clamped at
    /// `SLIDE_ANIM_FULL`. Drives the linear interpolation
    /// `pos = start + (target - start) * t / 4096` used by the
    /// renderer to slide the slot composite + NowChecking dialog
    /// into place. Mirrors retail's per-element `DAT_801ef194` /
    /// `DAT_801ef160` (collapsed into a single timer here since the
    /// engine doesn't currently break the slide into independent
    /// elements).
    slide_anim_t: u16,
    /// 12-bit fixed-point slide-in timer for the bottom info panel.
    /// Mirrors retail's `DAT_801ef1a0` in `FUN_801E08D8`, which is
    /// distinct from the slot-composite timer above: the info panel
    /// starts hidden (off-screen below the stage at y=394) and slides
    /// up to its parked position (y=138) only AFTER NowChecking
    /// completes. Holds at 0 during Browsing / NowChecking / Done;
    /// ramps during SlotPreview / ConfirmOverwrite / ConfirmDelete.
    info_panel_slide_anim_t: u16,
    /// Opt-in retail two-stage flow: the slot list is the console's two
    /// **memory-card slots** rather than the save blocks themselves, so
    /// Save mode must cross the same `NowChecking` card-read beat Load
    /// mode does before the host can show the card's block grid. See the
    /// module docs. Default `false` = the flat block-list model.
    card_slots_mode: bool,
}

impl SaveSelectSession {
    pub fn new(mode: SaveSelectMode, slots: Vec<SlotSnapshot>) -> Self {
        let phase = if slots.is_empty() {
            SelectPhase::Done(SelectOutcome::Cancelled)
        } else {
            SelectPhase::Browsing { cursor: 0 }
        };
        Self {
            mode,
            slots,
            phase,
            now_checking_frames: DEFAULT_NOW_CHECKING_FRAMES,
            slide_anim_t: 0,
            info_panel_slide_anim_t: 0,
            card_slots_mode: false,
        }
    }

    /// Opt into the retail two-stage memory-card flow (see the module
    /// docs). Set this when the slot list models the console's two
    /// **memory-card slots** instead of individual save blocks: Save mode
    /// then crosses the `NowChecking` card-read beat and lands in
    /// [`SelectPhase::SlotPreview`], where the host renders the chosen
    /// card's block grid, and the overwrite prompt fires from the preview
    /// rather than from the pill row.
    pub fn set_card_slots_mode(&mut self, on: bool) {
        self.card_slots_mode = on;
    }

    /// `true` when the two-stage memory-card flow is enabled.
    pub fn card_slots_mode(&self) -> bool {
        self.card_slots_mode
    }

    /// Current slide-in animation t (12-bit fixed-point 0..=4096).
    /// Render code interpolates `pos = start + (target - start) * t /
    /// 4096`. Returns 0 outside the Load-active phases (Browsing /
    /// Done). See [`SLIDE_ANIM_RATE`] for the ramp rate and
    /// [`SLIDE_ANIM_FULL`] for the fully-arrived sentinel.
    pub fn slide_anim_t(&self) -> u16 {
        self.slide_anim_t
    }

    /// Current info-panel slide-in animation t (12-bit fixed-point
    /// 0..=4096). Renderer uses this to interpolate the panel's
    /// y-origin between [`INFO_PANEL_OFFSCREEN_Y`] (off-screen below
    /// stage, t=0) and [`INFO_PANEL_PARKED_Y`] (parked under load
    /// chrome, t=4096) via [`interpolate_anim`]. Mirrors retail's
    /// `DAT_801ef1a0` which is held to 0 by `FUN_801DD35C` until the
    /// NowChecking dialog completes, then ramps during SlotPreview /
    /// Confirm phases.
    pub fn info_panel_slide_anim_t(&self) -> u16 {
        self.info_panel_slide_anim_t
    }

    /// Override the "Now checking" dialog duration (frames @ 60 Hz).
    /// Tests use this to bypass the 2-second beat.
    pub fn set_now_checking_frames(&mut self, frames: u16) {
        self.now_checking_frames = frames;
    }

    /// Read-only accessor used by render code to compute the dialog's
    /// dwell percentage.
    pub fn now_checking_frames(&self) -> u16 {
        self.now_checking_frames
    }

    pub fn mode(&self) -> SaveSelectMode {
        self.mode
    }

    pub fn slots(&self) -> &[SlotSnapshot] {
        &self.slots
    }

    pub fn phase(&self) -> SelectPhase {
        self.phase
    }

    pub fn is_done(&self) -> bool {
        matches!(self.phase, SelectPhase::Done(_))
    }

    pub fn outcome(&self) -> Option<SelectOutcome> {
        match self.phase {
            SelectPhase::Done(o) => Some(o),
            _ => None,
        }
    }

    /// Index of the slot the cursor is pointing at, regardless of phase.
    pub fn current_slot(&self) -> u8 {
        match self.phase {
            SelectPhase::Browsing { cursor } => cursor,
            SelectPhase::NowChecking { slot, .. } | SelectPhase::SlotPreview { slot } => slot,
            SelectPhase::ConfirmOverwrite { slot, .. }
            | SelectPhase::ConfirmDelete { slot, .. } => slot,
            SelectPhase::Done(_) => 0,
        }
    }

    fn slot_at(&self, idx: u8) -> Option<&SlotSnapshot> {
        self.slots.get(idx as usize)
    }

    pub fn tick(&mut self, input: SelectInput) -> Vec<SelectEvent> {
        let mut events = Vec::new();
        match self.phase {
            SelectPhase::Browsing { cursor } => self.tick_browsing(cursor, input, &mut events),
            SelectPhase::NowChecking {
                slot,
                frames_remaining,
            } => {
                self.tick_now_checking(slot, frames_remaining, &mut events);
            }
            SelectPhase::SlotPreview { slot } => {
                self.tick_slot_preview(slot, input, &mut events);
            }
            SelectPhase::ConfirmOverwrite { slot, cursor } => {
                self.tick_confirm(ConfirmKind::Overwrite, slot, cursor, input, &mut events);
            }
            SelectPhase::ConfirmDelete { slot, cursor } => {
                self.tick_confirm(ConfirmKind::Delete, slot, cursor, input, &mut events);
            }
            SelectPhase::Done(_) => {}
        }
        self.advance_slide_anim();
        events
    }

    /// Ramps `slide_anim_t` + `info_panel_slide_anim_t` based on the
    /// current phase.
    ///
    /// * `slide_anim_t` (slot composite pill + NowChecking dialog):
    ///   holds at 0 during Browsing/Done; ramps during NowChecking /
    ///   SlotPreview / Confirm.
    /// * `info_panel_slide_anim_t` (bottom info panel): holds at 0
    ///   during Browsing / NowChecking / Done so the panel stays
    ///   off-screen while the "checking" beat runs; ramps during
    ///   SlotPreview / Confirm so the panel slides up only after the
    ///   dialog dismisses. Mirrors retail's two-stage flow where
    ///   `DAT_801ef1a0` only starts incrementing once `DAT_801ef160`
    ///   (NowChecking) has retracted.
    fn advance_slide_anim(&mut self) {
        match self.phase {
            SelectPhase::Browsing { .. } | SelectPhase::Done(_) => {
                self.slide_anim_t = 0;
                self.info_panel_slide_anim_t = 0;
            }
            SelectPhase::NowChecking { .. } => {
                self.slide_anim_t = self.slide_anim_t.saturating_add(SLIDE_ANIM_RATE);
                if self.slide_anim_t > SLIDE_ANIM_FULL {
                    self.slide_anim_t = SLIDE_ANIM_FULL;
                }
                self.info_panel_slide_anim_t = 0;
            }
            SelectPhase::SlotPreview { .. }
            | SelectPhase::ConfirmOverwrite { .. }
            | SelectPhase::ConfirmDelete { .. } => {
                self.slide_anim_t = self.slide_anim_t.saturating_add(SLIDE_ANIM_RATE);
                if self.slide_anim_t > SLIDE_ANIM_FULL {
                    self.slide_anim_t = SLIDE_ANIM_FULL;
                }
                self.info_panel_slide_anim_t =
                    self.info_panel_slide_anim_t.saturating_add(SLIDE_ANIM_RATE);
                if self.info_panel_slide_anim_t > SLIDE_ANIM_FULL {
                    self.info_panel_slide_anim_t = SLIDE_ANIM_FULL;
                }
            }
        }
    }

    fn tick_browsing(&mut self, cursor: u8, input: SelectInput, events: &mut Vec<SelectEvent>) {
        if input.circle {
            self.phase = SelectPhase::Done(SelectOutcome::Cancelled);
            events.push(SelectEvent::Cancelled);
            return;
        }
        if input.up {
            let new = self.step(cursor, -1);
            if new != cursor {
                self.phase = SelectPhase::Browsing { cursor: new };
                events.push(SelectEvent::CursorMoved { slot: new });
            }
            return;
        }
        if input.down {
            let new = self.step(cursor, 1);
            if new != cursor {
                self.phase = SelectPhase::Browsing { cursor: new };
                events.push(SelectEvent::CursorMoved { slot: new });
            }
            return;
        }
        if input.cross {
            let snap = match self.slot_at(cursor) {
                Some(s) => s.clone(),
                None => return,
            };
            match (self.mode, snap.present) {
                (SaveSelectMode::Load, false) => {
                    events.push(SelectEvent::InvalidConfirm);
                }
                (SaveSelectMode::Load, true) => {
                    // Retail: pressing X on a non-empty slot opens the
                    // "Now checking. Do not remove MEMORY CARD" dialog
                    // for ~2 seconds, then transitions to the slot
                    // preview (portrait grid + info panel).
                    self.phase = SelectPhase::NowChecking {
                        slot: cursor,
                        frames_remaining: self.now_checking_frames,
                    };
                    events.push(SelectEvent::EnteredNowChecking { slot: cursor });
                }
                // Card-slots mode: a Save picks a *card*, not a block, so
                // it crosses the same "Now checking" card-read beat Load
                // does and lands in SlotPreview for the host to draw the
                // card's block grid. `present` here means "a card is in
                // this slot" - an empty slot is nothing to save into.
                (SaveSelectMode::Save, true) if self.card_slots_mode => {
                    self.phase = SelectPhase::NowChecking {
                        slot: cursor,
                        frames_remaining: self.now_checking_frames,
                    };
                    events.push(SelectEvent::EnteredNowChecking { slot: cursor });
                }
                (SaveSelectMode::Save, false) if self.card_slots_mode => {
                    events.push(SelectEvent::InvalidConfirm);
                }
                (SaveSelectMode::Save, true) => {
                    self.phase = SelectPhase::ConfirmOverwrite {
                        slot: cursor,
                        cursor: 1, // default to "No" for safety
                    };
                    events.push(SelectEvent::EnteredConfirm {
                        slot: cursor,
                        kind: ConfirmKind::Overwrite,
                    });
                }
                (SaveSelectMode::Save, false) => {
                    // Empty slot - go straight to "Saved" outcome (no
                    // destructive prompt needed).
                    self.phase = SelectPhase::Done(SelectOutcome::Saved(cursor));
                    events.push(SelectEvent::Confirmed {
                        slot: cursor,
                        kind: ConfirmKind::Overwrite,
                    });
                }
            }
            return;
        }
        if input.triangle && self.mode == SaveSelectMode::Save {
            // Triangle = delete shortcut on the save screen.
            if let Some(s) = self.slot_at(cursor)
                && s.present
            {
                self.phase = SelectPhase::ConfirmDelete {
                    slot: cursor,
                    cursor: 1,
                };
                events.push(SelectEvent::EnteredConfirm {
                    slot: cursor,
                    kind: ConfirmKind::Delete,
                });
            }
        }
    }

    fn tick_now_checking(
        &mut self,
        slot: u8,
        frames_remaining: u16,
        events: &mut Vec<SelectEvent>,
    ) {
        if frames_remaining == 0 {
            self.phase = SelectPhase::SlotPreview { slot };
            events.push(SelectEvent::EnteredSlotPreview { slot });
        } else {
            self.phase = SelectPhase::NowChecking {
                slot,
                frames_remaining: frames_remaining - 1,
            };
        }
    }

    fn tick_slot_preview(&mut self, slot: u8, input: SelectInput, events: &mut Vec<SelectEvent>) {
        if input.circle {
            self.phase = SelectPhase::Browsing { cursor: slot };
            events.push(SelectEvent::SlotPreviewCancelled { slot });
            return;
        }
        if input.cross {
            // Save mode reaches the preview only in card-slots mode (the
            // host is showing the card's block grid). Confirming there is
            // a destructive write, so it lands on the overwrite prompt
            // rather than committing - retail's "Do you wish to save?".
            if self.mode == SaveSelectMode::Save {
                self.phase = SelectPhase::ConfirmOverwrite {
                    slot,
                    cursor: 1, // default to "No" for safety
                };
                events.push(SelectEvent::EnteredConfirm {
                    slot,
                    kind: ConfirmKind::Overwrite,
                });
                return;
            }
            self.phase = SelectPhase::Done(SelectOutcome::Loaded(slot));
            events.push(SelectEvent::LoadConfirmed { slot });
        }
    }

    // PORT: FUN_801d688c (the Yes/No confirm cursor - retail sub-screen 0x03
    // drives it with `FUN_801D688C(&DAT_801E46D0, 2, 1)`). The shared
    // navigator lives in `crate::menu_input`; here it advances the 2-item
    // horizontal cursor and reports confirm / cancel / move, and the Yes/No
    // branch is decided from the resulting cursor (retail return `1` = the
    // caller inspects the cursor to pick Yes vs No).
    fn tick_confirm(
        &mut self,
        kind: ConfirmKind,
        slot: u8,
        cursor: u8,
        input: SelectInput,
        events: &mut Vec<SelectEvent>,
    ) {
        let mut cell = cursor as u32;
        let buttons = NavButtons {
            confirm: input.cross,
            cancel: input.circle,
            // The Yes/No prompt is horizontal; accept the vertical d-pad too
            // so the toggle works with either axis (equivalent to a 2-item
            // wrap either way).
            left: input.left || input.up,
            right: input.right || input.down,
        };
        // Backing out of the prompt returns where it was opened from: the
        // pill row in the flat model, but the card's block grid in
        // card-slots mode (the overwrite prompt is raised from the
        // preview there, so "No" must not eject the player to the pills).
        let back = if self.card_slots_mode && kind == ConfirmKind::Overwrite {
            SelectPhase::SlotPreview { slot }
        } else {
            SelectPhase::Browsing { cursor: slot }
        };
        match menu_cursor_nav(&mut cell, 2, true, buttons) {
            CursorNav::Cancel => {
                self.phase = back;
                events.push(SelectEvent::ConfirmCancelled { slot, kind });
            }
            CursorNav::Moved => {
                let new_cursor = (cell & CURSOR_INDEX_MASK) as u8;
                self.phase = match kind {
                    ConfirmKind::Overwrite => SelectPhase::ConfirmOverwrite {
                        slot,
                        cursor: new_cursor,
                    },
                    ConfirmKind::Delete => SelectPhase::ConfirmDelete {
                        slot,
                        cursor: new_cursor,
                    },
                };
                events.push(SelectEvent::CursorMoved { slot });
            }
            CursorNav::Confirm => {
                if cursor == 0 {
                    // Yes
                    let outcome = match kind {
                        ConfirmKind::Overwrite => SelectOutcome::Saved(slot),
                        ConfirmKind::Delete => SelectOutcome::Deleted(slot),
                    };
                    self.phase = SelectPhase::Done(outcome);
                    events.push(SelectEvent::Confirmed { slot, kind });
                } else {
                    // No → back where the prompt was opened from.
                    self.phase = back;
                    events.push(SelectEvent::ConfirmCancelled { slot, kind });
                }
            }
            CursorNav::None => {}
        }
    }

    /// Interpolate between `(start, target)` using `slide_anim_t()` as
    /// the 12-bit fixed-point t. Mirrors retail's
    /// `pos = start + (target - start) * t / 4096` math from
    /// `FUN_801E1C1C`. At t=0 returns `start`; at t=4096 returns
    /// `target`. Render code uses this to slide UI elements into
    /// place.
    pub fn interpolate(&self, start: (i32, i32), target: (i32, i32)) -> (i32, i32) {
        interpolate_anim(start, target, self.slide_anim_t)
    }

    fn step(&self, from: u8, dir: i8) -> u8 {
        let n = self.slots.len() as i16;
        if n == 0 {
            return from;
        }
        let mut cur = from as i16;
        cur = (cur + dir as i16).rem_euclid(n);
        cur as u8
    }
}

#[cfg(test)]
mod card_directory_tests {
    use super::*;

    /// Build a directory frame naming `slot` with the given prefix.
    fn frame(prefix: &[u8], slot: u8) -> Vec<u8> {
        let mut f = prefix.to_vec();
        f.extend_from_slice(format!("{slot:02}").as_bytes());
        f.resize(0x28, 0);
        f
    }

    fn junk_frame() -> Vec<u8> {
        let mut f = b"BESLES-01234SOME".to_vec();
        f.extend_from_slice(b"07");
        f.resize(0x28, 0);
        f
    }

    /// A matched filename stamps its slot as a Legaia save, and both
    /// regional prefixes match.
    #[test]
    fn matched_frames_classify_as_legaia_saves() {
        for prefix in CARD_SAVE_PREFIXES {
            let f = frame(prefix, 3);
            let classes = classify_card_directory(&[&f], 0);
            assert_eq!(classes[3], SlotContent::LegaiaSave, "prefix {prefix:?}");
        }
    }

    /// Absence of a match is not evidence of a free block: with no free
    /// blocks reported, every unmatched slot stays Foreign rather than
    /// inviting an overwrite.
    #[test]
    fn unmatched_slots_stay_foreign_without_free_blocks() {
        let junk = junk_frame();
        let classes = classify_card_directory(&[&junk], 0);
        assert!(classes.iter().all(|c| *c == SlotContent::Foreign));
    }

    /// The free-block budget is spent on unclaimed slots in order, and
    /// runs out - it never overwrites a matched slot's class.
    #[test]
    fn free_block_budget_fills_unclaimed_slots_in_order() {
        let f = frame(CARD_SAVE_PREFIXES[0], 0);
        let classes = classify_card_directory(&[&f], 2);
        assert_eq!(classes[0], SlotContent::LegaiaSave);
        assert_eq!(classes[1], SlotContent::Free);
        assert_eq!(classes[2], SlotContent::Free);
        assert_eq!(classes[3], SlotContent::Foreign);
    }

    /// A card reporting more free blocks than there are unclaimed slots
    /// simply runs out of slots.
    #[test]
    fn oversized_free_budget_saturates() {
        let classes = classify_card_directory(&[], 999);
        assert!(classes.iter().all(|c| *c == SlotContent::Free));
    }

    /// The two loops are ordered, and the order is the correctness
    /// property: every matched filename stamps its slot before any free
    /// block is spent, so a budget large enough to cover the whole card
    /// still cannot downgrade a real save to "free". Running the budget
    /// first - or folding the two into one sweep - would offer an
    /// occupied slot up for overwrite.
    #[test]
    fn matched_slots_survive_a_budget_that_covers_the_card() {
        let saves = [2u8, 7, 11];
        let frames: Vec<Vec<u8>> = saves
            .iter()
            .map(|s| frame(CARD_SAVE_PREFIXES[0], *s))
            .collect();
        let refs: Vec<&[u8]> = frames.iter().map(|f| f.as_slice()).collect();

        let classes = classify_card_directory(&refs, u32::MAX);
        for (slot, class) in classes.iter().enumerate() {
            let expected = if saves.contains(&(slot as u8)) {
                SlotContent::LegaiaSave
            } else {
                SlotContent::Free
            };
            assert_eq!(*class, expected, "slot {slot}");
        }
    }

    /// The slot number is the two digits after the prefix; a non-digit
    /// in the second position leaves a one-digit number rather than
    /// rejecting the name.
    #[test]
    fn slot_number_parses_one_and_two_digit_names() {
        let mut two = CARD_SAVE_PREFIXES[0].to_vec();
        two.extend_from_slice(b"12");
        two.resize(0x28, 0);
        assert_eq!(card_dir_slot_of(&two), Some(12));

        let mut one = CARD_SAVE_PREFIXES[0].to_vec();
        one.extend_from_slice(b"5_");
        one.resize(0x28, 0);
        assert_eq!(card_dir_slot_of(&one), Some(5));
    }

    /// Only the first fifteen frames are walked - the sixteenth class
    /// cell exists for the digit space, not for a block.
    #[test]
    fn walk_stops_after_fifteen_frames() {
        let f = frame(CARD_SAVE_PREFIXES[0], 15);
        let frames: Vec<&[u8]> = std::iter::repeat_n(f.as_slice(), 16).collect();
        let classes = classify_card_directory(&frames, 0);
        // All sixteen frames name slot 15, so it is claimed either way;
        // what matters is that the walk itself is bounded.
        assert_eq!(classes[15], SlotContent::LegaiaSave);
    }

    /// The snapshot builder keys each slot's constructor off its class,
    /// so captions follow the retail class byte without a second scan.
    #[test]
    fn snapshots_follow_the_class_byte() {
        let f = frame(CARD_SAVE_PREFIXES[0], 1);
        let snaps = card_directory_slots(&[&f], 1);
        assert_eq!(snaps.len(), CARD_DIR_FRAMES);
        assert_eq!(snaps[0].content, SlotContent::Free);
        assert!(!snaps[0].present);
        assert_eq!(snaps[1].content, SlotContent::LegaiaSave);
        assert!(snaps[1].present);
        assert_eq!(snaps[2].content, SlotContent::Foreign);
        assert!(!snaps[2].present);
    }

    /// A foreign block captions as "not a Legaia save" rather than as a
    /// free block - the whole reason the class byte is kept.
    #[test]
    fn foreign_blocks_do_not_caption_as_free() {
        let snaps = card_directory_slots(&[], 0);
        let mode = SlotInfoMode::for_slot(&snaps[0]);
        assert_eq!(mode, SlotInfoMode::NotLegaiaSave);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slots(present_set: &[bool]) -> Vec<SlotSnapshot> {
        present_set
            .iter()
            .enumerate()
            .map(|(i, p)| {
                if *p {
                    SlotSnapshot {
                        slot: i as u8,
                        present: true,
                        content: SlotContent::LegaiaSave,
                        label: format!("Slot {i}"),
                        play_time_seconds: 1234,
                        party_lv: 5,
                        location: "Town01".into(),
                        money: 100,
                        leader_char_id: 0,
                        leader_name: "Vahn".into(),
                        leader_hp: (100, 100),
                        leader_mp: (20, 20),
                    }
                } else {
                    SlotSnapshot::empty(i as u8)
                }
            })
            .collect()
    }

    #[test]
    fn empty_slots_session_done_immediately() {
        let s = SaveSelectSession::new(SaveSelectMode::Load, vec![]);
        assert!(s.is_done());
        assert_eq!(s.outcome(), Some(SelectOutcome::Cancelled));
    }

    #[test]
    fn slot_info_mode_follows_what_occupies_the_block() {
        let mut snap = SlotSnapshot::empty(0);
        // A free block, which is what `empty` means.
        assert_eq!(SlotInfoMode::for_slot(&snap), SlotInfoMode::FreeBlock);

        snap.content = SlotContent::Foreign;
        assert_eq!(SlotInfoMode::for_slot(&snap), SlotInfoMode::NotLegaiaSave);

        snap.content = SlotContent::LegaiaSave;
        assert_eq!(SlotInfoMode::for_slot(&snap), SlotInfoMode::Preview);
    }

    #[test]
    fn only_a_free_block_captions_differently_per_mode() {
        // A readable save fills the panel with stats, not a caption.
        assert_eq!(SlotInfoMode::Preview.caption(SaveSelectMode::Load), None);
        assert_eq!(SlotInfoMode::Preview.caption(SaveSelectMode::Save), None);

        // A foreign save reads the same either way.
        for m in [SaveSelectMode::Load, SaveSelectMode::Save] {
            assert_eq!(
                SlotInfoMode::NotLegaiaSave.caption(m),
                Some("Not a Legend of Legaia save.")
            );
        }

        // A free block is the one case that depends on why we're here.
        assert_eq!(
            SlotInfoMode::FreeBlock.caption(SaveSelectMode::Save),
            Some("Able to save.")
        );
        assert_eq!(
            SlotInfoMode::FreeBlock.caption(SaveSelectMode::Load),
            Some("No data")
        );
    }

    #[test]
    fn every_unloadable_slot_gets_a_caption() {
        // The bug this guards: an unreadable slot drew an empty panel.
        // Whatever a slot holds, if it has no preview it must have words.
        for content in [SlotContent::Free, SlotContent::Foreign] {
            let snap = SlotSnapshot {
                content,
                ..SlotSnapshot::empty(3)
            };
            for m in [SaveSelectMode::Load, SaveSelectMode::Save] {
                let caption = SlotInfoMode::for_slot(&snap).caption(m);
                assert!(
                    caption.is_some_and(|c| !c.is_empty()),
                    "{content:?} in {m:?} mode left the panel blank"
                );
            }
        }
    }

    #[test]
    fn load_empty_slot_invalid_confirm() {
        let mut s = SaveSelectSession::new(SaveSelectMode::Load, slots(&[false; 3]));
        let events = s.tick(SelectInput {
            cross: true,
            ..Default::default()
        });
        assert!(events.contains(&SelectEvent::InvalidConfirm));
        assert!(matches!(s.phase(), SelectPhase::Browsing { .. }));
    }

    #[test]
    fn load_full_slot_runs_now_checking_then_preview_then_load() {
        let mut s = SaveSelectSession::new(SaveSelectMode::Load, slots(&[true, false, false]));
        // Use a short timer so the test doesn't loop 120 times.
        s.set_now_checking_frames(2);
        let events = s.tick(SelectInput {
            cross: true,
            ..Default::default()
        });
        // Should enter NowChecking with the configured frame count.
        match s.phase() {
            SelectPhase::NowChecking {
                slot: 0,
                frames_remaining: 2,
            } => {}
            other => panic!("expected NowChecking, got {other:?}"),
        }
        assert!(events.contains(&SelectEvent::EnteredNowChecking { slot: 0 }));
        // Tick 3 times: counts down 2→1→0→SlotPreview.
        s.tick(SelectInput::default());
        s.tick(SelectInput::default());
        let events = s.tick(SelectInput::default());
        match s.phase() {
            SelectPhase::SlotPreview { slot: 0 } => {}
            other => panic!("expected SlotPreview, got {other:?}"),
        }
        assert!(events.contains(&SelectEvent::EnteredSlotPreview { slot: 0 }));
        // X on preview confirms load.
        let events = s.tick(SelectInput {
            cross: true,
            ..Default::default()
        });
        assert_eq!(s.outcome(), Some(SelectOutcome::Loaded(0)));
        assert!(events.contains(&SelectEvent::LoadConfirmed { slot: 0 }));
    }

    #[test]
    fn now_checking_ignores_input() {
        let mut s = SaveSelectSession::new(SaveSelectMode::Load, slots(&[true, false, false]));
        s.set_now_checking_frames(5);
        s.tick(SelectInput {
            cross: true,
            ..Default::default()
        });
        // Pressing X again while in NowChecking is a no-op (just ticks down).
        s.tick(SelectInput {
            cross: true,
            ..Default::default()
        });
        match s.phase() {
            SelectPhase::NowChecking {
                frames_remaining: 4,
                ..
            } => {}
            other => panic!("input must not skip NowChecking; got {other:?}"),
        }
    }

    #[test]
    fn slot_preview_circle_returns_to_browsing() {
        let mut s = SaveSelectSession::new(SaveSelectMode::Load, slots(&[true, false, false]));
        s.set_now_checking_frames(0);
        s.tick(SelectInput {
            cross: true,
            ..Default::default()
        });
        // Frames=0 → first NowChecking tick advances to SlotPreview.
        s.tick(SelectInput::default());
        match s.phase() {
            SelectPhase::SlotPreview { slot: 0 } => {}
            other => panic!("expected SlotPreview, got {other:?}"),
        }
        let events = s.tick(SelectInput {
            circle: true,
            ..Default::default()
        });
        match s.phase() {
            SelectPhase::Browsing { cursor: 0 } => {}
            other => panic!("expected Browsing, got {other:?}"),
        }
        assert!(events.contains(&SelectEvent::SlotPreviewCancelled { slot: 0 }));
    }

    #[test]
    fn save_overwrite_default_cursor_is_no() {
        let mut s = SaveSelectSession::new(SaveSelectMode::Save, slots(&[true, false, false]));
        s.tick(SelectInput {
            cross: true,
            ..Default::default()
        });
        match s.phase() {
            SelectPhase::ConfirmOverwrite { cursor: 1, .. } => {}
            _ => panic!("default cursor should be 'No'"),
        }
    }

    #[test]
    fn overwrite_cursor_toggles_with_directions() {
        let mut s = SaveSelectSession::new(SaveSelectMode::Save, slots(&[true, false, false]));
        s.tick(SelectInput {
            cross: true,
            ..Default::default()
        });
        // cursor at 1 (No)
        s.tick(SelectInput {
            left: true,
            ..Default::default()
        });
        match s.phase() {
            SelectPhase::ConfirmOverwrite { cursor: 0, .. } => {}
            _ => panic!(),
        }
        // Confirm Yes.
        s.tick(SelectInput {
            cross: true,
            ..Default::default()
        });
        assert_eq!(s.outcome(), Some(SelectOutcome::Saved(0)));
    }

    #[test]
    fn save_into_empty_slot_goes_directly_to_saved() {
        let mut s = SaveSelectSession::new(SaveSelectMode::Save, slots(&[false, false, false]));
        s.tick(SelectInput {
            cross: true,
            ..Default::default()
        });
        assert_eq!(s.outcome(), Some(SelectOutcome::Saved(0)));
    }

    #[test]
    fn cursor_wraps() {
        let mut s = SaveSelectSession::new(SaveSelectMode::Load, slots(&[false, false, false]));
        s.tick(SelectInput {
            up: true,
            ..Default::default()
        });
        match s.phase() {
            SelectPhase::Browsing { cursor: 2 } => {}
            other => panic!("up from 0 should wrap to 2; got {other:?}"),
        }
    }

    #[test]
    fn circle_cancels_session() {
        let mut s = SaveSelectSession::new(SaveSelectMode::Load, slots(&[true, false, false]));
        s.tick(SelectInput {
            circle: true,
            ..Default::default()
        });
        assert_eq!(s.outcome(), Some(SelectOutcome::Cancelled));
    }

    #[test]
    fn circle_in_save_confirm_returns_to_browse() {
        // Save-mode ConfirmOverwrite still keeps the back-on-Circle
        // behavior (NowChecking only fires in Load mode).
        let mut s = SaveSelectSession::new(SaveSelectMode::Save, slots(&[true, false, false]));
        s.tick(SelectInput {
            cross: true,
            ..Default::default()
        });
        s.tick(SelectInput {
            circle: true,
            ..Default::default()
        });
        match s.phase() {
            SelectPhase::Browsing { cursor: 0 } => {}
            _ => panic!(),
        }
    }

    #[test]
    fn delete_shortcut_in_save_mode() {
        let mut s = SaveSelectSession::new(SaveSelectMode::Save, slots(&[true, false, false]));
        s.tick(SelectInput {
            triangle: true,
            ..Default::default()
        });
        match s.phase() {
            SelectPhase::ConfirmDelete { .. } => {}
            other => panic!("expected ConfirmDelete, got {other:?}"),
        }
    }

    #[test]
    fn delete_yes_emits_deleted_outcome() {
        let mut s = SaveSelectSession::new(SaveSelectMode::Save, slots(&[true, false, false]));
        s.tick(SelectInput {
            triangle: true,
            ..Default::default()
        });
        // cursor = 1 (No) - switch to Yes.
        s.tick(SelectInput {
            left: true,
            ..Default::default()
        });
        s.tick(SelectInput {
            cross: true,
            ..Default::default()
        });
        assert_eq!(s.outcome(), Some(SelectOutcome::Deleted(0)));
    }

    // --- card-slots mode (opt-in retail two-stage flow) ---

    #[test]
    fn card_slots_mode_is_off_by_default() {
        let s = SaveSelectSession::new(SaveSelectMode::Save, slots(&[true]));
        assert!(
            !s.card_slots_mode(),
            "the flat block-list model must stay the default so the native \
             shell's save flow is unchanged"
        );
    }

    #[test]
    fn card_slots_save_crosses_now_checking_then_previews() {
        let mut s = SaveSelectSession::new(SaveSelectMode::Save, slots(&[true, false]));
        s.set_card_slots_mode(true);
        s.set_now_checking_frames(1);
        // X on a slot holding a card reads it, exactly like Load mode -
        // NOT the flat model's straight-to-ConfirmOverwrite.
        let events = s.tick(SelectInput {
            cross: true,
            ..Default::default()
        });
        assert!(events.contains(&SelectEvent::EnteredNowChecking { slot: 0 }));
        s.tick(SelectInput::default());
        let events = s.tick(SelectInput::default());
        assert!(matches!(s.phase(), SelectPhase::SlotPreview { slot: 0 }));
        assert!(events.contains(&SelectEvent::EnteredSlotPreview { slot: 0 }));
    }

    #[test]
    fn card_slots_save_on_empty_slot_is_an_invalid_blip() {
        // `present == false` means "no card in this slot" here - there is
        // nothing to save into, so it must blip rather than report Saved
        // (the flat model's empty-slot behaviour).
        let mut s = SaveSelectSession::new(SaveSelectMode::Save, slots(&[false, false]));
        s.set_card_slots_mode(true);
        let events = s.tick(SelectInput {
            cross: true,
            ..Default::default()
        });
        assert!(events.contains(&SelectEvent::InvalidConfirm));
        assert!(matches!(s.phase(), SelectPhase::Browsing { .. }));
        assert!(s.outcome().is_none(), "must not commit a save");
    }

    #[test]
    fn card_slots_save_confirms_overwrite_from_the_preview() {
        let mut s = SaveSelectSession::new(SaveSelectMode::Save, slots(&[true]));
        s.set_card_slots_mode(true);
        s.set_now_checking_frames(0);
        s.tick(SelectInput {
            cross: true,
            ..Default::default()
        });
        s.tick(SelectInput::default());
        assert!(matches!(s.phase(), SelectPhase::SlotPreview { .. }));
        // X on the preview raises the destructive prompt (defaulting to
        // "No"), it does not commit.
        s.tick(SelectInput {
            cross: true,
            ..Default::default()
        });
        match s.phase() {
            SelectPhase::ConfirmOverwrite { slot: 0, cursor: 1 } => {}
            other => panic!("expected ConfirmOverwrite defaulting to No, got {other:?}"),
        }
        // "No" returns to the grid, not to the pill row.
        s.tick(SelectInput {
            circle: true,
            ..Default::default()
        });
        assert!(
            matches!(s.phase(), SelectPhase::SlotPreview { slot: 0 }),
            "cancelling the overwrite must return to the card's block grid"
        );
        // Yes commits.
        s.tick(SelectInput {
            cross: true,
            ..Default::default()
        });
        s.tick(SelectInput {
            left: true,
            ..Default::default()
        });
        s.tick(SelectInput {
            cross: true,
            ..Default::default()
        });
        assert_eq!(s.outcome(), Some(SelectOutcome::Saved(0)));
    }

    #[test]
    fn card_slots_load_still_loads_from_the_preview() {
        let mut s = SaveSelectSession::new(SaveSelectMode::Load, slots(&[true]));
        s.set_card_slots_mode(true);
        s.set_now_checking_frames(0);
        s.tick(SelectInput {
            cross: true,
            ..Default::default()
        });
        s.tick(SelectInput::default());
        s.tick(SelectInput {
            cross: true,
            ..Default::default()
        });
        assert_eq!(s.outcome(), Some(SelectOutcome::Loaded(0)));
    }

    #[test]
    fn flat_save_mode_unchanged_when_card_slots_mode_is_off() {
        // Regression guard for the native shell: with the flag off, Save
        // mode must still go straight from the pill row to the overwrite
        // prompt / Saved outcome.
        let mut s = SaveSelectSession::new(SaveSelectMode::Save, slots(&[true, false]));
        s.tick(SelectInput {
            cross: true,
            ..Default::default()
        });
        assert!(matches!(s.phase(), SelectPhase::ConfirmOverwrite { .. }));
        let mut s = SaveSelectSession::new(SaveSelectMode::Save, slots(&[false]));
        s.tick(SelectInput {
            cross: true,
            ..Default::default()
        });
        assert_eq!(s.outcome(), Some(SelectOutcome::Saved(0)));
    }

    #[test]
    fn play_time_string_format() {
        let mut snap = SlotSnapshot::empty(0);
        snap.play_time_seconds = 3 * 3600 + 25 * 60 + 7;
        assert_eq!(snap.play_time_string(), "03:25:07");
    }

    #[test]
    fn empty_snapshot_label_includes_empty() {
        let s = SlotSnapshot::empty(2);
        assert!(s.label.contains("empty"));
        assert!(!s.present);
    }

    #[test]
    fn slide_anim_holds_at_zero_while_browsing() {
        let mut s = SaveSelectSession::new(SaveSelectMode::Load, slots(&[true, false, false]));
        assert_eq!(s.slide_anim_t(), 0);
        for _ in 0..32 {
            s.tick(SelectInput::default());
            assert_eq!(s.slide_anim_t(), 0, "should stay 0 while Browsing");
        }
    }

    #[test]
    fn slide_anim_ramps_in_now_checking_then_clamps() {
        let mut s = SaveSelectSession::new(SaveSelectMode::Load, slots(&[true, false, false]));
        s.set_now_checking_frames(120);
        // Enter NowChecking.
        s.tick(SelectInput {
            cross: true,
            ..Default::default()
        });
        // The cross-press tick already entered NowChecking and
        // advance_slide_anim ran once -> t=256.
        assert_eq!(s.slide_anim_t(), SLIDE_ANIM_RATE);
        // Tick 15 more times: t goes 256 -> 512 -> ... -> 4096.
        for i in 2..=16 {
            s.tick(SelectInput::default());
            let expected = (SLIDE_ANIM_RATE as u32 * i).min(SLIDE_ANIM_FULL as u32) as u16;
            assert_eq!(
                s.slide_anim_t(),
                expected,
                "frame {i}: expected {expected}, got {}",
                s.slide_anim_t()
            );
        }
        // Should now be clamped at 4096.
        assert_eq!(s.slide_anim_t(), SLIDE_ANIM_FULL);
        s.tick(SelectInput::default());
        assert_eq!(s.slide_anim_t(), SLIDE_ANIM_FULL, "stays clamped");
    }

    #[test]
    fn slide_anim_resets_on_cancel_back_to_browsing() {
        let mut s = SaveSelectSession::new(SaveSelectMode::Load, slots(&[true, false, false]));
        s.set_now_checking_frames(0);
        // Enter NowChecking (cross), then NowChecking auto-advances
        // to SlotPreview on the next tick because frames_remaining=0.
        s.tick(SelectInput {
            cross: true,
            ..Default::default()
        });
        s.tick(SelectInput::default());
        assert!(matches!(s.phase(), SelectPhase::SlotPreview { .. }));
        // Slide should have advanced.
        assert!(s.slide_anim_t() > 0);
        // Cancel back to Browsing.
        s.tick(SelectInput {
            circle: true,
            ..Default::default()
        });
        assert!(matches!(s.phase(), SelectPhase::Browsing { .. }));
        assert_eq!(s.slide_anim_t(), 0, "must reset on cancel");
    }

    #[test]
    fn interpolate_endpoints_match_start_and_target() {
        let start = (160, 96);
        let target = (48, 40);
        assert_eq!(interpolate_anim(start, target, 0), start);
        assert_eq!(interpolate_anim(start, target, SLIDE_ANIM_FULL), target);
        // Midpoint: half-way between (160, 96) and (48, 40) is
        // (104, 68). Integer division truncates toward zero, so
        // verify the exact rounded value.
        let mid = interpolate_anim(start, target, SLIDE_ANIM_FULL / 2);
        assert_eq!(mid, (104, 68));
    }

    #[test]
    fn info_panel_slide_holds_at_zero_in_browsing_and_now_checking() {
        let mut s = SaveSelectSession::new(SaveSelectMode::Load, slots(&[true, false, false]));
        s.set_now_checking_frames(120);
        // Browsing.
        for _ in 0..8 {
            s.tick(SelectInput::default());
            assert_eq!(s.info_panel_slide_anim_t(), 0);
        }
        // Enter NowChecking; the info panel still holds at 0 here so
        // the panel stays hidden while the dialog plays.
        s.tick(SelectInput {
            cross: true,
            ..Default::default()
        });
        for _ in 0..8 {
            s.tick(SelectInput::default());
            assert_eq!(
                s.info_panel_slide_anim_t(),
                0,
                "info panel must stay hidden during NowChecking"
            );
        }
    }

    #[test]
    fn info_panel_slide_ramps_in_slot_preview_then_clamps() {
        let mut s = SaveSelectSession::new(SaveSelectMode::Load, slots(&[true, false, false]));
        s.set_now_checking_frames(0);
        // Enter NowChecking, auto-advance to SlotPreview because
        // frames_remaining = 0.
        s.tick(SelectInput {
            cross: true,
            ..Default::default()
        });
        s.tick(SelectInput::default());
        assert!(matches!(s.phase(), SelectPhase::SlotPreview { .. }));
        // First SlotPreview tick already advanced once.
        let mut last = s.info_panel_slide_anim_t();
        assert_eq!(last, SLIDE_ANIM_RATE);
        // Ramp to clamp.
        for _ in 0..32 {
            s.tick(SelectInput::default());
            let now = s.info_panel_slide_anim_t();
            assert!(now >= last, "monotonic non-decreasing");
            assert!(now <= SLIDE_ANIM_FULL, "never exceeds clamp");
            last = now;
        }
        assert_eq!(last, SLIDE_ANIM_FULL);
    }

    #[test]
    fn info_panel_slide_resets_on_cancel_back_to_browsing() {
        let mut s = SaveSelectSession::new(SaveSelectMode::Load, slots(&[true, false, false]));
        s.set_now_checking_frames(0);
        s.tick(SelectInput {
            cross: true,
            ..Default::default()
        });
        s.tick(SelectInput::default());
        // SlotPreview reached; ramp a few frames.
        for _ in 0..4 {
            s.tick(SelectInput::default());
        }
        assert!(s.info_panel_slide_anim_t() > 0);
        // Cancel back to Browsing.
        s.tick(SelectInput {
            circle: true,
            ..Default::default()
        });
        assert!(matches!(s.phase(), SelectPhase::Browsing { .. }));
        assert_eq!(s.info_panel_slide_anim_t(), 0);
    }

    #[test]
    fn info_panel_offscreen_to_parked_interpolation() {
        // Endpoint check: anim_t=0 -> off-screen y=394; t=4096 -> parked y=138.
        let off = (0, INFO_PANEL_OFFSCREEN_Y);
        let park = (0, INFO_PANEL_PARKED_Y);
        assert_eq!(interpolate_anim(off, park, 0).1, INFO_PANEL_OFFSCREEN_Y);
        assert_eq!(
            interpolate_anim(off, park, SLIDE_ANIM_FULL).1,
            INFO_PANEL_PARKED_Y
        );
    }

    #[test]
    fn interpolate_method_uses_session_anim_t() {
        let mut s = SaveSelectSession::new(SaveSelectMode::Load, slots(&[true, false, false]));
        s.set_now_checking_frames(120);
        // Browsing: t=0 -> returns start.
        assert_eq!(s.interpolate((100, 50), (200, 80)), (100, 50));
        // Enter NowChecking, then tick 16 frames to reach t=4096.
        s.tick(SelectInput {
            cross: true,
            ..Default::default()
        });
        for _ in 0..16 {
            s.tick(SelectInput::default());
        }
        assert_eq!(s.slide_anim_t(), SLIDE_ANIM_FULL);
        assert_eq!(s.interpolate((100, 50), (200, 80)), (200, 80));
    }
}
