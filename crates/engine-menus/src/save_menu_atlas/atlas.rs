//! The baked atlas itself and the cell lookups over it (HUD digits, status and element badges).

use super::*;

/// Pre-decoded save-menu atlas - RGBA8 pixels + the source rects
/// engines sample to compose the retail save/load screen.
///
/// Build once at boot from PROT.DAT + PROT 0899 bytes via
/// [`build_atlas`], hand to engine-render's `upload_sprite_atlas`,
/// then emit one sprite quad per 9-slice tile + one per slot pill
/// each frame the save-select UI is active.
#[derive(Debug, Clone)]
pub struct SaveMenuAtlas {
    /// RGBA8 pixel data, exactly `4 * width * height` bytes.
    pub rgba: Vec<u8>,
    pub width: u32,
    pub height: u32,
    /// Whether [`ATLAS_RECT_HUD_DIGITS`] actually carries the numeral
    /// strip. False when [`build_atlas`] was handed no menu-glyph TIM;
    /// [`SaveMenuAtlas::band_hud_digits`] answers `None` then, and the
    /// battle HUD falls back to font glyphs on the same 8-px grid.
    pub has_hud_digits: bool,
    /// Bit `i` set = status badge `i` (ladder order, `0x18` first) carries
    /// its cell. Three of the nine need the row-511 extension palettes, so a
    /// caller whose slice starts at the system-UI sheet gets six; the HUD
    /// falls back to its labelled tag for the rest.
    pub status_badges_baked: u16,
    /// Bit `i` set = plain element badge `i` carries its cell. Clear for
    /// every badge when the caller's slice cannot reach the extension strip.
    pub element_badges_baked: u8,
    /// Whether [`ATLAS_RECT_CROSS_OUT`] carries the red cross-out X. Set by
    /// [`add_cross_out_mark`]; [`SaveMenuAtlas::band_cross_out`] answers
    /// `None` until then.
    pub has_cross_out: bool,
    /// Whether [`ATLAS_RECT_ROT_STAMP`] carries the Rot stamp. Set by
    /// [`add_cross_out_mark`] alongside the X.
    pub has_rot_stamp: bool,
    /// Whether [`ATLAS_RECT_CURSE_PLATE`] carries the Curse plate. Set by
    /// [`add_cross_out_mark`] alongside the X.
    pub has_curse_plate: bool,
}

