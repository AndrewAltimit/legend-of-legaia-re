//! Save-slot select session.
//!
//! PORT: FUN_801E08D8 (info-panel renderer), FUN_801E1C1C (slide-in animator)
//! REF: FUN_801DD35C (the sub-mode dispatcher this session runs under)
//!
//! `FUN_801DD35C` is `REF:` here, not `PORT:` - `menu.rs` carries the one
//! `PORT:` for that address, and the catalog counts occurrences. The
//! `overlay_menu`, `overlay_title`, `overlay_save_ui_*` and
//! `overlay_shop_save` dumps of it are the same function (byte-identical
//! bodies), so which overlay hosts it is an open question rather than a
//! per-subsystem variant; see the note in `engine-title/src/title_overlay.rs`.
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
//! chosen card's fifteen blocks. A host declares which of the two it has by
//! building a [`SaveRack`] and calling [`SaveSelectSession::for_rack`];
//! [`SaveRack::CardPorts`] routes Save-mode confirmation through the same
//! `NowChecking` card-read beat Load mode already uses and lands the
//! overwrite prompt after the grid rather than before it. The driver around
//! that second stage - grid cursor, card read, commit target - is
//! [`crate::save_screen::SaveScreenFlow`], so both hosts run one copy of it.
//!
//! Engines call [`SaveSelectSession::tick`] each frame and react to
//! returned [`SelectEvent`]s. The session never reads the save data
//! itself - engines pre-load slot metadata into [`SlotSnapshot`] entries
//! and hand them to [`SaveSelectSession::new`]. A host reading an actual
//! memory card can instead use [`SaveSelectSession::from_card_directory`],
//! which derives the whole slot list from the card's directory.
//!
//! ## The card-read beat
//!
//! Retail does not time the "Now checking" dialog out; it polls four
//! memory-card kernel events every frame (`FUN_801E3900`) and branches on
//! what comes back, with a 120-frame backstop. [`card_status_poll`] is
//! that poll and it runs on every frame of
//! [`SelectPhase::NowChecking`]. A host with card hardware latches the
//! events through [`SaveSelectSession::set_card_events`]; leaving them
//! alone (the default) reduces the beat to the frame countdown a
//! disk-backed host expects.

use legaia_engine_system::menu_input::{CURSOR_INDEX_MASK, CursorNav, NavButtons, menu_cursor_nav};

mod card_directory;
mod card_io;
mod phases;
mod session;

pub use card_directory::*;
pub use card_io::*;
pub use phases::*;
pub use session::*;

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
    /// The block's stored checksum word (`+0x1FFC`) disagrees with a fresh
    /// sum of the words before it. Retail's grid never looks - it classifies
    /// by filename - so a damaged save still prints its cell and its info
    /// panel; the read's verify (`FUN_801DD35C` sub-mode `0x05`,
    /// `0x801DF880`) is where it is refused, as "Damaged data."
    pub damaged: bool,
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
    /// The party rows the info panel prints, one per member, from the
    /// block's **save summary** - what retail's preview reads
    /// (`FUN_801E08D8` over the per-slot copy of the block's `0x200..0x300`),
    /// never the character records. Empty on a snapshot built without a
    /// card block; the panel then prints the leader fields as its one row
    /// ([`Self::panel_members`]).
    pub members: Vec<SlotMember>,
    /// The location string passes retail's test (shorter than 33 bytes,
    /// every byte in `0x20..0xB0`). A block that fails it prints only its
    /// block badge.
    pub location_valid: bool,
    /// The file is filed under the Japanese build's prefix: retail's panel
    /// adds an "Old Filename" line over the location.
    pub old_filename: bool,
    /// The card block the save's file lives in, when this snapshot came off
    /// a card. The grid cell is the save **number**; the BIOS puts the file
    /// in whatever block was free, so the two differ on any card that was
    /// saved out of order.
    pub card_block: Option<u8>,
}

/// One party member of a [`SlotSnapshot`]'s info panel, as the save summary
/// stores it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SlotMember {
    /// Roster id: the save-icon tile drawn as the member's face.
    pub id: u8,
    pub name: String,
    pub level: u8,
    /// Current / maximum HP as stored (signed halfwords).
    pub hp: (i16, i16),
    /// Current / maximum MP.
    pub mp: (i16, i16),
}

impl SlotSnapshot {
    pub fn empty(slot: u8) -> Self {
        Self {
            slot,
            present: false,
            damaged: false,
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
            members: Vec::new(),
            location_valid: true,
            old_filename: false,
            card_block: None,
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

    /// Format play time as `HH:MM:SS`, clamped at `99:59:59` as retail's
    /// panel clamps it (`slti v0,a2,0x64` at `0x801E0B38`: an hour count of
    /// 100 or more prints `99`, `59`, `59`).
    pub fn play_time_string(&self) -> String {
        let (h, m, s) = self.play_time_hms();
        format!("{h:02}:{m:02}:{s:02}")
    }

    /// [`Self::play_time_string`]'s three numbers.
    pub fn play_time_hms(&self) -> (u32, u32, u32) {
        let secs = self.play_time_seconds;
        if secs / 3600 >= 100 {
            (99, 59, 59)
        } else {
            (secs / 3600, (secs % 3600) / 60, secs % 60)
        }
    }

    /// The rows the info panel prints: the summary's members, or - for a
    /// snapshot that carries none - the leader fields as one row.
    pub fn panel_members(&self) -> Vec<SlotMember> {
        // A block whose summary - stored or derived - names no member still
        // lifted a party (that is what `present` means), so it prints the
        // leader rather than a bare panel.
        if !self.members.is_empty() || !self.present {
            return self.members.clone();
        }
        let clamp = |v: u16| v.min(i16::MAX as u16) as i16;
        vec![SlotMember {
            id: self.leader_char_id,
            name: self.leader_name.clone(),
            level: self.party_lv,
            hp: (clamp(self.leader_hp.0), clamp(self.leader_hp.1)),
            mp: (clamp(self.leader_mp.0), clamp(self.leader_mp.1)),
        }]
    }
}

/// Save-select operating mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveSelectMode {
    Load,
    Save,
}

#[cfg(test)]
mod card_directory_tests;

#[cfg(test)]
mod tests;
