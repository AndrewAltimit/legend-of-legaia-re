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
//!   [`legaia_engine_session::native::NativeSession::open_disc`] will (`.cue` sheets resolve to their
//!   `.bin`, the ISO9660 tree is walked by [`legaia_engine_scene::vfs::DiscVfs`])
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
///
/// Only what the game has no option of its own for lives here: the disc,
/// and the `play-window` launch switches with no persisted option behind
/// them (`--psx-render`, `--retail`, `--no-entry-pulse`, `--smart-monsters` and the gameplay tweaks
/// `--no-shiny-seru`, `--flee-exp`, `--exp-rate`, `--no-fortress-encounter-fix`, `--jewel-fix`). The enhancement knobs the game
/// itself remembers (lighting, fog, see-through walls, ...) are edited in
/// the game's own [`OPTIONS_FILE`], so the launcher and the in-game keys
/// change one value.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LauncherSettings {
    /// The disc image the user picked (`.bin`, or the `.cue` they chose).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disc: Option<PathBuf>,
    /// Start with PSX-faithful rasterisation (`play-window --psx-render`).
    #[serde(default, skip_serializing_if = "is_false")]
    pub psx_render: bool,
    /// Start retail-faithful (`play-window --retail`): every enhancement
    /// off for the session, nothing written back to the options file.
    #[serde(default, skip_serializing_if = "is_false")]
    pub retail: bool,
    /// Start without the scene-entry pulse enhancement
    /// (`play-window --no-entry-pulse`). Stored inverted so the shipped
    /// default (pulse on) writes nothing.
    #[serde(default, skip_serializing_if = "is_false")]
    pub no_entry_pulse: bool,
    /// Start with smarter monster targeting (`play-window --smart-monsters`).
    #[serde(default, skip_serializing_if = "is_false")]
    pub smart_monsters: bool,
    /// Start without Shiny Seru (`play-window --no-shiny-seru`). Stored
    /// inverted so the shipped default (shiny on) writes nothing.
    #[serde(default, skip_serializing_if = "is_false")]
    pub no_shiny_seru: bool,
    /// Start with run-away EXP (`play-window --flee-exp`).
    #[serde(default, skip_serializing_if = "is_false")]
    pub flee_exp: bool,
    /// The EXP rate in permille (`play-window --exp-rate`); `None` is retail's
    /// `1x`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exp_rate_permille: Option<u16>,
    /// Start without the Fortress encounter fix
    /// (`play-window --no-fortress-encounter-fix`). Stored inverted: the fix
    /// is on by default.
    #[serde(default, skip_serializing_if = "is_false")]
    pub no_fortress_encounter_fix: bool,
    /// Start with the jewel fix (`play-window --jewel-fix`).
    #[serde(default, skip_serializing_if = "is_false")]
    pub jewel_fix: bool,
    /// Start without the battle HP-readout guard
    /// (`play-window --no-hp-readout-guard`). Stored inverted: the guard is
    /// on by default.
    #[serde(default, skip_serializing_if = "is_false")]
    pub no_hp_readout_guard: bool,
    /// Spirit AP (`play-window --spirit-ap`); `None` is retail's 32.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spirit_ap: Option<i16>,
    /// Enemy-damage AP (`play-window --damage-ap`); `None` is retail's 100.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub damage_ap: Option<i16>,
    /// Flat Seru catch rate (`play-window --seru-catch-rate`); `None` keeps
    /// each Seru's own chance.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seru_catch_pct: Option<u8>,
    /// Enemy attack count in permille (`play-window --enemy-attack-count`);
    /// `None` is `1x`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enemy_attack_permille: Option<u16>,
    /// Enemy stat scale in permille (`play-window --enemy-stat-scale`);
    /// `None` is `1x`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enemy_stat_permille: Option<u16>,
    /// Start with the bonus equipment drop (`play-window --equipment-drops`).
    #[serde(default, skip_serializing_if = "is_false")]
    pub equipment_drops: bool,
}

