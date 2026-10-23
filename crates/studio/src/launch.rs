//! Starting the game (`legaia-engine play-window`) on the open disc, and
//! showing the user where things went: opening a folder in the platform's
//! file manager.
//!
//! The game is a separate program shipped in the same release folder
//! (`docs/tooling/releases.md`), so Studio starts it as a child process
//! rather than linking the engine. The child gets the same argument list the
//! engine's own first-run launcher boots (`play-window --disc <bin>
//! --boot-ui`, `legaia_engine_shell::launcher::play_args`) and runs in the
//! same per-user data folder (`launcher::data_dir`), so saves, key bindings
//! and options are shared with a game started by double-clicking it. Its log
//! goes to a file in that folder, which the error line names when the game
//! exits with a failure. A game that dies in the X11 startup race (`SIGSEGV`
//! before its event loop opens) is started once more ([`Game::poll`]).

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use crate::library::{Library, Source, USA_EXE};

/// The engine binary's file name on this platform.
pub fn engine_file_name() -> String {
    format!("legaia-engine{}", std::env::consts::EXE_SUFFIX)
}

/// The engine binary shipped beside `studio_exe`: in the same folder (the
/// release archive, a `target/<profile>` build), or - when Studio runs from
/// inside the macOS `Legaia Studio.app/Contents/MacOS/` - in the archive
/// folder three levels up.
pub fn find_engine(studio_exe: &Path) -> Option<PathBuf> {
    let name = engine_file_name();
    studio_exe
        .ancestors()
        .skip(1)
        .take(4)
        .map(|d| d.join(&name))
        .find(|p| p.is_file())
}

/// The per-user folder the game keeps its saves, key bindings and options
/// in, the one its own launcher runs it from: `$LEGAIA_ENGINE_DATA_DIR`,
/// else `<platform data dir>/legaia-engine`.
pub fn engine_data_dir() -> Option<PathBuf> {
    std::env::var_os("LEGAIA_ENGINE_DATA_DIR")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| dirs::data_dir().map(|d| d.join("legaia-engine")))
}

/// The engine's launcher settings file, `launcher.toml` in its config
/// folder: `$LEGAIA_ENGINE_CONFIG_DIR`, else `<platform config
/// dir>/legaia-engine` (`legaia_engine_shell::launcher::settings_path`).
pub fn engine_launcher_settings() -> Option<PathBuf> {
    std::env::var_os("LEGAIA_ENGINE_CONFIG_DIR")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| dirs::config_dir().map(|d| d.join("legaia-engine")))
        .map(|d| d.join("launcher.toml"))
}

/// Record `disc` as the disc the game boots, the way the engine's own disc
/// picker does (`launcher::remember`): the `disc` key of `settings`, so a
/// later double-click on the game starts on it without asking. Only that
/// key is written; any other key in the file is kept as it was. A file that
/// exists but does not parse is left alone (the game re-asks for the disc
/// in that case anyway) and reported as an error.
pub fn record_disc_for_engine(settings: &Path, disc: &Path) -> anyhow::Result<()> {
    use anyhow::Context;
    let mut table = match std::fs::read_to_string(settings) {
        Ok(text) => text
            .parse::<toml::Table>()
            .with_context(|| format!("{} does not parse; left as it is", settings.display()))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => toml::Table::new(),
        Err(e) => return Err(e).with_context(|| format!("reading {}", settings.display())),
    };
    let disc = disc
        .to_str()
        .with_context(|| format!("{} is not valid UTF-8", disc.display()))?;
    if table.get("disc").and_then(|v| v.as_str()) == Some(disc) {
        return Ok(());
    }
    table.insert("disc".into(), toml::Value::String(disc.to_string()));
    if let Some(dir) = settings.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    std::fs::write(settings, toml::to_string(&table)?)
        .with_context(|| format!("writing {}", settings.display()))
}

/// The `play-window` switches the engine's launcher keeps in `settings`
/// (`psx_render`, `retail`; `legaia_engine_shell::launcher::LauncherSettings`),
/// as arguments. A missing or unreadable file gives none.
pub fn launch_switches(settings: &Path) -> Vec<std::ffi::OsString> {
    let Some(table) = std::fs::read_to_string(settings)
        .ok()
        .and_then(|t| t.parse::<toml::Table>().ok())
    else {
        return Vec::new();
    };
    [("psx_render", "--psx-render"), ("retail", "--retail")]
        .into_iter()
        .filter(|(key, _)| table.get(*key).and_then(|v| v.as_bool()) == Some(true))
        .map(|(_, flag)| flag.into())
        .collect()
}

