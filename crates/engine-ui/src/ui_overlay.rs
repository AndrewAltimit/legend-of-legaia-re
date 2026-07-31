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
        0x20 => "K.O.",
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
    /// Retail parks the party status plate off-screen while an arts input
    /// session owns the frame (`docs/subsystems/minigame-muscle-dome.md` -
    /// its draws move to `y = 230`, under the 228-line display window), and
    /// the arts-input reference frame shows the command bar in its place.
    /// Hosts set this while a command-entry session is up; the builder then
    /// emits no party strip.
    pub input_session_parked: bool,
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

/// Party-strip stage geometry. Three provenance grades, called out per
/// constant.
///
/// **Measured (capture):** the strip itself. The retail frame
/// `captures/tetsu_idle` is a native 320x228 framebuffer grab of a solo-Vahn
/// battle. Per-pixel it holds exactly one full-width lozenge spanning
/// `x 8..=311` by `y 188..=207`, 6-px chamfered caps, no gauge bar of any
/// kind, and one text row at `y 194`: the name at `x 16`, the gold `HP`
/// label sprite at `x 80`, `cur` right-aligned to `x 132`, `/` at `x 137`,
/// `max` right-aligned to `x 176`, then the green `MP` label at `x 192`
/// with its pair right-aligned to `x 236` / `x 272` around a `/` at
/// `x 241`. The `retail_Tetsu_rush` reference carries four-digit HP against
/// the same columns, which is what fixes the numerals as right-aligned
/// rather than left-run.
///
/// **Falsified:** `FUN_801D84C0`'s per-party-size anchors (solo `0x72`,
/// pair `0x3F`/`0xA5`, trio `0x0C`/`0x72`) and `FUN_801DBC30`'s `0x40`-wide
/// label-strip blit do **not** position this HUD. Retail's solo name sits at
/// `x 0x10`; the solo anchor would put a `0x40`-wide plate at `x 0x6A..0xA9`,
/// nowhere near it. [`party_panel_stage_x`] keeps the table so the mirror
/// test against `legaia_engine_vm::battle_party_panel::panel_anchors` stays
/// total, but the strip no longer reads it.
///
/// **Approximate:** the multi-member layout. Retail's party-row widget is
/// per-ordinal - `FUN_8002C69C` widget kinds `0x33` / `0x34` / `0x35` each
/// call `FUN_8002C2E4` with `a0 = 0 / 1 / 2` - and each kind rides its own
/// screen element, whose `(x, y)` comes from the element record
/// (`FUN_80031D00` loads `+0xE` / `+0x10` per element and stores `+0x1D`
/// into `gp+0x14C` before the dispatch). So retail positions each member's
/// row independently and no captured frame pins where. The engine stacks
/// one identical full-width strip per live member, bottom row on the pinned
/// band, which keeps every measured column exact for every member.
///
/// One engine-wide caveat rides all of these: retail's display window is 228
/// lines while [`BOOT_UI_STAGE_H`] is 240, so the strip's pinned `y 188`
/// leaves 32 stage lines under it here against retail's 20.
const STRIP_STAGE_X: i32 = 8;
/// Strip width (stage px) - captured span `x 8..=311`.
const STRIP_STAGE_W: i32 = 304;
/// Stage Y of the bottom-most strip - captured band `y 188..=207`.
const STRIP_STAGE_Y: i32 = 188;
/// Strip height (stage px) - captured band `y 188..=207`.
const STRIP_STAGE_H: i32 = 20;
/// Vertical pitch between stacked member strips (approximation - one strip
/// plus a 1-px gap; retail's per-element Y is not pinned).
const STRIP_ROW_PITCH: i32 = 21;
/// Text row offset from the strip top - captured glyph row `y 194`.
const STRIP_TEXT_DY: i32 = 6;

/// Strip columns (stage px), measured from `captures/tetsu_idle`.
const STRIP_NAME_X: i32 = 16;
const STRIP_HP_LABEL_X: i32 = 80;
const STRIP_HP_CUR_RIGHT: i32 = 132;
const STRIP_HP_SLASH_X: i32 = 137;
const STRIP_HP_MAX_RIGHT: i32 = 176;
const STRIP_MP_LABEL_X: i32 = 192;
const STRIP_MP_CUR_RIGHT: i32 = 236;
const STRIP_MP_SLASH_X: i32 = 241;
const STRIP_MP_MAX_RIGHT: i32 = 272;
/// Right-hand gutter the status badge rides (approximation - see the badge's
/// own comment in [`battle_hud_draws_for`]).
const STRIP_BADGE_X: i32 = 278;

