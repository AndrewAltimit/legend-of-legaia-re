//! Retail pause-menu **Items** / **Magic** screen sessions + view models.
//!
//! The draw builders live in `legaia-engine-ui`
//! (`ui_menu/pause_lists.rs`: `items_screen_draws_for` /
//! `magic_screen_draws_for`); this module is the renderer-agnostic data
//! side both hosts (play-window + the web play page) feed them from:
//!
//! - [`MenuTextTables`] - the disc-derived text: item names + info-window
//!   descriptions (`PTR_DAT_8007436C`, `docs/formats/item-table.md`),
//!   spell names / MP / descriptions (`DAT_800754C8` + the `0x80075DB0`
//!   description pointer table, `docs/formats/spell-table.md`) and the
//!   accessory passive name/description table (`0x8007625C`,
//!   `docs/formats/accessory-passive-table.md`).
//! - [`PauseItemsSession`] - the retail Items screen's focus model
//!   (command window -> list) layered over the item-use flow
//!   ([`crate::inventory_use::InventoryUseSession`]), with real bag
//!   counts and 12-row list paging.
//! - [`items_screen_model`] / [`magic_screen_model`] - owned view models
//!   the hosts map 1:1 onto the engine-ui `PauseItemsView` /
//!   `PauseMagicView` structs.
//!
//! Retail provenance for the layouts + phase words is in
//! `docs/subsystems/field-menu.md` (`FUN_801D0D18` command window,
//! `FUN_801DCB60`/`FUN_801D0F1C` item info, `FUN_801D2C98` caster window,
//! `FUN_801D2E74` spell info).

use crate::input::PadButton;
use crate::inventory_use::{InventoryUseInput, InventoryUseSession, InventoryUseState};
use crate::spell_menu::{SpellMenuPhase, SpellMenuSession};
use legaia_engine_vm::battle_formulas::{MpCostModifier, mp_cost_after_ability_bits};

/// Rows per list page (both retail list windows show 12 rows filling the
/// 182-px content height at the 0xE pitch).
pub const LIST_PAGE_ROWS: usize = 12;

/// Default bag capacity backing the Items list's page count. The retail
/// header reads `PAGE 1 / 6` on the catalogued capture - six 12-row pages
/// = 72 bag slots (the `0x80085958 + i*2` slot array scanned over
/// `_DAT_8007B5EA.._DAT_8007B5EC`).
pub const DEFAULT_BAG_PAGES: u16 = 6;

/// Ra-Seru summon spell-id block (`Palma`..`Ozma`, the egg-derived
/// summons): these rows lead with the wider winged element icon in the
/// spell list. See `docs/formats/spell-table.md`.
pub const RA_SERU_SPELL_IDS: std::ops::RangeInclusive<u8> = 0x9A..=0xA0;

/// Disc-derived pause-menu text tables (best-effort per table; every
/// lookup has a caller-side fallback so a PROT.DAT-only load still
/// renders ids).
#[derive(Debug, Clone, Default)]
pub struct MenuTextTables {
    /// Item names + info-window descriptions.
    pub item_names: Option<legaia_asset::item_names::ItemNameTable>,
    /// Spell names / MP / info-window descriptions.
    pub spell_names: Option<legaia_asset::spell_names::SpellNameTable>,
    /// Accessory ("Goods") passive name/description records - the green +
    /// white lines of the item info window's extra widget box.
    pub passives: Option<legaia_asset::accessory_passive::AccessoryPassiveTable>,
}

