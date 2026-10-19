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
//! exits with a failure.

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
}

impl Game {
    /// Start `engine` on `bin`. The working directory is `data_dir` (the
    /// engine's data folder, [`engine_data_dir`]; created when missing);
    /// stdout and stderr go to [`GAME_LOG`] inside it.
    pub fn start(engine: &Path, bin: &Path, data_dir: Option<&Path>) -> anyhow::Result<Self> {
        use anyhow::Context;
        let mut cmd = Command::new(engine);
        cmd.args(play_args(bin)).stdin(Stdio::null());
        let mut log = None;
        if let Some(dir) = data_dir
            && std::fs::create_dir_all(dir).is_ok()
        {
            cmd.current_dir(dir);
            let path = dir.join(GAME_LOG);
            if let Ok(f) = std::fs::File::create(&path) {
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
        Ok(Self { child, log })
    }

    /// `None` while the game runs; its exit once it has ended (an error
    /// reading the status counts as a failed exit).
    pub fn poll(&mut self) -> Option<Result<(), String>> {
        match self.child.try_wait() {
            Ok(None) => None,
            Ok(Some(status)) if status.success() => Some(Ok(())),
            Ok(Some(status)) => Some(Err(status.to_string())),
            Err(e) => Some(Err(e.to_string())),
        }
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
        let mut game = Game::start(&engine, Path::new("/g/l.bin"), Some(&data)).unwrap();
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

    #[test]
    fn log_tail_keeps_the_last_lines() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("log");
        std::fs::write(&p, "a\n\nb\nc\n").unwrap();
        assert_eq!(log_tail(&p, 2), vec!["b".to_string(), "c".to_string()]);
        assert!(log_tail(&dir.path().join("none"), 2).is_empty());
    }
}
