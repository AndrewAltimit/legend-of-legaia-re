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

use crate::battle_hud_chrome::{
    STATUS_BADGE_PANEL_SEAT, STATUS_BADGE_SIZE, STATUS_BADGE_TAG_DY,
    message_banner_chrome_draws_for, message_banner_content, message_banner_text_draws_for,
};
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
    /// The **one** status element retail draws for this slot: the sprite id
    /// `0x18..=0x20` its priority ladder selected, or `0` for the no-ailment
    /// base marker. Engines fill it from
    /// `legaia_engine_core::battle_hud::BattleSlotHud::status_sprite`; see
    /// [`status_element_label`] for the id space.
    pub status_sprite: u8,
    /// Displayed character level - the count retail draws beside the base
    /// marker when `status_sprite == 0`. `0` suppresses the number.
    pub level: u8,
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
    /// `name` borrows from the caller; ownership stays in the engine-core
    /// view buffer.
    pub fn from_plain(meta: HudSlotMeta, name: &'a str) -> Self {
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
            status_sprite: meta.status_sprite,
            level: meta.level,
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
    /// See [`HudSlotView::status_sprite`].
    pub status_sprite: u8,
    /// See [`HudSlotView::level`].
    pub level: u8,
}

/// Short label for a retail status-element sprite id.
///
/// The ids are `FUN_8002C2E4`'s priority-ladder outputs and the ailment each
/// one stands for is pinned in
/// `legaia_engine_vm::status_effects::display_flags`. `engine-ui` sits below
/// `engine-vm` in the crate graph so the id space is mirrored here as
/// literals rather than imported; the mapping is one line per row of that
/// module's table.
///
/// The retail **art** for these ids is a sprite sheet the engine does not
/// resolve, so hosts draw a labelled badge instead - the selection is what is
/// ported, not the pixels. Returns `""` for `0` (the no-ailment base marker)
/// and for any id outside the band.
pub fn status_element_label(sprite: u8) -> &'static str {
    match sprite {
        0x18 => "VNM",
        0x19 => "TOX",
        0x1A => "STN",
        0x1B => "ROT",
        0x1C => "CNF",
        0x1D => "NMB",
        0x1E => "SLP",
        0x1F => "CRS",
        // Retail's own badge cell for this id reads **Faint** - the word is
        // legible in a battle frame's framebuffer (a party member at 0 HP
        // wears it where the `LV` label would be). The tag stands in for that
        // cell, so it carries the same word rather than a "K.O." of the
        // port's own invention.
        0x20 => "Faint",
        _ => "",
    }
}

/// Badge colour for a retail status-element sprite id. Engine-chosen (the
/// retail sheet's palette is not resolved) but grouped by what the ailment
/// does: violet for the two poisons, grey for the inert pair (Stone / Numb),
/// blue for Sleep, magenta for the delegation group - which is the colour
/// family `FUN_8004A908` tints the *actor* mesh with for the same three
/// masks - amber for Rot, cyan for Curse, dim for K.O.
pub fn status_element_color(sprite: u8) -> [f32; 4] {
    match sprite {
        0x18 => [0.72, 0.45, 0.95, 1.0], // Venom
        0x19 => [0.85, 0.30, 0.90, 1.0], // Toxic
        0x1A => [0.62, 0.60, 0.55, 1.0], // Stone
        0x1B => [1.0, 0.72, 0.20, 1.0],  // Rot
        0x1C => [0.94, 0.35, 0.94, 1.0], // Confuse
        0x1D => [0.70, 0.70, 0.78, 1.0], // Numb
        0x1E => [0.45, 0.65, 1.0, 1.0],  // Sleep
        0x1F => [0.40, 0.90, 0.95, 1.0], // Curse
        0x20 => [0.55, 0.30, 0.30, 1.0], // K.O.
        _ => [1.0, 1.0, 1.0, 1.0],
    }
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

/// RGBA for a retail gauge / readout tint index.
///
/// The index space is retail's (`FUN_80046A20` / the readout-tint pair
/// `FUN_800349EC` / `FUN_80035EA8`: `2` dead, `3` status override, `7` high,
/// `6` mid, `9` low), and so are the **colours** - they are no longer
/// approximations.
///
/// Retail resolves a tier by *selecting a whole font CLUT*: the drawn
/// numerals' palette byte is `tier + 6`, whose 16-entry CLUT sits at VRAM
/// `(16 * (tier + 6), 510)`, and the glyph body is that CLUT's **entry 15**.
/// Read straight out of a retail battle frame's VRAM (a party frame with one
/// member at 0 HP, two in the danger tier), which pins five of the six rows
/// at once:
///
/// | tier | palette | VRAM x | entry 15 | reads as |
/// |---|---|---|---|---|
/// | 2 empty / K.O. | 8 | 128 | `(230, 32, 0)` | red |
/// | 3 status lock | 9 | 144 | `(230, 106, 230)` | magenta |
/// | 6 caution | 12 | 192 | `(230, 172, 0)` | amber |
/// | 7 normal | 13 | 208 | `(205, 205, 205)` | light grey |
/// | 9 danger | 15 | 240 | `(222, 90, 0)` | orange |
///
/// Note tier 2 is **red**, not a grey-out: retail paints a downed member's
/// whole readout in the brightest colour on the strip. The port used to grey
/// it, which read as "this panel is disabled" rather than "this member is
/// down".
pub fn gauge_fill_color(idx: u8) -> [f32; 4] {
    match idx {
        2 => [0.902, 0.125, 0.0, 1.0],
        3 => [0.902, 0.416, 0.902, 1.0],
        6 => [0.902, 0.675, 0.0, 1.0],
        9 => [0.871, 0.353, 0.0, 1.0],
        _ => READOUT_NORMAL,
    }
}

/// Retail's "normal" readout colour - `(205, 205, 205)`, entry 15 of the
/// tier-7 CLUT. It is also the colour every **name** on the battle strip is
/// drawn in, downed members included; see [`gauge_fill_color`].
pub const READOUT_NORMAL: [f32; 4] = [0.804, 0.804, 0.804, 1.0];

/// Everything one battle-HUD frame draws, plus the chrome inputs the
/// retail-shaped strip needs. Bundled so [`battle_hud_draws_for`] stays under
/// clippy's argument-count threshold as the surface grows.
#[derive(Default)]
pub struct BattleHudFrame<'a> {
    /// Per-slot rows, indexed by **absolute actor-table slot** (party
    /// `0..party_count`, monsters above). Inactive slots are empty-name
    /// entries the builder skips - compacting would mis-anchor popups.
    pub slots: &'a [HudSlotView<'a>],
    pub popups: &'a [HudPopupView],
    pub log: &'a [HudLogView<'a>],
    /// 1x1 solid-white atlas rect from [`font_solid_src`]. `None` degrades
    /// the strip to text-only (no filled rects).
    pub solid_src: Option<(u32, u32, u32, u32)>,
    /// Surface size in pixels. The party strip is laid out on the retail
    /// 320x240 stage and integer-upscaled + centred into this surface, the
    /// same transform the boot/menu stage uses.
    pub surface: (u32, u32),
    /// System-UI atlas rects for the strip chrome (gradient fill + gold
    /// 9-slice frame) and the gold/green HP / MP label sprites
    /// (`OVERLAY_SYSTEM_UI_LABEL_HP` / `_MP`, `legaia_asset::title_pak`).
    /// `None` degrades the chrome to solid-texel rects and the labels to
    /// tinted "HP" / "MP" text.
    pub chrome: Option<&'a SaveMenuAtlasRects>,
    /// Top-left announcement plaque label. Retail draws the **acting
    /// actor's** name in a framed plaque at the top-left corner - "Vahn"
    /// while Vahn's item lands (`captures/tetsu_idle`), "Tetsu" through the
    /// enemy's rush - so hosts pass
    /// `engine-core::battle_hud::battle_plaque_label`. The plaque *skin* is
    /// not yet pinned; the label draws as clean text at the plaque's
    /// captured text inset until it is.
    pub plaque: Option<&'a str>,
    /// Element-badge index (`0..8`) the plaque wears in front of the name.
    /// Retail's plaque interior is `20 + 5 + name width` when the actor
    /// carries one, which is the second half of the pinned plaque law
    /// (`battle_chrome::name_plaque`). `None` draws the bare name.
    pub plaque_badge: Option<u8>,
    /// A battle message holding the **top-of-screen banner**. Retail's
    /// banner and the actor-name plaque are one seat (content pen
    /// `(16, 12)`), so this wins: while a message is up the builder draws
    /// the class-0 frame and its text and emits no plaque.
    pub banner: Option<&'a str>,
    /// The host is already drawing its own box on that seat (the sparring
    /// tutorial prompt). Suppresses the plaque without drawing a banner -
    /// two text runs on one pen is the artifact this exists to stop.
    pub plaque_seat_taken: bool,
    /// Atlas cells for the status-element and element badges
    /// ([`crate::battle_hud_chrome::BattleBadgeRects`]). Any `None` cell
    /// falls back to the engine's labelled text tag.
    pub badges: Option<&'a crate::battle_hud_chrome::BattleBadgeRects>,
    /// Retail parks the party status plate off-screen while an arts input
    /// session owns the frame (`docs/subsystems/minigame-muscle-dome.md` -
    /// its draws move to `y = 230`, under the 228-line display window), and
    /// the arts-input reference frame shows the command bar in its place.
    /// Hosts set this while a command-entry session is up; the builder then
    /// emits no party strip.
    pub input_session_parked: bool,
    /// Stage rect (`x, y, w, h`) of a box the **host** draws on top of the
    /// battle screen this frame - today the sparring-tutorial prompt, whose
    /// rect is the retail emitter's own (`battle_tutorial::BoxStyle::box_rect`).
    ///
    /// Retail's bottom-anchored prompt styles anchor at `y = 0xCC / 0xB0 /
    /// 0x9A`, and a one-line style-2/3 box therefore lands on `188..212` -
    /// exactly the rows the active-actor bar occupies, and inside the roster
    /// panels' `164..212`. Drawing both puts two text runs on the same
    /// pixels, the same defect the plaque seat already has an exclusive
    /// branch for; the builder resolves it the same way and omits whichever
    /// party surface the box's drawn footprint covers.
    ///
    /// This is the box's **centre** rect: the drawn window skin extends
    /// [`HOST_BOX_SKIN`] px beyond it on every side (the reading box's
    /// 9-slice inflation), and that inflated rect is what is tested.
    ///
    /// `None` (the default) leaves both surfaces alone.
    pub host_box: Option<(i32, i32, i32, i32)>,
    /// Actor-table slot of the actor the frame belongs to
    /// (`engine-core::battle_hud::battle_active_actor`). When it names a live
    /// party member, retail replaces that member's resting panel readout with
    /// the full-width active-actor bar; hosts also pass the same actor's name
    /// as [`Self::plaque`].
    pub active_slot: Option<u8>,
    /// Diagnostic readout ([`diag_hud_enabled`]): the engine's debug rows -
    /// monster HP numerals/bars (retail draws **no** monster gauge,
    /// `docs/subsystems/battle-action.md`), per-slot LV / AP readouts, and
    /// the base-marker + level element (`FUN_8002C2E4`'s no-ailment arm,
    /// whose retail widget placement is not pinned). Off by default.
    pub diag: bool,
}

