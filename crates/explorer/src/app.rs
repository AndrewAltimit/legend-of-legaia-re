//! The egui front end over [`Explorer`].

mod actors_ui;
mod media_ui;
mod viewport;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::Receiver;

use eframe::egui::{self, Color32, ColorImage, RichText, TextureHandle, TextureOptions};

use crate::analyze::Analysis;
use crate::extract_job::{ExtractJob, OutDir as ExtractOutDir, Outcome};
use crate::player::Player;
use crate::raster::{Model, Shading};
use crate::scene_vram::SceneVram;
use crate::{Explorer, Source, export};
use viewport::Viewport;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Browse,
    Actors,
    Media,
    Extract,
}

impl Mode {
    const ALL: [Mode; 4] = [Mode::Browse, Mode::Actors, Mode::Media, Mode::Extract];

    /// The settings / `LEGAIA_EXPLORER_TAB` spelling.
    fn key(self) -> &'static str {
        match self {
            Mode::Browse => "browse",
            Mode::Actors => "actors",
            Mode::Media => "media",
            Mode::Extract => "extract",
        }
    }

    fn parse(s: &str) -> Option<Mode> {
        Mode::ALL
            .into_iter()
            .find(|m| m.key().eq_ignore_ascii_case(s.trim()))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Overview,
    Textures,
    Model,
    Audio,
    Text,
    Hex,
}

impl Tab {
    const ALL: [Tab; 6] = [
        Tab::Overview,
        Tab::Textures,
        Tab::Model,
        Tab::Audio,
        Tab::Text,
        Tab::Hex,
    ];

    fn name(self) -> &'static str {
        match self {
            Tab::Overview => "Overview",
            Tab::Textures => "Textures",
            Tab::Model => "3D model",
            Tab::Audio => "Audio",
            Tab::Text => "Text",
            Tab::Hex => "Hex",
        }
    }

    /// The settings / `LEGAIA_EXPLORER_TAB` spelling.
    fn key(self) -> &'static str {
        match self {
            Tab::Overview => "overview",
            Tab::Textures => "textures",
            Tab::Model => "model",
            Tab::Audio => "audio",
            Tab::Text => "text",
            Tab::Hex => "hex",
        }
    }

    fn parse(s: &str) -> Option<Tab> {
        Tab::ALL
            .into_iter()
            .find(|&t| t.key().eq_ignore_ascii_case(s.trim()))
    }
}

/// Scripted-run hooks, read from the environment once at start-up. They let
/// a test (or a bug report) open a source, pick an entry and a tab, and save
/// a screenshot without anyone at the keyboard.
#[derive(Debug, Default, Clone)]
pub struct Automation {
    /// `LEGAIA_EXPLORER_SELECT`: PROT index to select once loaded.
    pub select: Option<u32>,
    /// `LEGAIA_EXPLORER_TMD`: which TMD of the selected entry the 3D model
    /// tab shows (the `TMD #` picker), instead of the first.
    pub tmd: Option<usize>,
    /// `LEGAIA_EXPLORER_TAB`: preview tab (`textures`, `model`, `audio`, ...)
    /// or `extract`.
    pub tab: Option<String>,
    /// `LEGAIA_EXPLORER_EXTRACT_TO`: start an extraction into this folder.
    pub extract_to: Option<PathBuf>,
    /// `LEGAIA_EXPLORER_SCREENSHOT`: save a PNG of the window once the
    /// scripted state has settled (after the extraction, when one runs),
    /// then quit.
    pub screenshot: Option<PathBuf>,
    /// `LEGAIA_EXPLORER_EXPORT_DIR`: export without a save dialog, into this
    /// folder.
    pub export_dir: Option<PathBuf>,
    /// `LEGAIA_EXPLORER_ACTOR`: open the Actors view on `monster:<id>`,
    /// `char:<slot>` (0 Vahn .. 3 Terra), optionally `@<clip>:<frame>`.
    pub actor: Option<String>,
    /// `LEGAIA_EXPLORER_MEDIA`: open the Movies & voice view on this ISO path
    /// (`MOV/MV1.STR`), optionally `@<frame>`.
    pub media: Option<String>,
    /// `LEGAIA_EXPLORER_SEARCH`: start with this text in the archive search.
    pub search: Option<String>,
    /// `LEGAIA_EXPLORER_FIND`: start with this text in the Text tab's find
    /// field.
    pub find: Option<String>,
    /// `LEGAIA_EXPLORER_PLAY`: press Play once the disc has loaded.
    pub play: bool,
    /// `LEGAIA_EXPLORER_BROWSE`: open the in-app file browser at start, for
    /// a disc (`disc`), an extracted folder (`folder`) or the extraction
    /// output folder (`out`) or the export folder (`export`).
    pub browse: Option<String>,
}

impl Automation {
    pub fn from_env() -> Self {
        let path = |k: &str| std::env::var_os(k).map(PathBuf::from);
        Self {
            select: std::env::var("LEGAIA_EXPLORER_SELECT")
                .ok()
                .and_then(|s| s.trim().parse().ok()),
            tmd: std::env::var("LEGAIA_EXPLORER_TMD")
                .ok()
                .and_then(|s| s.trim().parse().ok()),
            tab: std::env::var("LEGAIA_EXPLORER_TAB").ok(),
            extract_to: path("LEGAIA_EXPLORER_EXTRACT_TO"),
            screenshot: path("LEGAIA_EXPLORER_SCREENSHOT"),
            export_dir: path("LEGAIA_EXPLORER_EXPORT_DIR"),
            actor: std::env::var("LEGAIA_EXPLORER_ACTOR").ok(),
            media: std::env::var("LEGAIA_EXPLORER_MEDIA").ok(),
            search: std::env::var("LEGAIA_EXPLORER_SEARCH").ok(),
            find: std::env::var("LEGAIA_EXPLORER_FIND").ok(),
            play: std::env::var_os("LEGAIA_EXPLORER_PLAY").is_some_and(|v| !v.is_empty()),
            browse: std::env::var("LEGAIA_EXPLORER_BROWSE").ok(),
        }
    }
}

/// A file dialog that comes back empty sooner than this never appeared (no
/// player picks and cancels that fast); the in-app browser opens instead.
const DIALOG_NEVER_SHOWN: std::time::Duration = std::time::Duration::from_millis(250);

/// Where a controller's export lands when no export folder is remembered:
/// `Legaia Explorer` in Pictures, else in Documents, else in the home
/// folder.
fn default_export_dir() -> PathBuf {
    dirs::picture_dir()
        .or_else(dirs::document_dir)
        .or_else(dirs::home_dir)
        .unwrap_or_default()
        .join("Legaia Explorer")
}

/// `name` in `dir`, or `stem (2).ext`, `stem (3).ext`, ... when it is
/// taken: an export without a dialog never writes over an earlier one.
fn unused_name(dir: &Path, name: &str) -> PathBuf {
    let first = dir.join(name);
    if !first.exists() {
        return first;
    }
    let p = Path::new(name);
    let stem = p
        .file_stem()
        .map_or_else(String::new, |s| s.to_string_lossy().into_owned());
    let ext = p.extension().map(|e| format!(".{}", e.to_string_lossy()));
    (2..)
        .map(|n| dir.join(format!("{stem} ({n}){}", ext.as_deref().unwrap_or(""))))
        .find(|p| !p.exists())
        .unwrap_or(first)
}

/// The width (points) the top bar's right-aligned View and About buttons
/// need, with their spacing.
const VIEW_ABOUT_W: f32 = 110.0;
/// The bar width from which the first row also carries the app's name.
const TITLE_ROOM_W: f32 = 860.0;
/// The room the *Game options* button takes with its long label and the
/// item spacing before it.
const GAME_OPTIONS_LONG_W: f32 = 104.0;

/// A collapsing header's title laid out to fit the row it sits on, cut with
/// an ellipsis when it does not; `true` when it was cut.
fn truncated_header(ui: &egui::Ui, title: &str) -> (egui::WidgetText, bool) {
    // The header's arrow indent and its button padding sit left and right
    // of the text.
    let room = ui.available_width() - ui.spacing().indent - ui.spacing().button_padding.x;
    let galley = egui::WidgetText::from(title).into_galley(
        ui,
        Some(egui::TextWrapMode::Truncate),
        room.max(40.0),
        egui::TextStyle::Button,
    );
    let elided = galley.elided;
    (egui::WidgetText::Galley(galley), elided)
}

/// The widest a view's left list may be: 45% of the window, never below
/// `min`. Every view's side panel takes it, so the preview beside the list
/// keeps room at the minimum window size (at 720 points the browser's own
/// 430-point default left a 3D model view a few rows tall).
pub(crate) fn side_panel_max(ctx: &egui::Context, min: f32) -> f32 {
    (ctx.content_rect().width() * 0.45).max(min)
}

/// A playback key a player view takes: Space plays or pauses, Left and Right
/// step one frame back or on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Transport {
    PlayPause,
    Back,
    Forward,
}

/// The playback key pressed this frame, consumed. Only while no widget holds
/// the keyboard: a focused button takes Space as its own click and a text
/// field takes all three as typing, so the views keep a typed search intact.
pub(crate) fn transport_key(ui: &egui::Ui) -> Option<Transport> {
    if ui.ctx().memory(|m| m.focused().is_some()) {
        return None;
    }
    ui.input_mut(|i| {
        let none = egui::Modifiers::NONE;
        if i.consume_key(none, egui::Key::Space) {
            Some(Transport::PlayPause)
        } else if i.consume_key(none, egui::Key::ArrowLeft) {
            Some(Transport::Back)
        } else if i.consume_key(none, egui::Key::ArrowRight) {
            Some(Transport::Forward)
        } else {
            None
        }
    })
}

/// The list row an arrow, Page, Home or End key moves to from `cur`, when the
/// frame has such a key and it moves. A nothing-selected list starts at its
/// top. Rows beyond `last` do not exist.
pub(crate) fn list_step(ui: &egui::Ui, cur: Option<usize>, last: usize) -> Option<usize> {
    let to = ui.input(|i| {
        if i.key_pressed(egui::Key::ArrowDown) {
            Some(cur.map_or(0, |c| (c + 1).min(last)))
        } else if i.key_pressed(egui::Key::ArrowUp) {
            Some(cur.map_or(0, |c| c.saturating_sub(1)))
        } else if i.key_pressed(egui::Key::PageDown) {
            Some(cur.map_or(0, |c| (c + 20).min(last)))
        } else if i.key_pressed(egui::Key::PageUp) {
            Some(cur.map_or(0, |c| c.saturating_sub(20)))
        } else if i.key_pressed(egui::Key::Home) {
            Some(0)
        } else if i.key_pressed(egui::Key::End) {
            Some(last)
        } else {
            None
        }
    });
    to.filter(|k| Some(*k) != cur)
}

/// `n` of something, with the plural: *1 texture*, *2 textures*.
fn count_noun(n: usize, noun: &str) -> String {
    format!("{n} {noun}{}", if n == 1 { "" } else { "s" })
}

/// An export that takes long enough to freeze the window if it ran in the
/// frame: it runs on its own thread, with a line up saying so.
struct ExportJob {
    /// What the status line names while it runs.
    what: String,
    /// Where the *Show folder* button goes once it has finished.
    folder: Option<PathBuf>,
    /// The line to show when it is done, or why it failed.
    done: std::sync::mpsc::Receiver<Result<String, String>>,
}

/// How long an informational status line ("... written to ...") stays up.
/// Errors stay until dismissed.
const STATUS_TTL: std::time::Duration = std::time::Duration::from_secs(8);

/// Which VRAM image the entry model view samples.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TexSource {
    /// Only the TIMs inside the selected entry.
    Entry,
    /// The VRAM the engine builds for the entry's scene (CDNAME block).
    Scene,
}

struct ModelView {
    key: (usize, usize),
    model: Option<Model>,
    entry_vram: psx_tim::Vram,
    source: TexSource,
    /// The scene-cache key the view last drew with (`None` = entry VRAM).
    drawn_with: Option<String>,
    view: Viewport,
}

/// A scene VRAM build: in flight on a worker, or done.
enum SceneSlot {
    Building(Receiver<Result<SceneVram, String>>),
    Ready(Arc<SceneVram>),
    Failed(String),
}

pub struct App {
    pub explorer: Explorer,
    mode: Mode,
    tab: Tab,
    about_open: bool,
    /// The keyboard-shortcuts box (`?`, or the View menu).
    keys_open: bool,
    /// Ctrl+E this frame: the shown view's main export button takes it
    /// ([`Self::take_export_key`]); left over, the status line says the
    /// view has nothing to save.
    export_key: bool,
    /// The export running on its own thread, if any.
    export_job: Option<ExportJob>,
    /// The window's inner size, tracked for the settings file.
    window_size: Option<[f32; 2]>,
    window_pos: Option<[i32; 2]>,
    path_text: String,
    /// The typed-path row with a source open (`Ctrl+L` / *Open > Type a
    /// path*); with nothing open the row always shows.
    path_row: bool,
    /// Focus the path field on its next draw.
    path_focus: bool,
    filter: String,
    class_filter: Option<String>,
    // Textures tab.
    tim_sel: usize,
    clut: usize,
    tim_zoom: f32,
    /// *Fit* was pressed: the next draw sets the zoom to the pane.
    tim_fit: bool,
    /// The Extract view's options are unfolded; a started run folds them, so
    /// a small window has room for its log.
    extract_options_open: bool,
    tim_tex: Option<((usize, usize, usize), TextureHandle)>,
    /// The decode failure for the key [`Self::tim_tex`] would hold.
    tim_err: Option<((usize, usize, usize), String)>,
    // Model tab.
    tmd_sel: usize,
    model: Option<ModelView>,
    /// Scene VRAM images by `block:kind`, built on demand.
    scenes: HashMap<String, SceneSlot>,
    actors: actors_ui::ActorsState,
    media: media_ui::MediaState,
    /// Entry the browser should expand and scroll to on its next frame.
    reveal: Option<usize>,
    // Audio tab.
    vab_sel: usize,
    seq_loop: bool,
    /// Waveforms of the samples the Audio tab has shown, by entry, bank and
    /// sample: its rows are virtual, so only what scrolls past is decoded.
    waves: HashMap<(usize, usize, usize), Vec<u8>>,
    player: Player,
    /// The Hex tab's *Go to* field.
    hex_goto: String,
    /// The Text tab's find field: only lines holding every word show.
    text_filter: String,
    /// Where the transport's seek slider is being dragged to (seconds); the
    /// stream moves there when the pointer lets go.
    seek_drag: Option<f64>,
    // Extract mode.
    out_text: String,
    job: Option<ExtractJob>,
    /// The extraction log shows only its warnings.
    log_warnings_only: bool,
    /// The game binary shipped beside this one, if any.
    engine: Option<PathBuf>,
    /// A game started with Play, while it runs.
    game: Option<crate::launch::Game>,
    /// What [`Self::game`] is: the game's launcher screen (*Game options*),
    /// which plays the game itself from its own Play, or the game.
    game_is_launcher: bool,
    /// The banner a failed game put up and the report its button copies.
    error_report: Option<(String, String)>,
    /// The failed game's log the error banner's *Show log* button opens.
    error_log: Option<PathBuf>,
    /// Each view's left panel width this run, by view key, for the next start.
    side_widths: std::collections::BTreeMap<&'static str, f32>,
    /// The folder the last export or extraction wrote into, for the
    /// status line's *Show folder* button.
    last_written: Option<PathBuf>,
    /// The informational status text last seen and when it appeared (or
    /// was last hovered): it clears itself after [`STATUS_TTL`].
    status_seen: Option<(String, std::time::Instant)>,
    /// The zoom last set on egui, to tell a change the user made from one
    /// [`crate::settings::fit_zoom`] made.
    zoom_applied: f32,
    /// Theme and zoom from the settings have been pushed into egui.
    appearance_applied: bool,
    // Automation.
    auto: Automation,
    auto_selected: bool,
    /// The remembered Browse entry ([`crate::settings::Settings::entry`]) has been restored
    /// (or there was none to restore) for the start-up source.
    entry_restored: bool,
    auto_extract_started: bool,
    settle_since: Option<std::time::Instant>,
    screenshot_requested: bool,
    /// The controllers' input ([`crate::gamepad::spawn`]), turned into key
    /// presses in `raw_input_hook`; `None` without a controller backend.
    pad: Option<std::sync::mpsc::Receiver<crate::gamepad::PadInput>>,
    /// A controller was the last thing pressed (a key or a mouse button
    /// clears it): the file dialogs give way to [`Self::file_browser`],
    /// which a controller can work, and an export goes straight into the
    /// export folder.
    pad_active: bool,
    /// The in-app file browser while it is open (`crate::file_browser`).
    file_browser: Option<crate::file_browser::FileBrowser>,
    /// The disc the game itself boots, offered on the start page while it
    /// is not on the recent list ([`crate::launch::engine_disc_offer`]).
    game_disc: Option<PathBuf>,
    /// [`Self::game_disc`] was found in the usual folders on a first run
    /// ([`crate::launch::found_disc_offer`]), not chosen in the game.
    game_disc_found: bool,
}

impl App {
    pub fn new(explorer: Explorer, initial: Option<PathBuf>, auto: Automation) -> Self {
        let mut explorer = explorer;
        let mut player = Player::default();
        player.set_volume(explorer.settings.volume_percent());
        // Recent entries whose disc or folder has gone are dropped at start.
        if explorer.settings.prune_recent() {
            explorer.save_settings();
        }
        // The disc or folder opened last, when it is still there. When it
        // has gone (moved, renamed, an unplugged drive) say so: the recent
        // list has just dropped it, and the start page alone would read as
        // a first run.
        let last = explorer.settings.last_source.clone();
        let open = initial.or_else(|| last.clone().filter(|p| p.exists()));
        if open.is_none()
            && let Some(gone) = last.filter(|p| !p.exists())
        {
            explorer.error = Some(format!(
                "The disc opened last is no longer at {} - open it from where it is now.",
                gone.display()
            ));
        }
        let path_text = open
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        if let Some(p) = &open {
            explorer.open(p);
        }
        let out_text = explorer
            .settings
            .last_out_dir
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        // The view and tab open when the app last closed.
        let mode = explorer
            .settings
            .mode
            .as_deref()
            .and_then(Mode::parse)
            .unwrap_or(Mode::Browse);
        let tab = explorer
            .settings
            .tab
            .as_deref()
            .and_then(Tab::parse)
            .unwrap_or(Tab::Overview);
        let mut app = Self {
            explorer,
            mode,
            tab,
            about_open: false,
            keys_open: false,
            export_key: false,
            export_job: None,
            window_size: None,
            window_pos: None,
            path_text,
            path_row: false,
            path_focus: false,
            filter: String::new(),
            class_filter: None,
            tim_sel: 0,
            clut: 0,
            tim_zoom: 2.0,
            tim_fit: false,
            zoom_applied: 1.0,
            extract_options_open: true,
            tim_tex: None,
            tim_err: None,
            tmd_sel: 0,
            model: None,
            scenes: HashMap::new(),
            actors: actors_ui::ActorsState::default(),
            media: media_ui::MediaState::default(),
            reveal: None,
            vab_sel: 0,
            seq_loop: false,
            waves: HashMap::new(),
            player,
            seek_drag: None,
            text_filter: String::new(),
            hex_goto: String::new(),
            out_text,
            job: None,
            log_warnings_only: false,
            engine: std::env::current_exe()
                .ok()
                .and_then(|exe| crate::launch::find_engine(&exe)),
            game: None,
            game_is_launcher: false,
            error_report: None,
            error_log: None,
            side_widths: Default::default(),
            last_written: None,
            status_seen: None,
            appearance_applied: false,
            auto,
            auto_selected: false,
            entry_restored: false,
            auto_extract_started: false,
            settle_since: None,
            screenshot_requested: false,
            pad: None,
            pad_active: false,
            file_browser: None,
            game_disc: None,
            game_disc_found: false,
        };
        app.game_disc = crate::launch::engine_disc_offer(&app.explorer.settings.recent);
        // A first run with nothing to open: look where a dump usually is.
        if app.game_disc.is_none()
            && app.explorer.settings.recent.is_empty()
            && app.explorer.library.is_none()
            && app.explorer.loading().is_none()
        {
            app.game_disc = crate::launch::found_disc_offer();
            app.game_disc_found = app.game_disc.is_some();
        }
        if let Some(t) = app.auto.tab.clone() {
            if t.eq_ignore_ascii_case("about") {
                app.about_open = true;
            } else if t.eq_ignore_ascii_case("keys") {
                app.keys_open = true;
            } else if let Some(m) = Mode::parse(&t) {
                app.mode = m;
            } else if let Some(tab) = Tab::parse(&t) {
                app.mode = Mode::Browse;
                app.tab = tab;
            }
        }
        if app.auto.actor.is_some() {
            app.mode = Mode::Actors;
        }
        app.actors.pick = app
            .explorer
            .settings
            .actor
            .as_deref()
            .and_then(actors_ui::Pick::parse);
        if app.auto.media.is_some() {
            app.mode = Mode::Media;
        }
        if let Some(q) = app.auto.search.clone() {
            app.filter = q;
        }
        if let Some(q) = app.auto.find.clone() {
            app.text_filter = q;
        }
        if let Some(dir) = app.auto.extract_to.clone() {
            app.out_text = dir.display().to_string();
            app.mode = Mode::Extract;
        }
        app
    }

