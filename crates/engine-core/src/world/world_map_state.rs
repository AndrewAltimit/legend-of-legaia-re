//! Overworld state: the world-map controller, its entity state machines and the encounter / region trackers.
//!
//! Split out of the composite [`World`] so the state one subsystem owns
//! reads as one unit. Fields keep their retail provenance notes.

use super::*;

/// Overworld state: the world-map controller, its entity state machines and the encounter / region trackers.
pub struct WorldMapState {
    /// World-map camera and entity state. `Some` when `mode == SceneMode::WorldMap`,
    /// `None` otherwise.
    pub ctrl: Option<WorldMapController>,
    /// Per-entity world-map state machines (the port of `FUN_801DA51C` in
    /// [`vm::world_map`]). One [`vm::world_map::WorldMapEntityCtx`] per
    /// installed overworld entity (encounter zones / town portals / NPCs).
    /// Empty unless [`Self::install_world_map_entities`] seeded them, so
    /// world-map mode without gameplay (camera-only) keeps ticking untouched.
    /// Driven each [`SceneMode::WorldMap`] tick by `Self::tick_world_map`.
    pub entities: Vec<vm::world_map::WorldMapEntityCtx>,
    /// Per-entity role config, paired by index with [`Self::world_map_entities`].
    /// Empty (or shorter than the entity list) means an entity has no specific
    /// role: its encounters fall back to [`Self::world_map_encounter`]'s shared
    /// formation and it surfaces a plain interaction. Installed together with
    /// the entities via [`Self::install_world_map_entities_with_configs`].
    pub entity_configs: Vec<WorldMapEntityConfig>,
    /// Per-entity overworld world position `(x, z)`, paired by index with
    /// [`Self::world_map_entities`]. Populated only by
    /// [`Self::install_world_map_entities_at`] (the disc placement seeding);
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
    /// of `Self::tick_world_map` to flip into [`SceneMode::Battle`]. `None`
    /// between encounters.
    pub pending_encounter: Option<u16>,
    /// Region-keyed random-encounter state for the overworld (the
    /// `FUN_801D9E1C` port, [`crate::region_encounter`]). When set,
    /// `Self::tick_world_map` rolls it once per 128-unit tile the player
    /// crosses, latching [`Self::pending_world_map_encounter`] on a trigger.
    /// `None` on a camera-only world map (no region data routed).
    ///
    /// REF: FUN_801D9E1C
    pub region_tracker: Option<crate::region_encounter::RegionEncounterTracker>,
    /// Player tile (`world >> 7`) at the previous overworld step check, for
    /// per-tile step detection. `None` until the first world-map tick seeds it.
    pub last_tile: Option<(i32, i32)>,
    /// Overworld player walk speed in world units per frame (per held d-pad
    /// direction). Default [`Self::WORLD_MAP_PLAYER_SPEED`].
    pub player_speed: i16,
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
            player_speed: World::WORLD_MAP_PLAYER_SPEED,
        }
    }
}

impl Default for WorldMapState {
    fn default() -> Self {
        Self::new()
    }
}
