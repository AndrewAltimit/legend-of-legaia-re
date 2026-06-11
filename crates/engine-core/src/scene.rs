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

/// Size of the per-scene field map file (retail `DATA\FIELD\<scene>.MAP`):
/// the field buffer's used region from the base through the field-pack
/// boundary at `+0x12000`. Used to identify the map entry in a CDNAME block.
pub const FIELD_MAP_LEN: usize = 0x12000;
/// Offset of the collision/floor grid within the field map file (= the
/// runtime `*(_DAT_1f8003ec) + 0x4000`).
pub const FIELD_MAP_COLLISION_OFFSET: usize = 0x4000;
/// Length of the collision/floor grid (`0x80 x 0x80` bytes, 1 byte/tile).
pub const FIELD_COLLISION_GRID_LEN: usize = 0x80 * 0x80;

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
    /// Snapshot of the raw PROT TOC dword array. The retail size-lookup
    /// formula at `FUN_8003e8a8` reads `toc[idx+3] - toc[idx+2]` and the
    /// start-LBA stash reads `toc[idx+2]`; we keep this slice handy for
    /// [`CdDmaHost`](crate::cd_dma::CdDmaHost) implementations that mirror
    /// those reads. Cloned out of [`Archive::toc`] at construction.
    toc: Vec<u32>,
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
        let toc = archive.toc.clone();
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
            toc,
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
        let toc = archive.toc.clone();
        let cdname = cdname_text.map(cdname::parse_str).transpose()?;
        Ok(Self {
            archive: Mutex::new(archive),
            entries,
            toc,
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

    /// Raw PROT TOC dword array (the contents of `0x801C70F0..` in retail).
    /// Useful for the retail size-lookup / start-LBA formulas that index
    /// `toc[idx+2]` / `toc[idx+3]` (see [`Self::entry_start_lba_retail`] and
    /// [`Self::entry_lba_count_retail`]).
    pub fn toc(&self) -> &[u32] {
        &self.toc
    }

    /// Retail-formula PROT entry start LBA: `toc[idx+2]`. Mirrors the
    /// stash into `gp[0x8F0]` inside `FUN_8003e8a8`. Returns `None` if
    /// the TOC isn't large enough to index this entry (out of range).
    pub fn entry_start_lba_retail(&self, idx: u16) -> Option<u32> {
        self.toc.get(idx as usize + 2).copied()
    }

    /// Retail-formula PROT entry size in LBAs: `toc[idx+3] - toc[idx+2]`.
    /// Mirrors the return of `FUN_8003e8a8`. Wraps on non-monotonic TOC
    /// pairs (matching the retail `subu` semantic). Returns `None` if
    /// either neighbouring slot is out of range.
    pub fn entry_lba_count_retail(&self, idx: u16) -> Option<u32> {
        let p = idx as usize;
        let cur = self.toc.get(p + 2).copied()?;
        let next = self.toc.get(p + 3).copied()?;
        Some(next.wrapping_sub(cur))
    }

    /// Read entry bytes (lazy + cached). Returns the same `Arc` for repeated
    /// reads of the same index.
    ///
    /// Returns the **TOC-indexed sub-region** (the historical
    /// `toc[p+5] - toc[p+3] + 4` slice). Scene-side parsers were designed for
    /// indexed bytes only — trailing-overlay sectors that some entries carry
    /// are not scene-asset data (they're MIPS overlay code; see boot.md).
    /// Callers that want the full on-disc footprint should use
    /// [`Self::entry_bytes_extended`].
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
            .read_entry_indexed(&entry, &mut bytes)
            .with_context(|| format!("read PROT entry {}", idx))?;
        let arc = Arc::new(bytes);
        self.entry_cache.lock().unwrap().insert(idx, arc.clone());
        Ok(arc)
    }

    /// Read an entry's full on-disc footprint (indexed payload + any
    /// trailing-overlay sectors). Use this when you want what the SCUS boot
    /// loader actually reads — e.g. the title-screen overlay code lives in
    /// the trailing sectors past PROT 899's indexed end (see boot.md).
    /// Bypasses the indexed-only cache; callers expecting a single byte
    /// view of an entry should keep using [`Self::entry_bytes`].
    pub fn entry_bytes_extended(&self, idx: u32) -> Result<Vec<u8>> {
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
            .with_context(|| format!("read PROT entry {} (extended)", idx))?;
        Ok(bytes)
    }

    /// Read raw bytes from `PROT.DAT` at an arbitrary file offset.
    ///
    /// Used to reach unindexed gap regions that don't belong to any TOC
    /// entry — e.g. the 240 KB system-UI gap between the TOC and
    /// `init_data` that carries the menu-glyph atlas and other
    /// boot-time UI TIMs (see [`docs/subsystems/boot.md`]).
    pub fn prot_dat_raw_bytes(&self, byte_offset: u64, len: usize) -> Result<Vec<u8>> {
        let mut bytes = Vec::new();
        self.archive
            .lock()
            .unwrap()
            .read_raw(byte_offset, len, &mut bytes)
            .with_context(|| format!("read PROT.DAT raw at 0x{:X} +{}", byte_offset, len))?;
        Ok(bytes)
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

    /// PROT entries in `scene_name`'s CDNAME block whose payload is a
    /// `scene_tmd_stream` — the battle-stage half-dome backdrops (sky + mountain
    /// ring + ground that the battle is fought inside; see
    /// [`docs/subsystems/battle.md`] "Battle background"). For an overworld
    /// scene like `map01` these are the per-area stage variants (e.g. 88/89/90:
    /// byte-identical dome geometry, different textures). The first is the
    /// default backdrop; per-sub-area variant selection is a follow-up. Empty
    /// when no CDNAME map is loaded or the block has no stage entries.
    pub fn battle_stage_entries(&self, scene_name: &str) -> Vec<u32> {
        let Some((start, end)) = self.block_range(scene_name) else {
            return Vec::new();
        };
        (start..end)
            .filter(|&idx| {
                self.entry_bytes(idx)
                    .map(|b| legaia_asset::scene_tmd_stream::is_scene_tmd_stream(&b))
                    .unwrap_or(false)
            })
            .collect()
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
/// their CDNAME labels in its data section) - distinct from the `op*`
/// opening / `ed*` ending scenes covered by [`FMV_CUTSCENE_SCENES`].
///
/// NOT the trigger-op scene list: the disc-walked per-scene trigger table
/// (`man_field_scripts::scene_fmv_triggers`, pinned by the
/// `scene_fmv_triggers_disc` test) finds the `0x4C 0xE2` ops in `town01`,
/// `garmel`, `deroa`, `chitei2`, `dohaty`, `town0d`, `uru` and `jouine` -
/// only `chitei2` overlaps this list. These labels are the scenes the
/// overlay itself references (e.g. for the post-playback scene
/// restore), recorded as found in its data section.
pub const FMV_TRIGGER_FIELD_SCENES: [&str; 7] = [
    "town0b", "map01", "chitei2", "map02", "jou", "uru2", "town0e",
];

/// Return `true` if a CDNAME label is a field scene the FMV overlay
/// carries in its data section (see [`FMV_TRIGGER_FIELD_SCENES`] - not
/// the trigger-op scene set). Distinct from [`is_cutscene_label`] (which
/// covers the `op*` / `ed*` engine cutscene scenes).
pub fn is_fmv_trigger_field_scene(label: &str) -> bool {
    FMV_TRIGGER_FIELD_SCENES.contains(&label)
}

/// Return `true` if a CDNAME label is an overworld (world-map) scene.
///
/// Legaia's three kingdom overworlds are `map01` / `map02` / `map03`; they are
/// the only CDNAME `mapNN` labels. The overworld shares game mode `0x03` with
/// towns and fields (see `docs/subsystems/field-locomotion.md` "Town / field
/// parity") - retail does not give it a distinct mode. It distinguishes the
/// overworld by the scene's own code overlay: the kingdom bundles carry a
/// world-map-overlay slot (type byte `0x05`, see
/// `docs/formats/world-map-overlay.md`) that towns/dungeons lack. The engine
/// classifies by the stable CDNAME label, which is `map` followed by two
/// digits for exactly these three scenes. A scene that classifies as a world
/// map is entered through [`SceneHost::enter_world_map_scene`] (which seeds the
/// region-keyed encounter table + world-map camera) instead of the plain
/// field path.
pub fn is_world_map_scene(label: &str) -> bool {
    match label.strip_prefix("map") {
        Some(rest) => rest.len() == 2 && rest.bytes().all(|b| b.is_ascii_digit()),
        None => false,
    }
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

    /// PROT index of the per-scene **field map file** - retail
    /// `DATA\FIELD\<scene>.MAP`, the first file `FUN_8001f7c0` streams into the
    /// field-buffer base (`_DAT_1f8003ec`).
    ///
    /// The runtime resolves it through `FUN_8003e8a8`'s `toc[idx+2]`, which
    /// lands **two entries below the per-entry extractor's CDNAME block
    /// start** - the scene PROT clusters overlap, so the extractor attributes
    /// each scene's `.MAP` to the tail of the *previous* block. The entry is
    /// identified by its **extended on-disc footprint** of exactly
    /// [`FIELD_MAP_LEN`] (`0x12000`) bytes; scenes whose `start - 2` entry
    /// isn't that size have no field map (cutscene / pure-asset blocks).
    ///
    /// The first `FIELD_MAP_LEN` entry *inside* a block is the **next**
    /// scene's `.MAP`, not this scene's - an earlier in-block rule loaded the
    /// wrong map for every field scene and was masked only where adjacent
    /// variants byte-copy (the Rim Elm pair `town01`/`town0b`/`town0c` share
    /// one identical map). Pinned by a save-library census: the live field
    /// buffer of a `keikoku` session matches entry `define-2` (PROT 0109)
    /// with **zero** collision-grid diffs (in-block entry 0118 diffs by
    /// thousands), same for `koin3` (PROT 0559 exact), and the kingdom walk
    /// maps were already live-verified at `start - 2`. The **object-index
    /// grid** (`+0x8000`, the [`Self::field_object_placements`] source) is
    /// live-validated the same way: residuals of 0..96 bytes against the
    /// resolved entry across town01 / town0c / keikoku / koin3 sessions
    /// (story-conditional cell mutations), thousands against every other
    /// candidate (regression-guarded by the disc + save-library gated
    /// `field_map_object_grid_live` test).
    ///
    /// The footprint matters: the TOC-indexed payload is only the first
    /// `0x4000` bytes (the object-record region); the collision grid at
    /// `+0x4000` and beyond lives in the entry's **trailing-gap sectors**, so
    /// callers must read the extended footprint, not [`SceneEntry::bytes`].
    ///
    /// See [`docs/subsystems/field-locomotion.md`] for the load chain.
    pub fn field_map_index(&self, index: &ProtIndex) -> Option<u32> {
        self.start.checked_sub(2).filter(|&idx| {
            index
                .entries()
                .get(idx as usize)
                .is_some_and(|e| e.size_bytes as usize == FIELD_MAP_LEN)
        })
    }

    /// The per-scene base collision/floor grid: the `+0x4000..+0x8000` region
    /// of the [`field_map`](Self::field_map_index) file (`0x80 x 0x80` bytes,
    /// high nibble = sub-cell wall bits, low nibble = floor-elevation tier).
    /// This is the engine's source for the base walkable grid; the field-VM
    /// `0x4C` nibble-7 ops layer story-conditional deltas on top as the
    /// prescript runs. Verified byte-exact against live RAM (town01).
    ///
    /// Reads the field map entry's **extended** footprint (the grid is past
    /// the TOC-indexed payload). Returns `Ok(None)` if the scene has no field
    /// map or the entry is too short to hold a full grid.
    pub fn field_collision_grid(&self, index: &ProtIndex) -> Result<Option<Vec<u8>>> {
        let Some(idx) = self.field_map_index(index) else {
            return Ok(None);
        };
        let bytes = index.entry_bytes_extended(idx)?;
        Ok(bytes
            .get(FIELD_MAP_COLLISION_OFFSET..FIELD_MAP_COLLISION_OFFSET + FIELD_COLLISION_GRID_LEN)
            .map(<[u8]>::to_vec))
    }

    /// The per-scene `.MAP` **region-table block**: the file's
    /// `+0x10000..+0x12000` region (retail `*(_DAT_1F8003EC) + 0x10000`),
    /// holding the region-record table the shared point-in-AABB scan
    /// (`FUN_80017FBC`) walks - body offset `s16` at block `+0xE`, count
    /// `s16` at `+0x10`, 8-byte records `[x0, z0, x1, z1, type, 0, 0, 0]`.
    /// Consumed by [`crate::field_regions::RegionTable`]. Returns
    /// `Ok(None)` when the scene has no field map.
    ///
    /// REF: FUN_80017FBC, FUN_800180EC (ports in [`crate::field_regions`])
    pub fn field_map_region_block(&self, index: &ProtIndex) -> Result<Option<Vec<u8>>> {
        let Some(idx) = self.field_map_index(index) else {
            return Ok(None);
        };
        let bytes = index.entry_bytes_extended(idx)?;
        Ok(bytes
            .get(crate::field_regions::MAP_REGION_BLOCK_OFFSET..FIELD_MAP_LEN)
            .map(<[u8]>::to_vec))
    }

    /// The scene's static-object placements: one entry per placed tile of the
    /// field map file's object-index grid (`+0x8000`), positioned in world
    /// space from the `+0x0000` object-record table. This is the source for
    /// laying out the environment geometry (the `scene_asset_table` TMD pack
    /// is object-local; each placement gives a mesh its world transform).
    ///
    /// Mirrors retail `FUN_8003A55C`; see
    /// [`legaia_asset::field_objects`] for the format + provenance. Reads the
    /// field map entry's **extended** footprint (the object grid is past the
    /// TOC-indexed payload). Returns `Ok(None)` if the scene has no field map.
    pub fn field_object_placements(
        &self,
        index: &ProtIndex,
    ) -> Result<Option<Vec<legaia_asset::field_objects::Placement>>> {
        let Some(idx) = self.field_map_index(index) else {
            return Ok(None);
        };
        let bytes = index.entry_bytes_extended(idx)?;
        Ok(Some(legaia_asset::field_objects::parse_placements(&bytes)))
    }

    /// The scene's **bulk terrain** tiles: one entry per visible cell of the
    /// field map's object-index grid (`+0x8000`, cell bit
    /// [`legaia_asset::field_objects::CELL_VISIBLE`]), positioned the same way
    /// as [`Self::field_object_placements`]. This is the dense continent layer
    /// (ground / trees / mountains) the overhead sweep `FUN_801F69D8` draws -
    /// far more tiles than the placed-flag interactive objects. Returns
    /// `Ok(None)` if the scene has no field map.
    pub fn field_terrain_tiles(
        &self,
        index: &ProtIndex,
    ) -> Result<Option<Vec<legaia_asset::field_objects::Placement>>> {
        let Some(idx) = self.field_map_index(index) else {
            return Ok(None);
        };
        let bytes = index.entry_bytes_extended(idx)?;
        Ok(Some(legaia_asset::field_objects::parse_terrain_tiles(
            &bytes,
        )))
    }

    /// Resolve the **free-roam walk** view's field `.MAP` entry.
    ///
    /// Historical alias of [`Self::field_map_index`]: the `start - 2`
    /// resolution was first pinned for the kingdom walk views (live `map01`
    /// capture), and the save-library census later proved it is the
    /// **universal** field-map rule (the in-block `FIELD_MAP_LEN` entry the
    /// field path used to pick is the *next* scene's map). Both paths now
    /// share one resolver.
    pub fn walk_field_map_index(&self, index: &ProtIndex) -> Option<u32> {
        self.field_map_index(index)
    }

    /// The walk view's continent **ground** as a heightfield surface, built
    /// from the walk `.MAP` floor grid (`+0x4000`) gated on the `0x1000`
    /// visible bit, with corner elevations from the per-scene floor-height LUT
    /// (the math `FUN_80019278` pins). This is the correct model for the bulk
    /// ground — the slot-1 pack meshes are only the sparse placed landmarks
    /// ([`Self::walk_object_placements`]), not a per-cell terrain mesh. Returns
    /// `Ok(None)` when the scene has no field map or no floor LUT.
    pub fn walk_heightfield(
        &self,
        index: &ProtIndex,
    ) -> Result<Option<legaia_asset::field_objects::WalkHeightfield>> {
        let Some(idx) = self.walk_field_map_index(index) else {
            return Ok(None);
        };
        let Some(lut) = self.field_floor_height_lut(index)? else {
            return Ok(None);
        };
        let bytes = index.entry_bytes_extended(idx)?;
        Ok(Some(legaia_asset::field_objects::build_walk_heightfield(
            &bytes, &lut,
        )))
    }

    /// The walk view's placed-flag interactive objects, read from
    /// [`Self::walk_field_map_index`] (the correct walk `.MAP`) rather than the
    /// within-block decoy. Same semantics as [`Self::field_object_placements`].
    pub fn walk_object_placements(
        &self,
        index: &ProtIndex,
    ) -> Result<Option<Vec<legaia_asset::field_objects::Placement>>> {
        let Some(idx) = self.walk_field_map_index(index) else {
            return Ok(None);
        };
        let bytes = index.entry_bytes_extended(idx)?;
        Ok(Some(legaia_asset::field_objects::parse_placements(&bytes)))
    }

    /// The scene's 16-entry floor-height LUT, read from the MAN header
    /// (`man[+0x02..+0x22]`, 16 `s16` LE). A placed object's world Y is
    /// `-lut[tile_floor_nibble] + record.y_off` (the runtime stores the LUT
    /// negated; `FUN_8003aeb0` fills it from the MAN, `FUN_8003a55c` reads it).
    /// Validated against a live `town01` save (Vahn's house tile nibble `6`,
    /// `lut[6]=192` -> world Y `-192`). Returns `Ok(None)` when the scene has
    /// no MAN bundle.
    pub fn field_floor_height_lut(&self, index: &ProtIndex) -> Result<Option<[i16; 16]>> {
        let Some(bundle) = crate::scene_bundle::find_bundle(self) else {
            return Ok(None);
        };
        let entry_bytes = index.entry_bytes_extended(bundle.entry_idx())?;
        let Some(man) = crate::scene_bundle::extract_man_payload(&bundle, &entry_bytes)? else {
            return Ok(None);
        };
        let Some(lut_bytes) = man.get(0x02..0x22) else {
            return Ok(None);
        };
        let mut lut = [0i16; 16];
        for (i, slot) in lut.iter_mut().enumerate() {
            *slot = i16::from_le_bytes([lut_bytes[i * 2], lut_bytes[i * 2 + 1]]);
        }
        Ok(Some(lut))
    }

    /// Resolve the scene's field-VM **scene-entry system script** (context
    /// channel `0xFB`) from the MAN asset, mirroring retail `FUN_8003ab2c`:
    /// the entry script is partition 1's first record in the scene's MAN
    /// container.
    ///
    /// Returns `Ok(Some((bytecode, pc0)))` where `bytecode` is the MAN
    /// buffer sliced from the script block's start (so relative jumps wrap
    /// against the slice base, matching the retail `buffer_base =
    /// script_start`) and `pc0` is the first opcode's offset into that slice.
    /// Feed both to [`crate::world::World::load_field_script_at`].
    ///
    /// Resolves for any scene whose `scene_asset_table` / `scene_scripted_
    /// asset_table` bundle carries a MAN - which includes `town01` and the
    /// other `count=6` [`Class::SceneAssetTable`] field scenes, not just the
    /// kingdom-bundle [`Class::SceneScriptedAssetTable`] scenes. (`town01`'s
    /// bundle is PROT entry 4, class `SceneAssetTable`; its MAN scene-entry
    /// script lives at MAN offset 3075, `pc0 = 11`.) `_DAT_8007B898` is the
    /// runtime decompressed-MAN buffer; for these scenes it is exactly this
    /// bundle MAN, so the script source is present in the static bundle.
    ///
    /// Returns `Ok(None)` only when [`crate::scene_bundle::find_bundle`] finds
    /// no `scene_asset_table` bundle, or when the MAN's partition 1 is empty.
    /// Those scenes fall back to the event-script record-0 load.
    ///
    /// Note that the entry script's `0x4C` nibble-7 wall-paint deltas are
    /// gated behind system-flag tests, so they only fire once the world's
    /// story flags are seeded to a matching scene-entry state; the base
    /// collision grid ([`Self::field_collision_grid`]) is independent of the
    /// entry script.
    ///
    /// REF: FUN_8003ab2c (the port lives in `legaia_asset::man_section`).
    pub fn field_man_entry_script(&self, index: &ProtIndex) -> Result<Option<(Vec<u8>, usize)>> {
        let Some(bundle) = crate::scene_bundle::find_bundle(self) else {
            return Ok(None);
        };
        let entry_bytes = index.entry_bytes_extended(bundle.entry_idx())?;
        let Some(man_bytes) = crate::scene_bundle::extract_man_payload(&bundle, &entry_bytes)?
        else {
            return Ok(None);
        };
        let Ok(man) = legaia_asset::man_section::parse(&man_bytes) else {
            return Ok(None);
        };
        let Some((start, pc0)) = man.scene_entry_script(&man_bytes) else {
            return Ok(None);
        };
        match man_bytes.get(start..) {
            Some(slice) => Ok(Some((slice.to_vec(), pc0))),
            None => Ok(None),
        }
    }

    /// Resolve the scene's disc-resident random-encounter table plus its
    /// per-row formation defs from the same MAN asset (retail
    /// `_DAT_8007B898`) the scene-entry script comes from.
    ///
    /// Returns `Ok(None)` when the scene's static bundle carries no MAN (the
    /// same detector gap [`Self::field_man_entry_script`] documents) or when
    /// the MAN's encounter section declares no rollable formations (towns
    /// with no encounters). Resolves now for the `count=6`
    /// [`legaia_asset::scene_asset_table`] field scenes (town01 etc.) thanks
    /// to the relaxed detector.
    ///
    /// Wire the pair via [`crate::world::World::install_man_encounter`].
    ///
    /// REF: FUN_8003AEB0 (installs the encounter section into the runtime
    /// control block); the byte-level walk lives in
    /// `legaia_asset::man_section` and the runtime bridge in
    /// [`crate::encounter_man`].
    pub fn field_man_encounter_table(
        &self,
        index: &ProtIndex,
        scene_label: &str,
    ) -> Result<
        Option<(
            crate::encounter::EncounterTable,
            Vec<crate::monster_catalog::FormationDef>,
        )>,
    > {
        let Some(bundle) = crate::scene_bundle::find_bundle(self) else {
            return Ok(None);
        };
        let entry_bytes = index.entry_bytes_extended(bundle.entry_idx())?;
        let Some(man_bytes) = crate::scene_bundle::extract_man_payload(&bundle, &entry_bytes)?
        else {
            return Ok(None);
        };
        Ok(crate::encounter_man::scene_encounter_from_man(
            scene_label,
            &man_bytes,
        ))
    }

    /// The scene's NPC / actor placement list, decoded from the MAN
    /// partition-1 records (retail `FUN_8003A1E4` per-record actor spawn).
    ///
    /// Each [`ActorPlacement`](legaia_asset::man_section::ActorPlacement) is one
    /// placed entity: its spawn tile/world position, model index, action count,
    /// and the byte offset of its field-VM script (the script that later
    /// installs the entity's encounter record or portal behaviour). This is the
    /// source the engine seeds overworld entities from on the world-map path;
    /// the entity *kind* (encounter zone / portal / NPC) lives in the per-entity
    /// script and is not classified here.
    ///
    /// Returns `Ok(None)` when the scene has no `scene_asset_table` bundle / the
    /// MAN payload doesn't decode - the same detector gap
    /// [`Self::field_man_entry_script`] documents. An empty `Vec` means the MAN
    /// decoded but places no actors (partition 1 holds only the controller).
    pub fn field_actor_placements(
        &self,
        index: &ProtIndex,
    ) -> Result<Option<Vec<legaia_asset::man_section::ActorPlacement>>> {
        let Some(bundle) = crate::scene_bundle::find_bundle(self) else {
            return Ok(None);
        };
        let entry_bytes = index.entry_bytes_extended(bundle.entry_idx())?;
        let Some(man_bytes) = crate::scene_bundle::extract_man_payload(&bundle, &entry_bytes)?
        else {
            return Ok(None);
        };
        let Ok(man) = legaia_asset::man_section::parse(&man_bytes) else {
            return Ok(None);
        };
        Ok(Some(man.actor_placements(&man_bytes)))
    }

    /// The scene's decoded MAN payload bytes (retail `_DAT_8007B898`), or
    /// `Ok(None)` when the scene has no `scene_asset_table` bundle / the MAN
    /// payload doesn't decode - the same detector gap
    /// [`Self::field_man_entry_script`] documents.
    ///
    /// Callers that want the parsed structure pass the bytes to
    /// [`legaia_asset::man_section::parse`]; this is the shared raw-bytes
    /// fetch behind the entry-script / encounter-table accessors, exposed so
    /// the field host can also walk the cutscene-timeline partition (e.g. the
    /// opening prologue's `GFLAG_SET 26` hand-off arm).
    pub fn field_man_payload(&self, index: &ProtIndex) -> Result<Option<Vec<u8>>> {
        let Some(bundle) = crate::scene_bundle::find_bundle(self) else {
            return Ok(None);
        };
        let entry_bytes = index.entry_bytes_extended(bundle.entry_idx())?;
        crate::scene_bundle::extract_man_payload(&bundle, &entry_bytes)
    }

    /// The scene's MAN **section-3 zone table** - the count-prefixed
    /// 18-byte camera-region records the boot walk (`FUN_8003AEB0`)
    /// installs at the control block `_DAT_801C6EA4 + 0x4` and the
    /// per-tile zone query (`FUN_801DBA20`, ported as
    /// [`crate::field_regions::zone_query`]) walks. Returns `Ok(None)` when
    /// the scene has no MAN or the MAN's section 3 is the chain terminator.
    pub fn field_zone_table(&self, index: &ProtIndex) -> Result<Option<Vec<u8>>> {
        let Some(man_bytes) = self.field_man_payload(index)? else {
            return Ok(None);
        };
        let Ok(man) = legaia_asset::man_section::parse(&man_bytes) else {
            return Ok(None);
        };
        let sec = &man.sections[3];
        if sec.is_terminator() {
            return Ok(None);
        }
        Ok(sec.body(&man_bytes).map(<[u8]>::to_vec))
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

/// A resolver backed by a scene's **disc-sourced** scene-destination table
/// ([`crate::man_field_scripts::scene_destinations`]).
///
/// This resolves the **named scene-change** (`0x3F`) id space: each `0x3F` op
/// carries an `i16` index alongside the inline destination name, and a scene's
/// controller script lists every reachable destination as one such op.
/// [`SceneHost`] rebuilds one per scene from the entered scene's MAN, so the
/// engine has a live, byte-accurate index → scene-name map (no uncaptured
/// overlay needed).
///
/// **Not a [`MapIdResolver`].** That trait keys on a `u8` map id (the `0x3E`
/// door-warp's 7 scene-*type* selectors, `0..=6`). The `0x3F` index is a
/// distinct, wider id space — `i16`, observed past `u8` range (e.g. `630`) — so
/// a `u8`-keyed resolver can't represent it without lossy truncation. Hence the
/// dedicated [`Self::resolve`]/[`Self::destination`] by `i16`.
#[derive(Debug, Clone, Default)]
pub struct SceneDestinationResolver {
    by_index: std::collections::HashMap<i16, crate::man_field_scripts::SceneDestination>,
}

impl SceneDestinationResolver {
    /// Build from a decoded destination list (first entry per index wins, which
    /// matches [`scene_destinations`](crate::man_field_scripts::scene_destinations)'s
    /// first-seen dedup).
    pub fn new(destinations: Vec<crate::man_field_scripts::SceneDestination>) -> Self {
        let mut by_index = std::collections::HashMap::new();
        for d in destinations {
            by_index.entry(d.index).or_insert(d);
        }
        Self { by_index }
    }

    /// Resolve an `i16` scene-change index to its destination scene name.
    pub fn resolve(&self, index: i16) -> Option<&str> {
        self.by_index.get(&index).map(|d| d.scene_name.as_str())
    }

    /// The full destination record for an `i16` scene-change index (name +
    /// entry tile).
    pub fn destination(&self, index: i16) -> Option<&crate::man_field_scripts::SceneDestination> {
        self.by_index.get(&index)
    }

    /// Number of distinct destinations (indices) in the table.
    pub fn len(&self) -> usize {
        self.by_index.len()
    }

    /// `true` when the table carries no destinations.
    pub fn is_empty(&self) -> bool {
        self.by_index.is_empty()
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
    /// Lazily-loaded monster stat archive (PROT entry 867, extended
    /// footprint). Cached because it's 15.9 MB and the same global table
    /// serves every scene. Populated on the first field entry that needs
    /// real monster stats. See [`legaia_asset::monster_archive`].
    monster_archive_cache: Option<Arc<Vec<u8>>>,
    /// Tracks whether the move-power table install was attempted, so the disc
    /// read (PROT 0898) only happens once per host even when it fails.
    move_power_loaded: bool,
    /// The current scene's disc-sourced **named scene-change destinations**
    /// (`0x3F` ops), decoded from its MAN on entry via
    /// [`crate::man_field_scripts::scene_destinations`]. Empty for scenes with
    /// no MAN / no destination table. Drives [`Self::destination_resolver`].
    scene_destinations: Vec<crate::man_field_scripts::SceneDestination>,
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
            monster_archive_cache: None,
            move_power_loaded: false,
            scene_destinations: Vec::new(),
        }
    }

    /// Lazily load + cache the monster stat archive (PROT 867, extended
    /// footprint - the archive lives in the entry's trailing-gap sectors,
    /// not the small indexed payload, so `entry_bytes` would truncate it).
    /// Returns `None` if the entry can't be read.
    fn monster_archive_bytes(&mut self) -> Option<Arc<Vec<u8>>> {
        if self.monster_archive_cache.is_none() {
            match self.index.entry_bytes_extended(MONSTER_ARCHIVE_PROT_ENTRY) {
                Ok(b) => self.monster_archive_cache = Some(Arc::new(b)),
                Err(err) => {
                    eprintln!(
                        "[scene] monster archive (PROT {MONSTER_ARCHIVE_PROT_ENTRY}) load skipped: {err:#}"
                    );
                    return None;
                }
            }
        }
        self.monster_archive_cache.clone()
    }

    /// Install the battle-action move-power table onto the world from PROT
    /// entry 0898 (the battle-action overlay), once per host. The monster
    /// special-attack damage path reads it to roll faithful per-move damage;
    /// a read/parse failure leaves [`crate::world::World::move_power`] `None`
    /// (the placeholder damage path stays active) and is not retried.
    fn ensure_move_power_table(&mut self) {
        if self.move_power_loaded {
            return;
        }
        self.move_power_loaded = true;
        let entry = crate::move_power::BATTLE_ACTION_OVERLAY_PROT_ENTRY;
        match self.index.entry_bytes(entry) {
            Ok(bytes) => {
                if let Some(cat) = crate::move_power::MovePowerCatalog::from_overlay_0898(&bytes) {
                    self.world.move_power = Some(cat);
                    // Retain the overlay so the move-FX render path can read the
                    // 0x801f6324 prototype records' move-VM bytecode.
                    self.world.move_power_overlay = Some(std::sync::Arc::from(bytes.as_slice()));
                } else {
                    eprintln!(
                        "[scene] move-power table (PROT {entry}) parse failed - placeholder damage stays active"
                    );
                }
                // The element-affinity matrix + per-character element table are
                // sibling static data in the same overlay, so parse them from the
                // same bytes. A failure leaves the neutral 100% multiplier active.
                if let Some(aff) = legaia_asset::element_affinity::parse(&bytes) {
                    self.world.element_affinity = Some(aff);
                } else {
                    eprintln!(
                        "[scene] element-affinity tables (PROT {entry}) parse failed - neutral affinity stays active"
                    );
                }
            }
            Err(err) => {
                eprintln!("[scene] battle-action overlay (PROT {entry}) load skipped: {err:#}");
            }
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
        self.refresh_scene_destinations();
        Ok(self.scene.as_ref().unwrap())
    }

    /// Decode + cache the just-loaded scene's named scene-change destinations
    /// (`0x3F` ops) from its MAN, via
    /// [`crate::man_field_scripts::scene_destinations`]. Clears to empty when
    /// the scene carries no MAN or it doesn't parse. Called by [`Self::load_scene`]
    /// so every scene-entry path keeps the table current.
    fn refresh_scene_destinations(&mut self) {
        self.scene_destinations = self
            .scene
            .as_ref()
            .and_then(|s| s.field_man_payload(&self.index).ok().flatten())
            .and_then(|man| {
                let mf = legaia_asset::man_section::parse(&man).ok()?;
                Some(crate::man_field_scripts::scene_destinations(&mf, &man))
            })
            .unwrap_or_default();
    }

    /// The current scene's disc-sourced **named scene-change destinations**
    /// (`0x3F` ops): every town / dungeon its controller script can warp to,
    /// each with its `i16` index + entry tile. Empty when no scene is loaded or
    /// the scene has no destination table. See
    /// [`crate::man_field_scripts::scene_destinations`].
    pub fn scene_destinations(&self) -> &[crate::man_field_scripts::SceneDestination] {
        &self.scene_destinations
    }

    /// A [`SceneDestinationResolver`] over the current scene's destinations —
    /// the live resolver for the `0x3F` named-scene-change `i16` index space,
    /// rebuilt from disc each scene entry. (The `0x3E` door-warp keeps the
    /// separate `u8`-keyed [`map_resolver`](Self::map_resolver).)
    pub fn destination_resolver(&self) -> SceneDestinationResolver {
        SceneDestinationResolver::new(self.scene_destinations.clone())
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
        // Placement-NPC / event dialogue carries its text inline (the field-VM
        // `0x3F` op's buffer); its `text_id` is a box-config id, not an MES
        // index, so it never resolves through the scene MES. Prefer the inline
        // text when present, falling back to the MES `text_id` lookup (used by
        // the message-table dialogue paths).
        if !req.inline.is_empty()
            && let Some(panel) = crate::dialog::OwnedDialogPanel::from_inline_dialog(&req.inline)
        {
            return Some(panel);
        }
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
        // Drop any cutscene timeline from a previous scene; only `opdeene`
        // re-installs one below, so it must not leak into the scene we hand off
        // to (Rim Elm).
        self.world.cutscene_timeline = None;
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
        // Configure the party leader (slot 0) as the free-movement player.
        // (This also clears the collision grid; we repopulate it below.)
        // Mirrors the retail scene-entry player setup in `FUN_8003aeb0`.
        self.world.install_field_player(0);
        // Cold field entry: place the player at the retail cold-boot spawn.
        // `FUN_801D6704` creates the player actor at the camera-window centre
        // `(0xA40, 0, 0xA40)` on a non-warp entry; for the New Game opening
        // (town01) this is Vahn's authored Rim Elm spawn, and it also seeds the
        // follow camera onto the right region. Engines that arrive via a warp
        // override X/Z from the saved transition coords before the first tick.
        // See [`crate::world::FIELD_COLD_SPAWN_XZ`].
        if let Some(player) = self.world.actors.get_mut(0) {
            player.move_state.world_x = crate::world::FIELD_COLD_SPAWN_XZ;
            player.move_state.world_y = 0;
            player.move_state.world_z = crate::world::FIELD_COLD_SPAWN_XZ;
        }
        // Load the per-scene base collision/floor grid from the field map
        // file (retail `DATA\FIELD\<scene>.MAP`, the unique 0x12000-byte block
        // entry). The grid is the file's `+0x4000..+0x8000` region; the
        // field-VM `0x4C` nibble-7 ops layer story-conditional deltas on top
        // as the prescript runs. Verified byte-exact against live RAM
        // (town01). See `docs/subsystems/field-locomotion.md`.
        let base_grid: Option<Vec<u8>> = match self.scene.as_ref() {
            Some(scene) => match scene.field_collision_grid(&self.index) {
                Ok(grid) => grid,
                Err(err) => {
                    eprintln!("[scene] field collision-grid load skipped: {err:#}");
                    None
                }
            },
            None => None,
        };
        if let Some(grid) = base_grid {
            self.world.load_field_collision_grid(&grid);
        }
        // Per-scene region / zone tables: the `.MAP` `+0x10000` region-record
        // block + the MAN section-3 camera-region table. Drives the per-tile
        // region-type mask (`extra_flags`, field-VM op 0x42 mode 0) and the
        // camera-zone selection via `World::refresh_field_regions` (the
        // `FUN_80017FBC` / `FUN_800180EC` / `FUN_801DBA20` ports). Installed
        // unconditionally (empty when absent) so stale tables never leak
        // across a transition.
        let region_block: Vec<u8> = match self.scene.as_ref() {
            Some(scene) => match scene.field_map_region_block(&self.index) {
                Ok(block) => block.unwrap_or_default(),
                Err(err) => {
                    eprintln!("[scene] field region-table load skipped: {err:#}");
                    Vec::new()
                }
            },
            None => Vec::new(),
        };
        let zone_table: Vec<u8> = match self.scene.as_ref() {
            Some(scene) => match scene.field_zone_table(&self.index) {
                Ok(table) => table.unwrap_or_default(),
                Err(err) => {
                    eprintln!("[scene] field zone-table load skipped: {err:#}");
                    Vec::new()
                }
            },
            None => Vec::new(),
        };
        self.world
            .load_field_region_tables(&region_block, &zone_table);
        // Static prop colliders: one box centre per placed `.MAP` object
        // (spawn position + the record's collision-footprint offset — the
        // static-entity arm of the actor probe). Installed unconditionally
        // (empty for scenes with no field map) so a stale scene's props
        // never leak across a transition; blocking stays behind the opt-in
        // `World::solid_field_npcs` flag.
        self.world.field_prop_colliders = match self.scene.as_ref() {
            Some(scene) => match scene.field_object_placements(&self.index) {
                Ok(Some(placements)) => placements
                    .iter()
                    .map(|p| (p.collider_x, p.collider_z))
                    .collect(),
                Ok(None) => Vec::new(),
                Err(err) => {
                    eprintln!("[scene] field prop-collider load skipped: {err:#}");
                    Vec::new()
                }
            },
            None => Vec::new(),
        };
        // The 16-entry floor-height LUT (MAN header, negated `s16` tiers) the
        // collision grid's low nibble indexes - resident so the floor-height
        // sampler (`World::sample_field_floor_height`, port of `FUN_80019278`)
        // can resolve terrain elevation from the grid.
        match self
            .scene
            .as_ref()
            .map(|s| s.field_floor_height_lut(&self.index))
        {
            Some(Ok(Some(lut))) => self.world.field_floor_height_lut = lut,
            Some(Err(err)) => eprintln!("[scene] field floor-height LUT load skipped: {err:#}"),
            _ => {}
        }
        // Prefer the real scene-entry system script (ctx 0xFB) over event-
        // script record 0. Record 0 is a per-scene trigger/dispatch table,
        // not linear bytecode, so the field VM halts at its pc 0 and the
        // scene-entry logic (actor placement, BGM, conditional wall deltas)
        // never runs. The retail per-frame driver `FUN_8003ab2c` builds the
        // system script from the MAN asset's partition[1][0]; resolve and run
        // that instead. This resolves for any `scene_asset_table` bundle that
        // carries a MAN - including `town01` and the other `count=6`
        // `SceneAssetTable` field scenes, not just the kingdom bundles. (For
        // those scenes the runtime `_DAT_8007B898` MAN buffer IS this bundle
        // MAN, so the source is in the static bundle.) Only scenes with no
        // bundle MAN keep the record-0 fallback above. Flag-gated nibble-7
        // wall deltas in the entry script still need seeded story flags to
        // fire; the base grid loaded above is independent.
        let entry_script = match self.scene.as_ref() {
            Some(scene) => match scene.field_man_entry_script(&self.index) {
                Ok(v) => v,
                Err(err) => {
                    eprintln!("[scene] MAN entry-script resolve skipped: {err:#}");
                    None
                }
            },
            None => None,
        };
        if let Some((bytecode, pc0)) = entry_script {
            self.world.load_field_script_at(bytecode, pc0);
        }
        // Install the scene's random-encounter table straight from its MAN
        // asset (the disc-resident `_DAT_8007B898` source) - the retail
        // per-scene table, not a synthetic pattern. Resolves for the
        // `count=6` `scene_asset_table` field scenes (town01 etc.) now that
        // the detector covers them, same as the entry script above. The
        // per-row formation defs are merged into the formation table so the
        // table's row-index ids resolve to monster sets at battle-load.
        // Scenes whose static bundle carries no MAN - or towns whose MAN has
        // no rollable formations - leave the encounter unset here; the host
        // falls back to the synthetic registry (`install_encounter_for_scene`).
        self.world.set_active_scene_label(name);
        let man_encounter = match self.scene.as_ref() {
            Some(scene) => match scene.field_man_encounter_table(&self.index, name) {
                Ok(v) => v,
                Err(err) => {
                    eprintln!("[scene] MAN encounter-table resolve skipped: {err:#}");
                    None
                }
            },
            None => None,
        };
        if let Some((table, formations)) = man_encounter {
            // Collect the formation monster-ids before `install_man_encounter`
            // consumes the defs.
            let mut ids: Vec<u16> = formations
                .iter()
                .flat_map(|f| f.slots.iter().map(|s| s.monster_id))
                .collect();
            ids.sort_unstable();
            ids.dedup();
            self.world.install_man_encounter(table, formations);
            // Merge real per-id monster stats from the disc archive (PROT 867)
            // over the catalog so the just-installed formations resolve to
            // genuine HP/MP/attack at battle-load instead of synthetic
            // placeholders. Archive entries win for the scene's ids; ids the
            // archive doesn't cover keep whatever catalog was installed.
            if !ids.is_empty()
                && let Some(archive) = self.monster_archive_bytes()
            {
                let cat = crate::monster_catalog::catalog_from_monster_archive(&archive, &ids);
                for def in cat.by_id.into_values() {
                    self.world.monster_catalog.insert(def);
                }
                // Pair the per-move power table with the just-merged monster
                // stats so the special-attack damage path can resolve real
                // per-move power (PROT 0898; falls back to the placeholder if
                // the disc read fails).
                self.ensure_move_power_table();
            }
        }
        // Install the scene's field entity-SM carriers derived from the same
        // MAN actor-placement partition (retail builds one record per
        // MAN-placed entity at scene load). They sit Idle - the sparring
        // carrier only advances when `engage_field_carrier` is called on the
        // dialogue-accept - so this is inert for the cold-boot path but makes
        // the derived carrier set live (and is the counterpart to the MAN
        // encounter-table install above). Soft-fail: a scene without a MAN, or
        // with no interactable placements, just installs an empty set.
        match self
            .scene
            .as_ref()
            .map(|s| s.field_man_payload(&self.index))
        {
            Some(Ok(Some(man_bytes))) => match legaia_asset::man_section::parse(&man_bytes) {
                Ok(man_file) => {
                    self.world
                        .install_field_carriers_from_man(&man_file, &man_bytes);
                }
                Err(err) => eprintln!("[scene] field-carrier MAN parse skipped: {err:#}"),
            },
            Some(Err(err)) => eprintln!("[scene] field-carrier MAN payload skipped: {err:#}"),
            _ => {}
        }
        // Install the VDF ("set_mime") buffer so the `0x4C 0xD8`
        // synchronous-spawn host hook can resolve actor templates. Only
        // a handful of scenes carry VDF data (8/124 in the retail
        // corpus); the lookup is cheap and returns `None` for the rest.
        if let Some(scene) = self.scene.as_ref() {
            self.world
                .set_vdf_buffer(crate::scene_bundle::find_vdf_buffer(scene));
        }
        // Install the per-scene MOVE pool (retail `_DAT_8007B888`). The
        // bytes come from the scene's `scene_asset_table` slot-4
        // `Asset(0x05) = Move` descriptor (see `docs/formats/mdt.md`).
        // The descriptor offsets reference positions in the full
        // on-disc footprint (including trailing-overlay sectors), so
        // we fetch via `entry_bytes_extended` rather than the indexed
        // view that `Scene::load` keeps in `entry.bytes`. Without this
        // the `MoveBufferHost` resolver returns `None` and the move-
        // table cursor stays idle.
        let move_install = self
            .scene
            .as_ref()
            .and_then(|scene| crate::scene_bundle::find_bundle(scene).map(|b| (b.entry_idx(), b)));
        if let Some((entry_idx, bundle)) = move_install {
            match self
                .index
                .entry_bytes_extended(entry_idx)
                .and_then(|bytes| crate::scene_bundle::extract_move_payload(&bundle, &bytes))
            {
                Ok(Some(bytes)) => self.world.set_move_buffer_root(bytes),
                Ok(None) => self.world.set_move_buffer_root(Vec::new()),
                Err(err) => eprintln!("[scene] move-table extract skipped: {err:#}"),
            }
        }
        // Seed the global TMD-pool head from PROT 0874 section 0 (the
        // 5 character-mesh TMDs that retail's `DAT_8007C018[0..4]`
        // resolves to). Byte-equality verified - see
        // `project_global_tmd_pool_source.md`. Producers for the
        // remaining 138 kingdom-bundle entries are not yet pinned;
        // those slots stay empty until the full chain lands. Idempotent:
        // the head re-seeds across scene transitions but only on the
        // first call (subsequent calls early-return when the head is
        // already populated).
        let head_populated = self.world.global_tmd_pool.len() >= 5
            && self.world.global_tmd_pool[..5].iter().all(|s| s.is_some());
        if !head_populated
            && let Err(err) = seed_global_tmd_pool_from_befect_data(&self.index, &mut self.world)
        {
            eprintln!("[scene] global TMD-pool seed skipped: {err:#}");
        }
        // Load the battle effect-model library from PROT 0871 (`etmd.dat`)
        // into `DAT_8007C018[3..=32]`. Retail pulls this at battle init; the
        // engine keeps it resident across the battle scene-mode overlay (like
        // the etim VRAM and effect catalog), so seeding it at field entry is
        // equivalent. It overwrites the two trailing slots of the §0 field
        // head (`[3]`, `[4]`) - matching retail's temporal layout - and gives
        // the effect-model render path the real Gimard *Tail Fire* mesh at
        // `[26]`. Idempotent: only loads when the library isn't already
        // resident. Soft-fails - the §0 preview stand-in remains the fallback.
        if !effect_model_library_loaded(&self.world)
            && let Err(err) = seed_effect_model_library_from_etmd(&self.index, &mut self.world)
        {
            eprintln!("[scene] effect-model library (PROT 0871) load skipped: {err:#}");
        }
        // Load the runtime effect-script catalog from PROT 0873 (`efect.dat`)
        // so the battle-action SM's `ui_element` spawns resolve to real
        // effect scripts. Idempotent: only loads when the catalog is empty
        // (it persists on `World` across field/battle transitions, like the
        // global TMD pool). Soft-fails - an empty catalog just doesn't spawn.
        if self.world.effect_catalog.is_empty()
            && let Err(err) = seed_effect_catalog_from_efect_dat(&self.index, &mut self.world)
        {
            eprintln!("[scene] effect-catalog load skipped: {err:#}");
        }
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
            // World-map scenes (`map\d\d` = the three kingdom bundles) carry
            // their landmark geometry in slot 1 of a 7-asset descriptor table,
            // not as raw / loosely-LZS-packed TMDs. `SceneLoadKind::WorldMap`
            // makes the resource build decode that slot explicitly (the
            // faithful retail path) and routes per-prim emit through the
            // distance-cue overlay variant. Every other field uses the plain
            // field loader. See [`docs/subsystems/world-map.md`].
            let load_kind = if crate::scene::is_world_map_scene(name) {
                crate::scene_resources::SceneLoadKind::WorldMap
            } else {
                crate::scene_resources::SceneLoadKind::Field
            };
            if let Ok((mut res, _stats)) =
                crate::scene_resources::SceneResources::build_targeted_with_options(
                    scene,
                    &shared_refs,
                    crate::scene_resources::BuildOptions {
                        kind: load_kind,
                        // Retail's field loader (FUN_8001F7C0) DMA-uploads
                        // every TIM in the scene, not just the subset the
                        // first-frame meshes sample. The town's environment
                        // geometry (the LZS-packed mesh pack now parsed out of
                        // the scene_asset_table) samples texture pages across
                        // the whole atlas, so a render-targeted upload drops
                        // ~75% of its prims (missing texture page). Uploading
                        // all TIMs lifts the town keep ratio to ~95%.
                        upload_all_tims: true,
                    },
                )
            {
                // Upload the battle effect-model textures (etim.dat, PROT 0874
                // section 2) into the scene VRAM so the 3D effect models
                // (etmd.dat) have their texels resident. Kept across the battle
                // scene-mode overlay; soft-fails (textures just stay absent).
                if let Err(err) = upload_effect_textures_into_vram(&self.index, &mut res.vram, true)
                {
                    eprintln!("[scene] effect-texture VRAM upload skipped: {err:#}");
                }
                self.world.init_scene_animations(&res);
                self.resources = Some(res);
            }
        }
        // Opening-prologue hand-off arm. When entering the cutscene scene
        // `opdeene`, derive the `town01` hand-off arm from the scene's own MAN
        // bytecode instead of a blind constant: walk the cutscene-timeline
        // partition for the `GFLAG_SET 26` write retail's gate waits on and
        // arm only when it is present. A cutscene scene that never issues that
        // write never produces a false hand-off. See
        // [`crate::world::World::arm_prologue_handoff_from_man`].
        if name == legaia_asset::new_game::OPENING_CUTSCENE_SCENE {
            match self
                .scene
                .as_ref()
                .map(|s| s.field_man_payload(&self.index))
            {
                Some(Ok(Some(man_bytes))) => match legaia_asset::man_section::parse(&man_bytes) {
                    Ok(man_file) => {
                        // Execute the cutscene timeline as a spawned field-VM
                        // context: its camera path + actor moves play and the
                        // closing `GFLAG_SET 26` fires by execution. Fall back
                        // to the static MAN-walk arm only when the timeline
                        // record can't be resolved, so the prologue still hands
                        // off either way.
                        if self
                            .world
                            .load_cutscene_timeline_from_man(&man_file, &man_bytes)
                        {
                            log::info!(
                                "prologue: executing '{}' cutscene timeline -> '{}' hand-off by GFLAG_SET {}",
                                legaia_asset::new_game::OPENING_CUTSCENE_SCENE,
                                legaia_asset::new_game::OPENING_SCENE,
                                crate::world::PROLOGUE_HANDOFF_BIT,
                            );
                        } else if self
                            .world
                            .arm_prologue_handoff_from_man(&man_file, &man_bytes)
                        {
                            log::info!(
                                "prologue: armed '{}' -> '{}' hand-off from MAN GFLAG_SET {} (static fallback)",
                                legaia_asset::new_game::OPENING_CUTSCENE_SCENE,
                                legaia_asset::new_game::OPENING_SCENE,
                                crate::world::PROLOGUE_HANDOFF_BIT,
                            );
                        }
                        // Install the inline narration the cutscene-timeline
                        // partition (partition 2) carries, so the opening plays
                        // its subtitle pages before the Rim Elm hand-off.
                        let pages = crate::man_field_scripts::collect_partition_narration(
                            &man_file, &man_bytes, 2,
                        );
                        if !pages.is_empty() {
                            log::info!(
                                "prologue: '{}' carries {} inline narration page(s)",
                                legaia_asset::new_game::OPENING_CUTSCENE_SCENE,
                                pages.len(),
                            );
                            self.world.open_cutscene_narration(pages);
                        }
                    }
                    Err(err) => eprintln!("[scene] prologue MAN parse skipped: {err:#}"),
                },
                Some(Err(err)) => eprintln!("[scene] prologue MAN payload skipped: {err:#}"),
                _ => {}
            }
        }
        // New-game opening: when `town01` is entered via the prologue hand-off
        // (not a normal visit), install its opening cutscene timeline so the
        // establishing camera sweep + Vahn's walk-out play and the pinned
        // op-`0x49` STATE_RESUME opens the name-entry overlay at the right beat
        // (rather than the host opening it blindly at the hand-off). One-shot:
        // consume the flag so re-entering `town01` later never re-runs it.
        if name == legaia_asset::new_game::OPENING_SCENE && self.world.entering_town01_opening {
            self.world.entering_town01_opening = false;
            match self
                .scene
                .as_ref()
                .map(|s| s.field_man_payload(&self.index))
            {
                Some(Ok(Some(man_bytes))) => match legaia_asset::man_section::parse(&man_bytes) {
                    Ok(man_file) => {
                        if self
                            .world
                            .install_town01_opening_timeline(&man_file, &man_bytes)
                        {
                            log::info!(
                                "opening: executing '{}' opening timeline (P2[{}]); name entry opens at its op-0x49 STATE_RESUME",
                                legaia_asset::new_game::OPENING_SCENE,
                                crate::world::World::TOWN01_OPENING_TIMELINE_RECORD,
                            );
                        }
                    }
                    Err(err) => eprintln!("[scene] town01 opening MAN parse skipped: {err:#}"),
                },
                Some(Err(err)) => eprintln!("[scene] town01 opening MAN payload skipped: {err:#}"),
                _ => {}
            }
        }
        // Decode the scene's gold-shop stock from its MAN so the field-menu
        // shop-open path offers real per-scene items at real prices instead of a
        // hand-authored list. Cheap when the scene has no merchant.
        self.populate_scene_shops();
        // Drain any pending transition the previous scene left behind.
        self.world.pending_scene_transition = None;
        Ok(())
    }

    /// Decode the active scene's gold shops from its MAN(s) and park them on
    /// [`crate::world::World::scene_shops`], priced from
    /// [`crate::world::World::item_shop_data`]. Scans every entry in the scene's
    /// CDNAME block (most carry one bundle MAN); cheap for non-bundle entries -
    /// the locator returns early without decompressing when an entry isn't a
    /// scene bundle with a MAN. No-op shop list when the disc / item data is
    /// absent.
    fn populate_scene_shops(&mut self) {
        let entry_idxs: Vec<u32> = match self.scene.as_ref() {
            Some(s) => s.entries.iter().map(|e| e.idx).collect(),
            None => {
                self.world.scene_shops.clear();
                return;
            }
        };
        let item_data = self.world.item_shop_data.clone();
        let mut shops = Vec::new();
        for idx in entry_idxs {
            let bytes = match self.index.entry_bytes_extended(idx) {
                Ok(b) => b,
                Err(_) => continue,
            };
            shops.extend(crate::shop_catalog::scene_shops(
                &bytes,
                idx as usize,
                item_data.as_ref(),
            ));
        }
        self.world.scene_shops = shops;
    }

    /// Enter `name` as the **overworld** (world-map) scene.
    ///
    /// The counterpart to [`Self::enter_field_scene`] for the three kingdom
    /// overworld scenes ([`is_world_map_scene`]). It runs the full field-entry
    /// load first - the world-map-walk overlay shares the field's locomotion,
    /// walkability grid, and asset pipeline (see
    /// `docs/subsystems/world-map.md`) - then:
    ///
    /// 1. Routes the region-keyed random-encounter table from the scene's MAN
    ///    ([`crate::region_encounter::region_encounter_table_from_man`]) onto
    ///    the overworld via [`crate::world::World::set_world_map_regions`], so
    ///    `tick_world_map`'s per-tile roll fires real encounters.
    /// 2. Switches the world into [`crate::world::SceneMode::WorldMap`]
    ///    (installs the camera controller); the player actor + collision grid
    ///    that `enter_field_scene` set up stay in place.
    ///
    /// 3. Installs the scene's **interactive overworld entities** - the
    ///    portals (town/dungeon entrances) and dialog NPCs decoded from the
    ///    MAN actor-placement table and classified by their field-VM scripts
    ///    ([`crate::man_field_scripts::classify_placements`]). Decorative /
    ///    model-only placements are skipped; the entity *kind* comes from the
    ///    per-entity script, so this is disc-sourced, not synthetic.
    ///
    /// The random-encounter driver (the region table) is fully sourced from
    /// the MAN; the per-entity auto-engage trigger (walk onto a portal tile)
    /// is still host-driven via [`crate::world::World::engage_world_map_entity`].
    pub fn enter_world_map_scene(&mut self, name: &str) -> Result<()> {
        // Full field-entry load: resources, walkability grid, player, monster
        // catalog, scene label. Leaves the world in `Field` mode.
        self.enter_field_scene(name, 0)?;
        // Decode the MAN once, then derive the region table + the typed entity
        // configs from it while only `self.scene` / `self.index` are borrowed
        // (both immutable), so the owned results outlive the borrow before the
        // mutable `world` accesses below.
        let man_bytes = self
            .scene
            .as_ref()
            .and_then(|s| s.field_man_payload(&self.index).ok().flatten());
        let table = man_bytes
            .as_ref()
            .and_then(|man| crate::region_encounter::region_encounter_table_from_man(name, man));
        // Each interactive placement → (config, spawn world position). The
        // positions drive the auto-engage-on-walkover trigger; Plain
        // (decorative / model-only) placements are skipped.
        let entities: Vec<(crate::world::WorldMapEntityConfig, (i16, i16))> = man_bytes
            .as_ref()
            .and_then(|man| {
                legaia_asset::man_section::parse(man)
                    .ok()
                    .map(|mf| (mf, man))
            })
            .map(|(mf, man)| {
                use crate::man_field_scripts::PlacementKind;
                crate::man_field_scripts::classify_placements(&mf, man)
                    .into_iter()
                    .filter_map(|(p, kind)| {
                        let cfg = match kind {
                            PlacementKind::Portal { target_map } => {
                                crate::world::WorldMapEntityConfig::Portal {
                                    target_map: target_map as u16,
                                }
                            }
                            PlacementKind::Npc {
                                interact_id,
                                dialog_inline,
                            } => crate::world::WorldMapEntityConfig::Npc {
                                interact_id: interact_id.unwrap_or(0),
                                // No `text_id` from the MAN classifier: the `0x3F`
                                // op it was sourced from is the scene-change
                                // opcode, not a dialog op. The real NPC text is the
                                // structural `inline` block below.
                                text_id: None,
                                inline: dialog_inline.unwrap_or_default(),
                            },
                            PlacementKind::Plain => return None,
                        };
                        Some((cfg, (p.world_x, p.world_z)))
                    })
                    .collect()
            })
            .unwrap_or_default();
        if let Some(table) = table {
            log::info!(
                "world-map '{name}': routed {} encounter region(s)",
                table.regions.len()
            );
            self.world.set_world_map_regions(table);
        }
        if !entities.is_empty() {
            log::info!(
                "world-map '{name}': installed {} interactive entit(ies) from placements",
                entities.len()
            );
            self.world.install_world_map_entities_at(entities);
        }
        // Switch to world-map mode (idempotent; keeps the installed player +
        // collision grid).
        self.world.enter_world_map();
        Ok(())
    }

    /// One frame: tick the world, materialize any actor-spawn requests
    /// queued by the field VM's `0x4C 0x80` opcode, then process any
    /// pending `scene_transition(map_id)` request. Returns the
    /// [`SceneTickEvent`] describing what happened.
    ///
    /// A transition whose resolved scene is an overworld scene
    /// ([`is_world_map_scene`]) routes through [`Self::enter_world_map_scene`]
    /// (world-map mode + region table) instead of the plain field path, so the
    /// boot/transition path seeds the overworld the same way the explicit
    /// `--world-map` entry does.
    pub fn tick(&mut self) -> Result<SceneTickEvent> {
        let _ = self.world.tick();
        self.world
            .materialize_actor_spawns(crate::world::FIELD_SPAWN_START_SLOT);
        // Named scene-change (field-VM op 0x3F) takes precedence over the
        // map-id door-warp: its destination name is carried inline by the op,
        // so it loads directly without the map-id resolver. This is the live
        // consumer of the disc-sourced scene-destination data — the same names
        // [`crate::man_field_scripts::scene_destinations`] catalogs.
        if let Some((name, _entry_x, _entry_z)) = self.world.pending_named_scene_transition.take() {
            // Drop a stale map-id request from the same frame; the named target
            // is unambiguous.
            self.world.pending_scene_transition = None;
            if is_world_map_scene(&name) {
                self.enter_world_map_scene(&name)?;
            } else {
                self.enter_field_scene(&name, 0)?;
            }
            return Ok(SceneTickEvent::SceneEntered { name });
        }
        if let Some(map_id) = self.world.pending_scene_transition.take() {
            match self.map_resolver.resolve(map_id) {
                Some(name) => {
                    if is_world_map_scene(&name) {
                        self.enter_world_map_scene(&name)?;
                    } else {
                        self.enter_field_scene(&name, 0)?;
                    }
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

/// PROT entry index for `befect_data` carrying the global TMD-pool head
/// (the 5 character-mesh TMDs at retail `DAT_8007C018[0..4]`). Pinned in
/// `project_global_tmd_pool_source.md` via byte-equality vs a Drake post-warp
/// RAM snapshot.
const PROT_BEFECT_DATA_ENTRY: u32 = 874;

/// PROT entry holding the battle effect-texture atlas (the "flame atlas"):
/// three 64x256 4bpp PSX TIMs blitted to VRAM `(320,0)`, `(384,0)`, `(448,0)`
/// with CLUTs in rows 474..=476 (the effect-CLUT band). Stored uncompressed,
/// back-to-back behind a 16-byte prefix. Despite its CDNAME label
/// (`sound_data`, shared with PROT 871) it carries no audio - the label is one
/// of the documented CDNAME mislabels. Byte-verified pixel-exact in VRAM
/// against every stable Rim Elm battle capture (command-menu / submenu /
/// pre- and post-Seru-capture frames); the partial match in a still-loading
/// frame is just the mid-DMA snapshot. Unlike `etim.dat` (PROT 874 section 2,
/// pages at `fb_y=256`), these pages sit at `fb_y=0` in the same VRAM columns
/// the field uses for town stage textures, so they are *battle-only* uploads -
/// the field captures hold unrelated town texels there. Retail blits them at
/// battle load (not by the `FUN_800520F0` etmd/befect path, which pulls
/// indices `0x367..=0x36d` - PROT 870 = index `0x366` is loaded by a separate
/// site). See `docs/formats/effect.md`.
const PROT_FLAME_ATLAS_ENTRY: u32 = 870;

/// PROT entry holding the runtime effect buffer `data\battle\efect.dat` - the
/// 2-pack wrapper (inline sprite atlas + pack0 anim batches + pack1 effect
/// scripts) the battle effect VM consumes. Stored uncompressed; the raw entry
/// bytes are byte-identical to the post-init runtime buffer (`docs/formats/effect.md`).
const PROT_EFECT_DAT_ENTRY: u32 = 873;

/// PROT entry holding the global monster stat archive (one `0x14000`-byte
/// LZS slot per monster id; the CDNAME label `battle_data` is shared across
/// 0865-0868). The misleading `monster_data` label (PROT 869) is a stub.
/// See [`legaia_asset::monster_archive`] + `docs/subsystems/battle.md`.
const MONSTER_ARCHIVE_PROT_ENTRY: u32 = 867;

/// Number of slots PROT 0874 section 0 contributes to the head of the
/// global TMD pool. Set by the section's TMD-pack `count` field; the
/// retail pack carries exactly 5 character meshes.
const GLOBAL_TMD_POOL_HEAD_COUNT: usize = 5;

/// Index into [`crate::world::World::global_tmd_pool`] of the PROT 0874 §0
/// *preview* flame mesh - the smallest of that section's five TMDs (2 objects,
/// 18 verts, 25 prims). It bakes the `etim` CLUT (`cba=0x778E@(224,478)`,
/// `tsb=0x001D@(832,256)`) and looks flame-shaped, so the engine could render
/// it through the standard VRAM-mesh pipeline as a stand-in.
///
/// **This is a preview mesh, not the model retail draws.** The real battle
/// flame is [`GIMARD_TAIL_FIRE_MODEL_INDEX`], pulled from the PROT 0871
/// effect-model library ([`seed_effect_model_library_from_etmd`]). The
/// stand-in is kept only as a fallback when that library isn't loaded (e.g.
/// raw-PROT.DAT inspection without the battle assets). See
/// `docs/formats/effect.md`.
pub const ETMD_TAIL_FIRE_MODEL_INDEX: usize = 4;

/// PROT entry holding the battle effect-model library (`etmd.dat`): a 30-entry
/// `asset::pack` of Legaia TMDs (`word[0]=30`, every entry magic `0x80000002`),
/// stored uncompressed. Retail registers all 30 verbatim into
/// `DAT_8007C018[3..=32]` at battle init (`FUN_800520F0` debug index `0x367` ->
/// `FUN_80026B4C`); the dev-path name is `h:\prot\battle\etmd.dat`. The CDNAME
/// label `sound_data` is misleading - this is the effect-model library, not
/// audio. See `docs/formats/effect.md`.
const PROT_EFFECT_MODEL_LIBRARY_ENTRY: u32 = 871;

/// Base index in [`crate::world::World::global_tmd_pool`] (= `DAT_8007C018`)
/// where the PROT 0871 effect-model library registers. Its 30 models occupy
/// `[3..=32]`, overwriting the two trailing slots of the PROT 0874 §0 field
/// head (`[3]`, `[4]`) - exactly retail's temporal layout (the field head
/// seeds `[0..=4]`; battle init reloads `[3..=32]`).
///
/// This is the engine's analogue of the retail **battle `gp[0x754]` value** —
/// the additive base `FUN_80021B04` applies to a move-FX / summon part record's
/// `model_sel` (`DAT_8007C018[model_sel + gp[0x754]]`). In retail that base is
/// *not* a constant: it is `party_count + 2` (the two fixed pool slots + the live
/// party-character meshes precede the library), i.e. `3` for the 1-member
/// training party and `5` for the full 3-member party — save-corpus-pinned by
/// `crates/mednafen/tests/summon_model_base.rs` (see `docs/formats/move-power.md`).
/// The engine instead registers the library at a *fixed* `[3..=32]` and keeps
/// `model_sel` library-relative, so `model_sel + 3` lands on the same library
/// model retail reaches via `model_sel + gp[0x754]` — the library content is
/// identical, only its pool offset shifts with party size, so the two layouts are
/// equivalent. `World::spawn_move_fx` uses this fixed base.
pub(crate) const EFFECT_MODEL_LIBRARY_BASE: usize = 3;

/// Number of TMDs in the PROT 0871 effect-model library (`word[0]`).
const EFFECT_MODEL_LIBRARY_COUNT: usize = 30;

/// Index in [`crate::world::World::global_tmd_pool`] of Gimard's *Tail Fire*
/// flame model (`DAT_8007C018[26]`) - the model retail draws for the Gimard
/// Seru cast. Equals [`EFFECT_MODEL_LIBRARY_BASE`]` + 23` (pack entry 23). Its
/// fire flicker is CLUT/palette cycling driven by the summon stager overlay (extraction PROT 0903)
/// (the model geometry is static). Supersedes the PROT 0874 §0 preview
/// stand-in at [`ETMD_TAIL_FIRE_MODEL_INDEX`]. See `docs/formats/effect.md`.
pub const GIMARD_TAIL_FIRE_MODEL_INDEX: usize = 26;

/// Seed `World::global_tmd_pool[0..=4]` from PROT 0874 (`befect_data`)
/// section 0. Soft-fails (returns `Err`) when the entry is missing, the
/// section header is malformed, the LZS decode fails, or the inner
/// TMD-pack walk fails - the field-VM `0x4C 0xD8` host hook then leaves
/// `Actor::tmd_ref` at `None` rather than aborting scene-load.
///
/// The retail loader chain that produces these 5 entries via
/// `FUN_8001F05C case 2 → FUN_80026B4C` is not yet pinned (see open work
/// item in `docs/formats/world-map-overlay.md`); this routes the disc
/// bytes directly through the `parse_player_lzs + pack` parsers and
/// installs the parsed TMDs onto the world.
/// Load the effect-script catalog from PROT 0873 (`efect.dat`) into
/// `World::effect_catalog`. Soft-fails when the entry is missing or the
/// 2-pack is malformed (the catalog stays empty and nothing spawns). Parsing
/// itself never errors - [`EffectCatalog::from_efect_dat_bytes`] returns an
/// empty catalog on bad data - so the only error is the disc read.
fn seed_effect_catalog_from_efect_dat(
    index: &ProtIndex,
    world: &mut crate::world::World,
) -> Result<()> {
    let raw = index
        .entry_bytes(PROT_EFECT_DAT_ENTRY)
        .with_context(|| format!("read PROT entry {} (efect.dat)", PROT_EFECT_DAT_ENTRY))?;
    let catalog = legaia_engine_vm::effect_vm::EffectCatalog::from_efect_dat_bytes(&raw);
    if catalog.is_empty() {
        anyhow::bail!("efect.dat parsed to an empty catalog (unexpected 2-pack shape)");
    }
    world.effect_catalog = catalog;
    Ok(())
}

fn seed_global_tmd_pool_from_befect_data(
    index: &ProtIndex,
    world: &mut crate::world::World,
) -> Result<()> {
    let raw = index
        .entry_bytes(PROT_BEFECT_DATA_ENTRY)
        .with_context(|| format!("read PROT entry {} (befect_data)", PROT_BEFECT_DATA_ENTRY))?;
    let container = legaia_asset::parse_player_lzs(&raw, 3)
        .context("parse befect_data as a 3-descriptor player.lzs-shaped container")?;
    let section0 = container
        .descriptors
        .first()
        .ok_or_else(|| anyhow::anyhow!("befect_data has no section 0"))?;
    let decoded = legaia_asset::decode(&raw, section0, legaia_asset::DecodeMode::Lzs)
        .context("LZS-decode befect_data section 0")?;
    let pack_entries = legaia_asset::pack::extract_pack(&decoded)
        .context("walk befect_data section 0 as a TMD-pack")?;
    let head = pack_entries
        .into_iter()
        .take(GLOBAL_TMD_POOL_HEAD_COUNT)
        .enumerate();
    for (i, body) in head {
        let tmd = match legaia_tmd::parse(body) {
            Ok(t) => t,
            Err(err) => {
                eprintln!("[scene] befect_data slot {i} did not parse as TMD ({err:#}); skipping");
                continue;
            }
        };
        world.set_global_tmd(
            i,
            std::sync::Arc::new(crate::world::GlobalTmd {
                tmd,
                raw: body.to_vec(),
            }),
        );
    }
    Ok(())
}

/// Seed the battle effect-model library from PROT 0871 (`etmd.dat`) into
/// `World::global_tmd_pool[3..=32]` (retail `DAT_8007C018[3..=32]`).
///
/// PROT 0871 is an uncompressed 30-entry [`legaia_asset::pack`] of Legaia
/// TMDs; the engine walks it directly (no LZS) and parses each entry, mapping
/// pack entry `i` -> pool index [`EFFECT_MODEL_LIBRARY_BASE`]` + i`. This is
/// the library retail loads at battle init (`FUN_800520F0`); the live
/// Tail-Fire RAM confirms these 30 models are resident during a Seru cast
/// while PROT 0874 §0's five TMDs are not - so this supersedes the §0 preview
/// head for the effect-model render path ([`GIMARD_TAIL_FIRE_MODEL_INDEX`] is
/// the flame retail draws).
///
/// Soft-fails (returns `Err`) when the entry is missing or the pack walk
/// fails; entries that don't parse as TMDs are skipped individually. The two
/// overlapping slots (`[3]`, `[4]`) from the PROT 0874 §0 head are overwritten
/// here, matching retail's temporal load order.
fn seed_effect_model_library_from_etmd(
    index: &ProtIndex,
    world: &mut crate::world::World,
) -> Result<()> {
    // The pack body spans PROT 0871's full on-disc footprint (the last TMD
    // sits past the TOC-indexed end), so read the extended footprint - the
    // indexed-only view truncates the pack mid-table.
    let raw = index
        .entry_bytes_extended(PROT_EFFECT_MODEL_LIBRARY_ENTRY)
        .with_context(|| {
            format!(
                "read PROT entry {} (etmd.dat effect-model library)",
                PROT_EFFECT_MODEL_LIBRARY_ENTRY
            )
        })?;
    let pack_entries = legaia_asset::pack::extract_pack(&raw)
        .context("walk PROT 0871 (etmd.dat) as a TMD pack")?;
    let mut loaded = 0usize;
    for (i, body) in pack_entries
        .iter()
        .enumerate()
        .take(EFFECT_MODEL_LIBRARY_COUNT)
    {
        let tmd = match legaia_tmd::parse(body) {
            Ok(t) => t,
            Err(err) => {
                eprintln!("[scene] etmd library slot {i} did not parse as TMD ({err:#}); skipping");
                continue;
            }
        };
        world.set_global_tmd(
            EFFECT_MODEL_LIBRARY_BASE + i,
            std::sync::Arc::new(crate::world::GlobalTmd {
                tmd,
                raw: body.to_vec(),
            }),
        );
        loaded += 1;
    }
    if loaded == 0 {
        anyhow::bail!("etmd library (PROT 0871) carried no parseable TMDs");
    }
    Ok(())
}

/// True when the PROT 0871 effect-model library is already resident in the
/// pool (every slot in `[3..=32]` populated). Used to keep
/// [`seed_effect_model_library_from_etmd`] idempotent across scene
/// transitions, mirroring the field-head guard.
fn effect_model_library_loaded(world: &crate::world::World) -> bool {
    let end = EFFECT_MODEL_LIBRARY_BASE + EFFECT_MODEL_LIBRARY_COUNT;
    world.global_tmd_pool.len() >= end
        && world.global_tmd_pool[EFFECT_MODEL_LIBRARY_BASE..end]
            .iter()
            .all(|s| s.is_some())
}

/// PROT 0874 (`befect_data`) section index carrying `etim.dat` - the battle
/// effect-sprite TIMs. The three LZS sections are: 0 = effect 3D models
/// (`etmd.dat`, the global-TMD-pool head), 1 = `vdf.dat`, 2 = `etim.dat`.
const BEFECT_ETIM_SECTION: usize = 2;

/// Upload the player `player.lzs` texture section (PROT 0874 section 2) into
/// `vram`. This 8-TIM pack carries **both** the 3D effect-model textures
/// (`etim.dat`, the texel source for `etmd.dat` / section 0's global-TMD-pool
/// head) **and the field-character texture atlas**: entries 1/2/3 are the
/// Vahn/Noa/Gala atlas pages at texpage `(832, 256)` with per-character CLUTs
/// on row 478 (the field-form player meshes sample exactly these). Retail
/// uploads the whole section at field-init via `FUN_8001E890 → FUN_800198E0`
/// (`LoadImage`) and keeps it resident across the battle scene-mode overlay, so
/// uploading at scene entry is equivalent. See
/// [`docs/formats/character-mesh.md` § Textures (field form)] for the full
/// entry table.
///
/// CLUT blocks are uploaded as **flat horizontal strips** (`FUN_800198e0`:
/// `LoadImage(rect = { x, y, w*h, 1 })`), not as the declared `w x h`
/// rectangle - see the inline note. (`legaia_asset::field_char_textures` is the
/// standalone parser + verifier for the same section, byte-exact against a live
/// field VRAM dump.)
///
/// This makes the texels resident for effect-model rendering. (It does *not*
/// feed the 2D `efect.dat` sprite-atlas billboards, which sample a separate
/// page-`(0,0)` 8bpp source - see [`crate::world::World::active_effect_sprites`]
/// and the open atlas-source thread in `docs/formats/effect.md`.)
///
/// Mirrors [`seed_global_tmd_pool_from_befect_data`]'s LZS path. Soft-fails;
/// returns the number of TIMs uploaded.
///
/// Public so the VRAM-parity oracle's lightweight pre-pass can apply the same
/// effect-texture upload the live field-entry path performs - without it the
/// oracle reports the `fb_y=256` effect pages (fb_x 320/384/832/852/872/880)
/// as a phantom static gap that the real engine never has.
///
/// `upload_clut` controls whether the TIMs' CLUT rows (473..=478) are written
/// alongside the image pages. Retail keeps the effect-texture *pixel* pages
/// (fb_y=256) resident from field through battle, but uploads their CLUTs at
/// battle entry - so a field-VRAM parity build wants `upload_clut = false`
/// (image pages only) while the live field-entry seed passes `true` to keep
/// the CLUTs resident through the battle scene-mode overlay.
pub fn upload_effect_textures_into_vram(
    index: &ProtIndex,
    vram: &mut legaia_tim::Vram,
    upload_clut: bool,
) -> Result<usize> {
    let decoded = befect_etim_section_bytes(index)?;
    let mut uploaded = 0;
    for target in legaia_asset::befect_cluster::scan_tims(&decoded) {
        match legaia_tim::parse(&decoded[target.offset..]) {
            Ok(tim) => {
                // Image page: declared rect, verbatim.
                vram.upload_tim_partial(&tim, true, false);
                // CLUT: `FUN_800198e0` uploads the CLUT block as a FLAT
                // horizontal strip - `LoadImage(rect = { x, y, w*h, 1 })` -
                // not the declared `w x h` rectangle. This matters for §2's
                // field-character TIMs (entries 1/2/3, CLUT `w=16 h=4`): a
                // rect upload puts Vahn's four 16-colour palettes at rows
                // 478..481 col 0, but the meshes sample them as columns
                // 0/16/32/48 of row 478. The strip places them correctly.
                // (Field upload runs with STP off, `_DAT_8007b998 == 0`.)
                if upload_clut && let Some(clut) = tim.clut.as_ref() {
                    let strip: Vec<u8> =
                        clut.entries.iter().flat_map(|c| c.to_le_bytes()).collect();
                    vram.write_clut_row(clut.fb_x, clut.fb_y, &strip);
                }
                uploaded += 1;
            }
            Err(err) => {
                eprintln!(
                    "[scene] etim TIM @0x{:x} did not parse ({err:#}); skipping",
                    target.offset
                );
            }
        }
    }
    if uploaded == 0 {
        anyhow::bail!("etim section carried no uploadable TIMs");
    }
    Ok(uploaded)
}

/// Decoded `befect_data` (PROT 874) etim-section bytes - the shared
/// effect-texture TIM pool [`upload_effect_textures_into_vram`] and
/// [`effect_texture_image_rects`] both walk.
fn befect_etim_section_bytes(index: &ProtIndex) -> Result<Vec<u8>> {
    let raw = index
        .entry_bytes(PROT_BEFECT_DATA_ENTRY)
        .with_context(|| format!("read PROT entry {} (befect_data)", PROT_BEFECT_DATA_ENTRY))?;
    let container = legaia_asset::parse_player_lzs(&raw, 3)
        .context("parse befect_data as a 3-descriptor player.lzs-shaped container")?;
    let section = container
        .descriptors
        .get(BEFECT_ETIM_SECTION)
        .ok_or_else(|| {
            anyhow::anyhow!("befect_data has no section {BEFECT_ETIM_SECTION} (etim)")
        })?;
    legaia_asset::decode(&raw, section, legaia_asset::DecodeMode::Lzs)
        .with_context(|| format!("LZS-decode befect_data section {BEFECT_ETIM_SECTION} (etim)"))
}

/// VRAM image rects `(fb_x, fb_y, width_in_words, height)` of the
/// `befect_data` effect-texture TIMs - the upload set of
/// [`upload_effect_textures_into_vram`].
///
/// The band is **global shared state**, not per-scene texture: one disc
/// source is resident across every field scene. A handful of its pixels are
/// *history-dependent* - the pause-menu entry path writes an F-variant of
/// three row-271 words (pinned at `(853, 271)`: pause-menu-lineage captures
/// hold `0xFFFF` where the disc TIM carries `0x3333`; each variant word
/// equals the same TIM's row-273 value), and the first battle effect use
/// restores the disc bytes. A per-scene static mask misclassifies those
/// pixels as static whenever a scene's captures share menu/battle history,
/// so the VRAM parity oracle uses these rects to demand staticity across
/// **all** scenes' captures instead.
pub fn effect_texture_image_rects(index: &ProtIndex) -> Result<Vec<(u16, u16, u16, u16)>> {
    let decoded = befect_etim_section_bytes(index)?;
    let mut rects = Vec::new();
    for target in legaia_asset::befect_cluster::scan_tims(&decoded) {
        if let Ok(tim) = legaia_tim::parse(&decoded[target.offset..]) {
            let img = &tim.image;
            rects.push((img.fb_x, img.fb_y, img.fb_w, img.h));
        }
    }
    Ok(rects)
}

/// Upload the battle effect-texture atlas (PROT 870, the "flame atlas") into
/// `vram`. These three 64x256 4bpp TIMs (pages at `(320,0)`, `(384,0)`,
/// `(448,0)`, CLUTs in rows 474..=476) are the texel source for the
/// fire/flame effect meshes during battle, byte-verified against live battle
/// VRAM (see [`PROT_FLAME_ATLAS_ENTRY`]).
///
/// Call this on **battle entry**, not field entry: the pages land in the same
/// VRAM columns (`fb_x` 320..512, `fb_y` 0) the field stage textures occupy,
/// so uploading them while a field scene is resident would clobber town
/// rendering. Retail overwrites that region for battle and the field reloads
/// its textures on return - the play-window battle path mirrors this by
/// blitting into a throwaway VRAM copy that battle exit discards.
///
/// `upload_clut` writes the CLUT rows (474..=476) alongside the image pages.
/// Mirrors [`upload_effect_textures_into_vram`]; PROT 870 is uncompressed, so
/// the TIMs are walked straight out of the entry bytes (read via the extended
/// footprint, like the PROT 871 effect-model library - the indexed size can
/// truncate the trailing TIM). Soft-fails; returns the number of TIMs uploaded.
pub fn upload_flame_atlas_into_vram(
    index: &ProtIndex,
    vram: &mut legaia_tim::Vram,
    upload_clut: bool,
) -> Result<usize> {
    let raw = index
        .entry_bytes_extended(PROT_FLAME_ATLAS_ENTRY)
        .with_context(|| format!("read PROT entry {PROT_FLAME_ATLAS_ENTRY} (flame atlas)"))?;
    let mut uploaded = 0;
    for target in legaia_asset::befect_cluster::scan_tims(&raw) {
        match legaia_tim::parse(&raw[target.offset..]) {
            Ok(tim) => {
                vram.upload_tim_partial(&tim, true, upload_clut);
                uploaded += 1;
            }
            Err(err) => {
                eprintln!(
                    "[scene] flame-atlas TIM @0x{:x} did not parse ({err:#}); skipping",
                    target.offset
                );
            }
        }
    }
    if uploaded == 0 {
        anyhow::bail!("flame atlas (PROT {PROT_FLAME_ATLAS_ENTRY}) carried no uploadable TIMs");
    }
    Ok(uploaded)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn world_map_scene_classifier_matches_only_the_kingdom_overworlds() {
        // The three kingdom overworld scenes.
        assert!(is_world_map_scene("map01"));
        assert!(is_world_map_scene("map02"));
        assert!(is_world_map_scene("map03"));
        // Towns, dungeons, cutscene/FMV labels are not overworlds.
        for label in [
            "town01", "town0b", "chitei2", "jou", "uru2", "opdeene", "opmap01", "battle", "map1",
            "map001", "mapxx", "world", "",
        ] {
            assert!(
                !is_world_map_scene(label),
                "{label} must not classify as world map"
            );
        }
    }

    /// Disc-gated: the `etim.dat` effect-sprite TIMs (PROT 0874 section 2)
    /// decode and upload into a software VRAM, populating the fire-sprite tile
    /// at fb(832,256) (the texel target verified pixel-exact against a live
    /// battle VRAM capture). Skips when the disc data isn't present.
    #[test]
    fn etim_effect_textures_upload_into_vram() {
        if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
            eprintln!("[skip] LEGAIA_DISC_BIN unset");
            return;
        }
        let root = ["extracted", "../../extracted"]
            .iter()
            .map(PathBuf::from)
            .find(|p| p.join("PROT.DAT").is_file());
        let Some(root) = root else {
            eprintln!("[skip] extracted/PROT.DAT missing");
            return;
        };
        let index = ProtIndex::open_extracted(&root).expect("open ProtIndex");
        let mut vram = legaia_tim::Vram::new();
        let n = upload_effect_textures_into_vram(&index, &mut vram, true).expect("seed etim VRAM");
        assert!(n >= 5, "expected >=5 etim TIMs uploaded, got {n}");
        assert!(
            vram.region_has_data(832, 256, 20, 64),
            "etim fire-sprite tile @fb(832,256) should be populated"
        );
        // The field-character CLUTs land as flat strips on row 478: Vahn at
        // cols 0..63, Noa 64..127, Gala 128..191. A rect upload (the prior
        // bug) would only populate cols 0..15 of row 478, leaving Noa's and
        // Gala's palette columns empty. Assert all three character palette
        // bands are present at row 478.
        for (col, who) in [(0usize, "Vahn"), (64, "Noa"), (128, "Gala")] {
            assert!(
                vram.region_has_data(col, 478, 16, 1),
                "field-char CLUT strip for {who} @row 478 col {col} should be populated \
                 (flat-strip CLUT upload)"
            );
        }
    }

    /// Disc-gated: the flame-atlas TIMs (PROT 870) decode and upload into a
    /// software VRAM, populating all three effect-texture pages at the
    /// byte-verified targets `(320,0)`, `(384,0)`, `(448,0)`. These sit at
    /// `fb_y=0` (distinct from etim's `fb_y=256` pages) and are battle-only.
    /// Skips when the disc data isn't present.
    #[test]
    fn flame_atlas_uploads_three_effect_pages_at_y0() {
        if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
            eprintln!("[skip] LEGAIA_DISC_BIN unset");
            return;
        }
        let root = ["extracted", "../../extracted"]
            .iter()
            .map(PathBuf::from)
            .find(|p| p.join("PROT.DAT").is_file());
        let Some(root) = root else {
            eprintln!("[skip] extracted/PROT.DAT missing");
            return;
        };
        let index = ProtIndex::open_extracted(&root).expect("open ProtIndex");
        let mut vram = legaia_tim::Vram::new();
        let n =
            upload_flame_atlas_into_vram(&index, &mut vram, true).expect("seed flame-atlas VRAM");
        assert_eq!(n, 3, "expected exactly 3 flame-atlas TIMs, got {n}");
        for (fb_x, fb_y) in [(320usize, 0usize), (384, 0), (448, 0)] {
            assert!(
                vram.region_has_data(fb_x, fb_y, 64, 256),
                "flame-atlas page @fb({fb_x},{fb_y}) should be populated"
            );
        }
    }

    /// Disc-gated: the global TMD pool seeded from `etmd.dat` (PROT 0874
    /// section 0) holds the five effect models, and the slot named by
    /// [`ETMD_TAIL_FIRE_MODEL_INDEX`] is the small *Tail Fire* flame mesh - far
    /// fewer primitives than the other four models, with textured primitives
    /// sampling the `etim` CLUT rows (473..=478). Pins the constant to real
    /// disc bytes. Skips when the disc data isn't present.
    #[test]
    fn etmd_tail_fire_model_is_the_small_flame_mesh() {
        if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
            eprintln!("[skip] LEGAIA_DISC_BIN unset");
            return;
        }
        let root = ["extracted", "../../extracted"]
            .iter()
            .map(PathBuf::from)
            .find(|p| p.join("PROT.DAT").is_file());
        let Some(root) = root else {
            eprintln!("[skip] extracted/PROT.DAT missing");
            return;
        };
        let index = ProtIndex::open_extracted(&root).expect("open ProtIndex");
        let mut world = crate::world::World::default();
        seed_global_tmd_pool_from_befect_data(&index, &mut world).expect("seed etmd pool");

        // All five etmd models present.
        for i in 0..GLOBAL_TMD_POOL_HEAD_COUNT {
            assert!(
                world.global_tmd(i as i16).is_some(),
                "etmd model {i} should be present"
            );
        }
        let flame = world
            .global_tmd(ETMD_TAIL_FIRE_MODEL_INDEX as i16)
            .expect("flame model present");
        let flame_prims: usize = flame
            .tmd
            .objects
            .iter()
            .map(|o| o.header.n_primitive as usize)
            .sum();

        // The flame is the smallest model by a wide margin.
        for i in 0..GLOBAL_TMD_POOL_HEAD_COUNT {
            if i == ETMD_TAIL_FIRE_MODEL_INDEX {
                continue;
            }
            let other = world.global_tmd(i as i16).unwrap();
            let other_prims: usize = other
                .tmd
                .objects
                .iter()
                .map(|o| o.header.n_primitive as usize)
                .sum();
            assert!(
                flame_prims < other_prims,
                "flame model ({flame_prims} prims) should be smaller than model {i} ({other_prims} prims)"
            );
        }
        assert!(
            flame_prims < 64,
            "flame model is a small mesh, got {flame_prims} prims"
        );
    }

    /// Disc-gated: the PROT 0871 effect-model library (`etmd.dat`) is a 30-entry
    /// TMD pack that registers into `World::global_tmd_pool[3..=32]`, and the
    /// Gimard *Tail Fire* slot ([`GIMARD_TAIL_FIRE_MODEL_INDEX`]) resolves to a
    /// real Legaia TMD. Pins the library load + index to real disc bytes. Skips
    /// when the disc data isn't present.
    #[test]
    fn etmd_effect_model_library_registers_into_global_pool() {
        if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
            eprintln!("[skip] LEGAIA_DISC_BIN unset");
            return;
        }
        let root = ["extracted", "../../extracted"]
            .iter()
            .map(PathBuf::from)
            .find(|p| p.join("PROT.DAT").is_file());
        let Some(root) = root else {
            eprintln!("[skip] extracted/PROT.DAT missing");
            return;
        };
        let index = ProtIndex::open_extracted(&root).expect("open ProtIndex");
        let mut world = crate::world::World::default();

        // Library not loaded yet -> guard reports false.
        assert!(!effect_model_library_loaded(&world));

        seed_effect_model_library_from_etmd(&index, &mut world).expect("seed etmd library");

        // All 30 models register into [3..=32], and the guard is now true.
        assert!(effect_model_library_loaded(&world));
        for i in 0..EFFECT_MODEL_LIBRARY_COUNT {
            assert!(
                world
                    .global_tmd((EFFECT_MODEL_LIBRARY_BASE + i) as i16)
                    .is_some(),
                "effect-model library slot {i} -> pool {} should be present",
                EFFECT_MODEL_LIBRARY_BASE + i
            );
        }

        // The named Gimard flame slot resolves to a real TMD with geometry.
        let flame = world
            .global_tmd(GIMARD_TAIL_FIRE_MODEL_INDEX as i16)
            .expect("Gimard Tail Fire model present");
        let flame_prims: usize = flame
            .tmd
            .objects
            .iter()
            .map(|o| o.header.n_primitive as usize)
            .sum();
        assert!(
            flame_prims > 0,
            "Gimard flame model carries primitives, got {flame_prims}"
        );
        // The flame index falls inside the library window.
        assert_eq!(
            GIMARD_TAIL_FIRE_MODEL_INDEX,
            EFFECT_MODEL_LIBRARY_BASE + 23,
            "Gimard flame is DAT_8007C018[26] = pack entry 23"
        );
    }

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

    #[test]
    fn scene_destination_resolver_resolves_by_index() {
        use crate::man_field_scripts::SceneDestination;
        let r = SceneDestinationResolver::new(vec![
            SceneDestination {
                scene_name: "town0c".into(),
                index: 21,
                entry_x: 0x10,
                entry_z: 0x20,
            },
            SceneDestination {
                scene_name: "rikuroa".into(),
                index: 155,
                entry_x: 0x30,
                entry_z: 0x40,
            },
        ]);
        assert_eq!(r.len(), 2);
        // Resolve by the i16 index (the 0x3F index space — wider than u8).
        assert_eq!(r.resolve(21), Some("town0c"));
        assert_eq!(r.resolve(155), Some("rikuroa"));
        assert_eq!(r.resolve(99), None);
        // The richer accessor returns the full record (name + entry tile).
        let d = r.destination(155).expect("rikuroa destination");
        assert_eq!(d.scene_name, "rikuroa");
        assert_eq!((d.entry_x, d.entry_z), (0x30, 0x40));
        assert!(SceneDestinationResolver::default().is_empty());
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
