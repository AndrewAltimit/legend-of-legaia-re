//! Disc-gated oracle for the browser play page's **New Game flow** - the boot
//! title's menu rows and the opening `town01` name-entry overlay
//! (`LegaiaRuntime::boot_title_*` / `name_entry_*`, blitted by
//! `site/js/play-app.js` and `site/_content/play.html`).
//!
//! Both screens are shared `engine-ui` builders, so what needs pinning is not
//! their pixels but that the browser host actually *reaches* them, with the
//! state that makes them mean something:
//!
//! 1. **The title menu has a source in both configurations.** With the disc's
//!    title art present the TIM's own NEW GAME / CONTINUE bands carry the rows;
//!    without it they fall back to `title_menu_draws_for` sampling the
//!    menu-glyph atlas, exactly as the native window falls back. Exactly one of
//!    the two draws at a time - both would double-render the rows.
//! 2. **Name entry lands with its state.** The overlay is opened by the
//!    establishing timeline's pinned op-`0x49`, which *suspends the field VM*
//!    until the SM commits. So the contract is the round trip - opens, renders,
//!    commits, the name reaches the party record, the script resumes - not the
//!    glyph layout. A cosmetic overlay here would park the opening forever, the
//!    same failure an unclosable shop produces.
//!
//! The retail behaviours the flow is pinned against (middle control = restore
//! default, confirm prompt opens on **No**) are asserted through the engine SM
//! the page drives, not re-derived here.
//!
//! No Sony bytes are asserted, only structural facts. Skips + passes when
//! `LEGAIA_DISC_BIN` is unset.

#![cfg(not(target_arch = "wasm32"))]

use legaia_web_viewer::runtime::LegaiaRuntime;

const UP: u16 = 0x0010;
const CROSS: u16 = 0x4000;

fn loaded() -> Option<LegaiaRuntime> {
    let disc = std::env::var("LEGAIA_DISC_BIN").ok()?;
    let bytes = std::fs::read(&disc).ok()?;
    let mut rt = LegaiaRuntime::new();
    rt.load_disc(bytes, String::new()).ok()?;
    Some(rt)
}

/// Drive the title session to its main menu (skip fade-in, Start, then read).
fn to_main_menu(rt: &mut LegaiaRuntime) {
    rt.boot_title_start();
    // Start (0x0008) leaves PressStart for MainMenu.
    rt.boot_title_step(0x0008);
}

#[test]
fn title_menu_rows_have_exactly_one_source() {
    let Some(mut rt) = loaded() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    to_main_menu(&mut rt);

    let art: serde_json::Value =
        serde_json::from_str(&rt.boot_title_draws_json(960, 720)).expect("title draws json");
    assert_eq!(art["active"], true, "title session must be running");
    assert!(
        !art["sprites"].as_array().unwrap().is_empty(),
        "with the disc's title art the TIM bands carry the card + menu rows"
    );
    assert!(
        art["glyphs"].as_array().unwrap().is_empty(),
        "the menu-glyph fallback must stay silent while the title art draws \
         the rows - emitting both double-renders NEW GAME / CONTINUE"
    );

    // The fallback the native window uses when the title art is absent. The
    // page must have the same second source, or a PROT-only load shows a menu
    // with no rows at all.
    assert!(
        rt.boot_title_glyph_atlas_dims()[0] > 0,
        "menu-glyph atlas must resolve from PROT.DAT - it is the no-title-art \
         source for the menu rows"
    );
    rt.debug_drop_title_atlas();
    let fallback: serde_json::Value =
        serde_json::from_str(&rt.boot_title_draws_json(960, 720)).expect("fallback draws json");
    assert!(
        !fallback["glyphs"].as_array().unwrap().is_empty(),
        "without title art the menu rows must come from title_menu_draws_for"
    );
}

#[test]
fn name_entry_opens_renders_commits_and_releases_the_field_vm() {
    let Some(mut rt) = loaded() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    // The opening `town01` entry (not a casual visit) is what installs the
    // establishing timeline whose op-`0x49` opens the overlay.
    rt.debug_enter_town01_opening()
        .expect("enter town01 as the new-game opening");

    let mut ticks = 0;
    while !rt.name_entry_is_active() && ticks < 8000 {
        rt.tick_frame().expect("tick");
        ticks += 1;
    }
    assert!(
        rt.name_entry_is_active(),
        "the town01 opening timeline must reach its pinned op-0x49 and open \
         name entry ({ticks} ticks)"
    );

    // The overlay renders through the shared builders, both layers.
    let draws: serde_json::Value =
        serde_json::from_str(&rt.name_entry_draws_json(960, 720)).expect("name entry draws json");
    assert_eq!(draws["open"], true);
    assert!(
        !draws["texts"].as_array().unwrap().is_empty(),
        "name_entry_draws_for must emit the grid / control-bar text"
    );
    assert!(
        !draws["sprites"].as_array().unwrap().is_empty(),
        "name_entry_chrome_sprite_draws_for must emit the two windows + cursor"
    );

    // Retail state, read through the page's own probe: the cursor opens on
    // Select, so a bare confirm keeps the template default.
    let state: serde_json::Value =
        serde_json::from_str(&rt.name_entry_state_json()).expect("name entry state json");
    assert_eq!(state["control"], 2, "cursor opens on Select (retail 0x74)");
    let default_name = state["default"].as_str().unwrap().to_string();
    assert!(!default_name.is_empty(), "template default name is seeded");

    // Select -> the confirm prompt, which retail opens on **No**.
    rt.name_entry_input(CROSS);
    let confirming: serde_json::Value =
        serde_json::from_str(&rt.name_entry_state_json()).expect("confirm state json");
    assert_eq!(confirming["confirming"], true, "Select opens the confirm");
    assert_eq!(
        confirming["confirm_yes"], false,
        "the confirm prompt opens on No, not Yes"
    );

    // No -> Yes -> commit.
    rt.name_entry_input(UP);
    let committed = rt.name_entry_input(CROSS);
    assert!(committed, "confirming Yes commits the name");
    assert!(!rt.name_entry_is_active(), "the overlay closes on commit");
    assert_eq!(
        rt.party_display_name(0),
        default_name,
        "the committed name lands in the live party record - the screen is not \
         a cosmetic overlay"
    );

    // The suspended script resumes: the op-0x49 gate flips Armed -> Done, so
    // the world keeps ticking past the naming beat.
    let before = rt.debug_world_frame();
    for _ in 0..120 {
        rt.tick_frame().expect("tick after commit");
    }
    assert!(
        rt.debug_world_frame() > before,
        "the field VM must resume once the name commits"
    );

    // ...and the timeline must eventually hand the controls back. This is the
    // other half of "the screen lands with its state": a naming prompt that
    // commits into a timeline which never ends is still a dead page.
    let mut ticks = 0;
    while rt.debug_timeline_active() && ticks < 12000 {
        rt.tick_frame().expect("tick out of the opening timeline");
        ticks += 1;
    }
    assert!(
        !rt.debug_timeline_active(),
        "the town01 opening timeline must finish after the naming beat, \
         returning control to the player ({ticks} ticks)"
    );
}
