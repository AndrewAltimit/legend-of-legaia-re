//! Disc-gated coverage for the fishing minigame's venue-faithful WASM
//! surface on `LegaiaMinigames` (`minigames_fishing.rs` +
//! `minigames_fishing_scene.rs`) - the same API the site's minigames page
//! drives, exercised natively so a schema break fails before a browser sees
//! it. Pins the disc-decoded tables against the documented values
//! (`docs/subsystems/minigame-fishing.md`): the spawn pages' band-4 species,
//! the four reel-cadence templates, and the Vidna exchange's hidden
//! 50,000-point one-time row 0.
//!
//! Skips silently when `LEGAIA_DISC_BIN` is unset.

use legaia_web_viewer::minigames::LegaiaMinigames;

fn disc_bytes() -> Option<Vec<u8>> {
    let p = std::env::var_os("LEGAIA_DISC_BIN")?;
    std::fs::read(p).ok()
}

fn loaded() -> Option<LegaiaMinigames> {
    let bytes = disc_bytes()?;
    let mut mg = LegaiaMinigames::new();
    let status = mg.load_disc(bytes).expect("load_disc");
    assert!(
        status.contains(r#""venue_tables":true"#),
        "spawn + cadence tables should decode: {status}"
    );
    Some(mg)
}

#[test]
fn fishing_tables_decode_and_match_the_doc() {
    let Some(mg) = loaded() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    assert!(mg.fishing_pond_ready());

    // Venue spawn pages: the doc's decoded rows. Buma's Normal-lure (row 1)
    // band 4 is species 9 (the Spirit fish); Vidna's Heavy-lure (row 2)
    // band 4 is species 8.
    let buma = mg.fishing_spawn_json(0);
    assert!(buma.contains(r#""venue":0"#), "{buma}");
    let v: serde_json::Value = serde_json::from_str(&buma).expect("spawn json");
    let band4 = v["rows"][1]["bands"][4]["id"].as_u64().expect("band4");
    assert_eq!(band4, 9, "Buma Normal-lure band 4 = the rarest catch");
    let vidna = mg.fishing_spawn_json(1);
    let v: serde_json::Value = serde_json::from_str(&vidna).expect("spawn json");
    assert_eq!(v["rows"][2]["bands"][4]["id"].as_u64(), Some(8));

    // Point exchange: 6 rows per venue; Vidna row 0 is the 50,000-point
    // one-time prize invisible until affordable.
    let ex = mg.fishing_exchange_json(1);
    let v: serde_json::Value = serde_json::from_str(&ex).expect("exchange json");
    assert_eq!(v["rows"].as_array().map(|r| r.len()), Some(6), "{ex}");
    assert_eq!(v["rows"][0]["price"].as_u64(), Some(50_000), "{ex}");
    assert_eq!(v["rows"][0]["one_time"].as_bool(), Some(true), "{ex}");
    // With zero points, row 0 is below the cursor floor.
    assert_eq!(v["first_visible"].as_u64(), Some(1), "{ex}");

    // SCUS item names ride along on a full-disc load: every priced row
    // resolves a name.
    for row in v["rows"].as_array().unwrap() {
        assert!(row["name"].is_string(), "unnamed exchange row: {row}");
    }
}

#[test]
fn fishing_scene_and_player_decode() {
    let Some(mg) = loaded() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    assert!(mg.fishing_scene_ready(), "other1 scene should assemble");
    let info: serde_json::Value =
        serde_json::from_str(&mg.fishing_scene_info_json()).expect("info json");
    assert_eq!(info["player"].as_bool(), Some(true), "{info}");
    assert!(info["idle_frames"].as_u64().unwrap_or(0) > 0, "{info}");

    let pos = mg.fishing_scene_positions();
    let idx = mg.fishing_scene_indices();
    assert!(pos.len() > 3000, "venue mesh too small: {}", pos.len());
    assert_eq!(pos.len() % 3, 0);
    assert!(!idx.is_empty());
    assert_eq!(mg.fishing_scene_uvs().len() / 2, pos.len() / 3);
    assert_eq!(mg.fishing_scene_flat_rgba().len() / 4, pos.len() / 3);
    assert_eq!(mg.fishing_scene_vram().len(), 1024 * 512 * 2);

    // The angler body: a posed field mesh with a matching idle stream.
    let ppos = mg.fishing_player_positions();
    let parts = mg.fishing_player_part_count();
    assert!(!ppos.is_empty() && parts > 0);
    assert_eq!(mg.fishing_player_object_ids().len(), ppos.len() / 3);
    let dims = mg.fishing_player_idle_dims();
    assert_eq!(dims.len(), 2);
    assert_eq!(dims[0], parts, "idle clip bones == posed TMD objects");
    assert_eq!(
        mg.fishing_player_idle_frames().len() as u32,
        dims[0] * dims[1] * 6
    );

    // Ground queries return finite heights inside the AABB.
    let lo = &info["aabb"][0];
    let hi = &info["aabb"][1];
    let cx = ((lo[0].as_f64().unwrap() + hi[0].as_f64().unwrap()) / 2.0) as f32;
    let cz = ((lo[2].as_f64().unwrap() + hi[2].as_f64().unwrap()) / 2.0) as f32;
    let h = mg.fishing_scene_height_at(cx, cz);
    assert!(h.is_finite());
}

#[test]
fn pond_session_hooks_a_lure_row_species_and_lands_it() {
    let Some(mut mg) = loaded() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    assert!(mg.fishing_pond_start(0, 1, 2, 60, 0, 0, 0, 0, 0xBADF00D));

    // The lure-1 row of the Buma spawn page (the ids a hook may resolve to
    // outside the band-4 gate).
    let spawn: serde_json::Value =
        serde_json::from_str(&mg.fishing_spawn_json(0)).expect("spawn json");
    let row: Vec<u64> = (0..5)
        .map(|b| spawn["rows"][1]["bands"][b]["id"].as_u64().unwrap())
        .collect();

    // Cast: press, wind up, sweep deep, lock, fly.
    let tick = |mg: &mut LegaiaMinigames, reel: u32, cast: bool| -> String {
        mg.fishing_pond_tick(reel, cast, 0)
    };
    tick(&mut mg, 0, true);
    for _ in 0..12 {
        tick(&mut mg, 0, false);
    }
    for _ in 0..24 {
        tick(&mut mg, 0, false);
    }
    tick(&mut mg, 0, true);
    for _ in 0..0x14 {
        tick(&mut mg, 0, false);
    }
    let st: serde_json::Value =
        serde_json::from_str(&mg.fishing_pond_state_json()).expect("state json");
    assert_eq!(st["phase"].as_str(), Some("waiting"), "{st}");
    assert_eq!(st["casts"].as_i64(), Some(61), "landing counts the cast");

    // Hold reel A until a strike hooks (recasting when reeled all the way
    // in), then verify the species came from the equipped lure's row.
    let mut hooked_id = None;
    'outer: for _round in 0..40 {
        for _ in 0..1500 {
            let ev = tick(&mut mg, 0x40, false);
            if ev.contains(r#""e":"hooked""#) {
                let v: serde_json::Value = serde_json::from_str(&ev).unwrap();
                for e in v.as_array().unwrap() {
                    if e["e"].as_str() == Some("hooked") {
                        hooked_id = e["id"].as_u64();
                    }
                }
                break 'outer;
            }
            let st: serde_json::Value =
                serde_json::from_str(&mg.fishing_pond_state_json()).unwrap();
            if st["phase"].as_str() == Some("idle") {
                // Fully reeled in: cast again.
                tick(&mut mg, 0, true);
                for _ in 0..40 {
                    tick(&mut mg, 0, false);
                }
                tick(&mut mg, 0, true);
                for _ in 0..0x14 {
                    tick(&mut mg, 0, false);
                }
            }
        }
    }
    let id = hooked_id.expect("a strike lands under sustained reeling");
    assert!(
        row.contains(&id),
        "hooked {id} not in the lure-1 row {row:?}"
    );

    // Fight with a safe reel policy until it lands; the score credits the
    // persistent pool and the HUD reflects it.
    let mut landed_points = None;
    for _ in 0..30000 {
        let st: serde_json::Value = serde_json::from_str(&mg.fishing_pond_state_json()).unwrap();
        let tension = st["tension"].as_i64().unwrap();
        let ev = tick(&mut mg, if tension < 0x800 { 0x40 } else { 0 }, false);
        if ev.contains(r#""e":"landed""#) {
            let v: serde_json::Value = serde_json::from_str(&ev).unwrap();
            for e in v.as_array().unwrap() {
                if e["e"].as_str() == Some("landed") {
                    landed_points = e["points"].as_i64();
                }
            }
            break;
        }
        assert!(
            !ev.contains(r#""e":"snapped""#),
            "snapped under safe policy"
        );
    }
    let pts = landed_points.expect("the fight lands");
    assert!(pts > 0);
    let st: serde_json::Value = serde_json::from_str(&mg.fishing_pond_state_json()).unwrap();
    assert_eq!(st["points"].as_i64(), Some(pts));
    assert_eq!(st["best"].as_i64(), Some(pts));

    // The HUD draw list carries the point row and, mid-fight states aside,
    // resolves without panicking.
    let hud = mg.fishing_pond_hud_json();
    assert!(hud.contains(r#""t":"digit""#), "{hud}");
    assert!(hud.contains(r#""t":"cap""#), "{hud}");
}

#[test]
fn exchange_buy_spends_the_session_pool_and_latches_one_time_rows() {
    let Some(mut mg) = loaded() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    // Start with a rich pool so every row is in reach.
    assert!(mg.fishing_pond_start(1, 2, 2, 0, 200_000, 0, 0, 0, 1));
    let ex: serde_json::Value = serde_json::from_str(&mg.fishing_exchange_json(1)).unwrap();
    assert_eq!(ex["first_visible"].as_u64(), Some(0), "row 0 now visible");
    assert_eq!(ex["rows"][0]["available"].as_bool(), Some(true));

    // Buy the one-time row 0: points drop, the latch closes the row.
    let buy: serde_json::Value = serde_json::from_str(&mg.fishing_exchange_buy(1, 0, 1)).unwrap();
    assert_eq!(buy["cost"].as_u64(), Some(50_000), "{buy}");
    let ex: serde_json::Value = serde_json::from_str(&mg.fishing_exchange_json(1)).unwrap();
    assert_eq!(ex["points"].as_i64(), Some(150_000));
    assert_eq!(ex["rows"][0]["latched"].as_bool(), Some(true));
    assert_eq!(ex["rows"][0]["available"].as_bool(), Some(false));
    // A second buy refuses.
    assert_eq!(mg.fishing_exchange_buy(1, 0, 1), "null");
    // The session prize list carries the grant.
    let prizes = mg.fishing_prizes_json();
    assert!(prizes.contains(r#""qty":1"#), "{prizes}");
}
