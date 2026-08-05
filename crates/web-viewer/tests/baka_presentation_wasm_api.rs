//! Disc-gated: the Baka Fighter duel-presentation WASM surface
//! (`minigames_baka.rs`) must decode the fighters, animation banks, HUD
//! widget table and stage set off a real disc through the same calls
//! `site/js/minigame-baka.js` makes.
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

#[test]
fn duel_presentation_decodes_from_a_real_disc() {
    let Some(mg) = loaded() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    assert!(mg.baka_presentation_ready(), "presentation assets resolve");

    // The 51-record HUD widget table, every record on a resolvable art page.
    let hud: serde_json::Value = serde_json::from_str(&mg.baka_hud_json()).unwrap();
    let widgets = hud.as_array().unwrap();
    assert_eq!(widgets.len(), 51);
    for (i, w) in widgets.iter().enumerate() {
        assert!(w["page"].is_u64(), "widget {i} resolves to an art page");
        assert!(w["w"].as_u64().unwrap() > 0);
    }
    // Widget 0 is the PRESS START strip cell traced from the title path.
    assert_eq!(widgets[0]["u"], 48);
    assert_eq!(widgets[0]["v"], 48);
    assert_eq!(widgets[0]["w"], 112);
    assert_eq!(widgets[0]["h"], 16);

    // Player side: the three party fighters, rigged by the PROT 1203 bank
    // (15 / 16 / 15 bones - the per-character bank split).
    for (ch, bones) in [(0u32, 15u32), (1, 16), (2, 15)] {
        let parts = mg.baka_fighter_part_count(0, ch);
        assert_eq!(parts, bones, "party fighter {ch} part count");
        let dims = mg.baka_anim_dims(0, ch, 0);
        assert_eq!(dims[0], bones, "party fighter {ch} idle rig");
        assert!(dims[1] > 0);
        let n = mg.baka_fighter_positions(0, ch).len() / 3;
        assert!(n > 500, "party fighter {ch} has geometry");
        assert_eq!(mg.baka_fighter_object_ids(0, ch).len(), n);
        let frames = mg.baka_anim_pose_frames(0, ch, 0, parts);
        assert_eq!(frames.len(), (dims[1] * parts * 6) as usize);
    }

    // Every ladder rung: mesh + own idle rig covering its TMD objects.
    for roster in 3u32..=16 {
        let parts = mg.baka_fighter_part_count(1, roster);
        assert!(parts > 0, "opponent {roster} mesh");
        let dims = mg.baka_anim_dims(1, roster, 0);
        assert_eq!(dims[0], parts, "opponent {roster} idle rig == nobj");
        assert!(mg.baka_anim_record_count(1, roster) >= 6);
    }

    // The retail select-screen + tally-menu widget cells the site draws
    // (page-0 PLAYER SELECT banner + cursor arrows; the page-5 tally sheet's
    // NEXT GAME / PAY OUT / GET COIN cells beside the coin-digit strip).
    let cell = |i: usize| {
        (
            widgets[i]["u"].as_u64().unwrap(),
            widgets[i]["v"].as_u64().unwrap(),
            widgets[i]["w"].as_u64().unwrap(),
            widgets[i]["h"].as_u64().unwrap(),
        )
    };
    assert_eq!(cell(12), (1, 184, 254, 26), "PLAYER SELECT banner");
    assert_eq!(cell(48), (160, 32, 32, 32), "cursor arrow (left)");
    assert_eq!(cell(49), (192, 32, 32, 32), "cursor arrow (right)");
    assert_eq!(cell(44), (0, 192, 144, 24), "NEXT GAME (tally sheet)");
    assert_eq!(cell(45), (144, 192, 111, 24), "PAY OUT");
    assert_eq!(cell(46), (0, 218, 88, 16), "GET COIN");
    assert_eq!(cell(47), (88, 218, 16, 16), "coin digit cell");
    // The tally cells share one art page (the sheet also carrying VICTORY! /
    // ALL STAGE CLEAR!, widget 26).
    for i in [26usize, 28, 29, 44, 45, 46, 47] {
        assert_eq!(widgets[i]["page"], widgets[44]["page"], "widget {i} page");
    }

    // The stage set + the duel VRAM build.
    assert!(mg.baka_stage_positions(0).len() > 300, "arena wall mesh");
    assert_eq!(mg.baka_duel_vram(5).len(), 1024 * 512 * 2);
    // An art page decodes to RGBA through a widget's palette.
    let page = widgets[0]["page"].as_u64().unwrap() as usize;
    let palette = widgets[0]["palette"].as_u64().unwrap() as usize;
    let w = mg.baka_page_width(page);
    assert!(w > 0);
    assert_eq!(mg.baka_page_rgba(page, palette).len(), w * 256 * 4);
}

