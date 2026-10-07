//! The save-select screen as **one** composition: the panel title and pill
//! rows, the pills and their cursor, the "Now checking" beat, the preview's
//! block grid + info panel (or its caption), and the confirm messagebox -
//! both halves, text and sprites.
//!
//! Every play host reaches the same screen from two doors (the boot Continue
//! -> Load path and the pause menu's Load / Save rows), and each host used to
//! sequence the overlays itself, the text half in one function and the
//! sprite half in another. The page also returned before its text overlays
//! whenever the chrome atlas was absent, so a session without the atlas
//! showed the title and nothing else where the native window still printed
//! every line. Here the text half always draws and the sprite half draws
//! when `rects` is present, with the atlas-only label sprites switching the
//! text half's ASCII stand-ins off.
//!
//! The model comes from `legaia_engine_core::save_select::overlay_model`,
//! which a host borrows into [`SaveSelectOverlayView`]; the slide positions
//! are resolved here from the session's two 12-bit timers because the
//! endpoints are this crate's constants.

use crate::*;

/// The picked block's preview: the 5x3 grid, the info panel (or its
/// caption when the block holds nothing loadable) and the panel's slide.
#[derive(Clone, Copy)]
pub struct SaveSelectPreviewView<'a> {
    pub cells: &'a [SlotGridCell],
    /// The focused grid cell.
    pub cell: u8,
    pub info: Option<SlotInfoView<'a>>,
    pub caption: Option<&'a str>,
    /// The info panel's slide delta from its parked y (`0` = landed).
    pub panel_y_offset: i32,
}

/// A save-select screen for [`save_select_overlay_draws`].
#[derive(Clone, Copy)]
pub struct SaveSelectOverlayView<'a> {
    /// The header tab's word (`Load` / `Save`).
    pub title: &'a str,
    pub rows: &'a [SaveSelectRow<'a>],
    /// The row the text cursor sits on.
    pub cursor: usize,
    /// Only the committed card's pill, sliding up under the panel.
    pub single_pill: bool,
    /// Which pills draw.
    pub pills: &'a [u8],
    /// Draw the pointing hand on the pill row, at this row.
    pub pill_cursor: Option<usize>,
    /// The session's slide timer (`0..=0x1000`): the pill relocation and the
    /// "Now checking" panel ride it.
    pub slide_t: u16,
    /// The info-panel timer: the confirm messagebox rides it too.
    pub info_t: u16,
    pub now_checking: bool,
    /// A card-operation messagebox over the preview: the write / read beat
    /// and the result line after a confirmed Save or Load (two lines; an
    /// empty second line centres the first). Drawn on the "Now checking"
    /// panel.
    pub banner: Option<CardBannerView<'a>>,
    pub preview: Option<SaveSelectPreviewView<'a>>,
    /// The confirm prompt and its Yes / No cursor.
    pub confirm: Option<(&'a str, u8)>,
}

/// [`save_select_overlay_draws`]'s output.
#[derive(Default)]
pub struct SaveSelectOverlayDraws {
    pub texts: Vec<TextDraw>,
    pub sprites: Vec<SpriteDraw>,
}

/// Retail's slide, `FUN_801E1C1C`: `start + (target - start) * t / 0x1000`
/// (the engine's `save_select::interpolate_anim`).
fn slide(start: i32, target: i32, t: u16) -> i32 {
    start + (target - start) * i32::from(t) / 0x1000
}

