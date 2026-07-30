//! UI draw-builder functions: sprite/text batching, shop rows, level-up +
//! capture banners, and the battle HUD. Each returns renderer-agnostic
//! [`TextDraw`]/[`SpriteDraw`] batches.
//!
//! Dialog text is NOT built here: both hosts render dialog through the
//! `DialogSnapshot` path (native `window/hud.rs`, web `play_dialog`) at the
//! byte-pinned retail geometry. A plain typewriter fallback
//! (`dialog_box_draws_for` + its `DialogBoxLayout`/`DialogGlyphView` types)
//! used to live here; nothing called it on either host, so it was removed
//! rather than waived in the ui-host-drift gate.

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
    /// Gauge fill-colour index for the drawn HP bar - retail's
    /// `FUN_80046A20` code space (`2` dead, `3` status override, `7` high,
    /// `6` mid, `9` low). Engines derive it with
    /// `legaia_engine_core::battle_hud::BattleSlotHud::gauge_fill_indices`;
    /// [`gauge_fill_color`] maps it to RGBA.
    pub hp_fill: u8,
    /// Gauge fill-colour index for the drawn MP bar (same code space).
    pub mp_fill: u8,
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
            hp_fill: meta.hp_fill,
            mp_fill: meta.mp_fill,
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
    /// See [`HudSlotView::hp_fill`] / [`HudSlotView::mp_fill`].
    pub hp_fill: u8,
    pub mp_fill: u8,
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
// Reached on both hosts through [`battle_hud_draws_for`]: the native window
// (`engine-shell/.../window/hud.rs`) and the browser play page
// (`web-viewer/src/play_battle.rs`) call it every battle frame.
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

/// Locate a solid-white, fully-opaque texel in the font atlas and return it
/// as a 1x1 `src` rect for [`TextDraw`].
///
/// This is what gives both hosts a **filled-rect primitive** without any new
/// pipeline: every HUD draw already samples the font atlas with a colour
/// multiply (`texel * color` on native, the canvas multiply-tint blit on the
/// play page), so a quad whose source is one pure-white texel is a solid
/// rect of the draw's own colour at any destination size. Safe under nearest
/// sampling too - a 1x1 source spans exactly one texel, so no fragment can
/// reach a neighbour.
///
/// Both the extracted retail font (fill texels whitewashed to `0xFF`) and
/// the placeholder font (white 5x7 strokes) carry such a texel; `None` only
/// on a custom atlas with no opaque white anywhere, in which case the bar
/// builders degrade to text-only output.
pub fn font_solid_src(font: &legaia_font::Font) -> Option<(u32, u32, u32, u32)> {
    let (w, h) = font.atlas_dimensions();
    let rgba = font.atlas_rgba();
    for y in 0..h {
        for x in 0..w {
            let off = ((y * w + x) * 4) as usize;
            if rgba[off] == 0xFF
                && rgba[off + 1] == 0xFF
                && rgba[off + 2] == 0xFF
                && rgba[off + 3] == 0xFF
            {
                return Some((x, y, 1, 1));
            }
        }
    }
    None
}

/// RGBA for a retail gauge fill-colour index.
///
/// The **index space** is retail's (`FUN_80046A20` / the readout-tint pair
/// `FUN_800349EC` / `FUN_80035EA8`: `2` dead, `3` status override, `7` high,
/// `6` mid, `9` low). The **RGB values are approximations** - retail resolves
/// each index through a font-CLUT row whose entries are not pinned; these are
/// chosen to read the same way (green = healthy, amber = caution, red =
/// danger, violet = status-locked, grey = dead).
pub fn gauge_fill_color(idx: u8) -> [f32; 4] {
    match idx {
        2 => [0.30, 0.30, 0.34, 1.0],
        3 => [0.72, 0.45, 0.95, 1.0],
        6 => [1.0, 0.78, 0.15, 1.0],
        9 => [1.0, 0.28, 0.22, 1.0],
        _ => [0.25, 0.92, 0.40, 1.0],
    }
}

