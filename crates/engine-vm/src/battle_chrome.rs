//! Retail battle-screen chrome: the actor-name plaque, the party status
//! readout and the command-chip cluster.
//!
//! REF: FUN_801DBC30 (the battle-overlay blit that is *not* any of this)
//!
//! Everything here is packet-pinned. The source is the live libgpu ordering
//! table inside mednafen battle save states: each retail frame leaves its
//! queued `SPRT` / `POLY_FT4` nodes in main RAM, so a RAM image *is* a display
//! list, and every rect, texture page, CLUT and screen seat below is read out
//! of those packet words rather than measured off a screenshot. The anchors are
//! cross-checked against a full-VRAM dump of the same frame.
//!
//! ## One sprite sheet, one 3-slice, four palettes
//!
//! The whole battle chrome samples the **resident system-UI TIM**
//! ([`legaia_asset::title_pak::OVERLAY_SYSTEM_UI_TIM_OFFSET`], `PROT.DAT`
//! `0x18E0`), which uploads its pixels to VRAM page `(896, 256)` and its
//! 16-row CLUT block packed into VRAM row **511** as 16 side-by-side
//! sub-palettes. Text comes from the neighbouring menu-glyph atlas at page
//! `(896, 0)` through row **510** sub-palette 13.
//!
//! The plaque, the party bar and every command chip are the **same three
//! tiles** - a left cap, a repeating body and a right cap - drawn at two sheet
//! rows and two palettes:
//!
//! | Row | Tiles | Sub-palette | Used by |
//! |---|---|---|---|
//! | `v = 0` | `(208,0)` / `(192,0)` / `(216,0)` | 4 (blue) | party status bar, command chips |
//! | `v = 64` | `(208,64)` / `(192,64)` / `(216,64)` | 12 (carved gold) | actor-name plaque |
//!
//! The `v = 64` row is the same art
//! [`legaia_asset::title_pak::OVERLAY_SYSTEM_UI_TAB_CAP_L`] pins for the
//! field menu's tab banner, which is why the battle plaque and the pause
//! menu's title tab look like the same object: they are.
//!
//! ## Retail draws no gauge bar
//!
//! Neither readout carries a meter. The party status bar and the per-member
//! panel both draw a label sprite plus numerals and nothing else - there is no
//! HP or MP bar primitive anywhere in the strip's packet run. A filled gauge
//! in the party HUD is an engine invention, not retail.

use legaia_asset::title_pak;

/// VRAM page the system-UI sprite sheet uploads to.
pub const WIDGET_PAGE: (u16, u16) = (896, 256);
/// VRAM page the menu-glyph ASCII atlas uploads to.
pub const FONT_PAGE: (u16, u16) = (896, 0);

/// CLUT block row the system-UI sheet's sub-palettes pack into.
pub const WIDGET_CLUT_ROW: u16 = 511;
/// CLUT block row the menu-glyph atlas's sub-palettes pack into.
pub const FONT_CLUT_ROW: u16 = 510;

/// Sub-palette the party status bar and the command chips decode with (blue).
pub const SUBPAL_PLATE_BLUE: u16 = 4;
/// Sub-palette the actor-name plaque decodes with (carved gold) - the same
/// ramp [`title_pak::OVERLAY_SYSTEM_UI_TAB_CLUT_ROW`] names.
pub const SUBPAL_PLATE_GOLD: u16 = 12;
/// Sub-palette the LV / HP / MP label sprites decode with.
pub const SUBPAL_LABEL: u16 = 1;
/// Sub-palette the `/` separator decodes with.
pub const SUBPAL_SEPARATOR: u16 = 5;
/// Sub-palette the per-member panel background decodes with.
pub const SUBPAL_PANEL: u16 = 0;
/// Sub-palette the D-pad glyph decodes with.
pub const SUBPAL_DPAD: u16 = 7;
/// Sub-palette every battle-chrome glyph decodes with on the font page.
pub const SUBPAL_TEXT: u16 = 13;

/// VRAM `(x, y)` of a sub-palette in a packed 16-entry CLUT block row.
pub const fn subpalette(row: u16, index: u16) -> (u16, u16) {
    (index * 16, row)
}

// ---------------------------------------------------------------------------
// Sheet rects
// ---------------------------------------------------------------------------

/// One packet-pinned sprite: a 1:1 blit of `(u, v, w, h)` texels from `page`
/// through `clut` to screen `(x, y)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChromeBlit {
    /// Screen x of the blit's top-left corner.
    pub x: i16,
    /// Screen y of the blit's top-left corner.
    pub y: i16,
    /// Blit width in pixels (= source width; retail never scales these).
    pub w: u16,
    /// Blit height in pixels.
    pub h: u16,
    /// Source texel u within [`ChromeBlit::page`].
    pub u: u16,
    /// Source texel v within [`ChromeBlit::page`].
    pub v: u16,
    /// VRAM page the texels come from.
    pub page: (u16, u16),
    /// VRAM `(x, y)` of the 16-entry sub-palette.
    pub clut: (u16, u16),
}

/// The three tiles a plate run is built from, at one sheet row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlateArt {
    /// Sheet row (`v`) the three tiles sit on.
    pub v: u16,
    /// Sub-palette the run decodes with.
    pub subpal: u16,
}

/// The blue plate row: the party status bar and every command chip.
pub const PLATE_BLUE: PlateArt = PlateArt {
    v: 0,
    subpal: SUBPAL_PLATE_BLUE,
};
/// The carved-gold plate row: the actor-name plaque.
pub const PLATE_GOLD: PlateArt = PlateArt {
    v: 64,
    subpal: SUBPAL_PLATE_GOLD,
};

