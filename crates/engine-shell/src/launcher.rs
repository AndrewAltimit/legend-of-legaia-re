//! First-run launcher logic: the remembered disc path, disc validation, and
//! the "play now or ask for a disc" decision.
//!
//! `legaia-engine` run with **no subcommand** (a double-click, a desktop
//! shortcut) goes through here. The window half - the picker screen - lives
//! in the binary (`legaia-engine/launcher_window.rs`); everything in this
//! module is pure enough to test without a display or a disc:
//!
//! - [`LauncherSettings`] is the TOML file in the platform config directory
//!   ([`settings_path`]) holding the disc path the user last picked.
//! - [`validate_disc`] opens a candidate image the same way
//!   [`crate::BootSession::open_disc`] will (`.cue` sheets resolve to their
//!   `.bin`, the ISO9660 tree is walked by [`legaia_engine_core::DiscVfs`])
//!   and checks it is the disc the engine runs on: `SYSTEM.CNF` boots
//!   `SCUS_942.54` (Legend of Legaia, USA), and `PROT.DAT` is present.
//! - [`decide`] turns the saved settings into [`LaunchDecision::Play`] or
//!   [`LaunchDecision::Prompt`] with the reason the picker shows.
//!
//! The settings file stores a path only. No disc bytes are copied anywhere.

use std::fmt;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// Directory name under the platform config / data directories.
pub const APP_DIR: &str = "legaia-engine";
/// Settings file name inside [`config_dir`].
pub const SETTINGS_FILE: &str = "launcher.toml";
/// The boot executable of the disc the engine runs on.
pub const USA_EXECUTABLE: &str = "SCUS_942.54";

/// Environment override for the config directory (tests, portable installs).
pub const CONFIG_DIR_ENV: &str = "LEGAIA_ENGINE_CONFIG_DIR";
/// Environment override for the data directory (saves, key bindings, options).
pub const DATA_DIR_ENV: &str = "LEGAIA_ENGINE_DATA_DIR";

/// Where [`SETTINGS_FILE`] lives: `$LEGAIA_ENGINE_CONFIG_DIR`, else the
/// platform config dir (`~/.config/legaia-engine` on Linux,
/// `%APPDATA%\legaia-engine` on Windows,
/// `~/Library/Application Support/legaia-engine` on macOS).
pub fn config_dir() -> Option<PathBuf> {
    env_dir(CONFIG_DIR_ENV).or_else(|| dirs::config_dir().map(|d| d.join(APP_DIR)))
}

/// The working directory a launcher-started game runs in, so the
/// cwd-relative `legaia-input.toml`, `legaia-options.toml` and `saves/`
/// land in one per-user place no matter where the binary was started from:
/// `$LEGAIA_ENGINE_DATA_DIR`, else the platform data dir
/// (`~/.local/share/legaia-engine`, `%APPDATA%\legaia-engine`,
/// `~/Library/Application Support/legaia-engine`).
pub fn data_dir() -> Option<PathBuf> {
    env_dir(DATA_DIR_ENV).or_else(|| dirs::data_dir().map(|d| d.join(APP_DIR)))
}

fn env_dir(var: &str) -> Option<PathBuf> {
    std::env::var_os(var)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

/// Full path of the settings file, if a config directory is known.
pub fn settings_path() -> Option<PathBuf> {
    config_dir().map(|d| d.join(SETTINGS_FILE))
}

/// The launcher's persisted state.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LauncherSettings {
    /// The disc image the user picked (`.bin`, or the `.cue` they chose).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disc: Option<PathBuf>,
}

impl LauncherSettings {
    /// Read the settings file. A missing file is the first-run case and
    /// yields the empty default; a file that exists but does not parse is an
    /// error (the caller re-prompts rather than guessing).
    pub fn load(path: &Path) -> Result<Self> {
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(e).with_context(|| format!("read {}", path.display())),
        };
        toml::from_str(&text).with_context(|| format!("parse {}", path.display()))
    }

    /// Write the settings file, creating its directory.
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
        }
        let text = toml::to_string(self).context("serialise launcher settings")?;
        std::fs::write(path, text).with_context(|| format!("write {}", path.display()))
    }
}

