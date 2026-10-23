//! The launcher: what `legaia-engine` does when started with no subcommand
//! (a double-click, a desktop shortcut, the macOS app, `Legend of
//! Legaia.exe`).
//!
//! The pure half - remembered disc path, validation, re-prompt reasons, the
//! option rows - is [`legaia_engine_shell::launcher`]. This module is the
//! window: one screen drawn with the engine's own text overlay, so it needs
//! nothing the game does not already link.
//!
//! - **Disc card.** The disc in use and whether it is ready, or why not
//!   (first run, moved, unreadable, another region). *Choose disc...* opens
//!   the native file dialog (`rfd`); dropping a file on the window or typing
//!   a path also works, for desktops without a file-dialog portal. A disc
//!   that validates is remembered, and from then on the screen letters
//!   itself in the game's own font, read from that disc.
//! - **Options.** The engine's enhancement knobs the game remembers
//!   (written to its own `legaia-options.toml`, the value the in-game keys
//!   toggle) and the launch switches with no option behind them
//!   (`--psx-render`, `--retail`, `--no-entry-pulse`, `--smart-monsters`,
//!   the gameplay tweaks, kept in `launcher.toml`), in six groups. The list scrolls when it is
//!   longer than the panel: focus keeps the selected row in view, and a bar
//!   on the panel's edge shows where the view is. Every row says what it
//!   does in the help box.
//! - **Controls.** The current key bindings (`legaia-input.toml`), so a
//!   player sees them before the game starts.
//! - **Play** starts the game in a child process (winit allows one event
//!   loop per process, and the launcher holds it) and closes the window.
//!   A clean exit ends the launcher; a failure brings the window back with
//!   the end of the game's log (`legaia-engine-game.log`).
//!
//! The screen is keyboard-first: arrows move, Enter activates, Left / Right
//! change a value, Tab jumps between the columns, Page Up / Page Down / Home
//! / End move through the option list, Esc quits; the mouse hovers, clicks
//! (right-click steps a value back) and its wheel scrolls the option list.
//!
//! The game runs with its working directory set to [`launcher::data_dir`],
//! so key bindings, options and saves always land in the same per-user
//! place. `LEGAIA_LAUNCHER_DIRECT=1` skips the screen and boots a remembered
//! disc that still validates straight away, in this process.

use std::ffi::OsString;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use legaia_engine_menus::options::OptionsState;
use legaia_engine_render::window::EngineWindow;
use legaia_engine_render::{RenderTarget, TextOverlay, UploadedFontAtlas};
use legaia_engine_shell::launcher::{
    self, DiscInfo, DiscProblem, LaunchDecision, LauncherSettings, OPTION_ROWS, OptionRow,
};
use legaia_engine_ui::TextDraw;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::WindowId;

/// Debug aid: when set, the launcher writes its first frame to this PNG and
/// exits (lets a headless check see the screen without a human).
const SCREENSHOT_ENV: &str = "LEGAIA_LAUNCHER_SCREENSHOT";
/// With [`SCREENSHOT_ENV`]: open the right panel on `controls`.
const PANEL_ENV: &str = "LEGAIA_LAUNCHER_PANEL";
/// With [`SCREENSHOT_ENV`]: focus option row N (0-based, clamped), so a
/// capture can show the list scrolled.
const FOCUS_ENV: &str = "LEGAIA_LAUNCHER_FOCUS_ROW";
/// Skip the screen when the remembered disc is good (the older behaviour).
const DIRECT_ENV: &str = "LEGAIA_LAUNCHER_DIRECT";
const TITLE: &str = "Legend of Legaia";

/// Run the launcher. Returns the argument list to run in-process (only
/// under [`DIRECT_ENV`] with a good remembered disc), or `None` when the
/// launcher already finished - the user quit, or the game ran in a child
/// process.
pub(crate) fn run() -> Result<Option<Vec<OsString>>> {
    console::detach_if_double_clicked();
    let settings_file = launcher::settings_path();
    let loaded = match settings_file.as_deref() {
        Some(f) => LauncherSettings::load(f),
        None => Ok(LauncherSettings::default()),
    };
    let settings = loaded.as_ref().cloned().unwrap_or_default();
    let decision = launcher::decide(loaded, launcher::validate_disc);
    enter_data_dir();
    if let LaunchDecision::Play(info) = &decision
        && std::env::var_os(DIRECT_ENV).is_some_and(|v| !v.is_empty() && v != "0")
    {
        log::info!("launcher: booting {} directly", info.bin.display());
        let mut args = vec![OsString::from("legaia-engine")];
        args.extend(launcher::play_args_with(info, &settings));
        return Ok(Some(args));
    }
    let (disc, problem) = match decision {
        LaunchDecision::Play(info) => (Some(info), None),
        LaunchDecision::Prompt(p) => {
            log::info!("launcher: {p}");
            (None, Some(p))
        }
    };
    let mut app = LauncherApp::new(disc, problem, settings, settings_file);
    let event_loop = picker_event_loop()?;
    event_loop
        .run_app(&mut app)
        .context("launcher event loop")?;
    Ok(None)
}

/// Make the per-user data directory the working directory.
fn enter_data_dir() {
    let Some(dir) = launcher::data_dir() else {
        return;
    };
    if let Err(e) = std::fs::create_dir_all(&dir).and_then(|()| std::env::set_current_dir(&dir)) {
        log::warn!(
            "launcher: cannot use {} as the data directory ({e}); staying in the current one",
            dir.display()
        );
    } else {
        log::info!("launcher: data directory {}", dir.display());
    }
}

/// The console window a double-click on `legaia-engine.exe` brings with it.
///
/// The engine is a console program (its CLI subcommands need the ordinary
/// terminal contract), so Explorer gives a double-clicked copy a console
/// window of its own. When this process is that console's only user - no
/// shell shares it - the launcher frees it, and starts the game child with
/// `CREATE_NO_WINDOW` so it does not get a fresh one. Started from a
/// terminal, the console is shared and stays, so the log still prints. The
/// release archive's `Legend of Legaia.exe` (`legaia-launch`) avoids even the
/// brief flash this leaves; docs/tooling/releases.md has the reasoning.
mod console {
    #[cfg(windows)]
    static DETACHED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

    #[cfg(windows)]
    pub(super) fn detach_if_double_clicked() {
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn GetConsoleProcessList(list: *mut u32, count: u32) -> u32;
            fn FreeConsole() -> i32;
        }
        if std::env::var_os("LEGAIA_KEEP_CONSOLE").is_some() {
            return;
        }
        let mut pids = [0u32; 2];
        // SAFETY: the buffer holds `pids.len()` entries; the call writes at
        // most that many and returns the total attached-process count.
        let attached = unsafe { GetConsoleProcessList(pids.as_mut_ptr(), pids.len() as u32) };
        // SAFETY: plain Win32 call with no arguments.
        if attached == 1 && unsafe { FreeConsole() } != 0 {
            DETACHED.store(true, std::sync::atomic::Ordering::Relaxed);
        }
    }

    #[cfg(windows)]
    pub(super) fn no_window_if_detached(cmd: &mut std::process::Command) {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        if DETACHED.load(std::sync::atomic::Ordering::Relaxed) {
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
    }

    #[cfg(not(windows))]
    pub(super) fn detach_if_double_clicked() {}

    #[cfg(not(windows))]
    pub(super) fn no_window_if_detached(_cmd: &mut std::process::Command) {}
}

/// Build the launcher's event loop.
///
/// winit 0.30 delivers `DroppedFile` on X11, Windows and macOS but has no
/// drag-and-drop on Wayland at all, so a disc dropped on a native Wayland
/// window is silently ignored. The launcher is one small window that needs
/// nothing Wayland-specific, so on a Wayland session that also offers
/// XWayland (`DISPLAY` set) it opens as an X11 client instead: the compositor
/// bridges a drag from a Wayland file manager into XWayland, and the drop
/// arrives. The game itself runs in a fresh process on the default backend.
/// `LEGAIA_LAUNCHER_WAYLAND=1` keeps the launcher native (no drop; the file
/// dialog and the typed path still work).
fn picker_event_loop() -> Result<EventLoop<()>> {
    let mut builder = EventLoop::builder();
    #[cfg(all(unix, not(target_os = "macos")))]
    if prefer_x11_for_drop(
        std::env::var_os("WAYLAND_DISPLAY").is_some_and(|v| !v.is_empty()),
        std::env::var_os("DISPLAY").is_some_and(|v| !v.is_empty()),
        std::env::var_os("LEGAIA_LAUNCHER_WAYLAND").is_some(),
    ) {
        use winit::platform::x11::EventLoopBuilderExtX11;
        log::info!(
            "launcher: Wayland session - opening the launcher through XWayland so drops work"
        );
        builder.with_x11();
    }
    builder.build().context("create event loop")
}

