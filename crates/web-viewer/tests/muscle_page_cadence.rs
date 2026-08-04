//! Disc-free: the browser dome panel's **intermission cadence**, read off
//! `site/js/minigame-muscle.js` itself.
//!
//! The rule is retail's and lives in the engine
//! (`legaia_engine_core::muscle_dome::leg_boundary_raises_interval`, locked by
//! `engine-core/tests/muscle_intermission_cadence.rs`): the INTERVAL +
//! score-tally screen is the arena hub's between-LEGS beat (`FUN_801CF870`
//! state `0x0A`), and a turn boundary never reaches the hub at all - the
//! battle SM writes `ctx[6] = 0x14` and re-enters its own command cluster
//! (`FUN_801E295C`, `0x801E67E8..0x801E6810`).
//!
//! The page is the host that got it wrong, and no Rust-level test could see
//! it: its `finishPlayback` mapped `MusclePhase::TurnOver` straight onto
//! `mode = 'interval'`, so one long fight drew an intermission per turn while
//! the native window - keyed on the leg closing - drew none. That is the
//! host-drift shape `docs/tooling/host-drift.md` describes, so the guard has to
//! read the page source the way the drift gates do.
//!
//! No Sony bytes: this only reads the repo's own JS.

#![cfg(not(target_arch = "wasm32"))]

use std::path::PathBuf;

fn page_source() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("site")
        .join("js")
        .join("minigame-muscle.js");
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// The body of a top-level `function <name>(...) { ... }` in the page, matched
/// by brace balance from its opening `{`.
fn function_body(src: &str, name: &str) -> String {
    let needle = format!("function {name}(");
    let start = src
        .find(&needle)
        .unwrap_or_else(|| panic!("`function {name}(` not found in the dome page"));
    let open = start + src[start..].find('{').expect("function body opens");
    let bytes = src.as_bytes();
    let mut depth = 0usize;
    for (i, &b) in bytes.iter().enumerate().skip(open) {
        match b {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return src[open..=i].to_string();
                }
            }
            _ => {}
        }
    }
    panic!("unbalanced braces in `{name}`");
}

/// Everything up to the first `else if` / `} else` after a `phase === '<p>'`
/// test - i.e. the arm that phase takes.
fn phase_arm(body: &str, phase: &str) -> String {
    let key = format!("phase === '{phase}'");
    let at = body
        .find(&key)
        .unwrap_or_else(|| panic!("no `{key}` arm in finishPlayback"));
    let rest = &body[at..];
    let end = rest.find("} else").unwrap_or(rest.len());
    rest[..end].to_string()
}

#[test]
fn a_settled_turn_returns_to_the_command_cluster_and_raises_no_screen() {
    let src = page_source();
    let body = function_body(&src, "finishPlayback");
    let arm = phase_arm(&body, "turn_over");

    assert!(
        !arm.contains("'interval'"),
        "the turn arm of finishPlayback must not enter the INTERVAL screen - \
         a turn boundary never reaches the arena hub. Arm was:\n{arm}"
    );
    assert!(
        arm.contains("muscle_next_turn"),
        "the turn arm must take the next turn itself (retail writes \
         ctx[6] = 0x14 and carries on). Arm was:\n{arm}"
    );
    assert!(
        arm.contains("mode = 'select'") && arm.contains("selectSub = 'menu'"),
        "the turn arm must land back on the command cluster. Arm was:\n{arm}"
    );
}

#[test]
fn the_interval_screen_is_entered_only_from_a_finished_leg() {
    let src = page_source();
    let body = function_body(&src, "finishPlayback");

    // Every `mode = 'interval'` in the whole page must sit inside the decided
    // (won / lost) arm - the leg boundary - and nowhere else.
    let assignments: Vec<_> = src.match_indices("mode = 'interval'").collect();
    assert_eq!(
        assignments.len(),
        1,
        "exactly one place may enter the INTERVAL screen; found {}",
        assignments.len()
    );
    let decided = phase_arm(&body, "won");
    assert!(
        decided.contains("mode = 'interval'"),
        "the INTERVAL screen is entered from the cleared-leg arm; \
         arm was:\n{decided}"
    );
    assert!(
        decided.contains("step === 'next'"),
        "and only when the ladder has another leg to stage"
    );
}

#[test]
fn the_page_asks_the_engine_whether_the_leg_shows_the_screen() {
    let src = page_source();
    let body = function_body(&src, "reportLeg");
    assert!(
        body.contains("muscle_leg_shows_interval"),
        "the cadence verdict must come from the shared engine rule \
         (leg_boundary_raises_interval), not be re-derived on the page"
    );
}

#[test]
fn leaving_the_interval_screen_stages_the_next_leg_not_the_next_turn() {
    let src = page_source();
    let body = function_body(&src, "confirm");
    let at = body
        .find("mode === 'interval'")
        .expect("the confirm handler has an interval arm");
    let arm = &body[at..];
    let end = arm.find("} else").unwrap_or(arm.len());
    let arm = &arm[..end];
    assert!(
        !arm.contains("muscle_next_turn"),
        "the INTERVAL screen is between LEGS; leaving it stages a leg, \
         never a turn. Arm was:\n{arm}"
    );
    assert!(
        arm.contains("continueRun"),
        "leaving the INTERVAL screen continues the ladder run. Arm was:\n{arm}"
    );
}
