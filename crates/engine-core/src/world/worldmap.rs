//! World-map ticking, locomotion, entity install/engage, portal auto-engage, NPC dialog, and encounter setup.
//!
//! Split out of `world.rs` as additional `impl World` blocks; no logic
//! change from the original inline definitions.

use super::*;

impl World {
    /// Drive the world-map controller from this frame's pad.
    ///
    /// The pad word is whatever the host installed via [`World::set_pad`]
    /// before this [`World::tick`]; the "held" mask is the just-pressed
    /// edge (`pad & !pad_prev`), matching the retail newly-pressed word
    /// (`_DAT_8007B874`) that [`WorldMapController::tick`] expects. No-ops
    /// when no controller is installed (e.g. world-map mode entered
    /// without [`World::enter_world_map`]).
    ///
    /// Reads only pad bits, never wall-clock state, so the resulting
    /// controller mutation is deterministic across identical pad streams.
    ///
    /// When overworld entities are installed
    /// ([`Self::install_world_map_entities`]) it also steps each one through
    /// the ported entity SM ([`vm::world_map::step`]): the Idle state drains
    /// the shared encounter countdown and, when it reaches zero with
    /// encounters enabled, latches the configured formation, which this method
    /// then resolves into a battle ([`SceneMode::WorldMap`] → [`SceneMode::Battle`],
    /// returning to the world map on victory). Interactions / portal
    /// transitions surface as [`FieldEvent::FieldInteract`] for the host.
    ///
    /// The per-entity SM itself is ported in [`vm::world_map`]; the encounter
    /// formation resolver it gates is the retail BGM/asset resolver.
    ///
    /// REF: FUN_801DA51C
    /// REF: FUN_800243F0
    pub(crate) fn tick_world_map(&mut self) {
        let pad = self.input.pad();
        let pad_held = pad & !self.input.pad_prev();
        if let Some(ctrl) = &mut self.world_map_ctrl {
            ctrl.tick(pad, pad_held);
        }
        // Player is "walking" on the overworld this frame when any d-pad
        // direction is held. These are the Up/Right/Down/Left bits the
        // locomotion step ([`Self::step_world_map_locomotion`]) reads - the
        // face buttons (Triangle/Circle/Cross/Square, 0x1000..0x8000) must not
        // count as walking or a confirm press would suppress the talk-to gate.
        const WORLD_MAP_DPAD: u16 = input::PadButton::Up as u16
            | input::PadButton::Right as u16
            | input::PadButton::Down as u16
            | input::PadButton::Left as u16;
        self.world_map_player_walking = pad & WORLD_MAP_DPAD != 0;

        // Talk-to: open / dismiss an adjacent NPC's dialogue on a confirm
        // press. Runs before locomotion so opening a box suppresses movement +
        // portal auto-engage this frame (both gate off `current_dialog`).
        self.tick_world_map_npc_dialog();

        // Move the player first, then auto-engage any portal the player just
        // walked onto (sets its SM to Transitioning), so the entity SM step
        // below fires the portal's transition the *same* tick.
        self.step_world_map_locomotion();
        self.auto_engage_world_map_portals();

        if !self.world_map_entities.is_empty() {
            // Take the entity list out so the SM's host bridge can borrow the
            // world mutably (mirrors the monster-AI-state borrow window).
            let mut entities = std::mem::take(&mut self.world_map_entities);
            for (idx, ctx) in entities.iter_mut().enumerate() {
                let mut host = WorldMapEntityHostImpl { world: self };
                vm::world_map::step(idx, ctx, &mut host);
            }
            self.world_map_entities = entities;
        }

        // The region-keyed random-encounter roll (the `FUN_801D9E1C` path): on
        // each 128-unit tile crossing, roll the active region. No-op without a
        // region tracker, so a camera-only world map is unchanged.
        self.live_world_map_tick();

        // Resolve a latched overworld encounter into a battle. Runs for both
        // the entity-SM countdown path and the region-roll path.
        if let Some(formation_id) = self.pending_world_map_encounter.take() {
            self.begin_world_map_encounter(formation_id);
        }
    }

    /// Overworld player walk speed in world units per frame (per held d-pad
    /// direction). The field player moves ~8 units/frame
    /// (`FIELD_BASE_STEP`); the overworld uses the same baseline.
    pub const WORLD_MAP_PLAYER_SPEED: i16 = 8;

