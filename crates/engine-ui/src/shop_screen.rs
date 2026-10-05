//! The gold shop's **screen composition**: the frames of the windows a shop
//! screen shows, the Buy / Sell / Quit picker, the paged buy and sell lists,
//! the party-compare column, and the sprite half of the window painters'
//! cursor / pictogram requests.
//!
//! Retail's shop is a set of menu-overlay descriptor windows, each framed by
//! the caller and filled by its own content renderer (or, for the two
//! renderer-less list windows, by the SCUS kind-4 list kernel
//! `FUN_80032A44`). Which windows are up is the widget scripts' business -
//! a host gets the set from `engine-core::shop::shop_screen_windows`. This
//! module turns that set plus the live content into draws, so the native
//! `play-window` and the browser play page draw one screen rather than two.
//!
//! Texts come back in **stage** pixels (320x240, the shop text pipeline both
//! hosts scale in one place); sprites come back in **surface** pixels off the
//! shared stage transform, like every other chrome builder in this crate.
//! Without the chrome atlas the frames and sprites are absent and the hand /
//! pictograms fall back to ASCII glyphs, so a disc-less run keeps the layout.
//!
//! The list geometry is the kernel's (`docs/subsystems/field-menu.md`, "The
//! kind-4 list kernel"): `visible = (content_h - 4) / 0xE` rows, the block
//! vertically centred at `row0 = WY + (content_h - visible * 0xE) / 2 + 5`;
//! the hand at `WX - 6`; the page triangles at `WX + w + 4` / `WX - 0xC`,
//! `WY + h / 2 - 3`, drawn only while the list has the pad; the PAGE header
//! at `WX + W - 0x38`. Shop rows (class `0x3000` / `0xA000`) put the name at
//! `WX + 0x18` and a 5-digit price at `WX + 0x80`; bag rows (the sell list)
//! put the name at `WX + 0xC` and a 3-cell count at `WX + 0x6C`. A parked
//! list (the buy list behind the root picker) draws no hand and keeps every
//! row in its pen, which is why retail's root screen shows the stock white.
//!
//! REF: FUN_80032A44 - the list kernel whose draw half this lays out.
//! REF: FUN_801D4868 - the Buy / Sell / Quit picker (window 42).

use crate::ui_menu_window_painters::{PainterPictogram, PainterSprite};
use crate::{
    EquipStatBlock, PartyCompareMemberView, PartyCompareOutcome, SaveMenuAtlasRects, SpriteDraw,
    TextDraw, text_draws_for,
};

/// Window 31 - the Point Card toast.
pub const WIN_SHOP_POINT_CARD: usize = 31;
/// Window 32 - the purse.
pub const WIN_SHOP_PURSE: usize = 32;
/// Window 33 - the vendor-name plate (a kind-2 title tab).
pub const WIN_SHOP_VENDOR: usize = 33;
/// Window 34 - the hovered item's info panel.
pub const WIN_SHOP_ITEM_INFO: usize = 34;
/// Window 38 - the sell list (content id 2, renderer-less).
pub const WIN_SHOP_SELL_LIST: usize = 38;
/// Window 39 - the sell-list detail panel. Its renderer also frames a
/// widget box `0x90 x 0x28` at `(WX, WY + 0x45)`.
pub const WIN_SHOP_SELL_DETAIL: usize = 39;
/// Window 40 - the buy list (content id `0xB`, renderer-less).
pub const WIN_SHOP_BUY_LIST: usize = 40;
/// Window 41 - the party-wide compare column.
pub const WIN_SHOP_PARTY_COMPARE: usize = 41;
/// Window 42 - the Buy / Sell / Quit picker.
pub const WIN_SHOP_PICKER: usize = 42;

/// Row pitch of every list and picker here (`0xE`).
pub const SHOP_LIST_PITCH: i32 = 0x0E;

/// Fixed cell width of the kernel's digit fields.
const DIGIT_CELL: i32 = 8;

