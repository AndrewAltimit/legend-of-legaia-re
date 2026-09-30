//! Disc-gated: the fishing **line** reaches the draw path on both browser
//! hosts while a fish is hooked.
//!
//! Retail draws one `LINE_G2` packet a frame from the lure tick
//! (`FUN_801D26CC`): fish end to rod tip, clipped by `FUN_801D56E4`. The
//! engine builds it in `PondSession::line_frame`; the play page wraps it with
//! the shared `ui_fishing_line` builder into its screen-prim pass (the native
//! window's `fishing_line_screen_prims` makes the same two calls in a `bin/`
//! target no test can reach), and the minigames page strokes the endpoints
//! `fishing_line_json` hands back. Both run over the venue's real rods, lifted
//! off the `other1` bank, and both draw the rod model itself too
//! (`PondSession::rod_faces`: the play page through the shared
//! `ui_fishing_rod` builder, the minigames page off `fishing_rod_json`).

#![cfg(not(target_arch = "wasm32"))]

use legaia_web_viewer::minigames::LegaiaMinigames;
use legaia_web_viewer::runtime::LegaiaRuntime;

const REEL_A: u32 = 0x40;

fn disc_bytes() -> Option<Vec<u8>> {
    let p = std::env::var_os("LEGAIA_DISC_BIN")?;
    std::fs::read(p).ok()
}

/// The screen-prim vertices' colours, `(r, g, b)` in `0..=255`, off the page's
/// `ScreenVertex` stream (stride 48, `color: vec4<f32>` at 24, `flags` at 40).
fn prim_colours(bytes: &[u8]) -> Vec<([u8; 3], u32)> {
    bytes
        .as_chunks::<48>()
        .0
        .iter()
        .map(|v| {
            let f = |o: usize| f32::from_le_bytes(v[o..o + 4].try_into().unwrap());
            let flags = u32::from_le_bytes(v[40..44].try_into().unwrap());
            ([24, 28, 32].map(|o| (f(o) * 255.0).round() as u8), flags)
        })
        .collect()
}

#[test]
fn the_play_page_draw_list_carries_the_line_while_a_fish_is_hooked() {
    let Some(bytes) = disc_bytes() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rt = LegaiaRuntime::new();
    if rt.load_disc(bytes, String::new()).is_err() || rt.enter_field("town01").is_err() {
        eprintln!("[skip] the play page could not reach town01");
        return;
    }
    if !rt.play_fishing_start() {
        eprintln!("[skip] the fishing overlay did not decode");
        return;
    }
    let phase = |rt: &LegaiaRuntime| -> String {
        let v: serde_json::Value =
            serde_json::from_str(&rt.play_fishing_state_json()).expect("state json");
        v["phase"].as_str().unwrap_or_default().to_string()
    };
    let tick = |rt: &mut LegaiaRuntime, pad: u16, n: usize| {
        rt.set_pad(pad);
        for _ in 0..n {
            rt.tick_frame().expect("tick");
        }
    };
    const CROSS: u16 = 0x4000;
    const CIRCLE: u16 = 0x2000;
    let cast = |rt: &mut LegaiaRuntime| {
        tick(rt, 0, 1);
        tick(rt, CIRCLE, 1);
        tick(rt, 0, 40);
        tick(rt, CIRCLE, 1);
        tick(rt, 0, 30);
    };
    let line_drawn = |rt: &LegaiaRuntime| {
        let c = prim_colours(&rt.play_screen_prim_vertex_bytes());
        let has = |rgb: u8| c.iter().any(|(col, flags)| *flags == 0 && *col == [rgb; 3]);
        has(0x30) && has(0x80)
    };
    // The rod model's packet colours: each rod's dominant flat colour
    // (rod 0 red, rod 1 blue, rod 2 gold), read off the `other1` models.
    let rod_drawn = |rt: &LegaiaRuntime| {
        let c = prim_colours(&rt.play_screen_prim_vertex_bytes());
        [[0x80, 0x28, 0x28], [0x10, 0x10, 0x90], [0x9C, 0x80, 0x00]]
            .iter()
            .any(|rgb| c.iter().filter(|(col, f)| *f == 0 && col == rgb).count() >= 3)
    };

    assert!(!line_drawn(&rt), "no line at the shore");
    assert!(!rod_drawn(&rt), "no rod before the cast");
    cast(&mut rt);
    let mut hooked = false;
    for _ in 0..60_000 {
        tick(&mut rt, CROSS, 1);
        match phase(&rt).as_str() {
            "hooked" => {
                hooked = true;
                break;
            }
            "idle" => cast(&mut rt),
            _ => {}
        }
    }
    assert!(hooked, "no strike on the play page within the budget");
    let mut frames = 0;
    for _ in 0..8 {
        tick(&mut rt, 0, 1);
        if phase(&rt) != "hooked" {
            break;
        }
        assert!(
            line_drawn(&rt),
            "the hooked frame's screen prims carry no fish-to-rod line"
        );
        assert!(
            rod_drawn(&rt),
            "the hooked frame's screen prims carry no rod model"
        );
        frames += 1;
    }
    assert!(
        frames > 0,
        "the fight resolved before a frame could be drawn"
    );
}

