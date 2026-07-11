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

use crate::inn::InnSession;
use crate::shop::ShopSession;
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
    /// Pending operation flagged by the host hooks; consumed inside
    /// [`MenuRuntime::tick`].
    pending: Option<PendingOp>,
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
            pending: None,
        }
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

    /// `true` while the menu is active (ctx state != Closed).
    pub fn is_open(&self) -> bool {
        self.ctx.state != MenuState::Closed.as_byte()
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
    pub fn pending_trade_offer(&self) -> Option<legaia_asset::seru_trade::TradeOffer> {
        self.trade_session
            .as_ref()
            .and_then(|t| t.offers.get(self.trade_pending_offer).copied())
    }

    /// Per-frame tick. Drives the menu VM; on `SavePickSlot` / `LoadSlot`
    /// commit, runs disk I/O and emits a [`MenuTickEvent`].
    pub fn tick(&mut self, world: &mut World, input: MenuInput) -> MenuTickEvent {
        let mut host = MenuRuntimeHost {
            world,
            slot_count: self.slot_count,
            pending: &mut self.pending,
            selected_char: &mut self.selected_char,
            shop_session: &mut self.shop_session,
            inn_session: &mut self.inn_session,
            trade_session: &mut self.trade_session,
            trade_pending_offer: &mut self.trade_pending_offer,
        };
        step(&mut host, &mut self.ctx, input);

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
}

impl MenuRuntimeHost<'_> {
    /// Row actions for the current `ShopMenu` (Trade present iff trading on).
    fn shop_menu_rows(&self) -> &'static [MenuState] {
        shop_menu_rows(self.world.seru_trade_enabled())
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
    fn commit_shop_confirm(&mut self) {
        if let Some(session) = self.shop_session.as_ref() {
            if session.pending_is_buying {
                // Shared grant kernel (also driven by the shop / casino
                // randomizer runtime oracles).
                self.world.buy_from_shop(session);
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
            MenuState::ShopBuy => {
                if let Some(session) = self.shop_session.as_mut() {
                    session.select_buy_item(slot as usize);
                }
            }
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

        // A shop on a disc with seru trading enabled; the lead owns two seru.
        let mut world = World::new();
        world.seru_trade_config = Some(legaia_asset::seru_trade::SeruTradeConfig {
            enabled: true,
            seed: 0xABCD,
            max_offers: 4,
        });
        let mut lead = CharacterRecord::zeroed();
        let mut list = SpellList::default();
        list.ids[0] = 0x81;
        list.ids[1] = 0x88;
        list.count = 2;
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

        // The owner's spell list now holds the received seru, not the given one.
        let list = world.roster.members[offer.give.owner_slot as usize].spell_list();
        let ids = &list.ids[..list.count as usize];
        assert!(ids.contains(&offer.receive_seru_id), "received seru added");
        assert!(!ids.contains(&offer.give.seru_id), "given seru removed");
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
