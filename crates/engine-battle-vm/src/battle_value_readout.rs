//! The multi-cast value readout and its UI teardown.
//!
//! PORT: FUN_801E805C
//!
//! Two jobs in one per-frame body, both keyed on the summon / multi-cast
//! side-band window at `0x801F69xx`:
//!
//! 1. **Teardown.** When the readout flag `DAT_8007B64C` is up, close the UI
//!    elements the finished cast left open - element `0x50` when the shared
//!    buffer `_DAT_8007BD14` is live, then, for each queued entry, the pair
//!    `(id, id - 4)` through `FUN_801D8DE8`.
//! 2. **Render.** For each populated slot in `_DAT_801F6988`, draw a label quad
//!    plus the slot's value as decimal digits, positioned off the slot's HP-bar
//!    widget at `ctx[+0x1074 + ctx[+0x11B6 + slot*0xC] * 4]`.
//!
//! Transcribed from the DISASSEMBLY in
//! `ghidra/scripts/funcs/overlay_battle_action_801e805c.txt` (1123
//! instructions). See
//! [`docs/reference/functions/battle.md`](../../docs/reference/functions/battle.md)
//! for the entry.
//!
//! ## What this module ports
//!
//! The kernels whose every instruction is accounted for: the digit split, the
//! teardown id pairing, the slot-to-widget indirection, and the label quad's
//! geometry. The per-digit quad placement walks a chain of reciprocal divides
//! interleaved with stores across ~700 instructions and is not reproduced.
//!
//! ## The sheet, read out of a retail frame
//!
//! [`GLYPH_TPAGE`] / [`GLYPH_CLUT`] are not just this routine's - they are the
//! whole battle value readout's, per-hit numerals included, and the sheet they
//! name is legible. A mednafen battle save state carries VRAM verbatim, so
//! decoding page `(448, 0)` at 4bpp through the CLUT at `(48, 476)` shows:
//!
//! | texels | content |
//! |---|---|
//! | `v = 64..=87` | ten 24x24 digit cells, in strip order **`1234567890`** |
//! | `u = 0..=55, v = 224..=239` | the `DAMAGE` label - [`label_quad`]'s rect |
//! | `u = 0..=31, v = 240..=255` | the `HIT` label |
//! | `u = 32..=79, v = 240..=255` | the `TOTAL` label |
//!
//! The strip starts at `1`, not `0`, so a digit's cell is
//! [`digit_cell_u`]: `((d + 9) % 10) * 24`. The page is inside the battle
//! effect atlas (PROT 870), which the battle loader already makes resident -
//! the digits need no separate asset.
//!
//! ## The per-hit floating numeral
//!
//! The same sheet carries the numeral a landed hit throws, and the display
//! list pins its geometry directly. In `battle_melee_hit_spark` both frame
//! arenas are live in one RAM image, so the pair reads as an animation:
//!
//! | arena | cells | screen |
//! |---|---|---|
//! | earlier | `u = 0` and `u = 96` (= `15`) | `(118, 49)` and `(137, 49)`, each 18x19 |
//! | later | same cells | `(114, 32)` and `(137, 32)`, each 22x22 |
//!
//! Three laws fall out and are ported below: the run's horizontal **centre**
//! holds (136.5 in both), the cell **grows** toward its 1:1 24-px size, and
//! the run **rises** to a fixed screen row - `y = 32`, which is also where the
//! `battle_gimard_tail_fire` pair sits with its growth already finished
//! (cells 19, 20, 22 and 23 px at that same row, over a different monster at
//! a different `x`). Cell pitch is the cell width plus one in every sample.
//! The quads are `0x2C` at colour `0x808080` - opaque, unmodulated - so retail
//! does **not** fade the numeral out; it ends by no longer being emitted.
//!
//! The rate is pinned too, by the renderer itself: `FUN_801DF6B8` grows a
//! view-space square with the ring timer and rises it as it grows (see
//! [`popup_cells`] and the section that introduces it). Both hosts seat their
//! numerals through that kernel (`render-kernels::battle_numerals::
//! popup_value_cells`); [`value_cells`] is the older fixed-ramp layout,
//! kept for the dialog-font fallback's tests, with [`POP_FRAMES`] and
//! [`POP_START_CELL`] engine-chosen.
//!
//! # Wiring
//!
//! The multi-cast half above it - the teardown pass and the
//! `DAMAGE`/`HIT`/`TOTAL` combo cluster - stays unwired, and for a reason that
//! is neither the site nor the port. Retail calls this unconditionally at the
//! head of every action-SM tick (`jal 0x801e805c` at `0x801E2A70`, ahead of
//! the `ctx[+0x07]` jump), and that head is ported and live. It would have
//! nothing to say: the `0x801F6980` value window and the `0x801F6988` slot
//! list are written by the summon-overlay side band (PROT 0900 / the `readef`
//! streaming slots), which nothing in `engine-core` loads, so every slot reads
//! empty and both the teardown and the combo walk return nothing.