fn is_false(b: &bool) -> bool {
    !*b
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
/// that passes here is one [`legaia_engine_session::native::NativeSession::open_disc`] accepts.
pub fn validate_disc(path: &Path) -> std::result::Result<DiscInfo, DiscProblem> {
    use legaia_engine_scene::vfs::Vfs;

    if !path.exists() {
        return Err(DiscProblem::Missing(path.to_path_buf()));
    }
    // A typed relative path is made absolute against the directory it was
    // typed in: the game runs in the data directory, where it would not
    // resolve, and the remembered path must survive a different start
    // directory.
    let absolute = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let path = absolute.as_path();
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
    let vfs =
        legaia_engine_scene::vfs::DiscVfs::open(&bin).map_err(|e| unreadable(format!("{e:#}")))?;
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

/// [`play_args`] plus the launch switches `settings` turns on.
pub fn play_args_with(info: &DiscInfo, settings: &LauncherSettings) -> Vec<std::ffi::OsString> {
    let mut args = play_args(info);
    if settings.psx_render {
        args.push("--psx-render".into());
    }
    if settings.retail {
        // `--retail` holds the two switches below off itself, and clap
        // refuses them beside it.
        args.push("--retail".into());
        return args;
    }
    if settings.no_entry_pulse {
        args.push("--no-entry-pulse".into());
    }
    if settings.smart_monsters {
        args.push("--smart-monsters".into());
    }
    if settings.no_shiny_seru {
        args.push("--no-shiny-seru".into());
    }
    if settings.flee_exp {
        args.push("--flee-exp".into());
    }
    if let Some(rate) = settings.exp_rate_permille.filter(|&r| r != EXP_RATE_RETAIL) {
        args.push("--exp-rate".into());
        args.push(format!("{}", f64::from(rate) / 1000.0).into());
    }
    if settings.no_fortress_encounter_fix {
        args.push("--no-fortress-encounter-fix".into());
    }
    if settings.jewel_fix {
        args.push("--jewel-fix".into());
    }
    if settings.no_hp_readout_guard {
        args.push("--no-hp-readout-guard".into());
    }
    // `--flag=value`, so a negative AP is never read as a flag.
    let mut kv = |flag: &str, v: String| args.push(format!("--{flag}={v}").into());
    if let Some(ap) = settings.spirit_ap.filter(|&v| v != SPIRIT_AP_RETAIL) {
        kv("spirit-ap", ap.to_string());
    }
    if let Some(ap) = settings.damage_ap.filter(|&v| v != DAMAGE_AP_RETAIL) {
        kv("damage-ap", ap.to_string());
    }
    if let Some(p) = settings.seru_catch_pct {
        kv("seru-catch-rate", p.to_string());
    }
    if let Some(m) = settings
        .enemy_attack_permille
        .filter(|&v| v != SCALE_RETAIL)
    {
        kv("enemy-attack-count", (f64::from(m) / 1000.0).to_string());
    }
    if let Some(m) = settings.enemy_stat_permille.filter(|&v| v != SCALE_RETAIL) {
        kv("enemy-stat-scale", (f64::from(m) / 1000.0).to_string());
    }
    if settings.equipment_drops {
        args.push("--equipment-drops".into());
    }
    args
}

const SPIRIT_AP_RETAIL: i16 = legaia_engine_core::world::SPIRIT_AP_RETAIL;
const DAMAGE_AP_RETAIL: i16 = legaia_engine_core::world::DAMAGE_AP_RETAIL;
const SCALE_RETAIL: u16 = legaia_engine_core::world::SCALE_RETAIL;
type Tweaks = legaia_engine_session::session_switches::GameplayTweaks;

/// Step an optional slider value through `steps` (retail first); landing on
/// retail stores `None`, so the default writes nothing.
fn step_opt<T: PartialEq + Copy>(v: &mut Option<T>, steps: &[T], dir: i8) {
    let next = Tweaks::step_in(steps, v.unwrap_or(steps[0]), dir);
    *v = (next != steps[0]).then_some(next);
}

/// The EXP rate the launcher starts from and never passes: retail's `1x`.
const EXP_RATE_RETAIL: u16 = legaia_engine_core::world::EXP_RATE_RETAIL;

/// The game's options file, cwd-relative the way `play-window` reads it; the
/// launcher runs in [`data_dir`], so this is the per-user one.
pub const OPTIONS_FILE: &str = "legaia-options.toml";
/// The game's key-binding file, beside [`OPTIONS_FILE`].
pub const INPUT_FILE: &str = "legaia-input.toml";
/// The log a launcher-started game writes, in [`data_dir`].
pub const GAME_LOG: &str = "legaia-engine-game.log";

/// One row of the launcher's option list: a game option (persisted in
/// [`OPTIONS_FILE`], the same value the in-game key toggles) or a launch
/// switch (persisted in [`LauncherSettings`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptionRow {
    Fullscreen,
    WindowSize,
    EnhancedLighting,
    TimeOfDay,
    LampShadows,
    SeeThroughWalls,
    GroundFog,
    Bloom,
    PsxRender,
    CameraDistance,
    PreciseMovement,
    ReduceFlashing,
    MusicVolume,
    SfxVolume,
    MonoDownmix,
    EntryPulse,
    SmartMonsters,
    ShinySeru,
    FleeExp,
    ExpRate,
    FortressFix,
    JewelFix,
    HpReadoutGuard,
    BonusDrop,
    SpiritAp,
    DamageAp,
    SeruCatchRate,
    EnemyAttacks,
    EnemyStats,
    RetailTileCrop,
    RetailObjectWindow,
    Retail,
}

/// The option rows in screen order, with the heading each group starts
/// under (`Some` on a group's first row).
pub const OPTION_ROWS: [(Option<&str>, OptionRow); 32] = [
    (Some("Display"), OptionRow::Fullscreen),
    (None, OptionRow::WindowSize),
    (None, OptionRow::CameraDistance),
    (None, OptionRow::PsxRender),
    (Some("Enhancements"), OptionRow::EnhancedLighting),
    (None, OptionRow::TimeOfDay),
    (None, OptionRow::LampShadows),
    (None, OptionRow::Bloom),
    (None, OptionRow::GroundFog),
    (None, OptionRow::SeeThroughWalls),
    (None, OptionRow::EntryPulse),
    (Some("Play"), OptionRow::PreciseMovement),
    (None, OptionRow::SmartMonsters),
    (None, OptionRow::ReduceFlashing),
    (Some("Gameplay tweaks"), OptionRow::ShinySeru),
    (None, OptionRow::FleeExp),
    (None, OptionRow::BonusDrop),
    (None, OptionRow::FortressFix),
    (None, OptionRow::JewelFix),
    (None, OptionRow::HpReadoutGuard),
    (Some("Battle tuning"), OptionRow::ExpRate),
    (None, OptionRow::EnemyStats),
    (None, OptionRow::EnemyAttacks),
    (None, OptionRow::SpiritAp),
    (None, OptionRow::DamageAp),
    (None, OptionRow::SeruCatchRate),
    (Some("Sound"), OptionRow::MusicVolume),
    (None, OptionRow::SfxVolume),
    (None, OptionRow::MonoDownmix),
    (Some("Original game"), OptionRow::RetailTileCrop),
    (None, OptionRow::RetailObjectWindow),
    (None, OptionRow::Retail),
];

const TIMES_OF_DAY: [&str; 4] = ["auto", "day", "dusk", "night"];

/// `OptionsState::window_scale`'s range
/// (`legaia_engine_render::window::WindowSetup::SCALES`).
const WINDOW_SCALES: (u8, u8) = (2, 6);

fn on_off(b: bool) -> String {
    if b { "On" } else { "Off" }.into()
}

fn capitalise(s: &str) -> String {
    let mut c = s.chars();
    c.next()
        .map(|f| f.to_uppercase().chain(c).collect())
        .unwrap_or_default()
}

impl OptionRow {
    /// The row's label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Fullscreen => "Fullscreen",
            Self::WindowSize => "Window size",
            Self::EnhancedLighting => "Enhanced lighting",
            Self::TimeOfDay => "Time of day",
            Self::LampShadows => "Lamp shadows",
            Self::SeeThroughWalls => "See-through walls",
            Self::GroundFog => "Ground fog",
            Self::Bloom => "Bloom",
            Self::PsxRender => "PlayStation pixel look",
            Self::CameraDistance => "Camera distance",
            Self::PreciseMovement => "Free-angle movement",
            Self::ReduceFlashing => "Reduce flashing",
            Self::MusicVolume => "Music volume",
            Self::SfxVolume => "Sound effects volume",
            Self::MonoDownmix => "Mono output",
            Self::EntryPulse => "Living ground pulse",
            Self::SmartMonsters => "Smarter monsters",
            Self::ShinySeru => "Shiny Seru",
            Self::FleeExp => "Run-away EXP",
            Self::ExpRate => "EXP rate",
            Self::FortressFix => "Fortress battles fix",
            Self::JewelFix => "Boss casts respect jewels",
            Self::HpReadoutGuard => "Battle freeze guard",
            Self::BonusDrop => "Bonus equipment drops",
            Self::SpiritAp => "Spirit AP",
            Self::DamageAp => "AP from damage",
            Self::SeruCatchRate => "Seru catch rate",
            Self::EnemyAttacks => "Enemy attack count",
            Self::EnemyStats => "Enemy strength",
            Self::RetailTileCrop => "Original ground culling",
            Self::RetailObjectWindow => "Original prop culling",
            Self::Retail => "Original game only",
        }
    }

    /// One or two sentences on what the row does, for the help line.
    pub fn help(self) -> &'static str {
        match self {
            Self::Fullscreen => {
                "Fill the screen with a borderless window. In game: F11 or Alt+Enter."
            }
            Self::WindowSize => {
                "The game window's size when not fullscreen, in steps of the \
                 PlayStation's 320x240 picture. The game still draws at any size you \
                 drag the window to."
            }
            Self::EnhancedLighting => {
                "A time-of-day mood, real light at lamps and glowing props. Off draws the \
                 game's own baked shading. In game: I."
            }
            Self::TimeOfDay => {
                "The mood enhanced lighting uses: Auto follows the scene, or force day, \
                 dusk or night. In game: F8."
            }
            Self::LampShadows => "Shadows cast by the enhanced lamp lights. In game: Y.",
            Self::SeeThroughWalls => {
                "Walls between the camera and your character fade, so you never lose \
                 sight of them. In game: F4."
            }
            Self::GroundFog => "A low drifting mist in misty and night scenes. In game: F9.",
            Self::Bloom => "Bright lights and lit windows glow softly. In game: F10.",
            Self::PsxRender => {
                "The PlayStation's wobbling vertices and colour dithering, for a CRT-era \
                 look. Off gives clean modern edges."
            }
            Self::CameraDistance => {
                "How far the field camera sits behind your character. Retail is the \
                 original framing. In game: T."
            }
            Self::PreciseMovement => {
                "Walk at any angle instead of the original eight directions. In game: R."
            }
            Self::ReduceFlashing => {
                "Softens fast-flashing light effects (a photosensitivity guard). Off shows \
                 them exactly as the original does."
            }
            Self::MusicVolume => "Music level, 0 to 10; 8 is the original mix.",
            Self::SfxVolume => "Sound effect level, 0 to 10; 8 is the original mix.",
            Self::MonoDownmix => {
                "Mix everything, voices and movies included, down to one channel for \
                 single-speaker setups. The in-game Monaural setting is the original's own \
                 mono mode."
            }
            Self::EntryPulse => {
                "Keeps the living-flesh ground of a few scenes rolling, which the original \
                 leaves still. Applies from the next scene you enter."
            }
            Self::SmartMonsters => {
                "Monsters aim single-target attacks at your weakest party member instead \
                 of picking at random. Harder than the original."
            }
            Self::ShinySeru => {
                "Now and then a Seru you can capture appears shiny: stronger, and its \
                 magic deals more damage once caught. The original has no shiny Seru."
            }
            Self::FleeExp => {
                "Running away from a battle still earns a little experience. The \
                 original gives nothing for fleeing."
            }
            Self::ExpRate => "Multiplies the experience every monster gives; 1x is the original.",
            Self::FortressFix => {
                "Fixes a bug where Jette's fortress has no battles, and its Door of Light \
                 does not work, if you escaped the floating castle with under a minute \
                 left. Other saves play the same either way."
            }
            Self::JewelFix => {
                "Elemental jewels and guards protect against the bosses' signature \
                 attacks too, which the original lets through."
            }
            Self::BonusDrop => {
                "Now and then a won battle also gives a random piece of equipment, on \
                 top of the normal drop."
            }
            Self::SpiritAp => {
                "How much AP the Spirit command charges; 32 is the original. Negative \
                 values make Spirit drain the gauge."
            }
            Self::DamageAp => {
                "AP gained when a monster hurts you, per whole health bar lost; 100 is \
                 the original. Negative values make hits drain AP."
            }
            Self::SeruCatchRate => {
                "One catch chance for every Seru you can capture. Original keeps each \
                 Seru's own chance."
            }
            Self::EnemyAttacks => {
                "Multiplies how many hits monsters land with their normal attacks; a \
                 monster always keeps at least one."
            }
            Self::EnemyStats => {
                "Multiplies every monster's health and combat stats. Experience, gold \
                 and drops stay as they are."
            }
            Self::HpReadoutGuard => {
                "Stops a rare original bug where a battle freezes with the camera \
                 circling forever after a party member's HP gauge stops counting."
            }
            Self::RetailTileCrop => {
                "Draw only the ground the PlayStation's camera would see, as the original \
                 does. Off draws the whole map."
            }
            Self::RetailObjectWindow => {
                "Hide props outside the camera's area, as the original does. Off draws \
                 every prop in the map."
            }
            Self::Retail => {
                "Play exactly as the original looks and moves: every enhancement off for \
                 the session. The settings above are kept for next time."
            }
        }
    }

    /// Whether the row lives in the game's options file (else in
    /// [`LauncherSettings`]).
    pub fn is_game_option(self) -> bool {
        !matches!(
            self,
            Self::PsxRender
                | Self::Retail
                | Self::EntryPulse
                | Self::SmartMonsters
                | Self::ShinySeru
                | Self::FleeExp
                | Self::ExpRate
                | Self::FortressFix
                | Self::JewelFix
                | Self::HpReadoutGuard
                | Self::BonusDrop
                | Self::SpiritAp
                | Self::DamageAp
                | Self::SeruCatchRate
                | Self::EnemyAttacks
                | Self::EnemyStats
        )
    }

    /// Whether `--retail` overrides the row for the session (its value is
    /// kept, but has no effect while [`OptionRow::Retail`] is on).
    pub fn overridden_by_retail(self) -> bool {
        !matches!(
            self,
            Self::Retail
                | Self::PsxRender
                | Self::MusicVolume
                | Self::SfxVolume
                | Self::MonoDownmix
                | Self::Fullscreen
                | Self::WindowSize
        )
    }

    /// The row's value as shown.
    pub fn value(
        self,
        opts: &legaia_engine_menus::options::OptionsState,
        s: &LauncherSettings,
    ) -> String {
        match self {
            Self::Fullscreen => on_off(opts.fullscreen),
            Self::WindowSize => {
                let k = u32::from(opts.window_scale.clamp(WINDOW_SCALES.0, WINDOW_SCALES.1));
                format!("{}x{}", 320 * k, 240 * k)
            }
            Self::EnhancedLighting => on_off(opts.enhanced_lighting),
            Self::TimeOfDay => capitalise(&opts.lighting_time_of_day),
            Self::LampShadows => on_off(opts.light_shadows),
            Self::SeeThroughWalls => on_off(opts.occlusion_fade),
            Self::GroundFog => on_off(opts.volumetric_fog),
            Self::Bloom => on_off(opts.bloom),
            Self::PsxRender => on_off(s.psx_render),
            Self::CameraDistance => capitalise(opts.camera_distance.label()),
            Self::PreciseMovement => on_off(opts.precise_movement),
            Self::ReduceFlashing => on_off(opts.reduce_flashing),
            Self::MusicVolume => opts.bgm_volume.to_string(),
            Self::SfxVolume => opts.sfx_volume.to_string(),
            Self::MonoDownmix => on_off(opts.downmix_mono),
            Self::EntryPulse => on_off(!s.no_entry_pulse),
            Self::SmartMonsters => on_off(s.smart_monsters),
            Self::ShinySeru => on_off(!s.no_shiny_seru),
            Self::FleeExp => on_off(s.flee_exp),
            Self::ExpRate => {
                legaia_engine_session::session_switches::GameplayTweaks::exp_rate_label(
                    s.exp_rate_permille.unwrap_or(EXP_RATE_RETAIL),
                )
            }
            Self::FortressFix => on_off(!s.no_fortress_encounter_fix),
            Self::JewelFix => on_off(s.jewel_fix),
            Self::HpReadoutGuard => on_off(!s.no_hp_readout_guard),
            Self::BonusDrop => on_off(s.equipment_drops),
            Self::SpiritAp => s.spirit_ap.unwrap_or(SPIRIT_AP_RETAIL).to_string(),
            Self::DamageAp => s.damage_ap.unwrap_or(DAMAGE_AP_RETAIL).to_string(),
            Self::SeruCatchRate => Tweaks::catch_rate_label(s.seru_catch_pct),
            Self::EnemyAttacks => {
                Tweaks::exp_rate_label(s.enemy_attack_permille.unwrap_or(SCALE_RETAIL))
            }
            Self::EnemyStats => {
                Tweaks::exp_rate_label(s.enemy_stat_permille.unwrap_or(SCALE_RETAIL))
            }
            Self::RetailTileCrop => on_off(opts.retail_view_window),
            Self::RetailObjectWindow => on_off(opts.retail_static_window),
            Self::Retail => on_off(s.retail),
        }
    }

    /// Step the row's value: `dir >= 0` forward, `dir < 0` back. A switch
    /// flips either way, a volume clamps to `0..=10`, a list wraps.
    pub fn adjust(
        self,
        opts: &mut legaia_engine_menus::options::OptionsState,
        s: &mut LauncherSettings,
        dir: i8,
    ) {
        let step = |v: u8| -> u8 {
            if dir < 0 {
                v.saturating_sub(1)
            } else {
                v.saturating_add(1)
                    .min(legaia_engine_menus::options::VOLUME_MAX)
            }
        };
        match self {
            Self::Fullscreen => opts.fullscreen ^= true,
            Self::WindowSize => {
                let (lo, hi) = WINDOW_SCALES;
                let cur = opts.window_scale.clamp(lo, hi);
                // Wraps, like the other lists.
                opts.window_scale = if dir < 0 {
                    if cur == lo { hi } else { cur - 1 }
                } else if cur == hi {
                    lo
                } else {
                    cur + 1
                };
            }
            Self::EnhancedLighting => opts.enhanced_lighting ^= true,
            Self::TimeOfDay => {
                let cur = TIMES_OF_DAY
                    .iter()
                    .position(|t| t.eq_ignore_ascii_case(&opts.lighting_time_of_day))
                    .unwrap_or(0);
                let n = TIMES_OF_DAY.len();
                let next = if dir < 0 {
                    (cur + n - 1) % n
                } else {
                    (cur + 1) % n
                };
                opts.lighting_time_of_day = TIMES_OF_DAY[next].to_string();
            }
            Self::LampShadows => opts.light_shadows ^= true,
            Self::SeeThroughWalls => opts.occlusion_fade ^= true,
            Self::GroundFog => opts.volumetric_fog ^= true,
            Self::Bloom => opts.bloom ^= true,
            Self::PsxRender => s.psx_render ^= true,
            Self::CameraDistance => {
                // A three-step cycle: back one is forward two.
                let c = opts.camera_distance.cycle();
                opts.camera_distance = if dir < 0 { c.cycle() } else { c };
            }
            Self::PreciseMovement => opts.precise_movement ^= true,
            Self::ReduceFlashing => opts.reduce_flashing ^= true,
            Self::MusicVolume => opts.bgm_volume = step(opts.bgm_volume),
            Self::SfxVolume => opts.sfx_volume = step(opts.sfx_volume),
            Self::MonoDownmix => opts.downmix_mono ^= true,
            Self::EntryPulse => s.no_entry_pulse ^= true,
            Self::SmartMonsters => s.smart_monsters ^= true,
            Self::ShinySeru => s.no_shiny_seru ^= true,
            Self::FleeExp => s.flee_exp ^= true,
            Self::ExpRate => {
                let next = legaia_engine_session::session_switches::GameplayTweaks::step_exp_rate(
                    s.exp_rate_permille.unwrap_or(EXP_RATE_RETAIL),
                    dir,
                );
                s.exp_rate_permille = (next != EXP_RATE_RETAIL).then_some(next);
            }
            Self::FortressFix => s.no_fortress_encounter_fix ^= true,
            Self::JewelFix => s.jewel_fix ^= true,
            Self::HpReadoutGuard => s.no_hp_readout_guard ^= true,
            Self::BonusDrop => s.equipment_drops ^= true,
            Self::SpiritAp => step_opt(&mut s.spirit_ap, &Tweaks::SPIRIT_AP_STEPS, dir),
            Self::DamageAp => step_opt(&mut s.damage_ap, &Tweaks::DAMAGE_AP_STEPS, dir),
            Self::SeruCatchRate => {
                let next = Tweaks::step_in(&Tweaks::CATCH_RATE_STEPS, s.seru_catch_pct, dir);
                s.seru_catch_pct = next;
            }
            Self::EnemyAttacks => step_opt(
                &mut s.enemy_attack_permille,
                &Tweaks::ENEMY_ATTACK_RATES,
                dir,
            ),
            Self::EnemyStats => {
                step_opt(&mut s.enemy_stat_permille, &Tweaks::ENEMY_STAT_RATES, dir)
            }
            Self::RetailTileCrop => opts.retail_view_window ^= true,
            Self::RetailObjectWindow => opts.retail_static_window ^= true,
            Self::Retail => s.retail ^= true,
        }
    }
}

