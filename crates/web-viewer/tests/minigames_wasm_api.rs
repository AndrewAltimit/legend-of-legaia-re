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
fn slot_machine_art_decodes_off_the_disc() {
    let Some((mg, status)) = loaded() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    // The art pack (PROT 1200, the entry `FUN_801CEC94` loads) must reach the
    // page - without it the machine can only draw symbol *ids*.
    assert_eq!(status["slot"]["art"], true, "slot art pack: {status:?}");
    assert!(mg.slot_art_ready());

    // Every reel symbol decodes at its retail cell (`FUN_801d0fa8`): 64x64 RGBA.
    for sym in 0..legaia_asset::minigame_art::SLOT_SYMBOL_COUNT {
        let px = mg.slot_symbol_rgba(sym);
        assert_eq!(px.len(), 64 * 64 * 4, "symbol {sym} is a 64x64 sprite");
        assert!(
            px.chunks_exact(4).any(|p| p[3] != 0),
            "symbol {sym} is not fully transparent"
        );
    }

    // Symbols 0/1/2 are one piece of artwork behind three different CLUTs, so
    // their *alpha* masks match while their colours differ. This is the check
    // that would fail if the per-symbol CLUT (`0x7A80 + sym`) were ignored.
    let (a, b) = (mg.slot_symbol_rgba(0), mg.slot_symbol_rgba(1));
    let alpha_eq = a
        .chunks_exact(4)
        .zip(b.chunks_exact(4))
        .all(|(x, y)| (x[3] == 0) == (y[3] == 0));
    assert!(alpha_eq, "symbols 0 and 1 share one cell of artwork");
    assert_ne!(a, b, "...but different palettes, so different pixels");

    // The coin font: "COIN" + digits 0..9, 16px cells.
    let digits = mg.slot_digits_rgba();
    assert_eq!(digits.len() * 4 / 4, (64 + 10 * 16) * 16 * 4);

    // The 3 HUD widgets resolve through their own texpage + CLUT.
    let hud: serde_json::Value = serde_json::from_str(&mg.slot_hud_json()).unwrap();
    let hud = hud.as_array().unwrap();
    assert_eq!(hud.len(), 3, "the descriptor table has 3 records");
    // Record 0 is the cabinet: it samples the (640, 0) page, CLUT row 494.
    assert_eq!(hud[0]["texpage"], serde_json::json!([640, 0]));
    assert_eq!(hud[0]["clut"], serde_json::json!([0, 494]));
    for (i, rec) in hud.iter().enumerate() {
        let w = rec["w"].as_u64().unwrap() as usize;
        let h = rec["h"].as_u64().unwrap() as usize;
        assert_eq!(mg.slot_hud_rgba(i).len(), w * h * 4, "HUD widget {i}");
    }
}

#[test]
fn slot_machine_sound_decodes_off_the_disc() {
    let Some((mg, status)) = loaded() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    // The slot's cues are runtime-bank ids (>= 0x200), so they only resolve if
    // BOTH the efect.dat descriptor block (PROT 1199) and the VAB it samples
    // (PROT 1198) decode.
    assert_eq!(status["slot"]["sfx"], 11, "slot cue bank: {status:?}");

    let cues: serde_json::Value = serde_json::from_str(&mg.slot_sfx_json()).unwrap();
    let cues = cues.as_array().unwrap();
    assert_eq!(cues.len(), 11);
    // The block's 11 records span the two programs in order (4 tones, then 7) -
    // which is exactly the shape the PROT 1198 VAB declares (2 programs, 11
    // tones). The agreement is what says the table offset is right; the record
    // count alone is set by the class byte that terminates the block.
    assert_eq!(cues[0]["program"], 0);
    assert_eq!(cues[3]["program"], 0);
    assert_eq!(cues[4]["program"], 1);
    assert_eq!(cues[10]["program"], 1);
    assert_eq!(cues[10]["tone"], 6);

    let ids: serde_json::Value = serde_json::from_str(&mg.slot_sfx_cue_ids()).unwrap();
    for key in ["reel_stop", "payout_tick", "reach", "reach1", "reach2"] {
        let id = ids[key].as_u64().unwrap() as u16;
        let pcm = mg.slot_sfx_pcm(id);
        let rate = mg.slot_sfx_rate(id);
        assert!(!pcm.is_empty(), "cue {key} (0x{id:X}) decodes to PCM");
        assert!(
            (4000..=96_000).contains(&rate),
            "cue {key} plays at a sane rate ({rate} Hz)"
        );
        assert!(
            pcm.iter().any(|s| *s != 0),
            "cue {key} is not silence - a bank of zeros would mean a bad VAG index"
        );
    }

    // A cue the bank does not define stays silent rather than falling back to
    // some other sample.
    assert!(mg.slot_sfx_pcm(0x2FF).is_empty());
    assert_eq!(mg.slot_sfx_rate(0x2FF), 0);
}

#[test]
fn baka_roster_is_named_and_the_ladder_orders_it() {
    let Some((mg, status)) = loaded() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    assert_eq!(status["baka"]["named"], true, "roster names: {status:?}");

    let names: serde_json::Value = serde_json::from_str(&mg.baka_names_json()).unwrap();
    let names: Vec<&str> = names
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n.as_str().unwrap())
        .collect();
    assert_eq!(names.len(), 17);
    // Slots 0..2 are the playable party; the rest are the cabinet's fighters.
    // (Asserting they are non-empty ASCII, not asserting the Sony strings.)
    assert!(
        names.iter().all(|n| !n.is_empty() && n.is_ascii()),
        "every roster record carries a name"
    );

    // The ladder: 12 first-lap rungs (roster 5..=16) then the two post-clear
    // wrap-around opponents (roster 3 and 4).
    let ladder: serde_json::Value = serde_json::from_str(&mg.baka_ladder_json()).unwrap();
    let ladder = ladder.as_array().unwrap();
    assert_eq!(ladder.len(), 14);
    assert_eq!(ladder[0]["stage"], 2);
    assert_eq!(ladder[0]["roster"], 5);
    assert_eq!(ladder[11]["roster"], 16);
    assert_eq!(ladder[12]["roster"], 3);
    assert_eq!(ladder[13]["roster"], 4);

    // The payoff of getting the order right: across the twelve rungs the cabinet
    // actually serves first, the prize gold is strictly increasing. Read the
    // roster straight down instead and it is not - which is what made this look
    // unpinnable before.
    let roster: serde_json::Value = serde_json::from_str(&mg.baka_roster_json()).unwrap();
    let roster = roster.as_array().unwrap();
    let gold = |id: usize| roster[id]["gold"].as_i64().unwrap();
    let rungs: Vec<i64> = (0..12)
        .map(|i| gold(ladder[i]["roster"].as_u64().unwrap() as usize))
        .collect();
    assert!(
        rungs.windows(2).all(|w| w[0] < w[1]),
        "ladder prize gold ascends: {rungs:?}"
    );
    assert!(
        !(3..=16)
            .collect::<Vec<_>>()
            .windows(2)
            .all(|w| gold(w[0] as usize) < gold(w[1] as usize)),
        "...while plain roster order does not - the fact the ladder explains"
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