/// Readout slots the value window holds (`_DAT_801F6980..0x801F6987`, four
/// halfwords).
pub const VALUE_SLOTS: usize = 4;

/// Ordering-table tag word the quads carry (`0x09000000`).
pub const OT_TAG: u32 = 0x0900_0000;

/// Command + colour word every quad shares (`0x2C808080` - a textured quad at
/// neutral grey).
pub const CODE_COLOUR: u32 = 0x2C80_8080;

/// Quad size in bytes the primitive cursor advances by (`0x28`).
pub const PRIM_BYTES: usize = 0x28;

/// The UI element closed first when the shared buffer is live.
pub const SHARED_BUFFER_ELEMENT: u8 = 0x50;

/// Offset subtracted to make a queued entry's second teardown id.
pub const TEARDOWN_PAIR_DELTA: u8 = 4;

/// Stride of the per-slot widget-index records at `ctx[+0x11B6]` (`slot * 0xC`).
pub const SLOT_RECORD_STRIDE: usize = 0x0C;

/// Byte offset of the widget index inside the battle context (`+0x11B6`).
pub const SLOT_WIDGET_INDEX: usize = 0x11B6;

/// Byte offset of the widget pointer array inside the battle context
/// (`+0x1074`).
pub const WIDGET_TABLE: usize = 0x1074;

/// Does the teardown pass run?
///
/// Retail's condition is the readout flag up **and** at least one of the four
/// value halfwords non-zero. A flag with an all-zero window skips both the
/// batch and the flag clear, which is what leaves the flag standing for a later
/// frame.
pub fn teardown_runs(readout_flag: u8, values: [u16; VALUE_SLOTS]) -> bool {
    readout_flag != 0 && values.iter().any(|&v| v != 0)
}

/// The `(id, id - 4)` pair each queued entry closes.
///
/// Retail issues `FUN_801D8DE8(id, 0)` and then `FUN_801D8DE8(id - 4, 0)`; the
/// subtraction is `addiu a0, a0, -0x4` on a byte the load zero-extended, so it
/// is a 32-bit subtract that can go negative rather than a byte wrap. The port
/// keeps the signed form.
pub const fn teardown_pair(id: u8) -> (u8, i32) {
    (id, id as i32 - TEARDOWN_PAIR_DELTA as i32)
}

/// The queued entry ids for a batch of `count` entries.
///
/// Retail indexes `_DAT_801F6834 + (count - 1) * 4 + i`, so the *row* is chosen
/// by the count and the entries run along it. A zero count skips the loop.
pub const fn teardown_row_offset(count: u32, index: u32) -> Option<u32> {
    if count == 0 {
        None
    } else {
        Some((count - 1) * 4 + index)
    }
}

/// The widget-pointer index for a readout slot: two indirections, a `0xC`-stride
/// byte record then a word table.
///
/// Returns the byte offset into the battle context that holds the widget
/// pointer, given the byte already read from `ctx[+0x11B6 + slot * 0xC]`.
pub const fn widget_offset(widget_index: u8) -> usize {
    WIDGET_TABLE + widget_index as usize * 4
}

/// The byte offset of a slot's widget-index byte.
pub const fn slot_widget_index_offset(slot: usize) -> usize {
    SLOT_WIDGET_INDEX + slot * SLOT_RECORD_STRIDE
}

