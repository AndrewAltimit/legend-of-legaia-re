//! Disc-gated: the red cross-out X both play hosts draw over a forbidden
//! Ra-Seru chip bakes into the chrome atlas from the battle effect page
//! (PROT 870, page `(448, 0)`, CLUT `(64, 476)`, texels `(0, 96)`..`(63, 111)`),
//! into a cell no other atlas source writes.
//!
//! Skip-passes without disc data / extracted assets (CLAUDE.md convention).

use legaia_engine_core::save_menu_atlas as sma;
use legaia_engine_core::scene::SceneHost;
use std::path::PathBuf;

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

#[test]
fn the_cross_out_mark_bakes_into_a_free_atlas_cell() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };
    let host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    let idx = &host.index;
    // The window both hosts build the atlas from.
    let base = sma::SYSTEM_UI_CLUT_EXT_TIM_OFFSET as u64;
    let end = (legaia_asset::title_pak::OVERLAY_LOAD_EMPTY_FRAME_TIM_OFFSET
        + legaia_asset::title_pak::OVERLAY_LOAD_EMPTY_FRAME_TIM_SIZE) as u64;
    let panel = idx
        .prot_dat_raw_bytes(base, (end - base) as usize)
        .expect("system-UI slice");
    let pill = idx
        .entry_bytes_extended(legaia_asset::title_pak::PROT_INDEX_OVERLAY as u32)
        .expect("PROT 0899");
    let glyph = idx
        .prot_dat_raw_bytes(
            legaia_asset::menu_glyph_atlas::PROT_DAT_OFFSET,
            legaia_asset::menu_glyph_atlas::TIM_SIZE,
        )
        .ok();
    let mut atlas = sma::build_atlas(&panel, &pill, glyph.as_deref()).expect("atlas");
    let cell = sma::ATLAS_RECT_CROSS_OUT;
    let alpha = |a: &sma::SaveMenuAtlas| {
        let mut n = 0usize;
        let mut red = 0usize;
        for y in cell.1..cell.1 + cell.3 {
            for x in cell.0..cell.0 + cell.2 {
                let o = ((y * a.width + x) * 4) as usize;
                if a.rgba[o + 3] != 0 {
                    n += 1;
                    if a.rgba[o] > a.rgba[o + 1] && a.rgba[o] > a.rgba[o + 2] {
                        red += 1;
                    }
                }
            }
        }
        (n, red)
    };
    assert_eq!(alpha(&atlas).0, 0, "no other source writes the X's cell");
    assert_eq!(atlas.band_cross_out(), None);
    let flame = idx
        .entry_bytes_extended(sma::FLAME_ATLAS_PROT_ENTRY)
        .expect("PROT 870");
    assert!(sma::add_cross_out_mark(&mut atlas, &flame), "the X bakes");
    assert_eq!(atlas.band_cross_out(), Some(cell));
    let (ink, red) = alpha(&atlas);
    assert!(ink > 64, "the X carries ink ({ink} texels)");
    assert!(red * 2 > ink, "and it is red ({red} of {ink})");
    eprintln!("[ok] cross-out X baked at {cell:?}: {ink} texels, {red} red");
}