/// Duel facing (the site's pose step reads this instead of hard-coding a yaw):
/// the player stands on the LEFT and heads RIGHT toward the opponent, the
/// opponent stands on the RIGHT and heads LEFT toward the player - each looks at
/// the other. Also the ladder the site climbs: the disc's own serve order
/// (roster ids `5..=16` then the two second-lap rungs `3`, `4`) with a
/// strictly-monotonic first-lap prize and the 460 G full-clear total.
#[test]
fn facing_and_ladder_progression() {
    let Some(mg) = loaded() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };

    // Facing: player left/faces-right, opponent right/faces-left; each fighter's
    // facing is the negation of the other's side, so they look at each other.
    let facing: serde_json::Value = serde_json::from_str(&mg.baka_duel_facing_json()).unwrap();
    let (p, o) = (&facing["player"], &facing["opponent"]);
    assert_eq!(p["side"].as_i64().unwrap(), -1, "player stands on the left");
    assert_eq!(
        p["facing"].as_i64().unwrap(),
        1,
        "player heads right, toward the enemy"
    );
    assert_eq!(
        o["side"].as_i64().unwrap(),
        1,
        "opponent stands on the right"
    );
    assert_eq!(
        o["facing"].as_i64().unwrap(),
        -1,
        "opponent heads left, toward the player"
    );
    // Each fighter faces away from its own side (toward the center, where the
    // rival stands): facing == -side, equivalently facing == the other's side.
    assert_eq!(
        p["facing"].as_i64().unwrap(),
        -p["side"].as_i64().unwrap(),
        "player faces the opponent"
    );
    assert_eq!(
        o["facing"].as_i64().unwrap(),
        -o["side"].as_i64().unwrap(),
        "opponent faces the player"
    );
    assert_eq!(
        p["facing"].as_i64().unwrap(),
        o["side"].as_i64().unwrap(),
        "player heads toward where the opponent stands"
    );

    // Ladder: 14 paying rungs, served in the disc's own order.
    let ladder: serde_json::Value = serde_json::from_str(&mg.baka_ladder_json()).unwrap();
    let rungs = ladder.as_array().unwrap();
    let order: Vec<u64> = rungs
        .iter()
        .map(|r| r["roster"].as_u64().unwrap())
        .collect();
    assert_eq!(
        order,
        vec![5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 3, 4],
        "roster serve order: first lap 5..=16, then the two second-lap rungs 3, 4"
    );

    // Prize gold joined through the roster records: the first lap is strictly
    // increasing and the 14 paying records sum to the full-clear total.
    let roster: serde_json::Value = serde_json::from_str(&mg.baka_roster_json()).unwrap();
    let roster = roster.as_array().unwrap();
    let golds: Vec<u64> = order
        .iter()
        .map(|&rid| roster[rid as usize]["gold"].as_u64().unwrap())
        .collect();
    assert!(
        golds[..12].windows(2).all(|w| w[1] > w[0]),
        "first-lap (roster 5..=16) prize gold is strictly monotonic: {golds:?}"
    );
    assert_eq!(
        golds.iter().sum::<u64>(),
        460,
        "the 14 paying records sum to the full-clear prize total"
    );
}

/// The cabinet ladder-run bookkeeping through the WASM surface: the pot
/// accumulates the disc's own rung prizes, the between-match NEXT GAME /
/// PAY OUT choice banks or risks it, a loss forfeits it, and running every
/// rung pays the 460-coin full clear.
#[test]
fn ladder_run_cash_out_over_real_prizes() {
    let Some(mut mg) = loaded() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };

    let roster: serde_json::Value = serde_json::from_str(&mg.baka_roster_json()).unwrap();
    let gold = |rid: i64| roster[rid as usize]["gold"].as_u64().unwrap();

    // Full clear from rung 0: fight on at every choice; the pot ends at 460.
    let mut rid = mg.baka_run_start(0) as i64;
    assert_eq!(rid, 5, "the run opens on roster 5 (the disc's first rung)");
    let mut expected_pot = 0u64;
    for step in 0..14 {
        expected_pot += gold(rid);
        assert!(mg.baka_run_match_over(true), "win reported (step {step})");
        let st: serde_json::Value = serde_json::from_str(&mg.baka_run_state_json()).unwrap();
        assert_eq!(
            st["pot"].as_u64().unwrap(),
            expected_pot,
            "pot after {step}"
        );
        if step < 13 {
            assert_eq!(st["phase"], "choice", "the tally menu is up");
            rid = mg.baka_run_fight_on() as i64;
            assert!(rid >= 0, "next rung served");
        } else {
            assert_eq!(st["phase"], "all_clear");
            assert_eq!(st["banked"].as_u64().unwrap(), 460, "full clear pays 460");
        }
    }

    // Pay out mid-run: two wins then PAY OUT banks exactly those prizes.
    let first = mg.baka_run_start(0) as i64;
    mg.baka_run_match_over(true);
    let second = mg.baka_run_fight_on() as i64;
    mg.baka_run_match_over(true);
    let banked = mg.baka_run_pay_out();
    assert_eq!(banked as u64, gold(first) + gold(second));
    let st: serde_json::Value = serde_json::from_str(&mg.baka_run_state_json()).unwrap();
    assert_eq!(st["phase"], "paid_out");

    // Forfeit: two wins then a loss loses the whole pot.
    let first = mg.baka_run_start(3) as i64;
    mg.baka_run_match_over(true);
    let second = mg.baka_run_fight_on() as i64;
    mg.baka_run_match_over(true);
    let third = mg.baka_run_fight_on();
    assert!(third >= 0);
    mg.baka_run_match_over(false);
    let st: serde_json::Value = serde_json::from_str(&mg.baka_run_state_json()).unwrap();
    assert_eq!(st["phase"], "game_over");
    assert_eq!(st["pot"].as_u64().unwrap(), 0);
    assert_eq!(st["banked"].as_u64().unwrap(), 0);
    assert_eq!(
        st["forfeited"].as_u64().unwrap(),
        gold(first) + gold(second),
        "the pot at risk is what a loss forfeits"
    );
}

