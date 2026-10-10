//! Disc-gated: the command ring's three marks bake into the chrome atlas from
//! the battle effect page (PROT 870, page `(448, 0)`), each into a cell no
//! other atlas source writes:
//!
//! - the red cross-out X both play hosts draw over a forbidden Ra-Seru chip
//!   (CLUT `(64, 476)`, texels `(0, 96)`..`(63, 111)`, `FUN_801DBC30`);
//! - the blue Rot stamp over a refused Attack chip and each rotted arts-entry
//!   direction (CLUT `(176, 476)`, texels `(80, 96)`..`(111, 119)`,
//!   `FUN_801DBD04` / `FUN_801DBDDC`);
//! - the blue Curse plate over a refused Magic chip (CLUT `(0, 476)`, texels
//!   `(120, 96)`..`(183, 111)`, `FUN_801DBEC4`).
//!
//! Skip-passes without disc data / extracted assets (CLAUDE.md convention).

use legaia_engine_core::save_menu_atlas as sma;
use legaia_engine_core::scene::SceneHost;
use std::path::PathBuf;

fn extracted_dir() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("LEGAIA_EXTRACTED_DIR").map(PathBuf::from)
        && d.join("PROT.DAT").exists()
    {
        return Some(d);
    }
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

/// `(opaque texels, texels passing `pick`)` inside `cell`.
fn ink_in(
    a: &sma::SaveMenuAtlas,
    cell: (u32, u32, u32, u32),
    pick: impl Fn(&[u8]) -> bool,
) -> (usize, usize) {
    let mut n = 0usize;
    let mut picked = 0usize;
    for y in cell.1..cell.1 + cell.3 {
        for x in cell.0..cell.0 + cell.2 {
            let o = ((y * a.width + x) * 4) as usize;
            if a.rgba[o + 3] != 0 {
                n += 1;
                if pick(&a.rgba[o..o + 4]) {
                    picked += 1;
                }
            }
        }
    }
    (n, picked)
}

fn red(p: &[u8]) -> bool {
    p[0] > p[1] && p[0] > p[2]
}

fn blue(p: &[u8]) -> bool {
    p[2] > p[0]
}

#[test]
fn the_ring_marks_bake_into_free_atlas_cells() {
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
    let cells = [
        (
            "cross-out X",
            sma::ATLAS_RECT_CROSS_OUT,
            red as fn(&[u8]) -> bool,
        ),
        ("Rot stamp", sma::ATLAS_RECT_ROT_STAMP, blue),
        ("Curse plate", sma::ATLAS_RECT_CURSE_PLATE, blue),
    ];
    for (name, cell, pick) in cells {
        assert_eq!(
            ink_in(&atlas, cell, pick).0,
            0,
            "no other source writes the {name}'s cell"
        );
    }
    assert_eq!(atlas.band_cross_out(), None);
    assert_eq!(atlas.band_rot_stamp(), None);
    assert_eq!(atlas.band_curse_plate(), None);
    let flame = idx
        .entry_bytes_extended(sma::FLAME_ATLAS_PROT_ENTRY)
        .expect("PROT 870");
    assert!(sma::add_cross_out_mark(&mut atlas, &flame), "the X bakes");
    assert_eq!(atlas.band_cross_out(), Some(sma::ATLAS_RECT_CROSS_OUT));
    assert_eq!(atlas.band_rot_stamp(), Some(sma::ATLAS_RECT_ROT_STAMP));
    assert_eq!(atlas.band_curse_plate(), Some(sma::ATLAS_RECT_CURSE_PLATE));
    for (name, cell, pick) in cells {
        let (ink, hue) = ink_in(&atlas, cell, pick);
        assert!(ink > 64, "the {name} carries ink ({ink} texels)");
        assert!(hue * 4 > ink, "and carries its colour ({hue} of {ink})");
        eprintln!("[ok] {name} baked at {cell:?}: {ink} texels, {hue} in hue");
    }
}

/// The dialogue page mark - kind 1 of the cursor sprite primitive
/// `FUN_8002B994`, two `16 x 16` frames at sheet `(224, 64)` / `(240, 64)`,
/// CLUT row 7 - bakes into its own atlas cells as two different frames in
/// the pointing hand's silver ramp, not a tinted copy of the hand.
#[test]
fn the_page_mark_frames_bake_beside_the_hand() {
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
    let base = sma::SYSTEM_UI_CLUT_EXT_TIM_OFFSET as u64;
    let end = (legaia_asset::title_pak::OVERLAY_LOAD_EMPTY_FRAME_TIM_OFFSET
        + legaia_asset::title_pak::OVERLAY_LOAD_EMPTY_FRAME_TIM_SIZE) as u64;
    let panel = idx
        .prot_dat_raw_bytes(base, (end - base) as usize)
        .expect("system-UI slice");
    let pill = idx
        .entry_bytes_extended(legaia_asset::title_pak::PROT_INDEX_OVERLAY as u32)
        .expect("PROT 0899");
    let atlas = sma::build_atlas(&panel, &pill, None).expect("atlas");
    let cell = |c: (u32, u32, u32, u32)| -> Vec<u8> {
        let mut out = Vec::new();
        for y in c.1..c.1 + c.3 {
            let o = ((y * atlas.width + c.0) * 4) as usize;
            out.extend_from_slice(&atlas.rgba[o..o + (c.2 * 4) as usize]);
        }
        out
    };
    let [f0, f1] = sma::ATLAS_RECT_ADVANCE_ICON;
    for (i, f) in [f0, f1].into_iter().enumerate() {
        let (ink, grey) = ink_in(&atlas, f, |p| p[0].abs_diff(p[2]) < 48);
        assert!(ink > 40, "frame {i} carries ink ({ink} texels)");
        assert_eq!(grey, ink, "frame {i} is the silver ramp, untinted");
    }
    assert_ne!(cell(f0), cell(f1), "the strip's two frames differ");
    assert_ne!(
        cell(f0),
        cell(atlas.band_cursor()),
        "the mark is not the hand"
    );
    // The frame the cells sit beside stays clear of them.
    let (fx, fy, fw, fh) = sma::ATLAS_RECT_EMPTY_FRAME;
    for f in [f0, f1] {
        assert!(
            f.0 >= fx + fw || f.0 + f.2 <= fx || f.1 >= fy + fh || f.1 + f.3 <= fy,
            "{f:?} overlaps the empty-slot frame"
        );
    }
}
