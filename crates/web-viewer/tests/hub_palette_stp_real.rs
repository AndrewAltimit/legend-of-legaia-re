//! Disc-gated: which Muscle Dome hub packets blend.
//!
//! A hub emitter marks every variant packet semi-transparent (variant 1 =
//! ABR 1, variant 2 = ABR 2 through `clut + 1`), but the GPU blends only
//! texels whose CLUT colour carries STP. The file's CLUTs are not the VRAM
//! ones: the arena raises the upload STP flag `0x8007B998` before it loads
//! the dome data (`FUN_801CEA6C`, `0x801CEB00`), so every non-zero entry
//! reaches VRAM bit-15-set - which a live dome VRAM snapshot confirms on rows
//! 502 / 503. This pins both halves: the file carries STP on only four row-502
//! palettes, and the uploaded CLUTs (`muscle_dome::hub_page_tims`, the one
//! decoder all three hosts use) carry it on every non-empty one, so every
//! variant-2 knockout pass subtracts and every face pass adds.
//!
//! The earlier reading took the file's classes as the GPU's, drew the
//! variant-2 passes opaque, and that is what put white plates under the
//! INTERVAL tally labels.
//!
//! Skips + passes when `LEGAIA_DISC_BIN` is unset.

#![cfg(not(target_arch = "wasm32"))]

use legaia_engine_ui::other_game_hud as hud;
use legaia_engine_ui::ringside_backdrop::HubPaletteStp;
use legaia_engine_ui::screen_prim::{PaletteStp, palette_stp};

fn hub() -> Option<(Vec<u8>, Vec<u8>)> {
    let disc = std::env::var_os("LEGAIA_DISC_BIN")?;
    let host = legaia_engine_core::scene::SceneHost::open_disc(&disc).ok()?;
    let arena = host
        .index
        .entry_bytes_extended(legaia_engine_core::muscle_dome::ARENA_OVERLAY_PROT_INDEX as u32)
        .ok()?;
    let container = host
        .index
        .entry_bytes_extended(legaia_asset::muscle_dome::HUB_CONTAINER_PROT_INDEX)
        .ok()?;
    Some((arena, container))
}

fn classes(t: &legaia_tim::Tim) -> Vec<PaletteStp> {
    (0..t.palette_count())
        .map(|i| palette_stp(t.clut.as_ref().unwrap().palette(t.mode, i).unwrap()))
        .collect()
}

fn is_empty_palette(t: &legaia_tim::Tim, i: usize) -> bool {
    t.clut
        .as_ref()
        .unwrap()
        .palette(t.mode, i)
        .unwrap()
        .iter()
        .all(|&e| e == 0)
}

#[test]
fn hub_clut_stp_is_the_uploads_and_variant_two_passes_subtract() {
    let Some((arena, container)) = hub() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    eprintln!("[ran] hub palette STP");

    // The file: STP on row 502's palettes 0 / 2 / 6 / 8 only.
    let sections = legaia_lzs::decompress_container(&container).unwrap();
    let blob = &sections[0];
    let f0 = legaia_tim::parse(&blob[0xC..]).unwrap();
    let file_all: Vec<usize> = classes(&f0)
        .iter()
        .enumerate()
        .filter(|(_, c)| **c == PaletteStp::All)
        .map(|(i, _)| i)
        .collect();
    assert_eq!(file_all, vec![0, 2, 6, 8], "the file's STP palettes");

    // The upload: every non-empty palette on both rows is all-STP.
    let (t0, t1) = legaia_asset::muscle_dome::hub_page_tims(&container).expect("hub pages");
    for t in [&t0, &t1] {
        for (i, c) in classes(t).iter().enumerate() {
            if is_empty_palette(t, i) {
                assert_eq!(*c, PaletteStp::None);
            } else {
                assert_eq!(*c, PaletteStp::All, "palette {i} uploads STP-set");
            }
        }
    }

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
    let tally = hud::score_tally_quads(&mut table.clone(), [1, 2, 3, 4, 5, 6], [0x100; 6]);
    screens.push(tally.clone());
    let (mut additive, mut subtractive) = (0, 0);
    for q in screens.iter().flatten() {
        match ((q.tpage >> 5) & 3, stp.quad_abr(q)) {
            (_, None) => assert!(!q.semi_transparent || q.tpage >> 7 & 3 == 2, "{q:?}"),
            (1, Some(1)) => additive += 1,
            (2, Some(2)) => subtractive += 1,
            (m, a) => panic!("tpage ABR {m} resolved to {a:?}: {q:?}"),
        }
    }
    eprintln!("[ok] additive hub quads {additive}, subtractive {subtractive}");
    assert!(additive > 0 && subtractive > 0);
    // Every tally label strip draws twice - a knockout that subtracts and a
    // face that adds - and none draws opaque.
    let labels: Vec<_> = tally
        .iter()
        .filter(|q| q.uv[1].0 - q.uv[0].0 == 95)
        .collect();
    assert_eq!(labels.len(), 12);
    assert_eq!(
        labels.iter().filter(|q| stp.quad_abr(q) == Some(2)).count(),
        6
    );
    assert_eq!(
        labels.iter().filter(|q| stp.quad_abr(q) == Some(1)).count(),
        6
    );
}