/// A disc image that passed [`validate_disc`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscInfo {
    /// The path as chosen (kept as-is so a `.cue` stays a `.cue`).
    pub path: PathBuf,
    /// The raw `.bin` the engine reads (`path` itself unless it was a cue).
    pub bin: PathBuf,
    /// `SYSTEM.CNF`'s boot executable.
    pub executable: String,
}

/// Why a disc cannot be used, phrased for the picker screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiscProblem {
    /// No disc has been chosen yet (first run).
    NotSet,
    /// The settings file exists but is unreadable or malformed.
    BadSettings(String),
    /// No file at this path.
    Missing(PathBuf),
    /// The remembered disc's file is gone (moved, renamed, drive unplugged).
    Moved(PathBuf),
    /// The file exists but is not a readable Mode2/2352 PlayStation image.
    Unreadable { path: PathBuf, reason: String },
    /// A readable PlayStation disc, but not Legend of Legaia (USA).
    WrongDisc { path: PathBuf, reason: String },
}

impl fmt::Display for DiscProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DiscProblem::NotSet => {
                write!(f, "Choose your Legend of Legaia (USA) disc image to start.")
            }
            DiscProblem::BadSettings(why) => write!(
                f,
                "The saved launcher settings could not be read ({why}). Choose the disc again."
            ),
            DiscProblem::Missing(p) => write!(f, "There is no file at {}", p.display()),
            DiscProblem::Moved(p) => write!(
                f,
                "The disc image used last time is no longer there: {}",
                p.display()
            ),
            DiscProblem::Unreadable { path, reason } => {
                write!(f, "{} is not a usable disc image: {reason}", path.display())
            }
            DiscProblem::WrongDisc { path, reason } => {
                write!(
                    f,
                    "{} is not Legend of Legaia (USA): {reason}",
                    path.display()
                )
            }
        }
    }
}

/// Check that `path` is a Legend of Legaia (USA) disc image the engine can
/// boot. Opens the image through the same reader the game uses, so a disc
/// that passes here is one [`crate::BootSession::open_disc`] accepts.
pub fn validate_disc(path: &Path) -> std::result::Result<DiscInfo, DiscProblem> {
    use legaia_engine_core::Vfs;

    if !path.exists() {
        return Err(DiscProblem::Missing(path.to_path_buf()));
    }
    let unreadable = |reason: String| DiscProblem::Unreadable {
        path: path.to_path_buf(),
        reason,
    };
    let wrong = |reason: String| DiscProblem::WrongDisc {
        path: path.to_path_buf(),
        reason,
    };
    if path.is_dir() {
        return Err(unreadable(
            "it is a folder; choose the .bin (or .cue) file".into(),
        ));
    }
    let bin = legaia_iso::raw::resolve_disc_path(path).map_err(|e| unreadable(e.to_string()))?;
    let vfs = legaia_engine_core::DiscVfs::open(&bin).map_err(|e| unreadable(format!("{e:#}")))?;
    let cnf = vfs
        .read("SYSTEM.CNF")
        .map_err(|_| wrong("no SYSTEM.CNF - not a PlayStation game disc".into()))?;
    let detected = legaia_iso::region::parse(&cnf).map_err(|e| wrong(format!("{e:#}")))?;
    if !detected.executable.eq_ignore_ascii_case(USA_EXECUTABLE) {
        return Err(wrong(format!(
            "it boots {} ({}); the engine needs the USA disc, which boots {USA_EXECUTABLE}",
            detected.executable,
            detected.region.name()
        )));
    }
    if !vfs.exists("PROT.DAT") {
        return Err(wrong("PROT.DAT is missing from the disc".into()));
    }
    let exe = vfs
        .read(USA_EXECUTABLE)
        .map_err(|e| unreadable(format!("cannot read {USA_EXECUTABLE}: {e:#}")))?;
    if !exe.starts_with(b"PS-X EXE") {
        return Err(unreadable(format!(
            "{USA_EXECUTABLE} is damaged (no PS-X EXE header)"
        )));
    }
    Ok(DiscInfo {
        path: path.to_path_buf(),
        bin,
        executable: detected.executable,
    })
}

