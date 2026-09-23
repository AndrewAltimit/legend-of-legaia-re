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
/// `dim` is retail's save-screen backdrop level: the title art stays on
/// screen behind the Load/Save chrome at roughly 45 % of its brightness, with
/// the NEW GAME / CONTINUE rows drawn cursor-less. That is a separate knob
/// from `alpha`, which is the title's own fade-in ramp.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TitleBandState {
    /// Fade-in ramp, `0.0..=1.0`. `1.0` once the card is up.
    pub alpha: f32,
    /// Backdrop dim - the save-screen level rather than the title level.
    pub dim: bool,
    /// Draw the "PRESS START BUTTON" band (its own phase only).
    pub press_start: bool,
    /// `(cursor row, has focus)` when the NEW GAME / CONTINUE rows draw.
    /// `has_focus == false` draws both rows dim, which is what the
    /// save-screen backdrop wants: the rows sit behind the panel and no
    /// cursor is on them.
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

    /// The save-screen backdrop: dim, no prompt, both rows drawn cursor-less.
    pub fn backdrop() -> Self {
        Self {
            alpha: 1.0,
            dim: true,
            press_start: false,
            menu: Some((1, false)),
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
/// split at the VRAM texture-page seam. Both the fade alpha and the backdrop
/// dim fold into that one byte.
pub fn title_band_sprites(
    state: TitleBandState,
    stage_origin: (i32, i32),
    stage_scale: u32,
) -> Vec<SpriteDraw> {
    use legaia_asset::title_pak;
    let scale = stage_scale.max(1);
    let si = scale as i32;
    let alpha = state.alpha.clamp(0.0, 1.0);
    let lum = if state.dim { TITLE_BACKDROP_LUM } else { 1.0 };
    let color = [lum, lum, lum, alpha];
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
    fn the_backdrop_dims_the_wordmark_and_drops_the_cursor() {
        let card = title_band_sprites(TitleBandState::card(1.0), (0, 0), 1);
        let back = title_band_sprites(TitleBandState::backdrop(), (0, 0), 1);
        // The wordmark's modulation byte is the dim: 0x80 neutral -> 0x3A.
        assert!(back[0].color[0] < card[0].color[0]);
        assert_eq!(back[0].color[0], 0x3A as f32 / 128.0);
        // Both rows carry the unselected tint - no row is lit.
        assert_eq!(back[2].color, back[3].color);
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
