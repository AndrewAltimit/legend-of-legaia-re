//! Disc-gated: the dance HUD's texel source is a **disc** entry, and staging
//! it is what turns every dance quad from a rect that samples nothing into
//! retail's own sprite.
//!
//! The count-in banner had its geometry pinned to the instruction for a long
//! time and still drew as placeholder text on both hosts, because what was
//! missing was residency rather than a draw call: the widget table names a
//! 4bpp page and a CLUT strip that belong to the dance hall's scene, and the
//! port hosts the session over whichever scene the player walked in from.
//! This closes the loop off the user's own image - no baked page index, no
//! baked rect. What it asserts is structural (which rects the table names,
//! that exactly one pack member covers them, that the staged VRAM then
//! carries non-zero texels there); no Sony bytes are reproduced.
//!
//! Skips + passes when `LEGAIA_DISC_BIN` / `extracted/PROT.DAT` are absent.

use std::path::PathBuf;

use legaia_asset::static_overlay;
use legaia_engine_core::dance::{
    DANCE_HUD_ART_PROT_ENTRY, DanceGame, dance_widgets_with_abr, stage_dance_hud_vram,
};
use legaia_engine_core::scene::ProtIndex;

fn prot_dat() -> Option<PathBuf> {
    std::env::var_os("LEGAIA_DISC_BIN")?;
    for p in ["extracted/PROT.DAT", "../../extracted/PROT.DAT"] {
        let f = PathBuf::from(p);
        if f.is_file() {
            return Some(f);
        }
    }
    None
}

fn index() -> Option<ProtIndex> {
    let bytes = std::fs::read(prot_dat()?).ok()?;
    ProtIndex::from_bytes(bytes, None).ok()
}

fn dance_overlay(index: &ProtIndex) -> Option<Vec<u8>> {
    let rec = static_overlay::overlay_map()
        .by_prot_index(legaia_asset::dance_chart::DANCE_OVERLAY_PROT_INDEX as u32)?;
    let raw = index.entry_bytes_extended(rec.prot_index).ok()?;
    static_overlay::as_loaded(&raw, rec).ok()
}

/// The widget table names the banner's page, PROT 1230 carries exactly one
/// member at that origin, and the **second** page three rows name is absent
/// from the pack entirely.
///
/// This is the join the whole feature rests on, and it is the half a
/// geometry-only reading of the record cannot see: `tpage` and `clut` are
/// numbers in an overlay's data segment until something shows which disc
/// entry puts texels under them. Asserting the majority page rather than a
/// lone one is deliberate - the table is not uniform, and a test written to
/// the uniform reading passes only until somebody measures it.
#[test]
fn the_widget_table_and_the_hall_pack_name_the_same_page() {
    let Some(index) = index() else {
        eprintln!("[skip] PROT.DAT unavailable (disc-gated)");
        return;
    };
    let Some(overlay) = dance_overlay(&index) else {
        eprintln!("[skip] dance overlay unavailable (disc-gated)");
        return;
    };
    let widgets = dance_widgets_with_abr(&overlay);
    assert!(!widgets.is_empty(), "the widget table parses off the disc");

    // The banner's own record is the anchor; the page it names is the one
    // the rest of the HUD shares.
    let banner = widgets[0].0;
    let pages = [banner.tpage_xy()];
    let clut_rows = [(banner.clut >> 6) & 0x1FF];
    let on_page = widgets
        .iter()
        .filter(|(w, _)| w.tpage_xy() == pages[0] && (w.clut >> 6) & 0x1FF == clut_rows[0])
        .count();
    assert!(
        on_page * 10 > widgets.len() * 8,
        "the HUD's page is the table's majority page ({on_page} of {} rows)",
        widgets.len()
    );
    // ...and the rows that are NOT on it name at most one other page, which
    // this pack turns out not to carry - a separate residency question.
    let others: Vec<(u16, u16)> = widgets
        .iter()
        .map(|(w, _)| w.tpage_xy())
        .filter(|&p| p != pages[0])
        .fold(Vec::new(), |mut acc, p| {
            if !acc.contains(&p) {
                acc.push(p);
            }
            acc
        });
    assert!(
        others.len() <= 1,
        "at most one page beside the HUD's; got {others:?}"
    );

    // The hall pack carries a member at that origin, and its CLUT block
    // covers the palette columns the widget ids index.
    let raw = index
        .entry_bytes_extended(DANCE_HUD_ART_PROT_ENTRY)
        .expect("hall TIM pack entry reads");
    let members = legaia_prot::timpack::unpack(&raw);
    assert!(
        members.len() > 1,
        "PROT {DANCE_HUD_ART_PROT_ENTRY} unpacks as a TIM pack ({} member(s))",
        members.len()
    );
    let mut at_page = 0usize;
    for m in &members {
        let Ok(tim) = legaia_tim::parse(m) else {
            continue;
        };
        if (tim.image.fb_x, tim.image.fb_y) == pages[0] {
            at_page += 1;
            let clut = tim.clut.as_ref().expect("the HUD page is a CLUT image");
            assert_eq!(
                clut.fb_y, clut_rows[0],
                "the page's CLUT block is the strip the widget ids index"
            );
            // Over the rows on THIS page only: the outlier rows index a
            // column of their own page's strip, not of this one.
            let widest = widgets
                .iter()
                .filter(|(w, _)| w.tpage_xy() == pages[0])
                .map(|(w, _)| w.clut & 0x3F)
                .max()
                .unwrap_or(0);
            assert!(
                u32::from(clut.w) >= u32::from(widest + 1) * 16,
                "the strip covers palette column {widest}"
            );
        }
    }
    assert_eq!(
        at_page, 1,
        "exactly one pack member owns the HUD page {:?}",
        pages[0]
    );
    // The outlier page is genuinely absent from this pack - so a host that
    // staged "every rect the table names" would still not have it, and the
    // rows on it are a question for some other entry.
    for other in &others {
        let n = members
            .iter()
            .filter_map(|m| legaia_tim::parse(m).ok())
            .filter(|t| (t.image.fb_x, t.image.fb_y) == *other)
            .count();
        assert_eq!(n, 0, "the hall pack does not carry page {other:?}");
    }
}

