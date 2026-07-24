//! Host screen for the retail **developer menus**.
//!
//! Retail reaches its dev tools from debug branches inside the world-map and
//! field controllers: a scrolling row list (`FUN_801EAD98` draws it,
//! `FUN_801E9F64` feeds each row's pad edits), an EVENT FLAG editor in the
//! field overlay (`FUN_801DBD04` / `FUN_801DB8B4` / `FUN_801DB8F4`), a
//! character-parameter editor in the menu overlay (`FUN_801D6E18`) and an
//! equip commit (`FUN_801E5A08`). Each of those kernels is ported in its own
//! module; none of them had a caller, because the engine had no screen to
//! open them from. This module is that screen.
//!
//! What is retail here and what is the port's:
//!
//! * **Retail** - every value transform. Row navigation and per-row stepping
//!   run through [`legaia_engine_vm::world_map_dev_menu`]; the flag editor
//!   through [`crate::dev_menu`]; the character rows and the end-of-tick
//!   stat clamp through [`crate::debug_char_editor`]; the equip write
//!   through [`legaia_engine_vm::dev_equip_commit`].
//! * **The port's** - which rows exist. Retail's list is 24 rows of
//!   world-map debug tooling (`MAP_CHANGE`, `CAMERA`, `ENCOUNT`, ...) whose
//!   backing state is overlay-resident; [`DevMenuRow`] carries the subset
//!   whose backing state the engine actually owns, so no row steps a value
//!   nothing reads.
//!
//! The pad words are the **packed** ones (`_DAT_8007BB84` edge,
//! `_DAT_8007B850` held) built by `FUN_8001822C`, not the raw BIOS layout -
//! see [`crate::dev_menu`]'s `PACK_*` constants.
//!
//! This screen is a developer tool, so the shell keeps it behind an explicit
//! opt-in rather than a menu row a player can reach.

use crate::debug_char_editor::{DebugEditor, clamp_record_stats};
use crate::dev_menu::{
    EventFlagEditor, PACK_CIRCLE, PACK_CROSS, PACK_DOWN, PACK_LEFT, PACK_RIGHT, PACK_UP,
};
use legaia_engine_vm::dev_equip_commit::{EquipCommit, EquipCommitHost, commit_equip};
use legaia_engine_vm::world_map_dev_menu::{clamp1_255_step, wrap12_step};

/// A row of the engine's dev-menu list, named after the retail row it
/// stands in for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DevMenuRow {
    /// `MAP_CHANGE` - the 12-bit map-id ring.
    MapChange,
    /// `ENCOUNT` - the encounter rate, clamped to `1..=255`.
    EncounterRate,
    /// `EVENT_FLAG` - opens the flag editor page.
    EventFlag,
    /// `PLAYER_PARAM` - opens the character-parameter editor page.
    PlayerParam,
    /// `EQUIP` - commits the staged item id onto the staged slot.
    Equip,
}

impl DevMenuRow {
    /// The rows in list order.
    pub const ALL: [DevMenuRow; 5] = [
        DevMenuRow::MapChange,
        DevMenuRow::EncounterRate,
        DevMenuRow::EventFlag,
        DevMenuRow::PlayerParam,
        DevMenuRow::Equip,
    ];

    /// The label the list renderer draws.
    pub fn label(self) -> &'static str {
        match self {
            DevMenuRow::MapChange => "MAP_CHANGE",
            DevMenuRow::EncounterRate => "ENCOUNT",
            DevMenuRow::EventFlag => "EVENT_FLAG",
            DevMenuRow::PlayerParam => "PLAYER_PARAM",
            DevMenuRow::Equip => "EQUIP",
        }
    }
}

/// Which page has the pad.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DevPage {
    /// The row list.
    #[default]
    List,
    /// The EVENT FLAG editor.
    EventFlag,
    /// The character-parameter editor.
    PlayerParam,
}

/// Default encounter rate the `ENCOUNT` row starts on.
pub const DEFAULT_ENCOUNTER_RATE: i32 = 0x20;

