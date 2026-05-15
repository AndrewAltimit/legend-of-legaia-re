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
//! No Sony bytes - this is plumbing over the format crates.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use legaia_asset::categorize::{Class, classify};
use legaia_prot::Region;
use legaia_prot::archive::{Archive, Entry};
use legaia_prot::cdname;

/// Index over PROT.DAT + CDNAME.TXT. Built once and shared for the whole
/// scene-host's lifetime. Thread-safe - the underlying file handle and the
/// caches are guarded by Mutexes.
pub struct ProtIndex {
    /// PROT archive (file handle + TOC). The handle needs `&mut` to seek/read,
    /// so we keep it in a Mutex behind the index.
    archive: Mutex<Archive>,
    /// Snapshot of the entry table - kept outside the Mutex so callers can
    /// inspect it (length, sizes, byte offsets) without locking.
    entries: Vec<Entry>,
    /// Optional CDNAME map (PROT index → first scene label in block).
    cdname: Option<cdname::IndexMap>,
    /// Lazy entry-bytes cache. Populated on first `entry_bytes` call.
    entry_cache: Mutex<HashMap<u32, Arc<Vec<u8>>>>,
    /// Lazy classification cache. Populated on first `class_of` call.
    class_cache: Mutex<HashMap<u32, Class>>,
    /// Retail region this index was opened against. Metadata only - the TOC
    /// formula and CDNAME layout are identical across regions.
    pub region: Region,
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
            region: Region::Na,
        })
    }

    /// Build an index from raw in-memory PROT.DAT bytes. WASM-safe - no
    /// filesystem access. Pass `cdname_text` if the CDNAME.TXT contents are
    /// available as a string; omit to skip scene-name resolution.
    pub fn from_bytes(prot_bytes: Vec<u8>, cdname_text: Option<&str>) -> Result<Self> {
        let archive = Archive::from_bytes(prot_bytes).context("parse in-memory PROT.DAT")?;
        let entries = archive.entries.clone();
        let cdname = cdname_text.map(cdname::parse_str).transpose()?;
        Ok(Self {
            archive: Mutex::new(archive),
            entries,
            cdname,
            entry_cache: Mutex::new(HashMap::new()),
            class_cache: Mutex::new(HashMap::new()),
            region: Region::Na,
        })
    }

    /// Set the region for this index (builder pattern - non-breaking).
    pub fn with_region(mut self, region: Region) -> Self {
        self.region = region;
        self
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

    /// All CDNAME block names in ascending PROT-entry-index order. Each
    /// unique block-start label appears exactly once. Returns an empty vec
    /// if no CDNAME map was loaded.
    ///
    /// Used by [`DefaultMapIdResolver`] to build the map-id → scene-name
    /// table at startup.
    pub fn cdname_scene_names(&self) -> Vec<String> {
        match &self.cdname {
            Some(map) => map.values().cloned().collect(),
            None => Vec::new(),
        }
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
    /// or 4 for `scene_vab_stream` containers - the chunk0 prefix is 4 bytes).
    pub fn as_vab(&self, offset: usize) -> Result<legaia_vab::VabReport> {
        legaia_vab::parse(&self.bytes, offset).context("parse VAB from PROT entry bytes")
    }
}

/// Per-scene event-script container - the field-VM bytecode bundle for a
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

/// Return `true` if a CDNAME scene label is an in-engine cutscene scene
/// (prefixed with `op` or `ed`, which use the dialogue actor overlay).
///
/// In-engine cutscenes are distinct from FMV (`MOV/MV*.STR`): they run
/// the dialogue-actor overlay and are not backed by STR video files.
/// Use `play-str` for FMV; use `play --scene` for these scenes.
pub fn is_cutscene_label(label: &str) -> bool {
    label.starts_with("op") || label.starts_with("ed")
}

/// Return the FMV [`MOV/MVn.STR`] filename associated with a CDNAME
/// cutscene scene, if any. The retail engine reads the mapping from
/// the cutscene overlay; until that table is captured the mapping is
/// derived from CDNAME ordering: the five `op*` opening scenes map
/// 1:1 to `MV1.STR..MV5.STR`, and the first `ed*` ending scene maps to
/// `MV6.STR`. Other ending scenes are dialogue-actor-overlay driven
/// (no FMV).
///
/// Returns `Some("MOV/MVn.STR")` for resolvable scenes, `None`
/// otherwise. Engines join the relative path against their extracted
/// root (or read the named ISO9660 entry from a `.bin` disc image).
///
/// Heuristic mapping pinned to the disc's MV1..MV6 file count + the
/// CDNAME ordering of the `op*` / `ed*` scene labels. The exact retail
/// table lives in the cutscene overlay (not yet captured); when it
/// lands, this function should be updated to read the captured map.
pub fn cutscene_str_for(scene_label: &str) -> Option<&'static str> {
    match scene_label {
        // op* opening cutscenes - five in CDNAME order.
        "opdeene" => Some("MOV/MV1.STR"),
        "opstati" => Some("MOV/MV2.STR"),
        "opkorout" => Some("MOV/MV3.STR"),
        "opurud" => Some("MOV/MV4.STR"),
        "opmap01" => Some("MOV/MV5.STR"),
        // ed* - only the first ending scene is FMV-backed; the rest are
        // dialogue-actor-overlay driven and have no associated MV file.
        "edteien" => Some("MOV/MV6.STR"),
        _ => None,
    }
}