impl SaveMenuAtlas {
    /// Panel top-left corner tile (4x4).
    pub fn band_panel_tl(&self) -> (u32, u32, u32, u32) {
        title_pak::OVERLAY_SYSTEM_UI_PANEL_TL
    }
    /// Panel top-right corner tile (4x4).
    pub fn band_panel_tr(&self) -> (u32, u32, u32, u32) {
        title_pak::OVERLAY_SYSTEM_UI_PANEL_TR
    }
    /// Panel bottom-left corner tile (4x4).
    pub fn band_panel_bl(&self) -> (u32, u32, u32, u32) {
        title_pak::OVERLAY_SYSTEM_UI_PANEL_BL
    }
    /// Panel bottom-right corner tile (4x4).
    pub fn band_panel_br(&self) -> (u32, u32, u32, u32) {
        title_pak::OVERLAY_SYSTEM_UI_PANEL_BR
    }
    /// Panel top edge tile (24x4) - repeated horizontally between
    /// the top corners.
    pub fn band_panel_top(&self) -> (u32, u32, u32, u32) {
        title_pak::OVERLAY_SYSTEM_UI_PANEL_TOP
    }
    /// Panel bottom edge tile (24x4).
    pub fn band_panel_bot(&self) -> (u32, u32, u32, u32) {
        title_pak::OVERLAY_SYSTEM_UI_PANEL_BOT
    }
    /// Panel left edge tile (4x21).
    pub fn band_panel_left(&self) -> (u32, u32, u32, u32) {
        title_pak::OVERLAY_SYSTEM_UI_PANEL_LEFT
    }
    /// Panel right edge tile (4x21).
    pub fn band_panel_right(&self) -> (u32, u32, u32, u32) {
        title_pak::OVERLAY_SYSTEM_UI_PANEL_RIGHT
    }
    /// SLOT 1 pill rect (baked "SLOT 1" label).
    pub fn band_slot1(&self) -> (u32, u32, u32, u32) {
        title_pak::OVERLAY_SAVE_MENU_BAND_SLOT1
    }
    /// SLOT 2 pill rect (baked "SLOT 2" label).
    pub fn band_slot2(&self) -> (u32, u32, u32, u32) {
        title_pak::OVERLAY_SAVE_MENU_BAND_SLOT2
    }
    /// Pointing-finger cursor sprite (16x16, white ink + grey shadow).
    /// Lives in the same system-UI TIM as the panel chrome but uses
    /// CLUT row 7 instead of row 2.
    pub fn band_cursor(&self) -> (u32, u32, u32, u32) {
        title_pak::OVERLAY_SYSTEM_UI_CURSOR
    }
    /// Panel interior fill tile (32x29, gradient-baked). Used by the
    /// save/load screen, whose interior retail draws with a gouraud
    /// gradient.
    pub fn band_panel_interior(&self) -> (u32, u32, u32, u32) {
        title_pak::OVERLAY_SYSTEM_UI_PANEL_INTERIOR
    }
    /// The dark class-0 window skin's nine cells
    /// ([`ATLAS_RECTS_CLASS0_DARK`]): corners, edges, then the raw fill.
    pub fn band_class0_dark(&self) -> [(u32, u32, u32, u32); 9] {
        ATLAS_RECTS_CLASS0_DARK
    }
    /// Raw marbled-blue filigree interior tile (32x32), un-baked. The
    /// pause-menu windows tile this in 2D as their navy damask interior.
    pub fn band_panel_filigree(&self) -> (u32, u32, u32, u32) {
        ATLAS_RECT_FILIGREE
    }
    /// Status-panel "LV" label sprite (16x12).
    pub fn band_label_lv(&self) -> (u32, u32, u32, u32) {
        ATLAS_RECT_LABEL_LV
    }
    /// Status-panel "HP" label sprite (16x12).
    pub fn band_label_hp(&self) -> (u32, u32, u32, u32) {
        ATLAS_RECT_LABEL_HP
    }
    /// Status-panel "MP" label sprite (16x12).
    pub fn band_label_mp(&self) -> (u32, u32, u32, u32) {
        ATLAS_RECT_LABEL_MP
    }
    /// Top-level menu money pictogram (12x12, UI-icon code 0x62).
    pub fn band_icon_money(&self) -> (u32, u32, u32, u32) {
        ATLAS_RECT_ICON_MONEY
    }
    /// Top-level menu play-time tag (24x10, UI-icon code 0x63).
    pub fn band_label_time(&self) -> (u32, u32, u32, u32) {
        ATLAS_RECT_LABEL_TIME
    }
    /// Top-level menu casino-coin tag (24x10, UI-icon code 0x66).
    pub fn band_label_coin(&self) -> (u32, u32, u32, u32) {
        ATLAS_RECT_LABEL_COIN
    }
    /// AP-gauge left cap (arrow tip + red "AP" chip, 24x16).
    pub fn band_gauge_cap(&self) -> (u32, u32, u32, u32) {
        ATLAS_RECT_GAUGE_CAP
    }
    /// AP-gauge trough body (56x16).
    pub fn band_gauge_trough(&self) -> (u32, u32, u32, u32) {
        ATLAS_RECT_GAUGE_TROUGH
    }
    /// AP-gauge value box (16x16).
    pub fn band_gauge_box(&self) -> (u32, u32, u32, u32) {
        ATLAS_RECT_GAUGE_BOX
    }
    /// AP-gauge right arrow tip (8x16, ICO record `0x6A`).
    pub fn band_gauge_tip(&self) -> (u32, u32, u32, u32) {
        ATLAS_RECT_GAUGE_TIP
    }
    /// AP-gauge red value-digit strip ("0".."9", 6x6 cells, ICO records
    /// `0x6C..=0x75`).
    pub fn band_gauge_digits(&self) -> (u32, u32, u32, u32) {
        ATLAS_RECT_GAUGE_DIGITS
    }
    /// AP-gauge "100" glyph (16x6, ICO record `0x6B`).
    pub fn band_gauge_100(&self) -> (u32, u32, u32, u32) {
        ATLAS_RECT_GAUGE_100
    }
    /// AP-gauge meter-fill gradient column (2x6, procedurally baked
    /// from the `FUN_8002c0b0` gouraud endpoints).
    pub fn band_gauge_fill(&self) -> (u32, u32, u32, u32) {
        ATLAS_RECT_GAUGE_FILL
    }
    /// Dialog-window interior fill gradient column (4x32,
    /// procedurally baked from the `FUN_8002C69C` gouraud fill
    /// endpoints, alpha 191).
    pub fn band_dialog_fill(&self) -> (u32, u32, u32, u32) {
        ATLAS_RECT_DIALOG_FILL
    }
    /// Equipment pictogram: weapon (fist, 12x12).
    pub fn band_icon_weapon(&self) -> (u32, u32, u32, u32) {
        ATLAS_RECT_ICON_WEAPON
    }
    /// Equipment pictogram: helmet (12x12).
    pub fn band_icon_helmet(&self) -> (u32, u32, u32, u32) {
        ATLAS_RECT_ICON_HELMET
    }
    /// Equipment pictogram: body armor (12x12).
    pub fn band_icon_armor(&self) -> (u32, u32, u32, u32) {
        ATLAS_RECT_ICON_ARMOR
    }
    /// Equipment pictogram: boot (12x12).
    pub fn band_icon_boot(&self) -> (u32, u32, u32, u32) {
        ATLAS_RECT_ICON_BOOT
    }
    /// Equipment pictogram: accessory / "Goods" ring (12x12).
    pub fn band_icon_goods(&self) -> (u32, u32, u32, u32) {
        ATLAS_RECT_ICON_GOODS
    }
    /// "Condition" pager left triangle (16x16, CLUT row 7).
    pub fn band_pager_left(&self) -> (u32, u32, u32, u32) {
        ATLAS_RECT_PAGER_LEFT
    }
    /// "Condition" pager right triangle (16x16, CLUT row 7).
    pub fn band_pager_right(&self) -> (u32, u32, u32, u32) {
        ATLAS_RECT_PAGER_RIGHT
    }
    /// Tab-banner plaque left cap (8x20, CLUT row 12).
    pub fn band_tab_cap_l(&self) -> (u32, u32, u32, u32) {
        ATLAS_RECT_TAB_CAP_L
    }
    /// Tab-banner plaque body tile (16x20, CLUT row 12).
    pub fn band_tab_body(&self) -> (u32, u32, u32, u32) {
        ATLAS_RECT_TAB_BODY
    }
    /// Tab-banner plaque right cap (8x20, CLUT row 12).
    pub fn band_tab_cap_r(&self) -> (u32, u32, u32, u32) {
        ATLAS_RECT_TAB_CAP_R
    }
    /// ATR element icons (28x12), character order Vahn / Noa / Gala.
    pub fn band_atr_icons(&self) -> [(u32, u32, u32, u32); 3] {
        ATLAS_RECT_ATR_ICONS
    }
    /// The whole HUD numeral strip (80x12, ten 8x12 cells starting at
    /// `0`). Slice a single digit out of it with [`hud_digit_cell`].
    ///
    /// `None` when the caller built the atlas without the menu-glyph TIM,
    /// so consumers can fall back rather than blit ten blank cells.
    pub fn band_hud_digits(&self) -> Option<(u32, u32, u32, u32)> {
        self.has_hud_digits.then_some(ATLAS_RECT_HUD_DIGITS)
    }
    /// The red cross-out X ([`add_cross_out_mark`]), when baked.
    pub fn band_cross_out(&self) -> Option<(u32, u32, u32, u32)> {
        self.has_cross_out.then_some(ATLAS_RECT_CROSS_OUT)
    }
    /// The Rot stamp ([`add_cross_out_mark`]), when baked.
    pub fn band_rot_stamp(&self) -> Option<(u32, u32, u32, u32)> {
        self.has_rot_stamp.then_some(ATLAS_RECT_ROT_STAMP)
    }
    /// The Curse plate ([`add_cross_out_mark`]), when baked.
    pub fn band_curse_plate(&self) -> Option<(u32, u32, u32, u32)> {
        self.has_curse_plate.then_some(ATLAS_RECT_CURSE_PLATE)
    }
    /// Battle roster-panel background (102x48, sub-palette 0).
    pub fn band_battle_panel_bg(&self) -> (u32, u32, u32, u32) {
        ATLAS_RECT_BATTLE_PANEL_BG
    }
    /// Battle blue-plate left cap (8x20, sub-palette 4).
    pub fn band_battle_plate_cap_l(&self) -> (u32, u32, u32, u32) {
        ATLAS_RECT_BATTLE_PLATE_CAP_L
    }
    /// Battle blue-plate body tile (16x20); the last tile of a run clips.
    pub fn band_battle_plate_body(&self) -> (u32, u32, u32, u32) {
        ATLAS_RECT_BATTLE_PLATE_BODY
    }
    /// Battle blue-plate right cap (8x20).
    pub fn band_battle_plate_cap_r(&self) -> (u32, u32, u32, u32) {
        ATLAS_RECT_BATTLE_PLATE_CAP_R
    }
    /// The `/` between a current and a maximum (8x16, sub-palette 5).
    pub fn band_battle_separator(&self) -> (u32, u32, u32, u32) {
        ATLAS_RECT_BATTLE_SEPARATOR
    }
    /// Status-element badge cell for retail sprite id `sprite`
    /// (`0x18..=0x20`), or `None` when the id is outside the ladder's band
    /// or its palette source was out of the caller's slice.
    pub fn band_status_badge(&self, sprite: u8) -> Option<(u32, u32, u32, u32)> {
        let i = status_badge_index(sprite)?;
        (self.status_badges_baked & (1 << i) != 0).then(|| status_badge_atlas_rect(i))
    }
    /// Every status badge in ladder order, `None` per unbaked cell. The
    /// shape the HUD builders take, so one lookup covers the whole ladder.
    pub fn band_status_badges(&self) -> [Option<(u32, u32, u32, u32)>; STATUS_BADGE_COUNT] {
        std::array::from_fn(|i| {
            (self.status_badges_baked & (1 << i) != 0).then(|| status_badge_atlas_rect(i))
        })
    }
    /// Plain element badge `index` (`0..8`), or `None` when the extension
    /// strip was out of the caller's slice.
    pub fn band_element_badge(&self, index: usize) -> Option<(u32, u32, u32, u32)> {
        (index < ELEMENT_BADGE_COUNT && self.element_badges_baked & (1 << index) != 0)
            .then(|| element_badge_atlas_rect(index))
    }
    /// Every plain element badge, `None` per unbaked cell.
    pub fn band_element_badges(&self) -> [Option<(u32, u32, u32, u32)>; ELEMENT_BADGE_COUNT] {
        std::array::from_fn(|i| self.band_element_badge(i))
    }
    /// Empty-cell frame sprite for the load-screen slot grid (32x32,
    /// 20x20 hollow blue border centred in the sprite - outer 6 px
    /// margin is transparent).
    pub fn band_load_empty_frame(&self) -> (u32, u32, u32, u32) {
        ATLAS_RECT_EMPTY_FRAME
    }
    /// Character portrait sub-rect for the load-screen slot grid
    /// (16x16). `char_id`: 0=Vahn, 1=Noa, 2=Gala. Returns `None` for
    /// characters whose portrait isn't in the on-disc atlas (party
    /// members past index 2 don't have load-screen icons; retail
    /// renders them as plain empty-frame slots).
    pub fn band_load_portrait(&self, char_id: u8) -> Option<(u32, u32, u32, u32)> {
        if (char_id as usize) < title_pak::OVERLAY_LOAD_PORTRAIT_COUNT {
            Some((
                ATLAS_RECT_PORTRAIT_BASE_X + (char_id as u32) * ATLAS_RECT_PORTRAIT_W,
                ATLAS_RECT_PORTRAIT_BASE_Y,
                ATLAS_RECT_PORTRAIT_W,
                ATLAS_RECT_PORTRAIT_H,
            ))
        } else {
            None
        }
    }
}

