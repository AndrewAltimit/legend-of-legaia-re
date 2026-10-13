//! Atlas layout: every destination rect, cell size and source-sheet constant the bake reads.

use super::*;

/// Atlas dimensions in source pixels. Matches the legacy PROT-0899
/// save-menu atlas dimensions so existing engine-render plumbing
/// keeps working; the panel tiles slot in at coordinates
/// `(160..192, 0..32)` which are free in the source PROT 0899 atlas
/// (those columns hold tiny memory-card icons not used at SaveSelect).
pub const ATLAS_WIDTH: u32 = 256;
pub const ATLAS_HEIGHT: u32 = 256;

/// CLUT row used to render the slot pills: bright blue body with
/// white text.
pub(super) const PILL_CLUT: usize = 7;

/// Atlas placement of the **load-screen empty-cell frame** (32x32
/// sprite, hollow blue border). Chosen to sit inside unused atlas
/// columns at the bottom-right corner.
pub const ATLAS_RECT_EMPTY_FRAME: (u32, u32, u32, u32) = (200, 64, 32, 32);

/// Atlas placement of the status-panel **stat labels** (LV / HP / MP),
/// copied from CLUT row 1 of the system-UI TIM (the `0x800732a4` icon
/// records' palette - green/red ink). Packed in a free strip below the
/// filigree tile. Each is 16x10.
pub const ATLAS_RECT_LABEL_LV: (u32, u32, u32, u32) = (40, 232, 16, 10);
pub const ATLAS_RECT_LABEL_HP: (u32, u32, u32, u32) = (60, 232, 16, 10);
pub const ATLAS_RECT_LABEL_MP: (u32, u32, u32, u32) = (80, 232, 16, 10);

/// Atlas placement of the **raw** (un-gradient-baked) marbled-blue
/// filigree interior tile (32x32). Copied verbatim from CLUT row 2 of
/// the system-UI TIM so the field-menu chrome can tile it in 2D as the
/// window interior (the pause menu fills every window with this navy
/// damask, darkened by a per-draw colour, rather than the save screen's
/// gouraud-gradient variant at [`ATLAS_RECT_FILIGREE`]'s baked sibling).
/// Sits in a free atlas region below the portraits.
pub const ATLAS_RECT_FILIGREE: (u32, u32, u32, u32) = (0, 200, 32, 32);

/// The whole marbled patch the class-0 fill samples: texels `(128, 0)` of
/// CLUT row 2, 32 x 32 (`FUN_8002BDC4`'s `u 128..160, v 0..32` - the
/// `menu_status_town` display list's fill quads). The save screen's baked
/// interior keeps its 29-row slice ([`title_pak::OVERLAY_SYSTEM_UI_PANEL_INTERIOR`]).
pub(super) const FILIGREE_SRC: (u32, u32, u32, u32) = (128, 0, 32, 32);

/// The **dark class-0 window skin**: widget record `0x43` of the table at
/// `0x800732A4` (`00 01 00 03 80 20 20 20 F8 FF F8 FF` - class 0, tile-set 1,
/// sub-palette 3, fill rect `(128, 32, 32, 32)`, seat bias `(-8, -8)`) and
/// its tile-set at `0x80073A20`, the eight border quads. Every cell is the
/// blue skin's (record `3`, tile-set 0) 32 texel rows lower.
///
/// It is the frame of a battle prompt that waits for acknowledgement: the
/// tutorial emitter registers such a box with style byte `0x44 - 1`
/// (`FUN_801F747C`), and the frame emitter `FUN_8002C69C` indexes the widget
/// table with that byte (`0x8002C798..0x8002C7AC`).
///
/// Order: the four corners (TL, TR, BL, BR), top, bottom, left, right, then
/// the fill. Natural sheet coordinates - the atlas's `(128..192, 32..64)`
/// block is otherwise unused.
pub const ATLAS_RECTS_CLASS0_DARK: [(u32, u32, u32, u32); 9] = [
    (160, 32, 4, 4),
    (188, 32, 4, 4),
    (160, 60, 4, 4),
    (188, 60, 4, 4),
    (164, 32, 24, 4),
    (164, 60, 24, 4),
    (160, 36, 4, 24),
    (188, 36, 4, 24),
    (128, 32, 32, 32),
];
/// Row-511 sub-palette the dark skin decodes through (widget record `0x43`
/// byte `+3`; CLUT `0x7FC0 + 3`, VRAM `(48, 511)`).
pub(super) const CLASS0_DARK_CLUT_ROW: usize = 3;