impl MenuTextTables {
    /// Parse all three tables out of a `SCUS_942.54` image (each
    /// best-effort).
    pub fn from_scus(scus: &[u8]) -> Self {
        Self {
            item_names: legaia_asset::item_names::ItemNameTable::from_scus(scus),
            spell_names: legaia_asset::spell_names::SpellNameTable::from_scus(scus),
            passives: legaia_asset::accessory_passive::AccessoryPassiveTable::from_scus(scus),
        }
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
    /// white description line)` - what `FUN_801D0F1C` draws in the extra
    /// widget box from the `0x8007625C` record's `+4` / `+8` strings.
    pub fn item_passive_lines(&self, id: u8) -> Option<(String, String)> {
        let (_, record) = self.passives.as_ref()?.passive(id)?;
        let name = record.name.clone()?;
        // The white line is the description's first line (the retail `|`
        // break maps below the box; the box shows one line per row).
        let desc = record
            .description
            .clone()
            .map(|d| d.split('|').next().unwrap_or_default().trim().to_string())
            .unwrap_or_default();
        Some((name, desc))
    }
}

/// One bag row of the Items screen, resolved at session build.
#[derive(Debug, Clone, Default)]
pub struct PauseItemRow {
    pub id: u8,
    pub name: String,
    /// Real bag count (the world inventory count, not the session's
    /// one-entry-per-id item list length).
    pub count: u8,
    /// Info-window description (empty when the disc text is unavailable).
    pub desc: String,
    /// Accessory passive lines for the extra widget box.
    pub passive: Option<(String, String)>,
}

/// Focus of the Items screen (the retail submenu word `DAT_801E46A4`:
/// `5` = command window, `6` = list).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PauseItemsFocus {
    /// Hand on the Use / Throw Out / Arrange command window.
    Command,
    /// Hand inside the item list.
    List,
}

/// The retail Items screen session: the command-window/list focus model
/// layered over the item-use flow. The inner
/// [`InventoryUseSession`] stays the behaviour driver (admissibility
/// filter, target select, outcome) - hosts keep applying its outcome via
/// [`crate::field_menu_dispatch::apply_inventory_outcome`] with
/// [`Self::inner`].
pub struct PauseItemsSession {
    /// The item-use flow. Its `items` list is id-sorted, one entry per
    /// distinct bag id, parallel to [`Self::rows`]. NB its browsing
    /// cursor walks `filtered_items` (usable-in-context rows only);
    /// retail's list hand walks **every** bag row, so the screen keeps
    /// its own flat [`Self::list_cursor`] and only maps into the inner
    /// flow on a confirm.
    pub inner: InventoryUseSession,
    /// Resolved per-row display data (parallel to `inner.items`).
    pub rows: Vec<PauseItemRow>,
    pub focus: PauseItemsFocus,
    /// Command-window row (0 = Use, 1 = Throw Out, 2 = Arrange).
    pub command_cursor: u8,
    /// Flat hand position over [`Self::rows`] (all bag rows).
    cursor: usize,
    /// Set when the player backs out of the command window (Circle /
    /// Triangle) - the screen is finished without an item use.
    closed: bool,
}

impl PauseItemsSession {
    pub fn new(inner: InventoryUseSession, rows: Vec<PauseItemRow>) -> Self {
        Self {
            inner,
            rows,
            focus: PauseItemsFocus::Command,
            command_cursor: 0,
            cursor: 0,
            closed: false,
        }
    }

    /// The retail command-window grey-out: the bag scan found no held
    /// item.
    pub fn bag_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// Flat hand position over the full bag list (every row, not just
    /// the context-usable ones - the hand can rest on a non-usable row;
    /// confirming it buzzes, matching retail).
    pub fn list_cursor(&self) -> usize {
        self.cursor
    }

    /// 1-based current page of the list.
    pub fn page(&self) -> u16 {
        (self.list_cursor() / LIST_PAGE_ROWS) as u16 + 1
    }

    /// Total page count: the fixed bag capacity's page count (the retail
    /// header shows the bag's page total, not the held-item count).
    pub fn pages(&self) -> u16 {
        DEFAULT_BAG_PAGES.max(self.rows.len().div_ceil(LIST_PAGE_ROWS).max(1) as u16)
    }

    /// `true` while the item-use flow is in its target-select phase (the
    /// host overlays the target picker).
    pub fn target_select(&self) -> bool {
        matches!(self.inner.state, InventoryUseState::TargetSelect { .. })
    }

    /// Session finished (backed out of the command window, or the inner
    /// use flow reached `Done`).
    pub fn is_done(&self) -> bool {
        self.closed || self.inner.is_done()
    }

