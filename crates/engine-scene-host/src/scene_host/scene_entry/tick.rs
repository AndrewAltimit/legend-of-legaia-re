//! `SceneHost::tick`: the per-frame scene step, with the scene-transition hold it advances
//! and the fog-volume light measurement it refreshes.

use super::*;

impl SceneHost {
    /// Run the scene-transition hold for one tick. `true` while a transition
    /// is parked (the caller skips every drain this tick); `false` when there
    /// is none, or on the tick the hold releases - the parked request is then
    /// back in its pending seat for the drains below to commit.
    ///
    /// A named request (`World::pending_named_scene_transition`) or an
    /// overworld-portal crossing starts a hold. Requests raised while one is
    /// parked are dropped: retail's first packet has already spawned the
    /// actor and copied the destination, and the departing scene is held.
    ///
    /// REF: FUN_8001FD44 (the packet that spawns the actor), FUN_80021934
    fn advance_scene_transition_hold(&mut self) -> bool {
        use legaia_engine_field::scene_transition_actor::{HeldTransition, SceneTransitionHold};
        if self.world.scene_transition_hold.is_none() {
            let request =
                if let Some((n, x, z, d)) = self.world.pending_named_scene_transition.take() {
                    HeldTransition::Named(n, x, z, d)
                } else if let Some((dest_index, slot)) = self.world.take_world_map_transition() {
                    HeldTransition::Portal(dest_index, slot)
                } else {
                    return false;
                };
            log::info!("scene transition parked behind the streaming actor: {request:?}");
            self.world.scene_transition_hold = Some(SceneTransitionHold::new(request));
        }
        self.world.pending_named_scene_transition = None;
        while self.world.take_world_map_transition().is_some() {}
        let dt = self
            .world
            .clock
            .display_frame_step
            .clamp(1, u16::from(u8::MAX)) as u8;
        let Some(hold) = self.world.scene_transition_hold.as_mut() else {
            return false;
        };
        if !hold.tick(dt) {
            return true;
        }
        let hold = self
            .world
            .scene_transition_hold
            .take()
            .expect("checked above");
        log::info!(
            "scene transition committed after {} ticks: {:?}",
            hold.ticks,
            hold.request
        );
        match hold.request {
            HeldTransition::Named(n, x, z, d) => {
                self.world.pending_named_scene_transition = Some((n, x, z, d));
            }
            HeldTransition::Portal(dest_index, slot) => {
                self.world.pending_field_events.push(
                    legaia_engine_field::field_events::FieldEvent::WorldMapTransition {
                        dest_index,
                        slot,
                    },
                );
            }
        }
        false
    }