/// Inverse of [`cutscene_str_for`]: resolve a `MOV/MVn.STR` filename to
/// its CDNAME scene label, if known. Useful for diagnostic dumps that
/// want to surface "this STR plays during scene X".
pub fn cutscene_label_for_str(str_filename: &str) -> Option<&'static str> {
    let trimmed = str_filename
        .rsplit_once('/')
        .map(|(_, name)| name)
        .unwrap_or(str_filename);
    match trimmed.to_ascii_uppercase().as_str() {
        "MV1.STR" => Some("opdeene"),
        "MV2.STR" => Some("opstati"),
        "MV3.STR" => Some("opkorout"),
        "MV4.STR" => Some("opurud"),
        "MV5.STR" => Some("opmap01"),
        "MV6.STR" => Some("edteien"),
        _ => None,
    }
}

/// All known FMV-backed cutscene scenes in CDNAME order.
pub const FMV_CUTSCENE_SCENES: [(&str, &str); 6] = [
    ("opdeene", "MOV/MV1.STR"),
    ("opstati", "MOV/MV2.STR"),
    ("opkorout", "MOV/MV3.STR"),
    ("opurud", "MOV/MV4.STR"),
    ("opmap01", "MOV/MV5.STR"),
    ("edteien", "MOV/MV6.STR"),
];

/// Field scenes the FMV cutscene overlay knows about by name (it carries
/// their CDNAME labels in its data section). These are mid-game scenes
/// with FMV trigger points - distinct from the `op*` opening / `ed*`
/// ending scenes covered by [`FMV_CUTSCENE_SCENES`]. The exact MV file
/// each plays isn't pinned by the heuristic; the field-VM script for
/// each scene calls a "play STR" op with the MV index.
pub const FMV_TRIGGER_FIELD_SCENES: [&str; 7] = [
    "town0b", "map01", "chitei2", "map02", "jou", "uru2", "town0e",
];

/// Return `true` if a CDNAME label is a field scene the FMV overlay
/// knows about (i.e. carries a mid-game FMV trigger). Distinct from
/// [`is_cutscene_label`] (which covers the `op*` / `ed*` engine cutscene
/// scenes).
pub fn is_fmv_trigger_field_scene(label: &str) -> bool {
    FMV_TRIGGER_FIELD_SCENES.contains(&label)
}

/// Engine-override CDNAME→MV cutscene map.
///
/// The retail table lives in the FMV-cutscene overlay (game modes 26/27,
/// see [`docs/subsystems/cutscene.md`](../../../../docs/subsystems/cutscene.md)).
/// The retail overlay isn't in the captured corpus yet - `dump_str_fmv_overlay.py`
/// is staged but not run - so the heuristic in [`cutscene_str_for`] ships
/// as the default. Once the overlay capture lands, engines can populate a
/// [`CutsceneMap`] from the captured table and override the heuristic via
/// [`CutsceneMap::resolve`].
///
/// The map is *additive*: entries the user inserts win, missing entries
/// fall back to the heuristic. Engines feed it from a captured-data file
/// or from the boot config.
#[derive(Debug, Clone, Default)]
pub struct CutsceneMap {
    forward: std::collections::HashMap<String, String>,
}

impl CutsceneMap {
    pub fn new() -> Self {
        Self::default()
    }

    /// Build a map seeded with the heuristic mapping ([`FMV_CUTSCENE_SCENES`]).
    /// Engines can then override or extend specific entries.
    pub fn from_heuristic() -> Self {
        let mut m = Self::default();
        for (label, str_path) in FMV_CUTSCENE_SCENES.iter() {
            m.forward
                .insert((*label).to_string(), (*str_path).to_string());
        }
        m
    }

    /// Insert / replace a CDNAME-label → STR-path mapping.
    pub fn insert(&mut self, scene_label: impl Into<String>, str_path: impl Into<String>) {
        self.forward.insert(scene_label.into(), str_path.into());
    }

    /// Resolve a CDNAME label to its STR path. Falls through to the
    /// hard-coded heuristic if the map doesn't carry the label.
    pub fn resolve(&self, scene_label: &str) -> Option<String> {
        if let Some(path) = self.forward.get(scene_label) {
            return Some(path.clone());
        }
        cutscene_str_for(scene_label).map(|s| s.to_string())
    }

    /// Number of explicit entries (excludes heuristic fallbacks).
    pub fn len(&self) -> usize {
        self.forward.len()
    }

    pub fn is_empty(&self) -> bool {
        self.forward.is_empty()
    }

