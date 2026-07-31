//! **Arts command-input chrome** - the screen a party member's Arts
//! command puts up while entering directional commands, shared by every
//! host that draws it.
//!
//! Retail runs one input screen for the battle Arts command and for the
//! Muscle Dome's Attack command (the dome is a restricted normal battle:
//! same `FUN_801D0748` state `0x50` gauge-input arm, same `FUN_801D388C`
//! case-`9`/`0xB` accounting). This module is that one screen's geometry,
//! so the dome page and the battle hosts cannot drift apart.
//!
//! **Provenance.** Every rect, sub-palette and screen seat below is
//! byte-read out of a live dome input screen's captured GP0 packet ring
//! (savestate + scripted pad over the static recomp's debug TCP server,
//! cross-checked against a full-VRAM dump of the same moment) - see
//! `docs/subsystems/minigame-muscle-dome.md` § "Arts command input
//! (packet-pinned)". The source rects themselves live in
//! [`legaia_asset::title_pak`] (`OVERLAY_SYSTEM_UI_ARTS_*`); this module
//! holds the *composition*: which piece sits where on the 320x240 stage,
//! and how the entered buffer maps onto pennant seats.
//!
//! **What the screen is.** Four flat-topped hexagonal direction chips in
//! a diamond layout (High top, Left / Right flanking, Low bottom), each a
//! 24-wide body between two 15-wide pointed caps with a baked label strip
//! and a pointed diamond end at each tip, and a D-pad glyph in the middle.
//! Along the bottom runs the **input bar**: one gold-capped trough sized
//! to the AP pool, over whose left end the already-entered commands stamp
//! as **pennants** (a label strip between two 9x18 caps). The bar is a
//! single primitive - the "blue entered / red remaining" two-tone a
//! screenshot reads is the pennants covering the trough's left portion,
//! not a second bar. Bottom-right sits the **AP plate**, the same
//! cap / trough / fill / value-box pieces the status screen's AP gauge
//! draws (system-UI CLUT row 4); it reads the caster's Spirit gauge and
//! does **not** drain as commands are entered.
//!
//! REF: FUN_801D0748
//! REF: FUN_801D388C

use crate::{SpriteDraw, TextDraw};
use legaia_asset::title_pak;

// ------------------------------------------------------------ screen seats

/// Stage-space body anchor of each direction chip. Order matches
/// [`ChipDirection`]. The caps sit at body `x - 15` / `x + 24`, the label
/// strip at body `+ (0, 4)`, the diamond ends at body `x - 9` / `x + 24`,
/// `y + 4`.
pub const CHIP_ANCHOR_HIGH: (i32, i32) = (216, 26);
pub const CHIP_ANCHOR_LEFT: (i32, i32) = (176, 58);
pub const CHIP_ANCHOR_RIGHT: (i32, i32) = (256, 58);
pub const CHIP_ANCHOR_LOW: (i32, i32) = (216, 90);

/// Stage seat of the 16x16 D-pad glyph in the middle of the chip diamond
/// (captured FT4 `(220,62)-(235,77)`, so a 15x15 draw).
pub const DPAD_SEAT: (i32, i32) = (220, 62);
/// Drawn size of the D-pad glyph (the FT4 spans 15 texels, not 16).
pub const DPAD_SIZE: (u32, u32) = (15, 15);

/// Stage `y` of the input bar and of every settled pennant.
pub const BAR_Y: i32 = 188;
/// Stage `x` the input bar starts at.
pub const BAR_X: i32 = 0;
/// Captured bar length, in stage px, for the reference pool
/// [`BAR_REFERENCE_POOL`].
pub const BAR_W_AT_REFERENCE_POOL: i32 = 128;
/// AP pool the captured bar length was measured at.
pub const BAR_REFERENCE_POOL: u16 = 100;
/// Shortest bar drawn, so a tiny pool still reads as a bar.
pub const BAR_W_MIN: i32 = 48;