    /// One frame: tick the world, materialize any actor-spawn requests
    /// queued by the field VM's `0x4C 0x80` opcode, then process any
    /// pending `scene_transition(map_id)` request. Returns the
    /// [`SceneTickEvent`] describing what happened.
    ///
    /// A transition whose resolved scene is an overworld scene
    /// ([`is_world_map_scene`]) routes through [`Self::enter_world_map_scene`]
    /// (world-map mode + region table) instead of the plain field path, so the
    /// boot/transition path seeds the overworld the same way the explicit
    /// `--world-map` entry does.
    pub fn tick(&mut self) -> Result<SceneTickEvent> {
        let was_battle = matches!(self.world.mode, legaia_engine_mode::mode::SceneMode::Battle);
        let _ = self.world.tick();
        self.measure_fog_volume_light();
        // The arena hub's screens run every tick, including the ones after a
        // leg has closed; its hand-off arms the next leg's drain below.
        self.world.tick_muscle_hub();
        if matches!(self.world.mode, legaia_engine_mode::mode::SceneMode::Battle) {
            if !was_battle || self.world.audio.battle_sfx_bank.is_none() {
                self.install_battle_sfx_bank();
            }
            self.install_battle_monster_action_clips();
            self.ensure_battle_party_forms();
        } else {
            self.battle_party_forms = None;
        }
        // Post-battle field return: retail re-enters the field scene after a
        // battle (game-mode battle -> field reload), which re-runs the
        // scene-entry system script `P1[0]` from the MAN - the "P2 timeline
        // re-scan". That re-run is how a post-battle beat record spawns:
        // rikuroa's `P1[0]` tests the battle-staged marker `0x289` and issues
        // the op-`0x44` spawn of the post-victory record `P2[50]` (C1-gated on
        // `0x142`, the self-latching one-shot), whose own script bytes SET the
        // gate flag. The engine keeps the world state in place across the
        // battle, so the re-entry's script side is reproduced by reloading the
        // entry script here.
        // REF: FUN_8003ab2c (system-script rebuild on field entry)
        if was_battle && matches!(self.world.mode, legaia_engine_mode::mode::SceneMode::Field) {
            let entry_script = self
                .scene
                .as_ref()
                .and_then(|scene| scene.field_man_entry_script(&self.index).ok().flatten());
            if let Some((bytecode, pc0)) = entry_script {
                self.world.load_field_script_at(bytecode, pc0);
                log::info!("field: post-battle return re-ran the scene-entry script");
            }
        }
        self.world
            .materialize_actor_spawns(legaia_engine_core::world::FIELD_SPAWN_START_SLOT);
        // Field-VM op-0x44 SPAWN_RECORD: resolve each queued request against
        // the current scene MAN and install the partition-2 record as a
        // spawned context (retail FUN_8003BDE0 runs every spawned record as
        // an independent field-VM context). Cutscene-class spawns - the
        // opening chain's opstati / opurud entry scripts launching their
        // prologue records - install as THE modal cutscene timeline (cutscene
        // camera + locomotion lock + the chain's beat sequencing); an
        // ordinary scene's mid-play helper spawn installs as a concurrent
        // helper context that leaves the camera alone but, like the timeline,
        // holds the pad while it runs (`World::script_context_engages_player`).
        let pending_spawns = std::mem::take(&mut self.world.field_vm.pending_record_spawns);
        if !pending_spawns.is_empty()
            && let Some(Ok(Some(man_bytes))) = self
                .scene
                .as_ref()
                .map(|s| s.field_man_payload(&self.index))
            && let Ok(man_file) = legaia_scene_formats::man_section::parse(&man_bytes)
        {
            for global_index in pending_spawns {
                if self.world.cutscene.opening_chain_active {
                    // Opening-chain sequencing: one modal beat at a time. A
                    // request issued while a beat still plays is dropped -
                    // the retail opening never issues one - preserving the
                    // chain's install-at-scene-entry cadence.
                    if !self.world.cutscene_timeline_active()
                        && self
                            .world
                            .install_spawned_record(&man_file, &man_bytes, global_index)
                    {
                        log::info!(
                            "opening: op-0x44 spawned P2 record (global index {global_index}) as the cutscene timeline",
                        );
                    }
                } else if self.world.install_spawned_helper_record(
                    &man_file,
                    &man_bytes,
                    global_index,
                ) {
                    log::info!(
                        "field: op-0x44 spawned P2 record (global index {global_index}) as a concurrent helper context",
                    );
                }
            }
        }
        // Walk-on tile trigger: the per-frame tile compare that spawns a
        // gate-1 trigger's partition-2 record when the player crosses onto
        // its tile - town exits + walk-on story beats. Runs after the world
        // stepped (player position is current) and before the transition
        // drains (a record installed this frame steps from the next tick).
        // The scene-change packet does not load anything: it spawns the
        // streaming actor `FUN_80021934` and returns, and the departing scene
        // keeps running under the actor's `0x46`-frame countdown. Park every
        // transition behind that hold and commit it on the hand-off tick.
        if self.advance_scene_transition_hold() {
            return Ok(SceneTickEvent::Stepped);
        }
        self.dispatch_walk_on_trigger();
        // Named scene-change (field-VM op 0x3F) takes precedence over the
        // map-id door-warp: its destination name is carried inline by the op,
        // so it loads directly without the map-id resolver. This is the live
        // consumer of the disc-sourced scene-destination data - the same names
        // [`legaia_engine_field::man_field_scripts::scene_destinations`] catalogs.
        if let Some((name, entry_x, entry_z, dir)) =
            self.world.pending_named_scene_transition.take()
        {
            // Drop a stale map-id request from the same frame; the named target
            // is unambiguous.
            self.world.pending_scene_transition = None;
            // The op's entry tile IS retail's destination-entry operand, so it
            // is armed *before* the initialiser rather than applied after it:
            // the seat then reaches every scene-entry consumer (camera window,
            // region refresh, arrival trigger) instead of arriving one step
            // late, and the picker's cold-spawn relocation stands down for a
            // scene somebody actually walked into.
            self.world.arm_arrival_facing(dir);
            self.set_entry_seat_tile(entry_x, entry_z);
            if is_world_map_scene(&name) {
                self.enter_world_map_scene(&name)?;
            } else {
                self.enter_field_scene(&name, 0)?;
            }
            // Re-seat through the wall-aware tile helper: the operand above put
            // the player there already, and this adds the floor-height snap and
            // the bounded rescue a door tile that reads as wall needs.
            self.world.seat_player_at_tile(entry_x, entry_z);
            // ...facing the op's trailing `dir` compass sector (retail
            // resolves it through the SCUS 0x80073F04 table into the
            // arrival-facing global; the engine sets the heading directly).
            self.world.face_player_sector(dir);
            // Walk-on tile trigger at the arrival tile: retail's per-frame
            // tile compare fires immediately on the first post-arrival frame
            // (the last-tile globals are stale from the previous scene), so
            // an arrival onto a gate-1 kind-1 trigger spawns its partition-2
            // record - this is how the opening chain's `map01` fly-in and the
            // natural `town01` opening launch.
            self.spawn_arrival_trigger_record(entry_x, entry_z);
            return Ok(SceneTickEvent::SceneEntered { name });
        }
        // GAP-1: overworld-portal scene transition. The world-map entity SM
        // emits [`legaia_engine_field::field_events::FieldEvent::WorldMapTransition`] when the
        // player walks onto a portal tile
        // ([`legaia_engine_core::world::World::auto_engage_world_map_portals`] ->
        // `on_scene_transition`); without a consumer it was dropped to the
        // BGM-router `leftover`. Drain it here (the overworld->dungeon hop) and
        // load the destination.
        //
        // The `slot` points back at the engaged entity's config, and only an
        // [`legaia_engine_field::world_map_entity_config::WorldMapEntityConfig::OverworldPortal`] raises the
        // event: it carries the exact CDNAME destination + arrival tile (the
        // `map01` `0x3F` bridge), so it loads that scene and seats the player
        // at the entry tile - the same arrival semantics as the named `0x3F`
        // warp above. `dest_index` is that entrance's `0x3F` index and is
        // **not** resolved as a map id; the door-warp id space belongs to
        // [`legaia_engine_field::world_map_entity_config::WorldMapEntityConfig::MinigameDoor`], which arms the
        // minigame warp drained below instead of raising this event.
        if let Some((dest_index, slot)) = self.world.take_world_map_transition() {
            self.world.pending_scene_transition = None;
            if let Some(
                legaia_engine_field::world_map_entity_config::WorldMapEntityConfig::OverworldPortal {
                    scene_name,
                    entry_x,
                    entry_z,
                    dir,
                    record,
                    object,
                    ..
                },
            ) = self.world.world_map.entity_configs.get(slot as usize)
            {
                let name = scene_name.clone();
                let (entry_x, entry_z, dir, record, object) =
                    (*entry_x, *entry_z, *dir, *record, *object);
                // Retail runs the entrance's partition-2 record on the
                // crossing; the entity SM here keeps only its `0x3F`
                // destination. The flag operations that open the record run
                // before anything else in it, so replaying them leaves the
                // bank as the field VM would - and one of them is system
                // flag 2, which the destination's MAN loader turns into the
                // place-name banner.
                if let Some(man) = self.field_man_cache.clone()
                    && let Ok(mf) = legaia_scene_formats::man_section::parse(&man)
                {
                    let writes = if object {
                        legaia_engine_field::man_field_scripts::flat_record_path_walk(
                            &mf,
                            &man,
                            usize::from(record),
                            &|idx| self.world.system_flag_test(idx),
                        )
                        .map(|(_, w)| w)
                        .unwrap_or_default()
                    } else {
                        crate::place_name_banner::record_leading_flag_writes(
                            &mf,
                            &man,
                            usize::from(record),
                            |idx| self.world.system_flag_test(idx),
                        )
                    };
                    for (set, idx) in writes {
                        if set {
                            self.world.system_flag_set(idx);
                        } else {
                            self.world.system_flag_clear(idx);
                        }
                    }
                }
                self.world.arm_arrival_facing(dir);
                self.set_entry_seat_tile(entry_x, entry_z);
                if is_world_map_scene(&name) {
                    self.enter_world_map_scene(&name)?;
                } else {
                    self.enter_field_scene(&name, 0)?;
                }
                self.world.seat_player_at_tile(entry_x, entry_z);
                self.world.face_player_sector(dir);
                self.spawn_arrival_trigger_record(entry_x, entry_z);
                return Ok(SceneTickEvent::SceneEntered { name });
            }
            // The config row is gone (a host installed the event by hand, or
            // the entity set was replaced mid-tick). There is no id space left
            // to resolve `dest_index` against, so drop it rather than route it
            // through a resolver it does not belong to.
            let _ = dest_index;
        }
        // The mode-24 minigame door-warp drains ahead of the map-id
        // transition: it is the op-`0x3E` `op0 >= 100` arm, and its id is a
        // minigame sub-id, not a map id (see `legaia_engine_mode::minigame_entry`).
        if let Some(outcome) = self.drain_minigame_warp() {
            self.last_minigame_warp = Some(outcome);
            return Ok(SceneTickEvent::Stepped);
        }
        if let Some(map_id) = self.world.pending_scene_transition.take() {
            match self.map_resolver.resolve(map_id) {
                Some(name) => {
                    if is_world_map_scene(&name) {
                        self.enter_world_map_scene(&name)?;
                    } else {
                        self.enter_field_scene(&name, 0)?;
                    }
                    return Ok(SceneTickEvent::SceneEntered { name });
                }
                None => {
                    return Ok(SceneTickEvent::UnknownMapId { map_id });
                }
            }
        }
        Ok(SceneTickEvent::Stepped)
    }

