//! The shop sub-sessions the menu tick drives: buy recipient, quantity picker, panel
//! slides, the prize exchange, and the purchases they apply.

use super::*;

impl MenuRuntime {
    /// Drive the live buy-recipient picker one frame (retail sub-screen
    /// `0x1C`, `FUN_801DB380`). Confirming row 0 buys one copy into the bag;
    /// a party row buys **and equips immediately**, returning the displaced
    /// piece to the bag - both then drop back to the buy list.
    ///
    /// Both purchase arms carry the retail commit's **Point Card accrual**
    /// (`FUN_801DB380` at `0x801db4dc` / `0x801db73c`, the same
    /// `(price / 20) * 1` credit the quantity commit runs): the session is
    /// opened with the live gate ([`World::point_card_held`]), so a party
    /// carrying the card credits the bank and holds the window-31 toast for
    /// a press before the picker closes.
    pub(super) fn tick_recipient(&mut self, world: &mut World, input: MenuInput) {
        let Some(session) = self.recipient_session.as_mut() else {
            return;
        };
        let buttons = legaia_engine_system::menu_input::NavButtons::new(
            input.cross,
            input.circle || input.triangle,
            input.up,
            input.down,
        );
        let event = session.tick(buttons);
        self.ui_cue = recipient_ui_cue(&event);
        let done = session.is_done();
        // The credit only exists once the purchase actually landed - a
        // refusal (short purse, full stack, no equip record) must bank
        // nothing and show no window.
        let landed_credit = match event {
            BuyRecipientEvent::BoughtToBag {
                item_id,
                cost,
                point_credit,
            } => Self::apply_recipient_bag_buy(world, item_id, cost).then_some(point_credit),
            BuyRecipientEvent::BoughtAndEquipped {
                party_index,
                item_id,
                cost,
                point_credit,
            } => self
                .apply_recipient_equip_buy(world, party_index, item_id, cost)
                .then_some(point_credit),
            _ => None,
        };
        if let Some(credit) = landed_credit {
            self.arm_point_card_toast(world, credit);
        }
        if done || self.recipient_session.as_ref().is_some_and(|s| s.is_done()) {
            self.recipient_session = None;
            // The picker's own `ToastWait` already consumed the press that
            // dismissed window 31; the paint flag goes with the session.
            self.point_card_toast = None;
        }
    }

    /// Open the retail quantity stepper for whatever the list just staged
    /// (`FUN_801DB7F4` phase 0 buying, `FUN_801DBD94` phase 0 selling).
    ///
    /// The bound is the same kernel the row list used to draw
    /// ([`legaia_engine_menus::shop::buy_qty_max`] / the staged stack's count), so the
    /// screen changed interaction without changing what it allows. A staged
    /// item the bag cannot supply (a sell row with a zero count) opens no
    /// picker at all, which leaves the VM on the screen it routed to.
    pub(super) fn open_quantity_picker(&mut self, world: &mut World) {
        let Some(session) = self.shop_session.as_ref() else {
            return;
        };
        let Some(item_id) = session.pending_item_id else {
            return;
        };
        let held = world.party.inventory.get(&item_id).copied();
        if session.pending_is_buying {
            let price = session
                .inventory
                .find(item_id)
                .map(|i| u16::try_from(i.price).unwrap_or(u16::MAX))
                .unwrap_or(0);
            let max = legaia_engine_menus::shop::buy_qty_max(world.party.money, price, held);
            if max < 1 {
                return;
            }
            self.quantity_session = Some(legaia_engine_menus::shop::QuantityPicker::Buy(
                legaia_engine_menus::shop::BuyQuantitySession::new(
                    item_id,
                    price,
                    world.party.money,
                    held,
                    world.point_card_held(),
                ),
            ));
        } else {
            // Selling prices off the static item table, not the merchant's
            // stock row - the sell side has no per-shop price.
            let price = world
                .shops
                .item_shop_data
                .as_ref()
                .map(|d| d.price(item_id))
                .or_else(|| {
                    session
                        .inventory
                        .find(item_id)
                        .map(|i| u16::try_from(i.price).unwrap_or(u16::MAX))
                })
                .unwrap_or(0);
            let staged = session
                .pending_bag_slot
                .map(|slot| world.party.inventory.slot(slot).1)
                .or(held)
                .unwrap_or(0);
            if staged == 0 {
                return;
            }
            self.quantity_session = Some(legaia_engine_menus::shop::QuantityPicker::Sell(
                legaia_engine_menus::shop::SellQuantitySession::new(item_id, price, staged),
            ));
        }
    }