/// Stage `x` of pennant slot 0. Later slots sit at
/// `PENNANT_X0 + sum(AP spent before them)` - the captured seat law
/// (pitch 30 at the favored 30-AP command cost).
pub const PENNANT_X0: i32 = 7;

/// Stage top-left of the AP plate.
pub const AP_PLATE_SEAT: (i32, i32) = (208, 172);
/// Stage span of the AP plate's gouraud fill at a full gauge
/// (`x 235..285`, `y 177..183`).
pub const AP_FILL_X: i32 = 235;
pub const AP_FILL_W: i32 = 50;
pub const AP_FILL_Y: i32 = 177;
pub const AP_FILL_H: i32 = 6;

/// Arts-list window rect (Triangle), `(x, y, w, h)` in stage px -
/// captured `(6,28)`-`(160,188)`.
pub const LIST_WINDOW: (i32, i32, i32, i32) = (6, 28, 154, 160);
/// First list row's text baseline and the row pitch (`y = 36 + 30n`).
pub const LIST_ROW_Y0: i32 = 36;
pub const LIST_ROW_PITCH: i32 = 30;
/// Right edge the per-row AP cost is right-aligned to end at.
pub const LIST_COST_RIGHT_X: i32 = 152;
/// Command-string glyph origin inside a row (`(44 + 12k, y + 14)`).
pub const LIST_CMD_X0: i32 = 44;
pub const LIST_CMD_PITCH: i32 = 12;

/// Stage seat of the **Begin | Reselect** pick's first row, and its row
/// pitch. Screenshot-read, not packet-pinned (the capture covers the
/// entry screen; the review / Begin screens' piece decomposition is
/// still open - see the doc's "Still unpinned here").
pub const BEGIN_MENU_SEAT: (i32, i32) = (24, 144);
pub const BEGIN_MENU_PITCH_Y: i32 = 16;

/// The four entry directions, in the order the chips read on screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChipDirection {
    High,
    Left,
    Right,
    Low,
}

impl ChipDirection {
    /// Map a `legaia_art::queue::Command` byte (Left 1, Right 2, Down 3,
    /// Up 4) onto its chip. `None` for anything outside that space.
    pub fn from_command_byte(b: u8) -> Option<Self> {
        match b {
            1 => Some(Self::Left),
            2 => Some(Self::Right),
            3 => Some(Self::Low),
            4 => Some(Self::High),
            _ => None,
        }
    }

    /// Stage body anchor of this chip.
    pub fn anchor(self) -> (i32, i32) {
        match self {
            Self::High => CHIP_ANCHOR_HIGH,
            Self::Left => CHIP_ANCHOR_LEFT,
            Self::Right => CHIP_ANCHOR_RIGHT,
            Self::Low => CHIP_ANCHOR_LOW,
        }
    }

    /// Sheet `v` of this chip's word in the 24x18 label strip at
    /// `u = OVERLAY_SYSTEM_UI_ARTS_LABEL_U`.
    pub fn label_v(self) -> u32 {
        match self {
            Self::High => title_pak::OVERLAY_SYSTEM_UI_ARTS_LABEL_V_HIGH,
            Self::Left => title_pak::OVERLAY_SYSTEM_UI_ARTS_LABEL_V_LEFT,
            Self::Right => title_pak::OVERLAY_SYSTEM_UI_ARTS_LABEL_V_RIGHT,
            Self::Low => title_pak::OVERLAY_SYSTEM_UI_ARTS_LABEL_V_LOW,
        }
    }
}

// ------------------------------------------------------------ atlas rects