/// Sub-rect of digit `d` (`0..=9`) inside a HUD-numeral strip whose
/// whole-strip rect is `strip`. Retail addresses the strip the same way:
/// `u = d * 8` off the row origin.
///
/// Returns `None` for `d > 9`, so a caller can pass any byte through.
pub const fn hud_digit_cell(strip: (u32, u32, u32, u32), d: u32) -> Option<(u32, u32, u32, u32)> {
    if d > 9 {
        return None;
    }
    Some((strip.0 + d * HUD_DIGIT_W, strip.1, HUD_DIGIT_W, HUD_DIGIT_H))
}

/// Ladder position of a status-element sprite id (`0x18` -> `0`), or `None`
/// outside the nine-id band.
pub const fn status_badge_index(sprite: u8) -> Option<usize> {
    if sprite < 0x18 || sprite > 0x20 {
        return None;
    }
    Some((sprite - 0x18) as usize)
}

/// Atlas cell of status badge `index` in the re-seated block.
pub const fn status_badge_atlas_rect(index: usize) -> (u32, u32, u32, u32) {
    let i = index as u32;
    (
        ATLAS_RECT_STATUS_BADGES_ORIGIN.0 + (i % STATUS_BADGE_ATLAS_COLS) * STATUS_BADGE_W,
        ATLAS_RECT_STATUS_BADGES_ORIGIN.1 + (i / STATUS_BADGE_ATLAS_COLS) * STATUS_BADGE_H,
        STATUS_BADGE_W,
        STATUS_BADGE_H,
    )
}

/// Atlas cell of plain element badge `index` (one row, left to right).
pub const fn element_badge_atlas_rect(index: usize) -> (u32, u32, u32, u32) {
    (
        ATLAS_RECT_ELEMENT_BADGES_ORIGIN.0 + index as u32 * ELEMENT_BADGE_W,
        ATLAS_RECT_ELEMENT_BADGES_ORIGIN.1,
        ELEMENT_BADGE_W,
        ELEMENT_BADGE_H,
    )
}
