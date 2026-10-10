//! The duel HUD renderer: vital bars and frames, round pips, combo, stage and high-score digits.

// ------------------------------------------------------------- HUD renderer

/// One VITAL fill bar, as the HUD renderer builds it: a raw gouraud quad, one
/// pixel of width per 32 HP.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VitalBar {
    pub x0: i16,
    pub y0: i16,
    pub x1: i16,
    pub y1: i16,
    /// Colour at the bar's far (empty) end.
    pub rgb_far: [u8; 3],
    /// Colour at the bar's anchored end.
    pub rgb_anchor: [u8; 3],
}

/// Top scanline of the VITAL bars (`0x26`).
pub const VITAL_Y0: i16 = 0x26;
/// Bottom scanline of the VITAL bars (`0x2B`).
pub const VITAL_Y1: i16 = 0x2B;
/// The player's bar is right-anchored here and fills leftward.
pub const VITAL_ANCHOR_LEFT: i16 = 0x89;
/// The opponent's bar is left-anchored here and fills rightward.
pub const VITAL_ANCHOR_RIGHT: i16 = 0xB8;
/// Red channel both bars hold flat (`0xBC`).
pub const VITAL_RED: u8 = 0xBC;

/// Arithmetic-shift-right rounding toward zero, the `bgez` + bias idiom the
/// renderer applies to every HP division.
fn sra_round_to_zero(v: i32, shift: u32) -> i32 {
    (if v < 0 { v + ((1 << shift) - 1) } else { v }) >> shift
}

/// PORT: FUN_801d2afc (`0x801D2B44`..`0x801D2C38`) - the VITAL fill bar.
///
/// Width is `hp >> 5` (rounded toward zero), so one pixel per 32 HP: at
/// [`FULL_HP`] the bar is `0x64` pixels. The green channel *is* that width, so
/// the quad's far end runs `(0xBC, hp >> 5, 0)` and reddens toward the anchor
/// as HP drops. Slot `0` anchors right at [`VITAL_ANCHOR_LEFT`] and fills
/// leftward; slot `1` anchors left at [`VITAL_ANCHOR_RIGHT`] and fills
/// rightward.
pub fn vital_bar(slot: usize, hp: i32) -> VitalBar {
    let w = sra_round_to_zero(hp, 5);
    let green = w.clamp(0, 0xFF) as u8;
    let (x0, x1) = if slot == 0 {
        (VITAL_ANCHOR_LEFT - w as i16, VITAL_ANCHOR_LEFT)
    } else {
        (VITAL_ANCHOR_RIGHT, VITAL_ANCHOR_RIGHT + w as i16)
    };
    VitalBar {
        x0,
        y0: VITAL_Y0,
        x1,
        y1: VITAL_Y1,
        rgb_far: [VITAL_RED, green, 0],
        rgb_anchor: [VITAL_RED, 0, 0],
    }
}

/// Left edge of each side's VITAL frame (`li s1,0x1c` / `li s1,0xb0`).
pub const VITAL_FRAME_X: [i16; 2] = [0x1C, 0xB0];
/// Top scanline of the VITAL frames (`li s4,0x20`).
pub const VITAL_FRAME_Y0: i16 = 0x20;
/// Bottom scanline of the VITAL frames (`s4 + 0x10`).
pub const VITAL_FRAME_Y1: i16 = 0x30;

/// One cell of a VITAL bar frame as the HUD renderer emits it: a `POLY_FT4`
/// (`0x2C`, colour `0x808080`, texpage 5, CLUT `0x7D80`) spanning
/// `x0..x1` x [`VITAL_FRAME_Y0`]..[`VITAL_FRAME_Y1`] with the record's four
/// texture corners.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VitalFrameCell {
    pub x0: i16,
    pub y0: i16,
    pub x1: i16,
    pub y1: i16,
    /// Corner UVs, top-left / top-right / bottom-left / bottom-right.
    pub uv: [(u8, u8); 4],
}