/// Where each arts-input piece sits in the host's sprite atlas.
///
/// Both shipped hosts upload the same baked sheet
/// (`legaia_engine_core::save_menu_atlas`), so [`Self::BAKED`] is what
/// they pass; the struct stays explicit so a host sampling VRAM directly
/// (the browser dome page reads the widget page in texel space) can pass
/// the natural sheet rects instead via [`Self::SHEET`].
#[derive(Debug, Clone, Copy)]
pub struct ArtsInputAtlasRects {
    pub chip_body: (u32, u32, u32, u32),
    pub chip_cap_l: (u32, u32, u32, u32),
    pub chip_cap_r: (u32, u32, u32, u32),
    /// Top-left of the 24x18 label strip column; a word is the sub-rect
    /// at `(label_u, label_v0 + v)`.
    pub label_u: u32,
    pub label_w: u32,
    pub label_h: u32,
    pub diamond_l: (u32, u32, u32, u32),
    pub diamond_r: (u32, u32, u32, u32),
    pub pennant_cap_l: (u32, u32, u32, u32),
    pub pennant_cap_r: (u32, u32, u32, u32),
    pub dpad: (u32, u32, u32, u32),
    pub bar_end_l: (u32, u32, u32, u32),
    pub bar_body: (u32, u32, u32, u32),
    pub bar_arrow: (u32, u32, u32, u32),
}

impl ArtsInputAtlasRects {
    /// The pieces at their natural coordinates on the system-UI sheet.
    pub const SHEET: Self = Self {
        chip_body: title_pak::OVERLAY_SYSTEM_UI_ARTS_CHIP_BODY,
        chip_cap_l: title_pak::OVERLAY_SYSTEM_UI_ARTS_CHIP_CAP_L,
        chip_cap_r: title_pak::OVERLAY_SYSTEM_UI_ARTS_CHIP_CAP_R,
        label_u: title_pak::OVERLAY_SYSTEM_UI_ARTS_LABEL_U,
        label_w: title_pak::OVERLAY_SYSTEM_UI_ARTS_LABEL_W,
        label_h: title_pak::OVERLAY_SYSTEM_UI_ARTS_LABEL_H,
        diamond_l: title_pak::OVERLAY_SYSTEM_UI_ARTS_DIAMOND_L,
        diamond_r: title_pak::OVERLAY_SYSTEM_UI_ARTS_DIAMOND_R,
        // The pennant's left cap is the chip's own left diamond end.
        pennant_cap_l: title_pak::OVERLAY_SYSTEM_UI_ARTS_DIAMOND_L,
        pennant_cap_r: title_pak::OVERLAY_SYSTEM_UI_ARTS_PENNANT_CAP_R,
        dpad: title_pak::OVERLAY_SYSTEM_UI_ARTS_DPAD,
        bar_end_l: title_pak::OVERLAY_SYSTEM_UI_ARTS_BAR_END_L,
        bar_body: title_pak::OVERLAY_SYSTEM_UI_ARTS_BAR_BODY,
        bar_arrow: title_pak::OVERLAY_SYSTEM_UI_ARTS_BAR_ARROW,
    };

    /// The pieces as the shared save-menu atlas bakes them: identical to
    /// [`Self::SHEET`] except the chip triple, which is re-seated
    /// [`title_pak::OVERLAY_SYSTEM_UI_ARTS_CHIP_ATLAS_DY`] rows down out
    /// of the character-portrait strip's way.
    pub const BAKED: Self = Self {
        chip_body: title_pak::arts_chip_atlas_rect(title_pak::OVERLAY_SYSTEM_UI_ARTS_CHIP_BODY),
        chip_cap_l: title_pak::arts_chip_atlas_rect(title_pak::OVERLAY_SYSTEM_UI_ARTS_CHIP_CAP_L),
        chip_cap_r: title_pak::arts_chip_atlas_rect(title_pak::OVERLAY_SYSTEM_UI_ARTS_CHIP_CAP_R),
        ..Self::SHEET
    };

    /// Source sub-rect of one direction word in the label strip.
    pub fn label(&self, dir: ChipDirection) -> (u32, u32, u32, u32) {
        (self.label_u, dir.label_v(), self.label_w, self.label_h)
    }
}

