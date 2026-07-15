//! Disc-gated parity oracle for the browser play page's **pause menu**
//! (`site/play.html`, driven by `LegaiaRuntime::play_menu_*`).
//!
//! The site menu must be the SAME menu the native `legaia-engine play-window`
//! renders: it builds the real [`FieldMenuSubsession`] and blits the draw lists
//! the shared `legaia-engine-ui` builders emit. This test drives the browser
//! API exactly as `site/js/play-app.js` does - open the menu, walk the cursor
//! to each row, press Cross - and asserts every sub-screen renders its full
//! retail content (a substantial glyph list + the gold window chrome), not the
//! single-label generic frame the earlier build fell back to for Items / Magic
//! / Equip.
//!
//! Rendered at a 320x240 surface so the boot-UI stage transform is identity
//! (scale 1, origin 0) and the JSON draw coords are the raw engine-ui output.
//!
//! No Sony bytes are asserted - only structural facts (screens decode, the draw
//! lists are non-trivial). Skips + passes when `LEGAIA_DISC_BIN` is unset.

#![cfg(not(target_arch = "wasm32"))]

use legaia_web_viewer::runtime::LegaiaRuntime;

// PSX pad-edge bit masks (same layout `set_pad` / the page uses).
const UP: u16 = 0x0010;
const DOWN: u16 = 0x0040;
const CROSS: u16 = 0x4000;
const CIRCLE: u16 = 0x2000;

fn loaded_in_town() -> Option<LegaiaRuntime> {
    let disc = std::env::var("LEGAIA_DISC_BIN").ok()?;
    let bytes = std::fs::read(&disc).ok()?;
    let mut rt = LegaiaRuntime::new();
    rt.load_disc(bytes, String::new()).ok()?;
    rt.enter_field("town01").ok()?;
    Some(rt)
}

/// `(sprite_count, text_count)` of the current menu draw list at a 320x240
/// identity stage.
fn draw_counts(rt: &LegaiaRuntime) -> (usize, usize) {
    let json: serde_json::Value =
        serde_json::from_str(&rt.play_menu_draws_json(320, 240)).expect("draws json");
    assert_eq!(json["open"], true, "menu should be open");
    let sprites = json["sprites"].as_array().unwrap().len();
    let texts = json["texts"].as_array().unwrap().len();
    (sprites, texts)
}

/// Open the menu, walk the cursor down `row` steps from the top (Items), and
/// press Cross to open that sub-screen. Assumes a freshly opened menu (cursor
/// at row 0).
fn open_row(rt: &mut LegaiaRuntime, row: usize) {
    for _ in 0..row {
        rt.play_menu_input(DOWN);
    }
    rt.play_menu_input(CROSS);
}

#[test]
fn every_pause_menu_row_renders_full_retail_content() {
    let Some(mut rt) = loaded_in_town() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };

    rt.play_menu_open();
    assert!(rt.play_menu_is_open(), "menu opened");
    // A full disc load resolves the gold chrome atlas; without it the sprite
    // assertions below are vacuous.
    assert!(
        rt.play_menu_has_chrome(),
        "menu chrome atlas must resolve from a full disc"
    );

    // Top level: the command list + money/time box + party info panel.
    let (top_sprites, top_texts) = draw_counts(&rt);
    assert!(top_sprites > 0, "top-level draws window chrome");
    assert!(
        top_texts > 10,
        "top-level draws the 7 command rows + money/time + party panel (got {top_texts})"
    );

    // The three rows that used to fall back to a single centred label. Each now
    // builds its real sub-session and renders the full multi-panel layout, so
    // the glyph list is far richer than a 5-glyph placeholder label.
    //
    // row 0 = Items, 1 = Magic, 2 = Equip.
    for (row, name) in [(0usize, "Items"), (1, "Magic"), (2, "Equip")] {
        // Fresh menu each time so the cursor starts at row 0.
        rt.play_menu_close();
        rt.play_menu_open();
        open_row(&mut rt, row);
        let (sprites, texts) = draw_counts(&rt);
        eprintln!("{name}: sprites={sprites} texts={texts}");
        assert!(sprites > 0, "{name}: sub-screen draws window chrome");
        assert!(
            texts > 8,
            "{name}: sub-screen must render its full retail layout, not a \
             single-label generic frame (got {texts} text draws)"
        );
        // Back out of the sub-screen (Circle at the top phase finishes it).
        rt.play_menu_input(CIRCLE);
    }

    // Status + Options remain fully rendered (regression guard).
    for (row, name) in [(3usize, "Status"), (4, "Options")] {
        rt.play_menu_close();
        rt.play_menu_open();
        open_row(&mut rt, row);
        let (sprites, texts) = draw_counts(&rt);
        eprintln!("{name}: sprites={sprites} texts={texts}");
        assert!(sprites > 0, "{name}: sub-screen draws window chrome");
        assert!(
            texts > 8,
            "{name}: sub-screen renders content (got {texts})"
        );
        rt.play_menu_input(CIRCLE);
    }

    // Load / Save keep the generic frame + label (the page's DOM save-loader
    // owns disc-backed saving); the menu must still open and draw them.
    for (row, name) in [(5usize, "Load"), (6, "Save")] {
        rt.play_menu_close();
        rt.play_menu_open();
        open_row(&mut rt, row);
        let (sprites, texts) = draw_counts(&rt);
        assert!(sprites > 0, "{name}: placeholder frame chrome");
        assert!(texts > 0, "{name}: placeholder label");
        rt.play_menu_input(CIRCLE);
    }

    // Cursor navigation wraps (Up from Items lands on Save).
    rt.play_menu_close();
    rt.play_menu_open();
    rt.play_menu_input(UP);
    rt.play_menu_input(CROSS);
    let (sprites, _) = draw_counts(&rt);
    assert!(sprites > 0, "wrapped cursor opens the Save row");
}
