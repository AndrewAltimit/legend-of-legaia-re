//! Page ladder for the **end of the game**: `edlast`'s credits end on a
//! press-to-continue poll and then `49 0C`, which opens handler slot `0x33`
//! (`FUN_801EDF00`, the return-to-title soft reset). Retail slides the play
//! records in, waits for a face button, fades to white and reloads the
//! executable; the browser play page has to draw the records over the field
//! overlay and then hand off to the title.
//!
//! Before the slot was dispatched it fell through to nothing, the screen
//! closed on its first frame, and the party was left standing in `edlast`
//! with no way out - the soak harness's `softlock|edlast|free-roam` finding
//! (`scripts/replays/soak/fixed/edlast_ending_soft_reset.replay.toml`).
//!
//! The pad is the only actuator: Cross taps through the credits' poll and
//! the records screen, exactly as a player would.
//!
//! Coverage export (see `docs/tooling/reach-triage.md`):
//!
//! ```text
//! cargo llvm-cov clean --profraw-only
//! cargo llvm-cov -p legaia-web-viewer --test soft_reset_records_page --no-report
//! cargo llvm-cov report --json --output-path target/cov-soft_reset_records_page.json
//! ```
//!
//! Skips + passes when `LEGAIA_DISC_BIN` is unset.

#![cfg(not(target_arch = "wasm32"))]

use legaia_engine_core::input::PadButton;
use legaia_web_viewer::runtime::LegaiaRuntime;

const W: u32 = 960;
const H: u32 = 720;
/// The credits reach their poll after roughly 14100 vsyncs
/// (`docs/subsystems/cutscene.md`); the records then slide for 216 game
/// ticks and the reload counts `0x78` more.
const BUDGET: usize = 20_000;

fn field_texts(rt: &mut LegaiaRuntime) -> usize {
    let v: serde_json::Value =
        serde_json::from_str(&rt.play_overlay_draws_json(W, H)).unwrap_or_default();
    v["texts"].as_array().map_or(0, |a| a.len())
}

#[test]
fn soft_reset_records_page() {
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
    rt.enter_field("edlast").expect("enter_field(edlast)");

    let mut peak_texts = 0usize;
    let mut frame = 0usize;
    while frame < BUDGET && !rt.is_game_over() {
        // A tap held for four vsyncs every 64 - long enough for the game-tick
        // sample, sparse enough that the slide finishes between taps.
        let down = frame % 64 < 4;
        rt.set_pad(if down { PadButton::Cross.mask() } else { 0 });
        rt.tick_frame().expect("tick_frame");
        if frame.is_multiple_of(16) {
            peak_texts = peak_texts.max(field_texts(&mut rt));
        }
        frame += 1;
    }
    eprintln!(
        "soft_reset_records_page: hand-off at frame {frame}, peak overlay texts {peak_texts}"
    );
    assert!(
        rt.is_game_over(),
        "the ending never handed off to the title within {BUDGET} frames"
    );
    // The records screen is a few dozen label and number quads; nothing else
    // the field overlay draws in `edlast` comes close.
    assert!(
        peak_texts >= 20,
        "the records screen never drew over the field ({peak_texts} text quads at peak)"
    );
    // The page's own hand-off: the title owns the screen from here.
    let mut picked = String::new();
    for _ in 0..64 {
        picked = rt.game_over_input(0);
        if !picked.is_empty() {
            break;
        }
    }
    assert_eq!(picked, "quit", "the title hand-off did not complete");
}
