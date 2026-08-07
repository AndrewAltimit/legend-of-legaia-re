//! `ProtIndex`: the shared PROT.DAT + CDNAME index and per-scene field-map constants.

use super::*;

/// Size of the per-scene field map file (retail `DATA\FIELD\<scene>.MAP`):
/// the field buffer's used region from the base through the field-pack
/// boundary at `+0x12000`. Used to identify the map entry in a CDNAME block.
pub const FIELD_MAP_LEN: usize = 0x12000;
/// Offset of the collision/floor grid within the field map file (= the
/// runtime `*(_DAT_1f8003ec) + 0x4000`).
pub const FIELD_MAP_COLLISION_OFFSET: usize = 0x4000;
/// Length of the collision/floor grid (`0x80 x 0x80` bytes, 1 byte/tile).
pub const FIELD_COLLISION_GRID_LEN: usize = 0x80 * 0x80;

/// Per-scene battle-stage stream position within
/// [`ProtIndex::battle_stage_entries`]'s slot-ordered list. A scene bundle
/// carries one stage stream per sub-area, so the block's *first* stream is
/// frequently a neighbouring sub-area's backdrop rather than the scene's own.
/// Each row is pinned by byte-matching a retail battle save state's resident
/// dome (the vertex pool `_DAT_8007B864` points at) back to a PROT entry - see
/// [`ProtIndex::battle_stage_entry_for_scene`].
const BATTLE_STAGE_SLOT: &[(&str, usize)] = &[
    // Rim Elm: bundle slot 6 = entry 7 (2 objects, 341 vertices).
    ("town01", 1),
    // Overworld: bundle slot 5 = entry 88 (4 objects, 340 vertices).
    ("map01", 0),
];

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
    /// Lazy parse of the boot-resident system-UI TIM bundle (raw TOC
    /// entries 0/1). Populated on first `system_ui_bundle` call.
    system_ui_cache: Mutex<Option<Arc<legaia_asset::system_ui_bundle::SystemUiBundle>>>,
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
            system_ui_cache: Mutex::new(None),
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
            system_ui_cache: Mutex::new(None),
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

    /// Read an entry's bytes (lazy + cached). Returns the same `Arc` for
    /// repeated reads of the same index.
    ///
    /// This is **the entry**: all `toc[idx+3] - toc[idx+2]` sectors of it and
    /// nothing a neighbour owns - the window the boot loader streams, and the
    /// only window scene assets are addressed against. Every descriptor offset
    /// in a scene's asset table resolves inside it.
    ///
    /// It used to return the `toc[p+5] - toc[p+3] + 4` window, which measures
    /// entry `p`'s two *successors* (see
    /// [`docs/formats/prot.md`](../../../docs/formats/prot.md)). That window
    /// ran past the entry for most of the archive, and detection against it is
    /// what made the phantom `scene_scripted_asset_table` bundles resolve - a
    /// one-sector prescript entry whose "table at +0x800" was really the next
    /// entry's table at offset 0.
    pub fn entry_bytes(&self, idx: u32) -> Result<Arc<Vec<u8>>> {
        if let Some(b) = crate::lock_poison_tolerant(&self.entry_cache)
            .get(&idx)
            .cloned()
        {
            return Ok(b);
        }
        let entry = self
            .entries
            .get(idx as usize)
            .ok_or_else(|| anyhow::anyhow!("PROT index {} out of range", idx))?
            .clone();
        let mut bytes = Vec::new();
        crate::lock_poison_tolerant(&self.archive)
            .read_entry(&entry, &mut bytes)
            .with_context(|| format!("read PROT entry {}", idx))?;
        let arc = Arc::new(bytes);
        crate::lock_poison_tolerant(&self.entry_cache).insert(idx, arc.clone());
        Ok(arc)
    }

    /// An entry's bytes as an owned `Vec`.
    ///
    /// Historically the "extended footprint" view, as distinct from an
    /// "indexed" [`Self::entry_bytes`]. There is one view now and this
    /// returns it; the name is kept so existing call sites read the same.
    pub fn entry_bytes_extended(&self, idx: u32) -> Result<Vec<u8>> {
        Ok(self.entry_bytes(idx)?.as_ref().clone())
    }

    /// An entry's bytes, explicitly bounded by its TOC LBA gap
    /// (`(toc[idx+3] - toc[idx+2]) * 0x800`, see
    /// [`Self::entry_lba_count_retail`]).
    ///
    /// That bound *is* [`Self::entry_bytes`]'s length, so this truncates
    /// nothing. It stays as the explicit contract for the readers that must
    /// not see a neighbour's bytes - the per-summon move-VM stagers (PROT
    /// 0903.., the high-summon block, the enemy-boss block), whose spawn-site
    /// record pointers are only valid for their own load at the shared link
    /// base. Matches `legaia_asset::summon_overlay::unique_content_len`.
    pub fn entry_bytes_lba_footprint(&self, idx: u32) -> Result<Vec<u8>> {
        let mut bytes = self.entry_bytes_extended(idx)?;
        if let Some(count) = self.entry_lba_count_retail(idx as u16) {
            let footprint = count as usize * 0x800;
            if footprint > 0 && footprint <= bytes.len() {
                bytes.truncate(footprint);
            }
        }
        Ok(bytes)
    }

    /// Read raw bytes from `PROT.DAT` at an arbitrary file offset.
    ///
    /// Used to reach unindexed gap regions that don't belong to any TOC
    /// entry - e.g. the 240 KB system-UI gap between the TOC and
    /// `init_data` that carries the menu-glyph atlas and other
    /// boot-time UI TIMs (see [`docs/subsystems/boot.md`]).
    pub fn prot_dat_raw_bytes(&self, byte_offset: u64, len: usize) -> Result<Vec<u8>> {
        let mut bytes = Vec::new();
        crate::lock_poison_tolerant(&self.archive)
            .read_raw(byte_offset, len, &mut bytes)
            .with_context(|| format!("read PROT.DAT raw at 0x{:X} +{}", byte_offset, len))?;
        Ok(bytes)
    }

    /// The boot-resident **system-UI TIM bundle** - the `prot::timpack`s
    /// at raw PROT TOC entries 0 and 1 (the head region the extraction
    /// index space skips; menu-glyph atlas, system-UI sprite sheet, boot
    /// cursor parts). Retail uploads it once at boot via `FUN_800198E0`
    /// (image at declared rect, CLUT as a flattened `w*h x 1` strip) and
    /// never evicts it, so the engine's per-scene VRAM pre-pass layers it
    /// under every scene build. Lazy + cached; see
    /// [`legaia_asset::system_ui_bundle`] and
    /// [`docs/formats/tim-pack.md`].
    ///
    /// Raw TOC entry `n` spans sectors `toc[n] .. toc[n+1]` in this
    /// index's TOC frame (extraction entry `p` = raw entry `p + 2`).
    pub fn system_ui_bundle(&self) -> Result<Arc<legaia_asset::system_ui_bundle::SystemUiBundle>> {
        if let Some(b) = crate::lock_poison_tolerant(&self.system_ui_cache).clone() {
            return Ok(b);
        }
        let mut ranges = Vec::with_capacity(legaia_asset::system_ui_bundle::RAW_ENTRY_COUNT);
        for n in 0..legaia_asset::system_ui_bundle::RAW_ENTRY_COUNT {
            let (start, end) = match (self.toc.get(n), self.toc.get(n + 1)) {
                (Some(&s), Some(&e)) if s > 0 && e > s => (s as u64 * 0x800, e as u64 * 0x800),
                _ => anyhow::bail!("PROT TOC head words missing for raw entry {n}"),
            };
            ranges.push(self.prot_dat_raw_bytes(start, (end - start) as usize)?);
        }
        let bundle = legaia_asset::system_ui_bundle::parse_entries(&ranges[0], &ranges[1])
            .context("parse the system-UI bundle (raw TOC entries 0/1)")?;
        let arc = Arc::new(bundle);
        *crate::lock_poison_tolerant(&self.system_ui_cache) = Some(arc.clone());
        Ok(arc)
    }

    /// Detected class of an entry (lazy + cached).
    pub fn class_of(&self, idx: u32) -> Result<Class> {
        if let Some(c) = crate::lock_poison_tolerant(&self.class_cache)
            .get(&idx)
            .copied()
        {
            return Ok(c);
        }
        let bytes = self.entry_bytes(idx)?;
        let report = classify(&bytes);
        let class = report.class;
        crate::lock_poison_tolerant(&self.class_cache).insert(idx, class);
        Ok(class)
    }

    /// Look up a CDNAME block range (`first_idx, end_idx`) by scene label,
    /// in the **raw-TOC** frame the `#define` numbers live in. Returns `None`
    /// if no CDNAME map was loaded or the label isn't present.
    ///
    /// Almost every consumer wants [`Self::block_range_extraction`] instead -
    /// this index addresses entries in the extraction frame (`raw - 2`).
    pub fn block_range(&self, scene_name: &str) -> Option<(u32, u32)> {
        let map = self.cdname.as_ref()?;
        cdname::block_range_for_name(map, scene_name)
    }

    /// [`Self::block_range`] converted to the retail **extraction** frame this
    /// index addresses (`raw - RAW_TOC_INDEX_OFFSET`; head defines keep their
    /// legacy unshifted windows). See
    /// `legaia_prot::cdname::block_range_for_name_extraction`.
    pub fn block_range_extraction(&self, scene_name: &str) -> Option<(u32, u32)> {
        let map = self.cdname.as_ref()?;
        cdname::block_range_for_name_extraction(map, scene_name)
    }

    /// PROT entries in `scene_name`'s CDNAME block whose payload is a
    /// `scene_tmd_stream` - the battle-stage half-dome backdrops (sky + mountain
    /// ring + ground that the battle is fought inside; see
    /// [`docs/subsystems/battle.md`] "Battle background").
    ///
    /// A scene bundle lays its streams out at fixed **bundle slots** after the
    /// `.MAP` / v12 / event-script / asset-table / texture-pack head, so the
    /// returned vec is slot-ordered: element `n` is bundle slot
    /// `stage_slot_base + n`. A block carries one stream per sub-area, not one
    /// per texture variant. Empty when no CDNAME map is loaded or the block has
    /// no stage entries.
    ///
    /// Which of them a battle is actually fought inside is
    /// [`Self::battle_stage_entry_for_scene`] - it is *not* always the first.
    pub fn battle_stage_entries(&self, scene_name: &str) -> Vec<u32> {
        let Some((start, end)) = self.block_range_extraction(scene_name) else {
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

    /// The PROT entry a battle fought in `scene_name` uses as its stage
    /// backdrop: the entry whose stream the retail scene loader leaves in
    /// `_DAT_8007B864` (the type-`0x01` chunk walker `FUN_8001FE70`'s last
    /// chunk) when game mode flips to `0x15`.
    ///
    /// The walker records whatever the scene's asset table streamed, so the
    /// choice is scene data rather than a code table - and it is **not**
    /// uniformly the block's first stage stream. The per-scene bundle slot is
    /// pinned from retail battle save states by byte-matching the resident
    /// dome's vertex pool back to a PROT entry (rejecting matches that land
    /// past an entry's unique on-disc length, which is how the block's
    /// over-read tails produce phantom hits):
    ///
    /// | Scene | Stage-list position | Bundle slot | Entry | Pinned from |
    /// |---|---|---|---|---|
    /// | `town01` (Rim Elm) | 1 | 6 | 7 | the three Tetsu tutorial anchors |
    /// | `map01` (overworld) | 0 | 5 | 88 | the four camera-orbit angle saves |
    ///
    /// Scenes with no pinned slot fall back to the block's first stage stream.
    /// See [`docs/subsystems/battle.md`] "Battle background".
    pub fn battle_stage_entry_for_scene(&self, scene_name: &str) -> Option<u32> {
        let stages = self.battle_stage_entries(scene_name);
        let pinned = BATTLE_STAGE_SLOT
            .iter()
            .find(|(name, _)| *name == scene_name)
            .map(|(_, n)| *n);
        match pinned {
            Some(n) => stages.get(n).or_else(|| stages.first()).copied(),
            None => stages.first().copied(),
        }
    }

    /// First scene label whose block contains `idx`. Useful for diagnostics
    /// (e.g. "this BGM is part of which scene?").
    pub fn scene_for_index(&self, idx: u32) -> Option<&str> {
        let map = self.cdname.as_ref()?;
        cdname::block_for(map, idx)
    }

    /// The parsed CDNAME `#define` map (raw in-RAM TOC index → block name),
    /// or `None` when the index was built without `CDNAME.TXT`. The scene
    /// host clones this into [`crate::world::World::scene_toc_names`] so the
    /// world-side menu-warp drain can resolve a quick-travel placement's
    /// `scene_id` (that field is a raw TOC index - see
    /// `legaia_asset::worldmap_menu`).
    pub fn cdname_map(&self) -> Option<&cdname::IndexMap> {
        self.cdname.as_ref()
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
