//! Per-scene field terrain: walkability grid, map-region block, zone table, floor-height LUT, object cells, elevation overrides and the region / tile trackers.
//!
//! Split out of the composite [`World`] so the state one subsystem owns
//! reads as one unit. Fields keep their retail provenance notes.

/// Per-scene field terrain: walkability grid, map-region block, zone table, floor-height LUT, object cells, elevation overrides and the region / tile trackers.
pub struct FieldTerrain {
    /// Fixed map origin pair at `(_DAT_80089118, _DAT_80089120)` - used by ext
    /// sub-op 0x24 (world position lerp toward fixed map origin).
    pub map_origin_xz: (i32, i32),
    /// Per-scene field collision / floor grid. Retail equivalent: the
    /// walkability map at `*(_DAT_1F8003EC) + 0x4000` that the locomotion
    /// collision check (`FUN_801cfe4c`) samples. One byte per 128-unit
    /// tile, `0x80`-byte rows, up to `0x80` rows (`0x4000` bytes). The
    /// **high nibble** holds 4 sub-cell wall bits (a `2x2` quadrant grid of
    /// `64x64` cells); the **low nibble** is a floor-elevation tier
    /// (unused by the wall check). Loaded at field entry from the per-scene
    /// `DATA\FIELD\<scene>.MAP` `+0x4000` region - that disc blob is the
    /// **base** grid, and the live retail grid byte-matches it with zero
    /// diffs. The field-VM `0x4C` outer-nibble-7 op then applies
    /// story-conditional wall *deltas* on top as the scene prescript runs;
    /// it does not author the grid from scratch.
    /// Empty until the first field scene is entered.
    pub collision_grid: Vec<u8>,
    /// Per-scene `.MAP` region-table block (the file's `+0x10000..+0x12000`
    /// region - retail `*(_DAT_1F8003EC) + 0x10000`). Scanned per tile by
    /// the [`crate::field_regions`] ports to rebuild [`crate::world::StoryFlagState::extra_flags`]
    /// (the `_DAT_8007B8F4` mirror) and the scratch attribute box. Empty for
    /// scenes without a field map.
    pub map_region_block: Vec<u8>,
    /// Per-scene MAN section-3 zone table (the camera-region records the
    /// boot walk installs at the control block `_DAT_801C6EA4 + 0x4`):
    /// a count byte + 18-byte records, queried per tile by
    /// [`crate::field_regions::zone_query`]. Empty for scenes whose MAN has
    /// no section 3.
    pub zone_table: Vec<u8>,
    /// The scratchpad region-attribute block (`0x1F800384..87` +
    /// `0x1F80037C`) latched by the per-tile refresh; read by the zone
    /// query's kind-0 arm.
    pub region_attributes: crate::field_regions::RegionAttributes,
    /// The 18-byte zone record the player currently stands in (the camera-
    /// region payload `FUN_801DBC20` consumes), refreshed on tile crossing.
    /// `None` when no zone record matches (retail loads the default camera
    /// parameter set).
    pub zone_record: Option<[u8; crate::field_regions::ZONE_RECORD_STRIDE]>,
    /// The 16-entry floor-height LUT the collision grid's low nibble indexes
    /// (retail `DAT_1F80035C`, filled from the MAN header by `FUN_8003AEB0` as
    /// 16 negated `s16` elevation tiers). Resolved per-scene into here from
    /// [`crate::scene::SceneAssets::field_floor_height_lut`]; consumed by
    /// [`World::sample_field_floor_height`] (the port of `FUN_80019278`). All
    /// zero until a field scene supplies it.
    pub floor_height_lut: [i16; 16],
    /// The `.MAP` **object-grid** cell words (`+0x8000`, one `u16` per tile,
    /// `0x80 x 0x80`). [`World::sample_field_floor_height`] tests each tile's
    /// [`crate::world::CELL_ELEVATION_OVERRIDE`] (`0x800`) bit to pick the
    /// floor model: bilinear corner-nibble surface, or the flat tile mean plus
    /// the tile's [`crate::world::FieldTerrain::elevation_overrides`] record (ramps / stairs).
    /// Empty until a field scene supplies it - then every tile reads as a
    /// plain bilinear tile, the pre-override behaviour.
    pub object_cells: Vec<u16>,
    /// Which object-grid cell bit this scene records its **standable floor**
    /// with - [`legaia_asset::field_objects::CELL_WALK_VISIBLE`] (`0x1000`) for
    /// the scenes that set it, [`legaia_asset::field_objects::CELL_VISIBLE`]
    /// (`0x2000`) for the ones that never do.
    ///
    /// Both bits are draw gates on the same `u16`; `0x1000` is the walk view's
    /// and `0x2000` the overhead one's. Most scenes set `0x1000` on every tile
    /// the party may stand on, but eighteen of the disc's field scenes (the
    /// Sol / Karisto `kor*` band, the Drake-castle `jouin*` interiors,
    /// `tunnela`, `jagaroom`, `noaru`, `juui2`, `dream`, `edkorout`, `edlast`)
    /// carry object grids with `0x2000` cells and **not one** `0x1000` cell.
    /// Gating the standable test on `0x1000` alone therefore reads those scenes
    /// as having no floor at all, which makes
    /// [`World::resolve_cold_field_spawn`] inert there: every component is
    /// empty, so the retail seat is returned unresolved and `kor5` seats the
    /// player inside a wall.
    ///
    /// Retail never consults this grid to decide where the player may stand -
    /// standing is the collision grid's wall bits
    /// ([`World::field_tile_is_wall`]) - so the bit is only ever the port's
    /// extra "is this inside the authored area" filter, and the filter has to
    /// use whichever bit the scene actually authored.
    ///
    /// Set by [`World::load_field_object_cells`]; `CELL_WALK_VISIBLE` before
    /// any scene supplies a grid.
    pub floor_cell_bit: u16,
    /// The scene's kind-2 `.MAP` **elevation-override** records, primary
    /// (`+0x10000`) table followed by the fallback (`+0x12000`) one, so a
    /// linear first-match scan reproduces `FUN_801D5630`'s order. Consumed by
    /// [`World::sample_field_floor_height`] on
    /// [`crate::world::CELL_ELEVATION_OVERRIDE`] tiles.
    pub elevation_overrides: Vec<crate::world::ElevationOverride>,
    /// Live floor-height-ladder oscillators (field-VM op `0x4C` nibble-9
    /// sub-`0..2`, retail template `0x801F27EC` / tick `FUN_801DA930`). Each
    /// drives one rung of [`crate::world::FieldTerrain::floor_height_lut`].
    pub floor_tier_bobs: Vec<legaia_engine_vm::field_actor_timers::FloorTierBob>,
    /// Player tile `(col, row)` on the previous live-loop field tick. A
    /// change between ticks is one "step" and drives the encounter roll,
    /// mirroring the retail per-step counter rather than a per-frame roll.
    /// `None` until the first field tick records a tile. Managed by the live
    /// loop.
    pub last_tile: Option<(i16, i16)>,
    /// Region-keyed random-encounter state for the current FIELD scene (the
    /// same [`crate::region_encounter`] `FUN_801D9E1C` port the overworld
    /// uses, [`crate::world::WorldMapState::region_tracker`]). When set,
    /// [`Self::on_field_step`] rolls against the player's *active region*
    /// (per-region rate increment + formation-range pick) and drives the
    /// trigger through the [`crate::encounter::EncounterSession`]'s
    /// transition / grace SM, instead of the session's mean-rate tracker.
    /// `None` on scenes whose MAN has no encounter-region section (towns,
    /// or any engine that hasn't routed per-region data) - those fall back
    /// to the aggregated mean-rate `EncounterSession`.
    ///
    /// REF: FUN_801D9E1C
    pub region_tracker: Option<crate::region_encounter::RegionEncounterTracker>,
}

impl FieldTerrain {
    pub fn new() -> Self {
        Self {
            map_origin_xz: (0, 0),
            collision_grid: Vec::new(),
            map_region_block: Vec::new(),
            zone_table: Vec::new(),
            region_attributes: crate::field_regions::RegionAttributes::DEFAULT_FILL,
            zone_record: None,
            floor_height_lut: [0i16; 16],
            object_cells: Vec::new(),
            floor_cell_bit: legaia_asset::field_objects::CELL_WALK_VISIBLE,
            elevation_overrides: Vec::new(),
            floor_tier_bobs: Vec::new(),
            last_tile: None,
            region_tracker: None,
        }
    }
}

impl Default for FieldTerrain {
    fn default() -> Self {
        Self::new()
    }
}
