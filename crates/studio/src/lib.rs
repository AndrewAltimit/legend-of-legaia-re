//! Legaia Studio: a desktop asset browser and extractor over the user's own
//! Legend of Legaia disc image.
//!
//! The crate splits into a UI-free model ([`Studio`] and the modules under
//! it, exercised headlessly by the tests) and the egui front end ([`app`]).
//! Every format is read through the existing library crates; the extraction
//! tab runs the same `legaia_extract::run` pipeline the `legaia-extract`
//! command line does. Nothing from the disc is bundled: the app reads the
//! image the user picks, at runtime.

pub mod actors;
pub mod analyze;
pub mod app;
pub mod export;
pub mod extract_job;
pub mod library;
pub mod player;
pub mod raster;
pub mod scene_vram;
pub mod settings;

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};

use anyhow::Result;
use legaia_disc_audit::categorize::Class;
use rayon::prelude::*;

pub use analyze::Analysis;
pub use library::{Library, Source};
pub use settings::Settings;

/// One CDNAME block in the browser tree: the entries that inherit its name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    pub name: String,
    /// Positions in [`Library::entries`].
    pub entries: Vec<usize>,
}

/// Group a library's entries under their CDNAME block labels (the labels
/// the extracted filenames carry), in disc order.
pub fn blocks(lib: &Library) -> Vec<Block> {
    let mut out: Vec<Block> = Vec::new();
    for (pos, e) in lib.entries.iter().enumerate() {
        let name = lib.label(e.index).unwrap_or("(unnamed)").to_string();
        match out.last_mut() {
            Some(b) if b.name == name => b.entries.push(pos),
            _ => out.push(Block {
                name,
                entries: vec![pos],
            }),
        }
    }
    out
}

/// The UI-free application state.
pub struct Studio {
    pub settings: Settings,
    settings_path: Option<PathBuf>,
    pub library: Option<Arc<Library>>,
    pub blocks: Vec<Block>,
    loading: Option<(PathBuf, Receiver<Result<Library, String>>)>,
    /// Per-entry class, filled in by a background pass after loading.
    pub classes: Arc<Mutex<Vec<Option<Class>>>>,
    pub selected: Option<usize>,
    pub analysis: Option<Arc<Analysis>>,
    analyzing: Option<(usize, Receiver<Analysis>)>,
    /// Last error worth showing the user.
    pub error: Option<String>,
    /// Last informational message (export written, ...).
    pub status: Option<String>,
}

impl Studio {
    /// A fresh state with settings from `settings_path` (or defaults).
    pub fn new(settings_path: Option<PathBuf>) -> Self {
        let settings = settings_path
            .as_deref()
            .map(Settings::load)
            .unwrap_or_default();
        Self {
            settings,
            settings_path,
            library: None,
            blocks: Vec::new(),
            loading: None,
            classes: Arc::new(Mutex::new(Vec::new())),
            selected: None,
            analysis: None,
            analyzing: None,
            error: None,
            status: None,
        }
    }

    pub fn save_settings(&self) {
        if let Some(p) = &self.settings_path
            && let Err(e) = self.settings.save(p)
        {
            log::warn!("saving settings to {}: {e}", p.display());
        }
    }

    /// The path being loaded, while a load runs.
    pub fn loading(&self) -> Option<&Path> {
        self.loading.as_ref().map(|(p, _)| p.as_path())
    }

    pub fn is_analyzing(&self) -> bool {
        self.analyzing.is_some()
    }

    /// Start loading `path` on a worker thread; [`Self::poll`] picks it up.
    /// Validation runs first, synchronously, so a wrong file is reported at
    /// once.
    pub fn open(&mut self, path: &Path) {
        if let Err(e) = library::validate(path) {
            self.error = Some(format!("{e:#}"));
            return;
        }
        let (tx, rx) = channel();
        let p = path.to_path_buf();
        std::thread::spawn(move || {
            let _ = tx.send(Library::open(&p).map_err(|e| format!("{e:#}")));
        });
        self.loading = Some((path.to_path_buf(), rx));
        self.error = None;
    }

    /// Load `path` on this thread (tests, scripted runs).
    pub fn open_blocking(&mut self, path: &Path) -> Result<()> {
        let lib = Library::open(path)?;
        self.install(path, lib);
        Ok(())
    }

    fn install(&mut self, path: &Path, lib: Library) {
        let lib = Arc::new(lib);
        self.blocks = blocks(&lib);
        self.selected = None;
        self.analysis = None;
        self.analyzing = None;
        self.settings.last_source = Some(path.to_path_buf());
        self.save_settings();
        let classes = Arc::new(Mutex::new(vec![None; lib.entries.len()]));
        self.classes = Arc::clone(&classes);
        let bg = Arc::clone(&lib);
        std::thread::spawn(move || {
            (0..bg.entries.len()).into_par_iter().for_each(|pos| {
                if let Ok(bytes) = bg.entry_bytes(pos) {
                    let c = legaia_disc_audit::categorize::classify(&bytes).class;
                    if let Some(slot) = classes
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .get_mut(pos)
                    {
                        *slot = Some(c);
                    }
                }
            });
        });
        self.library = Some(lib);
    }

    /// Select entry `pos` and analyse it on a worker thread.
    pub fn select(&mut self, pos: usize) {
        let Some(lib) = self.library.clone() else {
            return;
        };
        if self.selected == Some(pos) {
            return;
        }
        self.selected = Some(pos);
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let index = lib.entries[pos].index;
            let bytes = lib.entry_bytes(pos).unwrap_or_default();
            let _ = tx.send(analyze::analyze(pos, index, bytes));
        });
        self.analyzing = Some((pos, rx));
    }

    /// Select and analyse on this thread (tests, scripted runs).
    pub fn select_blocking(&mut self, pos: usize) -> Result<Arc<Analysis>> {
        let lib = self
            .library
            .clone()
            .ok_or_else(|| anyhow::anyhow!("nothing loaded"))?;
        let index = lib
            .entries
            .get(pos)
            .ok_or_else(|| anyhow::anyhow!("no entry {pos}"))?
            .index;
        let a = Arc::new(analyze::analyze(pos, index, lib.entry_bytes(pos)?));
        self.selected = Some(pos);
        self.analysis = Some(Arc::clone(&a));
        self.analyzing = None;
        Ok(a)
    }

    /// Pick up finished background work. Returns `true` when something
    /// changed (the UI repaints).
    pub fn poll(&mut self) -> bool {
        let mut changed = false;
        if let Some((path, rx)) = &self.loading
            && let Ok(res) = rx.try_recv()
        {
            let path = path.clone();
            self.loading = None;
            match res {
                Ok(lib) => self.install(&path, lib),
                Err(e) => self.error = Some(e),
            }
            changed = true;
        }
        if let Some((pos, rx)) = &self.analyzing
            && let Ok(a) = rx.try_recv()
        {
            if Some(*pos) == self.selected {
                self.analysis = Some(Arc::new(a));
            }
            self.analyzing = None;
            changed = true;
        }
        changed
    }

    /// The class of entry `pos`, once the background pass has reached it.
    pub fn class_of(&self, pos: usize) -> Option<Class> {
        self.classes
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(pos)
            .copied()
            .flatten()
    }

    /// How many entries the background pass has classified.
    pub fn classified(&self) -> usize {
        self.classes
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .filter(|c| c.is_some())
            .count()
    }
}