/// Whether the launcher should open through XWayland (see `picker_event_loop`).
#[cfg_attr(any(not(unix), target_os = "macos"), allow(dead_code))]
fn prefer_x11_for_drop(wayland: bool, x11: bool, keep_wayland: bool) -> bool {
    wayland && x11 && !keep_wayland
}

// --- Palette -----------------------------------------------------------
//
// The shared dark palette (`legaia_app_theme`), which Studio's dark theme
// draws in too.

use legaia_app_theme::DARK as P;

const INK: [f32; 4] = P.ink.to_f32();
const DIM: [f32; 4] = P.ink_dim.to_f32();
const FAINT: [f32; 4] = P.ink_faint.to_f32();
const GOLD: [f32; 4] = P.accent.to_f32();
const GOOD: [f32; 4] = P.good.to_f32();
const WARN: [f32; 4] = P.warn.to_f32();
const ON_GOLD: [f32; 4] = P.on_accent.to_f32();
const BODY: [f32; 4] = P.ground.to_f32();
const BAND: [f32; 4] = P.band.to_f32();
const CARD: [f32; 4] = P.card.to_f32();
const FOCUS: [f32; 4] = P.hover.to_f32();
const BUTTON: [f32; 4] = P.raised.to_f32();
const PRIMARY: [f32; 4] = P.accent_fill.to_f32();
const PRIMARY_FOCUS: [f32; 4] = P.accent_fill_focus.to_f32();
const RULE: [f32; 4] = P.rule.to_f32();

/// Virtual stage the screen is laid out on; scaled up by an integer factor
/// (2x in the 960x720 window, 4x on a 2x HiDPI display) and centred.
const STAGE_W: i32 = 480;
const STAGE_H: i32 = 360;
const PAD: i32 = 14;
/// The two columns.
const LEFT_X: i32 = PAD;
const LEFT_W: i32 = 170;
const RIGHT_X: i32 = LEFT_X + LEFT_W + 10;
const RIGHT_W: i32 = STAGE_W - PAD - RIGHT_X;
const COLUMNS_Y: i32 = 108;
const BUTTON_H: i32 = 20;
const BUTTON_PITCH: i32 = 24;
const ROW_PITCH: i32 = 13;
/// The option list's inset from the panel's top and bottom edges.
const LIST_INSET: i32 = 5;
/// How many option-list lines (rows and group headings) the panel shows.
const VISIBLE_LINES: usize = ((STAGE_H - PAD - COLUMNS_Y - 2 * LIST_INSET) / ROW_PITCH) as usize;

/// One line of the option list: a group heading or a row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Line {
    Heading(&'static str),
    Row(OptionRow),
}

/// The option list's lines in screen order.
fn option_lines() -> Vec<Line> {
    let mut v = Vec::with_capacity(OPTION_ROWS.len() + 6);
    for (heading, row) in OPTION_ROWS {
        if let Some(h) = heading {
            v.push(Line::Heading(h));
        }
        v.push(Line::Row(row));
    }
    v
}

/// The first visible line that keeps line `idx` in view from `scroll`,
/// together with its group heading when `idx` opens a group.
fn scroll_to_reveal(lines: &[Line], scroll: usize, idx: usize) -> usize {
    let top = if idx > 0 && matches!(lines[idx - 1], Line::Heading(_)) {
        idx - 1
    } else {
        idx
    };
    let max = lines.len().saturating_sub(VISIBLE_LINES);
    let s = if top < scroll {
        top
    } else if idx >= scroll + VISIBLE_LINES {
        idx + 1 - VISIBLE_LINES
    } else {
        scroll
    };
    s.min(max)
}

/// A focusable control.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Item {
    Play,
    ChooseDisc,
    Panel,
    DataFolder,
    Quit,
    Opt(OptionRow),
}

const BUTTONS: [Item; 5] = [
    Item::Play,
    Item::ChooseDisc,
    Item::Panel,
    Item::DataFolder,
    Item::Quit,
];

/// What the right column shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Panel {
    Options,
    Controls,
}

/// A line of the help box with its ink.
type Message = (String, [f32; 4]);

/// A game the launcher started.
struct Running {
    child: std::process::Child,
    log: Option<PathBuf>,
    pumps: Vec<std::thread::JoinHandle<()>>,
}

struct LauncherApp {
    win: EngineWindow,
    font: legaia_font::Font,
    /// Whether [`Self::font`] is the disc's own (vs the built-in one).
    disc_font: bool,
    atlas: Option<UploadedFontAtlas>,
    solid: (u32, u32, u32, u32),
    disc: Option<DiscInfo>,
    problem: Option<DiscProblem>,
    settings: LauncherSettings,
    settings_file: Option<PathBuf>,
    options: OptionsState,
    focus: Item,
    panel: Panel,
    /// The option list's first visible line (see [`option_lines`]).
    opt_scroll: usize,
    /// A message that outranks the focused row's help (an error, a result).
    status: Option<Vec<Message>>,
    /// Path typed into the window (fallback when no file dialog is available).
    typed: String,
    modifiers: ModifiersState,
    /// Last cursor position, in surface pixels.
    cursor: Option<(f64, f64)>,
    game: Option<Running>,
    /// When the window last opened (input settles for a moment after).
    opened_at: Instant,
    screenshot: Option<PathBuf>,
}

impl LauncherApp {
    fn new(
        disc: Option<DiscInfo>,
        problem: Option<DiscProblem>,
        settings: LauncherSettings,
        settings_file: Option<PathBuf>,
    ) -> Self {
        let options = OptionsState::load_or_default(Path::new(launcher::OPTIONS_FILE));
        let panel = match std::env::var(PANEL_ENV).as_deref() {
            Ok("controls") => Panel::Controls,
            _ => Panel::Options,
        };
        let focus = if disc.is_some() {
            Item::Play
        } else {
            Item::ChooseDisc
        };
        let mut app = Self {
            win: EngineWindow::new(),
            font: legaia_font::Font::placeholder(),
            disc_font: false,
            atlas: None,
            solid: (0, 0, 1, 1),
            disc: None,
            problem,
            settings,
            settings_file,
            options,
            focus,
            panel,
            opt_scroll: 0,
            status: None,
            typed: String::new(),
            modifiers: ModifiersState::empty(),
            cursor: None,
            game: None,
            opened_at: Instant::now(),
            screenshot: std::env::var_os(SCREENSHOT_ENV).map(PathBuf::from),
        };
        app.set_disc(disc);
        app.status = app
            .problem
            .as_ref()
            .map(problem_message)
            .filter(|m| !m.is_empty());
        if let Some(n) = std::env::var(FOCUS_ENV)
            .ok()
            .and_then(|v| v.trim().parse::<i32>().ok())
        {
            app.focus = Item::Play;
            app.step_option(n + 1);
        }
        app
    }

    /// Adopt `disc` (or none) and letter the screen in its font.
    fn set_disc(&mut self, disc: Option<DiscInfo>) {
        let font = disc.as_ref().and_then(|d| launcher::disc_font(&d.bin));
        self.disc_font = font.is_some();
        self.font = font.unwrap_or_else(legaia_font::Font::placeholder);
        self.solid = legaia_engine_ui::font_solid_src(&self.font).unwrap_or((0, 0, 1, 1));
        self.disc = disc;
        self.atlas = None;
        self.upload_font();
    }

    fn upload_font(&mut self) {
        if self.atlas.is_some() {
            return;
        }
        if let Some(r) = self.win.renderer() {
            match r.upload_font(&self.font) {
                Ok(a) => self.atlas = Some(a),
                Err(e) => log::error!("launcher font upload: {e:#}"),
            }
        }
    }

