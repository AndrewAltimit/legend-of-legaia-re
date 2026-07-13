//! `SceneEntry`: one classified PROT entry with bytes ready, plus scene-asset accessors.

use super::*;

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
    ///
    /// CDNAME `#define` numbers are **raw in-RAM TOC indices**, shifted `+2`
    /// from the extraction-entry space this loader indexes
    /// (`legaia_prot::cdname::RAW_TOC_INDEX_OFFSET`; see
    /// `docs/formats/cdname.md` § numbering space). The window is converted
    /// here so a scene's entries are its **retail** block: the first entry is
    /// the scene's `.MAP` file ([`Self::field_map_index`]), the early entries
    /// its v12 / event-script sidecars. An unshifted window drops those first
    /// two retail entries and bleeds in the *next* block's first two - which
    /// mis-frames scenes whose asset table sits at the block edge (`rikuroa`'s
    /// v12 entry is its 2nd retail entry; the unshifted window instead found
    /// `geremi`'s v12 table and loaded Jeremi's MAN under the `rikuroa`
    /// label).
    pub fn load(index: &ProtIndex, name: &str) -> Result<Self> {
        // Head defines (`init_data 0`, `gameover_data 1`) keep their legacy
        // unshifted windows - the -2 conversion has no content to land on
        // there; see `cdname::block_range_for_name_extraction`.
        let (start, end) = index
            .block_range_extraction(name)
            .ok_or_else(|| anyhow::anyhow!("scene '{}' not found in CDNAME map", name))?;
        let mut entries = Vec::with_capacity((end - start) as usize);
        for idx in start..end {
            // Skip out-of-range indices defensively.
            if (idx as usize) >= index.entry_count() {
                break;
            }
            let mut bytes = index.entry_bytes(idx)?;
            let class = index.class_of(idx)?;
            // `scene_asset_table` descriptor offsets are FILE-relative
            // against the entry's **extended** on-disc footprint (indexed
            // payload + trailing-overlay sectors) - the retail loader
            // streams by LBA, so a bundle's LZS mesh/texture streams
            // routinely start past the TOC-indexed end (e.g. the opdeene
            // prologue's whole vignette geometry pack; see
            // docs/formats/scene-bundles.md). Load those entries at their
            // full footprint so the resource sweep can reach every stream.
            //
            // The same applies to plain `lzs_container` entries: dungeon
            // scenes carry their environment mesh pack as a standalone LZS
            // container (rikuroa = entry 156, 77 TMDs) whose streams start
            // inside the TOC window but run into the trailing sectors -
            // truncated at the TOC end every stream fails to decode and the
            // scene resolves zero environment meshes.
            if matches!(class, Class::SceneAssetTable | Class::LzsContainer)
                && let Ok(ext) = index.entry_bytes_extended(idx)
                && ext.len() > bytes.len()
            {
                bytes = Arc::new(ext);
            }
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
        // `self.start` is the retail block's first entry (extraction frame);
        // the audio-oracle-pinned BGM slots sit at `raw_define + 6 + id` in
        // the raw-TOC frame = `start + 8 + id` here (the raw define is
        // `start + 2`). Absolute slots unchanged from the pre-shift loader.
        let target = self.start + 8 + bgm_id as u32;
        self.entries.iter().find(|e| e.idx == target)
    }

    /// Iterate every entry of `class` (in CDNAME order). Useful for sweeping
    /// every TMD / VAB in a scene without rerunning the classifier.
    pub fn entries_of(&self, class: Class) -> impl Iterator<Item = &SceneEntry> {
        self.entries.iter().filter(move |e| e.class == class)
    }

    /// Find the per-scene event-scripts container - either a standalone
    /// `SceneEventScripts` entry or the prescript prefix of a
    /// `SceneScriptedAssetTable` entry.
    ///
    /// **The records have one consumer**: the move-VM stager installer.
    /// [`legaia_asset::scene_event_scripts::move_stager_records`] parses the
    /// `[count][offsets]`-indexed records as summon-format **move-VM stager**
    /// records (100% valid stager leads across the corpus - the field-resident
    /// sibling of the per-summon stagers, installed by field-VM op `0x34`
    /// sub-3 → `FUN_800252EC`, ported as
    /// [`crate::world::World::spawn_field_stager`]). The install ops live in
    /// the scene MAN's own scripts (partition-1 effect-actor records +
    /// partition-2 cutscene timelines); record 0 is typically the master
    /// ambient record installed on entry. Installer id = record index,
    /// live-pinned against the RAM `[u16 count][u16 offsets]` relocation at
    /// `_DAT_8007B8D0` (census + pin:
    /// `tests/scene_prescript_consumer_census_disc.rs`). The engine's
    /// historical record-0-as-field-VM read (`load_field_record`) has no
    /// retail counterpart and survives only as a MAN-less fallback.
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
    /// The `.MAP` is the scene's retail block's **first entry** - exactly
    /// [`Scene::start`] now that [`Scene::load`] converts the CDNAME raw-TOC
    /// window into the extraction frame. (The historical "two entries below
    /// the block start" rule was this same entry seen from the unshifted
    /// window; the extractor's shifted filename labels attribute it to the
    /// tail of the *previous* block.) The entry is identified by its
    /// **extended on-disc footprint** of exactly [`FIELD_MAP_LEN`]
    /// (`0x12000`) bytes; scenes whose first entry isn't that size have no
    /// field map (cutscene / pure-asset blocks).
    ///
    /// Pinned by a save-library census: the live field buffer of a `keikoku`
    /// session matches this entry (PROT 0109) with **zero** collision-grid
    /// diffs (in-block entry 0118 diffs by thousands), same for `koin3`
    /// (PROT 0559 exact), and the kingdom walk maps were live-verified at
    /// the same position. The **object-index grid** (`+0x8000`, the
    /// [`Self::field_object_placements`] source) is live-validated the same
    /// way: residuals of 0..96 bytes against the resolved entry across
    /// town01 / town0c / keikoku / koin3 sessions (story-conditional cell
    /// mutations), thousands against every other candidate
    /// (regression-guarded by the disc + save-library gated
    /// `field_map_object_grid_live` test).
    ///
    /// The footprint matters: the TOC-indexed payload is only the first
    /// `0x4000` bytes (the object-record region); the collision grid at
    /// `+0x4000` and beyond lives in the entry's **trailing-gap sectors**, so
    /// callers must read the extended footprint, not [`SceneEntry::bytes`].
    ///
    /// See [`docs/subsystems/field-locomotion.md`] for the load chain.
    pub fn field_map_index(&self, index: &ProtIndex) -> Option<u32> {
        let idx = self.start;
        index
            .entries()
            .get(idx as usize)
            .is_some_and(|e| e.size_bytes as usize == FIELD_MAP_LEN)
            .then_some(idx)
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

    /// The scene's kind-1 **tile-trigger** tables: `(primary, fallback)`
    /// record lists parsed from the `.MAP` `+0x10000` block and the
    /// `+0x12000` fallback window (the first sectors of the next PROT entry;
    /// retail reads `0x28` sectors contiguously from the `.MAP` LBA -
    /// `FUN_8001F7C0` - so both live in the entry's extended footprint).
    /// When the player enters a listed tile, retail spawns the referenced
    /// MAN partition-2 record (`gate == 1`) - the door / opening-cutscene
    /// walk-on dispatch. Returns empty lists when the scene has no field map.
    ///
    /// REF: FUN_801D1EC4, FUN_801D5630, FUN_8003BDE0
    pub fn field_tile_triggers(
        &self,
        index: &ProtIndex,
    ) -> Result<(
        Vec<crate::field_regions::TileTrigger>,
        Vec<crate::field_regions::TileTrigger>,
    )> {
        let Some(idx) = self.field_map_index(index) else {
            return Ok((Vec::new(), Vec::new()));
        };
        let bytes = index.entry_bytes_extended(idx)?;
        let primary = bytes
            .get(crate::field_regions::MAP_REGION_BLOCK_OFFSET..)
            .map(crate::field_regions::parse_tile_triggers)
            .unwrap_or_default();
        // The fallback table = the first sectors past the `.MAP`'s 0x12000
        // footprint (the next PROT entry on disc). Some maps' extended reads
        // include that window; otherwise read the sibling entry directly.
        let mut fallback = bytes
            .get(crate::field_regions::MAP_TRIGGER_FALLBACK_OFFSET..)
            .map(crate::field_regions::parse_tile_triggers)
            .unwrap_or_default();
        if fallback.is_empty()
            && let Ok(sibling) = index.entry_bytes_extended(idx + 1)
        {
            fallback = crate::field_regions::parse_tile_triggers(&sibling);
        }
        Ok((primary, fallback))
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

    /// The scene's **object binds**: for every `.MAP` tile carrying a kind-1
    /// trigger, the MAN partition-0 record retail attaches to a placed object
    /// anchored there, plus that record's animation id
    /// ([`crate::field_env::ObjectBind`]).
    ///
    /// A placed object with no bind at its footprint-anchor tile is never
    /// spawned; one whose bind names a nonzero anim id is drawn **posed** from
    /// that clip's frame 0 (the rest state) rather than at its raw object-local
    /// vertices. Mirrors the `func_0x801d5630` lookup inside `FUN_8003A55C`.
    /// Returns `Ok(None)` if the scene has no field map or no MAN.
    pub fn field_object_binds(
        &self,
        index: &ProtIndex,
    ) -> Result<Option<std::collections::HashMap<(u8, u8), crate::field_env::ObjectBind>>> {
        let Some(idx) = self.field_map_index(index) else {
            return Ok(None);
        };
        let Some(man) = self.field_man_payload(index)? else {
            return Ok(None);
        };
        let Ok(man_file) = legaia_asset::man_section::parse(&man) else {
            return Ok(None);
        };
        let map = index.entry_bytes_extended(idx)?;
        Ok(Some(crate::field_env::object_binds(&map, &man_file, &man)))
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
    /// ground - the slot-1 pack meshes are only the sparse placed landmarks
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

    /// The walk view's **decoration layer** - trees, mountain groups, and
    /// props: walk-visible cells whose record stamps a nonzero `+0x10` pack
    /// mesh without the placed flag (disjoint from
    /// [`Self::walk_object_placements`]). See
    /// [`legaia_asset::field_objects::parse_walk_decorations`].
    pub fn walk_decoration_placements(
        &self,
        index: &ProtIndex,
    ) -> Result<Option<Vec<legaia_asset::field_objects::Placement>>> {
        let Some(idx) = self.walk_field_map_index(index) else {
            return Ok(None);
        };
        let bytes = index.entry_bytes_extended(idx)?;
        Ok(Some(legaia_asset::field_objects::parse_walk_decorations(
            &bytes,
        )))
    }

    /// The scene's 16-entry floor-height LUT, read from the MAN header
    /// (`man[+0x02..+0x22]`, 16 `s16` LE). A placed object's world Y is
    /// `-lut[tile_floor_nibble] + record.y_off` (the runtime stores the LUT
    /// negated; `FUN_8003aeb0` fills it from the MAN, `FUN_8003a55c` reads it).
    /// Validated against a live `town01` save (Vahn's house tile nibble `6`,
    /// `lut[6]=192` -> world Y `-192`). Returns `Ok(None)` when the scene has
    /// no MAN (neither a bundle MAN nor a streaming-carrier MAN - the
    /// resolution order of [`Self::field_man_payload`], so dungeon scenes
    /// whose MAN ships in a streaming variant carrier, e.g. `rikuroa`,
    /// resolve their floor LUT too).
    pub fn field_floor_height_lut(&self, index: &ProtIndex) -> Result<Option<[i16; 16]>> {
        let Some(man) = self.field_man_payload(index)? else {
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
    /// Returns `Ok(None)` only when the scene resolves no MAN at all (no
    /// bundle MAN and no streaming variant), or when the MAN's partition 1 is
    /// empty. Those scenes fall back to the event-script record-0 load.
    ///
    /// Note that the entry script's `0x4C` nibble-7 wall-paint deltas are
    /// gated behind system-flag tests, so they only fire once the world's
    /// story flags are seeded to a matching scene-entry state; the base
    /// collision grid ([`Self::field_collision_grid`]) is independent of the
    /// entry script.
    ///
    /// Resolution order matches [`Self::field_man_payload`]: the asset-table
    /// bundle MAN first, then the block's **streaming variant MAN** for the
    /// bundle-MAN-less v12-family dungeons (`rikuroa` / `dolk2`). For those
    /// scenes the streaming carrier IS the runtime `_DAT_8007B898` MAN, so its
    /// `P1[0]` is the scene-entry system script retail runs - including the
    /// post-battle-return dispatch that spawns the partition-2 beat records
    /// (`rikuroa` P1[0] tests the `0x289` battle-staged marker and issues the
    /// op-`0x44` spawn of the post-victory record `P2[50]`).
    ///
    /// REF: FUN_8003ab2c (the port lives in `legaia_asset::man_section`).
    pub fn field_man_entry_script(&self, index: &ProtIndex) -> Result<Option<(Vec<u8>, usize)>> {
        let Some(man_bytes) = self.field_man_payload(index)? else {
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
    /// Returns `Ok(None)` when the scene resolves no MAN at all (the same
    /// detector gap [`Self::field_man_entry_script`] documents) or when the
    /// MAN's encounter section declares no rollable formations (towns with
    /// no encounters). Resolves now for the `count=6`
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
        // Same resolution order as [`Self::field_man_payload`] (bundle MAN
        // first, streaming variant carrier fallback): the v12-family
        // dungeons (`rikuroa` / `dolk2`) carry their encounter section -
        // including the scripted-battle rows the `3E FF <row>` op selects
        // (rikuroa row 17 = lone Caruban) - in the variant MAN, their only
        // carrier.
        let Some(man_bytes) = self.field_man_payload(index)? else {
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
        let Some(man_bytes) = self.field_man_payload(index)? else {
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
    ///
    /// Resolution order: the asset-table bundle MAN first; when the scene has
    /// no MAN-bearing bundle (the v12-family dungeons `rikuroa` / `dolk2`,
    /// whose v12 sidecar embeds only the count-4 MAN-less table), fall back
    /// to the block's **streaming variant MAN**
    /// ([`crate::scene_bundle::streaming_man_payloads`]) - the carrier the
    /// live script heap byte-matches at the Mt. Rikuroa Caruban beat.
    pub fn field_man_payload(&self, index: &ProtIndex) -> Result<Option<Vec<u8>>> {
        if let Some(bundle) = crate::scene_bundle::find_bundle(self) {
            let entry_bytes = index.entry_bytes_extended(bundle.entry_idx())?;
            if let Some(man) = crate::scene_bundle::extract_man_payload(&bundle, &entry_bytes)? {
                return Ok(Some(man));
            }
        }
        Ok(crate::scene_bundle::streaming_man_payloads(self)
            .into_iter()
            .next()
            .map(|(_, _, payload)| payload))
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
