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
const RIGHT: u16 = 0x0020;
const DOWN: u16 = 0x0040;
const LEFT: u16 = 0x0080;
const CROSS: u16 = 0x4000;
const CIRCLE: u16 = 0x2000;

/// Row indices into the retail command list (Items / Magic / Equip / Status /
/// Options / Load / Save).
const ROW_OPTIONS: usize = 4;
const ROW_LOAD: usize = 5;
const ROW_SAVE: usize = 6;

fn loaded_in_town() -> Option<LegaiaRuntime> {
    loaded_in("town01")
}

/// The same, in an arbitrary scene. The **Save** row is scene-gated on both
/// hosts (`World::scene_save_allowed`, seeded from the MAN header bit retail
/// copies into `_DAT_8007B6A8`), and across the disc that bit is set only on
/// the three kingdom world maps - so a test that needs to reach the save
/// screen through the menu has to stand in one of them, exactly as a player
/// does. See `docs/subsystems/save-screen.md`.
fn loaded_in(scene: &str) -> Option<LegaiaRuntime> {
    let disc = std::env::var("LEGAIA_DISC_BIN").ok()?;
    let bytes = std::fs::read(&disc).ok()?;
    let mut rt = LegaiaRuntime::new();
    rt.load_disc(bytes, String::new()).ok()?;
    rt.enter_field(scene).ok()?;
    Some(rt)
}

/// A kingdom world map - the only scene family whose MAN permits a menu save.
const SAVE_ALLOWED_SCENE: &str = "map01";

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

    // Load / Save render the real retail save-select screen against the
    // memory-card rack - the panel + SLOT pills, not a generic frame.
    //
    // Save has to be measured in a scene whose MAN permits it: this test's
    // `town01` is a field scene, so retail (and now this host) buzzes that
    // row. Measuring it here would pass on the ROOT list's own draws, which
    // clear both thresholds - a vacuous assertion dressed as a screen check.
    // `the_save_row_is_greyed_and_buzzes_in_a_field_scene` owns the gate.
    let mut save_rt = loaded_in(SAVE_ALLOWED_SCENE);
    for (row, name) in [(ROW_LOAD, "Load"), (ROW_SAVE, "Save")] {
        let host = if row == ROW_SAVE {
            match save_rt.as_mut() {
                Some(r) => r,
                None => continue,
            }
        } else {
            &mut rt
        };
        host.play_menu_close();
        host.play_menu_open();
        open_row(host, row);
        let (sprites, texts) = draw_counts(host);
        eprintln!("{name}: sprites={sprites} texts={texts}");
        // The retail panel alone is 14 9-slice tiles + interior; the old
        // placeholder frame was a bare box.
        assert!(
            sprites > 14,
            "{name}: must draw the retail 9-slice Load panel + the SLOT pills \
             (got {sprites} sprites)"
        );
        // The panel title is drawn per-glyph from the dialog font.
        assert!(texts >= 4, "{name}: panel title glyphs (got {texts})");
        host.play_menu_input(CIRCLE);
    }

    // Cursor navigation wraps (Up from Items lands on Save) - measured where
    // Save opens, for the same reason.
    if let Some(r) = save_rt.as_mut() {
        r.play_menu_close();
        r.play_menu_open();
        r.play_menu_input(UP);
        r.play_menu_input(CROSS);
        let (sprites, _) = draw_counts(r);
        assert!(sprites > 14, "wrapped cursor opens the Save row");
    }
}

/// The live session's gold, read back through the public save round-trip
/// (the runtime's world is crate-private).
fn session_money(rt: &mut LegaiaRuntime) -> i32 {
    let bytes = rt.export_save();
    legaia_save::SaveFile::parse(&bytes)
        .expect("session exports")
        .ext
        .money
}