    /// A view's left panel width: what it was when the app last closed, else
    /// `default`. Only read when the panel first shows; egui holds it after.
    fn side_width(&self, view: &str, default: f32) -> f32 {
        self.explorer
            .settings
            .side_widths
            .get(view)
            .copied()
            .filter(|w| w.is_finite() && *w > 0.0)
            .unwrap_or(default)
    }

    /// While files are dragged over the window, a frame round it and a
    /// line saying what a drop opens: without it nothing on screen shows
    /// that the window takes a drop until it has happened.
    fn drop_overlay(&self, ui: &egui::Ui) {
        let hovered: Vec<PathBuf> = ui.input(|i| {
            i.raw
                .hovered_files
                .iter()
                .filter_map(|f| f.path.clone())
                .collect()
        });
        if ui.input(|i| i.raw.hovered_files.is_empty()) {
            return;
        }
        let text = match crate::pick_drop(&hovered) {
            Some(p) => match crate::library::not_a_source(p) {
                Some(_) => "Legaia Explorer opens disc images and extracted folders; \
                            drop this on the Legend of Legaia window instead."
                    .to_string(),
                None => format!(
                    "Drop to open {}",
                    p.file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_else(|| p.display().to_string())
                ),
            },
            // Some platforms do not name the files until the drop.
            None => "Drop to open the disc image or extracted folder".to_string(),
        };
        let pal = crate::theme::palette(ui);
        let painter = ui.ctx().layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new("drop-overlay"),
        ));
        let screen = ui.ctx().content_rect();
        let ground = crate::theme::c(pal.ground);
        painter.rect_filled(
            screen,
            0.0,
            egui::Color32::from_rgba_unmultiplied(ground.r(), ground.g(), ground.b(), 200),
        );
        painter.rect_stroke(
            screen.shrink(6.0),
            8.0,
            egui::Stroke::new(3.0, crate::theme::c(pal.accent)),
            egui::StrokeKind::Inside,
        );
        painter.text(
            screen.center(),
            egui::Align2::CENTER_CENTER,
            text,
            egui::FontId::proportional(20.0),
            crate::theme::c(pal.ink),
        );
    }

    fn open_path(&mut self, p: &Path) {
        self.path_text = p.display().to_string();
        self.player.stop();
        self.model = None;
        self.tim_tex = None;
        self.tim_err = None;
        self.explorer.open(p);
    }

    fn select(&mut self, pos: usize) {
        if self.explorer.selected != Some(pos) {
            self.tim_sel = 0;
            self.clut = 0;
            self.tmd_sel = 0;
            self.vab_sel = 0;
            self.tim_tex = None;
            self.tim_err = None;
            self.model = None;
        }
        self.explorer.select(pos);
    }

    /// Where an export goes: the scripted folder, else a save dialog.
    fn export_target(&mut self, default_name: &str, ext: &str) -> Option<PathBuf> {
        if let Some(dir) = &self.auto.export_dir {
            let _ = std::fs::create_dir_all(dir);
            return Some(dir.join(default_name));
        }
        // A controller cannot type a file name: the export goes into the
        // export folder under its own name, and the status line says where.
        if self.pad_active {
            return self.export_into_folder(default_name);
        }
        let mut d = rfd::FileDialog::new()
            .set_file_name(default_name)
            .add_filter(ext.to_uppercase(), &[ext]);
        if let Some(dir) = &self.explorer.settings.last_export_dir {
            d = d.set_directory(dir);
        }
        let asked = std::time::Instant::now();
        let picked = d.save_file();
        if let Some(p) = &picked {
            self.explorer.settings.last_export_dir = p.parent().map(Path::to_path_buf);
            self.explorer.save_settings();
        } else if asked.elapsed() < DIALOG_NEVER_SHOWN {
            // The save window never appeared (no file-dialog service): the
            // export goes where a controller's would, rather than nowhere.
            log::warn!("the desktop save dialog did not open; exporting into the export folder");
            return self.export_into_folder(default_name);
        }
        picked
    }

    /// `default_name` in the export folder, made if need be, never over an
    /// earlier file: an export with no save window to name it.
    fn export_into_folder(&mut self, default_name: &str) -> Option<PathBuf> {
        let dir = self.export_dir();
        if let Err(e) = std::fs::create_dir_all(&dir) {
            self.explorer.error = Some(format!("Could not make {}: {e}", dir.display()));
            return None;
        }
        self.explorer.settings.last_export_dir = Some(dir.clone());
        self.explorer.save_settings();
        Some(unused_name(&dir, default_name))
    }

    /// Whether Ctrl+E was pressed this frame, clearing it: the one export
    /// button the shown view treats as its main one asks once per frame.
    fn take_export_key(&mut self) -> bool {
        std::mem::take(&mut self.export_key)
    }

    /// Run `work` on its own thread; its result becomes the status line (or
    /// the error banner). A second export waits for the first.
    fn start_export(
        &mut self,
        what: String,
        folder: Option<PathBuf>,
        work: impl FnOnce() -> anyhow::Result<String> + Send + 'static,
    ) {
        if let Some(job) = &self.export_job {
            self.explorer.error = Some(format!(
                "Still writing {}; try again when it is done.",
                job.what
            ));
            return;
        }
        let (tx, done) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(work().map_err(|e| format!("{e:#}")));
        });
        self.explorer.error = None;
        self.explorer.status = None;
        self.export_job = Some(ExportJob { what, folder, done });
    }

    /// Announce a finished [`ExportJob`].
    fn poll_export(&mut self, ctx: &egui::Context) {
        let Some(job) = &self.export_job else { return };
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
        let res = match job.done.try_recv() {
            Ok(res) => res,
            Err(std::sync::mpsc::TryRecvError::Empty) => return,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                Err("the export thread stopped without a result".to_string())
            }
        };
        let Some(job) = self.export_job.take() else {
            return;
        };
        match res {
            Ok(line) => {
                self.explorer.status = Some(line);
                self.last_written = job.folder;
            }
            Err(e) => self.explorer.error = Some(format!("Writing {} failed: {e}", job.what)),
        }
    }

    fn report(&mut self, what: &str, res: anyhow::Result<()>, path: &Path) {
        match res {
            Ok(()) => {
                self.explorer.status = Some(format!("{what} written to {}", path.display()));
                self.explorer.error = None;
                self.last_written = if path.is_dir() {
                    Some(path.to_path_buf())
                } else {
                    path.parent().map(Path::to_path_buf)
                };
            }
            Err(e) => self.explorer.error = Some(format!("{what} export failed: {e:#}")),
        }
    }

    /// Select the entry Browse had open when the app last closed, once,
    /// when the start-up source has loaded. A scripted `select` wins.
    fn restore_entry(&mut self) {
        if self.entry_restored {
            return;
        }
        let Some(lib) = self.explorer.library.clone() else {
            // Nothing loading means the start-up source is absent or failed:
            // a disc opened later starts with nothing selected.
            self.entry_restored = self.explorer.loading().is_none();
            return;
        };
        self.entry_restored = true;
        if self.auto.select.is_some() || self.explorer.selected.is_some() {
            return;
        }
        if let Some(pos) = self
            .explorer
            .settings
            .entry
            .and_then(|i| lib.position_of(i))
        {
            self.select(pos);
            self.reveal = Some(pos);
        }
    }

    fn run_automation(&mut self, ctx: &egui::Context) {
        if !self.auto_selected
            && let (Some(idx), Some(lib)) = (self.auto.select, self.explorer.library.clone())
        {
            self.auto_selected = true;
            if let Some(pos) = lib.position_of(idx) {
                self.select(pos);
                self.reveal = Some(pos);
                if let Some(t) = self.auto.tmd {
                    self.tmd_sel = t;
                }
            }
        }
        if !self.auto_extract_started
            && let Some(dir) = self.auto.extract_to.clone()
            && self.explorer.library.is_some()
        {
            self.auto_extract_started = true;
            self.out_text = dir.display().to_string();
            self.start_extract();
        }
        if let Some(what) = self.auto.browse.take() {
            use crate::file_browser::Purpose;
            let purpose = match what.as_str() {
                "folder" => Purpose::ExtractedFolder,
                "out" => Purpose::OutputFolder,
                "export" => Purpose::ExportFolder,
                _ => Purpose::Disc,
            };
            self.open_file_browser(purpose);
        }
        if self.auto.play && self.explorer.library.is_some() && self.explorer.loading().is_none() {
            self.auto.play = false;
            self.start_game();
        }
        let Some(shot) = self.auto.screenshot.clone() else {
            return;
        };
        // Settled once no load is in flight. With nothing to open (no path
        // given, no last disc) that is the first frame: the welcome screen
        // is the state to capture. The selection and extraction steps only
        // wait when there is a library for them to act on.
        let loaded = self.explorer.loading().is_none();
        let has_lib = self.explorer.library.is_some();
        let analysed = self.auto.select.is_none() || !has_lib || self.explorer.analysis.is_some();
        let extracted = self.auto.extract_to.is_none()
            || !has_lib
            || self.job.as_ref().is_some_and(|j| j.outcome().is_some());
        let idle = loaded
            && analysed
            && extracted
            && self.export_job.is_none()
            && !self.explorer.is_analyzing()
            && !self.media.busy();
        if !idle && self.explorer.error.is_none() {
            self.settle_since = None;
            ctx.request_repaint();
            return;
        }
        // Let the window map and a few frames land before reading it back.
        let since = *self.settle_since.get_or_insert_with(|| {
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            std::time::Instant::now()
        });
        if since.elapsed() > std::time::Duration::from_millis(1500) && !self.screenshot_requested {
            self.screenshot_requested = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
        }
        ctx.request_repaint();
        let image = ctx.input(|i| {
            i.raw.events.iter().find_map(|e| match e {
                egui::Event::Screenshot { image, .. } => Some(Arc::clone(image)),
                _ => None,
            })
        });
        if let Some(img) = image {
            let [w, h] = img.size;
            let rgba: Vec<u8> = img.pixels.iter().flat_map(|c| c.to_array()).collect();
            match psx_tim::write_png(&shot, w, h, &rgba) {
                Ok(()) => log::info!("screenshot saved to {}", shot.display()),
                Err(e) => log::error!("screenshot: {e:#}"),
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    fn start_extract(&mut self) {
        let Some(lib) = &self.explorer.library else {
            self.explorer.error = Some("Open a disc image first.".into());
            return;
        };
        let Source::Disc(bin) = &lib.source else {
            self.explorer.error = Some(
                "Extraction reads a disc image; the open source is an extracted folder.".into(),
            );
            return;
        };
        let out = PathBuf::from(self.out_text.trim());
        if self.out_text.trim().is_empty() {
            self.explorer.error = Some("Choose an output folder.".into());
            return;
        }
        let s = &mut self.explorer.settings;
        s.last_out_dir = Some(out.clone());
        let options = legaia_extract::Options {
            bin: bin.clone(),
            out,
            skip_verify: s.skip_verify,
            skip_png: s.skip_png,
            skip_xa: s.skip_xa,
            skip_catalog: s.skip_catalog,
            skip_font: s.skip_font,
            all_textures: !s.skip_all_textures,
            verbose: false,
        };
        self.explorer.save_settings();
        self.explorer.error = None;
        self.extract_options_open = false;
        self.job = Some(ExtractJob::start(options));
    }

    // --- Panels ------------------------------------------------------------

    /// The native file dialog for a disc image, opened where the last one
    /// was; the in-app browser while a controller is in hand, or when the
    /// dialog never appeared.
    fn pick_disc(&mut self) {
        use crate::file_browser::Purpose;
        if self.pad_active {
            self.open_file_browser(Purpose::Disc);
            return;
        }
        let mut dialog = rfd::FileDialog::new()
            .set_title("Choose your Legend of Legaia disc image")
            .add_filter("PlayStation disc image", &["bin", "cue", "BIN", "CUE"]);
        if let Some(dir) = self.last_dir() {
            dialog = dialog.set_directory(dir);
        }
        let asked = std::time::Instant::now();
        match dialog.pick_file() {
            Some(p) => self.open_path(&p),
            None => self.dialog_fallback(asked, Purpose::Disc),
        }
    }

    /// The native folder dialog for a `legaia-extract` output folder (the
    /// in-app browser, as [`Self::pick_disc`]).
    fn pick_folder(&mut self) {
        use crate::file_browser::Purpose;
        if self.pad_active {
            self.open_file_browser(Purpose::ExtractedFolder);
            return;
        }
        let mut dialog = rfd::FileDialog::new().set_title("Choose a legaia-extract output folder");
        if let Some(dir) = self.last_dir() {
            dialog = dialog.set_directory(dir);
        }
        let asked = std::time::Instant::now();
        match dialog.pick_folder() {
            Some(p) => self.open_path(&p),
            None => self.dialog_fallback(asked, Purpose::ExtractedFolder),
        }
    }

    /// The Extract view's *Choose...*: the folder the extraction writes
    /// into (the in-app browser, as [`Self::pick_disc`]).
    fn pick_out_folder(&mut self) {
        use crate::file_browser::Purpose;
        if self.pad_active {
            self.open_file_browser(Purpose::OutputFolder);
            return;
        }
        let asked = std::time::Instant::now();
        match rfd::FileDialog::new()
            .set_title("Choose the extraction output folder")
            .pick_folder()
        {
            Some(p) => self.out_text = p.display().to_string(),
            None => self.dialog_fallback(asked, Purpose::OutputFolder),
        }
    }

    /// The folder exports go into: the one remembered, else
    /// [`default_export_dir`]. A controller's export lands here; the save
    /// window opens here.
    fn export_dir(&self) -> PathBuf {
        self.explorer
            .settings
            .last_export_dir
            .clone()
            .filter(|d| d.is_dir())
            .unwrap_or_else(default_export_dir)
    }

    /// The View menu's (and a controller export's) *Change...*: choose the
    /// export folder (the in-app browser, as [`Self::pick_disc`]).
    fn pick_export_folder(&mut self) {
        use crate::file_browser::Purpose;
        if self.pad_active {
            self.open_file_browser(Purpose::ExportFolder);
            return;
        }
        let asked = std::time::Instant::now();
        let mut d = rfd::FileDialog::new().set_title("Choose where exports go");
        let dir = self.export_dir();
        if dir.is_dir() {
            d = d.set_directory(&dir);
        }
        match d.pick_folder() {
            Some(p) => self.set_export_dir(p),
            None => self.dialog_fallback(asked, Purpose::ExportFolder),
        }
    }

    /// Remember `dir` as the export folder and say so.
    fn set_export_dir(&mut self, dir: PathBuf) {
        self.explorer.status = Some(format!("Exports go to {}", dir.display()));
        self.last_written = None;
        self.explorer.settings.last_export_dir = Some(dir);
        self.explorer.save_settings();
    }

    /// A file dialog that came back empty: when it came back at once it
    /// never appeared (a desktop with no file-dialog service), so the in-app
    /// browser opens in its place. A dialog the player closed takes longer.
    fn dialog_fallback(
        &mut self,
        asked: std::time::Instant,
        purpose: crate::file_browser::Purpose,
    ) {
        if asked.elapsed() < DIALOG_NEVER_SHOWN {
            log::warn!("the desktop file dialog did not open; using the in-app browser");
            self.open_file_browser(purpose);
        }
    }

    /// Open the in-app file browser for `purpose`, where the last pick of
    /// its kind was.
    fn open_file_browser(&mut self, purpose: crate::file_browser::Purpose) {
        use crate::file_browser::{FileBrowser, Purpose};
        let start = match purpose {
            Purpose::Disc | Purpose::ExtractedFolder => self.last_dir(),
            Purpose::OutputFolder => {
                let out = PathBuf::from(self.out_text.trim());
                (!self.out_text.trim().is_empty())
                    .then(|| out.ancestors().find(|d| d.is_dir()).map(Path::to_path_buf))
                    .flatten()
            }
            // The default export folder may not exist yet: its nearest
            // existing parent.
            Purpose::ExportFolder => self
                .export_dir()
                .ancestors()
                .find(|d| d.is_dir())
                .map(Path::to_path_buf),
        };
        self.file_browser = Some(FileBrowser::open(purpose, start.as_deref()));
    }

    /// Draw the open file browser and act on what it came to.
    fn show_file_browser(&mut self, ctx: &egui::Context) {
        use crate::file_browser::{Outcome, Purpose};
        let Some(b) = self.file_browser.as_mut() else {
            return;
        };
        let purpose = b.purpose;
        match b.show(ctx) {
            Outcome::Open => {}
            Outcome::Cancelled => self.file_browser = None,
            Outcome::Picked(p) => {
                self.file_browser = None;
                match purpose {
                    Purpose::Disc | Purpose::ExtractedFolder => self.open_path(&p),
                    Purpose::OutputFolder => self.out_text = p.display().to_string(),
                    Purpose::ExportFolder => self.set_export_dir(p),
                }
            }
        }
    }

    /// The folder holding the source opened last, if it still exists.
    fn last_dir(&self) -> Option<PathBuf> {
        self.explorer
            .settings
            .last_source
            .as_deref()
            .and_then(Path::parent)
            .filter(|d| d.is_dir())
            .map(Path::to_path_buf)
    }

    /// The app-wide shortcuts that are not tied to a widget: Ctrl+O, F5
    /// (Play), Ctrl+1..4 (the views), Ctrl+E (save what the view shows).
    fn shortcuts(&mut self, ctx: &egui::Context) {
        let cmd = |k| egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, k);
        if ctx.input_mut(|i| i.consume_shortcut(&cmd(egui::Key::O))) {
            self.pick_disc();
        }
        self.export_key = ctx.input_mut(|i| i.consume_shortcut(&cmd(egui::Key::E)));
        if ctx.input_mut(|i| i.consume_shortcut(&cmd(egui::Key::L))) {
            self.path_row = !self.path_row;
            self.path_focus = self.path_row;
        }
        // Before plain F5, whose pattern a held Shift also matches.
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::SHIFT, egui::Key::F5)) {
            match self.play_blocked() {
                Some(why) => self.explorer.status = Some(why),
                None => self.open_game_launcher(),
            }
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::F5)) {
            // A key has no greyed-out look to hover over: say why not.
            match self.play_blocked() {
                Some(why) => self.explorer.status = Some(why),
                None => self.start_game(),
            }
        }
        for (key, mode) in [
            (egui::Key::Num1, Mode::Browse),
            (egui::Key::Num2, Mode::Actors),
            (egui::Key::Num3, Mode::Media),
            (egui::Key::Num4, Mode::Extract),
        ] {
            if ctx.input_mut(|i| i.consume_shortcut(&cmd(key))) {
                self.mode = mode;
            }
        }
    }

    fn top_bar(&mut self, ui: &mut egui::Ui) {
        // Wrapped, so a narrow window (the 720-point minimum, or a large UI
        // zoom) moves the last buttons to a second row instead of pushing
        // Play, View and About off the edge.
        ui.horizontal_wrapped(|ui| {
            // The name is in the window's title bar too; a narrow window
            // spends its first row on the controls instead, so the bar
            // stays one row down to the minimum width.
            if ui.available_width() >= TITLE_ROOM_W {
                ui.label(
                    RichText::new("Legaia Explorer")
                        .heading()
                        .color(crate::theme::accent(ui)),
                );
                ui.separator();
            }
            for (mode, label, key) in [
                (Mode::Browse, "Browse", "Ctrl+1"),
                (Mode::Actors, "Characters & monsters", "Ctrl+2"),
                (Mode::Media, "Movies & voice", "Ctrl+3"),
                (Mode::Extract, "Extract", "Ctrl+4"),
            ] {
                ui.selectable_value(&mut self.mode, mode, label)
                    .on_hover_text(key);
            }
            ui.separator();
            // One menu for every way in, so the first row holds at the
            // minimum width: the three buttons it replaces wrapped the bar.
            let mut picked: Option<PathBuf> = None;
            let mut action = None;
            pad_menu_button(ui, "Open", |ui, focus_first| {
                let disc = ui.add(egui::Button::new("Disc image...").shortcut_text("Ctrl+O"));
                if focus_first {
                    disc.request_focus();
                }
                if disc.clicked() {
                    action = Some(0);
                    ui.close();
                }
                if ui
                    .button("Extracted folder...")
                    .on_hover_text("A folder legaia-extract (or Extract) wrote")
                    .clicked()
                {
                    action = Some(1);
                    ui.close();
                }
                if ui
                    .add(egui::Button::new("Type a path...").shortcut_text("Ctrl+L"))
                    .clicked()
                {
                    action = Some(2);
                    ui.close();
                }
                ui.separator();
                if self.explorer.settings.recent.is_empty() {
                    ui.label(RichText::new("Discs and folders you open are listed here.").weak());
                } else {
                    ui.label(RichText::new("Recent").strong());
                    if let Some(p) = self.recent_list(ui) {
                        picked = Some(p);
                        ui.close();
                    }
                }
            })
            .response
            .on_hover_text("Open a disc image or an extracted folder (Ctrl+O), or a recent one");
            match action {
                Some(0) => self.pick_disc(),
                Some(1) => self.pick_folder(),
                Some(2) => {
                    self.path_row = true;
                    self.path_focus = true;
                }
                _ => {}
            }
            if let Some(p) = picked {
                self.open_path(&p);
            }
            ui.separator();
            self.play_button(ui);
            // View and About sit at the right edge when the row has room
            // for them, else they flow on after Play.
            // What is left of this row: in a wrapping row `available_width`
            // is the whole row's, which put View and About over Play.
            let left = ui.max_rect().right() - ui.cursor().left();
            if left >= VIEW_ABOUT_W {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("About").on_hover_text("F1").clicked() {
                        self.about_open = !self.about_open;
                    }
                    pad_menu_button(ui, "View", |ui, first| self.view_menu(ui, first));
                });
            } else {
                pad_menu_button(ui, "View", |ui, first| self.view_menu(ui, first));
                if ui.button("About").on_hover_text("F1").clicked() {
                    self.about_open = !self.about_open;
                }
            }
        });
        let show_path = self.path_row || self.explorer.library.is_none();
        if show_path {
            ui.horizontal(|ui| {
                ui.label("Path:");
                let resp = ui.add(
                    egui::TextEdit::singleline(&mut self.path_text)
                        .hint_text(
                            "disc .bin / .cue, or an extracted folder - or drop it on the window",
                        )
                        .desired_width(ui.available_width() - 70.0),
                );
                let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if self.path_row && std::mem::take(&mut self.path_focus) {
                    resp.request_focus();
                }
                if (ui.button("Open").clicked() || enter) && !self.path_text.trim().is_empty() {
                    let p = PathBuf::from(self.path_text.trim());
                    self.path_row = false;
                    self.open_path(&p);
                }
            });
        }
        ui.horizontal_wrapped(|ui| {
            if let Some(p) = self.explorer.loading() {
                ui.spinner();
                ui.label(format!("Loading {} ...", p.display()));
            } else if let Some(lib) = &self.explorer.library {
                let kind = match lib.source {
                    Source::Disc(_) => "disc image",
                    Source::Extracted(_) => "extracted folder",
                };
                // The path is in the field above; this line names the file
                // and what it holds, and stays one row (full path on hover).
                let name = lib
                    .source
                    .path()
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| lib.source.path().display().to_string());
                ui.label(format!(
                    "{kind} {name}{}: {} archive entries, {} files",
                    lib.region
                        .as_ref()
                        .map(|r| format!(", {r}"))
                        .unwrap_or_default(),
                    lib.entries.len(),
                    lib.iso_files.len(),
                ))
                .on_hover_text(lib.source.path().display().to_string());
                // Another region's disc browses fine but cannot start the
                // game; say so where the region is named, not only in the
                // greyed-out Play button's hover text.
                if matches!(lib.source, Source::Disc(_))
                    && let Err(why) = crate::launch::playable_disc(lib)
                {
                    ui.label(RichText::new("- Play needs the USA disc").color(crate::theme::warn(ui)))
                        .on_hover_text(why);
                }
                let n = self.explorer.classified();
                if n < lib.entries.len() {
                    ui.spinner();
                    ui.label(format!("classifying {n}/{}", lib.entries.len()));
                }
            } else {
                ui.label("No disc open. Legaia Explorer reads your own Legend of Legaia disc image; no game data is included.");
            }
        });
        let mut dismiss = false;
        let mut show: Option<PathBuf> = None;
        let mut change_export = false;
        // An informational line clears itself after `STATUS_TTL` (the clock
        // restarts while the pointer is on it); an error stays until it is
        // dismissed, and can be copied for a bug report.
        let now = std::time::Instant::now();
        match &self.explorer.status {
            Some(s) if self.status_seen.as_ref().is_none_or(|(seen, _)| seen != s) => {
                self.status_seen = Some((s.clone(), now));
            }
            None => self.status_seen = None,
            _ => {}
        }
        if let Some(job) = &self.export_job {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(format!("Writing {}...", job.what));
            });
        }
        if let Some(e) = &self.explorer.error {
            // A banner that wraps, with its buttons first so a long message
            // (a path, a log tail) can never push them off the window.
            let red = ui.visuals().error_fg_color;
            egui::Frame::new()
                .fill(red.gamma_multiply(0.12))
                .stroke(egui::Stroke::new(1.0_f32, red.gamma_multiply(0.5)))
                .corner_radius(4.0)
                .inner_margin(egui::Margin::symmetric(8, 5))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal_top(|ui| {
                        // A failed game's banner copies the whole report: the
                        // banner is one line, a bug report wants the log's end.
                        let report = self
                            .error_report
                            .as_ref()
                            .filter(|(banner, _)| banner == e)
                            .map(|(_, report)| report.clone());
                        let report_is_game = report.is_some();
                        let (label, hint) = if report_is_game {
                            ("Copy report", "Copy a report of this failure, with the end of the log, for a bug report")
                        } else {
                            ("Copy", "Copy this message to the clipboard")
                        };
                        if ui.small_button(label).on_hover_text(hint).clicked() {
                            ui.ctx().copy_text(report.unwrap_or_else(|| e.clone()));
                        }
                        if report_is_game
                            && let Some(log) = &self.error_log
                            && ui
                                .small_button("Show log")
                                .on_hover_text(log.display().to_string())
                                .clicked()
                        {
                            show = log.parent().map(Path::to_path_buf);
                        }
                        dismiss = ui.small_button("Dismiss").clicked();
                        // In a vertical child the wrapped lines count
                        // toward the banner's height; laid straight into
                        // the horizontal row, every line past the first
                        // spilled out of the frame and off the panel.
                        ui.vertical(|ui| {
                            ui.add(egui::Label::new(RichText::new(e).color(red)).wrap());
                        });
                    });
                });
        } else if let Some(s) = &self.explorer.status {
            let green = crate::theme::good(ui);
            // A button of the line holding the keyboard (or a pad's) focus
            // holds the line up, as the pointer over it does.
            let mut focused = false;
            let row = ui.horizontal(|ui| {
                ui.colored_label(green, s);
                if let Some(dir) = &self.last_written {
                    let b = ui
                        .small_button("Show folder")
                        .on_hover_text(dir.display().to_string());
                    focused |= b.has_focus();
                    if b.clicked() {
                        show = Some(dir.clone());
                    }
                }
                // A controller's export had no save window to choose the
                // folder in: the way to change it sits beside it.
                if self.pad_active
                    && self.last_written.is_some()
                    && self.last_written == self.explorer.settings.last_export_dir
                {
                    let b = ui
                        .small_button("Change folder...")
                        .on_hover_text("Choose the folder exports go into");
                    focused |= b.has_focus();
                    change_export = b.clicked();
                }
                let b = ui.small_button("Dismiss");
                focused |= b.has_focus();
                dismiss = b.clicked();
            });
            if let Some((_, at)) = &mut self.status_seen {
                if row.response.contains_pointer() || focused {
                    *at = now;
                }
                let left = STATUS_TTL.saturating_sub(now.duration_since(*at));
                if left.is_zero() {
                    dismiss = true;
                } else {
                    ui.ctx().request_repaint_after(left);
                }
            }
        }
        if dismiss {
            self.explorer.error = None;
            self.explorer.status = None;
            self.status_seen = None;
        }
        if let Some(dir) = show {
            self.show_folder(&dir);
        }
        if change_export {
            self.pick_export_folder();
        }
    }

    /// The recent discs and folders, newest first, each with a remove
    /// button; the one clicked to open, if any. Entries whose path has gone
    /// since start-up are dropped as the list is drawn.
    fn recent_list(&mut self, ui: &mut egui::Ui) -> Option<PathBuf> {
        if self.explorer.settings.prune_recent() {
            self.explorer.save_settings();
        }
        let mut open = None;
        let mut forget = None;
        for p in &self.explorer.settings.recent {
            let kind = if p.is_dir() { "folder" } else { "disc" };
            let name = p
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| p.display().to_string());
            ui.horizontal(|ui| {
                if ui
                    .small_button("x")
                    .on_hover_text("Remove from this list (the file is not touched)")
                    .clicked()
                {
                    forget = Some(p.clone());
                }
                let current = self
                    .explorer
                    .library
                    .as_ref()
                    .is_some_and(|_| self.explorer.settings.last_source.as_ref() == Some(p));
                let text = format!("{name}  ({kind})");
                if ui
                    .add(egui::Button::selectable(current, text))
                    .on_hover_text(p.display().to_string())
                    .clicked()
                {
                    open = Some(p.clone());
                }
                if let Some(dir) = p.parent() {
                    ui.label(RichText::new(dir.display().to_string()).weak());
                }
            });
        }
        if let Some(p) = forget {
            self.explorer.settings.forget_recent(&p);
            self.explorer.save_settings();
        }
        if !self.explorer.settings.recent.is_empty() {
            ui.separator();
            if ui.button("Clear list").clicked() {
                self.explorer.settings.recent.clear();
                self.explorer.save_settings();
            }
        }
        open
    }

    /// The View menu: theme and UI zoom, both remembered.
    /// `focus_first`: the menu was opened from the keyboard or a
    /// controller, and its first choice takes the focus.
    fn view_menu(&mut self, ui: &mut egui::Ui, focus_first: bool) {
        ui.label(RichText::new("Theme").strong());
        let mut theme = self.explorer.settings.theme;
        for (i, t) in crate::settings::Theme::ALL.into_iter().enumerate() {
            let r = ui.radio_value(&mut theme, t, t.name());
            if focus_first && i == 0 {
                r.request_focus();
            }
        }
        if theme != self.explorer.settings.theme {
            self.explorer.settings.theme = theme;
            ui.ctx().set_theme(theme_preference(theme));
            self.explorer.save_settings();
        }
        ui.separator();
        ui.label(RichText::new(format!("Zoom ({:.0}%)", ui.ctx().zoom_factor() * 100.0)).strong());
        egui::gui_zoom::zoom_menu_buttons(ui);
        let wanted = crate::settings::sane_zoom(self.explorer.settings.zoom);
        if wanted > ui.ctx().zoom_factor() + 1e-3 {
            ui.label(
                RichText::new(format!(
                    "Make the window larger for {:.0}%.",
                    wanted * 100.0
                ))
                .weak(),
            );
        }
        ui.separator();
        if ui
            .add(egui::Button::new("Keyboard shortcuts").shortcut_text("?"))
            .clicked()
        {
            self.keys_open = true;
            ui.close();
        }
        ui.separator();
        ui.label(RichText::new("Export folder").strong());
        let dir = self.export_dir();
        let name = dir.file_name().map_or_else(
            || dir.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        );
        ui.label(RichText::new(name).weak())
            .on_hover_text(dir.display().to_string());
        if ui
            .button("Change...")
            .on_hover_text(
                "Where exports go: the save window opens here, and an export made with a \
                 controller is written here",
            )
            .clicked()
        {
            ui.close();
            self.pick_export_folder();
        }
        ui.separator();
        ui.label(RichText::new("Audio volume").strong());
        let mut vol = self.player.volume();
        let resp = ui
            .add(egui::Slider::new(&mut vol, 0..=100).suffix("%"))
            .on_hover_text("Sample, music and movie previews; 100% is the full mixer output");
        if resp.changed() {
            self.player.set_volume(vol);
            self.explorer.settings.volume = Some(vol);
        }
        if (resp.changed() && !resp.dragged()) || resp.drag_stopped() {
            self.explorer.save_settings();
        }
    }

    /// Push the remembered theme and zoom into egui once, then keep the
    /// settings in step with zoom changes (the keyboard shortcuts egui
    /// handles itself, and the View menu).
    fn sync_appearance(&mut self, ctx: &egui::Context) {
        let s = &mut self.explorer.settings;
        if !self.appearance_applied {
            self.appearance_applied = true;
            crate::theme::install(ctx);
            ctx.set_theme(theme_preference(s.theme));
            self.zoom_applied = crate::settings::sane_zoom(s.zoom);
            ctx.set_zoom_factor(self.zoom_applied);
            return;
        }
        // A zoom that differs from the last one set here is the user's.
        let z = ctx.zoom_factor();
        if (z - self.zoom_applied).abs() > 1e-3 {
            s.zoom = (z.is_finite() && (z - 1.0).abs() > 1e-3).then_some(z);
            self.explorer.save_settings();
        }
        // What is remembered is what was asked for; what is applied is what
        // this window has room for, so a window made larger gets it back.
        let size = ctx.content_rect().size() * z;
        let fit = crate::settings::fit_zoom(
            crate::settings::sane_zoom(self.explorer.settings.zoom),
            size.x,
            size.y,
        );
        self.zoom_applied = fit;
        if (fit - z).abs() > 1e-3 {
            ctx.set_zoom_factor(fit);
            ctx.request_repaint();
        }
    }

    /// Open `dir` in the file manager, reporting a failure on the error line.
    fn show_folder(&mut self, dir: &Path) {
        if let Err(e) = crate::launch::open_folder(dir) {
            self.explorer.error = Some(format!(
                "Could not open a file manager on {}: {e}",
                dir.display()
            ));
        }
    }

    /// *Play*: start the game on the open disc, when the game binary ships
    /// beside this one and the disc is the one it runs on. The hover text
    /// says why it is unavailable otherwise.
    /// Why the game cannot start now, in the words of the Play button's hover
    /// and of `F5`.
    fn play_blocked(&self) -> Option<String> {
        let disc = match &self.explorer.library {
            Some(lib) => crate::launch::playable_disc(lib),
            None => Err("Open your Legend of Legaia (USA) disc image first.".to_string()),
        };
        match (&self.engine, &disc) {
            _ if self.game.is_some() && self.game_is_launcher => {
                Some("The game's options window is open; its Play starts the game.".to_string())
            }
            _ if self.game.is_some() => Some("The game is running.".to_string()),
            (None, _) => Some(format!(
                "{} was not found. It ships in the release folder's {} \
                 folder; keep that folder beside Legaia Explorer.",
                crate::launch::engine_file_name(),
                legaia_app_theme::release_layout::TOOLS_DIR
            )),
            (_, Err(why)) => Some(why.clone()),
            _ => None,
        }
    }

    fn play_button(&mut self, ui: &mut egui::Ui) {
        let running = self.game.is_some();
        let blocked = self.play_blocked();
        let button = if running && self.game_is_launcher {
            egui::Button::new("Options open")
        } else if running {
            egui::Button::new("Game running")
        } else if blocked.is_none() {
            crate::theme::primary_button("Play")
        } else {
            egui::Button::new("Play")
        };
        let resp = ui.add_enabled(blocked.is_none(), button);
        let resp = match &blocked {
            Some(why) => resp.on_disabled_hover_text(why),
            None => resp.on_hover_text(
                "Start the game (legaia-engine) on this disc, at the title screen (F5). \
                 Saves and settings are the same as when you start the game directly; \
                 the display options set in the game's own launcher apply here too.",
            ),
        };
        if resp.clicked() {
            self.start_game();
        }
        // The game's launcher screen: its options, keys and saves, which a
        // start from here plays with. Without it they sit in another program
        // a player who began here may never open.
        // The long label where the row has room for it beside View and
        // About, else the short one, so the bar stays one row at the
        // minimum window width; the hover says whose options they are.
        let left = ui.max_rect().right() - ui.cursor().left();
        let label = if left >= VIEW_ABOUT_W + GAME_OPTIONS_LONG_W {
            "Game options"
        } else {
            "Options"
        };
        let options = ui
            .add_enabled(blocked.is_none(), egui::Button::new(label))
            .on_hover_text(
                "Open the game's own launcher on this disc: the look (Enhanced or the \
                 original game), the enhancements, gameplay tweaks, keys and saves. \
                 Its Play starts the game; Play here uses the same settings. In game, \
                 F12 changes them too. Shift+F5.",
            );
        let options = match &blocked {
            Some(why) => options.on_disabled_hover_text(why),
            None => options,
        };
        if options.clicked() {
            self.open_game_launcher();
        }
    }

    /// Start the game on the open disc (the Play button, `LEGAIA_EXPLORER_PLAY`).
    fn start_game(&mut self) {
        self.start_engine(false);
    }

    /// Start the game's launcher screen on the open disc (*Game options*):
    /// its option list, key table and saves, then its own Play.
    fn open_game_launcher(&mut self) {
        self.start_engine(true);
    }

    /// The game itself, or its launcher screen with the disc chosen.
    fn start_engine(&mut self, launcher: bool) {
        let disc = match &self.explorer.library {
            Some(lib) => crate::launch::playable_disc(lib),
            None => return,
        };
        if self.game.is_some() {
            return;
        }
        if let (Some(engine), Ok(bin)) = (self.engine.clone(), disc) {
            self.player.stop();
            // The path as chosen (a `.cue` stays a `.cue`), like the game's
            // own picker.
            let chosen = self
                .explorer
                .settings
                .last_source
                .clone()
                .filter(|p| p.is_file())
                .unwrap_or_else(|| bin.clone());
            let args = if launcher {
                crate::launch::launcher_args(&chosen)
            } else {
                crate::launch::play_args(&bin)
            };
            match crate::launch::Game::start_with(
                &engine,
                args,
                crate::launch::engine_data_dir().as_deref(),
            ) {
                Ok(g) => {
                    self.explorer.status = Some(if launcher {
                        "Opened the game's options. Its Play starts the game with them.".into()
                    } else {
                        format!("Started the game on {}", bin.display())
                    });
                    self.explorer.error = None;
                    self.last_written = None;
                    self.game = Some(g);
                    self.game_is_launcher = launcher;
                    // Remember the disc for the game itself, so starting it
                    // directly later does not ask for it again.
                    if let Some(file) = crate::launch::engine_launcher_settings()
                        && let Err(e) = crate::launch::record_disc_for_engine(&file, &chosen)
                    {
                        log::warn!("not recording the disc for the game: {e:#}");
                    }
                }
                Err(e) => self.explorer.error = Some(format!("Could not start the game: {e:#}")),
            }
        }
    }

    /// Notice when a started game ends; a failed exit is reported with the
    /// last lines of its log.
    fn poll_game(&mut self, ctx: &egui::Context) {
        let Some(game) = &mut self.game else { return };
        ctx.request_repaint_after(std::time::Duration::from_millis(500));
        let Some(res) = game.poll() else { return };
        let log = game.log.clone();
        self.game = None;
        match res {
            Ok(()) => {
                // The launcher ends with a game its Play started, or on Quit.
                self.explorer.status = Some(if self.game_is_launcher {
                    "The game's options window has closed.".into()
                } else {
                    "The game has closed.".into()
                })
            }
            Err(headline) => {
                // The launcher's words for the same failure: how it ended,
                // then the reason its log gives, if it gives one.
                use legaia_app_theme::game_exit;
                let log_text = log.as_ref().and_then(|l| std::fs::read_to_string(l).ok());
                let why = log_text.as_deref().and_then(game_exit::failure_detail);
                let mut msg = headline.clone();
                if let Some(why) = &why {
                    msg.push(' ');
                    msg.push_str(why);
                }
                if let Some(log) = &log {
                    msg.push_str(&format!("\nFull log: {}", log.display()));
                }
                self.error_log = log.clone();
                self.error_report = Some((
                    msg.clone(),
                    game_exit::report_text(
                        "legaia-explorer",
                        env!("CARGO_PKG_VERSION"),
                        &headline,
                        why.as_deref(),
                        log_text.as_deref(),
                        game_exit::home_folder().as_deref(),
                    ),
                ));
                self.explorer.error = Some(msg);
            }
        }
    }

    fn browser(&mut self, ui: &mut egui::Ui) {
        let Some(lib) = self.explorer.library.clone() else {
            ui.label("Open a disc image to browse its PROT.DAT archive.");
            return;
        };
        ui.horizontal(|ui| {
            ui.label("Search:");
            // The field takes the panel's width, less the clear button and
            // the field's own inner margin (outside `desired_width`). A
            // resizable panel grows to fit its content, so a field even a
            // few points too wide would widen the panel on every frame.
            let clear_w = if self.filter.is_empty() { 0.0 } else { 28.0 };
            let resp = ui.add(
                egui::TextEdit::singleline(&mut self.filter)
                    .hint_text("scene, place name, kind or index (Ctrl+F)")
                    .desired_width((ui.available_width() - clear_w - 16.0).max(80.0)),
            );
            if !self.filter.is_empty()
                && ui
                    .small_button("x")
                    .on_hover_text("Clear the search (Esc)")
                    .clicked()
            {
                self.filter.clear();
            }
            if ui.input_mut(|i| {
                i.consume_shortcut(&egui::KeyboardShortcut::new(
                    egui::Modifiers::COMMAND,
                    egui::Key::F,
                ))
            }) {
                resp.request_focus();
            }
            if resp.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                self.filter.clear();
            }
        });
        // The kinds this disc has, with how many entries each holds.
        let count = |kind: &str| {
            (0..lib.entries.len())
                .filter(|&p| {
                    self.explorer
                        .class_of(p)
                        .is_some_and(|c| crate::kind_of_class(c.name()) == kind)
                })
                .count()
        };
        let kinds: Vec<(&'static str, usize)> = crate::KINDS
            .iter()
            .map(|&k| (k, count(k)))
            .filter(|&(_, n)| n > 0)
            .collect();
        ui.horizontal(|ui| {
            ui.label("Show:");
            egui::ComboBox::from_id_salt("browse_kind")
                .selected_text(self.class_filter.as_deref().unwrap_or("Everything"))
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.class_filter, None, "Everything");
                    for (k, n) in kinds {
                        ui.selectable_value(
                            &mut self.class_filter,
                            Some(k.to_string()),
                            format!("{k} ({n})"),
                        );
                    }
                });
        });
        let filter = self.filter.trim().to_string();
        let filtering = !filter.is_empty() || self.class_filter.is_some();
        // The rows the search shows, per block, in disc order.
        let shown: Vec<(usize, Vec<usize>)> = self
            .explorer
            .blocks
            .iter()
            .enumerate()
            .map(|(b, block)| {
                let rows = block
                    .entries
                    .iter()
                    .copied()
                    .filter(|&pos| {
                        self.explorer.entry_matches(
                            pos,
                            &block.name,
                            &filter,
                            self.class_filter.as_deref(),
                        )
                    })
                    .collect();
                (b, rows)
            })
            .filter(|(_, rows): &(usize, Vec<usize>)| !rows.is_empty())
            .collect();
        // Every row the search shows, in disc order: what the arrow keys walk.
        let visible: Vec<usize> = shown.iter().flat_map(|(_, r)| r.iter().copied()).collect();
        if filtering {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format!(
                        "{} of {} entries match, in {} blocks",
                        visible.len(),
                        lib.entries.len(),
                        shown.len()
                    ))
                    .weak(),
                );
                if ui.small_button("Show all").clicked() {
                    self.filter.clear();
                    self.class_filter = None;
                }
            });
        }
        ui.separator();
        if filtering && visible.is_empty() {
            ui.add_space(8.0);
            ui.label(RichText::new("Nothing matches.").strong());
            ui.add(
                egui::Label::new(
                    "Every word has to match a scene label (town01), a place name (Rim Elm), \
                     a kind of data (tim, vab, scene_tmd_stream) or an entry number (865 or \
                     0x361).",
                )
                .wrap(),
            );
        }
        let mut clicked = None;
        let mut iso_save: Option<crate::library::IsoFile> = None;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
                for (b, rows) in shown {
                    let block = &self.explorer.blocks[b];
                    let first = lib.entries[rows[0]].index;
                    let open = filtering.then_some(true).or_else(|| {
                        self.reveal
                            .filter(|s| block.entries.contains(s))
                            .map(|_| true)
                    });
                    let title = match crate::place_name(&block.name) {
                        Some(place) => {
                            format!("{first:04}  {}  -  {place}  ({})", block.name, rows.len())
                        }
                        None => format!("{first:04}  {}  ({})", block.name, rows.len()),
                    };
                    // egui lays a header's text out unwrapped, so the
                    // widest block title would push the panel past its
                    // width (the panel grows to its content). Truncate it to
                    // the panel instead; the full title is on hover.
                    let (header, elided) = truncated_header(ui, &title);
                    let shown_block = egui::CollapsingHeader::new(header)
                        .id_salt(("block", first))
                        .open(open)
                        .show(ui, |ui| {
                            for pos in rows {
                                let e = &lib.entries[pos];
                                let class = self
                                    .explorer
                                    .class_of(pos)
                                    .map(|c| c.name())
                                    .unwrap_or("...");
                                let kind = if class == "..." {
                                    class
                                } else {
                                    crate::kind_of_class(class)
                                };
                                let text = format!(
                                    "{:04}  {:>8}  {}",
                                    e.index,
                                    human_size(e.size_bytes),
                                    kind
                                );
                                let sel = self.explorer.selected == Some(pos);
                                let resp =
                                    ui.selectable_label(sel, RichText::new(text).monospace());
                                if self.reveal == Some(pos) {
                                    resp.scroll_to_me(Some(egui::Align::Center));
                                }
                                if resp.on_hover_text(class).clicked() {
                                    clicked = Some(pos);
                                }
                            }
                        });
                    if elided {
                        shown_block.header_response.on_hover_text(title);
                    }
                }
                if !lib.iso_files.is_empty() {
                    ui.separator();
                    egui::CollapsingHeader::new(format!("Disc files ({})", lib.iso_files.len()))
                        .id_salt("iso")
                        .show(ui, |ui| {
                            for f in lib.iso_files.clone() {
                                ui.horizontal(|ui| {
                                    ui.monospace(format!(
                                        "{:<20} {:>9}",
                                        f.path,
                                        human_size(f.size as u64)
                                    ));
                                    if ui.small_button("Save...").clicked() {
                                        iso_save = Some(f.clone());
                                    }
                                });
                            }
                        });
                }
            });
        self.reveal = None;
        if let Some(pos) = clicked {
            self.select(pos);
        }
        // Keyboard: with no text field focused, the arrows, Page Up / Down,
        // Home and End walk the rows the search shows, opening a block and
        // scrolling to the entry as they go.
        let typing = ui.egui_wants_keyboard_input();
        if !typing && !visible.is_empty() {
            let cur = self
                .explorer
                .selected
                .and_then(|s| visible.iter().position(|&p| p == s));
            let last = visible.len() - 1;
            if let Some(k) = list_step(ui, cur, last) {
                let pos = visible[k];
                self.select(pos);
                self.reveal = Some(pos);
            }
        }
        if let Some(f) = iso_save {
            let name = f.path.rsplit('/').next().unwrap_or(&f.path).to_string();
            if let Some(out) = self.export_target(&name, "bin") {
                let res = lib.iso_file_bytes(&f).and_then(|b| export::bytes(&b, &out));
                self.report(&f.path, res, &out);
            }
        }
    }

    fn preview(&mut self, ui: &mut egui::Ui) {
        let Some(lib) = self.explorer.library.clone() else {
            self.start_page(ui);
            return;
        };
        let Some(pos) = self.explorer.selected else {
            ui.add_space(12.0);
            ui.label(
                RichText::new("Pick an entry on the left to preview it")
                    .size(17.0)
                    .color(crate::theme::accent(ui)),
            );
            ui.add_space(6.0);
            for line in [
                "Each block is one scene or system file of the game, named as on the disc \
                 and by the place it is (town01 - Rim Elm).",
                "Search by place, kind of data or entry number (Ctrl+F); the arrow keys \
                 walk the list.",
                "Characters & monsters (Ctrl+2) and Movies & voice (Ctrl+3) show the \
                 game's actors and movies directly; Extract (Ctrl+4) unpacks everything.",
            ] {
                ui.add(egui::Label::new(line).wrap());
            }
            return;
        };
        let Some(a) = self.explorer.analysis.clone().filter(|a| a.pos == pos) else {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Reading entry...");
            });
            return;
        };
        let e = &lib.entries[pos];
        ui.horizontal_wrapped(|ui| {
            // The heading agrees with the tree: the block the entry belongs
            // to on the disc. The stem is the extracted file's name, whose
            // CDNAME label runs two entries late, so it can name a neighbour
            // (Rim Elm's first map files as `0001_gameover_data`).
            let stem = lib.stem(e.index);
            match lib.retail_block(e.index) {
                Some(retail) if lib.label(e.index) != Some(retail) => {
                    ui.heading(format!("{:04}_{retail}", e.index))
                        .on_hover_text(format!(
                            "On the disc this entry belongs to {retail}. Exported and extracted \
                         files keep the name {stem}: legaia-extract applies the CDNAME \
                         numbers two entries late (docs/formats/cdname.md)."
                        ));
                }
                _ => {
                    ui.heading(stem);
                }
            }
            ui.label(format!(
                "{} - {}",
                crate::kind_of_class(a.class.name()),
                human_size(e.size_bytes)
            ))
            .on_hover_text(format!("Category: {}", a.class.name()));
        });
        let count = |t: Tab| match t {
            Tab::Textures => Some(a.tims.len()),
            Tab::Model => Some(a.tmds.len()),
            Tab::Audio => Some(a.vabs.len() + a.seqs.len()),
            Tab::Text => Some(a.mes.len() + a.dialog.len()),
            _ => None,
        };
        // The chosen tab stays the preference while walking entries; an entry
        // with nothing for it shows its Overview instead of an empty tab.
        let shown = if count(self.tab) == Some(0) {
            Tab::Overview
        } else {
            self.tab
        };
        // Ctrl+Tab / Ctrl+Shift+Tab (and Ctrl+Page Down / Up) step through
        // the tabs this entry has something for.
        let step = ui.input_mut(|i| {
            let back = [egui::Key::Tab, egui::Key::PageUp].map(|k| {
                let m = if k == egui::Key::Tab {
                    egui::Modifiers::COMMAND | egui::Modifiers::SHIFT
                } else {
                    egui::Modifiers::COMMAND
                };
                i.consume_shortcut(&egui::KeyboardShortcut::new(m, k))
            });
            let fwd = [egui::Key::Tab, egui::Key::PageDown].map(|k| {
                i.consume_shortcut(&egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, k))
            });
            match (back.contains(&true), fwd.contains(&true)) {
                (true, _) => Some(false),
                (false, true) => Some(true),
                _ => None,
            }
        });
        if let Some(forward) = step {
            self.tab = cycle_tab(shown, forward, |t| count(t).is_none_or(|n| n > 0));
        }
        // Wraps, so a narrow window keeps every tab reachable; an empty
        // tab drops its "(0)" (it is greyed out anyway), which keeps the
        // row to one line at the minimum window width.
        ui.horizontal_wrapped(|ui| {
            // Tabs sit closer than ordinary buttons: a selectable tab needs
            // no gap to read as separate, and the six fit one row of the
            // narrowest pane.
            ui.spacing_mut().item_spacing.x = 2.0;
            // An entry with big counts would wrap the last tab onto a row of
            // its own at the narrowest pane; the counts go first.
            let room_for_counts = ui.available_width() >= TAB_COUNTS_MIN_WIDTH;
            for t in Tab::ALL {
                let n = count(t);
                let label = tab_label(t.name(), n.filter(|_| room_for_counts));
                let enabled = n.is_none_or(|n| n > 0);
                let resp = ui
                    .add_enabled(enabled, egui::Button::selectable(shown == t, label))
                    .on_disabled_hover_text(format!(
                        "Nothing for the {} tab in this entry",
                        t.name()
                    ));
                if resp.clicked() {
                    self.tab = t;
                }
            }
        });
        ui.separator();
        match shown {
            Tab::Overview => self.overview(ui, &lib, &a),
            Tab::Textures => self.textures(ui, &lib, &a),
            Tab::Model => self.model_tab(ui, &lib, &a),
            Tab::Audio => self.audio(ui, &lib, &a),
            Tab::Text => self.text(ui, &lib, &a),
            Tab::Hex => {
                if self.take_export_key() {
                    let name = format!("{}.BIN", lib.stem(a.index));
                    if let Some(out) = self.export_target(&name, "BIN") {
                        let res = export::bytes(&a.bytes, &out);
                        self.report("entry", res, &out);
                    }
                }
                hex_view(ui, &a.bytes, &mut self.hex_goto)
            }
        }
    }

    fn overview(&mut self, ui: &mut egui::Ui, lib: &crate::Library, a: &Analysis) {
        let e = &lib.entries[a.pos];
        // A value wider than the panel ends in an ellipsis and shows whole
        // on hover; the grid's value column is otherwise unbounded and ran
        // off the window's edge at the minimum width.
        let value = |ui: &mut egui::Ui, text: String| {
            ui.add(egui::Label::new(RichText::new(&text).monospace()).truncate())
                .on_hover_text(text)
        };
        // The plain half: what the entry is and holds, in a player's words,
        // each row's technical name on its hover.
        let kind = crate::kind_of_class(a.class.name());
        let counts = crate::ContentCounts {
            tims: a.tims.len(),
            tmds: a.tmds.len(),
            vabs: a.vabs.len(),
            seqs: a.seqs.len(),
            mes: a.mes.len(),
            dialog: a.dialog.len(),
            lzs: a.tim_sections.len(),
        };
        let holds = crate::contents_plain(&counts);
        let block = lib.retail_block(e.index);
        let place = block.and_then(crate::place_name);
        ui.label(RichText::new(kind).strong().size(16.0));
        ui.add(egui::Label::new(crate::kind_blurb(kind)).wrap());
        ui.add_space(6.0);
        egui::Grid::new("overview_plain")
            .num_columns(2)
            .striped(true)
            .show(ui, |ui| {
                ui.label("Place").on_hover_text(
                    "The scene this entry belongs to: the disc keeps each place's \
                     files side by side. The technical details name its CDNAME block.",
                );
                let place_text = match (place, block) {
                    (Some(p), Some(b)) => format!("{p} ({b})"),
                    (None, Some(b)) => b.to_string(),
                    _ => "-".to_string(),
                };
                ui.add(egui::Label::new(&place_text).truncate())
                    .on_hover_text(place_text.clone());
                ui.end_row();
                ui.label("Holds").on_hover_text(
                    "What this program found inside: textures are TIM images, 3D models \
                     TMD meshes, sound banks VAB, music tracks SEQ, text blocks MES, \
                     compressed sections LZS. The tabs above show each.",
                );
                let holds_text = if holds.is_empty() {
                    "nothing this program can show; the Hex tab has the bytes".to_string()
                } else {
                    holds.join(", ")
                };
                ui.add(egui::Label::new(&holds_text).wrap());
                ui.end_row();
                ui.label("Size");
                ui.label(human_size(e.size_bytes)).on_hover_text(format!(
                    "{} bytes, {} disc sectors of 2048 bytes",
                    e.size_bytes, e.size_sectors
                ));
                ui.end_row();
                ui.label("Entry").on_hover_text(
                    "Its number in the game's archive (PROT.DAT), the file that \
                     holds almost everything on the disc.",
                );
                ui.label(format!("{} of {}", e.index, lib.entries.len()))
                    .on_hover_text(format!("PROT index {} (0x{:X})", e.index, e.index));
                ui.end_row();
            });
        ui.add_space(8.0);
        // The technical half, behind a toggle the settings remember.
        let details = self.explorer.settings.overview_details;
        if ui
            .button(if details {
                "Hide technical details"
            } else {
                "Show technical details"
            })
            .on_hover_text(
                "The archive index, CDNAME block, disc position and chunk table, as the \
                 format pages and the command-line tools name them",
            )
            .clicked()
        {
            self.explorer.settings.overview_details = !details;
            self.explorer.save_settings();
        }
        if self.explorer.settings.overview_details {
            ui.add_space(4.0);
            egui::Grid::new("overview").num_columns(2).striped(true).show(ui, |ui| {
                ui.label("PROT index")
                    .on_hover_text("The entry's index in PROT.DAT's table of contents");
                value(ui, format!("{} (0x{:X})", e.index, e.index));
                ui.end_row();
                ui.label("Filename label")
                    .on_hover_text("The name legaia-extract gives the entry's file");
                value(ui, lib.label(e.index).unwrap_or("-").to_string());
                ui.end_row();
                ui.label("Retail CDNAME block")
                    .on_hover_text("CDNAME #define numbers are raw-TOC indices, two above extraction indices (docs/formats/cdname.md)");
                value(ui, block.unwrap_or("-").to_string());
                ui.end_row();
                ui.label("Category")
                    .on_hover_text("legaia-disc-audit's class for the entry, behind the plain kind");
                value(ui, a.class.name().to_string());
                ui.end_row();
                ui.label("Location")
                    .on_hover_text("Its first sector (LBA) and byte offset in PROT.DAT");
                value(ui, format!("PROT.DAT LBA {} (+0x{:X})", e.start_lba, e.byte_offset));
                ui.end_row();
                ui.label("Size");
                value(
                    ui,
                    format!("{} ({} sectors, {} bytes)", human_size(e.size_bytes), e.size_sectors, e.size_bytes),
                );
                ui.end_row();
                ui.label("Contents");
                let contents: Vec<String> = [
                    (a.tims.len(), "TIM"),
                    (a.tmds.len(), "TMD"),
                    (a.vabs.len(), "VAB"),
                    (a.seqs.len(), "SEQ"),
                    (a.mes.len(), "MES"),
                    (a.tim_sections.len(), "LZS sections"),
                ]
                .into_iter()
                .filter(|(n, _)| *n > 0)
                .map(|(n, what)| format!("{n} {what}"))
                .collect();
                value(
                    ui,
                    if contents.is_empty() {
                        "no recognised sub-assets".to_string()
                    } else {
                        contents.join(", ")
                    },
                );
                ui.end_row();
            });
            if !a.chunks.is_empty() {
                ui.add_space(8.0);
                ui.label(RichText::new("DATA_FIELD chunks").strong()).on_hover_text(
                    "The typed chunks a streamed entry is cut into (docs/formats/data-field.md)",
                );
                egui::Grid::new("chunks").striped(true).show(ui, |ui| {
                    ui.label("#");
                    ui.label("offset");
                    ui.label("type");
                    ui.label("size");
                    ui.end_row();
                    for (i, c) in a.chunks.iter().enumerate() {
                        ui.monospace(i.to_string());
                        ui.monospace(format!("0x{:X}", c.header_offset));
                        ui.monospace(c.type_name);
                        ui.monospace(c.size.to_string());
                        ui.end_row();
                    }
                });
            }
        }
        ui.add_space(8.0);
        let key = self.take_export_key();
        if ui
            .button("Save the raw entry (.BIN)...")
            .on_hover_text(EXPORT_KEY_HINT)
            .clicked()
            || key
        {
            let name = format!("{}.BIN", lib.stem(e.index));
            if let Some(out) = self.export_target(&name, "BIN") {
                let res = export::bytes(&a.bytes, &out);
                self.report("entry", res, &out);
            }
        }
    }

    fn textures(&mut self, ui: &mut egui::Ui, lib: &crate::Library, a: &Analysis) {
        if a.tims.is_empty() {
            ui.label("No TIM textures in this entry.");
            return;
        }
        self.tim_sel = self.tim_sel.min(a.tims.len() - 1);
        // Wide enough for one unwrapped row ("#0   320x8 4bpp lzs#0 @0x4F00")
        // when the pane has room, but never more than half of it: at the
        // minimum window width a 270-point list left the CLUT and zoom
        // sliders clipped off the pane's right edge.
        let (list_default, list_max) = tim_list_width(ui.available_width());
        egui::Panel::left("tim_list")
            .resizable(true)
            .default_size(list_default)
            .min_size(list_default.min(TIM_LIST_MIN))
            .max_size(list_max)
            .show(ui, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
                    for (i, t) in a.tims.iter().enumerate() {
                        let text = format!(
                            "#{i:<3} {}x{} {}bpp {} @0x{:X}",
                            t.width,
                            t.height,
                            t.bpp,
                            t.origin.describe(),
                            t.offset
                        );
                        if ui
                            .selectable_label(
                                self.tim_sel == i,
                                RichText::new(text.as_str()).monospace(),
                            )
                            .on_hover_text(text)
                            .clicked()
                        {
                            self.tim_sel = i;
                            self.clut = 0;
                        }
                    }
                });
            });
        let t = a.tims[self.tim_sel].clone();
        // The CLUT and Zoom controls share a row when the pane has room for
        // both, and each take one on a narrower pane: a wrapping row does
        // not wrap a slider's value box with it, so a wrapped Zoom slider
        // ran off the pane's edge. The sliders shrink so a label, its slider
        // and its value box fit one row of the narrowest pane.
        let stacked = ui.available_width() < TEX_CONTROLS_ROW_MIN;
        let zoom_row = |ui: &mut egui::Ui, zoom: &mut f32, fit: &mut bool, w: f32| {
            ui.spacing_mut().slider_width = w;
            ui.label("Zoom:");
            ui.add(egui::Slider::new(zoom, TIM_ZOOM.0..=TIM_ZOOM.1).step_by(0.5))
                .on_hover_text("Ctrl + scroll over the image zooms too; drag the image to move it");
            if ui
                .small_button("Fit")
                .on_hover_text("The whole texture in the pane")
                .clicked()
            {
                *fit = true;
            }
        };
        let slider_w =
            (ui.available_width() / if stacked { 1.0 } else { 2.0 } - 150.0).clamp(60.0, 140.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().slider_width = slider_w;
            if t.palettes > 1 {
                ui.label("CLUT:");
                ui.add(egui::Slider::new(&mut self.clut, 0..=t.palettes - 1));
            } else if t.palettes == 1 {
                ui.label("1 CLUT");
            } else if t.bpp <= 8 {
                ui.label("no CLUT in the file: indices shown in grey")
                    .on_hover_text(
                        "The game draws this texture with a palette another upload left \
                         in VRAM, so the file alone has no colours. The 3D model tab's \
                         scene VRAM shows it as the game does.",
                    );
            } else {
                ui.label("direct colour (no CLUT)");
            }
            if !stacked {
                ui.separator();
                zoom_row(ui, &mut self.tim_zoom, &mut self.tim_fit, slider_w);
            }
        });
        if stacked {
            ui.horizontal(|ui| zoom_row(ui, &mut self.tim_zoom, &mut self.tim_fit, slider_w));
        }
        let key = self.take_export_key();
        ui.horizontal_wrapped(|ui| {
            if ui
                .button("Export PNG...")
                .on_hover_text(EXPORT_KEY_HINT)
                .clicked()
                || key
            {
                let name = format!(
                    "{}_tim{:02}_clut{}.png",
                    lib.stem(a.index),
                    self.tim_sel,
                    self.clut
                );
                if let Some(out) = self.export_target(&name, "png") {
                    let res = export::tim_png(a, &t, self.clut, &out);
                    self.report("PNG", res, &out);
                }
            }
            if ui.button("Export all PNGs...").clicked() {
                let name = format!("{}_tim00.png", lib.stem(a.index));
                if let Some(first) = self.export_target(&name, "png") {
                    let dir = first.parent().map(Path::to_path_buf).unwrap_or_default();
                    if let Some(a) = self.explorer.analysis.clone() {
                        let stem = lib.stem(a.index);
                        self.start_export(
                            format!("{} PNGs", a.tims.len()),
                            Some(dir.clone()),
                            move || {
                                let mut failed = Vec::new();
                                for (i, t) in a.tims.iter().enumerate() {
                                    let p = dir.join(format!("{stem}_tim{i:02}.png"));
                                    if let Err(e) = export::tim_png(&a, t, 0, &p) {
                                        failed.push(format!("#{i}: {e:#}"));
                                    }
                                }
                                batch_outcome("PNGs", a.tims.len(), &failed, &dir)
                            },
                        );
                    }
                }
            }
        });
        let key = (a.pos, self.tim_sel, self.clut);
        // A failed decode is remembered with its key like a texture, so the
        // message stays up and the TIM is not re-decoded every frame.
        let have = self.tim_tex.as_ref().map(|(k, _)| *k) == Some(key)
            || self.tim_err.as_ref().map(|(k, _)| *k) == Some(key);
        if !have {
            self.tim_tex = None;
            self.tim_err = None;
            match export::tim_rgba(a, &t, self.clut) {
                Ok((w, h, rgba)) if w > 0 && h > 0 => {
                    let img = ColorImage::from_rgba_unmultiplied([w, h], &rgba);
                    self.tim_tex = Some((
                        key,
                        ui.ctx().load_texture("tim", img, TextureOptions::NEAREST),
                    ));
                }
                Ok((w, h, _)) => {
                    self.tim_err = Some((key, format!("empty TIM ({w}x{h})")));
                }
                Err(e) => self.tim_err = Some((key, format!("{e:#}"))),
            }
        }
        if let Some((_, e)) = &self.tim_err {
            ui.colored_label(ui.visuals().error_fg_color, e);
        }
        if let Some((_, tex)) = &self.tim_tex {
            let texels = tex.size_vec2();
            if std::mem::take(&mut self.tim_fit) {
                self.tim_zoom = fit_tim_zoom(texels, ui.available_size());
            }
            let size = texels * self.tim_zoom;
            let mut zoom_by = 1.0;
            egui::ScrollArea::both()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::hover());
                    checkerboard(ui, rect);
                    ui.painter().image(
                        tex.id(),
                        rect,
                        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                        Color32::WHITE,
                    );
                    // The texel under the pointer, for whoever is matching
                    // a model's UVs or a palette cell against the image.
                    if let Some(p) = resp.hover_pos() {
                        let at = (p - rect.min) / self.tim_zoom;
                        let (x, y) = (
                            (at.x as usize).min(texels.x as usize - 1),
                            (at.y as usize).min(texels.y as usize - 1),
                        );
                        resp.on_hover_text_at_pointer(format!("x {x}, y {y}"));
                        zoom_by = ui.input(|i| i.zoom_delta());
                    }
                });
            if zoom_by != 1.0 {
                self.tim_zoom = step_tim_zoom(self.tim_zoom, zoom_by);
            }
        }
    }

    /// The scene VRAM for entry `a`, starting its build if needed. `Err`
    /// carries a status line (building / failed / no scene).
    fn scene_vram(
        &mut self,
        lib: &Arc<crate::Library>,
        a: &Analysis,
    ) -> Result<(String, Arc<SceneVram>), String> {
        let Some(name) = lib.retail_block(a.index).map(str::to_string) else {
            return Err("this entry belongs to no CDNAME scene".into());
        };
        let kind = crate::scene_vram::load_kind(&name, a.class);
        let key = format!("{name}:{kind:?}");
        let slot = self.scenes.entry(key.clone()).or_insert_with(|| {
            let (tx, rx) = std::sync::mpsc::channel();
            let lib = Arc::clone(lib);
            let (index, class) = (a.index, a.class);
            std::thread::spawn(move || {
                let _ = tx.send(
                    crate::scene_vram::build(&lib, index, class).map_err(|e| format!("{e:#}")),
                );
            });
            SceneSlot::Building(rx)
        });
        if let SceneSlot::Building(rx) = slot
            && let Ok(res) = rx.try_recv()
        {
            *slot = match res {
                Ok(v) => SceneSlot::Ready(Arc::new(v)),
                Err(e) => SceneSlot::Failed(e),
            };
        }
        match slot {
            SceneSlot::Ready(v) => Ok((key, Arc::clone(v))),
            SceneSlot::Building(_) => Err(format!("building scene {name} ({kind:?} load)...")),
            SceneSlot::Failed(e) => Err(format!("scene {name}: {e}")),
        }
    }

    fn model_tab(&mut self, ui: &mut egui::Ui, lib: &Arc<crate::Library>, a: &Analysis) {
        if a.tmds.is_empty() {
            ui.label("No TMD meshes in this entry.");
            return;
        }
        self.tmd_sel = self.tmd_sel.min(a.tmds.len() - 1);
        let t = a.tmds[self.tmd_sel].clone();
        let key = (a.pos, self.tmd_sel);
        if self.model.as_ref().map(|m| m.key) != Some(key) {
            // Start on the entry's own TIMs only when they hold every CLUT
            // and texel the mesh's textured prims read; otherwise the mesh
            // samples pages another entry of its scene uploads, so start
            // on the scene.
            let model = Model::from_tmd(a.tmd_bytes(&t));
            let entry_vram = a.vram();
            let source = default_tex_source(model.as_ref(), &entry_vram);
            self.model = Some(ModelView {
                key,
                model,
                entry_vram,
                source,
                drawn_with: None,
                view: Viewport::new(Shading::Textured),
            });
        }
        // One row at the minimum window width: the mesh picker, the shading
        // picker, reset, and the exports folded into one menu. The triangle
        // count and the orbit hint are drawn inside the view.
        ui.horizontal_wrapped(|ui| {
            egui::ComboBox::from_id_salt("tmd")
                .selected_text(format!("Model #{}", self.tmd_sel))
                .show_ui(ui, |ui| {
                    for (i, t) in a.tmds.iter().enumerate() {
                        ui.selectable_value(
                            &mut self.tmd_sel,
                            i,
                            format!(
                                "#{i} {} @0x{:X}: {} obj, {} verts, {} prims{}",
                                t.origin.describe(),
                                t.offset,
                                t.objects,
                                t.verts,
                                t.prims,
                                if t.scene_stream {
                                    " (scene stream)"
                                } else {
                                    ""
                                }
                            ),
                        );
                    }
                })
                .response
                .on_hover_text(format!("{} models (TMD) in this entry", a.tmds.len()));
            if let Some(mv) = &mut self.model {
                mv.view.controls(
                    ui,
                    "texel x colour / 128, sampled from the chosen VRAM image",
                );
            }
            // Ctrl+E takes the textured glTF, the export that keeps the
            // most of what the view shows.
            let mut wanted = self.take_export_key().then_some("glb");
            pad_menu_button(ui, "Export", |ui, focus_first| {
                let obj = ui.button("Wavefront OBJ (.obj)...");
                if focus_first {
                    obj.request_focus();
                }
                if obj.clicked() {
                    wanted = Some("obj");
                    ui.close();
                }
                if ui
                    .button("glTF binary (.glb), textured...")
                    .on_hover_text("Textured from the VRAM image the view samples")
                    .clicked()
                {
                    wanted = Some("glb");
                    ui.close();
                }
            })
            .response
            .on_hover_text("Save this mesh as OBJ or textured glTF (Ctrl+E: glTF)");
            match wanted {
                Some("obj") => {
                    let name = format!("{}_tmd{:02}.obj", lib.stem(a.index), self.tmd_sel);
                    if let Some(out) = self.export_target(&name, "obj") {
                        let res = export::tmd_obj(a, &t, &out);
                        self.report("OBJ", res, &out);
                    }
                }
                Some(_) => {
                    let name = format!("{}_tmd{:02}.glb", lib.stem(a.index), self.tmd_sel);
                    let scene = match self.model.as_ref().map(|m| m.source) {
                        Some(TexSource::Scene) => match self.scene_vram(lib, a) {
                            Ok((_, v)) => Ok(Some(v)),
                            Err(why) => Err(why),
                        },
                        _ => Ok(None),
                    };
                    match scene {
                        // The view samples the scene's VRAM, and an export
                        // without it would quietly lose the textures.
                        Err(why) => {
                            self.explorer.error = Some(format!(
                                "Not exported: the scene's textures are not available ({why})."
                            ));
                        }
                        Ok(scene) => {
                            if let (Some(out), Some(a)) = (
                                self.export_target(&name, "glb"),
                                self.explorer.analysis.clone(),
                            ) {
                                let t = t.clone();
                                self.start_export(
                                    name,
                                    out.parent().map(Path::to_path_buf),
                                    move || {
                                        export::tmd_glb(
                                            &a,
                                            &t,
                                            scene.as_ref().map(|v| &v.vram),
                                            &out,
                                        )?;
                                        Ok(format!("glTF written to {}", out.display()))
                                    },
                                );
                            }
                        }
                    }
                }
                None => {}
            }
        });
        let mut status = None;
        let scene = if self
            .model
            .as_ref()
            .is_some_and(|m| m.source == TexSource::Scene)
        {
            match self.scene_vram(lib, a) {
                Ok(v) => Some(v),
                Err(e) => {
                    ui.ctx()
                        .request_repaint_after(std::time::Duration::from_millis(100));
                    status = Some(e);
                    None
                }
            }
        } else {
            None
        };
        let Some(mv) = &mut self.model else { return };
        ui.horizontal_wrapped(|ui| {
            ui.label("Textures:");
            ui.selectable_value(
                &mut mv.source,
                TexSource::Entry,
                format!("this entry ({})", count_noun(a.tims.len(), "texture")),
            )
            .on_hover_text("Only the TIMs stored inside the selected entry");
            ui.selectable_value(
                &mut mv.source,
                TexSource::Scene,
                format!("scene {}", lib.retail_block(a.index).unwrap_or("-")),
            )
            .on_hover_text(
                "The VRAM the engine builds for this entry's CDNAME scene: every TIM of the block, \
                 the shared init_data / player_data blocks, the system-UI bundle, and the \
                 field-character atlas and effect-texture pool the play host adds",
            );
            match (&scene, &status) {
                (Some((_, v)), _) => {
                    // One line, ending in an ellipsis on a narrow pane (the
                    // full text and the host layers show on hover).
                    ui.add(
                        egui::Label::new(
                            RichText::new(format!(
                                "{} as the game loads them",
                                count_noun(v.tims, "texture")
                            ))
                            .weak(),
                        )
                        .truncate(),
                    )
                    .on_hover_text(format!(
                        "{:?} load: {} TIMs ({} shared) + {} host layers\n{}",
                        v.kind,
                        v.tims,
                        v.shared_tims,
                        v.layers.len(),
                        v.layers.join("\n")
                    ));
                }
                (None, Some(e)) if mv.source == TexSource::Scene => {
                    ui.spinner();
                    ui.label(RichText::new(e).weak());
                }
                _ => {}
            }
        });
        let Some(model) = &mv.model else {
            ui.colored_label(
                ui.visuals().error_fg_color,
                "This TMD has no drawable triangles.",
            );
            return;
        };
        let (vram, with) = match (&scene, mv.source) {
            (Some((k, v)), TexSource::Scene) => (&v.vram, Some(k.clone())),
            _ => (&mv.entry_vram, None),
        };
        if mv.drawn_with != with {
            mv.drawn_with = with;
            mv.view.version += 1;
        }
        let dark = model.dark_baked_share();
        if mv.view.shading == Shading::Textured && dark >= legaia_tmd::mesh::MOSTLY_DARK_SHARE {
            // One line, the explanation on hover: as a paragraph it took
            // five rows from the view at the minimum window size.
            ui.horizontal(|ui| {
                // The button first: the truncated line takes what is left.
                if ui
                    .small_button("Show the shape")
                    .on_hover_text("Switch to Untextured + viewer light")
                    .clicked()
                {
                    mv.view.shading = Shading::Lit;
                }
                ui.add(
                    egui::Label::new(
                        RichText::new(format!(
                            "{:.0}% of this mesh is baked near-black on the disc.",
                            dark * 100.0
                        ))
                        .color(crate::theme::warn(ui)),
                    )
                    .truncate(),
                )
                .on_hover_text(
                    "The mesh carries near-black colour words on the disc. Retail scales every \
                     texel by that word, so it draws black here (in the game the scene's depth \
                     cue then blends it toward the far colour): the darkness is the data, not a \
                     missing texture.",
                );
            });
        }
        let caption = format!("{} triangles", model.triangles());
        mv.view.show(ui, model, vram, &caption);
    }

    /// The audio transport both sound views open with: Stop, how far the
    /// playing sound is (a bar; a slider that seeks for a streamed channel),
    /// and what it is. One row at any width: Stop and the time come first
    /// and the name is cut to what is left.
    pub(crate) fn transport_row(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let playing = self.player.now_playing.is_some();
            if ui.add_enabled(playing, egui::Button::new("Stop")).clicked() {
                self.player.stop();
                self.seek_drag = None;
            }
            let (Some(name), Some(progress)) =
                (self.player.now_playing.clone(), self.player.progress())
            else {
                self.seek_drag = None;
                ui.label("Stopped");
                return;
            };
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(100));
            match progress.total {
                Some(total) if self.player.can_seek() => {
                    let mut at = self.seek_drag.unwrap_or(progress.secs).min(total);
                    ui.spacing_mut().slider_width = TRANSPORT_BAR_W;
                    let r = ui
                        .add(egui::Slider::new(&mut at, 0.0..=total).show_value(false))
                        .on_hover_text("Drag to move through the sound");
                    if r.changed() {
                        self.seek_drag = Some(at);
                    } else if self.seek_drag.is_none()
                        && ui.input(|i| i.pointer.primary_released())
                        && let Some(p) = ui.input(|i| i.pointer.interact_pos())
                        && r.rect.contains(p)
                    {
                        // A press and release inside one frame moves no
                        // slider; the click's place on the rail still counts.
                        let along = (p.x - r.rect.left()) / r.rect.width().max(1.0);
                        self.seek_drag = Some(f64::from(along.clamp(0.0, 1.0)) * total);
                    }
                    if !r.is_pointer_button_down_on()
                        && let Some(to) = self.seek_drag.take()
                    {
                        self.player.seek_stream(to);
                    }
                    let shown = crate::player::Progress {
                        secs: at,
                        total: Some(total),
                    };
                    ui.monospace(shown.text());
                }
                Some(_) => {
                    ui.add(
                        egui::ProgressBar::new(progress.fraction().unwrap_or(0.0))
                            .desired_width(TRANSPORT_BAR_W)
                            .desired_height(ui.spacing().interact_size.y * 0.5),
                    );
                    ui.monospace(progress.text());
                }
                None => {
                    ui.monospace(progress.text());
                    if let Some((tick, bpm, _)) = self.player.seq_progress() {
                        ui.label(format!("tick {tick}, {bpm:.1} BPM"));
                    }
                }
            }
            ui.add(egui::Label::new(name.as_str()).truncate())
                .on_hover_text(name);
        });
    }

    fn audio(&mut self, ui: &mut egui::Ui, lib: &crate::Library, a: &Analysis) {
        if a.vabs.is_empty() && a.seqs.is_empty() {
            ui.label("No VAB sound banks or SEQ sequences in this entry.");
            return;
        }
        self.transport_row(ui);
        ui.separator();
        if !a.vabs.is_empty() {
            self.vab_sel = self.vab_sel.min(a.vabs.len() - 1);
            ui.horizontal(|ui| {
                ui.label(RichText::new("Sound bank").strong());
                egui::ComboBox::from_id_salt("vab")
                    .selected_text(format!("VAB @ 0x{:X}", a.vabs[self.vab_sel].offset))
                    .show_ui(ui, |ui| {
                        for (i, v) in a.vabs.iter().enumerate() {
                            ui.selectable_value(
                                &mut self.vab_sel,
                                i,
                                format!(
                                    "VAB @ 0x{:X}: {} programs, {} samples",
                                    v.offset,
                                    v.report.programs.len(),
                                    v.report.vag_samples.len()
                                ),
                            );
                        }
                    });
                let key = self.take_export_key();
                if ui
                    .button("Export all samples (WAV)...")
                    .on_hover_text(EXPORT_KEY_HINT)
                    .clicked()
                    || key
                {
                    let stem =
                        format!("{}_vab{:X}", lib.stem(a.index), a.vabs[self.vab_sel].offset);
                    if let Some(first) = self.export_target(&format!("{stem}_000.wav"), "wav") {
                        let dir = first.parent().map(Path::to_path_buf).unwrap_or_default();
                        let vab_sel = self.vab_sel;
                        if let Some(a) = self.explorer.analysis.clone() {
                            let total = a.vabs[vab_sel].report.vag_samples.len();
                            self.start_export(
                                format!("{total} WAVs"),
                                Some(dir.clone()),
                                move || {
                                    let failed =
                                        export::vab_all_wavs(&a, &a.vabs[vab_sel], &dir, &stem)?;
                                    batch_outcome("WAVs", total, &failed, &dir)
                                },
                            );
                        }
                    }
                }
            });
        }
        if !a.seqs.is_empty() {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Sequences").strong());
                ui.checkbox(&mut self.seq_loop, "loop");
            });
            for s in a.seqs.clone() {
                // Wrapped: the label and the two buttons outrun the pane at
                // the minimum window width.
                ui.horizontal_wrapped(|ui| {
                    ui.monospace(format!(
                        "SEQ @ 0x{:X}  {:.1} BPM, {} PPQN",
                        s.offset, s.bpm, s.ppqn
                    ));
                    let vab = a.vabs.get(self.vab_sel);
                    if ui
                        .add_enabled(vab.is_some(), egui::Button::new("Play with selected bank"))
                        .clicked()
                        && let Some(v) = vab
                        && let Err(e) = self.player.play_seq(a, v, &s, self.seq_loop)
                    {
                        self.explorer.error = Some(format!("{e:#}"));
                    }
                    if ui.button("Save .seq...").clicked() {
                        let name = format!("{}_{:X}.seq", lib.stem(a.index), s.offset);
                        if let Some(out) = self.export_target(&name, "seq") {
                            let res = export::bytes(&a.bytes[s.offset..], &out);
                            self.report("SEQ", res, &out);
                        }
                    }
                });
            }
            ui.separator();
        }
        let Some(v) = a.vabs.get(self.vab_sel).cloned() else {
            return;
        };
        egui::ScrollArea::vertical().show_rows(
            ui,
            22.0,
            v.report.vag_samples.len(),
            |ui, range| {
                for i in range {
                    let span = &v.report.vag_samples[i];
                    ui.horizontal(|ui| {
                        // Length, not bytes: 28 PCM samples per 16-byte ADPCM block.
                        let secs = (span.size / 16 * 28) as f32
                            / legaia_engine_audio::vab_bind::VAB_SAMPLE_RATE as f32;
                        ui.monospace(format!("sample {i:3}  {secs:>5.2} s"));
                        let wave = self
                            .waves
                            .entry((a.index as usize, v.offset, i))
                            .or_insert_with(|| {
                                export::vag_pcm(a, &v, i)
                                    .map(|pcm| export::wave_peaks(&pcm))
                                    .unwrap_or_default()
                            });
                        // The two buttons keep their room at the minimum
                        // window width: the waveform takes what is left.
                        let reserve = small_button_width(ui, "Play")
                            + small_button_width(ui, "WAV...")
                            + 2.0 * ui.spacing().item_spacing.x;
                        draw_wave(ui, wave, wave_width(ui.available_width(), reserve));
                        if ui.small_button("Play").clicked() {
                            match export::vag_pcm(a, &v, i) {
                                Ok(pcm) => {
                                    if let Err(e) = self.player.play_pcm(
                                        pcm,
                                        legaia_engine_audio::vab_bind::VAB_SAMPLE_RATE,
                                        format!("sample {i}"),
                                    ) {
                                        self.explorer.error = Some(format!("{e:#}"));
                                    }
                                }
                                Err(e) => self.explorer.error = Some(format!("{e:#}")),
                            }
                        }
                        if ui.small_button("WAV...").clicked() {
                            let name =
                                format!("{}_vab{:X}_{i:03}.wav", lib.stem(a.index), v.offset);
                            if let Some(out) = self.export_target(&name, "wav") {
                                let res = export::vag_wav(a, &v, i, &out);
                                self.report("WAV", res, &out);
                            }
                        }
                    });
                }
            },
        );
    }

    fn text(&mut self, ui: &mut egui::Ui, lib: &crate::Library, a: &Analysis) {
        if a.mes.is_empty() && a.dialog.is_empty() {
            ui.label("No dialog text or MES blobs in this entry.");
            return;
        }
        // Ctrl+E saves the first text block the tab shows.
        let mut key = self.take_export_key();
        // The find field sits above the scrolling text, so it stays in
        // reach however far the lines are scrolled.
        let total_lines = a.dialog.len() + a.mes.iter().map(|m| m.messages.len()).sum::<usize>();
        ui.horizontal(|ui| {
            ui.label("Find:");
            let clear_w = small_button_width(ui, "x") + ui.spacing().item_spacing.x;
            let field_w = (ui.available_width() - clear_w - 150.0).clamp(80.0, 260.0);
            ui.add(
                egui::TextEdit::singleline(&mut self.text_filter)
                    .hint_text("words in a line")
                    .desired_width(field_w),
            )
            .on_hover_text(
                "Shows only the lines holding every word typed here, in any order and \
                 either case. Save text still writes every line.",
            );
            if !self.text_filter.is_empty() {
                if ui.small_button("x").on_hover_text("Clear").clicked() {
                    self.text_filter.clear();
                }
                let terms = find_terms(&self.text_filter);
                let shown = a
                    .dialog
                    .iter()
                    .filter(|l| line_matches(&l.text, &terms))
                    .count()
                    + a.mes
                        .iter()
                        .flat_map(|m| m.messages.iter())
                        .filter(|s| line_matches(s, &terms))
                        .count();
                ui.label(format!("{shown} of {total_lines}"));
            }
        });
        let terms = find_terms(&self.text_filter);
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if !a.dialog.is_empty() {
                    // The button leads: after the heading and the markup
                    // note it sat past the pane's edge at the minimum width.
                    ui.horizontal(|ui| {
                        if ui
                            .small_button("Save text...")
                            .on_hover_text(EXPORT_KEY_HINT)
                            .clicked()
                            || std::mem::take(&mut key)
                        {
                            let name = format!("{}_dialog.txt", lib.stem(a.index));
                            if let Some(out) = self.export_target(&name, "txt") {
                                let body: String = a
                                    .dialog
                                    .iter()
                                    .map(|l| {
                                        let space = if l.from_man { "man" } else { "raw" };
                                        format!("{space}:0x{:X}\t{}\n", l.offset, l.text)
                                    })
                                    .collect();
                                let res = export::bytes(body.as_bytes(), &out);
                                self.report("text", res, &out);
                            }
                        }
                        ui.label(RichText::new(count_noun(a.dialog.len(), "dialog line")).strong());
                        const MARKUP: &str =
                            "language-pack markup: | = line break, {c1:00} = a name";
                        ui.add(egui::Label::new(RichText::new(MARKUP).weak()).truncate())
                            .on_hover_text(MARKUP);
                    });
                    let mut any = false;
                    egui::Grid::new("dialog")
                        .striped(true)
                        .num_columns(2)
                        .show(ui, |ui| {
                            for l in a.dialog.iter().filter(|l| line_matches(&l.text, &terms)) {
                                any = true;
                                let space = if l.from_man { "man" } else { "raw" };
                                ui.monospace(format!("{space}:0x{:05X}", l.offset));
                                ui.label(l.text.replace('|', "\n"));
                                ui.end_row();
                            }
                        });
                    if !any {
                        ui.label(RichText::new("No dialog line holds those words.").weak());
                    }
                    ui.separator();
                }
                for (k, m) in a.mes.iter().enumerate() {
                    let place = match m.chunk {
                        Some(c) => format!("chunk {c}"),
                        None => "whole entry".into(),
                    };
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(format!(
                                "MES {k}: {} layout, {place}, 0x{:X}",
                                m.format, m.offset
                            ))
                            .strong(),
                        );
                        if ui.small_button("Save text...").clicked() || std::mem::take(&mut key) {
                            let name = format!("{}_mes{k}.txt", lib.stem(a.index));
                            if let Some(out) = self.export_target(&name, "txt") {
                                let body: String = m
                                    .messages
                                    .iter()
                                    .enumerate()
                                    .map(|(i, s)| format!("[{i}]\n{s}\n\n"))
                                    .collect();
                                let res = export::bytes(body.as_bytes(), &out);
                                self.report("text", res, &out);
                            }
                        }
                    });
                    if m.messages.is_empty() {
                        ui.label(if m.records > 0 {
                            format!(
                                "{} records, none carrying a 0x1F-led text segment.",
                                m.records
                            )
                        } else {
                            "No messages decode from this blob.".to_string()
                        });
                    }
                    for (i, s) in m
                        .messages
                        .iter()
                        .enumerate()
                        .filter(|(_, s)| line_matches(s, &terms))
                    {
                        ui.horizontal_wrapped(|ui| {
                            ui.monospace(format!("{i:3}"));
                            ui.label(RichText::new(s).monospace());
                        });
                    }
                    ui.separator();
                }
            });
    }

    fn extract_panel(&mut self, ui: &mut egui::Ui) {
        ui.heading("Extract the whole disc");
        ui.label(
            "Unpacks the disc into a folder you choose: its files, every archive entry, the \
             textures as PNG, the voice and streamed music as WAV, the models in their raw form \
             (.tmd) and the dialog font. Nothing is uploaded anywhere. To get one model as OBJ or \
             glTF, use Export in Browse or Characters & monsters.",
        );
        ui.add_space(6.0);
        let running = self.job.as_ref().is_some_and(|j| j.outcome().is_none());
        let disc = match self.explorer.library.as_ref().map(|l| &l.source) {
            Some(Source::Disc(p)) => Some(p.clone()),
            _ => None,
        };
        // A first extraction proposes a folder beside the disc image.
        if self.out_text.trim().is_empty()
            && let Some(dir) = disc.as_deref().and_then(crate::launch::suggested_out_dir)
        {
            self.out_text = dir.display().to_string();
        }
        // The rows' first column and the buttons beside the folder field; the
        // field and the disc line take what is left of the window.
        let value_w = ui.available_width() - 100.0;
        egui::Grid::new("extract_opts")
            .num_columns(2)
            .show(ui, |ui| {
                ui.label("Disc image");
                match &disc {
                    Some(p) => {
                        let text = p.display().to_string();
                        ui.allocate_ui_with_layout(
                            egui::vec2(value_w, ui.spacing().interact_size.y),
                            egui::Layout::left_to_right(egui::Align::Center),
                            |ui| {
                                ui.add(
                                    egui::Label::new(RichText::new(&text).monospace()).truncate(),
                                )
                                .on_hover_text(&text)
                            },
                        )
                        .inner
                    }
                    None => {
                        let r = ui.add(crate::theme::primary_button("Open disc image..."));
                        if r.clicked() {
                            self.pick_disc();
                        }
                        r
                    }
                };
                ui.end_row();
                ui.label("Output folder");
                ui.horizontal(|ui| {
                    ui.add_enabled(
                        !running,
                        egui::TextEdit::singleline(&mut self.out_text)
                            .desired_width((value_w - 145.0).clamp(120.0, 900.0)),
                    );
                    if ui
                        .add_enabled(!running, egui::Button::new("Choose..."))
                        .clicked()
                    {
                        self.pick_out_folder();
                    }
                    let out = PathBuf::from(self.out_text.trim());
                    if !self.out_text.trim().is_empty()
                        && out.is_dir()
                        && ui
                            .button("Open")
                            .on_hover_text("Open this folder in the file manager")
                            .clicked()
                    {
                        self.show_folder(&out);
                    }
                });
                ui.end_row();
                // What the run will do to the chosen folder, before it starts.
                if !running && !self.out_text.trim().is_empty() {
                    ui.label("");
                    let out = PathBuf::from(self.out_text.trim());
                    match ExtractOutDir::of(&out) {
                        ExtractOutDir::New => ui.label(RichText::new("A new folder.").weak()),
                        ExtractOutDir::Empty => ui.label(RichText::new("An empty folder.").weak()),
                        // The run just finished is why it has files; that is
                        // not a warning.
                        ExtractOutDir::HasFiles
                            if self
                                .job
                                .as_ref()
                                .is_some_and(|j| j.outcome().is_some() && j.options.out == out) =>
                        {
                            ui.label(
                                RichText::new(
                                    "Another run here replaces the files with the same names.",
                                )
                                .weak(),
                            )
                        }
                        ExtractOutDir::HasFiles => ui.label(
                            RichText::new(
                                "This folder already has files: ones with the same names are \
                                 replaced, the rest are left alone.",
                            )
                            .color(ui.visuals().warn_fg_color),
                        ),
                        ExtractOutDir::NotAFolder => ui.label(
                            RichText::new("This is a file, not a folder.")
                                .color(ui.visuals().error_fg_color),
                        ),
                    };
                    ui.end_row();
                }
            });
        let options = egui::CollapsingHeader::new("Options")
            .open(Some(self.extract_options_open))
            .show(ui, |ui| {
                let s = &mut self.explorer.settings;
                ui.add_enabled_ui(!running, |ui| {
                    let mut every = !s.skip_all_textures;
                    ui.checkbox(&mut every, "Every texture as PNG, in a tim_scan folder")
                        .on_hover_text(
                            "Finds every texture on the disc, compressed ones too. Left out, \
                             only a few images are written. Command line: --all-textures",
                        );
                    s.skip_all_textures = !every;
                    for (flag, label, hover) in [
                        (
                            &mut s.skip_verify,
                            "Skip the check of the disc image",
                            "Compares the image's SHA-256 with the known dumps. --skip-verify",
                        ),
                        (
                            &mut s.skip_png,
                            "Skip the PNGs of the streamed textures",
                            "--skip-png",
                        ),
                        (
                            &mut s.skip_xa,
                            "Skip the voice and streamed music (WAV)",
                            "--skip-xa",
                        ),
                        (
                            &mut s.skip_catalog,
                            "Skip the texture lists (.tsv)",
                            "--skip-catalog",
                        ),
                        (&mut s.skip_font, "Skip the dialog font", "--skip-font"),
                    ] {
                        ui.checkbox(flag, label).on_hover_text(hover);
                    }
                });
            });
        if options.header_response.clicked() {
            self.extract_options_open = !self.extract_options_open;
        }
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            let can_start = !running && disc.is_some();
            let start = if can_start {
                crate::theme::primary_button("Start extraction")
            } else {
                egui::Button::new("Start extraction")
            };
            if ui
                .add_enabled(can_start, start)
                .on_disabled_hover_text(if running {
                    "An extraction is running."
                } else {
                    "Open a disc image first (an extracted folder is already extracted)."
                })
                .clicked()
            {
                self.start_extract();
            }
            if let Some(job) = &self.job
                && running
            {
                if ui.button("Cancel").clicked() {
                    job.cancel();
                }
                if job.is_cancelling() {
                    ui.label("cancelling...");
                }
            }
        });
        let Some(job) = &self.job else { return };
        let outcome = job.outcome();
        let p = job.progress();
        let steps = crate::extract_job::step_count(&job.options);
        let elapsed = crate::extract_job::human_duration(job.elapsed());
        let out = job.options.out.clone();
        ui.add_space(4.0);
        let units = if p.total > 0 {
            format!(" - {} of {}", p.done, p.total)
        } else {
            String::new()
        };
        // The pipeline names its steps with an arrow ("disc → ISO9660
        // files"). The log's monospace font (Hack) has the glyph; the bar's
        // proportional font (Ubuntu Light) and egui's emoji fallbacks do
        // not, so it is spelled the way the skip options spell it.
        let step_name = p.step_name.replace('\u{2192}', "->");
        let text = match &outcome {
            // Numbered the way the log numbers them: verification is step 0,
            // the rest count from 1.
            None if p.step == 0 => format!("Checking the disc image: {step_name}{units}"),
            None => format!("Step {} of {}: {step_name}{units}", p.step, steps - 1),
            Some(Outcome::Done(s)) => format!(
                "Done: {} images (PNG), {} sounds (WAV), {} archive entries, {} disc files",
                s.pngs + s.all_textures.1,
                s.wavs,
                s.prot_entries,
                s.iso_files,
            ),
            Some(Outcome::Cancelled) => "Cancelled".into(),
            // The reason is on the banner and in the log; a long path in the
            // bar ran off its end.
            Some(Outcome::Failed(_)) => "Failed: the reason is on the line above".into(),
        };
        let frac = if matches!(outcome, Some(Outcome::Done(_) | Outcome::Failed(_))) {
            // A failed bar at its first percent is a red dot under the text.
            1.0
        } else {
            p.fraction(steps)
        };
        let failed = matches!(outcome, Some(Outcome::Failed(_)));
        let mut bar = egui::ProgressBar::new(frac).text(text);
        if running {
            bar = bar.animate(true);
        }
        if failed {
            bar = bar.fill(ui.visuals().error_fg_color.gamma_multiply(0.6));
        }
        ui.add(bar);
        let mut show = false;
        ui.horizontal_wrapped(|ui| {
            let timing = match &outcome {
                None => match p.step_remaining(std::time::Instant::now()) {
                    Some(left) => format!(
                        "{elapsed} so far; about {} left in this step",
                        crate::extract_job::human_duration(left)
                    ),
                    None => format!("{elapsed} so far"),
                },
                Some(_) => format!("took {elapsed}"),
            };
            ui.label(RichText::new(timing).weak());
            if p.warnings > 0 {
                ui.separator();
                ui.label(
                    RichText::new(format!("{} warning(s)", p.warnings))
                        .color(ui.visuals().warn_fg_color),
                );
                ui.checkbox(&mut self.log_warnings_only, "Show only warnings");
            }
            if out.is_dir() {
                ui.separator();
                show = ui
                    .button("Open output folder")
                    .on_hover_text(out.display().to_string())
                    .clicked();
            }
        });
        if show {
            self.show_folder(&out);
        }
        if running {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(100));
        }
        ui.separator();
        let lines: Vec<&String> = if self.log_warnings_only && p.warnings > 0 {
            p.warning_lines().collect()
        } else {
            p.log.iter().collect()
        };
        let row = ui.text_style_height(&egui::TextStyle::Monospace);
        egui::ScrollArea::both()
            .stick_to_bottom(true)
            .auto_shrink([false, false])
            .show_rows(ui, row, lines.len(), |ui, range| {
                // One row per line, so the virtualised rows stay aligned.
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                let warn = ui.visuals().warn_fg_color;
                let err = ui.visuals().error_fg_color;
                for line in &lines[range] {
                    let text = RichText::new(line.as_str()).monospace();
                    let text = if line.starts_with(crate::extract_job::WARNING_PREFIX) {
                        text.color(warn)
                    } else if line.starts_with("FAILED") {
                        text.color(err)
                    } else {
                        text
                    };
                    ui.label(text);
                }
            });
    }

    /// Announce a finished extraction once, on whatever view is open: the
    /// run goes on in the background while the user browses, and its end
    /// used to show only on the Extract view.
    fn announce_extraction(&mut self) {
        let Some(job) = &mut self.job else { return };
        let Some(outcome) = job.take_finished() else {
            return;
        };
        let out = job.options.out.clone();
        let took = crate::extract_job::human_duration(job.elapsed());
        match outcome {
            Outcome::Done(_) => {
                self.explorer.status = Some(format!(
                    "Extraction finished in {took}: written to {}",
                    out.display()
                ));
                self.last_written = Some(out);
            }
            Outcome::Cancelled => {
                self.explorer.status = Some(format!(
                    "Extraction cancelled; the files written so far are in {}",
                    out.display()
                ));
                self.last_written = out.is_dir().then_some(out);
            }
            Outcome::Failed(e) => {
                self.explorer.error = Some(format!("Extraction failed: {e}"));
            }
        }
    }
}

