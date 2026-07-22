//! UI draw-builder functions: sprite/text batching, shop rows, dialog
//! boxes, level-up + capture banners, and the battle HUD. Each returns
//! renderer-agnostic [`TextDraw`]/[`SpriteDraw`] batches.

use crate::*;

/// Convert sprite requests to [`SpriteDraw`]s, applying a screen-space
/// `anchor` translation. The output `dst` width/height match the atlas
/// source rect 1:1 (no scaling - engines that want PSX-native 240px
/// vertical scaling should pre-scale `world_y` before calling this).
pub fn sprite_draws_for(requests: &[SpriteRequest], anchor: (i32, i32)) -> Vec<SpriteDraw> {
    requests
        .iter()
        .map(|r| SpriteDraw {
            dst: (
                anchor.0 + r.world_x,
                anchor.1 + r.world_y,
                r.atlas_src.2,
                r.atlas_src.3,
            ),
            src: r.atlas_src,
            color: r.color,
        })
        .collect()
}

/// Convert a [`legaia_font::Layout`] to a vector of [`TextDraw`]s anchored at
/// `pen` with the supplied tint. Glyph atlas coordinates come from the
/// layout; destination coordinates are pen-relative pixels with one quad per
/// glyph. The returned draws are batchable into a single [`TextOverlay`].
pub fn text_draws_for(
    layout: &legaia_font::Layout,
    pen: (i32, i32),
    color: [f32; 4],
) -> Vec<TextDraw> {
    layout
        .glyphs
        .iter()
        .map(|g| TextDraw {
            dst: (pen.0 + g.dst_x, pen.1 + g.dst_y, g.width, g.height),
            src: (g.atlas_x, g.atlas_y, g.width, g.height),
            color,
        })
        .collect()
}

/// Map a slice of [`TextDraw`]s whose `dst` coordinates are expressed in
/// **stage pixels** (a virtual 320×240 PSX framebuffer) into surface
/// coordinates: `dst = stage_origin + dst * stage_scale`, with the glyph
/// size scaled to match.
///
/// The menu text builders ([`field_menu_draws_for`],
/// [`status_screen_draws_for`], [`spell_menu_draws_for`], …) lay glyphs
/// out at retail-pinned stage-pixel pens. This is the single transform
/// that upscales + centers them into the surface, matching the chrome
/// emitted by [`menu_window_chrome_draws_for`] so text and window frame
/// stay locked together at any window size. Apply it in place after the
/// builder returns, then composite the result.
pub fn scale_stage_text_draws(draws: &mut [TextDraw], stage_origin: (i32, i32), stage_scale: u32) {
    let scale = stage_scale.max(1);
    for d in draws.iter_mut() {
        d.dst = (
            stage_origin.0 + d.dst.0 * scale as i32,
            stage_origin.1 + d.dst.1 * scale as i32,
            d.dst.2 * scale,
            d.dst.3 * scale,
        );
    }
}

/// One row in a shop or confirmation panel drawn by [`shop_draws_for`].
pub struct ShopRow<'a> {
    /// Display name for this row (item name, "Yes", "No", quantity digit, …).
    pub label: &'a str,
    /// Optional right-aligned price or value in gold. `None` for confirm /
    /// quantity rows where no price is shown.
    pub price: Option<u32>,
    /// Retail text ink for this row - the value the menu overlay stages into
    /// `_DAT_8007B454` before the string draw. `7` is normal white, `0` the
    /// greyed/unavailable pen and `6` the accent pen a stock row takes from
    /// its record's "already owned / restricted" marker. Callers derive it
    /// with `legaia_engine_core::shop::shop_stock_row_ink`; rows with no
    /// retail ink of their own pass [`SHOP_INK_NORMAL`].
    pub ink: u8,
}

/// Normal white text ink (retail `_DAT_8007B454 == 7`).
pub const SHOP_INK_NORMAL: u8 = 7;
/// Greyed / unavailable text ink (retail `_DAT_8007B454 == 0`).
pub const SHOP_INK_GREY: u8 = 0;
/// Accent text ink (retail `_DAT_8007B454 == 6`).
pub const SHOP_INK_MARKED: u8 = 6;

