//! `FieldPartyInventoryHost` for [`FieldHostImpl`].

use super::*;

impl<'a> FieldPartyInventoryHost for FieldHostImpl<'a> {
    fn give_item(&mut self, item_id: u8) {
        // Op 0x39 GIVE_ITEM: add one of `item_id` to the inventory, capacity-
        // checked like the retail add-by-id primitive FUN_800421D4(item_id, 1).
        let slot = self.world.party.inventory.entry(item_id).or_insert(0);
        *slot = slot.saturating_add(1).min(legaia_save::STACK_CAP);
        self.world
            .pending_field_events
            .push(FieldEvent::GiveItem { item_id });
    }

    fn take_item(&mut self, item_id: u8) {
        Self::take_item(self, item_id)
    }

    fn add_money(&mut self, delta: i32) {
        let new_total = (self.world.party.money as i64 + delta as i64).clamp(0, 9_999_999) as i32;
        self.world.party.money = new_total;
        self.world
            .pending_field_events
            .push(FieldEvent::AddMoney { delta });
    }

    /// Op `0x4E` party-bank read - the read side of the two purses `add_money`
    /// and the casino cash-out write. Sub-op 10 is party gold
    /// (`_DAT_8008459C`), sub-op 11 the casino coin bank (`_DAT_800845A4`);
    /// the VM normalises the 7-byte (sub-3 / sub-9) and 9-byte (sub-10 /
    /// sub-11) encoded forms onto that pair before calling.
    ///
    /// Without it every scripted gold gate reads an empty purse, so each of
    /// them takes its can't-afford branch: an inn stay, a paid tour, a train
    /// ticket and a casino coin purchase are all one `0x4E` sub-3 compare
    /// against a script literal (`legaia_scene_formats::inn_costs`). That is the
    /// whole affordability check - retail has no inn routine and no cost
    /// table, so this read is what makes the charge reachable at all.
    fn party_bank_value(&self, sub_op: u8) -> i32 {
        match sub_op {
            11 => self.world.minigames.casino_coins.min(i32::MAX as u32) as i32,
            _ => self.world.party.money,
        }
    }

    fn set_item_count(&mut self, slot_byte: u8, count: u8) {
        if count == 0 {
            self.world.party.inventory.remove(&slot_byte);
        } else {
            self.world.party.inventory.insert(slot_byte, count);
        }
        self.world
            .pending_field_events
            .push(FieldEvent::SetItemCount { slot_byte, count });
    }

    fn party_add(&mut self, char_id: u8) -> bool {
        Self::party_add(self, char_id)
    }

    fn party_remove(&mut self, char_id: u8) {
        // Retail op `0x3D`: find the id, shift the tail down, `count -= 1`;
        // the leader re-points at the new head when the count drops to 1
        // or the removed id was the leader.
        let mut list = self.world.present_party_list();
        if let Some(pos) = list.iter().position(|&id| id == char_id) {
            list.remove(pos);
            let leader_gone =
                matches!(self.world.party.party_leader_slot, Some(id) if id == char_id);
            if list.len() == 1 || leader_gone {
                self.world.party.party_leader_slot = list.first().copied();
            }
            self.world.install_present_party_list(list);
        }
        self.world
            .pending_field_events
            .push(FieldEvent::PartyRemove { char_id });
    }

    fn set_party_leader(&mut self, leader_id: u8) {
        self.world.party.party_leader_slot = Some(leader_id);
        self.world
            .pending_field_events
            .push(FieldEvent::SetPartyLeader { leader_id });
    }

    fn rebuild_party_roster(&mut self) {
        self.world
            .pending_field_events
            .push(FieldEvent::MenuRefresh);
    }

    /// Op `0x4C 0xE5` - the casino coin bank's script delta (a cabinet's
    /// fee, the dome's entry fee, a prize price). The arm and its missing
    /// lower clamp are documented on [`crate::casino_coin_bank`].
    fn add_coins(&mut self, coin_delta: i32) {
        self.world.add_script_coins(coin_delta);
    }

    fn restore_party_slot(&mut self, slot: u8) {
        Self::restore_party_slot(self, slot)
    }
}
