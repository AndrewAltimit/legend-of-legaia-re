//! The overworld map display's **place labels** and compass letters - the
//! draw half of PROT 0981's label pass, shared by both hosts.
//!
//! The gate and the projection live in `engine-screens`
//! (`legaia_engine_screens::world_map_labels`) and arrive here as
//! `(name, sx, sy)` triples in 320x240 stage pixels. Per label retail makes
//! three calls (`0x801CF3E4..0x801CF424`):
//!
//! ```text
//! FUN_80036888(name, 0, 0, sx, sy - 1)        ; the name
//! FUN_80034B6C(0)                             ; stage window style 0
//! FUN_8002C69C(sx, sy, FUN_80035F04(name), 8) ; the box
//! ```
//!
//! Style `0`'s descriptor is the dialog skin's, so the box is the dialog
//! window chrome around a `width x 8` centre rect. The emitter links at the
//! head of its ordering-table bucket, so the name sits over its own box and
//! an earlier record's box over a later one's; [`map_label_chrome_draws_for`]
//! returns the boxes last record first to keep that.
//!
//! REF: FUN_801CE850, FUN_80036888, FUN_80035F04, FUN_8002C69C

use crate::*;

/// One label as the host hands it over: the name's bytes and the projected
/// screen point.
pub type MapLabelDraw<'a> = (&'a [u8], i32, i32);

/// A label's measured width - `FUN_80035F04`, the rendered advance of the
/// name in the dialog font.
pub fn map_label_text_width(font: &legaia_font::Font, name: &[u8]) -> i32 {
    font.layout(name).advance_x.min(i32::MAX as u32) as i32
}

/// The boxes behind every label, later records first.
pub fn map_label_chrome_draws_for(
    rects: &SaveMenuAtlasRects,
    font: &legaia_font::Font,
    labels: &[MapLabelDraw<'_>],
    stage_origin: (i32, i32),
    stage_scale: u32,
) -> Vec<SpriteDraw> {
    let mut out = Vec::new();
    for &(name, sx, sy) in labels.iter().rev() {
        if name.is_empty() {
            continue;
        }
        out.extend(dialog_window_chrome_draws_for(
            rects,
            (sx, sy, map_label_text_width(font, name), 8),
            stage_origin,
            stage_scale,
        ));
    }
    out
}

/// Every label's name at its pen `(sx, sy - 1)`, in stage pixels (the host
/// scales them with [`scale_stage_text_draws`]).
pub fn map_label_text_draws_for(
    font: &legaia_font::Font,
    labels: &[MapLabelDraw<'_>],
) -> Vec<TextDraw> {
    let mut out = Vec::new();
    for &(name, sx, sy) in labels {
        if name.is_empty() {
            continue;
        }
        out.extend(text_draws_for(
            &font.layout(name),
            (sx, sy - 1),
            MENU_TEXT_WHITE,
        ));
    }
    out
}

/// The mini-map panel's compass letters, each at its pen, in stage pixels.
pub fn map_compass_text_draws_for(
    font: &legaia_font::Font,
    marks: &[(u8, (i32, i32))],
) -> Vec<TextDraw> {
    let mut out = Vec::new();
    for &(letter, pen) in marks {
        out.extend(text_draws_for(
            &font.layout(&[letter]),
            pen,
            MENU_TEXT_WHITE,
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn font() -> legaia_font::Font {
        legaia_font::Font::placeholder()
    }

    #[test]
    fn a_name_is_drawn_one_row_above_its_box_origin() {
        let f = font();
        let draws = map_label_text_draws_for(&f, &[(b"Rim Elm", 100, 60)]);
        assert!(!draws.is_empty());
        assert!(draws.iter().all(|d| d.dst.1 == 59));
        assert!(draws[0].dst.0 >= 100);
    }

    #[test]
    fn the_box_is_as_wide_as_the_name_and_empty_names_draw_nothing() {
        let f = font();
        assert!(map_label_text_width(&f, b"Sol Tower") > map_label_text_width(&f, b"Sol"));
        assert!(map_label_text_draws_for(&f, &[(b"", 1, 1)]).is_empty());
        assert_eq!(map_compass_text_draws_for(&f, &[]).len(), 0);
        assert!(!map_compass_text_draws_for(&f, &[(b'N', (0x98, 0x0A))]).is_empty());
    }
}