    fn save_settings(&mut self) {
        if let Some(f) = &self.settings_file
            && let Err(e) = self.settings.save(f)
        {
            log::warn!("launcher: could not save settings ({e:#})");
        }
    }

    fn save_options(&mut self) {
        if let Err(e) = self.options.save(Path::new(launcher::OPTIONS_FILE)) {
            self.status = Some(vec![(format!("Could not save the options: {e:#}"), WARN)]);
        }
    }

    fn try_disc(&mut self, path: &Path) {
        match launcher::validate_disc(path) {
            Ok(info) => {
                if let Some(f) = &self.settings_file {
                    if let Err(e) = launcher::remember(f, &info) {
                        log::warn!("launcher: could not save settings ({e:#})");
                    } else {
                        log::info!("launcher: remembered {} in {}", path.display(), f.display());
                    }
                }
                self.settings.disc = Some(info.path.clone());
                self.problem = None;
                self.typed.clear();
                self.set_disc(Some(info));
                self.focus = Item::Play;
                self.status = Some(vec![("Disc ready. Press Enter to play.".into(), GOOD)]);
            }
            Err(p) => {
                log::warn!("launcher: {p}");
                let mut msg = problem_message(&p);
                if self.disc.is_some() {
                    // The good disc stays; say what was refused.
                    msg.insert(0, (format!("{}:", problem_headline(&p)), WARN));
                } else {
                    self.problem = Some(p.clone());
                }
                if msg.is_empty() {
                    msg.push((p.to_string(), WARN));
                }
                self.status = Some(msg);
            }
        }
        self.win.request_redraw();
    }

    fn browse(&mut self) {
        let mut dialog = rfd::FileDialog::new()
            .set_title("Choose your Legend of Legaia (USA) disc image")
            .add_filter("PlayStation disc image", &["bin", "cue", "BIN", "CUE"]);
        if let Some(dir) = self
            .settings
            .disc
            .as_deref()
            .and_then(Path::parent)
            .filter(|d| d.is_dir())
        {
            dialog = dialog.set_directory(dir);
        }
        if let Some(w) = self.win.window.as_deref() {
            dialog = dialog.set_parent(w);
        }
        match dialog.pick_file() {
            Some(p) => self.try_disc(&p),
            None => {
                self.status = Some(vec![(
                    "No file chosen. If no dialog appeared, drag the .bin onto this window, \
                     or type its full path and press Enter."
                        .into(),
                    DIM,
                )]);
                self.win.request_redraw();
            }
        }
    }

    /// Start the game and close the window while it runs.
    fn play(&mut self, evl: &ActiveEventLoop) {
        let Some(info) = self.disc.clone() else {
            self.status = Some(vec![(
                "Choose your Legend of Legaia (USA) disc image first.".into(),
                WARN,
            )]);
            self.focus = Item::ChooseDisc;
            return;
        };
        match spawn_game(&info, &self.settings) {
            Ok(g) => {
                log::info!("launcher: game started on {}", info.bin.display());
                self.game = Some(g);
                self.status = None;
                self.close_window();
                evl.set_control_flow(ControlFlow::WaitUntil(
                    Instant::now() + Duration::from_millis(250),
                ));
            }
            Err(e) => {
                self.status = Some(vec![(format!("Could not start the game: {e:#}"), WARN)]);
            }
        }
    }

    /// Close the window while the loop still runs. Dropped after `run_app`
    /// returns instead, the X11 destroy request is never flushed and a
    /// frozen window stays on screen behind the game (seen under Xvfb).
    fn close_window(&mut self) {
        self.atlas = None;
        if let Some(w) = self.win.window.as_deref() {
            w.set_visible(false);
        }
        self.win.renderer = None;
        self.win.window = None;
    }

    fn open_window(&mut self, evl: &ActiveEventLoop) {
        if !self.win.open(evl, TITLE) {
            return;
        }
        if let Some(w) = self.win.window.as_deref() {
            w.set_min_inner_size(Some(winit::dpi::LogicalSize::new(
                STAGE_W as f64,
                STAGE_H as f64,
            )));
        }
        self.atlas = None;
        self.upload_font();
        self.opened_at = Instant::now();
        self.win.request_redraw();
    }

    /// Notice the game ending: a clean exit ends the launcher, a failure
    /// brings the window back with the end of the log.
    fn poll_game(&mut self, evl: &ActiveEventLoop) {
        let Some(game) = &mut self.game else { return };
        match game.child.try_wait() {
            Ok(None) => {
                evl.set_control_flow(ControlFlow::WaitUntil(
                    Instant::now() + Duration::from_millis(250),
                ));
            }
            Ok(Some(status)) => {
                let game = self.game.take().expect("game present");
                for p in game.pumps {
                    let _ = p.join();
                }
                if status.success() {
                    log::info!("launcher: the game closed");
                    evl.exit();
                    return;
                }
                let mut msg = vec![(format!("The game stopped with an error ({status})."), WARN)];
                if let Some(log) = &game.log {
                    for line in log_tail(log, 2) {
                        msg.push((line, DIM));
                    }
                    msg.push((
                        format!(
                            "Full log: {} in the saves folder.",
                            log.file_name().unwrap_or_default().to_string_lossy()
                        ),
                        FAINT,
                    ));
                }
                self.status = Some(msg);
                evl.set_control_flow(ControlFlow::Wait);
                self.open_window(evl);
            }
            Err(e) => {
                log::warn!("launcher: lost track of the game ({e}); exiting");
                evl.exit();
            }
        }
    }

    fn open_data_folder(&mut self) {
        let dir = std::env::current_dir().unwrap_or_default();
        if let Err(e) = open_folder(&dir) {
            self.status = Some(vec![(
                format!("Could not open a file manager on {}: {e}", dir.display()),
                WARN,
            )]);
        }
    }

    /// Activate the focused control (`dir` steps an option backwards).
    fn activate(&mut self, item: Item, dir: i8, evl: &ActiveEventLoop) {
        self.focus = item;
        match item {
            Item::Play => self.play(evl),
            Item::ChooseDisc => self.browse(),
            Item::Panel => {
                self.panel = match self.panel {
                    Panel::Options => Panel::Controls,
                    Panel::Controls => Panel::Options,
                };
            }
            Item::DataFolder => self.open_data_folder(),
            Item::Quit => evl.exit(),
            Item::Opt(row) => {
                if self.panel != Panel::Options {
                    return;
                }
                row.adjust(&mut self.options, &mut self.settings, dir);
                log::info!(
                    "launcher: {} -> {}",
                    row.label(),
                    row.value(&self.options, &self.settings)
                );
                self.status = None;
                if row.is_game_option() {
                    self.save_options();
                } else {
                    self.save_settings();
                }
            }
        }
        self.win.request_redraw();
    }

    /// Every focusable control in Up / Down order.
    fn order(&self) -> Vec<Item> {
        let mut v = BUTTONS.to_vec();
        if self.panel == Panel::Options {
            v.extend(OPTION_ROWS.iter().map(|(_, r)| Item::Opt(*r)));
        }
        v
    }

    fn step_focus(&mut self, delta: i32) {
        let order = self.order();
        let n = order.len() as i32;
        let cur = order.iter().position(|i| *i == self.focus).unwrap_or(0) as i32;
        self.focus = order[((cur + delta).rem_euclid(n)) as usize];
        self.status = None;
        self.reveal_focus();
    }

    /// Move the focus `delta` option rows, stopping at the list's ends
    /// (Page Up / Page Down / Home / End).
    fn step_option(&mut self, delta: i32) {
        if self.panel != Panel::Options {
            return;
        }
        let n = OPTION_ROWS.len() as i32;
        let cur = match self.focus {
            Item::Opt(r) => OPTION_ROWS.iter().position(|(_, o)| *o == r).unwrap_or(0) as i32,
            _ if delta < 0 => n,
            _ => -1,
        };
        self.focus = Item::Opt(OPTION_ROWS[(cur + delta).clamp(0, n - 1) as usize].1);
        self.status = None;
        self.reveal_focus();
    }

    /// Scroll the option list so the focused row is in view.
    fn reveal_focus(&mut self) {
        let Item::Opt(row) = self.focus else { return };
        let lines = option_lines();
        if let Some(idx) = lines.iter().position(|l| *l == Line::Row(row)) {
            self.opt_scroll = scroll_to_reveal(&lines, self.opt_scroll, idx);
        }
    }