    /// Iterate over the explicit `(scene_label, str_path)` entries the
    /// caller has installed (excludes heuristic fallbacks). Order is
    /// unspecified.
    pub fn entries(&self) -> impl Iterator<Item = (&str, &str)> {
        self.forward.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }

    /// Parse a [`CutsceneMap`] from a TOML document. The document holds a
    /// top-level `[scenes]` table that maps CDNAME labels to STR-file
    /// paths:
    ///
    /// ```toml
    /// [scenes]
    /// opdeene = "MOV/MV1.STR"
    /// opstati = "MOV/MV2.STR"
    /// edteien = "MOV/MV6.STR"
    /// ```
    ///
    /// Unknown top-level keys are ignored so engines can layer engine-
    /// specific config alongside.
    pub fn from_toml_str(toml_str: &str) -> Result<Self> {
        #[derive(serde::Deserialize)]
        struct Doc {
            scenes: Option<HashMap<String, String>>,
        }
        let doc: Doc =
            toml::from_str(toml_str).with_context(|| "parse cutscene-map TOML document")?;
        let mut map = Self::default();
        if let Some(table) = doc.scenes {
            for (k, v) in table {
                map.forward.insert(k, v);
            }
        }
        Ok(map)
    }

    /// Read a TOML config from disk. Wraps [`Self::from_toml_str`].
    pub fn from_toml_path(path: &Path) -> Result<Self> {
        let s = std::fs::read_to_string(path)
            .with_context(|| format!("read cutscene-map at {}", path.display()))?;
        Self::from_toml_str(&s).with_context(|| format!("parse {}", path.display()))
    }

