//! Engine core primitives: virtual filesystem, asset cache, frame time, and
//! the composite [`world::World`] that wires the per-VM hosts from
//! `legaia-engine-vm` into a single runtime.
//!
//! Engine-agnostic. No wgpu / windowing / audio dependencies - the asset
//! crates talk to this layer, the render and audio crates read from it.

pub mod actor_alloc_host;
pub mod ap_gauge;
pub mod art_strike;
pub mod battle_events;
pub mod battle_hud;
pub mod battle_round;
pub mod battle_runner;
pub mod battle_session;
pub mod battle_stats;
pub mod camera;
pub mod capture_observations;
pub mod cd_dma;
pub mod cheat_applier;
pub mod cutscene;
pub mod dialog;
pub mod encounter;
pub mod encounter_record;
pub mod encounter_registry;
pub mod equip_session;
pub mod equipment;
pub mod field_events;
pub mod field_menu;
pub mod field_menu_dispatch;
pub mod game_over;
pub mod inn;
pub mod input;
pub mod inventory_use;
pub mod items;
pub mod key_rebind;
pub mod levelup;
pub mod menu_glyph_atlas;
pub mod menu_runtime;
pub mod mode;
pub mod monster_catalog;
pub mod move_buffer_host;
pub mod options;
pub mod publisher_logos;
pub mod ram_map;
pub mod save_menu_atlas;
pub mod save_select;
pub mod scene;
pub mod scene_assets;
pub mod scene_bundle;
pub mod scene_resources;
pub mod seru_learning;
pub mod seru_stats;
pub mod shop;
pub mod spell_menu;
pub mod spells;
pub mod status_screen;
pub mod tactical_arts;
pub mod tactical_arts_editor;
pub mod target_picker;
pub mod title;
pub mod title_screen_atlas;
pub mod world;
pub mod world_map;

use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
#[cfg(not(target_arch = "wasm32"))]
use std::time::Instant;

/// Source of asset bytes.
///
/// Two backends planned: an extracted-directory backend (for development -
/// reads from `extracted/` produced by `legaia-extract`) and a disc-backed
/// backend (for end users - reads directly from a disc image).
///
/// Both yield raw bytes addressed by a logical name (e.g.
/// `"prot/0123_some_entry.bin"`). The asset crates above this layer turn
/// bytes into typed structures.
pub trait Vfs: Send + Sync {
    fn read(&self, name: &str) -> Result<Vec<u8>>;
    fn list(&self, prefix: &str) -> Result<Vec<String>>;
    fn exists(&self, name: &str) -> bool;
}

/// Filesystem-backed Vfs rooted at a directory (e.g. `extracted/`).
pub struct DirVfs {
    root: PathBuf,
}

impl DirVfs {
    pub fn new(root: impl Into<PathBuf>) -> Result<Self> {
        let root = root.into();
        if !root.is_dir() {
            anyhow::bail!("DirVfs root is not a directory: {}", root.display());
        }
        Ok(Self { root })
    }
}

impl Vfs for DirVfs {
    fn read(&self, name: &str) -> Result<Vec<u8>> {
        let p = self.root.join(name);
        std::fs::read(&p).with_context(|| format!("read {}", p.display()))
    }

    fn list(&self, prefix: &str) -> Result<Vec<String>> {
        let dir = self.root.join(prefix);
        let mut out = Vec::new();
        if !dir.is_dir() {
            return Ok(out);
        }
        for ent in std::fs::read_dir(&dir).with_context(|| format!("list {}", dir.display()))? {
            let ent = ent?;
            let rel = ent
                .path()
                .strip_prefix(&self.root)
                .unwrap_or(Path::new(""))
                .to_string_lossy()
                .into_owned();
            out.push(rel);
        }
        out.sort();
        Ok(out)
    }

    fn exists(&self, name: &str) -> bool {
        self.root.join(name).exists()
    }
}

