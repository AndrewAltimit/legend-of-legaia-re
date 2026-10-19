//! Disc-gated regression: **a Baka Fighter duel entered from its `koin1`
//! seat draws the duel HUD.**
//!
//! The duel HUD renderer `FUN_801d2afc` (VITAL bars and their frames, the
//! combo counters, round pips, attack-icon columns and the STAGE / PRESS
//! SELECT strip) was ported as a model the cabinet built every frame, but no
//! play host turned it into primitives: both drew only the cabinet's and the
//! round chrome's widget quads, so a seated duel showed a lone font round
//! digit. Both hosts now draw the layer through one builder,
//! `minigame_hud::baka_screen_prims`; this drives a real seat into a duel
//! and asserts that builder's output against the disc's own art.
//!
//! Skip-passes without `LEGAIA_DISC_BIN` / `extracted/` (CLAUDE.md convention).

use std::path::PathBuf;

use legaia_engine_minigame_scenes::baka_duel_scene::BakaDuelSurface;
use legaia_engine_mode::mode::SceneMode;
use legaia_engine_scene_host::scene_host::SceneHost;
use legaia_engine_system::input::PadButton;
use legaia_render_kernels::screen_prim::ScreenPrim;

fn extracted_dir() -> Option<PathBuf> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    if let Some(d) = std::env::var_os("LEGAIA_EXTRACTED_DIR").map(PathBuf::from)
        && d.join("PROT.DAT").exists()
    {
        return Some(d);
    }
    let d = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../extracted");
    if d.join("PROT.DAT").exists() {
        Some(d)
    } else {
        eprintln!("[skip] extracted/ missing - run legaia-extract first");
        None
    }
}

#[test]
fn a_seated_duel_draws_the_duel_hud() {
    let Some(extracted) = extracted_dir() else {
        return;
    };
    let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    host.enter_field_scene("koin1", 0).expect("enter koin1");
    for _ in 0..90 {
        let _ = host.tick();
    }
    host.world.minigames.casino_coins = 100;
    host.world.toggles.use_vm_dialogue = true;
    // P1[51]: the first Baka Fighter cabinet.
    host.world.trigger_field_interact(0, 51);
    host.world.set_pad(0);
    for _ in 0..2000 {
        if host.world.mode == SceneMode::BakaFighter {
            break;
        }
        let _ = host.tick();
    }
    assert_eq!(host.world.mode, SceneMode::BakaFighter, "the seat's warp");

    // Through the attract card and the player select into the duel band,
    // the cabinet states whose epilogue calls the HUD renderer.
    let mut hud_up = false;
    for i in 0..3000 {
        // Cross: the attract card's `0x844` edge takes it, and so does the
        // player select. Start would quit past the attract card.
        if i % 60 == 0 {
            host.world.set_pad(PadButton::Cross.mask());
        }
        let _ = host.tick();
        host.world.set_pad(0);
        if host
            .world
            .minigames
            .baka_fighter
            .as_ref()
            .is_some_and(|f| f.cabinet_frame().hud.is_some())
        {
            hud_up = true;
            break;
        }
    }
    assert!(
        hud_up,
        "the cabinet never reached a HUD-drawing state (cabinet state {:#x?}, mode {:?})",
        host.world
            .minigames
            .baka_fighter
            .as_ref()
            .map(|f| f.cabinet().state()),
        host.world.mode
    );

    let fight = host.world.minigames.baka_fighter.as_ref().expect("duel");
    let index = host.index.clone();
    let mut surface = BakaDuelSurface::default();
    let read = |i: usize| index.entry_bytes(i as u32).ok().map(|b| b.to_vec());
    assert!(surface.frame(read, Some(fight)).is_some(), "duel surface");
    assert!(
        surface.hud_art_resident(),
        "the HUD art decodes off the disc"
    );

    let prims = legaia_engine_screens::minigame_hud::baka_screen_prims(&surface, fight);
    let bars = prims
        .iter()
        .filter(|p| matches!(p, ScreenPrim::Flat(q) if q.gouraud.is_some()))
        .count();
    let cells = prims
        .iter()
        .filter(|p| {
            matches!(p, ScreenPrim::Textured(q)
                if q.ot_index == legaia_engine_minigames::baka_cabinet::DUEL_HUD_OT)
        })
        .count();
    assert_eq!(bars, 2, "the two VITAL fill bars");
    // Six bar-frame cells, two pips a side, three icons a side.
    assert!(cells >= 6 + 4 + 6, "the renderer's own cells: {cells}");
    // The STAGE strip carries the stage number now, so the font round digit
    // stands down.
    let digits = legaia_engine_screens::minigame_hud::baka_digit_placements(fight, true);
    assert!(
        !digits.contains(&(
            legaia_engine_minigames::baka_fighter_chrome::HUD_ROUND_PEN.0,
            legaia_engine_minigames::baka_fighter_chrome::HUD_ROUND_PEN.1,
            1
        )),
        "the font round digit still draws under the duel HUD: {digits:?}"
    );
}
