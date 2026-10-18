//! `MenuRuntime::tick`: one menu frame, and the window-widget choreography it syncs.

use super::*;

impl MenuRuntime {
    /// Per-frame tick. Drives the menu VM; on `SavePickSlot` / `LoadSlot`
    /// commit, runs disk I/O and emits a [`MenuTickEvent`].
    ///
    /// While a [`BuyRecipientSession`] is live the tick drives *it* instead
    /// of the menu VM - the retail shape, where sub-screen `0x1C` owns the
    /// pad and the buy list stays parked behind it.
    ///
    /// The **Point Card toast** takes precedence over the menu VM: retail's
    /// quantity commit parks in `FUN_801DB7F4` case 4 with window `0x1F`
    /// open and consumes the next confirm / cancel press itself before
    /// dropping back to the buy list, so the VM sees neither the frame nor
    /// the press. The recipient picker owns its own copy of that beat
    /// ([`BuyRecipientSession`]'s `ToastWait`), so while a picker is live it
    /// still drives - the toast is only what the host paints.
    /// Edge-detect `ctx.state` for the window-widget choreography and run
    /// the disc-resolved widget script the retail picker dispatcher
    /// (`FUN_801DAFD4`) runs on the same transition: the open script
    /// (`DAT_801E4E38`) on entering the `ShopMenu` picker, the slide-away
    /// script (`DAT_801E4E54`) on entering `ShopSell`. Menu teardown drops
    /// the window list. No-op (beyond the edge tracking) on a world
    /// without resolved overlay scripts.
    fn sync_widget_choreo(&mut self, world: &mut World) {
        let state = self.ctx.state;
        if state == self.widget_state_seen {
            return;
        }
        self.widget_state_seen = state;
        match MenuState::from_byte(state) {
            Some(MenuState::ShopMenu) => {
                world.run_shop_widget_open();
            }
            Some(MenuState::ShopSell) => {
                world.run_shop_widget_sell_away();
            }
            Some(MenuState::Closed) | Some(MenuState::Deactivate) => {
                world.menu.widgets.reset();
            }
            _ => {}
        }
    }

