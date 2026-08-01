//! Disc-gated oracle for the browser play page's **battle-HUD badge cells**.
//!
//! The nine status-element badges are 48x16 word cells on the system-UI
//! sheet, each on its own row-511 sub-palette. Six decode with the sheet
//! TIM's own sixteen palettes; `Stone`, `Rage` and `Faint` sit on
//! sub-palettes 16 / 17 / 18 and decode with **nothing but** the CLUT-only
//! continuation TIM one file earlier in `PROT.DAT`
//! (`save_menu_atlas::SYSTEM_UI_CLUT_EXT_TIM_OFFSET`, block at VRAM
//! `(256, 511)`).
//!
//! That makes the atlas slice's ROOT a load-bearing choice, and a silent one:
//! a slice rooted at the sheet puts the extension behind its own start, where
//! `build_atlas` cannot reach it. The bake still succeeds, the other six
//! badges still draw, and the three missing cells just fall back to the
//! engine's labelled text tag - which reads like a deliberate fallback rather
//! than a wrong constant.
//!
//! So this test is written as a CONTRAST: the base `crate::play_menu` uses
//! must resolve all nine, and the sheet-rooted base must resolve strictly
//! fewer. A test that only asserted "nine" would keep passing if
//! `build_atlas` ever started inventing cells.
//!
//! No Sony bytes are asserted, only cell presence. Skips + passes when
//! `LEGAIA_DISC_BIN` is unset.

#![cfg(not(target_arch = "wasm32"))]

use legaia_engine_core::save_menu_atlas::{
    SYSTEM_UI_CLUT_EXT_TIM_OFFSET, SaveMenuAtlas, build_atlas,
};
use legaia_engine_core::scene::ProtIndex;
use legaia_web_viewer::disc::{extract_cdname_txt, extract_prot_dat};

/// Build the atlas exactly as a host does, from a panel slice rooted at
/// `base`. `None` when the slice or the bake fails.
fn atlas_rooted_at(index: &ProtIndex, base: usize) -> Option<SaveMenuAtlas> {
    let end = legaia_asset::title_pak::OVERLAY_LOAD_EMPTY_FRAME_TIM_OFFSET
        + legaia_asset::title_pak::OVERLAY_LOAD_EMPTY_FRAME_TIM_SIZE;
    let panel = index.prot_dat_raw_bytes(base as u64, end - base).ok()?;
    let pill = index
        .entry_bytes_extended(legaia_asset::title_pak::PROT_INDEX_OVERLAY as u32)
        .ok()?;
    let glyph = index
        .prot_dat_raw_bytes(
            legaia_asset::menu_glyph_atlas::PROT_DAT_OFFSET,
            legaia_asset::menu_glyph_atlas::TIM_SIZE,
        )
        .ok();
    build_atlas(&panel, &pill, glyph.as_deref()).ok()
}

#[test]
fn the_pages_atlas_base_is_the_one_that_decodes_every_status_badge() {
    let Some(disc_path) = std::env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("LEGAIA_DISC_BIN unset; skipping battle-HUD badge test");
        return;
    };
    let disc = std::fs::read(&disc_path).expect("disc image");
    let prot = extract_prot_dat(&disc).expect("PROT.DAT extraction");
    let cdname = extract_cdname_txt(&disc).expect("CDNAME.TXT extraction");
    let index = ProtIndex::from_bytes(prot, Some(&cdname)).expect("ProtIndex");

    let ext_rooted = atlas_rooted_at(&index, SYSTEM_UI_CLUT_EXT_TIM_OFFSET)
        .expect("atlas from the CLUT-extension-rooted slice");
    let sheet_rooted = atlas_rooted_at(
        &index,
        legaia_asset::title_pak::OVERLAY_SYSTEM_UI_TIM_OFFSET,
    )
    .expect("atlas from the sheet-rooted slice");

    let filled = |a: &SaveMenuAtlas| {
        a.band_status_badges()
            .iter()
            .filter(|c| c.is_some())
            .count()
    };
    let ext_n = filled(&ext_rooted);
    let sheet_n = filled(&sheet_rooted);
    eprintln!("status badges: ext-rooted {ext_n}/9, sheet-rooted {sheet_n}/9");

    assert_eq!(
        ext_n, 9,
        "the base crates/web-viewer/src/play_menu.rs uses must decode all nine \
         status-element badges (Stone / Rage / Faint need the row-511 extension)"
    );
    // The contrast half: if this ever stops being strictly fewer, the
    // extension is no longer what makes the difference and the assertion
    // above has gone vacuous.
    assert!(
        sheet_n < ext_n,
        "sheet-rooted slice resolved {sheet_n}/9 badges - it is supposed to be \
         MISSING the three that need the CLUT extension, so the test above is \
         no longer measuring the base choice"
    );

    // The eight element badges share the sheet's own palettes, so both bases
    // must agree there - a difference would mean the split moved pixels, not
    // just palettes.
    let el = |a: &SaveMenuAtlas| {
        a.band_element_badges()
            .iter()
            .filter(|c| c.is_some())
            .count()
    };
    assert_eq!(
        el(&ext_rooted),
        el(&sheet_rooted),
        "element badges must not depend on the CLUT extension"
    );
}