/// The log file a Studio-started game writes into.
pub const GAME_LOG: &str = "legaia-studio-game.log";

/// The disc the game can boot from the open source, or why it cannot - a
/// sentence for the Play button's hover text.
pub fn playable_disc(lib: &Library) -> Result<PathBuf, String> {
    let Source::Disc(bin) = &lib.source else {
        return Err("The game boots from a disc image; open the .bin or .cue, \
                    not an extracted folder."
            .into());
    };
    match lib.exe.as_deref() {
        Some(exe) if exe.eq_ignore_ascii_case(USA_EXE) => Ok(bin.clone()),
        Some(exe) => Err(format!(
            "The game runs on the USA disc, which boots {USA_EXE}; this disc boots {exe}."
        )),
        None => Err(format!(
            "This disc has no readable SYSTEM.CNF; the game needs the USA disc ({USA_EXE})."
        )),
    }
}

/// The engine's argument list for `bin`: the title screen on that disc.
pub fn play_args(bin: &Path) -> Vec<std::ffi::OsString> {
    vec![
        "play-window".into(),
        "--disc".into(),
        bin.as_os_str().to_owned(),
        "--boot-ui".into(),
    ]
}

/// A game Studio started.
pub struct Game {
    child: Child,
    /// Where its log goes, when a log file could be created.
    pub log: Option<PathBuf>,
    engine: PathBuf,
    args: Vec<std::ffi::OsString>,
    data_dir: Option<PathBuf>,
    /// The startup race was already retried once.
    relaunched: bool,
}

/// The line `play-window` logs once its event loop exists
/// (`legaia_parity::window_child::EVENT_LOOP_OPEN_MARKER`, repeated here
/// because Studio does not link the parity crate). A game that dies of
/// `SIGSEGV` before writing it died inside `winit`'s `EventLoop::new`, in
/// libXi's first-use race against XInput2 raw events: pointer motion on the
/// display at the wrong moment (`docs/tooling/host-drift.md`). Studio starts
/// such a game once more.
pub const EVENT_LOOP_OPEN_MARKER: &str = "play-window: event loop open";

/// The error line's reason when the game died in the startup race twice.
pub const STARTUP_RACE_FAILURE: &str = "the game crashed twice while opening its window, \
     inside the X11 input library (libXi). Moving the mouse while the game starts can \
     trigger this; start it again.";

impl Game {
    /// Start `engine` on `bin`. The working directory is `data_dir` (the
    /// engine's data folder, [`engine_data_dir`]; created when missing);
    /// stdout and stderr go to [`GAME_LOG`] inside it.
    pub fn start(
        engine: &Path,
        bin: &Path,
        extra: &[std::ffi::OsString],
        data_dir: Option<&Path>,
    ) -> anyhow::Result<Self> {
        let mut args = play_args(bin);
        args.extend(extra.iter().cloned());
        let (child, log) = spawn(engine, &args, data_dir, false)?;
        Ok(Self {
            child,
            log,
            engine: engine.to_path_buf(),
            args,
            data_dir: data_dir.map(Path::to_path_buf),
            relaunched: false,
        })
    }

    /// `None` while the game runs; its exit once it has ended (an error
    /// reading the status counts as a failed exit). A first death in the X11
    /// startup race ([`EVENT_LOOP_OPEN_MARKER`]) starts the game once more
    /// and reads as still running; a second one is reported with
    /// [`STARTUP_RACE_FAILURE`].
    pub fn poll(&mut self) -> Option<Result<(), String>> {
        let status = match self.child.try_wait() {
            Ok(None) => return None,
            Ok(Some(status)) => status,
            Err(e) => return Some(Err(e.to_string())),
        };
        if status.success() {
            return Some(Ok(()));
        }
        if !died_in_startup_race(&status, self.log.as_deref()) {
            return Some(Err(status.to_string()));
        }
        if self.relaunched {
            return Some(Err(format!("{status}; {STARTUP_RACE_FAILURE}")));
        }
        self.relaunched = true;
        if let Some(log) = &self.log
            && let Ok(mut f) = std::fs::OpenOptions::new().append(true).open(log)
        {
            use std::io::Write;
            let _ = writeln!(
                f,
                "[studio] the game died of SIGSEGV before its window opened \
                 (libXi startup race); starting it once more"
            );
        }
        match spawn(&self.engine, &self.args, self.data_dir.as_deref(), true) {
            Ok((child, _)) => {
                self.child = child;
                None
            }
            Err(e) => Some(Err(format!("{status}; restarting it failed: {e:#}"))),
        }
    }
}