/// The whole dev-menu screen.
#[derive(Debug, Clone, Default)]
pub struct DevMenuSession {
    /// Which page has the pad.
    pub page: DevPage,
    /// Cursor over [`DevMenuRow::ALL`].
    pub row: usize,
    /// `MAP_CHANGE` value - a 12-bit ring.
    pub map_id: u16,
    /// `ENCOUNT` value - clamped to `1..=255`.
    pub encounter_rate: i32,
    /// The EVENT FLAG editor's two cursors.
    pub flags: EventFlagEditor,
    /// One byte per flag-list entry (the `+2` byte of each stride-`0xA`
    /// record), with `'X'` marking the end. Empty until a host supplies the
    /// overlay's debug table.
    pub flag_tags: Vec<u8>,
    /// The character-parameter editor's row + character cursors.
    pub chars: DebugEditor,
    /// Item id the `EQUIP` row commits.
    pub equip_item: u8,
    /// Equipment-table `+7` bits of [`Self::equip_item`], supplied by the
    /// host from the static tables.
    pub equip_slot_bits: u8,
    /// SFX cues raised this tick, for the host to drain.
    pub pending_sfx: Vec<u8>,
    /// The last equip commit, for the host to log.
    pub last_equip: Option<EquipCommit>,
}

impl DevMenuSession {
    /// A fresh screen on the first row.
    pub fn new() -> Self {
        Self {
            encounter_rate: DEFAULT_ENCOUNTER_RATE,
            ..Default::default()
        }
    }

    /// The row the cursor is on.
    pub fn current_row(&self) -> DevMenuRow {
        DevMenuRow::ALL[self.row.min(DevMenuRow::ALL.len() - 1)]
    }

    /// The formatted readout of a row, or `None` for the rows that only open
    /// a page.
    pub fn row_value(&self, row: DevMenuRow) -> Option<String> {
        match row {
            DevMenuRow::MapChange => Some(format!("{:03}", self.map_id)),
            DevMenuRow::EncounterRate => Some(format!("{:03}", self.encounter_rate)),
            DevMenuRow::EventFlag => Some(format!("{:04}", self.flags.value)),
            DevMenuRow::PlayerParam => Some(format!("CHR{}", self.chars.character)),
            DevMenuRow::Equip => Some(format!("{:03}", self.equip_item)),
        }
    }

    /// Move the list cursor for one frame of pad edges.
    fn step_row(&mut self, pad_edge: u16) {
        let last = DevMenuRow::ALL.len() - 1;
        if pad_edge & PACK_UP != 0 {
            self.row = if self.row == 0 { last } else { self.row - 1 };
        }
        if pad_edge & PACK_DOWN != 0 {
            self.row = if self.row == last { 0 } else { self.row + 1 };
        }
    }

    /// One frame of the screen.
    ///
    /// `records` is the live four-record party store; the character editor
    /// writes into it and the retail end-of-tick clamp pass runs over every
    /// record afterwards, exactly as `FUN_801D6E18` does - unconditionally,
    /// whichever page is up.
    pub fn tick(&mut self, pad_edge: u16, pad_held: u16, records: &mut [&mut [u8]]) {
        match self.page {
            DevPage::List => self.tick_list(pad_edge, records),
            DevPage::EventFlag => self.tick_event_flag(pad_edge, pad_held),
            DevPage::PlayerParam => self.tick_player_param(pad_edge, pad_held, records),
        }
        for record in records.iter_mut() {
            clamp_record_stats(record);
        }
    }

    fn tick_list(&mut self, pad_edge: u16, records: &mut [&mut [u8]]) {
        self.step_row(pad_edge);
        // Right / Left step the hovered row's value, the same two bits the
        // retail world-map row dispatcher reads.
        let pressed = u32::from(pad_edge);
        match self.current_row() {
            DevMenuRow::MapChange => self.map_id = wrap12_step(self.map_id, pressed),
            DevMenuRow::EncounterRate => {
                self.encounter_rate = clamp1_255_step(self.encounter_rate, pressed)
            }
            DevMenuRow::EventFlag => {
                if pad_edge & PACK_CROSS != 0 {
                    self.page = DevPage::EventFlag;
                }
            }
            DevMenuRow::PlayerParam => {
                if pad_edge & PACK_CROSS != 0 {
                    self.page = DevPage::PlayerParam;
                }
            }
            DevMenuRow::Equip => {
                if pad_edge & PACK_RIGHT != 0 {
                    self.equip_item = self.equip_item.wrapping_add(1);
                }
                if pad_edge & PACK_LEFT != 0 {
                    self.equip_item = self.equip_item.wrapping_sub(1);
                }
                let _ = records;
            }
        }
    }

    fn tick_event_flag(&mut self, pad_edge: u16, pad_held: u16) {
        if pad_edge & PACK_CIRCLE != 0 {
            self.page = DevPage::List;
            return;
        }
        if pad_edge & (PACK_UP | PACK_DOWN) != 0 && !self.flag_tags.is_empty() {
            if pad_edge & PACK_UP != 0 {
                self.flags.list_prev(&self.flag_tags);
            }
            if pad_edge & PACK_DOWN != 0 {
                self.flags.list_next(&self.flag_tags);
            }
        } else {
            self.flags.edit_value(pad_edge, pad_held);
        }
    }

