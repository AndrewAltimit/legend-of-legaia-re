//! Overworld state: the world-map controller, its entity state machines and the encounter / region trackers.
//!
//! Split out of the composite `World` so the state one subsystem owns
//! reads as one unit. Fields keep their retail provenance notes.

use crate::world_map::WorldMapController;

/// Overworld state: the world-map controller, its entity state machines and the encounter / region trackers.
pub struct WorldMapState {
    /// World-map camera and entity state. `Some` when `mode == SceneMode::WorldMap`,
    /// `None` otherwise.
    pub ctrl: Option<WorldMapController>,
    /// Per-entity world-map state machines (the port of `FUN_801DA51C` in
    /// [`crate::world_map_entity`]). One [`crate::world_map_entity::WorldMapEntityCtx`] per
    /// installed overworld entity (encounter zones / town portals / NPCs).
    /// Empty unless `World::install_world_map_entities` seeded them, so
    /// world-map mode without gameplay (camera-only) keeps ticking untouched.
    /// Driven each [`legaia_engine_mode::mode::SceneMode::WorldMap`] tick by `Self::tick_world_map`.
    pub entities: Vec<crate::world_map_entity::WorldMapEntityCtx>,
    /// Per-entity role config, paired by index with [`WorldMapState::entities`].
    /// Empty (or shorter than the entity list) means an entity has no specific
    /// role: its encounters fall back to [`WorldMapState::encounter`]'s shared
    /// formation and it surfaces a plain interaction. Installed together with
    /// the entities via `World::install_world_map_entities_with_configs`.
    pub entity_configs: Vec<WorldMapEntityConfig>,
    /// Per-entity overworld world position `(x, z)`, paired by index with
    /// [`WorldMapState::entities`]. Populated only by
    /// `World::install_world_map_entities_at` (the disc placement seeding);
    /// the config-only installers leave it empty. When present, it drives the
    /// **auto-engage-on-walkover** trigger in `Self::tick_world_map`: the
    /// player stepping onto a `Portal` entity's tile fires its transition with
    /// no host call, the port-side stand-in for retail's per-entity
    /// player-position-in-zone check.
    pub entity_positions: Vec<(i16, i16)>,
    /// Shared overworld encounter-rate state - the retail globals the
    /// world-map entity SM reads (`DAT_8007b604` countdown, `DAT_8007b5f8`
    /// enable flag) plus the formation an overworld encounter spawns.
    pub encounter: WorldMapEncounterState,
    /// Whether the player is moving on the overworld this tick (the entity
    /// SM's `_DAT_8007c364[+0x10] & 0x80000` player-walking gate). Set from
    /// the pad each world-map tick; a stationary player lets the interaction
    /// check fire.
    pub player_walking: bool,
    /// Overworld encounter pending resolution into a battle: the formation id
    /// an entity SM's encounter handler latched this frame. Drained at the end
    /// of `Self::tick_world_map` to flip into [`legaia_engine_mode::mode::SceneMode::Battle`]. `None`
    /// between encounters.
    pub pending_encounter: Option<u16>,
    /// Region-keyed random-encounter state for the overworld (the
    /// `FUN_801D9E1C` port, [`legaia_engine_battle::region_encounter`]). When set,
    /// `Self::tick_world_map` rolls it once per 128-unit tile the player
    /// crosses, latching [`WorldMapState::pending_encounter`] on a trigger.
    /// `None` on a camera-only world map (no region data routed).
    ///
    /// REF: FUN_801D9E1C
    pub region_tracker: Option<legaia_engine_battle::region_encounter::RegionEncounterTracker>,
    /// Player tile (`world >> 7`) at the previous overworld step check, for
    /// per-tile step detection. `None` until the first world-map tick seeds it.
    pub last_tile: Option<(i32, i32)>,
    /// The tile a debug seat put a **standing** player on
    /// (`legaia_engine_core::scene_host::SceneHost::debug_seat_standing`). Portal auto-engage
    /// skips it until the player steps off: a capture of a player stood on a
    /// portal tile is one whose portal has already fired, so re-engaging it on
    /// the first tick would cross the door a second time. `None` otherwise.
    pub seat_hold_tile: Option<(i32, i32)>,
    /// Sub-step carry of the overworld walk, in `1 / WORLD_MAP_FRAME_STEP`
    /// units: each tick adds one retail frame's (2-unit-rounded) step and
    /// commits a 2-unit sub-step per `2 * WORLD_MAP_FRAME_STEP` it holds, so
    /// the per-vsync port lands retail's `dt = 3` displacement over time
    /// (`10` units every `3` vsyncs). Cleared when no direction is held.
    pub walk_carry: i32,
}

impl WorldMapState {
    pub fn new() -> Self {
        Self {
            ctrl: None,
            entities: Vec::new(),
            entity_configs: Vec::new(),
            entity_positions: Vec::new(),
            encounter: WorldMapEncounterState::default(),
            player_walking: false,
            pending_encounter: None,
            region_tracker: None,
            last_tile: None,
            seat_hold_tile: None,
            walk_carry: 0,
        }
    }
}

impl Default for WorldMapState {
    fn default() -> Self {
        Self::new()
    }
}

