//! The egui front end over [`Studio`].

mod actors_ui;
mod media_ui;
mod viewport;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::Receiver;

use eframe::egui::{self, Color32, ColorImage, RichText, TextureHandle, TextureOptions};

use crate::analyze::Analysis;
use crate::extract_job::{ExtractJob, Outcome};
use crate::player::Player;
use crate::raster::{Model, Shading};
use crate::scene_vram::SceneVram;
use crate::{Source, Studio, export};
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

    /// The settings / `LEGAIA_STUDIO_TAB` spelling.
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

    /// The settings / `LEGAIA_STUDIO_TAB` spelling.
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
    /// `LEGAIA_STUDIO_SELECT`: PROT index to select once loaded.
    pub select: Option<u32>,
    /// `LEGAIA_STUDIO_TMD`: which TMD of the selected entry the 3D model
    /// tab shows (the `TMD #` picker), instead of the first.
    pub tmd: Option<usize>,
    /// `LEGAIA_STUDIO_TAB`: preview tab (`textures`, `model`, `audio`, ...)
    /// or `extract`.
    pub tab: Option<String>,
    /// `LEGAIA_STUDIO_EXTRACT_TO`: start an extraction into this folder.
    pub extract_to: Option<PathBuf>,
    /// `LEGAIA_STUDIO_SCREENSHOT`: save a PNG of the window once the
    /// scripted state has settled (after the extraction, when one runs),
    /// then quit.
    pub screenshot: Option<PathBuf>,
    /// `LEGAIA_STUDIO_EXPORT_DIR`: export without a save dialog, into this
    /// folder.
    pub export_dir: Option<PathBuf>,
    /// `LEGAIA_STUDIO_ACTOR`: open the Actors view on `monster:<id>`,
    /// `char:<slot>` (0 Vahn .. 3 Terra), optionally `@<clip>:<frame>`.
    pub actor: Option<String>,
    /// `LEGAIA_STUDIO_MEDIA`: open the Movies & voice view on this ISO path
    /// (`MOV/MV1.STR`), optionally `@<frame>`.
    pub media: Option<String>,
    /// `LEGAIA_STUDIO_SEARCH`: start with this text in the archive search.
    pub search: Option<String>,
    /// `LEGAIA_STUDIO_PLAY`: press Play once the disc has loaded.
    pub play: bool,
}

impl Automation {
    pub fn from_env() -> Self {
        let path = |k: &str| std::env::var_os(k).map(PathBuf::from);
        Self {
            select: std::env::var("LEGAIA_STUDIO_SELECT")
                .ok()
                .and_then(|s| s.trim().parse().ok()),
            tmd: std::env::var("LEGAIA_STUDIO_TMD")
                .ok()
                .and_then(|s| s.trim().parse().ok()),
            tab: std::env::var("LEGAIA_STUDIO_TAB").ok(),
            extract_to: path("LEGAIA_STUDIO_EXTRACT_TO"),
            screenshot: path("LEGAIA_STUDIO_SCREENSHOT"),
            export_dir: path("LEGAIA_STUDIO_EXPORT_DIR"),
            actor: std::env::var("LEGAIA_STUDIO_ACTOR").ok(),
            media: std::env::var("LEGAIA_STUDIO_MEDIA").ok(),
            search: std::env::var("LEGAIA_STUDIO_SEARCH").ok(),
            play: std::env::var_os("LEGAIA_STUDIO_PLAY").is_some_and(|v| !v.is_empty()),
        }
    }
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
    entry_vram: legaia_tim::Vram,
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
    pub studio: Studio,
    mode: Mode,
    tab: Tab,
    about_open: bool,
    /// The window's inner size, tracked for the settings file.
    window_size: Option<[f32; 2]>,
    window_pos: Option<[i32; 2]>,
    path_text: String,
    filter: String,
    class_filter: Option<String>,
    // Textures tab.
    tim_sel: usize,
    clut: usize,
    tim_zoom: f32,
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
    player: Player,
    // Extract mode.
    out_text: String,
    job: Option<ExtractJob>,
    /// The game binary shipped beside this one, if any.
    engine: Option<PathBuf>,
    /// A game started with Play, while it runs.
    game: Option<crate::launch::Game>,
    /// The folder the last export or extraction wrote into, for the
    /// status line's *Show folder* button.
    last_written: Option<PathBuf>,
    /// The informational status text last seen and when it appeared (or
    /// was last hovered): it clears itself after [`STATUS_TTL`].
    status_seen: Option<(String, std::time::Instant)>,
    /// Theme and zoom from the settings have been pushed into egui.
    appearance_applied: bool,
    // Automation.
    auto: Automation,
    auto_selected: bool,
    auto_extract_started: bool,
    settle_since: Option<std::time::Instant>,
    screenshot_requested: bool,
}

