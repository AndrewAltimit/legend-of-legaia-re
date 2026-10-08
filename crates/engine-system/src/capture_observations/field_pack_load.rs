//! Capture observation: field-pack loader destination pointer and asset-descriptor table.

/// `_DAT_8007B8D0` - the loader writes `field_pack_base + 0x12800`
/// to this cell after asset load completes. Read this value and
/// subtract `0x12800` to recover the active per-scene base.
pub const LOAD_DEST_PLUS_OFFSET_PTR: u32 = 0x8007B8D0;

/// Offset added to the heap-allocated buffer pointer to compute the
/// effect-data load destination. The loader stores
/// `(buffer_ptr + EFFECT_OFFSET)` in [`LOAD_DEST_PLUS_OFFSET_PTR`].
pub const EFFECT_OFFSET: u32 = 0x12800;

/// Static asset descriptor table base. Identical across all captured
/// saves; `FUN_80020224` walks this table after the field-pack load.
pub const ASSET_DESCRIPTOR_TABLE_PTR_ADDR: u32 = 0x8007B85C;

/// Pinned value of `_DAT_8007B85C` across the captured corpus.
pub const ASSET_DESCRIPTOR_TABLE_PTR_VALUE: u32 = 0x8015CBD0;

/// Scratchpad cell that holds the heap-resident scene asset buffer
/// pointer. The loader reads this every transition.
pub const SCRATCHPAD_BUFFER_PTR: u32 = 0x1F8003EC;

/// Address of the static scene asset loader.
pub const SCENE_ASSET_LOADER_ADDR: u32 = 0x8001F7C0;

/// Address of the descriptor-pair walker.
pub const DESCRIPTOR_WALKER_ADDR: u32 = 0x80020224;

/// Address of the asset-type dispatcher.
pub const ASSET_TYPE_DISPATCHER_ADDR: u32 = 0x8001F05C;

/// Overlay-resident scene-transition orchestrator.
pub const SCENE_TRANSITION_ORCHESTRATOR_ADDR: u32 = 0x801D6704;

/// Static scene-transition setup function (writes the new scene
/// name into the scene-name table + flips `_DAT_1F800394 |= 0x40`).
pub const SCENE_TRANSITION_SETUP_ADDR: u32 = 0x8001FD44;

/// Scene-transition pending bit set by `FUN_8001FD44` in
/// `_DAT_1F800394`.
pub const SCENE_TRANSITION_PENDING_BIT: u32 = 0x40;

/// Field-pack RAM base for the `town01` (intro Rim Elm) save.
/// = `0x8014BD30 - 0x12800`.
pub const TOWN01_FIELD_PACK_BASE: u32 = 0x80139530;

/// Field-pack RAM base for the `town0c` (Rim Elm Genesis Tree)
/// save. = `0x800B4DF0 - 0x12800`.
pub const TOWN0C_FIELD_PACK_BASE: u32 = 0x800A25F0;

/// Recover the active per-scene field-pack RAM base from a save's
/// main-RAM image. Returns `None` if the load-dest pointer reads
/// zero (no scene loaded yet) or below `EFFECT_OFFSET`.
pub fn recover_base(main_ram: &[u8]) -> Option<u32> {
    let off = (LOAD_DEST_PLUS_OFFSET_PTR - 0x80000000) as usize;
    let bytes = main_ram.get(off..off + 4)?;
    let raw = u32::from_le_bytes(bytes.try_into().ok()?);
    if raw < EFFECT_OFFSET || !(0x80000000..0x80200000).contains(&raw) {
        return None;
    }
    Some(raw - EFFECT_OFFSET)
}
