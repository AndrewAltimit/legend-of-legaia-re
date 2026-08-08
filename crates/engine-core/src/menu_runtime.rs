//! Engine-side menu runtime - wires
//! [`legaia_engine_vm::menu::MenuCtx`] / [`legaia_engine_vm::menu::step`] to
//! a [`crate::world::World`] and to disk-backed save / load slots.
//!
//! [`MenuRuntime`] owns the menu ctx, a save-slot directory, and a small
//! flag block driven by [`step`] callbacks.
//! Engines call [`MenuRuntime::tick`] each frame with a [`MenuInput`]; the
//! runtime advances the state machine, captures save bytes when the menu
//! commits at `SavePickSlot`, writes them to a file, and on `LoadSlot`
//! commit reads a file back into the world.
//!
//! Rendering is engine-side (see `asset-viewer` or any custom shell) - the
//! runtime exposes a [`MenuRuntime::current_label`] string per state so the
//! HUD overlay has something to render even before the per-screen layouts
//! land.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use legaia_engine_vm::menu::{MenuCtx, MenuHost, open, step};
pub use legaia_engine_vm::menu::{MenuInput, MenuState};
use legaia_save::{EquipmentSlots, Party, SpellList};

use crate::equipment::DiscEquipInfo;
use crate::inn::InnSession;
use crate::shop::{BuyListRoute, BuyRecipientEvent, BuyRecipientSession, ShopSession};
use crate::world::World;

/// File extension the runtime uses for save slots. PSX memory-card `.mcr`
/// support is layered on top of [`legaia_save::card`]; this runtime uses a
/// flat `<dir>/slot_NN.bin` shape for development convenience.
pub const SAVE_EXT: &str = "bin";

/// One menu-driven tick outcome - engines log / observe / react.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuTickEvent {
    /// Menu ticked normally - no slot operation requested this frame.
    Stepped,
    /// Save committed to slot `slot` at `path`. Engines flash a UI banner.
    Saved {
        slot: u8,
        path: PathBuf,
    },
    /// Load committed for slot `slot` from `path`. World state was
    /// replaced with the loaded party.
    Loaded {
        slot: u8,
        path: PathBuf,
    },
    /// Save / load operation requested but the file was missing or
    /// invalid. Engines surface the error to the player; the menu
    /// transitions back to the picker.
    SaveError {
        slot: u8,
        message: String,
    },
    LoadError {
        slot: u8,
        message: String,
    },
}

/// Per-frame menu runtime. Lives alongside the world; engines tick it
/// after [`crate::world::World::tick`] when in
/// [`crate::world::SceneMode::Menu`] (or whatever in-menu mode the engine
/// uses).
pub struct MenuRuntime {
    pub ctx: MenuCtx,
    /// Save-slot directory. Created lazily on first save.
    pub save_dir: PathBuf,
    /// Number of save slots the picker offers (default 3 - one per save
    /// file in the `slot_NN.bin` shape).
    pub slot_count: u8,
    /// Index into `World::roster.members` for the active character
    /// sub-screen (StatusEquipment / StatusMagic / StatusTacticalArts).
    /// Updated by `commit(StatusCharacter, slot)`.
    pub selected_char: usize,
    /// Active shop session. Set via [`MenuRuntime::open_shop`] before
    /// entering `ShopBuy`; cleared on `ShopExit` commit.
    pub shop_session: Option<ShopSession>,
    /// Active inn session. Set via [`MenuRuntime::open_inn`] before
    /// entering `InnConfirm`; cleared after the player confirms or cancels.
    pub inn_session: Option<InnSession>,
    /// Active seru-trade session, opened when the player picks **Trade** in the
    /// `ShopMenu` top picker (the randomizer's `--seru-trade` feature). Holds the
    /// vendor's offers for the current two-hour window; cleared on shop exit.
    pub trade_session: Option<crate::seru_trade::SeruTradeSession>,
    /// Offer index selected at `ShopTrade`, applied at `ShopTradeConfirm` (the
    /// `ShopTradeConfirm` cursor is the yes/no slot, not the offer).
    trade_pending_offer: usize,
    /// Disc-pinned per-item equip restrictions (character mask + slot
    /// category), installed by hosts via [`MenuRuntime::install_equip_info`].
    /// Feeds the retail buy-list kind dispatch: without it an equipment row
    /// cannot open the recipient picker.
    pub equip_info: Option<DiscEquipInfo>,
    /// Opt into the retail-shaped **equipment buy** flow: a confirmed
    /// buy-list row whose item kind is `1` (equipment) opens the
    /// buy-recipient picker (retail sub-screen `0x1C`, `FUN_801DB380`)
    /// instead of the quantity picker. Off by default - a host that enables
    /// it must also draw the picker (window 36 + the stat-compare windows
    /// 25 / 41); the browser play page does.
    pub retail_equipment_buy: bool,
    /// The live buy-recipient picker. While `Some`, [`MenuRuntime::tick`]
    /// drives it instead of the menu VM (the menu ctx stays parked on
    /// `ShopBuy`, exactly as retail parks list mode 1 under sub-screen
    /// `0x1C`).
    pub recipient_session: Option<BuyRecipientSession>,
    /// The live casino **prize-exchange** session (menu-overlay sub-screen
    /// `0x20`, field-VM op-`0x49` sub-op 7). While `Some`,
    /// [`MenuRuntime::tick`] drives it instead of the menu VM - the session
    /// owns its own browse / Yes-No phases (`FUN_801DC1CC`'s 4-state SM), so
    /// it never enters the [`MenuState`] graph. Cleared when the browse
    /// cancel exits; the tick then calls `World::finish_prize_exchange` so
    /// the suspended field script resumes past the counter op.
    pub prize_session: Option<crate::prize_exchange::PrizeExchangeSession>,
    /// Cursor to restore after a stay-on-the-list route (a refused buy and
    /// the recipient-picker hand both keep the hand on the confirmed row;
    /// the VM's route reset would drop it to row 0).
    stay_cursor: Option<u8>,
    /// The live **Point Card toast**: `Some(credit)` while retail's window
    /// `0x1F` is up, holding the points this purchase earned.
    ///
    /// Retail's buy commit (`FUN_801DB7F4` case 2 / `FUN_801DB380`) credits
    /// the Point Card, hands the widget VM the one-command script
    /// `0x801E4EDC` / `0x801E4EA8` - both are literally `[open window 0x1F]`
    /// followed by the terminator - and then stalls in a phase that returns
    /// to the buy list only on a confirm / cancel press. The beat exists
    /// **only** while the party holds the Point Card: case 3 short-circuits
    /// straight back to sub-screen `0x1B` when `FUN_80042F4C(0xFE)` is zero.
    ///
    /// A host paints window 31 (`engine-ui`'s `amount_prompt_draws_for`)
    /// while this is `Some`.
    point_card_toast: Option<i32>,
    /// The live **spell level-up notice**: `Some` while retail's window 7 is
    /// up, holding the `(caster, spell index)` pair `FUN_80035C00` set plus
    /// the assembled prompt line.
    ///
    /// Retail's magic-cast sub-screens (`FUN_801D9280` / `FUN_801D9594`)
    /// seed `_DAT_8007BB70` / `_DAT_8007BB78` to `0xFF`, run the effect
    /// apply, and hand the widget VM the one-command script `0x801E4D50` /
    /// `0x801E4D78` (`[open window 7]` + terminator) only when the sentinel
    /// changed - then stall for a confirm / cancel press. The engine arms
    /// this from [`crate::field_menu_dispatch::apply_spell_outcome`]'s
    /// return; a host paints window 7 (`engine-ui`'s
    /// `char_prompt_draws_for`) while this is `Some` and holds the pad
    /// until a press clears it.
    spell_level_notice: Option<crate::magic_xp::SpellLevelNotice>,
    /// Pending operation flagged by the host hooks; consumed inside
    /// [`MenuRuntime::tick`].
    pending: Option<PendingOp>,
    /// Last `ctx.state` the window-widget choreography reacted to. The
    /// shop states drive the disc-resolved widget scripts
    /// ([`crate::menu_widget`]) through
    /// [`World::run_shop_widget_open`] /
    /// [`World::run_shop_widget_sell_away`]; this cell edge-detects the
    /// state changes (retail's `FUN_801DAFD4` runs the scripts on the
    /// same transitions).
    ///
    /// [`World::run_shop_widget_open`]: World::run_shop_widget_open
    /// [`World::run_shop_widget_sell_away`]: World::run_shop_widget_sell_away
    widget_state_seen: u8,
}

