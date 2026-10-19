//! Binding the scene's actors to the uploaded meshes: the registered scene
//! actors' idle clips, the player's rig, and the MAN-placed field NPCs and
//! animated props.

use super::*;

/// What one field NPC upload reads besides its placement.
pub(super) struct NpcUploadSources<'a> {
    pub(super) res: &'a SceneResources,
    pub(super) scene_bundle: &'a Option<legaia_actor_formats::player_anm::PlayerAnmBundle>,
    pub(super) locomotion_bundle: &'a Option<legaia_actor_formats::player_anm::PlayerAnmBundle>,
    /// Scripted mesh re-binds (motion-VM op `0x0E`), by placement slot.
    pub(super) live_npc_meshes: &'a std::collections::BTreeMap<u8, (legaia_tmd::Tmd, Vec<u8>)>,
}

impl PlayWindowApp {
    /// Scripted mesh re-binds (motion-VM op `0x0E`): each live override's
    /// bytes. Empty until a script swaps a model, which is every scene until
    /// one does.
    pub(super) fn live_npc_mesh_overrides(
        &self,
    ) -> std::collections::BTreeMap<u8, (legaia_tmd::Tmd, Vec<u8>)> {
        self.session
            .host
            .world
            .npcs
            .models
            .iter()
            .filter_map(|(&slot, &id)| self.live_npc_mesh(id).map(|m| (slot, m)))
            .collect()
    }

    /// Bind the entered scene's actors to the world through the step both
    /// hosts run (`scene_actors::bind_scene_actors`) and keep what the draw
    /// half reads: the two ANM banks, the clip players and the clip targets
    /// the per-tick cue drain re-targets. The posed-mesh memo is keyed by
    /// `(slot, clip frame)`; the new scene reuses both, so it goes too.
    pub(super) fn adopt_scene_actor_binding(&mut self) {
        let b = legaia_engine_session::scene_actors::bind_scene_actors(&mut self.session.host);
        self.npc_anim_bundles = (b.scene_anm, b.locomotion_anm);
        self.npc_clip_players = b.clip_players.into_iter().collect();
        self.npc_bundle_special = b.clip_targets.into_iter().collect();
        self.npc_pose_cache.clear();
        self.npc_pose_verify.clear();
    }

