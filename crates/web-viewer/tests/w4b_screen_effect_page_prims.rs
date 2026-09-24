//! Disc-gated: the play page **draws** the field screen-effect wash.
//!
//! The field VM's op `0x34` sub-0 arm spawns a colour-tween actor whose
//! per-frame `FUN_80024EE4(layer, blend, packed)` call is retail's
//! scene-entry fade-from-black and the door prologue's fade-to-black. The
//! port simulated that tween on both hosts and neither host drew it: the
//! pool published a push per frame and no render surface read it, which no
//! gate could see because the producer was live, tagged and test-covered.
//!
//! This is the consumer's ladder on the page side. It enters a shipped
//! scene whose own entry script issues the instruction - no hand-written
//! bytecode, no seeded actor - and asserts three things about the frame the
//! page uploads: the pool publishes a push, the screen-prim pass carries at
//! least that many primitives, and the wash's own run is a **semi-transparent**
//! run whose vertex colour is not black. The last two are what separates
//! "the simulation ran" from "something reached the framebuffer".
//!
//! The native window composites the identical list through the same
//! `screen_effect_push_prims` emitter; that pairing is held by
//! `scripts/ci/check-ui-host-drift.py`'s `SIM_PAIRS` row, since the window's
//! draw section lives in a binary no integration test links.
//!
//! Skipped (passes) when `LEGAIA_DISC_BIN` is unset.

#![cfg(not(target_arch = "wasm32"))]

use legaia_web_viewer::runtime::LegaiaRuntime;

/// The scene the ladder drives. Its bundle MAN carries exactly one decoded
/// `34` sub-0 (the disc-wide census counts 583 across 57 scenes), and it is
/// issued from the scene-entry script rather than from a door prologue, so a
/// plain `enter_field` reaches it. Named rather than re-derived: a scan that
/// picks "the first scene carrying the op" would pick one whose only site is
/// a departure prologue and then measure nothing.
const SCENE: &str = "town0e";

/// Byte offsets into the shared `ScreenVertex` layout - re-stated from
/// `legaia_engine_ui::screen_prim` so a silent stride change fails here
/// rather than reading a neighbouring field as a colour.
const STRIDE: usize = 48;
const OFF_COLOR: usize = 24;

fn loaded_runtime() -> Option<LegaiaRuntime> {
    let disc = std::env::var("LEGAIA_DISC_BIN").ok()?;
    let bytes = std::fs::read(disc).ok()?;
    let mut rt = LegaiaRuntime::new();
    rt.load_disc(bytes, String::new()).ok()?;
    Some(rt)
}

#[test]
fn play_page_draws_the_field_screen_effect_wash() {
    let Some(mut rt) = loaded_runtime() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    };
    assert_eq!(
        STRIDE as u64,
        legaia_engine_ui::screen_prim::SCREEN_VERTEX_STRIDE,
        "the shared vertex stride moved"
    );
    assert_eq!(
        OFF_COLOR as u64,
        legaia_engine_ui::screen_prim::SCREEN_VERTEX_OFF_COLOR,
        "the shared colour offset moved"
    );
    rt.enter_field(SCENE).expect("enter the scene");

    let mut first = None;
    let mut drawn = 0u32;
    for f in 0..900u32 {
        let _ = rt.tick_frame();
        let pushes = rt.play_screen_effect_push_count();
        if pushes == 0 {
            continue;
        }
        first.get_or_insert(f);

        // (1) the pass carries the wash.
        let prims = rt.play_screen_prim_count();
        assert!(
            prims >= pushes,
            "tick {f}: {pushes} screen-effect push(es) but the pass carries \
             {prims} primitive(s)"
        );

        // (2) it is a *blended* run. A wash linked at ABR 1 (`B + F`) or 2
        // (`B - F`) that came out opaque would replace the frame with a flat
        // colour instead of ramping it - the failure a host gets by reading
        // the emitter's second argument as an ordering-table depth.
        let runs = rt.play_screen_prim_runs();
        assert!(!runs.is_empty(), "tick {f}: no draw runs with a wash up");
        assert!(
            runs.chunks(3).any(|r| r[0] >= 1),
            "tick {f}: every run is opaque - the wash lost its ABR mode"
        );

        // (3) the wash is not black. `packed == 0` draws nothing under either
        // equation, so a colour stream of zeros is the shape a lost channel
        // swap or a dropped operand takes.
        let verts = rt.play_screen_prim_vertex_bytes();
        assert_eq!(
            verts.len() % STRIDE,
            0,
            "tick {f}: vertex stream is not a whole number of vertices"
        );
        let non_black = verts.chunks(STRIDE).any(|v| {
            (0..3).any(|c| {
                let o = OFF_COLOR + c * 4;
                f32::from_le_bytes([v[o], v[o + 1], v[o + 2], v[o + 3]]) > 0.0
            })
        });
        assert!(
            non_black,
            "tick {f}: the frame's screen primitives are all black - a \
             neutral wash is not a drawn fade"
        );

        drawn += 1;
        if drawn >= 30 {
            break;
        }
    }

    eprintln!(
        "[w4b screen-effect page] {SCENE}: first push at tick {first:?}, {drawn} drawn frame(s)"
    );
    assert!(
        first.is_some(),
        "{SCENE} published no screen-effect push in 900 ticks - the scene's \
         entry script no longer reaches op 0x34 sub-0"
    );
}