/// Text tint for a retail ink-staging value (`_DAT_8007B454`).
pub fn shop_ink_color(ink: u8) -> [f32; 4] {
    match ink {
        0 => crate::MENU_TEXT_GREY,
        4 => crate::MENU_TEXT_GREEN,
        5 => crate::MENU_TEXT_TEAL,
        6 => crate::MENU_TEXT_GOLD,
        9 => crate::MENU_TEXT_ORANGE,
        _ => crate::MENU_TEXT_WHITE,
    }
}

/// Which row shape a shop list draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShopListKind {
    /// Window 40: class `0x3000` / `0xA000` rows - name at `+0x18`, 5-digit
    /// price at `+0x80`.
    Buy,
    /// Window 38: bag rows - name at `+0xC`, 3-cell count at `+0x6C`.
    Sell,
}

impl ShopListKind {
    /// The descriptor window the list lives in.
    pub fn window(self) -> usize {
        match self {
            ShopListKind::Buy => WIN_SHOP_BUY_LIST,
            ShopListKind::Sell => WIN_SHOP_SELL_LIST,
        }
    }
}

/// One list row: label, the right-hand number (price or held count) and the
/// retail ink the row stages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShopListRow<'a> {
    pub label: &'a str,
    pub value: u32,
    pub ink: u8,
}

/// A paged shop list as the composition draws it.
#[derive(Debug, Clone, Copy)]
pub struct ShopListView<'a> {
    pub kind: ShopListKind,
    pub rows: &'a [ShopListRow<'a>],
    /// Selected row, absolute (the page is derived from it).
    pub cursor: usize,
    /// The list has the pad (kernel mode 1): the hand and the page
    /// triangles draw. A parked list (mode 4) draws neither.
    pub browsing: bool,
}

/// The Buy / Sell / Quit picker's content.
#[derive(Debug, Clone, Copy)]
pub struct ShopPickerView<'a> {
    /// `(label, ink)` per row.
    pub rows: &'a [(&'a str, u8)],
    /// The hand's row, or `None` while the picker is parked.
    pub cursor: Option<usize>,
}

/// One party member for window 41, as a host projects it from
/// `engine-core::shop::party_compare_members`.
#[derive(Debug, Clone, Copy)]
pub struct ShopCompareMember<'a> {
    pub name: &'a str,
    pub already_equipped: bool,
    pub equippable: bool,
    /// The live eight-word menu block (HP, MP, AGL, ATK, UDF, LDF, SPD, INT).
    pub current: [i32; 8],
    /// The trial-equip block; `None` for a non-equipment staged id (current
    /// values, no arrows).
    pub candidate: Option<[i32; 8]>,
}

fn stat_block(b: [i32; 8]) -> EquipStatBlock {
    EquipStatBlock {
        hp: b[0],
        mp: b[1],
        agl: b[2],
        atk: b[3],
        udf: b[4],
        ldf: b[5],
        spd: b[6],
        int: b[7],
    }
}

/// Everything one shop frame shows.
#[derive(Debug, Clone, Copy)]
pub struct ShopScreenView<'a> {
    /// The window set, in draw order (`engine-core::shop::shop_screen_windows`).
    pub windows: &'a [usize],
    pub picker: Option<ShopPickerView<'a>>,
    pub list: Option<ShopListView<'a>>,
    /// Window 41's members. Drawn only when window 41 is in the set.
    pub party: &'a [ShopCompareMember<'a>],
}

/// The host-side context: the content-rect resolver, the chrome atlas rects
/// (absent on a disc-less run) and the stage transform.
#[derive(Clone, Copy)]
pub struct ShopScreenCtx<'a> {
    pub font: &'a legaia_font::Font,
    pub rects: crate::pause_menu::MenuRects<'a>,
    pub chrome: Option<&'a SaveMenuAtlasRects>,
    pub origin: (i32, i32),
    pub scale: u32,
}