    /// Which retail shop screen the open shop is showing, or `None` outside
    /// the gold shop (inn, seru trade, the transient exit beat, no shop).
    ///
    /// The sub-sessions decide it before the state byte does: a quantity
    /// stepper or the recipient picker owns the screen while the list state
    /// it was opened from stays parked underneath. A host feeds the result
    /// to [`legaia_engine_menus::shop::shop_screen_windows`] for the window set.
    pub fn shop_screen_phase(&self) -> Option<legaia_engine_menus::shop::ShopScreenPhase> {
        use legaia_engine_menus::shop::ShopScreenPhase as P;
        self.shop_session.as_ref()?;
        if let Some(view) = self.quantity_view() {
            return Some(if view.buying {
                P::BuyQuantity
            } else {
                P::SellQuantity
            });
        }
        if self.recipient_session.is_some() {
            return Some(P::BuyRecipient);
        }
        match MenuState::from_byte(self.ctx.state)? {
            MenuState::ShopMenu => Some(P::Root),
            MenuState::ShopBuy | MenuState::ShopQuantity | MenuState::ShopConfirm => {
                Some(P::BuyList)
            }
            MenuState::ShopSell => Some(P::SellList),
            _ => None,
        }
    }

    /// The free-running UI frame count (retail's `0x80084570` frame word, for
    /// the list kernel's blink gates).
    pub fn ui_frame(&self) -> u32 {
        self.ui_frames
    }

    /// The shop windows on screen this frame with their slide progress,
    /// `(window id, progress)` in draw order - see [`legaia_engine_menus::shop::ShopSlides`].
    pub fn shop_slides(&self) -> Vec<(usize, u8)> {
        if self.shop_session.is_none() {
            return Vec::new();
        }
        self.shop_slides.draw_list()
    }

    /// Step the shop slides one frame toward the current screen's window
    /// set. A shop that is closing (the transient exit beat) slides every
    /// window out; no shop drops them.
    pub(super) fn tick_shop_slides(&mut self) {
        // The windows wait for the field's fade to finish.
        if self.shop_fade_level().is_some() {
            self.shop_slides.tick(&[]);
            return;
        }
        match self.shop_screen_phase() {
            Some(phase) => {
                // The toast changes the set it rides over (the quantity
                // commit closes the stepper first), but its own frame is laid
                // by hand over every other window, so it joins no slide.
                let mut set = legaia_engine_menus::shop::shop_screen_windows(
                    phase,
                    self.point_card_toast.is_some(),
                );
                set.retain(|&w| w != 31);
                self.shop_slides.tick(&set);
            }
            None if self.shop_session.is_some() => self.shop_slides.tick(&[]),
            None => self.shop_slides.reset(),
        }
    }

    /// The live quantity screen's content, for the window a host draws over
    /// the parked list. `None` while no stepper owns the pad.
    pub fn quantity_view(&self) -> Option<QuantityView> {
        let picker = self.quantity_session.as_ref()?;
        Some(QuantityView {
            buying: picker.is_buying(),
            item_id: picker.item_id(),
            quantity: picker.quantity(),
            max: picker.max(),
            price: picker.price(),
        })
    }