    /// Upload the PLAYER's real character mesh. Town scripts install the
    /// player engine-side (`install_field_player`), not via the field-VM
    /// `0x4C 0xD8` spawn, so no `tmd_ref` spawn event ever fires for it -
    /// and the scene pre-bind points actor 0 at whatever scene TMD shares
    /// its slot index (usually an init_data UI mesh = invisible player).
    /// Resolve the lead's field mesh from the global TMD pool (PROT 0874 §0,
    /// retail `DAT_8007C018[0..4]`, seeded by `enter_field_scene`) and
    /// re-point the lead's slot - bound by `scene_actors::bind_lead_rig`,
    /// with its locomotion clips - at the uploaded mesh.
    pub(super) fn upload_player_rig(
        &mut self,
        player_rig: Option<legaia_engine_scene_host::scene_host::PlayerRigMesh>,
    ) {
        let world = &mut self.session.host.world;
        self.player_color_draw = None;
        // World-map mode binds the same lead mesh: `enter_world_map_scene`
        // runs `enter_field_scene` first, so the global TMD pool is seeded
        // identically, and retail draws the party leader's field figure on
        // the overworld. Gating this to `Field` alone left the overworld
        // player invisible (the world-map draw branch had nothing bound to
        // draw).
        if matches!(world.mode, SceneMode::Field | SceneMode::WorldMap)
            && let Some(pslot) = world.player_actor_slot
        {
            let roster_lead = usize::from(world.field_leader());
            if let Some(g) = player_rig {
                // The party locomotion ANM bundle (PROT 0874 §1; idle = bank
                // slot 1, walk = bank slot 0, both pinned live - see
                // `character_pack::LOCOMOTION_IDLE_SLOT` / `_WALK_SLOT`).
                // Banks cover the Vahn/Noa/Gala trio only.
                let locomotion_bank = self
                    .session
                    .host
                    .index
                    .entry_bytes(legaia_actor_formats::character_pack::PROT_ENTRY_INDEX)
                    .ok()
                    .and_then(|b| {
                        legaia_actor_formats::character_pack::field_locomotion_anm(&b).ok()
                    });
                // The rest pose and the bone cap come from the MESH's slot: a
                // party-slot mesh (the lead's form, or a `CC F8 50` operand
                // `>= 0xF0`) poses from its own bank, a scene-bank model has
                // none. The clip player stays the roster lead's - its settle
                // pick binds scene records itself while the party-bank bit is
                // down (`FieldPlayerAnim::select_scene_record`).
                let lead = g.party_slot.unwrap_or(usize::MAX);
                let locomotion = locomotion_bank.clone().filter(|_| lead <= 2);
                // Rest pose: frame 0 of the character's standing-idle clip.
                // Retail caps the live object count to the clip's bone count
                // (10; groups 10/11 are equipment-swap templates, never drawn
                // - `FUN_8001E890` / `FUN_80024d78`), so truncate the disc
                // TMD's 12-object table to match before posing bone i ->
                // object i.
                // (bone_count, per-bone (translation, rotation) pairs)
                type IdlePose = (usize, Vec<([i16; 3], [i16; 3])>);
                let idle_pose: Option<IdlePose> = locomotion.as_ref().and_then(|bundle| {
                    let rec_idx = legaia_actor_formats::character_pack::locomotion_record_index(
                        lead,
                        legaia_actor_formats::character_pack::LOCOMOTION_IDLE_SLOT,
                    );
                    let rec = bundle.record(rec_idx).ok()?;
                    let bones = rec.bone_count as usize;
                    let offsets: Vec<([i16; 3], [i16; 3])> = (0..bones)
                        .map(|bidx| match bundle.bone_transform(rec_idx, 0, bidx) {
                            Some(t) => (
                                [t.t_x as i16, t.t_y as i16, t.t_z as i16],
                                [t.r_x as i16, t.r_y as i16, t.r_z as i16],
                            ),
                            None => ([0; 3], [0; 3]),
                        })
                        .collect();
                    Some((bones, offsets))
                });
                // The player's working TMD: object table truncated to the
                // clip's bone count. This is also the copy pushed into
                // `scene_tmd_data`, so the per-frame posed rebuild (driven by
                // the actor's live `pose_frame`) poses the same 10 objects.
                let player_tmd = match &idle_pose {
                    Some((bones, _)) => {
                        let mut t = g.tmd.clone();
                        t.objects.truncate(*bones);
                        t
                    }
                    None => g.tmd.clone(),
                };
                let vmesh = match &idle_pose {
                    Some((_, offsets)) => {
                        legaia_tmd::mesh::tmd_to_vram_mesh_posed_rot(&player_tmd, &g.raw, offsets)
                    }
                    None => legaia_tmd::mesh::tmd_to_vram_mesh(&player_tmd, &g.raw),
                };
                if !vmesh.indices.is_empty()
                    && let Some(r) = self.win.renderer.as_ref()
                {
                    match r.upload_vram_mesh(
                        &vmesh.positions,
                        &vmesh.uvs,
                        &vmesh.cba_tsb,
                        &vmesh.normals,
                        &vmesh.colors,
                        &vmesh.indices,
                    ) {
                        Ok(m) => {
                            let new_idx = self.meshes.len();
                            self.meshes.push(m);
                            self.scene_tmd_data
                                .push((player_tmd.clone(), g.raw.clone()));
                            // Re-point, never bind: whether the slot is
                            // bound is the world's (`bind_lead_rig`).
                            if world.actors[pslot as usize].tmd_binding.is_some() {
                                world.set_actor_tmd_binding(pslot as usize, new_idx);
                            }
                            self.drained_spawn_slots.insert(pslot);
                            // The untextured half (pants / sleeves are F*/G*
                            // colour prims the VRAM mesh can't carry) rides
                            // the colour pipeline, posed with the same bone
                            // set, drawn per-frame at the actor's live model.
                            if let Some((_, offsets)) = &idle_pose {
                                let cmesh = legaia_tmd::mesh::tmd_to_color_mesh_posed_rot(
                                    &player_tmd,
                                    &g.raw,
                                    offsets,
                                );
                                if !cmesh.is_empty() {
                                    match r.upload_color_mesh_blended(
                                        &cmesh.positions,
                                        &cmesh.colors,
                                        &cmesh.indices,
                                        &cmesh.blend,
                                    ) {
                                        Ok(cm) => {
                                            let cidx = self.color_meshes.len();
                                            self.color_meshes.push(cm);
                                            self.player_color_draw = Some((cidx, pslot as usize));
                                        }
                                        Err(e) => log::warn!(
                                            "play-window: player colour half upload: {e:#}"
                                        ),
                                    }
                                }
                            }
                            let animated = world.locomotion.player_anim.is_some();
                            log::info!(
                                "play-window: player (roster {roster_lead}, model {:?}) -> pool \
                                 mesh slot {new_idx} ({}{}{})",
                                g.model_id,
                                if idle_pose.is_some() {
                                    "rest-posed from locomotion idle frame 0"
                                } else {
                                    "unposed: locomotion bundle unavailable"
                                },
                                if self.player_color_draw.is_some() {
                                    ", + colour half"
                                } else {
                                    ""
                                },
                                if animated {
                                    ", + idle/walk playback"
                                } else {
                                    ""
                                }
                            );
                        }
                        Err(e) => log::warn!("play-window: player mesh upload: {e:#}"),
                    }
                }
            } else {
                log::warn!(
                    "play-window: no rig mesh for roster slot {roster_lead} (model {:?}); \
                     player keeps the placeholder binding",
                    world.locomotion.player_live_model
                );
            }
        }
    }

