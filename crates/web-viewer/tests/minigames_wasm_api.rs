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
fn dance_jukebox_and_seamless_loop_render_from_a_real_disc() {
    let Some((mg, _)) = loaded() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    // The jukebox always carries the two tracks the dance overlay actually
    // loads, at extraction 1048/1054. The bank map is piecewise, so those are
    // sound-test slots 60/66 (`988 + slot`) = global BGM 2060/2066 - both
    // "Sol disco final". The ids asserted here were 2058/2064 while the flat
    // `990 + slot` base was assumed, which resolved to extraction 1046/1052
    // ("Vidna", a plain town theme, and an opening-title cue) once the bank
    // map was corrected. The rest of the Sol-disco floor family (2055/2059)
    // is present when those slots decode.
    let jb: serde_json::Value = serde_json::from_str(&mg.dance_jukebox_json()).unwrap();
    let tracks = jb["tracks"].as_array().unwrap();
    let bgms: Vec<u64> = tracks.iter().map(|t| t["bgm"].as_u64().unwrap()).collect();
    assert!(
        bgms.contains(&2060) && bgms.contains(&2066),
        "jukebox has both overlay tracks: {jb:?}"
    );
    assert!(
        tracks.len() >= 3,
        "the disco floor family should add tracks: {jb:?}"
    );
    for t in tracks {
        assert!(
            t["label"].as_str().unwrap().starts_with('M'),
            "labelled: {t}"
        );
    }

    // Each jukebox track renders to a seamless-loop slice: non-empty PCM
    // trimmed exactly to the loop end, with a well-ordered loop region.
    for &bgm in &bgms {
        let r = mg.music01_bgm_render(bgm as u16, 20.0);
        assert!(r.ok(), "bgm {bgm} rendered");
        assert!(r.loop_end() >= r.loop_start(), "loop region ordered: {bgm}");
        assert_eq!(
            r.pcm().len() as u32,
            r.loop_end() * 2,
            "pcm trimmed to loop end for {bgm}"
        );
        assert_eq!(r.rate(), 44_100);
    }
    // A non-bank id yields an empty render.
    let miss = mg.music01_bgm_render(9999, 20.0);
    assert!(!miss.ok() && miss.pcm().is_empty());
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
            credited += mg.slot_tick() as i64;
            let st: serde_json::Value = serde_json::from_str(&mg.slot_state_json()).unwrap();
            if st["can_stop"] == true {
                mg.slot_stop();
            }
            if st["stopped"] == 3 {
                break;
            }
        }
        // The tally is automatic: one more frame and the spin is banked.
        credited += mg.slot_tick() as i64;
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

/// The site drives the machine with **one key**. `slot_press` is that key: it
/// spins from idle, takes the three reel stops in sequence, and the frame tally
/// banks the win without a collect input. Three presses stop three reels; the
/// fourth starts the next spin.
#[test]
fn one_press_spins_stops_and_the_payout_banks_itself() {
    let Some((mut mg, _)) = loaded() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    assert!(mg.slot_start(0xC0FF_EE00, 60), "machine racks");

    let state = |mg: &LegaiaMinigames| -> serde_json::Value {
        serde_json::from_str(&mg.slot_state_json()).unwrap()
    };

    // Press 1: charge the bet and spin up.
    assert_eq!(mg.slot_press(), "spin");
    assert_eq!(state(&mg)["balance"], 60 - 3, "the bet is charged");
    // The reels are still ramping - retail refuses a stop, and so does this.
    assert_eq!(mg.slot_press(), "spinup");
    while state(&mg)["can_stop"] != true {
        mg.slot_tick();
    }

    // Presses 2..4: one reel each, in order.
    for reel in 1..=3 {
        assert_eq!(mg.slot_press(), "stop", "press stops reel {reel}");
        assert_eq!(state(&mg)["stopped"], reel, "reels stop in sequence");
        mg.slot_tick();
    }

    // No collect input anywhere above: the frame tally banked it, and the
    // machine is idle with the evaluated spin still latched for the display.
    let st = state(&mg);
    assert_eq!(
        st["phase"], "idle",
        "the machine tallied itself back to idle"
    );
    assert!(st["last"].is_object(), "the resolved spin stays latched");
    let payout = st["last"]["payout"].as_i64().unwrap();
    assert_eq!(
        st["balance"].as_i64().unwrap(),
        60 - 3 + payout,
        "the payout is in the balance without a collect input"
    );

    // And the next press starts a fresh spin off that balance.
    assert_eq!(mg.slot_press(), "spin");
    assert_eq!(state(&mg)["phase"], "spinning");
}

