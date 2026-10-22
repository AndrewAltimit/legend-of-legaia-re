//! Committing an equip: destination slot, the retail commit kernel bridge, unequip and the candidate preview.

use super::*;

impl EquipSession {
    /// The equip byte the retail per-slot applier would write this pick
    /// into, expressed as an engine [`EquipSlot`] index.
    ///
    /// `FUN_801E5A08` does **not** trust the row the player confirmed from:
    /// for the four armament rows it re-derives the destination from the
    /// item's own equipment-record `+7` class (`0x801E5A58..0x801E5AE4`),
    /// and only the Goods rows (`slot_arg >= 4`) are written verbatim at
    /// `slot_arg + 1`. The engine's Hand Guard row has no retail
    /// counterpart, and without disc restrictions there is no `+7` byte to
    /// read; both cases keep the browse row.
    ///
    /// REF: FUN_801E5A08
    pub fn retail_destination_slot(&self, browse_slot: u8, item_id: u8) -> u8 {
        if browse_slot == ENGINE_ONLY_HAND_GUARD_SLOT {
            return browse_slot;
        }
        let Some(row) = retail_slot_row_for_engine_slot(browse_slot) else {
            return browse_slot;
        };
        if row >= 4 {
            // Goods: retail writes `row + 1`, which is this engine slot.
            return browse_slot;
        }
        let Some(info) = self.restrictions.as_ref() else {
            return browse_slot;
        };
        let Some(cat) = info.category(item_id) else {
            return browse_slot;
        };
        let byte = legaia_engine_dev_menu::dev_menu_leaves::resolve_equip_slot(
            slot_bits_of_category(cat),
            self.active_party_slot as usize,
            &RETAIL_WEAPON_EQUIP_BYTE,
        );
        RETAIL_EQUIP_BYTE_TO_ENGINE_SLOT
            .get(byte)
            .copied()
            .unwrap_or(browse_slot)
    }

    /// Commit the swap for `slot → item_id` through the retail per-slot
    /// applier.
    ///
    /// The destination is [`Self::retail_destination_slot`], not the browse
    /// row, and the bag / refund / confirm-cue half runs through the ported
    /// body [`legaia_engine_dev_menu::dev_equip_commit::commit_equip`] so the
    /// field-menu equip screen and the dev menu's `EQUIP` row share one
    /// kernel. A pick the bag cannot pay for leaves the record untouched
    /// and emits [`EquipEvent::InvalidConfirm`], which is retail's
    /// `return 0` arm.
    ///
    /// PORT: FUN_801E5A08
    pub(super) fn commit(&mut self, slot: u8, item_id: u8) {
        let weapon_byte = RETAIL_WEAPON_EQUIP_BYTE
            .get(self.active_party_slot as usize)
            .copied()
            .unwrap_or(2) as usize;
        let Some(row) = retail_slot_row_for_engine_slot(slot) else {
            // The engine's Hand Guard row has no retail equip byte, so the
            // applier has no destination to resolve. Commit it in engine
            // space with the applier's own order of effects.
            self.commit_engine_only(slot, item_id);
            return;
        };
        let bits = self
            .restrictions
            .as_ref()
            .and_then(|info| info.category(item_id))
            .map(slot_bits_of_category);
        let Some(bits) = bits else {
            // No disc `+7` byte for this id: retail's class routing has no
            // input, so the browse row stands.
            self.commit_engine_only(slot, item_id);
            return;
        };
        // Retail's applier addresses the record's own `+0x196` window, so
        // stage the eight bytes in retail order, run the ported body over
        // them, and fold the result back into the engine's slot order. The
        // Hand Guard byte has no retail home and is carried across untouched.
        let base = legaia_engine_dev_menu::dev_equip_commit::EQUIP_SLOT_BASE;
        let mut record = vec![0u8; base + 8];
        {
            let win = &mut record[base..];
            win[0] = self.record.equip[2];
            win[1] = self.record.equip[1];
            win[weapon_byte] = self.record.equip[0];
            win[4] = self.record.equip[4];
            win[5] = self.record.equip[5];
            win[6] = self.record.equip[6];
            win[7] = self.record.equip[7];
        }
        let mut host = SessionEquipHost {
            inventory: &mut self.inventory,
            sfx: None,
        };
        let committed = legaia_engine_dev_menu::dev_equip_commit::commit_equip(
            &mut host,
            &mut record,
            item_id,
            self.active_party_slot as usize,
            row,
            bits,
            &RETAIL_WEAPON_EQUIP_BYTE,
        );
        let sfx = host.sfx;
        let Some(committed) = committed else {
            // Retail's `return 0`: the bag scan missed, and the record, the
            // bag and the audio are all untouched.
            self.events.push(EquipEvent::InvalidConfirm);
            return;
        };
        {
            let win = &record[base..];
            self.record.equip[2] = win[0];
            self.record.equip[1] = win[1];
            self.record.equip[0] = win[weapon_byte];
            self.record.equip[4] = win[4];
            self.record.equip[5] = win[5];
            self.record.equip[6] = win[6];
            self.record.equip[7] = win[7];
        }
        let slot = RETAIL_EQUIP_BYTE_TO_ENGINE_SLOT
            .get(committed.slot)
            .copied()
            .unwrap_or(slot);
        let removed = committed.refunded.unwrap_or(0);
        self.finish_commit(slot, removed, item_id, sfx);
    }