    /// Field NPCs + animated props: one draw per visible MAN partition-1
    /// placement (see `FieldNpcDraw` for the runtime-pinned model/anim
    /// resolution). Returns the props' light sets.
    pub(super) fn upload_field_npcs(
        &mut self,
        res: &SceneResources,
        scene_bundle: Option<legaia_actor_formats::player_anm::PlayerAnmBundle>,
        live_npc_meshes: &std::collections::BTreeMap<u8, (legaia_tmd::Tmd, Vec<u8>)>,
    ) -> Vec<legaia_render_kernels::scene_lighting::PropLights> {
        let mut prop_light_sets: Vec<legaia_render_kernels::scene_lighting::PropLights> =
            Vec::new();
        // The clip players, the banks and the clip targets are the scene's
        // binding (`adopt_scene_actor_binding`), not this upload's: a
        // mid-scene re-upload keeps every playhead where it stands.
        self.field_npc_draws.clear();
        self.npc_anim_srcs.clear();
        self.npc_rest_srcs.clear();
        self.npc_morph_static.clear();
        // The posed-mesh memo is keyed by `(slot, clip frame)` over the
        // uploaded meshes, which this upload replaces.
        self.npc_pose_cache.clear();
        self.npc_pose_verify.clear();
        if self.session.host.world.mode == SceneMode::Field && self.win.renderer.is_some() {
            // The per-scene ANM bundle (see `find_scene_anm_bundle`): field
            // builds don't surface it through `res.anm_packs`, and the prologue
            // scenes' vignette actors need it or they render as a FROZEN tableau
            // under the narration crawl. Resolved once at the top of this fn -
            // the placed static objects pose from the same bundle.
            let locomotion_bundle = self.npc_anim_bundles.1.clone();
            let placements = self
                .session
                .host
                .scene
                .as_ref()
                .and_then(|s| s.field_man_payload(&self.session.host.index).ok().flatten())
                .and_then(|man| {
                    legaia_scene_formats::man_section::parse(&man)
                        .ok()
                        .map(|mf| {
                            legaia_engine_field::man_field_scripts::classify_placements(&mf, &man)
                        })
                })
                .unwrap_or_default();
            let sources = NpcUploadSources {
                res,
                scene_bundle: &scene_bundle,
                locomotion_bundle: &locomotion_bundle,
                live_npc_meshes,
            };
            for (p, _kind) in &placements {
                self.upload_field_npc(p, &sources, &mut prop_light_sets);
            }
            if !self.field_npc_draws.is_empty() {
                log::info!(
                    "play-window: {} field NPC/prop draws ({} placements)",
                    self.field_npc_draws.len(),
                    placements.len()
                );
            }
        }
        prop_light_sets
    }