/// Start `engine` with `args` in `data_dir`, its output in [`GAME_LOG`]
/// there: a fresh file, or appended to on a relaunch.
fn spawn(
    engine: &Path,
    args: &[std::ffi::OsString],
    data_dir: Option<&Path>,
    append: bool,
) -> anyhow::Result<(Child, Option<PathBuf>)> {
    use anyhow::Context;
    let mut cmd = Command::new(engine);
    cmd.args(args).stdin(Stdio::null());
    let mut log = None;
    if let Some(dir) = data_dir
        && std::fs::create_dir_all(dir).is_ok()
    {
        cmd.current_dir(dir);
        let path = dir.join(GAME_LOG);
        let file = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .append(append)
            .truncate(!append)
            .open(&path);
        if let Ok(f) = file {
            let err = f.try_clone().ok();
            cmd.stdout(f);
            if let Some(e) = err {
                cmd.stderr(e);
            }
            log = Some(path);
        }
    }
    #[cfg(windows)]
    {
        // legaia-engine.exe is a console program; started from this
        // GUI program it would open a console window of its own.
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let child = cmd
        .spawn()
        .with_context(|| format!("starting {}", engine.display()))?;
    Ok((child, log))
}

/// A game killed by `SIGSEGV` whose log never reached
/// [`EVENT_LOOP_OPEN_MARKER`]. With no log to read, any `SIGSEGV` counts.
fn died_in_startup_race(status: &std::process::ExitStatus, log: Option<&Path>) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        const SIGSEGV: i32 = 11;
        status.signal() == Some(SIGSEGV)
            && !log
                .and_then(|l| std::fs::read_to_string(l).ok())
                .is_some_and(|t| t.contains(EVENT_LOOP_OPEN_MARKER))
    }
    #[cfg(not(unix))]
    {
        let _ = (status, log);
        false
    }
}

/// The last `n` non-empty lines of a log file, for an error message.
pub fn log_tail(path: &Path, n: usize) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    lines[lines.len().saturating_sub(n)..]
        .iter()
        .map(|l| l.to_string())
        .collect()
}

