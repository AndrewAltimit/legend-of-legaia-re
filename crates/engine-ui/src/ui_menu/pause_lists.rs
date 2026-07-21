//! Pause-menu **Items** and **Magic** screen draw builders - the retail
//! multi-window layouts.
//!
//! Retail composes each screen from descriptor-table windows
//! (`legaia_asset::menu_windows`; live sets read from the pause-menu window
//! linked list of the `sol_to_karisto_worldmap` PCSX-Redux captures, driven
//! to each screen by scripted pad walk -
//! `scripts/pcsx-redux/autorun_menu_screen_dump.lua`):
//!
//! - **Items** (draw order): "Items" title tab (id 0, `FUN_801DCA0C`), the
//!   Use / Throw Out / Arrange command window (id 13, `FUN_801D0D18`), the
//!   renderer-less item-list page (id 15) and the item info window (id 17,
//!   `FUN_801DCB60` -> the shared item-info panel `FUN_801D0F1C`). The
//!   id-17 renderer also emits a second framed widget box below itself
//!   (`FUN_8002C69C(WX, WY+0x38, 0x90, 0x28)`) that holds the accessory
//!   passive lines / Point Card points.
//! - **Magic** (draw order): "Magic" title tab (id 1, `FUN_801DCA50`), the
//!   renderer-less spell-list page (id 18), the caster window (id 19,
//!   `FUN_801D2C98`) and the spell info window (id 20, `FUN_801D2E74`).
//!
//! Frames come from the caller (`menu_window_chrome_draws_for`); this module
//! builds the window *content* in 320x240 stage pixels off each window's
//! content-origin pen. Command / caster / info offsets are byte-pinned from
//! the menu-overlay decompiles; the list-page layout (rows, PAGE header,
//! page arrow) is pixel-pinned from the same captures (the retail list
//! drawer is untraced - both list windows are renderer-less in the
//! descriptor table).

use super::field_panels::{MENU_TEXT_GOLD, MENU_TEXT_WHITE, menu_mp_ink, num_field_draws};
use crate::*;

/// Retail grey text ink - CLUT staging index 0, the non-focused-list row
/// ink (VRAM row-510 CLUT entry 15 reads `(132,132,132)`).
pub const MENU_TEXT_GREY: [f32; 4] = [0.517_647_1, 0.517_647_1, 0.517_647_1, 1.0];
/// Retail green text ink - CLUT staging index 4 (skill passives, the
/// magic info window's "MP Used" row; entry 15 reads `(107,222,107)`).
pub const MENU_TEXT_GREEN: [f32; 4] = [0.419_607_8, 0.870_588_2, 0.419_607_8, 1.0];
/// The list-page "PAGE" header label ink - a teal-green distinct from the
/// staged-ink CLUT rows (capture-measured `(16,181,156)`; the header
/// glyphs are small-cap sprites, not dialog-font text).
pub const MENU_TEXT_PAGE_TEAL: [f32; 4] = [0.062_745_1, 0.709_803_9, 0.611_764_7, 1.0];

/// Descriptor-table window ids spawned by the pause-menu Items / Magic
/// screens (live window structs carry the id at `+0x8`; sets read from
/// the menu-open captures).
pub mod pause_list_window_ids {
    /// "Items" title tab (renderer `FUN_801DCA0C`).
    pub const TAB_ITEMS: usize = 0;
    /// "Magic" title tab (renderer `FUN_801DCA50`).
    pub const TAB_MAGIC: usize = 1;
    /// Items screen: Use / Throw Out / Arrange command window
    /// (renderer `FUN_801D0D18`).
    pub const ITEMS_COMMAND: usize = 13;
    /// Items screen: right item-list page (renderer-less container; the
    /// list content is drawn by the items flow, not a window renderer).
    pub const ITEMS_LIST: usize = 15;
    /// Items screen: item info window (renderer `FUN_801DCB60`).
    pub const ITEMS_INFO: usize = 17;
    /// Magic screen: right spell-list page (renderer-less container).
    pub const MAGIC_LIST: usize = 18;
    /// Magic screen: caster (party) window (renderer `FUN_801D2C98`).
    pub const MAGIC_CASTER: usize = 19;
    /// Magic screen: spell info window (renderer `FUN_801D2E74`).
    pub const MAGIC_INFO: usize = 20;
    /// Items screen: Throw Out Yes/No confirm window (renderer
    /// `FUN_801D1B20`; opened by the throw-out SM `FUN_801D8734`
    /// phase 3, sliding the command window out and this one in).
    pub const ITEMS_THROW_CONFIRM: usize = 9;
}

/// Content rect of the Throw Out confirm window (descriptor id 9). It
/// overlays the command window's area - retail closes id 13 while the
/// confirm is open and reopens it on close.
pub const ITEMS_THROW_CONFIRM_RECT: (i32, i32, i32, i32) = (14, 38, 144, 54);

/// Items screen window set with the pinned descriptor content rects, in
/// retail draw order. The rects double as a disc-free fallback for hosts
/// that frame the windows before `legaia_asset::menu_windows` is parsed.
pub const ITEMS_SCREEN_WINDOW_RECTS: [(usize, (i32, i32, i32, i32)); 4] = [
    (pause_list_window_ids::TAB_ITEMS, (16, 12, 60, 12)),
    (pause_list_window_ids::ITEMS_COMMAND, (32, 44, 80, 38)),
    (pause_list_window_ids::ITEMS_LIST, (174, 22, 132, 182)),
    (pause_list_window_ids::ITEMS_INFO, (14, 108, 144, 40)),
];
/// Magic screen window set (see [`ITEMS_SCREEN_WINDOW_RECTS`]).
pub const MAGIC_SCREEN_WINDOW_RECTS: [(usize, (i32, i32, i32, i32)); 4] = [
    (pause_list_window_ids::TAB_MAGIC, (16, 12, 60, 12)),
    (pause_list_window_ids::MAGIC_LIST, (174, 22, 132, 182)),
    (pause_list_window_ids::MAGIC_CASTER, (14, 40, 144, 96)),
    (pause_list_window_ids::MAGIC_INFO, (14, 152, 144, 52)),
];
/// The extra framed widget box the id-17 item-info renderer emits below
/// its own window: `FUN_8002C69C(WX, WY + 0x38, 0x90, 0x28)` - content
/// rect `(14, 164, 144, 40)`. It hosts the accessory passive-effect
/// lines (and the Point Card points readout); hosts must draw its window
/// chrome alongside the id-17 frame.
pub const ITEMS_INFO_EXTRA_BOX_RECT: (i32, i32, i32, i32) = (14, 164, 144, 40);

