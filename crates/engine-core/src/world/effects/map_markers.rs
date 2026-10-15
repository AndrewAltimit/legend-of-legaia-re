//! World-map entity and player marker projections.

use super::*;
use legaia_engine_field::world_map_markers::{
    WorldMapEntityKind, WorldMapEntityMarker, WorldMapPlayerMarker,
};

impl World {
    /// Snapshot every placed overworld entity for the renderer: one
    /// [`WorldMapEntityMarker`] per installed entity that carries a world
    /// position, paired with its coarse [`WorldMapEntityKind`].
    ///
    /// Returns an empty vector unless the disc-placement seeding
    /// ([`Self::install_world_map_entities_at`]) populated
    /// [`legaia_engine_field::world_map_entity_config::WorldMapState::entity_positions`] - the config-only installers
    /// (which leave positions empty) produce no markers, so a camera-only or
    /// synthetic world map degrades cleanly. The marker `y` is the player
    /// actor's current plane (the placements are 2D), so markers sit on the
    /// player's walking plane rather than at an arbitrary `y = 0`.
    pub fn world_map_entity_markers(&self) -> Vec<WorldMapEntityMarker> {
        if self.world_map.entity_positions.is_empty() {
            return Vec::new();
        }
        let base_y = self
            .player_actor_slot
            .and_then(|s| self.actors.get(s as usize))
            .map(|a| a.move_state.world_y as f32)
            .unwrap_or(0.0);
        self.world_map
            .entity_positions
            .iter()
            .enumerate()
            .map(|(i, &(x, z))| {
                let kind = WorldMapEntityKind::of_config(self.world_map.entity_configs.get(i));
                WorldMapEntityMarker {
                    world_pos: [x as f32, base_y, z as f32],
                    kind,
                }
            })
            .collect()
    }

    /// The player's overworld position for the renderer, or `None` when there
    /// is no active player actor. The world-map draw path renders the party
    /// leader's field mesh at this position, oriented by
    /// [`WorldMapPlayerMarker::facing`]; the marker doubles as the diagnostic
    /// wireframe stand-in when the mesh upload failed.
    pub fn world_map_player_marker(&self) -> Option<WorldMapPlayerMarker> {
        let slot = self.player_actor_slot? as usize;
        let a = self.actors.get(slot)?;
        if !a.active {
            return None;
        }
        Some(WorldMapPlayerMarker {
            world_pos: [
                a.move_state.world_x as f32,
                a.move_state.world_y as f32,
                a.move_state.world_z as f32,
            ],
            facing: a.move_state.render_26,
        })
    }
}