/// A decimal split of a readout value, most significant digit first, with
/// leading zeros dropped.
///
/// Retail reaches the digits through reciprocal multiplies rather than `div`:
/// `0xD1B71759` with `mfhi >> 13` is `/ 10000`, and `0xCCCCCCCD` with
/// `mfhi >> 3` is `/ 10`. The ten-thousands digit is tested for zero before
/// anything is drawn, which is what suppresses the leading zero.
pub fn decimal_digits(value: u16) -> Vec<u8> {
    let mut digits = [0u8; 5];
    let mut v = value as u32;
    for slot in digits.iter_mut().rev() {
        *slot = (v % 10) as u8;
        v /= 10;
    }
    let first = digits.iter().position(|&d| d != 0).unwrap_or(4);
    digits[first..].to_vec()
}

/// The two reciprocal divides retail uses, as their own functions, so the
/// magic-constant arithmetic is checkable against plain division.
pub mod reciprocal {
    /// `multu v, 0xD1B71759; mfhi; srl 13` - unsigned divide by 10000.
    pub const fn div10000(v: u32) -> u32 {
        (((v as u64) * 0xD1B7_1759) >> 32) as u32 >> 13
    }

    /// `multu v, 0xCCCCCCCD; mfhi; srl 3` - unsigned divide by 10.
    pub const fn div10(v: u32) -> u32 {
        (((v as u64) * 0xCCCC_CCCD) >> 32) as u32 >> 3
    }
}

/// A textured quad the readout links into the ordering table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReadoutQuad {
    /// `+0x00` - ordering-table tag.
    pub tag: u32,
    /// `+0x04` - command and colour.
    pub code_colour: u32,
    /// `+0x08`, `+0x10`, `+0x18`, `+0x20` - screen XY per corner.
    pub xy: [(i16, i16); 4],
    /// `+0x0C`, `+0x14`, `+0x1C`, `+0x24` - texel UV per corner.
    pub uv: [(u8, u8); 4],
    /// `+0x0E` - CLUT word.
    pub clut: u16,
    /// `+0x16` - texture page.
    pub tpage: u16,
}

/// CLUT the label and digit glyphs sample (`0x7703` - VRAM `(48, 476)`).
pub const GLYPH_CLUT: u16 = 0x7703;

/// Texture page the label and digit glyphs sample (`0x27` - VRAM page
/// `(448, 0)` at 4bpp, inside the battle effect atlas).
pub const GLYPH_TPAGE: u16 = 0x27;

/// Texel row the ten digit cells start on.
pub const DIGIT_ROW_V: u8 = 64;

/// One digit cell is square, this many texels on a side.
pub const DIGIT_CELL: u8 = 24;

/// Screen pitch between adjacent cells is the drawn cell width plus this.
///
/// Zero: retail's pitch **is** the drawn width, because the drawn width is
/// inclusive. A readout quad's far corner sits at `x + w - 1`
/// (`render-kernels::battle_numerals::readout_quad`), so consecutive cells laid
/// `w` apart abut exactly. The two constants are one fact - a one-pixel gap
/// here with an exclusive far corner there overlapped every pair of digits by
/// a column and then spaced the run a column too wide.
pub const DIGIT_GAP: i32 = 0;

/// Screen row the floating numeral rises to and rests on.
pub const RESTING_TOP_Y: i32 = 32;

/// Frames the pop-in growth and the rise take. **Engine-chosen** - the
/// captures pin the endpoints, not the rate.
pub const POP_FRAMES: u16 = 8;

/// Drawn cell size the pop starts at, in screen pixels. The smallest measured
/// cell is 18; retail's own start frame is not captured.
pub const POP_START_CELL: u32 = 16;

/// Inclusive texel rect of the `DAMAGE` label (the one [`label_quad`] draws).
pub const LABEL_DAMAGE: (u8, u8, u8, u8) = (0x00, 0xE0, 0x37, 0xEF);

/// Inclusive texel rect of the `HIT` label.
pub const LABEL_HIT: (u8, u8, u8, u8) = (0x00, 0xF0, 0x1F, 0xFF);

/// Inclusive texel rect of the `TOTAL` label.
pub const LABEL_TOTAL: (u8, u8, u8, u8) = (0x20, 0xF0, 0x4F, 0xFF);