/// The duel page's widget geometry comes from the **ported** POLY_GT4 emitter
/// (`engine-core::baka_fighter::hud_widget_quad`, `FUN_801d5ed0`) through
/// `baka_hud_quad_json`, not from page-side arithmetic. Pin the two properties
/// the page cannot get right on its own: the half-extent truncates twice, and
/// the `size` argument scales it.
#[test]
fn hud_widget_quads_come_from_the_ported_emitter() {
    let Some(mg) = loaded() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    // Widget 0 is the PRESS START strip: a 112x16 cell, drawn centred.
    let q: serde_json::Value =
        serde_json::from_str(&mg.baka_hud_quad_json(0, 160, 204, 0x80, 0x1000, false)).unwrap();
    assert!(
        q["page"].is_u64(),
        "the widget resolves to an art page: {q}"
    );
    let (x0, x1) = (q["x0"].as_i64().unwrap(), q["x1"].as_i64().unwrap());
    let (y0, y1) = (q["y0"].as_i64().unwrap(), q["y1"].as_i64().unwrap());
    // The packet span is `centre - hw ..= centre + hw - 1`, so it is odd-width
    // by construction and centred on the requested point.
    assert_eq!(x0 + x1 + 1, 2 * 160, "centred on x");
    assert_eq!(y0 + y1 + 1, 2 * 204, "centred on y");
    // The UV span is the cell, inclusive.
    let (u0, u1) = (q["u0"].as_i64().unwrap(), q["u1"].as_i64().unwrap());
    let (v0, v1) = (q["v0"].as_i64().unwrap(), q["v1"].as_i64().unwrap());
    assert_eq!(u1 - u0 + 1, 112, "cell width");
    assert_eq!(v1 - v0 + 1, 16, "cell height");

    // The `size` term the page's old float formula dropped entirely.
    let half: serde_json::Value =
        serde_json::from_str(&mg.baka_hud_quad_json(0, 160, 204, 0x80, 0x800, false)).unwrap();
    let hw_full = x1 - x0 + 1;
    let hw_half = half["x1"].as_i64().unwrap() - half["x0"].as_i64().unwrap() + 1;
    assert!(
        hw_half < hw_full,
        "half size must shrink the quad: {hw_half} vs {hw_full}"
    );
    // The cell it samples does not change with the drawn size.
    assert_eq!(
        half["u1"].as_i64().unwrap() - half["u0"].as_i64().unwrap() + 1,
        112
    );

    // The mirror latch swaps the texture columns, not the destination rect.
    let m: serde_json::Value =
        serde_json::from_str(&mg.baka_hud_quad_json(0, 160, 204, 0x80, 0x1000, true)).unwrap();
    assert_eq!(m["mirror"], serde_json::Value::Bool(true));
    assert_eq!((m["x0"].as_i64(), m["x1"].as_i64()), (Some(x0), Some(x1)));

    // Brightness scales the gouraud pair (`channel * brightness >> 8`).
    let bright: serde_json::Value =
        serde_json::from_str(&mg.baka_hud_quad_json(0, 160, 204, 0xFF, 0x1000, false)).unwrap();
    let top_dim = q["rgb_top"].as_array().unwrap()[0].as_i64().unwrap();
    let top_hi = bright["rgb_top"].as_array().unwrap()[0].as_i64().unwrap();
    assert!(top_hi >= top_dim, "brightness raises the modulation");

    // An id past the 51-record table is `{}`, not a panic.
    assert_eq!(mg.baka_hud_quad_json(999, 0, 0, 0x80, 0x1000, false), "{}");
}