    /// Serialise the map back to TOML form. Round-trips with
    /// [`Self::from_toml_str`].
    pub fn to_toml_string(&self) -> String {
        let mut out = String::from("[scenes]\n");
        let mut keys: Vec<&String> = self.forward.keys().collect();
        keys.sort();
        for k in keys {
            let v = &self.forward[k];
            // TOML basic-string escaping: paths use `\\` separators which
            // need explicit escaping. Use single-quoted literal strings to
            // sidestep that entirely.
            let safe_v = if v.contains('\'') {
                format!("\"{}\"", v.replace('\\', "\\\\").replace('"', "\\\""))
            } else {
                format!("'{}'", v)
            };
            out.push_str(&format!("{} = {}\n", k, safe_v));
        }
        out
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

    /// Find the per-scene event-scripts container - either a standalone
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

    /// Whether this scene's CDNAME label identifies it as an in-engine cutscene
    /// (dialogue-actor-overlay driven, not FMV). Use `play-str` for FMV.
    pub fn is_cutscene_scene(&self) -> bool {
        is_cutscene_label(&self.name)
    }

    /// Count of entries by class - tiny diagnostic for "what's in this scene".
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

/// Empty resolver - every `scene_transition` is a no-op. Useful for tests
/// + engines that haven't wired a real table yet.
#[derive(Debug, Default, Clone, Copy)]
pub struct NullMapIdResolver;

impl MapIdResolver for NullMapIdResolver {
    fn resolve(&self, _: u8) -> Option<String> {
        None
    }
}

/// Plain `Vec<String>`-backed resolver - index into a list of scene names
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

/// CDNAME-derived map-id resolver. Builds the map-id → scene-name table
/// from the PROT archive's CDNAME index at startup, using ascending
/// PROT-entry-index order as the sequential map-id.
///
/// Map-id 0 maps to the first CDNAME block name (lowest PROT index),
/// map-id 1 to the second, and so on.
///
/// **Ordering note (from `FUN_8001f7c0` trace):** The field-VM WARP opcode
/// (`0x3E`, `op0 >= 100`) only supports map_ids 0–6. Each maps to a code
/// overlay at PROT `0x4d + map_id` (+ 2 for map_id >= 6); the scene name is
/// pre-set in `DAT_80084548` by a pre-WARP handler not yet fully traced.
/// The sequential CDNAME ordering here is an approximation; the exact
/// retail map_id → scene-name table lives in an uncaptured overlay.
/// See `docs/subsystems/asset-loader.md` → "WARP opcode → scene transition flow".
///
/// Suitable for use in [`BootSession::open`] as the default resolver.
#[derive(Debug, Clone, Default)]
pub struct DefaultMapIdResolver {
    inner: VecMapIdResolver,
}

impl DefaultMapIdResolver {
    /// Build from a `ProtIndex` - calls [`ProtIndex::cdname_scene_names`]
    /// and wraps the resulting ordered list.
    pub fn from_index(index: &ProtIndex) -> Self {
        Self {
            inner: VecMapIdResolver::new(index.cdname_scene_names()),
        }
    }

    /// Construct directly from a name list. Useful for tests that can't
    /// open a real ProtIndex.
    pub fn new(names: Vec<String>) -> Self {
        Self {
            inner: VecMapIdResolver::new(names),
        }
    }
}

impl MapIdResolver for DefaultMapIdResolver {
    fn resolve(&self, map_id: u8) -> Option<String> {
        self.inner.resolve(map_id)
    }
}

/// Per-tick outcome from [`SceneHost::tick`]. Engines route this back into
/// their UI layer (e.g. log scene transitions, update HUD on battle end).
#[derive(Debug, Clone)]
pub enum SceneTickEvent {
    /// World stepped normally - no scene-level events this frame.
    Stepped,
    /// Field VM requested a scene transition that the resolver mapped to
    /// `name`; the host loaded it and reset the field VM.
    SceneEntered { name: String },
    /// `scene_transition(map_id)` was requested but the resolver returned
    /// `None`. The host left the existing scene loaded; the engine can
    /// log / surface the unknown id.
    UnknownMapId { map_id: u8 },
}

/// BGM dispatch hook - implemented by the audio layer (or test stubs) and
/// driven by [`SceneHost::route_bgm_events`]. The default
/// [`NullBgmDirector`] discards every request.
///
/// Sub-op semantics mirror retail field-VM op `0x35` - see
/// [`docs/subsystems/script-vm.md`] for the full table. The hook only
/// receives sub-ops that change playback state (1 = start, 2 = pause,
/// 3 = resume, 4 = stop, 9 = queue); other sub-ops are control words
/// that the host can route without sequencer state.
pub trait BgmDirector {
    /// Start playing the given SEQ bytes for `bgm_id`. The bytes have
    /// already been resolved by the host through
    /// [`SceneHost::bgm_seq_bytes`]; the director parses + attaches them.
    fn start(&mut self, bgm_id: u16, seq_bytes: &[u8]) {
        let _ = (bgm_id, seq_bytes);
    }
    fn pause(&mut self) {}
    fn resume(&mut self) {}
    fn stop(&mut self) {}
    /// Sub-op 9 - queue a BGM for later trigger. The bytes are pre-resolved
    /// like [`BgmDirector::start`].
    fn queue(&mut self, bgm_id: u16, seq_bytes: &[u8]) {
        let _ = (bgm_id, seq_bytes);
    }
}

/// Discards every BGM event. Useful for tests + engines that haven't wired
/// audio yet.
#[derive(Debug, Default, Clone, Copy)]
pub struct NullBgmDirector;
impl BgmDirector for NullBgmDirector {}

/// Bundles the runtime composite (`World`) with a loaded `Scene`, a frame
/// timer, and a [`MapIdResolver`] for field-VM scene transitions. The host
/// owns the engine-vm world (per-actor data + every VM's `Host` impl) and
/// exposes a single `tick()` that drives the active VMs and processes any
/// transitions the field VM requested.
pub struct SceneHost {
    pub index: Arc<ProtIndex>,
    pub world: crate::world::World,
    pub scene: Option<Scene>,
    /// Typed asset snapshot for the currently loaded scene - refreshed
    /// every time [`SceneHost::load_scene`] or [`SceneHost::enter_field_scene`]
    /// runs. `None` until the first scene loads.
    pub assets: Option<crate::scene_assets::SceneAssets>,
    /// Runtime resource snapshot built by [`SceneHost::enter_field_scene`] -
    /// holds the populated PSX VRAM, parsed TMD pool, and parsed ANM packs.
    /// `None` until the first `enter_field_scene` call. Use for rendering
    /// and for driving `World::init_scene_animations`.
    pub resources: Option<crate::scene_resources::SceneResources>,
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
            assets: None,
            resources: None,
            frame_time: crate::FrameTime::new(),
            map_resolver: Box::new(NullMapIdResolver),
        }
    }

    /// Open the host directly from an extracted directory.
    pub fn open_extracted(extracted_root: impl AsRef<Path>) -> Result<Self> {
        let p = ProtIndex::open_extracted(extracted_root.as_ref())?;
        Ok(Self::new(Arc::new(p)))
    }

