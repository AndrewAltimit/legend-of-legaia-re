//! Overworld state: the world-map controller, its entity state machines and the encounter / region trackers.
//!
//! One subsystem's state, held as a unit on the composite [`World`].
//! Fields keep their retail provenance notes.

use super::*;

/// Overworld state: the world-map controller, its entity state machines and the encounter / region trackers.
pub struct WorldMapState {
    /// World-map camera and entity state. `Some` when `mode == SceneMode::WorldMap`,
    /// `None` otherwise.
    pub ctrl: Option<WorldMapController>,
    /// Per-entity world-map state machines (the port of `FUN_801DA51C` in
    /// [`legaia_engine_field::world_map_entity`]). One [`legaia_engine_field::world_map_entity::WorldMapEntityCtx`] per
    /// installed overworld entity (encounter zones / town portals / NPCs).
    /// Empty unless [`crate::world::World::install_world_map_entities`] seeded them, so
    /// world-map mode without gameplay (camera-only) keeps ticking untouched.
    /// Driven each [`legaia_engine_mode::mode::SceneMode::WorldMap`] tick by `Self::tick_world_map`.
    pub(crate) entities: Vec<legaia_engine_field::world_map_entity::WorldMapEntityCtx>,
    /// Per-entity role config, paired by index with [`crate::world::WorldMapState::entities`].
    /// Empty (or shorter than the entity list) means an entity has no specific
    /// role: its encounters fall back to [`crate::world::WorldMapState::encounter`]'s shared
    /// formation and it surfaces a plain interaction. Installed together with
    /// the entities via [`crate::world::World::install_world_map_entities_with_configs`].
    pub entity_configs: Vec<WorldMapEntityConfig>,
    /// Per-entity overworld world position `(x, z)`, paired by index with
    /// [`crate::world::WorldMapState::entities`]. Populated only by
    /// [`crate::world::World::install_world_map_entities_at`] (the disc placement seeding);
    /// the config-only installers leave it empty. When present, it drives the
    /// **auto-engage-on-walkover** trigger in `Self::tick_world_map`: the
    /// player stepping onto a `Portal` entity's tile fires its transition with
    /// no host call, the port-side stand-in for retail's per-entity
    /// player-position-in-zone check.
    pub entity_positions: Vec<(i16, i16)>,
    /// Shared overworld encounter-rate state - the retail globals the
    /// world-map entity SM reads (`DAT_8007b604` countdown, `DAT_8007b5f8`
    /// enable flag) plus the formation an overworld encounter spawns.
    pub(crate) encounter: WorldMapEncounterState,
    /// Whether the player is moving on the overworld this tick (the entity
    /// SM's `_DAT_8007c364[+0x10] & 0x80000` player-walking gate). Set from
    /// the pad each world-map tick; a stationary player lets the interaction
    /// check fire.
    pub(crate) player_walking: bool,
    /// Overworld encounter pending resolution into a battle: the formation id
    /// an entity SM's encounter handler latched this frame. Drained at the end
    /// of `Self::tick_world_map` to flip into [`legaia_engine_mode::mode::SceneMode::Battle`]. `None`
    /// between encounters.
    pub(crate) pending_encounter: Option<u16>,
    /// Region-keyed random-encounter state for the overworld (the
    /// `FUN_801D9E1C` port, [`legaia_engine_battle::region_encounter`]). When set,
    /// `Self::tick_world_map` rolls it once per 128-unit tile the player
    /// crosses, latching [`crate::world::WorldMapState::pending_encounter`] on a trigger.
    /// `None` on a camera-only world map (no region data routed).
    ///
    /// REF: FUN_801D9E1C
    pub region_tracker: Option<legaia_engine_battle::region_encounter::RegionEncounterTracker>,
    /// Player tile (`world >> 7`) at the previous overworld step check, for
    /// per-tile step detection. `None` until the first world-map tick seeds it.
    pub last_tile: Option<(i32, i32)>,
    /// The tile a debug seat put a **standing** player on
    /// ([`legaia_engine_scene_host::scene_host::SceneHost::debug_seat_standing`]). Portal auto-engage
    /// skips it until the player steps off: a capture of a player stood on a
    /// portal tile is one whose portal has already fired, so re-engaging it on
    /// the first tick would cross the door a second time. `None` otherwise.
    pub seat_hold_tile: Option<(i32, i32)>,
    /// Sub-step carry of the overworld walk, in `1 / WORLD_MAP_FRAME_STEP`
    /// units: each tick adds one retail frame's (2-unit-rounded) step and
    /// commits a 2-unit sub-step per `2 * WORLD_MAP_FRAME_STEP` it holds, so
    /// the per-vsync port lands retail's `dt = 3` displacement over time
    /// (`10` units every `3` vsyncs). Cleared when no direction is held.
    pub(crate) walk_carry: i32,
    /// The kingdom MAN's world-map location table (`DAT_80073EE0`, the
    /// section-5 trailer): the places the map display labels and its cursor
    /// steps between. Installed at scene load
    /// ([`crate::world::World::install_world_map_locations`]); empty off the
    /// three kingdom maps.
    pub locations: Vec<legaia_engine_field::map_display::MapLocation>,
    /// The live map display (retail game mode `0x0D`, PROT 0981), `Some` from
    /// the frame the L1 fade-up completes until its exit hands the frame back
    /// to the walk. While it is up it owns the whole overworld frame.
    pub map_display: Option<legaia_engine_field::map_display::MapDisplay>,
    /// What every entry of the overworld curvature table is offset by, in
    /// screen rows: `0` from the scene load's build, and
    /// [`legaia_render_kernels::overworld_curvature::REBUILD_BIAS`] once the
    /// map display has rebuilt the table inside a running frame. The lift
    /// outlasts the display and holds until the next scene load
    /// ([`crate::world::World::overworld_curve_bias`]).
    pub curve_bias: i16,
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
            locations: Vec::new(),
            map_display: None,
            curve_bias: 0,
        }
    }
}

impl Default for WorldMapState {
    fn default() -> Self {
        Self::new()
    }
}