#[derive(Debug, Clone)]
enum PendingOp {
    Save { slot: u8 },
    Load { slot: u8 },
}

impl MenuRuntime {
    pub fn new(save_dir: impl Into<PathBuf>) -> Self {
        Self {
            ctx: MenuCtx::default(),
            save_dir: save_dir.into(),
            slot_count: 3,
            selected_char: 0,
            shop_session: None,
            inn_session: None,
            trade_session: None,
            trade_pending_offer: 0,
            equip_info: None,
            retail_equipment_buy: false,
            recipient_session: None,
            prize_session: None,
            stay_cursor: None,
            point_card_toast: None,
            spell_level_notice: None,
            pending: None,
            widget_state_seen: 0,
        }
    }

    /// The live Point Card toast's credit, or `None` when window 31 is not
    /// up. A host paints the window while this is `Some`; the number it
    /// prints is the **bank** (`World::point_card`), not this delta - retail
    /// hands `_DAT_800845B4` to the renderer.
    pub fn point_card_toast(&self) -> Option<i32> {
        self.point_card_toast
    }

    /// The live spell level-up notice (retail window 7), or `None` when no
    /// menu cast leveled a spell. A host paints the window while this is
    /// `Some` and routes the pad through
    /// [`Self::dismiss_spell_level_notice`] instead of the screen below.
    pub fn spell_level_notice(&self) -> Option<&crate::magic_xp::SpellLevelNotice> {
        self.spell_level_notice.as_ref()
    }

    /// Park a menu-cast level-up on the window-7 beat - the engine's
    /// `FUN_80035C00` (hosts arm it with
    /// [`crate::field_menu_dispatch::apply_spell_outcome`]'s return).
    pub fn arm_spell_level_notice(&mut self, notice: crate::magic_xp::SpellLevelNotice) {
        self.spell_level_notice = Some(notice);
    }

    /// One frame of the window-7 hold: retail's cast sub-screens stall
    /// until the confirm or cancel mask fires, then close the window and
    /// return to the list. Returns `true` while the notice owns the pad
    /// (whether or not this press dismissed it) so the caller skips the
    /// screen below.
    pub fn dismiss_spell_level_notice(
        &mut self,
        cross: bool,
        circle: bool,
        triangle: bool,
    ) -> bool {
        if self.spell_level_notice.is_none() {
            return false;
        }
        if cross || circle || triangle {
            self.spell_level_notice = None;
        }
        true
    }

    /// Install the disc-pinned equip restrictions the retail buy dispatch
    /// reads ([`crate::equipment::DiscEquipInfo`], built from the static
    /// `SCUS_942.54` equipment stat-bonus table).
    pub fn install_equip_info(&mut self, info: DiscEquipInfo) {
        self.equip_info = Some(info);
    }

    /// Install a shop session and prepare for `ShopBuy` entry. Engines call
    /// this when the field VM triggers a shop transition.
    pub fn open_shop(&mut self, session: ShopSession) {
        self.shop_session = Some(session);
    }

    /// Open a shop into its **top-level Buy / Sell / Trade picker**
    /// ([`MenuState::ShopMenu`]) - the field-VM op-`0x49` merchant trigger path.
    /// The Trade row appears only when the disc enabled seru trading; selecting
    /// it opens this vendor's [`crate::seru_trade::SeruTradeSession`].
    pub fn open_shop_menu(&mut self, session: ShopSession) {
        self.shop_session = Some(session);
        self.trade_session = None;
        self.recipient_session = None;
        self.ctx.state = MenuState::ShopMenu.as_byte();
        self.ctx.cursor = 0;
    }

    /// Open a shop directly into its **buy list** - the field-VM op-`0x49`
    /// merchant trigger path (distinct from the pause-menu [`Self::open`]).
    /// Installs the session and enters `ShopBuy` at the top of the list, so a
    /// host that drained [`crate::world::World::take_pending_field_shop`] can
    /// hand the player straight into the store.
    pub fn open_shop_buy(&mut self, session: ShopSession) {
        self.shop_session = Some(session);
        self.ctx.state = MenuState::ShopBuy.as_byte();
        self.ctx.cursor = 0;
    }

    /// Install an inn session and prepare for `InnConfirm` entry. `cost` is
    /// the gold required for a rest at this inn - in production the scene's
    /// scripted gold-gate literal (see [`Self::open_scene_inn`], which
    /// resolves it from the loaded scene); passing a constant directly is
    /// the test / tooling path.
    pub fn open_inn(&mut self, cost: u32) {
        self.inn_session = Some(InnSession::new(cost));
    }

    /// Open the inn prompt with the **current scene's scripted cost** - the
    /// op-`0x4E` gold-gate literal scanned from the scene MAN at load
    /// ([`crate::scene::SceneHost::scene_inn_cost`]). Installs the session
    /// and enters `InnConfirm` at the Yes slot, mirroring the
    /// [`Self::open_shop_buy`] field-trigger entry shape. Returns the
    /// resolved cost, or `None` (no session installed, state untouched)
    /// when the scene charges nothing - free rests (Rim Elm's bed, Biron)
    /// have no gate + debit pair in their scripts.
    pub fn open_scene_inn(&mut self, host: &crate::scene::SceneHost) -> Option<u32> {
        let cost = host.scene_inn_cost()?;
        self.inn_session = Some(InnSession::new(cost));
        self.ctx.state = MenuState::InnConfirm.as_byte();
        self.ctx.cursor = 0;
        Some(cost)
    }

    /// Open the menu (entry-point - typically called when the field VM
    /// requests menu via op `0x4C` sub-1).
    pub fn open(&mut self) {
        open(&mut self.ctx);
    }

    /// `true` while the menu is active (ctx state != Closed), or while a
    /// prize-exchange session owns the pad (it runs outside the
    /// [`MenuState`] graph, so `ctx.state` stays `Closed` under it).
    pub fn is_open(&self) -> bool {
        self.ctx.state != MenuState::Closed.as_byte() || self.prize_session.is_some()
    }

    /// Open the casino prize-exchange screen (field-VM op-`0x49` sub-op 7) -
    /// the counterpart of [`Self::open_shop_menu`] for the session drained
    /// from `World::take_pending_prize_exchange`.
    pub fn open_prize_exchange(&mut self, session: crate::prize_exchange::PrizeExchangeSession) {
        self.prize_session = Some(session);
    }

    /// Raw state byte of the underlying [`MenuCtx`].
    pub fn ctx_state(&self) -> u8 {
        self.ctx.state
    }

    /// Cursor position within the current screen.
    pub fn cursor(&self) -> u8 {
        self.ctx.cursor
    }

