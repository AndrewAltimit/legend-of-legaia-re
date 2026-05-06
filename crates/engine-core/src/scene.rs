//! Scene-loading shell: PROT-resident asset indexing + per-CDNAME-block
//! bundle resolution + BGM lookup that mirrors the runtime field-VM
//! `0x35` opcode chain in [`docs/subsystems/script-vm.md`].
//!
//! Built on top of `legaia-prot` (TOC walker + CDNAME map) and the asset
//! categorizer (`legaia-asset::categorize`). Engines call:
//!
//! 1. [`ProtIndex::open_extracted`] once at startup.
//! 2. [`Scene::load`] when the scene name changes (resolves the CDNAME block
//!    and lazy-classifies every entry).
//! 3. [`Scene::find_bgm`] to resolve a BGM ID inside the active scene to a
//!    PROT entry (the BGM is the SEQ-shaped streaming container).
//!
//! See [`docs/subsystems/asset-loader.md`] for the per-mode layout the
//! retail engine uses.
//!
//! No Sony bytes — this is plumbing over the format crates.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use legaia_asset::categorize::{Class, classify};
use legaia_prot::archive::{Archive, Entry};
use legaia_prot::cdname;

/// Index over PROT.DAT + CDNAME.TXT. Built once and shared for the whole
/// scene-host's lifetime. Thread-safe — the underlying file handle and the
/// caches are guarded by Mutexes.
pub struct ProtIndex {
    /// PROT archive (file handle + TOC). The handle needs `&mut` to seek/read,
    /// so we keep it in a Mutex behind the index.
    archive: Mutex<Archive>,
    /// Snapshot of the entry table — kept outside the Mutex so callers can
    /// inspect it (length, sizes, byte offsets) without locking.
    entries: Vec<Entry>,
    /// Optional CDNAME map (PROT index → first scene label in block).
    cdname: Option<cdname::IndexMap>,
    /// Lazy entry-bytes cache. Populated on first `entry_bytes` call.
    entry_cache: Mutex<HashMap<u32, Arc<Vec<u8>>>>,
    /// Lazy classification cache. Populated on first `class_of` call.
    class_cache: Mutex<HashMap<u32, Class>>,
}

impl ProtIndex {
    /// Open an extracted directory tree (`PROT.DAT` + `CDNAME.TXT`).
    /// Mirrors the layout the `legaia-extract` pipeline produces.
    pub fn open_extracted(extracted_root: &Path) -> Result<Self> {
        let prot_path = extracted_root.join("PROT.DAT");
        let archive =
            Archive::open(&prot_path).with_context(|| format!("open {}", prot_path.display()))?;
        let entries = archive.entries.clone();
        let cdname_path = extracted_root.join("CDNAME.TXT");
        let cdname = if cdname_path.exists() {
            Some(
                cdname::parse(&cdname_path)
                    .with_context(|| format!("parse {}", cdname_path.display()))?,
            )
        } else {
            None
        };
        Ok(Self {
            archive: Mutex::new(archive),
            entries,
            cdname,
            entry_cache: Mutex::new(HashMap::new()),
            class_cache: Mutex::new(HashMap::new()),
        })
    }

    /// Total PROT entry count (typically 1232 in retail).
    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    /// Snapshot of the parsed entry table (size, byte_offset, etc).
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// Read entry bytes (lazy + cached). Returns the same `Arc` for repeated
    /// reads of the same index.
    pub fn entry_bytes(&self, idx: u32) -> Result<Arc<Vec<u8>>> {
        if let Some(b) = self.entry_cache.lock().unwrap().get(&idx).cloned() {
            return Ok(b);
        }
        let entry = self
            .entries
            .get(idx as usize)
            .ok_or_else(|| anyhow::anyhow!("PROT index {} out of range", idx))?
            .clone();
        let mut bytes = Vec::new();
        self.archive
            .lock()
            .unwrap()
            .read_entry(&entry, &mut bytes)
            .with_context(|| format!("read PROT entry {}", idx))?;
        let arc = Arc::new(bytes);
        self.entry_cache.lock().unwrap().insert(idx, arc.clone());
        Ok(arc)
    }

