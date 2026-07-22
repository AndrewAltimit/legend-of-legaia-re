//! Equipment-session state machine.
//!
//! Drives the "browse character → pick slot → pick item → confirm swap"
//! flow shared between the field menu's Equip screen and the
//! shop-buy-then-equip flow.
//!
//! The retail engine threads equip state through `FUN_80042558` (the
//! battle-stat aggregator) on commit; this module mirrors that:
//! `EquipSession::commit` writes the new equip id into the character
//! record and re-runs [`compute_battle_stats`] so the world's resolved
//! stats stay in sync.

use crate::battle_stats::{
    BattleStats, EquipmentTable, ItemModifier, StatRecord, StatusModifiers, compute_battle_stats,
};
use crate::equipment::{DiscEquipInfo, EquipSlot, engine_slot_disc_category};
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
    /// "Equip `<X>`?" Yes/No prompt. Cross confirms; Circle abandons.
    Confirm { slot: u8, item_id: u8, cursor: u8 },
    /// Session done - committed or aborted.
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
    /// Player cancelled - session terminating.
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
    /// Optional disc-pinned equip restrictions (`DAT_80074F68`). When set,
    /// the per-slot item list is gated on the active character's equip mask
    /// (`+6`) and, for the four unambiguous UI slots, the disc slot category
    /// (`+7`). When `None`, the legacy `id >> 5` placeholder rule is used.
    restrictions: Option<DiscEquipInfo>,
    /// Active party slot (`0` Vahn, `1` Noa, `2` Gala) the session is editing.
    /// Drives the equip-mask gate when `restrictions` is set.
    active_party_slot: u8,
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
            restrictions: None,
            active_party_slot: 0,
        }
    }

    /// Construct a session that gates the item list on the disc-pinned equip
    /// restrictions for `active_party_slot` (`0` Vahn, `1` Noa, `2` Gala).
    /// Each candidate item must be equippable by that character (the `+6`
    /// mask) and, for the weapon / body / helmet / boots slots, match the
    /// item's disc slot category (`+7`).
    #[allow(clippy::too_many_arguments)]
    pub fn new_with_restrictions(
        record: StatRecord,
        inventory: HashMap<u8, u8>,
        equipment: EquipmentTable,
        modifiers: StatusModifiers,
        active_status: Vec<StatusKind>,
        restrictions: DiscEquipInfo,
        active_party_slot: u8,
    ) -> Self {
        let mut s = Self::new(record, inventory, equipment, modifiers, active_status);
        s.restrictions = Some(restrictions);
        s.active_party_slot = active_party_slot;
        s
    }

    /// Current state.
    pub fn state(&self) -> EquipState {
        self.state
    }

    /// Live record (post-edits, pre-commit).
    pub fn record(&self) -> &StatRecord {
        &self.record
    }

    /// Read access to the in-progress inventory map. Engines render the
    /// per-candidate count from this.
    pub fn inventory(&self) -> &HashMap<u8, u8> {
        &self.inventory
    }

    /// Read access to the equipment-stat table backing this session.
    /// Engines render per-candidate stat-delta previews by comparing
    /// `equipment.get(id)` for the candidate against the slot's current
    /// occupant.
    pub fn equipment(&self) -> &EquipmentTable {
        &self.equipment
    }

    /// `true` if the session is in `Done` state.
    pub fn is_done(&self) -> bool {
        matches!(self.state, EquipState::Done(_))
    }

    /// Outcome - only valid when `is_done()`.
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
    ///
    /// When the session carries disc-pinned [`DiscEquipInfo`] restrictions,
    /// the list is gated on the active character's equip mask (`+6`) and, for
    /// the four unambiguous UI slots (weapon / body / helmet / boots), the
    /// item's disc slot category (`+7`). Otherwise it falls back to the legacy
    /// `id >> 5` placeholder rule.
    pub fn items_for_slot(&self, slot: u8) -> Vec<EquipItem> {
        let mut out: Vec<EquipItem> = self
            .inventory
            .iter()
            .filter_map(|(id, qty)| {
                if *qty == 0 {
                    return None;
                }
                if !self.item_fits_slot(*id, slot) {
                    return None;
                }
                Some(EquipItem {
                    id: *id,
                    slot,
                    owned: *qty > 0,
                })
            })
            .collect();
        out.sort_by_key(|i| i.id);
        out
    }

    /// Whether `id` is a valid candidate for UI `slot`. Uses the disc-pinned
    /// restrictions when installed, else the legacy `id >> 5` rule.
    fn item_fits_slot(&self, id: u8, slot: u8) -> bool {
        match &self.restrictions {
            Some(info) => {
                // Must be equippable by the active character (the `+6` mask).
                if !info.can_equip(id, self.active_party_slot) {
                    return false;
                }
                // For the four UI slots the disc `+7` byte resolves cleanly,
                // require a category match; the ambiguous head/hand slots are
                // mask-gated only (the disc cannot separate them).
                match EquipSlot::from_index(slot).and_then(engine_slot_disc_category) {
                    Some(wanted) => info.category(id) == Some(wanted),
                    None => true,
                }
            }
            // Legacy placeholder: the item id encodes its target slot in the
            // upper 3 bits (slot = id >> 5).
            None => ((id >> 5) & 0x07) == slot,
        }
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
                        // Confirm - commit the swap.
                        self.commit(slot, item_id);
                    } else {
                        // No - back to item picker.
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

    /// Refresh [`Self::preview_stats`] for a hovered candidate by
    /// **trial-equipping** it on a copy of the record - the retail
    /// candidate-list handler's preview mechanism: save the record's equip
    /// array into the `DAT_801EF0C8` staging buffer, write the candidate
    /// (or `0` for the Remove row), re-run the stat aggregator
    /// `FUN_801CF650`, restore the array. The engine expresses the
    /// save/restore as a copy.
    ///
    /// PORT: FUN_801d9c14 (state-2 trial-equip stat preview; see
    /// `ghidra/scripts/funcs/overlay_menu_801d9c14.txt` at
    /// `0x801d9e8c..0x801da058`)
    pub fn preview_candidate(&mut self, slot: u8, item_id: u8) {
        let mut copy = self.record;
        if let Some(cell) = copy.equip.get_mut(slot as usize) {
            *cell = item_id;
        }
        self.preview_stats =
            compute_battle_stats(&copy, &self.equipment, &self.active_status, &self.modifiers);
    }

    /// The candidate list's **Remove** row commit (class-`0x4000` payload-0
    /// row): return the equipped item to the bag and clear the slot,
    /// re-running the stat aggregate. Returns the removed id, or `None`
    /// when the slot is already empty - retail buzzes SFX `0x37` there and
    /// commits nothing.
    ///
    /// PORT: FUN_801d9c14 (confirm path `0x801da0b4..0x801da134`: equipped
    /// byte zero -> `FUN_80035B50(0x37)`; else SFX `0x24`,
    /// `FUN_800421D4(equipped, 1)`, `sb zero` into the record slot)
    pub fn unequip(&mut self, slot: u8) -> Option<u8> {
        let removed = *self.record.equip.get(slot as usize)?;
        if removed == 0 {
            return None;
        }
        *self.inventory.entry(removed).or_insert(0) += 1;
        self.record.equip[slot as usize] = 0;
        self.preview_stats = compute_battle_stats(
            &self.record,
            &self.equipment,
            &self.active_status,
            &self.modifiers,
        );
        self.state = EquipState::Done(EquipOutcome::Committed {
            slot,
            removed,
            added: 0,
        });
        self.events.push(EquipEvent::Committed {
            slot,
            removed,
            added: 0,
        });
        Some(removed)
    }

    /// The Equip screen's slot-browse confirm dispatch: row `0` is the
    /// "Best Equipment" auto-equip, rows `1..=7` open the candidate list
    /// for slot `row - 1`. (Retail's cancel leaves for the character
    /// picker, sub-screen `0x12`; the engine host owns that transition.)
    ///
    /// `candidates` are the four best-armament ids the retail candidate
    /// computer `FUN_801CF88C` parks at `DAT_801EF0C0` - the engine host
    /// supplies its own pick.
    ///
    /// PORT: FUN_801d99f0 (menu-overlay sub-screen `0x13`; see
    /// `ghidra/scripts/funcs/overlay_menu_801d99f0.txt` - confirm on row 0
    /// runs the applier and cues SFX `0x24` on change / buzz `0x23` on
    /// none, other rows hand off to sub-screen `0x14`)
    pub fn slot_browse_confirm(&mut self, row: u8, candidates: [u8; 4]) -> SlotBrowseOutcome {
        if row == 0 {
            let changed =
                apply_best_equipment(&mut self.record.equip, candidates, &mut self.inventory);
            if changed > 0 {
                self.preview_stats = compute_battle_stats(
                    &self.record,
                    &self.equipment,
                    &self.active_status,
                    &self.modifiers,
                );
                SlotBrowseOutcome::BestEquipApplied(changed)
            } else {
                SlotBrowseOutcome::BestEquipNothing
            }
        } else {
            SlotBrowseOutcome::OpenCandidates {
                slot: (row - 1).min(7),
            }
        }
    }
}

