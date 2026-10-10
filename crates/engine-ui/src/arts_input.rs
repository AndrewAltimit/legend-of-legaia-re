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

use crate::SpriteDraw;
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
///
/// The battle **command-select** diamond (Item / Attack / element / Spirit)
/// is a different cluster with different art, but its capture puts its
/// D-pad at this same rect and its centre at the same `(228, 70)` these
/// four chip anchors imply - two independent captures agreeing, which is
/// what says the arts entry reuses the command diamond's seat rather than
/// merely sitting near it. Do not confuse the two clusters: this one
/// carries High / Left / Right / Low.
pub const DPAD_SEAT: (i32, i32) = (220, 62);
/// Drawn size of the D-pad glyph (the FT4 spans 15 texels, not 16).
pub const DPAD_SIZE: (u32, u32) = (15, 15);

/// Stage `y` of the input bar and of every settled pennant.
pub const BAR_Y: i32 = 188;
/// Stage `x` the input bar starts at.
pub const BAR_X: i32 = 0;
/// Captured bar length, in stage px, for the reference pool
/// [`BAR_REFERENCE_POOL`]: the `94`-wide record body between the two end
/// pieces ([`BAR_ENDS_W`]).
pub const BAR_W_AT_REFERENCE_POOL: i32 = 128;
/// AP pool the captured bar length was measured at.
pub const BAR_REFERENCE_POOL: u16 = 100;
/// Combined width of the bar's two end pieces - the 16-wide pointed left
/// end and the 18-wide arrow - which frame the record body on either side.
pub const BAR_ENDS_W: i32 = title_pak::OVERLAY_SYSTEM_UI_ARTS_BAR_END_L.2 as i32
    + title_pak::OVERLAY_SYSTEM_UI_ARTS_BAR_ARROW.2 as i32;
/// Shortest bar drawn: the two end pieces with no body.
pub const BAR_W_MIN: i32 = BAR_ENDS_W;

/// Stage `x` the **pennant anchor** of slot 0 sits at - the cursor
/// `ctx[+0x6D8]`, seeded to 16 on the gauge build (`0x801D3A3C` /
/// `0x801D3A44`) and advanced by each committed command's own `+0x74` cost
/// (`0x801D3D68..0x801D3D74`), not by a fixed pitch.
pub const PENNANT_ANCHOR_X0: i32 = 16;

/// Stage `x` of pennant slot 0's drawn left edge. The pennant record is
/// anchored at [`PENNANT_ANCHOR_X0`] and its left cap is drawn one cap width
/// to the left (`16 - 9`), which is the captured `7`.
pub const PENNANT_X0: i32 = PENNANT_ANCHOR_X0 - 9;

/// The AP cost every width in this screen is measured against: retail
/// subtracts it once, in `FUN_801D388C` case `9`, at `0x801D3B6C` and
/// `0x801D3B98`. `30` is that subtraction's zero point - not a threshold and
/// not a table index.
pub const CHIP_COST_ZERO_POINT: i32 = 30;

/// A record's drawn width is its command's cost minus this bias
/// (`addiu v0,v0,-0x6` then `sh v0,0x6(a1)` at `0x801D3B44`). At the favored
/// 30-AP cost that is the captured 24-wide chip body and label strip.
pub const COST_WIDTH_BIAS: i32 = 6;