#[test]
fn the_minigames_page_gets_the_engine_line_off_the_real_rod() {
    let Some(bytes) = disc_bytes() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut mg = LegaiaMinigames::new();
    if mg.load_disc(bytes).is_err() || !mg.fishing_pond_ready() || !mg.fishing_scene_ready() {
        eprintln!("[skip] the pond tables or the venue scene did not decode");
        return;
    }
    assert!(mg.fishing_pond_start(0, 1, 2, 100, 0, 0, 0, 0, 0x1357_9BDF));
    let phase = |mg: &LegaiaMinigames| -> String {
        let v: serde_json::Value =
            serde_json::from_str(&mg.fishing_pond_state_json()).expect("state json");
        v["phase"].as_str().unwrap_or_default().to_string()
    };
    assert_eq!(
        mg.fishing_line_json(100, 150, true),
        "null",
        "no line at the shore"
    );
    assert_eq!(mg.fishing_rod_json(), "[]", "no rod at the shore");

    let mut hooked = false;
    'casts: for _ in 0..24 {
        mg.fishing_pond_tick(0, true, 0);
        for _ in 0..4000 {
            if phase(&mg) == "power" {
                break;
            }
            mg.fishing_pond_tick(0, false, 0);
        }
        for _ in 0..40 {
            mg.fishing_pond_tick(0, false, 0);
        }
        mg.fishing_pond_tick(0, true, 0);
        for _ in 0..4000 {
            if phase(&mg) == "waiting" {
                break;
            }
            mg.fishing_pond_tick(0, false, 0);
        }
        let mut f = 0usize;
        while f < 4000 && phase(&mg) == "waiting" {
            let held = (f / 6).is_multiple_of(2);
            mg.fishing_pond_tick(
                if held { REEL_A } else { 0 },
                false,
                if f.is_multiple_of(6) { REEL_A } else { 0 },
            );
            let _ = mg.fishing_line_json(100, 150, true);
            f += 1;
        }
        if phase(&mg) == "hooked" {
            hooked = true;
            break 'casts;
        }
    }
    assert!(hooked, "no strike on the minigames page within the budget");
    mg.fishing_pond_tick(REEL_A, false, 0);
    let line: serde_json::Value =
        serde_json::from_str(&mg.fishing_line_json(100, 150, true)).expect("line json");
    assert_eq!(line["fish"], serde_json::json!([100, 150]), "{line}");
    assert_eq!(line["fish_rgb"], serde_json::json!([0x30, 0x30, 0x30]));
    assert_eq!(line["rod_rgb"], serde_json::json!([0x80, 0x80, 0x80]));
    // The rod end is the real rod's projected tip, inside the clip window.
    let rx = line["rod"][0].as_i64().unwrap();
    let ry = line["rod"][1].as_i64().unwrap();
    assert!((0..=320).contains(&rx) && (4..=228).contains(&ry), "{line}");
    // The rod model is out with it: faces in draw order (farthest bucket
    // first), the tip a corner of a face near the line's own bucket.
    let rod: serde_json::Value = serde_json::from_str(&mg.fishing_rod_json()).expect("rod json");
    let faces = rod.as_array().expect("a face list");
    assert!(faces.len() > 10, "{} rod faces", faces.len());
    let ots: Vec<i64> = faces.iter().map(|f| f["ot"].as_i64().unwrap()).collect();
    assert!(ots.windows(2).all(|w| w[0] >= w[1]), "draw order {ots:?}");
    let line_ot = line["ot"].as_i64().unwrap();
    assert!(
        faces.iter().any(|f| {
            let xy = f["xy"].as_array().unwrap();
            (0..4).any(|i| {
                xy[2 * i].as_i64() == Some(rx)
                    && xy[2 * i + 1].as_i64() == Some(ry)
                    && (f["ot"].as_i64().unwrap() - line_ot).abs() <= 1
            })
        }),
        "no rod face meets the line's rod end {line}"
    );
    // A lure that did not project draws no line.
    mg.fishing_pond_tick(REEL_A, false, 0);
    assert_eq!(mg.fishing_line_json(0, 0, false), "null");
}
