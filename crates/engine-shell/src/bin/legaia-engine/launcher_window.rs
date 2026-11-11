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
//!   itself in the game's own font, read from that disc. The last few
//!   discs are kept (`LauncherSettings::recent_discs`); right-click or
//!   Shift+Enter on *Choose disc* steps to the next that still validates.
//! - **Options.** The engine's enhancement knobs the game remembers
//!   (written to its own `legaia-options.toml`, the value the in-game keys
//!   toggle) and the launch switches with no option behind them
//!   (`--psx-render`, `--retail`, `--no-entry-pulse`, `--smart-monsters`,
//!   the gameplay tweaks, kept in `launcher.toml`), in six groups. The list scrolls when it is
//!   longer than the panel: focus keeps the selected row in view, and a bar
//!   on the panel's edge shows where the view is. Every row says what it
//!   does in the help box; a value the original game does not have reads
//!   green, the original's own dim (`OptionRow::original_value`).
//! - **Controls.** The current key bindings (`legaia-input.toml`), so a
//!   player sees them before the game starts.
//! - **Recent.** The remembered discs, the saves on the game's own card and
//!   on card 2, and the remembered memory cards, as one list: a disc or card
//!   is picked or forgotten, a save starts the game on it
//!   (`--resume-save`, without the title screen).
//! - **Saves folder** opens the data directory; its second action (or a
//!   dropped `.mcr` / `.mcd` / `.gme` / `.mcs`) mounts an emulator memory
//!   card as the save screen's card 2 (`--card`, `LauncherSettings::card`):
//!   first the cards used here before, in turn
//!   (`LauncherSettings::recent_cards`), then a file dialog.
//! - **Play** starts the game in a child process (winit allows one event
//!   loop per process, and the launcher holds it) and closes the window.
//!   A clean exit ends the launcher; a failure brings the window back with
//!   the end of the game's log (`legaia-engine-game.log`).
//!
//! A controller works the screen as the game's menus do: the D-pad or left
//! stick moves, Cross activates, Triangle is the right-click, Circle leaves
//! the Controls page, Start plays, L1 / R1 page the options.
//!
//! The screen is keyboard-first: arrows move, Enter activates, Left / Right
//! change a value, Tab jumps between the columns, Page Up / Page Down / Home
//! / End move through the option list, Delete puts the focused option back
//! on its default (a changed row carries a gold dot), Esc quits; the mouse
//! hovers, clicks (right-click steps a value back, middle-click restores the
//! default) and its wheel scrolls the option list.
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
use legaia_engine_menus::key_rebind::{
    KeyRebindEvent, KeyRebindInput, KeyRebindPhase, KeyRebindSession,
};
use legaia_engine_menus::options::OptionsState;
use legaia_engine_render::window::EngineWindow;
use legaia_engine_render::{RenderTarget, TextOverlay, UploadedFontAtlas};
use legaia_engine_shell::launcher::{
    self, DiscInfo, DiscProblem, ExpansionInfo, LaunchDecision, LauncherSettings, OPTION_ROWS,
    OptionRow,
};
use legaia_engine_shell::window_placement;
use legaia_engine_system::input::Mapping;
use legaia_engine_ui::TextDraw;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, ModifiersState, NamedKey, PhysicalKey};
use winit::window::WindowId;

/// Debug aid: when set, the launcher writes its first frame to this PNG and
/// exits (lets a headless check see the screen without a human).
const SCREENSHOT_ENV: &str = "LEGAIA_LAUNCHER_SCREENSHOT";
/// With [`SCREENSHOT_ENV`]: open the right panel on `controls` or `recent`.
const PANEL_ENV: &str = "LEGAIA_LAUNCHER_PANEL";
/// With [`SCREENSHOT_ENV`]: focus option row N (0-based, clamped), so a
/// capture can show the list scrolled.
const FOCUS_ENV: &str = "LEGAIA_LAUNCHER_FOCUS_ROW";
/// With [`SCREENSHOT_ENV`]: open the window at this logical size (`WxH`),
/// so a capture can show the screen on a large or odd-sized window.
const SIZE_ENV: &str = "LEGAIA_LAUNCHER_SIZE";
/// Skip the screen when the remembered disc is good (the older behaviour).
const DIRECT_ENV: &str = "LEGAIA_LAUNCHER_DIRECT";
const TITLE: &str = "Legend of Legaia";
/// The help box's last line after a failed game: both ways to the report.
const COPY_REPORT_HINT: &str = "Click or Ctrl+C: report";

/// Run the launcher. Returns the argument list to run in-process (only
/// under [`DIRECT_ENV`] with a good remembered disc), or `None` when the
/// launcher already finished - the user quit, or the game ran in a child
/// process.
///
/// `opened` is a file the player opened the game with (`--open`, from the
/// `Legend of Legaia` program's "Open with" or a drop on its icon): the
/// window takes it as if it were dropped on it, once it is open.
pub(crate) fn run(opened: Option<PathBuf>) -> Result<Option<Vec<OsString>>> {
    console::detach_if_double_clicked();
    // Resolved now: the launcher moves into the data directory below.
    let opened = opened.map(|p| std::path::absolute(&p).unwrap_or(p));
    let mut settings_file = launcher::settings_path();
    let loaded = match settings_file.as_deref() {
        Some(f) => {
            let opened = launcher::open_settings(f);
            if !opened.keep_file {
                // Neither readable nor movable: run on the defaults and
                // leave the file as it is rather than write over it.
                settings_file = None;
            }
            opened.settings
        }
        None => Ok(LauncherSettings::default()),
    };
    let settings = loaded.as_ref().cloned().unwrap_or_default();
    let decision = launcher::decide(loaded, launcher::validate_disc);
    enter_data_dir();
    if let LaunchDecision::Play(info) = &decision
        && opened.is_none()
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
    app.opened = opened;
    let event_loop = match picker_event_loop() {
        Ok(l) => l,
        Err(e) => no_window(&format!("{e:#}")),
    };
    event_loop
        .run_app(&mut app)
        .context("launcher event loop")?;
    if let Some(why) = app.win.open_error.take() {
        no_window(&why);
    }
    Ok(None)
}

/// The launcher's window could not open, so it has no screen to say that on,
/// and started by a double-click it has no terminal either: without this the
/// program just ends and nothing has happened. Tell the player in a message
/// box and end as a failure.
fn no_window(why: &str) -> ! {
    let text = legaia_app_theme::game_exit::window_failure_message(why);
    eprintln!("{text}");
    if std::env::var_os(SCREENSHOT_ENV).is_none() {
        let _ = rfd::MessageDialog::new()
            .set_level(rfd::MessageLevel::Error)
            .set_title(TITLE)
            .set_description(&text)
            .set_buttons(rfd::MessageButtons::Ok)
            .show();
    }
    std::process::exit(legaia_app_theme::game_exit::ALREADY_REPORTED_EXIT);
}

/// `legaia-engine launch --disc <disc>`: the `play-window` argument list a
/// *Play* in this launcher would use on `disc`, every switch and the pack in
/// `launcher.toml` included. Legaia Explorer's *Play* goes through here, so a
/// game started from either program is the same game. A settings file that
/// does not parse plays on the defaults, the way the launcher window treats it.
///
/// The disc becomes the launcher's disc and the newest of its recent ones
/// ([`LauncherSettings::remember_disc`]), as a *Play* in the launcher makes
/// it: the disc played before stays one Shift+Enter away there.
pub(crate) fn launch_args(disc: &Path) -> Result<Vec<OsString>> {
    let settings_file = launcher::settings_path();
    let loaded = settings_file.as_deref().and_then(|f| {
        LauncherSettings::load(f)
            .inspect_err(|e| log::warn!("launch: {} ignored: {e:#}", f.display()))
            .ok()
    });
    let info = launcher::validate_disc(disc).map_err(|p| anyhow::anyhow!("{p}"))?;
    // A settings file that does not parse is left alone (the launcher window
    // re-prompts on it); only a readable or absent one is updated.
    let settings = match (settings_file.as_deref(), loaded) {
        (Some(f), Some(mut s)) => {
            s.remember_disc(&info.path);
            if let Err(e) = s.save(f) {
                log::warn!(
                    "launch: could not remember the disc in {} ({e:#})",
                    f.display()
                );
            }
            s
        }
        (_, s) => s.unwrap_or_default(),
    };
    let mut args = vec![OsString::from("legaia-engine")];
    args.extend(launcher::play_args_with(&info, &settings));
    Ok(args)
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
/// On a Wayland session that also offers XWayland the launcher opens as an
/// X11 client, because winit 0.30 has no drag-and-drop on Wayland
/// ([`legaia_app_theme::drop_backend`]); Legaia Explorer makes the same
/// choice. The game itself runs in a fresh process on the default backend.
/// `LEGAIA_LAUNCHER_WAYLAND=1` keeps the launcher native (no drop; the file
/// dialog and the typed path still work).
fn picker_event_loop() -> Result<EventLoop<()>> {
    let mut builder = EventLoop::builder();
    #[cfg(all(unix, not(target_os = "macos")))]
    if legaia_app_theme::drop_backend::prefer_x11_for_drop_from_env(
        legaia_app_theme::drop_backend::LAUNCHER_KEEP_WAYLAND,
    ) {
        use winit::platform::x11::EventLoopBuilderExtX11;
        log::info!(
            "launcher: Wayland session - opening the launcher through XWayland so drops work"
        );
        builder.with_x11();
    }
    builder.build().context("create event loop")
}

// --- Palette -----------------------------------------------------------
//
// The shared dark palette (`legaia_app_theme`), which Explorer's dark theme
// draws in too.

use legaia_app_theme::Role;

const INK: Role = Role::Ink;
const DIM: Role = Role::InkDim;
const FAINT: Role = Role::InkFaint;
const GOLD: Role = Role::Accent;
const GOOD: Role = Role::Good;
const WARN: Role = Role::Warn;
const ON_GOLD: Role = Role::OnAccent;
const BODY: Role = Role::Ground;
const BAND: Role = Role::Band;
const CARD: Role = Role::Card;
const FOCUS: Role = Role::Hover;
const BUTTON: Role = Role::Raised;
const PRIMARY: Role = Role::AccentFill;
const PRIMARY_FOCUS: Role = Role::AccentFillFocus;
const RULE: Role = Role::Rule;
const KEY: Role = legaia_app_theme::chrome::KEY;

/// Virtual stage the screen is laid out on; scaled up by an integer factor
/// (2x in the 960x720 window, 4x on a 2x HiDPI display) and centred.
const STAGE_W: i32 = 480;
const STAGE_H: i32 = 360;
/// How many stage rows the header band may give up so a window a little
/// short of a whole step keeps it ([`header_squeeze`]). A 960x720 window
/// on a 768-pixel-tall screen comes back about 700 tall once the window
/// manager's title bar and panel are paid for; without the give it fell to
/// 1x, a quarter of the window.
const HEADER_GIVE: i32 = 11;
/// The header band's height on an unsqueezed stage.
const HEADER_H: i32 = 44;
const PAD: i32 = 14;
/// The two columns.
const LEFT_X: i32 = PAD;
const LEFT_W: i32 = 170;
const RIGHT_X: i32 = LEFT_X + LEFT_W + 10;
const RIGHT_W: i32 = STAGE_W - PAD - RIGHT_X;
const COLUMNS_Y: i32 = 108;
const BUTTON_H: i32 = 17;
const BUTTON_PITCH: i32 = 20;
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

/// The help box while a disc path is typed or pasted into the window.
const TYPED_PATH_HELP: &str = "Enter checks the typed path and uses the disc there. \
     Backspace edits it, Esc clears it, Ctrl+V pastes.";

/// The disc card's line with no disc chosen. The card holds two lines of it;
/// that no game data ships is on the *Choose disc* help.
const FIRST_RUN_CARD: &str =
    "Use your own .bin or .cue dump: click Choose disc, drop the file here, or paste its path.";

/// The *Choose disc* help on the first run. Short enough to leave the help
/// box its key reminder: the first run is when a player least knows the
/// screen is keyboard-driven. The disc card above says how to choose.
const FIRST_RUN_HELP: &str = "Pick your own disc image: a dump's .bin, or its .cue. It is checked and remembered; \
     no game data comes with this program. Ctrl+O.";

/// A focusable control.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Item {
    Play,
    Online,
    ChooseDisc,
    Expansion,
    Panel,
    DataFolder,
    Quit,
    Opt(OptionRow),
    /// A row of the Controls page: the rebind session's row with this index.
    Bind(u8),
    /// The Controls page's last row: every key back on its default.
    BindReset,
    /// The button that opens the Recent page.
    Recent,
    /// A row of the Recent page: an index into [`LauncherApp::recent`].
    RecentRow(usize),
}

const BUTTONS: [Item; 8] = [
    Item::Play,
    Item::Online,
    Item::ChooseDisc,
    Item::Expansion,
    Item::Panel,
    Item::Recent,
    Item::DataFolder,
    Item::Quit,
];

/// What the right column shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Panel {
    Options,
    Controls,
    Recent,
}

/// What a row of the Recent page acts on.
#[derive(Debug, Clone, PartialEq, Eq)]
enum RecentKind {
    /// A remembered disc image: Enter uses it.
    Disc(PathBuf),
    /// A remembered memory card: Enter mounts it (or takes it out).
    Card(PathBuf),
    /// Open the file dialog for a new memory card.
    NewCard,
    /// A save on a card: Enter starts the game on it.
    Save(launcher::SaveRow),
    /// A line that does nothing (the empty note under a heading).
    Note,
}

/// One row of the Recent page.
struct RecentEntry {
    label: String,
    value: String,
    value_ink: Role,
    help: String,
    kind: RecentKind,
}

impl RecentEntry {
    /// A line with no action.
    fn note(text: &str) -> Self {
        Self {
            label: text.to_string(),
            value: String::new(),
            value_ink: DIM,
            help: String::new(),
            kind: RecentKind::Note,
        }
    }
}

/// One line of the Recent page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RecentLine {
    Heading(&'static str),
    Row(usize),
}

