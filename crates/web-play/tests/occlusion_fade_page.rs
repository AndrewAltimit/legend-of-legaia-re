//! The play page's camera-occlusion fade ("See-through walls"), end to end.
//!
//! The fade is default on in both hosts and is the persisted engine option
//! `OptionsState::occlusion_fade`. On the page it runs through one export
//! per frame, `play_occlusion_fade`, which `site/js/play-view-frame.js` calls
//! inside the frame's `try`. That call once passed three frame locals the
//! tick-owner move into the engine had deleted, so every frame threw a
//! `ReferenceError` the `try` swallowed and the fade never staged - with the
//! box ticked and the option on. The pins here:
//!
//! - **static**: every bare identifier the call passes is declared in
//!   `_frame`, and the page's box writes and mirrors the engine option;
//! - **disc-free**: a stored options blob from before the field existed
//!   reads it as on;
//! - **disc-gated**: on town01, a seat a wall hides from the follow camera
//!   ramps the fade up, and the toggle, the pause menu and the option
//!   setter each stand it down.
//!
//! The disc-gated test skips (passes) when `LEGAIA_DISC_BIN` is unset.

#![cfg(not(target_arch = "wasm32"))]

use legaia_engine_menus::options::OptionsState;
use legaia_web_play::runtime::LegaiaRuntime;

fn site(rel: &str) -> String {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../site")
        .join(rel);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("read {}: {e}", p.display()))
}

/// The text between the bracket opening at `open` and its match.
fn balanced(src: &str, open: usize, l: char, r: char) -> &str {
    let mut depth = 0usize;
    for (i, c) in src[open..].char_indices() {
        if c == l {
            depth += 1;
        } else if c == r {
            depth -= 1;
            if depth == 0 {
                return &src[open + 1..open + i];
            }
        }
    }
    panic!("unbalanced {l}{r} at {open}");
}

/// The body of the method / function whose header starts with `head`.
fn body<'a>(src: &'a str, head: &str) -> &'a str {
    let start = src
        .find(head)
        .unwrap_or_else(|| panic!("{head} is defined"));
    let open = start + src[start..].find('{').expect("function body");
    balanced(src, open, '{', '}')
}

/// Bare identifiers in `expr`: not a property (`.x`), not a keyword.
fn free_identifiers(expr: &str) -> Vec<String> {
    let mut out = Vec::new();
    let bytes = expr.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i] as char;
        if c.is_ascii_alphabetic() || c == '_' || c == '$' {
            let s = i;
            while i < bytes.len() {
                let d = bytes[i] as char;
                if d.is_ascii_alphanumeric() || d == '_' || d == '$' {
                    i += 1;
                } else {
                    break;
                }
            }
            let prev = expr[..s].trim_end().chars().last();
            let word = &expr[s..i];
            if prev != Some('.') && !matches!(word, "this" | "true" | "false" | "null") {
                out.push(word.to_string());
            }
        } else if c.is_ascii_digit() {
            while i < bytes.len() && (bytes[i] as char).is_ascii_alphanumeric() {
                i += 1;
            }
        } else {
            i += 1;
        }
    }
    out
}

#[test]
fn page_fade_call_names_only_declared_identifiers() {
    let src = site("js/play-view-frame.js");
    let frame = body(&src, "_frame(skipDraw)");
    let at = frame
        .find("rt.play_occlusion_fade(")
        .expect("_frame calls play_occlusion_fade");
    let open = at + "rt.play_occlusion_fade".len();
    let args = balanced(frame, open, '(', ')');
    let free = free_identifiers(args);
    assert!(!free.is_empty(), "the eye argument is a local: {args}");
    for id in free {
        let declared = ["const ", "let ", "var "].iter().any(|kw| {
            frame.contains(&format!("{kw}{id} ")) || frame.contains(&format!("{kw}{id}="))
        });
        assert!(
            declared,
            "play_occlusion_fade's argument `{id}` is not declared in _frame: the call \
             throws inside the frame's try and the fade never stages"
        );
    }
}