    /// Scroll the option list by `lines` (negative is up), clamped.
    fn scroll_options(&mut self, lines: i32) {
        let max = option_lines().len().saturating_sub(VISIBLE_LINES) as i32;
        self.opt_scroll = (self.opt_scroll as i32 + lines).clamp(0, max) as usize;
    }

    /// Tab: jump between the button column and the option rows.
    fn switch_column(&mut self) {
        self.focus = match self.focus {
            Item::Opt(_) => Item::Play,
            _ if self.panel == Panel::Options => Item::Opt(OPTION_ROWS[0].1),
            f => f,
        };
        self.reveal_focus();
    }

    // --- Drawing --------------------------------------------------------

    fn stage(&self) -> Stage {
        let (w, h) = self.win.surface_size();
        let scale = ((w as i32 / STAGE_W).min(h as i32 / STAGE_H)).max(1);
        let origin = (
            (w as i32 - STAGE_W * scale) / 2,
            (h as i32 - STAGE_H * scale) / 2,
        );
        Stage {
            scale,
            origin,
            surface: (w, h),
        }
    }

    /// Lay the screen out; `hits` gets each control's stage rectangle.
    fn layout(&self, c: &mut Canvas, hits: &mut Vec<(Item, Rect)>) {
        // Header band.
        c.fill_surface(BODY);
        c.rect(Rect::new(0, 0, STAGE_W, 44), BAND);
        c.rect(Rect::new(0, 44, STAGE_W, 1), RULE);
        c.text_scaled(PAD, 7, "Legend of Legaia", GOLD, 2);
        let sub = format!("engine port {}", env!("CARGO_PKG_VERSION"));
        let w = c.width(&sub);
        c.text(STAGE_W - PAD - w, 20, &sub, FAINT);

        self.disc_card(c);

        // Left column: the buttons.
        for (i, item) in BUTTONS.iter().enumerate() {
            let r = Rect::new(
                LEFT_X,
                COLUMNS_Y + i as i32 * BUTTON_PITCH,
                LEFT_W,
                BUTTON_H,
            );
            hits.push((*item, r));
            let focused = self.focus == *item;
            let enabled = *item != Item::Play || self.disc.is_some();
            let (bg, ink) = match (*item == Item::Play, enabled, focused) {
                (true, true, false) => (PRIMARY, ON_GOLD),
                (true, true, true) => (PRIMARY_FOCUS, ON_GOLD),
                (_, false, f) => (if f { FOCUS } else { BUTTON }, FAINT),
                (false, true, true) => (FOCUS, INK),
                (false, true, false) => (BUTTON, INK),
            };
            c.rect(r, bg);
            if focused {
                c.outline(r, GOLD);
            }
            let label = match item {
                Item::Play => "Play",
                Item::ChooseDisc => {
                    if self.disc.is_some() {
                        "Change disc..."
                    } else {
                        "Choose disc..."
                    }
                }
                Item::Panel => match self.panel {
                    Panel::Options => "Controls",
                    Panel::Controls => "Back to options",
                },
                Item::DataFolder => "Open saves folder",
                Item::Quit => "Quit",
                Item::Opt(_) => "",
            };
            c.text_centered(r, label, ink);
        }

        // Left column, below the buttons: help for the focused control.
        let help_y = COLUMNS_Y + BUTTONS.len() as i32 * BUTTON_PITCH + 4;
        let help = Rect::new(LEFT_X, help_y, LEFT_W, STAGE_H - PAD - help_y);
        c.rect(help, CARD);
        let mut y = help.y + 6;
        let lines: Vec<Message> = match &self.status {
            Some(s) => s.clone(),
            None => vec![(self.help_text(), DIM)],
        };
        let inner = help.w - 12;
        'outer: for (text, ink) in &lines {
            for l in c.wrap(text, inner) {
                if y + 12 > help.y + help.h - 14 {
                    break 'outer;
                }
                c.text(help.x + 6, y, &l, *ink);
                y += 12;
            }
            y += 3;
        }
        c.text(
            help.x + 6,
            help.y + help.h - 14,
            "Arrows, Enter, Tab, Esc",
            FAINT,
        );