/// Atlas placement of the status-page **AP gauge** pieces, copied from
/// CLUT row 4 of the system-UI TIM (the status gauge palette - purple
/// frame + red "AP" chip). Packed in the free strip right of the
/// filigree tile. Sizes mirror the `OVERLAY_SYSTEM_UI_GAUGE_*` source
/// rects.
pub const ATLAS_RECT_GAUGE_CAP: (u32, u32, u32, u32) = (40, 200, 24, 16);
pub const ATLAS_RECT_GAUGE_TROUGH: (u32, u32, u32, u32) = (66, 200, 56, 16);
pub const ATLAS_RECT_GAUGE_BOX: (u32, u32, u32, u32) = (124, 200, 16, 16);
pub const ATLAS_RECT_GAUGE_TIP: (u32, u32, u32, u32) = (142, 200, 8, 16);
/// Red value-digit strip ("0".."9", ten 6x6 cells at 6-px pitch - the
/// ICO records for codes `0x6C..=0x75`).
pub const ATLAS_RECT_GAUGE_DIGITS: (u32, u32, u32, u32) = (152, 200, 60, 6);
/// The "100" glyph shown at a full 100 AP (ICO record `0x6B`, CLUT
/// row 1).
pub const ATLAS_RECT_GAUGE_100: (u32, u32, u32, u32) = (214, 200, 16, 6);
/// AP-gauge meter-fill gradient column (2x6). Not TIM-sourced: baked
/// procedurally from the `FUN_8002c0b0` gouraud-quad endpoint colours
/// (dark-red -> gold -> dark-red vertical diamond gradient, linear
/// per-row interpolation of the 3-row quad pair). Engines stretch it
/// horizontally to `value/2` px.
pub const ATLAS_RECT_GAUGE_FILL: (u32, u32, u32, u32) = (232, 200, 2, 6);
/// Dialog-window interior fill gradient column (4x32). Not
/// TIM-sourced: baked procedurally from the `FUN_8002C69C` hardcoded
/// fill - two stacked semi-transparent (mode 0) gouraud quads, top
/// RGB `(0x18,0x18,0x28)` fading to bottom RGB `(0x40,0x40,0xA0)`,
/// drawn twice = `0.25*back + 0.75*gradient`. Baked with alpha 191
/// (= 0.75) so a single alpha-blended sprite stretched over the box
/// rect reproduces the retail translucency exactly. Engines stretch
/// it to the window width and height.
pub const ATLAS_RECT_DIALOG_FILL: (u32, u32, u32, u32) = (240, 200, 4, 32);

/// Atlas placement of the status-page **equipment pictograms** (12x12
/// gold slot icons), copied from CLUT row 8 of the system-UI TIM.
/// Packed in a row between the gauge strip and the LV/HP/MP labels.
pub const ATLAS_RECT_ICON_WEAPON: (u32, u32, u32, u32) = (40, 216, 12, 12);
pub const ATLAS_RECT_ICON_HELMET: (u32, u32, u32, u32) = (54, 216, 12, 12);
pub const ATLAS_RECT_ICON_ARMOR: (u32, u32, u32, u32) = (68, 216, 12, 12);
pub const ATLAS_RECT_ICON_BOOT: (u32, u32, u32, u32) = (82, 216, 12, 12);
pub const ATLAS_RECT_ICON_GOODS: (u32, u32, u32, u32) = (96, 216, 12, 12);

/// Atlas placement of the top-level pause menu's money/play-time box
/// icons (UI-icon codes `0x62` / `0x63` / `0x66` - money pictogram,
/// play-time tag, casino coin tag), copied from the label CLUT row of
/// the system-UI TIM (`title_pak::OVERLAY_SYSTEM_UI_ICON_MONEY` /
/// `_LABEL_TIME` / `_LABEL_COIN`).
pub const ATLAS_RECT_ICON_MONEY: (u32, u32, u32, u32) = (110, 216, 12, 12);
pub const ATLAS_RECT_LABEL_TIME: (u32, u32, u32, u32) = (124, 216, 24, 10);
pub const ATLAS_RECT_LABEL_COIN: (u32, u32, u32, u32) = (150, 216, 24, 10);

