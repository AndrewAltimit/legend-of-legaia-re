//! Pad ladder for the browser play page's **developer-menu EQUIP row** - the
//! host half of the `dev-menu` reach cluster in
//! `docs/tooling/reach-triage.md` (`dev_equip_commit.rs` `801e5a08`,
//! `world_map_panel.rs` `801ea9b0`).
//!
//! `play_compose_ladder`'s dev-menu rung walks the row list and the Records
//! page but never confirms a row, so neither the row-action dispatcher
//! (retail's *cancel* leg) nor the EQUIP commit was on any ladder. Both are
//! reached here the way a visitor reaches them: the page's opt-in, then pad
//! words through the world's own pump - `LegaiaRuntime::tick_dev_menu` is
//! called from `tick_frame` and there is no page-side key table
//! (`docs/tooling/host-drift.md` tier 5).
//!
//! What the *kernels* do with the bag and the record is asserted disc-free in
//! `crates/engine-core/tests/w1d_world_map_render_ladder.rs`, over the same
//! `DevMenuSession` wrapper both hosts call. This file's job is narrower and
//! is the half that file cannot cover: that a pad stream on a real host
//! reaches them at all.
//!
//! Coverage export:
//!
//! ```text
//! cargo llvm-cov --release -p legaia-web-viewer \
//!     --test w1d_dev_menu_equip_ladder \
//!     --json --output-path target/cov-w1d_dev_menu_equip_ladder.json
//! ```
//!
//! Skips + passes when `LEGAIA_DISC_BIN` is unset.

#![cfg(not(target_arch = "wasm32"))]

use legaia_engine_core::input::PadButton;
use legaia_web_viewer::runtime::LegaiaRuntime;

const W: u32 = 960;
const H: u32 = 720;

/// One press through the world pad: a frame with the bit down, then a frame
/// at neutral, so the engine's `pad & !pad_prev` edge fires once.
fn tap(rt: &mut LegaiaRuntime, mask: u16) {
    rt.set_pad(mask);
    rt.tick_frame().expect("tick_frame");
    rt.set_pad(0);
    rt.tick_frame().expect("tick_frame");
}

fn dev_draws(rt: &mut LegaiaRuntime) -> serde_json::Value {
    serde_json::from_str(&rt.play_dev_menu_draws_json(W, H)).unwrap_or(serde_json::Value::Null)
}

fn text_count(v: &serde_json::Value) -> usize {
    v["texts"].as_array().map(|a| a.len()).unwrap_or(0)
}

#[test]
fn w1d_dev_menu_equip_ladder() {
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
    rt.enter_field("town01").expect("enter_field(town01)");
    for _ in 0..5 {
        rt.tick_frame().expect("tick_frame");
    }

    // The opt-in is the only non-pad step, exactly as the page's own control
    // is the only non-pad step for a visitor.
    assert_eq!(
        text_count(&dev_draws(&mut rt)),
        0,
        "the dev overlay must draw nothing before the opt-in"
    );
    rt.play_dev_menu_set_enabled(true);
    assert!(rt.play_dev_menu_enabled());
    rt.tick_frame().expect("tick_frame");
    let list = dev_draws(&mut rt);
    assert!(list["open"] == true, "dev overlay closed after the opt-in");
    assert!(text_count(&list) > 0, "the row list drew nothing");

    // Walk to the last row (`EQUIP`). The list is five rows, so four Downs
    // land on it from the top whether the picker clamps or wraps; each press
    // has to move the `>` cursor, which is what makes this a walk and not a
    // sequence of no-ops.
    let mut seen = vec![list.to_string()];
    for i in 0..4 {
        tap(&mut rt, PadButton::Down.mask());
        let now = dev_draws(&mut rt).to_string();
        assert!(
            !seen.contains(&now),
            "Down #{i} did not move the dev-menu cursor"
        );
        seen.push(now);
    }

    // Right/Left step the staged item id on the EQUIP row, so the row's value
    // column has to change and change back - the proof the cursor really is
    // on EQUIP rather than on some row that ignores the bits.
    let at_equip = dev_draws(&mut rt).to_string();
    tap(&mut rt, PadButton::Right.mask());
    let stepped = dev_draws(&mut rt).to_string();
    assert_ne!(
        at_equip, stepped,
        "Right did not step the EQUIP row's staged item id"
    );
    tap(&mut rt, PadButton::Left.mask());
    assert_eq!(
        at_equip,
        dev_draws(&mut rt).to_string(),
        "Left did not undo the step"
    );

    // The confirm. With an empty bag the commit's `find_in_bag` guard makes
    // it a no-op, which is the property: a dev commit must never conjure an
    // item the party does not own.
    let before_name = rt.party_display_name(0);
    tap(&mut rt, PadButton::Cross.mask());
    assert!(
        rt.play_dev_menu_enabled(),
        "the EQUIP confirm closed the dev menu"
    );
    assert!(
        text_count(&dev_draws(&mut rt)) > 0,
        "the row list stopped drawing after the confirm"
    );
    assert_eq!(
        rt.party_display_name(0),
        before_name,
        "the confirm disturbed the party record"
    );

    // Cancel: retail's row-action dispatcher runs on this leg, not on the
    // confirm. The screen stays up and the cursor stays where it was.
    let at_cancel = dev_draws(&mut rt).to_string();
    tap(&mut rt, PadButton::Circle.mask());
    assert_eq!(
        at_cancel,
        dev_draws(&mut rt).to_string(),
        "the cancel leg moved the row cursor"
    );

    rt.play_dev_menu_set_enabled(false);
    rt.tick_frame().expect("tick_frame");
    assert_eq!(
        text_count(&dev_draws(&mut rt)),
        0,
        "turning the opt-in off must take the overlay with it"
    );
    eprintln!("w1d_dev_menu_equip_ladder: cleared");
}