        // Right column.
        let panel = Rect::new(RIGHT_X, COLUMNS_Y, RIGHT_W, STAGE_H - PAD - COLUMNS_Y);
        c.rect(panel, CARD);
        match self.panel {
            Panel::Options => self.options_panel(c, panel, hits),
            Panel::Controls => self.controls_panel(c, panel),
        }
    }

    fn disc_card(&self, c: &mut Canvas) {
        let card = Rect::new(PAD, 54, STAGE_W - 2 * PAD, 46);
        c.rect(card, CARD);
        let (accent, head) = match (&self.disc, &self.problem) {
            (Some(d), _) => (
                GOOD,
                format!(
                    "Legend of Legaia (USA) - ready{}",
                    if d.path != d.bin {
                        " (from the .cue)"
                    } else {
                        ""
                    }
                ),
            ),
            (None, Some(DiscProblem::NotSet)) | (None, None) => {
                (GOLD, "Choose your Legend of Legaia (USA) disc image".into())
            }
            (None, Some(p)) => (WARN, problem_headline(p).into()),
        };
        c.rect(Rect::new(card.x, card.y, 3, card.h), accent);
        c.text(card.x + 10, card.y + 6, "Disc", FAINT);
        let tx = card.x + 10 + c.width("Disc") + 10;
        c.text(tx, card.y + 6, &head, accent);
        let inner = card.x + card.w - 8 - tx;
        let (line, ink) = if !self.typed.is_empty() {
            (format!("Path: {}_", self.typed), GOLD)
        } else if let Some(d) = &self.disc {
            (d.path.display().to_string(), DIM)
        } else {
            match &self.problem {
                Some(DiscProblem::NotSet) | None => (
                    "Use your own .bin or .cue dump: click Choose disc, drop the file here, \
                     or type its path. No game data is included."
                        .into(),
                    DIM,
                ),
                Some(p) => match problem_path(p) {
                    Some(path) => (path.display().to_string(), DIM),
                    None => (p.to_string(), WARN),
                },
            }
        };
        // A path keeps its end (the file name); a sentence wraps.
        let is_path = !self.typed.is_empty()
            || self.disc.is_some()
            || self
                .problem
                .as_ref()
                .is_some_and(|p| problem_path(p).is_some());
        // A chosen file reads as its name, then the folder it is in.
        if is_path && self.typed.is_empty() {
            let p = Path::new(&line);
            let name = p
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| line.clone());
            c.text(tx, card.y + 20, &c.ellipsize_left(&name, inner), INK);
            if let Some(dir) = p.parent().filter(|d| !d.as_os_str().is_empty()) {
                // The last two folders are what a person recognises; the
                // full path is in the help for *Change disc*.
                let parts: Vec<String> = dir
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy().into_owned())
                    .collect();
                let tail = parts[parts.len().saturating_sub(2)..].join("/");
                let folder = if parts.len() > 2 {
                    format!(".../{tail}")
                } else {
                    dir.display().to_string()
                };
                let shown = format!("in {}", c.ellipsize_left(&folder, inner - c.width("in ")));
                c.text(tx, card.y + 32, &shown, FAINT);
            }
            return;
        }
        let lines = if is_path {
            vec![c.ellipsize_left(&line, inner)]
        } else {
            c.wrap(&line, inner)
        };
        for (i, l) in lines.iter().take(2).enumerate() {
            c.text(tx, card.y + 20 + i as i32 * 12, l, ink);
        }
    }

    fn options_panel(&self, c: &mut Canvas, panel: Rect, hits: &mut Vec<(Item, Rect)>) {
        let retail = self.settings.retail;
        let lines = option_lines();
        let scroll = self
            .opt_scroll
            .min(lines.len().saturating_sub(VISIBLE_LINES));
        let mut y = panel.y + LIST_INSET;
        for line in lines.iter().skip(scroll).take(VISIBLE_LINES) {
            let row = match *line {
                Line::Heading(h) => {
                    // A heading sits on a band of the header's colour, so a
                    // group reads as one block without a rule crowding the
                    // row above it.
                    c.rect(Rect::new(panel.x + 4, y, panel.w - 14, ROW_PITCH - 1), BAND);
                    c.text(panel.x + 8, y, h, GOLD);
                    y += ROW_PITCH;
                    continue;
                }
                Line::Row(r) => r,
            };
            let r = Rect::new(panel.x + 4, y - 1, panel.w - 14, ROW_PITCH);
            hits.push((Item::Opt(row), r));
            let focused = self.focus == Item::Opt(row);
            if focused {
                c.rect(r, FOCUS);
                c.rect(Rect::new(r.x, r.y, 2, r.h), GOLD);
            }
            let muted = retail && row.overridden_by_retail();
            let ink = if muted { FAINT } else { INK };
            c.text(panel.x + 10, y, row.label(), ink);
            let value = if muted {
                "Original".to_string()
            } else {
                row.value(&self.options, &self.settings)
            };
            let vink = match value.as_str() {
                _ if muted => FAINT,
                "On" => GOOD,
                "Off" => DIM,
                _ => GOLD,
            };
            let shown = if focused {
                format!("< {value} >")
            } else {
                value
            };
            let w = c.width(&shown);
            c.text(panel.x + panel.w - 16 - w, y, &shown, vink);
            y += ROW_PITCH;
        }
        // The scroll bar: the whole list is the track, the view the thumb.
        if lines.len() > VISIBLE_LINES {
            let track = Rect::new(
                panel.x + panel.w - 6,
                panel.y + LIST_INSET,
                2,
                VISIBLE_LINES as i32 * ROW_PITCH,
            );
            c.rect(track, RULE);
            let n = lines.len() as i32;
            let thumb_h = (track.h * VISIBLE_LINES as i32 / n).max(8);
            let max = n - VISIBLE_LINES as i32;
            let thumb_y = track.y + (track.h - thumb_h) * scroll as i32 / max.max(1);
            c.rect(Rect::new(track.x, thumb_y, track.w, thumb_h), DIM);
        }
    }

    fn controls_panel(&self, c: &mut Canvas, panel: Rect) {
        let mapping =
            legaia_engine_system::input::Mapping::load_or_default(Path::new(launcher::INPUT_FILE));
        let keys = |button: &str| -> String {
            let mut k: Vec<&str> = mapping
                .bindings
                .iter()
                .filter(|(_, b)| b.as_str() == button)
                .map(|(k, _)| match k.as_str() {
                    "RShift" => "Right Shift",
                    "LShift" => "Left Shift",
                    "RCtrl" => "Right Ctrl",
                    "LCtrl" => "Left Ctrl",
                    k => k,
                })
                .collect();
            k.sort_unstable();
            if k.is_empty() {
                "-".into()
            } else {
                k.join(", ")
            }
        };
        let dpad = ["Up", "Down", "Left", "Right"];
        let dpad_keys = if dpad.iter().all(|b| keys(b) == *b) {
            "Arrow keys".to_string()
        } else {
            dpad.iter().map(|b| keys(b)).collect::<Vec<_>>().join(" ")
        };
        let mut rows: Vec<(String, String)> = vec![("D-pad".into(), dpad_keys)];
        for b in [
            "Cross", "Circle", "Triangle", "Square", "L1", "R1", "L2", "R2", "Start", "Select",
        ] {
            rows.push((b.into(), keys(b)));
        }
        let mut y = panel.y + 5;
        c.text(panel.x + 8, y, "Button", GOLD);
        let kh = "Key";
        c.text(panel.x + panel.w - 10 - c.width(kh), y, kh, GOLD);
        y += ROW_PITCH;
        for (b, k) in rows {
            c.text(panel.x + 10, y, &b, INK);
            let w = c.width(&k);
            c.text(panel.x + panel.w - 10 - w, y, &k, GOLD);
            y += ROW_PITCH;
        }
        y += 4;
        for line in [
            "A gamepad works too. Key Config on the in-game Options screen rebinds keys.",
            "Esc twice quits the game; F12 lists the display keys.",
        ] {
            for l in c.wrap(line, panel.w - 20) {
                c.text(panel.x + 10, y, &l, DIM);
                y += 12;
            }
        }
    }

    fn help_text(&self) -> String {
        match self.focus {
            Item::Play => {
                if self.disc.is_some() {
                    "Start the game at the title screen. Saves and settings are kept in \
                     your own folder."
                        .into()
                } else {
                    "Choose your disc image first.".into()
                }
            }
            Item::ChooseDisc => match &self.disc {
                Some(d) => format!("Pick another disc image. Now: {}", d.path.display()),
                None => "Pick your Legend of Legaia (USA) disc image: the .bin of a \
                         Mode2/2352 dump, or its .cue. It is checked and remembered. Ctrl+O."
                    .into(),
            },
            Item::Panel => match self.panel {
                Panel::Options => "Show the keyboard controls.".into(),
                Panel::Controls => "Back to the options.".into(),
            },
            Item::DataFolder => {
                let dir = std::env::current_dir().unwrap_or_default();
                format!("Saves, options and key bindings live in {}", dir.display())
            }
            Item::Quit => "Close the launcher. Esc.".into(),
            Item::Opt(row) => {
                let mut s = row.help().to_string();
                if self.settings.retail && row.overridden_by_retail() {
                    s.push_str(" Original game only is on, so this plays as the original for now.");
                }
                s
            }
        }
    }

    fn redraw(&mut self, evl: &ActiveEventLoop) {
        let Some(atlas) = self.atlas.as_ref() else {
            return;
        };
        let stage = self.stage();
        let mut canvas = Canvas {
            font: &self.font,
            solid: self.solid,
            condense: !self.disc_font,
            stage,
            draws: Vec::new(),
        };
        let mut hits = Vec::new();
        self.layout(&mut canvas, &mut hits);
        let draws = canvas.draws;
        let Some(r) = self.win.renderer() else {
            return;
        };
        let overlay = TextOverlay {
            atlas,
            draws: &draws,
            blend: &[],
        };
        if let Some(path) = self.screenshot.take() {
            match r
                .capture_rgba(RenderTarget::TextOnly(&overlay))
                .and_then(|img| legaia_engine_shell::window::write_capture_png(&path, &img))
            {
                Ok(()) => println!("[ok] launcher screenshot {}", path.display()),
                Err(e) => eprintln!("launcher screenshot failed: {e:#}"),
            }
            evl.exit();
            return;
        }
        if let Err(e) = r.render(RenderTarget::TextOnly(&overlay)) {
            log::error!("launcher render: {e:#}");
        }
    }

    /// The control under the cursor, if any.
    fn hit(&self) -> Option<Item> {
        let (cx, cy) = self.cursor?;
        let st = self.stage();
        let x = ((cx as i32) - st.origin.0).div_euclid(st.scale);
        let y = ((cy as i32) - st.origin.1).div_euclid(st.scale);
        // Re-run the layout to get the rectangles (cheap: a few hundred quads).
        let mut canvas = Canvas {
            font: &self.font,
            solid: self.solid,
            condense: !self.disc_font,
            stage: st,
            draws: Vec::new(),
        };
        let mut hits = Vec::new();
        self.layout(&mut canvas, &mut hits);
        hits.into_iter()
            .find(|(_, r)| r.contains(x, y))
            .map(|(i, _)| i)
    }
}

impl ApplicationHandler for LauncherApp {
    fn resumed(&mut self, evl: &ActiveEventLoop) {
        if self.game.is_none() && self.win.window.is_none() {
            self.open_window(evl);
        }
    }

    fn about_to_wait(&mut self, evl: &ActiveEventLoop) {
        self.poll_game(evl);
    }