/// The two draw lists one battle-HUD frame produces: `text` samples the
/// dialog-font atlas (glyphs + solid-texel rects), `sprites` samples the
/// resident system-UI atlas (strip chrome + HP/MP label sprites). Hosts
/// composite `sprites` under `text`, the same layering as the dialog and
/// menu chrome.
#[derive(Default)]
pub struct BattleHudDraws {
    pub text: Vec<TextDraw>,
    pub sprites: Vec<SpriteDraw>,
}

/// Shared host toggle for the diagnostic battle readout (and the engine's
/// "ENCOUNTER!" transition banner, which has no retail counterpart): set
/// `LEGAIA_DIAG_HUD` to anything but `0`/empty. Reads the environment so
/// both hosts resolve the same answer; on wasm the variable never exists
/// and the diagnostics stay off.
pub fn diag_hud_enabled() -> bool {
    std::env::var("LEGAIA_DIAG_HUD")
        .map(|v| !v.is_empty() && v != "0")
        .unwrap_or(false)
}

/// Battle-HUD stage geometry, mirrored from the packet-pinned
/// `legaia_engine_vm::battle_chrome` because `engine-ui` sits below
/// `engine-vm` in the crate graph. `engine-shell`'s HUD tests pin the two
/// sets equal, which is the only thing that keeps the copy honest.
///
/// Retail's party readout is **two mutually-exclusive surfaces**, not one:
/// per-member roster panels at rest, and a single full-width bar for the
/// actor entering a command or acting. The bar does not replace the panels
/// by hiding them - retail parks them at `y = 230`, under its 228-line
/// display window. The engine stage is 240 lines, so `y = 230` would still
/// be visible here and the port omits the parked draws instead.
///
/// Every seat below is packet-pinned from a save-state display-list walk.
/// What is **not** pinned, and is called out where it is used: the 102x48
/// marbled panel background and the 8x16 `/` separator have no rect in the
/// engine's system-UI atlas set yet, so the panel draws in the shared
/// chrome and the separator as a font glyph.
const BAR_X: i32 = 8;
/// Plate top of the active-actor bar.
const BAR_Y: i32 = 188;
/// Interior width of the active-actor bar; the plate spans `8 ..= 312`.
const BAR_INTERIOR_W: i32 = 288;
/// Plate height, every plate run on the battle screen.
const PLATE_H: i32 = 20;
/// Width a plate run occupies for a given interior (a cap at each end).
const PLATE_CAP_W: i32 = 8;