/// Atlas placement of the "Condition" pager **arrow sprites** (16x16
/// solid triangles), copied from CLUT row 7 of the system-UI TIM (the
/// pointing-hand cursor row; sources
/// `title_pak::OVERLAY_SYSTEM_UI_PAGER_LEFT/RIGHT`).
pub const ATLAS_RECT_PAGER_LEFT: (u32, u32, u32, u32) = (104, 232, 16, 16);
pub const ATLAS_RECT_PAGER_RIGHT: (u32, u32, u32, u32) = (124, 232, 16, 16);

/// Atlas placement of the dialogue **page mark** - the two-frame "press"
/// icon a waiting page shows at its box's lower-right rim (kind `1` of the
/// cursor sprite primitive `FUN_8002B994`, [`legaia_engine_dialog::cursor_sprite`]). The
/// frames' sheet cells `(224, 64)` / `(240, 64)` fall inside rows this atlas
/// gives the load screen's empty-slot frame, so they are re-seated here;
/// CLUT row 7, the pointing hand's.
pub const ATLAS_RECT_ADVANCE_ICON: [(u32, u32, u32, u32); 2] =
    [(192, 160, 16, 16), (208, 160, 16, 16)];

/// Atlas placement of the field-menu **tab-banner plaque** pieces (left
/// cap 8x20, 16x20 body tile, right cap 8x20), copied from CLUT row 12 of
/// the system-UI TIM (`title_pak::OVERLAY_SYSTEM_UI_TAB_*`).
pub const ATLAS_RECT_TAB_CAP_L: (u32, u32, u32, u32) = (144, 232, 8, 20);
pub const ATLAS_RECT_TAB_BODY: (u32, u32, u32, u32) = (154, 232, 16, 20);
pub const ATLAS_RECT_TAB_CAP_R: (u32, u32, u32, u32) = (172, 232, 8, 20);

/// Atlas placement of the status summary window's per-character **ATR
/// element icons** (28x12), decoded from the system-UI extension-strip
/// TIM (`title_pak::OVERLAY_SYSTEM_UI_EXT_TIM_OFFSET`) with the sibling
/// CLUT-variant TIM's palettes. Character order Vahn / Noa / Gala.
pub const ATLAS_RECT_ATR_ICONS: [(u32, u32, u32, u32); 3] =
    [(184, 232, 28, 12), (212, 232, 28, 12), (184, 244, 28, 12)];

/// Atlas placement of the **battle roster panel background** - one 102x48
/// marbled plate, the whole of a resting party member's readout skin.
///
/// Natural sheet coordinates: `engine_ui::battle_chrome::PANEL_BG` is
/// `(0, 0, 102, 48)` on the system-UI sheet through sub-palette 0, and the
/// atlas's `(0..102, 0..48)` corner is free, so the rect reads the same on
/// both sides.
pub const ATLAS_RECT_BATTLE_PANEL_BG: (u32, u32, u32, u32) = (0, 0, 102, 48);

/// Atlas placement of the **blue plate 3-slice** - the left cap, repeating
/// body tile and right cap the active-actor bar and every command chip are
/// built from (`battle_chrome::PLATE_BLUE`, sheet row `v = 0`, sub-palette 4).
///
/// Natural sheet coordinates again. The carved-gold sibling at `v = 64` is
/// already baked as the field menu's tab banner
/// ([`ATLAS_RECT_TAB_CAP_L`] and friends) - it is the same art, which is why
/// the battle plaque and the pause menu's title tab look alike.
pub const ATLAS_RECT_BATTLE_PLATE_CAP_L: (u32, u32, u32, u32) = (208, 0, 8, 20);
pub const ATLAS_RECT_BATTLE_PLATE_BODY: (u32, u32, u32, u32) = (192, 0, 16, 20);
pub const ATLAS_RECT_BATTLE_PLATE_CAP_R: (u32, u32, u32, u32) = (216, 0, 8, 20);