impl App {
    pub fn new(studio: Studio, initial: Option<PathBuf>, auto: Automation) -> Self {
        let mut studio = studio;
        // Recent entries whose disc or folder has gone are dropped at start.
        if studio.settings.prune_recent() {
            studio.save_settings();
        }
        // The disc or folder opened last, when it is still there. When it
        // has gone (moved, renamed, an unplugged drive) say so: the recent
        // list has just dropped it, and the start page alone would read as
        // a first run.
        let last = studio.settings.last_source.clone();
        let open = initial.or_else(|| last.clone().filter(|p| p.exists()));
        if open.is_none()
            && let Some(gone) = last.filter(|p| !p.exists())
        {
            studio.error = Some(format!(
                "The disc opened last is no longer at {} - open it from where it is now.",
                gone.display()
            ));
        }
        let path_text = open
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        if let Some(p) = &open {
            studio.open(p);
        }
        let out_text = studio
            .settings
            .last_out_dir
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        // The view and tab open when the app last closed.
        let mode = studio
            .settings
            .mode
            .as_deref()
            .and_then(Mode::parse)
            .unwrap_or(Mode::Browse);
        let tab = studio
            .settings
            .tab
            .as_deref()
            .and_then(Tab::parse)
            .unwrap_or(Tab::Overview);
        let mut app = Self {
            studio,
            mode,
            tab,
            about_open: false,
            window_size: None,
            window_pos: None,
            path_text,
            filter: String::new(),
            class_filter: None,
            tim_sel: 0,
            clut: 0,
            tim_zoom: 2.0,
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
            player: Player::default(),
            out_text,
            job: None,
            engine: std::env::current_exe()
                .ok()
                .and_then(|exe| crate::launch::find_engine(&exe)),
            game: None,
            last_written: None,
            status_seen: None,
            appearance_applied: false,
            auto,
            auto_selected: false,
            auto_extract_started: false,
            settle_since: None,
            screenshot_requested: false,
        };
        if let Some(t) = app.auto.tab.clone() {
            if t.eq_ignore_ascii_case("about") {
                app.about_open = true;
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

    fn open_path(&mut self, p: &Path) {
        self.path_text = p.display().to_string();
        self.player.stop();
        self.model = None;
        self.tim_tex = None;
        self.tim_err = None;
        self.studio.open(p);
    }

    fn select(&mut self, pos: usize) {
        if self.studio.selected != Some(pos) {
            self.tim_sel = 0;
            self.clut = 0;
            self.tmd_sel = 0;
            self.vab_sel = 0;
            self.tim_tex = None;
            self.tim_err = None;
            self.model = None;
        }
        self.studio.select(pos);
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
        if let Some(dir) = &self.studio.settings.last_export_dir {
            d = d.set_directory(dir);
        }
        let picked = d.save_file();
        if let Some(p) = &picked {
            self.studio.settings.last_export_dir = p.parent().map(Path::to_path_buf);
            self.studio.save_settings();
        }
        picked
    }

    fn report(&mut self, what: &str, res: anyhow::Result<()>, path: &Path) {
        match res {
            Ok(()) => {
                self.studio.status = Some(format!("{what} written to {}", path.display()));
                self.studio.error = None;
                self.last_written = if path.is_dir() {
                    Some(path.to_path_buf())
                } else {
                    path.parent().map(Path::to_path_buf)
                };
            }
            Err(e) => self.studio.error = Some(format!("{what} export failed: {e:#}")),
        }
    }

    fn run_automation(&mut self, ctx: &egui::Context) {
        if !self.auto_selected
            && let (Some(idx), Some(lib)) = (self.auto.select, self.studio.library.clone())
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
            && self.studio.library.is_some()
        {
            self.auto_extract_started = true;
            self.out_text = dir.display().to_string();
            self.start_extract();
        }
        if self.auto.play && self.studio.library.is_some() && self.studio.loading().is_none() {
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
        let loaded = self.studio.loading().is_none();
        let has_lib = self.studio.library.is_some();
        let analysed = self.auto.select.is_none() || !has_lib || self.studio.analysis.is_some();
        let extracted = self.auto.extract_to.is_none()
            || !has_lib
            || self.job.as_ref().is_some_and(|j| j.outcome().is_some());
        let idle =
            loaded && analysed && extracted && !self.studio.is_analyzing() && !self.media.busy();
        if !idle && self.studio.error.is_none() {
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
            match legaia_tim::write_png(&shot, w, h, &rgba) {
                Ok(()) => log::info!("screenshot saved to {}", shot.display()),
                Err(e) => log::error!("screenshot: {e:#}"),
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    fn start_extract(&mut self) {
        let Some(lib) = &self.studio.library else {
            self.studio.error = Some("Open a disc image first.".into());
            return;
        };
        let Source::Disc(bin) = &lib.source else {
            self.studio.error = Some(
                "Extraction reads a disc image; the open source is an extracted folder.".into(),
            );
            return;
        };
        let out = PathBuf::from(self.out_text.trim());
        if self.out_text.trim().is_empty() {
            self.studio.error = Some("Choose an output folder.".into());
            return;
        }
        let s = &mut self.studio.settings;
        s.last_out_dir = Some(out.clone());
        let options = legaia_extract::Options {
            bin: bin.clone(),
            out,
            skip_verify: s.skip_verify,
            skip_png: s.skip_png,
            skip_xa: s.skip_xa,
            skip_catalog: s.skip_catalog,
            skip_font: s.skip_font,
            all_textures: s.all_textures,
            verbose: false,
        };
        self.studio.save_settings();
        self.studio.error = None;
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
        self.studio
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
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::F5)) {
            self.start_game();
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
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("Legaia Studio")
                    .heading()
                    .color(crate::theme::accent(ui)),
            );
            ui.separator();
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
            if ui
                .button("Open disc image...")
                .on_hover_text("Ctrl+O")
                .clicked()
            {
                self.pick_disc();
            }
            if ui.button("Open extracted folder...").clicked() {
                self.pick_folder();
            }
            ui.add_enabled_ui(!self.studio.settings.recent.is_empty(), |ui| {
                ui.menu_button("Recent", |ui| {
                    if let Some(p) = self.recent_list(ui) {
                        ui.close();
                        self.open_path(&p);
                    }
                })
                .response
                .on_disabled_hover_text("Discs and folders you open are listed here.");
            });
            ui.separator();
            self.play_button(ui);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("About").on_hover_text("F1").clicked() {
                    self.about_open = !self.about_open;
                }
                ui.menu_button("View", |ui| self.view_menu(ui));
            });
        });
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
            if (ui.button("Open").clicked() || enter) && !self.path_text.trim().is_empty() {
                let p = PathBuf::from(self.path_text.trim());
                self.open_path(&p);
            }
        });
        ui.horizontal(|ui| {
            if let Some(p) = self.studio.loading() {
                ui.spinner();
                ui.label(format!("Loading {} ...", p.display()));
            } else if let Some(lib) = &self.studio.library {
                let kind = match lib.source {
                    Source::Disc(_) => "disc image",
                    Source::Extracted(_) => "extracted folder",
                };
                ui.label(format!(
                    "{kind}: {} - {} PROT entries, {} ISO files{}",
                    lib.source.path().display(),
                    lib.entries.len(),
                    lib.iso_files.len(),
                    lib.region
                        .as_ref()
                        .map(|r| format!(", region {r}"))
                        .unwrap_or_default()
                ));
                let n = self.studio.classified();
                if n < lib.entries.len() {
                    ui.spinner();
                    ui.label(format!("classifying {n}/{}", lib.entries.len()));
                }
            } else {
                ui.label("No disc open. Legaia Studio reads your own Legend of Legaia disc image; no game data is included.");
            }
        });
        let mut dismiss = false;
        let mut show: Option<PathBuf> = None;
        // An informational line clears itself after `STATUS_TTL` (the clock
        // restarts while the pointer is on it); an error stays until it is
        // dismissed, and can be copied for a bug report.
        let now = std::time::Instant::now();
        match &self.studio.status {
            Some(s) if self.status_seen.as_ref().is_none_or(|(seen, _)| seen != s) => {
                self.status_seen = Some((s.clone(), now));
            }
            None => self.status_seen = None,
            _ => {}
        }
        if let Some(e) = &self.studio.error {
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
                        if ui
                            .small_button("Copy")
                            .on_hover_text("Copy this message to the clipboard")
                            .clicked()
                        {
                            ui.ctx().copy_text(e.clone());
                        }
                        dismiss = ui.small_button("Dismiss").clicked();
                        ui.add(egui::Label::new(RichText::new(e).color(red)).wrap());
                    });
                });
        } else if let Some(s) = &self.studio.status {
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
            self.studio.error = None;
            self.studio.status = None;
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
        if self.studio.settings.prune_recent() {
            self.studio.save_settings();
        }
        let mut open = None;
        let mut forget = None;
        for p in &self.studio.settings.recent {
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
                    .studio
                    .library
                    .as_ref()
                    .is_some_and(|_| self.studio.settings.last_source.as_ref() == Some(p));
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
            self.studio.settings.forget_recent(&p);
            self.studio.save_settings();
        }
        if !self.studio.settings.recent.is_empty() {
            ui.separator();
            if ui.button("Clear list").clicked() {
                self.studio.settings.recent.clear();
                self.studio.save_settings();
            }
        }
        open
    }

    /// The View menu: theme and UI zoom, both remembered.
    fn view_menu(&mut self, ui: &mut egui::Ui) {
        ui.label(RichText::new("Theme").strong());
        let mut theme = self.studio.settings.theme;
        for t in crate::settings::Theme::ALL {
            ui.radio_value(&mut theme, t, t.name());
        }
        if theme != self.studio.settings.theme {
            self.studio.settings.theme = theme;
            ui.ctx().set_theme(theme_preference(theme));
            self.studio.save_settings();
        }
        ui.separator();
        ui.label(RichText::new(format!("Zoom ({:.0}%)", ui.ctx().zoom_factor() * 100.0)).strong());
        egui::gui_zoom::zoom_menu_buttons(ui);
    }

    /// Push the remembered theme and zoom into egui once, then keep the
    /// settings in step with zoom changes (the keyboard shortcuts egui
    /// handles itself, and the View menu).
    fn sync_appearance(&mut self, ctx: &egui::Context) {
        let s = &mut self.studio.settings;
        if !self.appearance_applied {
            self.appearance_applied = true;
            crate::theme::install(ctx);
            ctx.set_theme(theme_preference(s.theme));
            ctx.set_zoom_factor(crate::settings::sane_zoom(s.zoom));
            return;
        }
        let z = ctx.zoom_factor();
        if (z - crate::settings::sane_zoom(s.zoom)).abs() > 1e-3 {
            s.zoom = (z.is_finite() && (z - 1.0).abs() > 1e-3).then_some(z);
            self.studio.save_settings();
        }
    }

    /// Open `dir` in the file manager, reporting a failure on the error line.
    fn show_folder(&mut self, dir: &Path) {
        if let Err(e) = crate::launch::open_folder(dir) {
            self.studio.error = Some(format!(
                "Could not open a file manager on {}: {e}",
                dir.display()
            ));
        }
    }

    /// *Play*: start the game on the open disc, when the game binary ships
    /// beside this one and the disc is the one it runs on. The hover text
    /// says why it is unavailable otherwise.
    fn play_button(&mut self, ui: &mut egui::Ui) {
        let running = self.game.is_some();
        let disc = match &self.studio.library {
            Some(lib) => crate::launch::playable_disc(lib),
            None => Err("Open your Legend of Legaia (USA) disc image first.".to_string()),
        };
        let blocked = match (&self.engine, &disc) {
            _ if running => Some("The game is running.".to_string()),
            (None, _) => Some(format!(
                "{} was not found beside legaia-studio. It ships in the same \
                 release folder; keep the two programs together.",
                crate::launch::engine_file_name()
            )),
            (_, Err(why)) => Some(why.clone()),
            _ => None,
        };
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

    /// Start the game on the open disc (the Play button, `LEGAIA_STUDIO_PLAY`).
    fn start_game(&mut self) {
        let disc = match &self.studio.library {
            Some(lib) => crate::launch::playable_disc(lib),
            None => return,
        };
        if self.game.is_some() {
            return;
        }
        if let (Some(engine), Ok(bin)) = (self.engine.clone(), disc) {
            self.player.stop();
            // The launch switches the game's own launcher keeps
            // (`--psx-render`, `--retail`), so both starts look the same.
            let extra = crate::launch::engine_launcher_settings()
                .map(|f| crate::launch::launch_switches(&f))
                .unwrap_or_default();
            match crate::launch::Game::start(
                &engine,
                &bin,
                &extra,
                crate::launch::engine_data_dir().as_deref(),
            ) {
                Ok(g) => {
                    self.studio.status = Some(format!("Started the game on {}", bin.display()));
                    self.studio.error = None;
                    self.last_written = None;
                    self.game = Some(g);
                    // Remember the disc for the game itself, so starting it
                    // directly later does not ask for it again. The path as
                    // chosen (a `.cue` stays a `.cue`), like its own picker.
                    let chosen = self
                        .studio
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
                Err(e) => self.studio.error = Some(format!("Could not start the game: {e:#}")),
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
            Ok(()) => self.studio.status = Some("The game has closed.".into()),
            Err(status) => {
                let mut msg = format!("The game stopped with an error ({status}).");
                if let Some(log) = &log {
                    for line in crate::launch::log_tail(log, 3) {
                        msg.push_str("\n  ");
                        msg.push_str(&line);
                    }
                    msg.push_str(&format!("\nFull log: {}", log.display()));
                }
                self.studio.error = Some(msg);
            }
        }
    }

    fn browser(&mut self, ui: &mut egui::Ui) {
        let Some(lib) = self.studio.library.clone() else {
            ui.label("Open a disc image to browse its PROT.DAT archive.");
            return;
        };
        ui.horizontal(|ui| {
            ui.label("Search:");
            let resp = ui.add(
                egui::TextEdit::singleline(&mut self.filter)
                    .hint_text("scene, place name, category or index (Ctrl+F)"),
            );
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
        let mut classes: Vec<&'static str> = (0..lib.entries.len())
            .filter_map(|p| self.studio.class_of(p).map(|c| c.name()))
            .collect();
        classes.sort_unstable();
        classes.dedup();
        egui::ComboBox::from_label("Category")
            .selected_text(self.class_filter.as_deref().unwrap_or("all"))
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut self.class_filter, None, "all");
                for c in classes {
                    ui.selectable_value(&mut self.class_filter, Some(c.to_string()), c);
                }
            });
        ui.separator();
        let filter = self.filter.trim().to_string();
        let mut clicked = None;
        let mut iso_save: Option<crate::library::IsoFile> = None;
        // Every row the search shows, in disc order: what the arrow keys walk.
        let mut visible: Vec<usize> = Vec::new();
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
                for block in &self.studio.blocks {
                    let rows: Vec<usize> = block
                        .entries
                        .iter()
                        .copied()
                        .filter(|&pos| {
                            self.studio.entry_matches(
                                pos,
                                &block.name,
                                &filter,
                                self.class_filter.as_deref(),
                            )
                        })
                        .collect();
                    if rows.is_empty() {
                        continue;
                    }
                    visible.extend_from_slice(&rows);
                    let first = lib.entries[rows[0]].index;
                    let open = (!filter.is_empty() || self.class_filter.is_some())
                        .then_some(true)
                        .or_else(|| {
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
                    egui::CollapsingHeader::new(title)
                        .id_salt(("block", first))
                        .open(open)
                        .show(ui, |ui| {
                            for pos in rows {
                                let e = &lib.entries[pos];
                                let class =
                                    self.studio.class_of(pos).map(|c| c.name()).unwrap_or("...");
                                let text = format!(
                                    "{:04} {:>8}  {}",
                                    e.index,
                                    human_size(e.size_bytes),
                                    class
                                );
                                let sel = self.studio.selected == Some(pos);
                                let resp =
                                    ui.selectable_label(sel, RichText::new(text).monospace());
                                if self.reveal == Some(pos) {
                                    resp.scroll_to_me(Some(egui::Align::Center));
                                }
                                if resp.clicked() {
                                    clicked = Some(pos);
                                }
                            }
                        });
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
        let typing = ui.ctx().wants_keyboard_input();
        if !typing && !visible.is_empty() {
            let cur = self
                .studio
                .selected
                .and_then(|s| visible.iter().position(|&p| p == s));
            let last = visible.len() - 1;
            let step = ui.input(|i| {
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
            if let Some(k) = step
                && Some(k) != cur
            {
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
        let Some(lib) = self.studio.library.clone() else {
            self.start_page(ui);
            return;
        };
        let Some(pos) = self.studio.selected else {
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
        let Some(a) = self.studio.analysis.clone().filter(|a| a.pos == pos) else {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Reading entry...");
            });
            return;
        };
        let e = &lib.entries[pos];
        ui.horizontal(|ui| {
            ui.heading(lib.stem(e.index));
            ui.label(format!("{} - {}", a.class.name(), human_size(e.size_bytes)));
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
        ui.horizontal(|ui| {
            for t in Tab::ALL {
                let n = count(t);
                let label = match n {
                    Some(n) => format!("{} ({n})", t.name()),
                    None => t.name().to_string(),
                };
                let enabled = n.is_none_or(|n| n > 0);
                let resp = ui.add_enabled(enabled, egui::Button::selectable(shown == t, label));
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
        egui::Grid::new("overview").num_columns(2).striped(true).show(ui, |ui| {
            ui.label("PROT index");
            ui.monospace(format!("{} (0x{:X})", e.index, e.index));
            ui.end_row();
            ui.label("Filename label");
            ui.monospace(lib.label(e.index).unwrap_or("-"));
            ui.end_row();
            ui.label("Retail CDNAME block");
            ui.monospace(lib.retail_block(e.index).unwrap_or("-"))
                .on_hover_text("CDNAME #define numbers are raw-TOC indices, two above extraction indices (docs/formats/cdname.md)");
            ui.end_row();
            ui.label("Category");
            ui.monospace(a.class.name());
            ui.end_row();
            ui.label("Location");
            ui.monospace(format!(
                "PROT.DAT LBA {} (+0x{:X}), {} sectors, {} bytes",
                e.start_lba, e.byte_offset, e.size_sectors, e.size_bytes
            ));
            ui.end_row();
            ui.label("Contents");
            ui.monospace(format!(
                "{} TIM, {} TMD, {} VAB, {} SEQ, {} MES, {} LZS sections",
                a.tims.len(),
                a.tmds.len(),
                a.vabs.len(),
                a.seqs.len(),
                a.mes.len(),
                a.tim_sections.len()
            ));
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
        egui::SidePanel::left("tim_list")
            .resizable(true)
            // Wide enough for one unwrapped row ("#0   320x8 4bpp lzs#0 @0x4F00").
            .default_width(270.0)
            .show_inside(ui, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
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
                            .selectable_label(self.tim_sel == i, RichText::new(text).monospace())
                            .clicked()
                        {
                            self.tim_sel = i;
                            self.clut = 0;
                        }
                    }
                });
            });
        let t = a.tims[self.tim_sel].clone();
        // Wraps rather than clipping the export buttons off a narrow pane.
        ui.horizontal_wrapped(|ui| {
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
                    let mut n = 0;
                    for (i, t) in a.tims.iter().enumerate() {
                        let p = dir.join(format!("{}_tim{i:02}.png", lib.stem(a.index)));
                        if export::tim_png(a, t, 0, &p).is_ok() {
                            n += 1;
                        }
                    }
                    self.studio.status = Some(format!("{n} PNGs written to {}", dir.display()));
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
                checkerboard(ui.painter(), rect);
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
            // Textures inside the entry: show those. None: the mesh samples
            // pages another entry of its scene uploads, so start there.
            let source = if a.tims.is_empty() {
                TexSource::Scene
            } else {
                TexSource::Entry
            };
            self.model = Some(ModelView {
                key,
                model: Model::from_tmd(a.tmd_bytes(&t)),
                entry_vram: a.vram(),
                source,
                drawn_with: None,
                view: Viewport::new(Shading::Textured),
            });
        }
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt("tmd")
                .selected_text(format!("TMD #{}", self.tmd_sel))
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
                });
            if let Some(mv) = &mut self.model {
                mv.view.controls(
                    ui,
                    "texel x colour / 128, sampled from the chosen VRAM image",
                );
                if let Some(m) = &mv.model {
                    ui.label(format!("{} triangles", m.triangles()));
                }
            }
            ui.separator();
            if ui.button("Export OBJ...").clicked() {
                let name = format!("{}_tmd{:02}.obj", lib.stem(a.index), self.tmd_sel);
                if let Some(out) = self.export_target(&name, "obj") {
                    let res = export::tmd_obj(a, &t, &out);
                    self.report("OBJ", res, &out);
                }
            }
            if ui.button("Export glTF (.glb)...").clicked() {
                let name = format!("{}_tmd{:02}.glb", lib.stem(a.index), self.tmd_sel);
                let scene = match self.model.as_ref().map(|m| m.source) {
                    Some(TexSource::Scene) => self.scene_vram(lib, a).ok().map(|(_, v)| v),
                    _ => None,
                };
                if let Some(out) = self.export_target(&name, "glb") {
                    let res = export::tmd_glb(a, &t, scene.as_ref().map(|v| &v.vram), &out);
                    self.report("glTF", res, &out);
                }
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
        ui.horizontal(|ui| {
            ui.label("Textures:");
            ui.selectable_value(
                &mut mv.source,
                TexSource::Entry,
                format!("this entry ({} TIM)", a.tims.len()),
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
                    ui.label(
                        RichText::new(format!(
                            "{:?} load: {} TIMs ({} shared) + {} host layers",
                            v.kind,
                            v.tims,
                            v.shared_tims,
                            v.layers.len()
                        ))
                        .weak(),
                    )
                    .on_hover_text(v.layers.join("\n"));
                }
                (None, Some(e)) if mv.source == TexSource::Scene => {
                    ui.spinner();
                    ui.label(RichText::new(e).weak());
                }
                _ => {}
            }
            ui.label(RichText::new("Drag to orbit, scroll to zoom.").weak());
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
        mv.view.show(ui, model, vram);
    }

    fn audio(&mut self, ui: &mut egui::Ui, lib: &crate::Library, a: &Analysis) {
        if a.vabs.is_empty() && a.seqs.is_empty() {
            ui.label("No VAB sound banks or SEQ sequences in this entry.");
            return;
        }
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
            if ui.button("Stop").clicked() {
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
                        match export::vab_all_wavs(a, &a.vabs[self.vab_sel], &dir, &stem) {
                            Ok(n) => {
                                self.studio.status =
                                    Some(format!("{n} WAVs written to {}", dir.display()))
                            }
                            Err(e) => self.studio.error = Some(format!("{e:#}")),
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
                ui.horizontal(|ui| {
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
                        self.studio.error = Some(format!("{e:#}"));
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
                        ui.monospace(format!("sample {i:3}  {:>7} B", span.size));
                        if ui.small_button("Play").clicked() {
                            match export::vag_pcm(a, &v, i) {
                                Ok(pcm) => {
                                    if let Err(e) = self.player.play_pcm(
                                        pcm,
                                        legaia_engine_audio::vab_bind::VAB_SAMPLE_RATE,
                                        format!("sample {i}"),
                                    ) {
                                        self.studio.error = Some(format!("{e:#}"));
                                    }
                                }
                                Err(e) => self.studio.error = Some(format!("{e:#}")),
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
            "Runs the full legaia-extract pipeline: ISO9660 files, PROT entries with CDNAME names, \
             categorize.json, streaming sub-assets, TIM -> PNG, CD-XA audio -> WAV, the TIM catalogs \
             and the dialog font. The output folder is yours; nothing is uploaded anywhere.",
        );
        ui.add_space(6.0);
        let running = self.job.as_ref().is_some_and(|j| j.outcome().is_none());
        let disc = match self.studio.library.as_ref().map(|l| &l.source) {
            Some(Source::Disc(p)) => Some(p.clone()),
            _ => None,
        };
        // A first extraction proposes a folder beside the disc image.
        if self.out_text.trim().is_empty()
            && let Some(dir) = disc.as_deref().and_then(crate::launch::suggested_out_dir)
        {
            self.out_text = dir.display().to_string();
        }
        egui::Grid::new("extract_opts")
            .num_columns(2)
            .show(ui, |ui| {
                ui.label("Disc image");
                match &disc {
                    Some(p) => ui.monospace(p.display().to_string()),
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
                        egui::TextEdit::singleline(&mut self.out_text).desired_width(420.0),
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
                });
                ui.end_row();
                ui.label("Options");
                ui.vertical(|ui| {
                    let s = &mut self.studio.settings;
                    ui.add_enabled_ui(!running, |ui| {
                        ui.checkbox(
                            &mut s.skip_verify,
                            "Skip SHA-256 verification (--skip-verify)",
                        );
                        ui.checkbox(&mut s.skip_png, "Skip TIM -> PNG conversion (--skip-png)");
                        ui.checkbox(&mut s.skip_xa, "Skip CD-XA audio -> WAV (--skip-xa)");
                        ui.checkbox(
                            &mut s.skip_catalog,
                            "Skip TIM catalog TSVs (--skip-catalog)",
                        );
                        ui.checkbox(&mut s.skip_font, "Skip dialog font (--skip-font)");
                    ui.checkbox(
                        &mut s.all_textures,
                        "Export all textures as PNG: every TIM, raw and compressed, into tim_scan/ (--all-textures)",
                    );
                    });
                });
                ui.end_row();
            });
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
        let Some(outcome) = self.job.as_ref().map(ExtractJob::outcome) else {
            return;
        };
        if matches!(outcome, Some(Outcome::Done(_)) | Some(Outcome::Cancelled)) {
            // The folder this job wrote into (`start_extract` records it).
            if let Some(out) = self.studio.settings.last_out_dir.clone()
                && out.is_dir()
                && ui.button("Open output folder").clicked()
            {
                self.show_folder(&out);
            }
        }
        let Some(job) = &self.job else { return };
        let p = job.progress();
        let text = match &outcome {
            None => format!(
                "step {}/{}: {} ({}/{})",
                p.step,
                legaia_extract::STEP_COUNT,
                p.step_name,
                p.done,
                p.total
            ),
            Some(Outcome::Done(s)) => format!(
                "Done: {} disc files, {} PROT entries, {} streams, {} PNGs, {} WAVs{}",
                s.iso_files,
                s.prot_entries,
                s.streams,
                s.pngs,
                s.wavs,
                if s.all_textures.0 > 0 {
                    format!(
                        ", {} textures -> {} PNGs in tim_scan/",
                        s.all_textures.0, s.all_textures.1
                    )
                } else {
                    String::new()
                }
            ),
            Some(Outcome::Cancelled) => "Cancelled".into(),
            Some(Outcome::Failed(e)) => format!("Failed: {e}"),
        };
        let frac = if matches!(outcome, Some(Outcome::Done(_))) {
            1.0
        } else {
            p.fraction()
        };
        ui.add(egui::ProgressBar::new(frac).text(text));
        if p.warnings > 0 {
            ui.label(format!("{} warning(s) in the log", p.warnings));
        }
        if running {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(100));
        }
        ui.separator();
        let row = ui.text_style_height(&egui::TextStyle::Monospace);
        egui::ScrollArea::both()
            .stick_to_bottom(true)
            .auto_shrink([false, false])
            .show_rows(ui, row, p.log.len(), |ui, range| {
                // One row per line, so the virtualised rows stay aligned.
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                for line in &p.log[range] {
                    ui.monospace(line);
                }
            });
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.sync_appearance(ctx);
        if self.studio.poll() {
            ctx.request_repaint();
        }
        if self.studio.loading().is_some() || self.studio.is_analyzing() {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }
        if let Some(lib) = &self.studio.library
            && self.studio.classified() < lib.entries.len()
        {
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
        }
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .filter_map(|f| f.path.clone())
                .collect()
        });
        if let Some(p) = dropped.first() {
            self.open_path(p);
        }
        self.run_automation(ctx);
        self.poll_game(ctx);
        self.shortcuts(ctx);
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::F1)) {
            self.about_open = !self.about_open;
        }

        if let Some(r) = ctx.input(|i| i.viewport().inner_rect) {
            // egui points include the UI zoom; the viewport's inner size at
            // start does not, so store the unzoomed size or a zoomed window
            // would shrink on every start.
            let z = ctx.zoom_factor();
            self.window_size = Some([r.width() * z, r.height() * z]);
        }
        // The outer corner in physical pixels (`placement`): points times
        // pixels per point, which already folds in the UI zoom. Wayland
        // reports no position, so nothing is kept there.
        if let Some(r) = ctx.input(|i| i.viewport().outer_rect) {
            let ppp = ctx.pixels_per_point();
            self.window_pos = Some([
                (r.min.x * ppp).round() as i32,
                (r.min.y * ppp).round() as i32,
            ]);
        }
        egui::TopBottomPanel::top("top").show(ctx, |ui| self.top_bar(ui));
        self.about_window(ctx);
        match self.mode {
            // Nothing open yet: the start page takes the whole window, in
            // every view that needs a disc to show anything (a view restored
            // from the last session used to open on a bare "open a disc
            // first" line with no way to do it).
            Mode::Browse | Mode::Actors | Mode::Media if self.studio.library.is_none() => {
                egui::CentralPanel::default().show(ctx, |ui| self.start_page(ui));
            }
            Mode::Browse => {
                egui::SidePanel::left("browser")
                    .resizable(true)
                    .default_width(430.0)
                    .min_width(300.0)
                    .show(ctx, |ui| self.browser(ui));
                egui::CentralPanel::default().show(ctx, |ui| self.preview(ui));
            }
            Mode::Actors => self.actors_mode(ctx),
            Mode::Media => self.media_mode(ctx),
            Mode::Extract => {
                egui::CentralPanel::default().show(ctx, |ui| self.extract_panel(ui));
            }
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.player.stop();
        let s = &mut self.studio.settings;
        s.mode = Some(self.mode.key().to_string());
        s.tab = Some(self.tab.key().to_string());
        if let Some(size) = self.window_size {
            s.window_size = Some(size);
        }
        if let Some(pos) = self.window_pos {
            s.window_pos = Some(pos);
        }
        self.studio.save_settings();
        if let Some(job) = &mut self.job {
            job.cancel();
            job.join();
        }
    }
}

/// `THIRD-PARTY-LICENSES.txt` where the release archives put it
/// (`docs/tooling/releases.md`): beside the binary, or beside the macOS
/// `.app` bundle the binary sits three folders inside.
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
        egui::Window::new("About Legaia Studio")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.heading(format!("Legaia Studio {}", env!("CARGO_PKG_VERSION")));
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
                ui.label(RichText::new("Keyboard").strong());
                egui::Grid::new("about_keys")
                    .num_columns(2)
                    .spacing([16.0, 3.0])
                    .show(ui, |ui| {
                        for (keys, what) in [
                            ("Ctrl+O", "open a disc image"),
                            ("Ctrl+1 .. Ctrl+4", "Browse, Characters, Movies, Extract"),
                            ("F5", "play the open disc"),
                            ("Ctrl+F, Esc", "search the archive, clear the search"),
                            (
                                "Up / Down, PgUp / PgDn, Home / End",
                                "walk the archive tree",
                            ),
                            (
                                "Ctrl+= / Ctrl+- / Ctrl+0",
                                "zoom in, out, reset (also View)",
                            ),
                            ("F1", "this box"),
                        ] {
                            ui.monospace(keys);
                            ui.label(what);
                            ui.end_row();
                        }
                    });
            });
        self.about_open = open;
    }
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
                let side = ((ui.available_width() - COLUMN) / 2.0).max(12.0);
                ui.horizontal(|ui| {
                    ui.add_space(side);
                    ui.vertical(|ui| {
                        ui.set_max_width(COLUMN);
                        ui.add_space(28.0);
                        ui.label(RichText::new("Legaia Studio").size(30.0).color(accent));
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
                                    "Your .bin (a raw Mode2/2352 dump) or its .cue. Ctrl+O",
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
                        if let Some(p) = self.studio.loading() {
                            ui.add_space(10.0);
                            ui.horizontal(|ui| {
                                ui.spinner();
                                ui.label(format!("Opening {} ...", p.display()));
                            });
                        }
                        if !self.studio.settings.recent.is_empty() {
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
                                        "Unpack the whole disc into a folder of PNG, WAV, \
                                         OBJ and glTF files.",
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
                                 pick. F1 lists the keyboard shortcuts.",
                            )
                            .weak(),
                        );
                    });
                });
            });
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

fn checkerboard(p: &egui::Painter, rect: egui::Rect) {
    p.rect_filled(rect, 0.0, Color32::from_gray(60));
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
                p.rect_filled(r, 0.0, Color32::from_gray(80));
            }
        }
    }
}

fn hex_view(ui: &mut egui::Ui, bytes: &[u8]) {
    let rows = bytes.len().div_ceil(16);
    let h = ui.text_style_height(&egui::TextStyle::Monospace);
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show_rows(ui, h, rows, |ui, range| {
            for r in range {
                let off = r * 16;
                let line = &bytes[off..(off + 16).min(bytes.len())];
                let mut s = format!("{off:08X}  ");
                for (i, b) in line.iter().enumerate() {
                    s.push_str(&format!("{b:02X} "));
                    if i == 7 {
                        s.push(' ');
                    }
                }
                for i in line.len()..16 {
                    s.push_str("   ");
                    if i == 7 {
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
                ui.monospace(s);
            }
        });
}
