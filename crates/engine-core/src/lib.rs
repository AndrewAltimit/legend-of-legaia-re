//! Engine core primitives: virtual filesystem, asset cache, frame time, and
//! the composite [`world::World`] that wires the per-VM hosts from
//! `legaia-engine-vm` into a single runtime.
//!
//! Engine-agnostic. No wgpu / windowing / audio dependencies — the asset
//! crates talk to this layer, the render and audio crates read from it.

pub mod world;

use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Source of asset bytes.
///
/// Two backends planned: an extracted-directory backend (for development —
/// reads from `extracted/` produced by `legaia-extract`) and a disc-backed
/// backend (for end users — reads directly from a disc image).
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

/// Trivial bytes cache keyed by Vfs name.
///
/// Lives behind a Mutex so it can be shared across loader threads later. The
/// API is intentionally narrow — a real engine would need eviction policy,
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
pub struct FrameTime {
    started_at: Instant,
    last_frame: Instant,
}

impl FrameTime {
    pub fn new() -> Self {
        let now = Instant::now();
        Self {
            started_at: now,
            last_frame: now,
        }
    }

    pub fn tick(&mut self) -> Duration {
        let now = Instant::now();
        let dt = now - self.last_frame;
        self.last_frame = now;
        dt
    }

    pub fn elapsed(&self) -> Duration {
        Instant::now() - self.started_at
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
}