/// A raw 128 KiB memory card carrying one Legaia save in `block`, lead named
/// `name`. Synthesised, not disc-derived - no Sony bytes.
fn card_with_save(block: u8, name: &str, gold: i32) -> Vec<u8> {
    use legaia_save::card;
    let mut buf = vec![0u8; card::CARD_SIZE];
    buf[..2].copy_from_slice(&card::CARD_MAGIC);
    for i in 1..=card::DIR_FRAMES {
        let off = card::DIR_FRAME_SIZE * i;
        buf[off..off + 4].copy_from_slice(&card::state::FREE.to_le_bytes());
    }
    let f = card::DIR_FRAME_SIZE * block as usize;
    buf[f..f + 4].copy_from_slice(&card::state::FIRST_BLOCK.to_le_bytes());
    buf[f + 8..f + 10].copy_from_slice(&0xFFFFu16.to_le_bytes());
    buf[f + 10..f + 22].copy_from_slice(b"BASCUS-94254");
    let b = card::BLOCK_SIZE * block as usize;
    let sc = &mut buf[b..b + card::BLOCK_SIZE];
    sc[..2].copy_from_slice(&card::SAVE_BLOCK_MAGIC);
    let mut rec = legaia_save::CharacterRecord::zeroed();
    rec.set_name(name);
    rec.set_magic_rank(12);
    rec.set_hp_mp_sp(legaia_save::HpMpSp {
        hp_cur: 180,
        hp_max: 200,
        mp_cur: 20,
        mp_max: 30,
        sp_cur: 0,
        sp_max: 0,
    });
    legaia_save::write_retail_char_records(sc, std::slice::from_ref(&rec.raw)).unwrap();
    legaia_save::write_retail_gold(sc, gold).unwrap();
    // A real save carries the CDNAME label of the scene it was written in
    // (game+0x208) and the location's display name (game+0x000) - the fields
    // the load screen's info panel and the resume-into-scene path read.
    let scene = b"town01";
    let at = card::RETAIL_SCENE_LABEL_OFFSET;
    sc[at..at + scene.len()].copy_from_slice(scene);
    let loc = b"Rim Elm";
    let at = card::RETAIL_LOCATION_NAME_OFFSET;
    sc[at..at + loc.len()].copy_from_slice(loc);
    buf
}

/// Drive the retail card flow: pill row -> "Now checking" -> the card's 5x3
/// block grid -> confirm. Asserts each phase renders its own retail furniture
/// (so a regression back to a static frame fails here), and that a Load
/// actually lifts the card's save into the live world.
#[test]
fn load_screen_walks_the_retail_card_flow_off_an_inserted_card() {
    let Some(mut rt) = loaded_in_town() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    rt.insert_card(0, card_with_save(3, "Vahn", 1234), "card A".into())
        .expect("insert card into port 1");

    rt.play_menu_open();
    // The atlas is built lazily on the first open; without it every sprite
    // assertion below is vacuous.
    assert!(rt.play_menu_has_chrome(), "chrome atlas must resolve");
    open_row(&mut rt, ROW_LOAD);
    let (browsing_sprites, _) = draw_counts(&rt);

    // X on SLOT 1 reads the card: the "Now checking. Do not remove MEMORY
    // CARD" dialog is a panel + two centred text lines on top of the pills.
    rt.play_menu_input(CROSS);
    let (checking_sprites, checking_texts) = draw_counts(&rt);
    assert!(
        checking_texts > 20,
        "NowChecking draws both message lines per-glyph (got {checking_texts})"
    );
    assert!(
        checking_sprites > browsing_sprites,
        "NowChecking adds its dialog panel over the pill row \
         ({checking_sprites} vs browsing {browsing_sprites})"
    );

    // The dialog holds for its retail beat, then the grid comes up.
    for _ in 0..200 {
        rt.play_menu_input(0);
    }
    let (grid_sprites, empty_cell_texts) = draw_counts(&rt);
    // 15 empty-cell frames + the grid cursor + the info panel's 9-slice -
    // comfortably past the dialog's sprite count.
    assert!(
        grid_sprites > 15,
        "SlotPreview draws the 5x3 block grid + info panel (got {grid_sprites})"
    );
    // The cursor starts on cell 0 = block 1, which is free on this card.
    // Retail does not leave that panel blank: a free block gets a centred
    // caption ("No data" on the Load path). Title-only would be ~4 glyphs,
    // so anything at/above this floor means the caption rendered. That the
    // panel prints the caption and NOT the save's stat rows is pinned by the
    // comparison against the occupied cell further down.
    assert!(
        empty_cell_texts >= 8,
        "a free block must caption the info panel, not leave it blank \
         (got {empty_cell_texts})"
    );

    // Confirming an EMPTY block must do nothing: the session only knows the
    // phase, so without a host gate this reports Loaded and then fails to
    // parse a block that holds no save - closing the screen for nothing.
    rt.play_menu_input(CROSS);
    let (_, still_texts) = draw_counts(&rt);
    assert_eq!(
        still_texts, empty_cell_texts,
        "X on an empty block must leave the preview up, not load"
    );
    assert!(
        rt.play_menu_take_load_scene().is_empty(),
        "an empty block must not load anything"
    );

    // Walk the cursor onto the real save in block 3 (= cell 2): the info
    // panel is bound to the focused cell, so its rows appear now.
    let before = rt.play_menu_draws_json(320, 240);
    rt.play_menu_input(RIGHT);
    rt.play_menu_input(RIGHT);
    let after = rt.play_menu_draws_json(320, 240);
    assert_ne!(
        before, after,
        "moving the grid cursor must re-draw (cursor + the focused block's info)"
    );
    let (_, filled_texts) = draw_counts(&rt);
    assert!(
        filled_texts > empty_cell_texts + 10,
        "focusing the card's save must print its name / LV / HP / MP / time \
         (got {filled_texts} vs {empty_cell_texts} on an empty block)"
    );

    // Confirm cell 2 = block 3: the card's save lands in the live world.
    rt.play_menu_input(CROSS);
    assert_eq!(
        session_money(&mut rt),
        1234,
        "the card's save is now the live session"
    );
    assert!(
        !rt.play_menu_take_load_scene().is_empty(),
        "a card load reports the scene the save was written in"
    );
}

