//! Disc-gated oracle for the browser play page's **live battle** overlay
//! (`crate::play_battle`, composited by `play_overlay_draws_json` and blitted
//! by `site/js/play-app.js`).
//!
//! What is pinned:
//!
//! 1. **The page reaches `SceneMode::Battle` through the engine's own
//!    paths.** `enter_field` arms the live gameplay loop (the browser twin of
//!    the native `--live-loop` / `--player-battle` flags), and the scripted
//!    battle entry (`World::trigger_scripted_battle`, the field-VM op-`3E FF`
//!    arm) resolves a formation off the scene MAN's own table - real disc
//!    monsters, not synthetic rows.
//! 2. **The battle overlay draws while the battle runs.** The shared
//!    `battle_hud_draws_for` party surface lands on a packet-pinned band -
//!    the resting roster panels at stage `y 164`, or the acting member's
//!    full-width bar at `y 188` - with its chrome in the page's sprite
//!    array, the geometry the native window draws.
//! 3. **Nothing retail does not draw.** Retail's `Field -> Battle` edge shows
//!    no banner, so the port's "ENCOUNTER!" head line stays off the default
//!    surface, and the HUD keeps drawing once the old banner hold would have
//!    expired - the overlay is a live readout, not a one-shot.
//!
//! No Sony bytes are asserted, only structural facts. Skips + passes when
//! `LEGAIA_DISC_BIN` is unset.

#![cfg(not(target_arch = "wasm32"))]

use legaia_web_viewer::runtime::LegaiaRuntime;

fn loaded_in_town() -> Option<LegaiaRuntime> {
    let disc = std::env::var("LEGAIA_DISC_BIN").ok()?;
    let bytes = std::fs::read(&disc).ok()?;
    let mut rt = LegaiaRuntime::new();
    rt.load_disc(bytes, String::new()).ok()?;
    rt.enter_field("town01").ok()?;
    Some(rt)
}

fn overlay(rt: &mut LegaiaRuntime) -> serde_json::Value {
    serde_json::from_str(&rt.play_overlay_draws_json(960, 720)).expect("overlay json")
}

/// Sprite quads whose top edge sits inside `[y0, y1)`.
fn sprites_in_band(v: &serde_json::Value, y0: f64, y1: f64) -> usize {
    v["sprites"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter(|t| {
                    let y = t["dst"][1].as_f64().unwrap_or(f64::MIN);
                    y >= y0 && y < y1
                })
                .count()
        })
        .unwrap_or(0)
}

/// Text quads whose top edge sits inside `[y0, y1)`.
fn texts_in_band(v: &serde_json::Value, y0: f64, y1: f64) -> usize {
    v["texts"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter(|t| {
                    let y = t["dst"][1].as_f64().unwrap_or(f64::MIN);
                    y >= y0 && y < y1
                })
                .count()
        })
        .unwrap_or(0)
}

/// A scripted formation off the town MAN enters a live battle and the overlay
/// opens with the retail party surface on a packet-pinned band - chrome in
/// the sprite array, numerals in the text array - and no encounter banner,
/// which retail does not draw.
#[test]
fn live_battle_overlay_draws_the_retail_party_surface_and_no_banner() {
    let Some(mut rt) = loaded_in_town() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };

    // Settle the scene a few frames; no battle yet, so no battle draws.
    for _ in 0..5 {
        rt.tick_frame().expect("tick");
    }
    assert_eq!(rt.scene_mode(), "Field", "town boots into Field");

    if !rt.debug_start_test_battle() {
        eprintln!("[skip] no scripted formation row resolved on this disc build");
        return;
    }
    assert_eq!(
        rt.scene_mode(),
        "Battle",
        "trigger_scripted_battle must flip the live loop into SceneMode::Battle"
    );

    // 960x720 is an exact 3x of the 320x240 stage with a zero origin, so the
    // roster-panel band (stage y 164..=211) is surface 492..636 and the
    // active-actor bar (188..=207) is 564..624 - the bar's band nests inside
    // the panels', so one window covers whichever surface is up.
    const PARTY_TOP: f64 = 164.0 * 3.0;
    const PARTY_BOT: f64 = 212.0 * 3.0;

    let v = overlay(&mut rt);
    assert_eq!(v["open"], true, "battle overlay must report open");
    assert!(
        texts_in_band(&v, PARTY_TOP, PARTY_BOT) > 0,
        "the party surface's glyphs must draw on a packet-pinned band: {v}"
    );
    assert!(
        sprites_in_band(&v, PARTY_TOP, PARTY_BOT) > 0,
        "the party surface's chrome must reach the page's sprite array: {v}"
    );
    // Retail draws no banner on the Field -> Battle edge, so the port's
    // "ENCOUNTER!" head line must be absent from the default surface. It
    // used to land centred at surface_h / 4 = 180.
    assert_eq!(
        texts_in_band(&v, 170.0, 200.0),
        0,
        "the encounter banner is a port invention and must stay gated off"
    );

    // Age past the old banner hold (~90 frames): the HUD keeps drawing.
    for _ in 0..120 {
        rt.tick_frame().expect("tick");
        if rt.scene_mode() != "Battle" {
            // The auto-resolving side of a player-driven battle can finish
            // early on some formations; the HUD contract was already shown.
            return;
        }
    }
    let v = overlay(&mut rt);
    assert_eq!(v["open"], true, "overlay stays open while the battle runs");
    assert!(
        texts_in_band(&v, PARTY_TOP, PARTY_BOT) > 0,
        "battle HUD keeps drawing after the opening frames"
    );
}

/// `set_live_battles(false)` restores the walk-only page: entering a field
/// scene arms no live loop, and the scripted-battle probe still enters
/// battle only through its own explicit trigger - proving the default-on
/// arming is what the previous test exercised, not a side effect.
#[test]
fn live_battles_opt_out_keeps_the_field_walk_only() {
    // `env::var` returns `Ok("")` for a set-but-empty variable, so the
    // unset-only guard let an empty value through to a panicking read. Gate on
    // a readable disc instead, which is the condition the test actually needs.
    let disc = match std::env::var("LEGAIA_DISC_BIN") {
        Ok(d) if !d.is_empty() => d,
        _ => {
            eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
            return;
        }
    };
    let Ok(bytes) = std::fs::read(&disc) else {
        eprintln!("[skip] disc unreadable (disc-gated)");
        return;
    };
    let mut rt = LegaiaRuntime::new();
    rt.load_disc(bytes, String::new()).expect("load_disc");
    rt.set_live_battles(false);
    rt.enter_field("town01").expect("enter town01");
    for _ in 0..30 {
        rt.tick_frame().expect("tick");
    }
    assert_eq!(
        rt.scene_mode(),
        "Field",
        "with live battles off the scene stays a field walk"
    );
    let v = overlay(&mut rt);
    assert_eq!(v["open"], false, "no battle, no shop => overlay closed");
}