/// Top-left plaque, measured from `captures/tetsu_idle`: the art box spans
/// `x 8..=50` by `y 8..=27` (the same 20-px chamfered lozenge as the strip)
/// with the name at `(16, 14)`, i.e. an `(8, 6)` inset and a 9-px right pad,
/// so the box width tracks the label. The plaque *skin* is not pinned to an
/// atlas cell, so the label draws as clean text at the measured inset.
const PLAQUE_TEXT_X: i32 = 16;
const PLAQUE_TEXT_Y: i32 = 14;
const PLAQUE_BOX_X: i32 = 8;
const PLAQUE_BOX_Y: i32 = 8;
const PLAQUE_BOX_H: i32 = 20;
/// Left inset + right pad the plaque box adds around its label.
const PLAQUE_PAD: i32 = 17;

/// Stage X of party section `ordinal` (0-based) for `count` live party
/// members - retail's `FUN_801D84C0` anchor table, mirrored here as literals
/// because `engine-ui` sits below `engine-vm` in the crate graph, and pinned
/// equal to `legaia_engine_vm::battle_party_panel::panel_anchors` by
/// `engine-shell`'s HUD tests. The battle strip does **not** read it - see
/// [`STRIP_STAGE_X`] for why that table was falsified as this HUD's source.
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

    // Right-aligned stage text - the captured numerals right-align against
    // fixed columns with a fixed slash between the fields, which is what
    // keeps a four-digit HP (the Tetsu-rush reference) in the same box as a
    // three-digit one.
    let stage_text_r = |out: &mut Vec<TextDraw>, s: &str, right: i32, y: i32, c: [f32; 4]| {
        let layout = font.layout_ascii(s);
        let x = right - layout.advance_x as i32;
        let mut draws = text_draws_for(&layout, (x, y), c);
        scale_stage_text_draws(&mut draws, origin, scale as u32);
        out.extend(draws);
    };
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
    // HP / MP label: the resident system-UI atlas sprites
    // (`OVERLAY_SYSTEM_UI_LABEL_HP` / `_MP`, `legaia_asset::title_pak`) - the
    // same two cells the pause menu's party panel draws, and the two the
    // retail strip draws, gold `HP` and green `MP` out of one CLUT row.
    // Without an atlas they degrade to tinted text at the same columns.
    let label =
        |sprites: &mut Vec<SpriteDraw>, text: &mut Vec<TextDraw>, is_hp: bool, x: i32, y: i32| {
            match frame.chrome {
                Some(rects) => {
                    let src = if is_hp {
                        rects.label_hp
                    } else {
                        rects.label_mp
                    };
                    stage_sprite(sprites, src, x, y);
                }
                None => {
                    let (s, c) = if is_hp {
                        ("HP", label_gold)
                    } else {
                        ("MP", label_green)
                    };
                    stage_text(text, font, s, x, y, c);
                }
            }
        };
    // The lozenge skin one strip / plaque draws. Retail's own cell is a
    // chamfered pill that is not pinned to an atlas rect, so with the
    // system-UI atlas this is the shared blue dialog gradient under the gold
    // 9-slice frame, and without it a solid-texel interior plus a 1-px rim.
    let lozenge =
        |text: &mut Vec<TextDraw>, sprites: &mut Vec<SpriteDraw>, rect: (i32, i32, i32, i32)| {
            match frame.chrome {
                Some(rects) => {
                    sprites.push(SpriteDraw {
                        dst: (
                            origin.0 + (rect.0 + 2) * scale,
                            origin.1 + (rect.1 + 2) * scale,
                            ((rect.2 - 4) * scale) as u32,
                            ((rect.3 - 4) * scale) as u32,
                        ),
                        src: rects.dialog_fill,
                        color: white,
                    });
                    nine_slice_border_into(sprites, rects, rect, origin, scale as u32);
                }
                None => {
                    stage_rect(text, rect.0, rect.1, rect.2, rect.3, strip_bg);
                    stage_rect(text, rect.0, rect.1, rect.2, 1, strip_frame);
                    stage_rect(text, rect.0, rect.1 + rect.3 - 1, rect.2, 1, strip_frame);
                    stage_rect(text, rect.0, rect.1, 1, rect.3, strip_frame);
                    stage_rect(text, rect.0 + rect.2 - 1, rect.1, 1, rect.3, strip_frame);
                }
            }
        };

    // Retail parks the status plate off-screen while an arts input session
    // owns the frame (`docs/subsystems/minigame-muscle-dome.md`: its draws
    // move to `y = 230`, under the 228-line display window), and the
    // arts-input reference frame shows the bottom of the screen carrying the
    // command bar instead. The port emits nothing rather than drawing at an
    // off-screen Y, since the engine stage is 12 lines taller than retail's
    // display window and `y = 230` would still be visible here.
    if !live_party.is_empty() && !frame.input_session_parked {
        let rows = live_party.len().min(3) as i32;
        for (ordinal, (i, slot)) in live_party.iter().take(3).enumerate() {
            // Bottom row on the pinned band, earlier members stacked above.
            let sy = STRIP_STAGE_Y - (rows - 1 - ordinal as i32) * STRIP_ROW_PITCH;
            lozenge(
                &mut text,
                &mut sprites,
                (STRIP_STAGE_X, sy, STRIP_STAGE_W, STRIP_STAGE_H),
            );

            let base = if slot.alive { white } else { dim };
            // Numerals take the retail readout-tint law; a dead member's
            // whole row dims (the readout law's own K.O. index).
            let hp_tint = if !slot.alive {
                dim
            } else {
                tint(
                    hp_bar_color_index(slot.hp, slot.hp_max, slot.status_sprite != 0),
                    white,
                )
            };
            let mp_tint = if !slot.alive {
                dim
            } else {
                tint(mp_bar_color_index(slot.mp, slot.mp_max), white)
            };

            let ty = sy + STRIP_TEXT_DY;
            stage_text(&mut text, font, slot.name, STRIP_NAME_X, ty, base);
            label(&mut sprites, &mut text, true, STRIP_HP_LABEL_X, ty);
            stage_text_r(
                &mut text,
                &slot.hp.to_string(),
                STRIP_HP_CUR_RIGHT,
                ty,
                hp_tint,
            );
            stage_text(&mut text, font, "/", STRIP_HP_SLASH_X, ty, hp_tint);
            stage_text_r(
                &mut text,
                &slot.hp_max.to_string(),
                STRIP_HP_MAX_RIGHT,
                ty,
                hp_tint,
            );
            if slot.mp_max > 0 {
                label(&mut sprites, &mut text, false, STRIP_MP_LABEL_X, ty);
                stage_text_r(
                    &mut text,
                    &slot.mp.to_string(),
                    STRIP_MP_CUR_RIGHT,
                    ty,
                    mp_tint,
                );
                stage_text(&mut text, font, "/", STRIP_MP_SLASH_X, ty, mp_tint);
                stage_text_r(
                    &mut text,
                    &slot.mp_max.to_string(),
                    STRIP_MP_MAX_RIGHT,
                    ty,
                    mp_tint,
                );
            }
            popup_anchor[*i] = (
                origin.0 + STRIP_HP_LABEL_X * scale,
                origin.1 + (sy - 26) * scale,
            );

            // Status badge in the member's own right-hand gutter - retail
            // draws exactly one element per party slot and which one is
            // `FUN_8002C2E4`'s ladder, already resolved into `status_sprite`
            // upstream. Only the ailment arm draws by default: the no-ailment
            // arm is the base marker sprite `0x0A` plus the `+0x130` level,
            // and neither retail reference frame shows a marker or a level
            // anywhere near the strip, so the level readout is
            // diagnostic-only. The badge rides the gutter between the MP
            // maximum's column and the right cap because that is the one part
            // of the row nothing else claims - retail's own element pen is
            // caller-supplied and unpinned, and a badge above the row lands
            // on the member stacked over it. Art + placement are engine
            // approximations; the selection is the ported part.
            if slot.status_sprite != 0 {
                let label_s = status_element_label(slot.status_sprite);
                let c = status_element_color(slot.status_sprite);
                stage_text(&mut text, font, label_s, STRIP_BADGE_X, ty, c);
            }
        }
    }

    // ---- Top-left plaque ----
    //
    // Retail's battle screen keeps a small framed plaque in the top-left
    // corner naming the actor the frame belongs to - "Vahn" while Vahn's
    // item lands (`captures/tetsu_idle`, `captures/item_catch`), the
    // monster's name through an enemy turn (the Tetsu-rush reference). Its
    // skin is a brown/gold lozenge that is not pinned to an atlas cell, so
    // the box draws in the shared chrome and the label as clean text at the
    // measured inset. This is also where the port's monster readout now
    // lives: retail draws no monster gauge at all
    // (`docs/subsystems/battle-action.md`), so the name is all that stays.
    if let Some(name) = frame.plaque
        && !name.is_empty()
    {
        let w = font.layout_ascii(name).advance_x as i32 + PLAQUE_PAD;
        lozenge(
            &mut text,
            &mut sprites,
            (PLAQUE_BOX_X, PLAQUE_BOX_Y, w, PLAQUE_BOX_H),
        );
        stage_text(&mut text, font, name, PLAQUE_TEXT_X, PLAQUE_TEXT_Y, white);
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
