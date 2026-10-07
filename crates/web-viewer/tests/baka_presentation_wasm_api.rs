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

/// A whole cabinet run through the page's one per-frame step, `baka_frame` -
/// the play hosts' `BakaFight::frame`: the attract card and the player select
/// on the cabinet's own edges, a duel thrown through the packed pad, the
/// result tally draining the disc's rung prize into the winnings accumulator,
/// and PAY OUT on the cabinet's NEXT GAME / PAY OUT sheet (Right, then Cross)
/// leaving with exactly that pot.
///
/// This replaces the page's own ladder (`LadderRun` behind `baka_run_*`),
/// which kept a second pot and a second choice menu beside the cabinet's.
#[test]
fn a_cabinet_run_banks_the_rung_prize_on_pay_out() {
    let Some(mut mg) = loaded() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    const CROSS: u16 = 0x40;
    const RIGHT: u16 = 0x2000;
    let throw = [0x80u16, 0x20, 0x40]; // throws 1 / 2 / 3 = Square / Circle / Cross
    let roster: serde_json::Value = serde_json::from_str(&mg.baka_roster_json()).unwrap();
    let first_prize = roster[5]["gold"].as_u64().unwrap();

    for attempt in 0..6 {
        assert!(mg.baka_start_cabinet(), "the cabinet racks");
        let mut won_rung = false;
        let mut paid_sheet = false;
        for t in 0..60_000u32 {
            let cab: serde_json::Value = serde_json::from_str(&mg.baka_cabinet_json()).unwrap();
            let st: serde_json::Value = serde_json::from_str(&mg.baka_state_json()).unwrap();
            let mut edge = 0u16;
            if cab["front_end"] == true {
                if t % 20 == 0 {
                    edge = CROSS;
                }
            } else if st["phase"] == "match_over" {
                won_rung |= st["winner"] == 0;
                // Fast-forward the tally; on the NEXT GAME / PAY OUT sheet
                // (cabinet state `0x68`) step the cursor to PAY OUT, then
                // take it.
                if cab["state"] == 0x68 && !paid_sheet {
                    edge = RIGHT;
                    paid_sheet = true;
                } else if t % 25 == 0 {
                    edge = CROSS;
                }
            } else if st["can_choose"] == true {
                // 2 beats 1, 3 beats 2, 1 beats 3.
                let pick = match st["chosen"][1].as_u64() {
                    Some(1) => 2,
                    Some(2) => 3,
                    Some(3) => 1,
                    _ => 1,
                };
                edge = throw[pick - 1];
            }
            let out: serde_json::Value = serde_json::from_str(&mg.baka_frame(edge, 0)).unwrap();
            if out["exit"] == true {
                let w = out["winnings"].as_u64().unwrap();
                if won_rung {
                    assert!(
                        w >= first_prize,
                        "PAY OUT leaves with the first rung's prize: {w} vs {first_prize}"
                    );
                    eprintln!("[ran] cabinet run paid out {w} coins (attempt {attempt})");
                    return;
                }
                assert_eq!(w, 0, "a lost first rung leaves with nothing");
                break;
            }
        }
    }
    panic!("six cabinet runs never won and paid out the first rung");
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

/// The standalone page draws the duel through the engine's surface
/// (`baka_scene_*`, `BakaDuelSurface`) - the kernel the native window and the
/// play page pose it with - and the PLAYER SELECT pick seats that fighter's
/// own roster record.
#[test]
fn duel_draws_through_the_engine_surface_with_the_picked_fighter() {
    let Some(mut mg) = loaded() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    eprintln!("[ran] engine duel surface");
    assert_eq!(mg.baka_scene_frame(), -1, "no duel, no surface");
    assert!(
        !mg.baka_start_as(3, 5, 1),
        "the select cursor spans the three party fighters"
    );

    let mut verts = Vec::new();
    let mut last_gen = -1;
    for player in 0..3usize {
        assert!(mg.baka_start_as(player, 5, 0x1234));
        let generation = mg.baka_scene_frame();
        assert!(generation >= 0, "surface builds for fighter {player}");
        assert_ne!(generation, last_gen, "a new pairing is a new generation");
        last_gen = generation;
        let pos = mg.baka_scene_positions();
        assert!(!pos.is_empty() && pos.len() % 3 == 0);
        let n = pos.len() / 3;
        assert_eq!(mg.baka_scene_uvs().len(), n * 2);
        assert_eq!(mg.baka_scene_cba_tsb().len(), n * 2);
        assert_eq!(mg.baka_scene_flat_rgba().len(), n * 4);
        let idx = mg.baka_scene_indices();
        assert!(!idx.is_empty() && idx.iter().all(|&i| (i as usize) < n));
        assert!(!mg.baka_scene_vram().is_empty());
        assert_eq!(mg.baka_scene_vp(4.0 / 3.0).len(), 16);
        verts.push(n);
        // Ticking poses the same buffers while the round-start spin moves
        // the arena camera.
        let vp0 = mg.baka_scene_vp(1.0);
        for _ in 0..30 {
            mg.baka_tick(1);
        }
        assert_eq!(
            mg.baka_scene_frame(),
            generation,
            "same pairing, same buffers"
        );
        assert_ne!(mg.baka_scene_vp(1.0), vp0, "the round-start spin moves");
    }
    // The chrome's announcer lines stage off the disc's XA32 / XA33 and the
    // duel starts them - the round banner's line once the first round ends.
    let mut first_fire = None;
    for t in 0..20_000u32 {
        mg.baka_choose((1 + t % 3) as u8);
        mg.baka_tick(1);
        let xa: serde_json::Value = serde_json::from_str(&mg.baka_xa_state_json()).unwrap();
        if xa["fired"].as_u64().unwrap() > 0 {
            first_fire = Some(t);
            break;
        }
    }
    eprintln!("first announcer line at tick {first_fire:?}");
    let xa: serde_json::Value = serde_json::from_str(&mg.baka_xa_state_json()).unwrap();
    assert!(xa["lines"].as_u64().unwrap() > 0);
    assert_eq!(
        xa["staged"], xa["lines"],
        "every announcer line stages: {xa}"
    );
    assert!(
        xa["fired"].as_u64().unwrap() > 0,
        "the duel starts lines: {xa}"
    );

    // Each pick seats its own mesh (the three party meshes differ).
    assert!(
        verts[0] != verts[1] || verts[1] != verts[2],
        "the picked fighter's mesh is drawn: {verts:?}"
    );
}