/// PORT: FUN_801d2afc (`0x801D2C9C`..`0x801D2DAC`) - the VITAL bar frames.
///
/// Each side lays the [`legaia_asset::baka_opponents::HUD_BAR_FRAME_CELLS`]
/// table cells left to right from [`VITAL_FRAME_X`], each `width` wide - the
/// cap, the stretched body, the cap. There is no "VITAL" label in the duel
/// HUD: widget `0x18` is only drawn by the select screen (`0x801D2450`).
pub fn vital_frame_cells(
    side: usize,
    cells: &[legaia_asset::baka_opponents::BakaBarFrameCell],
) -> Vec<VitalFrameCell> {
    let mut x = VITAL_FRAME_X[side.min(1)];
    cells
        .iter()
        .map(|c| {
            let x0 = x;
            x = x.wrapping_add(c.width as i16);
            VitalFrameCell {
                x0,
                y0: VITAL_FRAME_Y0,
                x1: x,
                y1: VITAL_FRAME_Y1,
                uv: c.uv,
            }
        })
        .collect()
}

/// Texture-U of a **filled** round-win pip (`0x30`).
pub const PIP_U_FILLED: u8 = 0x30;
/// Texture-U of an **empty** round-win pip (`0x40`).
pub const PIP_U_EMPTY: u8 = 0x40;
/// Scanline the pip row sits on (`0x30`).
pub const PIP_Y: i16 = 0x30;

/// PORT: FUN_801d2afc (`0x801D2FBC`..`0x801D30C4`) - the round-win pip row.
///
/// One 16x16 cell per pip up to the win target ([`crate::baka_fighter`]'s
/// best-of-3 `DAT_801DBED0`), filled while `i < wins`. The player's row steps
/// right from `0x70`, the opponent's steps **left** from `0xC0`. Returns
/// `(x, y, u)` per pip in draw order.
pub fn round_win_pips(slot: usize, wins: u32, target: u32) -> Vec<(i16, i16, u8)> {
    (0..target as i16)
        .map(|i| {
            let x = if slot == 0 {
                0x70 + i * 16
            } else {
                0xC0 - i * 16
            };
            let u = if (i as u32) < wins {
                PIP_U_FILLED
            } else {
                PIP_U_EMPTY
            };
            (x, PIP_Y, u)
        })
        .collect()
}

/// PORT: FUN_801d2afc (`0x801D3150`..`0x801D31A0`) - the combo counter's
/// brightness.
///
/// Not a monotonic ramp. The count is first clamped to `12`, then: `<= 3`
/// draws at full `0xF0`; `4..=6` *dims* to `(10 - combo) * 16 + 0x80`
/// (`0xE0` / `0xD0` / `0xC0`); and from `7` up the second `< 4` test fires and
/// it snaps back to `0xF0`. The dip is the flash - a long streak is drawn at
/// full brightness, not brighter.
pub fn combo_counter_level(combo: i32) -> u8 {
    let combo = if combo >= 13 { 12 } else { combo };
    if combo < 4 {
        return 0xF0;
    }
    let k = 10 - combo;
    if k < 4 { 0xF0 } else { (k * 16 + 0x80) as u8 }
}

/// PORT: FUN_801d2afc (`0x801D3270`..`0x801D32FC`) - the stage digits.
///
/// The drawn value is `DAT_801DC110 + 1` (the stage counter is 0-based, the
/// cabinet numbers its rungs from 1). Below ten only the units digit is drawn,
/// at `x = 0x48`; from ten the tens digit goes at `x = 0x40` first. Returns
/// `(x, y, digit)` in draw order.
pub fn stage_digits(stage_display: i32) -> Vec<(i16, i16, u8)> {
    let v = stage_display;
    let mut out = Vec::with_capacity(2);
    if v >= 10 {
        out.push((0x40, 0x1E, ((v / 10) % 10) as u8));
    }
    out.push((0x48, 0x1E, (v % 10) as u8));
    out
}

