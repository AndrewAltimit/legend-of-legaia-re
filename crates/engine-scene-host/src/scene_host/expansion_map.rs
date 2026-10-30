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
//! tile every tick, an exit it reports becomes a scene change, and each
//! trigger it fires goes to the pack's battle, dialog or quest runtime. Its
//! encounter zones key the pack's encounter roll on the world side
//! (`World::expansion_encounter_key`).

use super::*;

use legaia_engine_expansion::map::{ActiveMap, FiredTrigger};
use legaia_engine_expansion::quest::QuestEvent;
use legaia_engine_scene::expansion_map::detail::{needs_library, resolve_mesh_library};
use legaia_engine_scene::expansion_map::{
    BaseInputs, SceneOverride, build_field_image, floor_lut, prop_records, prop_scales,
    resolve_map_npcs, tile_centre_world,
};
use legaia_expansion_format::ExpansionPack;
use legaia_expansion_format::map::{ExitTarget, MapEvent};
use legaia_expansion_format::refs::DiscRef;

/// The global BGM id of sound-test track `n`: the global pool is the
/// `music_01` bank in sound-test order, addressed from `2000`
/// (`docs/subsystems/script-vm.md`, "BGM lookup").
const GLOBAL_BGM_BASE: u16 = 2000;

impl SceneHost {
    /// Make `pack` the world's active expansion pack through the one
    /// activation path, [`legaia_engine_core::world::World::activate_expansion`]
    /// (its battle tables, formations and XP curve go in with it). Its maps
    /// can then be entered with [`Self::enter_expansion_map`]. Fails with the
    /// pack's own problem list, leaving no pack active.
    pub fn activate_expansion_pack(&mut self, pack: Arc<ExpansionPack>) -> Result<()> {
        self.world.activate_expansion(pack).map_err(|problems| {
            anyhow::anyhow!(
                "the expansion pack does not activate: {}",
                problems.join("; ")
            )
        })
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
        self.enter_expansion_map_seated(map_id, spawn, None)
    }