/// Everything one battle-HUD frame draws, plus the two chrome inputs the
/// retail-shaped panels need. Bundled so [`battle_hud_draws_for`] stays under
/// clippy's argument-count threshold as the surface grows.
pub struct BattleHudFrame<'a> {
    /// Per-slot rows, indexed by **absolute actor-table slot** (party
    /// `0..party_count`, monsters above). Inactive slots are empty-name
    /// entries the builder skips - compacting would mis-anchor popups.
    pub slots: &'a [HudSlotView<'a>],
    pub popups: &'a [HudPopupView],
    pub log: &'a [HudLogView<'a>],
    /// 1x1 solid-white atlas rect from [`font_solid_src`]. `None` degrades
    /// the panels to text-only (no filled bars / panel chrome).
    pub solid_src: Option<(u32, u32, u32, u32)>,
    /// Surface size in pixels. The party panels are laid out on the retail
    /// 320x240 stage and integer-upscaled + centred into this surface, the
    /// same transform the boot/menu stage uses.
    pub surface: (u32, u32),
}

/// Party-panel stage geometry. Pinned values come from the battle-overlay
/// disassembly; the rest are engine approximations and say so.
///
/// **Pinned (disassembly):** the per-party-size X anchors are retail's
/// `FUN_801D84C0` panel-anchor table (solo `0x72`; pair `0x3F`/`0xA5`; trio
/// `0x0C`/`0x72`), and the panel width is the `0x40`-px label-strip blit of
/// `FUN_801DBC30`. Canonical port + provenance:
/// `legaia_engine_vm::battle_party_panel` (`panel_anchors`, `label_strip`);
/// mirrored here as literals because `engine-ui` sits below `engine-vm` in
/// the crate graph. `engine-shell`'s HUD tests pin the two sets equal.
///
/// **Inferred:** the trio's third anchor `0xD8` - retail's table stores two
/// positioned panels per arm; both pinned pairs sit `0x66` apart, and
/// `0x72 + 0x66 = 0xD8` continues that stride.
///
/// **Approximate:** the panel Y band, panel height, and every in-panel bar /
/// pip rect - retail's vertical placement lives in the text-actor open call
/// (`FUN_8003541C` layout args), which is not fully decoded.
const PANEL_STAGE_W: i32 = 0x40;
/// Stage Y of the panel band (approximation - see [`PANEL_STAGE_W`]).
const PANEL_STAGE_Y: i32 = 186;
/// Stage height of one panel (approximation).
const PANEL_STAGE_H: i32 = 52;

/// Stage X of party panel `ordinal` (0-based) for `count` live party members.
/// See [`PANEL_STAGE_W`] for what is pinned vs inferred here.
fn party_panel_stage_x(count: usize, ordinal: usize) -> i32 {
    match (count, ordinal) {
        (1, _) => 0x72,
        (2, 0) => 0x3F,
        (2, _) => 0xA5,
        (_, 0) => 0x0C,
        (_, 1) => 0x72,
        (_, _) => 0xD8,
    }
}