    /// Move the overworld player actor from the held d-pad, bounded by the
    /// scene's walkability grid.
    ///
    /// Held d-pad is remapped through the overworld camera azimuth
    /// ([`world_map_camera_relative_bits`]) so "screen up" walks away from the
    /// follow camera and "screen right" walks screen-right regardless of how
    /// the map is rotated - the same camera-relative remap retail's
    /// `func_0x800467e8` applies, and the counterpart to the field's
    /// [`Self::decode_field_direction`].
    ///
    /// Collision is **not** a separate unknown: the retail world-map-walk
    /// overlay's locomotion is the same `FUN_801d01b0` as the field, colliding
    /// against the same `_DAT_1f8003ec + 0x4000` walkability grid
    /// ([`Self::field_tile_is_wall`]) which [`crate::scene::SceneHost::enter_field_scene`]
    /// already loads from the scene's MAP file. Stepping runs through the shared
    /// [`Self::advance_with_collision`], so walls stop the overworld player
    /// exactly as on the field.
    ///
    /// No-op without a live player actor, while a dialog owns the frame, in the
    /// top-view debug camera, or while the player's movement-disabled flag
    /// (`+0x10 & 0x80000`) is set (encounter queued / cutscene owns the player).
    fn step_world_map_locomotion(&mut self) {
        if self.current_dialog.is_some() {
            return;
        }
        // In the top-view debug camera the d-pad scrolls the camera
        // ([`WorldMapController::tick`]); only walk the player in walk mode.
        if self
            .world_map_ctrl
            .as_ref()
            .is_some_and(|c| c.is_top_view())
        {
            return;
        }
        let Some(slot) = self.player_actor_slot else {
            return;
        };
        let slot = slot as usize;
        if slot >= self.actors.len() || !self.actors[slot].active {
            return;
        }
        if self.actors[slot].move_state.flags & 0x0008_0000 != 0 {
            return;
        }
        // Held d-pad → camera-relative direction bits. `sx`/`sy` are the raw
        // screen deltas (right / forward); the azimuth remap rotates them into
        // world space against the overworld follow camera.
        let pad = self.input.pad();
        let mut sx = 0i32;
        let mut sy = 0i32;
        if pad & input::PadButton::Up.mask() != 0 {
            sy += 1;
        }
        if pad & input::PadButton::Down.mask() != 0 {
            sy -= 1;
        }
        if pad & input::PadButton::Right.mask() != 0 {
            sx += 1;
        }
        if pad & input::PadButton::Left.mask() != 0 {
            sx -= 1;
        }
        let azimuth = self.world_map_ctrl.as_ref().map(|c| c.azimuth).unwrap_or(0);
        let dir_bits = world_map_camera_relative_bits(azimuth, sx, sy);
        if dir_bits == 0 {
            return;
        }
        // Record the heading from the world-space movement direction (the same
        // `render_26` field the field path stores from `decode_field_direction`,
        // a PSX 12-bit angle: 4096 = full turn). The world-map walk uses the
        // camera-relative bits rather than `decode_field_direction`, so it must
        // set the heading itself; the player marker reads it to draw a facing
        // tick. Deterministic: same pad + azimuth -> same heading.
        let dz = (dir_bits & 0x1000 != 0) as i32 - (dir_bits & 0x4000 != 0) as i32;
        let dx = (dir_bits & 0x2000 != 0) as i32 - (dir_bits & 0x8000 != 0) as i32;
        if dx != 0 || dz != 0 {
            let heading = (((dx as f32).atan2(dz as f32) / std::f32::consts::TAU * 4096.0).round()
                as i32)
                .rem_euclid(4096) as i16;
            self.actors[slot].move_state.render_26 = heading;
        }
        let mut speed = self.world_map_player_speed.max(1) as i32;
        // Diagonal normalise: when both axes are moving, x0.75 - mirroring the
        // field controller (`FUN_801d01b0`) and the retail world-map walk
        // overlay (`speed -= speed >> 2`). `advance_with_collision` steps both
        // axes by the same amount, so without this a diagonal travels `speed` on
        // each axis = ~1.41x the cardinal speed.
        if dx != 0 && dz != 0 {
            speed -= speed >> 2;
        }
        self.advance_with_collision(slot, dir_bits, speed);
    }

