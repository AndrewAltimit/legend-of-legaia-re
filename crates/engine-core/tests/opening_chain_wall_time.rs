//! Disc-gated pacing oracle: the zero-input New Game opening chain runs at
//! retail wall-time.
//!
//! Retail's cutscene records are paced in **display frames**. Op-`0x4A`
//! `WaitFrames` accumulates the frame-skip factor `DAT_1F800393` into
//! `ctx[+0x54]` once per logic tick, and the camera mover (`FUN_801DC0BC`)
//! accumulates the same factor into its progress - so a logic tick that runs
//! once per `dt` display frames credits exactly one unit per display frame,
//! and every authored duration is a duration in 60 Hz frames.
//!
//! The engine's sim clock runs at 100 Hz, so a timeline stepped once per sim
//! tick drains those waits 1.67x too fast. `World::step_spawned_record_contexts`
//! paces the timeline off the retail-frame sub-clock instead; this test pins
//! the resulting wall-times against a headless retail capture of the same
//! chain (per-leg display-frame counts read off the live scene-label global
//! while a recompiled retail build played the opening with zero input):
//!
//! | leg       | retail frames | retail seconds |
//! |-----------|---------------|----------------|
//! | `opdeene` | 3953          | 65.9           |
//! | `opstati` | 2040          | 34.0           |
//! | `opurud`  | 2507          | 41.8           |
//! | `map01`   | 1845          | 30.8           |
//! | total     | 10345         | 172.4          |
//!
//! The per-leg tolerance is deliberately loose (25 %) - the point is to catch
//! a *unit* regression (a leg running at sim-tick rate is 67 % fast, far
//! outside it), not to freeze the current numbers.
//!
//! The errors are **one-sided**: each retail leg span includes the scene's
//! load + mode-transition window (label flip -> the record's first world
//! tick), which the engine does not model - its scene loads are instant. The
//! `map01` leg carries the largest such window and gets its own asymmetric
//! band, pinned by a per-frame decomposition of the retail capture against
//! the record's authored ops (`chain_cam_full` camera trace vs the P2[38]
//! disasm): 355 frames of world-map load + mode-2 init before the record's
//! first op, then 322 frames of authored title-card `WaitFrames` (1:1),
//! then the `apply 900` fly-in glide running concurrent with the authored
//! `4A` 160 + 600 + 330 tail (1090 frames, the 3-page Mist crawl scrolling
//! under them, non-blocking), then ~40 frames of transition fade. The
//! engine plays the record-authored ~1410 frames and skips the ~440 frames
//! of load + teardown, so it runs ~24 % SHORT and must never run LONG - a
//! leg running long means record pacing broke (the historical failure was
//! the final crawl being serialized against the authored tail waits).
//!
//! Skip-passes without disc data (CLAUDE.md convention).

use legaia_engine_core::scene::SceneHost;
use std::path::PathBuf;

/// Engine sim ticks per second.
const SIM_HZ: f64 = 100.0;
/// Retail display frames per second.
const RETAIL_FPS: f64 = 60.0;

/// `(scene, retail display frames)` for each zero-input opening leg.
const RETAIL_LEGS: &[(&str, f64)] = &[
    ("opdeene", 3953.0),
    ("opstati", 2040.0),
    ("opurud", 2507.0),
    ("map01", 1845.0),
];

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

fn skip_or_host() -> Option<SceneHost> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    let extracted = extracted_dir()?;
    Some(SceneHost::open_extracted(&extracted).expect("open SceneHost"))
}

#[test]
fn opening_chain_legs_run_at_retail_wall_time() {
    let Some(mut host) = skip_or_host() else {
        return;
    };
    host.world.begin_new_game();
    host.enter_field_scene(legaia_asset::new_game::OPENING_CUTSCENE_SCENE, 0)
        .expect("enter opdeene");

    // Drive the chain with zero input, recording the tick count each leg
    // occupies until the run reaches Rim Elm.
    let mut legs: Vec<(String, u32)> = Vec::new();
    let mut label = host.world.active_scene_label.clone();
    let (mut start, mut ticks) = (0u32, 0u32);
    while ticks < 120_000 {
        let _ = host.tick();
        ticks += 1;
        if host.world.active_scene_label != label {
            legs.push((label, ticks - start));
            label = host.world.active_scene_label.clone();
            start = ticks;
            if label == "town01" {
                break;
            }
        }
    }
    assert_eq!(label, "town01", "chain reached Rim Elm (ticks {ticks})");

    let mut engine_total = 0.0f64;
    let mut retail_total = 0.0f64;
    for &(scene, retail_frames) in RETAIL_LEGS {
        let observed = legs
            .iter()
            .find(|(l, _)| l == scene)
            .unwrap_or_else(|| panic!("chain played the {scene} leg (legs: {legs:?})"))
            .1;
        let engine_s = f64::from(observed) / SIM_HZ;
        let retail_s = retail_frames / RETAIL_FPS;
        engine_total += engine_s;
        retail_total += retail_s;
        let signed = (engine_s - retail_s) / retail_s;
        let err_pct = signed.abs() * 100.0;
        eprintln!(
            "[pacing] {scene}: engine {engine_s:.1}s vs retail {retail_s:.1}s ({err_pct:.1}%)"
        );
        if scene == "map01" {
            // Asymmetric band (see the module doc): the engine skips the
            // ~440 retail frames of world-map load + teardown, so the leg
            // legitimately runs up to ~24% short - but the record-authored
            // pacing itself is 1:1, so a leg running LONG means the record
            // pacing broke (e.g. the final crawl serialized against the
            // authored `4A` 600 + 330 tail instead of scrolling under it).
            assert!(
                signed < 0.05,
                "map01 leg runs {err_pct:.1}% LONG (engine {engine_s:.1}s vs retail \
                 {retail_s:.1}s) - the fly-in record's authored pacing is 1:1 with \
                 retail, so a long leg means a wait/crawl was serialized that retail \
                 runs concurrently"
            );
            assert!(
                signed > -0.28,
                "map01 leg is {err_pct:.1}% short (engine {engine_s:.1}s vs retail \
                 {retail_s:.1}s) - more than the un-modeled ~440-frame load + \
                 teardown window accounts for"
            );
        } else {
            assert!(
                signed.abs() < 0.25,
                "{scene} leg is {err_pct:.1}% off retail wall-time \
                 (engine {engine_s:.1}s vs retail {retail_s:.1}s) - a leg stepped at the \
                 100 Hz sim rate instead of the 60 Hz retail-frame sub-clock runs ~67% fast"
            );
        }
    }
    let total_signed = (engine_total - retail_total) / retail_total;
    let total_pct = total_signed.abs() * 100.0;
    eprintln!(
        "[pacing] chain: engine {engine_total:.1}s vs retail {retail_total:.1}s ({total_pct:.1}%)"
    );
    // One-sided like the legs: the engine's instant scene loads leave the
    // whole chain a little short of retail; running LONG is the regression
    // signal (a serialized wait/crawl somewhere in the chain).
    assert!(
        total_signed < 0.05 && total_signed > -0.15,
        "whole chain is {total_pct:.1}% off retail wall-time \
         (engine {engine_total:.1}s vs retail {retail_total:.1}s)"
    );
}