/// Two shared kernels an earlier wave added that **no ladder on either host
/// entered** - the reach export finds them live and never executed. Each gets
/// a rung, and the second rung's answer is a negative one.
///
/// `Camera::take_camera_snap_beats` runs once per frame from the page's
/// `resolve_camera_frame`, but only inside the scripted branch, so a ladder
/// that never parks in a cutscene never reaches it. The published camera
/// frame names the arm it took, which is the assertion.
#[test]
fn a_cutscene_scene_drives_the_pages_snap_beat_bank() {
    let Some(mut rt) = loaded_runtime() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    };
    // The opening prologue's first tableau - a scene whose timeline owns the
    // camera, rather than a town where the follow arm always wins.
    rt.enter_field("opdeene").expect("enter the prologue scene");
    let mut arms: Vec<String> = Vec::new();
    for _ in 0..900u32 {
        let _ = rt.tick_frame();
        let v: serde_json::Value =
            serde_json::from_str(&rt.play_camera_view_json()).expect("camera json");
        let arm = v["arm"].as_str().unwrap_or("").to_string();
        if !arms.contains(&arm) {
            arms.push(arm.clone());
        }
        if arm == "cutscene" {
            eprintln!("[w4b snap beats] opdeene reached the cutscene camera arm");
            return;
        }
    }
    panic!("opdeene never published a cutscene camera frame - arms seen: {arms:?}");
}

/// `FieldSceneAnim::ocean_only` is the **damaged-bundle** arm: it installs
/// only when a kingdom bundle's slot-5 CLUT-walk table fails to parse. This
/// rung enters the kingdom scenes and asserts the page installed the
/// *walker*, which is the measurement behind the disclosure that no shipped
/// content reaches the fallback - a ladder cannot enter it without a
/// modified disc.
#[test]
fn a_kingdom_scene_installs_the_walker_not_the_ocean_fallback() {
    let Some(mut rt) = loaded_runtime() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    };
    let mut seen = 0u32;
    for scene in ["map01", "map02", "map03"] {
        if rt.enter_field(scene).is_err() {
            continue;
        }
        for _ in 0..60u32 {
            let _ = rt.tick_frame();
        }
        let kind = rt.play_field_anim_kind();
        if kind == 0 {
            continue;
        }
        seen += 1;
        assert_eq!(
            kind & 2,
            0,
            "{scene}: the page installed the legacy ocean-head fallback, so \
             its slot-5 CLUT-walk table stopped parsing"
        );
        assert_eq!(kind & 1, 1, "{scene}: no CLUT walker installed");
    }
    eprintln!(
        "[w4b ocean fallback] {seen} kingdom scene(s) installed the walker, none the fallback"
    );
    assert!(
        seen > 0,
        "no kingdom scene installed a VRAM animator - the rung measured nothing"
    );
}