    /// Detected class of an entry (lazy + cached).
    pub fn class_of(&self, idx: u32) -> Result<Class> {
        if let Some(c) = self.class_cache.lock().unwrap().get(&idx).copied() {
            return Ok(c);
        }
        let bytes = self.entry_bytes(idx)?;
        let report = classify(&bytes);
        let class = report.class;
        self.class_cache.lock().unwrap().insert(idx, class);
        Ok(class)
    }

    /// Look up a CDNAME block range (`first_idx, end_idx`) by scene label.
    /// Returns `None` if no CDNAME map was loaded or the label isn't present.
    pub fn block_range(&self, scene_name: &str) -> Option<(u32, u32)> {
        let map = self.cdname.as_ref()?;
        cdname::block_range_for_name(map, scene_name)
    }

    /// First scene label whose block contains `idx`. Useful for diagnostics
    /// (e.g. "this BGM is part of which scene?").
    pub fn scene_for_index(&self, idx: u32) -> Option<&str> {
        let map = self.cdname.as_ref()?;
        cdname::block_for(map, idx)
    }
}

/// One PROT entry classified, with bytes ready. The format-typed parsers
/// (TMD / VAB / SEQ / etc.) live in their own crates; we keep the bytes
/// + class + index here and let the engine dispatch.
#[derive(Debug, Clone)]
pub struct SceneEntry {
    pub idx: u32,
    pub class: Class,
    pub bytes: Arc<Vec<u8>>,
}

impl SceneEntry {
    /// Parse this entry as a SEQ (PsyQ sequence). Errors if the bytes don't
    /// start with the `pQES` magic.
    pub fn as_seq(&self) -> Result<legaia_seq::Seq> {
        legaia_seq::Seq::parse(&self.bytes).context("parse SEQ from PROT entry bytes")
    }

    /// Parse a VAB header at `offset` (most common: 0 for standalone VAB,
    /// or 4 for `scene_vab_stream` containers — the chunk0 prefix is 4 bytes).
    pub fn as_vab(&self, offset: usize) -> Result<legaia_vab::VabReport> {
        legaia_vab::parse(&self.bytes, offset).context("parse VAB from PROT entry bytes")
    }
}

/// Per-scene event-script container — the field-VM bytecode bundle for a
/// scene, with each record's `(start, end)` byte range pre-walked. Returned
/// by [`Scene::find_event_scripts`].
///
/// Frame-divider note: many records open with the four-byte sentinel
/// `0xFFFF 0x0000` (the field VM's "frame divider"). [`record`] returns the
/// raw record bytes as-is; the VM-side helper
/// [`crate::world::World::load_field_record`] is responsible for skipping
/// the sentinel before dispatch.
#[derive(Debug)]
pub struct EventScripts<'a> {
    /// PROT index of the entry the records came from.
    pub entry_idx: u32,
    /// Backing bytes; record ranges index into this slice.
    pub bytes: &'a [u8],
    /// `(start, end)` byte ranges, one per record.
    pub record_ranges: Vec<(usize, usize)>,
}

impl<'a> EventScripts<'a> {
    /// Number of records in the prescript.
    pub fn len(&self) -> usize {
        self.record_ranges.len()
    }

    /// `true` if no records are present (caller should treat as "no field
    /// scripts" rather than panic).
    pub fn is_empty(&self) -> bool {
        self.record_ranges.is_empty()
    }

    /// Borrow record `i` as a slice. Returns `None` for out-of-range indices.
    pub fn record(&self, i: usize) -> Option<&'a [u8]> {
        let (s, e) = *self.record_ranges.get(i)?;
        self.bytes.get(s..e)
    }
}

/// A scene = the per-CDNAME-block bundle of PROT entries that the runtime
/// loads together. Mirrors the per-scene shape `FUN_8001f7c0` consumes.
pub struct Scene {
    pub name: String,
    pub start: u32,
    pub end: u32,
    /// Every entry in `start..end` with its class + bytes ready. Lazy: this
    /// is populated when `Scene::load` is called, but the entries
    /// themselves cache through `ProtIndex` so re-loading is cheap.
    pub entries: Vec<SceneEntry>,
}