    fn tick_player_param(&mut self, pad_edge: u16, pad_held: u16, records: &mut [&mut [u8]]) {
        if pad_edge & PACK_CIRCLE != 0 {
            self.page = DevPage::List;
            return;
        }
        self.chars
            .tick(u32::from(pad_edge), u32::from(pad_held), records);
        // Retail's confirm row zeroes the `+0x185` span and cues an SFX
        // rather than stepping a field.
        if pad_edge & PACK_CROSS != 0
            && self.chars.row == crate::debug_char_editor::CONFIRM_ROW
            && let Some(record) = records.get_mut(self.chars.character as usize)
            && crate::debug_char_editor::apply_confirm_clear(record)
        {
            self.pending_sfx.push(crate::debug_char_editor::CONFIRM_SFX);
        }
    }

    /// Commit the staged equip onto the selected character.
    ///
    /// This is the `EQUIP` row's confirm; the host calls it when the row is
    /// hovered and Cross is pressed, supplying its own bag through
    /// [`EquipCommitHost`] and the per-character weapon-slot table.
    pub fn commit_equip_row<H: EquipCommitHost>(
        &mut self,
        host: &mut H,
        record: &mut [u8],
        weapon_slot_table: &[i16],
    ) -> Option<EquipCommit> {
        let out = commit_equip(
            host,
            record,
            self.equip_item,
            self.chars.character as usize,
            0,
            self.equip_slot_bits,
            weapon_slot_table,
        );
        self.last_equip = out;
        out
    }

    /// Drain the SFX cues raised since the last call.
    pub fn drain_sfx(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.pending_sfx)
    }
}

/// [`EquipCommitHost`] over the engine's id-keyed bag.
///
/// The retail trait speaks in bag *indices* because retail's bag is an
/// array; the engine's is a `HashMap<item_id, count>`, so the id doubles as
/// its own index here.
pub struct WorldEquipHost<'a> {
    /// The engine bag.
    pub inventory: &'a mut std::collections::HashMap<u8, u8>,
    /// Cues the commit raised.
    pub sfx: Vec<u8>,
}