/// One shop frame: texts in stage pixels, sprites in surface pixels.
#[derive(Debug, Clone, Default)]
pub struct ShopScreenDraws {
    pub texts: Vec<TextDraw>,
    pub sprites: Vec<SpriteDraw>,
}

/// The 9-slice frame rect of a shop window, or of the widget box window 39
/// frames below itself.
fn frame_rect(ctx: &ShopScreenCtx<'_>, id: usize) -> (i32, i32, i32, i32) {
    ctx.rects.frame_rect(id)
}

/// Content rect of window 39's widget box (`0x90 x 0x28` at `WY + 0x45`).
pub fn sell_detail_box_rect(detail: (i32, i32, i32, i32)) -> (i32, i32, i32, i32) {
    (detail.0, detail.1 + 0x45, 0x90, 0x28)
}

/// The frames of `windows`, in order. Window 33 wears the carved plaque; the
/// sell detail adds its widget box.
pub fn shop_window_frames(ctx: &ShopScreenCtx<'_>, windows: &[usize]) -> Vec<SpriteDraw> {
    let Some(rects) = ctx.chrome else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for &id in windows {
        if id == WIN_SHOP_VENDOR {
            let (x, y, w, _) = ctx.rects.rect(id);
            out.extend(crate::tab_banner_draws(
                rects,
                (x, y),
                w,
                ctx.origin,
                ctx.scale,
            ));
            continue;
        }
        out.extend(crate::menu_window_chrome_draws_for(
            rects,
            frame_rect(ctx, id),
            ctx.origin,
            ctx.scale,
        ));
        if id == WIN_SHOP_SELL_DETAIL {
            let (x, y, w, h) = sell_detail_box_rect(ctx.rects.rect(id));
            out.extend(crate::menu_window_chrome_draws_for(
                rects,
                (x - 8, y - 8, w + 16, h + 16),
                ctx.origin,
                ctx.scale,
            ));
        }
    }
    out
}

fn stage_sprite(ctx: &ShopScreenCtx<'_>, src: (u32, u32, u32, u32), at: (i32, i32)) -> SpriteDraw {
    let s = ctx.scale.max(1) as i32;
    SpriteDraw {
        dst: (
            ctx.origin.0 + at.0 * s,
            ctx.origin.1 + at.1 * s,
            src.2 * ctx.scale.max(1),
            src.3 * ctx.scale.max(1),
        ),
        src,
        color: [1.0, 1.0, 1.0, 1.0],
    }
}

/// The hand cursor at stage `at`: the atlas sprite, or a `>` glyph without
/// the atlas.
fn hand(ctx: &ShopScreenCtx<'_>, at: (i32, i32), out: &mut ShopScreenDraws) {
    match ctx.chrome {
        Some(r) => out.sprites.push(stage_sprite(ctx, r.cursor, at)),
        None => out.texts.extend(text_draws_for(
            &ctx.font.layout_ascii(">"),
            (at.0 + 4, at.1),
            crate::MENU_TEXT_GOLD,
        )),
    }
}