/// Per-seat widening coefficient `DAT_8007B650` (SCUS file `0x6BE50`,
/// immediately followed by the `Auto` / `Command` strings, which is what
/// fixes the file/VA pairing). Seat order is the 12-byte-stride seat array's
/// at `0x80076BBC`: Left, High, Low, Right.
///
/// A chip's seat `x` is `seat_x - (cost - 30) * K / 2`, so `K` makes the arm
/// chips grow *away* from the D-pad glyph between them: the Left chip's right
/// edge stays pinned at 200, the two centre chips stay centred on 228, and
/// the Right chip's left edge stays pinned at 256.
pub const CHIP_WIDEN_K: [i32; 4] = [2, 1, 1, 0];

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

    /// This chip's index into the retail 12-byte-stride seat array at
    /// `0x80076BBC` and into [`CHIP_WIDEN_K`]: the case-`9` loop walks
    /// `0x0C` (Left), `0x0F` (High), `0x0E` (Low), `0x0D` (Right), whose
    /// seat `x` values `176 / 216 / 216 / 256` are exactly the four
    /// `CHIP_ANCHOR_*` abscissas below.
    pub fn seat_slot(self) -> usize {
        match self {
            Self::Left => 0,
            Self::High => 1,
            Self::Low => 2,
            Self::Right => 3,
        }
    }

    /// This chip's index in **Command-byte order** (`Left, Right, Down, Up`
    /// = action slots `0x0C..=0x0F`) - the order the disc's per-character
    /// swing-cost row is keyed by, so a host passes its cost row verbatim.
    /// Not the same as [`Self::seat_slot`], which is the screen's.
    pub fn command_index(self) -> usize {
        match self {
            Self::Left => 0,
            Self::Right => 1,
            Self::Low => 2,
            Self::High => 3,
        }
    }

    /// Stage body anchor of this chip at the favored 30-AP cost. Use
    /// [`ArtsInputFrame::chip_anchor`] for the cost-shifted seat.
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

    /// Source sub-rect of the word a chip-label icon id names
    /// ([`chip_icon_label_v`]); `fallback` is used for an id outside the
    /// six chip words.
    pub fn label_for_icon(&self, icon: u8, fallback: ChipDirection) -> (u32, u32, u32, u32) {
        let v = chip_icon_label_v(icon).unwrap_or(fallback.label_v());
        (self.label_u, v, self.label_w, self.label_h)
    }
}

/// Sheet `v` of the word a chip's window record names by its `+0x0E` icon
/// id: SCUS's icon table `0x800732A4` (12-byte records, `(u, v)` at `+4`;
/// the stride is the `idx * 12 + 0x800732A4` of `0x8002C7A0..0x8002C7AC`)
/// carries the six words at ids `0x0C..=0x11` in the order RaSeru, Arms,
/// Right, Left, High, Low, and repeats them for the pennant ids
/// `0x12..=0x17` (a pennant takes its chip's id `+ 6`). `None` for any
/// other id.
pub fn chip_icon_label_v(icon: u8) -> Option<u32> {
    let word = match icon {
        0x0C..=0x11 => icon - 0x0C,
        0x12..=0x17 => icon - 0x12,
        _ => return None,
    };
    Some(match word {
        0 => title_pak::OVERLAY_SYSTEM_UI_ARTS_LABEL_V_RASERU,
        1 => title_pak::OVERLAY_SYSTEM_UI_ARTS_LABEL_V_ARMS,
        2 => title_pak::OVERLAY_SYSTEM_UI_ARTS_LABEL_V_RIGHT,
        3 => title_pak::OVERLAY_SYSTEM_UI_ARTS_LABEL_V_LEFT,
        4 => title_pak::OVERLAY_SYSTEM_UI_ARTS_LABEL_V_HIGH,
        _ => title_pak::OVERLAY_SYSTEM_UI_ARTS_LABEL_V_LOW,
    })
}

// ------------------------------------------------------------------ frame

/// Which surface the input session is showing. Mirrors
/// `legaia_engine_core::arts_command_input::ArtsInputScreen`; kept local
/// so this crate stays a leaf under `engine-core`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtsInputScreen {
    Entering,
    Review,
    Targeting,
}