/// Left texel of a decimal digit's cell.
///
/// The strip runs `1234567890`, so `1` is the first cell and `0` the last -
/// `((d + 9) % 10) * 24`. Read straight off the decoded sheet, and
/// corroborated by the two-cell `15` in `battle_melee_hit_spark` landing on
/// `u = 0` and `u = 96`.
pub const fn digit_cell_u(digit: u8) -> u8 {
    ((digit + 9) % 10) * DIGIT_CELL
}

/// One drawn digit of a floating value readout.
///
/// Screen fields are stage pixels on retail's 320x240 stage; texel fields
/// address [`GLYPH_TPAGE`] through [`GLYPH_CLUT`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ValueCell {
    /// The decimal digit this cell shows.
    pub digit: u8,
    /// Stage-pixel top-left corner.
    pub x: i32,
    pub y: i32,
    /// Stage-pixel extent (square while the pop is running).
    pub w: u32,
    pub h: u32,
    /// Texel top-left corner on the glyph page.
    pub u: u8,
    pub v: u8,
    /// Texel extent - always [`DIGIT_CELL`]; the screen extent is what scales.
    pub cell: u8,
}

/// Drawn cell size `age` frames into the pop, in screen pixels.
///
/// Grows linearly from [`POP_START_CELL`] to the 1:1 [`DIGIT_CELL`] over
/// [`POP_FRAMES`] and then holds. The endpoints are measured; the ramp
/// between them is engine-chosen.
pub fn pop_cell_px(age: u16) -> u32 {
    let end = u32::from(DIGIT_CELL);
    if age >= POP_FRAMES {
        return end;
    }
    let span = end - POP_START_CELL;
    POP_START_CELL + span * u32::from(age) / u32::from(POP_FRAMES)
}

/// Top edge `age` frames into the pop: rises from `start_y` to
/// [`RESTING_TOP_Y`] over [`POP_FRAMES`], then holds. A numeral that starts at
/// or above the resting row never moves.
pub fn pop_top_y(start_y: i32, age: u16) -> i32 {
    if start_y <= RESTING_TOP_Y {
        // Already at or above the resting row - the rise never pushes down.
        return start_y;
    }
    if age >= POP_FRAMES {
        return RESTING_TOP_Y;
    }
    let travel = start_y - RESTING_TOP_Y;
    start_y - travel * i32::from(age) / i32::from(POP_FRAMES)
}

/// Lay a value out as floating digit cells.
///
/// `centre_x` is the stage column the run is centred on - retail holds the
/// centre fixed while the cells grow - and `start_y` the top edge the run pops
/// in at, which the rise walks up to [`RESTING_TOP_Y`]. `age` is frames since
/// the hit landed.
///
/// Leading zeros are dropped ([`decimal_digits`]), so a zero value still draws
/// a single `0` cell.
pub fn value_cells(value: u16, centre_x: i32, start_y: i32, age: u16) -> Vec<ValueCell> {
    let digits = decimal_digits(value);
    let size = pop_cell_px(age);
    let pitch = size as i32 + DIGIT_GAP;
    let run = pitch * digits.len() as i32 - DIGIT_GAP;
    let left = centre_x - run / 2;
    let top = pop_top_y(start_y, age);
    digits
        .iter()
        .enumerate()
        .map(|(i, &d)| ValueCell {
            digit: d,
            x: left + i as i32 * pitch,
            y: top,
            w: size,
            h: size,
            u: digit_cell_u(d),
            v: DIGIT_ROW_V,
            cell: DIGIT_CELL,
        })
        .collect()
}