/// A line of the help box with its ink.
type Message = (String, Role);

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
    /// The disc *Choose disc*'s second action (right-click, Shift+Enter)
    /// switches to: the next remembered disc that still validates
    /// ([`launcher::next_recent_disc`]).
    recent_offer: Option<DiscInfo>,
    /// A disc image found in the usual folders on a first run
    /// ([`launcher::find_disc`]), offered until a disc is chosen.
    found_disc: Option<DiscInfo>,
    /// The Recent page's rows and lines ([`Self::build_recent`]), rebuilt
    /// each time the page opens or a row changes something.
    recent: Vec<RecentEntry>,
    recent_lines: Vec<RecentLine>,
    /// The Recent page's first visible line.
    recent_scroll: usize,
    /// The remembered expansion pack, checked ([`launcher::inspect_expansion`]),
    /// or why it could not be read.
    expansion: Option<Result<ExpansionInfo, String>>,
    /// The playable packs found in the installed and bundled packs folders
    /// ([`launcher::discover_packs`]), read once when the window opens: the
    /// Expansion button steps through them before it opens a file dialog.
    packs: Vec<ExpansionInfo>,
    /// The remembered memory card, checked ([`launcher::inspect_card`]), or
    /// why it cannot be mounted.
    card: Option<Result<launcher::CardInfo, String>>,
    settings: LauncherSettings,
    settings_file: Option<PathBuf>,
    options: OptionsState,
    /// The palette the screen draws in ([`Self::resolve_theme`]).
    dark: bool,
    focus: Item,
    /// Where Tab left each column ([`Self::switch_column`]): the button,
    /// and the option or key row, focused when the other column was
    /// entered, so Tab back returns to the same place.
    left_at: (Item, Option<Item>),
    panel: Panel,
    /// The option list's first visible line (see [`option_lines`]).
    opt_scroll: usize,
    /// A message that outranks the focused row's help (an error, a result).
    status: Option<Vec<Message>>,
    /// The typed path was tried and failed: the next character or paste
    /// starts over rather than adding to it.
    typed_tried: bool,
    /// The help-box text a file dragged over the window replaced
    /// ([`drop_hint`]), put back if the drag leaves without a drop.
    hover_saved: Option<Option<Vec<Message>>>,
    /// The last failed game's report, for Ctrl+C.
    failure_report: Option<String>,
    /// Held once used: on X11 the copied text lives in this process, and the
    /// last `Clipboard` dropped takes it away.
    clipboard: Option<arboard::Clipboard>,
    /// Path typed into the window (fallback when no file dialog is available).
    typed: String,
    /// A file the game was opened with (`--open`), taken as a drop once the
    /// window is open ([`Self::open_path`]).
    opened: Option<PathBuf>,
    modifiers: ModifiersState,
    /// Last cursor position, in surface pixels.
    cursor: Option<(f64, f64)>,
    game: Option<Running>,
    /// When the window last opened (input settles for a moment after).
    opened_at: Instant,
    screenshot: Option<PathBuf>,
    /// The connected controllers, opened with the window (none in a
    /// scripted capture): the D-pad moves the focus, Cross activates, as in
    /// the game's own menus ([`Self::pad_input`]).
    pad: Option<legaia_engine_shell::window::GamepadHost>,
    /// A held controller direction's auto-repeat.
    nav_repeat: NavRepeat,
    /// The Controls page's table and the key it may be waiting for. Read from
    /// [`launcher::INPUT_FILE`] when the page opens; every bound key is
    /// written straight back, the same file the game's Key Config writes.
    rebind: KeyRebindSession,
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
            Ok("recent") => Panel::Recent,
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
            recent_offer: None,
            found_disc: None,
            recent: Vec::new(),
            recent_lines: Vec::new(),
            recent_scroll: 0,
            expansion: settings
                .expansion
                .as_deref()
                .map(launcher::inspect_expansion),
            packs: launcher::discover_packs(&launcher::pack_dirs())
                .into_iter()
                .flatten()
                .filter(ExpansionInfo::playable)
                .collect(),
            card: settings.card.as_deref().map(launcher::inspect_card),
            settings,
            settings_file,
            dark: true,
            options,
            focus,
            left_at: (focus, None),
            panel,
            opt_scroll: 0,
            status: None,
            typed_tried: false,
            hover_saved: None,
            failure_report: None,
            clipboard: None,
            typed: String::new(),
            opened: None,
            modifiers: ModifiersState::empty(),
            cursor: None,
            game: None,
            opened_at: Instant::now(),
            rebind: KeyRebindSession::new(Mapping::load_or_default(Path::new(
                launcher::INPUT_FILE,
            ))),
            screenshot: std::env::var_os(SCREENSHOT_ENV).map(PathBuf::from),
            pad: None,
            nav_repeat: NavRepeat::default(),
        };
        app.set_disc(disc);
        app.refresh_recent_offer();
        if app.panel == Panel::Recent {
            app.build_recent();
        }
        // The first run only: a player who already has the dump need not
        // hunt for it. A disc used before is offered instead when one is
        // remembered, and a remembered disc that broke keeps its reason.
        if app.disc.is_none()
            && app.recent_offer.is_none()
            && matches!(app.problem, Some(DiscProblem::NotSet) | None)
        {
            app.found_disc =
                launcher::find_disc(&launcher::disc_search_dirs(), launcher::validate_disc);
            if let Some(found) = &app.found_disc {
                log::info!("launcher: found a disc image at {}", found.path.display());
            }
        }
        let offer = app
            .disc
            .is_none()
            .then_some(app.recent_offer.as_ref())
            .flatten();
        app.status = match (&app.problem, offer) {
            (Some(p), _) => Some(problem_status(p, offer)),
            (None, Some(o)) => Some(vec![(recent_offer_line(o), GOOD)]),
            (None, None) => None,
        }
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
        let rgba = (!self.dark).then(|| self.font.atlas_rgba_without_shadow());
        let (w, h) = self.font.atlas_dimensions();
        if let Some(r) = self.win.renderer() {
            let uploaded = match &rgba {
                // Black letter shadows smear into dark ink on a light card.
                Some(pixels) => r.upload_font_atlas(pixels, w, h),
                None => r.upload_font(&self.font),
            };
            match uploaded {
                Ok(a) => self.atlas = Some(a),
                Err(e) => log::error!("launcher font upload: {e:#}"),
            }
        }
    }

    /// Pick the palette from the *Menu colours* row and the desktop.
    fn resolve_theme(&mut self) {
        use winit::window::Theme;
        let window_dark = self
            .win
            .window
            .as_deref()
            .and_then(|w| w.theme())
            .map(|t| t == Theme::Dark);
        self.dark =
            legaia_app_theme::resolve_dark(self.options.ui_theme.forced_dark(), window_dark);
    }

    fn save_settings(&mut self) {
        if let Some(f) = &self.settings_file
            && let Err(e) = self.settings.save(f)
        {
            log::warn!("launcher: could not save settings ({e:#})");
            self.status = Some(vec![(format!("Could not save the settings: {e:#}"), WARN)]);
        }
    }

    fn save_options(&mut self) {
        if let Err(e) = self.options.save(Path::new(launcher::OPTIONS_FILE)) {
            self.status = Some(vec![(format!("Could not save the options: {e:#}"), WARN)]);
        }
    }

    /// Recompute [`Self::recent_offer`] after the disc or the list changed.
    fn refresh_recent_offer(&mut self) {
        self.recent_offer = launcher::next_recent_disc(&self.settings, launcher::validate_disc);
    }

    /// *Choose disc*'s second action: switch to the next remembered disc
    /// that still validates. The list keeps its order while stepping, so
    /// repeated steps visit each disc; *Play* moves the one played to the
    /// head.
    fn step_recent_disc(&mut self) {
        let Some(info) = self.recent_offer.take() else {
            self.status = Some(vec![(
                "No other disc has been chosen here yet. Choose disc... adds one; the last \
                 few are remembered."
                    .into(),
                DIM,
            )]);
            self.win.request_redraw();
            return;
        };
        log::info!("launcher: switched to recent disc {}", info.path.display());
        self.settings.disc = Some(info.path.clone());
        self.save_settings();
        self.problem = None;
        self.typed.clear();
        let name = file_name_of(&info.path);
        self.set_disc(Some(info));
        self.refresh_recent_offer();
        self.focus = Item::Play;
        self.status = Some(vec![(
            format!("Disc ready: {name}, chosen before. Press Enter to play."),
            GOOD,
        )]);
        self.win.request_redraw();
    }

    /// Take a dropped (or `--open`ed) file: an expansion pack, a memory
    /// card, else a disc image.
    fn open_path(&mut self, path: &Path) {
        if launcher::expansion_pack_path(path).is_some() {
            self.try_expansion(path);
        } else if launcher::is_card_file(path) {
            self.try_card(path);
        } else {
            self.try_disc(path);
        }
    }

    fn try_disc(&mut self, path: &Path) {
        match launcher::validate_disc(path) {
            Ok(info) => {
                self.settings.remember_disc(&info.path);
                self.save_settings();
                if let Some(f) = &self.settings_file {
                    log::info!("launcher: remembered {} in {}", path.display(), f.display());
                }
                self.problem = None;
                self.typed.clear();
                self.set_disc(Some(info));
                self.refresh_recent_offer();
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

    fn clipboard(&mut self) -> Result<&mut arboard::Clipboard, arboard::Error> {
        if self.clipboard.is_none() {
            self.clipboard = Some(arboard::Clipboard::new()?);
        }
        Ok(self.clipboard.as_mut().expect("just set"))
    }

    /// Ctrl+V: the clipboard's text into the disc path line. A file manager's
    /// "copy as path" brings its quotes, which `clean_typed_path` takes off.
    fn paste_path(&mut self) {
        match self.clipboard().and_then(|c| c.get_text()) {
            Ok(t) if !t.trim().is_empty() => {
                if std::mem::take(&mut self.typed_tried) {
                    self.typed.clear();
                }
                self.typed
                    .extend(t.trim().chars().filter(|c| !c.is_control()));
                self.status = None;
            }
            Ok(_) | Err(_) => {
                self.status = Some(vec![("The clipboard holds no text to paste.".into(), DIM)]);
            }
        }
    }

    /// Ctrl+C after a failed game: the report for a bug report.
    fn copy_failure_report(&mut self) {
        let Some(report) = self.failure_report.clone() else {
            return;
        };
        let done = self.clipboard().and_then(|c| c.set_text(report));
        self.status = Some(vec![match done {
            Ok(()) => (
                "Report copied. Paste it into your bug report before closing this window.".into(),
                GOOD,
            ),
            Err(e) => (format!("Could not copy the report: {e}"), WARN),
        }]);
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
            .map(Path::to_path_buf)
            .filter(|d| d.is_dir())
            .or_else(|| launcher::packs_dir().filter(|d| d.is_dir()))
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
                     or type or paste its full path (Ctrl+V) and press Enter."
                        .into(),
                    DIM,
                )]);
                self.win.request_redraw();
            }
        }
    }

    /// Adopt the expansion pack `chosen` names (a folder, its `pack.toml`, or
    /// an `.lxpack`) when it validates; a pack with errors is shown and not
    /// remembered.
    fn try_expansion(&mut self, chosen: &Path) {
        let Some(path) = launcher::expansion_pack_path(chosen) else {
            self.status = Some(vec![(
                format!(
                    "{} is not an expansion pack: choose an .lxpack file, or the pack.toml in a pack folder.",
                    chosen.display()
                ),
                WARN,
            )]);
            self.win.request_redraw();
            return;
        };
        match launcher::inspect_expansion(&path) {
            Ok(info) if info.playable() => {
                log::info!(
                    "launcher: expansion pack {} at {}",
                    info.summary(),
                    path.display()
                );
                let mut msg = vec![(format!("Expansion pack: {}", info.summary()), GOOD)];
                if info.warnings > 0 {
                    msg.push((
                        format!(
                            "{} warning(s); `asset expansion validate` lists them.",
                            info.warnings
                        ),
                        DIM,
                    ));
                }
                self.status = Some(msg);
                self.settings.expansion = Some(info.path.clone());
                self.expansion = Some(Ok(info));
                self.save_settings();
            }
            Ok(info) => {
                log::warn!("launcher: expansion pack at {} has errors", path.display());
                let mut msg = vec![(
                    format!(
                        "{} has {} error(s) and was not loaded:",
                        path.display(),
                        info.errors.len()
                    ),
                    WARN,
                )];
                msg.extend(info.errors.iter().take(4).map(|e| (e.clone(), DIM)));
                self.status = Some(msg);
            }
            Err(e) => {
                self.status = Some(vec![(
                    format!("Could not read {}: {e}", path.display()),
                    WARN,
                )]);
            }
        }
        self.win.request_redraw();
    }

    /// Play without an expansion pack from now on.
    fn clear_expansion(&mut self) {
        if self.settings.expansion.take().is_some() {
            self.expansion = None;
            self.save_settings();
            self.status = Some(vec![(
                "No expansion pack: the game as it shipped.".into(),
                DIM,
            )]);
        }
        self.win.request_redraw();
    }

    /// The Expansion button: the next found pack ([`Self::packs`]) after
    /// the chosen one, and past the last one (or with none found) the file
    /// dialog.
    fn next_expansion(&mut self) {
        let at = self.settings.expansion.as_deref().and_then(|cur| {
            self.packs
                .iter()
                .position(|p| p.path == cur || same_file(&p.path, cur))
        });
        let next = at.map_or(0, |i| i + 1);
        match self.packs.get(next).map(|p| p.path.clone()) {
            Some(path) => self.try_expansion(&path),
            None => self.browse_expansion(),
        }
    }

    fn browse_expansion(&mut self) {
        let mut dialog = rfd::FileDialog::new()
            .set_title("Choose an expansion pack (.lxpack, or pack.toml in a pack folder)")
            .add_filter("Expansion pack", &["lxpack", "LXPACK", "toml", "TOML"]);
        if let Some(dir) = self
            .settings
            .expansion
            .as_deref()
            .and_then(Path::parent)
            .map(Path::to_path_buf)
            .filter(|d| d.is_dir())
            .or_else(|| launcher::packs_dir().filter(|d| d.is_dir()))
        {
            dialog = dialog.set_directory(dir);
        }
        if let Some(w) = self.win.window.as_deref() {
            dialog = dialog.set_parent(w);
        }
        if let Some(p) = dialog.pick_file() {
            self.try_expansion(&p);
        }
    }

    /// Start the game and close the window while it runs.
    /// Start the game; `online` opens its Multiplayer screen once the
    /// field is up (`--multiplayer`).
    fn play(&mut self, evl: &ActiveEventLoop, online: bool, resume: Option<&Path>) {
        let Some(info) = self.disc.clone() else {
            self.status = Some(vec![(
                "Choose your Legend of Legaia (USA) disc image first.".into(),
                WARN,
            )]);
            self.focus = Item::ChooseDisc;
            return;
        };
        if let Some(problem) = self.expansion_problem() {
            self.status = Some(vec![(problem, WARN)]);
            self.focus = Item::Expansion;
            return;
        }
        if let Some(Err(why)) = &self.card {
            self.status = Some(vec![
                ("Card 2 cannot be used:".into(), WARN),
                (why.clone(), WARN),
                (CARD_FIX.into(), DIM),
            ]);
            self.focus = Item::DataFolder;
            return;
        }
        // The disc played becomes the newest of the remembered ones.
        if self.settings.recent_discs.first() != Some(&info.path) {
            self.settings.remember_disc(&info.path);
            self.save_settings();
            self.refresh_recent_offer();
        }
        match spawn_game(&info, &self.settings, online, resume) {
            Ok(g) => {
                log::info!("launcher: game started on {}", info.bin.display());
                self.game = Some(g);
                self.status = None;
                self.failure_report = None;
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
    /// Why the remembered expansion pack cannot be played, if it cannot.
    fn expansion_problem(&self) -> Option<String> {
        match self.expansion.as_ref()? {
            Ok(info) if info.playable() => None,
            Ok(info) => Some(format!(
                "The expansion pack has {} error(s): {} Choose it again or clear it (Delete).",
                info.errors.len(),
                info.errors.first().cloned().unwrap_or_default()
            )),
            Err(e) => Some(format!(
                "The expansion pack cannot be read ({e}). Choose it again or clear it (Delete)."
            )),
        }
    }

    fn close_window(&mut self) {
        self.atlas = None;
        self.remember_placement();
        if let Some(w) = self.win.window.as_deref() {
            w.set_visible(false);
        }
        self.win.renderer = None;
        self.win.window = None;
    }

    /// Remember where the window is, for the next start
    /// (`window_placement`); a screenshot run leaves the file alone.
    fn remember_placement(&self) {
        if self.screenshot.is_none()
            && let Some(w) = self.win.window.as_deref()
        {
            window_placement::remember(window_placement::Which::Launcher, w);
        }
    }

    fn open_window(&mut self, evl: &ActiveEventLoop) {
        let mut setup = legaia_engine_render::window::WindowSetup::default();
        // Where the window last closed, when that is still on a monitor; a
        // screenshot run opens where the desktop puts it.
        if self.screenshot.is_none() {
            setup.position = window_placement::restore(window_placement::Which::Launcher, evl);
        }
        if let Some((w, h)) = std::env::var(SIZE_ENV)
            .ok()
            .and_then(|v| v.split_once('x').map(|(w, h)| (w.parse(), h.parse())))
            .and_then(|(w, h)| Some((w.ok()?, h.ok()?)))
        {
            setup.logical_size = (w, h);
        }
        if !self.win.open_with(evl, TITLE, setup) {
            return;
        }
        if let Some(w) = self.win.window.as_deref() {
            w.set_min_inner_size(Some(winit::dpi::LogicalSize::new(
                STAGE_W as f64,
                (STAGE_H - HEADER_GIVE) as f64,
            )));
        }
        self.resolve_theme();
        self.atlas = None;
        self.upload_font();
        self.opened_at = Instant::now();
        if self.pad.is_none() && self.screenshot.is_none() {
            self.pad = legaia_engine_shell::window::GamepadHost::open();
        }
        if let Some(pad) = &mut self.pad {
            // A button still held from the game (the Start that quit it)
            // re-enters as a fresh press only after it is let go.
            let _ = pad.poll();
        }
        self.win.request_redraw();
    }

    /// Poll the controllers and act on the buttons pressed since the last
    /// poll, while the window is up: the D-pad (or left stick) moves the
    /// focus and steps an option, Cross activates, Triangle is the second
    /// action (a right-click), Circle backs out of the Controls page, Start
    /// plays, L1 / R1 page the option list.
    fn pad_input(&mut self, evl: &ActiveEventLoop) {
        let Some(pad) = &mut self.pad else { return };
        if self.win.window.is_none() {
            return;
        }
        // The table the Controls page edits, and the stick's dead zone.
        pad.set_mapping(&self.rebind.mapping().gamepad);
        pad.set_dead_zone(self.options.stick_dead_zone_fraction());
        let poll = pad.poll();
        let now = Instant::now();
        let pressed = poll.edges.pressed | poll.taps;
        // A held direction steps again, as a held arrow key does.
        let pressed = pressed | self.nav_repeat.step(poll.edges.held, now);
        evl.set_control_flow(ControlFlow::WaitUntil(now + Duration::from_millis(16)));
        for name in &poll.connected {
            self.status = Some(vec![(format!("Controller connected: {name}"), GOOD)]);
            self.win.request_redraw();
        }
        for name in &poll.disconnected {
            self.status = Some(vec![(format!("Controller disconnected: {name}"), DIM)]);
            self.win.request_redraw();
        }
        if let Some((_, glyphs)) = poll.capture {
            self.rebind
                .set_pad_style(glyphs, legaia_engine_system::gamepad::ButtonSpace::Standard);
        }
        if self.opened_at.elapsed() < Duration::from_millis(300) {
            return;
        }
        // A row waiting for its key takes a controller button too.
        if self.awaiting_key().is_some() {
            if let Some((name, _)) = poll.capture {
                self.bind_capture(name);
                self.win.request_redraw();
            }
            return;
        }
        if pressed == 0 {
            return;
        }
        for nav in pad_nav(pressed) {
            match nav {
                PadNav::Focus(d) => self.step_focus(d),
                PadNav::Horizontal(d) => self.horizontal(d, evl),
                PadNav::Page(d) => self.step_option(d * (VISIBLE_LINES as i32 - 2)),
                PadNav::Back => {
                    if self.panel != Panel::Options {
                        self.leave_page();
                    }
                }
                PadNav::Activate(d) => self.activate(self.focus, d, evl),
                PadNav::Play => {
                    if self.disc.is_some() {
                        self.activate(Item::Play, 1, evl);
                    }
                }
            }
        }
        self.win.request_redraw();
    }

    /// Left / Right (arrow keys, D-pad): step the focused option, or move
    /// between the columns.
    fn horizontal(&mut self, dir: i8, evl: &ActiveEventLoop) {
        match self.focus {
            Item::Opt(_) => self.activate(self.focus, dir, evl),
            Item::Bind(_) | Item::BindReset if dir < 0 => self.focus = Item::Panel,
            Item::Bind(_) | Item::BindReset => {}
            Item::RecentRow(_) if dir < 0 => self.focus = Item::Recent,
            Item::RecentRow(_) => {}
            _ if dir > 0 => self.switch_column(),
            _ => {}
        }
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
                // The disc going away under a running launcher (unplugged,
                // moved) is the common cause, and the disc card would
                // otherwise still say "ready": check it again.
                let disc_problem = self
                    .disc
                    .as_ref()
                    .and_then(|d| launcher::validate_disc(&d.path).err());
                let log_text = game
                    .log
                    .as_deref()
                    .and_then(|l| std::fs::read_to_string(l).ok());
                let msg = failure_message(
                    &status,
                    game.log.as_deref(),
                    log_text.as_deref(),
                    disc_problem.as_ref(),
                );
                self.failure_report = Some(failure_report(&status, log_text.as_deref()));
                // The game writes both files while it runs (its `F12`
                // panel: the options, the PlayStation pixel look): show what
                // it left, and do not write the values it had at Play back
                // over them on the next change here.
                self.options = OptionsState::load_or_default(Path::new(launcher::OPTIONS_FILE));
                if let Some(stored) = self
                    .settings_file
                    .as_deref()
                    .and_then(|f| LauncherSettings::load(f).ok())
                {
                    self.settings.switches = stored.switches;
                }
                if let Some(p) = disc_problem {
                    log::info!("launcher: after the failed start, {p}");
                    self.disc = None;
                    self.problem = Some(p);
                    self.focus = Item::ChooseDisc;
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
        // The game runs in the launcher's working directory (the data
        // directory, or where the launcher started when that could not be
        // made), so that is where its saves are.
        let dir = match std::env::current_dir() {
            Ok(d) => d,
            Err(e) => {
                self.status = Some(vec![(
                    format!("Could not tell which folder the saves are in: {e}"),
                    WARN,
                )]);
                return;
            }
        };
        // The path either way: a file manager may open behind this window,
        // or not at all on a desktop with none set.
        self.status = Some(vec![match open_folder(&dir) {
            Ok(()) => (format!("Saves and settings are in {}", dir.display()), DIM),
            Err(e) => (
                format!("Could not open a file manager on {}: {e}", dir.display()),
                WARN,
            ),
        }]);
    }

    /// Mount `path` as the game's second memory card when it reads as one.
    fn try_card(&mut self, path: &Path) {
        match launcher::inspect_card(path) {
            Ok(info) => {
                log::info!("launcher: memory card {}", info.path.display());
                self.settings.remember_card(&info.path);
                self.save_settings();
                self.status = Some(vec![(card_summary(&info), GOOD)]);
                self.card = Some(Ok(info));
            }
            Err(why) => {
                log::warn!("launcher: memory card {}: {why}", path.display());
                self.status = Some(vec![
                    ("Not a memory card the game can use:".into(), WARN),
                    (why, WARN),
                    (CARD_FIX.into(), DIM),
                ]);
            }
        }
        self.focus = Item::DataFolder;
        self.win.request_redraw();
    }

    /// Stop mounting the remembered memory card.
    fn clear_card(&mut self) {
        if self.settings.card.take().is_some() {
            self.card = None;
            self.save_settings();
            self.status = Some(vec![(
                "No memory card: the game uses only its own save card.".into(),
                DIM,
            )]);
        }
        self.win.request_redraw();
    }

    /// *Saves folder*'s second action: mount the next memory card used
    /// here before that still reads ([`launcher::next_recent_card`]), and
    /// past the last one, the file dialog.
    fn step_card(&mut self) {
        let Some(info) = launcher::next_recent_card(&self.settings, launcher::inspect_card) else {
            self.browse_card();
            return;
        };
        log::info!(
            "launcher: memory card {} (used before)",
            info.path.display()
        );
        self.settings.card = Some(info.path.clone());
        self.save_settings();
        self.status = Some(vec![(card_summary(&info), GOOD)]);
        self.card = Some(Ok(info));
        self.focus = Item::DataFolder;
        self.win.request_redraw();
    }

    fn browse_card(&mut self) {
        let mut dialog = rfd::FileDialog::new()
            .set_title("Choose a memory card from your emulator")
            .add_filter("PlayStation memory card", &launcher::CARD_EXTENSIONS)
            .add_filter("Any file", &["*"]);
        if let Some(dir) = self
            .settings
            .card
            .as_deref()
            .and_then(Path::parent)
            .filter(|d| d.is_dir())
        {
            dialog = dialog.set_directory(dir);
        }
        if let Some(w) = self.win.window.as_deref() {
            dialog = dialog.set_parent(w);
        }
        if let Some(p) = dialog.pick_file() {
            self.try_card(&p);
        }
    }

    /// Activate the focused control (`dir` steps an option backwards).
    fn activate(&mut self, item: Item, dir: i8, evl: &ActiveEventLoop) {
        self.focus = item;
        match item {
            Item::Play => self.play(evl, false, None),
            Item::Online => self.play(evl, true, None),
            Item::ChooseDisc if dir < 0 => self.step_recent_disc(),
            Item::ChooseDisc => match self.found_disc.clone() {
                Some(found) if self.disc.is_none() => self.try_disc(&found.path),
                _ => self.browse(),
            },
            Item::Expansion if dir < 0 => self.clear_expansion(),
            Item::Expansion => self.next_expansion(),
            Item::Panel => {
                self.panel = match self.panel {
                    Panel::Options => {
                        self.rebind = KeyRebindSession::new(Mapping::load_or_default(Path::new(
                            launcher::INPUT_FILE,
                        )));
                        Panel::Controls
                    }
                    Panel::Controls => Panel::Options,
                    Panel::Recent => {
                        self.rebind = KeyRebindSession::new(Mapping::load_or_default(Path::new(
                            launcher::INPUT_FILE,
                        )));
                        Panel::Controls
                    }
                };
            }
            Item::Recent => {
                if self.panel == Panel::Recent {
                    self.panel = Panel::Options;
                } else {
                    self.build_recent();
                    self.panel = Panel::Recent;
                }
            }
            Item::RecentRow(row) => self.activate_recent(row, dir, evl),
            Item::Bind(row) => {
                self.rebind.begin_bind(row);
                self.status = None;
            }
            Item::BindReset => {
                self.rebind = KeyRebindSession::new(Mapping::default());
                self.save_bindings("Every key is back on its default.".into());
            }
            Item::DataFolder if dir < 0 => self.step_card(),
            Item::DataFolder => self.open_data_folder(),
            Item::Quit => evl.exit(),
            Item::Opt(row) => {
                if self.panel != Panel::Options {
                    return;
                }
                row.adjust(&mut self.options, &mut self.settings, dir);
                self.option_changed(row);
            }
        }
        self.win.request_redraw();
    }

    /// The Controls page is waiting for the key of this rebind row.
    fn awaiting_key(&self) -> Option<u8> {
        match self.rebind.phase() {
            KeyRebindPhase::AwaitingKey { cursor } => Some(cursor),
            _ => None,
        }
    }

    /// A key arrived while the Controls page waits for one: Esc cancels, a
    /// key the game can bind takes the row, any other says why it cannot.
    fn capture_key(&mut self, physical: PhysicalKey) {
        let PhysicalKey::Code(code) = physical else {
            return;
        };
        if code == winit::keyboard::KeyCode::Escape {
            let _ = self.rebind.tick(KeyRebindInput {
                circle: true,
                ..Default::default()
            });
            self.status = None;
            return;
        }
        let name = legaia_engine_shell::window::keycode_to_name(code);
        if name.is_empty() {
            self.status = Some(vec![(UNBINDABLE_KEY.to_string(), WARN)]);
            return;
        }
        let before = self.rebind.mapping().clone();
        let events = self.rebind.tick(KeyRebindInput {
            key_pressed: Some(name.to_string()),
            ..Default::default()
        });
        for e in events {
            if let KeyRebindEvent::Bound { button, key, .. } = e {
                let done = bound_message(&before, self.rebind.mapping(), button.name(), &key);
                self.save_bindings(done);
            }
        }
    }

    /// A controller button arrived while the Controls page waits: it takes
    /// the row's controller slot (`KeyRebindSession`'s capture), under the
    /// keys' swap rule.
    fn bind_capture(&mut self, name: &str) {
        let events = self.rebind.tick(KeyRebindInput {
            key_pressed: Some(name.to_string()),
            ..Default::default()
        });
        for e in events {
            if let KeyRebindEvent::Bound {
                button,
                key,
                evicted,
            } = e
            {
                let mut done = format!(
                    "{} is on the controller's {key}.",
                    button_label(button.name())
                );
                if let Some(old) = evicted {
                    done.push_str(&format!(" The button that was there moved to {old}."));
                }
                self.save_bindings(done);
            }
        }
    }

    /// Write the table to the file the game reads, and say what happened.
    fn save_bindings(&mut self, done: String) {
        self.status = Some(vec![
            match self.rebind.mapping().save(Path::new(launcher::INPUT_FILE)) {
                Ok(()) => (done, GOOD),
                Err(e) => (format!("The keys could not be saved: {e:#}"), WARN),
            },
        ]);
    }

    /// Back from the Controls or Recent page to the options; a focus on one
    /// of its rows goes to the button that opened it.
    fn leave_page(&mut self) {
        self.panel = Panel::Options;
        match self.focus {
            Item::Bind(_) | Item::BindReset => self.focus = Item::Panel,
            Item::RecentRow(_) => self.focus = Item::Recent,
            _ => {}
        }
    }

    // --- The Recent page ------------------------------------------------

    /// Rebuild the Recent page's rows from the settings, the cards and the
    /// saves on them.
    fn build_recent(&mut self) {
        let mut rows = Vec::new();
        let mut lines = Vec::new();
        let mut row = |entry: RecentEntry, lines: &mut Vec<RecentLine>| {
            lines.push(RecentLine::Row(rows.len()));
            rows.push(entry);
        };
        // Saves: the game's own card, then card 2, each newest first as far
        // as a card can say. A card keeps no dates; the longest-played save
        // is the one a player left off on.
        let own = launcher::own_card_path();
        let mut groups: Vec<(&'static str, String, Vec<launcher::SaveRow>)> = vec![(
            "Continue your game",
            "your own card".to_string(),
            launcher::list_saves(&own),
        )];
        if let Some(card) = &self.settings.card {
            groups.push((
                "Continue from card 2",
                file_name_of(card),
                launcher::list_saves(card),
            ));
        }
        for (heading, card_name, mut saves) in groups {
            lines.push(RecentLine::Heading(heading));
            if saves.is_empty() {
                row(
                    RecentEntry::note(if heading == "Continue your game" {
                        "No saves yet."
                    } else {
                        "No Legend of Legaia saves on this card."
                    }),
                    &mut lines,
                );
            }
            saves.sort_by_key(|s| std::cmp::Reverse(s.play_time_seconds));
            for save in saves {
                let mut value = save.location.clone();
                let time = save.play_time();
                if !time.is_empty() {
                    if !value.is_empty() {
                        value.push(' ');
                    }
                    value.push_str(&time);
                }
                row(
                    RecentEntry {
                        label: format!("{} Lv {}", save.name, save.level),
                        value,
                        value_ink: DIM,
                        help: format!(
                            "Enter starts the game on this save, without the title screen. {} on \
                             {card_name}.",
                            save.number.map_or_else(
                                || format!("Block {}", save.cell + 1),
                                |n| format!("Save {n:02}")
                            ),
                        ),
                        kind: RecentKind::Save(save),
                    },
                    &mut lines,
                );
            }
        }
        // Discs.
        lines.push(RecentLine::Heading("Discs"));
        let mut discs: Vec<PathBuf> = self.settings.recent_discs.clone();
        if let Some(d) = self.settings.disc.as_ref().filter(|d| !discs.contains(d)) {
            discs.insert(0, d.clone());
        }
        // The disc in use leads; the rest keep newest-first.
        if let Some(cur) = self.disc.as_ref().map(|d| d.path.clone())
            && let Some(i) = discs.iter().position(|p| *p == cur)
        {
            let d = discs.remove(i);
            discs.insert(0, d);
        }
        if discs.is_empty() {
            row(RecentEntry::note("No disc chosen yet."), &mut lines);
        }
        for path in discs {
            let in_use = self.disc.as_ref().is_some_and(|d| d.path == path);
            let (value, ink, help) = if in_use {
                (
                    "In use".to_string(),
                    GOOD,
                    "The disc Play starts. Delete forgets it.".to_string(),
                )
            } else {
                match launcher::validate_disc(&path) {
                    Ok(_) => (
                        "Ready".to_string(),
                        INK,
                        "Enter switches to this disc. Delete forgets it.".to_string(),
                    ),
                    Err(p) => (
                        match p {
                            DiscProblem::Missing(_) | DiscProblem::Moved(_) => "Not found",
                            _ => "Not usable",
                        }
                        .to_string(),
                        WARN,
                        format!("{p} Delete forgets it."),
                    ),
                }
            };
            row(
                RecentEntry {
                    label: file_name_of(&path),
                    value,
                    value_ink: ink,
                    help: format!("{help} {}", path.display()),
                    kind: RecentKind::Disc(path),
                },
                &mut lines,
            );
        }
        // Memory cards.
        lines.push(RecentLine::Heading("Memory cards"));
        let mut cards: Vec<PathBuf> = self.settings.recent_cards.clone();
        if let Some(c) = self.settings.card.as_ref().filter(|c| !cards.contains(c)) {
            cards.insert(0, c.clone());
        }
        for path in cards {
            let mounted = self.settings.card.as_ref() == Some(&path);
            let (value, ink, help) = if mounted {
                (
                    "Card 2".to_string(),
                    GOOD,
                    "Mounted as card 2. Enter or Delete takes it out.".to_string(),
                )
            } else {
                match launcher::inspect_card(&path) {
                    Ok(info) => (
                        format!("{} saves", info.saves),
                        INK,
                        "Enter mounts this card as card 2. Delete forgets it.".to_string(),
                    ),
                    Err(why) => (
                        if path.exists() {
                            "Not usable"
                        } else {
                            "Not found"
                        }
                        .to_string(),
                        WARN,
                        format!("{why} Delete forgets it."),
                    ),
                }
            };
            row(
                RecentEntry {
                    label: file_name_of(&path),
                    value,
                    value_ink: ink,
                    help: format!("{help} {}", path.display()),
                    kind: RecentKind::Card(path),
                },
                &mut lines,
            );
        }
        row(
            RecentEntry {
                label: "Mount a memory card...".to_string(),
                value: String::new(),
                value_ink: DIM,
                help: "Choose an emulator memory card (.mcr, .mcd, .gme, .mcs). Its saves load \
                       and save in place, as card 2."
                    .to_string(),
                kind: RecentKind::NewCard,
            },
            &mut lines,
        );
        self.recent = rows;
        self.recent_lines = lines;
        self.recent_scroll = 0;
    }

    /// Enter, a click: use the row. Right-click, Shift+Enter: the row's
    /// Delete.
    fn activate_recent(&mut self, index: usize, dir: i8, evl: &ActiveEventLoop) {
        if dir < 0 {
            self.forget_recent(index);
            return;
        }
        let Some(entry) = self.recent.get(index) else {
            return;
        };
        match entry.kind.clone() {
            RecentKind::Note => {}
            RecentKind::Disc(path) => {
                if self.disc.as_ref().is_some_and(|d| d.path == path) {
                    self.status = Some(vec![("This disc is already in use.".into(), DIM)]);
                } else {
                    self.try_disc(&path);
                    self.after_recent_change(Item::Play);
                }
            }
            RecentKind::Card(path) => {
                if self.settings.card.as_ref() == Some(&path) {
                    self.clear_card();
                } else {
                    self.try_card(&path);
                }
                self.after_recent_change(Item::RecentRow(index));
            }
            RecentKind::NewCard => {
                self.browse_card();
                self.after_recent_change(Item::RecentRow(index));
            }
            RecentKind::Save(save) => {
                // In the data directory: a file the game is handed, not a save.
                let file =
                    std::path::absolute("continue.lgsf").unwrap_or_else(|_| "continue.lgsf".into());
                match launcher::write_continue_file(&save.card, save.cell, &file) {
                    Ok(()) => self.play(evl, false, Some(&file)),
                    Err(e) => {
                        self.status =
                            Some(vec![(format!("Could not read that save: {e:#}"), WARN)]);
                    }
                }
            }
        }
    }

    /// Delete, right-click: take a disc or a card off the remembered lists.
    fn forget_recent(&mut self, index: usize) {
        let Some(kind) = self.recent.get(index).map(|e| e.kind.clone()) else {
            return;
        };
        match kind {
            RecentKind::Disc(path) => {
                if self.disc.as_ref().is_some_and(|d| d.path == path) {
                    self.status = Some(vec![(
                        "This is the disc in use. Change disc... replaces it.".into(),
                        DIM,
                    )]);
                    return;
                }
                self.settings.recent_discs.retain(|p| *p != path);
                if self.settings.disc.as_ref() == Some(&path) {
                    self.settings.disc = None;
                }
                self.save_settings();
                self.refresh_recent_offer();
            }
            RecentKind::Card(path) => {
                if self.settings.card.as_ref() == Some(&path) {
                    self.clear_card();
                }
                self.settings.recent_cards.retain(|p| *p != path);
                self.save_settings();
            }
            RecentKind::Save(_) | RecentKind::NewCard | RecentKind::Note => return,
        }
        self.after_recent_change(Item::Recent);
    }

    /// Rows changed: rebuild them and keep the focus on `want` when it is
    /// still there, else on the page's button.
    fn after_recent_change(&mut self, want: Item) {
        self.build_recent();
        self.focus = match want {
            Item::RecentRow(i) if i >= self.recent.len() => Item::Recent,
            _ => want,
        };
    }

    /// The first row of the Recent page that does something.
    fn first_recent_row(&self) -> Item {
        self.recent
            .iter()
            .position(|e| e.kind != RecentKind::Note)
            .map_or(Item::Recent, Item::RecentRow)
    }

    /// Scroll the Recent page so the focused row is in view, with its
    /// heading when it opens a group.
    fn reveal_recent(&mut self) {
        let Item::RecentRow(row) = self.focus else {
            return;
        };
        let Some(idx) = self
            .recent_lines
            .iter()
            .position(|l| *l == RecentLine::Row(row))
        else {
            return;
        };
        let top = if idx > 0 && matches!(self.recent_lines[idx - 1], RecentLine::Heading(_)) {
            idx - 1
        } else {
            idx
        };
        let max = self.recent_lines.len().saturating_sub(VISIBLE_LINES);
        self.recent_scroll = if top < self.recent_scroll {
            top
        } else if idx >= self.recent_scroll + VISIBLE_LINES {
            idx + 1 - VISIBLE_LINES
        } else {
            self.recent_scroll
        }
        .min(max);
    }

    /// `Delete` (or `Backspace` with nothing typed) on an option row: put
    /// that row back on its first-start value.
    fn reset_focused(&mut self) {
        if self.focus == Item::Expansion {
            self.clear_expansion();
            return;
        }
        if self.focus == Item::DataFolder {
            self.clear_card();
            return;
        }
        if let Item::RecentRow(i) = self.focus {
            self.forget_recent(i);
            return;
        }
        if let Item::Opt(row) = self.focus
            && self.panel == Panel::Options
            && !row.is_default(&self.options, &self.settings)
        {
            row.reset(&mut self.options, &mut self.settings);
            self.option_changed(row);
        }
    }

    /// Log and store a row the player just changed.
    fn option_changed(&mut self, row: OptionRow) {
        log::info!(
            "launcher: {} -> {}",
            row.label(),
            row.value(&self.options, &self.settings)
        );
        self.status = None;
        if row == OptionRow::Theme {
            self.resolve_theme();
            self.atlas = None;
            self.upload_font();
        }
        if row == OptionRow::Preset {
            // One switch over both files.
            self.save_options();
            self.save_settings();
        } else if row.is_game_option() {
            self.save_options();
        } else {
            self.save_settings();
        }
    }

    /// Every focusable control in Up / Down order.
    fn order(&self) -> Vec<Item> {
        let mut v = BUTTONS.to_vec();
        match self.panel {
            Panel::Options => v.extend(OPTION_ROWS.iter().map(|(_, r)| Item::Opt(*r))),
            Panel::Controls => {
                v.extend((0..BIND_ROWS).map(Item::Bind));
                v.push(Item::BindReset);
            }
            Panel::Recent => v.extend(
                self.recent
                    .iter()
                    .enumerate()
                    .filter(|(_, e)| e.kind != RecentKind::Note)
                    .map(|(i, _)| Item::RecentRow(i)),
            ),
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
        if self.panel == Panel::Recent {
            self.reveal_recent();
        }
        let Item::Opt(row) = self.focus else { return };
        let lines = option_lines();
        if let Some(idx) = lines.iter().position(|l| *l == Line::Row(row)) {
            self.opt_scroll = scroll_to_reveal(&lines, self.opt_scroll, idx);
        }
    }

    /// Scroll the option list by `lines` (negative is up), clamped.
    fn scroll_options(&mut self, lines: i32) {
        if self.panel == Panel::Recent {
            let max = self.recent_lines.len().saturating_sub(VISIBLE_LINES) as i32;
            self.recent_scroll = (self.recent_scroll as i32 + lines).clamp(0, max) as usize;
            return;
        }
        let max = option_lines().len().saturating_sub(VISIBLE_LINES) as i32;
        self.opt_scroll = (self.opt_scroll as i32 + lines).clamp(0, max) as usize;
    }

    /// Tab: jump between the button column and the option rows, back to
    /// where the focus was in that column when it last left it (the first
    /// option or key row, and *Play*, before then).
    fn switch_column(&mut self) {
        let order = self.order();
        let usable = |i: &Item| order.contains(i);
        self.focus = match self.focus {
            right @ (Item::Opt(_) | Item::Bind(_) | Item::BindReset | Item::RecentRow(_)) => {
                self.left_at.1 = Some(right);
                Some(self.left_at.0)
                    .filter(usable)
                    .or_else(|| order.iter().copied().find(|i| BUTTONS.contains(i)))
                    .unwrap_or(Item::Play)
            }
            button => {
                self.left_at.0 = button;
                let first = match self.panel {
                    Panel::Options => Item::Opt(OPTION_ROWS[0].1),
                    Panel::Controls => Item::Bind(0),
                    Panel::Recent => self.first_recent_row(),
                };
                // A row of the other page (the panel switched since) or one
                // that is gone does not count.
                self.left_at.1.filter(usable).unwrap_or(first)
            }
        };
        self.reveal_focus();
    }

    // --- Drawing --------------------------------------------------------

    fn stage(&self) -> Stage {
        stage_for(self.win.surface_size())
    }

    /// Lay the screen out; `hits` gets each control's stage rectangle.
    fn layout(&self, c: &mut Canvas, hits: &mut Vec<(Item, Rect)>) {
        // Header band.
        c.fill_surface(BODY);
        // The header band runs edge to edge and up to the window's top: the
        // stage scales by whole steps, so a large window centres it with
        // margins, and a band drawn only across the stage stopped short of
        // them (and floated under a dark strip above). A squeezed stage
        // starts `sq` rows above the surface, so the title moves down half
        // as far as the band shrinks.
        let sq = c.stage.squeeze;
        c.band_to_top(HEADER_H, BAND);
        c.full_width(HEADER_H, 1, RULE);
        c.text_scaled(PAD, 7 + sq / 2, "Legend of Legaia", GOLD, 2);
        let sub = format!("engine port {}", env!("CARGO_PKG_VERSION"));
        let w = c.width(&sub);
        c.text(STAGE_W - PAD - w, 20 + sq / 2, &sub, FAINT);

        self.disc_card(c);

        // Left column: the buttons.
        let expansion_label = match &self.expansion {
            None => "Expansion pack...".to_string(),
            Some(Ok(info)) if info.playable() => {
                // A name reads from its start: a long one keeps its head.
                c.ellipsize_right(&format!("Pack: {}", info.short_name), LEFT_W - 12)
            }
            Some(_) => "Pack: has errors".to_string(),
        };
        for (i, item) in BUTTONS.iter().enumerate() {
            let r = button_rect(i);
            hits.push((*item, r));
            let focused = self.focus == *item;
            let enabled = !matches!(item, Item::Play | Item::Online) || self.disc.is_some();
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
                Item::Online => "Play online",
                Item::ChooseDisc => {
                    if self.disc.is_some() {
                        "Change disc..."
                    } else if self.found_disc.is_some() {
                        "Use this disc"
                    } else {
                        "Choose disc..."
                    }
                }
                Item::Panel => match self.panel {
                    Panel::Controls => "Options",
                    _ => "Controls",
                },
                Item::Recent => match self.panel {
                    Panel::Recent => "Options",
                    _ => "Recent",
                },
                Item::Expansion => &expansion_label,
                Item::DataFolder => "Saves folder",
                Item::Quit => "Quit",
                Item::Opt(_) | Item::Bind(_) | Item::BindReset | Item::RecentRow(_) => "",
            };
            let label = fit_button_label(c, label, r.w);
            c.text_centered(r, &label, ink);
        }

        // Left column, below the buttons: help for the focused control.
        let help = help_rect();
        c.rect(help, CARD);
        let lines = self.help_lines();
        let hl = help_layout(c, &lines);
        let mut y = help.y + 6;
        'outer: for (wrapped, ink) in &hl.lines {
            for l in wrapped {
                if y + 12 > hl.bottom {
                    break 'outer;
                }
                c.text(help.x + 6, y, l, *ink);
                y += 12;
            }
            y += 3;
        }
        let footer = hl.footer;
        if footer {
            c.text(
                help.x + 6,
                help.y + help.h - 14,
                if self.failure_report.is_some() {
                    COPY_REPORT_HINT
                } else {
                    "Arrows, Enter, Tab, Esc"
                },
                FAINT,
            );
        }

        // Right column.
        let panel = panel_rect();
        c.rect(panel, CARD);
        match self.panel {
            Panel::Options => self.options_panel(c, panel, hits),
            Panel::Controls => self.controls_panel(c, panel, hits),
            Panel::Recent => self.recent_panel(c, panel, hits),
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
        let (accent, head) = match (&self.disc, &self.found_disc) {
            (None, Some(_)) => (GOLD, "Found your disc image".to_string()),
            _ => (accent, head),
        };
        c.rect(Rect::new(card.x, card.y, 3, card.h), accent);
        c.text(card.x + 10, card.y + 6, "Disc", FAINT);
        let tx = card.x + 10 + c.width("Disc") + 10;
        c.text(tx, card.y + 6, &head, accent);
        let inner = card.x + card.w - 8 - tx;
        let (line, ink) = if !self.typed.is_empty() {
            (format!("Path: {}_", self.typed), GOLD)
        } else if let Some(d) = self.disc.as_ref().or(self.found_disc.as_ref()) {
            (d.path.display().to_string(), DIM)
        } else {
            match &self.problem {
                Some(DiscProblem::NotSet) | None => (FIRST_RUN_CARD.into(), DIM),
                Some(p) => match problem_path(p) {
                    Some(path) => (path.display().to_string(), DIM),
                    None => (p.to_string(), WARN),
                },
            }
        };
        // A path keeps its end (the file name); a sentence wraps.
        let is_path = !self.typed.is_empty()
            || self.disc.is_some()
            || self.found_disc.is_some()
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
                let folder = short_folder(dir);
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
        let retail = self.settings.switches.retail;
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
            // The one value-ink rule the play window's settings page uses
            // too (`chrome::option_value`).
            let vink = if muted {
                FAINT
            } else {
                legaia_app_theme::chrome::option_value(&value, row.original_value().as_deref())
            };
            let shown = if focused {
                format!("< {value} >")
            } else {
                value
            };
            let w = c.width(&shown);
            let vx = panel.x + panel.w - 16 - w;
            c.text(vx, y, &shown, vink);
            // A row moved off its first-start value carries a gold dot
            // before its value, so a player can see at a glance what they
            // changed (`Delete` puts it back).
            if !muted && !row.is_default(&self.options, &self.settings) {
                c.rect(Rect::new(vx - 8, y + 4, 3, 3), GOLD);
            }
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

    fn controls_panel(&self, c: &mut Canvas, panel: Rect, hits: &mut Vec<(Item, Rect)>) {
        let mapping = self.rebind.mapping();
        let waiting = self.awaiting_key();
        let mut y = panel.y + LIST_INSET;
        for (i, row) in self
            .rebind
            .rows()
            .iter()
            .take(usize::from(BIND_ROWS))
            .enumerate()
        {
            let item = Item::Bind(i as u8);
            let r = Rect::new(panel.x + 4, y - 1, panel.w - 8, ROW_PITCH);
            hits.push((item, r));
            let focused = self.focus == item;
            if focused {
                c.rect(r, FOCUS);
                c.rect(Rect::new(r.x, r.y, 2, r.h), GOLD);
            }
            c.text(panel.x + 10, y, button_label(row.button.name()), INK);
            // Key names take the key ink the play window's F12 panel and
            // Explorer's shortcut list use (`chrome::KEY`); gold is for
            // headings and values here.
            let (shown, ink) = if waiting == Some(i as u8) {
                ("Press a key or button...".to_string(), GOLD)
            } else {
                // The keys, then the controller button (in the labels of the
                // controller pressed last).
                let pad = if row.pad.is_empty() {
                    "no button"
                } else {
                    row.pad.as_str()
                };
                match keys_for_button(mapping, row.button.name()) {
                    k if k.is_empty() => (format!("No key  |  {pad}"), WARN),
                    k => (format!("{k}  |  {pad}"), KEY),
                }
            };
            let shown = if focused && waiting.is_none() {
                format!("< {shown} >")
            } else {
                shown
            };
            let w = c.width(&shown);
            c.text(panel.x + panel.w - 10 - w, y, &shown, ink);
            y += ROW_PITCH;
        }
        let r = Rect::new(panel.x + 4, y - 1, panel.w - 8, ROW_PITCH);
        hits.push((Item::BindReset, r));
        if self.focus == Item::BindReset {
            c.rect(r, FOCUS);
            c.rect(Rect::new(r.x, r.y, 2, r.h), GOLD);
        }
        c.text(panel.x + 10, y, "All keys and buttons back to default", INK);
        y += ROW_PITCH;
        for (i, l) in controls_note_lines(c, panel).into_iter().enumerate() {
            c.text(panel.x + 10, y + 4 + i as i32 * 12, &l, DIM);
        }
    }

    fn recent_panel(&self, c: &mut Canvas, panel: Rect, hits: &mut Vec<(Item, Rect)>) {
        let scroll = self
            .recent_scroll
            .min(self.recent_lines.len().saturating_sub(VISIBLE_LINES));
        let mut y = panel.y + LIST_INSET;
        for line in self.recent_lines.iter().skip(scroll).take(VISIBLE_LINES) {
            let index = match *line {
                RecentLine::Heading(h) => {
                    c.rect(Rect::new(panel.x + 4, y, panel.w - 14, ROW_PITCH - 1), BAND);
                    c.text(panel.x + 8, y, h, GOLD);
                    y += ROW_PITCH;
                    continue;
                }
                RecentLine::Row(i) => i,
            };
            let entry = &self.recent[index];
            let item = Item::RecentRow(index);
            let r = Rect::new(panel.x + 4, y - 1, panel.w - 14, ROW_PITCH);
            let focused = self.focus == item;
            if entry.kind != RecentKind::Note {
                hits.push((item, r));
                if focused {
                    c.rect(r, FOCUS);
                    c.rect(Rect::new(r.x, r.y, 2, r.h), GOLD);
                }
            }
            let ink = if entry.kind == RecentKind::Note {
                FAINT
            } else {
                INK
            };
            // The shorter side keeps its whole width; the longer one is cut
            // in the middle, so a file name keeps its end and a save row
            // its play time.
            let room = panel.w - 24;
            let gap = 8;
            let (lw, vw) = (c.width(&entry.label), c.width(&entry.value));
            let (label_room, value_room) = if lw + vw + gap <= room {
                (lw, vw)
            } else if vw <= room / 2 {
                (room - vw - gap, vw)
            } else if lw <= room / 2 {
                (lw, room - lw - gap)
            } else {
                (room / 2 - gap / 2, room / 2 - gap / 2)
            };
            let label = c.ellipsize_middle(&entry.label, label_room);
            let value = c.ellipsize_middle(&entry.value, value_room);
            c.text(panel.x + 10, y, &label, ink);
            let vw = c.width(&value);
            c.text(panel.x + panel.w - 14 - vw, y, &value, entry.value_ink);
            y += ROW_PITCH;
        }
        if self.recent_lines.len() > VISIBLE_LINES {
            let track = Rect::new(
                panel.x + panel.w - 6,
                panel.y + LIST_INSET,
                2,
                VISIBLE_LINES as i32 * ROW_PITCH,
            );
            c.rect(track, RULE);
            let n = self.recent_lines.len() as i32;
            let thumb_h = (track.h * VISIBLE_LINES as i32 / n).max(8);
            let max = n - VISIBLE_LINES as i32;
            let thumb_y = track.y + (track.h - thumb_h) * scroll as i32 / max.max(1);
            c.rect(Rect::new(track.x, thumb_y, track.w, thumb_h), DIM);
        }
    }

    fn help_text(&self) -> String {
        match self.focus {
            Item::Play => {
                if self.disc.is_some() {
                    "Start the game at the title screen. In game, F12 or Select+Start \
                     opens these options. Values in green differ from the original game."
                        .into()
                } else {
                    "Choose your disc image first.".into()
                }
            }
            Item::ChooseDisc if self.disc.is_none() && self.found_disc.is_some() => {
                let found = self.found_disc.as_ref().expect("just checked");
                format!(
                    "Enter uses {}, found in your folders. Ctrl+O or Shift+Enter picks another.",
                    file_name_of(&found.path)
                )
            }
            Item::ChooseDisc => match (&self.disc, &self.recent_offer) {
                (Some(_), Some(offer)) => change_disc_help(offer),
                (Some(d), None) => {
                    format!("Pick another disc image. Now: {}", d.path.display())
                }
                (None, Some(offer)) => choose_disc_help(offer),
                (None, None) => FIRST_RUN_HELP.into(),
            },
            Item::Online => "Start on the Multiplayer screen: host a room for your network, \
                             or join one by its address and room code. Tab opens it in play."
                .into(),
            Item::Expansion => match &self.expansion {
                None => match self.packs.as_slice() {
                    [] => "Packs add maps, quests and fights made for the port. Choose an \
                           .lxpack file, or the pack.toml in a pack folder."
                        .into(),
                    [one] => format!(
                        "Packs add maps, quests and fights made for the port. Enter picks {}, \
                         which came with the game; again, a file.",
                        one.name
                    ),
                    many => format!(
                        "Packs add maps, quests and fights made for the port. Enter steps \
                         through the {} found, then offers a file.",
                        many.len()
                    ),
                },
                Some(Ok(info)) if info.playable() => format!(
                    "{}. New Game and Continue play with it. Enter: {}. Delete or right-click: \
                     none.",
                    info.summary(),
                    if self.packs.len() > 1 {
                        "the next pack"
                    } else {
                        "choose another file"
                    }
                ),
                Some(_) => self.expansion_problem().unwrap_or_default(),
            },
            Item::Panel => match self.panel {
                Panel::Controls => "Back to the options. Right or Tab goes to the keys. In the \
                                    game, F12 lists the toggle keys too."
                    .into(),
                _ => "See and change the keyboard keys. In the game, F12 or Select+Start opens \
                      settings, cheats, scenes, tools and the key list."
                    .into(),
            },
            Item::Recent => "Discs and memory cards used before, and the saves on them: \
                             continue a save without the title screen."
                .into(),
            Item::RecentRow(i) => self
                .recent
                .get(i)
                .map(|e| e.help.clone())
                .unwrap_or_default(),
            Item::Bind(row) => match self.awaiting_key() {
                Some(_) => waiting_message(self.rebind_button_label(row)),
                None => format!(
                    "Enter, Cross or click, then press the key or controller button for {}. \
                     One another button is on swaps with it.",
                    self.rebind_button_label(row)
                ),
            },
            Item::BindReset => {
                "Put every button back on the key and controller button it starts on.".into()
            }
            Item::DataFolder => {
                data_folder_help(self.card.as_ref(), self.settings.other_recent_cards())
            }
            Item::Quit => "Close the launcher. Esc.".into(),
            Item::Opt(row) => {
                let mut s = row.help().to_string();
                if self.settings.switches.retail && row.overridden_by_retail() {
                    s.push_str(" Off while Original game only is on.");
                }
                s
            }
        }
    }

    /// The name of rebind row `row`'s button.
    fn rebind_button_label(&self, row: u8) -> &'static str {
        self.rebind
            .rows()
            .get(usize::from(row))
            .map_or("", |r| button_label(r.button.name()))
    }

    /// The help box's lines: a status when one outranks the focus, else the
    /// focused control's help, and under a changed option row the value it
    /// started on and the key that restores it.
    fn help_lines(&self) -> Vec<Message> {
        if let Some(s) = &self.status {
            return s.clone();
        }
        // A path being typed: the card shows it, the box says what the keys
        // do with it (the focused button's help would not).
        if !self.typed.is_empty() {
            return vec![(TYPED_PATH_HELP.into(), DIM)];
        }
        let mut lines = vec![(self.help_text(), DIM)];
        if let Item::Opt(row) = self.focus
            && self.panel == Panel::Options
        {
            if !row.is_default(&self.options, &self.settings) {
                // The default note names the original's value too.
                lines.push((default_note(row), GOLD));
            } else if !(self.settings.switches.retail && row.overridden_by_retail())
                && let Some(note) = original_note(row, &self.options, &self.settings)
            {
                lines.push(note);
            }
        }
        lines
    }

    fn redraw(&mut self, evl: &ActiveEventLoop) {
        let Some(atlas) = self.atlas.as_ref() else {
            return;
        };
        let stage = self.stage();
        let mut canvas = Canvas {
            palette: legaia_app_theme::palette(self.dark),
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

    /// Whether the cursor is on the help box: after a failed game a click
    /// there copies the report, for a player with no hand on the keyboard.
    fn cursor_on_help(&self) -> bool {
        let Some((cx, cy)) = self.cursor else {
            return false;
        };
        let st = self.stage();
        let x = ((cx as i32) - st.origin.0).div_euclid(st.scale);
        let y = ((cy as i32) - st.origin.1).div_euclid(st.scale);
        help_rect().contains(x, y)
    }

    /// The control under the cursor, if any.
    fn hit(&self) -> Option<Item> {
        let (cx, cy) = self.cursor?;
        let st = self.stage();
        let x = ((cx as i32) - st.origin.0).div_euclid(st.scale);
        let y = ((cy as i32) - st.origin.1).div_euclid(st.scale);
        // Re-run the layout to get the rectangles (cheap: a few hundred quads).
        let mut canvas = Canvas {
            palette: legaia_app_theme::palette(self.dark),
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
        if self.win.window.is_some()
            && let Some(path) = self.opened.take()
        {
            self.open_path(&path);
        }
    }

    fn about_to_wait(&mut self, evl: &ActiveEventLoop) {
        self.poll_game(evl);
        self.pad_input(evl);
    }

    fn exiting(&mut self, _evl: &ActiveEventLoop) {
        self.remember_placement();
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
            // While a file is dragged over the window, the help box says
            // what a drop will do with it; leaving puts the old text back.
            WindowEvent::HoveredFile(p) => {
                if self.hover_saved.is_none() {
                    self.hover_saved = Some(self.status.take());
                }
                self.status = Some(vec![(drop_hint(&p), GOOD)]);
                self.win.request_redraw();
            }
            WindowEvent::HoveredFileCancelled => {
                if let Some(old) = self.hover_saved.take() {
                    self.status = old;
                }
                self.win.request_redraw();
            }
            WindowEvent::DroppedFile(p) => {
                self.hover_saved = None;
                self.open_path(&p);
            }
            WindowEvent::ModifiersChanged(m) => self.modifiers = m.state(),
            // A scripted capture shows the scripted focus, not whatever row
            // the display's resting pointer happens to sit over.
            WindowEvent::CursorMoved { .. } if self.screenshot.is_some() => {}
            WindowEvent::CursorMoved { position, .. } => {
                let at = (position.x, position.y);
                // The first report is where the pointer already was. A window
                // opening under a resting pointer must keep *Play* focused, or
                // the Enter that follows flips whichever option it landed on.
                let moved = self.cursor.is_some_and(|c| c != at);
                self.cursor = Some(at);
                if moved
                    && self.awaiting_key().is_none()
                    && let Some(item) = self.hit()
                    && item != self.focus
                {
                    self.focus = item;
                    self.win.request_redraw();
                }
            }
            WindowEvent::CursorLeft { .. } => self.cursor = None,
            WindowEvent::MouseWheel { delta, .. } if self.panel != Panel::Controls => {
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
                // A click while the Controls page waits for a key calls it off.
                if self.awaiting_key().is_some() {
                    self.capture_key(PhysicalKey::Code(winit::keyboard::KeyCode::Escape));
                    self.win.request_redraw();
                    return;
                }
                let dir = match button {
                    MouseButton::Left => 1,
                    MouseButton::Right => -1,
                    // A middle click on an option is its `Delete`.
                    MouseButton::Middle => {
                        if let Some(item @ Item::Opt(_)) = self.hit() {
                            self.focus = item;
                            self.reset_focused();
                            self.win.request_redraw();
                        }
                        return;
                    }
                    _ => return,
                };
                log::debug!("launcher: {button:?} click at {:?}", self.cursor);
                if dir > 0 && self.failure_report.is_some() && self.cursor_on_help() {
                    self.copy_failure_report();
                    self.win.request_redraw();
                    return;
                }
                if let Some(item) = self.hit()
                    && (dir > 0
                        || matches!(
                            item,
                            Item::Opt(_)
                                | Item::Expansion
                                | Item::ChooseDisc
                                | Item::DataFolder
                                | Item::RecentRow(_)
                        ))
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
                if self.awaiting_key().is_some() {
                    if !event.repeat {
                        self.capture_key(event.physical_key);
                        self.win.request_redraw();
                    }
                    return;
                }
                let typing = !self.typed.is_empty();
                match &event.logical_key {
                    Key::Named(NamedKey::Escape) => {
                        if typing {
                            self.typed.clear();
                            self.typed_tried = false;
                        } else if self.panel != Panel::Options {
                            self.leave_page();
                        } else {
                            evl.exit();
                        }
                    }
                    Key::Named(NamedKey::Enter) => {
                        if typing {
                            let p = PathBuf::from(clean_typed_path(&self.typed));
                            self.try_disc(&p);
                            self.typed_tried = !self.typed.is_empty();
                        } else {
                            // Shift+Enter is the right-click: the second
                            // action (the previous disc, a value back).
                            let dir = if self.modifiers.shift_key() { -1 } else { 1 };
                            self.activate(self.focus, dir, evl);
                        }
                    }
                    Key::Named(NamedKey::Space) if !typing => self.activate(self.focus, 1, evl),
                    Key::Named(NamedKey::Backspace) if typing => {
                        self.typed_tried = false;
                        self.typed.pop();
                    }
                    Key::Named(NamedKey::Delete | NamedKey::Backspace) => self.reset_focused(),
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
                        self.horizontal(dir, evl);
                    }
                    Key::Character(c)
                        if !typing
                            && (self.modifiers.control_key() || self.modifiers.super_key())
                            && c.eq_ignore_ascii_case("o") =>
                    {
                        self.browse();
                    }
                    Key::Character(c)
                        if (self.modifiers.control_key() || self.modifiers.super_key())
                            && c.eq_ignore_ascii_case("v") =>
                    {
                        self.paste_path();
                    }
                    Key::Character(c)
                        if (self.modifiers.control_key() || self.modifiers.super_key())
                            && c.eq_ignore_ascii_case("c") =>
                    {
                        self.copy_failure_report();
                    }
                    // On the key table a stray letter is a player looking for
                    // a rebind, not the start of a disc path.
                    _ if matches!(self.focus, Item::Bind(_) | Item::BindReset) => {}
                    _ => {
                        if !(self.modifiers.control_key() || self.modifiers.super_key())
                            && let Some(t) = &event.text
                        {
                            if std::mem::take(&mut self.typed_tried) {
                                self.typed.clear();
                            }
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
    /// Stage rows the header band gives up ([`HEADER_GIVE`] at most): the
    /// stage's top `squeeze` rows sit above the surface.
    squeeze: i32,
}

/// The largest whole-step stage that fits a `(w, h)` surface, centred. A
/// surface up to [`HEADER_GIVE`] stage rows short of a step still takes it,
/// with the header band that much shorter ([`Stage::squeeze`]); everything
/// under the header keeps its size, so every fit the layout tests measure
/// holds at any squeeze.
fn stage_for((w, h): (u32, u32)) -> Stage {
    let (w, h) = (w as i32, h as i32);
    let scale = (w / STAGE_W).min(h / (STAGE_H - HEADER_GIVE)).max(1);
    let squeeze = (STAGE_H - h / scale).clamp(0, HEADER_GIVE);
    let shown = STAGE_H - squeeze;
    let origin = (
        (w - STAGE_W * scale) / 2,
        (h - shown * scale) / 2 - squeeze * scale,
    );
    Stage {
        scale,
        origin,
        surface: (w as u32, h as u32),
        squeeze,
    }
}

/// Quads in stage coordinates, emitted in surface pixels.
struct Canvas<'a> {
    /// The palette every role resolves in.
    palette: &'static legaia_app_theme::Palette,
    font: &'a legaia_font::Font,
    solid: (u32, u32, u32, u32),
    /// Set the built-in font 6 px a character instead of its 8 (its glyphs
    /// are 5 px wide; the wider pitch is for the game HUD's fallback).
    condense: bool,
    stage: Stage,
    draws: Vec<TextDraw>,
}

impl Canvas<'_> {
    fn fill_surface(&mut self, color: Role) {
        self.draws.push(TextDraw {
            dst: (0, 0, self.stage.surface.0, self.stage.surface.1),
            src: self.solid,
            color: self.palette.get(color).to_f32(),
        });
    }

    /// A band `h` stage rows tall at stage row `y`, across the whole
    /// surface (the margins beside the stage included).
    fn full_width(&mut self, y: i32, h: i32, color: Role) {
        let s = self.stage.scale;
        let top = self.stage.origin.1 + y * s;
        let bottom = top + h.max(0) * s;
        self.draws.push(TextDraw {
            dst: (
                0,
                top.max(0),
                self.stage.surface.0,
                (bottom - top.max(0)).max(0) as u32,
            ),
            src: self.solid,
            color: self.palette.get(color).to_f32(),
        });
    }

    /// A band from the surface's top edge down to stage row `bottom`,
    /// across the whole surface: the header in a window taller than the
    /// stage reaches the top instead of floating under a dark margin.
    fn band_to_top(&mut self, bottom: i32, color: Role) {
        let h = self.stage.origin.1 + bottom * self.stage.scale;
        self.draws.push(TextDraw {
            dst: (0, 0, self.stage.surface.0, h.max(0) as u32),
            src: self.solid,
            color: self.palette.get(color).to_f32(),
        });
    }

    fn rect(&mut self, r: Rect, color: Role) {
        let s = self.stage.scale;
        self.draws.push(TextDraw {
            dst: (
                self.stage.origin.0 + r.x * s,
                self.stage.origin.1 + r.y * s,
                (r.w.max(0) * s) as u32,
                (r.h.max(0) * s) as u32,
            ),
            src: self.solid,
            color: self.palette.get(color).to_f32(),
        });
    }

    fn outline(&mut self, r: Rect, color: Role) {
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

    fn text(&mut self, x: i32, y: i32, text: &str, color: Role) {
        self.text_scaled(x, y, text, color, 1);
    }

    fn text_scaled(&mut self, x: i32, y: i32, text: &str, color: Role, k: i32) {
        let s = self.stage.scale;
        let layout = self.font.layout_ascii(&Self::clean(text));
        for d in legaia_engine_ui::text_draws_for(&layout, (0, 0), self.palette.get(color).to_f32())
        {
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

    fn text_centered(&mut self, r: Rect, text: &str, color: Role) {
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

    /// `text` cut to `max_w` pixels by dropping characters from its end
    /// behind `...` - for a name, whose start says what it is.
    /// `text` cut in the middle to `max_w`: the end of a file name (a seed,
    /// `(Track 2)`, a translation's tag) is usually what tells two apart.
    fn ellipsize_middle(&self, text: &str, max_w: i32) -> String {
        if self.width(text) <= max_w {
            return text.to_string();
        }
        let chars: Vec<char> = text.chars().collect();
        for keep in (2..chars.len()).rev() {
            let head = keep.div_ceil(2);
            let tail = keep - head;
            let shown: String = chars[..head]
                .iter()
                .chain(std::iter::once(&'.'))
                .chain(std::iter::once(&'.'))
                .chain(std::iter::once(&'.'))
                .chain(chars[chars.len() - tail..].iter())
                .collect();
            if self.width(&shown) <= max_w {
                return shown;
            }
        }
        String::from("...")
    }

    fn ellipsize_right(&self, text: &str, max_w: i32) -> String {
        if self.width(text) <= max_w {
            return text.to_string();
        }
        let chars: Vec<char> = text.chars().collect();
        for end in (1..chars.len()).rev() {
            let s = format!("{}...", chars[..end].iter().collect::<String>().trim_end());
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
fn spawn_game(
    info: &DiscInfo,
    settings: &LauncherSettings,
    online: bool,
    resume: Option<&Path>,
) -> Result<Running> {
    use std::process::{Command, Stdio};
    let exe = std::env::current_exe().context("locate the legaia-engine executable")?;
    let mut cmd = Command::new(&exe);
    match resume {
        Some(save) => cmd.args(launcher::play_args_continuing(info, settings, save)),
        None => cmd.args(launcher::play_args_with(info, settings)),
    };
    if online {
        cmd.arg("--multiplayer");
    }
    cmd.stdin(Stdio::null())
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

/// The help-box account of a failed game: how it ended, then either what is
/// wrong with the disc (the disc card names the problem too) or where the
/// log is - second, so the reason after it can never push it out of the
/// box - and the reason the log gives.
fn failure_message(
    status: &std::process::ExitStatus,
    log: Option<&Path>,
    log_text: Option<&str>,
    disc_problem: Option<&DiscProblem>,
) -> Vec<Message> {
    let mut msg = vec![(legaia_app_theme::game_exit::exit_headline(status), WARN)];
    if let Some(p) = disc_problem {
        msg.extend(problem_message(p));
        return msg;
    }
    if let Some(log) = log {
        msg.push((
            format!(
                "{} in the saves folder.",
                log.file_name().unwrap_or_default().to_string_lossy()
            ),
            FAINT,
        ));
    }
    if let Some(why) = log_text.and_then(legaia_app_theme::game_exit::failure_detail) {
        msg.push((why, DIM));
    }
    msg
}

/// The text Ctrl+C copies after a failed game.
fn failure_report(status: &std::process::ExitStatus, log_text: Option<&str>) -> String {
    use legaia_app_theme::game_exit;
    game_exit::report_text(
        "legaia-engine",
        env!("CARGO_PKG_VERSION"),
        &game_exit::exit_headline(status),
        log_text.and_then(game_exit::failure_detail).as_deref(),
        log_text,
        game_exit::home_folder().as_deref(),
    )
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
/// One line naming a mounted memory card and what the game will find on it.
fn card_summary(info: &launcher::CardInfo) -> String {
    let name = file_name_of(&info.path);
    match info.saves {
        0 => format!("Card 2 is {name}, no Legaia saves yet."),
        1 => format!("Card 2 is {name}, 1 save."),
        n => format!("Card 2 is {name}, {n} saves."),
    }
}

/// The advice under a memory card that would not mount.
const CARD_FIX: &str = "Choose a .mcr, .mcd, .gme or .mcs card (Shift+Enter), or Delete.";

/// *Saves folder*'s help: the game's own card is in that folder; an
/// emulator card can ride beside it as card 2 (`--card`).
///
/// `others` is how many cards used before the second action can step to
/// ([`LauncherSettings::other_recent_cards`]).
fn data_folder_help(card: Option<&Result<launcher::CardInfo, String>>, others: usize) -> String {
    let mut s = String::from("Open the folder your saves and settings live in. ");
    match (card, others) {
        (None, 0) => {
            s.push_str("Right-click or Shift+Enter: use an emulator memory card as card 2.")
        }
        (None, _) => s.push_str("Right-click or Shift+Enter: card 2 from a card used before."),
        (Some(Ok(info)), others) => {
            s.push_str(&card_summary(info));
            s.push_str(if others > 0 {
                " Shift+Enter: the next. Delete removes it."
            } else {
                " Delete removes it."
            });
        }
        (Some(Err(_)), _) => s.push_str("Card 2 cannot be read. Delete removes it."),
    }
    s
}

/// What a controller button does on the launcher screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PadNav {
    /// Up / Down: the focus.
    Focus(i32),
    /// Left / Right: step an option, or change column.
    Horizontal(i8),
    /// L1 / R1: a page of the option list.
    Page(i32),
    /// Circle: out of the Controls page.
    Back,
    /// Cross (`1`) or Triangle, the right-click (`-1`).
    Activate(i8),
    /// Start: play the ready disc.
    Play,
}

/// A held controller direction steps again, as a held arrow key does: the
/// first repeat [`NAV_REPEAT_DELAY`] after the press, then one every
/// [`NAV_REPEAT_PERIOD`]. Only the moves repeat (D-pad, stick, L1 / R1);
/// a held Cross does not press twice.
#[derive(Debug, Default)]
struct NavRepeat {
    held: u16,
    since: Option<Instant>,
    last: Option<Instant>,
}

const NAV_REPEAT_DELAY: Duration = Duration::from_millis(400);
const NAV_REPEAT_PERIOD: Duration = Duration::from_millis(80);

impl NavRepeat {
    /// The moves to repeat this poll, given what the controller holds.
    fn step(&mut self, held: u16, now: Instant) -> u16 {
        use legaia_engine_system::pad::PadButton as B;
        let moves = held
            & (B::Up.mask()
                | B::Down.mask()
                | B::Left.mask()
                | B::Right.mask()
                | B::L1.mask()
                | B::R1.mask());
        if moves != self.held {
            self.held = moves;
            self.since = Some(now);
            self.last = None;
            return 0;
        }
        let Some(since) = self.since.filter(|_| moves != 0) else {
            return 0;
        };
        let due = match self.last {
            None => now.duration_since(since) >= NAV_REPEAT_DELAY,
            Some(last) => now.duration_since(last) >= NAV_REPEAT_PERIOD,
        };
        if due {
            self.last = Some(now);
            moves
        } else {
            0
        }
    }
}

/// The launcher actions for one poll's newly pressed PSX buttons, in order:
/// the moves first, then at most one activation (Triangle before Cross
/// before Start), so a chord cannot both play and change an option.
fn pad_nav(pressed: u16) -> Vec<PadNav> {
    use legaia_engine_system::pad::PadButton as B;
    let on = |b: B| pressed & b.mask() != 0;
    let mut out = Vec::new();
    for (b, nav) in [
        (B::Up, PadNav::Focus(-1)),
        (B::Down, PadNav::Focus(1)),
        (B::Left, PadNav::Horizontal(-1)),
        (B::Right, PadNav::Horizontal(1)),
        (B::L1, PadNav::Page(-1)),
        (B::R1, PadNav::Page(1)),
        (B::Circle, PadNav::Back),
    ] {
        if on(b) {
            out.push(nav);
        }
    }
    if on(B::Triangle) {
        out.push(PadNav::Activate(-1));
    } else if on(B::Cross) {
        out.push(PadNav::Activate(1));
    } else if on(B::Start) {
        out.push(PadNav::Play);
    }
    out
}

/// A path's file name, for a line that names a disc.
fn file_name_of(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| p.display().to_string())
}

/// The line that offers a remembered disc back.
fn recent_offer_line(offer: &DiscInfo) -> String {
    format!(
        "Right-click or Shift+Enter: {}, used before.",
        file_name_of(&offer.path)
    )
}

/// The disc card's folder line: the last two folders, which are what a
/// person recognises (the full path is in the help for *Change disc*), with
/// `...` only when folders were left out. A root or drive prefix is not a
/// folder: `/media/usb` shows whole.
fn short_folder(dir: &Path) -> String {
    use std::path::Component;
    let names: Vec<String> = dir
        .components()
        .filter(|c| matches!(c, Component::Normal(_)))
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    if names.len() > 2 {
        format!(".../{}", names[names.len() - 2..].join("/"))
    } else {
        dir.display().to_string()
    }
}

/// What dropping `path` on the window will do, shown while it is dragged
/// over: the routing [`LauncherApp::open_path`] applies to the drop.
fn drop_hint(path: &Path) -> String {
    let name = file_name_of(path);
    if launcher::expansion_pack_path(path).is_some() {
        format!("Drop to play with the expansion pack {name}.")
    } else if launcher::is_card_file(path) {
        format!("Drop to load the saves on the memory card {name}.")
    } else {
        format!("Drop to use {name} as the disc.")
    }
}

/// *Change disc*'s help while another remembered disc is on offer: the disc
/// card already shows the current one.
fn change_disc_help(offer: &DiscInfo) -> String {
    format!(
        "Pick another disc image (Ctrl+O). Right-click or Shift+Enter: {}, chosen before.",
        file_name_of(&offer.path)
    )
}

/// *Choose disc*'s help with no usable disc but a remembered one on offer.
fn choose_disc_help(offer: &DiscInfo) -> String {
    format!(
        "Pick your disc image (.bin or .cue). {}",
        recent_offer_line(offer)
    )
}

/// The help box's lines for a disc problem; with a remembered disc on offer,
/// the problem's reason and the offer in place of its advice.
fn problem_status(p: &DiscProblem, offer: Option<&DiscInfo>) -> Vec<Message> {
    let mut msg = problem_message(p);
    if let Some(o) = offer {
        msg.truncate(1);
        msg.push((recent_offer_line(o), GOOD));
    }
    msg
}

fn problem_headline(p: &DiscProblem) -> &'static str {
    match p {
        DiscProblem::NotSet => "Choose your Legend of Legaia (USA) disc image",
        DiscProblem::BadSettings { .. } => "The saved settings could not be read",
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
        // The card shows where the unreadable file now is.
        DiscProblem::BadSettings { path, kept, .. } => Some(kept.as_deref().unwrap_or(path)),
        DiscProblem::NotSet => None,
    }
}

/// The help-box explanation of `p`, with what to do about it.
fn problem_message(p: &DiscProblem) -> Vec<Message> {
    let (why, hint): (String, String) = match p {
        DiscProblem::NotSet => return Vec::new(),
        // The box is small: the file's name, not its folder (the log and
        // the terminal line carry the whole path).
        DiscProblem::BadSettings { why, kept, .. } => (
            sentence(why),
            match kept {
                Some(k) => format!(
                    "The old file was kept as {}. Choose the disc again.",
                    file_name_of(k)
                ),
                None => "That file is left as it is. Choose the disc again.".into(),
            },
        ),
        DiscProblem::Missing(_) | DiscProblem::Moved(_) => (
            "It may have been moved, renamed, or be on a drive that is not connected.".into(),
            "Choose it again where it is now.".into(),
        ),
        DiscProblem::Unreadable { reason, fix, .. } => (
            sentence(reason),
            fix.clone().unwrap_or_else(|| {
                "Choose the .bin of a raw dump (2352-byte sectors), or the .cue beside it.".into()
            }),
        ),
        DiscProblem::WrongDisc { reason, fix, .. } => (
            sentence(reason),
            fix.clone()
                .unwrap_or_else(|| "Choose the image of the Legend of Legaia (USA) disc.".into()),
        ),
    };
    vec![(why, WARN), (hint, DIM)]
}

/// `s` as a sentence: capitalised, ending in one full stop.
fn sentence(s: &str) -> String {
    let s = capitalise_first(s.trim());
    if s.ends_with(['.', '!', '?']) {
        s
    } else {
        format!("{s}.")
    }
}

/// The rows the buttons take: the last two (the saves folder and Quit) share
/// one, so the help box under them keeps room for a whole message.
const BUTTON_ROWS: i32 = BUTTONS.len() as i32 - 2;

/// Button `i`'s rect: one full-width row each, except the last two, which
/// split the bottom row.
/// A button label that fits a button `w` stage pixels wide, with a margin
/// on each side: the label itself, else its short form, else the label
/// ellipsized. The placeholder font the screen letters in before a disc is
/// chosen is wider than the disc's, and "Saves folder" ran past the edges
/// of its half-width button on a first run.
fn fit_button_label(c: &Canvas, label: &str, w: i32) -> String {
    let room = w - 2 * BUTTON_LABEL_MARGIN;
    if c.width(label) <= room {
        return label.to_string();
    }
    let short = match label {
        "Saves folder" => Some("Saves"),
        "Back to options" => Some("Options"),
        "Expansion pack..." => Some("Expansion..."),
        _ => None,
    };
    match short {
        Some(s) if c.width(s) <= room => s.to_string(),
        _ => c.ellipsize_right(label, room),
    }
}

/// Stage pixels kept clear between a button's edge and its label.
const BUTTON_LABEL_MARGIN: i32 = 3;

fn button_rect(i: usize) -> Rect {
    // The first four buttons take a row each; the rest pair up, two to a
    // row (Controls / Recent, Saves folder / Quit).
    let i = i as i32;
    if i < FULL_BUTTONS {
        return Rect::new(LEFT_X, COLUMNS_Y + i * BUTTON_PITCH, LEFT_W, BUTTON_H);
    }
    let paired = i - FULL_BUTTONS;
    let y = COLUMNS_Y + (FULL_BUTTONS + paired / 2) * BUTTON_PITCH;
    let half = (LEFT_W - 4) / 2;
    let x = if paired % 2 == 0 {
        LEFT_X
    } else {
        LEFT_X + LEFT_W - half
    };
    Rect::new(x, y, half, BUTTON_H)
}

/// How many of [`BUTTONS`] take a whole row.
const FULL_BUTTONS: i32 = 4;

/// The help box under the buttons.
fn help_rect() -> Rect {
    let y = COLUMNS_Y + BUTTON_ROWS * BUTTON_PITCH + 4;
    Rect::new(LEFT_X, y, LEFT_W, STAGE_H - PAD - y)
}

/// The right-hand panel (options or controls).
fn panel_rect() -> Rect {
    Rect::new(RIGHT_X, COLUMNS_Y, RIGHT_W, STAGE_H - PAD - COLUMNS_Y)
}

/// How many of the rebind session's rows the Controls page lists: its order
/// ends on L3 and R3, which no default binds and the game never reads.
const BIND_ROWS: u8 = 14;

/// The controls panel's lines: the rows, and the reset row under them.
#[cfg(test)]
const CONTROLS_ROWS: i32 = BIND_ROWS as i32 + 1;

/// The note under the controls table, wrapped to the panel.
const CONTROLS_NOTES: [&str; 1] = ["Each row: keys | controller button."];

/// Why a pressed key was refused, and which ones work: the names
/// `keycode_to_name` knows. The rest are the game window's own shortcuts.
const UNBINDABLE_KEY: &str = "That key belongs to the game window's own shortcuts. Use an arrow \
                              key, Z X C V A S D Q W E, 1, 2, Enter, Space or Right Shift. Esc \
                              cancels.";

/// The help line while a row waits for its key.
fn waiting_message(button: &str) -> String {
    format!(
        "Press the key or controller button for {button}. Esc cancels; the button it has \
         keeps it."
    )
}

/// A pad button's name on the Controls page.
fn button_label(name: &str) -> &'static str {
    match name {
        "Up" => "D-pad up",
        "Down" => "D-pad down",
        "Left" => "D-pad left",
        "Right" => "D-pad right",
        "Cross" => "Cross",
        "Circle" => "Circle",
        "Triangle" => "Triangle",
        "Square" => "Square",
        "L1" => "L1",
        "R1" => "R1",
        "L2" => "L2",
        "R2" => "R2",
        "Start" => "Start",
        "Select" => "Select",
        _ => "",
    }
}

/// A key's name as the Controls page shows it.
fn key_label(key: &str) -> &str {
    match key {
        "RShift" => "Right Shift",
        "Up" => "Up arrow",
        "Down" => "Down arrow",
        "Left" => "Left arrow",
        "Right" => "Right arrow",
        k => k,
    }
}

/// Every key on `button`, sorted so the row does not flicker between frames.
fn keys_for_button(m: &Mapping, button: &str) -> String {
    let mut k: Vec<&str> = m
        .bindings
        .iter()
        .filter(|(_, b)| b.as_str() == button)
        .map(|(k, _)| key_label(k))
        .collect();
    k.sort_unstable();
    k.join(", ")
}

/// What a finished rebind did, naming a button the key was taken from.
fn bound_message(before: &Mapping, after: &Mapping, button: &str, key: &str) -> String {
    let done = format!("{} is on {}.", button_label(button), key_label(key));
    match before.bindings.get(key).filter(|b| b.as_str() != button) {
        Some(other) => match keys_for_button(after, other) {
            k if k.is_empty() => format!("{done} {} has no key now.", button_label(other)),
            k => format!("{done} {} moved to {k}.", button_label(other)),
        },
        None => done,
    }
}

fn controls_note_lines(c: &Canvas, panel: Rect) -> Vec<String> {
    CONTROLS_NOTES
        .iter()
        .flat_map(|n| c.wrap(n, panel.w - 20))
        .collect()
}

/// The stage row just past the controls notes' last line (the fit test's
/// measure of [`LauncherApp::controls_panel`]).
#[cfg(test)]
fn controls_notes_bottom(c: &Canvas, panel: Rect) -> i32 {
    panel.y
        + LIST_INSET
        + CONTROLS_ROWS * ROW_PITCH
        + 4
        + controls_note_lines(c, panel).len() as i32 * 12
}

/// How a help-box message lays out: its wrapped lines, whether the key
/// reminder keeps its line, the last row text may reach, and whether every
/// line fits.
struct HelpLayout {
    lines: Vec<(Vec<String>, Role)>,
    footer: bool,
    bottom: i32,
    /// Read by the fit test; the window draws what fits either way.
    #[cfg_attr(not(test), allow(dead_code))]
    fits: bool,
}

fn help_layout(c: &Canvas, msg: &[Message]) -> HelpLayout {
    let help = help_rect();
    let lines: Vec<(Vec<String>, Role)> = msg
        .iter()
        .map(|(text, ink)| (c.wrap(text, help.w - 12), *ink))
        .collect();
    // The key reminder gives its line up to a message that needs it (a disc
    // problem's reason and fix): cutting the fix off mid-sentence would
    // leave the one line that says what to do unread.
    let rows: i32 = lines.iter().map(|(l, _)| l.len() as i32).sum();
    let needed = rows * 12 + 3 * (lines.len() as i32 - 1).max(0);
    let footer = needed <= help.h - 6 - 14;
    let bottom = help.y + help.h - if footer { 14 } else { 4 };
    HelpLayout {
        fits: help.y + 6 + needed <= bottom,
        lines,
        footer,
        bottom,
    }
}

/// The help box's note under a changed option row: the first-start value,
/// the key that restores it, and the original game's value when it has one.
fn default_note(row: OptionRow) -> String {
    match row.original_value() {
        Some(orig) => format!(
            "Default: {} (Delete). Original: {orig}.",
            row.default_value()
        ),
        None => format!("Default: {} (Delete)", row.default_value()),
    }
}

/// The help box's note on how the focused row (at its default) compares
/// with the original game: green when the value is not the original's
/// (naming the original's), dim when it is. `None` for a row the original
/// has no counterpart for.
fn original_note(row: OptionRow, opts: &OptionsState, s: &LauncherSettings) -> Option<Message> {
    let orig = row.original_value()?;
    Some(if row.departs_from_original(opts, s) {
        (format!("Not as in the original game ({orig})."), GOOD)
    } else {
        ("As in the original game.".into(), FAINT)
    })
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

/// Whether two paths name one file (a remembered pack path written before a
/// folder moved under a symlink, say).
fn same_file(a: &Path, b: &Path) -> bool {
    matches!((a.canonicalize(), b.canonicalize()), (Ok(x), Ok(y)) if x == y)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folder_line_elides_only_when_folders_were_left_out() {
        assert_eq!(short_folder(Path::new("/media/usb")), "/media/usb");
        assert_eq!(
            short_folder(Path::new("/home/me/Downloads/Legaia")),
            ".../Downloads/Legaia"
        );
        assert_eq!(short_folder(Path::new("discs")), "discs");
    }

    fn canvas(font: &legaia_font::Font) -> Canvas<'_> {
        Canvas {
            palette: legaia_app_theme::palette(true),
            font,
            solid: (0, 0, 1, 1),
            condense: true,
            stage: Stage {
                scale: 2,
                origin: (0, 0),
                surface: (960, 720),
                squeeze: 0,
            },
            draws: Vec::new(),
        }
    }

    /// A window a few rows short of a whole step keeps the step: the header
    /// band gives up the rows and the stage under it is unchanged and on
    /// the surface.
    #[test]
    fn a_window_a_little_short_keeps_its_scale() {
        let full = stage_for((960, 720));
        assert_eq!((full.scale, full.squeeze, full.origin), (2, 0, (0, 0)));
        // A 960x720 window clamped by a 768-pixel screen's panels.
        let short = stage_for((960, 699));
        assert_eq!((short.scale, short.squeeze), (2, 11));
        assert_eq!(short.origin.1 + short.squeeze * short.scale, 0);
        assert!(short.origin.1 + STAGE_H * short.scale <= 699);
        assert_eq!(stage_for((1920, 1080)).scale, 3);
        assert_eq!(stage_for((1366, 705)).scale, 2);
        // Too short for the give: the step below.
        let small = stage_for((960, 690));
        assert_eq!((small.scale, small.squeeze), (1, 0));
        // The minimum window.
        let min = stage_for((STAGE_W as u32, (STAGE_H - HEADER_GIVE) as u32));
        assert_eq!(
            (min.scale, min.squeeze, min.origin),
            (1, HEADER_GIVE, (0, -HEADER_GIVE))
        );
        // The squeezed header still holds the title: 24 rows of it.
        let title_y = 7 + HEADER_GIVE / 2;
        assert!(title_y >= HEADER_GIVE && title_y + 24 <= HEADER_H);
    }

    #[test]
    fn controller_buttons_drive_the_screen_like_its_keys() {
        use legaia_engine_system::pad::PadButton as B;
        assert!(pad_nav(0).is_empty());
        assert_eq!(pad_nav(B::Down.mask()), [PadNav::Focus(1)]);
        assert_eq!(pad_nav(B::Left.mask()), [PadNav::Horizontal(-1)]);
        assert_eq!(pad_nav(B::R1.mask()), [PadNav::Page(1)]);
        assert_eq!(pad_nav(B::Cross.mask()), [PadNav::Activate(1)]);
        assert_eq!(pad_nav(B::Triangle.mask()), [PadNav::Activate(-1)]);
        assert_eq!(pad_nav(B::Circle.mask()), [PadNav::Back]);
        assert_eq!(pad_nav(B::Start.mask()), [PadNav::Play]);
        // One activation per poll: a chord does not both play and step.
        assert_eq!(
            pad_nav(B::Cross.mask() | B::Start.mask() | B::Up.mask()),
            [PadNav::Focus(-1), PadNav::Activate(1)]
        );
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
        let s = c.ellipsize_right("Pack: Legend of Legaia: A Long Subtitle", 120);
        assert!(s.starts_with("Pack: "), "{s}");
        assert!(s.ends_with("..."));
        assert!(c.width(&s) <= 120);
        assert_eq!(c.ellipsize_right("short", 120), "short");
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
        assert_eq!(app.focus, start, "Tab back returns to the button it left");
    }

    /// A file dragged over the window is named with what a drop does, by
    /// the same routing the drop takes.
    #[test]
    fn drop_hint_follows_the_drop_routing() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            drop_hint(Path::new("/g/Legend of Legaia (USA).cue")),
            "Drop to use Legend of Legaia (USA).cue as the disc."
        );
        assert!(drop_hint(Path::new("/g/story.lxpack")).contains("expansion pack story.lxpack"));
        assert!(drop_hint(Path::new("/g/epsxe.mcr")).contains("memory card epsxe.mcr"));
        std::fs::write(dir.path().join("pack.toml"), "").unwrap();
        assert!(drop_hint(dir.path()).contains("expansion pack"));
    }

    /// Tab keeps the player's place in each column: an option row walked to
    /// and a button walked to both come back on the next Tab.
    #[test]
    fn tab_returns_to_where_each_column_was_left() {
        let mut app = LauncherApp::new(None, Some(DiscProblem::NotSet), Default::default(), None);
        app.focus = Item::Expansion;
        app.switch_column();
        assert_eq!(app.focus, Item::Opt(OPTION_ROWS[0].1));
        let row = Item::Opt(OPTION_ROWS[5].1);
        app.focus = row;
        app.switch_column();
        assert_eq!(app.focus, Item::Expansion);
        app.focus = Item::Panel;
        app.switch_column();
        assert_eq!(app.focus, row);
        app.switch_column();
        assert_eq!(app.focus, Item::Panel);
        // On the Controls page an option row is not a place to return to.
        app.panel = Panel::Controls;
        app.switch_column();
        assert_eq!(app.focus, Item::Bind(0));
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
        // `--open` (the game program's "Open with") is the launcher's alone.
        let opened = Cli::try_parse_from(["legaia-engine", "--open", "/g/l.bin"]).unwrap();
        assert!(opened.cmd.is_none());
        assert_eq!(opened.open.as_deref(), Some(Path::new("/g/l.bin")));
        assert!(Cli::try_parse_from(["legaia-engine", "--open", "/g/l.bin", "config"]).is_err());
        let info = DiscInfo {
            path: "/g/l.cue".into(),
            bin: "/g/l.bin".into(),
            executable: launcher::USA_EXECUTABLE.into(),
        };
        let settings = LauncherSettings {
            switches: legaia_engine_session::session_switches::PersistedSwitches {
                psx_render: true,
                retail: true,
                ..Default::default()
            },
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
            switches: legaia_engine_session::session_switches::PersistedSwitches {
                no_entry_pulse: true,
                smart_monsters: true,
                arts_saved_list: true,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut argv = vec![OsString::from("legaia-engine")];
        argv.extend(launcher::play_args_with(&info, &settings));
        match Cli::try_parse_from(argv).unwrap().cmd {
            Some(Cmd::PlayWindow {
                no_entry_pulse,
                smart_monsters,
                arts_saved_list,
                retail,
                ..
            }) => assert!(no_entry_pulse && smart_monsters && arts_saved_list && !retail),
            other => panic!("expected play-window, got {other:?}"),
        }
        // So do the gameplay-tweak rows.
        let settings = LauncherSettings {
            switches: legaia_engine_session::session_switches::PersistedSwitches {
                no_shiny_seru: true,
                flee_exp: true,
                exp_rate_permille: Some(2000),
                no_fortress_encounter_fix: true,
                no_tetsu_spar_fix: true,
                jewel_fix: true,
                no_hp_readout_guard: true,
                spirit_ap: Some(-16),
                damage_ap: Some(200),
                seru_catch_pct: Some(75),
                enemy_attack_permille: Some(2000),
                enemy_stat_permille: Some(1500),
                equipment_drops: true,
                ..Default::default()
            },
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
                no_tetsu_spar_fix,
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
                assert!(no_tetsu_spar_fix);
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
            .position(|l| *l == Line::Row(OptionRow::MasterVolume))
            .unwrap();
        assert_eq!(lines[sound - 1], Line::Heading("Sound"));
        assert_eq!(scroll_to_reveal(&lines, max, sound), (sound - 1).min(max));
        // Every row is reachable and visible within the panel height.
        assert!(VISIBLE_LINES as i32 * ROW_PITCH + 2 * LIST_INSET <= STAGE_H - PAD - COLUMNS_Y);
    }

    #[test]
    fn typed_path_quotes_are_stripped() {
        assert_eq!(clean_typed_path("  \"/a b/c.bin\" "), "/a b/c.bin");
        assert_eq!(clean_typed_path("'/a/c.cue'"), "/a/c.cue");
        assert_eq!(clean_typed_path("/plain.bin"), "/plain.bin");
    }

    /// The fonts the launcher letters itself in: the built-in one (first
    /// run, condensed) and, when a disc is known, the disc's dialog font.
    fn fonts() -> Vec<(legaia_font::Font, bool, &'static str)> {
        let mut v = vec![(legaia_font::Font::placeholder(), true, "built-in font")];
        if let Some(bin) = std::env::var_os("LEGAIA_DISC_BIN")
            && let Some(f) = launcher::disc_font(Path::new(&bin))
        {
            v.push((f, false, "disc font"));
        } else {
            eprintln!("skip: disc-font half (LEGAIA_DISC_BIN unset)");
        }
        v
    }

    fn canvas_for(font: &legaia_font::Font, condense: bool) -> Canvas<'_> {
        Canvas {
            condense,
            ..canvas(font)
        }
    }

    /// Every message the help box shows, in its widest form.
    fn help_messages() -> Vec<(String, Vec<Message>)> {
        let mut out: Vec<(String, Vec<Message>)> = Vec::new();
        let one = |s: &str| vec![(s.to_string(), DIM)];
        for (_, row) in OPTION_ROWS {
            out.push((row.label().into(), one(row.help())));
            out.push((
                format!("{} (changed)", row.label()),
                vec![(row.help().to_string(), DIM), (default_note(row), GOLD)],
            ));
            if let Some(orig) = row.original_value() {
                out.push((
                    format!("{} (not original)", row.label()),
                    vec![
                        (row.help().to_string(), DIM),
                        (format!("Not as in the original game ({orig})."), GOOD),
                    ],
                ));
            }
            if row.overridden_by_retail() {
                let s = format!("{} Off while Original game only is on.", row.help());
                out.push((format!("{} (retail)", row.label()), one(&s)));
            }
        }
        let path = PathBuf::from("/home/player/Games/Legend of Legaia (USA).bin");
        let problems = [
            DiscProblem::NotSet,
            DiscProblem::BadSettings {
                path: "/home/player/.config/legaia-engine/launcher.toml".into(),
                why: "expected `=`, found newline, line 1".into(),
                kept: Some("/home/player/.config/legaia-engine/launcher.toml.broken".into()),
            },
            DiscProblem::Missing(path.clone()),
            DiscProblem::Moved(path.clone()),
            DiscProblem::WrongDisc {
                path: path.clone(),
                reason: "it is the Europe release (SCES_019.44); the engine runs the USA disc \
                         (SCUS_942.54)"
                    .into(),
                fix: Some("A randomized or translated copy of the USA disc works too.".into()),
            },
            DiscProblem::WrongDisc {
                path: path.clone(),
                reason: "it is not a PlayStation game disc (it has no SYSTEM.CNF boot file)".into(),
                fix: Some("Choose the image of the Legend of Legaia (USA) game disc.".into()),
            },
            DiscProblem::Unreadable {
                path: path.clone(),
                reason: "PROT.DAT, the file that holds the game's data, is missing from it".into(),
                fix: Some(legaia_app_theme::disc_text::DAMAGED_FIX.into()),
            },
            DiscProblem::Unreadable {
                path: path.clone(),
                reason: "its game program SCUS_942.54 is damaged (it is not a PlayStation \
                         program)"
                    .into(),
                fix: Some(legaia_app_theme::disc_text::DAMAGED_FIX.into()),
            },
        ];
        // A remembered disc offered back, under each problem and in the
        // Choose disc help with a disc in use (the longest file name the
        // test path has).
        let offer = DiscInfo {
            path: path.clone(),
            bin: path.clone(),
            executable: launcher::USA_EXECUTABLE.into(),
        };
        for p in problems {
            out.push((format!("{p:?}"), problem_message(&p)));
            out.push((
                format!("{p:?} + recent offer"),
                problem_status(&p, Some(&offer)),
            ));
        }
        out.push((
            "change disc + recent offer".into(),
            one(&change_disc_help(&offer)),
        ));
        out.push((
            "choose disc + recent offer".into(),
            one(&choose_disc_help(&offer)),
        ));
        // The memory card's lines: the Saves folder help with each card
        // state, and the refusals.
        let card = |saves| launcher::CardInfo {
            path: "/home/player/Emulators/DuckStation/memcards/SCUS-94254_1.mcd".into(),
            format: "mcr",
            saves,
        };
        for others in [0, 5] {
            out.push((
                format!("saves, no card, {others} used before"),
                one(&data_folder_help(None, others)),
            ));
            for n in [0, 1, 15] {
                out.push((
                    format!("saves, card with {n}, {others} used before"),
                    one(&data_folder_help(Some(&Ok(card(n))), others)),
                ));
            }
            out.push((
                format!("saves, bad card, {others} used before"),
                one(&data_folder_help(Some(&Err(String::new())), others)),
            ));
        }
        let why = "Unrecognised save container (4096 bytes).".to_string();
        for head in [
            "Not a memory card the game can use:",
            "Card 2 cannot be used:",
        ] {
            out.push((
                format!("card refused: {head}"),
                vec![
                    (head.into(), WARN),
                    (why.clone(), WARN),
                    (CARD_FIX.into(), DIM),
                ],
            ));
        }
        // Every reason the disc sniffer gives.
        let mut iso = vec![0u8; psx_iso::sniff::SNIFF_LEN];
        iso[16 * 2048 + 1..16 * 2048 + 6].copy_from_slice(b"CD001");
        for (ext, head) in [
            ("chd", &b"MComprHD"[..]),
            ("pbp", b"\0PBP"),
            ("ecm", b"ECM\0"),
            ("zip", b"PK\x03\x04"),
            ("ppf", b"PPF"),
            ("ccd", b"[CloneCD]"),
            ("m3u", b"x.cue"),
            ("iso", &iso),
            ("bin", &[1u8; 8]),
            ("bin", &[1u8; 4096]),
        ] {
            let d = psx_iso::sniff::diagnose_bytes(ext, head, head.len() as u64).unwrap();
            let p = DiscProblem::Unreadable {
                path: path.clone(),
                reason: d.what,
                fix: Some(d.fix),
            };
            out.push((format!("sniff {ext}"), problem_message(&p)));
        }
        let renamed = DiscProblem::Unreadable {
            path: path.clone(),
            reason: "This .cue names \"Legend of Legaia (USA) (Track 1).bin\", which is not in \
                     its folder."
                .into(),
            fix: Some(
                "If the .bin was renamed, choose \"Legend of Legaia (USA).bin\" itself.".into(),
            ),
        };
        out.push(("renamed cue".into(), problem_message(&renamed)));
        out.push(("typed path".into(), vec![(TYPED_PATH_HELP.into(), DIM)]));
        // The Controls page's lines.
        out.push(("unbindable key".into(), vec![(UNBINDABLE_KEY.into(), WARN)]));
        for b in ["Cross", "Up"] {
            out.push((
                format!("waiting {b}"),
                one(&waiting_message(button_label(b))),
            ));
        }
        let mut swapped = Mapping::default();
        swapped.bindings.remove("X");
        swapped.bindings.insert("X".into(), "Cross".into());
        swapped.bindings.insert("Z".into(), "Square".into());
        out.push((
            "bound with a swap".into(),
            one(&bound_message(&Mapping::default(), &swapped, "Cross", "X")),
        ));
        // A failed game: the longest headline, the log line and a reason at
        // its cap; and a failure the disc explains.
        #[cfg(unix)]
        {
            use std::os::unix::process::ExitStatusExt;
            let status = std::process::ExitStatus::from_raw(255 << 8);
            let log = format!("Error: {}\n", "word ".repeat(40));
            out.push((
                "failed game".into(),
                failure_message(
                    &status,
                    Some(Path::new("/d/legaia-engine-game.log")),
                    Some(&log),
                    None,
                ),
            ));
            out.push((
                "failed game, disc gone".into(),
                failure_message(&status, None, None, Some(&DiscProblem::Moved(path.clone()))),
            ));
        }
        out
    }

    /// Every help-box message fits whole, in both fonts: nothing the box
    /// shows is cut off mid-sentence.
    #[test]
    fn every_help_message_fits_the_help_box() {
        let mut cut = Vec::new();
        for (font, condense, name) in fonts() {
            let c = canvas_for(&font, condense);
            for (what, msg) in help_messages() {
                let l = help_layout(&c, &msg);
                if !l.fits {
                    let rows: Vec<_> = l.lines.iter().map(|(w, _)| w).collect();
                    cut.push(format!("{name}: {what}: {rows:?}"));
                }
            }
        }
        assert!(cut.is_empty(), "the help box cuts off:\n{}", cut.join("\n"));
    }

    /// The buttons' own help leaves the help box its key reminder, with and
    /// without a disc: the reminder is the screen's only word that it is
    /// keyboard-driven, and the first run is when a player needs it most.
    #[test]
    fn button_help_keeps_the_key_reminder() {
        let disc = DiscInfo {
            path: "/home/player/Games/Legend of Legaia (USA).bin".into(),
            bin: "/home/player/Games/Legend of Legaia (USA).bin".into(),
            executable: launcher::USA_EXECUTABLE.into(),
        };
        let mut lost = Vec::new();
        for (font, condense, name) in fonts() {
            let c = canvas_for(&font, condense);
            for with_disc in [false, true] {
                let mut app =
                    LauncherApp::new(None, Some(DiscProblem::NotSet), Default::default(), None);
                app.status = None;
                if with_disc {
                    app.problem = None;
                    app.disc = Some(disc.clone());
                }
                for item in BUTTONS {
                    app.focus = item;
                    let l = help_layout(&c, &app.help_lines());
                    if !l.footer {
                        lost.push(format!("{name}: {item:?} (disc: {with_disc})"));
                    }
                }
            }
        }
        assert!(
            lost.is_empty(),
            "the key reminder is pushed out:\n{}",
            lost.join("\n")
        );
    }

    /// Every button label fits its button, in both fonts, in each of its
    /// states (no disc / disc, options / controls panel, a pack loaded).
    #[test]
    fn every_button_label_fits_its_button() {
        let labels = [
            "Play",
            "Play online",
            "Choose disc...",
            "Change disc...",
            "Controls",
            "Back to options",
            "Expansion pack...",
            "Pack: has errors",
            "Saves folder",
            "Quit",
        ];
        for (font, condense, name) in fonts() {
            let c = canvas_for(&font, condense);
            for (i, _) in BUTTONS.iter().enumerate() {
                let r = button_rect(i);
                for label in labels {
                    let shown = fit_button_label(&c, label, r.w);
                    assert!(
                        c.width(&shown) <= r.w - 2 * BUTTON_LABEL_MARGIN,
                        "{name}: {label:?} as {shown:?} is wider than button {i} ({} px)",
                        r.w
                    );
                }
            }
        }
    }

    /// The controls table and its notes stay inside the panel, in both fonts.
    /// The first-run line fits the disc card's two lines in both fonts: the
    /// sentence was once cut off in the middle on the very first screen.
    #[test]
    fn first_run_card_fits_its_two_lines() {
        for (font, condense, name) in fonts() {
            let c = canvas_for(&font, condense);
            let inner = STAGE_W - 2 * PAD - 8 - 10 - c.width("Disc") - 10;
            let lines = c.wrap(FIRST_RUN_CARD, inner);
            assert!(lines.len() <= 2, "{name}: {lines:?}");
        }
    }

    /// The Controls page's table is the rebind kernel's: each listed row is a
    /// button the page has a name for, a bound key is written where the game
    /// reads it, and every key the refusal names is one the kernel can take.
    #[test]
    fn controls_page_rows_and_named_keys_are_real() {
        use winit::keyboard::KeyCode as K;
        let s = KeyRebindSession::new(Mapping::default());
        for row in s.rows().iter().take(usize::from(BIND_ROWS)) {
            assert!(!button_label(row.button.name()).is_empty(), "{row:?}");
            assert!(!row.key.is_empty(), "{row:?} has no default key");
        }
        for r in &s.rows()[usize::from(BIND_ROWS)..] {
            assert!(r.key.is_empty(), "{r:?} is left off the page but has a key");
        }
        for k in [
            K::ArrowUp,
            K::ArrowDown,
            K::ArrowLeft,
            K::ArrowRight,
            K::KeyZ,
            K::KeyX,
            K::KeyC,
            K::KeyV,
            K::KeyA,
            K::KeyS,
            K::KeyD,
            K::KeyQ,
            K::KeyW,
            K::KeyE,
            K::Digit1,
            K::Digit2,
            K::Enter,
            K::Space,
            K::ShiftRight,
        ] {
            assert!(
                !legaia_engine_shell::window::keycode_to_name(k).is_empty(),
                "{k:?}"
            );
        }
    }

    /// A held controller direction steps again, a held Cross does not.
    #[test]
    fn a_held_direction_repeats_and_a_held_cross_does_not() {
        use legaia_engine_system::pad::PadButton as B;
        let mut r = NavRepeat::default();
        let t0 = Instant::now();
        let at = |ms| t0 + Duration::from_millis(ms);
        let down = B::Down.mask() | B::Cross.mask();
        assert_eq!(
            r.step(down, at(0)),
            0,
            "the press itself is the poll's edge"
        );
        assert_eq!(r.step(down, at(300)), 0);
        assert_eq!(r.step(down, at(400)), B::Down.mask());
        assert_eq!(r.step(down, at(440)), 0);
        assert_eq!(r.step(down, at(480)), B::Down.mask());
        assert_eq!(r.step(B::Cross.mask(), at(500)), 0, "let go");
        assert_eq!(r.step(B::Cross.mask(), at(2000)), 0, "Cross never repeats");
    }

    /// A controller press while a row waits binds the row's controller
    /// button and writes nothing to the key table.
    #[test]
    fn a_waiting_row_takes_a_controller_button() {
        let before = Mapping::default();
        let mut s = KeyRebindSession::new(before.clone());
        s.begin_bind(0);
        let _ = s.tick(KeyRebindInput {
            key_pressed: Some("Pad3".into()),
            ..Default::default()
        });
        use legaia_engine_system::gamepad::ButtonSpace;
        assert_eq!(
            s.mapping().gamepad.button_for(ButtonSpace::Standard, 3),
            Some(legaia_engine_system::pad::PadButton::Cross)
        );
        assert_eq!(s.mapping().bindings, before.bindings);
    }

    /// A rebind says what moved, and a key nothing else was on moves nothing.
    #[test]
    fn bound_message_names_what_moved() {
        let before = Mapping::default();
        let mut s = KeyRebindSession::new(before.clone());
        s.begin_bind(0);
        let _ = s.tick(KeyRebindInput {
            key_pressed: Some("X".into()),
            ..Default::default()
        });
        assert_eq!(
            bound_message(&before, s.mapping(), "Cross", "X"),
            "Cross is on X. Square moved to Z."
        );
        assert_eq!(
            bound_message(&before, &before, "Cross", "E"),
            "Cross is on E."
        );
    }

    /// Leaving the Controls page moves a focus that sat on one of its rows,
    /// and Tab reaches the table and comes back.
    #[test]
    fn controls_page_focus_stays_on_a_real_control() {
        let mut app = LauncherApp::new(None, Some(DiscProblem::NotSet), Default::default(), None);
        app.panel = Panel::Controls;
        app.focus = Item::Panel;
        app.switch_column();
        assert_eq!(app.focus, Item::Bind(0));
        assert!(app.order().contains(&Item::BindReset));
        app.leave_page();
        assert_eq!(app.focus, Item::Panel);
        assert!(!app.order().contains(&Item::Bind(0)));
    }

    #[test]
    fn controls_notes_fit_the_panel() {
        let panel = panel_rect();
        for (font, condense, name) in fonts() {
            let c = canvas_for(&font, condense);
            let bottom = controls_notes_bottom(&c, panel);
            assert!(
                bottom <= panel.y + panel.h - 2,
                "{name}: the controls notes end at {bottom}, past the panel's {}",
                panel.y + panel.h
            );
        }
    }
}
