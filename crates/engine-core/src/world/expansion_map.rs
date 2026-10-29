//! The `World` side of an expansion-pack map (`World::expansion.map`): the
//! encounter zone under the player, which keys the pack's encounter roll, and
//! the map location a save carries.
//!
//! Both are the identity with no map active: the roll keys on the scene
//! label as before, and a save carries no location.

use legaia_engine_expansion::map::MapLocation;

use super::World;

impl World {
    /// The key the pack's encounter tables are looked up under
    /// (`battle.toml` `[[encounters]] scene`): on a pack map the table of
    /// the encounter zone under the player - `None` outside every zone, so
    /// the map rolls nothing there - else the active scene's label.
    pub(in crate::world) fn expansion_encounter_key(&self) -> Option<String> {
        let Some(map) = self.expansion.map.as_ref() else {
            return Some(self.active_scene_label.clone());
        };
        let player = self.actors.get(self.player_actor_slot? as usize)?;
        let tile = map.tile_at_world(
            i32::from(player.move_state.world_x),
            i32::from(player.move_state.world_z),
        )?;
        map.encounter_table_at(tile).map(str::to_string)
    }

    /// The pack map location the player stands at, if on one.
    pub fn expansion_map_location(&self) -> Option<MapLocation> {
        let map = self.expansion.map.as_ref()?;
        let player = self.actors.get(self.player_actor_slot? as usize)?;
        Some(MapLocation {
            map_id: map.map_id.clone(),
            x: player.move_state.world_x,
            z: player.move_state.world_z,
            facing: player.move_state.render_26 as u16 & 0x0FFF,
        })
    }

    /// Stamp the pack map location onto the pack's progress, for the save
    /// about to be written ([`Self::expansion_save_block`]); cleared off every
    /// map. No-op with no pack.
    pub fn expansion_record_map_location(&mut self) {
        let Some(pack) = self.expansion.pack.clone() else {
            return;
        };
        let location = self.expansion_map_location();
        self.expansion.quests.sync_pack(&pack);
        self.expansion.quests.progress.map = location;
    }

    /// The pack map location the loaded save was made at, if any.
    pub fn expansion_saved_map_location(&self) -> Option<&MapLocation> {
        self.expansion
            .pack
            .as_ref()
            .and(self.expansion.quests.progress.map.as_ref())
    }
}
