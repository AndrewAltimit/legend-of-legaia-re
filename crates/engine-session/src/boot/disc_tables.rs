//! The disc reads a session is built from: the `SCUS_942.54` tables (new-game
//! template, item / equipment / spell / shop data, the SFX bank), the dialog
//! font, and the arts-shout and battle CD-XA clip banks.

#[cfg(not(target_arch = "wasm32"))]
use super::SceneSource;
#[cfg(not(target_arch = "wasm32"))]
use std::path::Path;

/// Read + parse the new-game starting-party template from a boot source's
/// `SCUS_942.54`. Returns `None` (not an error) when the executable isn't
/// reachable or doesn't parse, so a boot never fails just because the seed
/// data is unavailable.
pub(super) fn read_starting_party(
    scus: &[u8],
) -> Option<legaia_game_tables::new_game::StartingParty> {
    legaia_game_tables::new_game::StartingParty::from_scus(scus)
}

/// Read + decode the new-game starting-inventory seed from a boot source's
/// `SCUS_942.54` (`FUN_80034A6C`). Returns `None` when the executable isn't
/// reachable or doesn't decode, so a boot never fails on missing seed data.
pub(super) fn read_starting_inventory(
    scus: &[u8],
) -> Option<legaia_game_tables::new_game::StartingInventory> {
    legaia_game_tables::new_game::StartingInventory::from_scus(scus)
}

/// Read the raw `SCUS_942.54` bytes from a boot source. Returns `None`
/// (not an error) when the executable isn't reachable.
#[cfg(not(target_arch = "wasm32"))]
pub(super) fn read_scus(source: &SceneSource<'_>) -> Option<Vec<u8>> {
    use legaia_engine_scene::vfs::Vfs;
    match source {
        SceneSource::Extracted(root) => legaia_engine_scene::vfs::DirVfs::new(*root)
            .ok()?
            .read("SCUS_942.54")
            .ok(),
        #[cfg(not(target_arch = "wasm32"))]
        SceneSource::Disc(path) => legaia_engine_scene::vfs::DiscVfs::open(path)
            .ok()?
            .read("SCUS_942.54")
            .ok(),
    }
}

/// Build the retail proportional dialog font straight from the boot source:
/// the 4bpp font TIM at [`legaia_font::FONT_TIM_PROT_DAT_OFFSET`] inside
/// `PROT.DAT` supplies the glyph bitmaps and `SCUS_942.54` the per-character
/// advance table (`0x80073F1C`).
///
/// This is the disc-only path - it needs no `extracted/font/` artifacts and no
/// save state, so a `--disc <image>` boot renders text on retail metrics
/// instead of the fixed-width placeholder. Returns `None` (never an error)
/// when either half is unreachable.
pub(super) fn read_dialog_font(
    index: &legaia_engine_scene::scene::ProtIndex,
    scus: &[u8],
) -> Option<legaia_font::Font> {
    let tim = index
        .prot_dat_raw_bytes(
            legaia_font::FONT_TIM_PROT_DAT_OFFSET,
            legaia_font::FONT_TIM_LEN,
        )
        .ok()?;
    let font = legaia_font::Font::from_disc_tim_and_scus(&tim, scus).ok()?;
    Some(attach_escape_icons(font, index, scus))
}

/// Attach the `0xCE` escape sprites (controller buttons, icons) to `font`
/// from the boot-resident TIMs at the head of `PROT.DAT` and the SCUS sprite
/// records, so every shared text layout draws them
/// ([`legaia_font::Font::with_escape_icons`]). Unchanged when unreachable.
pub fn attach_escape_icons(
    font: legaia_font::Font,
    index: &legaia_engine_scene::scene::ProtIndex,
    scus: &[u8],
) -> legaia_font::Font {
    use legaia_font::escape_icons::{ICON_PROT_DAT_LEN, ICON_PROT_DAT_OFFSET};
    match index.prot_dat_raw_bytes(ICON_PROT_DAT_OFFSET, ICON_PROT_DAT_LEN) {
        Ok(head) => font.with_escape_icons_from_disc(&head, scus),
        Err(_) => font,
    }
}