impl<'a> ShopRow<'a> {
    /// A row at the normal white ink.
    pub fn new(label: &'a str, price: Option<u32>) -> Self {
        Self {
            label,
            price,
            ink: SHOP_INK_NORMAL,
        }
    }
}

/// Build [`TextDraw`]s for a 2-D shop / confirmation panel.
///
/// Layout traced from `FUN_801d5de0` in `overlay_shop_save.bin`:
/// ```text
/// [title]
/// > item name              1500G
///   item name               200G   ← unaffordable rows are dimmed
///   …
/// Gold: 9999G
/// ```
/// Column offsets relative to `pen`:
/// - cursor `>`: x + 0 (`CURSOR_X`)
/// - item name: x + 20 (`LABEL_X`, retail `0x14`)
/// - price (left-aligned): x + 112 (`PRICE_X`, retail `0x70`)
/// - line height: 14 px (`LINE_H`, retail `0x0E`)
///
/// Rows where `gold < price` are rendered dim; selected row has a
/// gold-coloured price. `gold = None` suppresses the gold footer line.
/// A row whose [`ShopRow::ink`] is not [`SHOP_INK_NORMAL`] overrides that
/// affordability derivation with its retail pen - `0` dim, `6` the accent
/// colour the retail stock list uses for owned/restricted stock.
///
/// A natural anchor for a PSX-style 320×240 surface is `(8, 140)`.
pub fn shop_draws_for<'a>(
    font: &legaia_font::Font,
    title: &str,
    rows: &[ShopRow<'a>],
    cursor: usize,
    gold: Option<i32>,
    pen: (i32, i32),
) -> Vec<TextDraw> {
    // Constants confirmed from overlay_shop_save FUN_801d5de0.
    const LINE_H: i32 = 14;
    const CURSOR_X: i32 = 0;
    const LABEL_X: i32 = 20; // retail 0x14
    const PRICE_X: i32 = 112; // retail 0x70, left edge of 6-digit price field

    let white: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
    let dim: [f32; 4] = [0.55, 0.55, 0.55, 1.0];
    let gold_col: [f32; 4] = [1.0, 0.85, 0.3, 1.0];
    let marked: [f32; 4] = [0.45, 0.68, 1.0, 1.0];

    let mut out = Vec::new();

    // Title line
    let title_layout = font.layout_ascii(title);
    out.extend(text_draws_for(&title_layout, pen, white));

    // Item rows
    for (i, row) in rows.iter().enumerate() {
        let row_y = pen.1 + LINE_H + i as i32 * LINE_H;
        let selected = i == cursor;

        // Retail dims rows the player cannot afford (gold < price).
        let can_afford = match (gold, row.price) {
            (Some(g), Some(p)) => g >= p as i32,
            _ => true,
        };
        // A retail ink other than the normal pen wins over the affordability
        // derivation: the stock list's `6` accent marks owned / restricted
        // stock the player *can* afford, and its `0` also covers a full stack.
        let (can_afford, ink_fg) = match row.ink {
            SHOP_INK_GREY => (false, Some(dim)),
            SHOP_INK_MARKED => (can_afford, Some(marked)),
            _ => (can_afford, None),
        };
        let fg = ink_fg.unwrap_or(if !can_afford || !selected { dim } else { white });

        if selected {
            let cur_layout = font.layout_ascii(">");
            out.extend(text_draws_for(
                &cur_layout,
                (pen.0 + CURSOR_X, row_y),
                white,
            ));
        }

        let label_layout = font.layout_ascii(row.label);
        out.extend(text_draws_for(&label_layout, (pen.0 + LABEL_X, row_y), fg));

        if let Some(price) = row.price {
            let price_str = format!("{price}G");
            let price_layout = font.layout_ascii(&price_str);
            let price_fg = if !can_afford {
                dim
            } else if selected {
                gold_col
            } else {
                dim
            };
            out.extend(text_draws_for(
                &price_layout,
                (pen.0 + PRICE_X, row_y),
                price_fg,
            ));
        }
    }

    // Gold footer (retail FUN_801d0148: gold icon at panel_x, amount at x+40).
    if let Some(g) = gold {
        let gold_y = pen.1 + LINE_H + rows.len() as i32 * LINE_H + 4;
        let gold_str = format!("Gold: {g}G");
        let gold_layout = font.layout_ascii(&gold_str);
        out.extend(text_draws_for(&gold_layout, (pen.0, gold_y), gold_col));
    }

    out
}

