//! The tab banner and its label.

use crate::*;

/// Compose the retail **tab-banner plaque** for a field-menu title tab
/// (the carved brown plaque behind "Status" / "Equip" / "Options") as
/// atlas [`SpriteDraw`]s.
///
/// Retail draws the class-2 tab window's entire chrome as six sprites
/// (RAM prim scan over the `menu_status_town` capture): a left cap at
/// `(WX-8, WY-4)`, the 16x20 body tile repeated across the tab's content
/// width `w` (with a partial remainder), and a right cap at `(WX+w,
/// WY-4)`. No gold 9-slice frame or filigree interior is drawn for tab
/// windows - the label text lands directly on the plaque at the content
/// origin. `pen` is the tab window's content origin and `content_w` its
/// descriptor width (60 in the retail table).
///
/// REF: FUN_801DCAD8 / FUN_801DCA94 / FUN_801DCB1C - the tab content
/// renderers (label string only; the plaque is caller-drawn chrome).
pub fn tab_banner_draws(
    rects: &SaveMenuAtlasRects,
    pen: (i32, i32),
    content_w: i32,
    stage_origin: (i32, i32),
    stage_scale: u32,
) -> Vec<SpriteDraw> {
    let (wx, wy) = pen;
    let y = wy - 4;
    let scale = stage_scale.max(1) as i32;
    let mut out = Vec::with_capacity(6);
    let mut push = |src: (u32, u32, u32, u32), sx: i32, sy: i32| {
        let (_, _, w, h) = src;
        out.push(SpriteDraw {
            dst: (
                stage_origin.0 + sx * scale,
                stage_origin.1 + sy * scale,
                w * stage_scale,
                h * stage_scale,
            ),
            src,
            color: [1.0, 1.0, 1.0, 1.0],
        });
    };
    push(rects.tab_cap_l, wx - 8, y);
    let (bx, by, bw, bh) = rects.tab_body;
    let mut x = wx;
    // A zero-width body cell (an atlas without the tab plaque) would never
    // advance `x`: the loop pushed draws until memory ran out.
    while bw > 0 && x < wx + content_w {
        let this_w = (wx + content_w - x).min(bw as i32);
        push((bx, by, this_w as u32, bh), x, y);
        x += this_w;
    }
    push(rects.tab_cap_r, wx + content_w, y);
    out
}

/// Build the [`TextDraw`]s for a field-menu title tab's **label** - the
/// word that lands on the plaque [`tab_banner_draws`] composes.
///
/// The five title tabs share one renderer shape: stage the text ink to
/// index 7 (white), then emit the label at the window's content origin
/// with no offset of its own. They differ only in which string they
/// pass, which is why one function serves all five - the tab identity
/// lives in the caller's `label`, exactly as it lives in the retail
/// renderer's string operand.
///
/// `pen` is the tab window's content origin, the same `pen`
/// [`tab_banner_draws`] takes, so a caller frames the plaque and labels
/// it from one pair of coordinates.
///
/// Retained as the pen-only entry point for a caller that has no parsed
/// descriptor to dispatch through (`crate::menu::window_dispatch`); the
/// geometry itself is
/// [`title_tab_draws_for`](crate::menu::window_painters::title_tab_draws_for),
/// which is the same routine window 43 names.
///
/// REF: FUN_801DCA0C (Items), FUN_801DCA50 (Magic), FUN_801DCA94 (Equip),
/// REF: FUN_801DCAD8 (Status), FUN_801DCB1C (Options) - all five are ported
/// by `title_tab_draws_for`, which this delegates to.
pub fn tab_label_draws(font: &legaia_font::Font, label: &str, pen: (i32, i32)) -> Vec<TextDraw> {
    crate::menu::window_painters::title_tab_draws_for(
        font,
        crate::menu::window_painters::PainterRect::new(pen.0, pen.1, 0, 0),
        label,
    )
}

#[cfg(test)]
mod tab_label_tests {
    use super::*;

    /// The label is drawn from the window content origin - the plaque is
    /// positioned around it, not the other way round - and its first
    /// visible glyph sits one glyph pad right of it: the retail literal is
    /// `"@Status"`, whose blank `@` costs only the pad.
    #[test]
    fn label_starts_one_lead_past_the_content_origin() {
        let font = legaia_font::synthetic_for_tests();
        let draws = tab_label_draws(&font, "Status", (90, 16));
        assert!(!draws.is_empty());
        let lead = crate::menu::window_painters::menu_string_lead(&font);
        assert_eq!(draws[0].dst.0, 90 + lead);
        assert_eq!(draws[0].dst.1, 16);
    }

    /// On the retail font the lead is exactly one pixel: `@` is width `0`.
    #[test]
    fn the_lead_is_one_pixel_on_the_retail_widths() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../extracted");
        let Ok(font) = legaia_font::Font::load_from_extracted(&root) else {
            eprintln!("[skip] extracted font missing");
            return;
        };
        assert_eq!(crate::menu::window_painters::menu_string_lead(&font), 1);
    }

    /// All five tabs ink white (retail staging index 7); none of them
    /// stages a different colour.
    #[test]
    fn every_tab_label_inks_white() {
        let font = legaia_font::synthetic_for_tests();
        for label in ["Items", "Magic", "Equip", "Status", "Options"] {
            let draws = tab_label_draws(&font, label, (0, 0));
            assert!(
                draws.iter().all(|d| d.color == MENU_TEXT_WHITE),
                "{label} inked off-white"
            );
        }
    }

    /// Each tab's label is the only thing that differs between the five
    /// renderers, so a different word must produce a different run of
    /// glyphs at the same pen.
    #[test]
    fn tab_identity_lives_in_the_label() {
        let font = legaia_font::synthetic_for_tests();
        let items = tab_label_draws(&font, "Items", (90, 16));
        let magic = tab_label_draws(&font, "Magic", (90, 16));
        assert_eq!(items[0].dst.0, magic[0].dst.0);
        assert_ne!(
            items.iter().map(|d| d.src).collect::<Vec<_>>(),
            magic.iter().map(|d| d.src).collect::<Vec<_>>()
        );
    }

    /// An atlas with no tab plaque (every rect zero, as `Default` builds)
    /// still terminates: the body loop used to spin on a zero-width cell,
    /// appending draws until the process ran out of memory.
    #[test]
    fn zero_width_body_cell_draws_only_the_caps() {
        let rects = SaveMenuAtlasRects::default();
        let draws = tab_banner_draws(&rects, (40, 20), 96, (0, 0), 2);
        assert_eq!(draws.len(), 2);
    }
}
