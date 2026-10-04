//! The first-run launcher: what `legaia-engine` does when started with no
//! subcommand (a double-click, a desktop shortcut).
//!
//! The decision logic - remembered disc path, validation, re-prompt reasons -
//! is [`legaia_engine_shell::launcher`]. This module is the window half:
//!
//! - A remembered disc that still validates boots straight into the game: the
//!   caller re-parses [`launcher::play_args`] and runs `play-window` in this
//!   process, so no launcher window ever opens.
//! - Otherwise a small picker screen opens, drawn with the engine's own text
//!   overlay (the built-in ASCII font - no disc is loaded yet). Enter or a
//!   click opens the native file dialog (`rfd`); dropping a file on the window
//!   or typing a path also works, for desktops without a file-dialog portal.
//!   A disc that validates is remembered and the game starts in a fresh
//!   process: winit allows one event loop per process, and the picker used it.
//!
//! Either way the game runs with its working directory set to
//! [`launcher::data_dir`], so key bindings, options and saves always land in
//! the same per-user place.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use legaia_engine_render::window::EngineWindow;
use legaia_engine_render::{RenderTarget, TextDraw, TextOverlay, UploadedFontAtlas};
use legaia_engine_shell::launcher::{
    self, DiscInfo, DiscProblem, LaunchDecision, LauncherSettings,
};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::WindowId;

/// Debug aid: when set, the picker writes its first frame to this PNG and
/// exits (lets a headless check see the screen without a human).
const SCREENSHOT_ENV: &str = "LEGAIA_LAUNCHER_SCREENSHOT";

/// Run the launcher. Returns the argument list to run in-process (the
/// remembered disc is good), or `None` when the launcher already finished -
/// the user quit, or the game ran in a child process.
pub(crate) fn run() -> Result<Option<Vec<OsString>>> {
    console::detach_if_double_clicked();
    let settings_file = launcher::settings_path();
    let settings = match settings_file.as_deref() {
        Some(f) => LauncherSettings::load(f),
        None => Ok(LauncherSettings::default()),
    };
    let decision = launcher::decide(settings, launcher::validate_disc);
    enter_data_dir();
    match decision {
        LaunchDecision::Play(info) => {
            log::info!(
                "launcher: booting {} ({}); settings in {}",
                info.bin.display(),
                info.executable,
                settings_file
                    .as_deref()
                    .map_or_else(|| "<none>".into(), |p| p.display().to_string())
            );
            let mut args = vec![OsString::from("legaia-engine")];
            args.extend(launcher::play_args(&info));
            Ok(Some(args))
        }
        LaunchDecision::Prompt(problem) => {
            log::info!("launcher: {problem}");
            let Some(info) = pick(problem, settings_file)? else {
                return Ok(None);
            };
            relaunch(&info)?;
            Ok(None)
        }
    }
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

/// Start the game in a child process and exit with its status.
fn relaunch(info: &DiscInfo) -> Result<()> {
    let exe = std::env::current_exe().context("locate the legaia-engine executable")?;
    let mut cmd = std::process::Command::new(&exe);
    cmd.args(launcher::play_args(info));
    console::no_window_if_detached(&mut cmd);
    let status = cmd
        .status()
        .with_context(|| format!("start {}", exe.display()))?;
    std::process::exit(status.code().unwrap_or(1));
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

/// Build the picker's event loop.
///
/// winit 0.30 delivers `DroppedFile` on X11, Windows and macOS but has no
/// drag-and-drop on Wayland at all, so a disc dropped on a native Wayland
/// window is silently ignored. The picker is one small window that needs
/// nothing Wayland-specific, so on a Wayland session that also offers
/// XWayland (`DISPLAY` set) it opens as an X11 client instead: the compositor
/// bridges a drag from a Wayland file manager into XWayland, and the drop
/// arrives. The game itself runs in a fresh process on the default backend.
/// `LEGAIA_LAUNCHER_WAYLAND=1` keeps the picker native (no drop; the file
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
        log::info!("launcher: Wayland session - opening the picker through XWayland so drops work");
        builder.with_x11();
    }
    builder.build().context("create event loop")
}

/// Whether the picker should open through XWayland (see `picker_event_loop`).
#[cfg_attr(any(not(unix), target_os = "macos"), allow(dead_code))]
fn prefer_x11_for_drop(wayland: bool, x11: bool, keep_wayland: bool) -> bool {
    wayland && x11 && !keep_wayland
}

/// Open the picker window; returns the validated disc, or `None` on quit.
fn pick(problem: DiscProblem, settings_file: Option<PathBuf>) -> Result<Option<DiscInfo>> {
    let mut app = PickerApp {
        win: EngineWindow::new(),
        font: legaia_font::Font::placeholder(),
        atlas: None,
        reason: problem.to_string(),
        first_run: problem == DiscProblem::NotSet,
        status: None,
        typed: String::new(),
        chosen: None,
        settings_file,
        screenshot: std::env::var_os(SCREENSHOT_ENV).map(PathBuf::from),
    };
    let event_loop = picker_event_loop()?;
    event_loop
        .run_app(&mut app)
        .context("launcher event loop")?;
    Ok(app.chosen)
}