/// One row of plain text for the dialog presenter. Engines populate
/// this from their `DialogPanel::page_glyphs` view; the renderer wraps
/// them into `TextDraw`s without re-implementing the layout pass.
#[derive(Debug, Clone, Copy)]
pub struct DialogGlyphView {
    /// ASCII / latin glyph byte. Wide-glyph references are pre-folded
    /// to the operand byte (matches `DialogPanel`'s emit path).
    pub byte: u8,
    /// CLUT additive index (0..15). Engines pass `0` for the default
    /// pen; non-zero values come from inline `0xCF` color escapes.
    pub clut: u8,
}

/// Layout used by [`dialog_box_draws_for`]. Engines pass this once per
/// frame; the function recomputes the text wrap on the fly so engines
/// that care about retail-correct line breaks can substitute their own
/// layout pre-pass.
#[derive(Debug, Clone, Copy)]
pub struct DialogBoxLayout {
    /// Top-left of the panel in surface pixels.
    pub origin: (i32, i32),
    /// Width / height of the panel rectangle in pixels.
    pub size: (u32, u32),
    /// Internal margin between panel edge and text.
    pub padding: (i32, i32),
    /// Per-line vertical advance.
    pub line_h: i32,
    /// Maximum text columns per line (in glyphs). When a glyph would
    /// overflow this width, the renderer wraps to the next line.
    pub cols: u16,
}

impl Default for DialogBoxLayout {
    /// Retail layout traced from the dialog overlay (`FUN_801D84D0`):
    /// origin (8, 168), size (304, 56), padding (8, 8), line_h 14,
    /// cols 36 (matches the proportional dialog font's average advance
    /// at 304 px wide). Engines that don't render at 320×240 should
    /// override these.
    fn default() -> Self {
        Self {
            origin: (8, 168),
            size: (304, 56),
            padding: (8, 8),
            line_h: 14,
            cols: 36,
        }
    }
}

/// Resolve a dialog CLUT additive index to an RGBA tint. The retail
/// dialog renderer uses a small palette indexed at `_DAT_8007B454`;
/// we approximate the most common entries.
pub fn dialog_clut_color(clut: u8) -> [f32; 4] {
    match clut {
        0 => [1.0, 1.0, 1.0, 1.0],    // default white
        1 => [1.0, 0.85, 0.2, 1.0],   // gold (NPC names)
        2 => [0.5, 1.0, 0.5, 1.0],    // green (heal)
        3 => [1.0, 0.4, 0.4, 1.0],    // red (warning)
        4 => [0.4, 0.6, 1.0, 1.0],    // blue (lore)
        _ => [0.85, 0.85, 0.85, 1.0], // dim fallback
    }
}

/// Build [`TextDraw`]s for an open dialog box.
///
/// Layout: panel rectangle drawn first (engines render the rectangle
/// outside the text path; we don't emit it here since it's a quad, not
/// a glyph), then text wrapped onto sequential lines inside the
/// padded interior. Engines that want a "page break" cursor can layer
/// their own caret quad on top.
///
/// Wrapping: a simple left-to-right, glyph-width-driven greedy wrap.
/// Newline byte (`'\n'`) starts a new line. Spaces are kept literal.
/// Glyph layout uses [`legaia_font::Font::layout_ascii`] per glyph so
/// the proportional dialog font's advance values are honoured.
pub fn dialog_box_draws_for(
    font: &legaia_font::Font,
    glyphs: &[DialogGlyphView],
    layout: &DialogBoxLayout,
) -> Vec<TextDraw> {
    let interior_x = layout.origin.0 + layout.padding.0;
    let interior_y = layout.origin.1 + layout.padding.1;
    let max_x = layout.origin.0 + layout.size.0 as i32 - layout.padding.0;
    let max_y = layout.origin.1 + layout.size.1 as i32 - layout.padding.1;
    let mut pen_x = interior_x;
    let mut pen_y = interior_y;
    let mut out = Vec::with_capacity(glyphs.len());
    for g in glyphs {
        if g.byte == b'\n' {
            pen_x = interior_x;
            pen_y += layout.line_h;
            continue;
        }
        // Layout a single-byte string and look at the resulting glyph
        // width - that's the proportional advance.
        let s = [g.byte];
        let one = font.layout_ascii(std::str::from_utf8(&s).unwrap_or(" "));
        let advance = one
            .glyphs
            .first()
            .map(|gl| gl.width as i32 + 1)
            .unwrap_or(8);
        if pen_x + advance > max_x {
            pen_x = interior_x;
            pen_y += layout.line_h;
        }
        if pen_y + layout.line_h > max_y {
            // Out of vertical room - drop the rest of this page.
            break;
        }
        if let Some(gl) = one.glyphs.first() {
            out.push(TextDraw {
                dst: (pen_x + gl.dst_x, pen_y + gl.dst_y, gl.width, gl.height),
                src: (gl.atlas_x, gl.atlas_y, gl.width, gl.height),
                color: dialog_clut_color(g.clut),
            });
        }
        pen_x += advance;
    }
    out
}