impl Scene {
    /// Load every PROT entry in the named CDNAME block. Errors if the block
    /// isn't present.
    pub fn load(index: &ProtIndex, name: &str) -> Result<Self> {
        let (start, end) = index
            .block_range(name)
            .ok_or_else(|| anyhow::anyhow!("scene '{}' not found in CDNAME map", name))?;
        let mut entries = Vec::with_capacity((end - start) as usize);
        for idx in start..end {
            // Skip out-of-range indices defensively.
            if (idx as usize) >= index.entry_count() {
                break;
            }
            let bytes = index.entry_bytes(idx)?;
            let class = index.class_of(idx)?;
            entries.push(SceneEntry { idx, class, bytes });
        }
        Ok(Self {
            name: name.to_string(),
            start,
            end,
            entries,
        })
    }

    /// Resolve a BGM ID (the value the field VM's opcode `0x35` writes to
    /// `_DAT_8007BAC8`) to a scene-local entry.
    ///
    /// The retail resolver `FUN_800243F0` (see
    /// [`docs/subsystems/script-vm.md`] BGM lookup table) treats the slot
    /// at `block_start + 6 + id` as the per-scene BGM bank. IDs `>= 2000`
    /// resolve through the global BGM pool (not modeled here yet).
    pub fn find_bgm(&self, bgm_id: u16) -> Option<&SceneEntry> {
        if bgm_id >= 2000 {
            return None;
        }
        let target = self.start + 6 + bgm_id as u32;
        self.entries.iter().find(|e| e.idx == target)
    }

    /// Iterate every entry of `class` (in CDNAME order). Useful for sweeping
    /// every TMD / VAB in a scene without rerunning the classifier.
    pub fn entries_of(&self, class: Class) -> impl Iterator<Item = &SceneEntry> {
        self.entries.iter().filter(move |e| e.class == class)
    }

    /// Find the per-scene event-scripts container — either a standalone
    /// `SceneEventScripts` entry or the prescript prefix of a
    /// `SceneScriptedAssetTable` entry. The records inside are the field-VM
    /// (`FUN_801DE840`) per-event bytecode the scene runs on entry.
    ///
    /// Returns the first match in CDNAME order; most scenes carry exactly one
    /// such entry. Returns `None` if the scene has no event scripts (some
    /// title / cutscene-only scenes are pure asset bundles).
    pub fn find_event_scripts(&self) -> Option<EventScripts<'_>> {
        for entry in &self.entries {
            let ranges = match entry.class {
                Class::SceneEventScripts => {
                    legaia_asset::scene_event_scripts::record_ranges(&entry.bytes)
                }
                Class::SceneScriptedAssetTable => {
                    legaia_asset::scene_scripted_asset_table::record_ranges(&entry.bytes)
                }
                _ => None,
            };
            if let Some(ranges) = ranges
                && !ranges.is_empty()
            {
                return Some(EventScripts {
                    entry_idx: entry.idx,
                    bytes: &entry.bytes,
                    record_ranges: ranges,
                });
            }
        }
        None
    }

    /// Count of entries by class — tiny diagnostic for "what's in this scene".
    pub fn class_counts(&self) -> HashMap<Class, usize> {
        let mut out = HashMap::new();
        for e in &self.entries {
            *out.entry(e.class).or_insert(0) += 1;
        }
        out
    }
}

/// Resolver from a field-VM `scene_transition(map_id)` byte to a CDNAME
/// scene name. The retail engine reads this from a table in the field
/// overlay we haven't fully captured; engines wire their own table.
///
/// Implementors return `None` when the map id has no mapped scene
/// (the host then leaves the world in its current scene; the engine
/// can log the unknown id).
pub trait MapIdResolver {
    fn resolve(&self, map_id: u8) -> Option<String>;
}

/// Empty resolver — every `scene_transition` is a no-op. Useful for tests
/// + engines that haven't wired a real table yet.
#[derive(Debug, Default, Clone, Copy)]
pub struct NullMapIdResolver;