/// Read + decode the sound-effect descriptor bank from a boot source's
/// `SCUS_942.54` (`DAT_8006F198`, see `sfx-table.md`). Returns `None` when the
/// executable isn't reachable or the table doesn't decode, so a boot never
/// fails on missing SFX data - the director just keeps its empty bank and
/// resolved cues no-op until one is staged.
pub(super) fn read_sfx_bank(scus: &[u8]) -> Option<(legaia_engine_audio::SfxBank, Vec<(u8, u8)>)> {
    let table = legaia_game_tables::sfx_table::SfxTable::from_scus(scus)?;
    let bank = legaia_engine_audio::SfxBank::from_descriptors(
        table
            .active()
            .map(|(id, d)| (id, d.program, d.tone, d.note, d.flags)),
    );
    // The routing half of the same table: which VAB slot each cue keys.
    Some((bank, table.cue_slots().collect()))
}

/// Demux + decode the battle **arts-voice shout** banks from a disc image:
/// the per-character CD-XA clip files (`XA2.XA` Vahn / `XA4.XA` Noa /
/// `XA6.XA` Gala, 16-channel short-mono banks) plus the `SCUS_942.54`
/// cue tables that map each art's action constant to its candidate-channel
/// pool (`legaia_art::arts_voice`, the `FUN_8004C140` tables).
///
/// Channel demux needs the raw 2352-byte sectors (the CD-XA subheaders carry
/// the channel number; a 2048-byte ISO view strips them), so this reads the
/// disc through [`legaia_iso::raw::RawDisc`] - extracted-directory boots
/// can't stage a shout bank. Returns `None` when the disc / executable /
/// tables don't resolve; the caller degrades to silent arts.
///
/// The per-file staging is [`crate::xa_banks::install_shout_file`], the call
/// the browser page stages the same files through. Public so disc-gated tests
/// can build the same bank the boot path stages.
#[cfg(not(target_arch = "wasm32"))]
pub fn read_arts_shout_bank(disc: &Path) -> Option<legaia_engine_audio::ArtsShoutBank> {
    let scus = read_scus(&SceneSource::Disc(disc))?;
    let table = legaia_art::arts_voice::ArtsVoiceTable::parse_from_scus(&scus)?;
    let mut raw = legaia_iso::raw::RawDisc::open(disc).ok()?;
    let volume = legaia_iso::iso9660::read_volume(&mut raw).ok()?;
    let files = legaia_iso::iso9660::walk_files(&mut raw, &volume.root).ok()?;
    let mut bank = legaia_engine_audio::ArtsShoutBank::new();
    for (cslot, name) in crate::xa_banks::shout_files() {
        let rec = find_disc_file(&files, name)?;
        let sectors = rec.size.div_ceil(legaia_iso::raw::USER_DATA_SIZE as u32);
        let streams = legaia_xa::demux::demux_disc_range(&mut raw, rec.lba, sectors).ok()?;
        crate::xa_banks::install_shout_file(&mut bank, cslot, &streams, Some(&table));
    }
    bank.has_clips().then_some(bank)
}

/// The directory record of the ISO file named `name` (`XA/XA2.XA;1` matches
/// `XA2.XA`).
#[cfg(not(target_arch = "wasm32"))]
fn find_disc_file<'a>(
    files: &'a [(String, legaia_iso::iso9660::DirectoryRecord)],
    name: &str,
) -> Option<&'a legaia_iso::iso9660::DirectoryRecord> {
    files
        .iter()
        .find_map(|(path, rec)| (crate::xa_banks::file_key(path) == name).then_some(rec))
}

pub use crate::xa_banks::BATTLE_XA_CLIP_SLOTS;