/// Staging writes texels at the named rects and nowhere else, and a rect list
/// the pack does not answer stages nothing (the non-vacuity half - without it
/// a stage that uploaded the whole pack would pass just as well).
#[test]
fn staging_lands_the_hud_page_and_only_it() {
    let Some(index) = index() else {
        eprintln!("[skip] PROT.DAT unavailable (disc-gated)");
        return;
    };
    let Some(overlay) = dance_overlay(&index) else {
        eprintln!("[skip] dance overlay unavailable (disc-gated)");
        return;
    };
    let widgets = dance_widgets_with_abr(&overlay);
    let page = widgets.first().expect("widgets parse").0.tpage_xy();
    let clut_row = (widgets[0].0.clut >> 6) & 0x1FF;
    let rects = vec![(page, (0u16, clut_row))];

    let mut vram = legaia_tim::Vram::new();
    let n = stage_dance_hud_vram(&index, &rects, &mut vram);
    assert_eq!(n, 1, "one member covers the HUD page");

    // Texels landed at the page...
    let page_nonzero = (0..256u16)
        .flat_map(|dy| (0..64u16).map(move |dx| (dx, dy)))
        .filter(|&(dx, dy)| vram.pixel((page.0 + dx) as usize, (page.1 + dy) as usize) != 0)
        .count();
    assert!(
        page_nonzero > 4096,
        "the HUD page holds real texels ({page_nonzero} non-zero halfwords of 16384)"
    );
    // ...and at the CLUT strip.
    let clut_nonzero = (0..256u16)
        .filter(|&dx| vram.pixel(dx as usize, clut_row as usize) != 0)
        .count();
    assert!(
        clut_nonzero > 16,
        "the CLUT strip holds real palette entries ({clut_nonzero} of 256)"
    );
    // Nothing else moved: the field columns this pack would otherwise
    // repaint are untouched, which is the whole reason the stage filters.
    let elsewhere = (0..256u16)
        .flat_map(|dy| (0..64u16).map(move |dx| (dx, dy)))
        .filter(|&(dx, dy)| vram.pixel((704 + dx) as usize, dy as usize) != 0)
        .count();
    assert_eq!(
        elsewhere, 0,
        "a neighbouring 256x256 page of the same pack stays unwritten"
    );

    // Non-vacuity: a page nothing in the pack owns stages nothing.
    let mut empty = legaia_tim::Vram::new();
    assert_eq!(
        stage_dance_hud_vram(&index, &[((960, 496), (0, 496))], &mut empty),
        0,
        "a rect the pack does not answer uploads nothing"
    );
}

/// The count-in banner's own record resolves inside the staged page, and the
/// run exposes it to a host through [`DanceGame::widget`].
#[test]
fn the_countin_record_addresses_the_staged_page() {
    let Some(index) = index() else {
        eprintln!("[skip] PROT.DAT unavailable (disc-gated)");
        return;
    };
    let Some(overlay) = dance_overlay(&index) else {
        eprintln!("[skip] dance overlay unavailable (disc-gated)");
        return;
    };
    let game = DanceGame::from_overlay(&overlay, false).expect("real chart loads");
    let (w, abr) = game.widget(0).expect("the count-in record is reachable");

    // The cell is the banner's, at unit scale: 160x32 stage pixels.
    assert_eq!(
        (w.w, w.h),
        (0xA0, 0x20),
        "record 0 is the 160x32 banner cell"
    );
    assert_eq!(w.scale, 0x1000, "at unit scale");
    assert_eq!(abr, 1, "additive, like every row of the table");
    // Its texel rect fits the 256-wide 4bpp page it names.
    assert!(
        u32::from(w.u) + u32::from(w.w) <= 256,
        "the cell fits the page horizontally"
    );
    assert!(
        u32::from(w.v) + u32::from(w.h) <= 256,
        "the cell fits the page vertically"
    );
    // And the rects the run publishes for staging include that page.
    let rects = game.hud_vram_rects();
    assert!(
        rects.iter().any(|&(p, _)| p == w.tpage_xy()),
        "the run's staging rects name the banner's own page"
    );
}