    /// [`Self::enter_expansion_map`] with the player seated at world
    /// `(x, z)` facing the 12-bit angle `facing` instead of at the spawn -
    /// a save made on the map resuming where it was made. The spawn's tile
    /// still stands as the last tile seen, so no zone fires on the seat.
    pub fn enter_expansion_map_seated(
        &mut self,
        map_id: &str,
        spawn: &str,
        seat: Option<(i16, i16, u16)>,
    ) -> Result<()> {
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
        // The geometry pack as the field load registers it: where generated
        // wall boxes join it.
        let geometry = legaia_engine_scene::scene_resources::field_geometry_pack(&base);
        // Meshes from other scenes, model assets and decorations resolve
        // against the disc and the pack's assets before the image is built.
        let library = if needs_library(def, geometry.map(|(e, _)| e)) {
            Some(
                resolve_mesh_library(
                    &self.index,
                    def,
                    &pack.assets,
                    &base,
                    geometry.map(|(e, _)| e),
                )
                .with_context(|| format!("map '{map_id}' meshes"))?,
            )
        } else {
            None
        };
        let image = build_field_image(
            def,
            &grid,
            BaseInputs {
                base_map: base_map.as_deref(),
                geometry_entry,
                geometry_len: geometry.map(|(_, n)| n),
                library: library.as_ref(),
            },
        )
        .with_context(|| format!("map '{map_id}' field image"))?;
        drop(base);
        if image.dropped_decorations > 0 {
            log::warn!(
                "expansion: map '{map_id}': {} decorations on placed props' tiles left out",
                image.dropped_decorations
            );
        }
        if !image.covered_cells.is_empty() {
            log::warn!(
                "expansion: map '{map_id}': props replace the cell mesh of tiles {:?}",
                image.covered_cells
            );
        }
        if image.decoration_chunk > 1 {
            log::info!(
                "expansion: map '{map_id}' bakes its decorations in {0} x {0} tile cells",
                image.decoration_chunk
            );
        }
        let npcs = resolve_map_npcs(&self.index, def, &base_name)
            .with_context(|| format!("map '{map_id}' NPCs"))?;
        let overrides = SceneOverride {
            map_id: map_id.to_string(),
            field_map: image.image,
            floor_lut: floor_lut(def, base_lut),
            prop_scales: prop_scales(def),
            npcs,
            added_tmds: image.added_tmds,
            geometry_entry: geometry.map(|(e, _)| e),
            vram_patches: library.map(|l| l.vram_patches).unwrap_or_default(),
            prop_records: prop_records(def),
        };
        let (sx, sz) = match seat {
            Some((x, z, _)) => (x, z),
            None => tile_centre_world(def, point.tile[0], point.tile[1]),
        };
        let seat_tile = def
            .tile_at_world(&grid, i32::from(sx), i32::from(sz))
            .unwrap_or((point.tile[0], point.tile[1]));
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
        // The map's NPCs: the placements the entry seated on the scene,
        // standing on their tiles (`World::install_expansion_npcs`).
        let npcs = self
            .scene
            .as_ref()
            .and_then(|s| s.overrides.as_ref())
            .map(|o| o.npcs.clone())
            .unwrap_or_default();
        let facings: Vec<u8> = def.npcs.iter().map(|n| n.facing).collect();
        self.world.install_expansion_npcs(&npcs, &facings);
        if let Some((_, _, facing)) = seat
            && let Some(player) = self.world.actors.first_mut()
        {
            player.move_state.render_26 = (facing & 0x0FFF) as i16;
        }
        self.world.recentre_field_window_on_player();
        // The place-name banner: the loader's balloon, carrying the map's
        // display name where a retail scene's MAN carries its own.
        if let Some(banner) = crate::place_name_banner::man_load_banner(def.name.as_bytes(), true) {
            self.world.cutscene.text_balloon = Some(banner);
        }
        if let Some(DiscRef::Bgm { id }) = def.bgm {
            self.world.pending_field_events.push(
                legaia_engine_field::field_events::FieldEvent::Bgm {
                    text_id: GLOBAL_BGM_BASE + id,
                    sub_op: 1,
                },
            );
        }
        // The pack's saved flags answer which `once` triggers have fired.
        self.world.expansion.quests.sync_pack(&pack);
        let flags = &self.world.expansion.quests.progress.flags;
        let active = ActiveMap::new(
            map_id,
            Arc::new(def.clone()),
            Arc::new(grid),
            seat_tile,
            |f| flags.is_set(f),
        );
        self.world.expansion.map = Some(active);
        // The map's fog pool and its gate (no-op without `[fog]`).
        self.world.arm_expansion_fog();
        // NPCs whose `present` gate is shut are off the map this visit.
        self.world.expansion_hide_absent_npcs();
        // Quests see the map as the scene entered.
        self.world
            .expansion_notify(QuestEvent::SceneEntered(map_id.to_string()));
        // The first arrival cutscene whose gate holds and that is unseen (or
        // repeats) plays once the field is free.
        if seat.is_none()
            && let Some(c) = def.enter_cutscenes.iter().find(|e| {
                (e.repeat
                    || !self
                        .world
                        .expansion
                        .quests
                        .progress
                        .flags
                        .is_set(&legaia_expansion_format::cutscene::seen_flag(&e.cutscene)))
                    && self.world.expansion_prerequisites_hold(&e.requires)
            })
        {
            self.world.expansion_play_cutscene(&c.cutscene);
        }
        log::info!(
            "expansion: entered map '{map_id}' over '{base_name}' at spawn {:?} {:?}",
            point.id,
            point.tile
        );
        Ok(())
    }

    /// Enter the pack map the running mode instance is played on
    /// (`World::pack_mode_wants_map`) without closing the mode seat: the
    /// seat is lifted off the world across the map entry, put back with the
    /// world in `SceneMode::Expansion`, and set to return to the field of
    /// that map. A map that fails to enter is logged and not tried again,
    /// and the mode keeps drawing as its top-down screen.
    ///
    /// Run from [`SceneHost::tick`] after the world stepped, so a seat that
    /// `World::enter_pack_mode` or the picker opened this frame is on its map
    /// by the next.
    pub fn sync_pack_mode_map(&mut self) -> Option<SceneTickEvent> {
        let (map, spawn) = self.world.pack_mode_wants_map()?;
        if self.failed_pack_mode_map.as_deref() == Some(map.as_str()) {
            return None;
        }
        let seat = self.world.expansion.mode_seat.take();
        let entered = self.enter_expansion_map(&map, &spawn);
        self.world.expansion.mode_seat = seat;
        self.world.mode = legaia_engine_mode::mode::SceneMode::Expansion;
        if let Err(e) = entered {
            log::warn!("expansion: mode map '{map}' did not enter: {e:#}");
            self.failed_pack_mode_map = Some(map);
            return None;
        }
        if let Some(seat) = self.world.expansion.mode_seat.as_deref_mut() {
            seat.return_mode = legaia_engine_mode::mode::SceneMode::Field;
        }
        // The seat closes onto the map's spawn, not the scene it left.
        self.world.remember_pack_mode_player();
        let name = self
            .scene
            .as_ref()
            .map(|s| s.name.clone())
            .unwrap_or_default();
        Some(SceneTickEvent::SceneEntered { name })
    }

