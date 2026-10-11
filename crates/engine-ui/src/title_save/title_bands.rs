//! The title card's **band composition** - one builder, both hosts.
//!
//! The retail title screen is sub-rects of the PROT 0888 title TIM
//! ([`legaia_asset::title_pak`]'s `TITLE_BAND_*`) placed at
//! [`TITLE_ART_POS`]. Both hosts used to carry their own copy of that
//! composition, which is how the save-screen backdrop came to exist on one
//! host only: the browser play page dropped its title session the moment the
//! Load row opened, so the save-select drew over black while the native
//! window kept the art behind it at retail's dim.

use crate::*;

/// Which title-TIM bands draw this frame, and how bright.
///
/// `dim` is retail's save-screen backdrop: the title strips stay on screen
/// behind the Load/Save chrome at roughly 45 % of their brightness, laid out
/// by the menu overlay's own title drawer ([`title_strip_rows`]) rather than
/// by the title card's composition. That is a separate knob from `alpha`,
/// which is the title's own fade-in ramp.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TitleBandState {
    /// Fade-in ramp, `0.0..=1.0`. `1.0` once the card is up.
    pub alpha: f32,
    /// Backdrop dim - the save-screen level rather than the title level.
    pub dim: bool,
    /// Draw the "PRESS START BUTTON" band (its own phase only).
    pub press_start: bool,
    /// `(cursor row, has focus)` when the NEW GAME / CONTINUE rows draw.
    /// `has_focus == false` draws both rows dim. The backdrop (`dim`) reads
    /// only the cursor: retail keeps the title's cursor row lit behind the
    /// Load window and halves the other ([`title_strip_rows`]).
    pub menu: Option<(u8, bool)>,
}

impl TitleBandState {
    /// The live title card at full brightness with neither prompt nor rows.
    pub fn card(alpha: f32) -> Self {
        Self {
            alpha,
            dim: false,
            press_start: false,
            menu: None,
        }
    }

    /// The save-screen backdrop: dim, no prompt, CONTINUE lit.
    ///
    /// The Load window opens from the title's CONTINUE row, so the title
    /// cursor `_DAT_8007B820` reads `1` whenever retail draws this backdrop
    /// (the `title_menu_idle` and `save_select_idle` states both hold `1`),
    /// and [`title_strip_rows`] lights that row and halves NEW GAME.
    pub fn backdrop() -> Self {
        Self {
            alpha: 1.0,
            dim: true,
            press_start: false,
            menu: Some((1, true)),
        }
    }
}

/// Luminance the save-screen backdrop draws the title art at.
///
/// Retail has no alpha here: `FUN_801E02A4` re-emits the art with one
/// brightness byte in all three RGB modulation slots, and `0x80` is the
/// neutral level, so the engine's `0.45` lands at `0x3A`.
pub const TITLE_BACKDROP_LUM: f32 = 0.45;