    /// Per-tile overworld step → region-keyed encounter roll (the world-map
    /// counterpart to [`Self::live_field_tick`]).
    ///
    /// A "step" is the player actor crossing into a new 128-unit tile
    /// (`world >> 7`); each step drives one
    /// [`crate::region_encounter::RegionEncounterTracker::on_step`] against the
    /// player's current position. A trigger latches
    /// [`Self::pending_world_map_encounter`], which [`Self::tick_world_map`]
    /// resolves into a battle. The RNG comes from the world's shared
    /// deterministic source, drawn only on the trigger branch, so replays stay
    /// bit-identical. No-op without a player actor or region tracker.
    fn live_world_map_tick(&mut self) {
        let Some(slot) = self.player_actor_slot else {
            return;
        };
        let (wx, wz) = match self.actors.get(slot as usize) {
            Some(a) => (a.move_state.world_x, a.move_state.world_z),
            None => return,
        };
        let tile = ((wx as i32) >> 7, (wz as i32) >> 7);
        let crossed = match self.world_map_last_tile {
            Some(prev) if prev != tile => {
                self.world_map_last_tile = Some(tile);
                true
            }
            None => {
                self.world_map_last_tile = Some(tile);
                false
            }
            _ => false,
        };
        if !crossed {
            return;
        }
        // Roll the active region. Take the tracker out so the RNG closure can
        // borrow `self` (same pattern as the entity-SM borrow window).
        if let Some(mut tracker) = self.world_map_region_tracker.take() {
            tracker.set_modifiers(self.encounter_rate_modifiers());
            let roll = tracker.on_step(wx, wz, || self.next_rng());
            self.world_map_region_tracker = Some(tracker);
            if let Some(roll) = roll {
                self.pending_world_map_encounter = Some(roll.formation_id as u16);
            }
        }
    }

    /// Route the scene's region-keyed encounter table onto the overworld so
    /// `Self::tick_world_map` rolls random encounters per region. Resets the
    /// step-tile latch. Pair with [`Self::enter_world_map`] (or call after it).
    pub fn set_world_map_regions(&mut self, table: crate::region_encounter::RegionEncounterTable) {
        self.world_map_region_tracker =
            Some(crate::region_encounter::RegionEncounterTracker::new(table));
        self.world_map_last_tile = None;
    }

    /// Route the current FIELD scene's region-keyed encounter table so
    /// [`Self::on_field_step`] rolls per *active region* (the retail
    /// `FUN_801D9E1C` rate counter + formation-range pick) instead of the
    /// aggregated mean-rate [`crate::encounter::EncounterSession`]. The
    /// session stays installed and supplies the transition / grace
    /// bracketing (via [`crate::encounter::EncounterSession::trigger_with`]);
    /// the region tracker just replaces *which* rate and formation a step
    /// rolls. `None` clears the field tracker (back to the mean path).
    ///
    /// The scene-entry path calls this from the same MAN the mean table is
    /// built from ([`crate::region_encounter::region_encounter_table_from_man`]),
    /// so a scene whose MAN has no encounter-region section keeps the mean
    /// path untouched.
    pub fn set_field_regions(
        &mut self,
        table: Option<crate::region_encounter::RegionEncounterTable>,
    ) {
        self.field_region_tracker = table.map(crate::region_encounter::RegionEncounterTracker::new);
    }

    /// Seed `count` overworld entity state machines (all Idle) so
    /// `Self::tick_world_map` drives encounter / interaction gameplay.
    /// Replaces any previously installed set. The retail engine builds one
    /// record per on-map entity from the scene's entity table; the clean-room
    /// world takes the count and pairs it with the shared encounter state
    /// configured via [`Self::set_world_map_encounter`].
    pub fn install_world_map_entities(&mut self, count: usize) {
        self.world_map_entities = (0..count)
            .map(|_| vm::world_map::WorldMapEntityCtx::default())
            .collect();
        self.world_map_entity_configs.clear();
    }

    /// Seed overworld entities with per-entity [`WorldMapEntityConfig`]s. One
    /// state machine (Idle) is created per config, so encounter zones spawn
    /// their own formation and portals carry their own target map. Replaces any
    /// previously installed set.
    pub fn install_world_map_entities_with_configs(&mut self, configs: Vec<WorldMapEntityConfig>) {
        self.world_map_entities = (0..configs.len())
            .map(|_| vm::world_map::WorldMapEntityCtx::default())
            .collect();
        self.world_map_entity_configs = configs;
        self.world_map_entity_positions.clear();
    }