    /// Drive one frame from an edge-triggered PSX pad word.
    ///
    /// - **Command focus**: Up/Down cycle the three rows; Cross on "Use"
    ///   moves the hand into the list (bag permitting); Circle/Triangle
    ///   close the screen. Throw Out / Arrange are not yet modelled
    ///   (retail sub-flows; held).
    /// - **List focus**: Up/Down move the hand, Left/Right flip 12-row
    ///   pages, Cross confirms into the use flow, Circle returns the hand
    ///   to the command window.
    /// - **Target select**: everything forwards to the inner flow.
    pub fn input_pad_edge(&mut self, pressed: u16) {
        let up = pressed & PadButton::Up.mask() != 0;
        let down = pressed & PadButton::Down.mask() != 0;
        let left = pressed & PadButton::Left.mask() != 0;
        let right = pressed & PadButton::Right.mask() != 0;
        let cross = pressed & PadButton::Cross.mask() != 0;
        let circle = pressed & PadButton::Circle.mask() != 0;
        let triangle = pressed & PadButton::Triangle.mask() != 0;

        if self.target_select() {
            if let Some(ev) = simple_inventory_input(pressed) {
                self.inner.input(ev);
            }
            return;
        }
        match self.focus {
            PauseItemsFocus::Command => {
                if circle || triangle {
                    self.closed = true;
                    return;
                }
                if up {
                    self.command_cursor = (self.command_cursor + 2) % 3;
                }
                if down {
                    self.command_cursor = (self.command_cursor + 1) % 3;
                }
                if cross && self.command_cursor == 0 && !self.bag_empty() {
                    self.focus = PauseItemsFocus::List;
                }
                // Throw Out (1) / Arrange (2): retail sub-flows not yet
                // modelled - the confirm is a no-op.
            }
            PauseItemsFocus::List => {
                if circle {
                    self.focus = PauseItemsFocus::Command;
                    return;
                }
                let n = self.rows.len();
                if n == 0 {
                    return;
                }
                if up {
                    self.cursor = if self.cursor == 0 {
                        n - 1
                    } else {
                        self.cursor - 1
                    };
                }
                if down {
                    self.cursor = if self.cursor + 1 >= n {
                        0
                    } else {
                        self.cursor + 1
                    };
                }
                if left {
                    // Page flip: jump the hand by one page, clamped.
                    self.cursor = self.cursor.saturating_sub(LIST_PAGE_ROWS);
                }
                if right {
                    self.cursor = (self.cursor + LIST_PAGE_ROWS).min(n - 1);
                }
                if cross {
                    // Map the hand row into the inner flow's filtered
                    // cursor space; a non-usable row has no mapping and
                    // the confirm is a buzz no-op (retail).
                    if let Some(fpos) = self
                        .inner
                        .filtered_items
                        .iter()
                        .position(|&ix| ix == self.cursor)
                    {
                        if let InventoryUseState::Browsing { cursor } = &mut self.inner.state {
                            *cursor = fpos;
                        }
                        self.inner.input(InventoryUseInput::Confirm);
                    }
                }
            }
        }
    }
}

fn simple_inventory_input(pressed: u16) -> Option<InventoryUseInput> {
    if pressed & PadButton::Up.mask() != 0 {
        Some(InventoryUseInput::Up)
    } else if pressed & PadButton::Down.mask() != 0 {
        Some(InventoryUseInput::Down)
    } else if pressed & PadButton::Cross.mask() != 0 {
        Some(InventoryUseInput::Confirm)
    } else if pressed & PadButton::Circle.mask() != 0 {
        Some(InventoryUseInput::Cancel)
    } else {
        None
    }
}

/// Owned view model of the Items screen - maps 1:1 onto the engine-ui
/// `PauseItemsView`.
#[derive(Debug, Clone, Default)]
pub struct ItemsScreenModel {
    /// The current page's visible rows: `(name, count)`.
    pub page_rows: Vec<(String, u16)>,
    pub page: u16,
    pub pages: u16,
    /// `true` = hand inside the list (rows drop to the grey staging-0
    /// ink); `false` = command-window focus (rows white).
    pub focus_list: bool,
    pub command_cursor: u8,
    /// List row on the current page.
    pub list_cursor_on_page: u8,
    pub bag_empty: bool,
    /// Info-window content for the staged (hovered) item.
    pub info: Option<ItemsInfoModel>,
    /// `true` while the use flow is picking a target - hosts overlay the
    /// target picker.
    pub target_select: bool,
}

