//! `EquipSession::input`: the per-frame pad step over the slot browse and item list.

use super::*;

impl EquipSession {
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
                } else if input.down && cursor + 1 < SLOT_BROWSE_ROWS {
                    cursor += 1;
                    self.state = EquipState::SlotPicker { cursor };
                    self.events.push(EquipEvent::CursorMoved {
                        state: EquipStateKind::SlotPicker,
                        cursor: cursor as u16,
                    });
                } else if input.cross {
                    // Retail's `FUN_801D99F0` recomputes the candidate array
                    // (`FUN_801CF88C`) *before* dispatching the row, so row 0
                    // applies a pick made from the state the confirm saw.
                    let candidates = self.best_equipment_now();
                    match self.slot_browse_confirm(cursor, candidates) {
                        SlotBrowseOutcome::BestEquipApplied(changed) => {
                            self.events
                                .push(EquipEvent::BestEquipmentApplied { changed });
                        }
                        SlotBrowseOutcome::BestEquipNothing => {
                            self.events.push(EquipEvent::BestEquipmentUnchanged);
                        }
                        SlotBrowseOutcome::OpenCandidates { slot } => {
                            self.state = EquipState::ItemPicker { slot, cursor: 0 };
                            self.events.push(EquipEvent::EnteredItemPicker { slot });
                            // Retail stages the trial equip as the candidate
                            // list opens, so the stat-compare block is already
                            // showing the top row's preview on the first frame.
                            if let Some(item) = self.items_for_slot(slot).first().copied() {
                                self.preview_candidate(slot, item.id);
                            }
                        }
                    }
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
                    // Retail's candidate list previews the **hovered** row, not
                    // just the confirmed one - the trial equip re-runs on every
                    // cursor move so the stat-compare block tracks the hand.
                    if let Some(item) = items.get(cursor as usize) {
                        self.preview_candidate(slot, item.id);
                    }
                } else if input.down && (cursor as usize + 1) < n {
                    cursor += 1;
                    self.state = EquipState::ItemPicker { slot, cursor };
                    self.events.push(EquipEvent::CursorMoved {
                        state: EquipStateKind::ItemPicker,
                        cursor,
                    });
                    if let Some(item) = items.get(cursor as usize) {
                        self.preview_candidate(slot, item.id);
                    }
                } else if input.cross && (cursor as usize) < n {
                    let item = items[cursor as usize];
                    if item.id == REMOVE_ROW_ID {
                        // The Remove row commits straight away - retail has
                        // no Yes/No prompt on it (`0x801DA0B4` writes the
                        // record byte in the same frame as the confirm).
                        if self.unequip(slot).is_none() {
                            self.events.push(EquipEvent::InvalidConfirm);
                        }
                        return;
                    }
                    if item.equipped {
                        // The Goods list's equipped row: retail commits
                        // nothing and runs the shared tail, which steps the
                        // hand to the next slot row (wrapping at 8) and
                        // returns to the slot picker (`0x801DA1DC..0x801DA254`).
                        let next = browse_row_for_slot(slot) + 1;
                        self.state = EquipState::SlotPicker {
                            cursor: if next < 8 { next } else { 0 },
                        };
                        return;
                    }
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
                    // Refresh the preview through the ported trial-equip
                    // (`FUN_801d9c14`) rather than re-deriving it here.
                    self.preview_candidate(slot, item.id);
                } else if input.circle {
                    // Retail's cancel returns to sub-screen 0x13 with the hand
                    // back on the slot's own row ([`browse_row_for_slot`]).
                    self.state = EquipState::SlotPicker {
                        cursor: browse_row_for_slot(slot),
                    };
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
}