/// Build [`TextDraw`]s for the battle HUD.
///
/// Two surfaces in one list:
///
/// * **Party panels** - retail-shaped per-character panels across the bottom
///   of the 320x240 stage (X anchors + width pinned from the battle overlay,
///   see [`PANEL_STAGE_W`]): a bordered backdrop, the character name, a
///   filled HP bar + `cur/max` numerals, a filled MP bar + numerals, the AP
///   pip row, the status-letter strip, and a "K.O." overlay. Bar fills take
///   the retail gauge index carried in [`HudSlotView::hp_fill`] /
///   [`HudSlotView::mp_fill`] (`FUN_80046A20`) through
///   [`gauge_fill_color`]; numerals take the retail readout-tint law
///   ([`hp_bar_color_index`] / [`mp_bar_color_index`]).
/// * **Monster rows** - compact `name  cur/max` rows with a thin HP bar,
///   anchored at `pen` in raw surface pixels, one row per actor-table slot.
///   An engine enhancement: retail's HUD draws no monster gauge at all, but
///   hiding them would lose information the debug HUD already showed.
///
/// The log column stacks below the monster rows; damage popups anchor to
/// their slot's panel (party) or row (monsters). All filled rects ride
/// [`BattleHudFrame::solid_src`]; without it the builder emits text only.
pub fn battle_hud_draws_for(
    font: &legaia_font::Font,
    frame: &BattleHudFrame<'_>,
    pen: (i32, i32),
) -> Vec<TextDraw> {
    const LINE_H: i32 = 14;
    /// Monster-row column origins (surface px, relative to `pen.x`): HP
    /// numerals, K.O. tag, status strip. Sized from measured advances of the
    /// retail dialog font (longest monster name 69 px + gutter).
    const HP_X: i32 = 78;
    const KO_X: i32 = 150;
    const STATUS_X: i32 = 190;
    const STATUS_STEP: i32 = 8;
    const POPUP_X: i32 = 80;
    /// Monster HP bar width / height, surface px.
    const MBAR_W: i32 = 60;
    const MBAR_H: i32 = 3;

    let white: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
    let monster: [f32; 4] = [1.0, 0.7, 0.7, 1.0];
    let dim: [f32; 4] = [0.5, 0.5, 0.5, 1.0];
    let red: [f32; 4] = [1.0, 0.4, 0.4, 1.0];
    let green: [f32; 4] = [0.5, 1.0, 0.5, 1.0];
    let yellow: [f32; 4] = [1.0, 0.95, 0.4, 1.0];
    let cyan: [f32; 4] = [0.5, 0.85, 1.0, 1.0];
    let panel_bg: [f32; 4] = [0.05, 0.07, 0.16, 0.80];
    let panel_frame: [f32; 4] = [0.78, 0.72, 0.50, 0.95];
    let bar_back: [f32; 4] = [0.10, 0.10, 0.12, 0.90];
    let pip_on: [f32; 4] = [0.45, 0.80, 1.0, 1.0];
    let pip_off: [f32; 4] = [0.28, 0.30, 0.36, 1.0];

    // The boot/menu stage transform: integer upscale of the 320x240 stage,
    // centred in the surface.
    let scale = (frame.surface.0 / BOOT_UI_STAGE_W)
        .min(frame.surface.1 / BOOT_UI_STAGE_H)
        .clamp(1, 4) as i32;
    let origin = (
        (frame.surface.0 as i32 - BOOT_UI_STAGE_W as i32 * scale) / 2,
        (frame.surface.1 as i32 - BOOT_UI_STAGE_H as i32 * scale) / 2,
    );

    let mut out = Vec::new();
    // Solid rect in stage pixels (scaled into the surface).
    let stage_rect = |out: &mut Vec<TextDraw>, x: i32, y: i32, w: i32, h: i32, c: [f32; 4]| {
        if let Some(src) = frame.solid_src
            && w > 0
            && h > 0
        {
            out.push(TextDraw {
                dst: (
                    origin.0 + x * scale,
                    origin.1 + y * scale,
                    (w * scale) as u32,
                    (h * scale) as u32,
                ),
                src,
                color: c,
            });
        }
    };
    // Text laid out at a stage-pixel pen, upscaled with the stage transform
    // (same shape as `scale_stage_text_draws`, applied per burst).
    let stage_text = |out: &mut Vec<TextDraw>,
                      font: &legaia_font::Font,
                      s: &str,
                      x: i32,
                      y: i32,
                      c: [f32; 4]| {
        let layout = font.layout_ascii(s);
        let mut draws = text_draws_for(&layout, (x, y), c);
        scale_stage_text_draws(&mut draws, origin, scale as u32);
        out.extend(draws);
    };

    // Retail readout-tint law -> HUD palette. The "normal" tier (7) keeps the
    // row's base colour so monster rows stay tinted; danger -> red, caution ->
    // yellow, K.O. -> dim.
    let tint = |idx: u8, base: [f32; 4]| -> [f32; 4] {
        match idx {
            9 => red,
            6 => yellow,
            2 => dim,
            _ => base,
        }
    };

    let party_count = frame
        .slots
        .iter()
        .filter(|s| s.is_party && !s.name.is_empty())
        .count()
        .clamp(1, 3);

    // Per-slot popup anchor, filled in as rows/panels are laid out. Surface px.
    let mut popup_anchor: Vec<(i32, i32)> = frame
        .slots
        .iter()
        .enumerate()
        .map(|(i, _)| (pen.0 + POPUP_X, pen.1 + i as i32 * LINE_H - 16))
        .collect();

    let mut party_ordinal = 0usize;
    for (i, slot) in frame.slots.iter().enumerate() {
        if slot.name.is_empty() {
            continue;
        }
        if slot.is_party {
            // ---- Retail-shaped bottom panel ----
            let px = party_panel_stage_x(party_count, party_ordinal);
            party_ordinal += 1;
            let py = PANEL_STAGE_Y;
            popup_anchor[i] = (origin.0 + (px + 8) * scale, origin.1 + (py - 26) * scale);

            // Backdrop + 1-px frame.
            stage_rect(&mut out, px, py, PANEL_STAGE_W, PANEL_STAGE_H, panel_bg);
            stage_rect(&mut out, px, py, PANEL_STAGE_W, 1, panel_frame);
            stage_rect(
                &mut out,
                px,
                py + PANEL_STAGE_H - 1,
                PANEL_STAGE_W,
                1,
                panel_frame,
            );
            stage_rect(&mut out, px, py, 1, PANEL_STAGE_H, panel_frame);
            stage_rect(
                &mut out,
                px + PANEL_STAGE_W - 1,
                py,
                1,
                PANEL_STAGE_H,
                panel_frame,
            );

            let base = if slot.alive { white } else { dim };
            stage_text(&mut out, font, slot.name, px + 4, py + 2, base);

            // HP bar (fill colour = retail gauge index) + numerals (retail
            // readout tint). `hp` is the ramping display value upstream.
            let hp_frac = if slot.hp_max == 0 {
                0.0
            } else {
                (slot.hp as f32 / slot.hp_max as f32).clamp(0.0, 1.0)
            };
            // Retail's dead arm (`FUN_80046A20` colour 2) greys the whole
            // gauge, not just the fill - a zero-width fill would never show
            // it, so the track itself takes the dead colour.
            let hp_track = if slot.hp_fill == 2 {
                gauge_fill_color(2)
            } else {
                bar_back
            };
            stage_rect(&mut out, px + 4, py + 16, 56, 5, hp_track);
            let fill_w = (56.0 * hp_frac).round() as i32;
            let fill_w = if slot.hp > 0 { fill_w.max(1) } else { fill_w };
            stage_rect(
                &mut out,
                px + 4,
                py + 16,
                fill_w,
                5,
                gauge_fill_color(slot.hp_fill),
            );
            let hp_color = if !slot.alive {
                dim
            } else {
                tint(
                    hp_bar_color_index(slot.hp, slot.hp_max, !slot.status_letters.is_empty()),
                    base,
                )
            };
            stage_text(
                &mut out,
                font,
                &format!("{}/{}", slot.hp, slot.hp_max),
                px + 4,
                py + 22,
                hp_color,
            );

            // MP bar + numerals (party rows only carry an MP ceiling).
            if slot.mp_max > 0 {
                let mp_frac = (slot.mp as f32 / slot.mp_max as f32).clamp(0.0, 1.0);
                let mp_track = if slot.mp_fill == 2 {
                    gauge_fill_color(2)
                } else {
                    bar_back
                };
                stage_rect(&mut out, px + 4, py + 38, 26, 4, mp_track);
                stage_rect(
                    &mut out,
                    px + 4,
                    py + 38,
                    (26.0 * mp_frac).round() as i32,
                    4,
                    gauge_fill_color(slot.mp_fill),
                );
                let mp_color = if !slot.alive {
                    dim
                } else {
                    tint(mp_bar_color_index(slot.mp, slot.mp_max), base)
                };
                stage_text(
                    &mut out,
                    font,
                    &format!("{}/{}", slot.mp, slot.mp_max),
                    px + 33,
                    py + 33,
                    mp_color,
                );
            }

            // AP pips.
            if slot.ap_max > 0 {
                if frame.solid_src.is_some() {
                    for n in 0..slot.ap_max {
                        let c = if n < slot.ap_filled { pip_on } else { pip_off };
                        stage_rect(&mut out, px + 4 + n as i32 * 6, py + 45, 4, 4, c);
                    }
                } else {
                    // Text-only fallback.
                    let pips: String = (0..slot.ap_max)
                        .map(|n| if n < slot.ap_filled { 'o' } else { '-' })
                        .collect();
                    stage_text(&mut out, font, &pips, px + 4, py + 42, base);
                }
            }

            // Status strip above the panel.
            for (k, letter) in slot.status_letters.iter().enumerate() {
                let s = (*letter as char).to_string();
                stage_text(
                    &mut out,
                    font,
                    &s,
                    px + 4 + k as i32 * STATUS_STEP,
                    py - 12,
                    yellow,
                );
            }

            if !slot.alive {
                stage_text(&mut out, font, "K.O.", px + 18, py + 22, red);
            }
        } else {
            // ---- Compact monster row (engine enhancement; retail hides
            // monster HP entirely) ----
            let row_y = pen.1 + i as i32 * LINE_H;
            let base = if slot.alive { monster } else { dim };

            let name_layout = font.layout_ascii(slot.name);
            out.extend(text_draws_for(&name_layout, (pen.0, row_y), base));

            let hp_color = if !slot.alive {
                dim
            } else {
                tint(
                    hp_bar_color_index(slot.hp, slot.hp_max, !slot.status_letters.is_empty()),
                    base,
                )
            };
            let hp_layout = font.layout_ascii(&format!("{}/{}", slot.hp, slot.hp_max));
            out.extend(text_draws_for(&hp_layout, (pen.0 + HP_X, row_y), hp_color));

            // Thin HP bar under the numerals (surface px, unscaled like the
            // rest of the monster block).
            if let Some(src) = frame.solid_src {
                let frac = if slot.hp_max == 0 {
                    0.0
                } else {
                    (slot.hp as f32 / slot.hp_max as f32).clamp(0.0, 1.0)
                };
                let track = if slot.hp_fill == 2 {
                    gauge_fill_color(2)
                } else {
                    bar_back
                };
                out.push(TextDraw {
                    dst: (pen.0 + HP_X, row_y + 12, MBAR_W as u32, MBAR_H as u32),
                    src,
                    color: track,
                });
                let w = (MBAR_W as f32 * frac).round() as i32;
                let w = if slot.hp > 0 { w.max(1) } else { w };
                if w > 0 {
                    out.push(TextDraw {
                        dst: (pen.0 + HP_X, row_y + 12, w as u32, MBAR_H as u32),
                        src,
                        color: gauge_fill_color(slot.hp_fill),
                    });
                }
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
                    (pen.0 + STATUS_X + k as i32 * STATUS_STEP, row_y),
                    yellow,
                ));
            }
        }
    }

    let log_x = pen.0;
    let log_y = pen.1 + frame.slots.len() as i32 * LINE_H + 4;
    for (i, line) in frame.log.iter().enumerate() {
        let layout = font.layout_ascii(line.text);
        out.extend(text_draws_for(
            &layout,
            (log_x, log_y + i as i32 * LINE_H),
            line.color,
        ));
    }

    for popup in frame.popups {
        if (popup.slot as usize) >= frame.slots.len() {
            continue;
        }
        let (ax, ay) = popup_anchor[popup.slot as usize];
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
        out.extend(text_draws_for(&layout, (ax, ay), popup_color));
    }

    out
}