/// Name-glyph pen inside the active-actor bar.
const BAR_NAME: (i32, i32) = (16, 192);
/// HP / MP label-sprite seats inside the bar.
const BAR_HP_LABEL: (i32, i32) = (80, 194);
const BAR_MP_LABEL: (i32, i32) = (192, 194);
/// `/` separator seats - the separator sits four rows above its numerals.
const BAR_HP_SEPARATOR: (i32, i32) = (136, 188);
const BAR_MP_SEPARATOR: (i32, i32) = (240, 188);
/// Numeral pen row, every field in the bar.
const BAR_DIGIT_Y: i32 = 192;
/// Right edges the four numeral fields are laid out back from. **Both**
/// halves of a `cur / max` pair are right-aligned - the field grows leftward
/// one 8-px cell per digit - which is what keeps a four-digit HP inside its
/// own field. A forward-running maximum is what a capture whose values are
/// all three digits looks like, and it overruns as soon as they are not.
const BAR_HP_CUR_RIGHT: i32 = 134;
const BAR_HP_MAX_RIGHT: i32 = 178;
const BAR_MP_CUR_RIGHT: i32 = 238;
const BAR_MP_MAX_RIGHT: i32 = 274;

/// Width and horizontal pitch of one HUD numeral cell - retail's, and the
/// unit every numeral field above is measured in.
const DIGIT_W: i32 = 8;

/// Plate top-left of the actor-name plaque - fixed, every battle.
const PLAQUE_X: i32 = 8;
const PLAQUE_Y: i32 = 8;
/// Vertical inset of the plaque's contents from its plate top.
const PLAQUE_CONTENT_DY: i32 = 4;
/// Width of the element badge the plaque wears, and the gap between it and
/// the first name glyph (`battle_chrome::BADGE_W` / `PLAQUE_BADGE_GAP`).
const PLAQUE_BADGE_W: i32 = 20;
const PLAQUE_BADGE_GAP: i32 = 5;

/// How far a host-drawn window's skin extends past its centre rect, every
/// side - the dialog reading box's 9-slice inflation, which the tutorial
/// prompt reuses ([`crate::battle_tutorial_chrome_draws_for`]).
pub const HOST_BOX_SKIN: i32 = 8;

/// Roster-panel background size and row.
const PANEL_W: i32 = 102;
const PANEL_H: i32 = 48;
const PANEL_Y: i32 = 164;

/// Per-member panel content seats, relative to the panel's top-left corner
/// (`battle_chrome::panel`).
const PANEL_NAME: (i32, i32) = (5, 4);
const PANEL_LV_LABEL: (i32, i32) = (64, 6);
/// Right edge the level numerals are laid out back from - right-aligned like
/// every other number on the screen, not a pen.
const PANEL_LV_DIGITS_RIGHT: i32 = 96;
const PANEL_LV_DIGIT_Y: i32 = 4;
const PANEL_HP_LABEL: (i32, i32) = (4, 21);
const PANEL_MP_LABEL: (i32, i32) = (4, 36);
const PANEL_HP_SEPARATOR: (i32, i32) = (57, 15);
const PANEL_MP_SEPARATOR: (i32, i32) = (57, 30);
const PANEL_HP_DIGIT_Y: i32 = 19;
const PANEL_MP_DIGIT_Y: i32 = 34;
const PANEL_CUR_RIGHT: i32 = 57;
/// Right edge a maximum is laid out back from. `102 - 5`: the panel's
/// content box is inset five pixels on both sides, so a four-digit maximum
/// runs `65..97` and still clears the plate.
const PANEL_MAX_RIGHT: i32 = 97;

/// Panel-background x seats for a party of `count`, left to right
/// (`battle_chrome::panel_seats`). These are the **backgrounds**;
/// [`party_panel_stage_x`] carries the same layout as *text* anchors, which
/// is the `+5` `PANEL_NAME` inset and only the slots `FUN_801D84C0` writes.
fn panel_seats(count: usize) -> &'static [i32] {
    match count {
        1 => &[109],
        2 => &[58, 160],
        3 => &[7, 109, 211],
        _ => &[],
    }
}

/// Stage X of party section `ordinal` (0-based) for `count` live party
/// members - retail's `FUN_801D84C0` anchor table, which seats the roster
/// panels' **name pens**, `+5` inside the panel background
/// ([`panel_seats`]). Mirrored here as literals for the same crate-graph
/// reason as the rest of this block, and pinned equal to
/// `legaia_engine_vm::battle_party_panel::panel_anchors` by `engine-shell`'s
/// HUD tests.
pub fn party_panel_stage_x(count: usize, ordinal: usize) -> i32 {
    match (count, ordinal) {
        (1, _) => 0x72,
        (2, 0) => 0x3F,
        (2, _) => 0xA5,
        (_, 0) => 0x0C,
        (_, 1) => 0x72,
        (_, _) => 0xD8,
    }
}

