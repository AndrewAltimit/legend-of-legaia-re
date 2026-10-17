//! Smaller field-side projections both play hosts used to carry a copy of:
//! the save-menu atlas' band table, the name-entry view, and the field party
//! HUD readout.

use legaia_engine_core::world::World;
use legaia_engine_menus::name_entry::NameEntry;
use legaia_engine_menus::save_menu_atlas::SaveMenuAtlas;
use legaia_engine_ui::battle_hud_chrome::BattleBadgeRects;
use legaia_engine_ui::{BattleHudDraws, NameEntryView, SaveMenuAtlasRects};

/// The save-menu atlas' sprite rects, as the menus and the battle HUD name
/// them - one field per band of the built atlas.
pub fn save_menu_rects(a: &SaveMenuAtlas) -> SaveMenuAtlasRects {
    SaveMenuAtlasRects {
        class0_dark: Some(legaia_engine_ui::Class0SkinRects::from_cells(
            a.band_class0_dark(),
        )),
        panel_tl: a.band_panel_tl(),
        panel_tr: a.band_panel_tr(),
        panel_bl: a.band_panel_bl(),
        panel_br: a.band_panel_br(),
        panel_top: a.band_panel_top(),
        panel_bot: a.band_panel_bot(),
        panel_left: a.band_panel_left(),
        panel_right: a.band_panel_right(),
        slot1: a.band_slot1(),
        slot2: a.band_slot2(),
        cursor: a.band_cursor(),
        panel_interior: a.band_panel_interior(),
        panel_filigree: a.band_panel_filigree(),
        label_lv: a.band_label_lv(),
        label_hp: a.band_label_hp(),
        label_mp: a.band_label_mp(),
        icon_money: a.band_icon_money(),
        label_time: a.band_label_time(),
        label_coin: a.band_label_coin(),
        gauge_cap: a.band_gauge_cap(),
        gauge_trough: a.band_gauge_trough(),
        gauge_box: a.band_gauge_box(),
        gauge_tip: a.band_gauge_tip(),
        gauge_digits: a.band_gauge_digits(),
        gauge_100: a.band_gauge_100(),
        gauge_fill: a.band_gauge_fill(),
        dialog_fill: a.band_dialog_fill(),
        icon_weapon: a.band_icon_weapon(),
        icon_helmet: a.band_icon_helmet(),
        icon_armor: a.band_icon_armor(),
        icon_boot: a.band_icon_boot(),
        icon_goods: a.band_icon_goods(),
        pager_left: a.band_pager_left(),
        pager_right: a.band_pager_right(),
        tab_cap_l: a.band_tab_cap_l(),
        tab_body: a.band_tab_body(),
        tab_cap_r: a.band_tab_cap_r(),
        atr_icons: a.band_atr_icons(),
        load_empty_frame: Some(a.band_load_empty_frame()),
        load_portrait_by_char: [
            a.band_load_portrait(0),
            a.band_load_portrait(1),
            a.band_load_portrait(2),
        ],
        battle: Some(legaia_engine_ui::BattleChromeRects {
            panel_bg: a.band_battle_panel_bg(),
            plate_cap_l: a.band_battle_plate_cap_l(),
            plate_body: a.band_battle_plate_body(),
            plate_cap_r: a.band_battle_plate_cap_r(),
            separator: a.band_battle_separator(),
            digits: a.band_hud_digits(),
            cross_out: a.band_cross_out(),
            rot_stamp: a.band_rot_stamp(),
            curse_plate: a.band_curse_plate(),
        }),
    }
}

/// The battle HUD's badge cells in a built atlas. Which cells carry art is a
/// property of the bake (three status badges need the row-511 extension
/// palettes), so it travels with the atlas.
pub fn battle_badge_rects(a: &SaveMenuAtlas) -> BattleBadgeRects {
    BattleBadgeRects {
        status: a.band_status_badges(),
        element: a.band_element_badges(),
    }
}

/// Project the live name-entry session onto the renderer-agnostic view the
/// `engine-ui` builders consume: the grid / control cursor split
/// (`NameEntry::cursor_cells`) and the caret blink on the world's sim
/// `frame` (`name_entry::caret_on`).
pub fn name_entry_view(entry: &NameEntry, frame: u64) -> NameEntryView<'_> {
    use legaia_engine_menus::name_entry::{GRID, NameEntryState, caret_on};
    let (grid_cursor, control_cursor) = entry.cursor_cells();
    NameEntryView {
        grid_rows: &GRID,
        name: &entry.name,
        default_name: &entry.default_name,
        grid_cursor,
        control_cursor,
        confirming: entry.state == NameEntryState::Confirm,
        confirm_yes: entry.confirm_yes,
        caret_on: caret_on(frame),
    }
}

/// Atlas sources the field party HUD samples, held by the host.
#[derive(Clone, Copy, Default)]
pub struct FieldHudSources<'a> {
    /// The system-UI chrome rects (`LV` / `HP` / `MP` cells, numerals).
    pub chrome: Option<&'a SaveMenuAtlasRects>,
    /// A fully-opaque texel inside the chrome atlas, for the translucent
    /// plate (`atlas_opaque_texel`).
    pub scrim_src: Option<(u32, u32, u32, u32)>,
    /// The font atlas' solid-white texel (`font_solid_src`).
    pub solid_src: Option<(u32, u32, u32, u32)>,
}

/// The field party HUD's two draw halves for this frame, in surface pixels,
/// at the kernel's row `y` (`HudDecision::Draw { y }` - the host asks its
/// suppress gate and its HUD kernel first).
pub fn field_party_hud_draws(
    world: &World,
    font: &legaia_font::Font,
    sources: FieldHudSources<'_>,
    y: i16,
    surface_w: u32,
    surface_h: u32,
) -> BattleHudDraws {
    use legaia_engine_ui::field_party_hud as fp;
    let rows =
        legaia_engine_core::world_views::world_map_panel_host::field_party_hud_members(world);
    let members: Vec<fp::FieldHudMember<'_>> = rows
        .iter()
        .map(|m| fp::FieldHudMember {
            name: &m.name,
            level: m.level,
            hp: m.hp,
            hp_max: m.hp_max,
            mp: m.mp,
            mp_max: m.mp_max,
            alive: m.alive,
        })
        .collect();
    let (origin, scale) = legaia_engine_ui::pause_menu::stage_transform(surface_w, surface_h);
    fp::field_party_hud_draws_for(
        font,
        &fp::FieldPartyHudFrame {
            members: &members,
            y: i32::from(y),
            chrome: sources.chrome,
            scrim_src: sources.scrim_src,
            solid_src: sources.solid_src,
            origin,
            scale: scale as i32,
        },
    )
}