    fn window_event(&mut self, evl: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => evl.exit(),
            WindowEvent::Resized(s) => {
                self.win.handle_resize(s.width, s.height);
                self.win.request_redraw();
            }
            WindowEvent::ScaleFactorChanged { .. } => self.win.request_redraw(),
            WindowEvent::RedrawRequested => self.redraw(evl),
            WindowEvent::DroppedFile(p) => self.try_disc(&p),
            WindowEvent::ModifiersChanged(m) => self.modifiers = m.state(),
            // A scripted capture shows the scripted focus, not whatever row
            // the display's resting pointer happens to sit over.
            WindowEvent::CursorMoved { .. } if self.screenshot.is_some() => {}
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor = Some((position.x, position.y));
                if let Some(item) = self.hit()
                    && item != self.focus
                {
                    self.focus = item;
                    self.win.request_redraw();
                }
            }
            WindowEvent::CursorLeft { .. } => self.cursor = None,
            WindowEvent::MouseWheel { delta, .. } if self.panel == Panel::Options => {
                let lines = match delta {
                    winit::event::MouseScrollDelta::LineDelta(_, y) => -y.round() as i32,
                    winit::event::MouseScrollDelta::PixelDelta(p) => {
                        -(p.y / f64::from(ROW_PITCH * self.stage().scale)).round() as i32
                    }
                };
                if lines != 0 {
                    self.scroll_options(lines);
                    // The row under a resting cursor has changed.
                    if let Some(item) = self.hit() {
                        self.focus = item;
                    }
                    self.win.request_redraw();
                }
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button,
                ..
            } => {
                let dir = match button {
                    MouseButton::Left => 1,
                    MouseButton::Right => -1,
                    _ => return,
                };
                log::debug!("launcher: {button:?} click at {:?}", self.cursor);
                if let Some(item) = self.hit()
                    && (dir > 0 || matches!(item, Item::Opt(_)))
                {
                    self.activate(item, dir, evl);
                }
            }
            // A synthetic press is winit replaying a key already held when
            // the window gained focus (the game's last key, when the window
            // comes back) - not a press made here.
            WindowEvent::KeyboardInput {
                event,
                is_synthetic: false,
                ..
            } if event.state == ElementState::Pressed => {
                log::debug!(
                    "launcher: key {:?} repeat {}",
                    event.logical_key,
                    event.repeat
                );
                // Keys still auto-repeating from before the window opened
                // (the Enter that started the game, when the window comes
                // back) must not act; neither does a held activation key.
                let settling = self.opened_at.elapsed() < Duration::from_millis(300);
                let activation = matches!(
                    event.logical_key,
                    Key::Named(NamedKey::Enter | NamedKey::Space)
                );
                if settling || (event.repeat && activation) {
                    return;
                }
                let typing = !self.typed.is_empty();
                match &event.logical_key {
                    Key::Named(NamedKey::Escape) => {
                        if typing {
                            self.typed.clear();
                        } else if self.panel == Panel::Controls {
                            self.panel = Panel::Options;
                        } else {
                            evl.exit();
                        }
                    }
                    Key::Named(NamedKey::Enter) => {
                        if typing {
                            let p = PathBuf::from(clean_typed_path(&self.typed));
                            self.try_disc(&p);
                        } else {
                            self.activate(self.focus, 1, evl);
                        }
                    }
                    Key::Named(NamedKey::Space) if !typing => self.activate(self.focus, 1, evl),
                    Key::Named(NamedKey::Backspace) if typing => {
                        self.typed.pop();
                    }
                    Key::Named(NamedKey::ArrowUp) if !typing => self.step_focus(-1),
                    Key::Named(NamedKey::ArrowDown) if !typing => self.step_focus(1),
                    Key::Named(NamedKey::Tab) if !typing => self.switch_column(),
                    Key::Named(NamedKey::PageUp) if !typing => {
                        self.step_option(-(VISIBLE_LINES as i32 - 2));
                    }
                    Key::Named(NamedKey::PageDown) if !typing => {
                        self.step_option(VISIBLE_LINES as i32 - 2);
                    }
                    Key::Named(NamedKey::Home) if !typing => {
                        self.step_option(-(OPTION_ROWS.len() as i32));
                    }
                    Key::Named(NamedKey::End) if !typing => {
                        self.step_option(OPTION_ROWS.len() as i32);
                    }
                    Key::Named(NamedKey::ArrowLeft) | Key::Named(NamedKey::ArrowRight)
                        if !typing =>
                    {
                        let dir = if event.logical_key == Key::Named(NamedKey::ArrowLeft) {
                            -1
                        } else {
                            1
                        };
                        match self.focus {
                            Item::Opt(_) => self.activate(self.focus, dir, evl),
                            _ if dir > 0 && self.panel == Panel::Options => self.switch_column(),
                            _ => {}
                        }
                    }
                    Key::Character(c)
                        if !typing
                            && (self.modifiers.control_key() || self.modifiers.super_key())
                            && c.eq_ignore_ascii_case("o") =>
                    {
                        self.browse();
                    }
                    _ => {
                        if !(self.modifiers.control_key() || self.modifiers.super_key())
                            && let Some(t) = &event.text
                        {
                            self.typed.extend(t.chars().filter(|c| !c.is_control()));
                            if typing || !self.typed.trim().is_empty() {
                                self.status = None;
                            } else {
                                self.typed.clear();
                            }
                        }
                    }
                }
                self.win.request_redraw();
            }
            _ => {}
        }
    }
}

// --- Drawing helpers ---------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Rect {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
}

impl Rect {
    const fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Self { x, y, w, h }
    }
    fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.w && y < self.y + self.h
    }
}

#[derive(Debug, Clone, Copy)]
struct Stage {
    scale: i32,
    origin: (i32, i32),
    surface: (u32, u32),
}

/// Quads in stage coordinates, emitted in surface pixels.
struct Canvas<'a> {
    font: &'a legaia_font::Font,
    solid: (u32, u32, u32, u32),
    /// Set the built-in font 6 px a character instead of its 8 (its glyphs
    /// are 5 px wide; the wider pitch is for the game HUD's fallback).
    condense: bool,
    stage: Stage,
    draws: Vec<TextDraw>,
}

impl Canvas<'_> {
    fn fill_surface(&mut self, color: [f32; 4]) {
        self.draws.push(TextDraw {
            dst: (0, 0, self.stage.surface.0, self.stage.surface.1),
            src: self.solid,
            color,
        });
    }

    fn rect(&mut self, r: Rect, color: [f32; 4]) {
        let s = self.stage.scale;
        self.draws.push(TextDraw {
            dst: (
                self.stage.origin.0 + r.x * s,
                self.stage.origin.1 + r.y * s,
                (r.w.max(0) * s) as u32,
                (r.h.max(0) * s) as u32,
            ),
            src: self.solid,
            color,
        });
    }

    fn outline(&mut self, r: Rect, color: [f32; 4]) {
        self.rect(Rect::new(r.x, r.y, r.w, 1), color);
        self.rect(Rect::new(r.x, r.y + r.h - 1, r.w, 1), color);
        self.rect(Rect::new(r.x, r.y, 1, r.h), color);
        self.rect(Rect::new(r.x + r.w - 1, r.y, 1, r.h), color);
    }

    /// `|` is the dialog font's line break, and a byte outside ASCII has no
    /// glyph; both are replaced so a path draws on one line.
    fn clean(text: &str) -> String {
        text.chars()
            .map(|c| match c {
                '|' => '/',
                c if c.is_ascii() => c,
                _ => '?',
            })
            .collect()
    }

    fn width(&self, text: &str) -> i32 {
        self.pitch(self.font.layout_ascii(&Self::clean(text)).advance_x as i32)
    }

    fn pitch(&self, x: i32) -> i32 {
        if self.condense { x * 3 / 4 } else { x }
    }

    fn text(&mut self, x: i32, y: i32, text: &str, color: [f32; 4]) {
        self.text_scaled(x, y, text, color, 1);
    }

    fn text_scaled(&mut self, x: i32, y: i32, text: &str, color: [f32; 4], k: i32) {
        let s = self.stage.scale;
        let layout = self.font.layout_ascii(&Self::clean(text));
        for d in legaia_engine_ui::text_draws_for(&layout, (0, 0), color) {
            self.draws.push(TextDraw {
                dst: (
                    self.stage.origin.0 + (x + self.pitch(d.dst.0) * k) * s,
                    self.stage.origin.1 + (y + d.dst.1 * k) * s,
                    d.dst.2 * (k * s) as u32,
                    d.dst.3 * (k * s) as u32,
                ),
                ..d
            });
        }
    }

    fn text_centered(&mut self, r: Rect, text: &str, color: [f32; 4]) {
        let w = self.width(text);
        self.text(r.x + (r.w - w) / 2, r.y + (r.h - 12) / 2, text, color);
    }

    /// Word-wrap `text` to `max_w` stage pixels, hard-breaking a word wider
    /// than a line (a path).
    fn wrap(&self, text: &str, max_w: i32) -> Vec<String> {
        let mut lines = Vec::new();
        let mut cur = String::new();
        for word in text.split(' ') {
            let candidate = if cur.is_empty() {
                word.to_string()
            } else {
                format!("{cur} {word}")
            };
            if self.width(&candidate) <= max_w {
                cur = candidate;
                continue;
            }
            if !cur.is_empty() {
                lines.push(std::mem::take(&mut cur));
            }
            let mut piece = String::new();
            for ch in word.chars() {
                piece.push(ch);
                if self.width(&piece) > max_w {
                    piece.pop();
                    lines.push(std::mem::take(&mut piece));
                    piece.push(ch);
                }
            }
            cur = piece;
        }
        if !cur.is_empty() || lines.is_empty() {
            lines.push(cur);
        }
        lines
    }

    /// `text`, cut from the left with "..." until it fits `max_w`.
    fn ellipsize_left(&self, text: &str, max_w: i32) -> String {
        if self.width(text) <= max_w {
            return text.to_string();
        }
        let chars: Vec<char> = text.chars().collect();
        for start in 1..chars.len() {
            let s = format!("...{}", chars[start..].iter().collect::<String>());
            if self.width(&s) <= max_w {
                return s;
            }
        }
        "...".into()
    }
}

