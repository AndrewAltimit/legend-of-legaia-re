//! `MenuRuntimeHost`: the `MenuHost` the menu VM steps against, and its commit arms.

use super::*;

impl MenuRuntimeHost<'_> {
    /// Row actions for the current `ShopMenu` (Trade present iff trading on).
    fn shop_menu_rows(&self) -> &'static [MenuState] {
        shop_menu_rows(self.world.seru_trade_enabled())
    }

    /// The retail buy-list confirm dispatch for the row at `slot`
    /// ([`legaia_engine_menus::shop::buy_list_confirm_route`], `FUN_801DB21C` state 2):
    /// affordability against the purse first, then the item record's `+0`
    /// kind byte picks the follow-up screen. The kind comes from the
    /// on-disc item-effect tables ([`legaia_engine_core::world::DiscTables::item_effects`]); a
    /// PROT.DAT-only load has no kind byte and falls back to the stackable
    /// arm (the quantity flow), which is also where an equipment row lands
    /// while [`MenuRuntime::retail_equipment_buy`] is off.
    fn shop_buy_route(&self, slot: u8) -> Option<BuyListRoute> {
        let session = self.shop_session.as_ref()?;
        let item = session.inventory.items.get(slot as usize)?;
        // The item-name table's `+0` kind byte, via the item-effect parse.
        // Without it, the disc-built `DiscEquipInfo` indexes exactly the
        // kind-1 ids, so it answers the equipment test; a build with
        // neither table falls back to the stackable arm.
        let kind = match self.world.tables.item_effects.as_ref() {
            Some(t) => t.kind(item.item_id),
            None => match self.equip_info.as_ref() {
                Some(info) if info.is_equipment(item.item_id) => 1,
                _ => 2,
            },
        };
        let price = u16::try_from(item.price).unwrap_or(u16::MAX);
        // The list kernel refuses a disabled row before the sub-screen's
        // state-2 dispatch ever sees it: `FUN_80032A44` tests the row word's
        // `0x800` bit on confirm (`80032d04`) and buzzes (`0x23`) without
        // raising mode 2. The buy-row builder sets that bit for a purse short
        // of the price *or* a stack already at 99 - the first half is the
        // dispatch's own affordability test below, the second only the kernel
        // runs. Retail capture (Retock Items Shop, a 99-stack row): the hand
        // stays on the list.
        let held = self
            .world
            .party
            .inventory
            .get(&item.item_id)
            .copied()
            .unwrap_or(0);
        if held >= legaia_engine_menus::shop::SHOP_HELD_CAP {
            return Some(BuyListRoute::Refused);
        }
        Some(legaia_engine_menus::shop::buy_list_confirm_route(
            kind,
            self.world.party.money,
            price,
        ))
    }

    /// How many quantities the `ShopQuantity` screen offers for the staged
    /// item (cursor `n` = quantity `n + 1`).
    ///
    /// The bound is retail's own - `min(gold / price, 99, 99 - held)` buying,
    /// the staged bag count selling - through
    /// [`legaia_engine_menus::shop::quantity_row_count`]. The screen used to offer a flat
    /// nine rows, which capped a purchase at nine copies however much gold or
    /// stack room the player had.
    fn shop_quantity_row_count(&self) -> u8 {
        let Some(session) = self.shop_session.as_ref() else {
            return 1;
        };
        let held = session
            .pending_item_id
            .and_then(|id| self.world.party.inventory.get(&id).copied());
        session.quantity_rows(self.world.party.money, held)
    }

    /// The recipient picker needs both the opt-in and the disc mask table.
    fn recipient_enabled(&self) -> bool {
        self.retail_equipment_buy && self.equip_info.is_some()
    }

    /// Open the buy-recipient picker for the confirmed buy-list row (retail
    /// sub-screen `0x1C`): `party_count + 1` rows, per-member equippability
    /// off the equip record's character mask. The Point Card gate is the
    /// live bag test ([`World::point_card_held`]), so a party carrying the
    /// card gets the accrual and the toast beat the retail commit runs.
    fn open_recipient_picker(&mut self, slot: u8) {
        let Some(session) = self.shop_session.as_ref() else {
            return;
        };
        let Some(item) = session.inventory.items.get(slot as usize) else {
            return;
        };
        let Some(info) = self.equip_info.as_ref() else {
            return;
        };
        // One row per **present** member, in party-list order: retail hands
        // the cursor kernel `DAT_80084594 + 1` rows (`lbu a1,0x454(s1)` /
        // `addiu a1,a1,1` at `0x801DB434`) and resolves a confirmed row to
        // its character through the party list (`lbu s0,0x457(v0)` at
        // `0x801DB554`, `0x80084597 + row`), whose id indexes the character
        // mask table `0x801E43F0`. A lone Noa is row 1, and a member who
        // has not joined has no row.
        // REF: FUN_801DB380
        let can_equip: Vec<bool> = self
            .world
            .present_party_list()
            .iter()
            .take(3)
            .map(|&ch| info.can_equip(item.item_id, ch))
            .collect();
        let price = u16::try_from(item.price).unwrap_or(u16::MAX);
        let point_card_held = self.world.point_card_held();
        *self.recipient_session = Some(BuyRecipientSession::new(
            item.item_id,
            price,
            can_equip,
            point_card_held,
        ));
    }

    /// `StatusEquipment` commit: unequip the picked slot, credit the item back
    /// to the bag, and rebuild the party ability bitfields.
    fn commit_status_equipment(&mut self, slot: u8) {
        let idx = *self.selected_char;
        let mut removed = 0u8;
        if let Some(record) = self.world.party.roster.members.get_mut(idx) {
            let mut equip = record.equipment();
            if (slot as usize) < equip.slots.len() {
                removed = equip.slots[slot as usize];
                equip.slots[slot as usize] = 0;
                record.set_equipment(equip);
            }
        }
        // Return the unequipped item to the bag (retail puts it back);
        // zeroing the slot without crediting it destroyed the item.
        if removed != 0 {
            let _ = self.world.party.inventory.add(removed, 1);
        }
        // Unequipping can remove an accessory passive; rebuild the
        // ability bitfields so the bit (and any party-wide grant)
        // disappears immediately.
        self.world.refresh_party_ability_bits();
    }

    /// `StatusInventory` commit: decrement (or remove) the picked bag item.
    fn commit_status_inventory(&mut self, slot: u8) {
        // The drawn rows are the bag's occupied slots in slot order, and the
        // row payload is the slot - so the unit comes off the stack the player
        // pointed at, not off whichever slot a scan finds the id in.
        let rows: Vec<u8> = {
            let (start, end) = self.world.party.inventory.window_bounds();
            let slots = self.world.party.inventory.slots();
            slots[start.min(slots.len())..end.min(slots.len())]
                .iter()
                .enumerate()
                .filter(|(_, (id, count))| *id != 0 && *count > 0)
                .map(|(i, _)| (start + i) as u8)
                .collect()
        };
        if let Some(&bag_slot) = rows.get(slot as usize) {
            self.world.party.inventory.consume_slot(bag_slot, 1);
        }
    }

    /// Run the Point Card accrual for one purchase and arm the window-31
    /// toast when it landed. A closed gate (no Point Card in the bag) is
    /// retail's short-circuit: no credit, no window, no extra press.
    fn credit_point_card(&mut self, price: u16, qty: i32) {
        if let Some(credit) = self.world.credit_point_card(price, qty) {
            *self.point_card_toast = Some(credit);
        }
    }

    /// `ShopSell` commit: select the picked bag item for sale against the
    /// id-sorted inventory snapshot.
    fn commit_shop_sell(&mut self, slot: u8) {
        // The drawn rows, from the one seat the draw also reads
        // ([`MenuRuntime::sell_list_rows`]), so the hand's row and the
        // committed row are the same row.
        let rows: Vec<(u8, u8)> = MenuRuntime::sell_list_rows(self.world)
            .iter()
            .map(|r| (r.slot, r.id))
            .collect();
        if let Some(session) = self.shop_session.as_mut() {
            session.select_sell_row(slot as usize, &rows);
        }
    }

    /// `ShopConfirm` (Yes) commit: run the buy grant kernel or apply a sell
    /// against the live inventory.
    ///
    /// A buy that lands also runs the **Point Card accrual** - retail's
    /// `FUN_801DB7F4` case 2, which credits `(price / 20) * qty` into
    /// `_DAT_800845B4` while the party holds item `0xFE`, then opens window
    /// `0x1F` and stalls for a press (case 4). This engine screen is the
    /// port of that sub-screen, so the accrual belongs here rather than in
    /// [`World::buy_from_shop`]: the grant kernel is also the randomizer
    /// oracles' entry point, and retail's kernel-equivalent (`FUN_800421D4`
    /// + the purse store, case 3) carries no accrual either.
    fn commit_shop_confirm(&mut self) {
        if let Some(session) = self.shop_session.as_ref() {
            if session.pending_is_buying {
                let unit_price = session
                    .pending_item_id
                    .and_then(|id| session.inventory.find(id))
                    .map(|i| u16::try_from(i.price).unwrap_or(u16::MAX));
                // Shared grant kernel (also driven by the shop / casino
                // randomizer runtime oracles).
                let bought = self.world.buy_from_shop(session);
                if let (Some((_, qty, _)), Some(price)) = (bought, unit_price) {
                    self.credit_point_card(price, i32::from(qty));
                }
            } else if let Some(item_id) = session.pending_item_id {
                let held = self
                    .world
                    .party
                    .inventory
                    .get(&item_id)
                    .copied()
                    .unwrap_or(0);
                if let Some((item_id, qty, delta)) = session.try_sell(held) {
                    self.world.party.money = (self.world.party.money + delta).clamp(0, 9_999_999);
                    // Take the units off the slot the row named where the host
                    // staged one; the id path is the fallback for a row list
                    // built without slots.
                    match session.pending_bag_slot {
                        Some(bag_slot) => {
                            self.world.party.inventory.consume_slot(bag_slot, qty);
                        }
                        None => {
                            let entry = self.world.party.inventory.entry(item_id).or_insert(0);
                            *entry = entry.saturating_sub(qty);
                            let left = *entry;
                            if left == 0 {
                                self.world.party.inventory.remove(&item_id);
                            }
                        }
                    }
                }
            }
        }
    }

    /// `ShopTradeConfirm` (Yes) commit: apply the stashed seru-trade offer to
    /// the owner's spell list, then refresh the offer list.
    fn commit_shop_trade_confirm(&mut self) {
        let offer = self
            .trade_session
            .as_ref()
            .and_then(|t| t.offers.get(*self.trade_pending_offer).copied());
        if let Some(offer) = offer {
            self.world.apply_seru_trade(&offer);
            let pt = self.world.clock.play_time_seconds;
            if let Some(t) = self.trade_session.as_mut() {
                t.refresh(pt, &self.world.party.roster.members);
            }
        }
    }

    /// `InnConfirm` commit: on Yes (slot 0) charge the fee and restore the
    /// active party's HP/MP; clear the session regardless.
    fn commit_inn_confirm(&mut self, slot: u8) {
        if slot == 0 {
            // slot 0 = yes; slot 1 = no
            let can = self
                .inn_session
                .as_ref()
                .is_some_and(|s| s.can_afford(self.world.party.money));
            if can {
                let cost = self.inn_session.as_ref().unwrap().cost as i32;
                self.world.party.money -= cost;
                // Restore HP/MP for every present-party member: on the
                // record, which is the pool outside a fight, then projected
                // onto the member's actor mirror as every field heal does.
                let party_count = self.world.party.party_count as usize;
                for member in 0..party_count {
                    let rslot = self.world.party_roster_slot(member);
                    if let Some(rec) = self.world.party.roster.members.get_mut(rslot) {
                        let mut hms = rec.hp_mp_sp();
                        hms.hp_cur = hms.hp_max;
                        hms.mp_cur = hms.mp_max;
                        rec.set_hp_mp_sp(hms);
                    }
                    self.world.mirror_roster_hp_mp(rslot);
                }
            }
        }
        // Clear session regardless of yes/no.
        *self.inn_session = None;
    }
}