/// Compose the title-TIM bands into stage-scaled [`SpriteDraw`]s.
///
/// `stage_origin` / `stage_scale` are the canonical 320x240 stage both hosts
/// place every boot-UI element on; each band is sampled at its own source
/// rect and drawn at [`TITLE_ART_POS`] plus that rect's own `(x, y)`, so the
/// composition is the TIM's own layout offset by retail's title-quad
/// placement. The `<DEMO>` band and the packed "NEW GAME CONTINUE" footer
/// band are deliberately not emitted - the former is a demo-build leftover
/// retail never draws, the latter is superseded by the re-positioned rows
/// below.
///
/// The wordmark goes through [`backdrop_dim_sprites`] rather than a plain
/// tinted blit, because retail's own backdrop law is a modulation byte and a
/// split at the VRAM texture-page seam. The fade alpha folds into that byte.
///
/// With `state.dim` set the composition is a different routine's: the save
/// screen's backdrop is the menu overlay's own title drawer, whose rows and
/// strip rects differ from the card's (the TM and copyright lines sit 16 px
/// higher, the rows 5 px higher), so the dim path returns
/// [`title_strip_sprites`] instead.
pub fn title_band_sprites(
    state: TitleBandState,
    stage_origin: (i32, i32),
    stage_scale: u32,
) -> Vec<SpriteDraw> {
    use legaia_asset::title_pak;
    let scale = stage_scale.max(1);
    let si = scale as i32;
    let alpha = state.alpha.clamp(0.0, 1.0);
    if state.dim {
        let brightness = (TITLE_BACKDROP_LUM * alpha * 128.0)
            .round()
            .clamp(0.0, 255.0) as u8;
        let continue_selected = state.menu.is_none_or(|(cursor, _)| cursor == 1);
        return title_strip_sprites(continue_selected, brightness, alpha, stage_origin, scale);
    }
    let lum = 1.0;
    let color = [1.0, 1.0, 1.0, alpha];
    let (sx0, sy0) = stage_origin;
    let tpx = TITLE_ART_POS.0;
    let tpy = TITLE_ART_POS.1;
    let mut out: Vec<SpriteDraw> = Vec::new();
    let push = |out: &mut Vec<SpriteDraw>,
                src: (u32, u32, u32, u32),
                dsx: i32,
                dsy: i32,
                tint: [f32; 4]| {
        let (_, _, sw, sh) = src;
        out.push(SpriteDraw {
            dst: (
                sx0 + (tpx + dsx) * si,
                sy0 + (tpy + dsy) * si,
                sw * scale,
                sh * scale,
            ),
            src,
            color: tint,
        });
    };

    let wm = title_pak::TITLE_BAND_WORDMARK;
    let brightness = (lum * alpha * 128.0).round().clamp(0.0, 255.0) as u8;
    out.extend(backdrop_dim_sprites(
        wm,
        brightness,
        (
            sx0 + (tpx + wm.0 as i32) * si,
            sy0 + (tpy + wm.1 as i32) * si,
        ),
        scale,
    ));

    if state.press_start {
        let ps = title_pak::TITLE_BAND_PRESS_START;
        push(&mut out, ps, ps.0 as i32, ps.1 as i32, color);
    }

    if let Some((cursor, has_focus)) = state.menu {
        let row_dim = [color[0] * 0.5, color[1] * 0.5, color[2] * 0.5, color[3]];
        let ng = title_pak::TITLE_BAND_MENU_NEW_GAME;
        let co = title_pak::TITLE_BAND_MENU_CONTINUE;
        // Centred inside the title-art width, so the rows land on the
        // screen's horizontal centre (fb_x = 160) once `push` adds
        // `TITLE_ART_POS.x`.
        let art_w = TITLE_ART_SIZE.0 as u32;
        let ng_x = ((art_w - ng.2) / 2) as i32;
        let co_x = ((art_w - co.2) / 2) as i32;
        // Between the wordmark (ends y ~141) and the copyright lines
        // (start y ~195).
        let ng_y: i32 = 154;
        let co_y: i32 = ng_y + ng.3 as i32 + 4;
        let pick = |row: u8| {
            if has_focus && cursor == row {
                color
            } else {
                row_dim
            }
        };
        push(&mut out, ng, ng_x, ng_y, pick(0));
        push(&mut out, co, co_x, co_y, pick(1));
    }

    let tm = title_pak::TITLE_BAND_TM_COPYRIGHT;
    push(&mut out, tm, tm.0 as i32, tm.1 as i32, color);
    let cc = title_pak::TITLE_BAND_C_COPYRIGHT;
    push(&mut out, cc, cc.0 as i32, cc.1 as i32, color);
    out
}

/// Texel rects `(u, v, w, h)` of the six title-strip records at the head of
/// the menu overlay's sprite-descriptor table (`0x801E50A8`, PROT 0899 file
/// offset `0x16890`, 20-byte stride), inside the title TIM's page.
///
/// Every one reads tpage `0x0098` and CLUT `0x7AC0`: the 8bpp page at VRAM
/// `(512, 256)` with its palette at `(0, 491)`, which is exactly where the
/// title TIM (PROT 0890 at `0x14228`, [`legaia_asset::title_pak`]) uploads its
/// pixel and CLUT blocks. A `save_select_idle` VRAM capture holds that TIM
/// there byte for byte, so the atlas both hosts already build from PROT 0890
/// is the page these rects address.
///
/// `0` wordmark, `1` PRESS START BUTTON (in the table, not drawn by the
/// backdrop), `2` the TM line, `3` NEW GAME, `4` CONTINUE, `5` the copyright
/// line.
pub const TITLE_STRIP_RECORDS: [(u32, u32, u32, u32); 6] = [
    (0, 0, 254, 148),
    (0, 176, 254, 16),
    (0, 192, 254, 16),
    (0, 224, 64, 16),
    (64, 224, 64, 16),
    (0, 208, 254, 16),
];