/// Row pitch of the Items / Magic list pages and of the Use / Throw Out /
/// Arrange command rows (`FUN_801D0D18` steps `+0xE`; list rows measured
/// at the same 14-px pitch on the captures).
pub const PAUSE_LIST_ROW_PITCH: i32 = 0x0e;
/// First list row's pen Y offset from the list window's content origin
/// (row-0 glyph ink tops at content `y + 0xE` on the captures; ink sits
/// 2 px below the pen).
pub const PAUSE_LIST_ROWS_TOP: i32 = 0x0c;
/// Rows visible per list page (both captured screens show 12 rows filling
/// the 182-px content height at the 0xE pitch).
pub const PAUSE_LIST_VISIBLE_ROWS: usize = 12;
/// Caster-window per-member block pitch (`FUN_801D2C98` steps `+0x23`).
pub const MAGIC_CASTER_BLOCK_PITCH: i32 = 0x23;
/// Pen advance the leading element-icon token adds before a spell name
/// (the spell-name strings open with a `0xCE` inline-icon escape; name
/// ink measures 25 px right of the string pen on the captures).
pub const SPELL_ICON_ADVANCE: i32 = 25;
/// Same advance for the wider winged Ra-Seru-magic icon (22 px - "Meta"'s
/// name ink sits 3 px left of the regular spells').
pub const SPELL_ICON_ADVANCE_RA_SERU: i32 = 22;

/// Focus phase of the Items screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PauseItemsPhase {
    /// Hand on the Use / Throw Out / Arrange command window; list rows
    /// draw white (staging 7).
    Command,
    /// Hand inside the item list; every list row drops to the grey
    /// staging-0 ink (the hand is the selection highlight - retail tints
    /// no row).
    List,
}

/// One visible row of the item-list page (the current page's slice).
pub struct PauseItemsRow<'a> {
    pub name: &'a str,
    pub count: u16,
}

/// Item info window content (drawn only while an item is selected -
/// retail gates on `DAT_801E46B0 > 0`).
pub struct PauseItemInfo<'a> {
    pub name: &'a str,
    /// Bag count echoed next to the name (`FUN_801DCB60` re-resolves it
    /// through the bag-slot scan `FUN_80042EE0`).
    pub count: u16,
    pub desc: &'a str,
    /// Accessory passive-effect lines for the extra widget box: the
    /// staging-4 green line at `+0x38` and the staging-7 white line at
    /// `+0x48` (`FUN_801D0F1C` reads them from the accessory-passive
    /// table `0x8007625C`). `None` leaves the box empty.
    pub passive: Option<(&'a str, &'a str)>,
}

/// Plain-data view of the Items screen for [`items_screen_draws_for`].
pub struct PauseItemsView<'a> {
    /// Current page's visible rows (at most
    /// [`PAUSE_LIST_VISIBLE_ROWS`]).
    pub rows: &'a [PauseItemsRow<'a>],
    /// 1-based current page / page count for the header fraction.
    pub page: u16,
    pub pages: u16,
    pub phase: PauseItemsPhase,
    /// Command-window row (0 = Use, 1 = Throw Out, 2 = Arrange).
    pub command_cursor: u8,
    /// List row on the current page.
    pub list_cursor: u8,
    /// Greys the command rows (retail scans the bag and drops to
    /// staging 0 when no slot holds an item).
    pub bag_empty: bool,
    pub info: Option<PauseItemInfo<'a>>,
    /// Emit ASCII `>` cursors. `false` when the caller draws the retail
    /// pointing-hand sprite instead ([`items_screen_sprites_for`]).
    pub text_cursor: bool,
}

/// Shared list-page header + rows: "PAGE cur / total" and the name rows.
/// `name_inset` positions each row's name pen (`+0xC` for items; the
/// spell list adds the element-icon advance per row).
fn list_page_header_draws(
    font: &legaia_font::Font,
    page: u16,
    pages: u16,
    pen: (i32, i32),
) -> Vec<TextDraw> {
    let (lx, ly) = pen;
    let mut out = Vec::new();
    // Header ink tops at content y - 1 on the captures; pen y - 3. The
    // "PAGE" label + fraction digits are small-cap sprite glyphs in
    // retail - dialog-font stand-ins hold their measured columns
    // (label x + 0x4D, fraction cells around x + 0x68..0x7C).
    let hy = ly - 3;
    out.extend(text_draws_for(
        &font.layout_ascii("PAGE"),
        (lx + 0x4d, hy),
        MENU_TEXT_PAGE_TEAL,
    ));
    out.extend(num_field_draws(
        font,
        page as u64,
        lx + 0x68,
        hy,
        2,
        MENU_TEXT_GOLD,
    ));
    out.extend(text_draws_for(
        &font.layout_ascii("/"),
        (lx + 0x74, hy),
        MENU_TEXT_GOLD,
    ));
    out.extend(num_field_draws(
        font,
        pages as u64,
        lx + 0x78,
        hy,
        2,
        MENU_TEXT_GOLD,
    ));
    out
}