    /// Seed overworld entities with a per-entity config **and** world position.
    /// One Idle state machine per `(config, position)`. The positions enable
    /// the auto-engage-on-walkover trigger in `Self::tick_world_map` (the
    /// player stepping onto a `Portal` entity's tile fires it). Replaces any
    /// previously installed set. This is the disc-placement seeding path
    /// ([`crate::scene::SceneHost::enter_world_map_scene`] feeds it the
    /// classified actor placements + their spawn positions).
    pub fn install_world_map_entities_at(
        &mut self,
        entities: Vec<(WorldMapEntityConfig, (i16, i16))>,
    ) {
        self.world_map_entities = (0..entities.len())
            .map(|_| vm::world_map::WorldMapEntityCtx::default())
            .collect();
        self.world_map_entity_positions = entities.iter().map(|(_, pos)| *pos).collect();
        self.world_map_entity_configs = entities.into_iter().map(|(cfg, _)| cfg).collect();
    }

    /// Auto-engage any `Portal` overworld entity the player is standing on.
    ///
    /// The clean-room stand-in for retail's per-entity player-position-in-zone
    /// trigger: a portal whose placement tile (`pos >> 7`) matches the player's
    /// current tile is driven to its transition state, exactly as a host
    /// [`Self::engage_world_map_entity`] call would, so the next SM step fires
    /// the [`crate::field_events::FieldEvent::WorldMapTransition`]. Only `Idle`
    /// portals are engaged, so a portal fires once per visit and the player can
    /// stand on the tile without re-triggering. NPC entities are *not*
    /// auto-engaged (they are talk-to, not walk-onto). No-op without entity
    /// positions, a player actor, or while a dialog owns the frame.
    fn auto_engage_world_map_portals(&mut self) {
        if self.current_dialog.is_some() || self.world_map_entity_positions.is_empty() {
            return;
        }
        let Some(slot) = self.player_actor_slot else {
            return;
        };
        let (px, pz) = match self.actors.get(slot as usize) {
            Some(a) => (
                (a.move_state.world_x as i32) >> 7,
                (a.move_state.world_z as i32) >> 7,
            ),
            None => return,
        };
        // Collect the portals the player is standing on (still Idle), then
        // engage them - separated so the immutable scan drops before the
        // mutable `engage` borrow.
        let mut to_engage: Vec<usize> = Vec::new();
        for (idx, ctx) in self.world_map_entities.iter().enumerate() {
            if ctx.state != vm::world_map::EntityState::Idle as u16 {
                continue;
            }
            if !matches!(
                self.world_map_entity_configs.get(idx),
                Some(WorldMapEntityConfig::Portal { .. })
                    | Some(WorldMapEntityConfig::OverworldPortal { .. })
            ) {
                continue;
            }
            let Some(&(ex, ez)) = self.world_map_entity_positions.get(idx) else {
                continue;
            };
            if (ex as i32) >> 7 == px && (ez as i32) >> 7 == pz {
                to_engage.push(idx);
            }
        }
        for idx in to_engage {
            self.engage_world_map_entity(idx);
        }
    }

    /// Open / dismiss an overworld NPC's dialogue on a confirm press.
    ///
    /// The talk-to counterpart of [`Self::auto_engage_world_map_portals`]
    /// (portals are walk-onto, NPCs are talk-to). While a box is up, a
    /// confirm/cancel press (`Cross`/`Circle`) dismisses it: the overworld has
    /// no field VM ticking to run the op-`0x4C` dismiss hook the field path
    /// uses ([`vm_hosts`](crate::world)), so the world map owns the dismiss
    /// directly. Otherwise, a confirm press while the player stands within one
    /// tile of an [`WorldMapEntityConfig::Npc`] that carries inline dialog text
    /// (the `Dialog` op the placement walker found) opens that text against the
    /// scene's MES container - sets [`Self::current_dialog`] and emits
    /// [`FieldEvent::OpenDialog`], which the host renders through
    /// [`crate::scene::SceneHost::open_pending_dialog`], the same panel path
    /// the field VM's op `0x3F` feeds.
    ///
    /// No-op while walking (a held direction is a movement frame, not a
    /// talk-to), without entity positions, or without a player actor. An NPC
    /// with an interaction but no inline text is left to the SM's
    /// [`FieldEvent::FieldInteract`] path unchanged.
    fn tick_world_map_npc_dialog(&mut self) {
        // A box is up: a confirm/cancel press dismisses it (and the locomotion
        // + auto-engage steps stay gated off `current_dialog` meanwhile).
        if self.current_dialog.is_some() {
            // The inline-script runner, when active, owns dismissal.
            if self.inline_dialogue.is_none()
                && (self.input.just_pressed(input::PadButton::Cross)
                    || self.input.just_pressed(input::PadButton::Circle))
            {
                self.current_dialog = None;
                self.pending_field_events.push(FieldEvent::DialogDismissed);
            }
            return;
        }
        // Otherwise a confirm press next to a talkable NPC opens its dialogue.
        if self.world_map_player_walking || !self.input.just_pressed(input::PadButton::Cross) {
            return;
        }
        if self.world_map_entity_positions.is_empty() {
            return;
        }
        let Some(slot) = self.player_actor_slot else {
            return;
        };
        let (px, pz) = match self.actors.get(slot as usize) {
            Some(a) => (
                (a.move_state.world_x as i32) >> 7,
                (a.move_state.world_z as i32) >> 7,
            ),
            None => return,
        };
        // First talkable NPC within one tile (Chebyshev) of the player. An NPC
        // is talkable when it carries inline dialog text or a box-config id.
        let mut open: Option<(u16, Vec<u8>)> = None;
        for (idx, cfg) in self.world_map_entity_configs.iter().enumerate() {
            let (text_id, inline) = match cfg {
                WorldMapEntityConfig::Npc {
                    text_id, inline, ..
                } if text_id.is_some() || !inline.is_empty() => {
                    (text_id.unwrap_or(0), inline.clone())
                }
                _ => continue,
            };
            let Some(&(ex, ez)) = self.world_map_entity_positions.get(idx) else {
                continue;
            };
            if ((ex as i32 >> 7) - px).abs() <= 1 && ((ez as i32 >> 7) - pz).abs() <= 1 {
                open = Some((text_id, inline));
                break;
            }
        }
        if let Some((text_id, inline)) = open {
            self.current_dialog = Some(DialogRequest {
                text_id,
                inline: inline.clone(),
                world_x: 0,
                world_z: 0,
                depth_id: 0,
            });
            self.pending_field_events.push(FieldEvent::OpenDialog {
                text_id,
                inline,
                world_x: 0,
                world_z: 0,
                depth_id: 0,
            });
        }
    }