/// The retail dialog font of the disc at `bin`, so the launcher can letter
/// its screen the way the game does: the font TIM inside `PROT.DAT` and the
/// advance table in the boot executable (the same two reads a `--disc` boot
/// makes). `None` when either is unreachable; the launcher then keeps its
/// built-in font. Read at runtime from the user's disc; nothing is written.
pub fn disc_font(bin: &Path) -> Option<legaia_font::Font> {
    let mut raw = legaia_iso::raw::RawDisc::open(bin).ok()?;
    let volume = legaia_iso::iso9660::read_volume(&mut raw).ok()?;
    let files = legaia_iso::iso9660::walk_files(&mut raw, &volume.root).ok()?;
    let find = |name: &str| {
        files
            .iter()
            .find(|(p, _)| {
                let p = p.trim_start_matches(['/', '\\']);
                let p = p.split(';').next().unwrap_or(p);
                p.eq_ignore_ascii_case(name)
            })
            .map(|(_, rec)| rec.clone())
    };
    let prot = find("PROT.DAT")?;
    let scus_rec = find(USA_EXECUTABLE)?;
    let sector = legaia_iso::raw::USER_DATA_SIZE as u64;
    let mut scus = Vec::new();
    raw.read_user_data(
        scus_rec.lba,
        scus_rec.size.div_ceil(sector as u32),
        &mut scus,
    )
    .ok()?;
    scus.truncate(scus_rec.size as usize);
    let start = legaia_font::FONT_TIM_PROT_DAT_OFFSET;
    let end = start + legaia_font::FONT_TIM_LEN as u64;
    let first = start / sector;
    let count = end.div_ceil(sector) - first;
    let mut buf = Vec::new();
    raw.read_user_data(prot.lba + first as u32, count as u32, &mut buf)
        .ok()?;
    let skip = (start - first * sector) as usize;
    let tim = buf.get(skip..skip + legaia_font::FONT_TIM_LEN)?;
    legaia_font::Font::from_disc_tim_and_scus(tim, &scus).ok()
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
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
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

    #[test]
    fn launch_switches_ride_the_play_args_and_round_trip() {
        let info = ok_info(Path::new("/g/l.bin"));
        let off = LauncherSettings::default();
        assert_eq!(play_args_with(&info, &off), play_args(&info));
        let on = LauncherSettings {
            psx_render: true,
            retail: true,
            ..Default::default()
        };
        let a = play_args_with(&info, &on);
        assert!(a.iter().any(|s| s == "--psx-render"));
        assert!(a.iter().any(|s| s == "--retail"));
        // Off switches are not written; on ones are read back.
        assert_eq!(toml::to_string(&off).unwrap().trim(), "");
        let back: LauncherSettings = toml::from_str(&toml::to_string(&on).unwrap()).unwrap();
        assert_eq!(back, on);
        // A pre-switch file (disc only) still loads.
        let old: LauncherSettings = toml::from_str("disc = \"/g/l.bin\"").unwrap();
        assert!(!old.psx_render && !old.retail);
        // The two gameplay switches ride the args, unless `--retail` holds
        // them off (clap refuses them beside it).
        let port = LauncherSettings {
            no_entry_pulse: true,
            smart_monsters: true,
            ..Default::default()
        };
        let a = play_args_with(&info, &port);
        assert!(a.iter().any(|s| s == "--no-entry-pulse"));
        assert!(a.iter().any(|s| s == "--smart-monsters"));
        let a = play_args_with(
            &info,
            &LauncherSettings {
                retail: true,
                ..port
            },
        );
        assert!(a.iter().any(|s| s == "--retail"));
        assert!(
            !a.iter()
                .any(|s| s == "--no-entry-pulse" || s == "--smart-monsters")
        );
    }

    /// The gameplay-tweak rows: each stored value becomes its `play-window`
    /// flag, the defaults write nothing, the EXP rate cycles through the
    /// engine's list, and `--retail` holds them all off.
    #[test]
    fn gameplay_tweak_rows_ride_the_args() {
        use legaia_engine_menus::options::OptionsState;
        let info = ok_info(Path::new("/g/l.bin"));
        let mut o = OptionsState::default();
        let mut s = LauncherSettings::default();
        assert_eq!(OptionRow::ShinySeru.value(&o, &s), "On");
        assert_eq!(OptionRow::FortressFix.value(&o, &s), "On");
        assert_eq!(OptionRow::FleeExp.value(&o, &s), "Off");
        assert_eq!(OptionRow::ExpRate.value(&o, &s), "1x");
        let plain = play_args_with(&info, &s);
        OptionRow::ShinySeru.adjust(&mut o, &mut s, 1);
        OptionRow::FleeExp.adjust(&mut o, &mut s, 1);
        OptionRow::ExpRate.adjust(&mut o, &mut s, 1);
        OptionRow::FortressFix.adjust(&mut o, &mut s, 1);
        OptionRow::JewelFix.adjust(&mut o, &mut s, 1);
        assert_eq!(OptionRow::HpReadoutGuard.value(&o, &s), "On");
        OptionRow::HpReadoutGuard.adjust(&mut o, &mut s, 1);
        assert_eq!(OptionRow::SpiritAp.value(&o, &s), "32");
        assert_eq!(OptionRow::SeruCatchRate.value(&o, &s), "Original");
        for row in [
            OptionRow::BonusDrop,
            OptionRow::SpiritAp,
            OptionRow::DamageAp,
            OptionRow::SeruCatchRate,
            OptionRow::EnemyAttacks,
            OptionRow::EnemyStats,
        ] {
            row.adjust(&mut o, &mut s, 1);
        }
        assert_eq!(OptionRow::SpiritAp.value(&o, &s), "48");
        assert_eq!(OptionRow::EnemyStats.value(&o, &s), "1.25x");
        assert_eq!(OptionRow::SeruCatchRate.value(&o, &s), "25%");
        assert_eq!(OptionRow::ExpRate.value(&o, &s), "1.5x");
        assert!(!OptionRow::ExpRate.is_game_option());
        let a = play_args_with(&info, &s);
        for flag in [
            "--no-shiny-seru",
            "--flee-exp",
            "--exp-rate",
            "--no-fortress-encounter-fix",
            "--jewel-fix",
            "--no-hp-readout-guard",
            "--spirit-ap=48",
            "--damage-ap=150",
            "--seru-catch-rate=25",
            "--enemy-attack-count=1.5",
            "--enemy-stat-scale=1.25",
            "--equipment-drops",
        ] {
            assert!(a.iter().any(|x| x == flag), "{flag}");
            assert!(!plain.iter().any(|x| x == flag), "{flag} off by default");
        }
        assert!(a.iter().any(|x| x == "1.5"));
        // Back to 1x writes no rate.
        OptionRow::ExpRate.adjust(&mut o, &mut s, -1);
        assert_eq!(s.exp_rate_permille, None);
        let retail = LauncherSettings { retail: true, ..s };
        let a = play_args_with(&info, &retail);
        assert!(!a.iter().any(|x| x == "--flee-exp" || x == "--jewel-fix"));
    }

    #[test]
    fn option_rows_step_and_wrap() {
        use legaia_engine_camera::camera_distance::CameraDistance;
        use legaia_engine_menus::options::OptionsState;
        let mut o = OptionsState::default();
        let mut s = LauncherSettings::default();
        // Every row changes its shown value when stepped either way, and a
        // switch / list returns to the start after a full cycle.
        for (_, row) in OPTION_ROWS {
            let before = row.value(&o, &s);
            row.adjust(&mut o, &mut s, 1);
            assert_ne!(row.value(&o, &s), before, "{row:?} forward");
            row.adjust(&mut o, &mut s, -1);
            assert_eq!(row.value(&o, &s), before, "{row:?} back");
            assert!(!row.label().is_empty() && !row.help().is_empty());
        }
        assert_eq!(o, OptionsState::default());
        // Camera distance cycles three ways; volumes clamp.
        o.camera_distance = CameraDistance::Retail;
        OptionRow::CameraDistance.adjust(&mut o, &mut s, -1);
        assert_eq!(o.camera_distance, CameraDistance::Farther);
        o.bgm_volume = 10;
        OptionRow::MusicVolume.adjust(&mut o, &mut s, 1);
        assert_eq!(o.bgm_volume, 10);
        o.sfx_volume = 0;
        OptionRow::SfxVolume.adjust(&mut o, &mut s, -1);
        assert_eq!(o.sfx_volume, 0);
        // The two launch switches live in the launcher's file, not the game's.
        assert!(!OptionRow::PsxRender.is_game_option());
        assert!(!OptionRow::Retail.is_game_option());
        assert!(OptionRow::GroundFog.is_game_option());
    }

    /// The disc's own font loads (disc-gated: skips and passes without
    /// `LEGAIA_DISC_BIN`).
    #[test]
    fn real_disc_font_loads() {
        let Some(disc) = std::env::var_os("LEGAIA_DISC_BIN").map(PathBuf::from) else {
            eprintln!("[skip] LEGAIA_DISC_BIN unset");
            return;
        };
        if !disc.exists() {
            eprintln!("[skip] LEGAIA_DISC_BIN does not exist");
            return;
        }
        let font = disc_font(&disc).expect("the retail dialog font builds from the disc");
        // Proportional: an 'i' is narrower than an 'M'.
        assert!(font.advance_of(b'i') < font.advance_of(b'M'));
        eprintln!("[ran] dialog font from {}", disc.display());
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
