//! Minimal wgpu renderer for the Phase 1 asset viewer.
//!
//! PORT: FUN_80034b78, FUN_80034e4c, FUN_8002C69C, FUN_8002C488, FUN_8002B994, FUN_8003C310
//! PORT: FUN_80031D00, FUN_800337B0, FUN_80035CB8, FUN_80035DA0, FUN_80035E44, FUN_800349EC, FUN_80035EA8
//! PORT: FUN_8003C1F8 (per-glyph dialog-font sprite emit; the engine renders
//! in-game proportional dialog glyphs via the legaia-font atlas + textured-quad
//! overlay instead of the retail GP0 cell-UV push)
//!
//! Owns a wgpu device + surface, plus two render pipelines:
//!
//! * **Textured-quad** (Phase 1 TIM viewer) - `upload_texture` +
//!   `render(RenderTarget::Texture(...))`. Letterbox-preserves aspect ratio.
//! * **Flat-shaded mesh** (Phase 1 TMD viewer) - `upload_mesh` +
//!   `render(RenderTarget::Mesh { ... })`. Lit by a single directional
//!   light, depth-tested. Uses the `glam::Mat4` MVP supplied per-frame so
//!   the host can spin the model without re-uploading.
//!
//! Both pipelines share the same surface + depth attachment. PSX-faithful
//! rasterisation (affine UV warp, sub-pixel vertex jitter, 15-bit ordered
//! dithering) is opt-in via [`Renderer::set_psx_mode`]; GTE emulation and
//! batched draws are future phases.
//! REF: FUN_801D0148, FUN_801D5DE0, FUN_801D84D0, FUN_801E08D8, FUN_801E1C1C, FUN_801E36C4
//! REF: FUN_801E3EE0, FUN_801E3FF0

pub mod actor_bind;
pub mod actor_cull;
pub mod attach_swap;
pub mod battle_intro;
pub mod battle_on_screen;
pub mod gte_trace;
pub mod mode_transition;
pub mod window;

pub use glam;
pub use legaia_font;
pub use legaia_tim;
pub use wgpu;