impl App {
    /// Read the controllers from `rx` ([`crate::gamepad::spawn`]).
    pub fn with_gamepad(
        mut self,
        rx: Option<std::sync::mpsc::Receiver<crate::gamepad::PadInput>>,
    ) -> Self {
        self.pad = rx;
        self
    }
}

impl eframe::App for App {
    /// A controller's presses join the frame's input as the keys a keyboard
    /// user would press (`crate::gamepad`).
    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        use crate::gamepad::PadInput;
        let Some(rx) = &self.pad else { return };
        // A key or a mouse button from the desktop hands the file dialogs
        // back; the controller's own presses join below.
        if raw_input.events.iter().any(|e| {
            matches!(
                e,
                egui::Event::Key { pressed: true, .. }
                    | egui::Event::PointerButton { pressed: true, .. }
            )
        }) {
            self.pad_active = false;
        }
        let view = Mode::ALL.iter().position(|m| *m == self.mode).unwrap_or(0);
        while let Ok(input) = rx.try_recv() {
            match input {
                PadInput::Key(k) => {
                    self.pad_active = true;
                    raw_input.events.extend(crate::gamepad::key_events(k, view));
                }
                PadInput::Scroll(dy) => raw_input.events.push(egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0.0, -dy),
                    phase: egui::TouchPhase::Move,
                    modifiers: egui::Modifiers::NONE,
                }),
                PadInput::Connected(name) => {
                    self.explorer.status = Some(crate::gamepad::connected_line(&name));
                }
                PadInput::Disconnected(name) => {
                    self.explorer.status = Some(format!("Controller disconnected: {name}"));
                }
            }
        }
    }

    /// Work that goes on whether or not the window is drawn (minimised,
    /// occluded): the background loads, the extraction and the started game
    /// are polled, and their ends announced, here.
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.sync_appearance(ctx);
        if self.explorer.poll() {
            ctx.request_repaint();
        }
        if self.explorer.loading().is_some() || self.explorer.is_analyzing() {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }
        if let Some(lib) = &self.explorer.library
            && self.explorer.classified() < lib.entries.len()
        {
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
        }
        self.restore_entry();
        self.announce_extraction();
        if self.job.as_ref().is_some_and(|j| j.outcome().is_none()) {
            // Keep polling while a run goes on behind another view.
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
        }
        self.poll_game(ctx);
        self.poll_export(ctx);
        // A sound that has played out stops reading as playing.
        self.player.poll();
    }

    /// Input and drawing. Everything that reads this frame's events (drops,
    /// shortcuts, the scripted screenshot) sits here: `logic` also runs
    /// while the window is hidden, on the last shown frame's events.
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let dropped: Vec<PathBuf> = ui.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .collect()
        });
        self.drop_overlay(ui);
        if let Some(p) = crate::pick_drop(&dropped) {
            self.open_path(p);
            if dropped.len() > 1 {
                self.explorer.status = Some(format!(
                    "Opening {}. Legaia Explorer opens one disc at a time; the other {} \
                     dropped item(s) were left alone.",
                    p.display(),
                    dropped.len() - 1
                ));
            }
        }
        self.run_automation(ui);
        // The file browser takes the keys while it is open.
        if self.file_browser.is_some() {
            self.show_file_browser(ui.ctx());
        } else {
            self.shortcuts(ui);
        }
        if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::F1)) {
            self.about_open = !self.about_open;
        }
        // Typing in a field keeps `?` and Esc for the field.
        if !ui.ctx().egui_wants_keyboard_input() {
            if ui.input_mut(|i| {
                i.consume_key(egui::Modifiers::SHIFT, egui::Key::Questionmark)
                    || i.consume_key(egui::Modifiers::NONE, egui::Key::Questionmark)
            }) {
                self.keys_open = !self.keys_open;
            }
            if (self.about_open || self.keys_open)
                && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
            {
                self.about_open = false;
                self.keys_open = false;
            }
        }

        if let Some(r) = ui.input(|i| i.viewport().inner_rect) {
            // egui points include the UI zoom; the viewport's inner size at
            // start does not, so store the unzoomed size or a zoomed window
            // would shrink on every start.
            let z = ui.zoom_factor();
            self.window_size = Some([r.width() * z, r.height() * z]);
        }
        // The outer corner in physical pixels (`placement`): points times
        // pixels per point, which already folds in the UI zoom. Wayland
        // reports no position, so nothing is kept there.
        if let Some(r) = ui.input(|i| i.viewport().outer_rect) {
            let ppp = ui.pixels_per_point();
            self.window_pos = Some([
                (r.min.x * ppp).round() as i32,
                (r.min.y * ppp).round() as i32,
            ]);
        }
        egui::Panel::top("top").show(ui, |ui| self.top_bar(ui));
        self.about_window(ui);
        self.keys_window(ui);
        match self.mode {
            // Nothing open yet: the start page takes the whole window, in
            // every view that needs a disc to show anything (a view restored
            // from the last session used to open on a bare "open a disc
            // first" line with no way to do it).
            Mode::Browse | Mode::Actors | Mode::Media if self.explorer.library.is_none() => {
                egui::CentralPanel::default().show(ui, |ui| self.start_page(ui));
            }
            Mode::Browse => {
                let side_max = side_panel_max(ui, 300.0);
                let side = self.side_width("browse", 430.0).min(side_max);
                let panel = egui::Panel::left("browser")
                    .resizable(true)
                    .default_size(side)
                    .min_size(300.0)
                    .max_size(side_max)
                    .show(ui, |ui| self.browser(ui));
                self.side_widths
                    .insert("browse", panel.response.rect.width());
                egui::CentralPanel::default().show(ui, |ui| self.preview(ui));
            }
            Mode::Actors => self.actors_mode(ui),
            Mode::Media => self.media_mode(ui),
            Mode::Extract => {
                egui::CentralPanel::default().show(ui, |ui| self.extract_panel(ui));
            }
        }
        if self.take_export_key() {
            self.explorer.status = Some(nothing_to_export(self.mode).into());
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.player.stop();
        let s = &mut self.explorer.settings;
        for (view, width) in &self.side_widths {
            s.side_widths.insert((*view).to_string(), *width);
        }
        if let Some(pick) = self.actors.pick {
            s.actor = Some(pick.key());
        }
        if let Some(sel) = self.media.selected() {
            s.media = Some(sel.to_string());
        }
        s.mode = Some(self.mode.key().to_string());
        s.tab = Some(self.tab.key().to_string());
        if let (Some(lib), Some(pos)) = (&self.explorer.library, self.explorer.selected) {
            s.entry = lib.entries.get(pos).map(|e| e.index);
        }
        if let Some(size) = self.window_size {
            s.window_size = Some(size);
        }
        if let Some(pos) = self.window_pos {
            s.window_pos = Some(pos);
        }
        self.explorer.save_settings();
        if let Some(job) = &mut self.job {
            job.cancel();
            job.join();
        }
    }
}

