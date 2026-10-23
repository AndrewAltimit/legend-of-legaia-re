//! Owned view models for the Items and Magic list screens.

use super::*;

/// Ra-Seru summon spell-id block (`Palma`..`Ozma`, the egg-derived
/// summons): these rows lead with the wider winged element icon in the
/// spell list. See `docs/formats/spell-table.md`.
pub const RA_SERU_SPELL_IDS: std::ops::RangeInclusive<u8> = 0x9A..=0xA0;

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
    /// The Throw Out confirm window content (descriptor id 9, renderer
    /// `FUN_801D1B20`) - `Some` while the Yes/No prompt is open. Hosts
    /// draw it with `engine-ui::items_throw_confirm_draws_for` over the
    /// command window (the retail confirm slides the command window out
    /// and window 9 in).
    pub throw_confirm: Option<ThrowConfirmModel>,
    /// The special Use route's own confirm window content - `Some` while
    /// submenu `0xB` (Door of Light) or `0xD` (Incense) has its Yes/No
    /// prompt open. A different window and renderer from `throw_confirm`;
    /// hosts draw it with `engine-ui::confirm_prompt_draws`.
    pub special_confirm: Option<SpecialConfirmModel>,
}

/// Special Use-route confirm window content - the shape both
/// `FUN_801D1DAC` (window 10, Door of Light) and `FUN_801D1F10`
/// (window 12, Incense) render.
#[derive(Debug, Clone)]
pub struct SpecialConfirmModel {
    /// Which route raised the window - it picks the descriptor rect and
    /// the one-line vs three-line renderer.
    pub route: UseRoute,
    /// Name of the item being used, staged as the prompt's first line.
    pub item_name: String,
    /// 0 = Yes, 1 = No. Retail seeds these two windows to **Yes**,
    /// unlike the Throw Out confirm.
    pub cursor: u8,
}

/// Throw Out confirm window content (`FUN_801D1B20`).
#[derive(Debug, Clone, Default)]
pub struct ThrowConfirmModel {
    /// Name of the stack about to be discarded.
    pub name: String,
    /// Its bag count (the whole stack is discarded).
    pub count: u16,
    /// 0 = Yes, 1 = No (retail defaults to No).
    pub cursor: u8,
}

/// Item info window content (`FUN_801DCB60` / `FUN_801D0F1C`).
#[derive(Debug, Clone, Default)]
pub struct ItemsInfoModel {
    pub name: String,
    pub count: u16,
    pub desc: String,
    pub passive: Option<(String, String)>,
    /// The staged row is the **Point Card** (`0xFE`), which retail's shared
    /// info panel branches on before anything else: `FUN_801D0F1C` compares
    /// the staged id against `0xFE` at `0x801d0fc0` and, on a match, draws
    /// its "Points Left" label + the `_DAT_800845B4` bank and **jumps past**
    /// the whole passive / scope-pictogram block.
    ///
    /// The bank itself is not here because this model is built from the
    /// session alone; a host reads `legaia_engine_minigame_scenes::minigame_state::MinigameState::point_card` and
    /// calls `engine-ui`'s `item_points_panel_draws`. The passive lines stay
    /// `None` on this row without needing a suppression: the Point Card's
    /// effect descriptor carries the `0x41` no-passive sentinel.
    pub is_point_card: bool,
}

/// The Items screen's model while the Door of Wind destination list
/// (retail submenu `0xC`, window 11) has the screen.
///
/// The rows are the unlocked landmarks and the hand is the list kernel's,
/// so the page maths is the shared one. Two deliberate residuals, both
/// stated rather than papered over:
///
/// - the row **count column draws `0`**, because the shared list row model
///   carries a `count` and the port has no window-11 renderer of its own
///   to drop it; retail's destination rows have no count column;
/// - the info window stays closed (`info: None`), which is retail - the
///   staged-id gate `DAT_801E46B0` is not restaged by this submenu.
fn destination_list_model(s: &PauseItemsSession, sp: &SpecialUseSession) -> ItemsScreenModel {
    let n = sp.landmarks.len();
    let cursor = sp.cursor.min(n.saturating_sub(1));
    let start = (cursor / LIST_PAGE_ROWS) * LIST_PAGE_ROWS;
    ItemsScreenModel {
        page_rows: sp
            .landmarks
            .iter()
            .skip(start)
            .take(LIST_PAGE_ROWS)
            .map(|name| (name.clone(), 0))
            .collect(),
        page: (start / LIST_PAGE_ROWS) as u16 + 1,
        pages: n.div_ceil(LIST_PAGE_ROWS).max(1) as u16,
        focus_list: true,
        command_cursor: s.command_cursor,
        list_cursor_on_page: (cursor - start) as u8,
        bag_empty: false,
        info: None,
        target_select: false,
        throw_confirm: None,
        special_confirm: None,
    }
}