/// An empty machine reports `"broke"` rather than spinning on credit - the host
/// racks a new one on that.
#[test]
fn a_press_on_an_empty_machine_is_broke_not_a_free_spin() {
    let Some((mut mg, _)) = loaded() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    // Under the 3-coin gate from the start.
    assert!(mg.slot_start(0xC0FF_EE00, 2));
    assert_eq!(mg.slot_press(), "broke");
    let st: serde_json::Value = serde_json::from_str(&mg.slot_state_json()).unwrap();
    assert_eq!(st["balance"], 2, "no coins moved");
    assert_eq!(st["phase"], "idle");
}

/// The slot machine's **3D scene graph**, byte-checked against the retail frame.
///
/// The machine is a 3D scene, not a sprite collage: its paylines, medallions,
/// lamps, pedestals and marquee are GTE-projected quads whose model-space
/// positions live in four contiguous tables in the overlay's own rodata. This
/// pins that those tables decode, and that projecting them lands each element
/// where a retail framebuffer captured at the machine has it - the check that
/// would fail if the geometry were being measured off the art instead of read.
#[test]
fn the_slot_machines_3d_scene_decodes_and_projects_onto_the_retail_frame() {
    use legaia_asset::minigame_slot_scene as sc;

    let Some((mg, _)) = loaded() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    assert!(mg.slot_scene_ready(), "the scene graph decoded");
    let s: serde_json::Value = serde_json::from_str(&mg.slot_scene_json()).unwrap();
    assert_eq!(s["ok"], true);

    // Five paylines: three horizontal (y = -192 / 0 / +192) and two diagonals
    // that cross. The retail win evaluator reads exactly these five.
    let lines = s["paylines"].as_array().unwrap();
    assert_eq!(lines.len(), sc::PAYLINE_COUNT);
    let y = |i: usize, e: &str| lines[i][e][1].as_i64().unwrap();
    for (i, want) in [(0usize, -192i64), (1, 0), (2, 192)] {
        assert_eq!(y(i, "a"), want, "payline {i} is horizontal at y={want}");
        assert_eq!(y(i, "b"), want);
    }
    assert_eq!(y(3, "a"), -y(3, "b"), "payline 3 is a diagonal");
    assert_eq!(y(4, "a"), -y(4, "b"), "payline 4 is the other diagonal");
    assert_eq!(y(3, "a"), -y(4, "a"), "...and they mirror each other");

    // One medallion and one lamp per payline, and the column is symmetric about
    // the middle line - so a medallion's y matches its payline's.
    let meds = s["medallions"].as_array().unwrap();
    let lamps = s["lamps"].as_array().unwrap();
    assert_eq!(meds.len(), sc::PAYLINE_COUNT);
    assert_eq!(lamps.len(), sc::PAYLINE_COUNT);
    for (i, med) in meds.iter().enumerate().take(3) {
        assert_eq!(
            med["pos"][1].as_i64().unwrap(),
            y(i, "a"),
            "medallion {i} sits on payline {i}"
        );
    }
    // The medallions are one cell of art recoloured - their `art` field is the
    // CLUT column, and it is symmetric (2,1,0,1,2 across the column).
    let arts: Vec<i64> = meds
        .iter()
        .map(|m| m["art"].as_i64().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(arts[0], arts[2], "the two ±192 medallions share a palette");
    assert_eq!(arts[3], arts[4], "the two ±336 medallions share a palette");

    // The projection must land the scene on the retail 640x240 frame where a
    // capture at the machine has it. These targets were measured off that frame
    // and none of them entered the fit (which was solved on the lamps alone).
    let proj = |x: i32, y: i32, z: i32| sc::project(x, y, z);
    for (i, want_y) in [(0usize, 91.5f32), (1, 118.5), (2, 145.5)] {
        let p = &lamps[i]["pos"];
        let (_, sy) = proj(
            p[0].as_i64().unwrap() as i32,
            p[1].as_i64().unwrap() as i32,
            p[2].as_i64().unwrap() as i32,
        );
        assert!(
            (sy - want_y).abs() < 1.5,
            "lamp {i} projects to y={sy}, the retail frame has it at {want_y}"
        );
    }
    // The marquee panel: centred, at the top, 285px wide on screen.
    let panel = s["marquee"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["pos"][0].as_i64().unwrap() == 0)
        .expect("a centred marquee panel");
    let z = panel["pos"][2].as_i64().unwrap() as i32;
    let (hw, _) = sc::billboard_half(
        panel["half"][0].as_i64().unwrap() as i32,
        panel["half"][1].as_i64().unwrap() as i32,
        z,
    );
    assert!(
        (hw * 2.0 - 285.0).abs() < 8.0,
        "the marquee panel projects {}px wide; the retail frame has it ~285px",
        hw * 2.0
    );

    // The dot-matrix marquee: 21 message bitmaps, and the legend the attract
    // mode scrolls is a real, non-empty bitmap 13 rows tall.
    let msgs = s["messages"].as_array().unwrap();
    assert_eq!(msgs.len(), sc::MESSAGE_COUNT);
    for (i, m) in msgs.iter().enumerate() {
        assert_eq!(m["h"].as_u64().unwrap(), sc::DOT_ROWS as u64);
        let lit = m["bitmap"]
            .as_str()
            .unwrap()
            .split(',')
            .filter(|v| *v != "0")
            .count();
        assert!(lit > 0, "marquee message {i} is not blank");
    }

    // The dot grid must project onto the marquee panel it lives on.
    let (dx0, dy0) = proj(sc::DOT_X0, sc::DOT_Y0, sc::DOT_Z);
    let (dx1, dy1) = proj(
        sc::DOT_X0 + (sc::DOT_COLS as i32 - 1) * sc::DOT_X_STEP,
        sc::DOT_Y0 + (sc::DOT_ROWS as i32 - 1) * sc::DOT_Y_STEP,
        sc::DOT_Z,
    );
    let (px, _) = sc::project(0, panel["pos"][1].as_i64().unwrap() as i32, z);
    assert!(
        dx0 > px - hw && dx1 < px + hw && dy0 > 0.0 && dy1 < 60.0,
        "the dot grid ({dx0}..{dx1}, {dy0}..{dy1}) sits inside the marquee panel"
    );
}

#[test]
fn save_bar_portraits_decode_from_a_real_disc() {
    let Some((mg, _)) = loaded() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    // The three load-screen portrait TIMs (Vahn / Noa / Gala) - the faces the
    // site's save bar draws. 16x16 RGBA8, non-blank, and pairwise distinct
    // (three different characters, not one repeated cell).
    let mut faces = Vec::new();
    for char_id in 0..3usize {
        let rgba = mg.save_portrait_rgba(char_id);
        assert_eq!(rgba.len(), 16 * 16 * 4, "portrait {char_id} size");
        assert!(
            rgba.iter().any(|&b| b != 0),
            "portrait {char_id} is all-zero"
        );
        faces.push(rgba);
    }
    assert_ne!(faces[0], faces[1]);
    assert_ne!(faces[1], faces[2]);
    // Out-of-range char ids report empty rather than panicking.
    assert!(mg.save_portrait_rgba(3).is_empty());
}

/// The slot machine's and the duel's disc-pinned BGM resolve and render
/// non-silent stereo PCM, and the reel-spin motor tone (the direct-keyed
/// voice, not a ring cue) decodes with a real playback rate.
#[test]
fn slot_and_baka_bgm_and_the_spin_motor_decode_off_the_disc() {
    let Some((mg, _)) = loaded() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };

    assert_eq!(mg.minigame_bgm_rate(), 44_100);
    for game in ["slot", "baka"] {
        let ready: serde_json::Value =
            serde_json::from_str(&mg.minigame_bgm_ready_json(game)).unwrap();
        assert_eq!(ready["ok"], true, "{game} BGM ready: {ready}");
        let pcm = mg.minigame_bgm_pcm_i16(game, 2.0);
        assert_eq!(pcm.len(), 2 * 2 * 44_100, "{game}: 2 s of stereo PCM");
        assert!(pcm.iter().any(|&s| s != 0), "{game} BGM rendered silent");
    }
    // The two games are different tracks (the casino floor's vs the duel's).
    let slot: serde_json::Value =
        serde_json::from_str(&mg.minigame_bgm_ready_json("slot")).unwrap();
    let baka: serde_json::Value =
        serde_json::from_str(&mg.minigame_bgm_ready_json("baka")).unwrap();
    assert_ne!(slot["prot"], baka["prot"]);
    // An unknown game is a clean not-ok, not a panic.
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&mg.minigame_bgm_ready_json("nope")).unwrap()["ok"],
        false
    );

    // The reel-spin motor loop: program 1 / tone 0 keyed at 0x3C.
    let spin = mg.slot_spin_pcm();
    assert!(!spin.is_empty(), "spin-motor tone decoded");
    assert!(mg.slot_spin_rate() > 0);
}