/// Build [`TextDraw`]s for the retail Items screen's window contents.
///
/// `cmd_pen` / `list_pen` / `info_pen` are the content origins of the
/// id-13 / id-15 / id-17 descriptor windows. Offsets:
///
/// - command window: "Use" / "Throw Out" / "Arrange" at `(X+0x14,
///   Y + row*0xE)`, staging 7 white - staging 0 grey when the bag is
///   empty (PORT: FUN_801d0d18);
/// - list page: rows from `(X+0xC, Y+0xC)` at the 0xE pitch, count as a
///   2-digit fixed-cell field at `X+0x74`; the whole page draws white
///   while the command window has focus and grey once the hand enters
///   the list (capture-pinned - the retail drawer is untraced);
/// - info window: item name (staging 6 gold) at `(X, Y)`, bag count
///   2-digit gold at `X+0x7C`, description white at `(X, Y+0x10)`, and
///   the accessory passive lines in the extra widget box at `(X,
///   Y+0x38)` (staging 4 green) / `(X, Y+0x48)` (white)
///   (PORT: FUN_801dcb60; PORT: FUN_801d0f1c).
pub fn items_screen_draws_for(
    font: &legaia_font::Font,
    view: &PauseItemsView<'_>,
    cmd_pen: (i32, i32),
    list_pen: (i32, i32),
    info_pen: (i32, i32),
) -> Vec<TextDraw> {
    let mut out = Vec::new();
    let str_at = |out: &mut Vec<TextDraw>, s: &str, x: i32, y: i32, c: [f32; 4]| {
        out.extend(text_draws_for(&font.layout_ascii(s), (x, y), c));
    };

    // Command window (PORT: FUN_801d0d18).
    let (cx, cy) = cmd_pen;
    let cmd_ink = if view.bag_empty {
        MENU_TEXT_GREY
    } else {
        MENU_TEXT_WHITE
    };
    for (i, label) in ["Use", "Throw Out", "Arrange"].iter().enumerate() {
        let y = cy + i as i32 * PAUSE_LIST_ROW_PITCH;
        if view.text_cursor
            && view.phase == PauseItemsPhase::Command
            && i == view.command_cursor as usize
        {
            str_at(&mut out, ">", cx, y, MENU_TEXT_GOLD);
        }
        str_at(&mut out, label, cx + 0x14, y, cmd_ink);
    }

    // List page: header + rows.
    let (lx, ly) = list_pen;
    out.extend(list_page_header_draws(
        font, view.page, view.pages, list_pen,
    ));
    let row_ink = match view.phase {
        PauseItemsPhase::Command => MENU_TEXT_WHITE,
        PauseItemsPhase::List => MENU_TEXT_GREY,
    };
    for (i, row) in view.rows.iter().take(PAUSE_LIST_VISIBLE_ROWS).enumerate() {
        let y = ly + PAUSE_LIST_ROWS_TOP + i as i32 * PAUSE_LIST_ROW_PITCH;
        if view.text_cursor && view.phase == PauseItemsPhase::List && i == view.list_cursor as usize
        {
            str_at(&mut out, ">", lx - 8, y, MENU_TEXT_GOLD);
        }
        str_at(&mut out, row.name, lx + 0x0c, y, row_ink);
        out.extend(num_field_draws(
            font,
            row.count as u64,
            lx + 0x74,
            y,
            2,
            row_ink,
        ));
    }

    // Info window (PORT: FUN_801dcb60 / FUN_801d0f1c).
    if let Some(info) = &view.info {
        let (ix, iy) = info_pen;
        str_at(&mut out, info.name, ix, iy, MENU_TEXT_GOLD);
        out.extend(num_field_draws(
            font,
            info.count as u64,
            ix + 0x7c,
            iy,
            2,
            MENU_TEXT_GOLD,
        ));
        str_at(&mut out, info.desc, ix, iy + 0x10, MENU_TEXT_WHITE);
        if let Some((line1, line2)) = info.passive {
            str_at(&mut out, line1, ix, iy + 0x38, MENU_TEXT_GREEN);
            str_at(&mut out, line2, ix, iy + 0x48, MENU_TEXT_WHITE);
        }
    }
    out
}

/// Build the Items screen's [`SpriteDraw`]s: the pointing-hand cursor in
/// the focused window plus the page-turn arrows, mapped from the 320x240
/// menu stage into surface pixels.
///
/// Hand placements: command rows at `(X, Y + row*0xE)` (the
/// `FUN_801D0D18` cursor call), list rows at `(X-0xC, rowY)` - the shared
/// list-hand offset, measured on the captures. The page-right arrow (the
/// `FUN_8002B994` kind-3 solid triangle) sits at `(X+0x84, Y+(0xB6-16)/2)`,
/// vertically centred on the list window and overlapping its right frame
/// edge; it draws only while more pages follow. The left arrow mirrors it
/// at `X-0x14` on earlier pages (position inferred - page 1 is the only
/// captured state).
///
/// PORT: FUN_801d0d18 (command hand). REF: FUN_8002b994.
#[allow(clippy::too_many_arguments)]
pub fn items_screen_sprites_for(
    rects: &SaveMenuAtlasRects,
    phase: PauseItemsPhase,
    command_cursor: u8,
    list_cursor: u8,
    page: u16,
    pages: u16,
    cmd_pen: (i32, i32),
    list_pen: (i32, i32),
    stage_origin: (i32, i32),
    stage_scale: u32,
) -> Vec<SpriteDraw> {
    let mut out = Vec::new();
    match phase {
        PauseItemsPhase::Command => {
            push_stage_sprite(
                &mut out,
                rects.cursor,
                (
                    cmd_pen.0,
                    cmd_pen.1 + command_cursor as i32 * PAUSE_LIST_ROW_PITCH,
                ),
                stage_origin,
                stage_scale,
            );
        }
        PauseItemsPhase::List => {
            push_stage_sprite(
                &mut out,
                rects.cursor,
                list_hand_pos(list_pen, list_cursor),
                stage_origin,
                stage_scale,
            );
        }
    }
    out.extend(page_arrow_sprites(
        rects,
        page,
        pages,
        list_pen,
        stage_origin,
        stage_scale,
    ));
    out
}

/// Throw Out confirm window content for
/// [`items_throw_confirm_draws_for`].
pub struct PauseThrowConfirmView<'a> {
    /// Name of the stack about to be discarded.
    pub name: &'a str,
    /// Its bag count (the whole stack goes).
    pub count: u16,
    /// 0 = Yes, 1 = No (retail seeds `DAT_801E46D0` to 1 - No).
    pub cursor: u8,
    /// Emit an ASCII `>` cursor instead of the hand sprite.
    pub text_cursor: bool,
}

/// Y offset of the "Yes" row from the confirm window's content origin.
pub const THROW_CONFIRM_YES_Y: i32 = 0x1c;
/// Y offset of the "No" row.
pub const THROW_CONFIRM_NO_Y: i32 = 0x2a;
/// X offset of the Yes / No labels.
pub const THROW_CONFIRM_CHOICE_X: i32 = 0x3c;
/// X offset of the Yes / No hand cursor.
pub const THROW_CONFIRM_HAND_X: i32 = 0x28;

