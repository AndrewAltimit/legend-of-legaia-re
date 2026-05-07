//! Engine-side menu runtime — wires
//! [`legaia_engine_vm::menu::MenuCtx`] / [`legaia_engine_vm::menu::step`] to
//! a [`crate::world::World`] and to disk-backed save / load slots.
//!
//! [`MenuRuntime`] owns the menu ctx, a save-slot directory, and a small
//! flag block driven by [`step`](legaia_engine_vm::menu::step) callbacks.
//! Engines call [`MenuRuntime::tick`] each frame with a [`MenuInput`]; the
//! runtime advances the state machine, captures save bytes when the menu
//! commits at `SavePickSlot`, writes them to a file, and on `LoadSlot`
//! commit reads a file back into the world.
//!
//! Rendering is engine-side (see `asset-viewer` or any custom shell) — the
//! runtime exposes a [`MenuRuntime::current_label`] string per state so the
//! HUD overlay has something to render even before the per-screen layouts
//! land.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use legaia_engine_vm::menu::{MenuCtx, MenuHost, MenuInput, MenuState, open, step};
use legaia_save::{EquipmentSlots, Party, SpellList};

use crate::inn::InnSession;
use crate::shop::ShopSession;
use crate::world::World;

/// File extension the runtime uses for save slots. PSX memory-card `.mcr`
/// support is layered on top of [`legaia_save::card`]; this runtime uses a
/// flat `<dir>/slot_NN.bin` shape for development convenience.
pub const SAVE_EXT: &str = "bin";

/// One menu-driven tick outcome — engines log / observe / react.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuTickEvent {
    /// Menu ticked normally — no slot operation requested this frame.
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
    /// Number of save slots the picker offers (default 3 — one per save
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
            pending: None,
        }
    }

    /// Install a shop session and prepare for `ShopBuy` entry. Engines call
    /// this when the field VM triggers a shop transition.
    pub fn open_shop(&mut self, session: ShopSession) {
        self.shop_session = Some(session);
    }

    /// Install an inn session and prepare for `InnConfirm` entry. `cost` is
    /// the gold required for a rest at this inn.
    pub fn open_inn(&mut self, cost: u32) {
        self.inn_session = Some(InnSession::new(cost));
    }

    /// Open the menu (entry-point — typically called when the field VM
    /// requests menu via op `0x4C` sub-1).
    pub fn open(&mut self) {
        open(&mut self.ctx);
    }

    /// `true` while the menu is active (ctx state != Closed).
    pub fn is_open(&self) -> bool {
        self.ctx.state != MenuState::Closed.as_byte()
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
    /// `inventory` alongside the party records — use [`MenuRuntime::load_from_slot`]
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

    /// Engine-friendly label per active state — drives a HUD banner so the
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
            Some(MenuState::SavePickSlot) => "SAVE — PICK SLOT",
            Some(MenuState::SaveConfirmOverwrite) => "SAVE — OVERWRITE?",
            Some(MenuState::SaveWriting) => "SAVING…",
            Some(MenuState::SaveDone) => "SAVED",
            Some(MenuState::LoadSlot) => "LOAD — PICK SLOT",
            Some(MenuState::LoadProgress) => "LOADING…",
            Some(MenuState::ShopBuy) => "SHOP — BUY",
            Some(MenuState::ShopSell) => "SHOP — SELL",
            Some(MenuState::ShopQuantity) => "SHOP — HOW MANY?",
            Some(MenuState::ShopConfirm) => "SHOP — CONFIRM",
            Some(MenuState::ShopExit) => "SHOP — DONE",
            Some(MenuState::InnConfirm) => "INN — REST?",
            Some(MenuState::InnSleep) => "INN — RESTING",
            Some(MenuState::ItemPickTarget) => "ITEM — TARGET",
            Some(MenuState::ItemApply) => "ITEM — APPLY",
            Some(MenuState::ItemDone) => "ITEM — DONE",
            Some(MenuState::Confirm) => "CONFIRM?",
            Some(MenuState::Closing) => "CLOSING",
            Some(MenuState::Deactivate) => "",
            None => "?",
        }
    }
}

struct MenuRuntimeHost<'a> {
    world: &'a mut World,
    slot_count: u8,
    pending: &'a mut Option<PendingOp>,
    selected_char: &'a mut usize,
    shop_session: &'a mut Option<ShopSession>,
    inn_session: &'a mut Option<InnSession>,
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
            MenuState::StatusEquipment => {
                let idx = *self.selected_char;
                if let Some(record) = self.world.roster.members.get_mut(idx) {
                    let mut equip = record.equipment();
                    if (slot as usize) < equip.slots.len() {
                        equip.slots[slot as usize] = 0;
                        record.set_equipment(equip);
                    }
                }
            }
            MenuState::StatusInventory => {
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
            // --- Shop states ---
            MenuState::ShopBuy => {
                if let Some(session) = self.shop_session.as_mut() {
                    session.select_buy_item(slot as usize);
                }
            }
            MenuState::ShopSell => {
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
            MenuState::ShopQuantity => {
                if let Some(session) = self.shop_session.as_mut() {
                    session.set_quantity(slot);
                }
            }
            // slot 0 = confirm; slot 1 = cancel (falls through to _ => {})
            MenuState::ShopConfirm if slot == 0 => {
                if let Some(session) = self.shop_session.as_ref() {
                    if session.pending_is_buying {
                        if let Some((item_id, qty, delta)) = session.try_buy(self.world.money) {
                            self.world.money = (self.world.money + delta).clamp(0, 9_999_999);
                            let count = self.world.inventory.entry(item_id).or_insert(0);
                            *count = count.saturating_add(qty);
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
            MenuState::ShopExit => {
                *self.shop_session = None;
            }
            // --- Inn states ---
            MenuState::InnConfirm => {
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
            _ => {}
        }
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
        assert_eq!(runtime.current_label(), "SAVE — PICK SLOT");
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