// The pure, wgpu-free UI draw-list layer lives in `legaia-engine-ui`. The items
// below are the ones this crate and its dependents (the native shell, the
// asset-viewer, parity, and this crate's tests) name at the crate-root path;
// the list is explicit so a new `engine-ui` item does not silently join this
// crate's API. Anything else is reached as `legaia_engine_ui::<item>` (or add
// it here when a native caller needs the old path).
pub use legaia_engine_ui::{
    ArtsChainRow, ArtsEditorDrawArgs, ArtsEditorPhase, BOOT_UI_STAGE_H, BOOT_UI_STAGE_W,
    BattleChromeRects, BattleHudDraws, BattleHudFrame, BattleSpoilsView, COUNTER_PICTOGRAM_COINS,
    COUNTER_PICTOGRAM_GOLD, CardBannerView, CatchHudState, ComboLabelView, CounterSource,
    DevMenuListRow, EquipCandidateRow, EquipDrawPhase, EquipScreenView, EquipSlotRow,
    EquipStatBlock, EquipStatRow, FieldMenuPartyView, FieldMenuRowView, FishingBanners,
    FishingCaptions, FishingHudAtlas, HudDraw, HudLogView, HudPopupView, HudSlotMeta, HudSlotView,
    InventoryItemRow, InventoryTargetRow, InventoryUseDrawArgs, ListOrderDrawArgs,
    ListOrderRowView, MENU_TEXT_GOLD, MENU_TEXT_TEAL, MENU_TEXT_WHITE, MenuWindowPainter,
    NOW_CHECKING_PANEL_POS, NOW_CHECKING_PANEL_SIZE, NameEntryView, OPTIONS_INK_GOLD,
    OPTIONS_INK_TEAL, OptionsPopupDraw, OptionsRowView, PauseItemInfo, PauseItemsPhase,
    PauseItemsRow, PauseItemsView, PauseMagicCaster, PauseMagicInfo, PauseMagicPhase,
    PauseMagicRow, PauseMagicView, PauseThrowConfirmView, READOUT_NORMAL, RecordsField,
    RecordsLabels, RecordsScreenView, SAVE_SELECT_CURSOR_POS, SAVE_SELECT_SLOT_PITCH_Y,
    SAVE_SELECT_SLOT1_POS, SAVE_SELECT_SLOT1_POS_LOAD_ACTIVE, SAVE_SELECT_TITLE_COLOR,
    SAVE_SELECT_TITLE_POS, SLOT_GRID_ORIGIN, SLOT_GRID_PITCH_X, SLOT_GRID_PITCH_Y,
    SLOT_GRID_ROW_STAGGER_X, SLOT_INFO_CAPTION_CENTER_X, SLOT_INFO_PANEL_PARKED_Y,
    SLOT_INFO_PANEL_SIZE, SaveMenuAtlasRects, SaveSelectOverlayDraws, SaveSelectOverlayView,
    SaveSelectPreviewView, SaveSelectRow, ShopRow, SlotGridCell, SlotInfoView, SpellMenuDrawArgs,
    SpellRowView, SpellTargetView, SpriteDraw, SpriteRequest, StatusPanelView, StatusSatelliteView,
    StatusStatRow, TITLE_BACKDROP_LUM, TargetPanelCursor, TargetPanelMember, TargetPanelMode,
    TargetPanelView, TextDraw, TileBoardPromptLayout, TimedFightStripView, TitleBandState,
    VALUE_READOUT_FALLBACK_COLOR, ValueCellView, afterimage, apply_alpha, arts_input,
    atlas_opaque_texel, battle_chrome, battle_combo_cluster_draws_for, battle_command_ui,
    battle_defeat_windows, battle_hud_chrome, battle_hud_draws_for, battle_item_ui,
    battle_numerals, battle_result_line_draws_for, battle_spoils_draws_for, battle_spoils_windows,
    battle_stage_clear, battle_trail, battle_tutorial_chrome_draws_for,
    battle_tutorial_text_draws_for, battle_tutorial_text_width, battle_value_readout_draws_for,
    billboard, capture_banner_draws_for, cast_beam, catch_hud_draws, catch_result_draws,
    cutscene_text_stage_draws, dev_menu_cursor_xy, dev_menu_list_draws_for, diag_hud_enabled,
    dialog_advance_hand_sprite, dialog_option_hand_sprite, dialog_page_string,
    dialog_reading_box_lines, dialog_reading_box_text_draws_for, dialog_window_chrome_draws_for,
    effect_billboard, encounter_banner_draws_for, equip_screen_draws_for, equip_screen_sprites_for,
    field_menu_draws_for, field_party_hud, fishing_hud_draws_for, font_solid_src, gauge_fill_color,
    gte, hp_bar_color_index, incense_notice_sprites_for, incense_notice_text_draws_for,
    inventory_use_draws_for, key_rebind_draws_for, level_up_draws_for,
    menu_window_chrome_draws_for, menu_window_painters, minigame_fx, move_strip,
    mp_bar_color_index, name_entry_chrome_sprite_draws_for, name_entry_draws_for,
    now_checking_panel_draws_for, now_checking_text_draws_for, options_draws_for, other_game_hud,
    painter_at, painter_for, painter_rect, party_panel_stage_x, pause_menu, persistent_hud_draws,
    record_counters, record_offset, records_screen_draws_for, records_screen_fields,
    ringside_backdrop, save_refusal_panel_draws_for, save_refusal_text_draws_for,
    save_select_chrome_draws_for, save_select_cursor_draw_for, save_select_draws_for,
    save_select_overlay_draws, scale_stage_text_draws, scene_lighting, screen_prim, shop_draws_for,
    slot_info_caption_draws_for, slot_info_panel_draws_for, slot_info_panel_text_draws_for,
    slot_preview_grid_draws_for, spell_menu_draws_for, sprite_draws_for, status_icon_sprites_for,
    status_satellite_draws_for, status_satellite_icon_sprites_for, status_screen_draws_for,
    streak_pass, tab_banner_draws, tactical_arts_editor_draws_for, text_balloon_chrome_draws_for,
    text_balloon_text_draws_for, text_balloon_text_width, text_draws_for,
    tile_board_prompt_sprites_for, tile_board_prompt_text_draws_for,
    timed_fight_strip_chrome_draws, timed_fight_strip_text_draws, title_band_sprites,
    title_draws_for, title_menu_draws_for, title_strip_sprites, title_text_phase, ui_baka_strips,
    ui_boot_logos, ui_dance, ui_fishing_exchange, ui_fishing_hub, ui_fishing_line, ui_fishing_rod,
    ui_fishing_sprite, ui_slot_cabinet, ui_slot_paylines, ui_text_lines, vram_capture,
};