impl<'a> MenuHost for MenuRuntimeHost<'a> {
    /// The shop teardown holds for the windows' slide out
    /// ([`legaia_engine_menus::shop::SHOP_SLIDE_FRAMES`]): retail's exit
    /// sub-screen waits on the widget VM going idle (`_DAT_8007BB80 == 0`,
    /// `0x801DD17C`) before it starts the fade.
    fn transient_hold_frames(&self, state: MenuState) -> u16 {
        match state {
            MenuState::ShopExit => u16::from(legaia_engine_menus::shop::SHOP_SLIDE_FRAMES),
            _ => 8,
        }
    }

    fn screen_item_count(&self, state: MenuState) -> u8 {
        match state {
            MenuState::StatusTop => 8, // Character / Equip / Items / Magic / Arts / Config / Save / Load
            MenuState::StatusCharacter => {
                self.world.party.roster.members.len().min(u8::MAX as usize) as u8
            }
            MenuState::StatusEquipment => 8,
            MenuState::SavePickSlot | MenuState::LoadSlot => self.slot_count.max(1),
            MenuState::ShopBuy => self
                .shop_session
                .as_ref()
                .map(|s| s.buy_item_count().max(1))
                .unwrap_or(8),
            MenuState::ShopSell => self
                .world
                .party
                .inventory
                .values()
                .filter(|c| **c > 0)
                .count()
                .min(u8::MAX as usize) as u8,
            MenuState::ShopQuantity => self.shop_quantity_row_count(),
            MenuState::ShopConfirm | MenuState::InnConfirm => 2, // slot 0 = yes, 1 = no/cancel
            MenuState::ShopMenu => self.shop_menu_rows().len() as u8,
            MenuState::ShopTrade => self
                .trade_session
                .as_ref()
                .map(|t| t.offers.len().max(1).min(u8::MAX as usize) as u8)
                .unwrap_or(1),
            MenuState::ShopTradeConfirm => 2, // slot 0 = yes, 1 = no
            MenuState::StatusInventory => self
                .world
                .party
                .inventory
                .values()
                .filter(|c| **c > 0)
                .count()
                .min(16) as u8,
            MenuState::StatusMagic | MenuState::StatusTacticalArts => 8,
            _ => 1,
        }
    }