impl MapIdResolver for NullMapIdResolver {
    fn resolve(&self, _: u8) -> Option<String> {
        None
    }
}

/// Plain `Vec<String>`-backed resolver — index into a list of scene names
/// by map id. Useful for hardcoded test fixtures.
#[derive(Debug, Clone, Default)]
pub struct VecMapIdResolver {
    pub names: Vec<String>,
}

impl VecMapIdResolver {
    pub fn new(names: Vec<String>) -> Self {
        Self { names }
    }
}

impl MapIdResolver for VecMapIdResolver {
    fn resolve(&self, map_id: u8) -> Option<String> {
        self.names.get(map_id as usize).cloned()
    }
}

/// Per-tick outcome from [`SceneHost::tick`]. Engines route this back into
/// their UI layer (e.g. log scene transitions, update HUD on battle end).
#[derive(Debug, Clone)]
pub enum SceneTickEvent {
    /// World stepped normally — no scene-level events this frame.
    Stepped,
    /// Field VM requested a scene transition that the resolver mapped to
    /// `name`; the host loaded it and reset the field VM.
    SceneEntered { name: String },
    /// `scene_transition(map_id)` was requested but the resolver returned
    /// `None`. The host left the existing scene loaded; the engine can
    /// log / surface the unknown id.
    UnknownMapId { map_id: u8 },
}

/// Bundles the runtime composite (`World`) with a loaded `Scene`, a frame
/// timer, and a [`MapIdResolver`] for field-VM scene transitions. The host
/// owns the engine-vm world (per-actor data + every VM's `Host` impl) and
/// exposes a single `tick()` that drives the active VMs and processes any
/// transitions the field VM requested.
pub struct SceneHost {
    pub index: Arc<ProtIndex>,
    pub world: crate::world::World,
    pub scene: Option<Scene>,
    pub frame_time: crate::FrameTime,
    /// Map-id → scene-name resolver for `scene_transition(map_id)`.
    /// Default is [`NullMapIdResolver`] so transitions are silently
    /// dropped until the engine wires its own table.
    pub map_resolver: Box<dyn MapIdResolver + Send + Sync>,
}

impl SceneHost {
    /// Build a host over an already-opened ProtIndex.
    pub fn new(index: Arc<ProtIndex>) -> Self {
        Self {
            index,
            world: crate::world::World::default(),
            scene: None,
            frame_time: crate::FrameTime::new(),
            map_resolver: Box::new(NullMapIdResolver),
        }
    }

    /// Open the host directly from an extracted directory.
    pub fn open_extracted(extracted_root: impl AsRef<Path>) -> Result<Self> {
        let p = ProtIndex::open_extracted(extracted_root.as_ref())?;
        Ok(Self::new(Arc::new(p)))
    }

    /// Replace the map-id → scene-name resolver. Call once at startup with
    /// the engine's preferred resolver.
    pub fn set_map_resolver(&mut self, resolver: Box<dyn MapIdResolver + Send + Sync>) {
        self.map_resolver = resolver;
    }

    /// Load (or reload) the active scene without entering it. The world's
    /// `SceneMode` is left untouched. Use [`enter_field_scene`] if you want
    /// the field VM kicked off too.
    ///
    /// [`enter_field_scene`]: SceneHost::enter_field_scene
    pub fn load_scene(&mut self, name: &str) -> Result<&Scene> {
        let scene = Scene::load(&self.index, name)?;
        self.scene = Some(scene);
        Ok(self.scene.as_ref().unwrap())
    }