/// Resolve the window painters' sprite requests - the hand / pager marks
/// (`FUN_8002B994` kinds 0 / 2 / 3) and the pictograms (`FUN_8002C488`) -
/// to atlas sprites, or to ASCII glyphs without the atlas. An id the atlas
/// carries no cell for falls back to its glyph either way.
pub fn shop_marker_draws(
    ctx: &ShopScreenCtx<'_>,
    marks: &[PainterSprite],
    pictograms: &[PainterPictogram],
) -> ShopScreenDraws {
    let mut out = ShopScreenDraws::default();
    for m in marks {
        let src = ctx.chrome.and_then(|r| match m.sprite {
            0 => Some(r.cursor),
            2 => Some(r.pager_left),
            3 => Some(r.pager_right),
            _ => None,
        });
        match src {
            Some(src) => out.sprites.push(stage_sprite(ctx, src, (m.x, m.y))),
            None => out.texts.extend(text_draws_for(
                &ctx.font.layout_ascii(">"),
                (m.x, m.y),
                crate::MENU_TEXT_GOLD,
            )),
        }
    }
    for p in pictograms {
        let src = ctx.chrome.and_then(|r| match p.id {
            crate::COUNTER_PICTOGRAM_GOLD => Some(r.icon_money),
            crate::COUNTER_PICTOGRAM_COINS => Some(r.label_coin),
            _ => None,
        });
        match src {
            Some(src) => out.sprites.push(stage_sprite(ctx, src, (p.x, p.y))),
            None => {
                let glyph = match p.id {
                    crate::COUNTER_PICTOGRAM_GOLD => "G",
                    crate::COUNTER_PICTOGRAM_COINS => "C",
                    _ => "*",
                };
                out.texts.extend(text_draws_for(
                    &ctx.font.layout_ascii(glyph),
                    (p.x, p.y),
                    crate::MENU_TEXT_GOLD,
                ));
            }
        }
    }
    out
}

/// Visible rows of a kind-4 list window of content height `h`.
pub fn list_visible_rows(h: i32) -> usize {
    ((h - 4) / SHOP_LIST_PITCH).max(1) as usize
}

/// The first row's stage y in a kind-4 list window `(WY, h)`.
pub fn list_row0_y(wy: i32, h: i32) -> i32 {
    let visible = list_visible_rows(h) as i32;
    wy + (h - visible * SHOP_LIST_PITCH) / 2 + 5
}

/// Cells of window 39's sell-price field (`WX + 0x64`, five digits).
pub const SELL_DETAIL_PRICE_CELLS: i32 = 5;

/// Digits of `value` right-aligned in a `cells`-wide 8-px field at `pen` -
/// the fixed-cell number primitive (`FUN_80034B78`) every shop price and
/// count goes through.
pub fn shop_digit_field_draws(
    font: &legaia_font::Font,
    value: u32,
    pen: (i32, i32),
    cells: i32,
    color: [f32; 4],
) -> Vec<TextDraw> {
    digit_field(font, value, pen.0, pen.1, cells, color)
}

/// Digits of `value` right-aligned in a `cells`-wide 8-px field at `x`.
fn digit_field(
    font: &legaia_font::Font,
    value: u32,
    x: i32,
    y: i32,
    cells: i32,
    color: [f32; 4],
) -> Vec<TextDraw> {
    let s = value.to_string();
    let len = s.len() as i32;
    let mut out = Vec::new();
    for (i, ch) in s.chars().enumerate() {
        let cell = (cells - len + i as i32).max(0);
        out.extend(text_draws_for(
            &font.layout_ascii(&ch.to_string()),
            (x + cell * DIGIT_CELL, y),
            color,
        ));
    }
    out
}