// ---------------------------------------------------------------------------
// The per-hit numeral's own renderer - `FUN_801DF6B8`
// ---------------------------------------------------------------------------
//
// The floating numeral a landed hit throws is drawn by `FUN_801DF6B8`, which
// walks the 8-slot popup ring (`ctx[+0x83C]` value, `+0x318` seat, `+0x85C`
// timer; `battle_gauge_rearm::DamagePopupRing`) once per frame. Per live
// entry (`0x801DF700..0x801DFDA4`):
//
// 1. the anchor is the struck actor's display trio `+0x3C / +0x3E / +0x40`
//    with Y replaced by `+0x3E / 2 - timer * 3 / 2` (both divides truncate),
//    read with the timer **before** this frame's step;
// 2. the timer steps by `0x10 * DAT_1F800393` (`0x801DF7B8..0x801DF7C8`);
// 3. `FUN_800195A8` projects a view-space square of half-extent
//    `timer / 2` about the anchor (`sll 0xF; sra 0x10` at `0x801DF810`), so
//    the numeral grows as it rises and its on-screen size is set by the
//    camera distance, not by a fixed pixel ramp;
// 4. the rect is widened to at least `clamp(timer >> 5, 1, 12)` and cut to
//    at most 24 px, moved right by half a pitch per extra digit, clamped to
//    `y >= 32`, `x <= 280` and `x >= 8 + 32 * extra digits`, and the digits
//    are laid right to left at a pitch of the corner span plus one;
// 5. once the stepped timer passes `0x240` the value is zeroed - the numeral
//    lives 37 frames at a frame step of one.
//
// `battle_vahn_tri_somersault_super` holds a fresh entry (`242`; the drawn
// frame's timer is `0x70`, the RAM word already stepped to `0x80`) and its
// display list draws three 3x3 cells at `x = 183, 187, 191`;
// `battle_melee_hit_spark`'s `15` is two 22-px cells resting on row 32.

/// The ring timer's step per frame at a frame step of one.
pub const POPUP_TIMER_STEP: i32 = 0x10;

/// Past this stepped timer the entry's value is zeroed after the draw.
pub const POPUP_TIMER_LIMIT: i32 = 0x240;

/// The widest corner span a numeral cell keeps (`slti 0x19`).
pub const POPUP_MAX_CELL: i32 = 24;

/// The cap on the timer's minimum-width gate (`timer >> 5`, at most 12).
pub const POPUP_MIN_CELL_CAP: i32 = 12;

/// The right clamp on the units cell's left x (`slti 0x119`).
pub const POPUP_MAX_X: i32 = 0x118;

/// The ring timer an entry `age` frames old is drawn with (`age = 0` is the
/// push frame), i.e. after that frame's step.
pub const fn popup_timer(age: u16) -> i32 {
    (age as i32 + 1) * POPUP_TIMER_STEP
}

/// Is an entry `age` frames old still drawn? The value is zeroed after the
/// first draw whose stepped timer exceeds [`POPUP_TIMER_LIMIT`].
pub const fn popup_live(age: u16) -> bool {
    popup_timer(age) - POPUP_TIMER_STEP <= POPUP_TIMER_LIMIT
}

/// The anchor and view-space half-extent `FUN_801DF6B8` projects for an
/// entry `age` frames old over an actor whose display trio is `trio`.
pub fn popup_anchor(trio: [i32; 3], age: u16) -> ([i32; 3], i32) {
    let before = popup_timer(age) - POPUP_TIMER_STEP;
    let y = trio[1] / 2 - (before * 3) / 2;
    let half = (popup_timer(age) << 15) >> 16;
    ([trio[0], y, trio[2]], half)
}

