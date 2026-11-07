//! Battle **item window** - retail's state-`0x3C` surface, shared by both
//! play hosts.
//!
//! Every window rect below is packet-pinned out of a PCSX-Redux battle
//! save state's libgpu ordering table (the `battle_item_window` /
//! `battle_item_window_cursor1` captures walked into the item window from
//! `cort_evolved_battle_first_menu` by
//! `scripts/pcsx-redux/autorun_battle_item_window_capture.lua`): a RAM
//! image *is* the frame's display list, so the windows are read out of the
//! queued packet words. See `docs/subsystems/battle.md` § battle item
//! window.
//!
//! ## What the pins say
//!
//! * The **item-list window** is a tile grid of the system-UI window skin
//!   (widget page `(896, 256)`, CLUT row 511 sub-palette 2) spanning
//!   x `166..=313`, y `28..=164` - the stage rect [`LIST_WINDOW`].
//! * The **description window** is the same skin at x `8..=167`,
//!   y `122..=164` ([`DESC_WINDOW`]); it shows the highlighted item's
//!   info-window line ("Recover 200HP. Ally.").
//! * The **hand cursor** is the 16x16 pointing-finger sprite (CLUT row
//!   511 sub-palette 7 - the save-select / dialog hand) at
//!   `(167, 45 + 14*row)`: seat [`HAND_SEAT`], and the two captures one
//!   Down press apart pin the **row pitch** at 14 ([`ROW_PITCH`]).
//! * Eight rows fit a page ([`ROWS_PER_PAGE`]); the header is the list
//!   kernel's `PAGE` sprites and digits ([`LIST_HEADER_RECT`]).
//! * Counts are the kernel's three-cell number field ([`COUNT_CELLS_X`]).
//! * The **breadcrumb trail** of gold tab plates top-left (`Begin`, the
//!   acting member's name, `Item`) is not this module's: it is the HUD's
//!   trail - placement records 1, `0x1A` and `0x0C` - which the battle HUD
//!   builder draws for every command-entry window.
//!
//! The content pens follow the kernel: rows and hand on one `row_y`, the
//! name at `WX + 0xC`, the description on placement record `0x17`'s pen.
//!
//! The port draws the window with the shared 9-slice menu-window chrome
//! ([`crate::menu_window_chrome_draws_for`]) rather than re-tiling the
//! 32x32 grid: both compose the same system-UI window skin, and the
//! 9-slice is what every other faithful menu panel already goes through.
//!
//! During target select (retail state `0x64`) the item windows CLOSE and
//! the surface becomes the **target strip** - packet-pinned from the
//! `battle_item_target` / `battle_item_target_cursor1` captures
//! (`scripts/pcsx-redux/autorun_battle_item_target_capture.lua`, same
//! scenario): one full-width status bar at the screen's foot showing the
//! pointed-at member's name + HP + MP ([`TARGET_STRIP_WINDOW`] and the
//! `TARGET_*` pens below), the breadcrumb trail's third tab swapping from
//! `Item` to the **selected item's name**, and no list/description window
//! or hand cursor at all. The party roster never appears as a column -
//! retail's `0x64` cursor is the strip itself stepping across the party
//! band (LEFT/RIGHT), with the 3-member HUD parked offscreen (the capture
//! shows its digit rows at y `234..264`).
//!
//! **The strip is the party bar.** Every pinned pen of it - the plate at
//! `(8, 188)` closing at 312, the name at `(16, 192)`, the gold `HP` / `MP`
//! label widgets (`#0x07` / `#0x08`) at `(80, 194)` / `(192, 194)`, the
//! current-value runs ending at 134 / 238 - is the seat the ring's
//! full-width readout draws at (`ui_overlay`'s `BAR_*`), and the sub-draw
//! step retail runs into `0x64` opens placement record 7 (`07/0`, the same
//! record the ring's step 1 opens) for the member the cursor names. So this
//! module draws **nothing** at target select; the strip
//! itself is `battle_hud_draws_for`'s bar, fed by
//! `engine-battle-hud::battle_readout_bar_slot`
//! (`CommandSurface::ItemTarget`), which is what keeps the blue plate and
//! the sprite label widgets - not a menu-window skin and text stand-ins -
//! on the row. The `TARGET_*` constants stay as the capture's record, held
//! equal to the bar's seats by test.
//!
//! Two retail pieces of the same state are NOT this module's and stay
//! disclosed: the floating name tag beside the targeted actor (a
//! world-anchored plate - needs the target's projected position, which the
//! item-menu surface does not carry) and the camera refocus onto the
//! target. Both are visible in the captures.

use crate::{SaveMenuAtlasRects, SpriteDraw, TextDraw, text_draws_for};

// ------------------------------------------------------------- pinned rects

