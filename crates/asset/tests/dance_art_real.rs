//! Disc-gated regression for [`legaia_asset::dance_art`]: the dance
//! minigame's presentation art (PROT 1230) + the overlay's HUD widget table
//! (PROT 0980 rodata) + the dancer face-stamp rig.
//!
//! Skips silently when `LEGAIA_DISC_BIN` is unset or `extracted/PROT.DAT`
//! is missing.

use std::path::PathBuf;

use legaia_asset::dance_art::{self, FACE_RIGS};
use legaia_asset::static_overlay;
use legaia_prot::archive::Archive;

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

fn read_entry(arch: &mut Archive, index: u32) -> Vec<u8> {
    let e = arch
        .entries
        .iter()
        .find(|e| e.index == index)
        .cloned()
        .unwrap_or_else(|| panic!("PROT entry {index} present"));
    let mut buf = Vec::new();
    arch.read_entry(&e, &mut buf).expect("entry reads");
    buf
}

#[test]
fn dance_art_pack_and_widget_table_decode() {
    let Some(prot) = prot_dat() else {
        eprintln!("[skip] LEGAIA_DISC_BIN or extracted/PROT.DAT missing");
        return;
    };
    let mut arch = Archive::open(&prot).expect("PROT.DAT parses");

    // --- the art pack (PROT 1230) ---
    let raw = read_entry(&mut arch, dance_art::DANCE_ART_PROT_INDEX as u32);
    let tims = dance_art::parse_art_pack(&raw).expect("art pack decodes");
    assert_eq!(tims.len(), 31, "retail pack carries 31 TIMs");

    // HUD page at its pinned rect, with the 16-palette row-500 CLUT strip.
    let hud = dance_art::hud_page(&tims).expect("HUD page present");
    let clut = hud.clut.as_ref().expect("HUD page has a CLUT block");
    assert_eq!((clut.fb_x, clut.fb_y, clut.w, clut.h), (0, 500, 256, 1));
    let page = dance_art::hud_page_rgba(&tims, 0).expect("page decodes");
    assert_eq!((page.width, page.height), (256, 256));
    assert!(
        page.rgba.chunks_exact(4).any(|p| p[3] != 0),
        "HUD page not empty"
    );

    // The three pack face strips exist at the rig windows.
    for rig in &FACE_RIGS[1..4] {
        let strip = dance_art::pack_strip(&tims, rig)
            .unwrap_or_else(|| panic!("pack strip at {:?}", rig.base));
        assert_eq!(strip.image.h, 128, "full 128-row strip");
    }

    // --- the overlay widget table (PROT 0980) ---
    let rec = static_overlay::overlay_map()
        .by_prot_index(980)
        .expect("dance overlay in the static map");
    let raw = read_entry(&mut arch, 980);
    let overlay = static_overlay::as_loaded(&raw, rec).expect("overlay lifts");
    let widgets = dance_art::parse_widgets(&overlay).expect("widget table parses");
    assert_eq!(widgets.len(), dance_art::WIDGET_COUNT);

    // Spot-pin the traced rows: the digit font, the score box, the notes.
    let digit = &widgets[dance_art::W_DIGIT];
    assert_eq!((digit.u, digit.v, digit.w, digit.h), (0, 0, 16, 24));
    let score = &widgets[dance_art::W_SCORE_BOX];
    assert_eq!((score.u, score.v, score.w, score.h), (0, 208, 64, 40));
    let note = &widgets[dance_art::W_NOTE_BASE + 1];
    assert_eq!((note.w, note.h), (16, 16));
    // Track caps + body are the beat-flash CLUT targets.
    for id in [
        dance_art::W_TRACK_CAP_L,
        dance_art::W_TRACK_CAP_R,
        dance_art::W_TRACK_BODY,
    ] {
        assert_eq!(widgets[id].clut, dance_art::CLUT_TRACK_IDLE);
    }

    // --- face frames + a composed window per pack dancer ---
    for (case, rig) in FACE_RIGS.iter().enumerate().skip(1) {
        let frames = dance_art::parse_face_frames(&overlay, rig).expect("frames parse");
        assert_eq!(frames.len(), rig.poses);
        let strip = dance_art::pack_strip(&tims, rig).expect("strip");
        for pose in 0..rig.poses {
            let face = dance_art::face_window_rgba(strip, rig, &frames, pose, 0, 64)
                .unwrap_or_else(|e| panic!("face case {case} pose {pose}: {e}"));
            assert_eq!((face.width, face.height), (64, 64));
            assert!(face.rgba.chunks_exact(4).any(|p| p[3] != 0));
        }
    }

    // --- Noa's rig sources her field atlas (PROT 0874 §2 entry 2) ---
    let raw = read_entry(
        &mut arch,
        legaia_asset::field_char_textures::PROT_ENTRY_INDEX,
    );
    let pack = legaia_asset::field_char_textures::parse(&raw).expect("field textures parse");
    let rig = &FACE_RIGS[0];
    let atlas = pack
        .textures
        .iter()
        .map(|t| &t.tim)
        .find(|t| t.image.fb_x == rig.base.0 && t.image.fb_y == rig.base.1)
        .expect("Noa atlas at (852, 256)");
    let frames = dance_art::parse_face_frames(&overlay, rig).expect("Noa frames");
    let face = dance_art::face_window_rgba(atlas, rig, &frames, 1, 0, 64).expect("Noa face");
    assert!(face.rgba.chunks_exact(4).any(|p| p[3] != 0));
}