/// Atlas placement of the **`/` separator** between a current and a maximum
/// (`battle_chrome::SEPARATOR`, sub-palette 5). A sheet sprite, not a font
/// glyph - and it sits four rows above the numerals it separates.
pub const ATLAS_RECT_BATTLE_SEPARATOR: (u32, u32, u32, u32) = (96, 64, 8, 16);

/// Atlas placement of the **HUD numeral strip** - ten 8x12 digit cells
/// at an 8-px pitch, `0` first. Unlike every other rect in this atlas
/// the source is not the system-UI sheet but the neighbouring
/// **menu-glyph atlas** TIM (`legaia_ui_formats::menu_glyph_atlas`, VRAM page
/// `(960, 256)`), decoded through its sub-palette 13 - the palette
/// retail's own `SPRT` packets name for battle-HUD text.
///
/// These are the numerals the battle HUD draws: retail lays HP / MP /
/// level out as fixed 8-px cells from this strip, never as proportional
/// dialog-font glyphs, which is what keeps a four-digit value inside a
/// 102-px roster panel. Baked into this atlas rather than sampled from
/// the separately-uploaded menu-glyph atlas so the HUD's sprite list
/// stays one texture.
///
/// The one battle rect that is **not** at its natural coordinate: the
/// strip's own row is `(0, 208)`, which the filigree tile already holds,
/// so it re-seats - the same rule the arts chips follow.
pub const ATLAS_RECT_HUD_DIGITS: (u32, u32, u32, u32) = (0, 244, 80, 12);
/// Width and horizontal pitch of one [`ATLAS_RECT_HUD_DIGITS`] cell.
pub const HUD_DIGIT_W: u32 = 8;
/// Height of one HUD numeral cell.
pub const HUD_DIGIT_H: u32 = 12;

/// Atlas placement of the **status-element badge block** - the nine 48x16
/// word tags `FUN_8002C2E4`'s exclusive ladder selects from, sprite ids
/// `0x18..=0x20` (`legaia_ui_formats::ui_widgets::STATUS_BADGES`).
///
/// On the sheet they are a two-column block at `(0 | 48, 48..=112)`; here
/// they re-seat into a 4-wide grid because three of the sheet cells collide
/// with the save-menu slot pills this atlas already carries at
/// `(33, 97..128)`. Same rule the HUD numeral strip follows: natural
/// coordinates unless something already owns them.
pub const ATLAS_RECT_STATUS_BADGES_ORIGIN: (u32, u32) = (0, 128);
/// Badge cell size, sheet and atlas alike.
pub const STATUS_BADGE_W: u32 = 48;
pub const STATUS_BADGE_H: u32 = 16;
/// Badges per atlas row (`4 * 48 = 192` px, clear of the arts chip triple
/// at `x >= 200`).
pub const STATUS_BADGE_ATLAS_COLS: u32 = 4;
/// Number of status badges - the nine ladder outcomes.
pub const STATUS_BADGE_COUNT: usize = 9;

/// Sheet cell + row-511 sub-palette of each status badge, in ladder order
/// (`0x18` first). Mirrors what `legaia_ui_formats::ui_widgets` reads out of the
/// widget-class table; the disc-gated oracle
/// `crates/asset/tests/ui_widgets_real.rs` is what keeps the two honest.
///
/// Three of the nine reach past the system-UI TIM's own sixteen palettes -
/// row 511 runs on to VRAM x 288 through the CLUT-only TIM at
/// [`SYSTEM_UI_CLUT_EXT_TIM_OFFSET`].
pub(super) struct StatusBadgeCell {
    /// Retail sprite id (`0x18..=0x20`).
    pub(super) sprite: u8,
    /// Cell origin on the system-UI sheet.
    pub(super) sheet: (u32, u32),
    /// Row-511 sub-palette index.
    pub(super) subpalette: u16,
}