/// Convenience wrapper: convert engine-core's `DialogPanel::page_glyphs`
/// shape (named `(byte, clut)` pairs) directly to [`TextDraw`]s.
///
/// Engines that import [`legaia_engine_core::dialog::PanelGlyph`] should
/// prefer this wrapper to skip the manual `DialogGlyphView` mapping.
pub fn dialog_panel_draws_for(
    font: &legaia_font::Font,
    panel_glyphs: &[(u8, u8)],
    layout: &DialogBoxLayout,
) -> Vec<TextDraw> {
    let views: Vec<DialogGlyphView> = panel_glyphs
        .iter()
        .map(|&(byte, clut)| DialogGlyphView { byte, clut })
        .collect();
    dialog_box_draws_for(font, &views, layout)
}

/// Build [`TextDraw`]s for a level-up banner overlay.
///
/// Renders two lines anchored at `pen`:
/// ```text
/// LEVEL UP!  (char_id, new_level)
/// HP +hp_gained  MP +mp_gained
/// ```
/// Designed for a PSX-style 320×240 surface; a typical anchor is around
/// `(8, 60)` to appear near the top of the screen after battle.
pub fn level_up_draws_for(
    font: &legaia_font::Font,
    char_id: u8,
    new_level: u8,
    hp_gained: u16,
    mp_gained: u16,
    pen: (i32, i32),
) -> Vec<TextDraw> {
    const LINE_H: i32 = 16;
    let yellow: [f32; 4] = [1.0, 0.9, 0.2, 1.0];
    let green: [f32; 4] = [0.4, 1.0, 0.4, 1.0];

    let line1 = format!("LEVEL UP! (char {} -> Lv {})", char_id + 1, new_level);
    let line2 = format!("HP +{}  MP +{}", hp_gained, mp_gained);

    let layout1 = font.layout_ascii(&line1);
    let layout2 = font.layout_ascii(&line2);

    let mut out = text_draws_for(&layout1, pen, yellow);
    out.extend(text_draws_for(&layout2, (pen.0, pen.1 + LINE_H), green));
    out
}

/// Build [`TextDraw`]s for the post-battle Seru-capture banner.
///
/// `text` is the single active banner line from
/// `SeruCaptureSession::current_banner` (e.g. `"Captured: Spark!"` or
/// `"Character 1 learned Aqua!"`). Drawn in cyan, the sibling of
/// [`level_up_draws_for`]; a natural anchor near the top of a 320×240 surface
/// is `(8, 40)`.
pub fn capture_banner_draws_for(
    font: &legaia_font::Font,
    text: &str,
    pen: (i32, i32),
) -> Vec<TextDraw> {
    let cyan: [f32; 4] = [0.4, 0.9, 1.0, 1.0];
    let layout = font.layout_ascii(text);
    text_draws_for(&layout, pen, cyan)
}

