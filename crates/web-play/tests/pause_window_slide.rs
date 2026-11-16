//! Disc-gated: the pause menu's windows slide in from their parks, as the
//! window walker `FUN_80031AE4` moves them, and nothing of them shows past
//! the 320x240 game area on a wide canvas.
//!
//! Retail creates each window off the screen edge its descriptor names
//! (`FUN_800326AC`) and slides it home over twelve menu frames. The page
//! draws each window at the live origin the shared window list holds, so the
//! root list comes in from the left and the party panel from the right; on a
//! canvas wider than the stage the parked and sliding parts are clipped to
//! the stage rather than drawn in its margins.
//!
//! Skips + passes when `LEGAIA_DISC_BIN` is unset.

#![cfg(not(target_arch = "wasm32"))]

use legaia_web_play::runtime::LegaiaRuntime;

/// The pad word's Start bit.
const START: u16 = 0x0008;
/// A canvas twice the stage's height and four times its width: stage scale
/// 2, the stage at x 320..960.
const SURFACE: (u32, u32) = (1280, 480);
const STAGE_X: (i64, i64) = (320, 960);

/// Every drawn quad's `[x, y, w, h]` this frame, `None` before the menu
/// draws.
fn quads(rt: &LegaiaRuntime) -> Option<Vec<[i64; 4]>> {
    let json: serde_json::Value =
        serde_json::from_str(&rt.play_menu_draws_json(SURFACE.0, SURFACE.1)).expect("json");
    if !json["open"].as_bool().unwrap_or(false) {
        return None;
    }
    let mut out = Vec::new();
    for list in ["sprites", "texts"] {
        for q in json[list].as_array().expect("list") {
            let d = q["dst"].as_array().expect("dst");
            let v: Vec<i64> = d.iter().map(|n| n.as_i64().expect("int")).collect();
            if v[2] > 0 && v[3] > 0 {
                out.push([v[0], v[1], v[2], v[3]]);
            }
        }
    }
    Some(out)
}

#[test]
fn pause_windows_slide_in_inside_the_stage_or_skip() {
    let Ok(disc) = std::env::var("LEGAIA_DISC_BIN") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let mut rt = LegaiaRuntime::new();
    rt.load_disc(std::fs::read(&disc).expect("read disc"), String::new())
        .expect("load disc");
    rt.enter_field("town01").expect("enter town01");
    for _ in 0..60 {
        rt.play_frame_ticks(1, 0, 0).expect("tick");
    }
    rt.play_frame_ticks(1, START, START).expect("Start");
    assert!(rt.play_menu_is_open(), "Start opens the pause menu");
    // Run the wipe out to the first frame the menu draws.
    let mut frames = Vec::new();
    for _ in 0..80 {
        rt.play_frame_ticks(1, 0, 0).expect("tick");
        if let Some(q) = quads(&rt) {
            frames.push(q);
        }
        if frames.len() == 16 {
            break;
        }
    }
    assert_eq!(frames.len(), 16, "the menu draws once the wipe spawns it");
    for (f, q) in frames.iter().enumerate() {
        for r in q {
            assert!(
                r[0] >= STAGE_X.0 && r[0] + r[2] <= STAGE_X.1,
                "frame {f}: quad {r:?} outside the stage"
            );
        }
    }
    let area = |q: &Vec<[i64; 4]>| q.iter().map(|r| r[2] * r[3]).sum::<i64>();
    let settled = frames.last().expect("a frame");
    // Once the walker has finished, the layout holds still.
    assert_eq!(
        frames[frames.len() - 2],
        *settled,
        "the windows rest on their homes"
    );
    // The first drawn frames are the windows coming in: less of them is on
    // the stage than once they have landed, and the amount grows.
    assert!(
        area(&frames[1]) < area(settled),
        "the windows start off their homes ({} vs {})",
        area(&frames[1]),
        area(settled)
    );
    let moving = frames.windows(2).filter(|w| w[0] != w[1]).count();
    assert!(
        moving >= 8,
        "the slide spans the walker's twelve frames ({moving})"
    );
}