// ------------------------------------------------------------------ frame

/// Which surface the input session is showing. Mirrors
/// `legaia_engine_core::arts_command_input::ArtsInputScreen`; kept local
/// so this crate stays a leaf under `engine-core`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtsInputScreen {
    Entering,
    Review,
    BeginMenu { cursor: u8 },
    Targeting,
}

/// Everything the chrome draws from. Hosts project their live session
/// into this and call the builders below.
#[derive(Debug, Clone, Copy)]
pub struct ArtsInputFrame<'a> {
    /// Entered command bytes in order (Left 1, Right 2, Down 3, Up 4).
    pub buffer: &'a [u8],
    /// AP paid per entered command, parallel to `buffer` - the pennant
    /// seat law is `PENNANT_X0 + sum(spent[..n])`.
    pub spent: &'a [u16],
    /// Remaining AP (unused by the bar, which sizes off `pool_max`).
    pub pool: u16,
    /// Seeded AP pool - the bar's length.
    pub pool_max: u16,
    /// Value the AP plate shows (retail: the caster's Spirit gauge).
    pub plate_value: u8,
    /// Open Triangle arts-list page, `None` when closed.
    pub list_page: Option<u8>,
    pub phase: ArtsInputScreen,
}

impl ArtsInputFrame<'_> {
    /// `true` while the direction chips + D-pad are up. Retail draws
    /// them only during entry - the review / Begin screens keep the bar
    /// and drop the chips.
    pub fn chips_visible(&self) -> bool {
        self.phase == ArtsInputScreen::Entering
    }

    /// Length of the input bar in stage px, scaled off the reference
    /// capture (`BAR_W_AT_REFERENCE_POOL` at `BAR_REFERENCE_POOL` AP).
    pub fn bar_width(&self) -> i32 {
        let scaled = (BAR_W_AT_REFERENCE_POOL as i64 * self.pool_max.max(1) as i64
            / BAR_REFERENCE_POOL as i64) as i32;
        scaled.max(BAR_W_MIN)
    }

    /// Stage `x` of committed pennant `slot`.
    pub fn pennant_x(&self, slot: usize) -> i32 {
        PENNANT_X0 + self.spent.iter().take(slot).map(|&c| c as i32).sum::<i32>()
    }
}

// --------------------------------------------------------------- builders