/// Lay a value out the way `FUN_801DF6B8` does from the projected square
/// `rect = (left, top, right, bottom)` in stage pixels at ring timer `timer`
/// ([`popup_timer`]). Cells come back left to right; a cell's far corner is
/// `x + w - 1`, so `w` is retail's corner span plus one.
// PORT: FUN_801DF6B8 (the per-entry layout `0x801DF870..0x801DFD78`; the
// anchor and timer are [`popup_anchor`] / [`popup_timer`], the projection the
// host's camera)
pub fn popup_cells(value: u16, rect: (i32, i32, i32, i32), timer: i32) -> Vec<ValueCell> {
    let digits = decimal_digits(value);
    let extra = digits.len() as i32 - 1;
    let (mut x0, mut y0, mut x1, mut y1) = rect;
    // Minimum width (`0x801DF870..0x801DF92C`).
    let floor = (timer >> 5).clamp(1, POPUP_MIN_CELL_CAP);
    if x1 - x0 < floor {
        let d = (floor - (x1 - x0)) / 2;
        x0 -= 1 + d;
        x1 += 1 + d;
        y0 -= 1 + d;
        y1 += 1 + d;
    }
    // Maximum width (`0x801DF930..0x801DF9C8`).
    if x1 - x0 > POPUP_MAX_CELL {
        let d = (x1 - x0 - POPUP_MAX_CELL) / 2;
        x0 += 1 + d;
        x1 -= 1 + d;
        y0 += 1 + d;
        y1 -= 1 + d;
    }
    let w = x1 - x0;
    let h = y1 - y0;
    // Half a pitch right per extra digit, in doubled coordinates
    // (`0x801DF9E4..0x801DFAB8`).
    x0 = i32::from((x0 * 2 + extra * (w + 1)) as i16) >> 1;
    // Clamps (`0x801DFADC..0x801DFB54`).
    if y0 < RESTING_TOP_Y {
        y0 = RESTING_TOP_Y;
    }
    if x0 > POPUP_MAX_X {
        x0 = POPUP_MAX_X;
    }
    x0 = x0.max(extra * 32 + 8);
    // The units digit at `x0`, each higher digit one pitch further left.
    let n = digits.len() as i32;
    digits
        .iter()
        .enumerate()
        .map(|(i, &d)| ValueCell {
            digit: d,
            x: x0 - (n - 1 - i as i32) * (w + 1),
            y: y0,
            w: (w + 1).max(0) as u32,
            h: (h + 1).max(0) as u32,
            u: digit_cell_u(d),
            v: DIGIT_ROW_V,
            cell: DIGIT_CELL,
        })
        .collect()
}

// ---------------------------------------------------------------------------
// The combo cluster - `N HIT` / `TOTAL x` and `DAMAGE x`
// ---------------------------------------------------------------------------
//
// The right-hand counter cluster a landed action puts up, read out of two
// retail display lists (a RAM image is the frame's packet stream):
//
// * `player_steal_skeleton_banner` (the frame after Vahn's art chain
//   resolves): `7703/0027` quads at `(240..263, 142..165)` - the 24x24 hit
//   count `5`, right edge 264; `(272..303, 152..167)` - the 32x16 `HIT`
//   label; `(216..263, 170..185)` - the 48x16 `TOTAL` label; and two 16x16
//   digit cells `7` / `2` at `(272, 168)` and `(288, 168)`.
// * `battle_gimard_tail_fire_a` (a monster cast mid-flight): the 56x16
//   `DAMAGE` label at `(208..263, 170..185)` and the same two 16x16 digit
//   seats carrying `16`.
// * `battle_melee_hit_spark` (mid-chain): the same `HIT`/`TOTAL` cluster
//   with every packet 40 px to the right - the cluster is mid-glide.
//
// So the value row is one law for both styles: 16x16 cells, 16-px pitch,
// ending at x = 304, and the label is not fixed: it sits a constant gap left
// of the value's first cell (`shiny_refactor_gimard_levelup`'s three-digit
// `DAMAGE 233` puts its label at 192, sixteen left of the two-digit seat; the
// `TOTAL` label is assumed to follow the same law). The hit count is a
// separate 24x24 run ending at x = 264 on row 142. The physical attack band
// gets `HIT` + `TOTAL`; a cast gets `DAMAGE` alone.
//
// The glide is retail's own and it is pinned, not chosen. The cluster hangs
// off screen-element placement record 80 (`0x80076C10 + 80 * 0x18`: seat A
// `(328, 170)`, seat B `(168, 170)`), opened at mode 0 - spawn at A, glide to
// B - and closed in the action SM's `0x51` band (`FUN_801D8DE8(0x50, 1)` at
// `0x801E6360`, gated on the damage finisher's `_DAT_8007BD14`). The glide
// stepper `FUN_801D9BBC` walks `ctx[+0x11B4 + slot * 0xC]`: `[total][elapsed]
// .. [target x][target y][start x][start y]`, position = start + (target -
// start) * elapsed / total, snapping on arrival. `battle_melee_hit_spark`
// carries that record live: total `0x10`, elapsed `0x0C`, start `(328, 168)`,
// target `(168, 168)`, and the handle at `x = 208` - which is exactly the
// `+40` every packet of the cluster shows. So the cluster slides 160 px in
// over 16 frames, linearly, and every label / cell seat below is measured
// against the settled anchor `(168, 168)`.