#[test]
fn see_through_box_writes_and_mirrors_the_engine_option() {
    let html = site("_content/play.html");
    let apply = body(&html, "function applySeeThrough()");
    assert!(
        apply.contains("rt.set_occlusion_fade(!!seeThroughEl.checked)")
            && apply.contains("view.setOcclusionFade(!!seeThroughEl.checked)"),
        "the box writes the engine option and the view's gate"
    );
    let sync = body(&html, "function syncRenderToggles()");
    assert!(
        sync.contains("seeThroughEl.checked = !!rt.occlusion_fade()"),
        "the box mirrors the engine option"
    );
    let after = body(&html, "function syncAfterRuntime(reapply)");
    assert!(
        after.contains("syncRenderToggles()"),
        "a disc load mirrors the option into the box"
    );
    // The view's own gate starts on, so a page that has not synced yet
    // still draws the default.
    let app = site("js/play-app.js");
    assert!(app.contains("this.occlusionFade = true;"));
}

#[test]
fn stored_options_from_before_the_field_read_the_fade_as_on() {
    // The page stores `OptionsState` as JSON (`OPTIONS_STORAGE_KEY`).
    let old: OptionsState =
        serde_json::from_str(r#"{"muted":true,"volumetric_fog":false,"enhanced_lighting":true}"#)
            .expect("an old blob parses");
    assert!(old.occlusion_fade && old.light_shadows && old.muted);
    let mut rt = LegaiaRuntime::new();
    assert!(rt.occlusion_fade(), "a fresh runtime has the fade on");
    rt.set_occlusion_fade(false);
    assert!(!rt.occlusion_fade());
    rt.set_occlusion_fade(true);
    assert!(rt.occlusion_fade());
}

/// Step the page's per-frame fade `n` times from the engine's own follow
/// eye; the last answer.
fn step_fade(rt: &mut LegaiaRuntime, enabled: bool, n: usize) -> Vec<f32> {
    let mut last = Vec::new();
    for _ in 0..n {
        let _ = rt.tick_frame();
        let eye = rt.play_camera_eye();
        if eye.len() != 3 {
            return Vec::new();
        }
        last = rt.play_occlusion_fade(eye[0], eye[1], eye[2], enabled, false, false);
    }
    last
}

#[test]
fn town01_wall_dissolves_on_the_page_and_the_toggle_stands_it_down() {
    let Some(disc) = std::env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    };
    let bytes = std::fs::read(disc).expect("read disc");
    let mut rt = LegaiaRuntime::new();
    rt.load_disc(bytes, String::new()).expect("load disc");
    rt.enter_field("town01").expect("enter town01");
    for _ in 0..30 {
        let _ = rt.tick_frame();
    }
    assert!(rt.occlusion_fade(), "default on");

    // The first even-tile seat a wall hides from the follow camera.
    let mut seat = None;
    'sweep: for tz in (2..64).step_by(2) {
        for tx in (2..64).step_by(2) {
            if !rt.play_debug_seat(tx * 128 + 64, tz * 128 + 64) {
                continue;
            }
            for _ in 0..4 {
                let _ = rt.tick_frame();
            }
            let eye = rt.play_camera_eye();
            if eye.len() == 3 && rt.field_player_occluded(eye[0], eye[1], eye[2]) {
                seat = Some((tx, tz));
                break 'sweep;
            }
        }
    }
    let (tx, tz) = seat.expect("town01 has a seat a wall hides from the camera");
    eprintln!("[occlusion page] hidden seat tile ({tx}, {tz})");
    assert!(rt.play_debug_seat(tx * 128 + 64, tz * 128 + 64));

    let on = rt.occlusion_fade();
    let f = step_fade(&mut rt, on, 40);
    assert_eq!(f.len(), 7, "a hidden player stages a focus");
    assert!(f[0] > 0.5, "the ramp eases up toward full: {}", f[0]);

    // The box off: nothing staged, and the ramp resets.
    assert!(step_fade(&mut rt, false, 2).is_empty());

    // An engine-owned screen over the field stands it down too (the page
    // no longer spells this term itself).
    rt.play_menu_open();
    if rt.play_menu_is_open() {
        let eye = rt.play_camera_eye();
        if eye.len() == 3 {
            let f = rt.play_occlusion_fade(eye[0], eye[1], eye[2], true, false, false);
            assert!(f.is_empty(), "no fade under the pause menu");
        }
        rt.play_menu_close();
    }

    // The option setter is what the box and F4 drive.
    rt.set_occlusion_fade(false);
    let on = rt.occlusion_fade();
    assert!(step_fade(&mut rt, on, 2).is_empty());
}