/// Build [`TextDraw`]s for the Throw Out confirm window (descriptor
/// id 9, rect [`ITEMS_THROW_CONFIRM_RECT`]). Retail layout, all from the
/// window's content origin `(WX, WY)`:
///
/// - item name (staging 7 white) at `(WX, WY)`, the bag count right of
///   the name (the retail pen is `WX + 8 + name_glyphs*0xC`; the port
///   uses the proportional name advance + 8), then "You are about to"
///   8 px past the count (16 px for a 2-digit count);
/// - "Throw out?" at `(WX+6, WY+0xE)`;
/// - "Yes" / "No" (staging 5 teal) at `WX+0x3C` on rows `WY+0x1C` /
///   `WY+0x2A`, hand cursor at `WX+0x28` on the focused row.
///
/// PORT: FUN_801D1B20
pub fn items_throw_confirm_draws_for(
    font: &legaia_font::Font,
    view: &PauseThrowConfirmView<'_>,
    pen: (i32, i32),
) -> Vec<TextDraw> {
    let (wx, wy) = pen;
    let mut out = Vec::new();
    let str_at = |out: &mut Vec<TextDraw>, s: &str, x: i32, y: i32, c: [f32; 4]| {
        out.extend(text_draws_for(&font.layout_ascii(s), (x, y), c));
    };
    let name_layout = font.layout_ascii(view.name);
    out.extend(text_draws_for(&name_layout, (wx, wy), MENU_TEXT_WHITE));
    let count_x = wx + 8 + name_layout.advance_x as i32;
    out.extend(num_field_draws(
        font,
        view.count as u64,
        count_x,
        wy,
        1,
        MENU_TEXT_WHITE,
    ));
    let after_count = count_x + if view.count >= 10 { 0x10 } else { 0x8 };
    str_at(
        &mut out,
        "You are about to",
        after_count,
        wy,
        MENU_TEXT_WHITE,
    );
    str_at(&mut out, "Throw out?", wx + 6, wy + 0x0e, MENU_TEXT_WHITE);
    let teal = super::system_menus::OPTIONS_INK_TEAL;
    for (label, y, row) in [
        ("Yes", wy + THROW_CONFIRM_YES_Y, 0u8),
        ("No", wy + THROW_CONFIRM_NO_Y, 1u8),
    ] {
        if view.text_cursor && view.cursor == row {
            str_at(&mut out, ">", wx + THROW_CONFIRM_HAND_X, y, MENU_TEXT_GOLD);
        }
        str_at(&mut out, label, wx + THROW_CONFIRM_CHOICE_X, y, teal);
    }
    out
}

/// The confirm window's hand-cursor sprite at `(WX+0x28, rowY)` for the
/// focused Yes / No row.
///
/// PORT: FUN_801D1B20 (hand placement; the cursor word is `DAT_801E46D0`)
pub fn items_throw_confirm_sprites_for(
    rects: &SaveMenuAtlasRects,
    cursor: u8,
    pen: (i32, i32),
    stage_origin: (i32, i32),
    stage_scale: u32,
) -> Vec<SpriteDraw> {
    let (wx, wy) = pen;
    let y = if cursor == 0 {
        wy + THROW_CONFIRM_YES_Y
    } else {
        wy + THROW_CONFIRM_NO_Y
    };
    let mut out = Vec::new();
    push_stage_sprite(
        &mut out,
        rects.cursor,
        (wx + THROW_CONFIRM_HAND_X, y),
        stage_origin,
        stage_scale,
    );
    out
}

/// Focus phase of the Magic screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PauseMagicPhase {
    /// Hand on the caster window's party blocks; list rows draw white.
    Caster,
    /// Hand inside the spell list; list rows drop to the grey staging-0
    /// ink (same focus behaviour as the Items list).
    List,
}

/// One caster block of the Magic screen's party window.
pub struct PauseMagicCaster<'a> {
    pub name: &'a str,
    pub level: u16,
    pub mp: u16,
    pub mp_max: u16,
}

/// One visible row of the spell-list page.
pub struct PauseMagicRow<'a> {
    pub name: &'a str,
    /// `true` for Ra-Seru magic (Meta / Ozma / Terra block) - retail
    /// leads the name with the wider winged icon, shifting the name pen
    /// left by 3 px.
    pub ra_seru: bool,
}

/// Spell info window content (drawn only while a spell is selected).
pub struct PauseMagicInfo<'a> {
    pub name: &'a str,
    /// Learned spell level (the single digit after "Lv").
    pub level: u8,
    /// Description - up to two lines split on `\n`, drawn at the 0xE
    /// line pitch.
    pub desc: &'a str,
    /// Per-caster MP cost (retail runs the base cost through the
    /// MP-cost kernel `FUN_80035394`).
    pub mp_cost: u16,
}

/// Plain-data view of the Magic screen for [`magic_screen_draws_for`].
pub struct PauseMagicView<'a> {
    pub casters: &'a [PauseMagicCaster<'a>],
    /// Current page's visible spell rows.
    pub rows: &'a [PauseMagicRow<'a>],
    pub page: u16,
    pub pages: u16,
    pub phase: PauseMagicPhase,
    /// Caster block under the hand.
    pub caster_cursor: u8,
    /// List row on the current page.
    pub list_cursor: u8,
    pub info: Option<PauseMagicInfo<'a>>,
    /// `true` when the caller draws the LV / MP label tags as UI-icon
    /// atlas sprites ([`magic_screen_sprites_for`]) - suppresses the
    /// ASCII stand-ins here.
    pub label_icons: bool,
    pub text_cursor: bool,
}

