//! Disc-gated oracle for the browser play page's **field merchant** and the
//! post-action **banner overlays** (`LegaiaRuntime::play_shop_*` /
//! `play_overlay_draws_json`, blitted by `site/js/play-app.js`).
//!
//! Two things are pinned here, and the first is why the second exists.
//!
//! 1. **The shop catalog installs.** `World::try_arm_field_shop` validates a
//!    merchant's op-`0x49` sub-0 record against `item_shop_data`; with no
//!    catalog the validation fails and every merchant is silently inert. The
//!    browser runtime installs the catalog off `SCUS_942.54` at `load_disc`,
//!    the same table the native boot's `read_shop_item_data` reads.
//! 2. **A shop that arms can be closed.** Arming sets `field_shop_open`, which
//!    makes the op-`0x49` tristate report `Armed` and *suspends the field VM*.
//!    If the page could not close the shop the script would park at the first
//!    merchant forever, so the round trip - open, render, exit, VM resumes -
//!    is the contract that matters, not the pixel content.
//!
//! No Sony bytes are asserted, only structural facts. Skips + passes when
//! `LEGAIA_DISC_BIN` is unset.

#![cfg(not(target_arch = "wasm32"))]

use legaia_web_viewer::runtime::LegaiaRuntime;

const DOWN: u16 = 0x0040;
const CROSS: u16 = 0x4000;

fn loaded_in_town() -> Option<LegaiaRuntime> {
    let disc = std::env::var("LEGAIA_DISC_BIN").ok()?;
    let bytes = std::fs::read(&disc).ok()?;
    let mut rt = LegaiaRuntime::new();
    rt.load_disc(bytes, String::new()).ok()?;
    rt.enter_field("town01").ok()?;
    Some(rt)
}

/// The gold-shop catalog must resolve from the disc executable. Without it
/// `try_arm_field_shop` rejects every merchant record and the shop screen is
/// unreachable no matter how well it is wired.
#[test]
fn shop_item_catalog_installs_from_the_disc() {
    let Some(rt) = loaded_in_town() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    assert!(
        rt.debug_has_shop_catalog(),
        "item_shop_data must install from SCUS_942.54 at load_disc - \
         without it every field merchant is inert"
    );
}

/// Open a shop, render it, and leave: the draw list must carry real content
/// while it is up, and closing it must clear `field_shop_open` so the
/// suspended field VM resumes past the merchant op.
#[test]
fn shop_opens_renders_and_releases_the_field_vm() {
    let Some(mut rt) = loaded_in_town() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    // With no shop up the overlay is closed and cheap.
    let idle: serde_json::Value =
        serde_json::from_str(&rt.play_overlay_draws_json(320, 240)).expect("overlay json");
    assert_eq!(idle["open"], false, "no shop, no banner => overlay closed");
    assert!(!rt.play_shop_is_open());

    if !rt.debug_open_test_shop() {
        eprintln!("[skip] no priced merchant record available on this disc build");
        return;
    }
    assert!(rt.play_shop_is_open(), "shop opened");

    // The top picker (Buy / Sell / Exit) must render as real rows, not an
    // empty frame - this is the whole point of routing through
    // `engine-ui::shop_draws_for` rather than a stand-in.
    let open: serde_json::Value =
        serde_json::from_str(&rt.play_overlay_draws_json(320, 240)).expect("overlay json");
    assert_eq!(open["open"], true, "shop overlay is up");
    let texts = open["texts"].as_array().unwrap().len();
    assert!(
        texts > 8,
        "shop panel draws a title + the Buy/Sell/Exit rows (got {texts} glyph quads)"
    );

    // Walk the top picker to **Exit** (Buy / Sell / Exit - the vanilla disc
    // has no Trade row) and confirm.
    rt.play_shop_input(DOWN);
    rt.play_shop_input(DOWN);
    rt.play_shop_input(CROSS);
    // `ShopExit` is a transient state that commits on the FOLLOWING step, so
    // one more tick is needed to drop the session. This is why the page ticks
    // `play_shop_input` every frame while the shop is up rather than only on
    // an edge - gating the tick on a keypress would strand the exit here, with
    // the field VM still suspended.
    rt.play_shop_input(0);
    assert!(!rt.play_shop_is_open(), "Exit closes the shop");
    assert!(
        !rt.debug_field_shop_gate_held(),
        "closing the shop must clear field_shop_open so the suspended \
         op-0x49 flips Armed -> Done and the field VM resumes"
    );
}