pub(super) const STATUS_BADGE_CELLS: [StatusBadgeCell; STATUS_BADGE_COUNT] = [
    StatusBadgeCell {
        sprite: 0x18,
        sheet: (0, 48),
        subpalette: 9,
    },
    StatusBadgeCell {
        sprite: 0x19,
        sheet: (48, 48),
        subpalette: 10,
    },
    StatusBadgeCell {
        sprite: 0x1A,
        sheet: (48, 80),
        subpalette: 16,
    },
    StatusBadgeCell {
        sprite: 0x1B,
        sheet: (48, 112),
        subpalette: 14,
    },
    StatusBadgeCell {
        sprite: 0x1C,
        sheet: (0, 96),
        subpalette: 17,
    },
    StatusBadgeCell {
        sprite: 0x1D,
        sheet: (0, 64),
        subpalette: 11,
    },
    StatusBadgeCell {
        sprite: 0x1E,
        sheet: (0, 80),
        subpalette: 15,
    },
    StatusBadgeCell {
        sprite: 0x1F,
        sheet: (48, 64),
        subpalette: 13,
    },
    StatusBadgeCell {
        sprite: 0x20,
        sheet: (48, 96),
        subpalette: 18,
    },
];

/// File offset within `PROT.DAT` of the **row-511 sub-palette extension**:
/// a CLUT-only TIM (its image block is a four-word stub) whose CLUT block
/// uploads to VRAM `(256, 511)`, 16 entries x 3 rows - sub-palettes **16,
/// 17 and 18** of the same strip the system-UI sheet's own sixteen start.
///
/// It sits immediately before the system-UI TIM
/// (`0x1858 + 0x88 == 0x18E0`), which is why a slice rooted at the sheet
/// cannot see it; [`build_atlas`] accepts either root.
pub const SYSTEM_UI_CLUT_EXT_TIM_OFFSET: usize = 0x1858;
/// Byte extent of that TIM: 8 header + 108 CLUT block + 20 image stub.
pub const SYSTEM_UI_CLUT_EXT_TIM_SIZE: usize = 0x88;
/// Sub-palette index the extension TIM's palette 0 stands for.
pub const SYSTEM_UI_CLUT_EXT_FIRST_SUBPALETTE: u16 = 16;

/// Atlas placement of the **plain element-badge strip** - the eight 20x12
/// badges the actor-name plaque wears in front of an elemental actor's
/// name (widget records `0x8B..=0x92`).
///
/// Their texels are not on the system-UI sheet: they are row `v = 192` of
/// the 256x32 **extension strip** that continues the page at VRAM
/// `(896, 448)`, and each badge takes its own CLUT out of the
/// `(896.., 498..499)` block rather than the row-511 strip.
pub const ATLAS_RECT_ELEMENT_BADGES_ORIGIN: (u32, u32) = (0, 176);
/// Element-badge cell size.
pub const ELEMENT_BADGE_W: u32 = 20;
pub const ELEMENT_BADGE_H: u32 = 12;
/// Number of element badges in the plain strip.
pub const ELEMENT_BADGE_COUNT: usize = 8;
/// Horizontal pitch of the badges **on the extension strip** and the u of
/// badge 0 (`(6 + 32*i, 0, 20, 12)` in strip-local coordinates - strip
/// `v` is sheet `V - 192`).
pub(super) const ELEMENT_BADGE_SHEET_PITCH: u32 = 32;
pub(super) const ELEMENT_BADGE_SHEET_U0: u32 = 6;

/// The two sibling CLUT TIMs the plain element badges decode through:
/// badges `0..=3` take palettes `0..=3` of the strip's own block (VRAM row
/// 498, [`title_pak::OVERLAY_SYSTEM_UI_EXT_TIM_OFFSET`]) and badges
/// `4..=7` the same palettes of the row-499 sibling.
pub(super) const ELEMENT_BADGE_ROW499_TIM_OFFSET: usize = 0x100D0;

