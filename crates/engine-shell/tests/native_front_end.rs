//! The native play window's `--boot-ui` front end, through the window's own
//! frame path (`HeadlessPlayWindow::open_boot_ui`): the publisher logos hand
//! off to the title through the mode seat, the title's New Game enters the
//! opening scene, and the title's Continue - over a library card mounted in
//! port 2, as `--card` mounts one - loads the card's block 1 into its scene.
//!
//! The front end and the Continue screen are the session's
//! (`legaia_engine_session::FrontEnd`, `ContinueScreen`); the soak harness's
//! `front_end` module drives them on a bare session. These runs check the
//! window's arms act on what they report - the boot UI clears and the
//! session lands where the pick says.
//!
//! Skips + passes when `LEGAIA_DISC_BIN` is unset; the Continue run also
//! needs the save library (`LEGAIA_SAVES_LIBRARY`, else `saves/library` up
//! the tree).

use std::path::{Path, PathBuf};

use legaia_engine_menus::save_select::SelectPhase;
use legaia_engine_shell::window::HeadlessPlayWindow;
use legaia_engine_system::input::PadButton;
use legaia_engine_title::title::TitlePhase;

#[test]
fn boot_ui_logos_title_new_game_enters_the_opening() {
    let Some(disc) = std::env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let mut w = HeadlessPlayWindow::open_boot_ui(Path::new(&disc), "town01", None)
        .expect("open the boot-UI window");
    let fe = w.front_end().expect("--boot-ui opens on the front end");
    assert!(
        fe.logos_session().is_some(),
        "a disc with init.pak cold-boots on the publisher logos"
    );
    let (start, cross) = (PadButton::Start.mask(), PadButton::Cross.mask());
    let mut saw_title = false;
    let mut frames = 0;
    while w.boot_ui_active() && frames < 4000 {
        // One tap every eight frames: Start through the logos and the
        // prompt, Cross on the menu (the cursor opens on NEW GAME, no save
        // being mounted).
        let pad = match w.front_end().and_then(|fe| fe.title_session()) {
            _ if frames % 8 >= 2 => 0,
            Some(t) if matches!(t.phase(), TitlePhase::MainMenu { .. }) => cross,
            _ => start,
        };
        saw_title |= w.front_end().is_some_and(|fe| fe.title_session().is_some());
        w.display_frame(pad, 1);
        frames += 1;
    }
    let scene = w
        .session()
        .host
        .scene
        .as_ref()
        .map(|s| s.name.clone())
        .unwrap_or_default();
    eprintln!("[native-front-end] boot UI cleared after {frames} frames; scene '{scene}'");
    assert!(saw_title, "the logos handed the screen to the title card");
    assert!(!w.boot_ui_active(), "New Game clears the boot UI");
    assert_eq!(
        scene,
        legaia_game_tables::new_game::OPENING_CUTSCENE_SCENE,
        "New Game enters the prologue scene"
    );
}

/// The library card the Continue run mounts: `LEGAIA_SOAK_CARD` (the soak
/// harness's knob), else the playthrough card, under
/// `$LEGAIA_SAVES_LIBRARY/cards` or `saves/library/cards` up the tree.
fn library_card() -> Option<PathBuf> {
    let lib = match std::env::var_os("LEGAIA_SAVES_LIBRARY") {
        Some(p) => PathBuf::from(p),
        None => Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .map(|root| root.join("saves/library"))
            .find(|p| p.is_dir())?,
    };
    let name = std::env::var("LEGAIA_SOAK_CARD")
        .unwrap_or_else(|_| "playthrough-endgame-7saves.mcr".to_string());
    Some(lib.join("cards").join(name)).filter(|p| p.is_file())
}

/// Cold boot -> logos -> title (Continue live off the port-2 card) ->
/// Continue -> port 2's pill -> block 1 -> resumed into the save's own scene
/// with its money, all through the window's own frame path.
#[test]
fn boot_ui_continue_loads_the_port_two_card() {
    let Some(disc) = std::env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let Some(card_path) = library_card() else {
        eprintln!("[skip] no library card (set LEGAIA_SAVES_LIBRARY)");
        return;
    };
    let card = legaia_save::emu::MountedCard::open(&card_path).expect("the library card mounts");
    let (want_save, want_resume) = card
        .save_at(0)
        .expect("block 1 of the library card opens a save");
    let mut w = HeadlessPlayWindow::open_boot_ui(Path::new(&disc), "town01", Some(&card_path))
        .expect("open the boot-UI window");
    let (start, cross, down) = (
        PadButton::Start.mask(),
        PadButton::Cross.mask(),
        PadButton::Down.mask(),
    );
    let mut saw_continue = false;
    let mut frames = 0u32;
    while w.boot_ui_active() && frames < 6000 {
        let tap = frames % 8 < 2;
        let pad = if let Some(c) = w.continue_screen() {
            saw_continue = true;
            match c.session().phase() {
                _ if !tap => 0,
                // Port 2's pill, then into its block grid; block 1 is the
                // grid's first cell, where the cursor opens.
                SelectPhase::Browsing { cursor } if cursor < 1 => down,
                SelectPhase::ConfirmOverwrite { cursor, .. }
                | SelectPhase::ConfirmDelete { cursor, .. }
                    if cursor != 0 =>
                {
                    PadButton::Up.mask()
                }
                _ => cross,
            }
        } else {
            match w.front_end().and_then(|fe| fe.title_session()) {
                _ if !tap => 0,
                Some(t) => match t.phase() {
                    TitlePhase::MainMenu { cursor } if cursor < 1 => down,
                    TitlePhase::MainMenu { .. } => cross,
                    _ => start,
                },
                None => start,
            }
        };
        w.display_frame(pad, 1);
        frames += 1;
    }
    let scene = w
        .session()
        .host
        .scene
        .as_ref()
        .map(|s| s.name.clone())
        .unwrap_or_default();
    eprintln!(
        "[native-front-end] Continue cleared the boot UI after {frames} frames; scene '{scene}' \
         (block 1 names '{}')",
        want_resume.scene
    );
    assert!(saw_continue, "the title's Continue opened the save-select");
    assert!(!w.boot_ui_active(), "the Load clears the boot UI");
    if !want_resume.scene.is_empty() {
        assert_eq!(
            scene, want_resume.scene,
            "the save resumes in its own scene"
        );
    }
    assert_eq!(
        w.session().host.world.party.money,
        want_save.ext.money,
        "the save's money landed"
    );
}
