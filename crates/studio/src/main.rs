//! `legaia-studio`: the desktop asset browser / extractor.
//!
//! A GUI program with no command-line contract, so on Windows it is a
//! `windows`-subsystem binary: double-clicking it opens no console window.
//! An optional first argument (what "Open with" passes) is a disc image or
//! extracted folder to open; the `LEGAIA_STUDIO_*` variables documented on
//! [`legaia_studio::app::Automation`] script a run for screenshots and tests.

#![cfg_attr(windows, windows_subsystem = "windows")]

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use legaia_studio::Studio;
use legaia_studio::app::{App, Automation};
use legaia_studio::placement;
use legaia_studio::settings::Settings;
use winit::application::ApplicationHandler;
use winit::event::{DeviceEvent, DeviceId, StartCause, WindowEvent};
use winit::event_loop::ActiveEventLoop;
use winit::window::WindowId;

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
    // The size the window had when it last closed (the settings file).
    let size = studio
        .settings
        .window_size
        .filter(|[w, h]| w.is_finite() && h.is_finite())
        .map(|[w, h]| [w.max(720.0), h.max(480.0)])
        .unwrap_or([1280.0, 820.0]);
    let mut viewport = eframe::egui::ViewportBuilder::default()
        .with_title("Legaia Studio")
        .with_app_id("legaia-studio")
        .with_inner_size(size)
        .with_min_inner_size([720.0, 480.0])
        .with_drag_and_drop(true);
    if auto.screenshot.is_some() {
        // A scripted screenshot reads the window's own pixels back; on a
        // shared desktop another window on top would end up in the capture.
        viewport = viewport.with_window_level(eframe::egui::WindowLevel::AlwaysOnTop);
    }
    // The last window position, put back only onto a monitor that is still
    // connected; otherwise eframe centres the window (`placement`). The
    // monitors are only known once the event loop runs, so the probe below
    // reads them just before eframe creates the window, and the builder hook
    // (called during that creation) applies the result.
    let monitors: Arc<Mutex<Vec<placement::Monitor>>> = Arc::default();
    let saved_pos = studio.settings.window_pos;
    let zoom = legaia_studio::settings::sane_zoom(studio.settings.zoom);
    let seen = monitors.clone();
    let options = eframe::NativeOptions {
        viewport,
        centered: true,
        window_builder: Some(Box::new(move |b| {
            let monitors = seen.lock().map(|m| m.clone()).unwrap_or_default();
            match placement::restore(saved_pos, zoom, &monitors) {
                Some(pos) => b.with_position(pos),
                None => b,
            }
        })),
        ..Default::default()
    };
    let event_loop = winit::event_loop::EventLoop::<eframe::UserEvent>::with_user_event()
        .build()
        .map_err(eframe::Error::WinitEventLoop)?;
    let app = eframe::create_native(
        "Legaia Studio",
        options,
        Box::new(move |_cc| Ok(Box::new(App::new(studio, initial, auto)))),
        &event_loop,
    );
    let mut probe = MonitorProbe { app, monitors };
    event_loop
        .run_app(&mut probe)
        .map_err(eframe::Error::WinitEventLoop)
}

/// The eframe application, with the connected monitors recorded at each
/// `resumed` before eframe sees it (it creates the window there).
struct MonitorProbe<'a> {
    app: eframe::EframeWinitApplication<'a>,
    monitors: Arc<Mutex<Vec<placement::Monitor>>>,
}

impl ApplicationHandler<eframe::UserEvent> for MonitorProbe<'_> {
    fn resumed(&mut self, evl: &ActiveEventLoop) {
        let found: Vec<placement::Monitor> = evl
            .available_monitors()
            .map(|m| placement::Monitor {
                x: m.position().x,
                y: m.position().y,
                width: m.size().width,
                height: m.size().height,
                scale: m.scale_factor(),
            })
            .collect();
        if let Ok(mut m) = self.monitors.lock() {
            *m = found;
        }
        self.app.resumed(evl);
    }
    fn window_event(&mut self, evl: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        self.app.window_event(evl, id, event);
    }
    fn new_events(&mut self, evl: &ActiveEventLoop, cause: StartCause) {
        self.app.new_events(evl, cause);
    }
    fn user_event(&mut self, evl: &ActiveEventLoop, event: eframe::UserEvent) {
        self.app.user_event(evl, event);
    }
    fn device_event(&mut self, evl: &ActiveEventLoop, id: DeviceId, event: DeviceEvent) {
        self.app.device_event(evl, id, event);
    }
    fn about_to_wait(&mut self, evl: &ActiveEventLoop) {
        self.app.about_to_wait(evl);
    }
    fn suspended(&mut self, evl: &ActiveEventLoop) {
        self.app.suspended(evl);
    }
    fn exiting(&mut self, evl: &ActiveEventLoop) {
        self.app.exiting(evl);
    }
    fn memory_warning(&mut self, evl: &ActiveEventLoop) {
        self.app.memory_warning(evl);
    }
}
