//! Disc-gated oracles for play-page rows a side-by-side frame audit found
//! against the native window, each asserted through the page's own wasm
//! exports (the calls `site/js/play-app.js` / `play-minigames.js` make):
//!
//! 1. **Ground winding.** The walk-ground heightfield the page uploads is the
//!    shared `field_ground` render surface - triangles reversed onto the
//!    scene TMDs' parity - so the cutscene camera's NCLIP pass keeps it, as
//!    the native window's ground mesh does. The page used to upload the raw
//!    builder order and culled the whole floor under the opdeene prologue's
//!    tree vignette.
//! 2. **The fishing point exchange stays open.** The page's buy used to open,
//!    buy and close the sub-screen inside one call, so the HUD compose (which
//!    reads the same world field) never found it open and the screen the
//!    native window draws over the pond never drew here.
//!
//! Skips + passes when `LEGAIA_DISC_BIN` is unset.

#![cfg(not(target_arch = "wasm32"))]

use legaia_engine_core::scene::SceneHost;
use legaia_web_viewer::play::build_field_render;
use legaia_web_viewer::runtime::LegaiaRuntime;

fn runtime_in(scene: &str) -> Option<LegaiaRuntime> {
    let disc = std::env::var("LEGAIA_DISC_BIN").ok()?;
    let bytes = std::fs::read(&disc).ok()?;
    let mut rt = LegaiaRuntime::new();
    rt.load_disc(bytes, String::new()).ok()?;
    rt.enter_field(scene).ok()?;
    Some(rt)
}

#[test]
fn page_ground_uploads_the_shared_render_surface() {
    let Ok(disc) = std::env::var("LEGAIA_DISC_BIN") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let mut host = SceneHost::open_disc(&disc).expect("open disc");
    for scene in ["opdeene", "town01"] {
        host.enter_field_scene(scene, 0).expect("enter_field_scene");
        let f = build_field_render(
            &host.index,
            host.scene.as_ref().expect("scene"),
            host.resources.as_ref().expect("resources"),
            false,
            &host.world.hidden_object_records(),
        );
        let hf = f.ground.as_ref().expect("walk-ground heightfield");
        let Some(rt) = runtime_in(scene) else {
            eprintln!("[skip] disc unreadable");
            return;
        };
        let idx = rt.field_ground_indices();
        assert_eq!(
            idx,
            legaia_engine_core::field_ground::render_indices(hf),
            "{scene}: the page must upload the shared render winding"
        );
        assert_ne!(
            idx, hf.indices,
            "{scene}: the builder's raw order is the winding NCLIP discards"
        );
        let pos = rt.field_ground_positions();
        let want: Vec<f32> = legaia_engine_core::field_ground::render_positions(hf)
            .into_iter()
            .flatten()
            .collect();
        assert_eq!(pos, want, "{scene}: sunk positions from the same kernel");
        eprintln!("[ok] {scene}: {} ground quads", hf.quad_count());
    }
}

fn hud_text_count(rt: &mut LegaiaRuntime) -> usize {
    let v: serde_json::Value =
        serde_json::from_str(&rt.play_fishing_hud_json(960, 720)).expect("hud json");
    v["texts"].as_array().map_or(0, |a| a.len())
}

#[test]
fn fishing_exchange_is_held_open_and_drawn_by_the_hud() {
    let Some(mut rt) = runtime_in("town01") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    assert!(rt.play_fishing_start(), "fishing overlay decodes");
    let closed_texts = hud_text_count(&mut rt);
    let state = |rt: &LegaiaRuntime| -> serde_json::Value {
        serde_json::from_str(&rt.play_fishing_exchange_state_json()).expect("state json")
    };
    assert_eq!(state(&rt)["open"].as_bool(), Some(false));

    // Toggle opens it, and it is still open on the next compose.
    assert_eq!(rt.play_fishing_exchange_input(0), 0);
    let _ = rt.tick_frame();
    assert_eq!(state(&rt)["open"].as_bool(), Some(true));
    let open_texts = hud_text_count(&mut rt);
    assert!(
        open_texts > closed_texts,
        "the HUD compose must draw the exchange rows while it is open \
         ({closed_texts} texts closed, {open_texts} open)"
    );

    // A refused buy (fresh pool) leaves it open on the venue the panel named.
    assert_eq!(rt.play_fishing_prize_buy(1, 0), -1);
    let st = state(&rt);
    assert_eq!(st["open"].as_bool(), Some(true), "{st}");
    assert_eq!(st["venue"].as_u64(), Some(1), "{st}");

    // Toggle again closes it; the HUD goes back to the pond readout alone.
    assert_eq!(rt.play_fishing_exchange_input(0), 0);
    assert_eq!(state(&rt)["open"].as_bool(), Some(false));
    // (The pond readout itself can gain a line after a tick, so the bound is
    // the open count, not the first closed one.)
    assert!(hud_text_count(&mut rt) < open_texts);
    // Unknown codes are refused.
    assert_eq!(rt.play_fishing_exchange_input(9), -1);
    eprintln!("[ok] exchange texts: closed {closed_texts}, open {open_texts}");
}