/// `THIRD-PARTY-LICENSES.txt` where the release archives put it
/// (`docs/tooling/releases.md`): beside the program, or beside the macOS
/// `.app` bundle the program sits three folders inside.
fn third_party_licenses() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    exe.ancestors()
        .skip(1)
        .take(4)
        .map(|d| d.join("THIRD-PARTY-LICENSES.txt"))
        .find(|p| p.is_file())
}

impl App {
    fn about_window(&mut self, ctx: &egui::Context) {
        let mut open = self.about_open;
        let max_h = about_max_height(ctx.content_rect().height());
        egui::Window::new("About Legaia Explorer")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .max_height(max_h)
            .show(ctx, |ui| {
                // Scrolls when the window is shorter than the box (the
                // minimum window at a large zoom pushed its title bar and
                // last rows off the screen).
                egui::ScrollArea::vertical()
                    .max_height(max_h - ABOUT_CHROME_H)
                    .min_scrolled_height(max_h - ABOUT_CHROME_H)
                    .show(ui, |ui| {
                    ui.heading(format!("Legaia Explorer {}", env!("CARGO_PKG_VERSION")));
                    ui.label(
                        "Browse, preview and extract the assets on your own Legend of Legaia disc.",
                    );
                    ui.add_space(6.0);
                    ui.label(format!("License: {}", env!("CARGO_PKG_LICENSE")));
                    ui.label(
                        "The third-party crates this program is built from keep their own \
                         licenses; their texts and notices ship in THIRD-PARTY-LICENSES.txt \
                         next to the program.",
                    );
                    match third_party_licenses() {
                        Some(p) => {
                            ui.horizontal(|ui| {
                                ui.monospace(p.display().to_string());
                                if ui.small_button("Copy path").clicked() {
                                    ui.ctx().copy_text(p.display().to_string());
                                }
                            });
                        }
                        None => {
                            ui.label(
                                RichText::new(
                                    "(not found beside this binary - it is part of every release \
                                     archive; a source build can generate it with \
                                     scripts/ci/third-party-licenses.py)",
                                )
                                .weak(),
                            );
                        }
                    }
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new(
                            "No game data ships with this program; it reads only the disc image you \
                             choose, and writes only into folders you pick.",
                        )
                        .weak(),
                    );
                    ui.add_space(6.0);
                    if ui.button("Keyboard shortcuts (?)").clicked() {
                        self.keys_open = true;
                    }
                    });
            });
        self.about_open = open;
    }

    /// Every keyboard shortcut in one box (`?`, or the View menu).
    fn keys_window(&mut self, ctx: &egui::Context) {
        let mut open = self.keys_open;
        let max_h = about_max_height(ctx.content_rect().height());
        egui::Window::new("Keyboard shortcuts")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .max_height(max_h)
            .show(ctx, |ui| {
                // The controller rows make the box taller than the smallest
                // window: it scrolls there rather than run off the bottom.
                // Without a floor the scroll area took the half of the
                // window below a centred box's pivot, and the controller
                // rows sat behind a scroll bar in a window with room for
                // every row.
                let room = max_h - ABOUT_CHROME_H;
                egui::ScrollArea::vertical()
                    .max_height(room)
                    .min_scrolled_height(room)
                    .show(ui, |ui| {
                        egui::Grid::new("keys")
                            .num_columns(2)
                            .spacing([16.0, 3.0])
                            .show(ui, |ui| {
                                // Key names in the palette's key ink, as the
                                // launcher's Controls panel and the play window's
                                // F12 panel draw them.
                                let ink = crate::theme::c(crate::theme::palette(ui).info);
                                for (keys, what) in KEYS {
                                    ui.label(RichText::new(*keys).monospace().color(ink));
                                    ui.label(*what);
                                    ui.end_row();
                                }
                                // A controller works the same views; its buttons
                                // are named by position on a PlayStation pad.
                                ui.label(RichText::new("Controller").strong());
                                ui.end_row();
                                for (keys, what) in crate::gamepad::PAD_ROWS {
                                    ui.label(RichText::new(*keys).monospace().color(ink));
                                    ui.label(*what);
                                    ui.end_row();
                                }
                            });
                    });
            });
        self.keys_open = open;
    }
}