// --- The game process --------------------------------------------------

/// Start the game in a child process, its output copied into
/// [`launcher::GAME_LOG`] (in the data directory) as well as to this
/// process's own stdout / stderr, so a terminal still shows it.
fn spawn_game(info: &DiscInfo, settings: &LauncherSettings) -> Result<Running> {
    use std::process::{Command, Stdio};
    let exe = std::env::current_exe().context("locate the legaia-engine executable")?;
    let mut cmd = Command::new(&exe);
    cmd.args(launcher::play_args_with(info, settings))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    console::no_window_if_detached(&mut cmd);
    let log_path = std::env::current_dir()
        .ok()
        .map(|d| d.join(launcher::GAME_LOG));
    let log = log_path
        .as_ref()
        .and_then(|p| std::fs::File::create(p).ok())
        .map(|f| Arc::new(Mutex::new(f)));
    let mut child = cmd
        .spawn()
        .with_context(|| format!("start {}", exe.display()))?;
    let mut pumps = Vec::new();
    if let Some(out) = child.stdout.take() {
        pumps.push(pump(out, log.clone(), false));
    }
    if let Some(err) = child.stderr.take() {
        pumps.push(pump(err, log.clone(), true));
    }
    Ok(Running {
        child,
        log: log.and(log_path),
        pumps,
    })
}

fn pump<R: Read + Send + 'static>(
    mut src: R,
    log: Option<Arc<Mutex<std::fs::File>>>,
    to_stderr: bool,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let mut buf = [0u8; 8192];
        loop {
            let n = match src.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => n,
            };
            if let Some(l) = &log
                && let Ok(mut f) = l.lock()
            {
                let _ = f.write_all(&buf[..n]);
            }
            let _ = if to_stderr {
                std::io::stderr().write_all(&buf[..n])
            } else {
                std::io::stdout().write_all(&buf[..n])
            };
        }
    })
}

/// The lines of `log` worth showing a player: its last `n` error lines (a
/// panic, an `ERROR` record, an `Error:` exit), else its last line, each
/// without the logger's `[time LEVEL module]` prefix.
fn log_tail(log: &Path, n: usize) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(log) else {
        return Vec::new();
    };
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    let errors: Vec<&str> = lines
        .iter()
        .copied()
        .filter(|l| l.contains(" ERROR ") || l.contains("panicked") || l.starts_with("Error"))
        .collect();
    let pick = if errors.is_empty() { &lines } else { &errors };
    pick[pick.len().saturating_sub(n)..]
        .iter()
        .map(|l| strip_log_prefix(l).to_string())
        .collect()
}

/// `[2026-01-01T00:00:00Z INFO  module] text` -> `text`.
fn strip_log_prefix(line: &str) -> &str {
    if line.starts_with('[')
        && let Some(end) = line.find("] ")
    {
        return &line[end + 2..];
    }
    line
}

/// Open `dir` in the platform's file manager.
fn open_folder(dir: &Path) -> std::io::Result<()> {
    let opener = if cfg!(windows) {
        "explorer"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    std::process::Command::new(opener)
        .arg(dir)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|_| ())
}

/// The disc card's one-line summary of `p`; the full reason goes in the
/// help box ([`problem_message`]).
fn problem_headline(p: &DiscProblem) -> &'static str {
    match p {
        DiscProblem::NotSet => "Choose your Legend of Legaia (USA) disc image",
        DiscProblem::BadSettings(_) => "The saved settings could not be read",
        DiscProblem::Missing(_) => "There is no file there",
        DiscProblem::Moved(_) => "The disc used last time is no longer there",
        DiscProblem::Unreadable { .. } => "That file is not a usable disc image",
        DiscProblem::WrongDisc { .. } => "That disc is not Legend of Legaia (USA)",
    }
}

/// The file a problem is about, when it names one.
fn problem_path(p: &DiscProblem) -> Option<&Path> {
    match p {
        DiscProblem::Missing(p) | DiscProblem::Moved(p) => Some(p),
        DiscProblem::Unreadable { path, .. } | DiscProblem::WrongDisc { path, .. } => Some(path),
        DiscProblem::NotSet | DiscProblem::BadSettings(_) => None,
    }
}

/// The help-box explanation of `p`, with what to do about it.
fn problem_message(p: &DiscProblem) -> Vec<Message> {
    let (why, hint) = match p {
        DiscProblem::NotSet => return Vec::new(),
        DiscProblem::BadSettings(why) => (why.clone(), "Choose the disc again."),
        DiscProblem::Missing(_) | DiscProblem::Moved(_) => (
            "It may have been moved, renamed, or be on a drive that is not connected.".into(),
            "Choose it again where it is now.",
        ),
        DiscProblem::Unreadable { reason, .. } => (
            format!("{}.", capitalise_first(reason)),
            "Choose the .bin of a raw Mode2/2352 dump, or the .cue beside it.",
        ),
        DiscProblem::WrongDisc { reason, .. } => (
            format!("{}.", capitalise_first(reason)),
            "A randomized or translated USA disc works too.",
        ),
    };
    vec![(why, WARN), (hint.into(), DIM)]
}

fn capitalise_first(s: &str) -> String {
    let mut c = s.chars();
    c.next()
        .map(|f| f.to_uppercase().chain(c).collect())
        .unwrap_or_default()
}