/// Left-cap texel u (both rows) - `(208, v)`, 8x20.
pub const PLATE_CAP_L_U: u16 = 208;
/// Right-cap texel u (both rows) - `(216, v)`, 8x20.
pub const PLATE_CAP_R_U: u16 = 216;
/// Body-tile texel u (both rows) - `(192, v)`, 16x20.
pub const PLATE_BODY_U: u16 = 192;
/// Cap width in pixels.
pub const PLATE_CAP_W: u16 = 8;
/// Body-tile width in pixels; the final tile of a run is clipped to fit.
pub const PLATE_BODY_W: u16 = 16;
/// Plate height in pixels, all rows.
pub const PLATE_H: u16 = 20;

/// Per-member status **panel background**: one 102x48 marbled plate, texels
/// `(0, 0)` of the system-UI sheet, sub-palette 0. Three stacked cells - a
/// name row split by the LV badge, then an HP row and an MP row.
pub const PANEL_BG: (u16, u16, u16, u16) = (0, 0, 102, 48);

/// The `/` separator between a current and a maximum value: texels `(96, 64)`,
/// 8x16, sub-palette 5. Not a font glyph - a sheet sprite, and it sits four
/// rows **above** the numerals it separates.
pub const SEPARATOR: (u16, u16, u16, u16) = (96, 64, 8, 16);

/// The D-pad glyph at the centre of the command cluster: texels `(0, 112)`
/// 16x16, sub-palette 7, drawn 15x15 as a textured quad (not a sprite).
pub const DPAD_GLYPH: (u16, u16, u16, u16) = (0, 112, 16, 16);
/// Screen size the D-pad glyph is drawn at.
pub const DPAD_DRAW_W: u16 = 15;

/// Small numerals the readouts use: 8x12 cells at `v = 208`, `u = digit * 8`,
/// on the **font** page through sub-palette 13.
///
/// Every number on the battle screen is laid out in these cells, never in
/// the proportional dialog font - the names are proportional, the numbers
/// are not. That is what lets a four-digit HP live inside a 102-px roster
/// panel, and it is why each numeral field below is a fixed **right edge**
/// rather than a pen: the field grows leftward one 8-px cell per digit.
pub const DIGIT_V: u16 = 208;
/// Width and horizontal pitch of one small numeral cell.
pub const DIGIT_W: u16 = 8;
/// Height of one small numeral cell.
pub const DIGIT_H: u16 = 12;

/// Cell size the battle chrome draws a font-atlas glyph at (the cell is 16x16
/// in the sheet; retail blits 14x15 of it and advances by the glyph's own
/// width).
pub const GLYPH_DRAW: (u16, u16) = (14, 15);

/// Font-atlas cell for an ASCII code: `idx = code - 0x20`, sixteen cells per
/// row of 16x16 texels.
pub const fn glyph_cell(ascii: u8) -> (u16, u16) {
    let idx = (ascii as u16).wrapping_sub(0x20);
    ((idx % 16) * 16, (idx / 16) * 16)
}

/// Element-badge strip: eight 20x12 badges at a 32-texel pitch, first badge at
/// `u = 6`. Row `v = 192` is the plain shield, `v = 208` the winged variant.
/// Each badge carries its own sub-palette out of the CLUT block at VRAM x
/// `896..`, rows 498 / 499 - the palette is picked per badge, not per row.
pub const BADGE_U0: u16 = 6;
/// Horizontal pitch between element badges.
pub const BADGE_PITCH: u16 = 32;
/// Badge width in pixels.
pub const BADGE_W: u16 = 20;
/// Badge height in pixels.
pub const BADGE_H: u16 = 12;
/// Sheet row of the plain shield badges.
pub const BADGE_ROW_PLAIN: u16 = 192;
/// Sheet row of the winged badges.
pub const BADGE_ROW_WINGED: u16 = 208;

/// Texel rect of element badge `index` on `row`.
pub const fn element_badge(index: u16, row: u16) -> (u16, u16, u16, u16) {
    (BADGE_U0 + index * BADGE_PITCH, row, BADGE_W, BADGE_H)
}

// ---------------------------------------------------------------------------
// The 3-slice run
// ---------------------------------------------------------------------------

/// Compose a plate run: a left cap at `x`, body tiles filling `interior_w`
/// pixels from `x + 8` with the **last tile clipped** to the remainder, and a
/// right cap closing it.
///
/// The clipped final tile is the retail behaviour, not a rounding of it: a
/// 27-pixel interior emits a 16-wide tile and an 11-wide tile, and the caps
/// land at `x` and `x + 8 + 27` exactly. Total plate width is
/// `interior_w + 16`.
pub fn plate_run(x: i16, y: i16, interior_w: u16, art: PlateArt) -> Vec<ChromeBlit> {
    let clut = subpalette(WIDGET_CLUT_ROW, art.subpal);
    let blit = |x: i16, u: u16, w: u16| ChromeBlit {
        x,
        y,
        w,
        h: PLATE_H,
        u,
        v: art.v,
        page: WIDGET_PAGE,
        clut,
    };
    let mut out = vec![blit(x, PLATE_CAP_L_U, PLATE_CAP_W)];
    let interior_x = x + PLATE_CAP_W as i16;
    let mut done = 0u16;
    while done < interior_w {
        let w = PLATE_BODY_W.min(interior_w - done);
        out.push(blit(interior_x + done as i16, PLATE_BODY_U, w));
        done += w;
    }
    out.push(blit(
        interior_x + interior_w as i16,
        PLATE_CAP_R_U,
        PLATE_CAP_W,
    ));
    out
}

/// Total on-screen width of a plate run with `interior_w` pixels of interior.
pub const fn plate_width(interior_w: u16) -> u16 {
    interior_w + 2 * PLATE_CAP_W
}

// ---------------------------------------------------------------------------
// The element box -> plate law
// ---------------------------------------------------------------------------