/// One page of a shop list in its window: PAGE header, rows, and - while
/// browsing - the hand and the page triangles.
pub fn shop_list_draws(ctx: &ShopScreenCtx<'_>, list: &ShopListView<'_>) -> ShopScreenDraws {
    let mut out = ShopScreenDraws::default();
    let (wx, wy, w, h) = ctx.rects.rect(list.kind.window());
    let visible = list_visible_rows(h);
    if list.rows.is_empty() {
        return out;
    }
    let page = list.cursor.min(list.rows.len() - 1) / visible;
    let pages = list.rows.len().div_ceil(visible);
    let top = page * visible;
    let row0 = list_row0_y(wy, h);

    // PAGE header: "PAGE" at `WX + W - 0x38`, the current page right-packed
    // in the two cells at `WX + W - 0x20`, the slash `+0xD`, the total at
    // `+0x14`. Retail draws these from small-cap UI-icon cells (ICO `0x76`,
    // `0x79`, `0x7A + digit`); the font glyphs hold their columns.
    // The tag is 24 px wide in retail and ends where the digits start; the
    // font's "PAGE" is wider, so it is right-aligned onto that edge.
    let hy = wy - 4;
    let px = wx + w - 0x20;
    let tag = ctx.font.layout_ascii("PAGE");
    let hx = (px - 2 - tag.advance_x as i32).min(wx + w - 0x38);
    out.texts
        .extend(text_draws_for(&tag, (hx, hy), crate::MENU_TEXT_PAGE_TEAL));
    // Two 6-px digit cells per number, the tens cell left blank under 10.
    let mut cells = |value: usize, x: i32| {
        let tens = value / 10 % 10;
        if tens > 0 {
            out.texts.extend(text_draws_for(
                &ctx.font.layout_ascii(&tens.to_string()),
                (x, hy),
                crate::MENU_TEXT_GOLD,
            ));
        }
        out.texts.extend(text_draws_for(
            &ctx.font.layout_ascii(&(value % 10).to_string()),
            (x + 6, hy),
            crate::MENU_TEXT_GOLD,
        ));
    };
    cells(page + 1, px);
    cells(pages, px + 0x14);
    out.texts.extend(text_draws_for(
        &ctx.font.layout_ascii("/"),
        (px + 0x0D, hy),
        crate::MENU_TEXT_GOLD,
    ));

    for (i, row) in list.rows.iter().skip(top).take(visible).enumerate() {
        let y = row0 + i as i32 * SHOP_LIST_PITCH;
        let color = shop_ink_color(row.ink);
        let (name_x, value_x, cells) = match list.kind {
            ShopListKind::Buy => (wx + 0x18, wx + 0x80, 5),
            ShopListKind::Sell => (wx + 0x0C, wx + 0x6C, 3),
        };
        out.texts.extend(text_draws_for(
            &ctx.font.layout_ascii(row.label),
            (name_x, y),
            color,
        ));
        out.texts
            .extend(digit_field(ctx.font, row.value, value_x, y, cells, color));
    }

    if list.browsing {
        let sel = list.cursor.min(list.rows.len() - 1) - top;
        hand(
            ctx,
            (wx - 6, row0 + sel as i32 * SHOP_LIST_PITCH - 2),
            &mut out,
        );
        let ay = wy + h / 2 - 3;
        if let Some(r) = ctx.chrome {
            // The atlas pager cells are 16x16 with the triangle centred, so
            // the cell sits 8 px left of / above the kernel's 8x8 spot.
            if page + 1 < pages {
                out.sprites
                    .push(stage_sprite(ctx, r.pager_right, (wx + w + 4 - 4, ay - 4)));
            }
            if page > 0 {
                out.sprites
                    .push(stage_sprite(ctx, r.pager_left, (wx - 0x0C - 4, ay - 4)));
            }
        }
    }
    out
}

/// Window 42 - the Buy / Sell / Quit picker: rows at `WX + 0x14`, the hand
/// at the window origin column.
///
/// PORT: FUN_801D4868 (row pens; the ink comes from the host's
/// `shop_root_command_rows`)
pub fn shop_picker_draws(ctx: &ShopScreenCtx<'_>, picker: &ShopPickerView<'_>) -> ShopScreenDraws {
    let mut out = ShopScreenDraws::default();
    let (wx, wy, _, _) = ctx.rects.rect(WIN_SHOP_PICKER);
    for (i, (label, ink)) in picker.rows.iter().enumerate() {
        let y = wy + i as i32 * SHOP_LIST_PITCH;
        out.texts.extend(text_draws_for(
            &ctx.font.layout_ascii(label),
            (wx + 0x14, y),
            shop_ink_color(*ink),
        ));
        if picker.cursor == Some(i) {
            hand(ctx, (wx, y - 2), &mut out);
        }
    }
    out
}

