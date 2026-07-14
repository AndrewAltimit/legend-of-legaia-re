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