/// The plaque, the party bar and every command chip are one law over one data
/// source. Each is a record of the **screen-element placement table** at
/// `0x80076C10` ([`crate::battle_party_panel::ELEMENT_PLACEMENT_TABLE`],
/// layout in `docs/reference/memory-map.md`), and the plate is derived from
/// the record's content box rather than stored:
///
/// ```text
/// glyph pen = (rec.x, rec.y - 2)
/// plate     = (rec.x - 8, rec.y - 6), (rec.w + 16, 20)
/// ```
///
/// `rec.h` is `0x0C` in every initialised record, so the plate is always 20
/// tall, and `rec.w` **is** the interior width - which is why a plate is
/// sized to its content with the final body tile clipped. The same `-8` / `-4`
/// content-to-plate bias appears in
/// [`crate::battle_party_panel::cross_out_mark`], the other overlay leaf that
/// frames a content box.
///
/// The law is packet-verified on four surfaces at once: plaque `(16, 12)` w=63
/// -> `(8, 8)` 79x20; active-actor bar `(16, 192)` w=288 -> `(8, 188)` 304x20;
/// command chip `(204, 32)` w=48 -> `(196, 28)` 64x20; top-level chip
/// `(104, 86)` w=36 -> `(96, 82)` 52x20.
pub const PLATE_INSET_X: i16 = 8;
/// Vertical inset from the glyph pen to the plate top (see [`PLATE_INSET_X`]).
pub const PLATE_INSET_Y: i16 = 4;
/// Offset from a placement record's `y` to the glyph pen it seats.
pub const RECORD_PEN_DY: i16 = -2;

/// Placement-table record index whose content box **is** the actor-name
/// plaque (`0x80076C10 + 68 * 0x18` = `0x80077270`, element id pair `0x2323`,
/// kind `0x0202`).
///
/// Pinned by width rather than by name: across three battle save states the
/// record's `w` tracks the plaque's measured interior exactly - 27 for `Vahn`,
/// 62 for `CheDelilas`, 63 for `Gimard` behind its element badge - while its
/// live seat stays `(16, 14)` and its parked seat `(16, -24)`. The plaque
/// therefore slides in from above the screen, and `+0x14` points at the name
/// scratch buffer the string was measured out of.
pub const PLAQUE_PLACEMENT_RECORD: usize = 68;

/// The table itself is disc data - parser
/// [`legaia_asset::screen_elements`], which also carries the corrected extent
/// (103 records, not 200) and the per-family box heights.
pub use legaia_asset::screen_elements;

/// Glyph pen and plate rect for a placement record's `(x, y, w)`.
///
/// Returns `(pen, plate_origin, plate_interior_w)`; feed the last two to
/// [`plate_run`].
pub const fn plate_for_record(x: i16, y: i16, w: u16) -> ((i16, i16), (i16, i16), u16) {
    let pen = (x, y + RECORD_PEN_DY);
    let plate = (pen.0 - PLATE_INSET_X, pen.1 - PLATE_INSET_Y);
    (pen, plate, w)
}

// ---------------------------------------------------------------------------
// The actor-name plaque
// ---------------------------------------------------------------------------

/// Screen x of the actor-name plaque's left cap - fixed, every battle.
pub const PLAQUE_X: i16 = 8;
/// Screen y of the actor-name plaque's left cap - fixed, every battle.
pub const PLAQUE_Y: i16 = 8;
/// Gap between the element badge and the first name glyph.
pub const PLAQUE_BADGE_GAP: u16 = 5;
/// Vertical inset of the plaque's contents from the plate top.
pub const PLAQUE_CONTENT_DY: i16 = 4;

/// Where the actor-name plaque's pieces land, given the pixel width of the
/// name as the proportional font lays it out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NamePlaque {
    /// The plate run, in draw order.
    pub plate: Vec<ChromeBlit>,
    /// Screen seat of the element badge, when the actor carries one.
    pub badge: Option<(i16, i16)>,
    /// Screen seat of the first name glyph.
    pub text: (i16, i16),
    /// Interior width the plate was built at.
    pub interior_w: u16,
}

/// Lay the plaque out. The interior is exactly the content: an optional
/// 20-pixel badge plus a 5-pixel gap, then the measured name.
///
/// Packet-pinned across nine battle save states: "Vahn" (no badge) gives a
/// 27-pixel interior with the right cap at x=43, and "Gimard" behind a badge
/// gives 63 with the right cap at x=79.
pub fn name_plaque(name_w: u16, has_badge: bool) -> NamePlaque {
    let lead = if has_badge {
        BADGE_W + PLAQUE_BADGE_GAP
    } else {
        0
    };
    let interior_w = lead + name_w;
    let content_x = PLAQUE_X + PLATE_CAP_W as i16;
    let content_y = PLAQUE_Y + PLAQUE_CONTENT_DY;
    NamePlaque {
        plate: plate_run(PLAQUE_X, PLAQUE_Y, interior_w, PLATE_GOLD),
        badge: has_badge.then_some((content_x, content_y)),
        text: (content_x + lead as i16, content_y),
        interior_w,
    }
}

// ---------------------------------------------------------------------------
// The party status readouts
// ---------------------------------------------------------------------------