/// One row in the battle HUD's per-slot panel (built by
/// [`battle_hud_draws_for`]).
///
/// Engines populate this view from their HUD model on a per-frame basis.
/// The renderer is intentionally agnostic to the engine-core / engine-vm
/// types - pass plain data here to keep the layering clean.
#[derive(Clone, Copy)]
pub struct HudSlotView<'a> {
    /// Display name (character / monster). Empty string skips the row.
    pub name: &'a str,
    /// `true` for party rows (white text); `false` for monster rows
    /// (pale red text).
    pub is_party: bool,
    /// `true` if the actor is alive. Dead actors get a "K.O." overlay.
    pub alive: bool,
    pub hp: u16,
    pub hp_max: u16,
    pub mp: u16,
    pub mp_max: u16,
    /// Amount of AP committed to the action queue this turn.
    pub ap_filled: u8,
    /// Maximum AP for the slot this turn.
    pub ap_max: u8,
    /// One-letter abbreviations for active status icons. Engines pick the
    /// mapping (e.g. 'B' = Toxic, 'P' = Venom, 'S' = Curse, …).
    pub status_letters: &'a [u8],
}

/// One floating damage / heal / status popup.
#[derive(Clone, Copy)]
pub struct HudPopupView {
    pub slot: u8,
    pub amount: u16,
    pub is_heal: bool,
    pub is_crit: bool,
    /// Status letter to overlay on the popup ('B' = Toxic, etc.). `None`
    /// for plain numeric popups.
    pub status_letter: Option<u8>,
    /// Fade alpha 0..=1.0 multiplied into the text colour.
    pub alpha: f32,
}

/// One battle log line.
#[derive(Clone, Copy)]
pub struct HudLogView<'a> {
    pub text: &'a str,
    pub color: [f32; 4],
}

impl<'a> HudSlotView<'a> {
    /// Build a slot view from a plain-data row. The argument shape mirrors
    /// `legaia_engine_core::battle_hud::SlotView`; engines drive this from
    /// `BattleHud::slot_views()` without re-implementing the field copy.
    ///
    /// `name` and `status_letters` borrow from the caller; ownership stays
    /// in the engine-core view buffer.
    pub fn from_plain(meta: HudSlotMeta, name: &'a str, status_letters: &'a [u8]) -> Self {
        Self {
            name,
            is_party: meta.is_party,
            alive: meta.alive,
            hp: meta.hp,
            hp_max: meta.hp_max,
            mp: meta.mp,
            mp_max: meta.mp_max,
            ap_filled: meta.ap_filled,
            ap_max: meta.ap_max,
            status_letters,
        }
    }
}

/// Numeric fields of [`HudSlotView`] grouped into a payload struct so the
/// public constructor stays under clippy's argument-count threshold.
#[derive(Debug, Clone, Copy, Default)]
pub struct HudSlotMeta {
    pub is_party: bool,
    pub alive: bool,
    pub hp: u16,
    pub hp_max: u16,
    pub mp: u16,
    pub mp_max: u16,
    pub ap_filled: u8,
    pub ap_max: u8,
}

/// Retail HP-bar text colour index for a battle slot.
///
/// PORT: FUN_800349EC - returns the font-CLUT colour index the retail battle
/// HUD tints a character's HP readout with, keyed on the cur/max ratio. Index 2
/// is empty/K.O.; index 9 is danger (`cur <= max/4`); index 6 is caution
/// (`cur <= max/2`, or any time a status flag is set); index 7 is normal. The
/// thresholds use the same floored `max >> 2` / `max >> 1` comparisons as retail.
///
/// `status_active` models retail's per-character status byte (record `+0x36`,
/// `*(short *)(char*0x414 - 0x7ff7b7ca)`), which forces the caution tier even
/// above half HP; the engine approximates it with "any active status icon".
///
// Reached on native through [`battle_hud_draws_for`], which
// `engine-shell/.../window/hud.rs` calls for every battle frame. The browser
// play page has no battle host, so this law does not reach that host - see the
// `battle_hud_draws_for` entry in `scripts/ci/ui-host-drift-waivers.toml`.
pub fn hp_bar_color_index(cur: u16, max: u16, status_active: bool) -> u8 {
    if cur == 0 {
        return 2;
    }
    if (max >> 2) < cur {
        if status_active || cur <= (max >> 1) {
            6
        } else {
            7
        }
    } else {
        9
    }
}

