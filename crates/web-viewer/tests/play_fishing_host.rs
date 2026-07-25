//! Disc-gated oracle for the browser play page's **fishing host**
//! (`LegaiaRuntime::play_fishing_*`, blitted by `site/js/play-app.js`).
//!
//! The thing under test is not the HUD's pixels - those come from the shared
//! `legaia_engine_ui::fishing_hud_draws_for` consumer that the native window
//! already exercises. It is the three host-side facts the HUD reads, each of
//! which a draw call alone cannot supply:
//!
//! 1. **A session exists.** `play_fishing_start` has to lift the fishing
//!    overlay (PROT 0972) through the static-overlay map and decode its species
//!    table off the visitor's own disc. A page that draws the HUD without this
//!    renders a readout of state nothing produces.
//! 2. **Cast / reel input reaches the session.** The page adds no input path of
//!    its own: it routes a pad word, and `World::tick_fishing` is the driver.
//!    So the contract is that pad-word + `tick_frame` alone walks the phase
//!    machine (cast -> lock -> fight), which the cast-power test pins.
//! 3. **The point pool round-trips.** `exit_fishing` banks the session's points
//!    into `World::fishing_points`, and the suspended scene mode comes back -
//!    otherwise entering the minigame would strand the field.
//!
//! No Sony bytes are asserted, only structural facts. Skips + passes when
//! `LEGAIA_DISC_BIN` is unset.

#![cfg(not(target_arch = "wasm32"))]

use legaia_web_viewer::runtime::LegaiaRuntime;

const CROSS: u16 = 0x4000;

fn loaded_in_town() -> Option<LegaiaRuntime> {
    let disc = std::env::var("LEGAIA_DISC_BIN").ok()?;
    let bytes = std::fs::read(&disc).ok()?;
    let mut rt = LegaiaRuntime::new();
    rt.load_disc(bytes, String::new()).ok()?;
    rt.enter_field("town01").ok()?;
    Some(rt)
}

/// A session installs off the disc, and the HUD it feeds actually produces
/// quads. The negative half matters as much as the positive one: with no
/// session the payload must report closed, so a page that polls every frame
/// draws nothing until fishing starts.
#[test]
fn fishing_session_starts_and_the_hud_draws() {
    let Some(mut rt) = loaded_in_town() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    assert!(
        !rt.play_fishing_active(),
        "no session should be live before the page starts one"
    );
    let closed: serde_json::Value =
        serde_json::from_str(&rt.play_fishing_hud_json(960, 720)).expect("hud json");
    assert_eq!(closed["open"].as_bool(), Some(false));

    assert!(
        rt.play_fishing_start(),
        "the fishing overlay (PROT 0972) + species table must decode from the disc"
    );
    assert!(rt.play_fishing_active());

    let hud: serde_json::Value =
        serde_json::from_str(&rt.play_fishing_hud_json(960, 720)).expect("hud json");
    assert_eq!(hud["open"].as_bool(), Some(true), "{hud}");
    let texts = hud["texts"].as_array().expect("texts array");
    assert!(
        !texts.is_empty(),
        "the persistent HUD rows go through fishing_hud_draws_for and must \
         produce font quads: {hud}"
    );
    // Every quad is a real rect inside the surface - the stage transform was
    // applied, not skipped.
    for q in texts {
        let dst = q["dst"].as_array().expect("dst");
        assert!(dst[2].as_i64().unwrap_or(0) > 0, "zero-width quad: {q}");
        assert!(dst[3].as_i64().unwrap_or(0) > 0, "zero-height quad: {q}");
    }
    let stage = hud["stage"].as_array().expect("stage transform");
    assert!(
        stage[2].as_i64().unwrap_or(0) >= 1,
        "stage scale must be at least 1: {hud}"
    );
}