    /// The world-side frame tail a headless driver owes after each
    /// [`Self::tick`]: seat a requested summon creature unrendered, run
    /// [`legaia_engine_core::world::World::step_world_frame_tail`] with no clip banks, and
    /// step [`legaia_engine_core::world::World::step_field_vram_effects`] over the scene's
    /// own VRAM image. `engine-session`'s `BootSession` runs exactly this on
    /// every tick it does not hand to a play host.
    ///
    /// The tail is not presentation-only. The VRAM step is where the ambient
    /// move-VM parts tick (`World::step_ambient_fx`), and a part staged by
    /// op `0x34` sub-3 is what flips the system flag a `4C C5` / `4C C6` wait
    /// holds on (`2F 1C` / `2F 1D`): `garmel` P2[12] stages record 2 with
    /// `B4 1D 30 01` and parks on `4C C6 0A 00` until that part clears
    /// `0x0A`. A driver that ticks the scene without this tail parks there
    /// for good. What the tail hands back for drawing is returned; a headless
    /// caller drops it.
    pub fn step_headless_frame_tail(&mut self) -> legaia_engine_core::world::WorldFrameTail {
        self.world.seat_summon_creature_unrendered();
        let tail = self.world.step_world_frame_tail(None, None, |_| None);
        if let Some(res) = self.resources.as_mut() {
            let _ = self.world.step_field_vram_effects(&mut res.vram, false);
        }
        tail
    }

    /// Measure the field scene's luminance for the volumetric ground fog
    /// (`legaia_render_kernels::fog_volume::scene_luminance` over every textured mesh the
    /// scene's resources carry), once per scene label and only while the
    /// enhancement is on. Both play hosts tick through here, so both draw
    /// the bank at the same brightness.
    fn measure_fog_volume_light(&mut self) {
        if !self.world.toggles.volumetric_fog
            || self.world.mode != legaia_engine_mode::mode::SceneMode::Field
            || self
                .world
                .fog_volume
                .field_luma
                .as_ref()
                .is_some_and(|(l, _)| *l == self.world.active_scene_label)
        {
            return;
        }
        let Some(res) = self.resources.as_ref() else {
            return;
        };
        let meshes: Vec<legaia_tmd::mesh::VramMesh> = res
            .tmds
            .iter()
            .map(|t| legaia_tmd::mesh::tmd_to_vram_mesh(&t.tmd, &t.raw))
            .collect();
        if let Some(l) =
            legaia_render_kernels::fog_volume::scene_luminance(&res.vram, meshes.iter())
        {
            self.world.fog_volume.field_luma = Some((self.world.active_scene_label.clone(), l));
        }
    }
}
