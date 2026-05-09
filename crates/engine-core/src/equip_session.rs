//! Equipment-session state machine.
//!
//! Drives the "browse character → pick slot → pick item → confirm swap"
//! flow shared between the field menu's Equip screen and the
//! shop-buy-then-equip flow.
//!
//! The retail engine threads equip state through `FUN_80042558` (the
//! battle-stat aggregator) on commit; this module mirrors that:
//! [`EquipSession::commit`] writes the new equip id into the character
//! record and re-runs [`compute_battle_stats`] so the world's resolved
//! stats stay in sync.

use crate::battle_stats::{
    BattleStats, EquipmentTable, ItemModifier, StatRecord, StatusModifiers, compute_battle_stats,
};
use legaia_engine_vm::status_effects::StatusKind;
use std::collections::HashMap;

/// One equippable item the player can choose from in the browse phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EquipItem {
    pub id: u8,
    /// Slot index this item is meant for (`0..=7`). Items targeting other
    /// slots get filtered out of the browse list.
    pub slot: u8,
    /// `true` when the player owns at least one of these.
    pub owned: bool,
}

/// Phase of the equip session SM.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EquipState {
    /// Cursor on the equipment grid. Cross opens the item picker;
    /// Triangle cycles characters; Circle exits.
    SlotPicker { cursor: u8 },
    /// Cursor on the item list filtered to the active slot. Cross
    /// confirms; Circle goes back to slot picker.
    ItemPicker { slot: u8, cursor: u16 },
    /// "Equip <X>?" Yes/No prompt. Cross confirms; Circle abandons.
    Confirm { slot: u8, item_id: u8, cursor: u8 },
    /// Session done — committed or aborted.
    Done(EquipOutcome),
}

/// Result of a finished equip session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EquipOutcome {
    /// Player committed a swap. `removed` is the previous item id (0 for
    /// "nothing"); `added` is the newly-equipped id.
    Committed { slot: u8, removed: u8, added: u8 },
    /// Session was cancelled without committing.
    Cancelled,
}

/// Per-frame input bundle.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EquipInput {
    pub up: bool,
    pub down: bool,
    pub left: bool,
    pub right: bool,
    pub cross: bool,
    pub circle: bool,
    pub triangle: bool,
}

/// Events the session emits per `input()` call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EquipEvent {
    /// Cursor moved in either picker phase.
    CursorMoved { state: EquipStateKind, cursor: u16 },
    /// Player opened the item picker for a slot.
    EnteredItemPicker { slot: u8 },
    /// Player advanced to the confirm prompt.
    EnteredConfirm { slot: u8, item_id: u8 },
    /// Player committed a swap.
    Committed { slot: u8, removed: u8, added: u8 },
    /// Player cancelled — session terminating.
    Cancelled,
    /// Player tried to equip an item they don't own / item filter rejected.
    InvalidConfirm,
}

/// Phase tag for `EquipEvent::CursorMoved`. Avoids needing a clone of
/// `EquipState`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EquipStateKind {
    SlotPicker,
    ItemPicker,
    Confirm,
}

/// Equip session.
#[derive(Debug, Clone)]
pub struct EquipSession {
    /// Editable copy of the character's stat record. Engines pass in a
    /// snapshot at session start; on commit the session writes back.
    record: StatRecord,
    /// Catalog of items the player owns (one entry per id). Used to
    /// build the per-slot picker list.
    inventory: HashMap<u8, u8>,
    /// Equipment-stat table (resolves modifiers on commit).
    equipment: EquipmentTable,
    /// Status modifiers pulled from the world for the recompute pass.
    modifiers: StatusModifiers,
    /// Currently-applied status set on the character.
    active_status: Vec<StatusKind>,
    /// Current SM state.
    state: EquipState,
    /// Last computed `BattleStats` snapshot for HUD display ("equipping
    /// this item changes your ATK by +5"). Refreshed when the cursor
    /// moves over a candidate item.
    pub preview_stats: BattleStats,
    /// Events drained per `input()`.
    events: Vec<EquipEvent>,
}