/// Build the battle-HUD draw lists ([`BattleHudDraws`]).
///
/// The default (retail-shaped) surface:
///
/// * **The party strip** - ONE full-width strip across the bottom of the
///   320x240 stage, per the retail captures (see [`STRIP_STAGE_X`] for the
///   per-constant provenance): ornate frame + blue-gradient interior
///   (system-UI atlas chrome via [`BattleHudFrame::chrome`]), and per
///   member the name, the gold `HP` label sprite with right-aligned
///   `cur / max` numerals, and the green `MP` label sprite with numerals.
///   Numerals take the retail readout-tint law ([`hp_bar_color_index`] /
///   [`mp_bar_color_index`]); a dead member's name + numerals dim. Retail
///   draws **no HP/MP bars and no AP pips in the strip** - the captured
///   strip is numerals-only - so the engine draws none by default.
/// * **The plaque** - the acting actor's name as clean text at the
///   top-left plaque inset ([`BattleHudFrame::plaque`]).
/// * **Status badge** - when a slot's retail-selected status element
///   ([`HudSlotView::status_sprite`], `FUN_8002C2E4`'s ladder) is an
///   ailment, a labelled badge above the member's strip section (the
///   selection is retail; badge art + placement are engine approximations).
/// * Damage popups + the log column.
///
/// Retail draws **no monster gauge at all**
/// (`docs/subsystems/battle-action.md`), so monster rows, the per-slot
/// LV / AP readouts and the thin bars only draw under
/// [`BattleHudFrame::diag`] - the debug surface, off by default.
///
/// All filled rects ride [`BattleHudFrame::solid_src`]; without it the
/// builder emits text only. Chrome sprites ride
/// [`BattleHudFrame::chrome`]; without it the strip backdrop degrades to
/// solid-texel rects and the labels to tinted text.
pub fn battle_hud_draws_for(
    font: &legaia_font::Font,
    frame: &BattleHudFrame<'_>,
    pen: (i32, i32),
) -> BattleHudDraws {
    const LINE_H: i32 = 14;
    /// Diagnostic-row column origins (surface px, relative to `pen.x`): HP
    /// numerals, K.O. tag, status element, LV/AP tail. Sized from measured
    /// advances of the retail dialog font (longest monster name 69 px +
    /// gutter).
    const HP_X: i32 = 78;
    const KO_X: i32 = 150;
    const STATUS_X: i32 = 190;
    const DIAG_TAIL_X: i32 = 230;
    const POPUP_X: i32 = 80;
    /// Diagnostic HP bar width / height, surface px.
    const MBAR_W: i32 = 60;
    const MBAR_H: i32 = 3;

    let white: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
    let monster: [f32; 4] = [1.0, 0.7, 0.7, 1.0];
    let dim: [f32; 4] = [0.5, 0.5, 0.5, 1.0];
    let red: [f32; 4] = [1.0, 0.4, 0.4, 1.0];
    let green: [f32; 4] = [0.5, 1.0, 0.5, 1.0];
    let yellow: [f32; 4] = [1.0, 0.95, 0.4, 1.0];
    let cyan: [f32; 4] = [0.5, 0.85, 1.0, 1.0];
    // Fallback strip chrome (no system-UI atlas): interior + outline tints
    // approximating the captured strip's navy interior and light-blue rim.
    let strip_bg: [f32; 4] = [0.13, 0.13, 0.35, 0.85];
    let strip_frame: [f32; 4] = [0.58, 0.58, 0.84, 0.95];
    let bar_back: [f32; 4] = [0.10, 0.10, 0.12, 0.90];
    // Fallback label tints when the atlas sprites are unavailable: the
    // captured labels are gold "HP" / green "MP".
    let label_gold: [f32; 4] = [1.0, 0.78, 0.15, 1.0];
    let label_green: [f32; 4] = [0.25, 0.92, 0.55, 1.0];

    // The boot/menu stage transform: integer upscale of the 320x240 stage,
    // centred in the surface.
    let scale = (frame.surface.0 / BOOT_UI_STAGE_W)
        .min(frame.surface.1 / BOOT_UI_STAGE_H)
        .clamp(1, 4) as i32;
    let origin = (
        (frame.surface.0 as i32 - BOOT_UI_STAGE_W as i32 * scale) / 2,
        (frame.surface.1 as i32 - BOOT_UI_STAGE_H as i32 * scale) / 2,
    );

    let mut text: Vec<TextDraw> = Vec::new();
    let mut sprites: Vec<SpriteDraw> = Vec::new();
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

    // Retail readout-tint law -> HUD palette. Every tier but "normal" takes
    // retail's own CLUT colour ([`gauge_fill_color`]); the normal tier keeps
    // the caller's base so the diagnostic monster rows stay row-tinted.
    let tint = |idx: u8, base: [f32; 4]| -> [f32; 4] {
        match idx {
            2 | 3 | 6 | 9 => gauge_fill_color(idx),
            _ => base,
        }
    };

    // Per-slot popup anchor, filled in as rows/panels are laid out. Surface px.
    let mut popup_anchor: Vec<(i32, i32)> = frame
        .slots
        .iter()
        .enumerate()
        .map(|(i, _)| (pen.0 + POPUP_X, pen.1 + i as i32 * LINE_H - 16))
        .collect();

    // ---- The retail party strip ----
    //
    // ONE full-width strip across the stage bottom (see [`STRIP_STAGE_X`]
    // for the per-constant provenance). Solo lays the single member's
    // name / HP / MP across the measured retail columns; a pair or trio
    // takes one section per member at the pinned `FUN_801D84C0` anchors
    // with the rows stacked (approximation - no multi-member retail
    // capture pins the section contents).
    let live_party: Vec<(usize, &HudSlotView<'_>)> = frame
        .slots
        .iter()
        .enumerate()
        .filter(|(_, s)| s.is_party && !s.name.is_empty())
        .collect();

    // Sprite blit in stage pixels.
    let stage_sprite = |out: &mut Vec<SpriteDraw>, src: (u32, u32, u32, u32), x: i32, y: i32| {
        out.push(SpriteDraw {
            dst: (
                origin.0 + x * scale,
                origin.1 + y * scale,
                src.2 * scale as u32,
                src.3 * scale as u32,
            ),
            src,
            color: white,
        });
    };
    // LV / HP / MP label: the resident system-UI atlas sprites
    // (`OVERLAY_SYSTEM_UI_LABEL_LV` / `_HP` / `_MP`, `legaia_asset::title_pak`).
    // Retail packs all three into **one** sub-palette - the gold-vs-green
    // difference is baked into the texels, not a per-label CLUT - so they
    // draw untinted. Without an atlas they degrade to tinted text at the same
    // seats.
    let label =
        |sprites: &mut Vec<SpriteDraw>, text: &mut Vec<TextDraw>, which: u8, x: i32, y: i32| {
            match frame.chrome {
                Some(rects) => {
                    let src = match which {
                        0 => rects.label_hp,
                        1 => rects.label_mp,
                        _ => rects.label_lv,
                    };
                    stage_sprite(sprites, src, x, y);
                }
                None => {
                    let (s, c) = match which {
                        0 => ("HP", label_gold),
                        1 => ("MP", label_green),
                        _ => ("LV", label_gold),
                    };
                    stage_text(text, font, s, x, y, c);
                }
            }
        };
    // The battle screen's own cells, when the atlas carries them.
    let battle = frame.chrome.and_then(|r| r.battle);
    // The `/` between a current and a maximum: retail's 8x16 sheet sprite,
    // which seats four rows **above** its numerals. Without the atlas it
    // degrades to a font glyph on the numeral row.
    let separator =
        |sprites: &mut Vec<SpriteDraw>, text: &mut Vec<TextDraw>, x: i32, y: i32, c: [f32; 4]| {
            match battle {
                Some(b) => sprites.push(SpriteDraw {
                    dst: (
                        origin.0 + x * scale,
                        origin.1 + y * scale,
                        b.separator.2 * scale as u32,
                        b.separator.3 * scale as u32,
                    ),
                    src: b.separator,
                    color: c,
                }),
                None => stage_text(text, font, "/", x, y + 4, c),
            }
        };
    // A number, in retail's own numeral cells: 8x12 sprites off the
    // menu-glyph atlas strip, right-aligned so the field grows leftward one
    // cell per digit.
    //
    // This - not the dialog font - is how retail draws every HP, MP and
    // level on the battle screen, and it is what the seats above were
    // measured against. A proportional-font `9999` is wider than four cells
    // and overruns the roster panel's 102-px plate; four cells do not.
    //
    // Without the strip (an atlas built without the menu-glyph TIM, or no
    // atlas at all) the digits fall back to font glyphs **centred in the
    // same cells**, so the layout is identical and only the letterforms
    // differ.
    let numerals = |sprites: &mut Vec<SpriteDraw>,
                    text: &mut Vec<TextDraw>,
                    value: u32,
                    right: i32,
                    y: i32,
                    c: [f32; 4]| {
        let s = value.to_string();
        let left = right - s.len() as i32 * DIGIT_W;
        for (i, ch) in s.bytes().enumerate() {
            let cell_x = left + i as i32 * DIGIT_W;
            let d = u32::from(ch - b'0');
            match battle
                .and_then(|b| b.digits)
                .and_then(|strip| crate::hud_digit_rect(strip, d))
            {
                Some(src) => sprites.push(SpriteDraw {
                    dst: (
                        origin.0 + cell_x * scale,
                        origin.1 + y * scale,
                        src.2 * scale as u32,
                        src.3 * scale as u32,
                    ),
                    src,
                    color: c,
                }),
                None => {
                    let glyph = [ch];
                    let g = std::str::from_utf8(&glyph).unwrap_or("0");
                    let advance = font.layout_ascii(g).advance_x as i32;
                    stage_text(text, font, g, cell_x + (DIGIT_W - advance) / 2, y, c);
                }
            }
        }
    };
    // Fallback skin for a plate or panel with no atlas at all: a solid
    // interior plus a 1-px rim, which is what keeps a chrome-less host
    // readable.
    let flat_skin = |text: &mut Vec<TextDraw>, rect: (i32, i32, i32, i32)| {
        stage_rect(text, rect.0, rect.1, rect.2, rect.3, strip_bg);
        stage_rect(text, rect.0, rect.1, rect.2, 1, strip_frame);
        stage_rect(text, rect.0, rect.1 + rect.3 - 1, rect.2, 1, strip_frame);
        stage_rect(text, rect.0, rect.1, 1, rect.3, strip_frame);
        stage_rect(text, rect.0 + rect.2 - 1, rect.1, 1, rect.3, strip_frame);
    };
    // One plate run: retail's 3-slice out of the system-UI sheet - an 8-px
    // cap, body tiles filling the interior with the **last tile clipped** to
    // the remainder, and a closing cap. Total width is `interior + 16`, and
    // the clipped final tile is retail's behaviour rather than a rounding of
    // it (`battle_chrome::plate_run`).
    //
    // `art` picks the sheet row: the blue row (`v = 0`) for the bar and the
    // command chips, the carved-gold row (`v = 64`) for the actor plaque.
    // The gold row is the same tiles the field menu's tab banner samples,
    // which is why they are the atlas's `tab_*` rects.
    let plate_run = |text: &mut Vec<TextDraw>,
                     sprites: &mut Vec<SpriteDraw>,
                     x: i32,
                     y: i32,
                     interior_w: i32,
                     gold: bool| {
        let tiles = match (frame.chrome, battle) {
            (Some(r), Some(_)) if gold => Some((r.tab_cap_l, r.tab_body, r.tab_cap_r)),
            (Some(_), Some(b)) => Some((b.plate_cap_l, b.plate_body, b.plate_cap_r)),
            _ => {
                flat_skin(text, (x, y, interior_w + 2 * PLATE_CAP_W, PLATE_H));
                None
            }
        };
        let Some((cap_l, body, cap_r)) = tiles else {
            return;
        };
        let mut blit = |src: (u32, u32, u32, u32), bx: i32, w: u32| {
            sprites.push(SpriteDraw {
                dst: (
                    origin.0 + bx * scale,
                    origin.1 + y * scale,
                    w * scale as u32,
                    src.3 * scale as u32,
                ),
                src: (src.0, src.1, w, src.3),
                color: white,
            });
        };
        blit(cap_l, x, cap_l.2);
        let interior_x = x + PLATE_CAP_W;
        let mut done = 0;
        while done < interior_w {
            let w = (body.2 as i32).min(interior_w - done);
            blit(body, interior_x + done, w as u32);
            done += w;
        }
        blit(cap_r, interior_x + interior_w, cap_r.2);
    };
    // The roster panel's own background: retail blits the whole 102x48
    // marbled plate as one sprite, not as a 9-slice.
    let panel_plate =
        |text: &mut Vec<TextDraw>, sprites: &mut Vec<SpriteDraw>, rect: (i32, i32, i32, i32)| {
            match battle {
                Some(b) => sprites.push(SpriteDraw {
                    dst: (
                        origin.0 + rect.0 * scale,
                        origin.1 + rect.1 * scale,
                        b.panel_bg.2 * scale as u32,
                        b.panel_bg.3 * scale as u32,
                    ),
                    src: b.panel_bg,
                    color: white,
                }),
                None => flat_skin(text, rect),
            }
        };
    // The retail readout-tint law, per slot (`FUN_800349EC` / `FUN_80035EA8`).
    //
    // Two things the port used to get backwards, both read off a retail frame
    // whose third party member is at 0 HP:
    //
    // * a downed member's **name** is drawn in the ordinary readout colour,
    //   not dimmed - retail's name glyphs take CLUT `(208, 510)` on all three
    //   panels, the live pair and the downed one alike;
    // * a downed member's HP **and** MP readouts both take the dead tier,
    //   which is retail's brightest red, not a grey-out. The MP field takes it
    //   even when its own ratio would say "normal", so the death override sits
    //   above `mp_bar_color_index`.
    let tints = |slot: &HudSlotView<'_>| -> ([f32; 4], [f32; 4], [f32; 4]) {
        const DEAD: u8 = 2;
        let base = READOUT_NORMAL;
        let hp = if !slot.alive {
            gauge_fill_color(DEAD)
        } else {
            tint(
                hp_bar_color_index(slot.hp, slot.hp_max, slot.status_sprite != 0),
                READOUT_NORMAL,
            )
        };
        let mp = if !slot.alive {
            gauge_fill_color(DEAD)
        } else {
            tint(mp_bar_color_index(slot.mp, slot.mp_max), READOUT_NORMAL)
        };
        (base, hp, mp)
    };

    // The member whose readout takes the full-width bar, if any. Retail's two
    // party surfaces are **mutually exclusive**: while the bar owns the
    // screen the whole roster cluster is parked off-screen, so a frame shows
    // one or the other and never both.
    let bar_member = frame
        .active_slot
        .and_then(|a| live_party.iter().find(|(i, _)| *i == a as usize).copied());

    // Rows a host-drawn box claims this frame, skin included, and the row
    // test each party surface runs against them ([`BattleHudFrame::host_box`]).
    let host_box_rows = frame
        .host_box
        .map(|(_, y, _, h)| (y - HOST_BOX_SKIN, y + h + HOST_BOX_SKIN));
    let box_covers = |top: i32, height: i32| {
        host_box_rows.is_some_and(|(box_top, box_bot)| box_top < top + height && box_bot > top)
    };
    let panels_covered = box_covers(PANEL_Y, PANEL_H);
    let bar_covered = box_covers(BAR_Y, PLATE_H);

    // ---- Surface 1: the resting roster panels ----
    //
    // One 102x48 panel per live member at the packet-pinned seats, carrying
    // name + LV on the top cell, then an HP row and an MP row. Retail parks
    // the whole cluster at `y = 230` (under its 228-line display window)
    // while the bar or a command-entry session owns the frame; the engine
    // stage is 240 lines, so `y = 230` would still be visible here and the
    // port omits the draws instead.
    let seats = panel_seats(live_party.len().min(3));
    if !frame.input_session_parked && bar_member.is_none() && !panels_covered {
        for (ordinal, (i, slot)) in live_party.iter().take(3).enumerate() {
            let px = seats[ordinal];
            let py = PANEL_Y;
            let (base, hp_tint, mp_tint) = tints(slot);
            panel_plate(&mut text, &mut sprites, (px, py, PANEL_W, PANEL_H));

            stage_text(
                &mut text,
                font,
                slot.name,
                px + PANEL_NAME.0,
                py + PANEL_NAME.1,
                base,
            );
            // `FUN_8002C2E4` draws exactly **one** element per slot, and its
            // ladder is exclusive: the no-ailment arm is the base marker plus
            // the level, and any set bit (or zero HP) replaces both with a
            // single ailment sprite. Retail seats that element on the panel's
            // top cell, so the level and the ailment share one seat here too.
            //
            // The badge is retail's own 48x16 word cell off the system-UI
            // sheet, seated at the ladder caller's `pen + (0x33, -4)`
            // ([`battle_hud_chrome::STATUS_BADGE_PANEL_SEAT`]). Without the
            // cell (an atlas built from a slice that could not reach the
            // row-511 extension palettes) it degrades to the engine's
            // labelled tag on the level seat.
            if slot.status_sprite != 0 {
                match frame
                    .badges
                    .and_then(|b| b.status_badge(slot.status_sprite))
                {
                    Some(src) => stage_sprite(
                        &mut sprites,
                        src,
                        px + STATUS_BADGE_PANEL_SEAT.0,
                        py + STATUS_BADGE_PANEL_SEAT.1,
                    ),
                    // The fallback tag stands in for that 48x16 cell, so it
                    // belongs on the **badge's** seat, centred in the cell -
                    // not on the `LV` label seat the level would have used.
                    // The two are 8 px apart on x and 6 on y, which is what a
                    // host with no badge atlas (the browser play page) shows
                    // as a downed member's tag sitting off its own plate.
                    None => {
                        let label = status_element_label(slot.status_sprite);
                        let w = font.layout_ascii(label).advance_x as i32;
                        stage_text(
                            &mut text,
                            font,
                            label,
                            px + STATUS_BADGE_PANEL_SEAT.0 + (STATUS_BADGE_SIZE.0 - w) / 2,
                            py + STATUS_BADGE_PANEL_SEAT.1 + STATUS_BADGE_TAG_DY,
                            status_element_color(slot.status_sprite),
                        );
                    }
                }
            } else if slot.level > 0 {
                label(
                    &mut sprites,
                    &mut text,
                    2,
                    px + PANEL_LV_LABEL.0,
                    py + PANEL_LV_LABEL.1,
                );
                numerals(
                    &mut sprites,
                    &mut text,
                    u32::from(slot.level),
                    px + PANEL_LV_DIGITS_RIGHT,
                    py + PANEL_LV_DIGIT_Y,
                    base,
                );
            }

            label(
                &mut sprites,
                &mut text,
                0,
                px + PANEL_HP_LABEL.0,
                py + PANEL_HP_LABEL.1,
            );
            numerals(
                &mut sprites,
                &mut text,
                u32::from(slot.hp),
                px + PANEL_CUR_RIGHT,
                py + PANEL_HP_DIGIT_Y,
                hp_tint,
            );
            separator(
                &mut sprites,
                &mut text,
                px + PANEL_HP_SEPARATOR.0,
                py + PANEL_HP_SEPARATOR.1,
                hp_tint,
            );
            numerals(
                &mut sprites,
                &mut text,
                u32::from(slot.hp_max),
                px + PANEL_MAX_RIGHT,
                py + PANEL_HP_DIGIT_Y,
                hp_tint,
            );

            if slot.mp_max > 0 {
                label(
                    &mut sprites,
                    &mut text,
                    1,
                    px + PANEL_MP_LABEL.0,
                    py + PANEL_MP_LABEL.1,
                );
                numerals(
                    &mut sprites,
                    &mut text,
                    u32::from(slot.mp),
                    px + PANEL_CUR_RIGHT,
                    py + PANEL_MP_DIGIT_Y,
                    mp_tint,
                );
                separator(
                    &mut sprites,
                    &mut text,
                    px + PANEL_MP_SEPARATOR.0,
                    py + PANEL_MP_SEPARATOR.1,
                    mp_tint,
                );
                numerals(
                    &mut sprites,
                    &mut text,
                    u32::from(slot.mp_max),
                    px + PANEL_MAX_RIGHT,
                    py + PANEL_MP_DIGIT_Y,
                    mp_tint,
                );
            }

            popup_anchor[*i] = (
                origin.0 + (px + PANEL_W / 2) * scale,
                origin.1 + (py - 16) * scale,
            );
        }
    }

    // ---- Surface 2: the active-actor bar ----
    //
    // The full-width plate the acting party member's readout takes over: name
    // at the left, then the HP label with a right-aligned current and a
    // right-aligned maximum either side of the `/`, then the same pair for
    // MP. Retail draws **no gauge bar** here - the packet run carries no bar
    // primitive in either readout, and neither reference frame shows one.
    if let Some((i, slot)) = bar_member.filter(|_| !bar_covered) {
        let (base, hp_tint, mp_tint) = tints(slot);
        plate_run(&mut text, &mut sprites, BAR_X, BAR_Y, BAR_INTERIOR_W, false);
        stage_text(&mut text, font, slot.name, BAR_NAME.0, BAR_NAME.1, base);
        label(&mut sprites, &mut text, 0, BAR_HP_LABEL.0, BAR_HP_LABEL.1);
        numerals(
            &mut sprites,
            &mut text,
            u32::from(slot.hp),
            BAR_HP_CUR_RIGHT,
            BAR_DIGIT_Y,
            hp_tint,
        );
        separator(
            &mut sprites,
            &mut text,
            BAR_HP_SEPARATOR.0,
            BAR_HP_SEPARATOR.1,
            hp_tint,
        );
        numerals(
            &mut sprites,
            &mut text,
            u32::from(slot.hp_max),
            BAR_HP_MAX_RIGHT,
            BAR_DIGIT_Y,
            hp_tint,
        );
        if slot.mp_max > 0 {
            label(&mut sprites, &mut text, 1, BAR_MP_LABEL.0, BAR_MP_LABEL.1);
            numerals(
                &mut sprites,
                &mut text,
                u32::from(slot.mp),
                BAR_MP_CUR_RIGHT,
                BAR_DIGIT_Y,
                mp_tint,
            );
            separator(
                &mut sprites,
                &mut text,
                BAR_MP_SEPARATOR.0,
                BAR_MP_SEPARATOR.1,
                mp_tint,
            );
            numerals(
                &mut sprites,
                &mut text,
                u32::from(slot.mp_max),
                BAR_MP_MAX_RIGHT,
                BAR_DIGIT_Y,
                mp_tint,
            );
        }
        popup_anchor[i] = (
            origin.0 + BAR_HP_LABEL.0 * scale,
            origin.1 + (BAR_Y - 26) * scale,
        );
    }

    // ---- The top-left seat: banner OR plaque, never both ----
    //
    // Retail's message banner and the actor-name plaque share content pen
    // `(16, 12)`; they are alternatives on one seat. Drawing both is two
    // text runs on the same pixels, so the banner wins when a message is
    // up and the host can also claim the seat outright
    // (`plaque_seat_taken`) for a box it draws itself.
    if let Some(message) = frame.banner.filter(|m| !m.is_empty()) {
        // The class-0 nine-slice, sized to the measured message - and no
        // interior fill: retail's display list carries the border sprites
        // and the glyph run and nothing else, so the scene shows through.
        if let Some(rects) = frame.chrome {
            let content = message_banner_content(font, message);
            sprites.extend(message_banner_chrome_draws_for(
                rects,
                content,
                origin,
                scale as u32,
            ));
        }
        let mut rows = message_banner_text_draws_for(font, message);
        scale_stage_text_draws(&mut rows, origin, scale as u32);
        text.extend(rows);
    } else if let Some(name) = frame.plaque
        && !name.is_empty()
        && !frame.plaque_seat_taken
    {
        // Retail's top-left plaque names the actor the frame belongs to -
        // the party member on his turn, the monster through its attack - on
        // a carved gold plate whose interior is sized to the measured name.
        // Its live seat is `(8, 8)` and its parked seat `(8, -24)`: the
        // plaque slides in from above. The port draws only the live seat.
        //
        // An actor that carries an element wears its badge in front of the
        // name, and the interior grows by the badge plus a 5-px gap - the
        // second half of `battle_chrome::name_plaque`'s pinned law.
        let name_w = font.layout_ascii(name).advance_x as i32;
        let badge = frame
            .plaque_badge
            .and_then(|i| frame.badges.and_then(|b| b.element_badge(i)));
        let lead = if badge.is_some() {
            PLAQUE_BADGE_W + PLAQUE_BADGE_GAP
        } else {
            0
        };
        plate_run(
            &mut text,
            &mut sprites,
            PLAQUE_X,
            PLAQUE_Y,
            lead + name_w,
            true,
        );
        let content_x = PLAQUE_X + PLATE_CAP_W;
        let content_y = PLAQUE_Y + PLAQUE_CONTENT_DY;
        if let Some(src) = badge {
            stage_sprite(&mut sprites, src, content_x, content_y);
        }
        stage_text(&mut text, font, name, content_x + lead, content_y, white);
    }

    // ---- Diagnostic rows (LEGAIA_DIAG_HUD) ----
    //
    // The engine's debug readout: compact per-slot rows at `pen` in raw
    // surface pixels - monster HP numerals + thin gauge bars (retail draws
    // no monster gauge at all), K.O. tags, the selected status element, and
    // a per-party LV / AP tail (the base-marker + level element retail only
    // shows on the input panel).
    if frame.diag {
        for (i, slot) in frame.slots.iter().enumerate() {
            if slot.name.is_empty() {
                continue;
            }
            let row_y = pen.1 + i as i32 * LINE_H;
            let base = if !slot.alive {
                dim
            } else if slot.is_party {
                white
            } else {
                monster
            };

            let name_layout = font.layout_ascii(slot.name);
            text.extend(text_draws_for(&name_layout, (pen.0, row_y), base));

            let hp_color = if !slot.alive {
                dim
            } else {
                tint(
                    hp_bar_color_index(slot.hp, slot.hp_max, slot.status_sprite != 0),
                    base,
                )
            };
            let hp_layout = font.layout_ascii(&format!("{}/{}", slot.hp, slot.hp_max));
            text.extend(text_draws_for(&hp_layout, (pen.0 + HP_X, row_y), hp_color));

            // Thin HP bar under the numerals: fill colour = the retail gauge
            // index (`FUN_80046A20`) through `gauge_fill_color`; the dead arm
            // (index 2) greys the whole track, not just the fill.
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
                text.push(TextDraw {
                    dst: (pen.0 + HP_X, row_y + 12, MBAR_W as u32, MBAR_H as u32),
                    src,
                    color: track,
                });
                let w = (MBAR_W as f32 * frac).round() as i32;
                let w = if slot.hp > 0 { w.max(1) } else { w };
                if w > 0 {
                    text.push(TextDraw {
                        dst: (pen.0 + HP_X, row_y + 12, w as u32, MBAR_H as u32),
                        src,
                        color: gauge_fill_color(slot.hp_fill),
                    });
                }
                // MP twin for slots that carry a ceiling.
                if slot.mp_max > 0 {
                    let mfrac = (slot.mp as f32 / slot.mp_max as f32).clamp(0.0, 1.0);
                    let mtrack = if slot.mp_fill == 2 {
                        gauge_fill_color(2)
                    } else {
                        bar_back
                    };
                    text.push(TextDraw {
                        dst: (
                            pen.0 + HP_X + MBAR_W + 4,
                            row_y + 12,
                            (MBAR_W / 2) as u32,
                            MBAR_H as u32,
                        ),
                        src,
                        color: mtrack,
                    });
                    let mw = ((MBAR_W / 2) as f32 * mfrac).round() as i32;
                    if mw > 0 {
                        text.push(TextDraw {
                            dst: (
                                pen.0 + HP_X + MBAR_W + 4,
                                row_y + 12,
                                mw as u32,
                                MBAR_H as u32,
                            ),
                            src,
                            color: gauge_fill_color(slot.mp_fill),
                        });
                    }
                }
            }

            if !slot.alive {
                let ko_layout = font.layout_ascii("K.O.");
                text.extend(text_draws_for(&ko_layout, (pen.0 + KO_X, row_y), red));
            }

            // The single retail-selected status element, as a letter tag.
            if slot.status_sprite != 0 {
                let layout = font.layout_ascii(status_element_label(slot.status_sprite));
                text.extend(text_draws_for(
                    &layout,
                    (pen.0 + STATUS_X, row_y),
                    status_element_color(slot.status_sprite),
                ));
            }

            // Party tail: the base-marker level (`FUN_8002C2E4`'s no-ailment
            // arm) + the AP gauge state.
            if slot.is_party {
                let mut tail = String::new();
                if slot.level > 0 {
                    tail.push_str(&format!("LV{}", slot.level));
                }
                if slot.ap_max > 0 {
                    if !tail.is_empty() {
                        tail.push(' ');
                    }
                    tail.push_str(&format!("AP{}/{}", slot.ap_filled, slot.ap_max));
                }
                if !tail.is_empty() {
                    let layout = font.layout_ascii(&tail);
                    text.extend(text_draws_for(&layout, (pen.0 + DIAG_TAIL_X, row_y), base));
                }
            }
        }
    }

    let log_x = pen.0;
    let log_y = pen.1 + frame.slots.len() as i32 * LINE_H + 4;
    for (i, line) in frame.log.iter().enumerate() {
        let layout = font.layout_ascii(line.text);
        text.extend(text_draws_for(
            &layout,
            (log_x, log_y + i as i32 * LINE_H),
            line.color,
        ));
    }

    // ---- Popups: the diagnostic seat only ----
    //
    // Retail's landed-hit numeral is not a HUD widget. It is a run of 24x24
    // cells off the battle effect atlas, thrown over the **struck actor** and
    // rising to a fixed screen row - laid out by
    // `engine-vm::battle_value_readout::value_cells` and drawn by the host,
    // which is the only layer that knows where the actor projects. Anchoring a
    // number to a party panel (or, for a monster, to the diagnostic pen in the
    // top-left corner) put every damage figure somewhere retail never draws
    // one, so that pass is now the diagnostic readout's and nothing else's.
    for popup in frame.popups.iter().filter(|_| frame.diag) {
        if (popup.slot as usize) >= frame.slots.len() {
            continue;
        }
        let (ax, ay) = popup_anchor[popup.slot as usize];
        let popup_color = match (popup.is_heal, popup.is_crit) {
            (true, _) => apply_alpha(green, popup.alpha),
            (_, true) => apply_alpha(yellow, popup.alpha),
            _ => apply_alpha(cyan, popup.alpha),
        };
        let s = if let Some(letter) = popup.status_letter {
            format!("[{}]", letter as char)
        } else if popup.is_heal {
            format!("+{}", popup.amount)
        } else {
            format!("-{}", popup.amount)
        };
        let layout = font.layout_ascii(&s);
        text.extend(text_draws_for(&layout, (ax, ay), popup_color));
    }

    BattleHudDraws { text, sprites }
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
pub const ENEMY_MENU_STAGE_Y: i32 = 166;