struct PickerApp {
    win: EngineWindow,
    font: legaia_font::Font,
    atlas: Option<UploadedFontAtlas>,
    /// Why the picker is showing (first run, disc moved, wrong disc...).
    reason: String,
    first_run: bool,
    /// Outcome of the last pick attempt, if it failed.
    status: Option<String>,
    /// Path typed into the window (fallback when no file dialog is available).
    typed: String,
    chosen: Option<DiscInfo>,
    settings_file: Option<PathBuf>,
    screenshot: Option<PathBuf>,
}

const INK: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
const DIM: [f32; 4] = [0.62, 0.68, 0.78, 1.0];
const GOLD: [f32; 4] = [1.0, 0.85, 0.35, 1.0];
const WARN: [f32; 4] = [1.0, 0.55, 0.35, 1.0];

/// Virtual stage the screen is laid out on; scaled up by an integer factor.
const STAGE_W: u32 = 480;
const STAGE_H: u32 = 360;
/// Line pitch and page margin, in stage pixels. The character advance is
/// measured off the font (its layout adds spacing to the nominal width).
const LINE: i32 = 16;
const MARGIN: i32 = 16;

impl PickerApp {
    fn try_disc(&mut self, path: &Path, evl: &ActiveEventLoop) {
        match launcher::validate_disc(path) {
            Ok(info) => {
                if let Some(f) = &self.settings_file {
                    if let Err(e) = launcher::remember(f, &info) {
                        log::warn!("launcher: could not save settings ({e:#})");
                    } else {
                        log::info!("launcher: remembered {} in {}", path.display(), f.display());
                    }
                }
                self.chosen = Some(info);
                // Close the picker while the loop still runs. Dropped after
                // `run_app` returns instead, the X11 destroy request is never
                // flushed and the frozen picker stays on screen behind the
                // game for the whole session (seen under Xvfb).
                self.atlas = None;
                if let Some(w) = self.win.window.as_deref() {
                    w.set_visible(false);
                }
                self.win.renderer = None;
                self.win.window = None;
                evl.exit();
            }
            Err(p) => {
                log::warn!("launcher: {p}");
                self.status = Some(p.to_string());
                self.win.request_redraw();
            }
        }
    }

    fn browse(&mut self, evl: &ActiveEventLoop) {
        let mut dialog = rfd::FileDialog::new()
            .set_title("Choose your Legend of Legaia (USA) disc image")
            .add_filter("PlayStation disc image", &["bin", "cue", "BIN", "CUE"]);
        if let Some(w) = self.win.window.as_deref() {
            dialog = dialog.set_parent(w);
        }
        match dialog.pick_file() {
            Some(p) => self.try_disc(&p, evl),
            None => {
                self.status = Some(
                    "No file chosen. If no dialog appeared, drag the .bin onto this window, \
                     or type its full path and press Enter."
                        .into(),
                );
                self.win.request_redraw();
            }
        }
    }

    fn lines(&self, cols: usize) -> Vec<(String, [f32; 4])> {
        let mut out: Vec<(String, [f32; 4])> = Vec::new();
        let para = |text: &str, ink: [f32; 4], out: &mut Vec<(String, [f32; 4])>| {
            for l in wrap(text, cols) {
                out.push((l, ink));
            }
        };
        para("LEGEND OF LEGAIA - engine launcher", GOLD, &mut out);
        out.push((String::new(), INK));
        para(
            &self.reason,
            if self.first_run { INK } else { WARN },
            &mut out,
        );
        if let Some(s) = &self.status {
            out.push((String::new(), INK));
            para(s, WARN, &mut out);
        }
        out.push((String::new(), INK));
        if self.typed.is_empty() {
            para(
                "Press Enter or click to browse for the disc image (.bin or .cue).",
                INK,
                &mut out,
            );
            para(
                "You can also drag the file onto this window, or type its path.",
                DIM,
                &mut out,
            );
        } else {
            para("Path (Enter to use it, Esc to clear):", INK, &mut out);
            para(&format!("{}_", self.typed), GOLD, &mut out);
        }
        out.push((String::new(), INK));
        para(
            "No game data is included - use your own disc. The choice is \
             remembered. Esc quits.",
            DIM,
            &mut out,
        );
        if let Some(f) = &self.settings_file {
            para(&format!("Settings: {}", f.display()), DIM, &mut out);
        }
        out
    }

