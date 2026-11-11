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

/// Select in the duel opens the cabinet's pause menu through the real seat:
/// the disc's text, the shade behind it, the HUD gone, Circle back to the duel.
#[test]
fn select_in_a_seated_duel_opens_the_pause_menu() {
    use legaia_engine_minigames::baka_cabinet::{ST_DUEL, ST_PAUSE_3};
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
    host.world.trigger_field_interact(0, 51);
    host.world.set_pad(0);
    for _ in 0..2000 {
        if host.world.mode == SceneMode::BakaFighter {
            break;
        }
        let _ = host.tick();
    }
    let state = |h: &SceneHost| {
        h.world
            .minigames
            .baka_fighter
            .as_ref()
            .map(|f| f.cabinet().state())
    };
    for i in 0..4000 {
        if state(&host) == Some(ST_DUEL) {
            break;
        }
        if i % 60 == 0 {
            host.world.set_pad(PadButton::Cross.mask());
        }
        let _ = host.tick();
        host.world.set_pad(0);
    }
    assert_eq!(state(&host), Some(ST_DUEL), "the duel state");

    host.world.set_pad(PadButton::Select.mask());
    let _ = host.tick();
    host.world.set_pad(0);
    assert_eq!(state(&host), Some(ST_PAUSE_3), "Select opens the menu");
    // The press lands in the duel arm, whose epilogue still draws the HUD that
    // frame; the menu's own arm leaves the flag clear from the next one.
    let _ = host.tick();
    let fight = host.world.minigames.baka_fighter.as_ref().expect("duel");
    let (_, lines) = fight.pause_panel().expect("the disc's menu text");
    assert_eq!(lines.len(), 4, "three rows and the cursor");
    assert!(lines.iter().all(|l| !l.text.is_empty()));

    let index = host.index.clone();
    let mut surface = BakaDuelSurface::default();
    let read = |i: usize| index.entry_bytes(i as u32).ok().map(|b| b.to_vec());
    assert!(surface.frame(read, Some(fight)).is_some(), "duel surface");
    let prims = legaia_engine_screens::minigame_hud::baka_screen_prims(&surface, fight);
    assert!(
        prims
            .iter()
            .any(|p| matches!(p, ScreenPrim::Flat(q) if q.semi_transparent)),
        "the shade behind the menu"
    );
    assert!(
        !prims
            .iter()
            .any(|p| matches!(p, ScreenPrim::Flat(q) if q.gouraud.is_some())),
        "the VITAL bars are off under the menu"
    );

    host.world.set_pad(PadButton::Circle.mask());
    let _ = host.tick();
    host.world.set_pad(0);
    assert_eq!(state(&host), Some(ST_DUEL), "Circle backs out");
}

/// The attract title card's red flash reaches the screen layer both hosts
/// draw: the chrome produced its tint (`FUN_80024EE4(1, 1, 0xFF)`) and
/// neither host read it, so the card never flashed.
#[test]
fn the_title_card_red_flash_is_drawn() {
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
    host.world.trigger_field_interact(0, 51);
    host.world.set_pad(0);
    for _ in 0..2000 {
        if host.world.mode == SceneMode::BakaFighter {
            break;
        }
        let _ = host.tick();
    }
    assert_eq!(host.world.mode, SceneMode::BakaFighter, "the seat's warp");
    // No pad: the attract card runs its own clock.
    let mut red = None;
    for _ in 0..1200 {
        let _ = host.tick();
        let fight = host.world.minigames.baka_fighter.as_ref().expect("duel");
        if fight
            .chrome_frame()
            .tint
            .is_some_and(|t| t.rgb() == [0xFF, 0, 0])
        {
            red = Some(());
            break;
        }
    }
    assert!(red.is_some(), "the title card's flash frame");
    let fight = host.world.minigames.baka_fighter.as_ref().expect("duel");
    let index = host.index.clone();
    let mut surface = BakaDuelSurface::default();
    let read = |i: usize| index.entry_bytes(i as u32).ok().map(|b| b.to_vec());
    assert!(surface.frame(read, Some(fight)).is_some(), "duel surface");
    assert!(surface.hud_art_resident(), "the HUD art decodes");
    let prims = legaia_engine_screens::minigame_hud::baka_screen_prims(&surface, fight);
    assert!(
        prims.iter().any(|p| matches!(p, ScreenPrim::Flat(q)
            if q.semi_transparent && q.abr_mode == 1 && q.color[..3] == [0xFF, 0, 0]
                && q.ot_index == u32::from(legaia_engine_ui::baka_strips::BAKA_TINT_OT))),
        "the additive red quad"
    );
}
