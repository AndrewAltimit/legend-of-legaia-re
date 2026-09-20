//! Disc-gated pad ladder for the **publisher-logo boot phase** - the stage
//! retail plays before the title card (PROT 0895 `init.pak`, sequencer
//! `FUN_801CEFD4`).
//!
//! `docs/tooling/reach-triage.md` files `801cefd4` under *NO-LADDER, content
//! not driven* with the fixture spelled out: "One rung would do it: open the
//! logo phase and step the sequencer to its end". Every other union member
//! starts at a scene, a battle or the **title card** - the composition
//! ladder's rung 1 calls `boot_title_start`, which is the stage *after* this
//! one - so the sequencer, retail's play order and the quad emitter's
//! per-logo tables have never executed under coverage.
//!
//! This ladder is that rung, on the browser play page: the same
//! [`LegaiaRuntime`] object `site/js/play-app.js` constructs, driven with
//! nothing but pad words through `boot_logos_step` and read once a frame
//! through `boot_logos_draws_json`, exactly as the page's animation frame
//! does. Nothing is seated and no state is poked - `boot_logos_start` is the
//! page's own boot entry.
//!
//! ## The ladder
//!
//! | # | rung | what it proves |
//! |---|---|---|
//! | 1 | the atlas resolves off PROT 0895 | `build_atlas_from_init_pak` decodes the four TIMs and the phase opens |
//! | 2 | the sequence plays out on a neutral pad | the sequencer walks all three steps in retail's order, for exactly the frame budget its own table names, emitting each logo's quads at their retail destinations |
//! | 3 | Start skips the rest | the skip request the page hooks to Start ends the phase early |
//!
//! ## Why the assertions are on the composed frame
//!
//! A sequencer that runs and emits nothing reads identically to one that
//! played, so each rung scores the **draws**: which logo's quads the frame
//! carries, where they land, and how the fade level ramps. Retail's play
//! order is SCEA -> Contrail -> PROKION while the TIMs sit in the file as
//! PROKION, Contrail, SCEA, so the order the draws arrive in is itself the
//! evidence that the port walks `RETAIL_SEQUENCE` rather than the file.
//!
//! A frame's logo is recovered from its destination rects against
//! `LOGO_QUADS` - the port's own retail table - rather than by asking the
//! session, because asking the session proves only that the session agrees
//! with itself.
//!
//! Skips + passes when `LEGAIA_DISC_BIN` is unset. CI runs without disc data.

#![cfg(not(target_arch = "wasm32"))]

use legaia_engine_core::input::PadButton;
use legaia_engine_core::publisher_logos::{
    LOGO_CONTRAIL, LOGO_COUNT, LOGO_PROKION, LOGO_QUADS, LOGO_SCEA, RETAIL_SEQUENCE, STAGE,
};
use legaia_web_viewer::runtime::LegaiaRuntime;

/// The page's surface. At 960x720 the 640x480 boot stage fits at integer
/// scale 1, so a composed `dst` is the retail rect plus the letterbox
/// origin - which is what makes the per-quad comparison below exact.
const W: u32 = 960;
const H: u32 = 720;

fn letterbox() -> (i32, i32) {
    let scale = (W / STAGE.0).min(H / STAGE.1).max(1);
    (
        (W as i32 - (STAGE.0 * scale) as i32) / 2,
        (H as i32 - (STAGE.1 * scale) as i32) / 2,
    )
}

/// Frames the whole sequence takes at a neutral pad: every step's
/// fade-in + hold + fade-out.
fn sequence_frames() -> u32 {
    RETAIL_SEQUENCE.iter().map(|s| s.frames() as u32).sum()
}

/// One composed frame: its quads' `(dst, alpha)`.
fn frame_quads(rt: &LegaiaRuntime) -> (Vec<(i32, i32, u32, u32)>, f64) {
    let v: serde_json::Value =
        serde_json::from_str(&rt.boot_logos_draws_json(W, H)).unwrap_or(serde_json::Value::Null);
    if v["active"] != true {
        return (Vec::new(), 0.0);
    }
    let sprites = v["sprites"].as_array().cloned().unwrap_or_default();
    let alpha = sprites
        .iter()
        .filter_map(|s| s["color"][3].as_f64())
        .fold(0.0f64, f64::max);
    let rects = sprites
        .iter()
        .map(|s| {
            let d = &s["dst"];
            (
                d[0].as_i64().unwrap_or(0) as i32,
                d[1].as_i64().unwrap_or(0) as i32,
                d[2].as_u64().unwrap_or(0) as u32,
                d[3].as_u64().unwrap_or(0) as u32,
            )
        })
        .collect();
    (rects, alpha)
}

/// The atlas index whose `LOGO_QUADS` row those destinations are, or `None`
/// when they match no logo.
fn logo_of(rects: &[(i32, i32, u32, u32)]) -> Option<usize> {
    let (ox, oy) = letterbox();
    (0..LOGO_COUNT).find(|&idx| {
        let want: Vec<(i32, i32, u32, u32)> = LOGO_QUADS[idx]
            .iter()
            .map(|q| (ox + q.dst.0, oy + q.dst.1, q.dst.2, q.dst.3))
            .collect();
        !want.is_empty() && want == rects
    })
}