/// Item info window content (`FUN_801DCB60` / `FUN_801D0F1C`).
#[derive(Debug, Clone, Default)]
pub struct ItemsInfoModel {
    pub name: String,
    pub count: u16,
    pub desc: String,
    pub passive: Option<(String, String)>,
}

/// Assemble the Items screen view model from a live session.
pub fn items_screen_model(s: &PauseItemsSession) -> ItemsScreenModel {
    let cursor = s.list_cursor();
    let page0 = cursor / LIST_PAGE_ROWS;
    let start = page0 * LIST_PAGE_ROWS;
    let page_rows = s
        .rows
        .iter()
        .skip(start)
        .take(LIST_PAGE_ROWS)
        .map(|r| (r.name.clone(), r.count as u16))
        .collect();
    let info = s.rows.get(cursor).map(|r| ItemsInfoModel {
        name: r.name.clone(),
        count: r.count as u16,
        desc: r.desc.clone(),
        passive: r.passive.clone(),
    });
    ItemsScreenModel {
        page_rows,
        page: s.page(),
        pages: s.pages(),
        focus_list: s.focus == PauseItemsFocus::List,
        command_cursor: s.command_cursor,
        list_cursor_on_page: (cursor - start) as u8,
        bag_empty: s.bag_empty(),
        info,
        target_select: s.target_select(),
    }
}

/// Owned view model of the Magic screen - maps 1:1 onto the engine-ui
/// `PauseMagicView`.
#[derive(Debug, Clone, Default)]
pub struct MagicScreenModel {
    /// Caster blocks: `(name, level, mp, mp_max)`.
    pub casters: Vec<(String, u8, u16, u16)>,
    /// The current page's visible spell rows: `(name, ra_seru)`.
    pub page_rows: Vec<(String, bool)>,
    pub page: u16,
    pub pages: u16,
    /// `true` = hand inside the spell list; `false` = caster-window focus.
    pub focus_list: bool,
    pub caster_cursor: u8,
    pub list_cursor_on_page: u8,
    pub info: Option<MagicInfoModel>,
    /// `true` while the cast flow is picking a target.
    pub target_select: bool,
}

/// Spell info window content (`FUN_801D2E74`).
#[derive(Debug, Clone, Default)]
pub struct MagicInfoModel {
    pub name: String,
    /// Learned spell level (record `+0x161` list).
    pub level: u8,
    /// Description (line breaks are `'\n'`).
    pub desc: String,
    pub mp_cost: u16,
    pub ra_seru: bool,
}