    pub fn tick(&mut self, world: &mut World, input: MenuInput) -> MenuTickEvent {
        // The host-side open calls (`open_shop_menu` & siblings) write
        // `ctx.state` directly, so the entry edge lands here on the first
        // tick; the post-`step` call below catches in-menu transitions.
        self.sync_widget_choreo(world);
        self.ui_frames = self.ui_frames.wrapping_add(1);
        // Age first, then slide: the tick whose age ends the fade is the
        // tick the windows leave their park edges. Stepped the other way
        // round, the first frame past the fade found no slide yet and the
        // screen drew every window at home before the slide-in started from
        // off screen - the open played twice.
        if self.shop_session.is_some() || self.prize_session.is_some() {
            self.shop_age = self.shop_age.saturating_add(1);
        }
        self.tick_shop_slides();
        self.ui_cue = None;
        if self.prize_session.is_some() {
            self.tick_prize(world, input);
            return MenuTickEvent::Stepped;
        }
        if self.recipient_session.is_some() {
            self.tick_recipient(world, input);
            return MenuTickEvent::Stepped;
        }
        if self.quantity_session.is_some() {
            self.tick_quantity(world, input);
            return MenuTickEvent::Stepped;
        }
        if self.point_card_toast.is_some() {
            // `FUN_801DB7F4` case 4: `_DAT_800846D0 | _DAT_800846D4` - the
            // confirm and cancel masks - then SFX `0x20` and sub-screen
            // `0x1B`. Nothing else on the pad dismisses it.
            if input.cross || input.circle || input.triangle {
                self.point_card_toast = None;
            }
            return MenuTickEvent::Stepped;
        }
        if self.dismiss_spell_level_notice(input.cross, input.circle, input.triangle) {
            // Window 7 (spell level-up): the cast sub-screens stall on the
            // same confirm | cancel masks before returning to the list.
            return MenuTickEvent::Stepped;
        }
        if self.dismiss_art_learned_notice(input.cross, input.circle, input.triangle) {
            // Window 8 (art learned): the Items use sub-screen's same stall.
            return MenuTickEvent::Stepped;
        }
        let list_state_before = MenuState::from_byte(self.ctx.state);
        let list_cursor_before = self.ctx.cursor;
        // The shop's two lists are kind-4 kernel pages: Up / Down wrap inside
        // the page, Left / Right flip it. The menu VM's flat cursor would walk
        // straight off the page instead, and has no page flip at all.
        let mut input = input;
        if let Some(rows) = list_state_before.and_then(shop_list_page_rows)
            && (input.up || input.down || input.left || input.right)
        {
            let n = match list_state_before {
                Some(MenuState::ShopBuy) => self
                    .shop_session
                    .as_ref()
                    .map(|s| s.buy_item_count() as usize)
                    .unwrap_or(0),
                _ => Self::sell_list_rows(world).len(),
            };
            let pressed = menu_input_pad_word(input);
            let c = legaia_engine_menus::pause_screens::list_kernel_navigate_rows(
                self.ctx.cursor as usize,
                n,
                pressed,
                rows,
            );
            self.ctx.cursor = c.min(u8::MAX as usize) as u8;
            input.up = false;
            input.down = false;
            input.left = false;
            input.right = false;
        }
        let mut refused = false;
        let mut host = MenuRuntimeHost {
            refused: &mut refused,
            world,
            slot_count: self.slot_count,
            pending: &mut self.pending,
            selected_char: &mut self.selected_char,
            shop_session: &mut self.shop_session,
            inn_session: &mut self.inn_session,
            trade_session: &mut self.trade_session,
            trade_pending_offer: &mut self.trade_pending_offer,
            equip_info: &self.equip_info,
            retail_equipment_buy: self.retail_equipment_buy,
            recipient_session: &mut self.recipient_session,
            stay_cursor: &mut self.stay_cursor,
            point_card_toast: &mut self.point_card_toast,
        };
        step(&mut host, &mut self.ctx, input);
        // The shop teardown beat is over (its windows have slid out): the
        // session goes now, not on the beat's entry.
        if list_state_before == Some(MenuState::ShopExit)
            && MenuState::from_byte(self.ctx.state) != Some(MenuState::ShopExit)
        {
            self.shop_session = None;
        }
        self.ui_cue = list_state_before.and_then(|state| {
            shop_ui_cue(
                state,
                input,
                MenuState::from_byte(self.ctx.state) == Some(state)
                    && self.ctx.cursor != list_cursor_before,
                refused,
            )
        });
        // A stay-route (refused buy / recipient open) keeps the hand on the
        // confirmed row; the VM's transition reset dropped it to 0.
        if let Some(cursor) = self.stay_cursor.take() {
            self.ctx.cursor = cursor;
        }
        // The quantity screen is a stepper, not a list: the VM's route into
        // `ShopQuantity` is the last thing it decides about that screen -
        // the picker takes the pad from the next frame on.
        if MenuState::from_byte(self.ctx.state) == Some(MenuState::ShopQuantity)
            && self.quantity_session.is_none()
        {
            self.quantity_list_row = list_cursor_before;
            self.open_quantity_picker(world);
        }
        // In-menu state transitions (picker → Sell, teardown) drive the
        // window-widget scripts.
        self.sync_widget_choreo(world);

        // After the host hooks fire, consume any pending op.
        let pending = self.pending.take();
        match pending {
            Some(PendingOp::Save { slot }) => match self.save_to_slot(world, slot) {
                Ok(path) => MenuTickEvent::Saved { slot, path },
                Err(e) => MenuTickEvent::SaveError {
                    slot,
                    message: format!("{e:#}"),
                },
            },
            Some(PendingOp::Load { slot }) => match self.load_from_slot(world, slot) {
                Ok(path) => MenuTickEvent::Loaded { slot, path },
                Err(e) => MenuTickEvent::LoadError {
                    slot,
                    message: format!("{e:#}"),
                },
            },
            None => MenuTickEvent::Stepped,
        }
    }
}
