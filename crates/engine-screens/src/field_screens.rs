//! Smaller field-side projections both play hosts used to carry a copy of:
//! the save-menu atlas' band table, the name-entry view, the field party HUD
//! readout, the field overlay's stage windows and the opening narration.

use legaia_engine_core::world::World;
use legaia_engine_menus::name_entry::NameEntry;
use legaia_engine_menus::save_menu_atlas::SaveMenuAtlas;
use legaia_engine_ui::battle_hud_chrome::BattleBadgeRects;
use legaia_engine_ui::{BattleHudDraws, NameEntryView, SaveMenuAtlasRects};
use legaia_engine_world_views::passive_hud::WorldPassiveHud;

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
pub fn name_entry_view<'a>(
    entry: &'a NameEntry,
    frame: u64,
    prompts: Option<&'a legaia_ui_formats::name_entry_prompts::NameEntryPrompts>,
) -> NameEntryView<'a> {
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
        prompts,
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
    let rows = legaia_engine_world_views::world_map_panel_host::field_party_hud_members(world);
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

/// The field overlay's **passive-ability badge column** (`FUN_801d095c`):
/// the icons floated over the player's head while an accessory passive is
/// active, seated through the host's view-projection `vp` (column-major,
/// mapping onto the 320x240 stage). Empty while no passive is active or the
/// anchor does not project.
///
/// `World` answers the three head-relative world points
/// (`WorldPassiveHud::passive_hud_points`, lifted by fractions of the player's `+0x72`)
/// and the icon list for a resolved anchor (`WorldPassiveHud::passive_hud_icons`); the
/// projection between them is this function, so both play hosts seat the
/// column the same way. Retail takes the anchor's **X from the first**
/// projected point and its **Y from the third** - a mixed pair, not one
/// point's.
///
/// The caller still owns the suppress gate: the column answers the party
/// readout's `_DAT_8007B868` suppression, which each host asks with its own
/// screen terms.
pub fn passive_hud_icons(
    world: &World,
    vp: &[f32; 16],
) -> Vec<legaia_engine_field::field_passive_hud::HudIcon> {
    if !world.passive_hud_active() {
        return Vec::new();
    }
    let Some(points) = world.passive_hud_points() else {
        return Vec::new();
    };
    let project = |p: [f32; 3]| -> Option<(i32, i32)> {
        let v = [p[0], p[1], p[2], 1.0];
        let mut clip = [0.0f32; 4];
        for (i, c) in clip.iter_mut().enumerate() {
            *c = (0..4).map(|j| vp[j * 4 + i] * v[j]).sum();
        }
        if clip[3] <= 0.01 {
            return None;
        }
        Some((
            ((clip[0] / clip[3] * 0.5 + 0.5) * 320.0) as i32,
            ((0.5 - clip[1] / clip[3] * 0.5) * 240.0) as i32,
        ))
    };
    let (Some(first), Some(third)) = (project(points[0]), project(points[2])) else {
        return Vec::new();
    };
    world.passive_hud_icons((first.0, third.1))
}

/// The badge column's icons as surface-pixel text draws, through the stage
/// transform `(stage_origin, stage_scale)`.
///
/// The icon ids `0x47..=0x4D` name cells of the field pictogram bank
/// (`FUN_8002C488`), which neither host has a sprite atlas for, so each draws
/// as the same gold ASCII stand-in the menu painters' pictograms use.
pub fn passive_hud_text_draws(
    font: &legaia_font::Font,
    icons: &[legaia_engine_field::field_passive_hud::HudIcon],
    stage_origin: (i32, i32),
    stage_scale: u32,
) -> Vec<legaia_engine_ui::TextDraw> {
    let mut out = Vec::new();
    for i in icons {
        let mut draws = legaia_engine_ui::text_draws_for(
            &font.layout_ascii("*"),
            (i.x, i.y),
            legaia_engine_ui::MENU_TEXT_GOLD,
        );
        legaia_engine_ui::scale_stage_text_draws(&mut draws, stage_origin, stage_scale);
        out.extend(draws);
    }
    out
}