/// Stage x every title strip is centred on (`0xA0`).
pub const TITLE_STRIP_CENTRE_X: i32 = 0xA0;

/// One strip of the save-screen title backdrop: its centre `y` on the stage,
/// the record of [`TITLE_STRIP_RECORDS`] it draws, and whether it draws at
/// **half** the caller's brightness (the title row the cursor is not on).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TitleStripRow {
    pub y: i32,
    pub record: u8,
    pub half_bright: bool,
}

/// The title strips the menu overlay redraws behind the Load window when it
/// is opened from the title: the wordmark centred at `y = 0x50`, NEW GAME
/// and CONTINUE at `0xA0` / `0xAE`, and the TM and copyright lines at `0xBE`
/// / `0xCC`, every one centred on `x = 0xA0`. The row the title cursor
/// (`_DAT_8007B820`) is on keeps the caller's brightness, and the other draws
/// at half of it (`param >> 1` folded into the draw call).
///
/// This is the title screen, not a memory-card message screen. The records
/// the routine hands `FUN_801E2EE4` are the title TIM's own strips (see
/// [`TITLE_STRIP_RECORDS`]), and a `save_select_idle` framebuffer shows the
/// five rows at these centres with NEW GAME at half the brightness of
/// CONTINUE. The routine's one caller, `0x801E0260` in `FUN_801DD35C`, runs
/// it only while `_DAT_8007BB00` is set, and hands the same brightness byte
/// to the dimmed backdrop art `FUN_801E02A4` right after it.
///
/// Both play hosts draw it through [`title_band_sprites`]' dim arm
/// ([`TitleBandState::backdrop`]) for the save-select the title's Continue
/// opens: the native window's `boot_title_band_state`, the browser page's
/// `boot_title_backdrop_draws_json`. Each host has a test holding its output
/// to [`title_strip_sprites`].
///
/// Retail also computes a triangle-wave pulse off the frame counter
/// `DAT_801F3294 % 0xFFF` here and then never reads it - the value is dead
/// at every use site (the delay-slot `li a0, 2` overwrites the only register
/// it lived in), so the port omits it deliberately. The counter itself still
/// advances `0x20 * frame_skip` per call.
///
/// PORT: FUN_801E0418 (see `ghidra/scripts/funcs/overlay_menu_801e0418.txt`)
pub fn title_strip_rows(continue_selected: bool) -> [TitleStripRow; 5] {
    [
        TitleStripRow {
            y: 0x50,
            record: 0,
            half_bright: false,
        },
        TitleStripRow {
            y: 0xA0,
            record: 3,
            half_bright: continue_selected,
        },
        TitleStripRow {
            y: 0xAE,
            record: 4,
            half_bright: !continue_selected,
        },
        TitleStripRow {
            y: 0xBE,
            record: 2,
            half_bright: false,
        },
        TitleStripRow {
            y: 0xCC,
            record: 5,
            half_bright: false,
        },
    ]
}

/// The vertex modulation `FUN_801E2EE4` gives a record whose colour byte is
/// `0xFF` at caller brightness `b`: `(0xFF * b) >> 8`.
pub fn title_strip_modulation(brightness: u8) -> u8 {
    ((0xFF_u32 * u32::from(brightness)) >> 8) as u8
}