    /// Upload one placement's mesh halves, bind its clip, derive its light
    /// set and record its draw.
    fn upload_field_npc(
        &mut self,
        p: &legaia_scene_formats::man_section::ActorPlacement,
        sources: &NpcUploadSources<'_>,
        prop_light_sets: &mut Vec<legaia_render_kernels::scene_lighting::PropLights>,
    ) {
        let NpcUploadSources {
            res,
            scene_bundle,
            locomotion_bundle,
            live_npc_meshes,
        } = *sources;
        let Some(r) = self.win.renderer.as_ref() else {
            return;
        };
        let world = &self.session.host.world;
        // EVERY placement gets its mesh uploaded, including the ones
        // parked at the off-map hide box: the scene-entry
        // spawn-prologue pre-run can seat a parked placement INTO the
        // town per story state (retail's `FUN_8003A1E4` install runs
        // the record's story-flag-tested `MoveTo`s), and a mid-scene
        // walk-on timeline can seat one long after `upload_assets`
        // ran (the town01 Mei beat's `4C 51` / `C7` pokes). The draw
        // pass skips any NPC whose LIVE position is the hide box, so
        // parked actors stay invisible until a script places them -
        // filtering them out HERE instead left a timeline-seated NPC
        // permanently meshless (invisible while the world said it
        // stood on stage).
        // A scripted mesh re-bind (motion-VM op `0x0E`) wins over the
        // placement's spawn model. The bytes come from the scene's
        // model bank rather than `res.tmds`: that list is a magic
        // scan over the raw entries and cannot see a TMD inside an
        // LZS bundle descriptor, which is where the scenes that
        // script a re-bind keep their models.
        let live_model = world.field_npc_live_model(p.index as u8);
        let live_src = live_npc_meshes.get(&(p.index as u8)).cloned();
        let bound_model = live_model.unwrap_or(i16::from(p.model_index));
        let src = if live_src.is_some() {
            live_src
        } else if p.special_model {
            // The player bank (PROT 0874 §0), not the shared pool the
            // effect-model library overlays - `World::field_head_pool`.
            world
                .field_head_pool
                .get((p.model_index - 0xF0) as usize)
                .and_then(|s| s.as_ref())
                .map(|g| (g.tmd.clone(), g.raw.clone()))
        } else {
            res.tmds
                .get(p.model_index as usize)
                .map(|t| (t.tmd.clone(), t.raw.clone()))
        };
        let Some((mut tmd, raw)) = src else {
            // An unresolved mesh source means this placement can NEVER
            // draw, no matter what seats it later - for a special
            // model that is "Noa/Gala missing from the cutscene", and
            // silently dropping it here made that unfindable.
            log::warn!(
                "field NPC placement {} (model 0x{:02X}{}) has no mesh source - never drawn",
                p.index,
                p.model_index,
                if p.special_model {
                    ", special/global pool"
                } else {
                    ", scene pack"
                }
            );
            return;
        };
        // The bank the clip id names is the live party-bank bit's
        // (`World::npc_clip_party_bank`), not the spawn class: a
        // spawn prologue may already have dropped the bit for a
        // scene-bank gesture.
        let bundle = if world.npc_clip_party_bank(p.index as u8, p.special_model) {
            locomotion_bundle.as_ref()
        } else {
            scene_bundle.as_ref()
        };
        // Rest pose: frame 0 of the record the placement's anim byte
        // names; the object table truncates to the clip's bone count
        // (the retail count-equality contract).
        // The live clip id (actor `+0x5C` after the spawn prologue)
        // wins over the header byte: a save crystal ships header anim
        // 0 and its prologue sets the savepoint clip.
        let anim_id = world.field_npc_live_anim(p.index).unwrap_or(p.anim_id);
        let pose: Option<Vec<([i16; 3], [i16; 3])>> = match (anim_id, bundle) {
            (0, _) | (_, None) => None,
            (id, Some(b)) => {
                // The live clip player over the same record (the scene
                // binding's) poses this NPC per frame; the rest pose below
                // stays the frame-0 fallback / first frame.
                let rec_idx = (id - 1) as usize;
                b.record(rec_idx).ok().map(|rec| {
                    let bones = rec.bone_count as usize;
                    tmd.objects.truncate(bones);
                    (0..bones)
                        .map(|bi| match b.bone_transform(rec_idx, 0, bi) {
                            Some(t) => (
                                [t.t_x as i16, t.t_y as i16, t.t_z as i16],
                                [t.r_x as i16, t.r_y as i16, t.r_z as i16],
                            ),
                            None => ([0; 3], [0; 3]),
                        })
                        .collect()
                })
            }
        };
        let mut vmesh = match &pose {
            Some(offsets) => legaia_tmd::mesh::tmd_to_vram_mesh_posed_rot(&tmd, &raw, offsets),
            None => legaia_tmd::mesh::tmd_to_vram_mesh(&tmd, &raw),
        };
        let mut cmesh = match &pose {
            Some(offsets) => legaia_tmd::mesh::tmd_to_color_mesh_posed_rot(&tmd, &raw, offsets),
            None => legaia_tmd::mesh::tmd_to_color_mesh(&tmd, &raw),
        };
        let prop_curated = legaia_render_kernels::scene_lighting::tag_emissive_meshes(
            &raw, &mut vmesh, &mut cmesh, &res.vram,
        );
        let mesh_idx = if vmesh.indices.is_empty() {
            None
        } else {
            match r.upload_vram_mesh(
                &vmesh.positions,
                &vmesh.uvs,
                &vmesh.cba_tsb,
                &vmesh.normals,
                &vmesh.colors,
                &vmesh.indices,
            ) {
                Ok(m) => {
                    let idx = self.meshes.len();
                    self.meshes.push(m);
                    self.scene_tmd_data.push((tmd.clone(), raw.clone()));
                    Some(idx)
                }
                Err(e) => {
                    log::warn!("play-window: NPC mesh upload (slot {}): {e:#}", p.index);
                    None
                }
            }
        };
        let color_idx = if cmesh.is_empty() {
            None
        } else {
            match r.upload_color_mesh_blended(
                &cmesh.positions,
                &cmesh.colors,
                &cmesh.indices,
                &cmesh.blend,
            ) {
                Ok(cm) => {
                    let idx = self.color_meshes.len();
                    self.color_meshes.push(cm);
                    Some(idx)
                }
                Err(e) => {
                    log::warn!("play-window: NPC colour upload (slot {}): {e:#}", p.index);
                    None
                }
            }
        };
        if mesh_idx.is_none() && color_idx.is_none() {
            return;
        }
        // Every drawn placement is a clip target, not only the ones
        // that spawned with a clip: a placement whose live anim id is
        // still `0` here (rikuroa's party Noa, posed later by a
        // cutscene's `A2 10 18`) takes its first clip from an ANIMATE
        // cue, and a slot with no pose source dropped that cue and
        // drew the raw TMD - every part at the actor origin, the head
        // poking out of the ground. The redraw cuts the mesh to the
        // clip's bone count, as the spawn binding does.
        self.npc_anim_srcs
            .insert(p.index as u8, (tmd.clone(), raw.clone()));
        // A clip-less slot also keeps its rest source: an op-`0x4B`
        // morph change rebuilds its static mesh from it
        // (`take_npc_morph_rebuilds`).
        if !self.npc_clip_players.contains_key(&(p.index as u8)) {
            self.npc_rest_srcs
                .insert(p.index as u8, (tmd.clone(), raw.clone(), pose.clone()));
        }
        // Light-emitter samples from this prop's meshes, clustered
        // in the prop's own frame at its spawn anchor - the interior
        // candle sconces / wall lamps are MAN scene-actor props, not
        // `.MAP` placements, so this layer is where most of them
        // surface. The set follows the actor's live position each
        // frame (`scene_lighting::place_prop_lights`), so a prop a
        // script walks keeps its light. Painted-lamplight texels are
        // NOT read here: on a character they are skin and hair.
        // A set whose actor is parked in the hide box doesn't emit.
        {
            // A prop spawned in the hide box (the Genesis Tree, moved
            // into the plaza by its spawn script) still derives its
            // set; `place_prop_lights` drops it while it is hidden.
            {
                let y = world.sample_field_floor_height(p.world_x as i32, p.world_z as i32);
                let spawn = [p.world_x as f32, y as f32, p.world_z as f32];
                let model = Mat4::from_translation(Vec3::from(spawn));
                let mut samples = legaia_render_kernels::scene_lighting::vram_mesh_emitters(
                    &vmesh.positions,
                    &vmesh.cba_tsb,
                    &vmesh.colors,
                    &vmesh.indices,
                    Some((&res.vram, &vmesh.uvs)),
                );
                samples.extend(legaia_render_kernels::scene_lighting::color_mesh_emitters(
                    &cmesh.positions,
                    &cmesh.colors,
                    &cmesh.blend,
                    &cmesh.indices,
                ));
                if let Some(hit) = prop_curated {
                    samples.push(legaia_render_kernels::scene_lighting::curated_mesh_sample(
                        &hit,
                    ));
                }
                let lights = legaia_render_kernels::scene_lighting::cluster_all_scene_lights(
                    &legaia_render_kernels::scene_lighting::transform_samples(&samples, &model),
                );
                if !lights.is_empty() {
                    prop_light_sets.push(legaia_render_kernels::scene_lighting::PropLights {
                        slot: p.index as u8,
                        spawn,
                        lights,
                    });
                }
            }
        }
        self.field_npc_draws.push(FieldNpcDraw {
            slot: p.index as u8,
            mesh_idx,
            color_idx,
            spawn: (p.world_x, p.world_z),
            bound_model,
        });
    }