/// PORT: FUN_801d2afc (`0x801D3300`..`0x801D335C`) - the high-score readout's
/// right-aligned origin.
///
/// Retail counts the score's decimal places by testing it against
/// `10^k - 1` for `k = 0..=7`, starting from eight slots and decrementing on
/// each miss. The `- 1` makes the test off by one at every power of ten, so a
/// score of `9` counts as **two** places, not one - the readout sits one cell
/// further left than a true digit count would put it. That quirk is retail's;
/// the port keeps it.
pub fn high_score_places(score: i32) -> i32 {
    let mut places = 8;
    let mut p: i32 = 1;
    for _ in 0..8 {
        if score < p - 1 {
            places -= 1;
        }
        p = p.saturating_mul(10);
    }
    places
}

/// Screen x the high-score number is drawn from (`0x28 - places * 8`); the
/// readout is skipped entirely at a score of zero.
pub fn high_score_x(score: i32) -> Option<i16> {
    if score == 0 {
        return None;
    }
    Some(0x28 - (high_score_places(score) * 8) as i16)
}

/// PORT: FUN_801d2afc (`0x801D2DC8`..`0x801D2DF0`) - the running-max latch.
///
/// The HUD renderer is where the score's combo term is latched, and it feeds
/// **two** globals from the same source (`DAT_801DC094`, slot 1's
/// consecutive-hits-taken counter): `DAT_801DBF58` and `DAT_801DBEC8`. Both
/// are plain running maxima, so a streak that ends does not lower them.
pub fn max_streak_latch(latch: i32, streak: i32) -> i32 {
    if latch < streak { streak } else { latch }
}

/// Everything the HUD renderer emits for one frame, renderer-agnostic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HudFrame {
    /// Per-slot VITAL fill bars.
    pub bars: [VitalBar; 2],
    /// Per-slot round-win pips, `(x, y, u)`.
    pub pips: [Vec<(i16, i16, u8)>; 2],
    /// Per-slot combo-counter brightness. The sides are **crossed**: index `0`
    /// carries slot 1's hits-taken counter (the streak the player is landing).
    pub combo_level: [u8; 2],
    /// Stage digits, `(x, y, digit)`.
    pub stage: Vec<(i16, i16, u8)>,
    /// High-score readout origin, absent at a score of zero.
    pub high_score_x: Option<i16>,
    /// The running max-streak latch after this frame.
    pub max_streak: i32,
}

/// PORT: FUN_801d2afc - the duel HUD renderer.
///
/// Retail calls this from [`FUN_801CF388`'s epilogue](BakaCabinet::tick) once
/// per frame, for the in-duel states only, and only while the
/// suppress-scene flag is clear. Every piece it draws is one of the kernels
/// above; the textured cells themselves go through the widget-quad emitter
/// (`crate::baka_fighter::hud_widget_quad`), which stays a host job.
///
/// `combo_taken` is the per-slot consecutive-hits-taken counter; the renderer
/// crosses the two so each side shows the streak it is *landing*.
pub fn hud_frame(
    hp: [i32; 2],
    round_wins: [u32; 2],
    win_target: u32,
    combo_taken: [i32; 2],
    stage_display: i32,
    high_score: i32,
    latch: i32,
) -> HudFrame {
    HudFrame {
        bars: [vital_bar(0, hp[0]), vital_bar(1, hp[1])],
        pips: [
            round_win_pips(0, round_wins[0], win_target),
            round_win_pips(1, round_wins[1], win_target),
        ],
        combo_level: [
            combo_counter_level(combo_taken[1]),
            combo_counter_level(combo_taken[0]),
        ],
        stage: stage_digits(stage_display),
        high_score_x: high_score_x(high_score),
        max_streak: max_streak_latch(latch, combo_taken[1]),
    }
}