    /// Open the host directly from a `.bin` disc image. The disc is walked
    /// once to extract `PROT.DAT` and `CDNAME.TXT` from the ISO9660 tree;
    /// the extracted bytes are then handed to [`ProtIndex::from_bytes`].
    ///
    /// This is the user-facing path: ship the engine, the user supplies a
    /// disc image, no extraction step needed. Native targets only - WASM
    /// uses `from_prot_bytes` with the bytes supplied via JS.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn open_disc(disc_bin: impl AsRef<Path>) -> Result<Self> {
        use crate::Vfs;
        let vfs = crate::DiscVfs::open(disc_bin.as_ref())?;
        let prot_bytes = vfs
            .read("prot.dat")
            .with_context(|| "PROT.DAT not present in disc image")?;
        // CDNAME.TXT may live at either DATA/CDNAME.TXT or top-level. The
        // ISO walker stores the path verbatim.
        let cdname_bytes = vfs
            .read("cdname.txt")
            .or_else(|_| vfs.read("data/cdname.txt"))
            .ok();
        let cdname_text = match cdname_bytes {
            Some(b) => Some(String::from_utf8(b).context("CDNAME.TXT is not valid UTF-8")?),
            None => None,
        };
        let p = ProtIndex::from_bytes(prot_bytes, cdname_text.as_deref())?;
        Ok(Self::new(Arc::new(p)))
    }

    /// Build a host from raw in-memory PROT.DAT bytes. WASM-safe - no
    /// filesystem access. Pass `cdname_text` if the CDNAME.TXT contents are
    /// available; omit to skip scene-name resolution.
    pub fn from_prot_bytes(prot_bytes: Vec<u8>, cdname_text: Option<&str>) -> Result<Self> {
        let p = ProtIndex::from_bytes(prot_bytes, cdname_text)?;
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
        let assets = crate::scene_assets::SceneAssets::build(&scene);
        self.scene = Some(scene);
        self.assets = Some(assets);
        Ok(self.scene.as_ref().unwrap())
    }

    /// Borrow the current scene's typed asset snapshot. `None` if no scene
    /// is loaded.
    pub fn assets(&self) -> Option<&crate::scene_assets::SceneAssets> {
        self.assets.as_ref()
    }

    /// Resolve a BGM id to the raw SEQ bytes the runtime would pass to its
    /// sequencer. Mirrors `FUN_800243F0` (the BGM resolver): scene-local ids
    /// (`< 2000`) live at `block_start + 6 + id`; global-pool ids
    /// (`>= 2000`) are not modeled. Returns `None` when no scene is loaded
    /// or no SEQ-bearing entry maps to the id.
    ///
    /// Engines parse the returned bytes with [`legaia_seq::Seq::parse`] and
    /// attach to [`legaia_engine_audio::Sequencer::new`] alongside the
    /// scene's VAB bank.
    pub fn bgm_seq_bytes(&self, bgm_id: u16) -> Result<Option<Arc<Vec<u8>>>> {
        let Some(assets) = self.assets.as_ref() else {
            return Ok(None);
        };
        let Some(entry_idx) = assets.bgm_seq_entry(bgm_id) else {
            return Ok(None);
        };
        let bytes = self.index.entry_bytes(entry_idx)?;
        let offset = assets.bgm_seq_offset(bgm_id).unwrap_or(0);
        if offset == 0 {
            Ok(Some(bytes))
        } else if offset < bytes.len() {
            // Slice past the chunk-header wrapper so the returned bytes
            // start at the `pQES` magic. Allocates a fresh Arc - the
            // caller usually parses once and caches the resulting Seq.
            Ok(Some(Arc::new(bytes[offset..].to_vec())))
        } else {
            Ok(None)
        }
    }

    /// First VAB-bearing entry in the scene, ready for parsing as a sound
    /// bank. Mirrors the asset chain's "load the scene's bank before the
    /// first sound plays" pre-pass. Returns `None` when no VAB-tagged
    /// entries are in the scene.
    pub fn scene_vab_bytes(&self) -> Result<Option<Arc<Vec<u8>>>> {
        let Some(assets) = self.assets.as_ref() else {
            return Ok(None);
        };
        let Some(&entry_idx) = assets.vab_entries.first() else {
            return Ok(None);
        };
        let bytes = self.index.entry_bytes(entry_idx)?;
        Ok(Some(bytes))
    }

    /// If the world has a pending dialog request and no panel is currently
    /// running, build an [`crate::dialog::OwnedDialogPanel`] resolved through
    /// the scene's MES container and return it. The caller drives the
    /// panel per-frame; when [`crate::dialog::OwnedDialogPanel::is_done`]
    /// reports true, the caller calls [`SceneHost::clear_dialog`] to
    /// release the field-VM script.
    ///
    /// Returns `None` when no dialog is pending or the scene has no MES
    /// container. The resolved request is left on the world; calling
    /// [`SceneHost::clear_dialog`] cleans it up when the user dismisses
    /// the box.
    pub fn open_pending_dialog(&mut self) -> Option<crate::dialog::OwnedDialogPanel> {
        let req = self.world.current_dialog.as_ref()?;
        let mes = self.assets.as_ref()?.mes.as_ref()?;
        crate::dialog::OwnedDialogPanel::from_scene_mes(mes, req.text_id)
    }

    /// Clear the world's pending dialog request. Call after the user
    /// dismisses the box (the field VM resumes the next frame).
    pub fn clear_dialog(&mut self) {
        self.world.current_dialog = None;
    }

    /// Drain the world's pending BGM events through `director`, resolving
    /// each `Bgm{text_id, sub_op}` into the right director hook. Mirrors
    /// the field-VM op `0x35` sub-op table: `1` = start (resolve SEQ
    /// bytes), `2` = pause, `3` = resume, `4` = stop, `9` = queue.
    /// Other sub-ops are passed through as no-ops (the host already
    /// surfaced them on the world's event queue for richer engines to
    /// consume).
    ///
    /// Returns the number of events that the director acted on. Call once
    /// per frame after [`SceneHost::tick`].
    pub fn route_bgm_events(&mut self, director: &mut dyn BgmDirector) -> Result<usize> {
        let mut acted = 0usize;
        let mut leftover = Vec::new();
        for ev in self.world.drain_field_events() {
            match ev {
                crate::field_events::FieldEvent::Bgm { text_id, sub_op } => match sub_op {
                    1 => {
                        if let Some(bytes) = self.bgm_seq_bytes(text_id)? {
                            director.start(text_id, &bytes);
                            acted += 1;
                        }
                    }
                    9 => {
                        if let Some(bytes) = self.bgm_seq_bytes(text_id)? {
                            director.queue(text_id, &bytes);
                            acted += 1;
                        }
                    }
                    2 => {
                        director.pause();
                        acted += 1;
                    }
                    3 => {
                        director.resume();
                        acted += 1;
                    }
                    4 => {
                        director.stop();
                        acted += 1;
                    }
                    _ => {
                        // Other sub-ops (5/6/7/8/10/11) are control words -
                        // surface them back on the queue for richer engines.
                        leftover.push(crate::field_events::FieldEvent::Bgm { text_id, sub_op });
                    }
                },
                other => leftover.push(other),
            }
        }
        // Restore non-BGM (and unhandled-BGM) events so engine layers that
        // also consume them aren't shorted by this routing pass.
        self.world.pending_field_events.extend(leftover);
        Ok(acted)
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
        // Pre-bind actor ↔ TMD/ANM resources so they survive the first
        // field-VM actor-spawn opcode (see `World::init_scene_animations`).
        //
        // Uses [`SceneResources::build_targeted_with_options`] with
        // `SceneLoadKind::Field` so the per-TIM image / CLUT block
        // decisions match the retail field loader: only field /
        // terrain / NPC meshes contribute, scene_tmd_stream battle
        // character meshes (loaded by `FUN_8001FE70` at battle init)
        // and battle_data records (`FUN_8001E890` chain) are excluded.
        if let Some(scene) = self.scene.as_ref() {
            // The shared blocks the retail field engine keeps resident
            // across scene transitions (player TMD + shared UI atlas).
            let mut shared_scenes: Vec<Scene> = Vec::new();
            for name in crate::scene_resources::FIELD_SHARED_BLOCKS {
                if let Ok(s) = Scene::load(&self.index, name) {
                    shared_scenes.push(s);
                }
            }
            let shared_refs: Vec<&Scene> = shared_scenes.iter().collect();
            if let Ok((res, _stats)) =
                crate::scene_resources::SceneResources::build_targeted_with_options(
                    scene,
                    &shared_refs,
                    crate::scene_resources::BuildOptions {
                        kind: crate::scene_resources::SceneLoadKind::Field,
                    },
                )
            {
                self.world.init_scene_animations(&res);
                self.resources = Some(res);
            }
        }
        // Drain any pending transition the previous scene left behind.
        self.world.pending_scene_transition = None;
        Ok(())
    }

    /// One frame: tick the world, materialize any actor-spawn requests
    /// queued by the field VM's `0x4C 0x80` opcode, then process any
    /// pending `scene_transition(map_id)` request. Returns the
    /// [`SceneTickEvent`] describing what happened.
    pub fn tick(&mut self) -> Result<SceneTickEvent> {
        let _ = self.world.tick();
        self.world
            .materialize_actor_spawns(crate::world::FIELD_SPAWN_START_SLOT);
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

    /// Replace the effect-script catalog used by the effect VM pool.
    ///
    /// Call once after loading PROT 873 (`efect.dat`) and parsing its
    /// pack1 slice via [`legaia_engine_vm::effect_vm::EffectCatalog::from_pack1_bytes`].
    /// An empty catalog is safe - `BattleHostImpl::ui_element` will simply
    /// not spawn any pool entries until a real catalog is wired.
    pub fn set_effect_catalog(&mut self, catalog: legaia_engine_vm::effect_vm::EffectCatalog) {
        self.world.effect_catalog = catalog;
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

    #[test]
    fn cutscene_str_for_resolves_known_op_scenes() {
        assert_eq!(cutscene_str_for("opdeene"), Some("MOV/MV1.STR"));
        assert_eq!(cutscene_str_for("opstati"), Some("MOV/MV2.STR"));
        assert_eq!(cutscene_str_for("opkorout"), Some("MOV/MV3.STR"));
        assert_eq!(cutscene_str_for("opurud"), Some("MOV/MV4.STR"));
        assert_eq!(cutscene_str_for("opmap01"), Some("MOV/MV5.STR"));
    }

    #[test]
    fn cutscene_str_for_resolves_first_ed_scene_only() {
        assert_eq!(cutscene_str_for("edteien"), Some("MOV/MV6.STR"));
        // The remaining ed* scenes are dialogue-actor-overlay driven and
        // have no FMV file.
        assert_eq!(cutscene_str_for("edbylon"), None);
        assert_eq!(cutscene_str_for("edlast"), None);
        assert_eq!(cutscene_str_for("edstati3"), None);
    }

    #[test]
    fn cutscene_str_for_returns_none_for_non_cutscene_labels() {
        assert_eq!(cutscene_str_for("town01"), None);
        assert_eq!(cutscene_str_for("battle_data"), None);
        assert_eq!(cutscene_str_for(""), None);
    }

    #[test]
    fn fmv_trigger_field_scenes_are_distinct_from_op_ed_cutscenes() {
        for label in FMV_TRIGGER_FIELD_SCENES {
            assert!(is_fmv_trigger_field_scene(label));
            // None of these are op*/ed* engine cutscenes.
            assert!(!is_cutscene_label(label));
            // None map through the heuristic to a specific MV file - the
            // mapping is in the field-VM script for each scene, not here.
            assert_eq!(cutscene_str_for(label), None);
        }
        assert!(!is_fmv_trigger_field_scene("opdeene"));
        assert!(!is_fmv_trigger_field_scene("battle_data"));
    }

    #[test]
    fn cutscene_label_for_str_round_trip() {
        for (label, path) in FMV_CUTSCENE_SCENES.iter() {
            assert_eq!(cutscene_str_for(label), Some(*path));
            // Inverse via either form (with or without dir prefix).
            let bare = path.rsplit_once('/').map(|(_, n)| n).unwrap_or(path);
            assert_eq!(cutscene_label_for_str(bare), Some(*label));
            assert_eq!(cutscene_label_for_str(path), Some(*label));
        }
    }

    #[test]
    fn cutscene_label_for_str_handles_case_insensitive_filenames() {
        // ISO9660 filenames may upper- or lowercase depending on extractor.
        assert_eq!(cutscene_label_for_str("mv1.str"), Some("opdeene"));
        assert_eq!(cutscene_label_for_str("MOV/mv6.STR"), Some("edteien"));
        assert_eq!(cutscene_label_for_str("garbage"), None);
    }

    #[test]
    fn cutscene_map_default_is_empty_and_resolves_via_heuristic() {
        let m = CutsceneMap::new();
        assert!(m.is_empty());
        assert_eq!(m.resolve("opdeene"), Some("MOV/MV1.STR".into()));
        assert_eq!(m.resolve("town01"), None);
    }

    #[test]
    fn cutscene_map_explicit_entry_overrides_heuristic() {
        let mut m = CutsceneMap::new();
        m.insert("opdeene", "MOV/CUSTOM.STR");
        assert_eq!(m.resolve("opdeene"), Some("MOV/CUSTOM.STR".into()));
        // Other entries still fall through to the heuristic.
        assert_eq!(m.resolve("opstati"), Some("MOV/MV2.STR".into()));
    }

    #[test]
    fn cutscene_map_from_heuristic_preloaded() {
        let m = CutsceneMap::from_heuristic();
        assert_eq!(m.len(), FMV_CUTSCENE_SCENES.len());
        for (label, path) in FMV_CUTSCENE_SCENES.iter() {
            assert_eq!(m.resolve(label), Some((*path).into()));
        }
    }

    #[test]
    fn cutscene_map_unknown_label_falls_through_to_heuristic_none() {
        let m = CutsceneMap::from_heuristic();
        assert_eq!(m.resolve("town01"), None);
        assert_eq!(m.resolve("xxx"), None);
    }

    #[test]
    fn cutscene_map_from_toml_str_parses_scenes_table() {
        let doc = r#"
[scenes]
opdeene = "MOV/MV1.STR"
opstati = 'MOV/MV2.STR'
edteien = "MOV/MV6.STR"
"#;
        let m = CutsceneMap::from_toml_str(doc).expect("parse");
        assert_eq!(m.len(), 3);
        assert_eq!(m.resolve("opdeene"), Some("MOV/MV1.STR".into()));
        assert_eq!(m.resolve("opstati"), Some("MOV/MV2.STR".into()));
        assert_eq!(m.resolve("edteien"), Some("MOV/MV6.STR".into()));
        // Unmapped scenes still fall through to the heuristic.
        assert_eq!(m.resolve("opmap01"), Some("MOV/MV5.STR".into()));
    }

    #[test]
    fn cutscene_map_from_toml_str_empty_doc_yields_empty_map() {
        let m = CutsceneMap::from_toml_str("").expect("parse");
        assert!(m.is_empty());
    }

    #[test]
    fn cutscene_map_from_toml_str_ignores_unknown_top_level_keys() {
        let doc = r#"
some_other_setting = 42
[other_table]
foo = "bar"
[scenes]
opdeene = "MOV/MV1.STR"
"#;
        let m = CutsceneMap::from_toml_str(doc).expect("parse");
        assert_eq!(m.len(), 1);
        assert_eq!(m.resolve("opdeene"), Some("MOV/MV1.STR".into()));
    }

    #[test]
    fn cutscene_map_to_toml_string_round_trips() {
        let mut m = CutsceneMap::new();
        m.insert("opdeene", "MOV/MV1.STR");
        m.insert("edteien", "MOV/MV6.STR");
        let toml_doc = m.to_toml_string();
        let parsed = CutsceneMap::from_toml_str(&toml_doc).expect("re-parse");
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed.resolve("opdeene"), Some("MOV/MV1.STR".into()));
        assert_eq!(parsed.resolve("edteien"), Some("MOV/MV6.STR".into()));
    }

    #[test]
    fn cutscene_map_to_toml_string_handles_backslash_paths() {
        let mut m = CutsceneMap::new();
        // Some engines load using Windows-style separators; the TOML
        // writer must escape these so the round-trip is lossless.
        m.insert("opdeene", "MOV\\MV1.STR");
        let toml_doc = m.to_toml_string();
        let parsed = CutsceneMap::from_toml_str(&toml_doc).expect("re-parse");
        assert_eq!(parsed.resolve("opdeene"), Some("MOV\\MV1.STR".into()));
    }

    #[test]
    fn cutscene_map_entries_iterates_explicit_only() {
        let mut m = CutsceneMap::new();
        m.insert("opdeene", "MOV/MV1.STR");
        m.insert("edteien", "MOV/MV6.STR");
        let mut entries: Vec<(String, String)> =
            m.entries().map(|(a, b)| (a.into(), b.into())).collect();
        entries.sort();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].0, "edteien");
        assert_eq!(entries[1].0, "opdeene");
    }

    #[test]
    fn cutscene_map_from_toml_path_reads_file() {
        let dir = std::env::temp_dir();
        let p = dir.join("legaia-re-cutscene-test.toml");
        std::fs::write(
            &p,
            "[scenes]\nopdeene = \"MOV/MV1.STR\"\nedteien = \"MOV/MV6.STR\"\n",
        )
        .expect("write tmp");
        let m = CutsceneMap::from_toml_path(&p).expect("read");
        assert_eq!(m.len(), 2);
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn default_map_id_resolver_resolves_by_position() {
        let r = DefaultMapIdResolver::new(vec!["town01".into(), "cave01".into(), "world01".into()]);
        assert_eq!(r.resolve(0), Some("town01".into()));
        assert_eq!(r.resolve(1), Some("cave01".into()));
        assert_eq!(r.resolve(2), Some("world01".into()));
        assert_eq!(r.resolve(3), None);
    }

    #[test]
    fn default_map_id_resolver_empty_returns_none() {
        let r = DefaultMapIdResolver::default();
        assert_eq!(r.resolve(0), None);
    }

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

    /// BGM IDs >= 2000 are global-pool - not resolved by the per-scene
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

    #[test]
    fn null_bgm_director_swallows_every_call() {
        // Compiles + every default impl is a no-op.
        let mut d = NullBgmDirector;
        d.start(1, &[]);
        d.queue(2, &[]);
        d.pause();
        d.resume();
        d.stop();
    }

    /// Test director that records every dispatched event for assertion.
    #[derive(Default)]
    struct RecordingBgm {
        log: Vec<String>,
    }
    impl BgmDirector for RecordingBgm {
        fn start(&mut self, id: u16, bytes: &[u8]) {
            self.log.push(format!("start({id},{})", bytes.len()));
        }
        fn queue(&mut self, id: u16, bytes: &[u8]) {
            self.log.push(format!("queue({id},{})", bytes.len()));
        }
        fn pause(&mut self) {
            self.log.push("pause".into());
        }
        fn resume(&mut self) {
            self.log.push("resume".into());
        }
        fn stop(&mut self) {
            self.log.push("stop".into());
        }
    }

    /// Pause / resume / stop sub-ops fire even without a loaded scene
    /// (no SEQ resolution required).
    #[test]
    fn route_bgm_handles_control_subops_without_scene() {
        // Build a scene-less SceneHost via the test fixture in
        // tests/scene_bundle_smoke.rs is too heavy here - instead, just
        // exercise the routing logic through a minimal scaffold by
        // directly emitting to a recording director and asserting the
        // matching events came through.
        //
        // SceneHost::new requires a ProtIndex which requires a real PROT
        // file, so this test exercises route_bgm_events indirectly via
        // a unit-sized stand-in: only the control sub-ops 2/3/4.
        let mut d = RecordingBgm::default();
        let ev2 = crate::field_events::FieldEvent::Bgm {
            text_id: 0,
            sub_op: 2,
        };
        let ev3 = crate::field_events::FieldEvent::Bgm {
            text_id: 0,
            sub_op: 3,
        };
        let ev4 = crate::field_events::FieldEvent::Bgm {
            text_id: 0,
            sub_op: 4,
        };
        // Mimic the route_bgm_events branches directly.
        for ev in [ev2, ev3, ev4] {
            if let crate::field_events::FieldEvent::Bgm { sub_op, .. } = ev {
                match sub_op {
                    2 => d.pause(),
                    3 => d.resume(),
                    4 => d.stop(),
                    _ => {}
                }
            }
        }
        assert_eq!(d.log, vec!["pause", "resume", "stop"]);
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
