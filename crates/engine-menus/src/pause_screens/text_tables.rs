//! Disc-derived menu text: item / spell / accessory names and descriptions.

/// Disc-derived pause-menu text tables (best-effort per table; every
/// lookup has a caller-side fallback so a PROT.DAT-only load still
/// renders ids).
#[derive(Debug, Clone, Default)]
pub struct MenuTextTables {
    /// Item names + info-window descriptions.
    pub item_names: Option<legaia_game_tables::item_names::ItemNameTable>,
    /// Spell names / MP / info-window descriptions.
    pub spell_names: Option<legaia_game_tables::spell_names::SpellNameTable>,
    /// Accessory ("Goods") passive name/description records - the green +
    /// white lines of the item info window's extra widget box.
    pub passives: Option<legaia_game_tables::accessory_passive::AccessoryPassiveTable>,
    /// The arts-name table (`DAT_80075EC4`, [`legaia_art::arts_table`]) - what
    /// the `0xC5` markup token resolves against, keyed on `[character, art]`.
    pub arts: Option<Vec<legaia_art::arts_table::ArtTableEntry>>,
    /// Each arts-table record's description string, parallel to [`Self::arts`].
    pub art_descriptions: Option<Vec<Vec<u8>>>,
    /// The post-battle level-up window's seven lines, raw MES bytes, index
    /// `mask - 1` (screen elements `0x45..=0x4B`,
    /// [`legaia_ui_formats::screen_elements::RECORD_LEVEL_UP_BASE`]).
    pub level_up_lines: Option<Vec<Vec<u8>>>,
    /// The report window's drop line template, raw MES bytes
    /// ([`legaia_ui_formats::screen_elements::drop_line_template`]).
    pub drop_line: Option<Vec<u8>>,
    /// The item-detail printer's tables (`FUN_800337B0`): raw descriptions,
    /// the equipment slot templates and stat labels, per-id kind and bonus
    /// record ([`crate::shop::ItemDetailTables`]).
    pub item_detail: Option<crate::shop::ItemDetailTables>,
}

impl MenuTextTables {
    /// Parse all three tables out of a `SCUS_942.54` image (each
    /// best-effort).
    pub fn from_scus(scus: &[u8]) -> Self {
        Self {
            item_names: legaia_game_tables::item_names::ItemNameTable::from_scus(scus),
            spell_names: legaia_game_tables::spell_names::SpellNameTable::from_scus(scus),
            passives: legaia_game_tables::accessory_passive::AccessoryPassiveTable::from_scus(scus),
            arts: legaia_art::arts_table::parse_from_scus(scus),
            art_descriptions: legaia_art::arts_table::raw_descriptions_from_scus(scus),
            level_up_lines: (1..=7)
                .map(|mask| {
                    legaia_ui_formats::screen_elements::payload_string(
                        scus,
                        legaia_ui_formats::screen_elements::RECORD_LEVEL_UP_BASE + mask,
                    )
                })
                .collect(),
            drop_line: legaia_ui_formats::screen_elements::drop_line_template(scus),
            item_detail: crate::shop::ItemDetailTables::from_scus(scus),
        }
    }

    /// The arts name the `0xC5` token resolves for `[character, art]` - the
    /// table record whose row is the character and whose column is the art.
    pub fn art_name(&self, character: u8, art: u8) -> Option<&str> {
        self.arts
            .as_ref()?
            .iter()
            .find(|e| e.character as u8 == character && e.index == art)
            .map(|e| e.name.as_str())
    }

    /// Display name for item `id`, or `None`.
    pub fn item_name(&self, id: u8) -> Option<&str> {
        self.item_names.as_ref()?.name(id)
    }

    /// Info-window description for item `id`, or `None`.
    pub fn item_desc(&self, id: u8) -> Option<&str> {
        self.item_names.as_ref()?.desc(id)
    }

    /// Display name for spell `id`, or `None`.
    pub fn spell_name(&self, id: u8) -> Option<&str> {
        self.spell_names.as_ref()?.name(id)
    }

    /// Info-window description for spell `id`, or `None`.
    pub fn spell_desc(&self, id: u8) -> Option<&str> {
        self.spell_names.as_ref()?.desc(id)
    }

    /// The accessory passive lines for item `id`: `(green name line,
    /// white description)` from the `0x8007625C` record's `+4` / `+8`
    /// strings. The description is whole, its `'|'` breaks kept: every
    /// window that prints it (`FUN_801D0F1C`'s extra box, the shop's info and
    /// detail windows) goes through the line-breaking printer
    /// (`FUN_80036888`), which puts the rest one row pitch down. This used to
    /// keep the first line only, on the reading that the box shows one line.
    pub fn item_passive_text(&self, id: u8) -> Option<(String, String)> {
        let (_, record) = self.passives.as_ref()?.passive(id)?;
        let name = record.name.clone()?;
        Some((name, record.description.clone().unwrap_or_default()))
    }
}

/// One item id's display text, as every screen that shows an item needs it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ItemDisplayText {
    /// Display name. Falls back to the curated catalog, then to a raw id,
    /// so a screen never shows nothing.
    pub name: String,
    /// Info-panel description. Empty when the disc text is unavailable.
    pub desc: String,
    /// An accessory's two passive lines (`(name, description)`).
    pub passive: Option<(String, String)>,
    /// What the item-detail printer `FUN_800337B0` prints at the description
    /// pen, when the disc tables are up: the description (or undescribed
    /// equipment's slot template) with name escapes expanded, and equipment's
    /// stat line as `(label, label dx, value dx, value)`.
    pub detail: Option<ItemDetailText>,
    /// The scope icon the info panel draws beside a passive-bearing
    /// consumable's green line, as a font sprite cell ([`item_scope_icon`]).
    pub scope_icon: Option<u16>,
}

/// The scope icon `FUN_801D0F1C` draws beside a kind-`2` item's passive name
/// (`0x801D111C..0x801D11B8`), as a font sprite cell: icon `0x85`
/// (`ITEM_SCOPE_ALL`) when the effect descriptor's `+2` flags carry `0x08` and
/// its `+0` class is non-zero, or carry `0x20` without `0x08`; icon `0x84`
/// (`ITEM_SCOPE_ONE`) otherwise. The panel asks only when the item grants a
/// passive (index below `0x40`).
///
/// PORT: FUN_801d0f1c (`0x801D111C..0x801D11B8`, the scope pictogram)
pub fn item_scope_icon(effect: &legaia_game_tables::item_effect::ItemEffect) -> u16 {
    use legaia_font::sprite_cell_ids::{ITEM_SCOPE_ALL, ITEM_SCOPE_ONE};
    let all = if effect.flags & 0x08 != 0 {
        effect.class != 0
    } else {
        effect.flags & 0x20 != 0
    };
    if all { ITEM_SCOPE_ALL } else { ITEM_SCOPE_ONE }
}

/// One item's [`ItemDisplayText::detail`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ItemDetailText {
    pub text: Vec<u8>,
    pub stats: Vec<(String, i32, i32, u8)>,
}