/// Right edge the hit-count cells end at (the 24x24 run on row
/// [`COMBO_COUNT_Y`]).
pub const COMBO_COUNT_RIGHT_X: i32 = 264;

/// Top row of the hit-count cells.
pub const COMBO_COUNT_Y: i32 = 142;

/// Screen seat of the `HIT` label (32x16, [`LABEL_HIT`]).
pub const COMBO_HIT_LABEL_SEAT: (i32, i32) = (272, 152);

/// Screen seat of the `TOTAL` label (48x16, [`LABEL_TOTAL`]).
pub const COMBO_TOTAL_LABEL_SEAT: (i32, i32) = (216, 170);

/// Screen seat of the `DAMAGE` label (56x16, [`LABEL_DAMAGE`]).
pub const COMBO_DAMAGE_LABEL_SEAT: (i32, i32) = (208, 170);

/// Right edge the value cells (`TOTAL` / `DAMAGE`) end at.
pub const COMBO_VALUE_RIGHT_X: i32 = 304;

/// Top row of the value cells.
pub const COMBO_VALUE_Y: i32 = 168;

/// Drawn size of one value cell - the 24-texel digit blitted at 16 px.
pub const COMBO_VALUE_CELL: u32 = 16;

/// Screen x the cluster's anchor (placement record 80) settles at - its
/// seat B, the glide target `battle_melee_hit_spark` carries.
pub const COMBO_ANCHOR_REST_X: i32 = 168;

/// Screen x the anchor spawns at - record 80's seat A, the glide start.
pub const COMBO_ANCHOR_START_X: i32 = 328;

/// Horizontal offset the cluster starts at before it settles: the anchor's
/// whole glide, `328 - 168`.
pub const COMBO_SLIDE_IN_X: i32 = COMBO_ANCHOR_START_X - COMBO_ANCHOR_REST_X;

/// Frames the anchor's glide record runs for - the `total` byte of the
/// live glide record in `battle_melee_hit_spark` (`0x10`).
pub const COMBO_SLIDE_FRAMES: u16 = 16;

/// Horizontal offset of the cluster `age` frames after it appeared -
/// `FUN_801D9BBC`'s linear step from the start seat to the target, holding
/// at `0` once the record has snapped.
///
/// PORT: FUN_801D9BBC (PROT 0898; the per-handle step, applied to record 80)
pub fn combo_slide(age: u16) -> i32 {
    if age >= COMBO_SLIDE_FRAMES {
        return 0;
    }
    // start + (target - start) * elapsed / total, expressed as the offset
    // that remains from the rest seat.
    COMBO_SLIDE_IN_X - COMBO_SLIDE_IN_X * i32::from(age) / i32::from(COMBO_SLIDE_FRAMES)
}

/// Which pair of labels the cluster wears.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ComboStyle {
    /// A physical / arts chain: the hit count over `HIT`, the running
    /// damage over `TOTAL`.
    HitTotal,
    /// A cast: `DAMAGE` and the value alone.
    Damage,
}

/// One label blit of the cluster: an inclusive texel rect on
/// [`GLYPH_TPAGE`] and its screen seat, drawn 1:1.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ComboLabel {
    /// Inclusive texel rect `(u0, v0, u1, v1)`.
    pub uv: (u8, u8, u8, u8),
    pub x: i32,
    pub y: i32,
    /// Which word the rect carries - the font fallback's text.
    pub word: &'static str,
}

impl ComboLabel {
    /// Drawn width / height (the rect is inclusive).
    pub const fn size(&self) -> (u32, u32) {
        (
            (self.uv.2 - self.uv.0) as u32 + 1,
            (self.uv.3 - self.uv.1) as u32 + 1,
        )
    }
}

/// The laid-out cluster: its labels and every digit cell.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ComboCluster {
    pub labels: Vec<ComboLabel>,
    pub cells: Vec<ValueCell>,
}