/// Vfs backed by a PSX `.bin` disc image. Reads files directly from the
/// ISO9660 filesystem using `legaia-iso`.
///
/// Names are normalised to forward-slash, case-insensitive. Both
/// `"PROT.DAT"` and `"prot.dat"` resolve to the same entry.
///
/// `legaia-iso` is `cfg(not(target_arch = "wasm32"))` only - DiscVfs is
/// only available on native targets. WASM builds keep `MemoryVfs`.
#[cfg(not(target_arch = "wasm32"))]
pub struct DiscVfs {
    raw: Mutex<legaia_iso::raw::RawDisc>,
    /// Normalised lowercase forward-slash path → directory record.
    files: HashMap<String, legaia_iso::iso9660::DirectoryRecord>,
}

#[cfg(not(target_arch = "wasm32"))]
impl DiscVfs {
    /// Open a `.bin` disc image and walk its ISO9660 tree once.
    ///
    /// Subsequent reads are O(1) lookups into the file map plus a sector
    /// fetch from the disc.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let mut raw = legaia_iso::raw::RawDisc::open(path.as_ref())
            .with_context(|| format!("open disc image {}", path.as_ref().display()))?;
        let volume = legaia_iso::iso9660::read_volume(&mut raw).context("read ISO9660 volume")?;
        let walked =
            legaia_iso::iso9660::walk_files(&mut raw, &volume.root).context("walk ISO9660 tree")?;
        let mut files = HashMap::with_capacity(walked.len());
        for (path_in_iso, rec) in walked {
            let key = normalise_disc_name(&path_in_iso);
            files.insert(key, rec);
        }
        Ok(Self {
            raw: Mutex::new(raw),
            files,
        })
    }

    /// Number of files indexed. For a retail Legaia disc this is in the
    /// low hundreds (mostly inside `DATA/`).
    pub fn file_count(&self) -> usize {
        self.files.len()
    }

    /// Iterator over the indexed file paths in arbitrary order.
    pub fn iter_paths(&self) -> impl Iterator<Item = &String> {
        self.files.keys()
    }

    /// Read raw bytes for a logical file name.
    fn read_record(&self, rec: &legaia_iso::iso9660::DirectoryRecord) -> Result<Vec<u8>> {
        let sector_count = rec.size.div_ceil(legaia_iso::raw::USER_DATA_SIZE as u32);
        let mut buf = Vec::with_capacity(rec.size as usize);
        self.raw
            .lock()
            .unwrap()
            .read_user_data(rec.lba, sector_count, &mut buf)
            .with_context(|| format!("read disc file {}", rec.name))?;
        buf.truncate(rec.size as usize);
        Ok(buf)
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Vfs for DiscVfs {
    fn read(&self, name: &str) -> Result<Vec<u8>> {
        let key = normalise_disc_name(name);
        let rec = self
            .files
            .get(&key)
            .ok_or_else(|| anyhow::anyhow!("DiscVfs: '{}' not found in ISO9660 tree", name))?
            .clone();
        self.read_record(&rec)
    }

    fn list(&self, prefix: &str) -> Result<Vec<String>> {
        let key_prefix = normalise_disc_name(prefix);
        let mut out: Vec<String> = self
            .files
            .keys()
            .filter(|k| k.starts_with(&key_prefix))
            .cloned()
            .collect();
        out.sort();
        Ok(out)
    }

    fn exists(&self, name: &str) -> bool {
        self.files.contains_key(&normalise_disc_name(name))
    }
}

/// Normalise a disc-relative path: backslashes → forward slashes,
/// lowercased, leading slash stripped.
#[cfg(not(target_arch = "wasm32"))]
fn normalise_disc_name(name: &str) -> String {
    let mut s: String = name
        .chars()
        .map(|c| if c == '\\' { '/' } else { c })
        .collect();
    s = s.to_ascii_lowercase();
    while let Some(stripped) = s.strip_prefix('/') {
        s = stripped.to_string();
    }
    s
}

/// In-memory Vfs backed by a `HashMap`. Useful for tests and WASM targets
/// where no real filesystem is available.
pub struct MemoryVfs {
    files: std::collections::HashMap<String, Vec<u8>>,
}

impl MemoryVfs {
    pub fn new() -> Self {
        Self {
            files: std::collections::HashMap::new(),
        }
    }

    pub fn insert(&mut self, name: impl Into<String>, bytes: Vec<u8>) {
        self.files.insert(name.into(), bytes);
    }
}

impl Default for MemoryVfs {
    fn default() -> Self {
        Self::new()
    }
}

impl Vfs for MemoryVfs {
    fn read(&self, name: &str) -> Result<Vec<u8>> {
        self.files
            .get(name)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("MemoryVfs: '{}' not found", name))
    }

    fn list(&self, prefix: &str) -> Result<Vec<String>> {
        let mut out: Vec<String> = self
            .files
            .keys()
            .filter(|k| k.starts_with(prefix))
            .cloned()
            .collect();
        out.sort();
        Ok(out)
    }

    fn exists(&self, name: &str) -> bool {
        self.files.contains_key(name)
    }
}

