//! Entering and stepping an expansion-pack map: `SceneHost::enter_expansion_map`
//! and the per-tick exit / zone step.
//!
//! A pack map is a field scene entered over a retail **base scene**
//! (`docs/formats/expansion-pack-maps.md`). The entry is the ordinary
//! [`SceneHost::enter_field_scene`] of the base scene with a
//! [`SceneOverride`] armed: `load_scene` seats it on the [`Scene`], so every
//! `.MAP` read of the entry - collision grid, floor ladder, placed props,
//! ground cells - comes from the pack's tiles, and the base scene's MAN stays
//! off. The base scene's VRAM, geometry pack and sound load as they would
//! for the retail scene, which is what draws the map.
//!
//! What a retail scene does through its MAN a pack map does through
//! [`ActiveMap`] on `World::expansion.map`: the host feeds it the player's
//! tile every tick, an exit it reports becomes a scene change, and its
//! trigger events and encounter table wait there for the battle, dialog and
//! quest runtimes.

use super::*;

use legaia_engine_expansion::map::ActiveMap;
use legaia_engine_scene::expansion_map::{
    BaseInputs, SceneOverride, build_field_map_image, floor_lut, tile_centre_world,
    world_to_map_tile,
};
use legaia_expansion_format::ExpansionPack;
use legaia_expansion_format::map::ExitTarget;
use legaia_expansion_format::refs::DiscRef;

/// The global BGM id of sound-test track `n`: the global pool is the
/// `music_01` bank in sound-test order, addressed from `2000`
/// (`docs/subsystems/script-vm.md`, "BGM lookup").
const GLOBAL_BGM_BASE: u16 = 2000;

impl SceneHost {
    /// Make `pack` the world's active expansion pack. Its maps can then be
    /// entered with [`Self::enter_expansion_map`].
    pub fn set_expansion_pack(&mut self, pack: Arc<ExpansionPack>) {
        self.world.expansion.pack = Some(pack);
    }

    /// The id of the pack map the player is in, if any.
    pub fn active_expansion_map(&self) -> Option<&str> {
        self.world.expansion.map.as_ref().map(|m| m.map_id.as_str())
    }

    /// Enter map `map_id` of the active pack as a playable field scene, the
    /// player at spawn `spawn` (the map's first spawn when empty).
    ///
    /// Fails, leaving the running scene as it was, when no pack is active,
    /// the map or spawn is unknown, the tiles do not resolve, or the base
    /// scene does not load.
    pub fn enter_expansion_map(&mut self, map_id: &str, spawn: &str) -> Result<()> {
        let pack = self
            .world
            .expansion
            .pack
            .clone()
            .context("no expansion pack is active")?;
        let def = pack
            .maps
            .get(map_id)
            .with_context(|| format!("pack '{}' has no map '{map_id}'", pack.manifest.id))?;
        let grid = def
            .tile_grid()
            .with_context(|| format!("map '{map_id}' tiles"))?;
        let base_name = def
            .base
            .scene_name()
            .with_context(|| format!("map '{map_id}' names no base scene"))?
            .to_string();
        let point = def
            .spawn(spawn)
            .with_context(|| format!("map '{map_id}' has no spawn {spawn:?}"))?
            .clone();
        // The base scene's own field layer, read before the override hides
        // it: its map (a kept geometry layer, a borrowed ground cell), its
        // floor ladder (a map that names none), and the entry holding its
        // geometry pack (a prop named by TMD coordinate).
        let base = Scene::load(&self.index, &base_name)?;
        let base_map = base.field_map_bytes(&self.index)?;
        let base_lut = base.field_floor_height_lut(&self.index)?;
        let geometry_entry =
            legaia_engine_scene::scene_bundle::find_bundle(&base).map(|b| b.entry_idx());
        let image = build_field_map_image(
            def,
            &grid,
            BaseInputs {
                base_map: base_map.as_deref(),
                geometry_entry,
            },
        )
        .with_context(|| format!("map '{map_id}' field image"))?;
        drop(base);
        let overrides = SceneOverride {
            map_id: map_id.to_string(),
            field_map: image,
            floor_lut: floor_lut(def, base_lut),
        };
        let (sx, sz) = tile_centre_world(def, point.tile[0], point.tile[1]);
        self.pending_scene_override = Some(Arc::new(overrides));
        self.set_entry_seat(sx, sz);
        self.world.arm_arrival_facing(point.facing);
        if let Err(e) = self.enter_field_scene(&base_name, 0) {
            self.pending_scene_override = None;
            self.disarm_entry_seat();
            return Err(e);
        }
        // The base scene has no MAN under the override, so the entry fell
        // back to running its event-script record 0 as field bytecode; a pack
        // map runs no retail script at all.
        self.world.load_field_script(Vec::new());
        // No retail encounters: the map's encounter zones are the pack's
        // (`ActiveMap::encounter_table`).
        self.world.encounters.session = None;
        self.world.refresh_encounter_rollable();
        // The seat: the spawn tile's centre on its floor, facing the spawn's
        // sector.
        let y = self
            .world
            .sample_field_floor_height(i32::from(sx), i32::from(sz));
        if let Some(player) = self.world.actors.first_mut() {
            player.move_state.world_x = sx;
            player.move_state.world_z = sz;
            player.move_state.world_y = y.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16;
        }
        self.world.face_player_sector(point.facing);
        self.world.recentre_field_window_on_player();
        if let Some(DiscRef::Bgm { id }) = def.bgm {
            self.world.pending_field_events.push(
                legaia_engine_field::field_events::FieldEvent::Bgm {
                    text_id: GLOBAL_BGM_BASE + id,
                    sub_op: 1,
                },
            );
        }
        self.world.expansion.map = Some(ActiveMap::new(
            map_id,
            Arc::new(def.clone()),
            Arc::new(grid),
            (point.tile[0], point.tile[1]),
        ));
        log::info!(
            "expansion: entered map '{map_id}' over '{base_name}' at spawn {:?} {:?}",
            point.id,
            point.tile
        );
        Ok(())
    }

