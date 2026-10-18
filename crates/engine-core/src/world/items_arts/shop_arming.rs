//! Shop and prize-exchange sessions: buying, arming a field shop or exchange,
//! and the pending / finish hand-offs.

use super::*;

impl World {
    /// Commit a shop **buy** transaction for the session's pending item: if the
    /// player can afford it, deduct the gold and add the item(s) to
    /// [`legaia_engine_menus::party_state::PartyState::inventory`], returning `(item_id, qty, gold_delta)` (the delta is
    /// negative). Returns `None` when the buy isn't valid (unaffordable, sell
    /// mode, no pending item - see [`legaia_engine_menus::shop::ShopSession::try_buy`]).
    ///
    /// This is the engine's shop-purchase grant kernel, shared by the menu
    /// runtime's `ShopConfirm` commit and exercised directly by the shop /
    /// casino randomizer runtime oracles - the buy counterpart to
    /// [`Self::apply_steal`] / [`Self::apply_battle_loot`]. The item id sold is
    /// whatever the shop's stock holds, which for a town merchant is decoded
    /// straight from the scene's field-VM script (op `0x49`) the randomizer
    /// edits, so a patched shop id flows through here into the bag.
    pub fn buy_from_shop(
        &mut self,
        session: &legaia_engine_menus::shop::ShopSession,
    ) -> Option<(u8, u8, i32)> {
        let (item_id, qty, delta) = session.try_buy(self.party.money)?;
        // Retail fills a stack at 99: the buy list dims a row at 99 held
        // and the quantity picker clamps to `99 - held`
        // ([`legaia_engine_menus::shop::SHOP_HELD_CAP`]); refuse instead of silently
        // clamping so the menu's confirm mirrors the retail gate.
        let owned = *self.party.inventory.get(&item_id).unwrap_or(&0);
        if owned.saturating_add(qty) > legaia_engine_menus::shop::SHOP_HELD_CAP {
            return None;
        }
        self.party.money = (self.party.money + delta).clamp(0, 9_999_999);
        let _ = self.party.inventory.add(item_id, qty);
        Some((item_id, qty, delta))
    }

    /// Build a [`legaia_engine_menus::shop::ShopSession`] for the `idx`-th gold shop located in
    /// the active scene ([`legaia_engine_menus::shop_state::ShopState::scene_shops`], decoded from the scene MAN +
    /// priced from the SCUS item table at scene entry). `None` when `idx` is out
    /// of range (no merchant, or the disc / item data was absent at boot, leaving
    /// the list empty).
    ///
    /// This is the bridge from the disc-sourced per-scene stock to the menu
    /// runtime: a host installs the returned session via
    /// [`legaia_engine_menu_runtime::menu_runtime::MenuRuntime::open_shop`] when the player triggers
    /// the scene's merchant (field-VM op `0x49`).
    pub fn scene_shop_session(&self, idx: usize) -> Option<legaia_engine_menus::shop::ShopSession> {
        let shop = self.shops.scene_shops.get(idx)?;
        Some(legaia_engine_menus::shop::ShopSession::new(
            shop.inventory.clone(),
        ))
    }

    /// Debug seam: stage scene shop `idx` exactly as its merchant's op-`0x49`
    /// record would ([`Self::try_arm_field_shop`]'s staging - the pending
    /// session plus the armed gate), so each host's ordinary
    /// `take_pending_field_shop` drain opens it next frame. Lets a headless
    /// driver audit the same real stock on both hosts without walking to the
    /// merchant. `false` when the scene has no shop `idx` or one is armed.
    pub fn debug_arm_scene_shop(&mut self, idx: usize) -> bool {
        if self.shops.shop_armed {
            return false;
        }
        let Some(session) = self.scene_shop_session(idx) else {
            return false;
        };
        self.shops.pending_shop = Some(session);
        self.shops.shop_armed = true;
        self.shops.shop_open = true;
        true
    }

    /// Recognise + open a gold shop from a field-VM op-`0x49` sub-0 instruction
    /// (`instr` = the opcode byte onward: `[0x49][0x00][len][...][count][ids][name]`).
    ///
    /// The same strict, sellable-mask-gated record validation the shop catalog
    /// uses ([`legaia_scene_formats::shop_stock::parse_record`]) rejects every non-shop
    /// op-0x49 sub-0 (inn / save prompts carry MES text, not a priced item
    /// list), so this only fires on a real merchant. Gated on
    /// [`legaia_engine_menus::shop_state::ShopState::item_shop_data`] being installed - without prices there's no
    /// sellable mask (so a disc-free build can't false-positive) and no shop to
    /// price anyway. On a match it stages a priced [`legaia_engine_menus::shop::ShopSession`]
    /// on [`legaia_engine_menus::shop_state::ShopState::pending_shop`] and arms the op-0x49 gate; a no-op if a
    /// shop is already armed (single-open) or the record doesn't validate.
    ///
    /// Returns `true` when a shop was armed.
    pub fn try_arm_field_shop(&mut self, instr: &[u8]) -> bool {
        if self.shops.shop_armed {
            return false;
        }
        let Some(data) = self.shops.item_shop_data.as_ref() else {
            return false;
        };
        let mask = data.sellable_mask();
        let Some(rec) = legaia_scene_formats::shop_stock::parse_record(instr, 0, Some(&mask))
        else {
            return false;
        };
        let stock_ids: Vec<u8> = rec
            .id_offsets
            .iter()
            .filter_map(|&o| instr.get(o).copied())
            .collect();
        // Derive the vendor's identity + seru-trade schedule phase from the
        // shop's record (name + stock), so this vendor's trade offers differ
        // from every other trader's at the same play time.
        let vendor_id = legaia_game_tables::seru_trade::vendor_id_from_shop(&rec.name, &stock_ids);
        let vendor_offset =
            legaia_game_tables::seru_trade::vendor_bucket_offset(&rec.name, &stock_ids);
        // The rows themselves come from retail's builder over the record's
        // own id run (`[count][ids...]`, padding included): the hoisted
        // band first, and the record's last three entries only for a party
        // carrying or wearing the Platinum Card.
        // PORT: FUN_80030628 (live wiring; kernel =
        //       `legaia_engine_menus::menu_list_rows::build_shop_buy_rows`)
        let count = usize::from(instr[rec.count_off]);
        let record_ids = instr
            .get(rec.count_off + 1..rec.count_off + 1 + count)
            .unwrap_or(&[]);
        let inv = legaia_engine_menus::shop::ShopInventory::from_stock_record(
            0,
            record_ids,
            self.shop_tail_rows_allowed(),
            |id| data.price(id),
        );
        let mut session = legaia_engine_menus::shop::ShopSession::new(inv);
        session.vendor_id = vendor_id;
        session.vendor_bucket_offset = vendor_offset;
        self.shops.pending_shop = Some(session);
        self.shops.shop_armed = true;
        self.shops.shop_open = true;
        true
    }

