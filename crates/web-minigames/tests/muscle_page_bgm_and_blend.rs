//! Disc-free: two dome-page rules read off the site's own JS, where no
//! Rust-level test can see them (the host-drift shape
//! `docs/tooling/host-drift.md` describes).
//!
//! 1. **The battle theme waits for the fight.** Retail's arena init
//!    `FUN_801CEA6C` only loads and opens the theme, after releasing both BGM
//!    sources; the play is `FUN_801D1510`'s `jal 0x80026478` at
//!    `0x801D1548`, the fight start past the ROUND card. The page used to
//!    start it when the tab opened, under the Welcome card. It now starts and
//!    stops it off the renderer's `legMusic`, which reads only the leg modes.
//! 2. **A subtractive hub quad scales with its colour.** Every variant-2 hub
//!    packet is `B - F` with F the white knockout palette times the quad's
//!    Gouraud colour `c / 128`. Drawn at full strength whatever the colour,
//!    the score tally's rows still at level 0 knocked solid black boxes out
//!    of the ringside still. Both pages draw hub quads through the one
//!    `js/canvas-abr.js`.
//!
//! No Sony bytes: this only reads the repo's own JS and HTML.

#![cfg(not(target_arch = "wasm32"))]

use std::path::PathBuf;

fn site(rel: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../site")
        .join(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

#[test]
fn the_dome_theme_is_started_by_the_leg_not_the_tab() {
    let page = site("_content/minigames.html");
    assert!(
        !page.contains("game === 'muscle') MgBgm.start('muscle')"),
        "the tab switch must not start the arena theme"
    );
    let confirm_at = page
        .find("function muscleConfirm()")
        .expect("muscleConfirm");
    let confirm = &page[confirm_at..confirm_at + 400];
    assert!(
        !confirm.contains("MgBgm.start('muscle')"),
        "a confirm press must not start the arena theme"
    );
    let frame_at = page.find("function muscleFrame()").expect("muscleFrame");
    let frame = &page[frame_at..frame_at + 700];
    assert!(
        frame.contains("legMusic"),
        "the frame gates the theme on the leg"
    );
    assert!(frame.contains("MgBgm.stop()"), "and stops it between legs");

    let js = site("js/minigame-muscle.js");
    let at = js.find("legMusic:").expect("renderer exports legMusic");
    let body = &js[at..at + 300];
    for leg_mode in ["'select'", "'playback'"] {
        assert!(body.contains(leg_mode), "legMusic covers {leg_mode}");
    }
    for hub_mode in ["'intro'", "'interval'"] {
        assert!(
            !body.contains(hub_mode),
            "legMusic must not cover {hub_mode}"
        );
    }
}

#[test]
fn a_subtractive_hub_quad_is_scaled_by_its_colour_on_both_pages() {
    let abr = site("js/canvas-abr.js");
    assert!(abr.contains("function withAbr(g, abr, draw, rgb)"));
    assert!(abr.contains("subtractLevel(rgb)"));
    for (js, html) in [
        ("js/minigame-muscle.js", "_content/minigames.html"),
        ("js/play-minigames.js", "_content/play.html"),
    ] {
        let src = site(js);
        assert!(
            src.contains("window.LegaiaCanvasAbr"),
            "{js} draws hub quads through the shared canvas-abr.js"
        );
        assert!(
            !src.contains("function withAbr("),
            "{js} keeps no private copy of the blend"
        );
        let page = site(html);
        let abr_at = page
            .find("js/canvas-abr.js")
            .expect("page loads canvas-abr.js");
        let own_at = page
            .find(&format!("\"{js}\""))
            .expect("page loads its script");
        assert!(abr_at < own_at, "{html} loads canvas-abr.js first");
    }
}