impl EquipCommitHost for WorldEquipHost<'_> {
    fn find_in_bag(&self, item_id: u8) -> u16 {
        match self.inventory.get(&item_id) {
            Some(n) if *n > 0 => u16::from(item_id),
            _ => legaia_engine_vm::dev_equip_commit::BAG_MISS,
        }
    }

    fn take_from_bag(&mut self, bag_index: u16, qty: u8) {
        if let Some(n) = self.inventory.get_mut(&(bag_index as u8)) {
            *n = n.saturating_sub(qty);
            if *n == 0 {
                self.inventory.remove(&(bag_index as u8));
            }
        }
    }

    fn give_to_bag(&mut self, item_id: u8, qty: u8) {
        *self.inventory.entry(item_id).or_insert(0) = self
            .inventory
            .get(&item_id)
            .copied()
            .unwrap_or(0)
            .saturating_add(qty);
    }

    fn play_sfx(&mut self, cue: u8) {
        self.sfx.push(cue);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::debug_char_editor::offsets;

    fn records() -> Vec<Vec<u8>> {
        (0..4).map(|_| vec![0u8; 0x414]).collect()
    }

    fn drive(s: &mut DevMenuSession, recs: &mut [Vec<u8>], edge: u16, held: u16) {
        let mut views: Vec<&mut [u8]> = recs.iter_mut().map(|r| r.as_mut_slice()).collect();
        s.tick(edge, held, &mut views);
    }

    #[test]
    fn the_list_cursor_wraps_both_ways() {
        let mut s = DevMenuSession::new();
        let mut r = records();
        drive(&mut s, &mut r, PACK_UP, 0);
        assert_eq!(s.current_row(), DevMenuRow::Equip);
        drive(&mut s, &mut r, PACK_DOWN, 0);
        assert_eq!(s.current_row(), DevMenuRow::MapChange);
    }

    #[test]
    fn the_map_row_steps_the_twelve_bit_ring() {
        let mut s = DevMenuSession::new();
        let mut r = records();
        drive(&mut s, &mut r, PACK_LEFT, 0);
        assert_eq!(s.map_id, 0x0FFF, "wraps at the bottom of the ring");
        drive(&mut s, &mut r, PACK_RIGHT, 0);
        assert_eq!(s.map_id, 0);
    }

    #[test]
    fn the_encounter_row_holds_its_clamp() {
        let mut s = DevMenuSession::new();
        s.row = 1;
        s.encounter_rate = 1;
        let mut r = records();
        drive(&mut s, &mut r, PACK_LEFT, 0);
        assert_eq!(s.encounter_rate, 1);
        s.encounter_rate = 255;
        drive(&mut s, &mut r, PACK_RIGHT, 0);
        assert_eq!(s.encounter_rate, 255);
    }

    #[test]
    fn cross_opens_and_circle_closes_the_flag_page() {
        let mut s = DevMenuSession::new();
        s.row = 2;
        let mut r = records();
        drive(&mut s, &mut r, PACK_CROSS, 0);
        assert_eq!(s.page, DevPage::EventFlag);
        drive(&mut s, &mut r, PACK_CIRCLE, 0);
        assert_eq!(s.page, DevPage::List);
    }

    #[test]
    fn the_flag_page_edits_the_value_and_walks_the_list() {
        let mut s = DevMenuSession::new();
        s.page = DevPage::EventFlag;
        s.flag_tags = vec![0x41, 0x41, 0x41, 0x58];
        let mut r = records();
        // Left / Right nudge the raw flag value by one.
        drive(&mut s, &mut r, PACK_RIGHT, 0);
        assert_eq!(s.flags.value, 1);
        // Up / Down walk the flag list while it is populated.
        drive(&mut s, &mut r, PACK_DOWN, 0);
        assert_eq!(s.flags.list_cursor, 1);
        drive(&mut s, &mut r, PACK_UP, 0);
        assert_eq!(s.flags.list_cursor, 0);
        // With no list loaded, Up / Down fall through to the coarse step.
        s.flag_tags.clear();
        s.flags.value = 0x100;
        drive(&mut s, &mut r, PACK_DOWN, 0);
        assert_eq!(s.flags.value, 0x108);
    }

    #[test]
    fn the_param_page_edits_a_record_and_the_clamp_runs_every_tick() {
        let mut s = DevMenuSession::new();
        s.page = DevPage::PlayerParam;
        s.chars.row = 1; // level
        let mut r = records();
        // A level of 0 is out of the sanity range, so the clamp pass alone
        // pulls it to 1 even on an idle frame.
        drive(&mut s, &mut r, 0, 0);
        assert_eq!(r[0][offsets::LEVEL], 1);
        // Right steps the hovered field on the selected character.
        drive(&mut s, &mut r, PACK_RIGHT, 0);
        assert_eq!(r[0][offsets::LEVEL], 2);
    }

    #[test]
    fn the_equip_row_steps_the_staged_item_id() {
        let mut s = DevMenuSession::new();
        s.row = 4;
        let mut r = records();
        drive(&mut s, &mut r, PACK_RIGHT, 0);
        assert_eq!(s.equip_item, 1);
        drive(&mut s, &mut r, PACK_LEFT, 0);
        assert_eq!(s.equip_item, 0);
    }

    #[test]
    fn the_equip_row_commits_through_the_engine_bag() {
        let mut s = DevMenuSession::new();
        s.equip_item = 0x30;
        let mut bag = std::collections::HashMap::from([(0x30u8, 1u8)]);
        let mut host = WorldEquipHost {
            inventory: &mut bag,
            sfx: Vec::new(),
        };
        let mut record = vec![0u8; 0x414];
        let out = s
            .commit_equip_row(&mut host, &mut record, &[2, 2, 2])
            .expect("the bag holds the id");
        assert_eq!(out.slot, 0);
        assert_eq!(record[0x196], 0x30);
        assert!(!host.sfx.is_empty());
        assert!(!bag.contains_key(&0x30), "the stack was consumed");
    }

    #[test]
    fn a_missing_item_commits_nothing() {
        let mut s = DevMenuSession::new();
        s.equip_item = 0x77;
        let mut bag = std::collections::HashMap::new();
        let mut host = WorldEquipHost {
            inventory: &mut bag,
            sfx: Vec::new(),
        };
        let mut record = vec![0u8; 0x414];
        assert!(
            s.commit_equip_row(&mut host, &mut record, &[2, 2, 2])
                .is_none()
        );
        assert_eq!(record[0x196], 0);
    }

    #[test]
    fn every_row_carries_a_label_and_a_readout() {
        let s = DevMenuSession::new();
        for row in DevMenuRow::ALL {
            assert!(!row.label().is_empty());
            assert!(s.row_value(row).is_some());
        }
    }
}