/// What the launcher does on start-up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaunchDecision {
    /// The remembered disc is good: start the game on it.
    Play(DiscInfo),
    /// Show the picker, with this reason on screen.
    Prompt(DiscProblem),
}

/// Decide from the loaded settings (or the error loading them) whether to
/// play straight away. `validate` is [`validate_disc`] in the binary and a
/// stub in tests.
pub fn decide(
    settings: Result<LauncherSettings>,
    validate: impl Fn(&Path) -> std::result::Result<DiscInfo, DiscProblem>,
) -> LaunchDecision {
    let settings = match settings {
        Ok(s) => s,
        Err(e) => return LaunchDecision::Prompt(DiscProblem::BadSettings(format!("{e:#}"))),
    };
    let Some(disc) = settings.disc else {
        return LaunchDecision::Prompt(DiscProblem::NotSet);
    };
    match validate(&disc) {
        Ok(info) => LaunchDecision::Play(info),
        // A remembered path that vanished reads as "moved", not "mistyped".
        Err(DiscProblem::Missing(p)) => LaunchDecision::Prompt(DiscProblem::Moved(p)),
        Err(p) => LaunchDecision::Prompt(p),
    }
}

/// Remember `info` as the disc to boot next time.
pub fn remember(settings_file: &Path, info: &DiscInfo) -> Result<()> {
    let mut s = LauncherSettings::load(settings_file).unwrap_or_default();
    s.disc = Some(info.path.clone());
    s.save(settings_file)
}

