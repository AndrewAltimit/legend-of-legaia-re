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
use legaia_save::Party;

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
            pending: None,
        }
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

    /// Serialise the world's party to slot `slot` on disk.
    pub fn save_to_slot(&self, world: &mut World, slot: u8) -> Result<PathBuf> {
        std::fs::create_dir_all(&self.save_dir)
            .with_context(|| format!("create save dir {}", self.save_dir.display()))?;
        let path = self.slot_path(slot);
        let bytes = world.save_party().write();
        std::fs::write(&path, &bytes)
            .with_context(|| format!("write save slot {} to {}", slot, path.display()))?;
        Ok(path)
    }

    /// Load slot `slot` from disk into the world's roster.
    pub fn load_from_slot(&self, world: &mut World, slot: u8) -> Result<PathBuf> {
        let path = self.slot_path(slot);
        let bytes = std::fs::read(&path)
            .with_context(|| format!("read save slot {} from {}", slot, path.display()))?;
        let party = Party::parse(&bytes)
            .with_context(|| format!("parse save slot {} ({} bytes)", slot, bytes.len()))?;
        world.load_party(party);
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
}

impl<'a> MenuHost for MenuRuntimeHost<'a> {
    fn screen_item_count(&self, state: MenuState) -> u8 {
        match state {
            MenuState::StatusTop => 8, // Character / Equip / Items / Magic / Arts / Config / Save / Load
            MenuState::SavePickSlot | MenuState::LoadSlot => self.slot_count.max(1),
            MenuState::ShopBuy | MenuState::ShopSell => 8,
            MenuState::StatusInventory => 16,
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
            _ => {}
        }
        let _ = self.world;
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
    use legaia_save::CharacterRecord;

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
