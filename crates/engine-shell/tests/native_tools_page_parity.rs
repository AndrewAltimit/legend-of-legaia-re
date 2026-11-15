//! Disc-gated: what the browser play page offers beside its canvas while a
//! game runs, the window offers from its `F12` panel.
//!
//! The page's *Export session* button writes the running game as an `.lgsf`
//! at any moment (`export_save`), its *Copy report* button builds a bug
//! report from the running session, and its *Reset keys and buttons* button
//! puts the default bindings back. The window's twins are two Tools rows and
//! the Keys page's reset row; this drives each through the headless window.
//!
//! Skips + passes when `LEGAIA_DISC_BIN` is unset.

use legaia_engine_shell::window::HeadlessPlayWindow;
use legaia_engine_system::input::Mapping;
use legaia_web_play::runtime::LegaiaRuntime;

fn disc() -> Option<String> {
    let d = std::env::var("LEGAIA_DISC_BIN")
        .ok()
        .filter(|d| std::path::Path::new(d).is_file());
    if d.is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
    }
    d
}

/// The window's export is the page's: the same resume point and the same
/// party, in a file the page imports.
#[test]
fn the_window_exports_the_session_the_page_exports() {
    let Some(disc) = disc() else { return };
    let dir = tempfile::tempdir().expect("tempdir");
    let saves = dir.path().join("saves");
    let mut w =
        HeadlessPlayWindow::open_in(std::path::Path::new(&disc), "town01", &saves).expect("open");
    w.display_frame(0, 30);
    w.session_mut().host.world.party.money = 4321;

    let path = w.export_session().expect("export");
    assert_eq!(path.parent(), Some(saves.as_path()));
    assert_eq!(path.file_name().unwrap(), "session_town01_001.lgsf");
    let bytes = std::fs::read(&path).expect("read");
    let (sf, resume) = legaia_save::SaveFile::parse_with_resume(&bytes).expect("parse");
    assert_eq!(resume.scene, "town01");
    assert_eq!(sf.ext.money, 4321);
    // A second export is a second file.
    let again = w.export_session().expect("export");
    assert_eq!(again.file_name().unwrap(), "session_town01_002.lgsf");

    // The page, in the same scene after the same ticks, writes the same
    // resume point; and it imports the window's file.
    let mut rt = LegaiaRuntime::new();
    rt.load_disc(std::fs::read(&disc).expect("read disc"), String::new())
        .expect("load_disc");
    rt.enter_field("town01").expect("enter_field");
    rt.frame_ticks(30, 0, 0).expect("ticks");
    let page = rt.export_save_core().expect("page export");
    let (_, page_resume) = legaia_save::SaveFile::parse_with_resume(&page).expect("parse");
    assert_eq!(page_resume, resume, "one resume derivation on both hosts");
    let summary = rt.import_save(bytes).expect("the page imports the file");
    assert!(summary.contains("\"scene\":\"town01\""), "{summary}");
}

/// A fight that is committed and has not begun refuses the export on both
/// hosts (`World::host_save_allowed`).
#[test]
fn the_export_is_refused_where_the_page_refuses_it() {
    let Some(disc) = disc() else { return };
    let dir = tempfile::tempdir().expect("tempdir");
    let mut w = HeadlessPlayWindow::open_in(std::path::Path::new(&disc), "town01", dir.path())
        .expect("open");
    w.display_frame(0, 4);
    assert!(w.session().host.world.host_save_allowed());
    assert!(w.export_session().is_ok());
    let made = std::fs::read_dir(dir.path()).unwrap().count();
    w.session_mut().host.world.carriers.pending_battle = Some(21);
    assert!(!w.session().host.world.host_save_allowed());
    assert!(w.export_session().is_err());
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), made);
}

/// The report names the session and carries none of the player's paths.
#[test]
fn the_report_names_the_running_session() {
    let Some(disc) = disc() else { return };
    let mut w = HeadlessPlayWindow::open(std::path::Path::new(&disc), "town01").expect("open");
    w.display_frame(0, 12);
    let report = w.session_report();
    let frame = w.session().host.world.frame;
    assert!(report.starts_with("legaia-engine play-window "), "{report}");
    assert!(
        report.contains(&format!("scene town01, mode Field, frame {frame}")),
        "{report}"
    );
    assert!(report.contains("options: "), "{report}");
    assert!(!report.contains(&disc), "the report names no disc path");
    if let Some(home) = std::env::var("HOME").ok().filter(|h| h.len() > 1) {
        assert!(!report.contains(&home), "the report names no home folder");
    }
}

/// Two presses of the reset row put a rebound table back to the defaults;
/// one press changes nothing.
#[test]
fn the_keys_page_resets_bindings_on_the_second_press() {
    let Some(disc) = disc() else { return };
    let mut w = HeadlessPlayWindow::open(std::path::Path::new(&disc), "town01").expect("open");
    let defaults = Mapping::default();
    w.mapping_mut()
        .bindings
        .insert("Q".to_string(), "Cross".to_string());
    assert_ne!(w.mapping_mut().bindings, defaults.bindings);

    assert!(w.keys_footer().ends_with("Reset: Delete"));
    w.keys_reset_press();
    assert!(w.keys_footer().starts_with("Press again"));
    assert_ne!(
        w.mapping_mut().bindings,
        defaults.bindings,
        "one press only arms the reset"
    );
    w.keys_reset_press();
    assert_eq!(w.mapping_mut().bindings, defaults.bindings);
    assert!(w.mapping_mut().gamepad == defaults.gamepad);
    assert!(w.keys_footer().contains("back to the defaults"));
}