/// Window 41's members as the party-compare painter takes them.
pub fn shop_party_compare_views<'a>(
    members: &[ShopCompareMember<'a>],
) -> Vec<PartyCompareMemberView<'a>> {
    members
        .iter()
        .map(|m| {
            let current = stat_block(m.current);
            PartyCompareMemberView {
                name: m.name,
                outcome: if m.already_equipped {
                    PartyCompareOutcome::Equipped(crate::RECIPIENT_NOTE_EQUIPPED)
                } else if !m.equippable {
                    PartyCompareOutcome::CannotEquip(crate::RECIPIENT_NOTE_CANNOT_EQUIP)
                } else {
                    PartyCompareOutcome::Stats {
                        current,
                        candidate: m.candidate.map(stat_block),
                        labels: crate::COMPARE_LABELS_ATK,
                    }
                },
            }
        })
        .collect()
}

/// The whole composition for one frame: frames, picker, list and window 41.
/// The per-window painters the hosts already call (vendor name, purse, item
/// info, the quantity steppers, the sell detail, the toast) add their text
/// on top; [`shop_marker_draws`] resolves their sprite requests.
pub fn shop_screen_draws(ctx: &ShopScreenCtx<'_>, view: &ShopScreenView<'_>) -> ShopScreenDraws {
    let mut out = ShopScreenDraws {
        texts: Vec::new(),
        sprites: shop_window_frames(ctx, view.windows),
    };
    let has = |id: usize| view.windows.contains(&id);
    let mut add = |d: ShopScreenDraws| {
        out.texts.extend(d.texts);
        out.sprites.extend(d.sprites);
    };
    if let Some(p) = view.picker.as_ref().filter(|_| has(WIN_SHOP_PICKER)) {
        add(shop_picker_draws(ctx, p));
    }
    if let Some(l) = view.list.as_ref().filter(|l| has(l.kind.window())) {
        add(shop_list_draws(ctx, l));
    }
    if has(WIN_SHOP_PARTY_COMPARE) && !view.party.is_empty() {
        let (x, y, _, _) = ctx.rects.rect(WIN_SHOP_PARTY_COMPARE);
        let views = shop_party_compare_views(view.party);
        let fields = crate::party_compare_panel_fields(&views, (x, y));
        out.texts
            .extend(crate::compare_panel_draws_for(ctx.font, &fields));
    }
    out
}

/// Drop the stage texts a window's frame covers - for a window drawn over
/// others (the Point Card toast), whose frame the host's sprite pass puts
/// under every text.
pub fn occlude_texts(texts: &mut Vec<TextDraw>, frame: (i32, i32, i32, i32)) {
    let (x, y, w, h) = frame;
    texts.retain(|t| {
        let (tx, ty) = (t.dst.0, t.dst.1);
        !(tx >= x && tx < x + w && ty >= y && ty < y + h)
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kernel_page_geometry_matches_the_two_shop_lists() {
        // Window 40 (h 104) pages 7 rows from WY + 8; window 38 (h 158)
        // pages 11 rows from WY + 7 - the 7 / 11 the retail capture shows.
        assert_eq!(list_visible_rows(104), 7);
        assert_eq!(list_row0_y(46, 104), 54);
        assert_eq!(list_visible_rows(158), 11);
        assert_eq!(list_row0_y(46, 158), 53);
        // The pause item list (h 182) lands on its pinned `WY + 0xC`.
        assert_eq!(list_row0_y(22, 182), 22 + 0x0C);
    }

    #[test]
    fn ink_staging_maps_to_the_menu_pens() {
        assert_eq!(shop_ink_color(7), crate::MENU_TEXT_WHITE);
        assert_eq!(shop_ink_color(0), crate::MENU_TEXT_GREY);
        assert_eq!(shop_ink_color(5), crate::MENU_TEXT_TEAL);
    }

    #[test]
    fn occlusion_drops_only_covered_text() {
        let t = |x, y| TextDraw {
            src: (0, 0, 1, 1),
            dst: (x, y, 1, 1),
            color: [1.0; 4],
        };
        let mut v = vec![t(10, 10), t(100, 100)];
        occlude_texts(&mut v, (90, 90, 20, 20));
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].dst.0, 10);
    }
}