    /// Drive the live quantity stepper one frame (retail's two sibling
    /// quantity sub-screens).
    ///
    /// Confirm commits straight out of the stepper - retail has no Yes/No
    /// screen between the number and the transaction - so the engine's
    /// `ShopConfirm` state is never entered from this path. A cancel hands
    /// the pad back to the list the flow came from.
    pub(super) fn tick_quantity(&mut self, world: &mut World, input: MenuInput) {
        let pressed = menu_input_pad_word(input);
        let Some(picker) = self.quantity_session.as_mut() else {
            return;
        };
        let buying = picker.is_buying();
        let event = picker.tick(pressed, 1);
        self.ui_cue = quantity_ui_cue(&event);
        match event {
            legaia_engine_menus::shop::QuantityPickerEvent::Bought {
                item_id,
                qty,
                cost,
                point_credit,
            } => {
                let landed = Self::apply_quantity_buy(world, item_id, qty, cost);
                if landed && point_credit > 0 {
                    self.arm_point_card_toast(world, point_credit);
                }
            }
            legaia_engine_menus::shop::QuantityPickerEvent::Sold {
                item_id,
                qty,
                credit,
            } => {
                self.apply_quantity_sell(world, item_id, qty, credit);
                let empty = world.party.inventory.iter().all(|(_, c)| *c == 0);
                if let Some(p) = self.quantity_session.as_mut() {
                    p.finish_whole_stack(empty);
                }
            }
            _ => {}
        }
        let cancelled = matches!(
            event,
            legaia_engine_menus::shop::QuantityPickerEvent::Cancelled
        );
        let done = cancelled || self.quantity_session.as_ref().is_some_and(|p| p.is_done());
        if !done {
            return;
        }
        let to_root = self
            .quantity_session
            .as_ref()
            .is_some_and(|p| p.exits_to_shop_root());
        self.quantity_session = None;
        // The picker's own `ToastWait` already consumed the press that
        // dismissed window 31, so the paint flag goes with the session -
        // leaving it set would stall the buy list for a second press.
        self.point_card_toast = None;
        if let Some(session) = self.shop_session.as_mut() {
            session.pending_item_id = None;
            session.pending_bag_slot = None;
        }
        self.ctx.state = match (to_root, buying) {
            (true, _) => MenuState::ShopMenu,
            (false, true) => MenuState::ShopBuy,
            (false, false) => MenuState::ShopSell,
        }
        .as_byte();
        // The list kept its selection behind the stepper: the hand returns
        // to its row. A sale that emptied the last row leaves the hand on
        // the new last row - with the page derived from the cursor, that is
        // also the sell commit's scroll fix-up (`0x801dbef0..0x801dbf4c`),
        // which steps a lone last-page row back one page.
        let rows = match MenuState::from_byte(self.ctx.state) {
            Some(MenuState::ShopSell) => Self::sell_list_rows(world).len(),
            Some(MenuState::ShopBuy) => self
                .shop_session
                .as_ref()
                .map(|s| s.buy_item_count() as usize)
                .unwrap_or(0),
            _ => 0,
        };
        self.ctx.cursor = if rows == 0 {
            0
        } else {
            (self.quantity_list_row as usize).min(rows - 1) as u8
        };
        self.widget_state_seen = self.ctx.state;
    }

    /// The quantity screen's **buy** commit: the bag add plus the purse
    /// debit (retail's bag-add helper plus the case-3 purse store), refused
    /// past the shared 99-per-id stack cap or on a short purse.
    fn apply_quantity_buy(world: &mut World, item_id: u8, qty: u8, cost: i32) -> bool {
        let owned = *world.party.inventory.get(&item_id).unwrap_or(&0);
        if world.party.money < cost
            || u32::from(owned) + u32::from(qty)
                > u32::from(legaia_engine_menus::shop::SHOP_HELD_CAP)
        {
            return false;
        }
        world.party.money =
            (world.party.money - cost).clamp(0, legaia_engine_menus::shop::GOLD_CAP);
        world.party.inventory.add(item_id, qty);
        true
    }

    /// The quantity screen's **sell** commit: take the units off the staged
    /// bag slot (the id path is the fallback for a row list built without
    /// slots) and credit the purse, clamped by
    /// [`legaia_engine_menus::shop::apply_sale_gold`].
    fn apply_quantity_sell(&mut self, world: &mut World, item_id: u8, qty: u8, credit: i32) {
        world.party.money = legaia_engine_menus::shop::apply_sale_gold(world.party.money, credit);
        let staged_slot = self.shop_session.as_ref().and_then(|s| s.pending_bag_slot);
        match staged_slot {
            Some(slot) => {
                world.party.inventory.consume_slot(slot, qty);
            }
            None => {
                let entry = world.party.inventory.entry(item_id).or_insert(0);
                *entry = entry.saturating_sub(qty);
                if *entry == 0 {
                    world.party.inventory.remove(&item_id);
                }
            }
        }
    }