/// Item-list window stage rect `(x, y, w, h)` - packet-pinned.
pub const LIST_WINDOW: (i32, i32, i32, i32) = (166, 28, 147, 136);
/// Description window stage rect - packet-pinned.
pub const DESC_WINDOW: (i32, i32, i32, i32) = (8, 122, 159, 42);
/// The list kernel's content pen `WX`: placement record `0x16` (the item
/// list, `FUN_801D8DE8`'s table at `0x80076C10`) rests its content box at
/// `(174, 38)`, `131 x 120`.
pub const LIST_WX: i32 = 174;
/// Hand-cursor seat for row 0 (16x16 sprite top-left). The kind-4 list
/// kernel draws the hand `FUN_8002B994(0, browsing, WX - 6, row_y)`
/// (`0x80032FB4`) on the row's own `row_y`, and the bob table `0x80073D78`
/// adds `+2` in x for half of each 32-vsync period - the pause lists'
/// `list_hand_pos` rule. The `y` is the packet-pinned row-0 seat (the
/// `battle_item_window` capture holds the hand at `y = 45`; its `x = 167`
/// is one bob step, `-1`, of the same `WX - 6`).
pub const HAND_SEAT: (i32, i32) = (LIST_WX - 6 + 2, 45);
/// List row pitch - packet-pinned (the hand moves 14 per Down press).
pub const ROW_PITCH: i32 = 14;
/// Rows one page shows (screenshot-read: eight rows fill the window).
pub const ROWS_PER_PAGE: usize = 8;
/// Row text pen for row 0: the kernel's name pen `WX + 0xC` on the same
/// `row_y` it hands the hand cursor ([`HAND_SEAT`]). A pen's glyph ink
/// stands two rows under it, so the row-0 name reads from `y = 47`
/// (`battle_item_window_gaza2_retail`, `battle_magic_window_gaza2_retail`).
pub const ROW_TEXT_SEAT: (i32, i32) = (LIST_WX + 0xC, HAND_SEAT.1);
/// The per-row count's three-cell number field, `WX + 0x6C` off the list
/// kernel's pen `WX = 174` (`gaza2` item window capture).
pub const COUNT_CELLS_X: i32 = 282;
/// The list header's `(x, y, w)` for [`crate::shop_screen::page_header_draws`]
/// (`battle_item_window_gaza2_retail`: tag at x 249, y 35; digits at 279, 293, 299).
pub const LIST_HEADER_RECT: (i32, i32, i32) = (174, 37, 131);
/// The next-page triangle's seat (same capture).
pub const PAGE_NEXT_SEAT: (i32, i32) = (308, 94);
/// The previous-page triangle's seat: the kernel's `WX - 0xC` off the same list.
pub const PAGE_PREV_SEAT: (i32, i32) = (162, 94);
/// Description line pen: placement record `0x17` (the description box)
/// rests at `(16, 132)`, and the record walker hands the spawner
/// `y - 2` (`0x801D9368`), the plate records' `pen = (x, y - 2)` law. Its
/// glyph ink reads from `y = 132` on the item and magic captures.
pub const DESC_SEAT: (i32, i32) = (16, 130);
// ----------------------------------------------------------------- palette

/// Row ink for an enabled row: the kind-4 list kernel stages ink 7
/// (`0x8003312C..0x80033154`), the body-text white - `(206, 206, 206)` on
/// the `gaza2` item and magic captures.
pub const ROW_IDLE: [f32; 4] = crate::MENU_TEXT_WHITE;
/// Row ink for a row whose entry carries the disabled bit `0x800`: the
/// kernel drops the staging to ink 0, the menu grey.
pub const ROW_DIMMED: [f32; 4] = crate::MENU_TEXT_GREY;
/// Description ink: the info line draws in the same ink 7 as the rows
/// (`(206, 206, 206)` on both captures).
pub const DESC_INK: [f32; 4] = crate::MENU_TEXT_WHITE;
/// The `PAGE` label's green (retail draws it in the teal header ink).
pub const PAGE_LABEL_INK: [f32; 4] = [0.45, 0.95, 0.75, 1.0];
/// Ink of a dead target's name in the target strip.
pub const ROW_DEAD: [f32; 4] = [1.0, 0.55, 0.55, 1.0];

// ------------------------------------------------- target strip (state 0x64)
//
// The capture's record of the strip. Every seat is the party bar's
// (placement record 7, `ui_overlay`'s `BAR_*` - the pill the HUD builder
// draws); the test `target_strip_pins_are_the_party_bars_seats` holds the
// two equal. Nothing in this module draws from them.

/// Target-strip window stage rect - packet-pinned (left cap at x 8, body
/// tiles to the right cap at 304, one 20-px row at y 188).
pub const TARGET_STRIP_WINDOW: (i32, i32, i32, i32) = (8, 188, 304, 20);
/// Target name pen - packet-pinned (glyph run starts at (16, 192)).
pub const TARGET_NAME_SEAT: (i32, i32) = (16, 192);
/// `HP` label pen - packet-pinned (widget `#0x07` at (80, 194)).
pub const TARGET_HP_LABEL_SEAT: (i32, i32) = (80, 194);
/// Right edge the current-HP numerals end at - packet-pinned (the 8-px
/// digit run `102..134`; single capture, so right-alignment of shorter
/// values follows the HUD's numeral convention).
pub const TARGET_HP_CUR_RIGHT: i32 = 134;
/// Max-HP numeral pen (digits start at 146; the `/` sits in the
/// `134..146` gap) - packet-pinned.
pub const TARGET_HP_MAX_SEAT: (i32, i32) = (146, 192);
/// `MP` label pen - packet-pinned (widget `#0x08` at (192, 194)).
pub const TARGET_MP_LABEL_SEAT: (i32, i32) = (192, 194);
/// Right edge the current-MP numerals end at (digit run `214..238`).
pub const TARGET_MP_CUR_RIGHT: i32 = 238;
/// Max-MP numeral pen (digits start at 250) - packet-pinned.
pub const TARGET_MP_MAX_SEAT: (i32, i32) = (250, 192);
/// Numeral row baseline y (the 8x12 digit sprites sit at y 192).
pub const TARGET_VALUE_Y: i32 = 192;
/// The gold of the `HP` / `MP` label widgets the strip carries (the bar
/// draws the sprite widgets themselves).
pub const TARGET_LABEL_INK: [f32; 4] = [1.0, 0.85, 0.35, 1.0];