/// One laid-out row of the enemy target-selection strip.
///
/// Hosts build these from `engine-core::battle_hud::battle_enemy_target_rows`
/// after running `target_picker::layout_enemy_menu_rows` with their font's
/// measurer: `label` is the retail dedup-labelled monster name, `x` the
/// layout's stage-pixel left edge (320-wide stage), `selected` whether the
/// picker's cursor slot falls inside this row's formation run.
pub struct EnemyTargetRowView<'a> {
    pub label: &'a str,
    pub x: i16,
    pub selected: bool,
}

/// Stage Y of the enemy target strip. An approximation: retail's row Y comes
/// from the caller-side text-actor placement, not from the row builder
/// `FUN_801D9D3C` itself; the band sits above the party panels.
const ENEMY_MENU_STAGE_Y: i32 = 166;

/// Build [`TextDraw`]s for the enemy target-selection name strip.
///
/// The row *content* is retail's: dedup labels from `FUN_801D9D3C` and the
/// centre/relax/clamp X layout from its second half
/// (`target_picker::layout_enemy_menu_rows`), both run by the caller. This
/// builder only projects the laid-out rows onto the surface: each label at
/// its stage X (integer-upscaled + centred, the same transform as the battle
/// HUD panels), the selected row in white behind a `>` cursor, the rest
/// dimmed.
pub fn enemy_target_menu_draws_for(
    font: &legaia_font::Font,
    rows: &[EnemyTargetRowView<'_>],
    surface: (u32, u32),
) -> Vec<TextDraw> {
    let white: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
    let dim: [f32; 4] = [0.62, 0.62, 0.66, 1.0];
    let scale = (surface.0 / BOOT_UI_STAGE_W)
        .min(surface.1 / BOOT_UI_STAGE_H)
        .clamp(1, 4);
    let origin = (
        (surface.0 as i32 - BOOT_UI_STAGE_W as i32 * scale as i32) / 2,
        (surface.1 as i32 - BOOT_UI_STAGE_H as i32 * scale as i32) / 2,
    );
    let mut out = Vec::new();
    for row in rows {
        let color = if row.selected { white } else { dim };
        let mut draws = text_draws_for(
            &font.layout_ascii(row.label),
            (i32::from(row.x), ENEMY_MENU_STAGE_Y),
            color,
        );
        if row.selected {
            draws.extend(text_draws_for(
                &font.layout_ascii(">"),
                (i32::from(row.x) - 9, ENEMY_MENU_STAGE_Y),
                white,
            ));
        }
        scale_stage_text_draws(&mut draws, origin, scale);
        out.extend(draws);
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