/// `ui.menu_button` that a keyboard or a controller can work: a menu opened
/// with `Enter` (Cross) on its focused button takes the focus onto its
/// first item, so the arrows (the D-pad) walk its items and `Enter`
/// picks one. egui leaves the focus on the button, where `Down` jumped to
/// whatever control lay nearest below the menu (the path field under the
/// Open menu), and a controller could not reach the theme, the zoom, the
/// export folder or the recent list at all.
fn pad_menu_button<'a, R>(
    ui: &mut egui::Ui,
    title: impl egui::IntoAtoms<'a>,
    add_contents: impl FnOnce(&mut egui::Ui, bool) -> R,
) -> egui::InnerResponse<Option<R>> {
    // The menu learns it was opened from the keyboard one frame late (the
    // frame it first shows is the one its button reports focus in), so
    // its first item takes the focus on the frame after.
    let key = ui.id().with(("pad_menu", ui.next_auto_id()));
    let (was_open, focus_first) = ui.ctx().data_mut(|d| {
        (
            d.get_temp::<bool>(key).unwrap_or(false),
            d.remove_temp::<bool>(key.with("first")).unwrap_or(false),
        )
    });
    let r = ui.menu_button(title, |ui| add_contents(ui, focus_first));
    let open = r.inner.is_some();
    ui.ctx().data_mut(|d| d.insert_temp(key, open));
    if open && !was_open && r.response.has_focus() {
        ui.ctx().data_mut(|d| {
            d.insert_temp(key.with("first"), true);
            d.insert_temp(key.with("keys"), true);
        });
        ui.ctx().request_repaint();
    }
    // A menu the keys opened gives the focus back to its button when it
    // closes (Esc / Circle drops egui's focus altogether, and the next Tab
    // or D-pad press then started over from the top of the window).
    if !open && was_open {
        let keys = ui
            .ctx()
            .data_mut(|d| d.remove_temp::<bool>(key.with("keys")).unwrap_or(false));
        if keys {
            r.response.request_focus();
        }
    }
    r
}

