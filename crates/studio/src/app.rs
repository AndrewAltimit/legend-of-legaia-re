//! The egui front end over [`Studio`].

use std::path::{Path, PathBuf};
use std::sync::Arc;

use eframe::egui::{self, Color32, ColorImage, RichText, TextureHandle, TextureOptions};

use crate::analyze::Analysis;
use crate::extract_job::{ExtractJob, Outcome};
use crate::player::Player;
use crate::raster::{self, Camera, Model, Shading};
use crate::{Source, Studio, export};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Browse,
    Extract,
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

    fn parse(s: &str) -> Option<Tab> {
        let s = s.to_ascii_lowercase();
        let key = |t: Tab| match t {
            Tab::Model => "model",
            other => other.name(),
        };
        Tab::ALL
            .into_iter()
            .find(|&t| key(t).eq_ignore_ascii_case(&s))
    }
}

/// Scripted-run hooks, read from the environment once at start-up. They let
/// a test (or a bug report) open a source, pick an entry and a tab, and save
/// a screenshot without anyone at the keyboard.
#[derive(Debug, Default, Clone)]
pub struct Automation {
    /// `LEGAIA_STUDIO_SELECT`: PROT index to select once loaded.
    pub select: Option<u32>,
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
}

impl Automation {
    pub fn from_env() -> Self {
        let path = |k: &str| std::env::var_os(k).map(PathBuf::from);
        Self {
            select: std::env::var("LEGAIA_STUDIO_SELECT")
                .ok()
                .and_then(|s| s.trim().parse().ok()),
            tab: std::env::var("LEGAIA_STUDIO_TAB").ok(),
            extract_to: path("LEGAIA_STUDIO_EXTRACT_TO"),
            screenshot: path("LEGAIA_STUDIO_SCREENSHOT"),
            export_dir: path("LEGAIA_STUDIO_EXPORT_DIR"),
        }
    }
}

struct ModelView {
    key: (usize, usize),
    model: Option<Model>,
    vram: legaia_tim::Vram,
    cam: Camera,
    shading: Shading,
    rendered: Option<(Camera, Shading, [usize; 2])>,
    tex: Option<TextureHandle>,
}

pub struct App {
    pub studio: Studio,
    mode: Mode,
    tab: Tab,
    path_text: String,
    filter: String,
    class_filter: Option<String>,
    // Textures tab.
    tim_sel: usize,
    clut: usize,
    tim_zoom: f32,
    tim_tex: Option<((usize, usize, usize), TextureHandle)>,
    // Model tab.
    tmd_sel: usize,
    model: Option<ModelView>,
    /// Entry the browser should expand and scroll to on its next frame.
    reveal: Option<usize>,
    // Audio tab.
    vab_sel: usize,
    seq_loop: bool,
    player: Player,
    // Extract mode.
    out_text: String,
    job: Option<ExtractJob>,
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
        let open = initial.or_else(|| studio.settings.last_source.clone().filter(|p| p.exists()));
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
        let mut app = Self {
            studio,
            mode: Mode::Browse,
            tab: Tab::Overview,
            path_text,
            filter: String::new(),
            class_filter: None,
            tim_sel: 0,
            clut: 0,
            tim_zoom: 2.0,
            tim_tex: None,
            tmd_sel: 0,
            model: None,
            reveal: None,
            vab_sel: 0,
            seq_loop: false,
            player: Player::default(),
            out_text,
            job: None,
            auto,
            auto_selected: false,
            auto_extract_started: false,
            settle_since: None,
            screenshot_requested: false,
        };
        if let Some(t) = app.auto.tab.clone() {
            if t.eq_ignore_ascii_case("extract") {
                app.mode = Mode::Extract;
            } else if let Some(tab) = Tab::parse(&t) {
                app.tab = tab;
            }
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
        self.studio.open(p);
    }

    fn select(&mut self, pos: usize) {
        if self.studio.selected != Some(pos) {
            self.tim_sel = 0;
            self.clut = 0;
            self.tmd_sel = 0;
            self.vab_sel = 0;
            self.tim_tex = None;
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
        let Some(shot) = self.auto.screenshot.clone() else {
            return;
        };
        let loaded = self.studio.library.is_some() && self.studio.loading().is_none();
        let analysed = self.auto.select.is_none() || self.studio.analysis.is_some();
        let extracted = self.auto.extract_to.is_none()
            || self.job.as_ref().is_some_and(|j| j.outcome().is_some());
        let idle = loaded && analysed && extracted && !self.studio.is_analyzing();
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
            verbose: false,
        };
        self.studio.save_settings();
        self.studio.error = None;
        self.job = Some(ExtractJob::start(options));
    }

    // --- Panels ------------------------------------------------------------