/// Saving writes the live session into the inserted card, and the card
/// exports as a container that still parses - the round-trip the player
/// needs to resume in their emulator.
#[test]
fn save_screen_writes_the_session_into_the_inserted_card() {
    let Some(mut rt) = loaded_in(SAVE_ALLOWED_SCENE) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    assert!(
        rt.play_scene_save_allowed(),
        "{SAVE_ALLOWED_SCENE}'s MAN sets the save-allow bit - without it the \
         row buzzes and the rest of this test walks an unopened screen"
    );
    let original = card_with_save(3, "Vahn", 1234);
    rt.insert_card(0, original.clone(), "card A".into())
        .expect("insert");

    rt.play_menu_open();
    open_row(&mut rt, ROW_SAVE);
    rt.play_menu_input(CROSS); // pick SLOT 1
    for _ in 0..200 {
        rt.play_menu_input(0); // "Now checking" beat
    }
    // Walk to cell 2 (= block 3, the existing save) and confirm.
    rt.play_menu_input(RIGHT);
    rt.play_menu_input(RIGHT);
    rt.play_menu_input(CROSS);
    // The overwrite prompt defaults to "No" - a stray confirm must not write.
    assert!(
        !rt.card_slot_dirty(0),
        "the prompt must not write on its own"
    );
    rt.play_menu_input(LEFT); // -> Yes
    rt.play_menu_input(CROSS);
    assert!(rt.card_slot_dirty(0), "confirming Yes writes into the card");

    // The exported container is still a card an emulator can walk, with the
    // save where we put it.
    let exported = rt.export_card(0);
    assert_eq!(exported.len(), original.len(), "container shape preserved");
    let saves = legaia_save::card::parse_card(&exported).expect("card still parses");
    assert!(
        saves.iter().any(|s| s.block == 3),
        "block 3 still holds a save"
    );
    // Only block 3 may have moved.
    let b3 = legaia_save::card::BLOCK_SIZE * 3;
    let escaped: Vec<usize> = original
        .iter()
        .zip(exported.iter())
        .enumerate()
        .filter(|(_, (a, b))| a != b)
        .map(|(i, _)| i)
        .filter(|i| !(b3..b3 + legaia_save::card::BLOCK_SIZE).contains(i))
        .collect();
    assert!(escaped.is_empty(), "writes escaped block 3: {escaped:?}");
}