/// Row pitch a displaced enemy strip steps by ([`enemy_target_menu_rows_y`]).
/// One text row of the in-battle 14-px pitch.
const ENEMY_MENU_STEP: i32 = 14;

/// Stage Y the enemy target strip should draw at given the box a host is
/// drawing over the battle screen this frame (`BattleHudFrame::host_box`,
/// the same centre rect, `None` for "no box").
///
/// [`ENEMY_MENU_STAGE_Y`] is `166`, and retail's target-select tutorial hint
/// is style `5` - centred, bottom-anchored at `0xB0` - so a three-line hint
/// puts its own last row on **exactly** that Y. The two then draw on the same
/// pixels, which is what "Tetsu" reading through `...only one target.` is.
/// Retail's strip Y is caller-side and unpinned, so rather than guess a new
/// fixed seat the strip steps up in whole text rows until it clears the box's
/// drawn footprint (skin included). It never moves when no box is up, so a
/// frame without a prompt is unchanged.
pub fn enemy_target_menu_rows_y(host_box: Option<(i32, i32, i32, i32)>) -> i32 {
    let Some((_, by, _, bh)) = host_box else {
        return ENEMY_MENU_STAGE_Y;
    };
    let (box_top, box_bot) = (by - HOST_BOX_SKIN, by + bh + HOST_BOX_SKIN);
    let mut y = ENEMY_MENU_STAGE_Y;
    // A glyph row is at most the pitch tall, so "clear" means the row band
    // `[y, y + pitch)` misses `[box_top, box_bot)`.
    while y + ENEMY_MENU_STEP > box_top && y < box_bot && y > ENEMY_MENU_STEP {
        y -= ENEMY_MENU_STEP;
    }
    y
}

