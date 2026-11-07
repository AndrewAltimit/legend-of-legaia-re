//! Legaia Explorer: a desktop asset browser and extractor over the user's own
//! Legend of Legaia disc image.
//!
//! The crate splits into a UI-free model ([`Explorer`] and the modules under
//! it, exercised headlessly by the tests) and the egui front end ([`app`]).
//! Every format is read through the existing library crates; the extraction
//! tab runs the same `legaia_extract::run` pipeline the `legaia-extract`
//! command line does. Nothing from the disc is bundled: the app reads the
//! image the user picks, at runtime.

pub mod actors;
pub mod analyze;
pub mod app;
pub mod arts;
pub mod export;
pub mod extract_job;
pub mod launch;
pub mod library;
pub mod media;
pub mod placement;
pub mod player;
pub mod raster;
pub mod scene_vram;
pub mod settings;
pub mod theme;

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

/// The kinds the browse list names and its filter offers, in menu order.
pub const KINDS: [&str; 12] = [
    "Scene textures and models",
    "3D models",
    "Textures",
    "Sound and music",
    "Animation",
    "Characters",
    "Effects",
    "Maps and scripts",
    "Game code",
    "Packed data",
    "Empty or filler",
    "Unidentified",
];

/// Which of several dropped paths the window opens: the first that can be
/// a source (a disc image, a `.cue`, an extracted folder) rather than an
/// expansion pack or a memory card ([`library::not_a_source`]), else the
/// first, so its own message says what it is. A `.bin` dropped with its
/// `.cue` opens the `.cue`, which names the `.bin`.
pub fn pick_drop(dropped: &[PathBuf]) -> Option<&PathBuf> {
    let is_cue = |p: &PathBuf| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("cue"));
    let sources: Vec<&PathBuf> = dropped
        .iter()
        .filter(|p| library::not_a_source(p).is_none())
        .collect();
    sources
        .iter()
        .copied()
        .find(|p| is_cue(p))
        .or(sources.first().copied())
        .or(dropped.first())
}

/// What a `legaia_disc_audit` category holds, in the words of someone who has
/// not read the format pages. A disc-wide classification by what the entry
/// mostly is: a scene's model stream carries textures too, which its tabs
/// count. The raw category stays on the row's hover and the Overview tab, and
/// the search finds either.
pub fn kind_of_class(class: &str) -> &'static str {
    match class {
        "scene_tmd_stream" | "tmd_size_prefix" | "stage_geometry" => "3D models",
        "tim_passthrough" | "tim_pack" => "Textures",
        "scene_vab_stream" | "seq_container" | "vab_multi_bank" | "monster_sound_bank"
        | "bse_bank" => "Sound and music",
        "anm_container" => "Animation",
        "battle_data_pack" => "Characters",
        "effect_bundle" | "efect_pack" | "summon_readef" => "Effects",
        // A scene's main bundle: where its textures live.
        "scene_asset_table" | "scene_scripted_asset_table" => "Scene textures and models",
        "field_map" | "scene_v12_table" | "scene_event_scripts" => "Maps and scripts",
        "mips_overlay" | "overlay_ptr_table" | "overlay_data_blob" | "init_pak" => "Game code",
        "data_field_streaming" | "data_field_truncated" | "pack" | "lzs_container" => "Packed data",
        "empty" | "tiny" | "all_zeros" | "mostly_zeros" | "constant_byte" | "pochi_filler" => {
            "Empty or filler"
        }
        _ => "Unidentified",
    }
}

/// The display name of the scene a CDNAME label names (`town01` -> `Rim
/// Elm`), from the curated scene-name table (`legaia_gamedata`,
/// `docs/reference/scene-names.md`). `None` for a label the table does not
/// name, or one whose display name is the label itself.
pub fn place_name(label: &str) -> Option<&'static str> {
    arts::gamedata()
        .scene_by_id(label)
        .map(|s| s.name.as_str())
        .filter(|n| !n.eq_ignore_ascii_case(label))
}