fn rung1_atlas(rt: &mut LegaiaRuntime) -> Result<(), String> {
    if !rt.boot_logos_start() {
        return Err("boot_logos_start refused: the PROT 0895 atlas did not build".into());
    }
    if !rt.boot_logos_is_active() {
        return Err("logo phase did not open".into());
    }
    let dims = rt.boot_logos_atlas_dims();
    if dims.len() != 2 || dims[0] == 0 || dims[1] == 0 {
        return Err(format!("atlas dims are {dims:?}"));
    }
    let rgba = rt.boot_logos_atlas_rgba();
    if rgba.len() != (dims[0] as usize) * (dims[1] as usize) * 4 {
        return Err(format!(
            "atlas rgba is {} bytes for {}x{}",
            rgba.len(),
            dims[0],
            dims[1]
        ));
    }
    if rgba.iter().all(|&b| b == 0) {
        return Err("atlas decoded to all-zero pixels".into());
    }
    Ok(())
}

fn rung2_sequence(rt: &mut LegaiaRuntime) -> Result<(), String> {
    let budget = sequence_frames();
    let mut order: Vec<usize> = Vec::new();
    let mut alphas: Vec<f64> = Vec::new();
    let mut frames = 0u32;
    let mut done = false;
    while frames < budget + 8 {
        let (rects, alpha) = frame_quads(rt);
        if rects.is_empty() {
            return Err(format!("logo frame {frames} composed no quads"));
        }
        match logo_of(&rects) {
            Some(idx) => {
                if order.last() != Some(&idx) {
                    order.push(idx);
                }
            }
            None => {
                return Err(format!(
                    "frame {frames} composed {rects:?}, which is no logo's LOGO_QUADS row"
                ));
            }
        }
        alphas.push(alpha);
        // Neutral pad: nothing skips, the sequencer runs on its own clock.
        done = rt.boot_logos_step(0);
        frames += 1;
        if done {
            break;
        }
    }
    if !done {
        return Err(format!("sequence did not finish inside {frames} frames"));
    }
    if frames != budget {
        return Err(format!(
            "sequence took {frames} frames; the step table names {budget}"
        ));
    }
    let want = [LOGO_SCEA, LOGO_CONTRAIL, LOGO_PROKION];
    if order != want {
        return Err(format!(
            "play order was {order:?}, retail's is {want:?} \
             (file order is PROKION, Contrail, SCEA)"
        ));
    }
    // The ramp is the other half of "it played": a stuck level would hold one
    // alpha for the whole run.
    let lo = alphas.iter().cloned().fold(f64::INFINITY, f64::min);
    let hi = alphas.iter().cloned().fold(0.0f64, f64::max);
    if !(hi > 0.9 && lo < 0.2) {
        return Err(format!("fade level never ramped (min {lo:.3} max {hi:.3})"));
    }
    Ok(())
}

fn rung3_skip(rt: &mut LegaiaRuntime) -> Result<(), String> {
    if !rt.boot_logos_start() {
        return Err("logo phase would not re-open for the skip rung".into());
    }
    let mut frames = 0u32;
    for _ in 0..20 {
        if rt.boot_logos_step(0) {
            return Err("phase ended before the skip was pressed".into());
        }
        frames += 1;
    }
    if !rt.boot_logos_step(PadButton::Start.mask()) {
        return Err("Start did not end the logo phase".into());
    }
    if rt.boot_logos_is_active() {
        return Err("phase still active after the skip".into());
    }
    if frames + 1 >= sequence_frames() {
        return Err("the skip did not shorten the phase".into());
    }
    Ok(())
}

fn baseline() -> u32 {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../scripts/replays/w3c_boot_logos_baseline.toml");
    let text = std::fs::read_to_string(&path).expect("w3c_boot_logos_baseline.toml");
    let value: toml::Value = text.parse().expect("baseline TOML parses");
    value["reached"]
        .as_integer()
        .expect("baseline carries `reached`") as u32
}

#[test]
fn w3c_boot_logos_ladder() {
    let Ok(disc) = std::env::var("LEGAIA_DISC_BIN") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let Ok(bytes) = std::fs::read(&disc) else {
        eprintln!("[skip] disc unreadable (disc-gated)");
        return;
    };
    let mut rt = LegaiaRuntime::new();
    rt.load_disc(bytes, String::new()).expect("load_disc");

    type Rung = Box<dyn FnMut(&mut LegaiaRuntime) -> Result<(), String>>;
    let rungs: Vec<(&str, Rung)> = vec![
        ("atlas", Box::new(rung1_atlas)),
        ("sequence", Box::new(rung2_sequence)),
        ("skip", Box::new(rung3_skip)),
    ];
    let mut score = 0u32;
    for (name, mut rung) in rungs {
        match rung(&mut rt) {
            Ok(()) => {
                score += 1;
                eprintln!("[rung {score}] {name}: cleared");
            }
            Err(why) => {
                eprintln!("[stall] rung {} ({name}): {why}", score + 1);
                break;
            }
        }
    }
    eprintln!(
        "w3c_boot_logos_ladder: score {score} (sequence budget {} frames)",
        sequence_frames()
    );
    let base = baseline();
    assert!(
        score >= base,
        "boot-logos ladder regressed: score {score} < baseline {base} \
         (scripts/replays/w3c_boot_logos_baseline.toml)"
    );
    if score > base {
        eprintln!(
            "baseline can ratchet: set reached = {score} in \
             scripts/replays/w3c_boot_logos_baseline.toml"
        );
    }
}