/// Assemble the Items screen view model from a live session.
pub fn items_screen_model(s: &PauseItemsSession) -> ItemsScreenModel {
    // The Door of Wind destination list is a list window like the Use list
    // (retail window 11, driven by the same kind-4 kernel), so it projects
    // through the same rows / page / cursor channel rather than needing a
    // second one - which is what lets both hosts draw it with the list
    // renderer they already call.
    if let Some(sp) = s.special_use()
        && sp.phase == SpecialUsePhase::PickDestination
    {
        return destination_list_model(s, sp);
    }
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
    // Retail gates the info window on the staged id `DAT_801E46B0`: the
    // command SM's init phase zeroes it, the Use / Throw Out list phases
    // restage it from the hovered slot every frame.
    let info = if s.focus == PauseItemsFocus::Command {
        None
    } else {
        s.rows.get(cursor).map(|r| ItemsInfoModel {
            name: r.name.clone(),
            count: r.count as u16,
            desc: r.desc.clone(),
            passive: r.passive.clone(),
            is_point_card: r.id == crate::shop::POINT_CARD_ITEM_ID,
        })
    };
    let throw_confirm = if s.focus == PauseItemsFocus::ThrowOutConfirm {
        s.rows.get(cursor).map(|r| ThrowConfirmModel {
            name: r.name.clone(),
            count: r.count as u16,
            cursor: s.confirm_cursor,
        })
    } else {
        None
    };
    let special_confirm = s.special_use().and_then(|sp| {
        matches!(sp.phase, SpecialUsePhase::Confirm).then(|| SpecialConfirmModel {
            route: sp.route,
            item_name: s
                .rows
                .get(cursor)
                .map(|r| r.name.clone())
                .unwrap_or_default(),
            cursor: sp.cursor as u8,
        })
    });
    ItemsScreenModel {
        page_rows,
        page: s.page(),
        pages: s.pages(),
        // The hand sits inside the list for the Use list and both Throw
        // Out phases (rows drop to the grey staging-0 ink in all three).
        focus_list: matches!(
            s.focus,
            PauseItemsFocus::List
                | PauseItemsFocus::ThrowOutList
                | PauseItemsFocus::ThrowOutConfirm
        ),
        command_cursor: s.command_cursor,
        list_cursor_on_page: (cursor - start) as u8,
        bag_empty: s.bag_empty(),
        info,
        target_select: s.target_select(),
        throw_confirm,
        special_confirm,
    }
}

/// Owned view model of the Magic screen - maps 1:1 onto the engine-ui
/// `PauseMagicView`.
#[derive(Debug, Clone, Default)]
pub struct MagicScreenModel {
    /// Caster blocks: `(name, level, mp, mp_max)`.
    pub casters: Vec<(String, u8, u16, u16)>,
    /// The current page's visible spell rows: `(name, ra_seru, icon)`,
    /// `icon` being the operand of the `0xCE` element-plate escape the
    /// retail name string opens with ([`spell_icon`]).
    pub page_rows: Vec<(String, bool, Option<u8>)>,
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
    /// The name's element-plate escape operand ([`spell_icon`]).
    pub icon: Option<u8>,
}

/// Operand of the `0xCE` escape spell `id`'s retail display name opens
/// with - the element plate (`0x14..=0x1B`) or winged Ra-Seru plate
/// (`0x1D..=0x24`) the Magic screen's list kernel and info window draw in
/// front of the name, because both draw the raw `name_ptr` string
/// (`FUN_8002FF8C` -> `FUN_80036888`). `None` without the spell table.
pub fn spell_icon(text: Option<&MenuTextTables>, id: u8) -> Option<u8> {
    text?.spell_names.as_ref()?.icon(id)
}

/// Assemble the Magic screen view model from a live [`SpellMenuSession`].
///
/// Phase map: `CharSelect` = caster focus (the hovered caster's list
/// shows white), `SpellSelect` = list focus (rows grey, hovered spell
/// staged into the info window), `TargetSelect` = the host overlays the
/// target picker, `GroupConfirm` = the no-pick group flow, which draws the
/// list exactly as `SpellSelect` does and no picker at all. `text` fills
/// descriptions; names fall back
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
        // Retail's group sub-screen `0x10` draws no target rows: the spell
        // list keeps the hand and the info window keeps the staged spell.
        SpellMenuPhase::GroupConfirm { caster, cursor, .. } => {
            (*caster as usize, true, *cursor as usize, false)
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
    // `0` for a caster with no spells: the list kernel draws no PAGE header
    // at a zero row count (`FUN_80032A44`, `beq a0,zero` at `0x80032e18`).
    let pages = spells.len().div_ceil(LIST_PAGE_ROWS) as u16;
    // In caster focus the hovered caster's list previews from page 1; the
    // list cursor only exists in list focus.
    let cursor = if focus_list { list_cursor } else { 0 };
    let page0 = if spells.is_empty() {
        0
    } else {
        (cursor / LIST_PAGE_ROWS).min(spells.len().div_ceil(LIST_PAGE_ROWS) - 1)
    };
    let start = page0 * LIST_PAGE_ROWS;
    let page_rows: Vec<(String, bool, Option<u8>)> = spells
        .iter()
        .skip(start)
        .take(LIST_PAGE_ROWS)
        .map(|id| {
            (
                spell_name(*id),
                RA_SERU_SPELL_IDS.contains(id),
                spell_icon(text, *id),
            )
        })
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
                icon: spell_icon(text, *id),
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