/// Open `dir` in the platform's file manager.
pub fn open_folder(dir: &Path) -> std::io::Result<()> {
    let program = if cfg!(windows) {
        "explorer"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    Command::new(program)
        .arg(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
}

/// The output folder the Extract view proposes when none was chosen yet:
/// a folder beside the disc image, named after it.
pub fn suggested_out_dir(bin: &Path) -> Option<PathBuf> {
    let stem = bin.file_stem()?.to_string_lossy();
    Some(bin.parent()?.join(format!("{stem} - extracted")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_engine_beside_studio_or_above_the_app_bundle() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let engine = root.join(engine_file_name());
        assert_eq!(find_engine(&root.join("legaia-studio")), None);
        std::fs::write(&engine, b"").unwrap();
        assert_eq!(
            find_engine(&root.join("legaia-studio")),
            Some(engine.clone())
        );
        let bundled = root
            .join("Legaia Studio.app")
            .join("Contents")
            .join("MacOS")
            .join("legaia-studio");
        assert_eq!(find_engine(&bundled), Some(engine));
    }

    #[test]
    fn play_args_boot_the_title_screen() {
        let a = play_args(Path::new("/g/legaia.bin"));
        assert_eq!(a[0], "play-window");
        assert_eq!(a[1], "--disc");
        assert_eq!(a[2], "/g/legaia.bin");
        assert_eq!(a[3], "--boot-ui");
    }

    #[test]
    fn suggests_a_folder_beside_the_disc() {
        assert_eq!(
            suggested_out_dir(Path::new("/games/Legend of Legaia (USA).bin")),
            Some(PathBuf::from("/games/Legend of Legaia (USA) - extracted"))
        );
    }

    /// A game that fails is reported as a failed exit, its output in the
    /// log file, run in the data folder with the play arguments.
    #[cfg(unix)]
    #[test]
    fn a_failing_game_reports_its_exit_and_log() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let engine = dir.path().join(engine_file_name());
        std::fs::write(
            &engine,
            "#!/bin/sh\necho \"args $*\"\necho \"cwd $(pwd)\"\necho boom >&2\nexit 3\n",
        )
        .unwrap();
        std::fs::set_permissions(&engine, std::fs::Permissions::from_mode(0o755)).unwrap();
        let data = dir.path().join("data");
        let mut game = Game::start(&engine, Path::new("/g/l.bin"), &[], Some(&data)).unwrap();
        let res = loop {
            if let Some(r) = game.poll() {
                break r;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        };
        assert!(res.is_err(), "{res:?}");
        let log = game.log.clone().unwrap();
        assert_eq!(log, data.join(GAME_LOG));
        let text = std::fs::read_to_string(&log).unwrap();
        assert!(
            text.contains("args play-window --disc /g/l.bin --boot-ui"),
            "{text}"
        );
        assert!(text.contains(&format!("cwd {}", data.display())), "{text}");
        assert_eq!(log_tail(&log, 1), vec!["boom".to_string()]);
    }

    /// Poll `game` until it ends.
    fn wait(game: &mut Game) -> Result<(), String> {
        loop {
            if let Some(r) = game.poll() {
                return r;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }

    /// A game that dies of SIGSEGV before its event loop opens is started
    /// once more; a second such death is reported as the startup race, and
    /// a SIGSEGV after the marker is an ordinary failure with no relaunch.
    #[cfg(unix)]
    #[test]
    fn a_startup_race_death_is_relaunched_once() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let engine = dir.path().join(engine_file_name());
        let data = dir.path().join("data");
        let write_engine = |body: &str| {
            std::fs::write(&engine, format!("#!/bin/sh\necho launched\n{body}")).unwrap();
            std::fs::set_permissions(&engine, std::fs::Permissions::from_mode(0o755)).unwrap();
        };

        write_engine("kill -SEGV $$\n");
        let mut game = Game::start(&engine, Path::new("/g/l.bin"), &[], Some(&data)).unwrap();
        let err = wait(&mut game).unwrap_err();
        assert!(err.contains(STARTUP_RACE_FAILURE), "{err}");
        let text = std::fs::read_to_string(game.log.as_ref().unwrap()).unwrap();
        assert_eq!(text.matches("launched").count(), 2, "{text}");
        assert!(text.contains("[studio]"), "{text}");

        write_engine(&format!("echo '{EVENT_LOOP_OPEN_MARKER}'\nkill -SEGV $$\n"));
        let mut game = Game::start(&engine, Path::new("/g/l.bin"), &[], Some(&data)).unwrap();
        let err = wait(&mut game).unwrap_err();
        assert!(!err.contains(STARTUP_RACE_FAILURE), "{err}");
        let text = std::fs::read_to_string(game.log.as_ref().unwrap()).unwrap();
        assert_eq!(text.matches("launched").count(), 1, "{text}");
    }

    /// The engine's launcher file gains the disc and keeps every other key;
    /// a file that does not parse is not overwritten.
    #[test]
    fn records_the_disc_for_the_engine_without_clobbering() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("cfg").join("launcher.toml");
        record_disc_for_engine(&file, Path::new("/g/Legaia.cue")).unwrap();
        let t: toml::Table = std::fs::read_to_string(&file).unwrap().parse().unwrap();
        assert_eq!(t["disc"].as_str(), Some("/g/Legaia.cue"));
        std::fs::write(
            &file,
            "disc = \"/old.bin\"\nfuture_key = 7\n[extra]\nx = true\n",
        )
        .unwrap();
        record_disc_for_engine(&file, Path::new("/g/new.bin")).unwrap();
        let t: toml::Table = std::fs::read_to_string(&file).unwrap().parse().unwrap();
        assert_eq!(t["disc"].as_str(), Some("/g/new.bin"));
        assert_eq!(t["future_key"].as_integer(), Some(7));
        assert_eq!(t["extra"]["x"].as_bool(), Some(true));
        std::fs::write(&file, "not = = toml").unwrap();
        assert!(record_disc_for_engine(&file, Path::new("/g/new.bin")).is_err());
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "not = = toml");
    }

    #[test]
    fn launch_switches_follow_the_engine_launcher_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("launcher.toml");
        assert!(launch_switches(&file).is_empty(), "no file, no switches");
        std::fs::write(&file, "disc = \"/g/l.bin\"\npsx_render = true\n").unwrap();
        assert_eq!(launch_switches(&file), vec!["--psx-render"]);
        std::fs::write(&file, "retail = true\npsx_render = false\n").unwrap();
        assert_eq!(launch_switches(&file), vec!["--retail"]);
        std::fs::write(&file, "not = = toml").unwrap();
        assert!(launch_switches(&file).is_empty());
    }

    #[test]
    fn log_tail_keeps_the_last_lines() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("log");
        std::fs::write(&p, "a\n\nb\nc\n").unwrap();
        assert_eq!(log_tail(&p, 2), vec!["b".to_string(), "c".to_string()]);
        assert!(log_tail(&dir.path().join("none"), 2).is_empty());
    }
}