/// Build [`TextDraw`]s for the retail Magic screen's window contents.
///
/// `caster_pen` / `list_pen` / `info_pen` are the content origins of the
/// id-19 / id-18 / id-20 descriptor windows. Offsets:
///
/// - caster window: per-member block at `Yb = Y + 1 + i*0x23` - name
///   (staging 7) at `X+0x14`, LV tag at `(X+0x60, Yb+2)` with the
///   2-digit level at `X+0x70`, MP tag at `(X+0x24, Yb+0x10)` with the
///   4-digit current / `/` / 4-digit max at `X+0x34 / X+0x54 / X+0x5C`
///   on row `Yb+0xE` - the MP numbers take the [`menu_mp_ink`] tier, the
///   slash stays white (PORT: FUN_801d2c98);
/// - spell list: rows from `(X+0xC, Y+0xC)` at the 0xE pitch. Each retail
///   row is one string whose leading `0xCE` escape draws the element
///   icon, so the name pen sits [`SPELL_ICON_ADVANCE`] right of the row
///   pen ([`SPELL_ICON_ADVANCE_RA_SERU`] for the winged Ra-Seru icon);
///   the icon sprites themselves are not yet ported - the gap holds
///   their place. White while the caster window has focus, grey in list
///   focus (capture-pinned);
/// - info window: spell name gold at `(X + icon_advance, Y)` (the same
///   leading-icon string), "Lv<n>" gold at `X+0x78`, description white
///   from `(X, Y+0xE)` at the 0xE line pitch, "MP Used" (staging 4
///   green) at `(X+0x18, Y+0x2A)` with the 3-digit cost - also green -
///   at `X+0x74` (PORT: FUN_801d2e74).
pub fn magic_screen_draws_for(
    font: &legaia_font::Font,
    view: &PauseMagicView<'_>,
    caster_pen: (i32, i32),
    list_pen: (i32, i32),
    info_pen: (i32, i32),
) -> Vec<TextDraw> {
    let mut out = Vec::new();
    let str_at = |out: &mut Vec<TextDraw>, s: &str, x: i32, y: i32, c: [f32; 4]| {
        out.extend(text_draws_for(&font.layout_ascii(s), (x, y), c));
    };

    // Caster window (PORT: FUN_801d2c98).
    let (cx, cy) = caster_pen;
    for (i, caster) in view.casters.iter().enumerate() {
        let yb = cy + 1 + i as i32 * MAGIC_CASTER_BLOCK_PITCH;
        if view.text_cursor
            && view.phase == PauseMagicPhase::Caster
            && i == view.caster_cursor as usize
        {
            str_at(&mut out, ">", cx, yb, MENU_TEXT_GOLD);
        }
        str_at(&mut out, caster.name, cx + 0x14, yb, MENU_TEXT_WHITE);
        if !view.label_icons {
            str_at(&mut out, "LV", cx + 0x60, yb + 2, MENU_TEXT_WHITE);
        }
        out.extend(num_field_draws(
            font,
            caster.level as u64,
            cx + 0x70,
            yb,
            2,
            MENU_TEXT_WHITE,
        ));
        if !view.label_icons {
            str_at(&mut out, "MP", cx + 0x24, yb + 0x10, MENU_TEXT_WHITE);
        }
        let mp_ink = menu_mp_ink(caster.mp, caster.mp_max);
        let mp_y = yb + 0x0e;
        out.extend(num_field_draws(
            font,
            caster.mp as u64,
            cx + 0x34,
            mp_y,
            4,
            mp_ink,
        ));
        str_at(&mut out, "/", cx + 0x54, mp_y, MENU_TEXT_WHITE);
        out.extend(num_field_draws(
            font,
            caster.mp_max as u64,
            cx + 0x5c,
            mp_y,
            4,
            mp_ink,
        ));
    }

    // Spell list page: header + rows.
    let (lx, ly) = list_pen;
    out.extend(list_page_header_draws(
        font, view.page, view.pages, list_pen,
    ));
    let row_ink = match view.phase {
        PauseMagicPhase::Caster => MENU_TEXT_WHITE,
        PauseMagicPhase::List => MENU_TEXT_GREY,
    };
    for (i, row) in view.rows.iter().take(PAUSE_LIST_VISIBLE_ROWS).enumerate() {
        let y = ly + PAUSE_LIST_ROWS_TOP + i as i32 * PAUSE_LIST_ROW_PITCH;
        if view.text_cursor && view.phase == PauseMagicPhase::List && i == view.list_cursor as usize
        {
            str_at(&mut out, ">", lx - 8, y, MENU_TEXT_GOLD);
        }
        let advance = if row.ra_seru {
            SPELL_ICON_ADVANCE_RA_SERU
        } else {
            SPELL_ICON_ADVANCE
        };
        str_at(&mut out, row.name, lx + 0x0c + advance, y, row_ink);
    }

    // Info window (PORT: FUN_801d2e74).
    if let Some(info) = &view.info {
        let (ix, iy) = info_pen;
        str_at(
            &mut out,
            info.name,
            ix + SPELL_ICON_ADVANCE,
            iy,
            MENU_TEXT_GOLD,
        );
        let lv = format!("Lv{}", info.level);
        str_at(&mut out, &lv, ix + 0x78, iy, MENU_TEXT_GOLD);
        for (k, line) in info.desc.split('\n').take(2).enumerate() {
            str_at(
                &mut out,
                line,
                ix,
                iy + 0x0e + k as i32 * 0x0e,
                MENU_TEXT_WHITE,
            );
        }
        str_at(&mut out, "MP Used", ix + 0x18, iy + 0x2a, MENU_TEXT_GREEN);
        out.extend(num_field_draws(
            font,
            info.mp_cost as u64,
            ix + 0x74,
            iy + 0x2a,
            3,
            MENU_TEXT_GREEN,
        ));
    }
    out
}

/// Build the Magic screen's [`SpriteDraw`]s: the pointing-hand cursor in
/// the focused window, the caster blocks' LV / MP label tags (the same
/// row-1 UI-icon records the status page uses) and the page-turn arrows.
///
/// Hand placements: caster blocks at `(X, Y + 1 + block*0x23)` (the
/// `FUN_801D2C98` cursor call), list rows at `(X-0xC, rowY)`. Label tags:
/// LV at `(X+0x60, Yb+2)`, MP at `(X+0x24, Yb+0x10)` per block.
///
/// PORT: FUN_801d2c98 (caster hand + label placement). REF: FUN_8002b994.
#[allow(clippy::too_many_arguments)]
pub fn magic_screen_sprites_for(
    rects: &SaveMenuAtlasRects,
    n_casters: usize,
    phase: PauseMagicPhase,
    caster_cursor: u8,
    list_cursor: u8,
    page: u16,
    pages: u16,
    caster_pen: (i32, i32),
    list_pen: (i32, i32),
    stage_origin: (i32, i32),
    stage_scale: u32,
) -> Vec<SpriteDraw> {
    let mut out = Vec::new();
    let (cx, cy) = caster_pen;
    for i in 0..n_casters {
        let yb = cy + 1 + i as i32 * MAGIC_CASTER_BLOCK_PITCH;
        push_stage_sprite(
            &mut out,
            rects.label_lv,
            (cx + 0x60, yb + 2),
            stage_origin,
            stage_scale,
        );
        push_stage_sprite(
            &mut out,
            rects.label_mp,
            (cx + 0x24, yb + 0x10),
            stage_origin,
            stage_scale,
        );
    }
    match phase {
        PauseMagicPhase::Caster => {
            push_stage_sprite(
                &mut out,
                rects.cursor,
                (cx, cy + 1 + caster_cursor as i32 * MAGIC_CASTER_BLOCK_PITCH),
                stage_origin,
                stage_scale,
            );
        }
        PauseMagicPhase::List => {
            push_stage_sprite(
                &mut out,
                rects.cursor,
                list_hand_pos(list_pen, list_cursor),
                stage_origin,
                stage_scale,
            );
        }
    }
    out.extend(page_arrow_sprites(
        rects,
        page,
        pages,
        list_pen,
        stage_origin,
        stage_scale,
    ));
    out
}