    /// One tick of the active pack map: feed the player's tile to the map
    /// runtime and take the exit it reports. `None` when no map is active or
    /// nothing left the map this tick.
    ///
    /// A retail exit with an arrival tile rides the named scene change
    /// (`World::pending_named_scene_transition`), so it parks behind the
    /// streaming hold and arrives exactly as a door's `0x3F` does. One with
    /// no tile, and a map-to-map exit, enter at once.
    pub(crate) fn step_expansion_map(&mut self) -> Result<Option<SceneTickEvent>> {
        if self.world.mode != legaia_engine_mode::mode::SceneMode::Field
            || self.world.scene_transition_hold.is_some()
        {
            return Ok(None);
        }
        let Some(active) = self.world.expansion.map.as_mut() else {
            return Ok(None);
        };
        let Some(player) = self.world.actors.first() else {
            return Ok(None);
        };
        let tile = world_to_map_tile(
            &active.def,
            &active.grid,
            i32::from(player.move_state.world_x),
            i32::from(player.move_state.world_z),
        );
        let Some(exit) = active.step(tile) else {
            return Ok(None);
        };
        log::info!("expansion: map '{}' exit -> {exit:?}", active.map_id);
        match exit {
            ExitTarget::Retail {
                scene: DiscRef::Scene { name },
                tile: Some([tx, tz]),
                facing,
            } => {
                self.world.pending_named_scene_transition = Some((name, tx, tz, facing));
                Ok(None)
            }
            ExitTarget::Retail {
                scene: DiscRef::Scene { name },
                tile: None,
                ..
            } => {
                self.arm_picker_entry_seat(&name);
                self.enter_field_scene(&name, 0)?;
                Ok(Some(SceneTickEvent::SceneEntered { name }))
            }
            ExitTarget::Retail { scene, .. } => {
                log::warn!("expansion: exit target {scene:?} is not a scene; ignored");
                Ok(None)
            }
            ExitTarget::Map { map, spawn } => {
                self.enter_expansion_map(&map, &spawn)?;
                let name = self
                    .scene
                    .as_ref()
                    .map(|s| s.name.clone())
                    .unwrap_or_default();
                Ok(Some(SceneTickEvent::SceneEntered { name }))
            }
        }
    }
}