    /// Hand a fired trigger to its runtime: a battle or boss to the pack's
    /// fight path, a line to the pack dialogue runner, a challenge to the
    /// challenge arm, a mode to the mode seat, a rest point to the party
    /// restore, the rest to the quest tracker. A `once` trigger's flag goes up whether or not its content
    /// resolved, so a broken reference does not fire on every visit.
    fn run_map_trigger(&mut self, trigger: FiredTrigger) {
        let w = &mut self.world;
        match &trigger.event {
            MapEvent::Battle { formation } => {
                if !w.start_expansion_battle(formation) {
                    log::warn!("expansion: map trigger battle `{formation}` did not start");
                }
            }
            MapEvent::Boss { boss } => {
                if !w.start_expansion_boss(boss) {
                    log::warn!("expansion: map trigger boss `{boss}` did not start");
                }
            }
            MapEvent::Dialog { line } => {
                if let Err(e) = w.expansion_say(line) {
                    log::warn!("expansion: map trigger line `{line}`: {e:?}");
                }
            }
            MapEvent::Talk { npc } => w.expansion_talked(npc),
            MapEvent::StartQuest { quest } => {
                if !w.expansion_start_quest(quest) {
                    log::info!("expansion: map trigger quest `{quest}` did not start");
                }
            }
            MapEvent::SetFlag { flag } => w.expansion_notify(QuestEvent::FlagSet(flag.clone())),
            MapEvent::Counter { counter, add } => w.expansion_notify(QuestEvent::CounterAdd {
                counter: counter.clone(),
                add: *add,
            }),
            MapEvent::CardDealer { dealer } => {
                if !w.open_card_dealer(dealer) {
                    log::warn!("expansion: map trigger card dealer `{dealer}` did not open");
                }
            }
            MapEvent::Shop { shop } => {
                if !w.open_item_shop(shop) {
                    log::warn!("expansion: map trigger shop `{shop}` did not open");
                }
            }
            MapEvent::Challenge { challenge, start } => {
                if !w.expansion_take_challenge(challenge, *start) {
                    log::info!("expansion: map trigger challenge `{challenge}` did not arm");
                }
            }
            MapEvent::Mode { mode } => {
                if let Err(e) = w.enter_pack_mode(mode) {
                    log::warn!("expansion: map trigger mode `{mode}`: {e:?}");
                }
            }
            MapEvent::Rest { cost, save } => {
                w.expansion_rest(*cost, *save);
            }
            MapEvent::Cutscene { cutscene } => {
                if !w.expansion_play_cutscene(cutscene) {
                    log::warn!("expansion: map trigger cutscene `{cutscene}` did not play");
                }
            }
        }
        if let Some(flag) = trigger.once_flag {
            self.world.expansion_notify(QuestEvent::FlagSet(flag));
        }
    }

    /// One tick of the active pack map: feed the player's tile to the map
    /// runtime, run the triggers it fired and take the exit it reports. `None` when no map is active or
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
        let Some(player) = self.world.actors.first() else {
            return Ok(None);
        };
        let (x, z) = (
            i32::from(player.move_state.world_x),
            i32::from(player.move_state.world_z),
        );
        let Some(mut active) = self.world.expansion.map.take() else {
            return Ok(None);
        };
        let tile = active.tile_at_world(x, z);
        // A cutscene owns the map while it plays: the player it walks
        // through a zone or onto an exit fires nothing, and no NPC patrols.
        if self.world.expansion_cutscene_active() {
            active.observe(tile);
            self.world.expansion.map = Some(active);
            return Ok(None);
        }
        let exit = active.step_gated(tile, |p| self.world.expansion_prerequisites_hold(p));
        self.world.expansion.map = Some(active);
        self.world.step_expansion_npc_patrols();
        let Some(active) = self.world.expansion.map.as_mut() else {
            return Ok(None);
        };
        let fired = active.take_events();
        let map_id = active.map_id.clone();
        for trigger in fired {
            self.run_map_trigger(trigger);
        }
        let Some(exit) = exit else {
            return Ok(None);
        };
        log::info!("expansion: map '{map_id}' exit -> {exit:?}");
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