    /// Cluster the accumulated emitter samples (static layers + MAN
    /// props) into the scene's point lights. Consumed per frame by the
    /// redraw staging; inert unless dynamic lighting + shadows are on.
    pub(super) fn install_scene_lights(
        &mut self,
        light_samples: &[legaia_render_kernels::scene_lighting::EmitterSample],
        mut prop_light_sets: Vec<legaia_render_kernels::scene_lighting::PropLights>,
    ) {
        let world = &self.session.host.world;
        {
            self.scene_point_lights =
                legaia_render_kernels::scene_lighting::cluster_all_scene_lights(light_samples);
            self.scene_prop_lights = std::mem::take(&mut prop_light_sets);
            if !self.scene_point_lights.is_empty() {
                let w = world;
                let ppos = w.player_actor_slot.and_then(|s| {
                    w.actors.get(s as usize).map(|a| {
                        (
                            a.move_state.world_x,
                            a.move_state.world_y,
                            a.move_state.world_z,
                        )
                    })
                });
                log::info!(
                    "play-window: {} derived scene light(s) from {} emitter sample(s) \
                     (nearest 8 to the player shade per frame; player at {ppos:?})",
                    self.scene_point_lights.len(),
                    light_samples.len()
                );
                for (i, l) in self.scene_point_lights.iter().take(12).enumerate() {
                    log::info!(
                        "  light {i}: pos ({:.0}, {:.0}, {:.0}) radius {:.0} colour \
                         ({:.2}, {:.2}, {:.2})",
                        l.pos[0],
                        l.pos[1],
                        l.pos[2],
                        l.radius,
                        l.color[0],
                        l.color[1],
                        l.color[2]
                    );
                }
            }
        }
    }
}