    /// Load `name`, switch the world to [`crate::world::SceneMode::Field`],
    /// and load the requested event-script record (default 0) into the
    /// field-VM bytecode buffer. Returns `Err` if the scene has no event
    /// scripts or the record index is out of range.
    pub fn enter_field_scene(&mut self, name: &str, record_index: usize) -> Result<()> {
        self.load_scene(name)?;
        let record_bytes: Vec<u8> = {
            let scene = self
                .scene
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("scene was not loaded"))?;
            let scripts = scene
                .find_event_scripts()
                .ok_or_else(|| anyhow::anyhow!("scene '{}' has no event scripts", name))?;
            let record = scripts.record(record_index).ok_or_else(|| {
                anyhow::anyhow!(
                    "record index {} out of range (scene has {} records)",
                    record_index,
                    scripts.len()
                )
            })?;
            record.to_vec()
        };
        self.world.mode = crate::world::SceneMode::Field;
        self.world.load_field_record(&record_bytes);
        // Drain any pending transition the previous scene left behind.
        self.world.pending_scene_transition = None;
        Ok(())
    }

    /// One frame: tick the world and then process any pending field-VM
    /// `scene_transition(map_id)` request. Returns the [`SceneTickEvent`]
    /// describing what happened.
    pub fn tick(&mut self) -> Result<SceneTickEvent> {
        let _ = self.world.tick();
        if let Some(map_id) = self.world.pending_scene_transition.take() {
            match self.map_resolver.resolve(map_id) {
                Some(name) => {
                    self.enter_field_scene(&name, 0)?;
                    return Ok(SceneTickEvent::SceneEntered { name });
                }
                None => {
                    return Ok(SceneTickEvent::UnknownMapId { map_id });
                }
            }
        }
        Ok(SceneTickEvent::Stepped)
    }

    /// Convenience: hand off a path to the SCUS `extracted/` root, get a
    /// host with no scene loaded yet.
    pub fn from_extracted_root(root: impl Into<PathBuf>) -> Result<Self> {
        Self::open_extracted(root.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Smoke test: BGM index math matches the documented retail resolver.
    /// `block_start + 6 + bgm_id` for ids < 2000.
    #[test]
    fn find_bgm_uses_documented_offset() {
        let scene = Scene {
            name: "test".into(),
            start: 100,
            end: 200,
            entries: (100..200u32)
                .map(|idx| SceneEntry {
                    idx,
                    class: Class::UnknownOther,
                    bytes: Arc::new(vec![]),
                })
                .collect(),
        };
        let bgm = scene.find_bgm(0).unwrap();
        assert_eq!(bgm.idx, 106);
        let bgm = scene.find_bgm(7).unwrap();
        assert_eq!(bgm.idx, 113);
    }

    /// BGM IDs >= 2000 are global-pool — not resolved by the per-scene
    /// helper. The full resolver (with global pool) is engine-side; the
    /// scene-local helper just declines.
    #[test]
    fn find_bgm_global_pool_returns_none() {
        let scene = Scene {
            name: "test".into(),
            start: 0,
            end: 10,
            entries: vec![],
        };
        assert!(scene.find_bgm(2000).is_none());
        assert!(scene.find_bgm(3000).is_none());
    }

    #[test]
    fn vec_map_id_resolver_indexes_into_list() {
        let r = VecMapIdResolver::new(vec!["aaa".into(), "bbb".into(), "ccc".into()]);
        assert_eq!(r.resolve(0).as_deref(), Some("aaa"));
        assert_eq!(r.resolve(2).as_deref(), Some("ccc"));
        assert_eq!(r.resolve(3), None);
    }

    #[test]
    fn null_map_id_resolver_returns_none() {
        let r = NullMapIdResolver;
        assert_eq!(r.resolve(0), None);
        assert_eq!(r.resolve(255), None);
    }

    /// `class_counts` reports a histogram in CDNAME order.
    #[test]
    fn class_counts_matches_entries() {
        let scene = Scene {
            name: "t".into(),
            start: 0,
            end: 3,
            entries: vec![
                SceneEntry {
                    idx: 0,
                    class: Class::UnknownOther,
                    bytes: Arc::new(vec![]),
                },
                SceneEntry {
                    idx: 1,
                    class: Class::UnknownOther,
                    bytes: Arc::new(vec![]),
                },
                SceneEntry {
                    idx: 2,
                    class: Class::Empty,
                    bytes: Arc::new(vec![]),
                },
            ],
        };
        let counts = scene.class_counts();
        assert_eq!(counts.get(&Class::UnknownOther).copied(), Some(2));
        assert_eq!(counts.get(&Class::Empty).copied(), Some(1));
    }
}
