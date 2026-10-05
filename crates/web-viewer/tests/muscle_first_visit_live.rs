//! Disc-gated: the minigames page's Muscle Dome first visit runs the live
//! `FirstVisitHub` both play hosts run - a press ends the two card holds
//! early and the hub's two announcer lines play off the staged XA lane.
//!
//! Structural facts only - no Sony bytes asserted. Skips + passes when
//! `LEGAIA_DISC_BIN` is unset.

#![cfg(not(target_arch = "wasm32"))]

use legaia_web_viewer::minigames::LegaiaMinigames;

fn loaded() -> Option<LegaiaMinigames> {
    let disc = std::env::var("LEGAIA_DISC_BIN").ok()?;
    let bytes = std::fs::read(&disc).ok()?;
    let mut mg = LegaiaMinigames::new();
    mg.load_disc(bytes).ok()?;
    Some(mg)
}

/// Step the live visit until it reports `done`; returns the tick count.
fn run(mg: &mut LegaiaMinigames, pressed: bool) -> u32 {
    mg.muscle_first_visit_reset();
    for t in 1..=8192u32 {
        let v: serde_json::Value =
            serde_json::from_str(&mg.muscle_first_visit_step(pressed, 0, 1)).unwrap();
        assert_eq!(v["ok"], true, "the live visit draws: {v}");
        if v["done"] == true {
            return t;
        }
    }
    panic!("the first visit never finished");
}

#[test]
fn the_live_first_visit_skips_on_a_press_and_plays_its_announcer() {
    let Some(mut mg) = loaded() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    eprintln!("[ran] muscle first visit live");
    let fired0: serde_json::Value = serde_json::from_str(&mg.baka_xa_state_json()).unwrap();
    let idle = run(&mut mg, false);
    assert_eq!(
        mg.muscle_hub_xa_fired(),
        2,
        "intro + round-card lines start"
    );
    let fired1: serde_json::Value = serde_json::from_str(&mg.baka_xa_state_json()).unwrap();
    assert_eq!(
        fired1["fired"].as_u64().unwrap() - fired0["fired"].as_u64().unwrap(),
        2,
        "both hub lines staged and played: {fired1}"
    );
    let pressed = run(&mut mg, true);
    eprintln!("first visit: {idle} ticks idle, {pressed} pressing");
    assert!(pressed < idle, "a press ends the card holds early");
}