/// Screen y of the full-width **active-actor bar**'s plate.
pub const BAR_Y: i16 = 188;
/// Screen x of the active-actor bar's left cap.
pub const BAR_X: i16 = 8;
/// Interior width of the active-actor bar - fixed, so the bar always spans
/// `8 ..= 312`.
pub const BAR_INTERIOR_W: u16 = 288;
/// Name-glyph seat inside the active-actor bar.
pub const BAR_NAME: (i16, i16) = (16, 192);
/// HP label-sprite seat inside the active-actor bar.
pub const BAR_HP_LABEL: (i16, i16) = (80, 194);
/// MP label-sprite seat inside the active-actor bar.
pub const BAR_MP_LABEL: (i16, i16) = (192, 194);
/// HP `/` separator seat (numerals sit four rows lower).
pub const BAR_HP_SEPARATOR: (i16, i16) = (136, 188);
/// MP `/` separator seat.
pub const BAR_MP_SEPARATOR: (i16, i16) = (240, 188);
/// Screen y of every numeral in the active-actor bar.
pub const BAR_DIGIT_Y: i16 = 192;
/// Right edge the HP current value is laid out back from.
pub const BAR_HP_CUR_RIGHT: i16 = 134;
/// Right edge the HP **maximum** is laid out back from.
///
/// Both halves of a `cur / max` pair are right-aligned; neither runs
/// forward from a pen. Two captures with different digit counts fix this
/// edge: `battle_gimard_tail_fire_a` draws `154 / 180` with the maximum's
/// three cells at `154 / 162 / 170`, and a four-digit party frame draws
/// `4955 / 4984` with the maximum's four cells at `146 / 154 / 162 / 170`.
/// The left edge moves, `178` does not.
pub const BAR_HP_MAX_RIGHT: i16 = 178;
/// Right edge the MP current value is laid out back from.
pub const BAR_MP_CUR_RIGHT: i16 = 238;
/// Right edge the MP maximum is laid out back from - same two captures,
/// `20` at `258` and `856` at `250 / 258 / 266`.
pub const BAR_MP_MAX_RIGHT: i16 = 274;

/// Left edge of a right-aligned numeral field of `digits` cells.
pub const fn digits_left_of(right: i16, digits: u16) -> i16 {
    right - (digits * DIGIT_W) as i16
}

/// Cell count a value occupies, which is its decimal digit count (retail
/// pads nothing - a level of `1` is one cell, `99` is two).
pub const fn digit_count(value: u32) -> u16 {
    let mut n = 1;
    let mut v = value;
    while v >= 10 {
        v /= 10;
        n += 1;
    }
    n
}

/// Screen y of the per-member panel cluster.
pub const PANEL_Y: i16 = 164;
/// Screen y the panel cluster is parked at while the active-actor bar has the
/// screen. Below the 240-line display window, so the draws survive but are
/// never seen.
pub const PANEL_PARK_Y: i16 = 230;
/// Horizontal pitch between adjacent member panels.
pub const PANEL_PITCH: i16 = 102;
/// Inset from a panel's left edge to its name pen - the offset that turns
/// `battle_party_panel::panel_anchors` (which are *text* anchors) into panel
/// backgrounds.
pub const PANEL_TEXT_INSET: i16 = 5;

/// Per-member panel content seats, relative to the panel background's
/// top-left corner.
///
/// Every numeral field is a **right edge**, not a pen - see [`DIGIT_V`].
/// The panel's content box is inset 5 px on the left ([`NAME`]) and its
/// numerals close 5 px short of the right ([`MAX_RIGHT`] = `102 - 5`), so
/// a four-digit maximum runs `65..97` and still clears the panel.
pub mod panel {
    /// Name-glyph pen.
    pub const NAME: (i16, i16) = (5, 4);
    /// LV label sprite.
    pub const LV_LABEL: (i16, i16) = (64, 6);
    /// Right edge the level numerals are laid out back from. A level of
    /// `1` puts its one cell at `88`, `99` puts two at `80` / `88` -
    /// the same right-aligned law as every other number on the screen,
    /// and `FUN_8002C2E4`'s `pen + 0x4B` is the two-digit case of it
    /// (`name pen 5 + 0x4B = 80`).
    pub const LV_DIGITS_RIGHT: i16 = 96;
    /// Screen y of the level numerals.
    pub const LV_DIGIT_Y: i16 = 4;
    /// HP label sprite.
    pub const HP_LABEL: (i16, i16) = (4, 21);
    /// MP label sprite.
    pub const MP_LABEL: (i16, i16) = (4, 36);
    /// HP `/` separator (numerals sit four rows lower).
    pub const HP_SEPARATOR: (i16, i16) = (57, 15);
    /// MP `/` separator.
    pub const MP_SEPARATOR: (i16, i16) = (57, 30);
    /// Screen y of the HP numerals.
    pub const HP_DIGIT_Y: i16 = 19;
    /// Screen y of the MP numerals.
    pub const MP_DIGIT_Y: i16 = 34;
    /// Right edge a current value is laid out back from - also the `/`
    /// separator's left edge, which the current value butts against.
    pub const CUR_RIGHT: i16 = 57;
    /// Right edge a maximum value is laid out back from. Pinned across
    /// two-, three- and four-digit values in two captures: `20` at `81`,
    /// `180` / `856` at `73`, `4984` at `65`. Only the right edge holds.
    pub const MAX_RIGHT: i16 = 97;
}

/// Panel-background x seats for a party of `size`, left to right.
///
/// `battle_party_panel::panel_anchors` carries the same layout as *text*
/// anchors and only for the two slots `FUN_801D84C0` writes; these are the
/// backgrounds, for every slot, and they are what the packet run shows.
pub fn panel_seats(size: u8) -> Vec<i16> {
    match size {
        1 => vec![109],
        2 => vec![58, 160],
        3 => vec![7, 109, 211],
        _ => Vec::new(),
    }
}

// ---------------------------------------------------------------------------
// The command-chip cluster
// ---------------------------------------------------------------------------

/// Which seat of the four-way command cluster a chip takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChipSeat {
    /// Up.
    Up,
    /// Left.
    Left,
    /// Right.
    Right,
    /// Down.
    Down,
}

