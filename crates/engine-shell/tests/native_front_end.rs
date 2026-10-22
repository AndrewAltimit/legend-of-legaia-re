//! The native play window's `--boot-ui` front end, through the window's own
//! frame path (`HeadlessPlayWindow::open_boot_ui`): the publisher logos hand
//! off to the title through the mode seat, and the title's New Game enters
//! the opening scene.
//!
//! The front end is the session's (`legaia_engine_session::FrontEnd`); the
//! soak harness's `front_end` module drives it on a bare session. This run
//! checks the window's arm acts on what it reports - the boot UI clears and
//! the session lands in the prologue.
//!
//! Skips + passes when `LEGAIA_DISC_BIN` is unset.

use legaia_engine_shell::window::HeadlessPlayWindow;
use legaia_engine_system::input::PadButton;
use legaia_engine_title::title::TitlePhase;

#[test]
fn boot_ui_logos_title_new_game_enters_the_opening() {
    let Some(disc) = std::env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let mut w = HeadlessPlayWindow::open_boot_ui(std::path::Path::new(&disc), "town01")
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