/// The `play-window` argument list a launcher start boots: the title screen
/// (`--boot-ui`) on the validated disc. Everything else keeps its default.
pub fn play_args(info: &DiscInfo) -> Vec<std::ffi::OsString> {
    vec![
        "play-window".into(),
        "--disc".into(),
        info.bin.clone().into_os_string(),
        "--boot-ui".into(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok_info(p: &Path) -> DiscInfo {
        DiscInfo {
            path: p.to_path_buf(),
            bin: p.to_path_buf(),
            executable: USA_EXECUTABLE.into(),
        }
    }

    #[test]
    fn first_run_prompts_not_set() {
        let d = decide(Ok(LauncherSettings::default()), |p| Ok(ok_info(p)));
        assert_eq!(d, LaunchDecision::Prompt(DiscProblem::NotSet));
    }

    #[test]
    fn valid_saved_disc_plays() {
        let s = LauncherSettings {
            disc: Some("/games/legaia.bin".into()),
        };
        let d = decide(Ok(s), |p| Ok(ok_info(p)));
        assert_eq!(
            d,
            LaunchDecision::Play(ok_info(Path::new("/games/legaia.bin")))
        );
    }

    #[test]
    fn invalid_saved_disc_reprompts_with_reason() {
        let s = LauncherSettings {
            disc: Some("/games/other.bin".into()),
        };
        let d = decide(Ok(s), |p| {
            Err(DiscProblem::WrongDisc {
                path: p.to_path_buf(),
                reason: "boots SLUS_000.01".into(),
            })
        });
        match d {
            LaunchDecision::Prompt(DiscProblem::WrongDisc { path, .. }) => {
                assert_eq!(path, Path::new("/games/other.bin"))
            }
            other => panic!("expected a WrongDisc prompt, got {other:?}"),
        }
    }

    #[test]
    fn bad_settings_reprompts() {
        let d = decide(Err(anyhow::anyhow!("garbage")), |p| Ok(ok_info(p)));
        assert!(matches!(
            d,
            LaunchDecision::Prompt(DiscProblem::BadSettings(_))
        ));
    }

    #[test]
    fn missing_path_is_missing_not_unreadable() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("gone.bin");
        assert_eq!(validate_disc(&p), Err(DiscProblem::Missing(p.clone())));
        // Through `decide` with the real validator: the remembered-but-moved case.
        let s = LauncherSettings {
            disc: Some(p.clone()),
        };
        assert_eq!(
            decide(Ok(s), validate_disc),
            LaunchDecision::Prompt(DiscProblem::Moved(p))
        );
    }

    #[test]
    fn non_disc_file_is_unreadable() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("notes.bin");
        std::fs::write(&p, vec![0u8; 4096]).unwrap();
        assert!(matches!(
            validate_disc(&p),
            Err(DiscProblem::Unreadable { .. })
        ));
        assert!(matches!(
            validate_disc(dir.path()),
            Err(DiscProblem::Unreadable { .. })
        ));
    }

    #[test]
    fn cue_pointing_at_missing_bin_is_unreadable() {
        let dir = tempfile::tempdir().unwrap();
        let cue = dir.path().join("game.cue");
        std::fs::write(&cue, "FILE \"game.bin\" BINARY\n  TRACK 01 MODE2/2352\n").unwrap();
        match validate_disc(&cue) {
            Err(DiscProblem::Unreadable { reason, .. }) => assert!(reason.contains("game.bin")),
            other => panic!("expected Unreadable, got {other:?}"),
        }
    }

    #[test]
    fn settings_round_trip_and_missing_file_is_default() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("nested").join(SETTINGS_FILE);
        assert_eq!(
            LauncherSettings::load(&f).unwrap(),
            LauncherSettings::default()
        );
        let info = ok_info(Path::new("/games/Legend of Legaia (USA).cue"));
        remember(&f, &info).unwrap();
        let back = LauncherSettings::load(&f).unwrap();
        assert_eq!(back.disc.as_deref(), Some(info.path.as_path()));
    }

    #[test]
    fn malformed_settings_file_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join(SETTINGS_FILE);
        std::fs::write(&f, "disc = [not toml").unwrap();
        assert!(LauncherSettings::load(&f).is_err());
        assert!(matches!(
            decide(LauncherSettings::load(&f), |p| Ok(ok_info(p))),
            LaunchDecision::Prompt(DiscProblem::BadSettings(_))
        ));
    }

    #[test]
    fn play_args_boot_the_title_screen_on_the_bin() {
        let info = DiscInfo {
            path: "/g/l.cue".into(),
            bin: "/g/l.bin".into(),
            executable: USA_EXECUTABLE.into(),
        };
        let a = play_args(&info);
        assert_eq!(a[0], "play-window");
        assert_eq!(a[2], "/g/l.bin");
        assert!(a.iter().any(|s| s == "--boot-ui"));
    }

    /// The real disc validates (disc-gated: skips and passes without
    /// `LEGAIA_DISC_BIN`).
    #[test]
    fn real_disc_validates() {
        let Some(disc) = std::env::var_os("LEGAIA_DISC_BIN").map(PathBuf::from) else {
            eprintln!("[skip] LEGAIA_DISC_BIN unset");
            return;
        };
        if !disc.exists() {
            eprintln!("[skip] LEGAIA_DISC_BIN does not exist");
            return;
        }
        let info = validate_disc(&disc).expect("retail disc validates");
        assert_eq!(info.executable, USA_EXECUTABLE);
        assert_eq!(info.bin, disc);
        eprintln!("[ran] {} validates ({})", disc.display(), info.executable);
        // Sibling dumps, when present: the USA `.cue` resolves to its `.bin`;
        // another region's disc reads fine but is refused as the wrong disc.
        let Some(dir) = disc.parent() else { return };
        for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let p = entry.path();
            if p.extension().and_then(|e| e.to_str()) != Some("cue") {
                continue;
            }
            match validate_disc(&p) {
                Ok(i) => {
                    assert_eq!(i.path, p);
                    assert_eq!(i.executable, USA_EXECUTABLE);
                    eprintln!("[ran] {} -> {}", p.display(), i.bin.display());
                }
                Err(e @ DiscProblem::WrongDisc { .. }) => eprintln!("[ran] refused: {e}"),
                Err(e) => panic!("{}: unexpected {e:?}", p.display()),
            }
        }
    }
}