/// A cluster of command chips around a D-pad glyph.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChipCluster {
    /// Screen centre - also the D-pad glyph's centre.
    pub centre: (i16, i16),
    /// Distance from the centre to a left / right chip's centre.
    pub dx: i16,
    /// Distance from the centre to an up / down chip's centre; `0` when the
    /// cluster has no vertical arms.
    pub dy: i16,
    /// Interior width every chip in the cluster is built at. Uniform across
    /// the cluster - a chip is not sized to its own label.
    pub interior_w: u16,
}

/// The round's opening `Begin | Run` cluster: a horizontal pair. Seat and size
/// are fixed - the same packets appear in a solo tutorial fight and in a
/// three-member battle.
pub const CLUSTER_TOP_LEVEL: ChipCluster = ChipCluster {
    centre: (160, 92),
    dx: 38,
    dy: 0,
    interior_w: 36,
};

/// The per-actor command cluster (`Item` / `Attack` / the element command /
/// `Spirit`): a four-way diamond seated on the right of the stage.
pub const CLUSTER_COMMAND: ChipCluster = ChipCluster {
    centre: (228, 70),
    dx: 44,
    dy: 32,
    interior_w: 48,
};

impl ChipCluster {
    /// Top-left corner of the chip plate at `seat`.
    pub const fn plate_origin(&self, seat: ChipSeat) -> (i16, i16) {
        let half = (plate_width(self.interior_w) / 2) as i16;
        let (dx, dy) = match seat {
            ChipSeat::Up => (0, -self.dy),
            ChipSeat::Left => (-self.dx, 0),
            ChipSeat::Right => (self.dx, 0),
            ChipSeat::Down => (0, self.dy),
        };
        (
            self.centre.0 + dx - half,
            self.centre.1 + dy - (PLATE_H / 2) as i16,
        )
    }

    /// Label pen for the chip at `seat`: the interior's left edge, four rows
    /// down. Labels are left-aligned in the interior, not centred.
    pub const fn label_seat(&self, seat: ChipSeat) -> (i16, i16) {
        let (x, y) = self.plate_origin(seat);
        (x + PLATE_CAP_W as i16, y + PLAQUE_CONTENT_DY)
    }

    /// The D-pad glyph's drawn rect, centred on the cluster.
    pub const fn dpad_rect(&self) -> (i16, i16, u16, u16) {
        (
            self.centre.0 - 8,
            self.centre.1 - 8,
            DPAD_DRAW_W,
            DPAD_DRAW_W,
        )
    }
}

// ---------------------------------------------------------------------------
// Asset-crate cross-checks
// ---------------------------------------------------------------------------

/// The four battle rects the `engine-core` atlas bakes are these same rects.
///
/// `legaia_asset::title_pak` owns the *source* side (which texels of the
/// system-UI sheet to copy); this module owns the *draw* side (where they
/// land and how a run composes). They have to name one set of numbers.
pub const fn battle_chrome_source_rects_match() -> bool {
    let (pb_u, pb_v, pb_w, pb_h) = title_pak::OVERLAY_SYSTEM_UI_BATTLE_PANEL_BG;
    let (cl_u, cl_v, cl_w, cl_h) = title_pak::OVERLAY_SYSTEM_UI_BATTLE_PLATE_CAP_L;
    let (b_u, b_v, b_w, b_h) = title_pak::OVERLAY_SYSTEM_UI_BATTLE_PLATE_BODY;
    let (cr_u, cr_v, cr_w, cr_h) = title_pak::OVERLAY_SYSTEM_UI_BATTLE_PLATE_CAP_R;
    let (s_u, s_v, s_w, s_h) = title_pak::OVERLAY_SYSTEM_UI_BATTLE_SEPARATOR;
    pb_u == PANEL_BG.0 as u32
        && pb_v == PANEL_BG.1 as u32
        && pb_w == PANEL_BG.2 as u32
        && pb_h == PANEL_BG.3 as u32
        && title_pak::OVERLAY_SYSTEM_UI_BATTLE_PANEL_CLUT_ROW == SUBPAL_PANEL
        && cl_u == PLATE_CAP_L_U as u32
        && cl_v == PLATE_BLUE.v as u32
        && cl_w == PLATE_CAP_W as u32
        && cl_h == PLATE_H as u32
        && b_u == PLATE_BODY_U as u32
        && b_v == PLATE_BLUE.v as u32
        && b_w == PLATE_BODY_W as u32
        && b_h == PLATE_H as u32
        && cr_u == PLATE_CAP_R_U as u32
        && cr_v == PLATE_BLUE.v as u32
        && cr_w == PLATE_CAP_W as u32
        && cr_h == PLATE_H as u32
        && title_pak::OVERLAY_SYSTEM_UI_BATTLE_PLATE_CLUT_ROW == SUBPAL_PLATE_BLUE
        && s_u == SEPARATOR.0 as u32
        && s_v == SEPARATOR.1 as u32
        && s_w == SEPARATOR.2 as u32
        && s_h == SEPARATOR.3 as u32
        && title_pak::OVERLAY_SYSTEM_UI_BATTLE_SEPARATOR_CLUT_ROW == SUBPAL_SEPARATOR
}