/// One arts-input chrome sprite list: the chips + D-pad (entry only), the
/// input bar, and every committed pennant. The AP plate is
/// [`arts_input_ap_plate_draws`] - it samples the status-gauge pieces a
/// host already carries, so it stays a separate call.
///
/// `stage_origin` / `stage_scale` follow the same convention as the rest
/// of this crate's chrome builders: stage pixels on the canonical 320x240
/// stage, multiplied out to the surface.
pub fn arts_input_chrome_draws(
    rects: &ArtsInputAtlasRects,
    frame: &ArtsInputFrame<'_>,
    stage_origin: (i32, i32),
    stage_scale: u32,
) -> Vec<SpriteDraw> {
    let scale = stage_scale.max(1);
    let white = [1.0, 1.0, 1.0, 1.0];
    let mut out: Vec<SpriteDraw> = Vec::new();
    let mut push = |src: (u32, u32, u32, u32), x: i32, y: i32, w: u32, h: u32| {
        out.push(SpriteDraw {
            dst: (
                stage_origin.0 + x * scale as i32,
                stage_origin.1 + y * scale as i32,
                w * scale,
                h * scale,
            ),
            src,
            color: white,
        });
    };

    // --- Input bar: pointed left end, tiled body, arrow right end. One
    // trough sized to the pool; the pennants below stamp over its left
    // portion, which is what reads as "entered" against "remaining".
    let bar_w = frame.bar_width();
    let end_w = rects.bar_end_l.2 as i32;
    let body_w = rects.bar_body.2 as i32;
    let arrow_w = rects.bar_arrow.2 as i32;
    push(
        rects.bar_end_l,
        BAR_X,
        BAR_Y,
        rects.bar_end_l.2,
        rects.bar_end_l.3,
    );
    let body_end = (BAR_X + bar_w - arrow_w).max(BAR_X + end_w);
    let mut x = BAR_X + end_w;
    while x < body_end {
        let w = body_w.min(body_end - x);
        let (sx, sy, _, sh) = rects.bar_body;
        push((sx, sy, w as u32, sh), x, BAR_Y, w as u32, sh);
        x += w;
    }
    push(
        rects.bar_arrow,
        body_end,
        BAR_Y,
        rects.bar_arrow.2,
        rects.bar_arrow.3,
    );

    // --- Committed pennants: cap + label + cap at the cost-weighted seat.
    for (slot, &b) in frame.buffer.iter().enumerate() {
        let Some(dir) = ChipDirection::from_command_byte(b) else {
            continue;
        };
        let px = frame.pennant_x(slot);
        let cap_w = rects.pennant_cap_l.2 as i32;
        push(
            rects.pennant_cap_l,
            px,
            BAR_Y,
            rects.pennant_cap_l.2,
            rects.pennant_cap_l.3,
        );
        let label = rects.label(dir);
        push(label, px + cap_w, BAR_Y, label.2, label.3);
        push(
            rects.pennant_cap_r,
            px + cap_w + label.2 as i32,
            BAR_Y,
            rects.pennant_cap_r.2,
            rects.pennant_cap_r.3,
        );
    }

    // --- Direction chips + D-pad glyph (entry phase only).
    if frame.chips_visible() {
        for dir in [
            ChipDirection::High,
            ChipDirection::Left,
            ChipDirection::Right,
            ChipDirection::Low,
        ] {
            let (bx, by) = dir.anchor();
            let body_w = rects.chip_body.2 as i32;
            let cap_l_w = rects.chip_cap_l.2 as i32;
            push(
                rects.chip_cap_l,
                bx - cap_l_w,
                by,
                rects.chip_cap_l.2,
                rects.chip_cap_l.3,
            );
            push(
                rects.chip_body,
                bx,
                by,
                rects.chip_body.2,
                rects.chip_body.3,
            );
            push(
                rects.chip_cap_r,
                bx + body_w,
                by,
                rects.chip_cap_r.2,
                rects.chip_cap_r.3,
            );
            let label = rects.label(dir);
            push(label, bx, by + 4, label.2, label.3);
            push(
                rects.diamond_l,
                bx - rects.diamond_l.2 as i32,
                by + 4,
                rects.diamond_l.2,
                rects.diamond_l.3,
            );
            push(
                rects.diamond_r,
                bx + body_w,
                by + 4,
                rects.diamond_r.2,
                rects.diamond_r.3,
            );
        }
        push(
            rects.dpad,
            DPAD_SEAT.0,
            DPAD_SEAT.1,
            DPAD_SIZE.0,
            DPAD_SIZE.1,
        );
    }

    out
}

/// Rects the AP plate needs out of a host's system-UI atlas. These are
/// the status screen's own AP-gauge pieces (CLUT row 4) - the input
/// screen's plate is the same widget, so a host passes what it already
/// has rather than baking a second copy.
#[derive(Debug, Clone, Copy)]
pub struct ApPlateRects {
    pub cap: (u32, u32, u32, u32),
    pub trough: (u32, u32, u32, u32),
    pub fill: (u32, u32, u32, u32),
    pub box_: (u32, u32, u32, u32),
    pub digits: (u32, u32, u32, u32),
}