/// Per-overworld-entity role. The retail engine builds one record per on-map
/// entity from the scene's entity table; this is the port's slice the
/// gameplay SM acts on - an encounter zone spawns its own formation, a portal
/// targets a scene, an NPC just surfaces an interaction. Paired by index with
/// `WorldMapState::entities`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorldMapEntityConfig {
    /// A roaming-encounter zone: when the shared countdown drains while this
    /// entity is the one that fires, it spawns `formation_id` (instead of the
    /// map-wide [`WorldMapEncounterState::formation_id`]).
    EncounterZone { formation_id: u16 },
    /// A **mode-24 minigame door** placed on the overworld: engaging it (via
    /// `World::engage_world_map_entity`) arms the same door warp the field
    /// VM's op-`0x3E` arm does ([`legaia_engine_mode::minigame_entry::MinigameSubId`]).
    ///
    /// `sub_id` is the op's `op0 - 100`, which selects a *code overlay*, not a
    /// scene - so this variant never reaches a
    /// [`legaia_engine_scene::scene::MapIdResolver`]. It is sourced from the placement
    /// classifier's `PlacementKind::Portal`, which `is_genuine_warp` gates to
    /// `op0` in `100..=106`; on the three overworld scenes the only such
    /// placements are the `map02` / `map03` fishing signboards (`sub_id 0`).
    ///
    /// The overworld's town / dungeon entrances are **not** this - they are
    /// [`Self::OverworldPortal`], sourced from the `0x3F` named-scene-change
    /// bridge below, and they are the only producer of
    /// [`crate::field_events::FieldEvent::WorldMapTransition`].
    MinigameDoor { sub_id: u8 },
    /// An overworld town / dungeon entrance sourced from the disc's `.MAP`
    /// walk-on tile-trigger → MAN partition-2 record → `0x3F`
    /// named-scene-change bridge (pinned on the `map01` hub: each gate-1
    /// kind-1 tile trigger references a partition-2 record whose script's
    /// `0x3F` op carries the destination scene name + arrival entry tile).
    ///
    /// Unlike the door-warp [`Self::Portal`] (a 7-id scene-*type* selector
    /// whose name resolution lives in an uncaptured overlay), this carries
    /// the **exact CDNAME destination** + arrival tile straight from the
    /// controller's bytecode, so engaging it warps to a real scene. Engaging
    /// it surfaces a [`crate::field_events::FieldEvent::WorldMapTransition`]
    /// (whose `slot` points back at this config); the host's transition drain
    /// reads the destination from here.
    OverworldPortal {
        /// Destination CDNAME scene label (e.g. `"keikoku"`, `"rikuroa"`).
        scene_name: String,
        /// The `0x3F` op's `i16` destination index (story/entry id; the
        /// wider [`legaia_engine_scene::scene::SceneDestinationResolver`] key, not the
        /// door-warp `map_id`).
        index: i16,
        /// Arrival entry-tile X byte at the destination (`& 0x7F` tile).
        entry_x: u8,
        /// Arrival entry-tile Z byte at the destination.
        entry_z: u8,
        /// Arrival facing/depth selector (`& 7` indexes the entry-direction
        /// table).
        dir: u8,
        /// The partition-2 record the entrance's walk-on trigger names - the
        /// script retail runs on the crossing. The transition drain replays
        /// the flag operations that open it
        /// (`legaia_engine_core::place_name_banner::record_leading_flag_writes`).
        /// With [`Self::OverworldPortal::object`] set it is instead the
        /// **flat** MAN record a `.MAP` object bind names.
        record: u8,
        /// The entrance is a `.MAP` **object** (a gate-0 bind whose record
        /// runs on contact - `map01` P0[6], the Garmel mouth), not a gate-1
        /// walk-on trigger. `record` is then a flat record index, and the
        /// crossing replays the flag writes along the record's path to its
        /// `0x3F` ([`crate::man_field_scripts::flat_record_path_walk`]).
        object: bool,
    },
    /// A plain interactable (NPC / signpost). Surfaces a
    /// [`crate::field_events::FieldEvent::FieldInteract`] with `interact_id`.
    ///
    /// `inline` is the entity's own placement-script dialog text (the `0x3F`
    /// op's inline buffer the placement walker captured); `World::tick_world_map`
    /// opens it when the player presses confirm next to the entity. `text_id`
    /// is the box-config id carried alongside it (not an MES index). Both are
    /// empty/`None` when the placement has an interaction but no inline dialog.
    Npc {
        interact_id: u8,
        text_id: Option<u16>,
        inline: Vec<u8>,
    },
}

/// Shared overworld encounter-rate state read by the world-map entity SM.
#[derive(Debug, Clone)]
pub struct WorldMapEncounterState {
    /// `DAT_8007b604` - signed per-step encounter countdown shared across all
    /// overworld entities. Decremented in the entity SM's Idle state; an
    /// encounter fires when it reaches zero (and [`Self::enabled`]).
    pub countdown: i8,
    /// `DAT_8007b5f8 != 0` - master encounter-enable flag. When `false`,
    /// overworld encounters never fire regardless of the countdown.
    pub enabled: bool,
    /// Formation id an overworld encounter spawns (resolved against
    /// `DiscTables::formation_table`). The retail per-region resolution
    /// (`FUN_800243F0` → the MAN region table) is a separate thread; v0.1
    /// uses one configured formation for the whole map.
    pub formation_id: u16,
    /// Frames to reset the countdown to after an encounter fires (so the next
    /// encounter is paced rather than re-firing every frame at zero).
    pub reset_to: i8,
}

impl Default for WorldMapEncounterState {
    fn default() -> Self {
        Self {
            countdown: 0,
            enabled: false,
            formation_id: 0,
            reset_to: 64,
        }
    }
}
