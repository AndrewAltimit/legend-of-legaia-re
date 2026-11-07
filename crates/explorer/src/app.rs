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
    /// `LEGAIA_EXPLORER_PLAY`: press Play once the disc has loaded.
    pub play: bool,
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
            play: std::env::var_os("LEGAIA_EXPLORER_PLAY").is_some_and(|v| !v.is_empty()),
        }
    }
}

/// The width (points) the top bar's right-aligned View and About buttons
/// need, with their spacing.
const VIEW_ABOUT_W: f32 = 110.0;
/// The bar width from which the first row also carries the app's name.
const TITLE_ROOM_W: f32 = 860.0;

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
    // Extract mode.
    out_text: String,
    job: Option<ExtractJob>,
    /// The extraction log shows only its warnings.
    log_warnings_only: bool,
    /// The game binary shipped beside this one, if any.
    engine: Option<PathBuf>,
    /// A game started with Play, while it runs.
    game: Option<crate::launch::Game>,
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
            out_text,
            job: None,
            log_warnings_only: false,
            engine: std::env::current_exe()
                .ok()
                .and_then(|exe| crate::launch::find_engine(&exe)),
            game: None,
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
        };
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
        let mut d = rfd::FileDialog::new()
            .set_file_name(default_name)
            .add_filter(ext.to_uppercase(), &[ext]);
        if let Some(dir) = &self.explorer.settings.last_export_dir {
            d = d.set_directory(dir);
        }
        let picked = d.save_file();
        if let Some(p) = &picked {
            self.explorer.settings.last_export_dir = p.parent().map(Path::to_path_buf);
            self.explorer.save_settings();
        }
        picked
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
    /// was.
    fn pick_disc(&mut self) {
        let mut dialog = rfd::FileDialog::new()
            .set_title("Choose your Legend of Legaia disc image")
            .add_filter("PlayStation disc image", &["bin", "cue", "BIN", "CUE"]);
        if let Some(dir) = self.last_dir() {
            dialog = dialog.set_directory(dir);
        }
        if let Some(p) = dialog.pick_file() {
            self.open_path(&p);
        }
    }

    /// The native folder dialog for a `legaia-extract` output folder.
    fn pick_folder(&mut self) {
        let mut dialog = rfd::FileDialog::new().set_title("Choose a legaia-extract output folder");
        if let Some(dir) = self.last_dir() {
            dialog = dialog.set_directory(dir);
        }
        if let Some(p) = dialog.pick_folder() {
            self.open_path(&p);
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
    /// (Play), Ctrl+1..4 (the views).
    fn shortcuts(&mut self, ctx: &egui::Context) {
        let cmd = |k| egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, k);
        if ctx.input_mut(|i| i.consume_shortcut(&cmd(egui::Key::O))) {
            self.pick_disc();
        }
        if ctx.input_mut(|i| i.consume_shortcut(&cmd(egui::Key::L))) {
            self.path_row = !self.path_row;
            self.path_focus = self.path_row;
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
            ui.menu_button("Open", |ui| {
                if ui
                    .add(egui::Button::new("Disc image...").shortcut_text("Ctrl+O"))
                    .clicked()
                {
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
                    ui.menu_button("View", |ui| self.view_menu(ui));
                });
            } else {
                ui.menu_button("View", |ui| self.view_menu(ui));
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
            let row = ui.horizontal(|ui| {
                ui.colored_label(green, s);
                if let Some(dir) = &self.last_written
                    && ui
                        .small_button("Show folder")
                        .on_hover_text(dir.display().to_string())
                        .clicked()
                {
                    show = Some(dir.clone());
                }
                dismiss = ui.small_button("Dismiss").clicked();
            });
            if let Some((_, at)) = &mut self.status_seen {
                if row.response.contains_pointer() {
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
    fn view_menu(&mut self, ui: &mut egui::Ui) {
        ui.label(RichText::new("Theme").strong());
        let mut theme = self.explorer.settings.theme;
        for t in crate::settings::Theme::ALL {
            ui.radio_value(&mut theme, t, t.name());
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
        let button = if running {
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
    }

    /// Start the game on the open disc (the Play button, `LEGAIA_EXPLORER_PLAY`).
    fn start_game(&mut self) {
        let disc = match &self.explorer.library {
            Some(lib) => crate::launch::playable_disc(lib),
            None => return,
        };
        if self.game.is_some() {
            return;
        }
        if let (Some(engine), Ok(bin)) = (self.engine.clone(), disc) {
            self.player.stop();
            match crate::launch::Game::start(
                &engine,
                &bin,
                crate::launch::engine_data_dir().as_deref(),
            ) {
                Ok(g) => {
                    self.explorer.status = Some(format!("Started the game on {}", bin.display()));
                    self.explorer.error = None;
                    self.last_written = None;
                    self.game = Some(g);
                    // Remember the disc for the game itself, so starting it
                    // directly later does not ask for it again. The path as
                    // chosen (a `.cue` stays a `.cue`), like its own picker.
                    let chosen = self
                        .explorer
                        .settings
                        .last_source
                        .clone()
                        .filter(|p| p.is_file())
                        .unwrap_or(bin);
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
            Ok(()) => self.explorer.status = Some("The game has closed.".into()),
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
            Tab::Hex => hex_view(ui, &a.bytes),
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
        egui::Grid::new("overview").num_columns(2).striped(true).show(ui, |ui| {
            ui.label("PROT index");
            value(ui, format!("{} (0x{:X})", e.index, e.index));
            ui.end_row();
            ui.label("Filename label");
            value(ui, lib.label(e.index).unwrap_or("-").to_string());
            ui.end_row();
            ui.label("Retail CDNAME block")
                .on_hover_text("CDNAME #define numbers are raw-TOC indices, two above extraction indices (docs/formats/cdname.md)");
            value(ui, lib.retail_block(e.index).unwrap_or("-").to_string());
            ui.end_row();
            ui.label("Kind");
            value(ui, crate::kind_of_class(a.class.name()).to_string());
            ui.end_row();
            ui.label("Category");
            value(ui, a.class.name().to_string());
            ui.end_row();
            ui.label("Location");
            value(ui, format!("PROT.DAT LBA {} (+0x{:X})", e.start_lba, e.byte_offset));
            ui.end_row();
            ui.label("Size");
            value(
                ui,
                format!("{} ({} sectors, {} bytes)", human_size(e.size_bytes), e.size_sectors, e.size_bytes),
            );
            ui.end_row();
            ui.label("Contents");
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
            ui.label(RichText::new("DATA_FIELD chunks").strong());
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
        ui.add_space(8.0);
        if ui.button("Export raw entry (.BIN)...").clicked() {
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
        // Wraps rather than clipping the export buttons off a narrow pane;
        // the sliders shrink so a label, its slider and its value box fit
        // one row of the narrowest pane.
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().slider_width = (ui.available_width() - 110.0).clamp(60.0, 140.0);
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
            ui.separator();
            ui.label("Zoom:");
            ui.add(egui::Slider::new(&mut self.tim_zoom, 0.5..=8.0).step_by(0.5));
            ui.separator();
            if ui.button("Export PNG...").clicked() {
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
            let size = tex.size_vec2() * self.tim_zoom;
            egui::ScrollArea::both().show(ui, |ui| {
                let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
                checkerboard(ui, rect);
                ui.painter().image(
                    tex.id(),
                    rect,
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    Color32::WHITE,
                );
            });
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
            let mut wanted = None;
            ui.menu_button("Export", |ui| {
                if ui.button("Wavefront OBJ (.obj)...").clicked() {
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
            .on_hover_text("Save this mesh as OBJ or textured glTF");
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

    fn audio(&mut self, ui: &mut egui::Ui, lib: &crate::Library, a: &Analysis) {
        if a.vabs.is_empty() && a.seqs.is_empty() {
            ui.label("No VAB sound banks or SEQ sequences in this entry.");
            return;
        }
        let playing = self.player.now_playing.is_some();
        ui.horizontal(|ui| {
            match &self.player.now_playing {
                Some(n) => {
                    ui.label(format!("Playing: {n}"));
                    if let Some((tick, bpm, done)) = self.player.seq_progress() {
                        ui.label(format!(
                            "tick {tick}, {bpm:.1} BPM{}",
                            if done { ", finished" } else { "" }
                        ));
                    }
                    ui.ctx()
                        .request_repaint_after(std::time::Duration::from_millis(200));
                }
                None => {
                    ui.label("Stopped");
                }
            }
            if ui.add_enabled(playing, egui::Button::new("Stop")).clicked() {
                self.player.stop();
            }
        });
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
                if ui.button("Export all samples (WAV)...").clicked() {
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
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if !a.dialog.is_empty() {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(format!("{} dialog lines", a.dialog.len())).strong(),
                        );
                        ui.label(
                            RichText::new("language-pack markup: | = line break, {c1:00} = a name")
                                .weak(),
                        );
                        if ui.small_button("Save text...").clicked() {
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
                    });
                    egui::Grid::new("dialog")
                        .striped(true)
                        .num_columns(2)
                        .show(ui, |ui| {
                            for l in &a.dialog {
                                let space = if l.from_man { "man" } else { "raw" };
                                ui.monospace(format!("{space}:0x{:05X}", l.offset));
                                ui.label(l.text.replace('|', "\n"));
                                ui.end_row();
                            }
                        });
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
                        if ui.small_button("Save text...").clicked() {
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
                    for (i, s) in m.messages.iter().enumerate() {
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
                        && let Some(p) = rfd::FileDialog::new()
                            .set_title("Choose the extraction output folder")
                            .pick_folder()
                    {
                        self.out_text = p.display().to_string();
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

impl eframe::App for App {
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
        if let Some(p) = dropped.first() {
            self.open_path(p);
        }
        self.run_automation(ui);
        self.shortcuts(ui);
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
        egui::Window::new("Keyboard shortcuts")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
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
                    });
            });
        self.keys_open = open;
    }
}

/// The shortcuts box's rows: keys, then what they do.
const KEYS: &[(&str, &str)] = &[
    ("Ctrl+O", "open a disc image"),
    ("Ctrl+L", "type a path to open"),
    ("Ctrl+1 .. Ctrl+4", "Browse, Characters, Movies, Extract"),
    ("F5", "play the open disc"),
    ("Ctrl+F, Esc", "search the archive, clear the search"),
    ("Ctrl+Tab, Ctrl+Shift+Tab", "next, previous preview tab"),
    (
        "Up / Down, PgUp / PgDn, Home / End",
        "walk the archive tree",
    ),
    (
        "Space, Left / Right",
        "play or pause, step a frame (movies, animations)",
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
        crate::settings::Theme::System => egui::ThemePreference::System,
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
                        ui.add_space(28.0);
                        ui.label(RichText::new("Legaia Explorer").size(30.0).color(accent));
                        ui.label(
                            RichText::new(
                                "Browse, preview and extract everything on your own \
                                 Legend of Legaia disc - and start the game on it.",
                            )
                            .size(15.0),
                        );
                        ui.add_space(18.0);
                        ui.horizontal(|ui| {
                            let big = egui::vec2(230.0, 38.0);
                            if ui
                                .add(
                                    crate::theme::primary_button("Open disc image...")
                                        .min_size(big),
                                )
                                .on_hover_text(
                                    "Your .bin (a raw dump, 2352-byte sectors) or its .cue. Ctrl+O",
                                )
                                .clicked()
                            {
                                self.pick_disc();
                            }
                            if ui
                                .add(egui::Button::new("Open extracted folder...").min_size(big))
                                .on_hover_text("A folder legaia-extract (or Extract) wrote")
                                .clicked()
                            {
                                self.pick_folder();
                            }
                        });
                        ui.label(
                            RichText::new(
                                "or drop the file (or folder) on this window, or type its \
                                 path in the bar above",
                            )
                            .weak(),
                        );
                        if let Some(p) = self.explorer.loading() {
                            ui.add_space(10.0);
                            ui.horizontal(|ui| {
                                ui.spinner();
                                ui.label(format!("Opening {} ...", p.display()));
                            });
                        }
                        if !self.explorer.settings.recent.is_empty() {
                            ui.add_space(20.0);
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
                        ui.add_space(20.0);
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
                                         disc). F5",
                                    ),
                                ] {
                                    ui.label(RichText::new(title).strong().color(accent));
                                    ui.add(egui::Label::new(what).wrap());
                                    ui.end_row();
                                }
                            });
                        ui.add_space(20.0);
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

fn hex_view(ui: &mut egui::Ui, bytes: &[u8]) {
    let font = egui::TextStyle::Monospace.resolve(ui.style());
    let char_w = ui.fonts_mut(|f| f.glyph_width(&font, '0'));
    let per_row = hex_row_bytes(ui.available_width(), char_w);
    let rows = bytes.len().div_ceil(per_row);
    let h = ui.text_style_height(&egui::TextStyle::Monospace);
    egui::ScrollArea::both()
        .auto_shrink([false, false])
        .show_rows(ui, h, rows, |ui, range| {
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