// ------------------------------------------------------------------- views

/// One list row of the frame.
#[derive(Debug, Clone, Copy)]
pub struct BattleItemRowView<'a> {
    pub name: &'a str,
    pub count: u8,
    /// `false` draws the row dimmed (context filter refuses it).
    pub admissible: bool,
}

/// One target row of the target-select panel.
#[derive(Debug, Clone, Copy, Default)]
pub struct BattleItemTargetView<'a> {
    pub name: &'a str,
    pub hp: u16,
    pub hp_max: u16,
    pub mp: u16,
    pub mp_max: u16,
    pub alive: bool,
}

/// Everything one frame of the battle item window draws from.
#[derive(Debug, Clone, Copy, Default)]
pub struct BattleItemMenuFrame<'a> {
    pub rows: &'a [BattleItemRowView<'a>],
    /// Row index (into `rows`) under the item cursor, if any.
    pub cursor: Option<usize>,
    /// The highlighted item's info-window description line, if resolved.
    pub description: Option<&'a str>,
    /// Acting party member's display name (the middle breadcrumb).
    pub actor_name: &'a str,
    /// Target-select: when `Some`, the list column swaps to these rows
    /// with the hand on `.1`.
    pub targets: Option<(&'a [BattleItemTargetView<'a>], usize)>,
}

impl BattleItemMenuFrame<'_> {
    /// The third breadcrumb's label: `Item` while browsing, the selected
    /// item's name during target select (the retail state-0x64 trail reads
    /// `Begin | <actor> | <item name>` - packet capture).
    pub fn third_crumb(&self) -> &str {
        if self.targets.is_some()
            && let Some(row) = self.cursor.and_then(|c| self.rows.get(c))
        {
            return row.name;
        }
        "Item"
    }

    /// The page the cursor sits on (0-based).
    pub fn page(&self) -> usize {
        self.cursor.unwrap_or(0) / ROWS_PER_PAGE
    }

    /// Total pages the row list spans (minimum 1, like retail's header).
    pub fn pages(&self) -> usize {
        self.rows.len().div_ceil(ROWS_PER_PAGE).max(1)
    }
}

// ---------------------------------------------------------------- builders

fn scaled(
    dst: (i32, i32, u32, u32),
    src: (u32, u32, u32, u32),
    color: [f32; 4],
    stage_origin: (i32, i32),
    scale: i32,
) -> SpriteDraw {
    SpriteDraw {
        dst: (
            stage_origin.0 + dst.0 * scale,
            stage_origin.1 + dst.1 * scale,
            dst.2 * scale as u32,
            dst.3 * scale as u32,
        ),
        src,
        color,
    }
}

/// The window's sprite half: both 9-slice windows and the hand cursor. The
/// breadcrumb trail above them is the HUD's (records 1, `0x1A` and `0x0C`,
/// `engine-battle-hud::battle_breadcrumb_third_tab`).
pub fn battle_item_window_sprites(
    rects: &SaveMenuAtlasRects,
    frame: &BattleItemMenuFrame<'_>,
    stage_origin: (i32, i32),
    stage_scale: u32,
) -> Vec<SpriteDraw> {
    let scale = stage_scale.max(1) as i32;
    let mut out = Vec::with_capacity(64);
    // State 0x64: the item windows close and the target strip at the
    // screen's foot is the party bar (placement record 7), which the HUD
    // builder draws for the pointed-at member - nothing of it is this
    // window's (module doc).
    if frame.targets.is_none() {
        out.extend(crate::menu_window_chrome_draws_for(
            rects,
            LIST_WINDOW,
            stage_origin,
            stage_scale,
        ));
        out.extend(crate::menu_window_chrome_draws_for(
            rects,
            DESC_WINDOW,
            stage_origin,
            stage_scale,
        ));
    }
    // Hand cursor on the highlighted item row (browsing only - state 0x64
    // has no hand, the strip itself is the cursor).
    if frame.targets.is_none()
        && let Some(row) = frame.cursor.map(|c| c % ROWS_PER_PAGE)
    {
        let (_, _, w, h) = rects.cursor;
        out.push(scaled(
            (HAND_SEAT.0, HAND_SEAT.1 + row as i32 * ROW_PITCH, w, h),
            rects.cursor,
            [1.0, 1.0, 1.0, 1.0],
            stage_origin,
            scale,
        ));
    }
    out
}

fn emit(
    out: &mut Vec<TextDraw>,
    font: &legaia_font::Font,
    text: &str,
    pen: (i32, i32),
    ink: [f32; 4],
    stage_origin: (i32, i32),
    scale: i32,
) {
    emit_layout(out, &font.layout_ascii(text), pen, ink, stage_origin, scale);
}

/// [`emit`] over an already laid-out byte string (escape sprites included).
fn emit_layout(
    out: &mut Vec<TextDraw>,
    layout: &legaia_font::Layout,
    pen: (i32, i32),
    ink: [f32; 4],
    stage_origin: (i32, i32),
    scale: i32,
) {
    let mut draws = text_draws_for(layout, (0, 0), ink);
    for d in &mut draws {
        d.dst.0 = stage_origin.0 + (pen.0 + d.dst.0) * scale;
        d.dst.1 = stage_origin.1 + (pen.1 + d.dst.1) * scale;
        d.dst.2 *= scale as u32;
        d.dst.3 *= scale as u32;
    }
    out.extend(draws);
}