/// The shortcuts box's rows: keys, then what they do.
const KEYS: &[(&str, &str)] = &[
    ("Ctrl+O", "open a disc image"),
    ("Ctrl+L", "type a path to open"),
    ("Ctrl+1 .. Ctrl+4", "Browse, Characters, Movies, Extract"),
    ("F5", "play the open disc"),
    ("Shift+F5", "the game's options, keys and saves"),
    (
        "Ctrl+E",
        "save what the view shows (texture, model, entry, frame)",
    ),
    ("Ctrl+F, Esc", "search the archive, clear the search"),
    ("Ctrl+Tab, Ctrl+PgDn", "next preview tab"),
    ("Ctrl+Shift+Tab, Ctrl+PgUp", "previous preview tab"),
    (
        "Up / Down, PgUp / PgDn, Home / End",
        "walk the archive, character and movie lists",
    ),
    (
        "Space, Left / Right",
        "play or pause, step a frame (movies, animations)",
    ),
    (
        "Tab to a 3D view, then arrows",
        "turn the model (PgUp / PgDn zoom, Home resets)",
    ),
    (
        "Ctrl+= / Ctrl+- / Ctrl+0",
        "zoom in, out, reset (also View)",
    ),
    ("F1", "the About box"),
    ("?", "this box"),
    ("Esc", "close this box or the About box"),
];