/// The field party HUD's rearm terms for this frame, the first half of its
/// per-frame step on both play hosts.
///
/// A scene change is retail's rearm condition (its own arm includes "the
/// staged scene load is still pending"): without it the stationary compare
/// would run against the previous scene's coordinates and pop the readout up
/// mid-transition. `scene_latch` is the host's record of the scene the HUD
/// last ran in. Then retail's player-engaged term: a script or conversation
/// that holds the player restarts the idle countdown every frame, read a
/// frame late, the order retail's actor lists run in
/// (`FieldPartyHud::rearm_term`).
pub fn field_party_hud_rearm(
    hud: &mut legaia_engine_field::world_map_panel_host::FieldPartyHud,
    scene_latch: &mut Option<String>,
    host: &legaia_engine_scene_host::scene_host::SceneHost,
) {
    let scene = host.scene.as_ref().map(|s| s.name.clone());
    if scene != *scene_latch {
        *scene_latch = scene;
        hud.rearm();
    }
    let held = legaia_engine_world_views::world_map_panel_host::field_hud_rearm_held(&host.world);
    if hud.rearm_term(held) {
        hud.rearm();
    }
}

/// One frame of the field party HUD's decision kernel, the second half of
/// its per-frame step on both play hosts: the Field HP Display option
/// (`_DAT_800845C4`, not a camera mode), the suppress mask over the **packed**
/// d-pad of the word the world was handed this tick (`World::input` - not a
/// host's held keys, which would keep the HUD hidden while the world itself
/// was fed a neutral pad), the player's position for the stationary compare,
/// and the host's `suppressed` answer and projected stage `y`.
pub fn field_party_hud_tick(
    hud: &mut legaia_engine_field::world_map_panel_host::FieldPartyHud,
    world: &World,
    suppressed: bool,
    projected_y: Option<i16>,
) {
    let view_mode = legaia_engine_world_views::world_map_panel_host::field_hud_view_mode(world);
    let pad = legaia_engine_field::world_map_panel_host::packed_pad(world.input.pad());
    let player_pos = world
        .player_actor_slot
        .map(usize::from)
        .and_then(|s| world.actors.get(s))
        .map(|a| (a.move_state.world_x, a.move_state.world_z));
    hud.tick(suppressed, view_mode, pad, player_pos, 1, projected_y);
}

/// The field overlay's own stage windows that ride the shop-family overlay
/// group, in the retail 320x240 stage: the end-of-game records screen at the
/// engine's pen (op-`0x49` sub-op `0xC`, handler slot `0x33`,
/// `FUN_801EDF00`), then the floor window (sub-op 4, slot `0x23` - the Uru
/// Mais warp pads) and the code lock (sub-op 2, slot `0x21` - doman's password
/// door) through the shared line composition. A host scales the result with
/// its stage transform.
pub fn field_window_stage_draws(
    host: &legaia_engine_scene_host::scene_host::SceneHost,
    font: &legaia_font::Font,
) -> Vec<legaia_engine_ui::TextDraw> {
    let mut stage = Vec::new();
    if let Some(pen) = host.world.soft_reset_records_pen() {
        stage.extend(crate::dev_menu::records_draws_for_world(
            &host.world,
            font,
            pen,
        ));
    }
    let mut floor = host.flag_window_lines();
    floor.extend(host.code_lock_lines());
    stage.extend(legaia_engine_ui::text_lines::text_line_draws_for(
        font,
        floor
            .iter()
            .map(|l| (&l.text[..], i32::from(l.x), i32::from(l.y), l.marked)),
        [1.0, 1.0, 1.0, 1.0],
        legaia_engine_ui::text_lines::FLOOR_WINDOW_MARKED_INK,
    ));
    stage
}

/// The opening cutscene's narration in the retail 320x240 stage: the
/// bottom-up subtitle crawl (`FUN_80037174` - every visible line centred at
/// its current window Y) and the static title card, through
/// `engine-ui::cutscene_text_stage_draws`. Empty when neither is up. A host
/// scales the result with its stage transform.
// REF: FUN_80037174
pub fn cutscene_narration_stage_draws(
    world: &World,
    font: &legaia_font::Font,
) -> Vec<legaia_engine_ui::TextDraw> {
    let lines = world
        .cutscene
        .narration
        .as_ref()
        .map(|n| n.visible_lines())
        .unwrap_or_default();
    let crawl: Vec<(&str, i32)> = lines.iter().map(|l| (l.text, l.y)).collect();
    let card: Vec<&str> = world
        .cutscene
        .card
        .iter()
        .flatten()
        .map(String::as_str)
        .collect();
    if crawl.is_empty() && card.is_empty() {
        return Vec::new();
    }
    legaia_engine_ui::cutscene_text_stage_draws(font, &crawl, &card, [1.0, 1.0, 1.0, 1.0])
}