/// Map draws from stage pixels to the surface.
fn to_stage(draws: &mut [TextDraw], stage_origin: (i32, i32), scale: i32) {
    for d in draws {
        d.dst.0 = stage_origin.0 + d.dst.0 * scale;
        d.dst.1 = stage_origin.1 + d.dst.1 * scale;
        d.dst.2 *= scale as u32;
        d.dst.3 *= scale as u32;
    }
}

/// The list kernel's header on the battle windows: the `PAGE` tag and page
/// digits (UI-icon sprites, the shop's), and the two page triangles while the
/// frame word's blink is up (`0x80032FC0`).
///
/// PORT: FUN_80032A44 (header and page triangles)
fn list_header_draws(
    out: &mut Vec<TextDraw>,
    font: &legaia_font::Font,
    (page, pages): (usize, usize),
    page_arrows: bool,
    stage_origin: (i32, i32),
    scale: i32,
) {
    let (x, y, w) = LIST_HEADER_RECT;
    let mut draws = crate::shop_screen::page_header_draws(font, (x, y, w), page, pages);
    if page_arrows {
        let mut tri = |id: u16, at: (i32, i32)| {
            if let Some(c) = font.sprite_cell(id) {
                draws.push(TextDraw {
                    dst: (at.0, at.1, c.w, c.h),
                    src: (c.atlas_x, c.atlas_y, c.w, c.h),
                    color: [1.0, 1.0, 1.0, 1.0],
                });
            }
        };
        if page < pages {
            tri(legaia_font::sprite_cell_ids::PAGE_NEXT, PAGE_NEXT_SEAT);
        }
        if page > 1 {
            tri(legaia_font::sprite_cell_ids::PAGE_PREV, PAGE_PREV_SEAT);
        }
    }
    to_stage(&mut draws, stage_origin, scale);
    out.append(&mut draws);
}

/// The window's text half: the `PAGE n/m` header, the
/// visible rows (name + right-aligned count, or the target roster during
/// target select) and the description line.
pub fn battle_item_window_text(
    font: &legaia_font::Font,
    frame: &BattleItemMenuFrame<'_>,
    page_arrows: bool,
    stage_origin: (i32, i32),
    stage_scale: u32,
) -> Vec<TextDraw> {
    let scale = stage_scale.max(1) as i32;
    let mut out = Vec::new();

    match frame.targets {
        None => {
            list_header_draws(
                &mut out,
                font,
                (frame.page() + 1, frame.pages()),
                page_arrows,
                stage_origin,
                scale,
            );
            // The cursor's page of rows.
            let base = frame.page() * ROWS_PER_PAGE;
            for (i, row) in frame.rows.iter().skip(base).take(ROWS_PER_PAGE).enumerate() {
                let y = ROW_TEXT_SEAT.1 + i as i32 * ROW_PITCH;
                // The kernel inks a row by its enable bit alone; the hand is
                // what marks the row under the cursor.
                let ink = if row.admissible { ROW_IDLE } else { ROW_DIMMED };
                emit(
                    &mut out,
                    font,
                    row.name,
                    (ROW_TEXT_SEAT.0, y),
                    ink,
                    stage_origin,
                    scale,
                );
                let mut cells =
                    crate::num_field_draws(font, u64::from(row.count), COUNT_CELLS_X, y, 3, ink);
                to_stage(&mut cells, stage_origin, scale);
                out.append(&mut cells);
            }
        }
        Some(_) => {
            // State-0x64 target strip: the pointed-at member's name + HP +
            // MP on the party bar's pens - drawn by the HUD builder as
            // placement record 7 (`battle_readout_bar_slot`), not here.
            // The strip IS the cursor: stepping it re-points the bar.
        }
    }

    // The description line lives in the description window, which closes
    // with the list at target select (state 0x64 carries neither).
    if frame.targets.is_none()
        && let Some(desc) = frame.description
    {
        emit(
            &mut out,
            font,
            desc,
            DESC_SEAT,
            DESC_INK,
            stage_origin,
            scale,
        );
    }
    out
}

// ------------------------------------------------------------ spell window

/// Pen the spell rows' leading element icon sits at - the item rows' own
/// text pen (screenshot-read off the `0x46` capture: the icon cells start
/// under the same hand seat as the item rows).
pub const SPELL_ICON_PEN_X: i32 = ROW_TEXT_SEAT.0;
/// Name advance past a regular spell's element icon (screenshot-read: the
/// names start at x 211).
pub const SPELL_NAME_ADVANCE: i32 = 25;
/// Name advance past the winged Ra-Seru-magic icon, which is the wider of
/// the two (screenshot-read: `Meta` starts at x 218).
pub const SPELL_NAME_ADVANCE_RA_SERU: i32 = 32;
/// Second description line, one row pitch under [`DESC_SEAT`]
/// (screenshot-read: `Attack all enemies.` sits at y 146).
pub const DESC_LINE_PITCH: i32 = 14;
/// The `Lv` tab plate's glyph pen and interior width (screenshot-read: the
/// gold plate spans x `188..=247`, y `8..=28`).
pub const SPELL_LV_TAB: ((i32, i32), i32) = ((196, 12), 43);
/// The `MP` tab plate's glyph pen and interior width (plate x `250..=313`).
pub const SPELL_MP_TAB: ((i32, i32), i32) = ((258, 12), 47);

/// One row of the battle magic window.
#[derive(Debug, Clone, Copy)]
pub struct BattleSpellRowView<'a> {
    pub name: &'a str,
    /// `false` greys the row (the caster's MP does not cover it).
    pub admissible: bool,
    /// The row leads with the winged Ra-Seru-magic icon.
    pub ra_seru: bool,
    /// Operand of the `0xCE` element-plate escape the retail name string
    /// opens with; `None` without the disc spell table.
    pub icon: Option<u8>,
}

