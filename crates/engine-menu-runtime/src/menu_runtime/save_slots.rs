//! Save slots: slot paths, save / load to a slot or a memory card, and the path-level world save.

use super::*;
use legaia_engine_save_state::WorldSaveState;

impl MenuRuntime {
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