/// The gold plate row is byte-for-byte the field menu's tab-banner plaque.
pub const fn gold_plate_matches_tab_banner() -> bool {
    let (cl_u, cl_v, cl_w, cl_h) = title_pak::OVERLAY_SYSTEM_UI_TAB_CAP_L;
    let (b_u, b_v, b_w, b_h) = title_pak::OVERLAY_SYSTEM_UI_TAB_BODY;
    let (cr_u, cr_v, cr_w, cr_h) = title_pak::OVERLAY_SYSTEM_UI_TAB_CAP_R;
    cl_u == PLATE_CAP_L_U as u32
        && cl_v == PLATE_GOLD.v as u32
        && cl_w == PLATE_CAP_W as u32
        && cl_h == PLATE_H as u32
        && b_u == PLATE_BODY_U as u32
        && b_v == PLATE_GOLD.v as u32
        && b_w == PLATE_BODY_W as u32
        && b_h == PLATE_H as u32
        && cr_u == PLATE_CAP_R_U as u32
        && cr_v == PLATE_GOLD.v as u32
        && cr_w == PLATE_CAP_W as u32
        && cr_h == PLATE_H as u32
        && title_pak::OVERLAY_SYSTEM_UI_TAB_CLUT_ROW == SUBPAL_PLATE_GOLD
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seats(b: &[ChromeBlit]) -> Vec<(i16, u16, u16)> {
        b.iter().map(|c| (c.x, c.u, c.w)).collect()
    }

    #[test]
    fn gold_plate_is_the_tab_banner_art() {
        assert!(gold_plate_matches_tab_banner());
    }

    #[test]
    fn the_atlas_bakes_the_rects_this_module_draws() {
        assert!(battle_chrome_source_rects_match());
    }

    #[test]
    fn vahn_plaque_matches_the_captured_packets() {
        // `battle_melee_hit_spark` / `player_steal_skeleton_pre`: six sprites,
        // right cap at x=43, no badge.
        let p = name_plaque(27, false);
        assert_eq!(p.interior_w, 27);
        assert_eq!(p.text, (16, 12));
        assert_eq!(p.badge, None);
        assert_eq!(
            seats(&p.plate),
            vec![(8, 208, 8), (16, 192, 16), (32, 192, 11), (43, 216, 8)]
        );
        assert!(p.plate.iter().all(|c| c.y == 8 && c.h == 20 && c.v == 64));
        assert!(p.plate.iter().all(|c| c.clut == (192, 511)));
    }

    #[test]
    fn gimard_plaque_carries_the_badge_and_grows_by_it() {
        // `battle_gimard_tail_fire_a`: badge at (16,12), name pen at 41,
        // right cap at 79.
        let p = name_plaque(38, true);
        assert_eq!(p.interior_w, 63);
        assert_eq!(p.badge, Some((16, 12)));
        assert_eq!(p.text, (41, 12));
        assert_eq!(
            seats(&p.plate),
            vec![
                (8, 208, 8),
                (16, 192, 16),
                (32, 192, 16),
                (48, 192, 16),
                (64, 192, 15),
                (79, 216, 8),
            ]
        );
    }

    #[test]
    fn short_and_long_names_reproduce_their_captures() {
        // "Noa" (20 px) -> cap at 36; "Zeto" (24) -> 40; "Carl" (23) -> 39;
        // "CheDelilas" (62) -> 78.
        for (w, cap_x) in [(20u16, 36i16), (24, 40), (23, 39), (62, 78)] {
            let p = name_plaque(w, false);
            assert_eq!(p.plate.last().unwrap().x, cap_x, "name width {w}");
            assert_eq!(p.plate.last().unwrap().u, PLATE_CAP_R_U);
        }
    }

    #[test]
    fn active_actor_bar_spans_the_captured_run() {
        let run = plate_run(BAR_X, BAR_Y, BAR_INTERIOR_W, PLATE_BLUE);
        // A left cap, eighteen full body tiles, a right cap.
        assert_eq!(run.len(), 20);
        assert_eq!(run[0].x, 8);
        assert!(run[1..19].iter().all(|c| c.w == 16 && c.u == 192));
        assert_eq!(run[19].x, 304);
        assert_eq!(plate_width(BAR_INTERIOR_W), 304);
        assert!(run.iter().all(|c| c.clut == (64, 511) && c.v == 0));
    }

    #[test]
    fn bar_numeral_fields_land_where_retail_put_them() {
        // `battle_gimard_tail_fire_a`: HP 154 / 180, MP 20 / 20.
        assert_eq!(digits_left_of(BAR_HP_CUR_RIGHT, 3), 110);
        assert_eq!(digits_left_of(BAR_HP_MAX_RIGHT, 3), 154);
        assert_eq!(digits_left_of(BAR_MP_CUR_RIGHT, 2), 222);
        assert_eq!(digits_left_of(BAR_MP_MAX_RIGHT, 2), 258);
        // The separator sits four rows above the numerals it separates.
        assert_eq!(BAR_HP_SEPARATOR.1 + 4, BAR_DIGIT_Y);
    }

    /// The same two edges under a four-digit HP - the case that shows the
    /// maximum is right-aligned and not a forward-running pen. Packet
    /// cells: current `102/110/118/126`, maximum `146/154/162/170`, MP
    /// `214/222/230` and `250/258/266`.
    #[test]
    fn four_digit_bar_values_reproduce_their_capture() {
        assert_eq!(digits_left_of(BAR_HP_CUR_RIGHT, 4), 102);
        assert_eq!(digits_left_of(BAR_HP_MAX_RIGHT, 4), 146);
        assert_eq!(digits_left_of(BAR_MP_CUR_RIGHT, 3), 214);
        assert_eq!(digits_left_of(BAR_MP_MAX_RIGHT, 3), 250);
    }

    /// How many cells each field can take before it runs into the label
    /// cell before it or the sprite after it. The bar spends its width
    /// unevenly: four cells per HP field, three per MP field. The roster
    /// panel affords four everywhere. Nothing on the screen affords five,
    /// which is the ceiling both surfaces are laid out against.
    #[test]
    fn each_numeral_field_budgets_the_cells_retail_gave_it() {
        // HP: current clears the `HP` label cell, maximum clears the `/`.
        assert!(digits_left_of(BAR_HP_CUR_RIGHT, 4) >= BAR_HP_LABEL.0 + 16);
        assert!(digits_left_of(BAR_HP_MAX_RIGHT, 4) >= BAR_HP_SEPARATOR.0 + 8);
        assert!(BAR_HP_CUR_RIGHT <= BAR_HP_SEPARATOR.0);
        assert!(BAR_HP_MAX_RIGHT <= BAR_MP_LABEL.0);
        // MP: three cells fit, a fourth would cross the label / the `/`.
        assert!(digits_left_of(BAR_MP_CUR_RIGHT, 3) >= BAR_MP_LABEL.0 + 16);
        assert!(digits_left_of(BAR_MP_MAX_RIGHT, 3) >= BAR_MP_SEPARATOR.0 + 8);
        assert!(digits_left_of(BAR_MP_CUR_RIGHT, 4) < BAR_MP_LABEL.0 + 16);
        assert!(digits_left_of(BAR_MP_MAX_RIGHT, 4) < BAR_MP_SEPARATOR.0 + 8);
        assert!(BAR_MP_CUR_RIGHT <= BAR_MP_SEPARATOR.0);
        // The bar's interior ends at BAR_X + 8 + 288 = 304.
        assert!(BAR_MP_MAX_RIGHT <= BAR_X + PLATE_CAP_W as i16 + BAR_INTERIOR_W as i16);
    }

    #[test]
    fn digit_counts_are_the_cell_counts_retail_draws() {
        assert_eq!(digit_count(0), 1);
        assert_eq!(digit_count(1), 1);
        assert_eq!(digit_count(20), 2);
        assert_eq!(digit_count(180), 3);
        assert_eq!(digit_count(4984), 4);
        assert_eq!(digit_count(9999), 4);
    }

    #[test]
    fn panel_seats_match_the_text_anchors_the_overlay_publishes() {
        use crate::battle_party_panel::panel_anchors;
        for size in 1..=3u8 {
            let seats = panel_seats(size);
            let (primary, secondary) = panel_anchors(size).unwrap();
            assert_eq!(seats[0] + PANEL_TEXT_INSET, primary, "size {size}");
            if let Some(sec) = secondary {
                assert_eq!(seats[1] + PANEL_TEXT_INSET, sec, "size {size}");
            }
        }
        assert_eq!(panel_seats(3), vec![7, 109, 211]);
        assert_eq!(panel_seats(3)[1] - panel_seats(3)[0], PANEL_PITCH);
    }

    #[test]
    fn panel_content_seats_match_the_three_party_capture() {
        // Three-panel party frame, middle panel at x=109, y=164.
        let (px, py) = (109i16, PANEL_Y);
        assert_eq!((px + panel::NAME.0, py + panel::NAME.1), (114, 168));
        assert_eq!((px + panel::LV_LABEL.0, py + panel::LV_LABEL.1), (173, 170));
        assert_eq!((px + panel::HP_LABEL.0, py + panel::HP_LABEL.1), (113, 185));
        assert_eq!((px + panel::MP_LABEL.0, py + panel::MP_LABEL.1), (113, 200));
        assert_eq!(px + panel::HP_SEPARATOR.0, 166);
        assert_eq!(py + panel::HP_SEPARATOR.1, 179);
        assert_eq!(py + panel::HP_DIGIT_Y, 183);
        // That panel's HP row is 4560 / 4560: four cells back from 57,
        // four cells back from 97.
        assert_eq!(px + digits_left_of(panel::CUR_RIGHT, 4), 134);
        assert_eq!(px + digits_left_of(panel::MAX_RIGHT, 4), 174);
        // Its MP row is 783 / 783 - three cells against the same edges.
        assert_eq!(px + digits_left_of(panel::CUR_RIGHT, 3), 142);
        assert_eq!(px + digits_left_of(panel::MAX_RIGHT, 3), 182);
    }

    /// The solo-panel capture, whose values have fewer digits against the
    /// same edges: HP 180 / 180, MP 20 / 20, LV 1, panel at x=109.
    ///
    /// Read together with the three-party frame this is what falsifies a
    /// forward-running maximum: a 2-, 3- and 4-digit maximum share a right
    /// edge and no left one.
    #[test]
    fn solo_panel_capture_shares_the_same_right_edges() {
        let px = 109i16;
        assert_eq!(px + digits_left_of(panel::CUR_RIGHT, 3), 142);
        assert_eq!(px + digits_left_of(panel::MAX_RIGHT, 3), 182);
        assert_eq!(px + digits_left_of(panel::CUR_RIGHT, 2), 150);
        assert_eq!(px + digits_left_of(panel::MAX_RIGHT, 2), 190);
        // One-digit level at 197; the three-party frame's level 99 at
        // 189 / 197.
        assert_eq!(px + digits_left_of(panel::LV_DIGITS_RIGHT, 1), 197);
        assert_eq!(px + digits_left_of(panel::LV_DIGITS_RIGHT, 2), 189);
    }

    /// Every panel field has to fit the 102-px plate at its widest, or it
    /// bleeds into the neighbouring panel (`PANEL_PITCH` is also 102).
    #[test]
    fn widest_panel_fields_stay_inside_the_plate() {
        assert!(digits_left_of(panel::CUR_RIGHT, 4) >= panel::HP_LABEL.0 + 16);
        assert!(digits_left_of(panel::MAX_RIGHT, 4) >= panel::HP_SEPARATOR.0 + 8);
        assert!(panel::MAX_RIGHT <= PANEL_BG.2 as i16);
        assert!(panel::LV_DIGITS_RIGHT <= PANEL_BG.2 as i16);
        assert!(digits_left_of(panel::LV_DIGITS_RIGHT, 2) >= panel::LV_LABEL.0 + 16);
        // The content box is inset symmetrically: 5 px in from each edge.
        assert_eq!(panel::NAME.0, PANEL_BG.2 as i16 - panel::MAX_RIGHT);
        assert_eq!(panel::NAME.0, PANEL_TEXT_INSET);
    }

    #[test]
    fn command_cluster_reproduces_the_submenu_capture() {
        // `v0_1_battle_command_submenu`: Item / Attack / - / Spirit.
        let c = CLUSTER_COMMAND;
        assert_eq!(c.plate_origin(ChipSeat::Up), (196, 28));
        assert_eq!(c.plate_origin(ChipSeat::Left), (152, 60));
        assert_eq!(c.plate_origin(ChipSeat::Right), (240, 60));
        assert_eq!(c.plate_origin(ChipSeat::Down), (196, 92));
        assert_eq!(c.label_seat(ChipSeat::Up), (204, 32));
        assert_eq!(c.label_seat(ChipSeat::Left), (160, 64));
        assert_eq!(c.label_seat(ChipSeat::Down), (204, 96));
        assert_eq!(c.dpad_rect(), (220, 62, 15, 15));
        assert_eq!(
            seats(&plate_run(196, 28, c.interior_w, PLATE_BLUE)),
            vec![
                (196, 208, 8),
                (204, 192, 16),
                (220, 192, 16),
                (236, 192, 16),
                (252, 216, 8),
            ]
        );
    }

    #[test]
    fn top_level_cluster_reproduces_the_begin_run_capture() {
        // `v0_1_battle_command_menu` and `party_battle_gobu_gobu` agree:
        // Begin at 96, Run at 172, both y=82, D-pad at (152, 84).
        let c = CLUSTER_TOP_LEVEL;
        assert_eq!(c.plate_origin(ChipSeat::Left), (96, 82));
        assert_eq!(c.plate_origin(ChipSeat::Right), (172, 82));
        assert_eq!(c.label_seat(ChipSeat::Left), (104, 86));
        assert_eq!(c.label_seat(ChipSeat::Right), (180, 86));
        assert_eq!(c.dpad_rect(), (152, 84, 15, 15));
        assert_eq!(
            seats(&plate_run(96, 82, c.interior_w, PLATE_BLUE)),
            vec![
                (96, 208, 8),
                (104, 192, 16),
                (120, 192, 16),
                (136, 192, 4),
                (140, 216, 8),
            ]
        );
    }

    #[test]
    fn one_record_law_derives_all_four_plate_surfaces() {
        // (record x, record y, record w) -> (glyph pen, plate origin), each
        // read off the live placement table and matched against the packets.
        let cases = [
            ((16i16, 14i16, 63u16), (16i16, 12i16), (8i16, 8i16)), // plaque
            ((16, 194, 288), (16, 192), (8, 188)),                 // active bar
            ((204, 34, 48), (204, 32), (196, 28)),                 // Item chip
            ((160, 66, 48), (160, 64), (152, 60)),                 // Attack chip
            ((104, 88, 36), (104, 86), (96, 82)),                  // Begin chip
        ];
        for ((rx, ry, rw), pen, plate) in cases {
            let (got_pen, got_plate, interior) = plate_for_record(rx, ry, rw);
            assert_eq!(got_pen, pen, "pen for record ({rx},{ry},{rw})");
            assert_eq!(got_plate, plate, "plate for record ({rx},{ry},{rw})");
            assert_eq!(plate_width(interior), rw + 16);
        }
    }

    #[test]
    fn the_plaque_record_derives_the_plaque_layout() {
        // Record 68's w is the plaque interior, so the placement table and
        // the content-measured layout have to agree.
        assert_eq!(
            PLAQUE_PLACEMENT_RECORD * 0x18 + 0x8007_6C10,
            0x8007_7270_usize
        );
        for (w, badge, name_w) in [(27u16, false, 27u16), (63, true, 38)] {
            let (pen, plate, interior) = plate_for_record(16, 14, w);
            let p = name_plaque(name_w, badge);
            assert_eq!(interior, p.interior_w);
            assert_eq!(plate, (PLAQUE_X, PLAQUE_Y));
            assert_eq!(pen.1, p.text.1);
        }
    }

    #[test]
    fn glyph_cells_decode_the_captured_uvs() {
        assert_eq!(glyph_cell(b'V'), (96, 48));
        assert_eq!(glyph_cell(b'a'), (16, 64));
        assert_eq!(glyph_cell(b'h'), (128, 64));
        assert_eq!(glyph_cell(b'n'), (224, 64));
        assert_eq!(glyph_cell(b'G'), (112, 32));
        assert_eq!(glyph_cell(b'S'), (48, 48));
    }

    #[test]
    fn element_badges_step_by_the_captured_pitch() {
        assert_eq!(element_badge(0, BADGE_ROW_PLAIN), (6, 192, 20, 12));
        assert_eq!(element_badge(1, BADGE_ROW_PLAIN), (38, 192, 20, 12));
        assert_eq!(element_badge(5, BADGE_ROW_PLAIN), (166, 192, 20, 12));
        assert_eq!(element_badge(7, BADGE_ROW_PLAIN), (230, 192, 20, 12));
    }

    #[test]
    fn label_sprites_are_the_pinned_system_ui_rects() {
        assert_eq!(title_pak::OVERLAY_SYSTEM_UI_LABEL_LV, (192, 86, 16, 10));
        assert_eq!(title_pak::OVERLAY_SYSTEM_UI_LABEL_HP, (208, 86, 16, 10));
        assert_eq!(title_pak::OVERLAY_SYSTEM_UI_LABEL_MP, (224, 86, 16, 10));
        assert_eq!(title_pak::OVERLAY_SYSTEM_UI_LABEL_CLUT_ROW, SUBPAL_LABEL);
    }
}