/// With no card in either port the Load screen still renders retail's panel +
/// pills, and confirming an empty port blips rather than loading anything.
#[test]
fn load_screen_with_no_card_inserted_renders_and_refuses() {
    let Some(mut rt) = loaded_in_town() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    rt.play_menu_open();
    open_row(&mut rt, ROW_LOAD);
    let (sprites, _) = draw_counts(&rt);
    assert!(sprites > 14, "the panel + pills draw with no card inserted");
    rt.play_menu_input(CROSS);
    let json: serde_json::Value = serde_json::from_str(&rt.play_menu_draws_json(320, 240)).unwrap();
    assert_eq!(json["open"], true, "an empty port must not close the menu");
    assert!(
        rt.play_menu_take_load_scene().is_empty(),
        "nothing was loaded"
    );
}

/// The Tactical Arts chain editor is an **engine extension** with no retail
/// pause-menu row, reached by Triangle on the Status screen
/// (`field_menu_dispatch::try_open_arts_editor`). This asserts the browser
/// page actually reaches it - the gap the UI host-drift waiver used to cover -
/// and that the editor has live state behind it rather than a static frame.
#[test]
fn triangle_on_status_opens_the_arts_editor_on_the_play_page() {
    const TRIANGLE: u16 = 0x1000;
    const ROW_STATUS: usize = 3;

    let Some(mut rt) = loaded_in_town() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };

    rt.play_menu_open();
    open_row(&mut rt, ROW_STATUS);
    let (status_sprites, status_texts) = draw_counts(&rt);
    assert!(
        status_texts > 8,
        "Status must render its retail layout first (got {status_texts})"
    );

    // Triangle swaps the Status sub-session for the chain editor.
    rt.play_menu_input(TRIANGLE);
    assert!(
        rt.play_menu_is_open(),
        "the menu stays open across the swap"
    );
    let (arts_sprites, arts_texts) = draw_counts(&rt);
    eprintln!("arts editor: sprites={arts_sprites} texts={arts_texts}");
    assert!(
        (arts_sprites, arts_texts) != (status_sprites, status_texts),
        "Triangle must change the screen - identical draw lists mean the \
         editor never opened"
    );
    // Browse phase is "ARTS - <name>", the "+ New" row and the footer hint.
    assert!(
        arts_texts > 20,
        "the browse screen draws its header, rows and footer (got {arts_texts})"
    );

    // The editor is stateful, not a static frame: Cross on "+ New" enters the
    // Editing phase, and each direction appends to the working sequence.
    rt.play_menu_input(CROSS);
    let (_, editing_texts) = draw_counts(&rt);
    assert_ne!(
        editing_texts, arts_texts,
        "Cross on + New must enter the Editing phase"
    );

    // Appends below the 3-input minimum only grow the printed sequence...
    let mut prev = editing_texts;
    for dir in [LEFT, RIGHT] {
        rt.play_menu_input(dir);
        let (_, texts) = draw_counts(&rt);
        assert!(
            texts > prev,
            "appending below min length lengthens the sequence ({prev} -> {texts})"
        );
        prev = texts;
    }
    // ...and the append that REACHES the minimum shrinks the list, because the
    // "(need 3+ inputs)" tail is dropped from the Cross hint once the chain is
    // long enough to save. That drop is the min-length rule made visible.
    rt.play_menu_input(DOWN);
    let (_, at_min_texts) = draw_counts(&rt);
    assert!(
        at_min_texts < prev,
        "reaching min length drops the 'need 3+ inputs' hint ({prev} -> \
         {at_min_texts})"
    );

    // Circle backs out of the editor rather than leaving the page stranded.
    rt.play_menu_input(CIRCLE);
    assert!(rt.play_menu_is_open(), "backing out keeps the menu up");
}