/// The status line for a batch of `total` files, of which `failed` names the
/// ones that could not be written: a partial batch is a failure to read, not
/// a smaller success.
fn batch_outcome(
    what: &str,
    total: usize,
    failed: &[String],
    dir: &Path,
) -> anyhow::Result<String> {
    if failed.is_empty() {
        return Ok(format!("{total} {what} written to {}", dir.display()));
    }
    let written = total - failed.len();
    let first = &failed[0];
    anyhow::bail!(
        "{written} of {total} {what} written to {}; {} failed, first {first}",
        dir.display(),
        failed.len()
    )
}

/// Points the About box's title bar and frame take beyond its body.
const ABOUT_CHROME_H: f32 = 40.0;

/// The About box's tallest size in a window `screen_h` points tall: the
/// window less a margin, with room for at least a few lines.
fn about_max_height(screen_h: f32) -> f32 {
    (screen_h - 40.0).max(ABOUT_CHROME_H + 80.0)
}

fn theme_preference(t: crate::settings::Theme) -> egui::ThemePreference {
    match t {
        // egui's own System reads winit, which has nothing on X11; the
        // desktop's preference is what the launcher follows too.
        crate::settings::Theme::System => match legaia_app_theme::desktop_prefers_dark() {
            Some(true) => egui::ThemePreference::Dark,
            Some(false) => egui::ThemePreference::Light,
            None => egui::ThemePreference::System,
        },
        crate::settings::Theme::Light => egui::ThemePreference::Light,
        crate::settings::Theme::Dark => egui::ThemePreference::Dark,
    }
}

impl App {
    /// The start page: what the app is for, the two ways in as large
    /// buttons, the recent list, and what each view does.
    fn start_page(&mut self, ui: &mut egui::Ui) {
        const COLUMN: f32 = 700.0;
        let accent = crate::theme::accent(ui);
        let (top_gap, gap) = start_gaps(ui.available_height());
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                // The column narrows with the window, keeping a margin on
                // both sides; a fixed 700-point column ran its wrapped text
                // and the footer past the right edge at the minimum width.
                let (side, column) = start_column(ui.available_width(), COLUMN);
                ui.horizontal(|ui| {
                    ui.add_space(side);
                    ui.vertical(|ui| {
                        ui.set_max_width(column);
                        ui.add_space(top_gap);
                        ui.label(RichText::new("Legaia Explorer").size(30.0).color(accent));
                        ui.label(
                            RichText::new(
                                "Browse, preview and extract everything on your own \
                                 Legend of Legaia disc - and start the game on it.",
                            )
                            .size(15.0),
                        );
                        ui.add_space(gap - 2.0);
                        // The button a controller's first press lands on.
                        let mut first_focus: Option<egui::Response> = None;
                        ui.horizontal(|ui| {
                            let big = egui::vec2(230.0, 38.0);
                            let open = ui
                                .add(
                                    crate::theme::primary_button("Open disc image...")
                                        .min_size(big),
                                )
                                .on_hover_text(
                                    "Your .bin (a raw dump, 2352-byte sectors) or its .cue. Ctrl+O",
                                );
                            if open.clicked() {
                                self.pick_disc();
                            }
                            first_focus = Some(open);
                            if ui
                                .add(egui::Button::new("Open extracted folder...").min_size(big))
                                .on_hover_text("A folder legaia-extract (or Extract) wrote")
                                .clicked()
                            {
                                self.pick_folder();
                            }
                        });
                        ui.horizontal_wrapped(|ui| {
                            ui.label(
                                RichText::new(
                                    "or drop the file (or folder) on this window, type its \
                                     path in the bar above, or",
                                )
                                .weak(),
                            );
                            if ui
                                .link("find it in this window")
                                .on_hover_text(
                                    "A folder list a controller can work, for when the \
                                     desktop's file dialog is no use",
                                )
                                .clicked()
                            {
                                self.open_file_browser(crate::file_browser::Purpose::Disc);
                            }
                        });
                        if let Some(p) = self.explorer.loading() {
                            ui.add_space(10.0);
                            ui.horizontal(|ui| {
                                ui.spinner();
                                ui.label(format!("Opening {} ...", p.display()));
                            });
                        }
                        if let Some(disc) = self.game_disc.clone() {
                            ui.add_space(gap - 4.0);
                            let name = disc.file_name().map_or_else(
                                || disc.display().to_string(),
                                |n| n.to_string_lossy().into_owned(),
                            );
                            let (lead, why) = if self.game_disc_found {
                                ("Found on this computer:", "a disc image named for the game")
                            } else {
                                (
                                    "The disc the game uses:",
                                    "the disc image you chose in the game",
                                )
                            };
                            ui.horizontal_wrapped(|ui| {
                                ui.label(lead);
                                let offer = ui
                                    .add(egui::Button::new(RichText::new(&name).strong()))
                                    .on_hover_text(format!("Open {} - {why}", disc.display()));
                                if offer.clicked() {
                                    self.game_disc = None;
                                    self.open_path(&disc);
                                }
                                first_focus = Some(offer);
                            });
                        }
                        // With a controller and nothing focused, the D-pad
                        // has nowhere to start from: put the focus on the
                        // disc on offer, else on Open disc image.
                        if self.pad_active
                            && self.file_browser.is_none()
                            && ui.memory(|m| m.focused().is_none())
                            && let Some(r) = &first_focus
                        {
                            r.request_focus();
                        }
                        if !self.explorer.settings.recent.is_empty() {
                            ui.add_space(gap);
                            ui.label(RichText::new("Open again").strong().size(15.0));
                            egui::Frame::new()
                                .fill(ui.visuals().faint_bg_color)
                                .corner_radius(6.0)
                                .inner_margin(10.0)
                                .show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    if let Some(p) = self.recent_list(ui) {
                                        self.open_path(&p);
                                    }
                                });
                        }
                        ui.add_space(gap);
                        ui.label(RichText::new("What you can do").strong().size(15.0));
                        egui::Grid::new("start_features")
                            .num_columns(2)
                            .spacing([18.0, 8.0])
                            .show(ui, |ui| {
                                for (title, what) in [
                                    (
                                        "Browse",
                                        "Every entry of the game's archive by scene, with \
                                         textures, 3D models, sound, music and dialog to \
                                         preview and export.",
                                    ),
                                    (
                                        "Characters & monsters",
                                        "The party with any equipment and every monster, \
                                         each battle animation and Tactical Art.",
                                    ),
                                    (
                                        "Movies & voice",
                                        "The disc's movies with their soundtrack, and the \
                                         streamed voice and music.",
                                    ),
                                    (
                                        "Extract",
                                        "Unpack the whole disc into a folder: textures as \
                                         PNG, voice and streamed music as WAV, raw models and \
                                         data.",
                                    ),
                                    (
                                        "Play",
                                        "Start the game itself on the open disc (the USA \
                                         disc). F5. Game options (Shift+F5) sets its look, \
                                         keys and saves.",
                                    ),
                                ] {
                                    ui.label(RichText::new(title).strong().color(accent));
                                    ui.add(egui::Label::new(what).wrap());
                                    ui.end_row();
                                }
                            });
                        ui.add_space(gap);
                        ui.label(
                            RichText::new(
                                "No game data ships with this program: it reads only the \
                                 disc image you choose, and writes only into folders you \
                                 pick. Press ? for the keyboard shortcuts.",
                            )
                            .weak(),
                        );
                    });
                });
            });
    }
}

/// The start page's gaps (above the title, between its sections) for a page
/// `height` points tall: the roomy ones, or tighter ones on a short window,
/// where the roomy gaps put the last line under the window's bottom edge.
fn start_gaps(height: f32) -> (f32, f32) {
    if height < START_ROOMY_MIN_H {
        (8.0, 10.0)
    } else {
        (28.0, 20.0)
    }
}

/// The page height from which the start page takes its roomy gaps.
const START_ROOMY_MIN_H: f32 = 520.0;

/// The preview tab after (or before) `from`, skipping the ones `enabled`
/// rules out and wrapping at the ends; `from` itself when no other is.
fn cycle_tab(from: Tab, forward: bool, enabled: impl Fn(Tab) -> bool) -> Tab {
    let n = Tab::ALL.len();
    let at = Tab::ALL.iter().position(|&t| t == from).unwrap_or(0);
    (1..n)
        .map(|k| {
            let i = if forward { at + k } else { at + n - k };
            Tab::ALL[i % n]
        })
        .find(|&t| enabled(t))
        .unwrap_or(from)
}

/// The preview pane's width below which the tab labels leave out their counts.
const TAB_COUNTS_MIN_WIDTH: f32 = 430.0;

/// The hover line on each view's main export button.
const EXPORT_KEY_HINT: &str = "Ctrl+E";

/// The texture preview's zoom range.
const TIM_ZOOM: (f32, f32) = (0.5, 8.0);

/// The zoom after a Ctrl + scroll of factor `by`: kept on a quarter-step
/// grid inside [`TIM_ZOOM`], and always at least one step from `zoom` in the
/// scroll's direction, so a slow wheel still moves it.
fn step_tim_zoom(zoom: f32, by: f32) -> f32 {
    const STEP: f32 = 0.25;
    if !by.is_finite() || by <= 0.0 || by == 1.0 {
        return zoom;
    }
    let snapped = (zoom * by / STEP).round() * STEP;
    let moved = if by > 1.0 {
        snapped.max(zoom + STEP)
    } else {
        snapped.min(zoom - STEP)
    };
    moved.clamp(TIM_ZOOM.0, TIM_ZOOM.1)
}

/// The largest quarter-step zoom at which a texture of `texels` shows whole
/// in a pane of `room` points, inside [`TIM_ZOOM`].
fn fit_tim_zoom(texels: egui::Vec2, room: egui::Vec2) -> f32 {
    if texels.x <= 0.0 || texels.y <= 0.0 {
        return 1.0;
    }
    let fit = (room.x / texels.x).min(room.y / texels.y);
    ((fit / 0.25).floor() * 0.25).clamp(TIM_ZOOM.0, TIM_ZOOM.1)
}

/// The words of a find field, lower-cased; none for a blank field.
fn find_terms(query: &str) -> Vec<String> {
    query.split_whitespace().map(str::to_lowercase).collect()
}

