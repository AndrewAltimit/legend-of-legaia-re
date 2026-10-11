//! The disc reads a session is built from: the `SCUS_942.54` tables (new-game
//! template, item / equipment / spell / shop data, the SFX bank), the dialog
//! font, and the arts-shout and battle CD-XA clip banks.

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

pub use crate::xa_banks::BATTLE_XA_CLIP_SLOTS;

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