/// Demux + decode the battle **one-shot clip** banks from a disc image into
/// a generic `(clip_slot, channel)` bank: the files in
/// [`BATTLE_XA_CLIP_SLOTS`], staged through
/// [`crate::xa_banks::install_clip_file`] (the browser page stages the same
/// files through the same call). Same disc-only caveat as
/// [`read_arts_shout_bank`]. `None` when nothing decodes.
#[cfg(not(target_arch = "wasm32"))]
pub fn read_battle_xa_clip_bank(disc: &Path) -> Option<legaia_engine_audio::XaClipBank> {
    let mut raw = legaia_iso::raw::RawDisc::open(disc).ok()?;
    let volume = legaia_iso::iso9660::read_volume(&mut raw).ok()?;
    let files = legaia_iso::iso9660::walk_files(&mut raw, &volume.root).ok()?;
    let mut bank = legaia_engine_audio::XaClipBank::new();
    for &(slot, name) in BATTLE_XA_CLIP_SLOTS {
        let Some(rec) = find_disc_file(&files, name) else {
            continue;
        };
        let sectors = rec.size.div_ceil(legaia_iso::raw::USER_DATA_SIZE as u32);
        let Ok(streams) = legaia_xa::demux::demux_disc_range(&mut raw, rec.lba, sectors) else {
            continue;
        };
        crate::xa_banks::install_clip_file(&mut bank, slot, &streams);
    }
    bank.has_clips().then_some(bank)
}

/// Read the gold-shop item data (per-id buy price + "names a real item" mask)
/// from a boot source's `SCUS_942.54` item table. Returns `None` when the
/// executable isn't reachable or its item table doesn't parse, so a boot never
/// fails on missing shop data - the engine then leaves shop stock host-supplied
/// and unpriced. See [`legaia_engine_menus::shop_catalog`].
pub(super) fn read_shop_item_data(
    scus: &[u8],
) -> Option<legaia_engine_menus::shop_catalog::ShopItemData> {
    legaia_engine_menus::shop_catalog::ShopItemData::from_scus(scus)
}

/// Read + parse the static item-effect descriptor table (`DAT_800752C0`, see
/// `item-effect-table.md`) from a boot source's `SCUS_942.54`. Returns `None`
/// when the executable isn't reachable or the table doesn't parse, so a boot
/// never fails on missing item-effect data - the engine then keeps the curated
/// usability flags on its item catalog.
pub(super) fn read_retail_item_effects(
    scus: &[u8],
) -> Option<legaia_game_tables::item_effect::ItemEffectTable> {
    legaia_game_tables::item_effect::ItemEffectTable::from_scus(scus)
}

/// Read the static equipment stat-bonus table (`DAT_80074F68`, see
/// `equipment-table.md`) from a boot source's `SCUS_942.54` and build both the
/// disc-accurate equipment modifier table (stat bonuses keyed by real item ids)
/// and the per-item equip restrictions (character mask `+6` + slot category
/// `+7`) from the single parse. Returns `None` when the executable isn't
/// reachable or the table doesn't parse, so a boot never fails on missing
/// equipment data - the engine then falls back to the (fabricated-id) vanilla
/// equipment catalog.
pub(super) fn read_retail_equip_tables(
    scus: &[u8],
) -> Option<(
    legaia_engine_battle::battle_stats::EquipmentTable,
    legaia_engine_menus::equipment::DiscEquipInfo,
    legaia_game_tables::equip_stats::EquipStatTable,
)> {
    let table = legaia_game_tables::equip_stats::EquipStatTable::from_scus(scus)?;
    let modifiers = legaia_engine_menus::equipment::equip_modifier_table_from_disc(&table);
    let mut restrictions = legaia_engine_menus::equipment::DiscEquipInfo::from_disc(&table);
    // The Goods candidate lists are class-2 ids the equipment stat table does
    // not contain, so their index comes from the item-effect table instead -
    // without it the equip screen's three Goods rows browse an empty list.
    if let Some(effects) = legaia_game_tables::item_effect::ItemEffectTable::from_scus(scus) {
        restrictions.install_goods(&effects);
    }
    // The raw records travel too: the Throw Out list builder reads each
    // record's `+7` flags byte, which neither derived table keeps.
    Some((modifiers, restrictions, table))
}

/// Read the player Seru-magic catalog (MP cost + target shape from the spell
/// table, see `spell-table.md`) from a boot source's `SCUS_942.54`. Returns
/// `None` when the executable isn't reachable or doesn't parse, so a boot falls
/// back to the pinned `retail_seru_magic_catalog`.
pub(super) fn read_retail_spell_catalog(
    scus: &[u8],
) -> Option<legaia_engine_battle::spells::SpellCatalog> {
    legaia_engine_battle::retail_magic::seru_magic_catalog_from_scus(scus)
}