    /// Host signal that the player engaged overworld entity `idx` (walked onto
    /// a portal tile / pressed confirm on it). Drives the entity SM straight to
    /// its scene-transition state so the next `Self::tick_world_map` fires the
    /// transition; a [`WorldMapEntityConfig::Portal`] then surfaces a
    /// [`crate::field_events::FieldEvent::WorldMapTransition`] with its target
    /// map. No-op for an out-of-range index.
    ///
    /// Hosts can call this directly; `Self::auto_engage_world_map_portals`
    /// also calls it each tick for any `Portal` entity the player has walked
    /// onto (the engine-driven trigger), so an entity installed with a position
    /// via [`Self::install_world_map_entities_at`] fires on walk-over without a
    /// host call.
    pub fn engage_world_map_entity(&mut self, idx: usize) {
        if let Some(ctx) = self.world_map_entities.get_mut(idx) {
            // State 2 = Transitioning: the SM fires `on_scene_transition` and
            // retires the entity on the next tick.
            ctx.state = vm::world_map::EntityState::Transitioning as u16;
        }
    }

    /// Remove and return the first pending
    /// [`crate::field_events::FieldEvent::WorldMapTransition`] as
    /// `(target_map, slot)`.
    ///
    /// The host's scene-transition drain calls this each tick to consume the
    /// overworld-portal engage the entity SM emitted (walk-onto a portal tile),
    /// leaving every other queued field event in place. `None` when no
    /// world-map transition is queued. The returned `slot` indexes
    /// [`Self::world_map_entity_configs`], where an
    /// [`WorldMapEntityConfig::OverworldPortal`] carries the real CDNAME
    /// destination.
    pub fn take_world_map_transition(&mut self) -> Option<(u16, u8)> {
        let pos = self
            .pending_field_events
            .iter()
            .position(|e| matches!(e, FieldEvent::WorldMapTransition { .. }))?;
        match self.pending_field_events.remove(pos) {
            FieldEvent::WorldMapTransition { target_map, slot } => Some((target_map, slot)),
            _ => unreachable!("position matched WorldMapTransition"),
        }
    }

    /// Configure the shared overworld encounter rate. `enabled` is the master
    /// gate, `start_countdown` the initial per-step counter, `formation_id`
    /// the formation an encounter spawns (resolved against
    /// [`Self::formation_table`]), and `reset_to` the value the countdown is
    /// reset to after each encounter fires.
    pub fn set_world_map_encounter(
        &mut self,
        enabled: bool,
        start_countdown: i8,
        formation_id: u16,
        reset_to: i8,
    ) {
        self.world_map_encounter = WorldMapEncounterState {
            enabled,
            countdown: start_countdown,
            formation_id,
            reset_to,
        };
    }
}
