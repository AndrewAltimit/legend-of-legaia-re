//! Disc-gated: the **browser** host drives retail's mode chain the same way
//! the native one does.
//!
//! The open thread this answers is "the browser play page holds no mode
//! seat". It does now (`legaia_web_viewer::runtime::LegaiaRuntime` owns a
//! [`ModeSeat`](legaia_engine_core::mode::ModeSeat) and reconciles it once
//! per frame), but holding a seat is not the same as driving it: the native
//! session also **enters** two INIT modes by hand - `MAIN INIT` at field
//! entry and `CARD INIT` on the pause-menu open - and a host that only lets
//! `adopt_world_mode` follow the world skips both INIT frames and the
//! mode-change edge's pad swallow with them.
//!
//! So this drives **both hosts**, in one test, over the same ladder, and
//! compares the mode chains they walk. Transcribing one host's expected
//! sequence into the other's test would have passed while they diverged, so
//! neither side is a literal here: the native trace is the oracle and the
//! browser trace is the subject.
//!
//! The comparison is the **ordered chain of distinct words**, not a
//! frame-by-frame equality. The two hosts do not tick the same frame body
//! (the window drives a renderer, the page drives a draw list), so their
//! per-frame alignment is theirs; who owns the mode chain is the question.
//!
//! **The words alone cannot see an INIT mode, and that is a property of the
//! seat rather than of either host.** `ModeSeat::enter` resolves the INIT
//! column's plan and hands the word to the mode's RUN sibling before it
//! returns, so `MAIN INIT` and `CARD INIT` never appear in a sample taken
//! after the call - on *either* host. What they leave behind is one extra
//! mode-change edge each, so the edge **count** is the witness for the two
//! junctures and is compared beside the chain.
//!
//! Skips + passes when `LEGAIA_DISC_BIN` is unset.

#![allow(clippy::expect_fun_call)]

use legaia_engine_shell::{BootConfig, BootSession};
use legaia_web_viewer::runtime::LegaiaRuntime;

const SCENE: &str = "town01";

/// `(mode index, mode name)` with consecutive duplicates removed - the chain
/// of words the host walked.
fn chain(words: &[(u8, String)]) -> Vec<(u8, String)> {
    let mut out: Vec<(u8, String)> = Vec::new();
    for w in words {
        if out.last() != Some(w) {
            out.push(w.clone());
        }
    }
    out
}

/// The native host's chain: boot, enter the field live, open and close the
/// pause menu, ticking throughout.
fn native_chain(disc: &str) -> (Vec<(u8, String)>, u64) {
    let cfg = BootConfig {
        scene: SCENE.to_string(),
        enable_audio: false,
    };
    let mut s = BootSession::open_disc(std::path::Path::new(disc), &cfg).expect("open disc");
    let mut words = Vec::new();
    let sample = |s: &BootSession, words: &mut Vec<(u8, String)>| {
        let gm = s.mode_seat.game_mode();
        words.push((gm.as_index() as u8, s.mode_seat.mode_name().to_string()));
    };
    sample(&s, &mut words);
    s.enter_field_live(
        SCENE,
        &legaia_engine_shell::boot::FieldLiveOpts {
            live_loop: false,
            player_battle: false,
            ..Default::default()
        },
    )
    .expect("enter field live");
    sample(&s, &mut words);
    for _ in 0..4 {
        let _ = s.tick();
        sample(&s, &mut words);
    }
    s.open_field_menu();
    sample(&s, &mut words);
    for _ in 0..3 {
        let _ = s.tick();
        sample(&s, &mut words);
    }
    s.close_field_menu();
    sample(&s, &mut words);
    for _ in 0..3 {
        let _ = s.tick();
        sample(&s, &mut words);
    }
    (chain(&words), s.mode_seat.edges())
}

/// The browser host's chain over the same ladder, through the exact objects
/// `site/js/play-app.js` constructs and calls.
fn web_chain(disc_bytes: Vec<u8>) -> (Vec<(u8, String)>, u64) {
    let mut rt = LegaiaRuntime::new();
    rt.load_disc(disc_bytes, String::new()).expect("load_disc");
    let mut words = Vec::new();
    let sample = |rt: &LegaiaRuntime, words: &mut Vec<(u8, String)>| {
        let v: serde_json::Value =
            serde_json::from_str(&rt.mode_state_json()).expect("mode_state_json");
        words.push((
            v["word"].as_u64().unwrap_or(0) as u8,
            v["name"].as_str().unwrap_or("").to_string(),
        ));
    };
    sample(&rt, &mut words);
    rt.enter_field(SCENE).expect("enter_field");
    sample(&rt, &mut words);
    for _ in 0..4 {
        let _ = rt.tick_frame();
        sample(&rt, &mut words);
    }
    rt.play_menu_open();
    sample(&rt, &mut words);
    for _ in 0..3 {
        let _ = rt.tick_frame();
        sample(&rt, &mut words);
    }
    rt.play_menu_close();
    sample(&rt, &mut words);
    for _ in 0..3 {
        let _ = rt.tick_frame();
        sample(&rt, &mut words);
    }
    let edges: u64 = serde_json::from_str::<serde_json::Value>(&rt.mode_state_json())
        .ok()
        .and_then(|v| v["edges"].as_u64())
        .expect("mode_state_json carries the edge count");
    (chain(&words), edges)
}

#[test]
fn both_hosts_walk_the_same_mode_chain() {
    let Ok(disc) = std::env::var("LEGAIA_DISC_BIN") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let Ok(bytes) = std::fs::read(&disc) else {
        eprintln!("[skip] disc unreadable (disc-gated)");
        return;
    };
    let (native, native_edges) = native_chain(&disc);
    let (web, web_edges) = web_chain(bytes);
    eprintln!("[mode chain] native {native:?} edges={native_edges}");
    eprintln!("[mode chain] web    {web:?} edges={web_edges}");

    // Non-vacuity: a chain that never left the boot mode would compare equal
    // for the wrong reason.
    assert!(
        native.len() >= 4,
        "the native ladder walked only {native:?} - the ladder did not reach the field"
    );
    assert!(
        native.iter().any(|(_, n)| n.contains("MAIN MODE")),
        "the native ladder never reached the field: {native:?}"
    );
    assert!(
        native.iter().any(|(_, n)| n.contains("CARD MODE")),
        "the native ladder never opened the menu: {native:?}"
    );
    assert_eq!(
        web, native,
        "the browser host walks a different mode chain than the native one"
    );
    // The INIT junctures. Each `ModeSeat::enter` leaves one extra edge
    // behind, so a host that reached `MAIN MODE` / `CARD MODE` by adopting
    // the world's scene mode instead of entering `MAIN INIT` / `CARD INIT`
    // lands here with a lower count while the chain above still matches.
    assert_eq!(
        web_edges, native_edges,
        "the two hosts took a different number of mode-change edges over the same \
         ladder, so one of them reached a mode without entering its INIT sibling"
    );
}