    fn draws(&self, surface: (u32, u32)) -> Vec<TextDraw> {
        let scale = (surface.0 / STAGE_W).min(surface.1 / STAGE_H).max(1) as i32;
        let advance = self
            .font
            .layout_ascii("MMMMMMMMMM")
            .advance_x
            .div_ceil(10)
            .max(1) as i32;
        let cols = ((surface.0 as i32 / scale - 2 * MARGIN) / advance).max(16) as usize;
        let mut draws = Vec::new();
        for (i, (text, ink)) in self.lines(cols).iter().enumerate() {
            let layout = self.font.layout_ascii(text);
            let pen = (MARGIN, MARGIN + i as i32 * LINE);
            for d in legaia_engine_render::text_draws_for(&layout, pen, *ink) {
                draws.push(TextDraw {
                    dst: (
                        d.dst.0 * scale,
                        d.dst.1 * scale,
                        d.dst.2 * scale as u32,
                        d.dst.3 * scale as u32,
                    ),
                    ..d
                });
            }
        }
        draws
    }

    fn redraw(&mut self, evl: &ActiveEventLoop) {
        let (Some(r), Some(atlas)) = (self.win.renderer(), self.atlas.as_ref()) else {
            return;
        };
        let draws = self.draws(r.surface_size());
        let overlay = TextOverlay {
            atlas,
            draws: &draws,
            blend: &[],
        };
        if let Some(path) = self.screenshot.take() {
            match r
                .capture_rgba(RenderTarget::TextOnly(&overlay))
                .and_then(|img| crate::window::write_capture_png(&path, &img))
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
}

impl ApplicationHandler for PickerApp {
    fn resumed(&mut self, evl: &ActiveEventLoop) {
        if !self.win.open(evl, "Legend of Legaia - choose your disc") {
            return;
        }
        if let Some(r) = self.win.renderer() {
            match r.upload_font(&self.font) {
                Ok(a) => self.atlas = Some(a),
                Err(e) => log::error!("launcher font upload: {e:#}"),
            }
        }
        self.win.request_redraw();
    }

    fn window_event(&mut self, evl: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => evl.exit(),
            WindowEvent::Resized(s) => {
                self.win.handle_resize(s.width, s.height);
                self.win.request_redraw();
            }
            WindowEvent::RedrawRequested => self.redraw(evl),
            WindowEvent::DroppedFile(p) => self.try_disc(&p, evl),
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => self.browse(evl),
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                match &event.logical_key {
                    Key::Named(NamedKey::Escape) => {
                        if self.typed.is_empty() {
                            evl.exit();
                        } else {
                            self.typed.clear();
                        }
                    }
                    Key::Named(NamedKey::Enter) => {
                        if self.typed.is_empty() {
                            self.browse(evl);
                        } else {
                            let p = PathBuf::from(clean_typed_path(&self.typed));
                            self.try_disc(&p, evl);
                        }
                    }
                    Key::Named(NamedKey::Backspace) => {
                        self.typed.pop();
                    }
                    _ => {
                        if let Some(t) = &event.text {
                            self.typed.extend(t.chars().filter(|c| !c.is_control()));
                        }
                    }
                }
                self.win.request_redraw();
            }
            _ => {}
        }
    }
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

/// Word-wrap `text` to `cols` characters, breaking words longer than a line
/// (paths) mid-word. `|` is the dialog font's line break, so it is replaced.
fn wrap(text: &str, cols: usize) -> Vec<String> {
    let cols = cols.max(1);
    let mut lines = Vec::new();
    let mut cur = String::new();
    for word in text.replace('|', "/").split(' ') {
        let mut word: Vec<char> = word.chars().collect();
        loop {
            let used = cur.chars().count();
            let sep = usize::from(used > 0);
            if used + sep + word.len() <= cols {
                if sep == 1 {
                    cur.push(' ');
                }
                cur.extend(word.iter());
                break;
            }
            if used > 0 {
                lines.push(std::mem::take(&mut cur));
                continue;
            }
            // A single word wider than the line: hard-break it.
            let rest = word.split_off(cols);
            lines.push(word.into_iter().collect());
            word = rest;
        }
    }
    if !cur.is_empty() || lines.is_empty() {
        lines.push(cur);
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrap_breaks_words_and_long_paths() {
        assert_eq!(wrap("aa bb cc", 5), vec!["aa bb", "cc"]);
        assert_eq!(wrap("abcdefghij", 4), vec!["abcd", "efgh", "ij"]);
        assert_eq!(wrap("", 4), vec![""]);
        assert!(
            wrap("x /very/long/path/name.bin", 8)
                .iter()
                .all(|l| l.chars().count() <= 8)
        );
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
        let mut argv = vec![OsString::from("legaia-engine")];
        argv.extend(launcher::play_args(&info));
        match Cli::try_parse_from(argv).unwrap().cmd {
            Some(Cmd::PlayWindow { disc, boot_ui, .. }) => {
                assert_eq!(disc.as_deref(), Some(Path::new("/g/l.bin")));
                assert!(boot_ui);
            }
            other => panic!("expected play-window, got {other:?}"),
        }
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
}