/// List-window hand-cursor stage position: `(X-0xC, Y+0xC + row*0xE)`.
fn list_hand_pos(list_pen: (i32, i32), row: u8) -> (i32, i32) {
    (
        list_pen.0 - 0x0c,
        list_pen.1 + PAUSE_LIST_ROWS_TOP + row as i32 * PAUSE_LIST_ROW_PITCH,
    )
}

/// Content height of the id-15 / id-18 list windows (descriptor `h`).
const LIST_CONTENT_H: i32 = 182;

/// Page-turn arrow sprites for a list page: the right arrow at
/// `(X+0x84, Y + (182-16)/2)` while more pages follow (capture-pinned);
/// the left arrow mirrored at `X-0x14` on later pages (inferred - not
/// yet captured).
fn page_arrow_sprites(
    rects: &SaveMenuAtlasRects,
    page: u16,
    pages: u16,
    list_pen: (i32, i32),
    stage_origin: (i32, i32),
    stage_scale: u32,
) -> Vec<SpriteDraw> {
    let mut out = Vec::new();
    let (lx, ly) = list_pen;
    let arrow_y = ly + (LIST_CONTENT_H - 16) / 2;
    if page < pages {
        push_stage_sprite(
            &mut out,
            rects.pager_right,
            (lx + 0x84, arrow_y),
            stage_origin,
            stage_scale,
        );
    }
    if page > 1 {
        push_stage_sprite(
            &mut out,
            rects.pager_left,
            (lx - 0x14, arrow_y),
            stage_origin,
            stage_scale,
        );
    }
    out
}

/// Push one atlas sprite at a 320x240 stage position, mapped into surface
/// pixels with the shared stage transform (matching
/// [`crate::menu_window_chrome_draws_for`]).
fn push_stage_sprite(
    out: &mut Vec<SpriteDraw>,
    src: (u32, u32, u32, u32),
    stage_pos: (i32, i32),
    stage_origin: (i32, i32),
    stage_scale: u32,
) {
    let scale = stage_scale.max(1) as i32;
    let (_, _, w, h) = src;
    out.push(SpriteDraw {
        dst: (
            stage_origin.0 + stage_pos.0 * scale,
            stage_origin.1 + stage_pos.1 * scale,
            w * stage_scale,
            h * stage_scale,
        ),
        src,
        color: [1.0, 1.0, 1.0, 1.0],
    });
}

#[cfg(test)]
mod pause_list_tests {
    use super::*;
    use crate::MENU_TEXT_ORANGE;

    /// Minimal [`SaveMenuAtlasRects`] for placement tests - only the
    /// sprite sources these builders touch carry their byte-pinned
    /// sizes; everything else is zeroed.
    fn test_rects() -> SaveMenuAtlasRects {
        let z = (0, 0, 0, 0);
        SaveMenuAtlasRects {
            panel_tl: z,
            panel_tr: z,
            panel_bl: z,
            panel_br: z,
            panel_top: z,
            panel_bot: z,
            panel_left: z,
            panel_right: z,
            slot1: z,
            slot2: z,
            cursor: (152, 64, 16, 16),
            panel_interior: z,
            panel_filigree: z,
            label_lv: (40, 232, 16, 10),
            label_hp: z,
            label_mp: (80, 232, 16, 10),
            icon_money: z,
            label_time: z,
            label_coin: z,
            gauge_cap: z,
            gauge_trough: z,
            gauge_box: z,
            gauge_tip: z,
            gauge_digits: z,
            gauge_100: z,
            gauge_fill: z,
            dialog_fill: z,
            icon_weapon: z,
            icon_helmet: z,
            icon_armor: z,
            icon_boot: z,
            icon_goods: z,
            pager_left: (104, 232, 16, 16),
            pager_right: (124, 232, 16, 16),
            tab_cap_l: z,
            tab_body: z,
            tab_cap_r: z,
            atr_icons: [z; 3],
            load_empty_frame: None,
            load_portrait_by_char: [None; 3],
        }
    }

    fn draw_at(draws: &[TextDraw], x: i32, y: i32) -> bool {
        draws.iter().any(|d| d.dst.0 == x && d.dst.1 == y)
    }

