//! Page ladder for the casino **prize counter's Yes/No confirm** - window 46,
//! `FUN_801D603C` (`choice_panel_draws_for`), reach row `801d603c` in
//! `docs/tooling/reach-triage.md`.
//!
//! Both hosts draw the confirm through `ui_prize_exchange::prize_exchange_draws_for`
//! whenever the session's confirm phase is up, so what reaches it is a prize
//! walked past the coin and held-cap gates. The gate is seeded the way the
//! page's cheat panel and a clerk's `49 07 <block>` seed it - coins, then the
//! op's own arm - and the pad does the rest.
//!
//! Coverage export (see `docs/tooling/reach-triage.md`):
//!
//! ```text
//! cargo llvm-cov clean --profraw-only
//! cargo llvm-cov -p legaia-web-viewer --test prize_confirm_page --no-report
//! cargo llvm-cov report --json --output-path target/cov-prize_confirm_page.json
//! ```
//!
//! Skips + passes when `LEGAIA_DISC_BIN` is unset.

#![cfg(not(target_arch = "wasm32"))]

use legaia_engine_core::input::PadButton;
use legaia_web_viewer::runtime::LegaiaRuntime;

const W: u32 = 960;
const H: u32 = 720;

fn overlay(rt: &mut LegaiaRuntime) -> String {
    rt.play_overlay_draws_json(W, H)
}

fn texts(json: &str) -> usize {
    let v: serde_json::Value = serde_json::from_str(json).unwrap_or_default();
    v["texts"].as_array().map_or(0, |a| a.len())
}

fn tick(rt: &mut LegaiaRuntime, n: usize) {
    for _ in 0..n {
        rt.tick_frame().expect("tick_frame");
    }
}

/// Neutral frames through the open menu runtime - the page feeds it one
/// word a frame while it is up, and its fade / slide advance on that clock.
fn settle(rt: &mut LegaiaRuntime) {
    let frames = legaia_engine_core::menu_runtime::SHOP_FADE_FRAMES as usize
        + usize::from(legaia_engine_core::shop::SHOP_SLIDE_FRAMES)
        + 2;
    for _ in 0..frames {
        rt.play_shop_input(0);
    }
}

/// One edge into the open menu runtime, then settle.
fn press(rt: &mut LegaiaRuntime, b: PadButton) {
    rt.play_shop_input(b.mask());
    settle(rt);
}

#[test]
fn prize_confirm_page() {
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
    rt.enter_field("koin1").expect("enter_field(koin1)");
    for _ in 0..10 {
        rt.tick_frame().expect("tick_frame");
    }
    rt.cheat_set_coins(99_999.0);
    assert!(
        rt.debug_arm_prize_exchange(0),
        "koin1's prize block 0 did not arm"
    );
    // The page's drain opens it; it fades in before its windows draw
    // (`shop_fade_level`).
    tick(&mut rt, 1);
    settle(&mut rt);
    assert!(
        rt.play_shop_is_open(),
        "the page did not open the prize counter"
    );
    let list = overlay(&mut rt);
    assert!(texts(&list) > 0, "the prize list drew nothing");

    // Cross on the first row opens the Yes/No confirm: window 46 adds its
    // heading and two rows on top of the list.
    press(&mut rt, PadButton::Cross);
    let confirm = overlay(&mut rt);
    assert!(
        texts(&confirm) > texts(&list),
        "the confirm added no text ({} -> {})",
        texts(&list),
        texts(&confirm)
    );
    // Circle backs out of the confirm onto the list.
    press(&mut rt, PadButton::Circle);
    assert_eq!(
        texts(&overlay(&mut rt)),
        texts(&list),
        "Circle did not take the confirm down"
    );
    eprintln!("prize_confirm_page: cleared");
}