    fn top_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading("Legaia Studio");
            ui.separator();
            ui.selectable_value(&mut self.mode, Mode::Browse, "Browse");
            ui.selectable_value(&mut self.mode, Mode::Extract, "Extract");
            ui.separator();
            if ui.button("Open disc image...").clicked()
                && let Some(p) = rfd::FileDialog::new()
                    .set_title("Choose your Legend of Legaia disc image")
                    .add_filter("PlayStation disc image", &["bin", "cue", "BIN", "CUE"])
                    .pick_file()
            {
                self.open_path(&p);
            }
            if ui.button("Open extracted folder...").clicked()
                && let Some(p) = rfd::FileDialog::new()
                    .set_title("Choose a legaia-extract output folder")
                    .pick_folder()
            {
                self.open_path(&p);
            }
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
        if let Some(e) = &self.studio.error {
            ui.colored_label(Color32::from_rgb(230, 110, 100), e);
        } else if let Some(s) = &self.studio.status {
            ui.colored_label(Color32::from_rgb(140, 200, 140), s);
        }
    }

    fn browser(&mut self, ui: &mut egui::Ui) {
        let Some(lib) = self.studio.library.clone() else {
            ui.label("Open a disc image to browse its PROT.DAT archive.");
            return;
        };
        ui.horizontal(|ui| {
            ui.label("Filter:");
            ui.add(egui::TextEdit::singleline(&mut self.filter).hint_text("name or index"));
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
        let filter = self.filter.to_ascii_lowercase();
        let mut clicked = None;
        let mut iso_save: Option<crate::library::IsoFile> = None;
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
                            let e = &lib.entries[pos];
                            let text_ok = filter.is_empty()
                                || block.name.to_ascii_lowercase().contains(&filter)
                                || format!("{:04}", e.index).contains(&filter);
                            let class_ok = match &self.class_filter {
                                None => true,
                                Some(c) => {
                                    self.studio.class_of(pos).map(|k| k.name()) == Some(c.as_str())
                                }
                            };
                            text_ok && class_ok
                        })
                        .collect();
                    if rows.is_empty() {
                        continue;
                    }
                    let first = lib.entries[rows[0]].index;
                    let open = (!filter.is_empty() || self.class_filter.is_some())
                        .then_some(true)
                        .or_else(|| {
                            self.reveal
                                .filter(|s| block.entries.contains(s))
                                .map(|_| true)
                        });
                    egui::CollapsingHeader::new(format!(
                        "{first:04}  {}  ({})",
                        block.name,
                        rows.len()
                    ))
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
                            let resp = ui.selectable_label(sel, RichText::new(text).monospace());
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
            welcome(ui);
            return;
        };
        let Some(pos) = self.studio.selected else {
            ui.label("Select a PROT entry on the left.");
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
        ui.horizontal(|ui| {
            for t in Tab::ALL {
                let n = match t {
                    Tab::Textures => Some(a.tims.len()),
                    Tab::Model => Some(a.tmds.len()),
                    Tab::Audio => Some(a.vabs.len() + a.seqs.len()),
                    Tab::Text => Some(a.mes.len() + a.dialog.len()),
                    _ => None,
                };
                let label = match n {
                    Some(n) => format!("{} ({n})", t.name()),
                    None => t.name().to_string(),
                };
                let enabled = n.is_none_or(|n| n > 0);
                ui.add_enabled_ui(enabled, |ui| ui.selectable_value(&mut self.tab, t, label));
            }
        });
        ui.separator();
        match self.tab {
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
            .default_width(230.0)
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
        ui.horizontal(|ui| {
            if t.palettes > 1 {
                ui.label("CLUT:");
                ui.add(egui::Slider::new(&mut self.clut, 0..=t.palettes - 1));
            } else if t.palettes == 1 {
                ui.label("1 CLUT");
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
        if self.tim_tex.as_ref().map(|(k, _)| *k) != Some(key) {
            self.tim_tex = match export::tim_rgba(a, &t, self.clut) {
                Ok((w, h, rgba)) if w > 0 && h > 0 => {
                    let img = ColorImage::from_rgba_unmultiplied([w, h], &rgba);
                    Some((
                        key,
                        ui.ctx().load_texture("tim", img, TextureOptions::NEAREST),
                    ))
                }
                Ok(_) => None,
                Err(e) => {
                    ui.colored_label(Color32::LIGHT_RED, format!("{e:#}"));
                    None
                }
            };
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

    fn model_tab(&mut self, ui: &mut egui::Ui, lib: &crate::Library, a: &Analysis) {
        if a.tmds.is_empty() {
            ui.label("No TMD meshes in this entry.");
            return;
        }
        self.tmd_sel = self.tmd_sel.min(a.tmds.len() - 1);
        let t = a.tmds[self.tmd_sel].clone();
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
                                if t.scene_stream { " (scene stream)" } else { "" }
                            ),
                        );
                    }
                });
            let key = (a.pos, self.tmd_sel);
            if self.model.as_ref().map(|m| m.key) != Some(key) {
                let has_tex = !a.tims.is_empty();
                self.model = Some(ModelView {
                    key,
                    model: Model::from_tmd(a.tmd_bytes(&t)),
                    vram: a.vram(),
                    cam: Camera::default(),
                    shading: if has_tex { Shading::Textured } else { Shading::Lit },
                    rendered: None,
                    tex: None,
                });
            }
            if let Some(mv) = &mut self.model {
                ui.selectable_value(&mut mv.shading, Shading::Textured, "Textured (retail shading)")
                    .on_hover_text("texel x colour / 128 from this entry's own TIMs; textures living in other entries show as gaps");
                ui.selectable_value(&mut mv.shading, Shading::Lit, "Untextured + viewer light")
                    .on_hover_text("A viewer aid with a fixed light - not how retail shades");
                if ui.button("Reset view").clicked() {
                    mv.cam = Camera::default();
                }
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
                if let Some(out) = self.export_target(&name, "glb") {
                    let res = export::tmd_glb(a, &t, &out);
                    self.report("glTF", res, &out);
                }
            }
        });
        ui.label(RichText::new("Drag to orbit, scroll to zoom.").weak());
        let Some(mv) = &mut self.model else { return };
        let Some(model) = &mv.model else {
            ui.colored_label(Color32::LIGHT_RED, "This TMD has no drawable triangles.");
            return;
        };
        let avail = ui.available_size();
        let (rect, resp) = ui.allocate_exact_size(avail, egui::Sense::drag());
        if resp.dragged() {
            let d = resp.drag_delta();
            mv.cam.yaw += d.x * 0.01;
            mv.cam.pitch = (mv.cam.pitch - d.y * 0.01).clamp(-1.5, 1.5);
        }
        if resp.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll != 0.0 {
                mv.cam.zoom = (mv.cam.zoom * (1.0 - scroll * 0.002)).clamp(0.1, 10.0);
            }
        }
        let ppp = ui.ctx().pixels_per_point();
        let px = [
            ((rect.width() * ppp) as usize).clamp(16, 1600),
            ((rect.height() * ppp) as usize).clamp(16, 1200),
        ];
        let state = (mv.cam, mv.shading, px);
        if mv.rendered != Some(state) || mv.tex.is_none() {
            let rgba = raster::render(model, &mv.vram, mv.cam, mv.shading, px[0], px[1]);
            let img = ColorImage::from_rgba_unmultiplied(px, &rgba);
            match &mut mv.tex {
                Some(t) => t.set(img, TextureOptions::LINEAR),
                None => mv.tex = Some(ui.ctx().load_texture("tmd", img, TextureOptions::LINEAR)),
            }
            mv.rendered = Some(state);
        }
        if let Some(tex) = &mv.tex {
            ui.painter().image(
                tex.id(),
                rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        }
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
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
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
                egui::Grid::new("dialog").striped(true).num_columns(2).show(ui, |ui| {
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
                    ui.label(RichText::new(format!("MES {k}: {} layout, {place}, 0x{:X}", m.format, m.offset)).strong());
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
                    ui.label(format!(
                        "{} record boundaries; per-message text is decoded for the Compact layout only.",
                        m.records
                    ));
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
        egui::Grid::new("extract_opts")
            .num_columns(2)
            .show(ui, |ui| {
                ui.label("Disc image");
                match &disc {
                    Some(p) => ui.monospace(p.display().to_string()),
                    None => ui.colored_label(
                        Color32::LIGHT_RED,
                        "open a disc image first (Browse > Open disc image)",
                    ),
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
                    });
                });
                ui.end_row();
            });
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    !running && disc.is_some(),
                    egui::Button::new("Start extraction"),
                )
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
        let p = job.progress();
        let outcome = job.outcome();
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
                "Done: {} disc files, {} PROT entries, {} streams, {} PNGs, {} WAVs",
                s.iso_files, s.prot_entries, s.streams, s.pngs, s.wavs
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

        egui::TopBottomPanel::top("top").show(ctx, |ui| self.top_bar(ui));
        match self.mode {
            Mode::Browse => {
                egui::SidePanel::left("browser")
                    .resizable(true)
                    .default_width(430.0)
                    .min_width(300.0)
                    .show(ctx, |ui| self.browser(ui));
                egui::CentralPanel::default().show(ctx, |ui| self.preview(ui));
            }
            Mode::Extract => {
                egui::CentralPanel::default().show(ctx, |ui| self.extract_panel(ui));
            }
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.player.stop();
        self.studio.save_settings();
        if let Some(job) = &mut self.job {
            job.cancel();
            job.join();
        }
    }
}

fn welcome(ui: &mut egui::Ui) {
    ui.add_space(20.0);
    ui.heading("Welcome to Legaia Studio");
    ui.label("Browse, preview and extract the assets on your own Legend of Legaia disc.");
    ui.add_space(8.0);
    ui.label("1. Open your disc image (the .bin of a Mode2/2352 dump, or its .cue) with the button above, or drop it on this window.");
    ui.label("2. Browse the PROT.DAT archive on the left; previews show textures, 3D models, audio and dialog text.");
    ui.label(
        "3. Export what you select, or switch to Extract to unpack the whole disc into a folder.",
    );
    ui.add_space(8.0);
    ui.label(
        RichText::new("No game data ships with this program; it reads only the disc you supply.")
            .weak(),
    );
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
