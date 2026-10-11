//! Field-host bodies that edit the party and the bag: scripted item take, party
//! add and party-slot restore.

use super::*;

impl FieldHostImpl<'_> {
    // PORT: FUN_800430AC
    // REF: FUN_80042310
    // REF: FUN_80042EE0
    pub(super) fn take_item(&mut self, item_id: u8) {
        // Op 0x4C n5 sub-2 TAKE_ITEM `[4C, 52, item_id]`, the give-side mirror
        // of `give_item` above. Retail's arm at `0x801E1ABC` consumes one of
        // the id from the bag (`FUN_80042310(item_id, 1)`) and, **only when
        // the bag does not hold it**, falls back to taking it off whoever is
        // wearing it (`FUN_800430AC`). The fallback is why a script can
        // confiscate an equipped accessory at all.
        //
        // The ordering is the half that is easy to invert: `0x100` is the
        // consume primitive's *not-found* sentinel, so the `== 0x100` branch
        // is the fallback and not a success path. A bag hit must therefore
        // leave equipment alone - `take_item_prefers_the_bag_over_the_worn_copy`
        // pins that direction, because an implementation that unequips first
        // (or always) passes every other assertion here.
        //
        // `item_id == 0` is a bag miss by construction:
        // `RetailInventory::find_slot` (retail `FUN_80042EE0`) treats 0 as the
        // empty-slot sentinel and never matches it, so a zero operand goes
        // straight to the fallback, which then clears the first *empty*
        // accessory slot - a no-op the retail kernel reports as success.
        //
        // No `FieldEvent` is emitted. The give side has one because a host
        // renders an acquisition banner off it; nothing consumes a take yet,
        // and the observable this wire exists for is the world state - the bag
        // count and the accessory slot - which is what the tests read.
        let held = if item_id == 0 {
            None
        } else {
            self.world.party.inventory.get(&item_id).copied()
        };
        match held {
            Some(count) if count > 0 => {
                let left = count - 1;
                if left == 0 {
                    self.world.party.inventory.remove(&item_id);
                } else {
                    self.world.party.inventory.insert(item_id, left);
                }
            }
            _ => {
                legaia_engine_menus::equipment::party_unequip_accessory_by_id(
                    &mut self.world.party.roster,
                    item_id,
                );
            }
        }
    }

    pub(super) fn party_add(&mut self, char_id: u8) -> bool {
        // Retail (`FUN_801DE840` op `0x3C`): a linear search of the
        // present-party list `0x80084598` for `count = DAT_80084594`
        // entries; when absent and `count < 4`, append, `count += 1`, and
        // bubble the new id down while its left neighbour is larger (the
        // list stays sorted by character id). A count of 1 afterwards makes
        // the new member the leader (`0x80084597` / `_DAT_8007B8F8`).
        //
        // That list is the one battle setup reads, so the engine's battle
        // composition (`party_count` / `active_party`) follows it here.
        // Without that, a mid-scene join - Noa's "Let me help you!" beat on
        // Mt. Rikuroa, one op before the Caruban fight - reached the battle
        // with the pre-join count and Vahn fought alone.
        let mut list = self.world.present_party_list();
        let accepted = if list.contains(&char_id) || list.len() >= 4 {
            false
        } else {
            list.push(char_id);
            let mut i = list.len() - 1;
            while i > 0 && list[i - 1] > list[i] {
                list.swap(i - 1, i);
                i -= 1;
            }
            true
        };
        if accepted {
            if list.len() == 1 {
                self.world.party.party_leader_slot = list.first().copied();
            }
            self.world.install_present_party_list(list);
        }
        self.world
            .pending_field_events
            .push(FieldEvent::PartyAdd { char_id, accepted });
        accepted
    }

    /// Op `0x4C 0x82 <slot>` - full HP/MP restore of one party slot.
    ///
    /// Retail's inn / rest heal. There is no inn opcode and no native inn
    /// routine: the scene script composes a stay out of dialogue, a
    /// 2-option picker, an op-0x4E gold gate, an op-0x3A `ADD_MONEY` with
    /// the negative charge and the fades, then emits one of these per
    /// party slot. The charge and the restore are decoupled, which is why
    /// a free rest (a bed, an infirmary) is the same tail with the gate
    /// and debit dropped - and why `legaia_scene_formats::inn_costs` finds the
    /// prices scripted per scene rather than in a table.
    ///
    /// Retail addresses the slot by *literal operand*, so a script that
    /// heals slots 0/1/2 heals exactly those records - it does not walk
    /// "every active member". The port keeps that: an operand past the end
    /// of the roster is a no-op, matching a write into a record the game
    /// never populated.
    pub(super) fn restore_party_slot(&mut self, slot: u8) {
        let Some(member) = self.world.party.roster.members.get_mut(slot as usize) else {
            return;
        };
        let mut hms = member.hp_mp_sp();
        hms.hp_cur = hms.hp_max;
        hms.mp_cur = hms.mp_max;
        member.set_hp_mp_sp(hms);
        // The record is retail's only copy; the port's party actor mirrors
        // it, and a battle seats (and `save_party` writes back) from the
        // mirror - so a rest that left it stale healed nobody.
        self.world.mirror_roster_hp_mp(usize::from(slot));
    }
}