/// Build [`TextDraw`]s for the enemy target-selection name strip at a
/// caller-chosen stage row - the seat [`enemy_target_menu_rows_y`] picks,
/// which is [`ENEMY_MENU_STAGE_Y`] unless a host box shares the strip's row.
///
/// The row *content* is retail's: dedup labels from `FUN_801D9D3C` and the
/// centre/relax/clamp X layout from its second half
/// (`target_picker::layout_enemy_menu_rows`), both run by the caller. This
/// builder only projects the laid-out rows onto the surface: each label at
/// its stage X (integer-upscaled + centred, the same transform as the battle
/// HUD panels), the selected row in white behind a `>` cursor, the rest
/// dimmed.
///
/// There is deliberately no fixed-seat wrapper: both hosts share a row band
/// with a host-drawn prompt box, so every caller must pass the resolved seat
/// or it silently overprints the box.
pub fn enemy_target_menu_draws_at(
    font: &legaia_font::Font,
    rows: &[EnemyTargetRowView<'_>],
    surface: (u32, u32),
    stage_y: i32,
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
            (i32::from(row.x), stage_y),
            color,
        );
        if row.selected {
            draws.extend(text_draws_for(
                &font.layout_ascii(">"),
                (i32::from(row.x) - 9, stage_y),
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

/// One laid-out digit of a floating battle value readout.
///
/// Hosts fill these from `engine-vm::battle_value_readout::value_cells`, which
/// is where the geometry is pinned; this crate sits below `engine-vm` in the
/// crate graph, so the type is mirrored rather than imported. Fields are
/// **stage pixels** on the retail 320x240 stage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ValueCellView {
    /// The decimal digit the cell shows.
    pub digit: u8,
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

/// Colour the retail numeral art reads as - a hot gold, the top of the sheet's
/// own vertical ramp. Only the **fallback** uses it: retail's quads carry the
/// neutral colour word `0x808080` and take their colour from the texels, so a
/// host drawing the real cells must not tint them.
pub const VALUE_READOUT_FALLBACK_COLOR: [f32; 4] = [1.0, 0.78, 0.24, 1.0];

/// Fallback draw list for a floating value readout: the dialog font's digits
/// scaled into the pinned cells.
///
/// This is what a host without the battle effect atlas resident draws. The
/// **layout is retail's** - same cells, same pitch, same pop and rise - and
/// only the letterforms differ, the same bargain the HUD numerals already
/// strike. A host that can sample VRAM should draw the real 24x24 cells off
/// texture page `0x27` / CLUT `0x7703` instead and skip this entirely.
///
/// `origin` / `scale` are the stage-to-surface transform the caller already
/// uses for the rest of the battle chrome.
pub fn battle_value_readout_draws_for(
    font: &legaia_font::Font,
    cells: &[ValueCellView],
    color: [f32; 4],
    origin: (i32, i32),
    scale: u32,
) -> Vec<TextDraw> {
    let mut out = Vec::new();
    for c in cells {
        if c.w == 0 || c.h == 0 {
            continue;
        }
        let glyph = [b'0' + (c.digit % 10)];
        let s = match std::str::from_utf8(&glyph) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let layout = font.layout_ascii(s);
        for d in text_draws_for(&layout, (0, 0), color) {
            let (dx, dy, dw, dh) = d.dst;
            // Scale the glyph's own quad into the cell: the font atlas has no
            // 24-px digits, so each source rect is stretched rather than
            // blitted. The scale is taken from the glyph RECT (`dw` / `dh`),
            // not from the layout advance. The advance is the *proportional*
            // pen step - 5 px for `1`, 7 for `3` - while the atlas cell the
            // quad samples is a fixed 14x15, so dividing by it inflated every
            // digit by `rect / advance` (~2x-3x) and, because the advance is
            // per glyph, drew the digits of one number at DIFFERENT sizes.
            // Retail's readout is a run of uniform 24x24 cells, so the quad
            // is the cell.
            let sx = c.w as f32 / (dw.max(1) as f32);
            let sy = c.h as f32 / (dh.max(1) as f32);
            out.push(TextDraw {
                dst: (
                    origin.0 + (c.x * scale as i32) + (dx as f32 * sx) as i32 * scale as i32,
                    origin.1 + (c.y * scale as i32) + (dy as f32 * sy) as i32 * scale as i32,
                    ((dw as f32 * sx) as u32).max(1) * scale,
                    ((dh as f32 * sy) as u32).max(1) * scale,
                ),
                src: d.src,
                color: d.color,
            });
        }
    }
    out
}