    fn items_view<'a>(rows: &'a [PauseItemsRow<'a>], phase: PauseItemsPhase) -> PauseItemsView<'a> {
        PauseItemsView {
            rows,
            page: 1,
            pages: 6,
            phase,
            command_cursor: 0,
            list_cursor: 0,
            bag_empty: false,
            info: None,
            text_cursor: false,
        }
    }

    /// Command rows sit at the FUN_801d0d18 pens: `(X+0x14, Y + row*0xE)`.
    #[test]
    fn items_command_rows_at_pinned_pens() {
        let font = legaia_font::synthetic_for_tests();
        let rows = [];
        let draws = items_screen_draws_for(
            &font,
            &items_view(&rows, PauseItemsPhase::Command),
            (32, 44),
            (174, 22),
            (14, 108),
        );
        assert!(draw_at(&draws, 32 + 0x14, 44)); // Use
        assert!(draw_at(&draws, 32 + 0x14, 44 + 0x0e)); // Throw Out
        assert!(draw_at(&draws, 32 + 0x14, 44 + 0x1c)); // Arrange
    }

    /// List rows: name pen at `X+0xC`, first row at `Y+0xC`, 0xE pitch,
    /// count cells right-aligned in the 2-digit field at `X+0x74`.
    #[test]
    fn items_list_rows_at_pinned_pens() {
        let font = legaia_font::synthetic_for_tests();
        let rows = [
            PauseItemsRow {
                name: "Medicine",
                count: 15,
            },
            PauseItemsRow {
                name: "Antidote",
                count: 1,
            },
        ];
        let draws = items_screen_draws_for(
            &font,
            &items_view(&rows, PauseItemsPhase::Command),
            (32, 44),
            (174, 22),
            (14, 108),
        );
        assert!(draw_at(&draws, 174 + 0x0c, 22 + 0x0c));
        assert!(draw_at(&draws, 174 + 0x0c, 22 + 0x0c + 0x0e));
        // "15" fills both 8-px cells of the count field.
        assert!(draw_at(&draws, 174 + 0x74, 22 + 0x0c));
        assert!(draw_at(&draws, 174 + 0x74 + 8, 22 + 0x0c));
        // "1" right-aligns into the second cell only.
        assert!(draw_at(&draws, 174 + 0x74 + 8, 22 + 0x0c + 0x0e));
        assert!(!draw_at(&draws, 174 + 0x74, 22 + 0x0c + 0x0e));
    }

    /// The list page drops from staging-7 white to staging-0 grey when
    /// the hand enters it (capture-pinned focus behaviour).
    #[test]
    fn items_list_ink_follows_focus_phase() {
        let font = legaia_font::synthetic_for_tests();
        let rows = [PauseItemsRow {
            name: "Medicine",
            count: 15,
        }];
        let white = items_screen_draws_for(
            &font,
            &items_view(&rows, PauseItemsPhase::Command),
            (32, 44),
            (174, 22),
            (14, 108),
        );
        let grey = items_screen_draws_for(
            &font,
            &items_view(&rows, PauseItemsPhase::List),
            (32, 44),
            (174, 22),
            (14, 108),
        );
        let row_y = 22 + 0x0c;
        let ink_of = |draws: &[TextDraw]| {
            draws
                .iter()
                .find(|d| d.dst.1 == row_y && d.dst.0 >= 174)
                .map(|d| d.color)
                .unwrap()
        };
        assert_eq!(ink_of(&white), MENU_TEXT_WHITE);
        assert_eq!(ink_of(&grey), MENU_TEXT_GREY);
    }

    /// Empty bag greys the command rows (the FUN_801d0d18 bag scan).
    #[test]
    fn items_empty_bag_greys_commands() {
        let font = legaia_font::synthetic_for_tests();
        let rows = [];
        let mut view = items_view(&rows, PauseItemsPhase::Command);
        view.bag_empty = true;
        let draws = items_screen_draws_for(&font, &view, (32, 44), (174, 22), (14, 108));
        let use_ink = draws
            .iter()
            .find(|d| d.dst.0 >= 32 + 0x14 && d.dst.1 == 44)
            .map(|d| d.color)
            .unwrap();
        assert_eq!(use_ink, MENU_TEXT_GREY);
    }

    /// Info window: name/count gold at the FUN_801dcb60 pens, passive
    /// lines land in the extra widget box at `+0x38 / +0x48`.
    #[test]
    fn items_info_window_at_pinned_pens() {
        let font = legaia_font::synthetic_for_tests();
        let rows = [];
        let mut view = items_view(&rows, PauseItemsPhase::List);
        view.info = Some(PauseItemInfo {
            name: "Medicine",
            count: 15,
            desc: "Cure all status. Ally.",
            passive: Some(("Auto block", "Guards sometimes")),
        });
        let draws = items_screen_draws_for(&font, &view, (32, 44), (174, 22), (14, 108));
        assert!(draw_at(&draws, 14, 108)); // name
        assert!(draw_at(&draws, 14 + 0x7c, 108)); // count first cell
        assert!(draw_at(&draws, 14, 108 + 0x10)); // description
        assert!(draw_at(&draws, 14, 108 + 0x38)); // passive line 1
        assert!(draw_at(&draws, 14, 108 + 0x48)); // passive line 2
        let name_ink = draws
            .iter()
            .find(|d| d.dst.0 == 14 && d.dst.1 == 108)
            .map(|d| d.color)
            .unwrap();
        assert_eq!(name_ink, MENU_TEXT_GOLD);
    }

    fn magic_view<'a>(
        casters: &'a [PauseMagicCaster<'a>],
        rows: &'a [PauseMagicRow<'a>],
        phase: PauseMagicPhase,
    ) -> PauseMagicView<'a> {
        PauseMagicView {
            casters,
            rows,
            page: 1,
            pages: 1,
            phase,
            caster_cursor: 0,
            list_cursor: 0,
            info: None,
            label_icons: false,
            text_cursor: false,
        }
    }

    /// Caster blocks step at the FUN_801d2c98 pitch 0x23 with the pinned
    /// member-row pens; MP numbers take the tier ink, the slash stays
    /// white.
    #[test]
    fn magic_caster_blocks_at_pinned_pens() {
        let font = legaia_font::synthetic_for_tests();
        let casters = [
            PauseMagicCaster {
                name: "Vahn",
                level: 37,
                mp: 398,
                mp_max: 398,
            },
            PauseMagicCaster {
                name: "Noa",
                level: 37,
                mp: 50,
                mp_max: 435,
            },
        ];
        let draws = magic_screen_draws_for(
            &font,
            &magic_view(&casters, &[], PauseMagicPhase::Caster),
            (14, 40),
            (174, 22),
            (14, 152),
        );
        let yb0 = 40 + 1;
        let yb1 = yb0 + 0x23;
        assert!(draw_at(&draws, 14 + 0x14, yb0)); // Vahn
        assert!(draw_at(&draws, 14 + 0x14, yb1)); // Noa
        assert!(draw_at(&draws, 14 + 0x54, yb0 + 0x0e)); // slash
        // Full-MP member draws white; quarter-tank member drops to the
        // orange tier (menu_mp_ink).
        let num_ink = |y: i32| {
            draws
                .iter()
                .find(|d| d.dst.0 >= 14 + 0x34 && d.dst.0 < 14 + 0x54 && d.dst.1 == y)
                .map(|d| d.color)
                .unwrap()
        };
        assert_eq!(num_ink(yb0 + 0x0e), MENU_TEXT_WHITE);
        assert_eq!(num_ink(yb1 + 0x0e), MENU_TEXT_ORANGE);
    }

    /// Spell rows leave the element-icon gap: regular names at
    /// `X+0xC+25`, Ra-Seru names at `X+0xC+22`.
    #[test]
    fn magic_list_rows_leave_icon_gap() {
        let font = legaia_font::synthetic_for_tests();
        let rows = [
            PauseMagicRow {
                name: "Mushura",
                ra_seru: false,
            },
            PauseMagicRow {
                name: "Meta",
                ra_seru: true,
            },
        ];
        let draws = magic_screen_draws_for(
            &font,
            &magic_view(&[], &rows, PauseMagicPhase::List),
            (14, 40),
            (174, 22),
            (14, 152),
        );
        let y0 = 22 + 0x0c;
        assert!(draw_at(&draws, 174 + 0x0c + SPELL_ICON_ADVANCE, y0));
        assert!(draw_at(
            &draws,
            174 + 0x0c + SPELL_ICON_ADVANCE_RA_SERU,
            y0 + 0x0e
        ));
    }

    /// Info window: gold name + level, white description lines at the
    /// 0xE pitch, green "MP Used" + cost at the FUN_801d2e74 pens.
    #[test]
    fn magic_info_window_at_pinned_pens() {
        let font = legaia_font::synthetic_for_tests();
        let mut view = magic_view(&[], &[], PauseMagicPhase::List);
        view.info = Some(PauseMagicInfo {
            name: "Mushura",
            level: 1,
            desc: "Crazy Driver\nAttack enemies.",
            mp_cost: 60,
        });
        let draws = magic_screen_draws_for(&font, &view, (14, 40), (174, 22), (14, 152));
        assert!(draw_at(&draws, 14 + SPELL_ICON_ADVANCE, 152)); // name
        assert!(draw_at(&draws, 14 + 0x78, 152)); // Lv1
        assert!(draw_at(&draws, 14, 152 + 0x0e)); // desc line 1
        assert!(draw_at(&draws, 14, 152 + 0x1c)); // desc line 2
        assert!(draw_at(&draws, 14 + 0x18, 152 + 0x2a)); // MP Used
        let cost_ink = draws
            .iter()
            .find(|d| d.dst.0 >= 14 + 0x74 && d.dst.1 == 152 + 0x2a)
            .map(|d| d.color)
            .unwrap();
        assert_eq!(cost_ink, MENU_TEXT_GREEN);
    }

    /// Throw Out confirm window (FUN_801D1B20): name at the content
    /// origin, count right of the name, "Throw out?" at `(X+6, Y+0xE)`,
    /// Yes / No teal at `X+0x3C` on rows `Y+0x1C` / `Y+0x2A`, hand at
    /// `X+0x28` on the focused row.
    #[test]
    fn throw_confirm_draws_at_pinned_pens() {
        let font = legaia_font::synthetic_for_tests();
        let (wx, wy) = (14, 38);
        let view = PauseThrowConfirmView {
            name: "Medicine",
            count: 12,
            cursor: 1,
            text_cursor: true,
        };
        let draws = items_throw_confirm_draws_for(&font, &view, (wx, wy));
        assert!(draw_at(&draws, wx, wy)); // name
        assert!(draw_at(&draws, wx + 6, wy + 0x0e)); // Throw out?
        assert!(draw_at(&draws, wx + 0x3c, wy + 0x1c)); // Yes
        assert!(draw_at(&draws, wx + 0x3c, wy + 0x2a)); // No
        // Hand cursor (ASCII stand-in) on the "No" row (retail default).
        assert!(draw_at(&draws, wx + 0x28, wy + 0x2a));
        assert!(!draw_at(&draws, wx + 0x28, wy + 0x1c));
        // Yes / No stage the retail teal ink 5.
        let yes_ink = draws
            .iter()
            .find(|d| d.dst.0 >= wx + 0x3c && d.dst.1 == wy + 0x1c)
            .map(|d| d.color)
            .unwrap();
        assert_eq!(yes_ink, super::super::system_menus::OPTIONS_INK_TEAL);
        // The count sits right of the proportional name advance.
        let name_w = font.layout_ascii("Medicine").advance_x as i32;
        assert!(draw_at(&draws, wx + 8 + name_w, wy));
        // Sprite variant: hand at (X+0x28, rowY) for the focused row.
        let sprites = items_throw_confirm_sprites_for(&test_rects(), 0, (wx, wy), (0, 0), 1);
        assert!(
            sprites
                .iter()
                .any(|d| d.dst.0 == wx + 0x28 && d.dst.1 == wy + 0x1c)
        );
    }

    /// Sprite placement: command hand at `(X, rowY)`, list hand at
    /// `(X-0xC, rowY)`; the page-right arrow draws only while more pages
    /// follow.
    #[test]
    fn sprites_follow_phase_and_pages() {
        let rects = test_rects();
        let items_cmd = items_screen_sprites_for(
            &rects,
            PauseItemsPhase::Command,
            1,
            0,
            1,
            1,
            (32, 44),
            (174, 22),
            (0, 0),
            1,
        );
        assert!(
            items_cmd
                .iter()
                .any(|d| d.dst.0 == 32 && d.dst.1 == 44 + 0x0e)
        );
        let items_list = items_screen_sprites_for(
            &rects,
            PauseItemsPhase::List,
            0,
            2,
            1,
            6,
            (32, 44),
            (174, 22),
            (0, 0),
            1,
        );
        assert!(
            items_list
                .iter()
                .any(|d| d.dst.0 == 174 - 0x0c && d.dst.1 == 22 + 0x0c + 2 * 0x0e)
        );
        // Page 1 of 6 -> right arrow at the pinned spot, no left arrow.
        let arrow_y = 22 + (LIST_CONTENT_H - 16) / 2;
        assert!(
            items_list
                .iter()
                .any(|d| d.dst.0 == 174 + 0x84 && d.dst.1 == arrow_y)
        );
        assert!(!items_list.iter().any(|d| d.dst.0 == 174 - 0x14));
        // Single page -> no arrows at the arrow row besides the hand.
        assert!(!items_cmd.iter().any(|d| d.dst.1 == arrow_y));
    }
}
