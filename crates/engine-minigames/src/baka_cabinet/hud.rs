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
/// Each side lays the [`legaia_minigame_formats::baka_opponents::HUD_BAR_FRAME_CELLS`]
/// table cells left to right from [`VITAL_FRAME_X`], each `width` wide - the
/// cap, the stretched body, the cap. There is no "VITAL" label in the duel
/// HUD: widget `0x18` is only drawn by the select screen (`0x801D2450`).
pub fn vital_frame_cells(
    side: usize,
    cells: &[legaia_minigame_formats::baka_opponents::BakaBarFrameCell],
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

/// PORT: FUN_801d2afc (`0x801D3144`..`0x801D31A4`) - the lit attack icon's
/// brightness.
///
/// The icon row matching a fighter's chosen attack type (block `+0x24`)
/// takes this level off the held-type timer (block `+0x8C`, frames since the
/// type was committed); the other rows keep the packet's `0x80`. The timer is
/// first clamped to `12`, then: `<= 3` draws at `0xF0`; `4..=6` *dims* to
/// `(10 - t) * 16 + 0x80` (`0xE0` / `0xD0` / `0xC0`); and from `7` up the
/// second `< 4` test fires and it snaps back to `0xF0`. The dip is the flash
/// a fresh commit gets.
pub fn icon_lit_level(hold: i32) -> u8 {
    let t = if hold >= 13 { 12 } else { hold };
    if t < 4 {
        return 0xF0;
    }
    let k = 10 - t;
    if k < 4 { 0xF0 } else { (k * 16 + 0x80) as u8 }
}

/// Scanline of the attack-icon columns' first row (`li s4,0x60`).
pub const ICON_Y0: i16 = 0x60;
/// Left edge of each side's attack-icon column (`li s1,0x20` / `0x110`).
pub const ICON_X: [i16; 2] = [0x20, 0x110];
/// Neutral packet colour of every cell the HUD renderer emits itself.
pub const HUD_CELL_LEVEL: u8 = 0x80;

/// PORT: FUN_801d2afc (`0x801D30C8`..`0x801D3210`) - the attack-icon
/// columns' levels, `[side][row]`.
///
/// Side `s` reads its **own** block: row `chosen_type - 1` is lit at
/// [`icon_lit_level`] of the held-type timer, every other row stays at
/// [`HUD_CELL_LEVEL`]. A chosen type of `0` (none) or `4` (the special)
/// lights no row.
pub fn attack_icon_levels(chosen_type: [i32; 2], hold: [i32; 2]) -> [[u8; 3]; 2] {
    std::array::from_fn(|s| {
        std::array::from_fn(|row| {
            if row as i32 == chosen_type[s] - 1 {
                icon_lit_level(hold[s])
            } else {
                HUD_CELL_LEVEL
            }
        })
    })
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
/// leading field cell.
///
/// Retail starts a counter at `8` and decrements it once for every `k` in
/// `0..=7` with `score >= 10^k - 1`. The number drawer (`FUN_801D6A18`) lays
/// an eight-cell right-aligned field from `x0`, so with the field origin at
/// `0x28 - cell * 8` ([`high_score_x`]) the leading digit lands on `0x28`:
/// the readout is **left-aligned** there. The `- 1` makes the test pass one
/// power early on an all-nines score (`9`, `99`, ...), which moves that
/// score's readout one cell right of `0x28`. That quirk is retail's; the port
/// keeps it.
pub fn high_score_places(score: i32) -> i32 {
    let mut places = 8;
    let mut p: i32 = 1;
    for _ in 0..8 {
        if score >= p - 1 {
            places -= 1;
        }
        p = p.saturating_mul(10);
    }
    places
}

/// Screen x of the high-score number's eight-cell field (`0x28 - places * 8`);
/// the readout is skipped entirely at a score of zero.
pub fn high_score_x(score: i32) -> Option<i16> {
    if score == 0 {
        return None;
    }
    Some(0x28 - (high_score_places(score) * 8) as i16)
}

/// Scanline the high-score readout is centred on (`li a1,0x36`).
pub const HIGH_SCORE_Y: i16 = 0x36;
/// Brightness the high-score readout draws at (`li a3,0xa0`).
pub const HIGH_SCORE_LEVEL: i32 = 0xA0;

/// PORT: FUN_801d2afc (`0x801D3214`..`0x801D3258`) - the "PRESS SELECT TO
/// MENU" prompt's brightness: full `0x80` while bit 4 of the blink phase
/// `DAT_801DC12C` is set, half `0x40` otherwise - a 16-frame blink.
pub fn press_select_level(blink: i32) -> i32 {
    if blink & 0x10 != 0 { 0x80 } else { 0x40 }
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

/// What the HUD renderer reads for one frame: the fighter blocks' fields and
/// the cabinet globals.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HudInput {
    /// Per-slot HP (block `+0x08`).
    pub hp: [i32; 2],
    /// Per-slot round wins (block `+0x34`).
    pub round_wins: [u32; 2],
    /// Round-win target (`DAT_801DBED0`).
    pub win_target: u32,
    /// Per-slot consecutive-hits-taken counter (block `+0x30`).
    pub combo_taken: [i32; 2],
    /// Per-slot chosen attack type (block `+0x24`, `0` none .. `4` special).
    pub chosen_type: [i32; 2],
    /// Per-slot held-type timer (block `+0x8C`).
    pub type_hold: [i32; 2],
    /// The stage number drawn, `DAT_801DC110 + 1`.
    pub stage_display: i32,
    /// The running high score (`DAT_801DBEE4`).
    pub high_score: i32,
    /// The running-max latch before this frame (`DAT_801DBEC8`).
    pub latch: i32,
    /// The blink phase (`DAT_801DC12C`).
    pub blink: i32,
}

/// Everything the HUD renderer emits for one frame, renderer-agnostic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HudFrame {
    /// Per-slot VITAL fill bars.
    pub bars: [VitalBar; 2],
    /// Per-slot round-win pips, `(x, y, u)`.
    pub pips: [Vec<(i16, i16, u8)>; 2],
    /// The hit count drawn on each side. The sides are **crossed**: side `0`
    /// shows slot 1's hits-taken counter - the streak the player is landing -
    /// and side `1` slot 0's (the `(s + 1) & 1` fold at `0x801D2DFC`).
    pub combo: [i32; 2],
    /// Attack-icon levels, `[side][row]` ([`attack_icon_levels`]).
    pub icons: [[u8; 3]; 2],
    /// Stage digits, `(x, y, digit)`.
    pub stage: Vec<(i16, i16, u8)>,
    /// The "PRESS SELECT TO MENU" prompt's brightness.
    pub select_level: i32,
    /// The high score drawn, and its field origin; absent at zero.
    pub high_score: Option<(i16, i32)>,
    /// The running max-streak latch after this frame.
    pub max_streak: i32,
}

/// PORT: FUN_801d2afc - the duel HUD renderer.
///
/// Retail calls this from [`FUN_801CF388`'s epilogue](BakaCabinet::tick) once
/// per frame, for the in-duel states only, and only while the
/// suppress-scene flag is clear. Every piece it draws is one of the kernels
/// above; [`duel_hud_packets`] turns the frame into the packets retail links.
pub fn hud_frame(input: &HudInput) -> HudFrame {
    HudFrame {
        bars: [vital_bar(0, input.hp[0]), vital_bar(1, input.hp[1])],
        pips: [
            round_win_pips(0, input.round_wins[0], input.win_target),
            round_win_pips(1, input.round_wins[1], input.win_target),
        ],
        combo: [input.combo_taken[1], input.combo_taken[0]],
        icons: attack_icon_levels(input.chosen_type, input.type_hold),
        stage: stage_digits(input.stage_display),
        select_level: press_select_level(input.blink),
        high_score: high_score_x(input.high_score).map(|x| (x, input.high_score)),
        max_streak: max_streak_latch(input.latch, input.combo_taken[1]),
    }
}

// ------------------------------------------------------------- HUD packets

/// Ordering-table bucket the HUD renderer's own packets link at:
/// `*(0x1F8003F4) + 0x14`, the fifth slot - one farther than the widget
/// emitter's bucket, so the round chrome draws over the duel HUD.
pub const DUEL_HUD_OT: u32 = 5;
/// Texture page every textured cell of the duel HUD samples (`li a3,0x5`):
/// 4bpp at VRAM `(320, 0)`, the PROT 1203 art page.
pub const DUEL_HUD_TPAGE: u16 = 5;
/// CLUT of the bar frames and the pips (`0x7D80`).
pub const DUEL_HUD_CLUT_FRAME: u16 = 0x7D80;
/// CLUT of the "HIT!" label (`0x7D81`).
pub const DUEL_HUD_CLUT_HIT: u16 = 0x7D81;
/// CLUT of the combo digits (`0x7D82`).
pub const DUEL_HUD_CLUT_COMBO: u16 = 0x7D82;
/// CLUT of the attack icons (`0x7D83`).
pub const DUEL_HUD_CLUT_ICON: u16 = 0x7D83;
/// CLUT the number drawer `FUN_801D6A18` forces on widget `0x13` for its run
/// (`0x7D87 + arg`, the HUD passing `0`).
pub const NUMBER_CLUT: u16 = 0x7D87;
/// Widget id of the "PRESS SELECT TO MENU" prompt.
pub const WIDGET_PRESS_SELECT: u8 = 0x14;
/// Widget id of the "STAGE" label.
pub const WIDGET_STAGE: u8 = 0x12;
/// Combo digit / "HIT!" row's top scanline (`li s4,0x40`).
pub const COMBO_Y: i16 = 0x40;
/// Where each side's combo digits start, descending (`0x30` / `0x100`).
pub const COMBO_X: [i16; 2] = [0x30, 0x100];

/// One packet of the duel HUD, in retail's link order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DuelHudPacket {
    /// A VITAL fill bar: a `POLY_GT4` sampling the resident system texel
    /// (texpage `0x1F`, `uv (0, 0xF0)..(2, 0xF2)`, CLUT `0x7F84`) under the
    /// bar's gouraud pair, at [`DUEL_HUD_OT`]. Slot `0`'s far end is its
    /// left edge, slot `1`'s its right.
    Bar { slot: usize, bar: VitalBar },
    /// A textured `POLY_FT4` / `POLY_GT4` and the bucket it links at: the
    /// renderer's own cells at [`DUEL_HUD_OT`], the widget-emitter draws
    /// (`FUN_801D5ED0`) at its bucket `3`.
    Cell {
        quad: crate::baka_fighter::HudWidgetQuad,
        ot: u32,
    },
}