/// Everything the chrome draws from. Hosts project their live session
/// into this and call the builders below.
#[derive(Debug, Clone, Copy)]
pub struct ArtsInputFrame<'a> {
    /// Entered command bytes in order (Left 1, Right 2, Down 3, Up 4).
    pub buffer: &'a [u8],
    /// AP paid per entered command, parallel to `buffer` - the pennant
    /// anchor law is `PENNANT_ANCHOR_X0 + sum(spent[..n])`, and pennant `n`
    /// is `spent[n] - COST_WIDTH_BIAS` wide.
    pub spent: &'a [u16],
    /// This caster's AP cost for each of the four direction commands, in
    /// [`ChipDirection::command_index`] order (Left, Right, Down, Up = the
    /// action slots `0x0C..=0x0F`) - the per-(character, weapon)
    /// `DAT_801C9360[char][cmd] + 0x74` byte, which is exactly the row
    /// `ArtsInputView::costs` already carries. It sizes and re-seats the
    /// chips: an off-class command costs more than 30 and its chip is
    /// correspondingly wider, growing away from the D-pad.
    /// [`ArtsInputFrame::FAVORED_CHIP_COSTS`] is the all-30 case a host with
    /// no cost table can pass.
    pub chip_costs: [u16; 4],
    /// Each direction's chip-label icon id, in the same Command-byte order
    /// as [`Self::chip_costs`] - `legaia_engine_core`'s `retail_chip_icons`
    /// (the weapon arm reads `Arms`, the Ra-Seru arm `RaSeru`, an empty arm
    /// its plain direction word). A committed pennant wears its chip's word.
    /// [`ArtsInputFrame::PLAIN_CHIP_ICONS`] is the all-direction-words row.
    pub chip_icons: [u8; 4],
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

    /// Length of the input bar in stage px: the end pieces plus a body as
    /// wide as the pool less [`COST_WIDTH_BIAS`].
    ///
    /// The body is placement record `0x0F`'s width, which every writer sets
    /// to the gauge value less 6 - the entry build off the actor's AGL
    /// (`0x801D3A38`), the Spirit arm off the same AGL (`0x801E2F70`), and
    /// the Spirit band's grow toward the extended gauge less 6
    /// (`0x801E547C`). One pixel per AP: retail Spirit captures read a
    /// `188`-wide body under a `194` AGL and `199` under `205`, and the
    /// 100-AP entry capture's 128 px is `94 + 34`. (An earlier form scaled
    /// the capture linearly, `128 * pool / 100`, which only agreed at 100.)
    pub fn bar_width(&self) -> i32 {
        (self.pool_max as i32 - COST_WIDTH_BIAS).max(0) + BAR_ENDS_W
    }

    /// Stage `x` of committed pennant `slot`'s **drawn left edge** - the
    /// anchor cursor less one cap width.
    pub fn pennant_x(&self, slot: usize) -> i32 {
        self.pennant_anchor_x(slot) - 9
    }

    /// Stage `x` of committed pennant `slot`'s anchor: the cursor
    /// `ctx[+0x6D8]` after the preceding commands advanced it by their own
    /// costs.
    pub fn pennant_anchor_x(&self, slot: usize) -> i32 {
        PENNANT_ANCHOR_X0 + self.spent.iter().take(slot).map(|&c| c as i32).sum::<i32>()
    }

    /// Drawn width of committed pennant `slot` - its command's own cost less
    /// [`COST_WIDTH_BIAS`], copied from the pressed chip's record
    /// (`0x801D3D00` / `0x801D3D08`). The pennant itself has no cost
    /// special-case at all.
    pub fn pennant_w(&self, slot: usize) -> i32 {
        let cost = self.spent.get(slot).copied().unwrap_or(0) as i32;
        (cost - COST_WIDTH_BIAS).max(1)
    }

    /// This caster's cost for `dir`.
    pub fn chip_cost(&self, dir: ChipDirection) -> i32 {
        self.chip_costs[dir.command_index()] as i32
    }

    /// Drawn width of `dir`'s chip body - the same `cost - 6` law.
    pub fn chip_w(&self, dir: ChipDirection) -> i32 {
        (self.chip_cost(dir) - COST_WIDTH_BIAS).max(1)
    }

    /// Stage body anchor of `dir`'s chip at this caster's costs: the seat
    /// array's `x` re-seated by `(cost - 30) * K[slot] / 2`
    /// (`0x801D3B64..0x801D3B8C`; the `srl 31` / `addu` / `sra 1` triple is
    /// the signed divide-by-two, so the halving truncates toward zero).
    pub fn chip_anchor(&self, dir: ChipDirection) -> (i32, i32) {
        let (x, y) = dir.anchor();
        let shift =
            (self.chip_cost(dir) - CHIP_COST_ZERO_POINT) * CHIP_WIDEN_K[dir.seat_slot()] / 2;
        (x - shift, y)
    }

    /// The all-30 cost row - a favored-weapon caster, and the shape every
    /// captured screen was read at.
    pub const FAVORED_CHIP_COSTS: [u16; 4] = [30; 4];

    /// The plain direction words (Left, Right, Low, High) - icon ids
    /// `0x0F`, `0x0E`, `0x11`, `0x10`.
    pub const PLAIN_CHIP_ICONS: [u8; 4] = [0x0F, 0x0E, 0x11, 0x10];

    /// The atlas sub-rect of `dir`'s chip word for this caster.
    pub fn chip_label(
        &self,
        rects: &ArtsInputAtlasRects,
        dir: ChipDirection,
    ) -> (u32, u32, u32, u32) {
        rects.label_for_icon(self.chip_icons[dir.command_index()], dir)
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
        // The strip between the caps is the record's own width, `cost - 6` -
        // so an off-class command stamps a wider pennant, and the next one
        // starts that much further along the bar.
        let strip_w = frame.pennant_w(slot);
        push(
            rects.pennant_cap_l,
            px,
            BAR_Y,
            rects.pennant_cap_l.2,
            rects.pennant_cap_l.3,
        );
        let label = frame.chip_label(rects, dir);
        push(label, px + cap_w, BAR_Y, strip_w as u32, label.3);
        push(
            rects.pennant_cap_r,
            px + cap_w + strip_w,
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
            // Cost-shifted seat and cost-sized body: the one place the cost
            // geometry branches per direction.
            let (bx, by) = frame.chip_anchor(dir);
            let body_w = frame.chip_w(dir);
            let cap_l_w = rects.chip_cap_l.2 as i32;
            push(
                rects.chip_cap_l,
                bx - cap_l_w,
                by,
                rects.chip_cap_l.2,
                rects.chip_cap_l.3,
            );
            push(rects.chip_body, bx, by, body_w as u32, rects.chip_body.3);
            push(
                rects.chip_cap_r,
                bx + body_w,
                by,
                rects.chip_cap_r.2,
                rects.chip_cap_r.3,
            );
            let label = frame.chip_label(rects, dir);
            push(label, bx, by + 4, body_w as u32, label.3);
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

/// The Rot stamps over the rotted direction chips, for an acting member whose
/// `+0x16E` word is `status`: one sprite per stamp out of the atlas cell
/// `rot_stamp` (`BattleChromeRects::rot_stamp`, the 32x24 Rot stamp), in
/// retail's call order. Empty outside the entry phase - retail's phase-`0x50`
/// arm draws them right after the D-pad glyph, which is only up while the
/// chips are (`0x801D1D84..0x801D1E54`).
///
/// Each stamp is `FUN_801DBDDC`'s quad
/// ([`legaia_engine_vm::battle_party_panel::rot_stamp_on_arts_chip`]) at the
/// anchor [`legaia_engine_vm::battle_party_panel::arts_entry_rot_stamps`]
/// picks from the limb bits and this caster's chip costs, so a stamp keeps
/// the edge nearest the D-pad pinned as an off-class chip widens - the same
/// law the chips follow. Bit `0x08` stamps Left, `0x10` Right, `0x20` both
/// High and Low.
pub fn arts_input_rot_stamp_draws(
    rot_stamp: (u32, u32, u32, u32),
    frame: &ArtsInputFrame<'_>,
    status: u16,
    stage_origin: (i32, i32),
    stage_scale: u32,
) -> Vec<SpriteDraw> {
    use legaia_engine_vm::battle_party_panel as bpp;
    if !frame.chips_visible() {
        return Vec::new();
    }
    // `ctx[+0x14 + seat]` is in seat order (Left, High, Low, Right); the
    // frame carries the costs in Command-byte order (Left, Right, Down, Up).
    let byte = |dir: ChipDirection| frame.chip_costs[dir.command_index()].min(0xFF) as u8;
    let seat_costs = [
        byte(ChipDirection::Left),
        byte(ChipDirection::High),
        byte(ChipDirection::Low),
        byte(ChipDirection::Right),
    ];
    bpp::arts_entry_rot_stamps(status, seat_costs)
        .into_iter()
        .map(|(x, y, w)| {
            crate::battle_command_ui::mark_sprite(
                rot_stamp,
                bpp::rot_stamp_on_arts_chip(x, y, w),
                stage_origin,
                stage_scale,
            )
        })
        .collect()
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
    /// The plate's right tip (ICO `0x6A`, 8x16).
    pub tip: (u32, u32, u32, u32),
    /// The `100` glyph a full gauge shows in place of digits (16x6).
    pub full_mark: (u32, u32, u32, u32),
}

/// The bottom-right AP plate: cap + trough, the gouraud fill scaled to
/// `frame.plate_value` out of 100, the value box, the right tip, and the
/// value out of the red 6x6 strip (or the `100` glyph).
pub fn arts_input_ap_plate_draws(
    rects: &ApPlateRects,
    frame: &ArtsInputFrame<'_>,
    stage_origin: (i32, i32),
    stage_scale: u32,
) -> Vec<SpriteDraw> {
    ap_plate_draws(rects, frame.plate_value, stage_origin, stage_scale)
}

/// The AP plate at its pinned seat, showing `value` out of 100.
///
/// It is the status screen's gauge widget (staged kind `0x31`) and both
/// battle captures of it draw the same five sprites:
/// `nivora_duel_pre_megaton_press` (a full gauge) puts cap `(208, 172)`,
/// trough `(232, 172)`, box `(288, 172)`, tip `(304, 172)` and the `100`
/// glyph at `(288, 177)`; `v0_1_battle_command_submenu` (an empty one) the
/// same four frame pieces and the digit `0` at `(294, 177)`.
///
/// One widget, two owners: the arts-entry screen keeps it (sub-draw step 9
/// snaps placement record 82 in place, `52/3`) and the command ring is
/// where it first slides in (step 1, `52/0`, from `(328, 174)` to
/// `(208, 174)`), so the battle HUD builder draws the ring's copy through
/// this same body.
pub fn ap_plate_draws(
    rects: &ApPlateRects,
    value: u8,
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
    let filled = (AP_FILL_W as i64 * value.min(100) as i64 / 100) as i32;
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
    // The right tip closes the plate past the value box (anchor `+0x60`).
    push(
        rects.tip,
        box_x + rects.box_.2 as i32,
        py,
        rects.tip.2,
        rects.tip.3,
    );
    // The value, as the gauge widget lays it (`FUN_8002C0B0`, the status
    // screen's `ap_gauge_sprites`): a full gauge is the `100` glyph at the
    // box's left edge; below it the tens digit (when non-zero) sits there
    // and the ones digit one cell right.
    let value = value.min(100);
    if value == 100 {
        push(
            rects.full_mark,
            box_x,
            py + 5,
            rects.full_mark.2,
            rects.full_mark.3,
        );
    } else {
        let (dx, dy, _, dh) = rects.digits;
        let digit_w = title_pak::OVERLAY_SYSTEM_UI_GAUGE_DIGIT_W;
        let pitch = title_pak::OVERLAY_SYSTEM_UI_GAUGE_DIGIT_PITCH;
        let (tens, ones) = (u32::from(value) / 10, u32::from(value) % 10);
        if tens > 0 {
            push(
                (dx + tens * pitch, dy, digit_w, dh),
                box_x,
                py + 5,
                digit_w,
                dh,
            );
        }
        push(
            (dx + ones * pitch, dy, digit_w, dh),
            box_x + digit_w as i32,
            py + 5,
            digit_w,
            dh,
        );
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
            chip_costs: ArtsInputFrame::FAVORED_CHIP_COSTS,
            chip_icons: ArtsInputFrame::PLAIN_CHIP_ICONS,
            pool: 40,
            pool_max: 100,
            plate_value: 68,
            list_page: None,
            phase: ArtsInputScreen::Entering,
        }
    }

    /// The icon table's six words in id order, and the pennant ids `+ 6`
    /// naming the same words; a caster's `Arms` / `RaSeru` row reaches both
    /// the chips and the pennants.
    #[test]
    fn chip_icons_name_the_retail_words_on_chips_and_pennants() {
        use title_pak::*;
        let words = [
            OVERLAY_SYSTEM_UI_ARTS_LABEL_V_RASERU,
            OVERLAY_SYSTEM_UI_ARTS_LABEL_V_ARMS,
            OVERLAY_SYSTEM_UI_ARTS_LABEL_V_RIGHT,
            OVERLAY_SYSTEM_UI_ARTS_LABEL_V_LEFT,
            OVERLAY_SYSTEM_UI_ARTS_LABEL_V_HIGH,
            OVERLAY_SYSTEM_UI_ARTS_LABEL_V_LOW,
        ];
        for (i, &v) in words.iter().enumerate() {
            assert_eq!(chip_icon_label_v(0x0C + i as u8), Some(v));
            assert_eq!(chip_icon_label_v(0x12 + i as u8), Some(v));
        }
        assert_eq!(chip_icon_label_v(0x0B), None);
        // The plain row reproduces the per-direction words.
        let f = frame(&[], &[]);
        let r = ArtsInputAtlasRects::SHEET;
        for dir in [
            ChipDirection::Left,
            ChipDirection::Right,
            ChipDirection::Low,
            ChipDirection::High,
        ] {
            assert_eq!(f.chip_label(&r, dir), r.label(dir));
        }
        // Vahn's row: the Left chip and a committed Left pennant both read Arms.
        let mut f = frame(&[1], &[30]);
        f.chip_icons = [0x0D, 0x0C, 0x11, 0x10];
        let draws = arts_input_chrome_draws(&r, &f, (0, 0), 1);
        let arms = (r.label_u, OVERLAY_SYSTEM_UI_ARTS_LABEL_V_ARMS);
        let n = draws.iter().filter(|d| (d.src.0, d.src.1) == arms).count();
        assert_eq!(n, 2, "the Left chip and its pennant");
    }

    fn frame_costs<'a>(buffer: &'a [u8], spent: &'a [u16], costs: [u16; 4]) -> ArtsInputFrame<'a> {
        let mut f = frame(buffer, spent);
        f.chip_costs = costs;
        f
    }

    /// Retail's `FUN_801DBDDC` anchors are independent of the chip layout
    /// this module pins from a capture, yet every stamp lands on its chip:
    /// top edge on the chip's `y`, span covering the cost-sized body with a
    /// few pixels to spare - at the favoured cost and with off-class side
    /// chips alike.
    #[test]
    fn rot_stamps_cover_their_chip_bodies() {
        let cell = (64, 64, 32, 24);
        for costs in [ArtsInputFrame::FAVORED_CHIP_COSTS, [42, 38, 30, 30]] {
            let f = frame_costs(&[], &[], costs);
            let all = arts_input_rot_stamp_draws(cell, &f, 0x38, (0, 0), 1);
            assert_eq!(all.len(), 4);
            for (s, dir) in all.iter().zip([
                ChipDirection::Left,
                ChipDirection::Right,
                ChipDirection::High,
                ChipDirection::Low,
            ]) {
                let (bx, by) = f.chip_anchor(dir);
                let bw = f.chip_w(dir);
                let (x0, y0, w, h) = s.dst;
                assert_eq!((y0, h), (by, 24), "{dir:?} at {costs:?}");
                // High / Low are always stamped at the favoured width.
                if matches!(dir, ChipDirection::Left | ChipDirection::Right) {
                    assert!(x0 <= bx && x0 + w as i32 >= bx + bw, "{dir:?} at {costs:?}");
                }
                assert_eq!(s.src, cell);
            }
        }
        let f = frame(&[], &[]);
        assert!(arts_input_rot_stamp_draws(cell, &f, 0x1007, (0, 0), 1).is_empty());
        let mut review = frame(&[], &[]);
        review.phase = ArtsInputScreen::Review;
        assert!(
            arts_input_rot_stamp_draws(cell, &review, 0x38, (0, 0), 1).is_empty(),
            "no chips, no stamps"
        );
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
    fn pennant_width_is_the_commands_own_cost_less_six() {
        // Favored 30 -> the captured 24-wide strip.
        let f = frame(&[4], &[30]);
        assert_eq!(f.pennant_w(0), 24);
        // Off-class costs widen the same record linearly; the pennant has no
        // cost special-case of its own.
        let g = frame(&[1, 4], &[42, 36]);
        assert_eq!(g.pennant_w(0), 36);
        assert_eq!(g.pennant_w(1), 30);
        // ... and the next anchor advances by the previous cost, not by 30.
        assert_eq!(g.pennant_anchor_x(0), PENNANT_ANCHOR_X0);
        assert_eq!(g.pennant_anchor_x(1), PENNANT_ANCHOR_X0 + 42);
    }

    #[test]
    fn chip_seats_recentre_by_the_cost_delta_times_k_over_two() {
        // At the favored cost every chip sits on its captured anchor.
        let f = frame(&[], &[]);
        for dir in [
            ChipDirection::Left,
            ChipDirection::High,
            ChipDirection::Low,
            ChipDirection::Right,
        ] {
            assert_eq!(f.chip_anchor(dir), dir.anchor(), "{dir:?} at cost 30");
            assert_eq!(f.chip_w(dir), 24);
        }

        // Slot 0 (`K = 2`): the Left arm chip's RIGHT edge stays pinned at
        // 200 whatever the cost, growing leftward.
        for cost in [30u16, 36, 42, 48] {
            let g = frame_costs(&[], &[], [cost, 30, 30, 30]);
            let (x, _) = g.chip_anchor(ChipDirection::Left);
            assert_eq!(
                x + g.chip_w(ChipDirection::Left),
                200,
                "left chip right edge, cost {cost}"
            );
        }

        // Slots 1 and 2 (`K = 1`): High and Low stay centred on 228.
        for dir in [ChipDirection::High, ChipDirection::Low] {
            for cost in [30u16, 36, 42, 48] {
                let mut costs = ArtsInputFrame::FAVORED_CHIP_COSTS;
                costs[dir.command_index()] = cost;
                let g = frame_costs(&[], &[], costs);
                let (x, _) = g.chip_anchor(dir);
                assert_eq!(
                    x * 2 + g.chip_w(dir),
                    228 * 2,
                    "{dir:?} centre, cost {cost}"
                );
            }
        }

        // Slot 3 (`K = 0`): the Right chip's LEFT edge stays pinned at 256.
        for cost in [30u16, 36, 42, 48] {
            let g = frame_costs(&[], &[], [30, cost, 30, 30]);
            assert_eq!(g.chip_anchor(ChipDirection::Right).0, 256);
        }
    }

    #[test]
    fn the_seat_slot_order_is_the_retail_seat_arrays() {
        // `0x80076BBC`'s four rows read 176 / 216 / 216 / 256 at `+2`, and
        // that is exactly the four captured chip abscissas in this order.
        let seats = [176, 216, 216, 256];
        for dir in [
            ChipDirection::Left,
            ChipDirection::High,
            ChipDirection::Low,
            ChipDirection::Right,
        ] {
            assert_eq!(dir.anchor().0, seats[dir.seat_slot()], "{dir:?}");
        }
        assert_eq!(CHIP_WIDEN_K, [2, 1, 1, 0]);
    }

    #[test]
    fn bar_is_one_pixel_per_ap_between_its_end_pieces() {
        let mut f = frame(&[], &[]);
        assert_eq!(
            f.bar_width(),
            BAR_W_AT_REFERENCE_POOL,
            "the captured 100-AP bar"
        );
        // The retail Spirit captures: record 0x0F's body is AGL - 6.
        f.pool_max = 194;
        assert_eq!(f.bar_width(), 188 + BAR_ENDS_W);
        f.pool_max = 205;
        assert_eq!(f.bar_width(), 199 + BAR_ENDS_W);
        f.pool_max = 4;
        assert_eq!(f.bar_width(), BAR_W_MIN, "no body below the bias");
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

    /// The plate is the gauge widget's five sprites on the seats two retail
    /// display lists draw them at: a full gauge
    /// (`nivora_duel_pre_megaton_press`) closes with the tip at `(304, 172)`
    /// and shows the `100` glyph at `(288, 177)`; an empty one
    /// (`v0_1_battle_command_submenu`) shows its lone digit at `(294, 177)`.
    #[test]
    fn the_ap_plate_is_the_gauge_widgets_five_sprites() {
        let rects = ApPlateRects {
            cap: (0, 0, 24, 16),
            trough: (24, 0, 56, 16),
            fill: (0, 32, 1, 6),
            box_: (80, 0, 16, 16),
            digits: (0, 16, 60, 6),
            tip: (96, 0, 8, 16),
            full_mark: (0, 24, 16, 6),
        };
        let at = |draws: &[SpriteDraw], src: (u32, u32, u32, u32)| {
            draws
                .iter()
                .find(|d| d.src == src)
                .map(|d| (d.dst.0, d.dst.1))
        };
        let full = ap_plate_draws(&rects, 100, (0, 0), 1);
        assert_eq!(at(&full, rects.cap), Some((208, 172)));
        assert_eq!(at(&full, rects.trough), Some((232, 172)));
        assert_eq!(at(&full, rects.box_), Some((288, 172)));
        assert_eq!(at(&full, rects.tip), Some((304, 172)));
        assert_eq!(at(&full, rects.full_mark), Some((288, 177)));

        let empty = ap_plate_draws(&rects, 0, (0, 0), 1);
        assert_eq!(at(&empty, rects.tip), Some((304, 172)));
        assert_eq!(at(&empty, rects.full_mark), None);
        let zero = (
            rects.digits.0,
            rects.digits.1,
            title_pak::OVERLAY_SYSTEM_UI_GAUGE_DIGIT_W,
            rects.digits.3,
        );
        assert_eq!(at(&empty, zero), Some((294, 177)));

        // Two digits: tens on the box's left edge, ones one cell right.
        let mid = ap_plate_draws(&rects, 37, (0, 0), 1);
        let pitch = title_pak::OVERLAY_SYSTEM_UI_GAUGE_DIGIT_PITCH;
        let cell = |d: u32| (rects.digits.0 + d * pitch, zero.1, zero.2, zero.3);
        assert_eq!(at(&mid, cell(3)), Some((288, 177)));
        assert_eq!(at(&mid, cell(7)), Some((294, 177)));
    }
}