/// Emit [`title_strip_rows`] as stage-scaled sprites over the title atlas.
///
/// Each record is centred on `(TITLE_STRIP_CENTRE_X, row.y)` by its own half
/// extents (`FUN_801E2EE4`: `(w * 0x1000) >> 13`, then the `0x1000` scale),
/// and modulated by [`title_strip_modulation`] of the caller's brightness,
/// halved for the unselected title row. `alpha` is the host's fade alpha,
/// kept on the colour's fourth channel.
pub fn title_strip_sprites(
    continue_selected: bool,
    brightness: u8,
    alpha: f32,
    stage_origin: (i32, i32),
    stage_scale: u32,
) -> Vec<SpriteDraw> {
    let scale = stage_scale.max(1);
    let si = scale as i32;
    title_strip_rows(continue_selected)
        .iter()
        .map(|row| {
            let src = TITLE_STRIP_RECORDS[row.record as usize];
            let (_, _, w, h) = src;
            let b = if row.half_bright {
                brightness >> 1
            } else {
                brightness
            };
            let tint = f32::from(title_strip_modulation(b)) / 128.0;
            let x0 = TITLE_STRIP_CENTRE_X - (w >> 1) as i32;
            let y0 = row.y - (h >> 1) as i32;
            SpriteDraw {
                dst: (
                    stage_origin.0 + x0 * si,
                    stage_origin.1 + y0 * si,
                    w * scale,
                    h * scale,
                ),
                src,
                color: [tint, tint, tint, alpha],
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_card_draws_the_split_wordmark_and_both_copyright_lines() {
        let out = title_band_sprites(TitleBandState::card(1.0), (0, 0), 1);
        // Two backdrop halves + two copyright lines, nothing else.
        assert_eq!(out.len(), 4);
        assert_eq!(out[0].src.2 + out[1].src.2, 256);
    }

    #[test]
    fn press_start_and_menu_rows_are_their_own_bands() {
        let mut st = TitleBandState::card(1.0);
        st.press_start = true;
        st.menu = Some((0, true));
        let out = title_band_sprites(st, (0, 0), 1);
        assert_eq!(out.len(), 4 + 1 + 2);
    }

    #[test]
    fn the_backdrop_is_the_menu_overlays_title_strip_stack() {
        let card = title_band_sprites(TitleBandState::card(1.0), (0, 0), 1);
        let back = title_band_sprites(TitleBandState::backdrop(), (0, 0), 1);
        assert_eq!(back.len(), 5, "wordmark, two rows, two legal lines");
        // The wordmark lands where the retail GPU scan pinned the title quad.
        assert_eq!(back[0].src, (0, 0, 254, 148));
        assert_eq!((back[0].dst.0, back[0].dst.1), TITLE_ART_POS);
        // Dimmed: brightness 0x3A through the 0xFF colour byte -> 0x39.
        assert!(back[0].color[0] < card[0].color[0]);
        assert_eq!(back[0].color[0], 0x39 as f32 / 128.0);
        // CONTINUE (the cursor row) keeps the level, NEW GAME halves it.
        assert_eq!(back[2].src, TITLE_STRIP_RECORDS[4]);
        assert_eq!(back[2].color[0], 0x39 as f32 / 128.0);
        assert_eq!(back[1].src, TITLE_STRIP_RECORDS[3]);
        assert_eq!(back[1].color[0], 0x1C as f32 / 128.0);
    }

    #[test]
    fn title_strips_centre_on_the_retail_rows() {
        let out = title_strip_sprites(true, 0x80, 1.0, (0, 0), 1);
        let tops: Vec<(i32, i32)> = out.iter().map(|d| (d.dst.0, d.dst.1)).collect();
        // Centres (0xA0, 0x50 / 0xA0 / 0xAE / 0xBE / 0xCC) less half extents.
        assert_eq!(
            tops,
            vec![(33, 6), (128, 152), (128, 166), (33, 182), (33, 196)]
        );
        // The pulse the retail routine computes is dead; brightness is the
        // only per-row input besides the cursor.
        assert_eq!(title_strip_modulation(0x80), 0x7F);
        assert_eq!(title_strip_modulation(0xFF), 0xFE);
    }

    #[test]
    fn the_cursor_row_swaps_the_half_bright_row() {
        let ng = title_strip_rows(false);
        let co = title_strip_rows(true);
        assert!(!ng[1].half_bright && ng[2].half_bright);
        assert!(co[1].half_bright && !co[2].half_bright);
        assert!(ng.iter().chain(co.iter()).all(|r| r.record != 1));
    }

    #[test]
    fn a_scaled_stage_moves_every_band_by_the_same_transform() {
        let one = title_band_sprites(TitleBandState::card(1.0), (0, 0), 1);
        let two = title_band_sprites(TitleBandState::card(1.0), (10, 20), 2);
        for (a, b) in one.iter().zip(two.iter()) {
            assert_eq!(b.dst.0, 10 + a.dst.0 * 2);
            assert_eq!(b.dst.1, 20 + a.dst.1 * 2);
            assert_eq!(b.dst.2, a.dst.2 * 2);
        }
    }
}