/// Group a library's entries under the CDNAME block each one belongs to on
/// the disc (the retail block, [`Library::retail_block`]), in disc order.
///
/// Not the label the extracted filenames carry: those apply the `#define`
/// numbers as extraction indices, so every filename block starts two
/// entries late, and the browse tree would file the first two entries of a
/// block under the one before it - extraction 0874, the field character
/// pack, under `befect_data` instead of `player_data`
/// (`docs/formats/cdname.md`, numbering space).
pub fn blocks(lib: &Library) -> Vec<Block> {
    group_blocks(
        lib.entries
            .iter()
            .map(|e| lib.retail_block(e.index).unwrap_or("(unnamed)")),
    )
}

/// Runs of equal names, in order, as blocks of positions.
fn group_blocks<'a>(names: impl Iterator<Item = &'a str>) -> Vec<Block> {
    let mut out: Vec<Block> = Vec::new();
    for (pos, name) in names.enumerate() {
        let name = name.to_string();
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
pub struct Explorer {
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
    /// The last settings write failed and the banner has said so.
    settings_save_failed: bool,
    /// Last error worth showing the user.
    pub error: Option<String>,
    /// Last informational message (export written, ...).
    pub status: Option<String>,
}

impl Explorer {
    /// A fresh state with settings from `settings_path` (or defaults).
    pub fn new(settings_path: Option<PathBuf>) -> Self {
        let opened = settings_path.as_deref().map(Settings::open);
        let notice = opened.as_ref().and_then(|o| o.notice.clone());
        let settings_path = settings_path.filter(|_| opened.as_ref().is_none_or(|o| o.keep_file));
        let settings = opened.map(|o| o.settings).unwrap_or_default();
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
            settings_save_failed: false,
            error: notice,
            status: None,
        }
    }

    /// Write the settings. A failure goes on the error banner, once: the
    /// recent list, the theme and the zoom would otherwise be quietly gone at
    /// the next start.
    pub fn save_settings(&mut self) {
        let Some(p) = &self.settings_path else {
            return;
        };
        match self.settings.save(p) {
            Ok(()) => self.settings_save_failed = false,
            Err(e) => {
                log::warn!("saving settings to {}: {e}", p.display());
                if !self.settings_save_failed {
                    self.settings_save_failed = true;
                    self.error = Some(format!(
                        "Could not save your settings to {}: {e}. Your recent list, theme and \
                         zoom will not carry over; check that folder can be written.",
                        p.display()
                    ));
                }
            }
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
        self.settings.remember_recent(path);
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
            let _ = tx.send(analyze::analyze(pos, index, bytes, lib.text_codec()));
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
        let a = Arc::new(analyze::analyze(
            pos,
            index,
            lib.entry_bytes(pos)?,
            lib.text_codec(),
        ));
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

    /// Whether entry `pos` (in a block labelled `block`) passes the browser's
    /// search. Every whitespace-separated term of `query` must match, case
    /// blind, one of: the retail CDNAME block the tree groups it under, the
    /// label the filenames carry (the two differ by the +2 index shift), the entry's
    /// category, its PROT index (`865`, `0865` or `0x361`), or the display
    /// name of the scene its block names ([`place_name`]). `class`, when
    /// set, also requires that exact category.
    pub fn entry_matches(&self, pos: usize, block: &str, query: &str, class: Option<&str>) -> bool {
        let Some(lib) = &self.library else {
            return false;
        };
        let Some(e) = lib.entries.get(pos) else {
            return false;
        };
        let cat = self.class_of(pos).map(|c| c.name());
        // The filter holds a plain kind ("3D models") or a raw category.
        if class.is_some_and(|c| cat != Some(c) && cat.map(kind_of_class) != Some(c)) {
            return false;
        }
        // `block` is the retail block the tree groups under; the filename
        // label (two entries later in the define space) still finds it.
        let label = lib.label(e.index).unwrap_or("");
        let index_texts = [
            e.index.to_string(),
            format!("{:04}", e.index),
            format!("0x{:x}", e.index),
        ];
        let place = place_name(block).unwrap_or("").to_lowercase();
        query.split_whitespace().all(|term| {
            let t = term.to_lowercase();
            block.to_ascii_lowercase().contains(&t)
                || cat.is_some_and(|c| kind_of_class(c).to_lowercase().contains(&t))
                || place.contains(&t)
                || label.to_ascii_lowercase().contains(&t)
                || cat.is_some_and(|c| c.contains(&t))
                || index_texts.iter().any(|s| s.contains(&t))
        })
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
