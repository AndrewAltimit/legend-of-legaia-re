//! The title's attract movie ends on retail's abort buttons in the native
//! window. The movie holds the ticks that step the front end's own abort
//! test, so the window's movie drain has to ask for the attract id itself:
//! it asked only the world, which never knows that movie, and the attract
//! played its full ninety seconds whatever the player pressed.

use legaia_engine_shell::window::HeadlessPlayWindow;
use legaia_engine_system::input::PadButton;

#[test]
fn cross_ends_the_attract_movie() {
    let Some(disc) = std::env::var("LEGAIA_DISC_BIN")
        .ok()
        .filter(|d| std::path::Path::new(d).is_file())
    else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let mut w = HeadlessPlayWindow::open_boot_ui(std::path::Path::new(&disc), "town01", None)
        .expect("open");
    let playing = |w: &HeadlessPlayWindow| {
        w.front_end()
            .and_then(|f| f.title_session())
            .is_some_and(|t| t.attract_playing())
    };
    // Start through the logos, then leave the title alone until it times out.
    let mut frame = 0u32;
    while !playing(&w) {
        frame += 1;
        assert!(frame < 40_000, "the title never fired its attract movie");
        let on_title = w.front_end().is_some_and(|f| f.title_session().is_some());
        let pad = if !on_title && frame % 8 < 2 {
            PadButton::Start.mask()
        } else {
            0
        };
        w.ticks(pad, 1);
    }
    for _ in 0..4 {
        w.ticks(0, 1);
    }
    assert!(playing(&w), "the movie ended with nothing pressed");
    w.ticks(PadButton::Cross.mask(), 1);
    for _ in 0..4 {
        w.ticks(0, 1);
    }
    assert!(!playing(&w), "Cross did not end the attract movie");
}