/// Strip the quotes a pasted or shell-copied path often carries.
fn clean_typed_path(s: &str) -> String {
    let t = s.trim();
    let t = t
        .strip_prefix('"')
        .and_then(|x| x.strip_suffix('"'))
        .or_else(|| t.strip_prefix('\'').and_then(|x| x.strip_suffix('\'')))
        .unwrap_or(t);
    t.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn canvas(font: &legaia_font::Font) -> Canvas<'_> {
        Canvas {
            font,
            solid: (0, 0, 1, 1),
            condense: true,
            stage: Stage {
                scale: 2,
                origin: (0, 0),
                surface: (960, 720),
            },
            draws: Vec::new(),
        }
    }

    #[test]
    fn wrap_breaks_words_and_long_paths() {
        let font = legaia_font::Font::placeholder();
        let c = canvas(&font);
        let five = c.width("aa bb");
        assert_eq!(c.wrap("aa bb cc", five), vec!["aa bb", "cc"]);
        let four = c.width("abcd");
        assert_eq!(c.wrap("abcdefghij", four), vec!["abcd", "efgh", "ij"]);
        assert_eq!(c.wrap("", 32), vec![""]);
        assert!(
            c.wrap("x /very/long/path/name.bin", 64)
                .iter()
                .all(|l| c.width(l) <= 64)
        );
    }

    #[test]
    fn ellipsize_keeps_the_end_of_a_path() {
        let font = legaia_font::Font::placeholder();
        let c = canvas(&font);
        let s = c.ellipsize_left("/games/legaia/Legend of Legaia (USA).bin", 120);
        assert!(s.starts_with("..."));
        assert!(s.ends_with(".bin"));
        assert!(c.width(&s) <= 120);
        assert_eq!(c.ellipsize_left("short", 120), "short");
    }

    #[test]
    fn layout_hits_every_control_inside_the_stage() {
        let mut app = LauncherApp::new(None, Some(DiscProblem::NotSet), Default::default(), None);
        let font = legaia_font::Font::placeholder();
        let mut hits = Vec::new();
        for item in app.order() {
            // Focusing a control scrolls it into view.
            app.focus = item;
            app.reveal_focus();
            let mut c = canvas(&font);
            hits.clear();
            app.layout(&mut c, &mut hits);
            let (_, r) = hits
                .iter()
                .find(|(i, _)| *i == item)
                .unwrap_or_else(|| panic!("{item:?} has no hit rectangle"));
            assert!(
                r.x >= 0 && r.y >= 0 && r.x + r.w <= STAGE_W && r.y + r.h <= STAGE_H,
                "{item:?} {r:?} leaves the stage"
            );
        }
        // No two controls overlap.
        for (i, (a, ra)) in hits.iter().enumerate() {
            for (b, rb) in &hits[i + 1..] {
                let overlap = ra.x < rb.x + rb.w
                    && rb.x < ra.x + ra.w
                    && ra.y < rb.y + rb.h
                    && rb.y < ra.y + ra.h;
                assert!(!overlap, "{a:?} {ra:?} overlaps {b:?} {rb:?}");
            }
        }
    }

    #[test]
    fn focus_walks_every_control_and_wraps() {
        let mut app = LauncherApp::new(None, Some(DiscProblem::NotSet), Default::default(), None);
        assert_eq!(app.focus, Item::ChooseDisc, "no disc: focus the picker");
        let n = app.order().len() as i32;
        let start = app.focus;
        for _ in 0..n {
            app.step_focus(1);
        }
        assert_eq!(app.focus, start);
        app.switch_column();
        assert_eq!(app.focus, Item::Opt(OPTION_ROWS[0].1));
        app.switch_column();
        assert_eq!(app.focus, Item::Play);
    }

    #[test]
    fn no_subcommand_parses_and_play_args_reach_play_window() {
        use crate::cli::{Cli, Cmd};
        use clap::Parser;
        assert!(
            Cli::try_parse_from(["legaia-engine"])
                .unwrap()
                .cmd
                .is_none()
        );
        let info = DiscInfo {
            path: "/g/l.cue".into(),
            bin: "/g/l.bin".into(),
            executable: launcher::USA_EXECUTABLE.into(),
        };
        let settings = LauncherSettings {
            psx_render: true,
            retail: true,
            ..Default::default()
        };
        let mut argv = vec![OsString::from("legaia-engine")];
        argv.extend(launcher::play_args_with(&info, &settings));
        match Cli::try_parse_from(argv).unwrap().cmd {
            Some(Cmd::PlayWindow {
                disc,
                boot_ui,
                psx_render,
                retail,
                ..
            }) => {
                assert_eq!(disc.as_deref(), Some(Path::new("/g/l.bin")));
                assert!(boot_ui && psx_render && retail);
            }
            other => panic!("expected play-window, got {other:?}"),
        }
        // The gameplay switches parse too (without `--retail`, which clap
        // refuses them beside).
        let settings = LauncherSettings {
            no_entry_pulse: true,
            smart_monsters: true,
            ..Default::default()
        };
        let mut argv = vec![OsString::from("legaia-engine")];
        argv.extend(launcher::play_args_with(&info, &settings));
        match Cli::try_parse_from(argv).unwrap().cmd {
            Some(Cmd::PlayWindow {
                no_entry_pulse,
                smart_monsters,
                retail,
                ..
            }) => assert!(no_entry_pulse && smart_monsters && !retail),
            other => panic!("expected play-window, got {other:?}"),
        }
        // So do the gameplay-tweak rows.
        let settings = LauncherSettings {
            no_shiny_seru: true,
            flee_exp: true,
            exp_rate_permille: Some(2000),
            no_fortress_encounter_fix: true,
            jewel_fix: true,
            no_hp_readout_guard: true,
            spirit_ap: Some(-16),
            damage_ap: Some(200),
            seru_catch_pct: Some(75),
            enemy_attack_permille: Some(2000),
            enemy_stat_permille: Some(1500),
            equipment_drops: true,
            ..Default::default()
        };
        let mut argv = vec![OsString::from("legaia-engine")];
        argv.extend(launcher::play_args_with(&info, &settings));
        match Cli::try_parse_from(argv).unwrap().cmd {
            Some(Cmd::PlayWindow {
                no_shiny_seru,
                flee_exp,
                exp_rate,
                no_fortress_encounter_fix,
                jewel_fix,
                no_hp_readout_guard,
                spirit_ap,
                damage_ap,
                seru_catch_rate,
                enemy_attack_count,
                enemy_stat_scale,
                equipment_drops,
                ..
            }) => {
                assert_eq!((spirit_ap, damage_ap), (Some(-16), Some(200)));
                assert_eq!(seru_catch_rate, Some(75));
                assert_eq!(
                    (enemy_attack_count, enemy_stat_scale),
                    (Some(2000), Some(1500))
                );
                assert!(equipment_drops);
                assert!(no_shiny_seru && flee_exp && no_fortress_encounter_fix && jewel_fix);
                assert!(no_hp_readout_guard);
                assert_eq!(exp_rate, Some(2000));
            }
            other => panic!("expected play-window, got {other:?}"),
        }
    }

    #[test]
    fn option_scroll_reveals_rows_and_their_headings() {
        let lines = option_lines();
        assert!(
            lines.len() > VISIBLE_LINES,
            "the list is longer than the panel"
        );
        let last = lines.len() - 1;
        let max = lines.len() - VISIBLE_LINES;
        // The last row scrolls the view to the end; back at the first row
        // the first heading shows again.
        let s = scroll_to_reveal(&lines, 0, last);
        assert_eq!(s, max);
        assert_eq!(scroll_to_reveal(&lines, s, 1), 0);
        // A row already in view leaves the scroll alone.
        assert_eq!(scroll_to_reveal(&lines, 2, 5), 2);
        // A group's first row brings its heading with it.
        let sound = lines
            .iter()
            .position(|l| *l == Line::Row(OptionRow::MusicVolume))
            .unwrap();
        assert_eq!(lines[sound - 1], Line::Heading("Sound"));
        assert_eq!(scroll_to_reveal(&lines, max, sound), (sound - 1).min(max));
        // Every row is reachable and visible within the panel height.
        assert!(VISIBLE_LINES as i32 * ROW_PITCH + 2 * LIST_INSET <= STAGE_H - PAD - COLUMNS_Y);
    }

    #[test]
    fn picker_uses_xwayland_only_on_a_wayland_session_with_x11() {
        assert!(prefer_x11_for_drop(true, true, false));
        assert!(!prefer_x11_for_drop(true, true, true), "opt-out honoured");
        assert!(!prefer_x11_for_drop(true, false, false), "no XWayland");
        assert!(
            !prefer_x11_for_drop(false, true, false),
            "plain X11 already drops"
        );
    }

    #[test]
    fn typed_path_quotes_are_stripped() {
        assert_eq!(clean_typed_path("  \"/a b/c.bin\" "), "/a b/c.bin");
        assert_eq!(clean_typed_path("'/a/c.cue'"), "/a/c.cue");
        assert_eq!(clean_typed_path("/plain.bin"), "/plain.bin");
    }

    #[test]
    fn log_tail_skips_blank_lines() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("game.log");
        std::fs::write(&p, "a\n\nb\nc\n\n").unwrap();
        assert_eq!(log_tail(&p, 2), vec!["b", "c"]);
        assert!(log_tail(&dir.path().join("none.log"), 2).is_empty());
        // Error lines win over the chatter after them, prefix stripped.
        std::fs::write(
            &p,
            "[t INFO  m] loading\n[t ERROR m] no VRAM\n[t INFO  m] bye\n",
        )
        .unwrap();
        assert_eq!(log_tail(&p, 2), vec!["no VRAM"]);
    }
}