    /// One frame of the casino prize exchange: drive the session's browse /
    /// Yes-No SM, apply a Yes commit against the live coin bank / inventory /
    /// system flags ([`legaia_engine_minigames::prize_exchange::apply_redeem`] - the retail
    /// state-3 deltas), and on the browse cancel drop the session + flip the
    /// suspended op-`0x49` Armed -> Done so the counter script resumes.
    pub(super) fn tick_prize(&mut self, world: &mut World, input: MenuInput) {
        let Some(session) = self.prize_session.as_mut() else {
            return;
        };
        // The list is vertical (Up/Down) but the shared navigator's axis is
        // decrement/increment; accept both axes, like the recipient picker.
        let buttons = legaia_engine_system::menu_input::NavButtons::new(
            input.cross,
            input.circle || input.triangle,
            input.up || input.left,
            input.down || input.right,
        );
        let coins = world.minigames.casino_coins;
        let inventory = &world.party.inventory;
        let event = session.tick(buttons, coins, |id| {
            inventory.get(&id).copied().unwrap_or(0)
        });
        // The counter's cue for this frame, on the channel the shop's
        // screens use (`MenuRuntime::step_field_session` hands it to the
        // host's blip path).
        self.ui_cue = event.cue();
        match event {
            legaia_engine_minigames::prize_exchange::PrizeEvent::Redeemed {
                item_id,
                price,
                gate,
            } => {
                let flags = &mut world.flags.system_flags;
                let applied = legaia_engine_minigames::prize_exchange::apply_redeem(
                    &mut world.minigames.casino_coins,
                    &mut world.party.inventory,
                    &mut |g| {
                        // `World::system_flag_set`, inlined over the split
                        // borrow (MSB-first, idx >> 3).
                        let byte = (g >> 3) as usize;
                        if byte >= flags.len() {
                            flags.resize(byte + 1, 0);
                        }
                        flags[byte] |= 0x80u8 >> (g & 7);
                    },
                    item_id,
                    price,
                    gate,
                );
                if applied {
                    let flags = &world.flags.system_flags;
                    session.rebuild(|g| {
                        let byte = (g >> 3) as usize;
                        flags
                            .get(byte)
                            .is_some_and(|b| b & (0x80u8 >> (g & 7)) != 0)
                    });
                }
            }
            legaia_engine_minigames::prize_exchange::PrizeEvent::Exit => {
                self.prize_session = None;
                world.finish_prize_exchange();
            }
            _ => {}
        }
    }

    /// Row-0 commit of the recipient picker: one copy into the bag plus the
    /// gold debit (retail `FUN_800421D4(id, 1)` + the purse store), refused
    /// past the 99-per-id stack cap the buy paths share. `false` when the
    /// refusal fired and nothing changed.
    fn apply_recipient_bag_buy(world: &mut World, item_id: u8, cost: i32) -> bool {
        let owned = *world.party.inventory.get(&item_id).unwrap_or(&0);
        if owned >= legaia_engine_menus::shop::SHOP_HELD_CAP || world.party.money < cost {
            return false;
        }
        world.party.money =
            (world.party.money - cost).clamp(0, legaia_engine_menus::shop::GOLD_CAP);
        let _ = world.party.inventory.add(item_id, 1);
        true
    }

    /// Party-row commit: the buy **equips directly** - the piece never
    /// enters the bag - and the previously equipped item in the slot the
    /// disc category names returns to the bag (`FUN_801DB380` at
    /// `0x801db6a4`), then the gold debit runs and the ability bits rebuild
    /// (`FUN_80042558` - engine side `World::refresh_party_ability_bits`).
    /// `false` when the item has no disc equip record, the purse is short,
    /// or the party index is out of range - nothing changed in each case.
    fn apply_recipient_equip_buy(
        &self,
        world: &mut World,
        party_index: u8,
        item_id: u8,
        cost: i32,
    ) -> bool {
        use legaia_engine_menus::equipment::EquipSlot;
        use legaia_game_tables::equip_stats::EquipSlot as Disc;
        let Some(entry) = self.equip_info.as_ref().and_then(|i| i.entry(item_id)) else {
            return false;
        };
        if world.party.money < cost {
            return false;
        }
        let slot = match entry.category {
            Disc::Weapon => EquipSlot::Weapon,
            Disc::Body => EquipSlot::BodyArmor,
            Disc::Head => EquipSlot::Helmet,
            Disc::Footwear => EquipSlot::Boot,
        };
        let idx = slot.as_index() as usize;
        // The picker's row is a party-list position; the record it equips
        // is that entry's character (`0x80084597 + row`, then
        // `char * 0x414` at `0x801DB678`).
        let Some(&ch) = world.present_party_list().get(party_index as usize) else {
            return false;
        };
        let Some(record) = world.party.roster.members.get_mut(usize::from(ch)) else {
            return false;
        };
        let mut equip = record.equipment();
        let displaced = equip.slots[idx];
        equip.slots[idx] = item_id;
        record.set_equipment(equip);
        if displaced != 0 {
            let _ = world.party.inventory.add(displaced, 1);
        }
        world.party.money =
            (world.party.money - cost).clamp(0, legaia_engine_menus::shop::GOLD_CAP);
        world.refresh_party_ability_bits();
        true
    }
}