/// Trivial bytes cache keyed by Vfs name.
///
/// Lives behind a Mutex so it can be shared across loader threads later. The
/// API is intentionally narrow - a real engine would need eviction policy,
/// per-asset-type typed caches, and pinning. We add those when we need them.
pub struct AssetCache {
    inner: Mutex<HashMap<String, Arc<Vec<u8>>>>,
}

impl AssetCache {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
        }
    }

    pub fn get_or_load(&self, vfs: &dyn Vfs, name: &str) -> Result<Arc<Vec<u8>>> {
        if let Some(b) = self.inner.lock().unwrap().get(name).cloned() {
            return Ok(b);
        }
        let bytes = Arc::new(vfs.read(name)?);
        self.inner
            .lock()
            .unwrap()
            .insert(name.to_string(), bytes.clone());
        Ok(bytes)
    }
}

impl Default for AssetCache {
    fn default() -> Self {
        Self::new()
    }
}

/// Wall-clock + delta accumulator. Used by the frame loop to drive
/// fixed-timestep gameplay updates while letting render run uncapped.
///
/// On `wasm32-unknown-unknown` `std::time::Instant` is not implemented, so
/// this type becomes a zero-size stub - callers (JS `requestAnimationFrame`
/// loop) supply their own delta timing.
pub struct FrameTime {
    #[cfg(not(target_arch = "wasm32"))]
    started_at: Instant,
    #[cfg(not(target_arch = "wasm32"))]
    last_frame: Instant,
}

impl FrameTime {
    pub fn new() -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let now = Instant::now();
            Self {
                started_at: now,
                last_frame: now,
            }
        }
        #[cfg(target_arch = "wasm32")]
        Self {}
    }

    pub fn tick(&mut self) -> Duration {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let now = Instant::now();
            let dt = now - self.last_frame;
            self.last_frame = now;
            dt
        }
        #[cfg(target_arch = "wasm32")]
        Duration::ZERO
    }

    pub fn elapsed(&self) -> Duration {
        #[cfg(not(target_arch = "wasm32"))]
        {
            Instant::now() - self.started_at
        }
        #[cfg(target_arch = "wasm32")]
        Duration::ZERO
    }
}

impl Default for FrameTime {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_time_starts_at_zero() {
        let ft = FrameTime::new();
        assert!(ft.elapsed() < Duration::from_millis(50));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn disc_vfs_name_normalisation() {
        // Mixed case + backslashes + leading slash → lowercase forward-slash
        // form. Disc paths come from the ISO9660 walker which uses
        // forward slashes already, but user input via --disc-relative
        // names may use the retail-style backslashes.
        assert_eq!(super::normalise_disc_name("PROT.DAT"), "prot.dat");
        assert_eq!(super::normalise_disc_name("/PROT.DAT"), "prot.dat");
        assert_eq!(
            super::normalise_disc_name("DATA\\FIELD\\TOWN01\\STAGE.LZS"),
            "data/field/town01/stage.lzs"
        );
        assert_eq!(
            super::normalise_disc_name("data/cdname.txt"),
            "data/cdname.txt"
        );
    }
}
