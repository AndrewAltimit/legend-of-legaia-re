//! `legaia-studio`: the desktop asset browser / extractor.
//!
//! A GUI program with no command-line contract, so on Windows it is a
//! `windows`-subsystem binary: double-clicking it opens no console window.
//! An optional first argument (what "Open with" passes) is a disc image or
//! extracted folder to open; the `LEGAIA_STUDIO_*` variables documented on
//! [`legaia_studio::app::Automation`] script a run for screenshots and tests.

#![cfg_attr(windows, windows_subsystem = "windows")]

use std::path::PathBuf;

use legaia_studio::Studio;
use legaia_studio::app::{App, Automation};
use legaia_studio::settings::Settings;

const HELP: &str = "\
legaia-studio - browse, preview and extract the assets on your Legend of Legaia disc

Usage: legaia-studio [DISC.bin | DISC.cue | EXTRACTED_FOLDER]

Starts the desktop app. The optional path is opened at start; otherwise the
last disc or folder is reopened. No game data is included: the app reads only
the disc image you choose.
";

fn main() -> eframe::Result {
    // The two flags every binary in the release answers. On Windows this is
    // a GUI-subsystem program, so they print only when a console is attached.
    match std::env::args().nth(1).as_deref() {
        Some("--version" | "-V") => {
            println!("legaia-studio {}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        Some("--help" | "-h") => {
            print!("{HELP}");
            return Ok(());
        }
        _ => {}
    }
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let initial = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty() && !p.to_string_lossy().starts_with('-'))
        .or_else(|| std::env::var_os("LEGAIA_STUDIO_OPEN").map(PathBuf::from));
    let studio = Studio::new(Settings::default_path());
    let auto = Automation::from_env();
    let mut viewport = eframe::egui::ViewportBuilder::default()
        .with_title("Legaia Studio")
        .with_app_id("legaia-studio")
        .with_inner_size([1280.0, 820.0])
        .with_min_inner_size([720.0, 480.0])
        .with_drag_and_drop(true);
    if auto.screenshot.is_some() {
        // A scripted screenshot reads the window's own pixels back; on a
        // shared desktop another window on top would end up in the capture.
        viewport = viewport.with_window_level(eframe::egui::WindowLevel::AlwaysOnTop);
    }
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    eframe::run_native(
        "Legaia Studio",
        options,
        Box::new(move |_cc| Ok(Box::new(App::new(studio, initial, auto)))),
    )
}
