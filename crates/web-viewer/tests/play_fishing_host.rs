//! Disc-gated oracle for the browser play page's **fishing host**
//! (`LegaiaRuntime::play_fishing_*`, blitted by `site/js/play-app.js`).
//!
//! The thing under test is not the HUD's pixels - those come from the shared
//! `legaia_engine_ui::fishing_hud_draws_for` consumer that the native window
//! already exercises. It is the three host-side facts the HUD reads, each of
//! which a draw call alone cannot supply:
//!
//! 1. **A session exists.** `play_fishing_start` has to lift the fishing
//!    overlay (PROT 0972) through the static-overlay map and decode its species,
//!    spawn and cadence tables off the visitor's own disc. A page that draws
//!    the HUD without this renders a readout of state nothing produces.
//! 2. **Cast / reel input reaches the session.** The page adds no input path of
//!    its own: it routes a pad word, and `World::tick_fishing` is the driver.
//!    So the contract is that pad-word + `tick_frame` alone walks the retail
//!    phase machine (shore -> wind-up -> power -> flight -> waiting), which the
//!    cast test pins.
//! 3. **The persistent words round-trip.** `exit_fishing` banks the session's
//!    points and cast counter into `World::minigames`, and the suspended scene
//!    mode comes back - otherwise entering the minigame would strand the field.
//!
//! No Sony bytes are asserted, only structural facts. Skips + passes when
//! `LEGAIA_DISC_BIN` is unset.

#![cfg(not(target_arch = "wasm32"))]

use legaia_web_viewer::runtime::LegaiaRuntime;

const CROSS: u16 = 0x4000;
const CIRCLE: u16 = 0x2000;

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
/// `World::tick_fishing`. Pin that: a Circle edge starts the wind-up, the power
/// meter sweeps, a second Circle edge locks it, the lure flies and lands (the
/// persistent cast counter increments), and holding Cross reels the empty
/// line back in.
#[test]
fn pad_word_and_tick_frame_drive_the_cast() {
    let Some(mut rt) = loaded_in_town() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    assert!(rt.play_fishing_start());

    let state = |rt: &LegaiaRuntime| -> serde_json::Value {
        serde_json::from_str(&rt.play_fishing_state_json()).expect("state json")
    };
    let phase = |rt: &LegaiaRuntime| state(rt)["phase"].as_str().unwrap_or_default().to_string();
    let tick = |rt: &mut LegaiaRuntime, pad: u16, n: usize| {
        rt.set_pad(pad);
        for _ in 0..n {
            rt.tick_frame().expect("tick");
        }
    };
    assert_eq!(phase(&rt), "idle", "a session opens at the shore");
    let casts = state(&rt)["casts"].as_i64().expect("casts");

    // Circle edge -> wind-up; the meter opens after the wind-up frames.
    tick(&mut rt, 0, 1);
    tick(&mut rt, CIRCLE, 1);
    assert_eq!(phase(&rt), "windup", "Circle starts the cast");
    tick(&mut rt, 0, 30);
    assert_eq!(phase(&rt), "power");
    assert!(
        state(&rt)["cast_power"].as_i64().unwrap_or(-1) > 0,
        "the cast meter sweeps"
    );
    // A second Circle edge locks the power; the lure flies and lands.
    tick(&mut rt, CIRCLE, 1);
    assert_eq!(phase(&rt), "flight", "Circle locks the power meter");
    tick(&mut rt, 0, 40);
    assert_eq!(phase(&rt), "waiting", "the lure lands in the pre-hook loop");
    assert_eq!(
        state(&rt)["casts"].as_i64(),
        Some(casts + 1),
        "the landing increments the persistent cast counter"
    );
    // The catch HUD is up once a cast is out, so its bars channel is live.
    let hud: serde_json::Value =
        serde_json::from_str(&rt.play_fishing_hud_json(960, 720)).expect("hud json");
    assert!(
        !hud["bars"].as_array().expect("bars array").is_empty(),
        "the catch HUD's bars must resolve while a line is out: {hud}"
    );
    // Holding Cross reels the line in: the record shortens frame by frame.
    let record = state(&rt)["record"].as_i64().expect("record");
    tick(&mut rt, CROSS, 8);
    let st = state(&rt);
    let reeled = st["record"].as_i64().expect("record");
    assert!(
        reeled < record || st["phase"] != "waiting",
        "holding a reel button must shorten the line ({record} -> {reeled})"
    );
}

/// The fishing venue actors run on this host too, through the shared engine
/// kernel (`fishing_venue::tick_fishing_venue_on_host`) the native window's
/// minigame frame calls. With the page's `tick_fishing_actors` unwired the
/// wander actor is never armed, the held D-pad steers nothing and no retarget
/// ripple reaches the world's effect pool.
#[test]
fn the_venue_actors_run_on_the_play_page() {
    let Some(mut rt) = loaded_in_town() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    assert!(rt.play_fishing_start());
    let state = |rt: &LegaiaRuntime| -> serde_json::Value {
        serde_json::from_str(&rt.play_fishing_state_json()).expect("state json")
    };
    rt.set_pad(0);
    rt.tick_frame().expect("tick");
    let st = state(&rt);
    let facing0 = st["wander"]["facing"]
        .as_i64()
        .unwrap_or_else(|| panic!("the wander actor was never armed: {st}"));
    // Hold D-pad right at the idle shore: the fish turns, and its retarget
    // rolls spawn ripples into the shared pool.
    const RIGHT: u16 = 0x0020;
    rt.set_pad(RIGHT);
    let mut peak_parts = 0;
    for _ in 0..600 {
        rt.tick_frame().expect("tick");
        peak_parts = peak_parts.max(state(&rt)["fx_parts"].as_i64().unwrap_or(0));
    }
    let st = state(&rt);
    assert_ne!(
        st["wander"]["facing"].as_i64(),
        Some(facing0),
        "the held D-pad did not steer the wander: {st}"
    );
    assert!(
        peak_parts > 0,
        "no retarget ripple reached the world's effect pool"
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