/// Retail MP-bar text colour index for a battle slot.
///
/// PORT: FUN_80035EA8 - the MP sibling of [`hp_bar_color_index`]. Same
/// `cur <= max/4` / `cur <= max/2` ratio tiers (index 9 danger, 6 caution,
/// 7 normal) but with no K.O. (2) state and no status-flag override - MP has no
/// "empty = dead" colour, so a depleted bar simply reads as danger.
///
// Same reach as [`hp_bar_color_index`] - see the note there. The MP field it
// tints is drawn only when the slot carries a non-zero `mp_max`, which in the
// native window means party rows: `World` keeps the MP ceiling in
// `character_max_mp` (keyed by battle ordinal) and monsters have none.
pub fn mp_bar_color_index(cur: u16, max: u16) -> u8 {
    if (max >> 2) < cur {
        if cur <= (max >> 1) { 6 } else { 7 }
    } else {
        9
    }
}

/// Build [`TextDraw`]s for the battle HUD.
///
/// Layout (anchored at `pen`):
/// ```text
/// pen.x     +78         +161       +240        +319  +359
///   ┌──────────────────────────────────────────────────────┐
///   │ Vahn      HP 250/300  MP  10/30  APoooo----       T C│
///   │ Noa       HP 180/220  MP   5/20  APoooo----          │
///   │ Gala      HP  90/280  MP   0/15  AP--------          │
///   │                                                      │
///   │ Goblin    HP  50/100                                 │
///   │ Goblin    HP   0/100                         K.O.    │
///   └──────────────────────────────────────────────────────┘
///
/// pen.y + 80   [popup]  -25
///              [popup]  HEAL +50
/// ```
///
/// The log column uses `pen.x` and stacks downward from `pen.y +
/// slot_count * LINE_H`. Popups are drawn over each slot's row.
///
/// ## Column offsets
///
/// The columns are sized from **measured advances of the retail dialog font**
/// (`legaia_font::Font::load_from_extracted`), not guessed - every field is
/// given its widest realistic string plus an 8 px gutter:
///
/// | Field | Widest case | Measured px |
/// |---|---|---|
/// | name | longest monster name (`"Juggernaut"`) | 69 |
/// | HP | `"HP 250/300"` | 75 |
/// | MP | `"MP  10/ 30"` | 71 |
/// | AP | `"AP"` + 8 pips | 71 |
/// | K.O. | `"K.O."` | 32 |
///
/// This matters because the first draft of these offsets (HP `+70`, K.O.
/// `+110`, MP `+140`, AP `+200`, status `+220`) was narrower than the font:
/// four of the five columns overlapped their neighbour at three-digit HP, and
/// the K.O. label painted directly over the HP digits. Nothing caught it while
/// the builder had no caller. Widen a field here and the neighbour's offset
/// moves with it.
///
/// Constants:
/// - `LINE_H` = 14
/// - Status icons are tiled at `x + STATUS_X` with 8 px stride
/// - Damage popups are placed at `pen.x + POPUP_X, slot_y - 16`
pub fn battle_hud_draws_for(
    font: &legaia_font::Font,
    slots: &[HudSlotView<'_>],
    popups: &[HudPopupView],
    log: &[HudLogView<'_>],
    pen: (i32, i32),
) -> Vec<TextDraw> {
    const LINE_H: i32 = 14;
    // Column origins, relative to `pen.x`. See the table above.
    const HP_X: i32 = 78;
    const MP_X: i32 = 161;
    const AP_X: i32 = 240;
    const KO_X: i32 = 319;
    const STATUS_X: i32 = 359;
    const STATUS_STEP: i32 = 8;
    const POPUP_X: i32 = 80;

    let white: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
    let monster: [f32; 4] = [1.0, 0.7, 0.7, 1.0];
    let dim: [f32; 4] = [0.5, 0.5, 0.5, 1.0];
    let red: [f32; 4] = [1.0, 0.4, 0.4, 1.0];
    let green: [f32; 4] = [0.5, 1.0, 0.5, 1.0];
    let yellow: [f32; 4] = [1.0, 0.95, 0.4, 1.0];
    let cyan: [f32; 4] = [0.5, 0.85, 1.0, 1.0];

    let mut out = Vec::new();

    for (i, slot) in slots.iter().enumerate() {
        if slot.name.is_empty() {
            continue;
        }
        let row_y = pen.1 + i as i32 * LINE_H;
        let row_color = if !slot.alive {
            dim
        } else if slot.is_party {
            white
        } else {
            monster
        };

        let name_layout = font.layout_ascii(slot.name);
        out.extend(text_draws_for(&name_layout, (pen.0, row_y), row_color));

        // Retail tints HP/MP readouts by the cur/max ratio (FUN_800349EC /
        // FUN_80035EA8). Map the returned colour index to the HUD palette: the
        // "normal" tier (7) keeps the row's base colour so monster rows stay
        // tinted; danger -> red, caution -> yellow, K.O. -> dim.
        let bar_color = |idx: u8| -> [f32; 4] {
            match idx {
                9 => red,
                6 => yellow,
                2 => dim,
                _ => row_color,
            }
        };

        let hp_text = format!("HP {:>3}/{:>3}", slot.hp, slot.hp_max);
        let hp_layout = font.layout_ascii(&hp_text);
        let hp_color = if !slot.alive {
            dim
        } else {
            bar_color(hp_bar_color_index(
                slot.hp,
                slot.hp_max,
                !slot.status_letters.is_empty(),
            ))
        };
        out.extend(text_draws_for(&hp_layout, (pen.0 + HP_X, row_y), hp_color));

        if slot.mp_max > 0 {
            let mp_text = format!("MP {:>3}/{:>3}", slot.mp, slot.mp_max);
            let mp_layout = font.layout_ascii(&mp_text);
            let mp_color = if !slot.alive {
                dim
            } else {
                bar_color(mp_bar_color_index(slot.mp, slot.mp_max))
            };
            out.extend(text_draws_for(&mp_layout, (pen.0 + MP_X, row_y), mp_color));
        }

        if slot.ap_max > 0 {
            let mut ap_text = String::with_capacity(2 + slot.ap_max as usize);
            ap_text.push_str("AP");
            for n in 0..slot.ap_max {
                if n < slot.ap_filled {
                    ap_text.push('o'); // filled
                } else {
                    ap_text.push('-'); // empty
                }
            }
            let ap_layout = font.layout_ascii(&ap_text);
            out.extend(text_draws_for(&ap_layout, (pen.0 + AP_X, row_y), row_color));
        }

        if !slot.alive {
            let ko_layout = font.layout_ascii("K.O.");
            out.extend(text_draws_for(&ko_layout, (pen.0 + KO_X, row_y), red));
        }

        for (k, letter) in slot.status_letters.iter().enumerate() {
            let s = (*letter as char).to_string();
            let layout = font.layout_ascii(&s);
            out.extend(text_draws_for(
                &layout,
                (pen.0 + STATUS_X + k as i32 * STATUS_STEP, row_y - 12),
                yellow,
            ));
        }
    }

    let log_x = pen.0;
    let log_y = pen.1 + slots.len() as i32 * LINE_H + 4;
    for (i, line) in log.iter().enumerate() {
        let layout = font.layout_ascii(line.text);
        out.extend(text_draws_for(
            &layout,
            (log_x, log_y + i as i32 * LINE_H),
            line.color,
        ));
    }

    for popup in popups {
        if (popup.slot as usize) >= slots.len() {
            continue;
        }
        let slot_y = pen.1 + popup.slot as i32 * LINE_H;
        let popup_color = match (popup.is_heal, popup.is_crit) {
            (true, _) => apply_alpha(green, popup.alpha),
            (_, true) => apply_alpha(yellow, popup.alpha),
            _ => apply_alpha(cyan, popup.alpha),
        };
        let text = if let Some(letter) = popup.status_letter {
            format!("[{}]", letter as char)
        } else if popup.is_heal {
            format!("+{}", popup.amount)
        } else {
            format!("-{}", popup.amount)
        };
        let layout = font.layout_ascii(&text);
        out.extend(text_draws_for(
            &layout,
            (pen.0 + POPUP_X, slot_y - 16),
            popup_color,
        ));
    }

    out
}

pub fn apply_alpha(color: [f32; 4], alpha: f32) -> [f32; 4] {
    [
        color[0],
        color[1],
        color[2],
        color[3] * alpha.clamp(0.0, 1.0),
    ]
}