pub mod dyn_light;
pub mod occlusion_fade;
pub mod profile;
pub mod psx_blend;
pub mod psx_dither;
pub mod psx_light;
mod renderer;
pub mod scene_lights;
pub mod screen_overlay;
mod shaders;

pub use renderer::*;

/// Batch of [`TextDraw`]s to render in one pass against a shared font atlas.
/// Cheap to construct each frame; the renderer copies the geometry into a
/// reusable dynamic buffer before drawing.
pub struct TextOverlay<'a> {
    pub atlas: &'a UploadedFontAtlas,
    pub draws: &'a [TextDraw],
    /// Runs of [`Self::draws`] that are PSX semi-transparent packets. Every
    /// draw outside a span goes through the ordinary alpha-blend pipeline;
    /// a draw inside one goes through the span's ABR equation instead. Empty
    /// for an overlay with no semi-transparent packets.
    pub blend: &'a [OverlayBlendSpan],
}

/// A run of an overlay's draws blended with one PSX semi-transparency
/// equation (ABR, tpage bits 5..=6) instead of alpha: `0` = `B/2 + F/2`,
/// `1` = `B + F`, `2` = `B - F`, `3` = `B + F/4`, where `F` is the tinted
/// texel and `B` the framebuffer ([`psx_blend::blend_state`]). A texel with
/// atlas alpha `0` is PSX colour `0x0000` and is not drawn; the tint's alpha
/// is ignored.
///
/// Spans are ordered, do not overlap, and index [`TextOverlay::draws`];
/// draw order is unchanged - the pipeline switches at each span edge, so a
/// semi packet between two opaque ones stays between them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OverlayBlendSpan {
    /// Index of the span's first draw.
    pub start: u32,
    /// Number of draws in the span.
    pub count: u32,
    /// The ABR equation (`0..=3`).
    pub abr: u8,
}

impl OverlayBlendSpan {
    /// Split `count` draws starting at `base` into `(start, count, abr)`
    /// segments, one per maximal run of a single blend class - `None` for
    /// the alpha pipeline, `Some(abr)` for a span. Spans past `base + count`
    /// are clipped; a malformed (overlapping) span list keeps its first
    /// claim on each draw.
    pub fn segments(spans: &[Self], count: u32) -> Vec<(u32, u32, Option<u8>)> {
        let mut out = Vec::new();
        let mut at = 0u32;
        for sp in spans {
            let s = sp.start.max(at).min(count);
            let e = sp.start.saturating_add(sp.count).min(count);
            if s > at {
                out.push((at, s - at, None));
            }
            if e > s {
                out.push((s, e - s, Some(sp.abr & 3)));
                at = e;
            } else {
                at = at.max(s);
            }
        }
        if count > at {
            out.push((at, count - at, None));
        }
        out
    }
}

/// GPU-resident aliases of the moved [`SpriteDraw`] / [`TextOverlay`] shapes.
/// [`SpriteDraw`] itself lives in `legaia-engine-ui`; these two hold a wgpu
/// [`UploadedFontAtlas`] handle so they stay in this crate.
pub type UploadedSpriteAtlas = UploadedFontAtlas;
pub type SpriteOverlay<'a> = TextOverlay<'a>;

#[cfg(test)]
mod tests;