/// The bottom-right AP plate: cap + trough, the gouraud fill scaled to
/// `frame.plate_value` out of 100, the value box, and the value's digits
/// out of the red 6x6 strip.
pub fn arts_input_ap_plate_draws(
    rects: &ApPlateRects,
    frame: &ArtsInputFrame<'_>,
    stage_origin: (i32, i32),
    stage_scale: u32,
) -> Vec<SpriteDraw> {
    let scale = stage_scale.max(1);
    let white = [1.0, 1.0, 1.0, 1.0];
    let mut out: Vec<SpriteDraw> = Vec::new();
    let mut push = |src: (u32, u32, u32, u32), x: i32, y: i32, w: u32, h: u32| {
        out.push(SpriteDraw {
            dst: (
                stage_origin.0 + x * scale as i32,
                stage_origin.1 + y * scale as i32,
                w * scale,
                h * scale,
            ),
            src,
            color: white,
        });
    };
    let (px, py) = AP_PLATE_SEAT;
    push(rects.cap, px, py, rects.cap.2, rects.cap.3);
    push(
        rects.trough,
        px + rects.cap.2 as i32,
        py,
        rects.trough.2,
        rects.trough.3,
    );
    // Gouraud fill: the captured span is x 235..285 at a full gauge, so a
    // value of `v` fills `v/100` of that span.
    let filled = (AP_FILL_W as i64 * frame.plate_value.min(100) as i64 / 100) as i32;
    if filled > 0 {
        push(
            rects.fill,
            AP_FILL_X,
            AP_FILL_Y,
            filled as u32,
            AP_FILL_H as u32,
        );
    }
    let box_x = px + rects.cap.2 as i32 + rects.trough.2 as i32;
    push(rects.box_, box_x, py, rects.box_.2, rects.box_.3);
    // Value digits, right-aligned inside the box.
    let (dx, dy, _, dh) = rects.digits;
    let digit_w = title_pak::OVERLAY_SYSTEM_UI_GAUGE_DIGIT_W;
    let pitch = title_pak::OVERLAY_SYSTEM_UI_GAUGE_DIGIT_PITCH;
    let text = frame.plate_value.to_string();
    let mut cx = box_x + rects.box_.2 as i32 - 3 - (text.len() as i32) * digit_w as i32;
    for ch in text.bytes() {
        let d = (ch - b'0') as u32;
        push((dx + d * pitch, dy, digit_w, dh), cx, py + 5, digit_w, dh);
        cx += digit_w as i32;
    }
    out
}

