//! Disc-gated: the minigames page's Muscle Dome first visit runs the live
//! `FirstVisitHub` both play hosts run - a press ends the two card holds
//! early and the hub's two announcer lines play off the staged XA lane.
//!
//! Structural facts only - no Sony bytes asserted. Skips + passes when
//! `LEGAIA_DISC_BIN` is unset.

#![cfg(not(target_arch = "wasm32"))]

use legaia_web_minigames::LegaiaMinigames;

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

/// Only a contest's first leg is owed the first visit: once a leg has been
/// reported the page's next leg opens on its ROUND card, as the play hosts'
/// hub does (`DomeContest::opens_on_first_visit`). A settled run owes it
/// again - the next contest is a fresh arena entry.
#[test]
fn the_first_visit_is_owed_once_per_contest() {
    let Some(mut mg) = loaded() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    eprintln!("[ran] muscle first visit owed once");
    assert!(mg.muscle_first_visit_owed(), "no contest open yet");
    assert!(mg.muscle_contest_start(0b001, 0b111));
    assert!(mg.muscle_first_visit_owed(), "the first fight opens on it");
    for fight in 1..=2 {
        assert!(mg.muscle_report_leg(true, 0, 2, 100, 0b001, 0b111) >= 0);
        assert!(
            mg.muscle_leg_shows_interval(),
            "fight {fight}: the run continues"
        );
        assert!(
            !mg.muscle_first_visit_owed(),
            "fight {}: a later fight does not replay the intro",
            fight + 1
        );
    }
    // Losing ends the run; the next contest is a fresh entry.
    mg.muscle_report_leg(false, 0, 2, 100, 0b001, 0b111);
    assert!(mg.muscle_contest_settle(0b001, 0b111, false));
    assert!(mg.muscle_first_visit_owed());
}

/// The page's live first visit runs on the hub's cadence - a game tick every
/// second vsync on a step of two (`FUN_801CF870`'s floor store at
/// `0x801CF8DC..0x801CF8E0`, `HubCadence`) - so, stepped once a vsync as the
/// page steps it, its arms last what retail's do: `2` 34, `3` 14, `4` 52,
/// `5` 182, `6` 34 and `0x14` 2 vsyncs, the lengths
/// `autorun_hub_cadence.lua` reads off `minigame_muscle_dome_retail_welcome`.
/// A step of one every vsync ran five of them a vsync short.
#[test]
fn the_live_first_visit_holds_each_arm_for_retails_vsyncs() {
    let Some(mut mg) = loaded() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    mg.muscle_first_visit_reset();
    let mut spans: Vec<(i64, u32)> = Vec::new();
    let mut per_vsync = Vec::new();
    for _ in 1..=8192u32 {
        let v: serde_json::Value =
            serde_json::from_str(&mg.muscle_first_visit_step(false, 0, 1)).unwrap();
        let arm = v["arm_id"].as_i64().expect("arm_id");
        per_vsync.push(arm);
        match spans.last_mut() {
            Some((a, n)) if *a == arm => *n += 1,
            _ => spans.push((arm, 1)),
        }
        if v["done"] == true {
            break;
        }
    }
    eprintln!("[ran] first visit arms on the page cadence: {spans:?}");
    let arm = |id: i64| spans.iter().find(|(a, _)| *a == id).map(|&(_, n)| n);
    assert_eq!(
        [2, 3, 4, 5, 6, 0x14].map(arm),
        [34, 14, 52, 182, 34, 2].map(Some),
        "arms 2..6 and 0x14 in vsyncs: {spans:?}"
    );
    // The no-input replay the page draws without a live hub agrees with
    // the live walk vsync for vsync.
    for (i, &live) in per_vsync.iter().enumerate().step_by(7) {
        let replay: serde_json::Value =
            serde_json::from_str(&mg.muscle_first_visit_json(i as i32 + 1, 0, 1)).unwrap();
        assert_eq!(replay["arm_id"].as_i64(), Some(live), "vsync {}", i + 1);
    }
}