/// Assemble the Magic screen view model from a live [`SpellMenuSession`].
///
/// Phase map: `CharSelect` = caster focus (the hovered caster's list
/// shows white), `SpellSelect` = list focus (rows grey, hovered spell
/// staged into the info window), `TargetSelect` = the host overlays the
/// target picker. `text` fills descriptions; names fall back
/// catalog -> spell-name table -> `Spell XX`.
pub fn magic_screen_model(s: &SpellMenuSession, text: Option<&MenuTextTables>) -> MagicScreenModel {
    let casters: Vec<(String, u8, u16, u16)> = s
        .party()
        .iter()
        .map(|c| (c.name.clone(), c.level.max(1), c.mp, c.mp_max.max(c.mp)))
        .collect();

    let (caster_idx, focus_list, list_cursor, target_select) = match s.phase() {
        SpellMenuPhase::CharSelect { cursor } => (*cursor as usize, false, 0usize, false),
        SpellMenuPhase::SpellSelect { caster, cursor } => {
            (*caster as usize, true, *cursor as usize, false)
        }
        SpellMenuPhase::TargetSelect { caster, cursor, .. } => {
            (*caster as usize, true, *cursor as usize, true)
        }
        SpellMenuPhase::Done(_) => (0, false, 0, false),
    };

    let spell_name = |id: u8| -> String {
        s.catalog()
            .get(id)
            .map(|d| d.name.clone())
            .or_else(|| text.and_then(|t| t.spell_name(id)).map(str::to_string))
            .unwrap_or_else(|| format!("Spell {id:02X}"))
    };

    let spells: Vec<u8> = s
        .party()
        .get(caster_idx)
        .map(|c| c.spells.clone())
        .unwrap_or_default();
    let pages = spells.len().div_ceil(LIST_PAGE_ROWS).max(1) as u16;
    // In caster focus the hovered caster's list previews from page 1; the
    // list cursor only exists in list focus.
    let cursor = if focus_list { list_cursor } else { 0 };
    let page0 = if spells.is_empty() {
        0
    } else {
        (cursor / LIST_PAGE_ROWS).min(spells.len().div_ceil(LIST_PAGE_ROWS) - 1)
    };
    let start = page0 * LIST_PAGE_ROWS;
    let page_rows: Vec<(String, bool)> = spells
        .iter()
        .skip(start)
        .take(LIST_PAGE_ROWS)
        .map(|id| (spell_name(*id), RA_SERU_SPELL_IDS.contains(id)))
        .collect();

    // Info: the staged spell (hovered list row) - only while the hand is
    // in the list (retail gates on the staged id `DAT_801E46B0`).
    let info = if focus_list {
        spells.get(cursor).map(|id| {
            let level = s
                .party()
                .get(caster_idx)
                .map(|c| c.spell_level(cursor))
                .unwrap_or(1);
            let desc = text
                .and_then(|t| t.spell_desc(*id))
                .unwrap_or_default()
                .to_string();
            let base_cost = s
                .catalog()
                .get(*id)
                .map(|d| d.mp_cost as u16)
                .or_else(|| {
                    text.and_then(|t| t.spell_names.as_ref())
                        .and_then(|t| t.mp(*id))
                        .map(u16::from)
                })
                .unwrap_or(0);
            // Route the displayed cost through the per-caster MP-cost kernel
            // (`FUN_80035394`) so the Magic screen shows the discounted cost
            // an MP-saver ability actually charges, matching the battle path
            // (`BattleSpellSession::new` / `World::cast_spell_on_slots`).
            let ability_bits = s
                .party()
                .get(caster_idx)
                .map(|c| c.ability_bits)
                .unwrap_or(0);
            let mp_cost = mp_cost_after_ability_bits(
                base_cost,
                MpCostModifier::from_ability_flags(ability_bits),
            );
            MagicInfoModel {
                name: spell_name(*id),
                level,
                desc,
                mp_cost,
                ra_seru: RA_SERU_SPELL_IDS.contains(id),
            }
        })
    } else {
        None
    };

    MagicScreenModel {
        casters,
        page_rows,
        page: page0 as u16 + 1,
        pages,
        focus_list,
        caster_cursor: caster_idx as u8,
        list_cursor_on_page: (cursor - start) as u8,
        info,
        target_select,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inventory_use::{InventoryContext, TargetRow};
    use crate::items::ItemCatalog;
    use crate::spell_menu::{CasterSlot, SpellMenuInput};
    use crate::spells::SpellCatalog;

    fn items_session(ids_counts: &[(u8, u8)]) -> PauseItemsSession {
        let items: Vec<u8> = ids_counts.iter().map(|(id, _)| *id).collect();
        let rows: Vec<PauseItemRow> = ids_counts
            .iter()
            .map(|(id, count)| PauseItemRow {
                id: *id,
                name: format!("Item {id:02X}"),
                count: *count,
                desc: format!("Desc {id:02X}"),
                passive: None,
            })
            .collect();
        let targets = vec![TargetRow::new(0, "Vahn").with_stats(50, 100, 10, 30)];
        let inner = InventoryUseSession::new(
            ItemCatalog::vanilla(),
            items,
            targets,
            InventoryContext::Field,
        );
        PauseItemsSession::new(inner, rows)
    }

    fn edge(b: PadButton) -> u16 {
        b.mask()
    }

    /// The screen opens in command focus; Cross on "Use" moves the hand
    /// into the list; Circle in the list returns to the command window;
    /// Circle there closes.
    #[test]
    fn items_focus_walk_command_list_command_close() {
        let mut s = items_session(&[(0x77, 3)]);
        assert_eq!(s.focus, PauseItemsFocus::Command);
        s.input_pad_edge(edge(PadButton::Cross));
        assert_eq!(s.focus, PauseItemsFocus::List);
        s.input_pad_edge(edge(PadButton::Circle));
        assert_eq!(s.focus, PauseItemsFocus::Command);
        assert!(!s.is_done());
        s.input_pad_edge(edge(PadButton::Circle));
        assert!(s.is_done());
    }

    /// An empty bag keeps the hand on the command window ("Use" refuses).
    #[test]
    fn items_empty_bag_refuses_list_entry() {
        let mut s = items_session(&[]);
        assert!(s.bag_empty());
        s.input_pad_edge(edge(PadButton::Cross));
        assert_eq!(s.focus, PauseItemsFocus::Command);
    }

    /// Left/Right flip 12-row pages over the bag; the model slices the
    /// visible page and reports the retail 6-page bag total.
    #[test]
    fn items_page_flip_and_model_slice() {
        let rows: Vec<(u8, u8)> = (1..=30).map(|i| (i, 1)).collect();
        let mut s = items_session(&rows);
        s.input_pad_edge(edge(PadButton::Cross)); // into the list
        let m = items_screen_model(&s);
        assert_eq!(m.page, 1);
        assert_eq!(m.pages, DEFAULT_BAG_PAGES);
        assert_eq!(m.page_rows.len(), LIST_PAGE_ROWS);
        assert!(m.focus_list);

        s.input_pad_edge(edge(PadButton::Right));
        let m = items_screen_model(&s);
        assert_eq!(m.page, 2);
        assert_eq!(m.list_cursor_on_page, 0);
        // Page 3 holds the remaining 6 rows.
        s.input_pad_edge(edge(PadButton::Right));
        let m = items_screen_model(&s);
        assert_eq!(m.page, 3);
        assert_eq!(m.page_rows.len(), 6);
        // Clamped at the last row; Left returns.
        s.input_pad_edge(edge(PadButton::Left));
        let m = items_screen_model(&s);
        assert_eq!(m.page, 2);
    }

    /// The info model carries the hovered row's real count + description.
    #[test]
    fn items_info_follows_hovered_row() {
        let mut s = items_session(&[(0x77, 9), (0x78, 2)]);
        s.input_pad_edge(edge(PadButton::Cross));
        s.input_pad_edge(edge(PadButton::Down));
        let m = items_screen_model(&s);
        let info = m.info.expect("hovered row staged");
        assert_eq!(info.name, "Item 78");
        assert_eq!(info.count, 2);
        assert_eq!(info.desc, "Desc 78");
    }

    fn magic_session() -> SpellMenuSession {
        let party = vec![
            CasterSlot {
                slot: 0,
                name: "Vahn".into(),
                hp: 60,
                mp: 30,
                hp_max: 100,
                mp_max: 120,
                level: 7,
                spells: vec![0x81, 0x9c],
                spell_levels: vec![2, 1],
                ability_bits: 0,
            },
            CasterSlot {
                slot: 1,
                name: "Noa".into(),
                hp: 50,
                mp: 40,
                hp_max: 90,
                mp_max: 80,
                level: 6,
                spells: vec![0x83],
                spell_levels: vec![3],
                ability_bits: 0,
            },
        ];
        let targets = vec![crate::spell_menu::TargetRow {
            slot: 0,
            name: "Vahn".into(),
            hp: 60,
            hp_max: 100,
        }];
        SpellMenuSession::new(party, targets, SpellCatalog::vanilla())
    }

    /// Caster focus: mp/mp_max plumb through; the hovered caster's list
    /// previews white (focus_list = false) with no staged info.
    #[test]
    fn magic_model_caster_focus_carries_mp_max() {
        let s = magic_session();
        let m = magic_screen_model(&s, None);
        assert!(!m.focus_list);
        assert_eq!(m.casters.len(), 2);
        assert_eq!(m.casters[0], ("Vahn".to_string(), 7, 30, 120));
        assert_eq!(m.casters[1].3, 80);
        assert!(m.info.is_none());
        assert_eq!(m.page_rows.len(), 2);
    }

    /// List focus: rows grey (focus_list), the hovered spell stages into
    /// the info window with its learned level; Ra-Seru ids flag the wider
    /// icon.
    #[test]
    fn magic_model_list_focus_stages_info() {
        let mut s = magic_session();
        let _ = s.tick(SpellMenuInput {
            cross: true,
            ..Default::default()
        });
        assert!(matches!(s.phase(), SpellMenuPhase::SpellSelect { .. }));
        let m = magic_screen_model(&s, None);
        assert!(m.focus_list);
        let info = m.info.expect("hovered spell staged");
        assert_eq!(info.level, 2);
        assert!(!info.ra_seru);
        // Row 1 (0x9c = Horn) is in the Ra-Seru block.
        assert!(m.page_rows[1].1);
        let _ = s.tick(SpellMenuInput {
            down: true,
            ..Default::default()
        });
        let m = magic_screen_model(&s, None);
        let info = m.info.expect("hovered spell staged");
        assert!(info.ra_seru);
        assert_eq!(info.level, 1);
    }

    /// Description + name fall back through the MenuTextTables when the
    /// catalog has no entry.
    #[test]
    fn magic_model_desc_resolves_through_text_tables() {
        let mut s = magic_session();
        let _ = s.tick(SpellMenuInput {
            cross: true,
            ..Default::default()
        });
        let mut entries = vec![legaia_asset::spell_names::SpellEntry::default(); 0x82];
        entries[0x81].desc = Some("Crazy Driver\nAttack enemies.".to_string());
        let text = MenuTextTables {
            spell_names: Some(legaia_asset::spell_names::SpellNameTable::from_entries(
                entries,
            )),
            ..Default::default()
        };
        let m = magic_screen_model(&s, Some(&text));
        let info = m.info.expect("hovered spell staged");
        assert_eq!(info.desc, "Crazy Driver\nAttack enemies.");
    }

    /// PIN: the Magic screen's displayed MP cost is discounted through the
    /// per-caster MP-cost kernel (`FUN_80035394`). A caster with the half-MP
    /// ability bit (`0x20`) shows half cost; the quarter bit (`0x10`) shows a
    /// quarter shaved off; both set = half wins; no bits = full cost.
    fn staged_mp_cost(ability_bits: u32) -> u16 {
        let mut catalog = SpellCatalog::new();
        catalog.insert(crate::spells::SpellDef {
            id: 0x81,
            name: "Costly".into(),
            mp_cost: 40,
            ..Default::default()
        });
        let party = vec![CasterSlot {
            slot: 0,
            name: "Vahn".into(),
            hp: 60,
            mp: 120,
            hp_max: 100,
            mp_max: 120,
            level: 7,
            spells: vec![0x81],
            spell_levels: vec![1],
            ability_bits,
        }];
        let targets = vec![crate::spell_menu::TargetRow {
            slot: 0,
            name: "Vahn".into(),
            hp: 60,
            hp_max: 100,
        }];
        let mut s = SpellMenuSession::new(party, targets, catalog);
        // Enter the spell list so the hovered row stages into the info window.
        let _ = s.tick(SpellMenuInput {
            cross: true,
            ..Default::default()
        });
        assert!(matches!(s.phase(), SpellMenuPhase::SpellSelect { .. }));
        magic_screen_model(&s, None)
            .info
            .expect("hovered spell staged")
            .mp_cost
    }

    #[test]
    fn magic_model_displays_per_caster_discounted_mp_cost() {
        // No ability bits: full base cost.
        assert_eq!(staged_mp_cost(0x00), 40);
        // Half-MP bit (0x20): cost - (cost >> 1) = 20.
        assert_eq!(staged_mp_cost(0x20), 20);
        // Quarter bit (0x10): cost - (cost >> 2) = 30 (shaves 25%, not "to a quarter").
        assert_eq!(staged_mp_cost(0x10), 30);
        // Both bits set: Half (0x20) wins the priority - 20, not 30.
        assert_eq!(staged_mp_cost(0x30), 20);
    }
}
