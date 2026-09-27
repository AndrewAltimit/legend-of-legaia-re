//! Disc-gated: which Muscle Dome hub packets blend.
//!
//! A hub emitter marks every variant packet semi-transparent (variant 1 =
//! ABR 1, variant 2 = ABR 2 through `clut + 1`), but the GPU blends only
//! texels whose CLUT colour carries STP. This pins the two hub pages'
//! palette classes off the disc and what they make of the shared emitters'
//! quads: the row-502 records' own palettes blend additively, and every
//! variant-2 "shadow" and every row-503 packet draws opaque - so no hub
//! packet ever subtracts a texture, which is what lets the browser pages
//! draw ABR 2 textured quads plainly.
//!
//! Skips + passes when `LEGAIA_DISC_BIN` is unset.

#![cfg(not(target_arch = "wasm32"))]

use legaia_engine_ui::other_game_hud as hud;
use legaia_engine_ui::ringside_backdrop::HubPaletteStp;
use legaia_engine_ui::screen_prim::{PaletteStp, palette_stp};

fn hub() -> Option<(Vec<u8>, legaia_tim::Tim, legaia_tim::Tim)> {
    let disc = std::env::var_os("LEGAIA_DISC_BIN")?;
    let host = legaia_engine_core::scene::SceneHost::open_disc(&disc).ok()?;
    let arena = host
        .index
        .entry_bytes_extended(legaia_engine_core::muscle_dome::ARENA_OVERLAY_PROT_INDEX as u32)
        .ok()?;
    let container = host.index.entry_bytes_extended(1220).ok()?;
    let sections = legaia_lzs::decompress_container(&container).ok()?;
    let blob = sections.first()?;
    let t0 = legaia_tim::parse(blob.get(0xC..)?).ok()?;
    let t1 = legaia_tim::parse(blob.get(0xC + t0.byte_extent()..)?).ok()?;
    Some((arena, t0, t1))
}

#[test]
fn hub_palettes_are_all_stp_or_stp_free_and_only_row_502_blends() {
    let Some((arena, t0, t1)) = hub() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let classes = |t: &legaia_tim::Tim| -> Vec<PaletteStp> {
        (0..t.palette_count())
            .map(|i| palette_stp(t.clut.as_ref().unwrap().palette(t.mode, i).unwrap()))
            .collect()
    };
    let (c0, c1) = (classes(&t0), classes(&t1));
    assert!(
        !c0.iter().chain(&c1).any(|c| *c == PaletteStp::Mixed),
        "every hub palette is all-STP or STP-free"
    );
    let blending: Vec<usize> = (0..c0.len())
        .filter(|&i| c0[i] == PaletteStp::All)
        .collect();
    assert_eq!(blending, vec![0, 2, 6, 8], "row 502's STP palettes");
    assert!(
        c1.iter().all(|c| *c == PaletteStp::None),
        "row 503 has no STP"
    );

    let stp = HubPaletteStp::from_tims(&t0, &t1);
    let table = hud::parse_sprite_table(&arena);
    assert!(!table.is_empty());
    let mut screens: Vec<Vec<hud::HudQuad>> = Vec::new();
    for course in 0..3 {
        screens.push(hud::hub_screen_quads(
            &mut table.clone(),
            &hud::course_card_draws(course),
            0x80,
        ));
    }
    for round in 1..=9 {
        screens.push(hud::hub_screen_quads(
            &mut table.clone(),
            &hud::round_banner_draws(round),
            0x80,
        ));
    }
    screens.push(hud::hub_screen_quads(
        &mut table.clone(),
        hud::HUB_INTERVAL_HEADING,
        0x80,
    ));
    screens.push(hud::hub_screen_quads(
        &mut table.clone(),
        hud::HUB_INTRO_CARD,
        0x80,
    ));
    screens.push(hud::title_art_quads(
        &mut table.clone(),
        hud::TITLE_ART_ZOOM_END,
    ));
    let (mut additive, mut semi_opaque) = (0, 0);
    for q in screens.iter().flatten() {
        match stp.quad_abr(q) {
            Some(abr) => {
                assert_eq!(abr, 1, "a blending hub quad is additive: {q:?}");
                assert_eq!(q.tpage & 0x10, 0, "only row-502 records blend");
                additive += 1;
            }
            None if q.semi_transparent => semi_opaque += 1,
            None => {}
        }
        if (q.tpage >> 5) & 3 == 2 {
            assert_eq!(stp.quad_abr(q), None, "ABR 2 never meets an STP palette");
        }
    }
    eprintln!("[ok] additive hub quads {additive}, semi packets drawn opaque {semi_opaque}");
    assert!(
        additive > 0,
        "the ROUND banner / intro strip glow is additive"
    );
    assert!(semi_opaque > 0, "the variant-2 shadows draw opaque");
}