/// The page contributes no cast/reel input path of its own - the pad word it
/// already routes plus `tick_frame` is the whole path, because the driver is
/// `World::tick_fishing`. Pin that: ticking advances the cast oscillator, and a
/// Cross press locks the cast into the fight.
#[test]
fn pad_word_and_tick_frame_drive_the_cast_and_the_fight() {
    let Some(mut rt) = loaded_in_town() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    assert!(rt.play_fishing_start());

    let phase = |rt: &LegaiaRuntime| -> String {
        let v: serde_json::Value =
            serde_json::from_str(&rt.play_fishing_state_json()).expect("state json");
        v["phase"].as_str().unwrap_or_default().to_string()
    };
    let power = |rt: &LegaiaRuntime| -> i64 {
        let v: serde_json::Value =
            serde_json::from_str(&rt.play_fishing_state_json()).expect("state json");
        v["cast_power"].as_i64().unwrap_or(-1)
    };
    assert_eq!(phase(&rt), "casting");

    // Idle ticks sweep the cast meter (the oscillator the HUD's power bar
    // shows). One tick is enough to move it off zero.
    rt.set_pad(0);
    rt.tick_frame().expect("tick");
    assert!(power(&rt) > 0, "the cast meter must advance while casting");

    // A Cross edge locks the cast, hooking a fish: the phase leaves `casting`
    // and the fight state appears. `set_pad` has to transition 0 -> CROSS for
    // the engine to see a press edge.
    rt.set_pad(CROSS);
    rt.tick_frame().expect("tick");
    assert_eq!(phase(&rt), "fighting", "Cross must lock the cast");

    // Holding Cross reels the fish in: landing progress accrues frame by
    // frame. Progress is the right signal to pin rather than tension - the
    // fish's per-frame pull is a property of the hooked species, and a weak
    // one raises no tension at all, so a tension assertion would be a
    // species-lottery flake.
    let progress = |rt: &LegaiaRuntime| -> i64 {
        let v: serde_json::Value =
            serde_json::from_str(&rt.play_fishing_state_json()).expect("state json");
        v["progress"].as_i64().unwrap_or(-1)
    };
    let start = progress(&rt);
    for _ in 0..8 {
        rt.tick_frame().expect("tick");
    }
    let reeled = progress(&rt);
    assert!(
        reeled > start,
        "holding a reel button must reel the fish in ({start} -> {reeled})"
    );

    // Releasing it stops the reel: the driver's Idle arm neither reels nor
    // progresses, which is what makes the tug-of-war a choice.
    rt.set_pad(0);
    for _ in 0..8 {
        rt.tick_frame().expect("tick");
    }
    assert_eq!(
        progress(&rt),
        reeled,
        "releasing both reel buttons must stop the reel"
    );

    // The gauge block is live while fighting, so the HUD carries bar fills -
    // the channel the blind sprite atlas cannot produce through the shared
    // consumer.
    let hud: serde_json::Value =
        serde_json::from_str(&rt.play_fishing_hud_json(960, 720)).expect("hud json");
    assert!(
        !hud["bars"].as_array().expect("bars array").is_empty(),
        "the tension / depth gauges must resolve while a fish is on: {hud}"
    );
}

/// Leaving the minigame has to restore the suspended scene and bank the
/// points, or entering it once would strand the field for the rest of the
/// session.
#[test]
fn leaving_restores_the_scene_and_banks_the_points() {
    let Some(mut rt) = loaded_in_town() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let mode_before = rt.scene_mode();
    assert!(rt.play_fishing_start());
    assert_ne!(
        rt.scene_mode(),
        mode_before,
        "entering fishing must suspend the field mode"
    );

    let banked = rt.play_fishing_stop();
    assert!(banked >= 0, "stop must report the banked point total");
    assert!(!rt.play_fishing_active());
    assert_eq!(
        rt.scene_mode(),
        mode_before,
        "leaving fishing must restore the suspended mode"
    );
    // A second stop is a no-op rather than a panic (the page's button can be
    // double-clicked).
    assert_eq!(rt.play_fishing_stop(), -1);
}

/// The prize-exchange rows decode alongside the species table, with the retail
/// availability gating applied against the live pool. This is the surface that
/// gives the point record a purpose.
#[test]
fn prize_rows_decode_with_retail_gating() {
    let Some(mut rt) = loaded_in_town() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    assert!(rt.play_fishing_start());
    let v: serde_json::Value =
        serde_json::from_str(&rt.play_fishing_prizes_json(1)).expect("prizes json");
    assert_eq!(v["venue"].as_u64(), Some(1), "{v}");
    let rows = v["rows"].as_array().expect("rows");
    assert_eq!(rows.len(), 6, "each venue page carries six prize rows: {v}");
    // Row 0 of Vidna is the one-time prize hidden until affordable; with a
    // fresh pool it is neither available nor visible.
    assert_eq!(rows[0]["one_time"].as_bool(), Some(true), "{v}");
    assert_eq!(rows[0]["available"].as_bool(), Some(false), "{v}");
    assert_eq!(v["first_visible"].as_u64(), Some(1), "{v}");
    // Buying an unaffordable row is refused rather than granted.
    assert_eq!(rt.play_fishing_prize_buy(1, 0), -1);
}
