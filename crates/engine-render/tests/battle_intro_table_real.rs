//! Disc oracle for the transition's sprite descriptor table.
//!
//! The curtain style is the one field-to-battle transition the port draws end
//! to end, and its only disc input is the `0x14`-stride record table at overlay
//! VA `0x801D1EC4` inside PROT 0979 `field_battle_intro`. The unit tests drive
//! that style through a synthetic stand-in
//! (`legaia_engine_render::battle_intro::IntroQuadTable::neutral`), which
//! proves the arithmetic and proves nothing about the disc.
//!
//! This closes that gap: it reads the real entry, relocates it to the base
//! `crates/asset/data/static-overlays.toml` pins, parses the table, and checks
//! the records against what the *code* does with them. The table carries no
//! count and no magic, so "it parses" is not a claim - what makes this an
//! oracle is that the two records the style patches have exactly the shapes
//! the two passes need, and that their texture pages decode to the VRAM rects
//! the capture writes.
//!
//! Skips and passes with `LEGAIA_DISC_BIN` unset. No disc bytes are asserted
//! literally; every expectation is a decoded property.

use legaia_engine_render::battle_intro::{
    INTRO_QUAD_TABLE_LEN, INTRO_QUAD_TABLE_VA, IntroQuadTable,
};
use legaia_engine_render::vram_capture::{FIELD_CAPTURE_COLS, FIELD_CAPTURE_ROWS};
use legaia_engine_vm::battle_intro_styles::{
    CURTAIN_COL_DESC, CURTAIN_COL_TPAGE_LEFT, CURTAIN_COL_TPAGE_RIGHT, CURTAIN_LEFT_W,
    CURTAIN_RIGHT_W, CURTAIN_ROW_DESC, CURTAIN_ROW_TPAGE_LEFT, CURTAIN_ROW_TPAGE_RIGHT,
};

/// PROT index of the field-to-battle transition overlay.
const INTRO_OVERLAY_PROT: u32 = 979;

/// Decode a GP0 texture-page word into its VRAM origin and colour depth.
///
/// TSB bits `0..=3` are the X base in 64-halfword units, bit `4` the Y base
/// (0 or 256), bits `7..=8` the depth (`0` = 4bpp, `1` = 8bpp, `2` = 15bpp).
fn decode_tpage(tsb: u16) -> (u16, u16, u8) {
    (
        (tsb & 0x0F) * 64,
        if tsb & 0x10 != 0 { 256 } else { 0 },
        ((tsb >> 7) & 0x3) as u8,
    )
}

/// The overlay image at its pinned load base, or `None` when the disc-gated
/// inputs are absent (the workspace convention: skip and pass).
fn overlay_image() -> Option<(Vec<u8>, u32)> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    let prot = ["extracted", "../../extracted"]
        .into_iter()
        .map(|b| std::path::PathBuf::from(b).join("PROT.DAT"))
        .find(|p| p.is_file())?;
    let mut archive = legaia_prot::archive::Archive::open(&prot).ok()?;
    let entry = archive.entries.get(INTRO_OVERLAY_PROT as usize)?.clone();
    let mut raw = Vec::new();
    archive.read_entry(&entry, &mut raw).ok()?;
    let rec = legaia_asset::static_overlay::overlay_map().by_prot_index(INTRO_OVERLAY_PROT)?;
    let as_loaded = legaia_asset::static_overlay::as_loaded(&raw, rec).ok()?;
    Some((as_loaded, rec.base_va))
}