impl EquipSession {
    /// Construct a session for an actor.
    pub fn new(
        record: StatRecord,
        inventory: HashMap<u8, u8>,
        equipment: EquipmentTable,
        modifiers: StatusModifiers,
        active_status: Vec<StatusKind>,
    ) -> Self {
        let preview_stats = compute_battle_stats(&record, &equipment, &active_status, &modifiers);
        Self {
            record,
            inventory,
            equipment,
            modifiers,
            active_status,
            state: EquipState::SlotPicker { cursor: 0 },
            preview_stats,
            events: Vec::new(),
        }
    }

    /// Current state.
    pub fn state(&self) -> EquipState {
        self.state
    }

    /// Live record (post-edits, pre-commit).
    pub fn record(&self) -> &StatRecord {
        &self.record
    }

    /// `true` if the session is in `Done` state.
    pub fn is_done(&self) -> bool {
        matches!(self.state, EquipState::Done(_))
    }

    /// Outcome — only valid when `is_done()`.
    pub fn outcome(&self) -> Option<EquipOutcome> {
        match self.state {
            EquipState::Done(out) => Some(out),
            _ => None,
        }
    }

    /// Drain events the SM emitted since the last call.
    pub fn drain_events(&mut self) -> Vec<EquipEvent> {
        std::mem::take(&mut self.events)
    }

    /// Filter the inventory to items targeting `slot` and owned ≥ 1.
    /// Returns a list sorted by item id so cursor positions are stable
    /// across HashMap iteration orderings.
    pub fn items_for_slot(&self, slot: u8) -> Vec<EquipItem> {
        // Each item id encodes its target slot in the upper 3 bits of the
        // id (slot = id >> 5). This is a placeholder rule — engines that
        // wire the real per-item slot from disc data should override
        // [`Self::items_for_slot`] (next revision will accept a closure).
        let mut out: Vec<EquipItem> = self
            .inventory
            .iter()
            .filter_map(|(id, qty)| {
                if *qty == 0 {
                    return None;
                }
                let item_slot = (*id >> 5) & 0x07;
                if item_slot != slot {
                    return None;
                }
                Some(EquipItem {
                    id: *id,
                    slot: item_slot,
                    owned: *qty > 0,
                })
            })
            .collect();
        out.sort_by_key(|i| i.id);
        out
    }

    /// Apply a per-frame input.
    pub fn input(&mut self, input: EquipInput) {
        match self.state {
            EquipState::SlotPicker { mut cursor } => {
                if input.up && cursor > 0 {
                    cursor -= 1;
                    self.state = EquipState::SlotPicker { cursor };
                    self.events.push(EquipEvent::CursorMoved {
                        state: EquipStateKind::SlotPicker,
                        cursor: cursor as u16,
                    });
                } else if input.down && cursor < 7 {
                    cursor += 1;
                    self.state = EquipState::SlotPicker { cursor };
                    self.events.push(EquipEvent::CursorMoved {
                        state: EquipStateKind::SlotPicker,
                        cursor: cursor as u16,
                    });
                } else if input.cross {
                    self.state = EquipState::ItemPicker {
                        slot: cursor,
                        cursor: 0,
                    };
                    self.events
                        .push(EquipEvent::EnteredItemPicker { slot: cursor });
                } else if input.circle {
                    self.state = EquipState::Done(EquipOutcome::Cancelled);
                    self.events.push(EquipEvent::Cancelled);
                }
            }
            EquipState::ItemPicker { slot, mut cursor } => {
                let items = self.items_for_slot(slot);
                let n = items.len();
                if input.up && cursor > 0 {
                    cursor -= 1;
                    self.state = EquipState::ItemPicker { slot, cursor };
                    self.events.push(EquipEvent::CursorMoved {
                        state: EquipStateKind::ItemPicker,
                        cursor,
                    });
                } else if input.down && (cursor as usize + 1) < n {
                    cursor += 1;
                    self.state = EquipState::ItemPicker { slot, cursor };
                    self.events.push(EquipEvent::CursorMoved {
                        state: EquipStateKind::ItemPicker,
                        cursor,
                    });
                } else if input.cross && (cursor as usize) < n {
                    let item = items[cursor as usize];
                    if !item.owned {
                        self.events.push(EquipEvent::InvalidConfirm);
                        return;
                    }
                    self.state = EquipState::Confirm {
                        slot,
                        item_id: item.id,
                        cursor: 0, // Yes selected by default
                    };
                    self.events.push(EquipEvent::EnteredConfirm {
                        slot,
                        item_id: item.id,
                    });
                    // Refresh the preview by simulating the swap.
                    let mut copy = self.record;
                    copy.equip[slot as usize] = item.id;
                    self.preview_stats = compute_battle_stats(
                        &copy,
                        &self.equipment,
                        &self.active_status,
                        &self.modifiers,
                    );
                } else if input.circle {
                    self.state = EquipState::SlotPicker { cursor: slot };
                }
            }
            EquipState::Confirm {
                slot,
                item_id,
                mut cursor,
            } => {
                if input.left && cursor > 0 {
                    cursor -= 1;
                    self.state = EquipState::Confirm {
                        slot,
                        item_id,
                        cursor,
                    };
                    self.events.push(EquipEvent::CursorMoved {
                        state: EquipStateKind::Confirm,
                        cursor: cursor as u16,
                    });
                } else if input.right && cursor < 1 {
                    cursor += 1;
                    self.state = EquipState::Confirm {
                        slot,
                        item_id,
                        cursor,
                    };
                    self.events.push(EquipEvent::CursorMoved {
                        state: EquipStateKind::Confirm,
                        cursor: cursor as u16,
                    });
                } else if input.cross {
                    if cursor == 0 {
                        // Confirm — commit the swap.
                        self.commit(slot, item_id);
                    } else {
                        // No — back to item picker.
                        self.state = EquipState::ItemPicker { slot, cursor: 0 };
                    }
                } else if input.circle {
                    self.state = EquipState::ItemPicker { slot, cursor: 0 };
                }
            }
            EquipState::Done(_) => {}
        }
    }

