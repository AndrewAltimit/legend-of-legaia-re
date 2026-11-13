//! Disc-gated coverage for the fishing minigame's venue-faithful WASM
//! surface on `LegaiaMinigames` (`fishing_web.rs` +
//! `fishing_scene_web.rs`) - the same API the site's minigames page
//! drives, exercised natively so a schema break fails before a browser sees
//! it. Pins the disc-decoded tables against the documented values
//! (`docs/subsystems/minigame-fishing.md`): the spawn pages' band-4 species,
//! the four reel-cadence templates, and the Vidna exchange's hidden
//! 50,000-point one-time row 0.
//!
//! Skips silently when `LEGAIA_DISC_BIN` is unset.

use legaia_web_minigames::LegaiaMinigames;

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

    // The shore party: the setup's three spawns, each a posed field mesh
    // with a clip stream on its own rig.
    assert_eq!(mg.fishing_party_count(), 3, "lead + two flanking members");
    for m in 0..3 {
        let ppos = mg.fishing_player_positions(m);
        let parts = mg.fishing_player_part_count(m);
        assert!(!ppos.is_empty() && parts > 0, "member {m}");
        assert_eq!(mg.fishing_player_object_ids(m).len(), ppos.len() / 3);
        let dims = mg.fishing_player_idle_dims(m);
        assert_eq!(dims.len(), 2);
        assert_eq!(
            dims[0], parts,
            "member {m}: clip bones == posed TMD objects"
        );
        assert_eq!(
            mg.fishing_player_idle_frames(m).len() as u32,
            dims[0] * dims[1] * 6
        );
        eprintln!("[ran] member {m}: {}", mg.fishing_player_clip_json(m));
    }
    // The seats are the setup's spawn words; the lead stands on the shore
    // floor the capture reads (`+0x16 = -128`).
    let seats: serde_json::Value =
        serde_json::from_str(&mg.fishing_party_json(0)).expect("party json");
    let xz: Vec<(i64, i64)> = (0..3)
        .map(|i| {
            (
                seats[i]["x"].as_i64().unwrap(),
                seats[i]["z"].as_i64().unwrap(),
            )
        })
        .collect();
    assert_eq!(xz, vec![(4736, 10752), (4832, 10752), (4640, 10688)]);
    eprintln!("[ran] lead floor y = {}", seats[0]["y"]);
    assert_eq!(seats[0]["y"].as_f64(), Some(-128.0));
    assert_eq!(mg.fishing_venue_vp(0, 4.0 / 3.0).len(), 16);

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
    for _ in 0..96 {
        tick(&mut mg, 0, false);
    }
    tick(&mut mg, 0, true);
    // The flight runs until the power is spent.
    for _ in 0..200 {
        tick(&mut mg, 0, false);
        let st: serde_json::Value =
            serde_json::from_str(&mg.fishing_pond_state_json()).expect("state json");
        if st["phase"].as_str() == Some("waiting") {
            break;
        }
    }
    let st: serde_json::Value =
        serde_json::from_str(&mg.fishing_pond_state_json()).expect("state json");
    assert_eq!(st["phase"].as_str(), Some("waiting"), "{st}");
    assert_eq!(st["casts"].as_i64(), Some(61), "landing counts the cast");

    // The state carries the session's own lure point - the engine lure the
    // line's fish end and the water probe read, not a page projection of
    // the line record - and the in-water move (`0x801D2C3C..0x801D2DC8`)
    // pulls it in under the reel.
    let lure_xz = |mg: &LegaiaMinigames| -> (i64, i64) {
        let st: serde_json::Value = serde_json::from_str(&mg.fishing_pond_state_json()).unwrap();
        let lure: serde_json::Value = serde_json::from_str(&mg.fishing_lure_json()).unwrap();
        let p = st["lure_pos"]
            .as_array()
            .expect("a landed lure has a point");
        assert_eq!(p.len(), 3);
        let (x, z) = (p[0].as_i64().unwrap(), p[2].as_i64().unwrap());
        assert_eq!(
            (x, z),
            (lure["x"].as_i64().unwrap(), lure["z"].as_i64().unwrap())
        );
        (x, z)
    };
    let landed_at = lure_xz(&mg);
    let mut early_hook = None;
    for i in 0..8 {
        // A reel press starts the pull (`pull_step`'s edge test).
        let ev = mg.fishing_pond_tick(0x40, false, if i == 0 { 0x40 } else { 0 });
        let v: serde_json::Value = serde_json::from_str(&ev).unwrap();
        for e in v.as_array().unwrap() {
            if e["e"].as_str() == Some("hooked") {
                early_hook = e["id"].as_u64();
            }
        }
    }
    let st: serde_json::Value = serde_json::from_str(&mg.fishing_pond_state_json()).unwrap();
    if early_hook.is_none() {
        assert_eq!(st["phase"].as_str(), Some("waiting"), "{st}");
        assert_ne!(lure_xz(&mg), landed_at, "the reel pulls the lure in");
    }

    // Hold reel A until a strike hooks (recasting when reeled all the way
    // in), then verify the species came from the equipped lure's row.
    let mut hooked_id = early_hook;
    'outer: for _round in 0..if early_hook.is_some() { 0 } else { 40 } {
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
                // Fully reeled in: cast again, as deep as the first (a
                // short cast lands inside the empty-line gate and comes
                // straight back).
                tick(&mut mg, 0, true);
                for _ in 0..108 {
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

/// The page frames the pond through the venue's scripted camera, not a
/// visitor orbit: the shore view while idle, then the cast press and the
/// wind-up move it out over the water, and a recast brings the shore view
/// back. Retail gives the player no camera control in the fishing mode.
#[test]
fn the_page_camera_follows_the_cast() {
    let Some(mut mg) = loaded() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    assert!(mg.fishing_pond_start(0, 1, 2, 60, 0, 0, 0, 0, 0xBADF00D));
    let aspect = 4.0 / 3.0;
    let before = mg.fishing_venue_vp(0, aspect);
    assert_eq!(before.len(), 16);
    mg.fishing_pond_tick(0, false, 0);
    let shore = mg.fishing_venue_vp(0, aspect);
    assert_eq!(shore, before, "the idle frame is the seat framing");
    mg.fishing_pond_tick(0, true, 0);
    for _ in 0..legaia_engine_fishing::fishing::WINDUP_FRAMES {
        mg.fishing_pond_tick(0, false, 0);
    }
    let cast = mg.fishing_venue_vp(0, aspect);
    assert_eq!(cast.len(), 16);
    assert_ne!(cast, shore, "the wind-up moves the camera");
    // The sky strip is re-seated with the camera.
    assert!(!mg.fishing_sky_positions(0).is_empty());
    eprintln!(
        "[ran] shore vp {:?}\n      cast vp {:?}",
        &shore[12..16],
        &cast[12..16]
    );
}

/// The page's venue bake stands the play hosts' floor-pass selection: Buma's
/// bake carries `other1`'s three placed records (its mountains and bank, rows
/// `102..=110`), Vidna's none of them, so the Vidna view no longer shows
/// Buma's mountains. Selecting a venue swaps the buffers once.
#[test]
fn each_venue_bake_stands_its_own_floor_pass() {
    let Some(mut mg) = loaded() else {
        return;
    };
    assert!(mg.fishing_scene_ready(), "the venue scene decodes");
    assert!(
        !mg.fishing_scene_select_venue(0),
        "Buma is the opening bake"
    );
    let buma = mg.fishing_scene_positions().len();
    let buma_ground = mg.fishing_scene_ground_base();
    assert!(mg.fishing_scene_select_venue(1), "Vidna swaps the bake");
    assert!(!mg.fishing_scene_select_venue(1), "once");
    let vidna = mg.fishing_scene_positions().len();
    assert!(
        vidna < buma,
        "Vidna drops Buma's placed props: {vidna} vs {buma}"
    );
    let vidna_ground = mg.fishing_scene_ground_base();
    assert!(vidna_ground >= 0 && vidna_ground < buma_ground);
    // The ground span is the same heightfield, appended last in each bake.
    assert_eq!(
        buma as i32 / 3 - buma_ground,
        vidna as i32 / 3 - vidna_ground
    );
    assert!(mg.fishing_scene_select_venue(0));
    assert_eq!(mg.fishing_scene_positions().len(), buma);
}

/// The landed fish draws on this page as on the play hosts: the shared
/// catch bodies (`fishing_catch_*`, one span per species), every one
/// collapsed before the catch, the landed one posed through the close-up and
/// then hung from the plate's line (`fishing_catch_line_json`).
#[test]
fn the_landed_fish_is_posed_and_hung_on_the_plate() {
    let Some(mut mg) = loaded() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    assert!(mg.fishing_pond_start(0, 1, 2, 60, 0, 0, 0, 0, 0xBADF00D));
    let n = mg.fishing_catch_positions().len();
    assert!(n > 0, "the ten bodies decode");
    assert_eq!(mg.fishing_catch_uvs().len() / 2, n / 3);
    assert_eq!(mg.fishing_catch_flat_rgba().len() / 4, n / 3);
    let spread = |p: &[f32]| {
        let (mut lo, mut hi) = (f32::INFINITY, f32::NEG_INFINITY);
        for v in p.chunks(3) {
            lo = lo.min(v[1]);
            hi = hi.max(v[1]);
        }
        hi - lo
    };
    assert_eq!(
        spread(&mg.fishing_catch_positions()),
        0.0,
        "nothing shows yet"
    );
    assert_eq!(mg.fishing_catch_line_json(), "null");

    let state = |mg: &LegaiaMinigames| -> serde_json::Value {
        serde_json::from_str(&mg.fishing_pond_state_json()).unwrap()
    };
    let mut landed = false;
    for f in 0..200_000usize {
        let st = state(&mg);
        match st["phase"].as_str() {
            Some("landed") => {
                landed = true;
                break;
            }
            Some("idle") => {
                mg.fishing_pond_tick(0, true, 0);
                for _ in 0..legaia_engine_fishing::fishing::WINDUP_FRAMES + 112 {
                    mg.fishing_pond_tick(0, false, 0);
                }
                mg.fishing_pond_tick(0, true, 0);
            }
            Some("hooked") => {
                let t = st["tension"].as_i64().unwrap();
                mg.fishing_pond_tick(if t < 0x800 { 0x40 } else { 0 }, false, 0);
            }
            _ => {
                let m = if f % 65 < 25 { 0x40 } else { 0 };
                mg.fishing_pond_tick(m, false, m);
            }
        }
    }
    assert!(landed, "a fish lands");
    let mut lifted = 0;
    let mut line = None;
    for _ in 0..2000 {
        mg.fishing_pond_tick(0, false, 0);
        if spread(&mg.fishing_catch_positions()) > 0.0 {
            lifted += 1;
        }
        let l = mg.fishing_catch_line_json();
        if l != "null" {
            line = Some(l);
        }
    }
    assert!(lifted > 100, "the body shows through the catch: {lifted}");
    let line: serde_json::Value =
        serde_json::from_str(&line.expect("the plate hangs it from a line")).unwrap();
    assert_eq!(line["top"][1].as_i64(), Some(0), "{line}");
    assert_eq!(line["top"][0], line["tie"][0], "{line}");
    eprintln!("[ran] plate line {line}");
}