/// Text the input screen puts up alongside the sprite chrome: the
/// **Begin | Reselect** pick, when it is showing. Everything else on the
/// screen is baked art.
pub fn arts_input_text_draws(
    font: &legaia_font::Font,
    frame: &ArtsInputFrame<'_>,
    stage_origin: (i32, i32),
    stage_scale: u32,
) -> Vec<TextDraw> {
    let ArtsInputScreen::BeginMenu { cursor } = frame.phase else {
        return Vec::new();
    };
    let scale = stage_scale.max(1) as i32;
    let mut out = Vec::new();
    for (i, label) in ["Begin", "Reselect"].iter().enumerate() {
        let selected = i as u8 == cursor;
        let color = if selected {
            [1.0, 1.0, 1.0, 1.0]
        } else {
            [0.62, 0.66, 0.74, 1.0]
        };
        let text = if selected {
            format!("> {label}")
        } else {
            format!("  {label}")
        };
        let layout = font.layout_ascii(&text);
        let pen = (
            stage_origin.0 + BEGIN_MENU_SEAT.0 * scale,
            stage_origin.1 + (BEGIN_MENU_SEAT.1 + BEGIN_MENU_PITCH_Y * i as i32) * scale,
        );
        for g in &layout.glyphs {
            out.push(TextDraw {
                dst: (
                    pen.0 + g.dst_x * scale,
                    pen.1 + g.dst_y * scale,
                    g.width * scale as u32,
                    g.height * scale as u32,
                ),
                src: (g.atlas_x, g.atlas_y, g.width, g.height),
                color,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame<'a>(buffer: &'a [u8], spent: &'a [u16]) -> ArtsInputFrame<'a> {
        ArtsInputFrame {
            buffer,
            spent,
            pool: 40,
            pool_max: 100,
            plate_value: 68,
            list_page: None,
            phase: ArtsInputScreen::Entering,
        }
    }

    #[test]
    fn pennant_seats_follow_the_captured_cost_weighted_law() {
        let f = frame(&[4, 3, 4], &[30, 30, 30]);
        assert_eq!(f.pennant_x(0), 7);
        assert_eq!(f.pennant_x(1), 37, "pitch 30 at the favored cost");
        assert_eq!(f.pennant_x(2), 67);
        // An off-class arm widens the gap after it, exactly as the
        // "x = 7 + spent-before" law says.
        let g = frame(&[1, 4], &[42, 30]);
        assert_eq!(g.pennant_x(0), 7);
        assert_eq!(g.pennant_x(1), 49);
    }

    #[test]
    fn bar_scales_off_the_reference_capture_and_floors() {
        let mut f = frame(&[], &[]);
        assert_eq!(f.bar_width(), 128, "the captured 100-AP bar");
        f.pool_max = 50;
        assert_eq!(f.bar_width(), 64);
        f.pool_max = 4;
        assert_eq!(f.bar_width(), BAR_W_MIN, "floors so it still reads");
    }

    #[test]
    fn chips_draw_only_while_entering() {
        let mut f = frame(&[], &[]);
        let entering = arts_input_chrome_draws(&ArtsInputAtlasRects::BAKED, &f, (0, 0), 1).len();
        f.phase = ArtsInputScreen::Review;
        let review = arts_input_chrome_draws(&ArtsInputAtlasRects::BAKED, &f, (0, 0), 1).len();
        // Four chips x 6 pieces + the D-pad = 25 draws the review drops.
        assert_eq!(entering - review, 25);
    }

    #[test]
    fn each_entered_command_stamps_a_three_piece_pennant() {
        let empty =
            arts_input_chrome_draws(&ArtsInputAtlasRects::BAKED, &frame(&[], &[]), (0, 0), 1).len();
        let two = arts_input_chrome_draws(
            &ArtsInputAtlasRects::BAKED,
            &frame(&[4, 3], &[30, 30]),
            (0, 0),
            1,
        )
        .len();
        assert_eq!(two - empty, 6, "cap + label + cap per entered command");
    }

    #[test]
    fn the_baked_atlas_moves_only_the_chip_triple() {
        let s = ArtsInputAtlasRects::SHEET;
        let b = ArtsInputAtlasRects::BAKED;
        assert_eq!(b.chip_body.1, s.chip_body.1 + 34);
        assert_eq!(b.chip_cap_l.1, s.chip_cap_l.1 + 34);
        assert_eq!(b.chip_cap_r.1, s.chip_cap_r.1 + 34);
        assert_eq!(b.dpad, s.dpad);
        assert_eq!(b.bar_body, s.bar_body);
        assert_eq!(b.diamond_l, s.diamond_l);
        assert_eq!(b.label_u, s.label_u);
    }

    #[test]
    fn stage_scale_multiplies_every_seat() {
        let f = frame(&[4], &[30]);
        let one = arts_input_chrome_draws(&ArtsInputAtlasRects::BAKED, &f, (0, 0), 1);
        let two = arts_input_chrome_draws(&ArtsInputAtlasRects::BAKED, &f, (10, 20), 2);
        assert_eq!(one.len(), two.len());
        for (a, b) in one.iter().zip(two.iter()) {
            assert_eq!(b.dst.0, 10 + a.dst.0 * 2);
            assert_eq!(b.dst.1, 20 + a.dst.1 * 2);
            assert_eq!(b.dst.2, a.dst.2 * 2);
            assert_eq!(b.dst.3, a.dst.3 * 2);
            assert_eq!(b.src, a.src);
        }
    }
}