    fn commit_route_override(&self, state: MenuState, slot: u8) -> Option<MenuState> {
        // The shop top picker's row layout is dynamic (Trade only when enabled),
        // so resolve the committed slot against the live row list here.
        match state {
            MenuState::ShopMenu => self.shop_menu_rows().get(slot as usize).copied(),
            // The buy list's confirm dispatch (`FUN_801DB21C` state 2):
            // gold short stays on the list (retail buzzes and re-arms list
            // mode 1); an equipment row parks the list under the recipient
            // picker; a kind outside 1/2 falls back to the Buy/Sell/Quit
            // mode select. The stackable arm keeps the default
            // `ShopQuantity` route.
            MenuState::ShopBuy => match self.shop_buy_route(slot) {
                Some(BuyListRoute::Refused) => Some(MenuState::ShopBuy),
                Some(BuyListRoute::RecipientPicker) if self.recipient_enabled() => {
                    Some(MenuState::ShopBuy)
                }
                Some(BuyListRoute::ModeSelect) => Some(MenuState::ShopMenu),
                _ => None,
            },
            _ => None,
        }
    }

    fn commit(&mut self, state: MenuState, slot: u8) {
        match state {
            MenuState::SavePickSlot => {
                *self.pending = Some(PendingOp::Save { slot });
            }
            MenuState::LoadSlot => {
                *self.pending = Some(PendingOp::Load { slot });
            }
            MenuState::StatusCharacter => {
                *self.selected_char = slot as usize;
            }
            MenuState::StatusEquipment => self.commit_status_equipment(slot),
            MenuState::StatusInventory => self.commit_status_inventory(slot),
            // --- Shop states ---
            // Top picker: picking Trade opens this vendor's seru-trade session
            // (keyed to the shop's vendor id) for the current play-time window.
            // Buy / Sell / Exit are pure routes (handled by the route override).
            MenuState::ShopMenu => {
                if self.shop_menu_rows().get(slot as usize) == Some(&MenuState::ShopTrade) {
                    let vendor = self
                        .shop_session
                        .as_ref()
                        .map(|s| s.vendor_bucket_offset)
                        .unwrap_or(0);
                    *self.trade_session = self.world.open_seru_trade(vendor);
                }
            }
            MenuState::ShopBuy => match self.shop_buy_route(slot) {
                // Gold short: the retail refusal beat (buzz SFX `0x23`,
                // list re-armed) - no pending item, hand stays on the row.
                Some(BuyListRoute::Refused) => {
                    *self.refused = true;
                    *self.stay_cursor = Some(slot);
                }
                // Equipment: the recipient picker takes the pad; the buy
                // list parks behind it with the hand on the row.
                Some(BuyListRoute::RecipientPicker) if self.recipient_enabled() => {
                    self.open_recipient_picker(slot);
                    *self.stay_cursor = Some(slot);
                }
                _ => {
                    if let Some(session) = self.shop_session.as_mut() {
                        session.select_buy_item(slot as usize);
                    }
                }
            },
            MenuState::ShopSell => self.commit_shop_sell(slot),
            MenuState::ShopQuantity => {
                if let Some(session) = self.shop_session.as_mut() {
                    session.set_quantity(slot);
                }
            }
            // slot 0 = confirm; slot 1 = cancel (falls through to _ => {})
            MenuState::ShopConfirm if slot == 0 => self.commit_shop_confirm(),
            // Seru trade: pick an offer (stash its index for the confirm).
            MenuState::ShopTrade => {
                let has_offer = self
                    .trade_session
                    .as_ref()
                    .is_some_and(|t| (slot as usize) < t.offers.len());
                if has_offer {
                    *self.trade_pending_offer = slot as usize;
                }
            }
            // Seru trade confirm: slot 0 = Yes applies the stashed offer to the
            // owner's spell list, then refreshes the offer list (which may shrink
            // and reseeds when the play-time bucket has advanced); slot 1 = No.
            MenuState::ShopTradeConfirm if slot == 0 => self.commit_shop_trade_confirm(),
            // Transient teardown screen reached by routing out of the shop
            // menu (Quit / Triangle). Retail's exit sub-screen
            // (`FUN_801DD12C`, sub-screen 0) runs the close-all widget script
            // `0x801E4A78` and waits for the windows to finish sliding out
            // before it fades, so the shop session - which the window draws
            // read - outlives this beat: `MenuRuntime::tick` drops it on the
            // way out of `ShopExit`.
            MenuState::ShopExit => {
                *self.trade_session = None;
                *self.recipient_session = None;
            }
            // --- Inn states ---
            MenuState::InnConfirm => self.commit_inn_confirm(slot),
            _ => {}
        }
    }

    /// Triangle from a top-of-flow screen closes the menu. Tear down any
    /// active shop / inn session so a re-open starts clean (the routed
    /// `ShopExit` teardown already clears the shop session; this catches
    /// the inn-cancel path and any direct close).
    fn cancel(&mut self) {
        *self.shop_session = None;
        *self.inn_session = None;
        *self.trade_session = None;
        *self.recipient_session = None;
    }
}