/// The Options screen must **remember** an edit.
///
/// Drift the UI host-drift gate cannot see: both hosts call the same
/// `options_draws_for` builder, but the browser host built every Options
/// sub-session from `OptionsState::default()` and dropped the edited state on
/// close, so the screen was a working control panel wired to nothing - open it
/// twice and the second open had forgotten the first. The native window keeps
/// the state on `PlayWindowApp` (and persists it to `legaia-options.toml`).
///
/// The assertion is the round trip, not a particular row: change a value
/// through the session's own popup, close, reopen, and require the reopened
/// session to be seeded from the runtime's state rather than defaults.
#[test]
fn the_options_screen_remembers_an_edit_across_opens() {
    let Some(mut rt) = loaded_in_town() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let default_state =
        serde_json::to_string(&legaia_engine_core::options::OptionsState::default()).unwrap();

    rt.play_menu_open();
    open_row(&mut rt, ROW_OPTIONS);
    assert_eq!(
        rt.debug_open_options_json().as_deref(),
        Some(default_state.as_str()),
        "a fresh session starts at the defaults"
    );

    // Cross opens the value popup on the cursor row, Down steps the choice,
    // Cross commits it into the session state (retail writes the config word
    // inside the popup and never reverts).
    rt.play_menu_input(CROSS);
    rt.play_menu_input(DOWN);
    rt.play_menu_input(CROSS);
    let edited = rt
        .debug_open_options_json()
        .expect("options session still open");
    assert_ne!(
        edited, default_state,
        "committing a popup choice must change the session state - \
         otherwise the rest of this test proves nothing"
    );

    // Circle closes the screen and drops back to the command list.
    rt.play_menu_input(CIRCLE);
    assert!(
        rt.debug_open_options_json().is_none(),
        "Circle closes the Options sub-screen"
    );
    assert_eq!(
        rt.debug_options_json(),
        edited,
        "the closing session's state must be lifted onto the runtime"
    );

    // Reopen: the seed must be the lifted state, not a fresh default. The
    // close already parked the cursor back on the row that opened the screen,
    // so Cross alone re-enters it.
    rt.play_menu_input(CROSS);
    assert_eq!(
        rt.debug_open_options_json().as_deref(),
        Some(edited.as_str()),
        "reopening Options must seed from the runtime's state"
    );
}

/// The Save row is **scene-gated** on the browser host too.
///
/// Retail reads the per-scene save-allow byte (`_DAT_8007B6A8`, seeded from
/// the MAN header bit) at every draw of the root list and again on confirm,
/// so a field scene's Save row draws grey and buzzes. The browser page used
/// to build its own root list with `enabled: true` on every row and route
/// confirms off its own cursor, which meant a player could save in Rim Elm -
/// a scene whose own data forbids it. Both halves come off one
/// `FieldMenuGate` now, so the ink and the confirm cannot disagree.
#[test]
fn the_save_row_is_greyed_and_buzzes_in_a_field_scene() {
    let Some(mut rt) = loaded_in_town() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    assert!(
        !rt.play_scene_save_allowed(),
        "town01 is a field scene; its MAN clears the save-allow bit"
    );

    // The ink: opening the row must be refused outright.
    assert!(!rt.play_menu_open_row("Save"), "a gated row does not open");
    // And Load, beside it, must still open - or the gate is a blanket refusal
    // rather than retail's per-row routing.
    assert!(rt.play_menu_open_row("Load"), "Load is not gated here");
    rt.play_menu_close();

    // The confirm: walking the cursor onto Save and pressing Cross buzzes and
    // stays on the picker, so the menu is still showing the root list.
    rt.play_menu_open();
    open_row(&mut rt, ROW_SAVE);
    assert!(rt.play_menu_is_open(), "a buzz does not close the menu");
    let json: serde_json::Value =
        serde_json::from_str(&rt.play_menu_draws_json(320, 240)).expect("draws json");
    assert_eq!(json["open"], true);
    // The root list draws the money/time box; a save screen does not.
    let top = draw_counts(&rt);
    rt.play_menu_close();
    rt.play_menu_open();
    assert_eq!(
        draw_counts(&rt),
        top,
        "the buzzed confirm left the root list exactly as it was"
    );
}