/// Everything one frame of the battle magic window (`0x46`) draws from.
#[derive(Debug, Clone, Copy, Default)]
pub struct BattleSpellMenuFrame<'a> {
    pub rows: &'a [BattleSpellRowView<'a>],
    pub cursor: Option<usize>,
    /// The highlighted spell's info lines (spell name, then its scope).
    pub description: &'a [&'a str],
    /// The highlighted spell's level and MP cost - the two gold tabs at the
    /// top right.
    pub level_mp: Option<(u8, u16)>,
}

impl BattleSpellMenuFrame<'_> {
    /// The page the cursor sits on (0-based).
    pub fn page(&self) -> usize {
        self.cursor.unwrap_or(0) / ROWS_PER_PAGE
    }

    /// Total pages the row list spans (minimum 1).
    pub fn pages(&self) -> usize {
        self.rows.len().div_ceil(ROWS_PER_PAGE).max(1)
    }
}

/// The magic window's sprite half: the list and description windows (the
/// item window's own two rects, which the capture shows unchanged), the
/// `Lv` / `MP` tab plates and the hand.
///
/// The breadcrumb trail is not drawn here: in the magic window it is the
/// HUD's (`Begin | <name> | <Ra-Seru>`, record `0x0E`).
pub fn battle_spell_window_sprites(
    rects: &SaveMenuAtlasRects,
    frame: &BattleSpellMenuFrame<'_>,
    stage_origin: (i32, i32),
    stage_scale: u32,
) -> Vec<SpriteDraw> {
    let scale = stage_scale.max(1) as i32;
    let mut out = Vec::with_capacity(64);
    for rect in [LIST_WINDOW, DESC_WINDOW] {
        out.extend(crate::menu_window_chrome_draws_for(
            rects,
            rect,
            stage_origin,
            stage_scale,
        ));
    }
    if frame.level_mp.is_some() {
        for (pen, interior) in [SPELL_LV_TAB, SPELL_MP_TAB] {
            out.extend(crate::tab_banner_draws(
                rects,
                pen,
                interior,
                stage_origin,
                stage_scale,
            ));
        }
    }
    if let Some(row) = frame.cursor.map(|c| c % ROWS_PER_PAGE) {
        let (_, _, w, h) = rects.cursor;
        out.push(scaled(
            (HAND_SEAT.0, HAND_SEAT.1 + row as i32 * ROW_PITCH, w, h),
            rects.cursor,
            [1.0, 1.0, 1.0, 1.0],
            stage_origin,
            scale,
        ));
    }
    out
}