/// Atlas placement of the 3 character portrait TIMs (16x16 each).
/// Stacked horizontally just below the empty-frame rect; each portrait
/// occupies a 16x16 sub-region.
///
/// These four TIMs are also **widget records `0x86` / `0x87` / `0x88` and
/// `0x8A`** - `FUN_8002C488`'s second arm, the ids that draw through
/// texture page `0x1F` and take their CLUT from the four-word side table at
/// `0x80073DB8`. That table holds VRAM `(976, 304..307)`, which is exactly
/// where these TIMs' own CLUT blocks upload, and the records' rects
/// (`(64|80|96, 0, 16, 16)` and `(64, 16, 32, 32)` on page `0x1F`) address
/// the same VRAM the image blocks occupy. So the "four ids not on the
/// system-UI sheet" are the save-slot portraits and the empty-cell frame -
/// one asset, already baked here and already drawn by
/// `legaia_engine_ui::title_save::slot_grid`.
pub const ATLAS_RECT_PORTRAIT_W: u32 = 16;
pub const ATLAS_RECT_PORTRAIT_H: u32 = 16;
pub const ATLAS_RECT_PORTRAIT_BASE_X: u32 = 200;
pub const ATLAS_RECT_PORTRAIT_BASE_Y: u32 = 96;

/// CLUT row of the system-UI TIM that decodes the panel chrome.
/// Mirror of [`title_pak::OVERLAY_SYSTEM_UI_PANEL_CLUT_ROW`].
pub(super) const PANEL_CLUT_ROW: usize = title_pak::OVERLAY_SYSTEM_UI_PANEL_CLUT_ROW as usize;

/// CLUT row of the system-UI TIM that decodes the pointing-finger
/// cursor. Mirror of [`title_pak::OVERLAY_SYSTEM_UI_CURSOR_CLUT_ROW`].
pub(super) const CURSOR_CLUT_ROW: usize = title_pak::OVERLAY_SYSTEM_UI_CURSOR_CLUT_ROW as usize;

/// Atlas cell of the red **cross-out X** retail lays over a forbidden
/// command chip (`FUN_801DBC30`): 64x16, re-seated from the battle effect
/// page into a block of the atlas the other sources leave transparent.
pub const ATLAS_RECT_CROSS_OUT: (u32, u32, u32, u32) = (0, 48, 64, 16);

/// PROT entry of the battle effect atlas ([`add_cross_out_mark`]'s source;
/// read it with the extended footprint, as the battle-entry upload does).
pub const FLAME_ATLAS_PROT_ENTRY: u32 = 870;

/// VRAM page of the battle effect atlas the X sits on (`tpage 7`, 4bpp).
pub(super) const CROSS_OUT_PAGE: (u16, u16) = (448, 0);
/// Texel rect of the X on that page (`FUN_801DBC30`'s `u 0..=0x3F`,
/// `v 0x60..=0x6F`).
pub(super) const CROSS_OUT_TEXELS: (u32, u32, u32, u32) = (0, 0x60, 64, 16);
/// CLUT the X samples (`0x7704` = VRAM `(64, 476)`).
pub(super) const CROSS_OUT_CLUT: (u16, u16) = (64, 476);

/// Atlas cell of the blue **Rot** stamp retail lays over a refused Attack
/// chip and over each rotted arts-entry direction (`FUN_801DBD04` /
/// `FUN_801DBDDC`): the stamp's own 32x24 texels, re-seated from the battle
/// effect page like the X.
pub const ATLAS_RECT_ROT_STAMP: (u32, u32, u32, u32) = (64, 64, 32, 24);
/// Texel rect of the Rot stamp (`u 0x50..=0x6F`, `v 0x60..=0x77`).
pub(super) const ROT_STAMP_TEXELS: (u32, u32, u32, u32) = (0x50, 0x60, 32, 24);
/// CLUT the Rot stamp samples (`0x770B` = VRAM `(176, 476)`).
pub(super) const ROT_STAMP_CLUT: (u16, u16) = (176, 476);

/// Atlas cell of the blue **Curse** plate retail lays over a refused Magic
/// chip (`FUN_801DBEC4`): 64x16, off the same effect page.
pub const ATLAS_RECT_CURSE_PLATE: (u32, u32, u32, u32) = (0, 64, 64, 16);
/// Texel rect of the Curse plate (`u 0x78..=0xB7`, `v 0x60..=0x6F`).
pub(super) const CURSE_PLATE_TEXELS: (u32, u32, u32, u32) = (0x78, 0x60, 64, 16);
/// CLUT the Curse plate samples (`0x7700` = VRAM `(0, 476)`).
pub(super) const CURSE_PLATE_CLUT: (u16, u16) = (0, 476);