    /// The seru-trade offer currently being confirmed at `ShopTradeConfirm`
    /// (the one picked in `ShopTrade`), for the host to label the prompt.
    pub fn pending_trade_offer(&self) -> Option<legaia_asset::seru_trade::OwnerTrade> {
        self.trade_session
            .as_ref()
            .and_then(|t| t.offers.get(self.trade_pending_offer).copied())
    }

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
                world.menu_widgets.reset();
            }
            _ => {}
        }
    }

    pub fn tick(&mut self, world: &mut World, input: MenuInput) -> MenuTickEvent {
        // The host-side open calls (`open_shop_menu` & siblings) write
        // `ctx.state` directly, so the entry edge lands here on the first
        // tick; the post-`step` call below catches in-menu transitions.
        self.sync_widget_choreo(world);
        if self.prize_session.is_some() {
            self.tick_prize(world, input);
            return MenuTickEvent::Stepped;
        }
        if self.recipient_session.is_some() {
            self.tick_recipient(world, input);
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
        let mut host = MenuRuntimeHost {
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
        // A stay-route (refused buy / recipient open) keeps the hand on the
        // confirmed row; the VM's transition reset dropped it to 0.
        if let Some(cursor) = self.stay_cursor.take() {
            self.ctx.cursor = cursor;
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
    fn tick_recipient(&mut self, world: &mut World, input: MenuInput) {
        let Some(session) = self.recipient_session.as_mut() else {
            return;
        };
        let buttons = crate::menu_input::NavButtons::new(
            input.cross,
            input.circle || input.triangle,
            input.up,
            input.down,
        );
        let event = session.tick(buttons);
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

    /// One frame of the casino prize exchange: drive the session's browse /
    /// Yes-No SM, apply a Yes commit against the live coin bank / inventory /
    /// system flags ([`crate::prize_exchange::apply_redeem`] - the retail
    /// state-3 deltas), and on the browse cancel drop the session + flip the
    /// suspended op-`0x49` Armed -> Done so the counter script resumes.
    fn tick_prize(&mut self, world: &mut World, input: MenuInput) {
        let Some(session) = self.prize_session.as_mut() else {
            return;
        };
        // The list is vertical (Up/Down) but the shared navigator's axis is
        // decrement/increment; accept both axes, like the recipient picker.
        let buttons = crate::menu_input::NavButtons::new(
            input.cross,
            input.circle || input.triangle,
            input.up || input.left,
            input.down || input.right,
        );
        let coins = world.casino_coins;
        let inventory = &world.inventory;
        let event = session.tick(buttons, coins, |id| {
            inventory.get(&id).copied().unwrap_or(0)
        });
        match event {
            crate::prize_exchange::PrizeEvent::Redeemed {
                item_id,
                price,
                gate,
            } => {
                let flags = &mut world.system_flags;
                let applied = crate::prize_exchange::apply_redeem(
                    &mut world.casino_coins,
                    &mut world.inventory,
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
                    let flags = &world.system_flags;
                    session.rebuild(|g| {
                        let byte = (g >> 3) as usize;
                        flags
                            .get(byte)
                            .is_some_and(|b| b & (0x80u8 >> (g & 7)) != 0)
                    });
                }
            }
            crate::prize_exchange::PrizeEvent::Exit => {
                self.prize_session = None;
                world.finish_prize_exchange();
            }
            _ => {}
        }
    }

    /// Credit the Point Card for a landed recipient-picker purchase and arm
    /// the window-31 paint flag.
    ///
    /// Gated on **holding** the card, not on the credit being non-zero:
    /// retail's toast is `FUN_80042F4C(0xFE) != 0`, so a sub-20-gold buy
    /// still shows the window with the bank unchanged. `credit` is the
    /// session's own `(price / 20) * 1`.
    fn arm_point_card_toast(&mut self, world: &mut World, credit: i32) {
        if !world.point_card_held() {
            return;
        }
        world.point_card = crate::shop::apply_point_card(world.point_card, credit);
        self.point_card_toast = Some(credit);
    }

    /// Row-0 commit of the recipient picker: one copy into the bag plus the
    /// gold debit (retail `FUN_800421D4(id, 1)` + the purse store), refused
    /// past the 99-per-id stack cap the buy paths share. `false` when the
    /// refusal fired and nothing changed.
    fn apply_recipient_bag_buy(world: &mut World, item_id: u8, cost: i32) -> bool {
        let owned = *world.inventory.get(&item_id).unwrap_or(&0);
        if owned >= crate::shop::SHOP_HELD_CAP || world.money < cost {
            return false;
        }
        world.money = (world.money - cost).clamp(0, crate::shop::GOLD_CAP);
        *world.inventory.entry(item_id).or_insert(0) += 1;
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
        use crate::equipment::EquipSlot;
        use legaia_asset::equip_stats::EquipSlot as Disc;
        let Some(entry) = self.equip_info.as_ref().and_then(|i| i.entry(item_id)) else {
            return false;
        };
        if world.money < cost {
            return false;
        }
        let slot = match entry.category {
            Disc::Weapon => EquipSlot::Weapon,
            Disc::Body => EquipSlot::BodyArmor,
            Disc::Head => EquipSlot::Helmet,
            Disc::Footwear => EquipSlot::Boot,
        };
        let idx = slot.as_index() as usize;
        let Some(record) = world.roster.members.get_mut(party_index as usize) else {
            return false;
        };
        let mut equip = record.equipment();
        let displaced = equip.slots[idx];
        equip.slots[idx] = item_id;
        record.set_equipment(equip);
        if displaced != 0 {
            *world.inventory.entry(displaced).or_insert(0) += 1;
        }
        world.money = (world.money - cost).clamp(0, crate::shop::GOLD_CAP);
        world.refresh_party_ability_bits();
        true
    }

    /// Build the `<save_dir>/slot_NN.bin` path for `slot`.
    pub fn slot_path(&self, slot: u8) -> PathBuf {
        self.save_dir.join(format!("slot_{slot:02}.{SAVE_EXT}"))
    }

    /// Serialise the world's party and global state to slot `slot` on disk.
    ///
    /// Writes an `LGSF v1` file that includes `story_flags`, `money`, and
    /// `inventory` alongside the party records - use [`MenuRuntime::load_from_slot`]
    /// to restore. Old slot files (party-only format) are still loadable.
    pub fn save_to_slot(&self, world: &mut World, slot: u8) -> Result<PathBuf> {
        std::fs::create_dir_all(&self.save_dir)
            .with_context(|| format!("create save dir {}", self.save_dir.display()))?;
        let path = self.slot_path(slot);
        let bytes = world.save_full().write();
        std::fs::write(&path, &bytes)
            .with_context(|| format!("write save slot {} to {}", slot, path.display()))?;
        Ok(path)
    }

    /// Load slot `slot` from disk into the world's roster and global state.
    ///
    /// Accepts both `LGSF v1` (full save with globals) and the legacy party-only
    /// format written by older builds. In the legacy case `story_flags`, `money`,
    /// and `inventory` are left at their current values.
    pub fn load_from_slot(&self, world: &mut World, slot: u8) -> Result<PathBuf> {
        let path = self.slot_path(slot);
        let bytes = std::fs::read(&path)
            .with_context(|| format!("read save slot {} from {}", slot, path.display()))?;
        let sf = legaia_save::SaveFile::parse(&bytes)
            .with_context(|| format!("parse save slot {} ({} bytes)", slot, bytes.len()))?;
        world.load_full(sf);
        Ok(path)
    }

    /// Write the world's party into a free block chain on a PSX memory-card
    /// image at `card_path`. Reads the existing card, appends the save in the
    /// first free block(s), and writes it back in place. Returns the first
    /// block index written.
    ///
    /// This is a convenience on top of [`legaia_save::write_block`]; it does
    /// not update the engine's slot-file directory. Use `save_to_slot` for
    /// the flat `.bin` save path the menu runtime normally drives.
    pub fn save_to_card(&self, world: &mut World, card_path: &std::path::Path) -> Result<u8> {
        let mut card = std::fs::read(card_path)
            .with_context(|| format!("read card {}", card_path.display()))?;
        let bytes = world.save_party().write();
        let block = legaia_save::write_block(&mut card, &bytes, "BASCUS-94254LEGAIA")?;
        std::fs::write(card_path, &card)
            .with_context(|| format!("write card {}", card_path.display()))?;
        Ok(block)
    }

    /// Spell list for the currently selected character, or `None` if
    /// `selected_char` is out of bounds.  Engines call this to populate
    /// the `StatusMagic` screen rows.
    pub fn spell_view(&self, world: &World) -> Option<SpellList> {
        world
            .roster
            .members
            .get(self.selected_char)
            .map(|r| r.spell_list())
    }

    /// Equipment slots for the currently selected character, or `None` if
    /// `selected_char` is out of bounds.  Engines call this to populate
    /// the `StatusEquipment` screen rows.
    pub fn equipment_view(&self, world: &World) -> Option<EquipmentSlots> {
        world
            .roster
            .members
            .get(self.selected_char)
            .map(|r| r.equipment())
    }

    /// Sorted `(item_id, count)` pairs from the world's global inventory,
    /// ascending by item ID, filtering out zero-count entries.  Engines
    /// call this to populate the `StatusInventory` screen rows.
    pub fn inventory_items(world: &World) -> Vec<(u8, u8)> {
        let mut items: Vec<(u8, u8)> = world
            .inventory
            .iter()
            .filter(|(_, c)| **c > 0)
            .map(|(id, c)| (*id, *c))
            .collect();
        items.sort_by_key(|&(id, _)| id);
        items
    }

    /// Engine-friendly label per active state - drives a HUD banner so the
    /// player sees *something* before the per-screen layouts ship.
    pub fn current_label(&self) -> &'static str {
        match MenuState::from_byte(self.ctx.state) {
            Some(MenuState::Closed) => "",
            Some(MenuState::Idle) => "MENU",
            Some(MenuState::StatusTop) => "STATUS",
            Some(MenuState::StatusCharacter) => "CHARACTER",
            Some(MenuState::StatusEquipment) => "EQUIP",
            Some(MenuState::StatusInventory) => "ITEMS",
            Some(MenuState::StatusMagic) => "MAGIC",
            Some(MenuState::StatusTacticalArts) => "ARTS",
            Some(MenuState::StatusConfig) => "CONFIG",
            Some(MenuState::StatusLog) => "LOG",
            Some(MenuState::SavePickSlot) => "SAVE - PICK SLOT",
            Some(MenuState::SaveConfirmOverwrite) => "SAVE - OVERWRITE?",
            Some(MenuState::SaveWriting) => "SAVING…",
            Some(MenuState::SaveDone) => "SAVED",
            Some(MenuState::LoadSlot) => "LOAD - PICK SLOT",
            Some(MenuState::LoadProgress) => "LOADING…",
            Some(MenuState::ShopMenu) => "SHOP",
            Some(MenuState::ShopBuy) => "SHOP - BUY",
            Some(MenuState::ShopSell) => "SHOP - SELL",
            Some(MenuState::ShopQuantity) => "SHOP - HOW MANY?",
            Some(MenuState::ShopConfirm) => "SHOP - CONFIRM",
            Some(MenuState::ShopTrade) => "SHOP - TRADE SERU",
            Some(MenuState::ShopTradeConfirm) => "SHOP - TRADE?",
            Some(MenuState::ShopExit) => "SHOP - DONE",
            Some(MenuState::InnConfirm) => "INN - REST?",
            Some(MenuState::InnSleep) => "INN - RESTING",
            Some(MenuState::ItemPickTarget) => "ITEM - TARGET",
            Some(MenuState::ItemApply) => "ITEM - APPLY",
            Some(MenuState::ItemDone) => "ITEM - DONE",
            Some(MenuState::Confirm) => "CONFIRM?",
            Some(MenuState::Closing) => "CLOSING",
            Some(MenuState::Deactivate) => "",
            None => "?",
        }
    }
}

/// The action each row of the [`MenuState::ShopMenu`] top picker commits to, in
/// row order. The Trade row only exists when seru trading is enabled, so this is
/// the single source of truth the cursor count, render, route, and commit all
/// read (keeping the dynamic layout consistent).
pub fn shop_menu_rows(trading: bool) -> &'static [MenuState] {
    if trading {
        &[
            MenuState::ShopBuy,
            MenuState::ShopSell,
            MenuState::ShopTrade,
            MenuState::ShopExit,
        ]
    } else {
        &[MenuState::ShopBuy, MenuState::ShopSell, MenuState::ShopExit]
    }
}

struct MenuRuntimeHost<'a> {
    world: &'a mut World,
    slot_count: u8,
    pending: &'a mut Option<PendingOp>,
    selected_char: &'a mut usize,
    shop_session: &'a mut Option<ShopSession>,
    inn_session: &'a mut Option<InnSession>,
    trade_session: &'a mut Option<crate::seru_trade::SeruTradeSession>,
    trade_pending_offer: &'a mut usize,
    equip_info: &'a Option<DiscEquipInfo>,
    retail_equipment_buy: bool,
    recipient_session: &'a mut Option<BuyRecipientSession>,
    stay_cursor: &'a mut Option<u8>,
    point_card_toast: &'a mut Option<i32>,
}

impl MenuRuntimeHost<'_> {
    /// Row actions for the current `ShopMenu` (Trade present iff trading on).
    fn shop_menu_rows(&self) -> &'static [MenuState] {
        shop_menu_rows(self.world.seru_trade_enabled())
    }

    /// The retail buy-list confirm dispatch for the row at `slot`
    /// ([`crate::shop::buy_list_confirm_route`], `FUN_801DB21C` state 2):
    /// affordability against the purse first, then the item record's `+0`
    /// kind byte picks the follow-up screen. The kind comes from the
    /// on-disc item-effect tables ([`World::item_effects`]); a
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
        let kind = match self.world.item_effects.as_ref() {
            Some(t) => t.kind(item.item_id),
            None => match self.equip_info.as_ref() {
                Some(info) if info.is_equipment(item.item_id) => 1,
                _ => 2,
            },
        };
        let price = u16::try_from(item.price).unwrap_or(u16::MAX);
        Some(crate::shop::buy_list_confirm_route(
            kind,
            self.world.money,
            price,
        ))
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
        let can_equip: Vec<bool> = (0..self.world.roster.members.len().min(3))
            .map(|i| info.can_equip(item.item_id, i as u8))
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
        if let Some(record) = self.world.roster.members.get_mut(idx) {
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
            *self.world.inventory.entry(removed).or_insert(0) += 1;
        }
        // Unequipping can remove an accessory passive; rebuild the
        // ability bitfields so the bit (and any party-wide grant)
        // disappears immediately.
        self.world.refresh_party_ability_bits();
    }

    /// `StatusInventory` commit: decrement (or remove) the picked bag item.
    fn commit_status_inventory(&mut self, slot: u8) {
        let mut items: Vec<(u8, u8)> = self
            .world
            .inventory
            .iter()
            .filter(|(_, c)| **c > 0)
            .map(|(id, c)| (*id, *c))
            .collect();
        items.sort_by_key(|&(id, _)| id);
        if let Some(&(item_id, count)) = items.get(slot as usize) {
            if count > 1 {
                self.world.inventory.insert(item_id, count - 1);
            } else {
                self.world.inventory.remove(&item_id);
            }
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
        let sell_items: Vec<(u8, u8)> = {
            let mut v: Vec<(u8, u8)> = self
                .world
                .inventory
                .iter()
                .filter(|(_, c)| **c > 0)
                .map(|(id, c)| (*id, *c))
                .collect();
            v.sort_by_key(|&(id, _)| id);
            v
        };
        if let Some(session) = self.shop_session.as_mut() {
            session.select_sell_item(slot as usize, &sell_items);
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
                let held = self.world.inventory.get(&item_id).copied().unwrap_or(0);
                if let Some((item_id, qty, delta)) = session.try_sell(held) {
                    self.world.money = (self.world.money + delta).clamp(0, 9_999_999);
                    let entry = self.world.inventory.entry(item_id).or_insert(0);
                    *entry = entry.saturating_sub(qty);
                    if *entry == 0 {
                        self.world.inventory.remove(&item_id);
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
            let pt = self.world.play_time_seconds;
            if let Some(t) = self.trade_session.as_mut() {
                t.refresh(pt, &self.world.roster.members);
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
                .is_some_and(|s| s.can_afford(self.world.money));
            if can {
                let cost = self.inn_session.as_ref().unwrap().cost as i32;
                self.world.money -= cost;
                // Restore HP/MP for all active party members.
                let party_count = self.world.party_count as usize;
                for i in 0..party_count {
                    let max_hp = self
                        .world
                        .actors
                        .get(i)
                        .map(|a| a.battle.max_hp)
                        .unwrap_or(0);
                    let mp_max = self
                        .world
                        .roster
                        .members
                        .get(i)
                        .map(|r| r.hp_mp_sp().mp_max)
                        .unwrap_or(0);
                    if let Some(actor) = self.world.actors.get_mut(i)
                        && actor.active
                    {
                        actor.battle.hp = max_hp;
                        actor.battle.mp = mp_max;
                    }
                }
                // Sync restored values back to roster records.
                self.world.save_party();
            }
        }
        // Clear session regardless of yes/no.
        *self.inn_session = None;
    }
}

impl<'a> MenuHost for MenuRuntimeHost<'a> {
    fn screen_item_count(&self, state: MenuState) -> u8 {
        match state {
            MenuState::StatusTop => 8, // Character / Equip / Items / Magic / Arts / Config / Save / Load
            MenuState::StatusCharacter => {
                self.world.roster.members.len().min(u8::MAX as usize) as u8
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
                .inventory
                .values()
                .filter(|c| **c > 0)
                .count()
                .min(u8::MAX as usize) as u8,
            MenuState::ShopQuantity => 9, // quantities 1..=9 (cursor + 1)
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
                    let vendor = self.shop_session.as_ref().map(|s| s.vendor_id).unwrap_or(0);
                    *self.trade_session = self.world.open_seru_trade(vendor);
                }
            }
            MenuState::ShopBuy => match self.shop_buy_route(slot) {
                // Gold short: the retail refusal beat (buzz SFX `0x23`,
                // list re-armed) - no pending item, hand stays on the row.
                Some(BuyListRoute::Refused) => {
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
            // menu (Triangle) - clears the sessions as the menu closes.
            MenuState::ShopExit => {
                *self.shop_session = None;
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

/// Convenience accessor: save the world's roster directly to `path`,
/// bypassing the slot indirection. Used by tests + custom save flows.
pub fn save_world_to_path(world: &mut World, path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create dir {}", parent.display()))?;
    }
    let bytes = world.save_party().write();
    std::fs::write(path, &bytes).with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

/// Convenience accessor: load the party at `path` into `world`. Replaces
/// the world's current roster.
pub fn load_world_from_path(world: &mut World, path: &Path) -> Result<()> {
    let bytes = std::fs::read(path).with_context(|| format!("read {}", path.display()))?;
    let party = Party::parse(&bytes).with_context(|| format!("parse {}", path.display()))?;
    world.load_party(party);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_save::{CharacterRecord, EquipmentSlots, SpellList};

    fn world_with_party(n: usize) -> World {
        let members = (0..n).map(|_| CharacterRecord::zeroed()).collect();
        let mut world = World::default();
        world.load_party(Party { members });
        world
    }

    #[test]
    fn save_then_load_round_trips_through_disk() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let runtime = MenuRuntime::new(tmp.path().to_path_buf());

        let mut world = world_with_party(3);
        // Mutate one HP value so we can detect round-trip drift.
        world.actors[0].battle.hp = 0x1234;
        let _ = runtime.save_to_slot(&mut world, 1).expect("save_to_slot");
        let path = runtime.slot_path(1);
        assert!(path.exists());

        // Load into a fresh world; HP should match.
        let mut fresh = world_with_party(3);
        runtime
            .load_from_slot(&mut fresh, 1)
            .expect("load_from_slot");
        // The mirrored HP propagates through the BattleActor.
        assert_eq!(fresh.actors[0].battle.hp, 0x1234);
    }

    #[test]
    fn current_label_changes_with_state() {
        let mut runtime = MenuRuntime::new("/tmp/legaia-doesnt-need-this-dir");
        runtime.ctx.state = MenuState::SavePickSlot.as_byte();
        assert_eq!(runtime.current_label(), "SAVE - PICK SLOT");
        runtime.ctx.state = MenuState::Closed.as_byte();
        assert_eq!(runtime.current_label(), "");
    }

    #[test]
    fn slot_path_uses_save_ext() {
        let runtime = MenuRuntime::new("/tmp/legaia-test-save");
        let p = runtime.slot_path(7);
        assert!(p.to_string_lossy().ends_with("slot_07.bin"));
    }

    #[test]
    fn status_character_commit_sets_selected_char() {
        let mut world = world_with_party(3);
        let mut runtime = MenuRuntime::new("/tmp/legaia-test");
        runtime.ctx.state = MenuState::StatusCharacter.as_byte();
        runtime.ctx.cursor = 2;
        runtime.tick(
            &mut world,
            MenuInput {
                cross: true,
                ..Default::default()
            },
        );
        assert_eq!(runtime.selected_char, 2);
    }

    #[test]
    fn equipment_commit_unequips_slot() {
        let mut world = world_with_party(1);
        let equip = EquipmentSlots {
            slots: [1, 2, 3, 4, 5, 6, 7, 8],
        };
        world.roster.members[0].set_equipment(equip);

        // Slot 2 holds item id 3; it must come back to the bag on unequip.
        let before = world.inventory.get(&3).copied().unwrap_or(0);

        let mut runtime = MenuRuntime::new("/tmp/legaia-test");
        runtime.selected_char = 0;
        runtime.ctx.state = MenuState::StatusEquipment.as_byte();
        runtime.ctx.cursor = 2;
        runtime.tick(
            &mut world,
            MenuInput {
                cross: true,
                ..Default::default()
            },
        );

        let updated = world.roster.members[0].equipment();
        assert_eq!(updated.slots[2], 0, "slot 2 unequipped");
        assert_eq!(updated.slots[0], 1, "other slots unchanged");
        assert_eq!(updated.slots[7], 8, "other slots unchanged");
        // The unequipped item returned to the bag (not destroyed).
        assert_eq!(
            world.inventory.get(&3).copied().unwrap_or(0),
            before + 1,
            "unequipped item 3 returned to inventory"
        );
    }

    #[test]
    fn equipment_commit_out_of_bounds_char_is_noop() {
        let mut world = world_with_party(1);
        let mut runtime = MenuRuntime::new("/tmp/legaia-test");
        runtime.selected_char = 99; // no such char
        runtime.ctx.state = MenuState::StatusEquipment.as_byte();
        runtime.ctx.cursor = 0;
        // Should not panic.
        runtime.tick(
            &mut world,
            MenuInput {
                cross: true,
                ..Default::default()
            },
        );
    }

    #[test]
    fn inventory_commit_decrements_item_count() {
        let mut world = World::default();
        world.inventory.insert(5, 3);

        let mut runtime = MenuRuntime::new("/tmp/legaia-test");
        runtime.ctx.state = MenuState::StatusInventory.as_byte();
        runtime.ctx.cursor = 0;
        runtime.tick(
            &mut world,
            MenuInput {
                cross: true,
                ..Default::default()
            },
        );

        assert_eq!(world.inventory.get(&5), Some(&2));
    }

    #[test]
    fn inventory_commit_removes_last_item() {
        let mut world = World::default();
        world.inventory.insert(10, 1);

        let mut runtime = MenuRuntime::new("/tmp/legaia-test");
        runtime.ctx.state = MenuState::StatusInventory.as_byte();
        runtime.ctx.cursor = 0;
        runtime.tick(
            &mut world,
            MenuInput {
                cross: true,
                ..Default::default()
            },
        );

        assert!(!world.inventory.contains_key(&10));
    }

    #[test]
    fn inventory_commit_empty_inventory_is_noop() {
        let mut world = World::default();
        let mut runtime = MenuRuntime::new("/tmp/legaia-test");
        runtime.ctx.state = MenuState::StatusInventory.as_byte();
        runtime.ctx.cursor = 0;
        // Should not panic on empty inventory.
        runtime.tick(
            &mut world,
            MenuInput {
                cross: true,
                ..Default::default()
            },
        );
    }

    #[test]
    fn spell_view_returns_selected_char_spells() {
        let mut world = world_with_party(2);
        let mut list = SpellList {
            count: 2,
            ..SpellList::default()
        };
        list.ids[0] = 7;
        list.ids[1] = 14;
        world.roster.members[1].set_spell_list(list);

        let mut runtime = MenuRuntime::new("/tmp/legaia-test");
        runtime.selected_char = 1;

        let view = runtime.spell_view(&world).expect("char 1 exists");
        assert_eq!(view.count, 2);
        assert_eq!(view.ids[0], 7);
        assert_eq!(view.ids[1], 14);
    }

    #[test]
    fn spell_view_out_of_bounds_returns_none() {
        let world = world_with_party(1);
        let mut runtime = MenuRuntime::new("/tmp/legaia-test");
        runtime.selected_char = 5;
        assert!(runtime.spell_view(&world).is_none());
    }

    #[test]
    fn equipment_view_returns_selected_char_equipment() {
        let mut world = world_with_party(2);
        let equip = EquipmentSlots {
            slots: [9, 8, 7, 6, 5, 4, 3, 2],
        };
        world.roster.members[0].set_equipment(equip);

        let runtime = MenuRuntime::new("/tmp/legaia-test");
        let view = runtime.equipment_view(&world).expect("char 0 exists");
        assert_eq!(view.slots, [9, 8, 7, 6, 5, 4, 3, 2]);
    }

    #[test]
    fn inventory_items_sorted_by_id_filters_zeros() {
        let mut world = World::default();
        world.inventory.insert(30, 5);
        world.inventory.insert(2, 1);
        world.inventory.insert(15, 3);

        let items = MenuRuntime::inventory_items(&world);
        assert_eq!(items, vec![(2, 1), (15, 3), (30, 5)]);
    }

    #[test]
    fn screen_item_count_for_character_clamps_cursor_to_party_size() {
        let mut world = world_with_party(2);
        let mut runtime = MenuRuntime::new("/tmp/legaia-test");
        runtime.ctx.state = MenuState::StatusCharacter.as_byte();
        runtime.ctx.cursor = 0;
        // Down 3 times with 2 members: 0 -> 1 -> 0 -> 1
        for _ in 0..3 {
            runtime.tick(
                &mut world,
                MenuInput {
                    down: true,
                    ..Default::default()
                },
            );
        }
        assert_eq!(runtime.ctx.cursor, 1);
    }

    fn cross() -> MenuInput {
        MenuInput {
            cross: true,
            ..Default::default()
        }
    }

    fn triangle() -> MenuInput {
        MenuInput {
            triangle: true,
            ..Default::default()
        }
    }

    fn down() -> MenuInput {
        MenuInput {
            down: true,
            ..Default::default()
        }
    }

    #[test]
    fn shop_menu_trade_row_drives_a_seru_swap() {
        use crate::shop::{ShopInventory, ShopSession};

        // A shop on a disc with seru trading enabled; the lead owns the seru
        // the seed's bucket-0 offer wants (the bucket model trades a type the
        // party holds) plus an unrelated one.
        let seed = 0xABCDu64;
        let bucket0 = legaia_asset::seru_trade::bucket_offer(
            seed,
            0,
            &legaia_asset::seru_trade::default_pool(),
        );
        let mut world = World::new();
        world.seru_trade_config = Some(legaia_asset::seru_trade::SeruTradeConfig {
            enabled: true,
            seed,
            max_offers: 4,
        });
        let mut lead = CharacterRecord::zeroed();
        let mut list = SpellList::default();
        list.ids[0] = bucket0.want_id;
        list.count = 1;
        lead.set_spell_list(list);
        world.load_party(Party {
            members: vec![lead],
        });

        let mut runtime = MenuRuntime::new("/tmp/legaia-test");
        let mut shop = ShopSession::new(ShopInventory::new(0, vec![]));
        shop.vendor_id = 7;
        runtime.open_shop_menu(shop);
        assert_eq!(runtime.ctx.state, MenuState::ShopMenu.as_byte());

        // Rows = [Buy, Sell, Trade, Exit]; move to Trade (idx 2) and commit.
        runtime.tick(&mut world, down());
        runtime.tick(&mut world, down());
        assert_eq!(runtime.ctx.cursor, 2);
        runtime.tick(&mut world, cross());
        assert_eq!(runtime.ctx.state, MenuState::ShopTrade.as_byte());
        let offer = runtime
            .trade_session
            .as_ref()
            .and_then(|t| t.offers.first().copied())
            .expect("the vendor offers a trade");

        // Pick the first offer, then confirm Yes.
        runtime.tick(&mut world, cross());
        assert_eq!(runtime.ctx.state, MenuState::ShopTradeConfirm.as_byte());
        runtime.tick(&mut world, cross());
        assert_eq!(
            runtime.ctx.state,
            MenuState::ShopTrade.as_byte(),
            "after a trade the menu returns to the offer list"
        );

        // The owner's spell list now holds the received seru - at the offered
        // level - and no longer the given one.
        let list = world.roster.members[offer.owner_slot as usize].spell_list();
        let ids = &list.ids[..list.count as usize];
        let pos = ids
            .iter()
            .position(|&id| id == offer.received_id)
            .expect("received seru added");
        assert_eq!(
            list.levels[pos], offer.received_level,
            "received seru arrives at the offered level"
        );
        assert!(!ids.contains(&offer.given_id), "given seru removed");
    }

    #[test]
    fn shop_menu_hides_trade_row_when_trading_disabled() {
        use crate::shop::{ShopInventory, ShopSession};

        let mut world = World::default(); // no seru_trade_config -> disabled
        world.load_party(Party {
            members: vec![CharacterRecord::zeroed()],
        });
        let mut runtime = MenuRuntime::new("/tmp/legaia-test");
        runtime.open_shop_menu(ShopSession::new(ShopInventory::new(0, vec![])));

        // Rows = [Buy, Sell, Exit] (no Trade). Slot 2 routes to ShopExit.
        runtime.tick(&mut world, down());
        runtime.tick(&mut world, down());
        assert_eq!(runtime.ctx.cursor, 2);
        runtime.tick(&mut world, cross());
        assert_eq!(runtime.ctx.state, MenuState::ShopExit.as_byte());
    }

    #[test]
    fn shop_buy_flow_drives_through_tick_and_grants_item() {
        use crate::shop::{ShopInventory, ShopItem, ShopSession};

        let mut world = world_with_party(1);
        world.money = 500;
        let mut runtime = MenuRuntime::new("/tmp/legaia-test");
        runtime.open_shop(ShopSession::new(ShopInventory::new(
            1,
            vec![ShopItem {
                item_id: 10,
                price: 100,
            }],
        )));
        runtime.ctx.state = MenuState::ShopBuy.as_byte();

        // ShopBuy (cursor 0 = item 10) -> ShopQuantity.
        runtime.tick(&mut world, cross());
        assert_eq!(runtime.ctx.state, MenuState::ShopQuantity.as_byte());
        // ShopQuantity (cursor 0 = qty 1) -> ShopConfirm.
        runtime.tick(&mut world, cross());
        assert_eq!(runtime.ctx.state, MenuState::ShopConfirm.as_byte());
        // ShopConfirm (cursor 0 = yes) -> back to ShopBuy, purchase applied.
        runtime.tick(&mut world, cross());
        assert_eq!(runtime.ctx.state, MenuState::ShopBuy.as_byte());

        assert_eq!(world.money, 400, "100 gold deducted");
        assert_eq!(world.inventory.get(&10), Some(&1), "one item 10 granted");
        assert!(runtime.shop_session.is_some(), "still shopping");
    }

    /// The buy commit's Point Card accrual (`FUN_801DB7F4` case 2) and the
    /// window-31 beat that follows it (cases 3 + 4): a party carrying item
    /// `0xFE` banks 5% of the gold spent and the runtime holds the toast
    /// until a press, which the menu VM never sees.
    #[test]
    fn shop_buy_credits_the_point_card_and_holds_the_window_31_toast() {
        use crate::shop::{POINT_CARD_ITEM_ID, ShopInventory, ShopItem, ShopSession};

        let mut world = world_with_party(1);
        world.money = 500;
        world.inventory.insert(POINT_CARD_ITEM_ID, 1);
        let mut runtime = MenuRuntime::new("/tmp/legaia-test");
        runtime.open_shop(ShopSession::new(ShopInventory::new(
            1,
            vec![ShopItem {
                item_id: 10,
                price: 100,
            }],
        )));
        runtime.ctx.state = MenuState::ShopBuy.as_byte();

        runtime.tick(&mut world, cross()); // ShopBuy -> ShopQuantity
        runtime.tick(&mut world, cross()); // ShopQuantity -> ShopConfirm
        runtime.tick(&mut world, cross()); // ShopConfirm (yes): the commit

        assert_eq!(world.money, 400, "the gold debit still runs");
        assert_eq!(world.point_card, 5, "100 / 20 * 1 banked");
        assert_eq!(
            runtime.point_card_toast(),
            Some(5),
            "the toast is up with this purchase's credit"
        );

        // While the toast is up the VM is frozen: a d-pad frame moves
        // nothing, because retail's case 4 only tests the confirm / cancel
        // masks.
        let state_before = runtime.ctx.state;
        runtime.tick(&mut world, down());
        assert!(runtime.point_card_toast().is_some(), "d-pad does not clear");
        assert_eq!(runtime.ctx.state, state_before);

        runtime.tick(&mut world, cross());
        assert_eq!(runtime.point_card_toast(), None, "a press dismisses it");
    }

    /// The window-7 beat: an armed spell level-up notice freezes the menu
    /// VM (a d-pad frame moves nothing) and holds until a confirm / cancel
    /// press - the same stall the retail cast sub-screens run after the
    /// widget-VM `[open window 7]` script.
    #[test]
    fn spell_level_notice_holds_until_a_press() {
        let mut world = world_with_party(1);
        let mut runtime = MenuRuntime::new("/tmp/legaia-test");
        runtime.open();
        runtime.arm_spell_level_notice(crate::magic_xp::SpellLevelNotice {
            caster_slot: 0,
            spell_index: 0,
            spell_id: 0x83,
            new_level: 2,
            line: "Gimard's magic level increased.".into(),
        });
        assert!(runtime.spell_level_notice().is_some());

        let state_before = runtime.ctx.state;
        runtime.tick(&mut world, down());
        assert!(
            runtime.spell_level_notice().is_some(),
            "d-pad does not clear"
        );
        assert_eq!(runtime.ctx.state, state_before, "the VM is frozen");

        runtime.tick(&mut world, cross());
        assert_eq!(runtime.spell_level_notice(), None, "a press dismisses it");
        assert_eq!(runtime.ctx.state, state_before, "the press is consumed");
    }

    /// Without the card in the bag the accrual short-circuits and so does
    /// the beat - retail's case 3 returns straight to sub-screen `0x1B`.
    #[test]
    fn shop_buy_without_the_point_card_neither_credits_nor_toasts() {
        use crate::shop::{ShopInventory, ShopItem, ShopSession};

        let mut world = world_with_party(1);
        world.money = 500;
        let mut runtime = MenuRuntime::new("/tmp/legaia-test");
        runtime.open_shop(ShopSession::new(ShopInventory::new(
            1,
            vec![ShopItem {
                item_id: 10,
                price: 100,
            }],
        )));
        runtime.ctx.state = MenuState::ShopBuy.as_byte();
        runtime.tick(&mut world, cross());
        runtime.tick(&mut world, cross());
        runtime.tick(&mut world, cross());

        assert_eq!(world.inventory.get(&10), Some(&1), "the buy still lands");
        assert_eq!(world.point_card, 0);
        assert_eq!(runtime.point_card_toast(), None);
        assert_eq!(
            runtime.ctx.state,
            MenuState::ShopBuy.as_byte(),
            "and the list is reachable on the very next frame"
        );
    }

    /// A refused buy (short purse) must not bank points: retail's case 2
    /// is only reached from the quantity screen a affordable row opened.
    #[test]
    fn a_refused_buy_banks_no_points() {
        use crate::shop::{POINT_CARD_ITEM_ID, ShopInventory, ShopItem, ShopSession};

        let mut world = world_with_party(1);
        world.money = 50;
        world.inventory.insert(POINT_CARD_ITEM_ID, 1);
        let mut runtime = MenuRuntime::new("/tmp/legaia-test");
        runtime.open_shop(ShopSession::new(ShopInventory::new(
            1,
            vec![ShopItem {
                item_id: 10,
                price: 100,
            }],
        )));
        runtime.ctx.state = MenuState::ShopBuy.as_byte();
        runtime.tick(&mut world, cross());

        assert_eq!(world.point_card, 0);
        assert_eq!(runtime.point_card_toast(), None);
    }

    #[test]
    fn shop_buy_refusal_beat_stays_on_the_list_row() {
        use crate::shop::{ShopInventory, ShopItem, ShopSession};

        // 50 gold against a 100-gold row: the retail state-2 dispatch
        // refuses at the list (`slt gold, price` + buzz) - no pending
        // item, no quantity screen, hand still on the confirmed row.
        let mut world = world_with_party(1);
        world.money = 50;
        let mut runtime = MenuRuntime::new("/tmp/legaia-test");
        runtime.open_shop(ShopSession::new(ShopInventory::new(
            1,
            vec![
                ShopItem {
                    item_id: 9,
                    price: 10,
                },
                ShopItem {
                    item_id: 10,
                    price: 100,
                },
            ],
        )));
        runtime.ctx.state = MenuState::ShopBuy.as_byte();
        runtime.tick(&mut world, down());
        assert_eq!(runtime.ctx.cursor, 1);
        runtime.tick(&mut world, cross());
        assert_eq!(
            runtime.ctx.state,
            MenuState::ShopBuy.as_byte(),
            "refused buy stays on the list"
        );
        assert_eq!(runtime.ctx.cursor, 1, "hand stays on the refused row");
        assert!(
            runtime
                .shop_session
                .as_ref()
                .is_some_and(|s| s.pending_item_id.is_none()),
            "no pending item was staged"
        );
        // An affordable row still routes into the quantity picker.
        world.money = 500;
        runtime.tick(&mut world, cross());
        assert_eq!(runtime.ctx.state, MenuState::ShopQuantity.as_byte());
    }

    #[test]
    fn equipment_buy_opens_recipient_picker_and_equips_now() {
        use crate::equipment::{DiscEquipEntry, DiscEquipInfo};
        use crate::shop::{ShopInventory, ShopItem, ShopSession};
        use legaia_asset::equip_stats::EquipSlot as Disc;

        let mut world = world_with_party(2);
        world.money = 300;
        // Party member 1 already wears item 7 in the weapon slot.
        let mut eq = world.roster.members[1].equipment();
        eq.slots[crate::equipment::EquipSlot::Weapon.as_index() as usize] = 7;
        world.roster.members[1].set_equipment(eq);

        let mut runtime = MenuRuntime::new("/tmp/legaia-test");
        runtime.retail_equipment_buy = true;
        runtime.install_equip_info(DiscEquipInfo::from_entries([(
            0x30,
            DiscEquipEntry {
                mask: 0b010, // second party member only
                category: Disc::Weapon,
                is_ra_seru: false,
                passive_index: None,
            },
        )]));
        runtime.open_shop(ShopSession::new(ShopInventory::new(
            1,
            vec![ShopItem {
                item_id: 0x30,
                price: 120,
            }],
        )));
        runtime.ctx.state = MenuState::ShopBuy.as_byte();

        // Confirming the equipment row parks the list under the picker.
        runtime.tick(&mut world, cross());
        assert_eq!(runtime.ctx.state, MenuState::ShopBuy.as_byte());
        let session = runtime.recipient_session.as_ref().expect("picker open");
        assert_eq!(session.can_equip, vec![false, true]);

        // Row 2 = party member 1: buy and equip now. The displaced weapon
        // returns to the bag, the purse debits, the piece never enters it.
        runtime.tick(&mut world, MenuInput::default()); // Init frame
        runtime.tick(&mut world, down());
        runtime.tick(&mut world, down());
        // A confirm on row 1 (member 0, mask-rejected) would buzz and stay;
        // row 2 is the equippable member.
        runtime.tick(&mut world, cross());
        // One exit-beat frame drops back to the buy list (retail's
        // post-commit return).
        runtime.tick(&mut world, MenuInput::default());
        assert!(runtime.recipient_session.is_none(), "picker closed");
        assert_eq!(world.money, 180, "120 gold debited");
        assert_eq!(
            world.roster.members[1].equipment().slots
                [crate::equipment::EquipSlot::Weapon.as_index() as usize],
            0x30,
            "bought piece equipped directly"
        );
        assert_eq!(
            world.inventory.get(&7).copied(),
            Some(1),
            "displaced weapon returned to the bag"
        );
        assert_eq!(
            world.inventory.get(&0x30),
            None,
            "the purchase never entered the bag"
        );
    }

    #[test]
    fn recipient_row_zero_buys_one_copy_into_the_bag() {
        use crate::equipment::{DiscEquipEntry, DiscEquipInfo};
        use crate::shop::{ShopInventory, ShopItem, ShopSession};
        use legaia_asset::equip_stats::EquipSlot as Disc;

        let mut world = world_with_party(1);
        world.money = 200;
        let mut runtime = MenuRuntime::new("/tmp/legaia-test");
        runtime.retail_equipment_buy = true;
        runtime.install_equip_info(DiscEquipInfo::from_entries([(
            0x31,
            DiscEquipEntry {
                mask: 0b111,
                category: Disc::Body,
                is_ra_seru: false,
                passive_index: None,
            },
        )]));
        runtime.open_shop(ShopSession::new(ShopInventory::new(
            1,
            vec![ShopItem {
                item_id: 0x31,
                price: 60,
            }],
        )));
        runtime.ctx.state = MenuState::ShopBuy.as_byte();
        runtime.tick(&mut world, cross());
        assert!(runtime.recipient_session.is_some());
        // Row 0 (the bag) is the seeded cursor; confirm buys one copy.
        runtime.tick(&mut world, MenuInput::default()); // Init frame
        runtime.tick(&mut world, cross());
        assert_eq!(world.money, 140);
        assert_eq!(world.inventory.get(&0x31).copied(), Some(1));
        runtime.tick(&mut world, MenuInput::default()); // exit beat
        assert!(runtime.recipient_session.is_none());
    }

    #[test]
    fn equipment_buy_keeps_quantity_flow_while_retail_flow_is_off() {
        use crate::equipment::{DiscEquipEntry, DiscEquipInfo};
        use crate::shop::{ShopInventory, ShopItem, ShopSession};
        use legaia_asset::equip_stats::EquipSlot as Disc;

        let mut world = world_with_party(1);
        world.money = 500;
        let mut runtime = MenuRuntime::new("/tmp/legaia-test");
        // Equip info installed, but the retail flow not opted into: the
        // legacy quantity route must survive (the native window has no
        // picker surface yet).
        runtime.install_equip_info(DiscEquipInfo::from_entries([(
            0x30,
            DiscEquipEntry {
                mask: 0b111,
                category: Disc::Weapon,
                is_ra_seru: false,
                passive_index: None,
            },
        )]));
        runtime.open_shop(ShopSession::new(ShopInventory::new(
            1,
            vec![ShopItem {
                item_id: 0x30,
                price: 100,
            }],
        )));
        runtime.ctx.state = MenuState::ShopBuy.as_byte();
        runtime.tick(&mut world, cross());
        assert_eq!(runtime.ctx.state, MenuState::ShopQuantity.as_byte());
        assert!(runtime.recipient_session.is_none());
    }

    #[test]
    fn shop_triangle_from_list_tears_down_session_and_closes() {
        use crate::shop::{ShopInventory, ShopSession};

        let mut world = world_with_party(1);
        let mut runtime = MenuRuntime::new("/tmp/legaia-test");
        runtime.open_shop(ShopSession::new(ShopInventory::new(1, vec![])));
        runtime.ctx.state = MenuState::ShopBuy.as_byte();

        // Triangle from the buy list backs up to the top shop menu, and a second
        // Triangle leaves it via the ShopExit teardown screen.
        runtime.tick(&mut world, triangle());
        assert_eq!(runtime.ctx.state, MenuState::ShopMenu.as_byte());
        runtime.tick(&mut world, triangle());
        assert_eq!(runtime.ctx.state, MenuState::ShopExit.as_byte());
        // ShopExit fires its one-shot commit (clears the session) then holds.
        runtime.tick(&mut world, MenuInput::default());
        assert!(
            runtime.shop_session.is_none(),
            "session cleared on teardown"
        );
        // Holds, then closes.
        for _ in 0..8 {
            runtime.tick(&mut world, MenuInput::default());
        }
        assert_eq!(runtime.ctx.state, MenuState::Closing.as_byte());
    }

    #[test]
    fn inn_rest_drives_through_tick_restores_hp_and_charges_gold() {
        let mut world = world_with_party(1);
        world.money = 50;
        world.party_count = 1;
        world.actors[0].active = true;
        world.actors[0].battle.max_hp = 100;
        world.actors[0].battle.hp = 10;

        let mut runtime = MenuRuntime::new("/tmp/legaia-test");
        runtime.open_inn(10);
        runtime.ctx.state = MenuState::InnConfirm.as_byte();

        // InnConfirm (cursor 0 = yes) -> InnSleep, rest applied.
        runtime.tick(&mut world, cross());
        assert_eq!(runtime.ctx.state, MenuState::InnSleep.as_byte());
        assert_eq!(world.money, 40, "10 gold charged");
        assert_eq!(world.actors[0].battle.hp, 100, "HP restored to max");
        assert!(runtime.inn_session.is_none(), "inn session cleared");

        // Sleep fade holds, then closes.
        for _ in 0..8 {
            runtime.tick(&mut world, MenuInput::default());
        }
        assert_eq!(runtime.ctx.state, MenuState::Closing.as_byte());
    }

    #[test]
    fn inn_decline_closes_without_charging() {
        let mut world = world_with_party(1);
        world.money = 50;
        let mut runtime = MenuRuntime::new("/tmp/legaia-test");
        runtime.open_inn(10);
        runtime.ctx.state = MenuState::InnConfirm.as_byte();
        runtime.ctx.cursor = 1; // slot 1 = no

        runtime.tick(&mut world, cross());
        assert_eq!(runtime.ctx.state, MenuState::Closing.as_byte());
        assert_eq!(world.money, 50, "no gold charged on decline");
        assert!(runtime.inn_session.is_none(), "inn session cleared");
    }

    #[test]
    fn load_from_missing_slot_returns_error() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let runtime = MenuRuntime::new(tmp.path().to_path_buf());
        let mut world = world_with_party(3);
        let err = runtime.load_from_slot(&mut world, 99).unwrap_err();
        let msg = format!("{err:#}");
        assert!(
            msg.contains("read save slot") || msg.contains("No such file"),
            "unexpected error: {msg}"
        );
    }
}
