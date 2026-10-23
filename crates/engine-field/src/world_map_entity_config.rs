//! The overworld entity configuration records: each on-map entity's role
//! ([`WorldMapEntityConfig`]) and the encounter-zone countdown
//! ([`WorldMapEncounterState`]) the world-map entity state machine acts on.

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
    /// Engaging is the fallback, not the retail trigger. Retail reaches the
    /// door through the action-button probe, which runs the placement's own
    /// interaction record (the fishing signboard's frozen-lake / rod / lure
    /// checks, "Do you want to fish?", a 48-frame white fade) and lets that
    /// record's `0x3E` do the warp. The world map takes that path whenever
    /// the placement installed a record (keyed by [`Self::MinigameDoor::slot`])
    /// and the field-VM dialogue runner is on; only a door without one, or
    /// the simplified-dialogue mode with no runner, still warps on walk-over.
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
    MinigameDoor {
        /// The op's `op0 - 100`: the mode-24 overlay selector.
        sub_id: u8,
        /// The placement's partition-1 index - the `slot` its interaction
        /// record is installed under (`World::npcs.dialog_prologue`).
        slot: u8,
    },
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
        /// (`legaia_engine_scene_host::place_name_banner::record_leading_flag_writes`).
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