    /// Commit the swap for `slot → item_id`. Decrements the old item's
    /// inventory count if non-zero; bumps the previous slot occupant's
    /// inventory count.
    fn commit(&mut self, slot: u8, item_id: u8) {
        let removed = self.record.equip[slot as usize];
        // Inventory swap: decrement new item, restore old.
        if let Some(qty) = self.inventory.get_mut(&item_id) {
            *qty = qty.saturating_sub(1);
        }
        if removed != 0 {
            *self.inventory.entry(removed).or_insert(0) += 1;
        }
        self.record.equip[slot as usize] = item_id;
        self.preview_stats = compute_battle_stats(
            &self.record,
            &self.equipment,
            &self.active_status,
            &self.modifiers,
        );
        self.state = EquipState::Done(EquipOutcome::Committed {
            slot,
            removed,
            added: item_id,
        });
        self.events.push(EquipEvent::Committed {
            slot,
            removed,
            added: item_id,
        });
    }

    /// Test-only helper: install an `ItemModifier` that targets a slot
    /// implied by the key encoding (`id >> 5 == slot`).
    pub fn register_item(&mut self, id: u8, modifier: ItemModifier) {
        self.equipment.set(id, modifier);
    }

    /// Direct mutation of the inventory map (for tests / synthetic flows).
    pub fn give_item(&mut self, id: u8, qty: u8) {
        *self.inventory.entry(id).or_insert(0) += qty;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh_record() -> StatRecord {
        StatRecord {
            base_attack: 50,
            base_udf: 30,
            base_ldf: 25,
            base_accuracy: 80,
            base_evasion: 20,
            equip: [0; 8],
        }
    }

    fn fresh_session() -> EquipSession {
        let mut inv = HashMap::new();
        // Item 0x05 — slot (5 >> 5) = 0; helmet-class.
        inv.insert(0x05, 1);
        // Item 0x25 — slot 1 (0x25 >> 5 = 1)
        inv.insert(0x25, 2);
        let mut eq = EquipmentTable::new();
        eq.set(
            0x05,
            ItemModifier {
                atk: 5,
                ..Default::default()
            },
        );
        eq.set(
            0x25,
            ItemModifier {
                udf: 7,
                ..Default::default()
            },
        );
        EquipSession::new(
            fresh_record(),
            inv,
            eq,
            StatusModifiers::default(),
            Vec::new(),
        )
    }

    #[test]
    fn new_session_starts_on_slot_picker_cursor_zero() {
        let s = fresh_session();
        assert_eq!(s.state(), EquipState::SlotPicker { cursor: 0 });
        assert!(!s.is_done());
    }

    #[test]
    fn down_in_slot_picker_advances_cursor_and_emits_event() {
        let mut s = fresh_session();
        s.input(EquipInput {
            down: true,
            ..Default::default()
        });
        assert_eq!(s.state(), EquipState::SlotPicker { cursor: 1 });
        let evs = s.drain_events();
        assert!(matches!(
            evs[0],
            EquipEvent::CursorMoved {
                state: EquipStateKind::SlotPicker,
                cursor: 1
            }
        ));
    }

    #[test]
    fn slot_picker_clamps_cursor_to_zero_on_up() {
        let mut s = fresh_session();
        s.input(EquipInput {
            up: true,
            ..Default::default()
        });
        assert_eq!(s.state(), EquipState::SlotPicker { cursor: 0 });
    }

    #[test]
    fn slot_picker_clamps_cursor_to_seven_on_down() {
        let mut s = fresh_session();
        for _ in 0..20 {
            s.input(EquipInput {
                down: true,
                ..Default::default()
            });
        }
        assert_eq!(s.state(), EquipState::SlotPicker { cursor: 7 });
    }

    #[test]
    fn cross_in_slot_picker_opens_item_picker() {
        let mut s = fresh_session();
        s.input(EquipInput {
            cross: true,
            ..Default::default()
        });
        assert!(matches!(
            s.state(),
            EquipState::ItemPicker { slot: 0, cursor: 0 }
        ));
    }

    #[test]
    fn circle_in_slot_picker_terminates_session_with_cancel() {
        let mut s = fresh_session();
        s.input(EquipInput {
            circle: true,
            ..Default::default()
        });
        assert!(s.is_done());
        assert_eq!(s.outcome(), Some(EquipOutcome::Cancelled));
    }

    #[test]
    fn items_for_slot_filters_by_implied_slot() {
        let s = fresh_session();
        let slot0 = s.items_for_slot(0);
        assert_eq!(slot0.len(), 1);
        assert_eq!(slot0[0].id, 0x05);
        let slot1 = s.items_for_slot(1);
        assert_eq!(slot1.len(), 1);
        assert_eq!(slot1[0].id, 0x25);
        let slot7 = s.items_for_slot(7);
        assert!(slot7.is_empty());
    }

    #[test]
    fn cross_in_item_picker_advances_to_confirm_with_preview() {
        let mut s = fresh_session();
        // Open slot 0's item picker.
        s.input(EquipInput {
            cross: true,
            ..Default::default()
        });
        let _ = s.drain_events();
        // Confirm item 0x05 (atk +5). Preview ATK should be 50 + 5 = 55.
        s.input(EquipInput {
            cross: true,
            ..Default::default()
        });
        match s.state() {
            EquipState::Confirm { slot, item_id, .. } => {
                assert_eq!(slot, 0);
                assert_eq!(item_id, 0x05);
            }
            _ => panic!("expected Confirm state"),
        }
        assert_eq!(s.preview_stats.atk, 55);
    }

    #[test]
    fn cross_yes_in_confirm_commits_swap_and_writes_record() {
        let mut s = fresh_session();
        s.input(EquipInput {
            cross: true,
            ..Default::default()
        });
        s.input(EquipInput {
            cross: true,
            ..Default::default()
        });
        // Now in Confirm with cursor=Yes (0).
        s.input(EquipInput {
            cross: true,
            ..Default::default()
        });
        assert!(s.is_done());
        match s.outcome() {
            Some(EquipOutcome::Committed {
                slot,
                removed,
                added,
            }) => {
                assert_eq!(slot, 0);
                assert_eq!(removed, 0);
                assert_eq!(added, 0x05);
            }
            _ => panic!("expected Committed outcome"),
        }
        assert_eq!(s.record().equip[0], 0x05);
    }

    #[test]
    fn right_in_confirm_moves_cursor_to_no_then_cross_returns_to_item_picker() {
        let mut s = fresh_session();
        s.input(EquipInput {
            cross: true,
            ..Default::default()
        });
        s.input(EquipInput {
            cross: true,
            ..Default::default()
        });
        // Now in Confirm with cursor=0 (Yes).
        s.input(EquipInput {
            right: true,
            ..Default::default()
        });
        match s.state() {
            EquipState::Confirm { cursor, .. } => assert_eq!(cursor, 1),
            _ => panic!("expected Confirm"),
        }
        // Cross with cursor=No should bounce back to item picker.
        s.input(EquipInput {
            cross: true,
            ..Default::default()
        });
        assert!(matches!(
            s.state(),
            EquipState::ItemPicker { slot: 0, cursor: 0 }
        ));
    }

    #[test]
    fn circle_in_confirm_returns_to_item_picker_without_committing() {
        let mut s = fresh_session();
        s.input(EquipInput {
            cross: true,
            ..Default::default()
        });
        s.input(EquipInput {
            cross: true,
            ..Default::default()
        });
        s.input(EquipInput {
            circle: true,
            ..Default::default()
        });
        assert!(matches!(s.state(), EquipState::ItemPicker { slot: 0, .. }));
        assert_eq!(s.record().equip[0], 0);
    }

    #[test]
    fn commit_decrements_new_item_and_restores_old_inventory() {
        let mut s = fresh_session();
        // Pre-equip slot 0 with 0x05 to test the "restore old" path.
        s.give_item(0x06, 1);
        s.equipment.set(
            0x06,
            ItemModifier {
                atk: 7,
                ..Default::default()
            },
        );
        s.record.equip[0] = 0x05;
        s.input(EquipInput {
            cross: true,
            ..Default::default()
        });
        // Item picker shows two items now (0x05 and 0x06). Pick the
        // second (0x06).
        s.input(EquipInput {
            down: true,
            ..Default::default()
        });
        s.input(EquipInput {
            cross: true,
            ..Default::default()
        });
        // Confirm yes.
        s.input(EquipInput {
            cross: true,
            ..Default::default()
        });
        assert_eq!(s.record().equip[0], 0x06);
        // Old item (0x05) returned to inventory: should now have qty 2.
        assert_eq!(s.inventory.get(&0x05).copied().unwrap_or(0), 2);
        // New item (0x06) used: qty was 1, now 0.
        assert_eq!(s.inventory.get(&0x06).copied().unwrap_or(0), 0);
    }

    #[test]
    fn commit_recomputes_battle_stats() {
        let mut s = fresh_session();
        s.input(EquipInput {
            cross: true,
            ..Default::default()
        });
        s.input(EquipInput {
            cross: true,
            ..Default::default()
        });
        s.input(EquipInput {
            cross: true,
            ..Default::default()
        });
        // ATK should now be base 50 + item 0x05's +5 = 55.
        assert_eq!(s.preview_stats.atk, 55);
    }

    #[test]
    fn item_picker_circle_returns_to_slot_picker() {
        let mut s = fresh_session();
        s.input(EquipInput {
            cross: true,
            ..Default::default()
        });
        s.input(EquipInput {
            circle: true,
            ..Default::default()
        });
        assert!(matches!(s.state(), EquipState::SlotPicker { cursor: 0 }));
    }

    #[test]
    fn drain_events_empties_buffer() {
        let mut s = fresh_session();
        s.input(EquipInput {
            down: true,
            ..Default::default()
        });
        let evs = s.drain_events();
        assert!(!evs.is_empty());
        assert!(s.drain_events().is_empty());
    }

    #[test]
    fn item_picker_cursor_clamps_to_list_length() {
        let mut s = fresh_session();
        s.give_item(0x06, 1);
        s.input(EquipInput {
            cross: true,
            ..Default::default()
        });
        // Two items in slot 0 picker now.
        for _ in 0..10 {
            s.input(EquipInput {
                down: true,
                ..Default::default()
            });
        }
        match s.state() {
            EquipState::ItemPicker { cursor, .. } => assert_eq!(cursor, 1),
            _ => panic!("expected item picker"),
        }
    }
}