/// The widget emitter's bucket (`_DAT_801DBEBC`, bumped to `3` by every call).
pub const WIDGET_OT: u32 = 3;

/// A raw `POLY_FT4` (`0x2C`) the renderer builds itself: corners
/// `x0..x1` x `y0..y1`, the cell `u0..u1` x `v0..v1`, one colour level.
fn raw_cell(
    (x0, y0, x1, y1): (i16, i16, i16, i16),
    (u0, v0, u1, v1): (u8, u8, u8, u8),
    clut: u16,
    level: u8,
) -> DuelHudPacket {
    DuelHudPacket::Cell {
        quad: crate::baka_fighter::HudWidgetQuad {
            poly_code: 0x2C,
            x0,
            y0,
            x1,
            y1,
            uv: [(u0, v0), (u1, v0), (u0, v1), (u1, v1)],
            rgb_top: [level; 3],
            rgb_bottom: [level; 3],
            clut,
            tpage_attr: DUEL_HUD_TPAGE,
        },
        ot: DUEL_HUD_OT,
    }
}

/// PORT: FUN_801d2afc - the duel HUD renderer's packets, in the order it
/// links them.
///
/// - the two VITAL fill bars, then the two three-cell bar frames
///   ([`vital_frame_cells`] over `frame_cells`, the `0x801DBC34` table);
///   one bucket, so the frames (linked later) draw first and the bars over
///   them;
/// - per side, the combo count's digits (`(d*16, 0x20)` 16x16 via CLUT
///   `0x7D82`, descending 16 px from [`COMBO_X`]) and the "HIT!" cell
///   (`(0, 0x10)` 32x16 via `0x7D81`) 16 px right of the start, only while
///   the count is non-zero;
/// - the round-win pips (`0x7D80`);
/// - the attack-icon columns (`(row*16, 0x30)` via `0x7D83`) at their
///   levels;
/// - then, through the widget emitter (`widgets`, `DAT_801D7160`): "PRESS
///   SELECT TO MENU" at `(0xEA, 0x1E)`, "STAGE" at `(0x30, 0x1E)`, the stage
///   digits (`FUN_801D69E4`: widget `0x13`, `u = digit * 8`) and the high
///   score (`FUN_801D6A18`: the same widget, CLUT [`NUMBER_CLUT`], eight
///   cells 8 px apart from the field origin, at [`HIGH_SCORE_LEVEL`]).
///
/// A widget id past `widgets` is skipped; an empty `frame_cells` draws no
/// frames. The textured cells sample the PROT 1203 art page at texpage 5,
/// so a host draws them against VRAM that carries it.
pub fn duel_hud_packets(
    hud: &HudFrame,
    widgets: &[legaia_minigame_formats::baka_opponents::BakaHudWidget],
    frame_cells: &[legaia_minigame_formats::baka_opponents::BakaBarFrameCell],
) -> Vec<DuelHudPacket> {
    use crate::baka_fighter::{
        NUMBER_WIDGET, hud_widget_quad, right_aligned_number_cells, single_digit_cell,
    };
    type Widget = legaia_minigame_formats::baka_opponents::BakaHudWidget;
    let mut out = Vec::new();
    out.extend(
        hud.bars
            .iter()
            .enumerate()
            .map(|(slot, &bar)| DuelHudPacket::Bar { slot, bar }),
    );
    for side in 0..2 {
        for c in vital_frame_cells(side, frame_cells) {
            out.push(DuelHudPacket::Cell {
                quad: crate::baka_fighter::HudWidgetQuad {
                    poly_code: 0x2C,
                    x0: c.x0,
                    y0: c.y0,
                    x1: c.x1,
                    y1: c.y1,
                    uv: c.uv,
                    rgb_top: [HUD_CELL_LEVEL; 3],
                    rgb_bottom: [HUD_CELL_LEVEL; 3],
                    clut: DUEL_HUD_CLUT_FRAME,
                    tpage_attr: DUEL_HUD_TPAGE,
                },
                ot: DUEL_HUD_OT,
            });
        }
    }
    for (side, &count) in hud.combo.iter().enumerate() {
        if count == 0 {
            continue;
        }
        let start = COMBO_X[side];
        let mut x = start;
        let mut v = count;
        loop {
            let d = (v % 10).unsigned_abs() as u8 * 16;
            out.push(raw_cell(
                (x, COMBO_Y, x + 0xF, COMBO_Y + 0xF),
                (d, 0x20, d + 0xF, 0x2F),
                DUEL_HUD_CLUT_COMBO,
                HUD_CELL_LEVEL,
            ));
            x -= 0x10;
            v /= 10;
            if v == 0 {
                break;
            }
        }
        let hx = start + 0x10;
        out.push(raw_cell(
            (hx, COMBO_Y, hx + 0x1F, COMBO_Y + 0xF),
            (0, 0x10, 0x1F, 0x1F),
            DUEL_HUD_CLUT_HIT,
            HUD_CELL_LEVEL,
        ));
    }
    for pips in &hud.pips {
        for &(x, y, u) in pips {
            out.push(raw_cell(
                (x, y, x + 0x10, y + 0x10),
                (u, 0, u + 0xF, 0xF),
                DUEL_HUD_CLUT_FRAME,
                HUD_CELL_LEVEL,
            ));
        }
    }
    for (side, levels) in hud.icons.iter().enumerate() {
        let x = ICON_X[side];
        for (row, &level) in levels.iter().enumerate() {
            let y = ICON_Y0 + row as i16 * 0x10;
            let u = row as u8 * 0x10;
            out.push(raw_cell(
                (x, y, x + 0x10, y + 0x10),
                (u, 0x30, u + 0xF, 0x3F),
                DUEL_HUD_CLUT_ICON,
                level,
            ));
        }
    }
    let widget = |id: u8, x: i16, y: i16, level: i32, patch: &dyn Fn(&mut Widget)| {
        widgets.get(usize::from(id)).map(|w| {
            let mut w = *w;
            patch(&mut w);
            DuelHudPacket::Cell {
                quad: hud_widget_quad(&w, x, y, level, 0x1000, false),
                ot: WIDGET_OT,
            }
        })
    };
    out.extend(widget(
        WIDGET_PRESS_SELECT,
        0xEA,
        0x1E,
        hud.select_level,
        &|_| {},
    ));
    out.extend(widget(WIDGET_STAGE, 0x30, 0x1E, 0x80, &|_| {}));
    for &(x, y, digit) in &hud.stage {
        let u = single_digit_cell(digit).u;
        out.extend(widget(NUMBER_WIDGET, x, y, 0x80, &|w: &mut Widget| w.u = u));
    }
    if let Some((x0, score)) = hud.high_score {
        for c in right_aligned_number_cells(score) {
            out.extend(widget(
                NUMBER_WIDGET,
                x0 + c.x_offset,
                HIGH_SCORE_Y,
                HIGH_SCORE_LEVEL,
                &|w: &mut Widget| {
                    w.u = c.u;
                    w.clut = NUMBER_CLUT;
                },
            ));
        }
    }
    out
}