/// Compose the save-select screen. `rects` is the system-UI atlas; without
/// it only the text half draws, with ASCII stand-ins for the cursor and the
/// info panel's labels.
pub fn save_select_overlay_draws(
    font: &legaia_font::Font,
    rects: Option<&SaveMenuAtlasRects>,
    view: &SaveSelectOverlayView<'_>,
    stage_origin: (i32, i32),
    stage_scale: u32,
) -> SaveSelectOverlayDraws {
    let chrome = rects.is_some();
    // Once the write / read panel has slid all the way in, retail stops
    // drawing everything under it: the dispatcher's shared tail emits the
    // header tab, the pill and the block grid only while the panel's slide
    // timer `_DAT_801F01CC` is short of `0x1000` (`0x801DFCDC..0x801DFCE4`
    // in `FUN_801DD35C`), and the panel's own subtractive push
    // (`FUN_80024EE4(1, 2, t >> 4)`) has blacked the frame by then. The
    // result line keeps the parked timer, so it sits on black too.
    if let Some(banner) = view.banner.as_ref()
        && banner.slide_t >= 0x1000
    {
        let mut out = SaveSelectOverlayDraws::default();
        let block = view.preview.as_ref().map(|p| p.cell);
        let (sprites, texts) =
            card_banner_draws_for(font, rects, banner, block, stage_origin, stage_scale);
        out.sprites.extend(sprites);
        out.texts.extend(texts);
        return out;
    }
    let mut out = SaveSelectOverlayDraws {
        texts: save_select_draws_for(
            font,
            view.title,
            view.rows,
            view.cursor,
            None,
            stage_origin,
            stage_scale,
            !chrome,
        ),
        sprites: Vec::new(),
    };
    if let Some(rects) = rects {
        let anchor = if view.single_pill {
            (
                slide(
                    SAVE_SELECT_SLOT1_POS.0,
                    SAVE_SELECT_SLOT1_POS_LOAD_ACTIVE.0,
                    view.slide_t,
                ),
                slide(
                    SAVE_SELECT_SLOT1_POS.1,
                    SAVE_SELECT_SLOT1_POS_LOAD_ACTIVE.1,
                    view.slide_t,
                ),
            )
        } else {
            SAVE_SELECT_SLOT1_POS
        };
        out.sprites.extend(save_select_chrome_draws_for(
            rects,
            view.pills,
            anchor,
            stage_origin,
            stage_scale,
        ));
        if let Some(row) = view.pill_cursor {
            out.sprites.push(save_select_cursor_draw_for(
                rects,
                row,
                stage_origin,
                stage_scale,
            ));
        }
    }
    if view.now_checking {
        let x = slide(
            NOW_CHECKING_SLIDE_START_X,
            NOW_CHECKING_SLIDE_TARGET_X,
            view.slide_t,
        );
        let offset = (x - NOW_CHECKING_SLIDE_TARGET_X, 0);
        if let Some(rects) = rects {
            out.sprites.extend(now_checking_panel_draws_for(
                rects,
                stage_origin,
                stage_scale,
                offset,
            ));
        }
        out.texts.extend(now_checking_text_draws_for(
            font,
            stage_origin,
            stage_scale,
            offset,
        ));
    }
    if let Some(p) = view.preview.as_ref() {
        if let Some(rects) = rects {
            out.sprites.extend(slot_preview_grid_draws_for(
                rects,
                p.cells,
                p.cell,
                stage_origin,
                stage_scale,
            ));
            out.sprites.extend(slot_info_panel_draws_for(
                rects,
                p.info.as_ref(),
                p.panel_y_offset,
                stage_origin,
                stage_scale,
            ));
        }
        out.texts.extend(slot_info_panel_text_draws_for(
            font,
            p.info.as_ref(),
            p.panel_y_offset,
            stage_origin,
            stage_scale,
            chrome,
        ));
        if p.info.is_none()
            && let Some(caption) = p.caption
        {
            out.texts.extend(slot_info_caption_draws_for(
                font,
                caption,
                p.panel_y_offset,
                stage_origin,
                stage_scale,
            ));
        }
    }
    if let Some(banner) = view.banner.as_ref() {
        let block = view.preview.as_ref().map(|p| p.cell);
        let (sprites, texts) =
            card_banner_draws_for(font, rects, banner, block, stage_origin, stage_scale);
        out.sprites.extend(sprites);
        out.texts.extend(texts);
    }
    // The confirm is retail's centred messagebox (mode 3 of the slide-in
    // primitive), sliding up from below the stage on top of the preview.
    if let Some((prompt, cursor)) = view.confirm {
        let y = slide(
            CONFIRM_DIALOG_SLIDE_START_Y,
            CONFIRM_DIALOG_SLIDE_TARGET_Y,
            view.info_t,
        );
        if let Some(rects) = rects {
            out.sprites.extend(confirm_dialog_panel_draws_for(
                rects,
                y,
                stage_origin,
                stage_scale,
            ));
            // The badge sits on the prompt bar, so it draws after it.
            if let Some(p) = view.preview.as_ref() {
                out.sprites.extend(confirm_dialog_badge_draws_for(
                    prompt,
                    p.cell,
                    y,
                    stage_origin,
                    stage_scale,
                ));
            }
        }
        out.texts.extend(confirm_dialog_text_draws_for(
            font,
            prompt,
            cursor,
            y,
            stage_origin,
            stage_scale,
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Without the chrome atlas the phase overlays still print: the
    /// confirm's prompt and the caption reach the text half.
    #[test]
    fn the_text_half_draws_without_the_atlas() {
        let font = legaia_font::Font::placeholder();
        let cells = [SlotGridCell::default(); 15];
        let base = SaveSelectOverlayView {
            title: "Save",
            rows: &[],
            cursor: 0,
            single_pill: true,
            pills: &[0],
            pill_cursor: None,
            slide_t: 0x1000,
            info_t: 0x1000,
            now_checking: false,
            banner: None,
            preview: None,
            confirm: None,
        };
        let title_only = save_select_overlay_draws(&font, None, &base, (0, 0), 1);
        let view = SaveSelectOverlayView {
            preview: Some(SaveSelectPreviewView {
                cells: &cells,
                cell: 0,
                info: None,
                caption: Some("Free block"),
                panel_y_offset: 0,
            }),
            confirm: Some(("Do you wish to save?", 0)),
            ..base
        };
        let out = save_select_overlay_draws(&font, None, &view, (0, 0), 1);
        assert!(out.sprites.is_empty(), "no atlas, no sprites");
        assert!(
            out.texts.len() > title_only.texts.len(),
            "caption + confirm text drew"
        );
    }

    /// A parked write panel is drawn alone: the header, pills and grid stop
    /// under it, as retail's dispatcher tail stops them at `0x1000`.
    #[test]
    fn a_parked_write_panel_hides_the_screen_under_it() {
        let font = legaia_font::Font::placeholder();
        let cells = [SlotGridCell::default(); 15];
        let banner = |slide_t| CardBannerView {
            lines: ("  Saving to MEMORY CARD", "Do not remove MEMORY CARD"),
            note: "",
            work: true,
            slide_t,
            progress_t: 0,
        };
        let base = SaveSelectOverlayView {
            title: "Save",
            rows: &[],
            cursor: 0,
            single_pill: true,
            pills: &[0],
            pill_cursor: None,
            slide_t: 0x1000,
            info_t: 0x1000,
            now_checking: false,
            banner: Some(banner(0x800)),
            preview: Some(SaveSelectPreviewView {
                cells: &cells,
                cell: 0,
                info: None,
                caption: Some("Able to save."),
                panel_y_offset: 0,
            }),
            confirm: None,
        };
        let sliding = save_select_overlay_draws(&font, None, &base, (0, 0), 1);
        let parked = save_select_overlay_draws(
            &font,
            None,
            &SaveSelectOverlayView {
                banner: Some(banner(0x1000)),
                ..base
            },
            (0, 0),
            1,
        );
        let (_, alone) = card_banner_draws_for(&font, None, &banner(0x1000), Some(0), (0, 0), 1);
        assert_eq!(parked.texts.len(), alone.len(), "only the banner draws");
        assert!(sliding.texts.len() > parked.texts.len());
    }

    #[test]
    fn the_slide_matches_the_retail_lerp_endpoints() {
        assert_eq!(slide(416, 160, 0), 416);
        assert_eq!(slide(416, 160, 0x1000), 160);
        assert_eq!(slide(344, 88, 0x800), 216);
    }
}
