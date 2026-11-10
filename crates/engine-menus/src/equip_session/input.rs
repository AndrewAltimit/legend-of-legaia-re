//! `EquipSession::input`: the per-frame pad step over the slot browse and item list.

use super::*;
use legaia_engine_system::input::PadButton;
use legaia_engine_system::menu_input::{CursorNav, NavButtons, menu_cursor_nav};

/// The row the candidate confirm's tail leaves the hand on: the next row below
/// `slot`'s own, wrapping at 8 (`0x801DA1E8..0x801DA1F8`).
fn next_browse_row(slot: u8) -> u8 {
    let next = browse_row_for_slot(slot) + 1;
    if next < SLOT_BROWSE_ROWS { next } else { 0 }
}

impl EquipSession {
    /// Apply a per-frame input.
    pub fn input(&mut self, input: EquipInput) {
        match self.state {
            EquipState::SlotPicker { cursor } => {
                // Retail's slot browse is the list kernel `FUN_801D688C`
                // over the eight rows with wrap (`0x801D9B00`): confirm and
                // cancel are read before any move, Up / Down wrap.
                let mut cell = u32::from(cursor);
                let nav = menu_cursor_nav(
                    &mut cell,
                    u32::from(SLOT_BROWSE_ROWS),
                    true,
                    NavButtons::new(input.cross, input.circle, input.up, input.down),
                );
                let cursor = cell as u8;
                if nav == CursorNav::Moved {
                    self.state = EquipState::SlotPicker { cursor };
                    self.events.push(EquipEvent::CursorMoved {
                        state: EquipStateKind::SlotPicker,
                        cursor: cursor as u16,
                    });
                } else if nav == CursorNav::Confirm {
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
                            // The candidate step opens with the list's
                            // selection on row 1, the row below Remove
                            // (`sw s3,-0x4468` at `0x801D9D20`); the allocator
                            // `FUN_80030104` clamps it to the last row, so a
                            // list holding only Remove opens on Remove.
                            let items = self.items_for_slot(slot);
                            let cursor = 1.min(items.len().saturating_sub(1)) as u16;
                            self.state = EquipState::ItemPicker { slot, cursor };
                            self.events.push(EquipEvent::EnteredItemPicker { slot });
                            // Retail stages the trial equip as the candidate
                            // list opens, so the stat-compare block is already
                            // showing the hovered row's preview on the first frame.
                            if let Some(item) = items.get(usize::from(cursor)).copied() {
                                self.preview_candidate(slot, item.id);
                            }
                        }
                    }
                } else if nav == CursorNav::Cancel {
                    self.state = EquipState::Done;
                    self.events.push(EquipEvent::Cancelled);
                }
            }
            EquipState::ItemPicker { slot, mut cursor } => {
                let items = self.items_for_slot(slot);
                let n = items.len();
                // The candidate list runs the kind-4 kernel: Up / Down wrap
                // inside the 12-row page and Left / Right flip it.
                let mut pressed = 0u16;
                for (held, button) in [
                    (input.up, PadButton::Up),
                    (input.down, PadButton::Down),
                    (input.left, PadButton::Left),
                    (input.right, PadButton::Right),
                ] {
                    if held {
                        pressed |= button.mask();
                    }
                }
                let moved =
                    crate::pause_screens::list_kernel_navigate(usize::from(cursor), n, pressed)
                        as u16;
                if moved != cursor {
                    cursor = moved;
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
                } else if input.cross && (cursor as usize) < n {
                    // Retail has no prompt here. The confirm arm
                    // (`0x801DA0B4..0x801DA1D0`) writes the record on the frame
                    // of the press and every path through it, a refused one
                    // too, runs the shared tail (`0x801DA1DC..0x801DA254`):
                    // the hand steps to the next slot row, wrapping at 8, and
                    // the pad goes back to the slot browse. An equipped row
                    // tests no class the arm knows and goes straight there.
                    let item = items[cursor as usize];
                    if item.id == REMOVE_ROW_ID {
                        if self.unequip(slot).is_none() {
                            self.events.push(EquipEvent::InvalidConfirm);
                        }
                    } else if !item.equipped {
                        if item.owned {
                            self.commit(slot, item.id);
                        } else {
                            self.events.push(EquipEvent::InvalidConfirm);
                        }
                    }
                    self.state = EquipState::SlotPicker {
                        cursor: next_browse_row(slot),
                    };
                } else if input.circle {
                    // Retail's cancel returns to sub-screen 0x13 with the hand
                    // back on the slot's own row ([`browse_row_for_slot`]).
                    self.state = EquipState::SlotPicker {
                        cursor: browse_row_for_slot(slot),
                    };
                }
            }
            EquipState::Done => {}
        }
    }
}