    /// The shop buy list's tail probe over the live party: a Platinum Card in
    /// the bag's active window, or a `0xFF` byte in any present member's
    /// equipment block.
    ///
    /// REF: FUN_80030628 (ported as [`legaia_engine_menus::menu_list_rows::shop_tail_rows_allowed`])
    pub fn shop_tail_rows_allowed(&self) -> bool {
        let card = legaia_engine_menus::menu_list_rows::SHOP_TAIL_CARD_ID;
        let held = self.party.inventory.get(&card).is_some_and(|&c| c > 0);
        let members = usize::from(self.party.party_count);
        let equipment = (0..members).filter_map(|m| {
            self.party
                .roster
                .members
                .get(self.party_roster_slot(m))
                .map(|r| r.equipment().slots)
        });
        legaia_engine_menus::menu_list_rows::shop_tail_rows_allowed(held, equipment)
    }

    /// Drain the shop the field VM just opened (see [`Self::try_arm_field_shop`])
    /// so the host can drive its buy/sell UI. Returns `None` if no shop is
    /// pending. The op-0x49 gate stays armed until [`Self::finish_field_shop`].
    pub fn take_pending_field_shop(&mut self) -> Option<legaia_engine_menus::shop::ShopSession> {
        self.shops.pending_shop.take()
    }

    /// Mark the open field shop closed: the op-0x49 tristate flips Armed ->
    /// Done so the field VM resumes past the merchant op on its next step. The
    /// arm itself is cleared by the VM's resume (`op49_clear`).
    pub fn finish_field_shop(&mut self) {
        self.shops.shop_open = false;
    }

    /// Recognise + stage a casino **prize-exchange** counter (field-VM op
    /// `0x49` sub-op 7 - koin1's `49 07 00` / balden's `49 07 01`).
    ///
    /// `instr` is the opcode byte onward, so the prize **block index** is
    /// `instr[2]` (the byte after the sub-op; retail reads it through the
    /// parked operand pointer as `_DAT_8007B450[1]` and scales by `0x60`).
    /// Builds a [`legaia_engine_minigames::prize_exchange::PrizeExchangeSession`] over the
    /// installed [`legaia_engine_menus::shop_state::ShopState::prize_blocks`] table, raises the retail entry flag
    /// ([`legaia_engine_minigames::prize_exchange::PRIZE_EXCHANGE_VISITED_FLAG`]), and arms the
    /// op-`0x49` gate. No-op when the table isn't installed (disc-free), the
    /// block index is out of range, or an exchange is already armed.
    ///
    /// Returns `true` when a session was staged.
    pub fn try_arm_prize_exchange(&mut self, instr: &[u8]) -> bool {
        if self.shops.prize_exchange_armed {
            return false;
        }
        let block_idx = instr.get(2).copied().unwrap_or(0) as usize;
        let Some(block) = self.shops.prize_blocks.get(block_idx).cloned() else {
            return false;
        };
        // Retail state 0: `FUN_8003CE08(8)` on entry.
        self.system_flag_set(legaia_engine_minigames::prize_exchange::PRIZE_EXCHANGE_VISITED_FLAG);
        let flags = &self.flags.system_flags;
        let session =
            legaia_engine_minigames::prize_exchange::PrizeExchangeSession::new(block, |f| {
                // The same bit layout as `World::system_flag_test` (idx >> 3,
                // MSB-first), inlined so the visible-row walk can read the bank
                // while `self` is mutably borrowed.
                let byte = (f >> 3) as usize;
                flags
                    .get(byte)
                    .is_some_and(|b| b & (0x80u8 >> (f & 7)) != 0)
            });
        self.shops.pending_prize_exchange = Some(session);
        self.shops.prize_exchange_armed = true;
        self.shops.prize_exchange_open = true;
        true
    }

    /// Drain the prize-exchange session the field VM just opened (see
    /// [`Self::try_arm_prize_exchange`]) so the host can drive its UI. The
    /// op-0x49 gate stays armed until [`Self::finish_prize_exchange`].
    pub fn take_pending_prize_exchange(
        &mut self,
    ) -> Option<legaia_engine_minigames::prize_exchange::PrizeExchangeSession> {
        self.shops.pending_prize_exchange.take()
    }

    /// Mark the open prize exchange closed: the op-0x49 tristate flips
    /// Armed -> Done so the field VM resumes past the counter op.
    pub fn finish_prize_exchange(&mut self) {
        self.shops.prize_exchange_open = false;
    }
}
