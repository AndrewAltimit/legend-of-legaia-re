//! Disc-gated: the site/minigames.html WASM surface must resolve all three
//! playable minigames' tables out of a real disc image and drive each rules
//! engine through the same JSON API the page calls.
//!
//! This is the browser twin of the engine's `dance_minigame_real` /
//! `baka_minigame_real` / `slot_minigame_real` oracles: those pin the engine
//! side (`SceneHost::open_disc` -> `entry_bytes_extended` -> `as_loaded`), this
//! pins the *web* load path (`extract_prot_dat` -> `parse_prot_toc` ->
//! `as_loaded`) reaches the same tables, and that a play session steps.
//!
//! No Sony bytes are asserted - only structural facts (the tables decode, the
//! games advance, the state JSON is well-formed). Skips + passes when
//! `LEGAIA_DISC_BIN` is unset.

#![cfg(not(target_arch = "wasm32"))]

use legaia_web_viewer::minigames::LegaiaMinigames;

fn loaded() -> Option<(LegaiaMinigames, serde_json::Value)> {
    let disc = std::env::var("LEGAIA_DISC_BIN").ok()?;
    let bytes = std::fs::read(&disc).ok()?;
    let mut mg = LegaiaMinigames::new();
    let status: serde_json::Value = serde_json::from_str(&mg.load_disc(bytes).ok()?).unwrap();
    Some((mg, status))
}

#[test]
fn all_three_minigame_tables_decode_from_a_real_disc() {
    let Some((_, status)) = loaded() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    assert!(
        status["entries"].as_u64().unwrap() > 1000,
        "PROT TOC parsed"
    );

    // Dance: the baked step chart (PROT 0980).
    assert_eq!(status["dance"]["ok"], true, "dance chart: {status:?}");
    assert_eq!(status["dance"]["rows"], 3);
    assert_eq!(status["dance"]["beats"], 32);

    // Baka Fighter: the roster table (PROT 0976).
    assert_eq!(status["baka"]["ok"], true, "baka roster: {status:?}");
    assert_eq!(status["baka"]["fighters"], 17);

    // Slots: the per-symbol payout table (PROT 0975).
    assert_eq!(status["slot"]["ok"], true, "slot payouts: {status:?}");
    let payouts = status["slot"]["payouts"].as_array().unwrap();
    assert_eq!(payouts.len(), 10);
    assert!(
        payouts.iter().any(|p| p.as_u64().unwrap() > 0),
        "a real payout table is not all zeros"
    );
}

#[test]
fn dance_run_on_the_real_chart_scores() {
    let Some((mut mg, _)) = loaded() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    assert!(mg.dance_start(false), "real chart starts a run");

    // The chart must carry actual steps - a run against an all-zero chart
    // could never score, so the assertion below would be vacuous.
    let chart: serde_json::Value = serde_json::from_str(&mg.dance_chart_json()).unwrap();
    let steps: usize = chart["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r.as_array().unwrap().iter().filter(|c| **c != 0).count())
        .sum();
    assert!(steps > 0, "the real step chart carries steps");

    // Auto-play: on the first frame of each new beat the phase is inside the
    // acceptance window, so a CPU dancer presses the step the *judge* asks for
    // (`judged`, the raw chart cell - not the display half's held-sequence
    // substitution, which never scores).
    let mut last_beat = u64::MAX;
    let mut hits = 0;
    let mut notes = 0;
    for _ in 0..4000 {
        let st: serde_json::Value = serde_json::from_str(&mg.dance_state_json()).unwrap();
        if st["over"] == true {
            break;
        }
        let beat = st["beat"].as_u64().unwrap();
        if beat != last_beat && st["dead_zone"] == false {
            last_beat = beat;
            if let Some(sym) = st["judged"].as_u64()
                && sym != 0
            {
                notes += 1;
                if mg.dance_press(sym as u8) != "miss" {
                    hits += 1;
                }
            }
        }
        mg.dance_tick(1);
    }
    assert!(notes > 0, "the real chart presents judgeable notes");
    assert_eq!(hits, notes, "every well-timed press on a real note scored");
    let st: serde_json::Value = serde_json::from_str(&mg.dance_state_json()).unwrap();
    assert!(st["score"].as_u64().unwrap() > 0, "hits scored");
    assert_eq!(st["over"], true, "the song clock terminates the run");
}

#[test]
fn baka_duel_on_the_real_roster_decides() {
    let Some((mut mg, _)) = loaded() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let roster: serde_json::Value = serde_json::from_str(&mg.baka_roster_json()).unwrap();
    let roster = roster.as_array().unwrap();
    assert_eq!(roster.len(), 17);
    assert!(
        roster.iter().any(|f| f["gold"].as_u64().unwrap() > 0),
        "the real roster pays gold prizes"
    );

    assert!(mg.baka_start(5, 0x1234_5678), "fight vs roster fighter 5");
    // Throw the counter to whatever the opponent has committed; failing that,
    // throw type 1. The duel must reach a decision either way.
    for _ in 0..20_000 {
        let st: serde_json::Value = serde_json::from_str(&mg.baka_state_json()).unwrap();
        if st["phase"] == "match_over" {
            break;
        }
        if st["can_choose"] == true {
            let opp = st["chosen"][1].as_u64();
            // 2 beats 1, 3 beats 2, 1 beats 3.
            let pick = match opp {
                Some(1) => 2,
                Some(2) => 3,
                Some(3) => 1,
                _ => 1,
            };
            mg.baka_choose(pick);
        }
        mg.baka_tick(1);
    }
    let st: serde_json::Value = serde_json::from_str(&mg.baka_state_json()).unwrap();
    assert_eq!(st["phase"], "match_over", "the duel decides: {st:?}");
    assert!(st["winner"].is_number());
    assert!(
        st["gold"].as_u64().unwrap() > 0,
        "the prize is disc-sourced"
    );
}

#[test]
fn slot_session_on_the_real_paytable_spins_and_pays() {
    let Some((mut mg, _)) = loaded() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    assert!(
        mg.slot_start(0xC0FF_EE00, 60),
        "real paytable starts a machine"
    );

    let st: serde_json::Value = serde_json::from_str(&mg.slot_state_json()).unwrap();
    assert_eq!(st["balance"], 60);
    assert_eq!(st["phase"], "idle");
    assert_eq!(st["window"].as_array().unwrap().len(), 3);

    let mut credited = 0i64;
    for _ in 0..40 {
        if !mg.slot_spin() {
            break;
        }
        // Spin up, then stop all three reels.
        for _ in 0..60 {
            mg.slot_tick();
            let st: serde_json::Value = serde_json::from_str(&mg.slot_state_json()).unwrap();
            if st["can_stop"] == true {
                mg.slot_stop();
            }
            if st["phase"] == "payout" {
                break;
            }
        }
        credited += mg.slot_collect() as i64;
    }
    let st: serde_json::Value = serde_json::from_str(&mg.slot_state_json()).unwrap();
    // Coins were staked (the balance moved off its opening 60) and the machine
    // is back in a playable/idle state having tallied every spin.
    assert_ne!(st["balance"], 60, "the machine took / paid coins");
    assert!(
        st["net_take"].as_i64().unwrap() != 0,
        "the net-take heat counter accrued"
    );
    assert!(credited >= 0);
}