    /// Commit a swap the retail applier cannot address - the engine's own
    /// Hand Guard row, and any pick whose `+7` class byte is not on the disc
    /// (a synthetic session). Same order of effects as
    /// [`legaia_engine_dev_menu::dev_equip_commit::commit_equip`]: take one from
    /// the bag, refund the prior occupant, write, cue.
    fn commit_engine_only(&mut self, slot: u8, item_id: u8) {
        let owned = self.inventory.get(&item_id).copied().unwrap_or(0);
        if owned == 0 {
            self.events.push(EquipEvent::InvalidConfirm);
            return;
        }
        *self.inventory.entry(item_id).or_insert(0) -= 1;
        let removed = self.record.equip[slot as usize];
        if removed != 0 {
            let _ = self.inventory.add(removed, 1);
        }
        self.record.equip[slot as usize] = item_id;
        self.finish_commit(
            slot,
            removed,
            item_id,
            Some(legaia_engine_dev_menu::dev_equip_commit::EQUIP_SFX_CUE),
        );
    }

    /// Shared tail of both commit paths: re-aggregate, park the session on
    /// `Done`, and emit the confirm events.
    fn finish_commit(&mut self, slot: u8, removed: u8, item_id: u8, sfx: Option<u8>) {
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
        if let Some(cue) = sfx {
            self.events.push(EquipEvent::ConfirmSfx { cue });
        }
    }

    /// Test-only helper: install an `ItemModifier` that targets a slot
    /// implied by the key encoding (`id >> 5 == slot`).
    pub fn register_item(&mut self, id: u8, modifier: ItemModifier) {
        self.equipment.set(id, modifier);
    }

    /// Direct mutation of the inventory map (for tests / synthetic flows).
    pub fn give_item(&mut self, id: u8, qty: u8) {
        let _ = self.inventory.add(id, qty);
    }

    /// Refresh [`Self::preview_stats`] for a hovered candidate by
    /// **trial-equipping** it on a copy of the record - the retail
    /// candidate-list handler's preview mechanism: save the record's equip
    /// array into the `DAT_801EF0C8` staging buffer, write the candidate
    /// (or `0` for the Remove row), re-run the stat aggregator
    /// `FUN_801CF650`, restore the array. The engine expresses the
    /// save/restore as a copy.
    ///
    /// Driven from [`Self::input`]'s `ItemPicker` arm: the candidate list
    /// stages a preview as it opens, refreshes it on every cursor move, and
    /// re-runs it on the confirm that raises the Yes/No prompt.
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
    ///
    /// Driven from [`Self::input`]'s `ItemPicker` arm: [`Self::items_for_slot`]
    /// puts the Remove row first on an occupied slot and a confirm on it
    /// lands here. `engine-ui::equip_screen_draws_for` labels that row.
    pub fn unequip(&mut self, slot: u8) -> Option<u8> {
        let removed = *self.record.equip.get(slot as usize)?;
        if removed == 0 {
            return None;
        }
        let _ = self.inventory.add(removed, 1);
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
}