#[test]
fn the_real_descriptor_table_has_the_shape_the_curtain_patches() {
    let Some((image, base)) = overlay_image() else {
        eprintln!("LEGAIA_DISC_BIN unset or PROT 0979 unreadable; skipping");
        return;
    };
    let table = IntroQuadTable::parse_overlay(&image, base).expect("table is inside the image");
    assert_eq!(table.0.len(), INTRO_QUAD_TABLE_LEN);
    assert!(
        INTRO_QUAD_TABLE_VA > base,
        "the table VA must sit inside the pinned base"
    );

    // Every record the style touches is unity-scaled: `build_intro_quad`
    // multiplies the extent by `size_q12` before the caller's own scale, and
    // both curtain passes pass 0x1000 for x, so anything else would resize
    // every strip.
    for i in [CURTAIN_ROW_DESC, CURTAIN_COL_DESC] {
        assert_eq!(
            table.0[i].size_q12, 0x1000,
            "record {i} is not unity-scaled"
        );
    }

    // The row pass patches width and v per scanline but leaves `h`, so the
    // record has to be one texel tall - a scanline. The column pass patches u
    // and leaves `w` and `h`, so it has to be one texel wide and a full
    // screen tall.
    let row = table.0[CURTAIN_ROW_DESC];
    assert_eq!(
        (row.w, row.h),
        (1, 1),
        "the row record is not a single texel"
    );
    let col = table.0[CURTAIN_COL_DESC];
    assert_eq!(col.w, 1, "the column record is not one texel wide");
    assert_eq!(col.h, 240, "the column record is not a full screen tall");

    // Both edges white: with the passes' 0x80 intensity that shades to 0x7F,
    // the neutral PSX texture modulation, so the strips carry the captured
    // frame rather than tinting it.
    for i in [CURTAIN_ROW_DESC, CURTAIN_COL_DESC] {
        assert_eq!(table.0[i].top, [0xFF; 3], "record {i} top edge");
        assert_eq!(table.0[i].bottom, [0xFF; 3], "record {i} bottom edge");
    }

    // Records 0 and 1 are the full-screen halves the mid-pass rectangles use,
    // and their widths are the same split the row pass draws with.
    assert_eq!(table.0[0].w, CURTAIN_LEFT_W);
    assert_eq!(table.0[1].w, CURTAIN_RIGHT_W);
    assert_eq!(
        u16::from(table.0[0].w) + u16::from(table.0[1].w),
        320,
        "the two halves do not span one scanline"
    );
}

#[test]
fn the_styles_texture_pages_land_in_the_rects_the_capture_writes() {
    // Not disc-gated: the four page words are code constants. It belongs here
    // because it is the other half of the same claim - the table above says
    // *what* is drawn, this says *where it is sampled from*, and together they
    // are why a capture into these two rects makes the curtain show the field.
    let rows = [CURTAIN_ROW_TPAGE_LEFT, CURTAIN_ROW_TPAGE_RIGHT];
    let cols = [CURTAIN_COL_TPAGE_LEFT, CURTAIN_COL_TPAGE_RIGHT];

    for (pass, pages, rect) in [
        ("row", rows, FIELD_CAPTURE_ROWS),
        ("column", cols, FIELD_CAPTURE_COLS),
    ] {
        for (i, p) in pages.iter().enumerate() {
            let (x, y, depth) = decode_tpage(*p);
            assert_eq!(depth, 2, "{pass} page {i} is not 15-bpp");
            assert_eq!(y, rect.y, "{pass} page {i} is on the wrong VRAM row");
            assert!(
                x >= rect.x && x < rect.x + rect.w,
                "{pass} page {i} origin {x} is outside the capture rect"
            );
        }
        // The two pages are 192 halfwords apart, and the pass draws 0xC0 texels
        // from the first then 0x80 from the second - so together they cover the
        // rect's full 320-pixel width with no gap and no overlap.
        let (x0, _, _) = decode_tpage(pages[0]);
        let (x1, _, _) = decode_tpage(pages[1]);
        assert_eq!(x0, rect.x, "{pass} first page is not at the rect origin");
        assert_eq!(
            x0 + u16::from(CURTAIN_LEFT_W),
            x1,
            "{pass}: the first page's 0xC0 texels do not reach the second"
        );
        assert_eq!(
            x1 + u16::from(CURTAIN_RIGHT_W),
            rect.x + rect.w,
            "{pass}: the two pages do not end at the rect's right edge"
        );
    }
}