/// The magic window's text half: `PAGE n/m`, the cursor's page of rows
/// (each led by its element plate when the font carries the escape
/// sprites, else the name past the plate's advance),
/// the description lines and the `Lv` / `MP` tab labels.
pub fn battle_spell_window_text(
    font: &legaia_font::Font,
    frame: &BattleSpellMenuFrame<'_>,
    page_arrows: bool,
    stage_origin: (i32, i32),
    stage_scale: u32,
) -> Vec<TextDraw> {
    let scale = stage_scale.max(1) as i32;
    let mut out = Vec::new();
    list_header_draws(
        &mut out,
        font,
        (frame.page() + 1, frame.pages()),
        page_arrows,
        stage_origin,
        scale,
    );
    let base = frame.page() * ROWS_PER_PAGE;
    for (i, row) in frame.rows.iter().skip(base).take(ROWS_PER_PAGE).enumerate() {
        let y = ROW_TEXT_SEAT.1 + i as i32 * ROW_PITCH;
        let ink = if row.admissible { ROW_IDLE } else { ROW_DIMMED };
        // Retail draws the raw name string, so its leading `0xCE op 0x20`
        // draws the plate at the row pen and the name one table width plus
        // a space on - with the escape sprites attached, lay it out whole.
        if let Some(op) = row.icon.filter(|op| font.escape_cell(*op).is_some()) {
            let mut bytes = vec![0xCE, op, b' '];
            bytes.extend(row.name.bytes());
            emit_layout(
                &mut out,
                &font.layout(&bytes),
                (SPELL_ICON_PEN_X, y),
                ink,
                stage_origin,
                scale,
            );
            continue;
        }
        let advance = if row.ra_seru {
            SPELL_NAME_ADVANCE_RA_SERU
        } else {
            SPELL_NAME_ADVANCE
        };
        emit(
            &mut out,
            font,
            row.name,
            (SPELL_ICON_PEN_X + advance, y),
            ink,
            stage_origin,
            scale,
        );
    }
    for (i, line) in frame.description.iter().enumerate() {
        let pen = (DESC_SEAT.0, DESC_SEAT.1 + i as i32 * DESC_LINE_PITCH);
        emit(&mut out, font, line, pen, DESC_INK, stage_origin, scale);
    }
    if let Some((lv, mp)) = frame.level_mp {
        for (((pen, interior), label), value) in [SPELL_LV_TAB, SPELL_MP_TAB]
            .into_iter()
            .zip(["Lv", "MP"])
            .zip([lv.to_string(), mp.to_string()])
        {
            emit(
                &mut out,
                font,
                label,
                pen,
                crate::READOUT_NORMAL,
                stage_origin,
                scale,
            );
            let vw = font.layout_ascii(&value).advance_x as i32;
            emit(
                &mut out,
                font,
                &value,
                (pen.0 + interior - vw, pen.1),
                crate::READOUT_NORMAL,
                stage_origin,
                scale,
            );
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal atlas fixture: the 9-slice tiles at their real sizes (the
    /// pinned system-UI skin geometry) so the window composer runs its
    /// genuine tiling arithmetic, plus the hand-cursor cell.
    fn rects() -> SaveMenuAtlasRects {
        SaveMenuAtlasRects {
            panel_tl: (0, 0, 4, 4),
            panel_tr: (28, 0, 4, 4),
            panel_bl: (0, 25, 4, 4),
            panel_br: (28, 25, 4, 4),
            panel_top: (4, 0, 24, 4),
            panel_bot: (4, 25, 24, 4),
            panel_left: (0, 4, 4, 21),
            panel_right: (28, 4, 4, 21),
            panel_interior: (32, 0, 32, 29),
            panel_filigree: (64, 0, 32, 29),
            cursor: (152, 64, 16, 16),
            tab_cap_l: (96, 0, 8, 20),
            tab_body: (104, 0, 16, 20),
            tab_cap_r: (120, 0, 8, 20),
            ..Default::default()
        }
    }

    fn rows(n: usize) -> Vec<BattleItemRowView<'static>> {
        const NAMES: [&str; 4] = ["Healing Leaf", "Healing Flower", "Antidote", "Door of Wind"];
        (0..n)
            .map(|i| BattleItemRowView {
                name: NAMES[i % NAMES.len()],
                count: (i + 1) as u8,
                admissible: true,
            })
            .collect()
    }

    fn frame<'a>(rows: &'a [BattleItemRowView<'a>], cursor: usize) -> BattleItemMenuFrame<'a> {
        BattleItemMenuFrame {
            rows,
            cursor: Some(cursor),
            description: Some("Recover 200HP. Ally."),
            actor_name: "Vahn",
            targets: None,
        }
    }

    /// The frame prims exist: both packet-pinned windows are composed as
    /// full 9-slice panels (fill + 4 corners + edges), at the pinned rects.
    #[test]
    fn both_windows_draw_their_chrome_at_the_pinned_rects() {
        let r = rects();
        let all = rows(3);
        let sprites = battle_item_window_sprites(&r, &frame(&all, 0), (0, 0), 1);
        // A 9-slice panel is at least 9 sprites; two windows + the hand.
        let list_only = crate::menu_window_chrome_draws_for(&r, LIST_WINDOW, (0, 0), 1);
        let desc_only = crate::menu_window_chrome_draws_for(&r, DESC_WINDOW, (0, 0), 1);
        assert!(list_only.len() >= 9, "list window is a real 9-slice");
        assert!(desc_only.len() >= 9, "desc window is a real 9-slice");
        // The composed frame contains both panels verbatim, in order.
        let key = |s: &SpriteDraw| (s.dst, s.src);
        assert_eq!(
            sprites[..list_only.len()]
                .iter()
                .map(key)
                .collect::<Vec<_>>(),
            list_only.iter().map(key).collect::<Vec<_>>()
        );
        assert_eq!(
            sprites[list_only.len()..list_only.len() + desc_only.len()]
                .iter()
                .map(key)
                .collect::<Vec<_>>(),
            desc_only.iter().map(key).collect::<Vec<_>>()
        );
        // The pinned rects themselves - x 166..=313 / y 28..=164 and
        // x 8..=167 / y 122..=164, the packet grid's spans.
        assert_eq!(LIST_WINDOW, (166, 28, 147, 136));
        assert_eq!(DESC_WINDOW, (8, 122, 159, 42));
        // Every sprite of the frame stays on the 320x240 stage.
        for s in &sprites {
            assert!(s.dst.0 >= 0 && s.dst.0 + s.dst.2 as i32 <= 320, "{s:?}");
            assert!(s.dst.1 >= 0 && s.dst.1 + s.dst.3 as i32 <= 240, "{s:?}");
        }
    }

    /// The hand cursor rides the kernel's `WX - 6` plus the bob's `+2`, on
    /// the packet-pinned row-0 `y = 45` and pitch: one Down press = 14
    /// further, exactly the two captures.
    #[test]
    fn the_hand_cursor_sits_on_the_pinned_seat_and_pitch() {
        let r = rects();
        let all = rows(3);
        let hand_at = |cursor: usize| {
            let sprites = battle_item_window_sprites(&r, &frame(&all, cursor), (0, 0), 1);
            let hand = sprites.last().unwrap();
            assert_eq!(hand.src, r.cursor);
            (hand.dst.0, hand.dst.1)
        };
        assert_eq!(hand_at(0), (170, 45));
        assert_eq!(hand_at(1), (170, 59));
        assert_eq!(hand_at(2), (170, 73));
    }

    /// Rows page by eight and the header names the cursor's page.
    #[test]
    fn rows_page_by_eight_and_the_header_counts_pages() {
        let all = rows(11);
        let f0 = frame(&all, 0);
        assert_eq!((f0.page(), f0.pages()), (0, 2));
        let f9 = frame(&all, 9);
        assert_eq!((f9.page(), f9.pages()), (1, 2));
        // Page 2 draws the remainder (3 rows), and the hand wraps onto the
        // page-relative seat.
        let r = rects();
        let sprites = battle_item_window_sprites(&r, &f9, (0, 0), 1);
        let hand = sprites.last().unwrap();
        assert_eq!(hand.dst.1, 45 + ROW_PITCH); // row 9 = page row 1
        // An empty bag still has one page, like retail's PAGE 1/1 header.
        let empty = BattleItemMenuFrame {
            rows: &[],
            cursor: None,
            description: None,
            actor_name: "Vahn",
            targets: None,
        };
        assert_eq!(empty.pages(), 1);
    }

    /// Text half: rows land on the pinned pens, counts right-align to the
    /// count column, and the description line lands in the desc window.
    #[test]
    fn text_rows_land_on_the_pinned_pens() {
        let font = legaia_font::Font::placeholder();
        let all = rows(2);
        let draws = battle_item_window_text(&font, &frame(&all, 0), true, (0, 0), 1);
        assert!(!draws.is_empty());
        // Something draws on row 0's baseline and row 1's baseline.
        for row in 0..2i32 {
            let y = ROW_TEXT_SEAT.1 + row * ROW_PITCH;
            assert!(
                draws.iter().any(|d| d.dst.1 == y && d.dst.0 >= 0),
                "row {row} has no glyphs at y={y}"
            );
        }
        // Row 0's count "1" sits in the last of the field's three cells.
        assert!(
            draws
                .iter()
                .any(|d| d.dst.0 >= COUNT_CELLS_X + 16 && d.dst.1 == ROW_TEXT_SEAT.1),
            "count is right-aligned in its cells from x={COUNT_CELLS_X}"
        );
        // Description line at its pen.
        assert!(draws.iter().any(|d| d.dst.1 == DESC_SEAT.1));
        // The breadcrumb trail is the HUD's: nothing above the list window.
        assert!(!draws.iter().any(|d| d.dst.1 < LIST_WINDOW.1));
    }

    /// Target select is the packet-pinned state-0x64 surface: the item
    /// windows close, the hand goes, the third breadcrumb becomes the
    /// selected item's name - and the target strip at the screen's foot is
    /// the party bar (placement record 7), which the HUD builder draws, as
    /// it draws the trail, so this window emits nothing at all.
    #[test]
    fn target_select_hands_the_strip_to_the_party_bar() {
        let r = rects();
        let font = legaia_font::Font::placeholder();
        let all = rows(2);
        let targets = [
            BattleItemTargetView {
                name: "Vahn",
                hp: 100,
                hp_max: 120,
                mp: 30,
                mp_max: 40,
                alive: true,
            },
            BattleItemTargetView {
                name: "Noa",
                hp: 80,
                hp_max: 90,
                mp: 55,
                mp_max: 60,
                alive: true,
            },
        ];
        let mut f = frame(&all, 1);
        f.targets = Some((&targets, 1));
        let sprites = battle_item_window_sprites(&r, &f, (0, 0), 1);
        // No strip window, no list window, no description window, no hand.
        for (name, seat) in [
            (
                "target strip",
                (TARGET_STRIP_WINDOW.0, TARGET_STRIP_WINDOW.1),
            ),
            ("list window", (LIST_WINDOW.0, LIST_WINDOW.1)),
            ("description window", (DESC_WINDOW.0, DESC_WINDOW.1)),
            ("hand cursor", (HAND_SEAT.0, HAND_SEAT.1 + ROW_PITCH)),
        ] {
            assert!(
                !sprites.iter().any(|s| (s.dst.0, s.dst.1) == seat),
                "{name} is not this window's at target select"
            );
        }
        assert!(sprites.is_empty(), "the trail and the strip are the HUD's");
        let draws = battle_item_window_text(&font, &f, true, (0, 0), 1);
        // No text on the strip's row (the bar's), none in the list band, no
        // PAGE header, no description line.
        let has = |y: i32| draws.iter().any(|d| d.dst.1 == y);
        assert!(!has(TARGET_NAME_SEAT.1), "the bar's row is the HUD's");
        assert!(!has(TARGET_HP_LABEL_SEAT.1), "no HP/MP text stand-ins");
        assert!(!has(ROW_TEXT_SEAT.1), "no roster column in the list band");
        assert!(draws.is_empty(), "no PAGE header, rows or crumbs");
        assert!(!has(DESC_SEAT.1), "the description closes with its window");
        // The third breadcrumb is the held item's name, not `Item`.
        assert_eq!(f.third_crumb(), all[1].name);
        // By contrast, browsing draws the description at its pen.
        let browse = battle_item_window_text(&font, &frame(&all, 1), true, (0, 0), 1);
        assert!(browse.iter().any(|d| d.dst.1 == DESC_SEAT.1));
    }

    /// The capture's strip pens are the party bar's seats - the same
    /// placement record (7) on both frames - so the pin lives on in the
    /// HUD builder's constants and this module's record cannot drift from it.
    #[test]
    fn target_strip_pins_are_the_party_bars_seats() {
        use crate::battle_hud::{
            BAR_DIGIT_Y, BAR_HP_CUR_RIGHT, BAR_HP_LABEL, BAR_INTERIOR_W, BAR_MP_CUR_RIGHT,
            BAR_MP_LABEL, BAR_NAME, BAR_X, BAR_Y, PLATE_CAP_W, PLATE_H,
        };
        assert_eq!(TARGET_STRIP_WINDOW.0, BAR_X);
        assert_eq!(TARGET_STRIP_WINDOW.1, BAR_Y);
        // The strip's right cap starts where the bar's interior ends.
        assert_eq!(
            TARGET_STRIP_WINDOW.2,
            PLATE_CAP_W + BAR_INTERIOR_W + PLATE_CAP_W
        );
        assert_eq!(TARGET_STRIP_WINDOW.3, PLATE_H);
        assert_eq!(TARGET_NAME_SEAT, BAR_NAME);
        assert_eq!(TARGET_HP_LABEL_SEAT, BAR_HP_LABEL);
        assert_eq!(TARGET_MP_LABEL_SEAT, BAR_MP_LABEL);
        assert_eq!(TARGET_HP_CUR_RIGHT, BAR_HP_CUR_RIGHT);
        assert_eq!(TARGET_MP_CUR_RIGHT, BAR_MP_CUR_RIGHT);
        assert_eq!(TARGET_VALUE_Y, BAR_DIGIT_Y);
    }

    /// The stage transform multiplies every prim like the sibling chrome
    /// builders.
    #[test]
    fn stage_scale_multiplies_every_prim() {
        let r = rects();
        let all = rows(2);
        let one = battle_item_window_sprites(&r, &frame(&all, 0), (0, 0), 1);
        let two = battle_item_window_sprites(&r, &frame(&all, 0), (7, 11), 2);
        assert_eq!(one.len(), two.len());
        for (a, b) in one.iter().zip(two.iter()) {
            assert_eq!(b.dst.0, 7 + a.dst.0 * 2);
            assert_eq!(b.dst.1, 11 + a.dst.1 * 2);
            assert_eq!(b.dst.2, a.dst.2 * 2);
            assert_eq!(b.dst.3, a.dst.3 * 2);
        }
    }

    /// The magic window (`0x46`) is the item window's two windows plus the
    /// two gold `Lv` / `MP` tabs, with the hand on the same seat and pitch -
    /// what the `cort_evolved_battle_first_menu` film shows one RIGHT press
    /// into the ring. No row text sits left of the list window.
    #[test]
    fn the_magic_window_is_the_item_windows_plus_the_level_tabs() {
        let r = rects();
        let font = legaia_font::Font::placeholder();
        let rows = [
            BattleSpellRowView {
                name: "Meta",
                admissible: true,
                ra_seru: true,
                icon: Some(0x1D),
            },
            BattleSpellRowView {
                name: "Gimard",
                admissible: true,
                ra_seru: false,
                icon: Some(0x14),
            },
        ];
        let desc = ["Inferno", "Attack all enemies."];
        let frame = BattleSpellMenuFrame {
            rows: &rows,
            cursor: Some(1),
            description: &desc,
            level_mp: Some((5, 240)),
        };
        let sprites = battle_spell_window_sprites(&r, &frame, (0, 0), 1);
        let list = crate::menu_window_chrome_draws_for(&r, LIST_WINDOW, (0, 0), 1);
        let key = |s: &SpriteDraw| (s.dst, s.src);
        assert_eq!(
            sprites[..list.len()].iter().map(key).collect::<Vec<_>>(),
            list.iter().map(key).collect::<Vec<_>>()
        );
        let hand = sprites.last().unwrap();
        assert_eq!(
            (hand.dst.0, hand.dst.1),
            (HAND_SEAT.0, HAND_SEAT.1 + ROW_PITCH)
        );
        // Both tab plates are there: a left cap at each pinned plate edge.
        for x in [188, 250] {
            assert!(
                sprites
                    .iter()
                    .any(|s| s.src == r.tab_cap_l && s.dst.0 == x && s.dst.1 == 8),
                "tab plate at x {x}"
            );
        }
        let text = battle_spell_window_text(&font, &frame, true, (0, 0), 1);
        assert!(!text.is_empty());
        for t in &text {
            let in_list = t.dst.0 >= LIST_WINDOW.0;
            let in_desc = t.dst.0 >= DESC_WINDOW.0 && t.dst.1 >= DESC_WINDOW.1;
            let in_tabs = t.dst.1 < LIST_WINDOW.1;
            assert!(in_list || in_desc || in_tabs, "{t:?} outside the window");
        }
        // Without a level / cost the tabs stay down.
        let bare = BattleSpellMenuFrame {
            level_mp: None,
            ..frame
        };
        assert!(
            battle_spell_window_sprites(&r, &bare, (0, 0), 1)
                .iter()
                .all(|s| s.src != r.tab_cap_l)
        );
    }

    /// With the escape sprites attached, each spell row draws its element
    /// plate untinted at the row pen and the name one plate width plus a
    /// space further on - the same raw-name layout as the pause Magic list.
    #[test]
    fn spell_rows_draw_their_element_plate() {
        use legaia_font::escape_icons::{EscapeIcon, EscapeIcons};
        let mut icons = vec![None; 0x1E];
        icons[0x1D] = Some(EscapeIcon {
            index: 0x1D,
            w: 28,
            h: 12,
            y_offset: 0,
            advance: 28,
            rgba: vec![0xFF; 28 * 12 * 4],
        });
        let font = legaia_font::synthetic_for_tests().with_escape_icons(&EscapeIcons { icons });
        let rows = [BattleSpellRowView {
            name: "Meta",
            admissible: true,
            ra_seru: true,
            icon: Some(0x1D),
        }];
        let frame = BattleSpellMenuFrame {
            rows: &rows,
            cursor: None,
            description: &[],
            level_mp: None,
        };
        let text = battle_spell_window_text(&font, &frame, true, (0, 0), 1);
        let y = ROW_TEXT_SEAT.1;
        let plate = text
            .iter()
            .find(|t| t.dst.2 == 28 && t.dst.1 == y)
            .expect("the winged plate draws");
        assert_eq!(plate.dst.0, SPELL_ICON_PEN_X);
        assert_eq!(plate.color[..3], [1.0, 1.0, 1.0]);
        let space = font.layout(b" ").advance_x as i32;
        assert!(
            text.iter()
                .any(|t| t.dst.1 == y && t.dst.0 == SPELL_ICON_PEN_X + 28 + space),
            "the name follows the plate"
        );
    }
}