/// Right-aligned run of digit cells, `pitch` apart, the last cell's right
/// edge at `right_x`.
fn right_aligned_cells(value: u16, right_x: i32, y: i32, size: u32, pitch: i32) -> Vec<ValueCell> {
    let digits = decimal_digits(value);
    let n = digits.len() as i32;
    let left = right_x - (n - 1) * pitch - size as i32;
    digits
        .iter()
        .enumerate()
        .map(|(i, &d)| ValueCell {
            digit: d,
            x: left + i as i32 * pitch,
            y,
            w: size,
            h: size,
            u: digit_cell_u(d),
            v: DIGIT_ROW_V,
            cell: DIGIT_CELL,
        })
        .collect()
}

/// Lay the combo cluster out at its rest seats, shifted right by `slide`
/// (`0` once settled, [`COMBO_SLIDE_IN_X`] on the first frame).
///
/// `hits` is the landed-hit count (only drawn for [`ComboStyle::HitTotal`]);
/// `total` the running damage the value row shows. Cells past `u16::MAX`
/// saturate rather than wrap.
pub fn combo_cluster(style: ComboStyle, hits: u16, total: u32, slide: i32) -> ComboCluster {
    let total = total.min(u32::from(u16::MAX)) as u16;
    // The value row's label hugs the value: it moves one cell left per digit
    // past two (`shiny_refactor_gimard_levelup`'s `DAMAGE 233` sits its label
    // at `x = 192`, sixteen left of the two-digit seat), so the label's seat
    // constants name the two-digit layout.
    let label_shift = -(decimal_digits(total).len() as i32 - 2) * COMBO_VALUE_CELL as i32;
    let mut out = ComboCluster::default();
    match style {
        ComboStyle::HitTotal => {
            out.labels.push(ComboLabel {
                uv: LABEL_HIT,
                x: COMBO_HIT_LABEL_SEAT.0 + slide,
                y: COMBO_HIT_LABEL_SEAT.1,
                word: "HIT",
            });
            out.labels.push(ComboLabel {
                uv: LABEL_TOTAL,
                x: COMBO_TOTAL_LABEL_SEAT.0 + label_shift + slide,
                y: COMBO_TOTAL_LABEL_SEAT.1,
                word: "TOTAL",
            });
            // The count is the full-size 24-px cell, one gap apart like the
            // floating numeral's run.
            out.cells.extend(right_aligned_cells(
                hits,
                COMBO_COUNT_RIGHT_X + slide,
                COMBO_COUNT_Y,
                u32::from(DIGIT_CELL),
                i32::from(DIGIT_CELL) + DIGIT_GAP,
            ));
        }
        ComboStyle::Damage => {
            out.labels.push(ComboLabel {
                uv: LABEL_DAMAGE,
                x: COMBO_DAMAGE_LABEL_SEAT.0 + label_shift + slide,
                y: COMBO_DAMAGE_LABEL_SEAT.1,
                word: "DAMAGE",
            });
        }
    }
    out.cells.extend(right_aligned_cells(
        total,
        COMBO_VALUE_RIGHT_X + slide,
        COMBO_VALUE_Y,
        COMBO_VALUE_CELL,
        COMBO_VALUE_CELL as i32,
    ));
    out
}

/// The label quad, drawn once per readout pass (only for the first slot).
///
/// Geometry, straight off the stores at `0x801E8208..0x801E82AC`: the quad spans
/// `widget_x + 0x0A .. widget_x + 0x41` by `widget_y - 0x0E .. widget_y + 0x01`,
/// and samples the atlas rectangle `(0, 0xE0) .. (0x37, 0xEF)`. Both extents are
/// `0x37` wide, so the label is drawn 1:1.
pub fn label_quad(widget_x: u16, widget_y: u16) -> ReadoutQuad {
    let x0 = widget_x.wrapping_add(0x0A) as i16;
    let x1 = widget_x.wrapping_add(0x41) as i16;
    let y0 = widget_y.wrapping_sub(0x0E) as i16;
    let y1 = widget_y.wrapping_add(0x01) as i16;
    ReadoutQuad {
        tag: OT_TAG,
        code_colour: CODE_COLOUR,
        xy: [(x0, y0), (x1, y0), (x0, y1), (x1, y1)],
        uv: [(0x00, 0xE0), (0x37, 0xE0), (0x00, 0xEF), (0x37, 0xEF)],
        clut: GLYPH_CLUT,
        tpage: GLYPH_TPAGE,
    }
}

#[cfg(test)]
mod tests;