/// Whether `text` holds every one of `terms` ([`find_terms`]), in any order
/// and either case. A line break in the markup (`|`) reads as a space, so
/// words either side of one still match as typed. No terms match any line.
fn line_matches(text: &str, terms: &[String]) -> bool {
    if terms.is_empty() {
        return true;
    }
    let hay = text.replace('|', " ").to_lowercase();
    terms.iter().all(|t| hay.contains(t.as_str()))
}

/// The width of the transport's progress bar or seek slider.
const TRANSPORT_BAR_W: f32 = 120.0;

/// What the status line says when Ctrl+E finds nothing to save in `mode`.
fn nothing_to_export(mode: Mode) -> &'static str {
    match mode {
        Mode::Browse => {
            "Ctrl+E saves the entry the preview shows: pick an entry on the left first."
        }
        Mode::Actors => "Ctrl+E saves the shown character or monster: pick one on the left first.",
        Mode::Media => {
            "Ctrl+E saves the shown movie frame; for voice and music, use a channel's WAV button."
        }
        Mode::Extract => "Ctrl+E saves single assets; this view unpacks the whole disc instead.",
    }
}

/// A preview tab's label: its name with the count of what it shows, the
/// count left out when there is nothing (the tab is disabled then).
pub(crate) fn tab_label(name: &str, count: Option<usize>) -> String {
    match count {
        Some(n) if n > 0 => format!("{name} ({n})"),
        _ => name.to_string(),
    }
}

/// The texture list's smallest width, in points.
const TIM_LIST_MIN: f32 = 140.0;

/// The texture list's default and largest width in a preview pane `avail`
/// points wide: 270 points (one unwrapped row) at most, and never more
/// than half the pane, so the controls beside it keep their room.
/// The pane width from which the Textures tab's CLUT and Zoom controls fit
/// one row.
const TEX_CONTROLS_ROW_MIN: f32 = 520.0;

pub(crate) fn tim_list_width(avail: f32) -> (f32, f32) {
    let max = (avail * 0.5).max(TIM_LIST_MIN);
    (270.0f32.min(max), max)
}

/// The start page's left margin and column width for a pane `avail`
/// points wide: the column is centred at up to `max` points and never
/// leaves less than [`START_MARGIN`] on either side.
pub(crate) fn start_column(avail: f32, max: f32) -> (f32, f32) {
    let column = max.min(avail - 2.0 * START_MARGIN).max(0.0);
    (((avail - column) / 2.0).max(START_MARGIN), column)
}

/// The start page's smallest side margin, in points.
const START_MARGIN: f32 = 16.0;

/// The texture source a model view opens on: the entry's own TIMs when they
/// cover every textured prim of the mesh (`Model::texture_coverage`), else
/// the scene's VRAM, where the missing pages and CLUTs are uploaded.
fn default_tex_source(model: Option<&Model>, entry_vram: &psx_tim::Vram) -> TexSource {
    match model.map(|m| m.texture_coverage(entry_vram)) {
        Some((covered, textured)) if covered < textured => TexSource::Scene,
        _ => TexSource::Entry,
    }
}

fn human_size(n: u64) -> String {
    if n >= 1 << 20 {
        format!("{:.1} MB", n as f64 / (1 << 20) as f64)
    } else if n >= 1 << 10 {
        format!("{:.1} KB", n as f64 / 1024.0)
    } else {
        format!("{n} B")
    }
}

fn checkerboard(ui: &egui::Ui, rect: egui::Rect) {
    // The palette's own pair, so the board sits in either theme: a mid-grey
    // one was the one dark panel left in the light theme.
    let pal = crate::theme::palette(ui);
    let (under, over) = (crate::theme::c(pal.stripe), crate::theme::c(pal.rule));
    let p = ui.painter();
    p.rect_filled(rect, 0.0, under);
    let s = 8.0;
    let cols = (rect.width() / s).ceil() as i32;
    let rows = (rect.height() / s).ceil() as i32;
    if cols * rows > 40_000 {
        return;
    }
    for y in 0..rows {
        for x in 0..cols {
            if (x + y) % 2 == 0 {
                let min = rect.min + egui::vec2(x as f32 * s, y as f32 * s);
                let r = egui::Rect::from_min_size(min, egui::vec2(s, s)).intersect(rect);
                p.rect_filled(r, 0.0, over);
            }
        }
    }
}

fn hex_view(ui: &mut egui::Ui, bytes: &[u8], goto: &mut String) {
    let font = egui::TextStyle::Monospace.resolve(ui.style());
    let char_w = ui.fonts_mut(|f| f.glyph_width(&font, '0'));
    // A jump to a typed offset: the row holding it goes to the top.
    let mut jump = None;
    ui.horizontal(|ui| {
        ui.label("Go to:");
        let field = ui
            .add(
                egui::TextEdit::singleline(goto)
                    .hint_text("hex offset")
                    .desired_width(90.0)
                    .font(egui::TextStyle::Monospace),
            )
            .on_hover_text("An offset into this entry in hex (1F40 or 0x1F40), then Enter");
        let entered = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        match parse_hex_offset(goto) {
            Some(off) if off < bytes.len() => {
                if entered {
                    jump = Some(off);
                }
            }
            Some(_) => {
                ui.colored_label(
                    ui.visuals().error_fg_color,
                    format!("past the end (0x{:X} bytes)", bytes.len()),
                );
            }
            None if !goto.trim().is_empty() => {
                ui.colored_label(ui.visuals().error_fg_color, "not a hex number");
            }
            None => {
                ui.label(RichText::new(format!("0x{:X} bytes", bytes.len())).weak());
            }
        }
    });
    let per_row = hex_row_bytes(ui.available_width(), char_w);
    let rows = bytes.len().div_ceil(per_row);
    let h = ui.text_style_height(&egui::TextStyle::Monospace);
    let mut area = egui::ScrollArea::both().auto_shrink([false, false]);
    if let Some(off) = jump {
        let row = (off / per_row) as f32;
        area = area.vertical_scroll_offset(row * (h + ui.spacing().item_spacing.y));
    }
    area.show_rows(ui, h, rows, |ui, range| {
        for r in range {
            // One text row per dump row: a wrapped row would put the
            // virtualised rows out of step with the scroll bar.
            ui.add(
                egui::Label::new(
                    egui::RichText::new(hex_line(bytes, r * per_row, per_row)).monospace(),
                )
                .extend(),
            );
        }
    });
}

/// A typed hex offset: digits with an optional `0x` (or `$`) in front and
/// spaces or underscores anywhere. `None` for anything else, a blank field
/// included.
fn parse_hex_offset(text: &str) -> Option<usize> {
    let t: String = text
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '_')
        .collect();
    let digits = t
        .strip_prefix("0x")
        .or_else(|| t.strip_prefix("0X"))
        .or_else(|| t.strip_prefix('$'))
        .unwrap_or(&t);
    if digits.is_empty() {
        return None;
    }
    usize::from_str_radix(digits, 16).ok()
}

/// The characters a hex dump row of `n` bytes takes: the offset and its gap,
/// three per byte, the mid-row gap, a space, then one per byte.
fn hex_line_chars(n: usize) -> usize {
    10 + 3 * n + usize::from(n > 8) + 1 + n
}

/// Bytes per hex dump row: 16 where the row fits `width` points at
/// `char_w` per character, else 8 (the minimum window's Hex tab).
fn hex_row_bytes(width: f32, char_w: f32) -> usize {
    if hex_line_chars(16) as f32 * char_w <= width {
        16
    } else {
        8
    }
}

/// The hex dump row of `bytes` at `off`, `per_row` bytes wide; a short last
/// row is padded so its ASCII column lines up.
fn hex_line(bytes: &[u8], off: usize, per_row: usize) -> String {
    let line = &bytes[off.min(bytes.len())..(off + per_row).min(bytes.len())];
    let mut s = format!("{off:08X}  ");
    for i in 0..per_row {
        match line.get(i) {
            Some(b) => s.push_str(&format!("{b:02X} ")),
            None => s.push_str("   "),
        }
        if i == 7 && per_row > 8 {
            s.push(' ');
        }
    }
    s.push(' ');
    s.extend(line.iter().map(|&b| {
        if (0x20..0x7F).contains(&b) {
            b as char
        } else {
            '.'
        }
    }));
    s
}

/// The narrowest a waveform gets before the row lets its buttons go.
const WAVE_MIN_W: f32 = 24.0;

/// The width (whole points) a sample row's waveform takes: what is left of
/// `available` after `reserve` (the buttons behind it), at most
/// [`export::WAVE_POINTS`] and at least [`WAVE_MIN_W`].
fn wave_width(available: f32, reserve: f32) -> usize {
    (available - reserve)
        .floor()
        .clamp(WAVE_MIN_W, export::WAVE_POINTS as f32) as usize
}

/// The width a `small_button` labelled `text` takes.
fn small_button_width(ui: &egui::Ui, text: &str) -> f32 {
    let font = egui::TextStyle::Button.resolve(ui.style());
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), font, egui::Color32::WHITE);
    galley.size().x + 2.0 * ui.spacing().button_padding.x
}

/// `peaks` folded to `width` bars, each the loudest of the peaks it covers.
fn fold_peaks(peaks: &[u8], width: usize) -> Vec<u8> {
    if width == 0 || peaks.len() <= width {
        return peaks.to_vec();
    }
    (0..width)
        .map(|x| {
            let from = x * peaks.len() / width;
            let to = ((x + 1) * peaks.len() / width).max(from + 1);
            peaks[from..to].iter().copied().max().unwrap_or(0)
        })
        .collect()
}

/// A sample's waveform as a row of peak bars, one per horizontal point,
/// `width` points wide.
fn draw_wave(ui: &mut egui::Ui, peaks: &[u8], width: usize) {
    let size = egui::vec2(width as f32, 16.0);
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    let p = ui.painter_at(rect);
    let ink = crate::theme::c(crate::theme::palette(ui).info);
    let mid = rect.center().y;
    for (x, &peak) in fold_peaks(peaks, width).iter().enumerate() {
        let h = (f32::from(peak) / 255.0 * rect.height() * 0.5).max(0.5);
        let x = rect.left() + x as f32 + 0.5;
        p.line_segment(
            [egui::pos2(x, mid - h), egui::pos2(x, mid + h)],
            egui::Stroke::new(1.0, ink),
        );
    }
}

#[cfg(test)]
mod layout_tests {
    use super::*;

    /// A menu opened with `Enter` on its focused button (a controller's
    /// Cross) puts the focus on its first item, `Down` walks the items, and
    /// `Esc` (Circle) gives the focus back to the button.
    #[test]
    fn a_menu_opened_from_the_keys_takes_the_focus_and_gives_it_back() {
        let ctx = egui::Context::default();
        let first = std::cell::Cell::new(None);
        let second = std::cell::Cell::new(None);
        let button = std::cell::Cell::new(None);
        let frame = |keys: &[egui::Key]| {
            let events = keys
                .iter()
                .map(|&key| egui::Event::Key {
                    key,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                })
                .collect();
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(800.0, 600.0),
                )),
                events,
                ..Default::default()
            };
            let _ = ctx.run_ui(input, |ui| {
                let r = pad_menu_button(ui, "Menu", |ui, focus_first| {
                    let a = ui.button("First");
                    if focus_first {
                        a.request_focus();
                    }
                    first.set(Some(a.id));
                    second.set(Some(ui.button("Second").id));
                });
                button.set(Some(r.response.id));
                if ui.ctx().memory(|m| m.focused()).is_none() && keys.is_empty() {
                    r.response.request_focus();
                }
            });
        };
        let focused = || ctx.memory(|m| m.focused());
        frame(&[]);
        frame(&[]);
        assert_eq!(focused(), button.get(), "the button starts focused");
        frame(&[egui::Key::Enter]);
        for _ in 0..3 {
            frame(&[egui::Key::F24]);
        }
        assert_eq!(focused(), first.get(), "the first item takes the focus");
        frame(&[egui::Key::ArrowDown]);
        frame(&[egui::Key::F24]);
        assert_eq!(focused(), second.get(), "Down walks the menu");
        frame(&[egui::Key::Escape]);
        for _ in 0..3 {
            frame(&[egui::Key::F24]);
        }
        assert_eq!(focused(), button.get(), "Esc hands the focus back");
    }

    #[test]
    fn hex_rows_halve_where_sixteen_bytes_do_not_fit() {
        let full = hex_line(&[0x41; 16], 0, 16);
        assert_eq!(full.len(), hex_line_chars(16));
        assert_eq!(hex_line(&[0x41; 8], 0, 8).len(), hex_line_chars(8));
        // A short last row keeps its ASCII column in line.
        let short = hex_line(&[1, 2, 3], 0, 8);
        assert_eq!(short.len(), hex_line_chars(8) - 5);
        assert!(short.ends_with(" ..."));
        assert_eq!(&hex_line(b"0123456789", 8, 8)[..8], "00000008");
        // The minimum window's pane (about 390 points at 7.4 per char)
        // gets 8; a wide one 16.
        assert_eq!(hex_row_bytes(390.0, 7.4), 8);
        assert_eq!(hex_row_bytes(900.0, 7.4), 16);
    }

    /// One frame with `key` pressed; what `transport_key` read, with a text
    /// field focused first when `typing`.
    fn transport_with(key: egui::Key, typing: bool) -> Option<Transport> {
        let ctx = egui::Context::default();
        let field = egui::Id::new("field");
        let mut got = None;
        let press = |k| egui::Event::Key {
            key: k,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        for frame in 0..2 {
            let input = egui::RawInput {
                events: if frame == 1 { vec![press(key)] } else { vec![] },
                ..Default::default()
            };
            let _ = ctx.run_ui(input, |ui| {
                let mut s = String::new();
                let r = ui.add(egui::TextEdit::singleline(&mut s).id(field));
                if typing && frame == 0 {
                    r.request_focus();
                }
                if frame == 1 {
                    got = transport_key(ui);
                }
            });
        }
        got
    }

    #[test]
    fn playback_keys_yield_to_a_focused_field() {
        assert_eq!(
            transport_with(egui::Key::Space, false),
            Some(Transport::PlayPause)
        );
        assert_eq!(
            transport_with(egui::Key::ArrowLeft, false),
            Some(Transport::Back)
        );
        assert_eq!(
            transport_with(egui::Key::ArrowRight, false),
            Some(Transport::Forward)
        );
        // A search being typed keeps its space and its caret keys.
        assert_eq!(transport_with(egui::Key::Space, true), None);
        assert_eq!(transport_with(egui::Key::ArrowLeft, true), None);
    }

    #[test]
    fn waveform_gives_way_to_the_row_buttons() {
        // Wide pane: the full waveform.
        assert_eq!(wave_width(800.0, 100.0), export::WAVE_POINTS);
        // The minimum window's audio pane: what the buttons leave.
        assert_eq!(wave_width(180.5, 100.0), 80);
        // Never below the floor.
        assert_eq!(wave_width(50.0, 100.0), WAVE_MIN_W as usize);
    }

    #[test]
    fn folded_peaks_keep_the_loudest_of_each_span() {
        let peaks: Vec<u8> = (0..120).map(|i| (i % 7) as u8).collect();
        let f = fold_peaks(&peaks, 40);
        assert_eq!(f.len(), 40);
        assert!(f.iter().all(|&p| p >= 2));
        assert_eq!(fold_peaks(&[1, 9, 3, 4], 2), vec![9, 4]);
        // Narrower than the target: unchanged.
        assert_eq!(fold_peaks(&[5, 6], 10), vec![5, 6]);
    }

    #[test]
    fn start_column_keeps_a_margin_at_the_minimum_width() {
        // The minimum window is 720 points wide; the column used to stay
        // 700 wide behind a 12-point margin and ran past the right edge.
        let (side, column) = start_column(720.0, 700.0);
        assert!(side >= START_MARGIN);
        assert!(side * 2.0 + column <= 720.0 + 0.01);
        // A wide window centres the full column.
        let (side, column) = start_column(1280.0, 700.0);
        assert_eq!(column, 700.0);
        assert_eq!(side, 290.0);
    }

    #[test]
    fn tim_list_leaves_half_the_pane_to_the_controls() {
        let (default, max) = tim_list_width(400.0);
        assert!(default <= 200.0 && max <= 200.0);
        let (default, _) = tim_list_width(1000.0);
        assert_eq!(default, 270.0);
        // Never below the floor, however narrow.
        assert_eq!(tim_list_width(100.0).1, TIM_LIST_MIN);
    }

    #[test]
    fn texture_zoom_steps_and_fits_inside_its_range() {
        // A slow wheel still moves one step; a fast one moves further.
        assert_eq!(step_tim_zoom(2.0, 1.01), 2.25);
        assert_eq!(step_tim_zoom(2.0, 0.99), 1.75);
        assert_eq!(step_tim_zoom(2.0, 1.5), 3.0);
        assert_eq!(step_tim_zoom(8.0, 1.5), 8.0);
        assert_eq!(step_tim_zoom(0.5, 0.5), 0.5);
        assert_eq!(step_tim_zoom(2.0, 1.0), 2.0);
        assert_eq!(step_tim_zoom(2.0, f32::NAN), 2.0);
        // 256x256 in a 380x300 pane: 1.0 shows it whole, 1.25 would not.
        let fit = fit_tim_zoom(egui::vec2(256.0, 256.0), egui::vec2(380.0, 300.0));
        assert_eq!(fit, 1.0);
        // A strip far wider than the pane stops at the smallest zoom.
        assert_eq!(
            fit_tim_zoom(egui::vec2(1024.0, 16.0), egui::vec2(300.0, 300.0)),
            0.5
        );
        assert_eq!(
            fit_tim_zoom(egui::vec2(16.0, 16.0), egui::vec2(900.0, 900.0)),
            8.0
        );
    }

    #[test]
    fn hex_offsets_parse_with_or_without_a_prefix() {
        assert_eq!(parse_hex_offset("1F40"), Some(0x1F40));
        assert_eq!(parse_hex_offset(" 0x1f40 "), Some(0x1F40));
        assert_eq!(parse_hex_offset("$10"), Some(0x10));
        assert_eq!(parse_hex_offset("0001_0000"), Some(0x1_0000));
        assert_eq!(parse_hex_offset(""), None);
        assert_eq!(parse_hex_offset("0x"), None);
        assert_eq!(parse_hex_offset("town01"), None);
        assert_eq!(parse_hex_offset("-4"), None);
    }

    #[test]
    fn find_matches_every_word_in_any_order_and_case() {
        let t = find_terms("  LOCKED house ");
        assert_eq!(t, ["locked", "house"]);
        assert!(line_matches(
            "The house is locked and you|can't get inside.",
            &t
        ));
        assert!(!line_matches("The cupboard is empty!", &t));
        // A break in the markup does not split a phrase's words apart.
        assert!(line_matches("locked|house", &find_terms("locked house")));
        // A blank field shows every line.
        assert!(line_matches("anything", &find_terms("   ")));
    }

    #[test]
    fn start_page_tightens_on_a_short_window() {
        // The minimum window leaves the page about 400 points.
        let (top, gap) = start_gaps(400.0);
        let (roomy_top, roomy_gap) = start_gaps(640.0);
        assert!(top < roomy_top && gap < roomy_gap);
        assert_eq!(start_gaps(START_ROOMY_MIN_H), (roomy_top, roomy_gap));
    }

    #[test]
    fn tab_cycling_skips_empty_tabs_and_wraps() {
        let has = |t: Tab| !matches!(t, Tab::Audio | Tab::Text);
        assert_eq!(cycle_tab(Tab::Model, true, has), Tab::Hex);
        assert_eq!(cycle_tab(Tab::Hex, true, has), Tab::Overview);
        assert_eq!(cycle_tab(Tab::Overview, false, has), Tab::Hex);
        assert_eq!(cycle_tab(Tab::Hex, false, has), Tab::Model);
        assert_eq!(cycle_tab(Tab::Hex, true, |t| t == Tab::Hex), Tab::Hex);
    }

    #[test]
    fn about_box_fits_a_short_window() {
        // 480 points at 1.25 zoom is 384 points tall.
        assert!(about_max_height(384.0) <= 384.0 - 20.0);
        assert!(about_max_height(50.0) > ABOUT_CHROME_H);
    }

    #[test]
    fn a_partial_batch_is_an_error_naming_the_count_and_the_first_cause() {
        let dir = Path::new("/out");
        assert_eq!(
            batch_outcome("PNGs", 3, &[], dir).unwrap(),
            "3 PNGs written to /out"
        );
        let e = batch_outcome("PNGs", 3, &["#1: empty TIM".into()], dir).unwrap_err();
        let e = format!("{e:#}");
        assert!(e.contains("2 of 3 PNGs"), "{e}");
        assert!(e.contains("#1: empty TIM"), "{e}");
    }

    #[test]
    fn empty_tabs_drop_their_count() {
        assert_eq!(tab_label("Textures", Some(2)), "Textures (2)");
        assert_eq!(tab_label("Audio", Some(0)), "Audio");
        assert_eq!(tab_label("Hex", None), "Hex");
    }
}