/// Outcome of [`EquipSession::slot_browse_confirm`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotBrowseOutcome {
    /// Best Equipment applied `n` swaps (retail SFX `0x24` + stat
    /// re-aggregate).
    BestEquipApplied(u32),
    /// Nothing to change (retail buzz `0x23`).
    BestEquipNothing,
    /// A slot row confirmed - open the candidate list (retail sub-screen
    /// `0x14`).
    OpenCandidates { slot: u8 },
}

/// The "Best Equipment" applier over the four armament slots: for each
/// slot whose best-candidate id differs from the equipped id, take one
/// candidate from the bag (a candidate not in the bag is skipped - no
/// swap, no count), return the old item when non-zero, and write the
/// candidate into the slot. Returns the number of slots changed.
///
/// PORT: FUN_801cf760 (see `ghidra/scripts/funcs/overlay_menu_801cf760.txt`:
/// per-slot `beq candidate, equipped` skip, `FUN_80042EE0` bag-slot find
/// with the `0x100` not-found sentinel, `FUN_80043048(slot, 1)` take,
/// `FUN_800421D4(old, 1)` give-back, `sb` commit, changed-count return)
/// REF: FUN_801cf88c (the best-candidate computer filling `DAT_801EF0C0`;
/// the engine host derives `candidates` itself)
pub fn apply_best_equipment(
    equips: &mut [u8; 8],
    candidates: [u8; 4],
    inventory: &mut HashMap<u8, u8>,
) -> u32 {
    let mut changed = 0;
    for (slot, &candidate) in candidates.iter().enumerate() {
        let equipped = equips[slot];
        if candidate == equipped {
            continue;
        }
        // Bag must actually hold the candidate (retail's 0x100 sentinel
        // check); the Remove direction never happens here - a zero
        // candidate is never in the bag.
        let Some(qty) = inventory.get_mut(&candidate) else {
            continue;
        };
        if *qty == 0 {
            continue;
        }
        *qty -= 1;
        if equipped != 0 {
            *inventory.entry(equipped).or_insert(0) += 1;
        }
        equips[slot] = candidate;
        changed += 1;
    }
    changed
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
            base_spd: 35,
            base_int: 18,
            equip: [0; 8],
        }
    }

    fn fresh_session() -> EquipSession {
        let mut inv = HashMap::new();
        // Item 0x05 - slot (5 >> 5) = 0; helmet-class.
        inv.insert(0x05, 1);
        // Item 0x25 - slot 1 (0x25 >> 5 = 1)
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
    fn restrictions_gate_item_list_by_character_mask() {
        use crate::equipment::{DiscEquipEntry, DiscEquipInfo};
        use legaia_asset::equip_stats::EquipSlot as Disc;

        // Two weapons: one Vahn-only (mask 1), one anyone (mask 7).
        let mut inv = HashMap::new();
        inv.insert(0x20, 1); // Vahn sword
        inv.insert(0x30, 1); // universal weapon

        let info = DiscEquipInfo::from_entries([
            (
                0x20,
                DiscEquipEntry {
                    mask: 1,
                    category: Disc::Weapon,
                    is_ra_seru: false,
                },
            ),
            (
                0x30,
                DiscEquipEntry {
                    mask: 7,
                    category: Disc::Weapon,
                    is_ra_seru: false,
                },
            ),
        ]);

        // Vahn (slot 0) sees both weapons in the weapon slot (0).
        let vahn = EquipSession::new_with_restrictions(
            fresh_record(),
            inv.clone(),
            EquipmentTable::new(),
            StatusModifiers::default(),
            Vec::new(),
            info.clone(),
            0,
        );
        let v_weapons = vahn.items_for_slot(0);
        let mut v_ids: Vec<u8> = v_weapons.iter().map(|i| i.id).collect();
        v_ids.sort();
        assert_eq!(v_ids, vec![0x20, 0x30]);

        // Noa (slot 1) only sees the universal weapon (the Vahn-only one is
        // masked out).
        let noa = EquipSession::new_with_restrictions(
            fresh_record(),
            inv,
            EquipmentTable::new(),
            StatusModifiers::default(),
            Vec::new(),
            info,
            1,
        );
        let n_weapons = noa.items_for_slot(0);
        let n_ids: Vec<u8> = n_weapons.iter().map(|i| i.id).collect();
        assert_eq!(n_ids, vec![0x30]);
    }

    #[test]
    fn restrictions_gate_unambiguous_slot_by_disc_category() {
        use crate::equipment::{DiscEquipEntry, DiscEquipInfo};
        use legaia_asset::equip_stats::EquipSlot as Disc;

        let mut inv = HashMap::new();
        inv.insert(0x20, 1); // weapon
        inv.insert(0x40, 1); // body armor

        let info = DiscEquipInfo::from_entries([
            (
                0x20,
                DiscEquipEntry {
                    mask: 7,
                    category: Disc::Weapon,
                    is_ra_seru: false,
                },
            ),
            (
                0x40,
                DiscEquipEntry {
                    mask: 7,
                    category: Disc::Body,
                    is_ra_seru: false,
                },
            ),
        ]);
        let s = EquipSession::new_with_restrictions(
            fresh_record(),
            inv,
            EquipmentTable::new(),
            StatusModifiers::default(),
            Vec::new(),
            info,
            0,
        );
        // UI slot 0 = Weapon -> only the weapon.
        let weapons = s.items_for_slot(0);
        assert_eq!(weapons.iter().map(|i| i.id).collect::<Vec<_>>(), vec![0x20]);
        // UI slot 2 = BodyArmor -> only the armor.
        let body = s.items_for_slot(2);
        assert_eq!(body.iter().map(|i| i.id).collect::<Vec<_>>(), vec![0x40]);
        // UI slot 0 must not surface the body armor.
        assert!(!weapons.iter().any(|i| i.id == 0x40));
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
    fn preview_reflects_equipment_spd_and_int() {
        // A footwear/head item that carries SPD + INT bonuses (the equipment
        // table's +4 / +0 bytes). The preview must move SPD/INT, not acc/eva.
        let mut inv = HashMap::new();
        inv.insert(0x05, 1);
        let mut eq = EquipmentTable::new();
        eq.set(
            0x05,
            ItemModifier {
                spd: 6,
                int: 4,
                ..Default::default()
            },
        );
        let mut s = EquipSession::new(
            fresh_record(),
            inv,
            eq,
            StatusModifiers::default(),
            Vec::new(),
        );
        // Open slot 0's picker, then confirm item 0x05.
        s.input(EquipInput {
            cross: true,
            ..Default::default()
        });
        s.input(EquipInput {
            cross: true,
            ..Default::default()
        });
        // fresh_record base SPD=35, INT=18.
        assert_eq!(s.preview_stats.spd, 41);
        assert_eq!(s.preview_stats.int, 22);
        // Derived accuracy / evasion stay at the base (AGL-derived).
        assert_eq!(s.preview_stats.acc, 80);
        assert_eq!(s.preview_stats.eva, 20);
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

    #[test]
    fn best_equipment_swaps_only_differing_in_bag_candidates() {
        // FUN_801cf760 law: per slot, skip when candidate == equipped or
        // the bag lacks the candidate; otherwise take one, return the old
        // item, count the change.
        let mut equips = [0u8; 8];
        equips[0] = 0x05; // already the best - must not move
        equips[1] = 0x25; // will be replaced by 0x26
        let mut inv = HashMap::new();
        inv.insert(0x26, 1);
        // Slot 2's candidate 0x45 is NOT in the bag - skipped, no count.
        let changed = apply_best_equipment(&mut equips, [0x05, 0x26, 0x45, 0], &mut inv);
        assert_eq!(changed, 1);
        assert_eq!(equips[0], 0x05);
        assert_eq!(equips[1], 0x26);
        assert_eq!(equips[2], 0);
        assert_eq!(inv.get(&0x26), Some(&0));
        // The displaced 0x25 went back to the bag.
        assert_eq!(inv.get(&0x25), Some(&1));
    }

    #[test]
    fn slot_browse_row0_applies_best_and_other_rows_open_candidates() {
        let mut s = fresh_session();
        s.give_item(0x06, 1); // better slot-0 item in the bag
        match s.slot_browse_confirm(0, [0x06, 0, 0, 0]) {
            SlotBrowseOutcome::BestEquipApplied(1) => {}
            other => panic!("expected one swap, got {other:?}"),
        }
        assert_eq!(s.record().equip[0], 0x06);
        // Nothing left to improve: retail buzzes 0x23.
        assert_eq!(
            s.slot_browse_confirm(0, [0x06, 0, 0, 0]),
            SlotBrowseOutcome::BestEquipNothing
        );
        // A slot row opens the candidate list for row - 1.
        assert_eq!(
            s.slot_browse_confirm(3, [0, 0, 0, 0]),
            SlotBrowseOutcome::OpenCandidates { slot: 2 }
        );
    }

    #[test]
    fn unequip_returns_item_to_bag_and_refuses_empty_slot() {
        let mut s = fresh_session();
        // Equip 0x05 into slot 0 first (bag holds one).
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
        assert_eq!(s.record().equip[0], 0x05);
        assert_eq!(s.inventory().get(&0x05), Some(&0));

        // Remove row: item returns to the bag, slot clears.
        let mut s2 = s.clone();
        assert_eq!(s2.unequip(0), Some(0x05));
        assert_eq!(s2.record().equip[0], 0);
        assert_eq!(s2.inventory().get(&0x05), Some(&1));
        // Already-empty slot refuses (retail buzz 0x37, no commit).
        assert_eq!(s2.unequip(1), None);
    }

    #[test]
    fn preview_candidate_is_a_trial_that_leaves_the_record_alone() {
        let mut s = fresh_session();
        let base_atk = s.preview_stats.atk;
        s.preview_candidate(0, 0x05); // +5 atk modifier from fresh_session
        assert_eq!(s.preview_stats.atk, base_atk + 5);
        // The trial restored the record - nothing equipped.
        assert_eq!(s.record().equip[0], 0);
        // Remove-row preview (candidate 0) lands back on the bare stats.
        s.preview_candidate(0, 0);
        assert_eq!(s.preview_stats.atk, base_atk);
    }
}
