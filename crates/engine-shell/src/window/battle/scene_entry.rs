//! Entering and leaving the battle render state: the stage backdrop build,
//! the battle VRAM and mesh registration, the display-list log, the exit
//! restore, and the rule that only registered combatants keep a mesh.

use super::*;

/// The stage shell's two meshes for one object list: the textured half and
/// the untextured (`F*` / `G*`) half, each drawn twice - the second copy
/// under the per-stage transform.
///
/// Retail draws the shell TWICE, the second copy under a per-stage diagonal
/// transform. The shell on the disc is an authored HALF (open toward -X, -Z
/// or +X, never +Z); the second copy is what closes the horizon, so a
/// single-copy backdrop is a half dome no matter how the camera orbits.
/// `append_scaled` reverses winding and flips normals when the transform has
/// negative determinant, which is the mesh-level analogue of retail's
/// `0x40000000 -> 0x48000000` draw-mode swap.
///
/// The shell's UNTEXTURED half: between a fifth and a tenth of a backdrop's
/// prims are `F*`/`G*` flat / gouraud panels carrying a baked colour word and
/// no UVs - the sky band, the painted wall faces, the flat water. The
/// VRAM-mesh builder drops every prim with no UVs, so those ride a colour
/// mesh with the same second copy. Retail has no such split - `FUN_8001ADA4`
/// case 3 walks the whole primitive list and the GPU takes
/// `POLY_F*`/`POLY_G*` packets as readily as `POLY_*T*` ones.
pub(super) fn stage_shell_meshes(
    tmd: &legaia_tmd::Tmd,
    raw: &[u8],
    second: legaia_scene_formats::battle_backdrop::SecondCopy,
    objects: &[usize],
) -> (legaia_tmd::mesh::VramMesh, legaia_tmd::mesh::ColorMesh) {
    let tmd0 = legaia_scene_formats::battle_backdrop::objects_tmd(tmd, objects);
    let mut vmesh = legaia_tmd::mesh::tmd_to_vram_mesh(&tmd0, raw);
    let first = vmesh.clone();
    vmesh.append_scaled(&first, second.scale());
    let mut cmesh = legaia_tmd::mesh::tmd_to_color_mesh(&tmd0, raw);
    let cfirst = cmesh.clone();
    cmesh.append_scaled(&cfirst, second.scale());
    (vmesh, cmesh)
}

/// One copy of the given backdrop objects, textured and untextured halves -
/// the mesh of a slot the backdrop draw transforms per frame, so nothing is
/// baked in ([`legaia_engine_scene_host::scene_host::BattleStageLayers`]).
pub(super) fn stage_slot_meshes(
    tmd: &legaia_tmd::Tmd,
    raw: &[u8],
    objects: &[usize],
) -> (legaia_tmd::mesh::VramMesh, legaia_tmd::mesh::ColorMesh) {
    let tmd0 = legaia_scene_formats::battle_backdrop::objects_tmd(tmd, objects);
    (
        legaia_tmd::mesh::tmd_to_vram_mesh(&tmd0, raw),
        legaia_tmd::mesh::tmd_to_color_mesh(&tmd0, raw),
    )
}

use legaia_engine_session::battle_stage::BattleStage;

impl PlayWindowApp {
    /// Build the battle's draw state on the `Field -> Battle` edge: the
    /// battle VRAM (stage textures, the effect atlas, each monster's texture
    /// pool injected at its slot), the backdrop shell and ground grid, and
    /// the monster and party meshes, bound to their actors.
    ///
    /// `stage` is the fight's stage as the session resolved it on the
    /// battle edge (`battle_stage::stage_battle_world`, which has already
    /// written the world state it leaves); this build only reads it.
    pub(in crate::window) fn enter_battle_render(&mut self, stage: Option<BattleStage>) {
        let monsters = self.session.host.world.battle_monster_slots();
        if monsters.is_empty() {
            return;
        }
        let Some(field_base) = self.cpu_vram_base.clone() else {
            return;
        };
        let Some(archive) = self.monster_archive_bytes() else {
            return;
        };
        // Fresh battle: drop any previous battle's facial-animation state
        // (re-registered per assembled member below) and make sure the
        // static SCUS face-frame tables are loaded. Done before the
        // renderer borrow below (both take `&mut self`).
        self.battle_faces.clear();
        self.load_face_tables();
        // The party's forms are the engine's to build (normally already done
        // by the tick that entered the battle); this covers a battle entered
        // outside a tick.
        self.session.host.ensure_battle_party_forms();
        // Build the battle-stage backdrop (the scene's scene_tmd_stream
        // half-dome). Its VRAM (scene + stage-dome textures resident) becomes
        // the battle base, so the dome renders textured behind the actors;
        // fall back to the field VRAM when the scene has no stage.
        let base = match &stage {
            Some(s) => s.vram.clone(),
            None => field_base,
        };
        let Some(r) = self.win.renderer.as_ref() else {
            return;
        };

        // Work on a throwaway copy so the field VRAM stays clean for the
        // restore on battle exit.
        let mut vram = base;
        // Blit the battle effect-texture atlas (PROT 870) into the battle VRAM
        // copy. Its pages land at fb_y=0 in the same columns the field stage
        // textures occupy, so this is a battle-only upload that battle exit
        // discards (the field VRAM base is untouched). Byte-verified against
        // live battle captures; soft-fails so a missing disc entry just leaves
        // the flame pages absent.
        if let Err(e) = legaia_engine_scene::scene::upload_flame_atlas_into_vram(
            &self.session.host.index,
            &mut vram,
            true,
        ) {
            log::warn!("play-window: flame-atlas VRAM upload skipped: {e:#}");
        }
        self.battle_mesh_base = self.meshes.len();
        self.battle_rest_vmesh.clear();
        self.battle_color_mesh_base = self.color_meshes.len();
        // Upload the stage dome mesh (drawn as the backdrop). Its textures live
        // in the stage VRAM, so build it unfiltered (all textured prims are
        // resident). Appended after `battle_mesh_base`, so battle exit truncates
        // it away with the monster meshes.
        self.battle_stage_mesh = None;
        self.battle_stage_color_mesh = None;
        self.battle_stage_spun_mesh = None;
        self.battle_stage_spun_color_mesh = None;
        self.battle_stage_shell = None;
        self.battle_stage_outdoor = false;
        // REF: FUN_800513f0 - the backdrop registration whose object-list edit
        // and second-copy transform this host consumes through
        // `legaia_scene_formats::battle_backdrop`.
        if let Some(BattleStage {
            dome: (tmd, raw),
            second,
            grid_far,
            outdoor,
            ..
        }) = &stage
        {
            // Retail's backdrop registration edits the object list rather
            // than truncating it: it drops index 1 and keeps the rest
            // (`legaia_scene_formats::battle_backdrop`, ported from `FUN_800513f0`).
            // On the two-object stage shells that is object 0 alone - object 1
            // is the ground-level ribbon of near props no retail capture shows,
            // and drawing it painted an engine-only white streak across the
            // Tetsu arena floor. On the four-object overworld domes it keeps
            // objects 0 (sky), 2 (mountains) and 3 (the flat ground ring).
            // `_DAT_8007B64B` (region `+8` bit 5) keeps object 1, and the
            // evolved-Cort arrival's hand-back rebinds slot 0 to it - the one
            // shared kernel is `SceneHost::battle_stage_object_indices`.
            let objects = self
                .session
                .host
                .battle_stage_object_indices(tmd.objects.len());
            // The shell proper is every slot but 1, baked with its second
            // copy. Slot 1 is the object the backdrop draw turns about Y
            // each frame (`BattleStageLayers`), so it is one copy of its
            // own, drawn twice under the frame's angle.
            let layers = self.session.host.battle_stage_layers(tmd.objects.len());
            let (vmesh, cmesh) = stage_shell_meshes(tmd, raw, *second, &layers.fixed);
            let spun = (!layers.spun.is_empty()).then(|| stage_slot_meshes(tmd, raw, &layers.spun));
            self.battle_stage_shell = Some((objects, *second));
            if !cmesh.is_empty()
                && let Ok(cm) = r.upload_color_mesh_blended(
                    &cmesh.positions,
                    &cmesh.colors,
                    &cmesh.indices,
                    &cmesh.blend,
                )
            {
                self.battle_stage_color_mesh = Some(self.color_meshes.len());
                self.color_meshes.push(cm);
            }
            if !vmesh.indices.is_empty()
                && let Ok(m) = r.upload_vram_mesh(
                    &vmesh.positions,
                    &vmesh.uvs,
                    &vmesh.cba_tsb,
                    &vmesh.normals,
                    &vmesh.colors,
                    &vmesh.indices,
                )
            {
                self.battle_stage_mesh = Some(self.meshes.len());
                self.meshes.push(m);
                self.scene_tmd_data.push((tmd.clone(), raw.clone()));
                if let Some((o, oc)) = spun {
                    if !o.indices.is_empty()
                        && let Ok(om) = r.upload_vram_mesh(
                            &o.positions,
                            &o.uvs,
                            &o.cba_tsb,
                            &o.normals,
                            &o.colors,
                            &o.indices,
                        )
                    {
                        self.battle_stage_spun_mesh = Some(self.meshes.len());
                        self.meshes.push(om);
                        self.scene_tmd_data.push((tmd.clone(), raw.clone())); // keep meshes/data aligned
                    }
                    if !oc.is_empty()
                        && let Ok(cm) = r.upload_color_mesh_blended(
                            &oc.positions,
                            &oc.colors,
                            &oc.indices,
                            &oc.blend,
                        )
                    {
                        self.battle_stage_spun_color_mesh = Some(self.color_meshes.len());
                        self.color_meshes.push(cm);
                    }
                }
                // Flat tiled ground grid under the actors (retail's
                // `func_0x801d02c0` grid), textured from the constant
                // retail page/CLUT/UV-window address - the scene battle
                // VRAM holds that scene's own ground tile there (see
                // `build_battle_ground_grid`).
                self.battle_ground_mesh = None;
                self.battle_ground_cue_far = None;
                self.battle_stage_outdoor = *outdoor;
                // Pre-cue colour = the settled battle ambient
                // (`0x8007B7B0` -> `RGBC`), see `build_ground_grid_rgbc`.
                let grid = build_battle_ground_grid(
                    legaia_engine_battle_vm::battle_ground_grid::GRID_RGBC_SETTLED,
                );
                match r.upload_vram_mesh(
                    &grid.positions,
                    &grid.uvs,
                    &grid.cba_tsb,
                    &grid.normals,
                    &grid.colors,
                    &grid.indices,
                ) {
                    Ok(gm) => {
                        self.battle_ground_mesh = Some(self.meshes.len());
                        self.battle_ground_rgbc =
                            legaia_engine_battle_vm::battle_ground_grid::GRID_RGBC_SETTLED;
                        // The grid's GTE depth cue: far colour per stage
                        // class (resolved in `battle_stage::resolve_battle_stage`), ramped
                        // by the emitter's per-vertex `SZ >> 2` law at draw
                        // time (see the redraw grid push).
                        self.battle_ground_cue_far = Some(*grid_far);
                        self.meshes.push(gm);
                        self.scene_tmd_data.push((tmd.clone(), raw.clone())); // keep meshes/data aligned
                    }
                    Err(e) => log::warn!("play-window: battle ground grid upload: {e:#}"),
                }
            }
        }
        let mut bound = 0usize;
        // Every actor slot this call registers a battle mesh on. It is the
        // battle's whole display list - see the un-register pass after the
        // party loop for why the complement matters.
        let mut registered: Vec<usize> = Vec::new();
        for (actor_idx, monster_id, slot) in monsters {
            let mesh = match legaia_actor_formats::monster_archive::mesh(&archive, monster_id) {
                Ok(Some(m)) => m,
                Ok(None) => continue,
                Err(e) => {
                    log::warn!("play-window: monster {monster_id} mesh decode failed: {e:#}");
                    continue;
                }
            };
            // Parse the embedded TMD up front so it can be retained parallel
            // to `meshes`; `battle_render_mesh` only yields a mesh when this
            // same parse succeeds.
            let Ok(tmd) = legaia_tmd::parse(mesh.tmd_bytes()) else {
                continue;
            };
            let Some(vmesh) = mesh.battle_render_mesh(slot, &mut vram) else {
                continue;
            };
            // The boss effect page the loader stages behind every id above
            // `0xB2` (Cort's Mystic Shield samples it).
            legaia_actor_formats::monster_archive::write_boss_effect_page(
                &archive, monster_id, &mut vram,
            );
            if vmesh.indices.is_empty() {
                continue;
            }
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
                    self.battle_rest_vmesh.insert(idx, vmesh);
                    // Keep `scene_tmd_data` length-parallel with `meshes`.
                    self.scene_tmd_data.push((tmd, mesh.tmd_bytes().to_vec()));
                    self.session.host.world.actors[actor_idx].tmd_binding = Some(idx);
                    registered.push(actor_idx);
                    // The texture slot and idle clip are the world's,
                    // installed on the battle edge for every host
                    // (`battle_stage::stage_battle_world`).
                    // The archive-order action-clip set (the `+0x4C` tag
                    // table the hit-reaction family and the AI's picked
                    // swings index) is the engine's to install:
                    // `SceneHost::tick` stages it the tick a battle is up,
                    // for a monster this window draws or not.
                    bound += 1;
                }
                Err(e) => log::warn!("play-window: monster {monster_id} mesh upload: {e:#}"),
            }
        }

        // Load the REAL battle party meshes and bind them to the party actor
        // slots. The faithful source is the retail battle loader's per-character
        // ASSEMBLY: each member's mesh is spliced from their player battle file's
        // equipment-id sections (`legaia_actor_formats::battle_char_assembly`, extraction
        // PROT 863..865) and relocated into the slot's runtime VRAM band
        // (`relocate_tsb_cba`, the registration-time TSB/CBA pass of
        // FUN_800513F0). The band's PIXELS come from the same player file: the
        // equipped sections' texture pools + the two record[0] image blocks,
        // uploaded at the pinned `FUN_80052FA0`/`FUN_80053B9C` placement
        // (`character_texture_uploads`; byte-exact vs live battle VRAM - see
        // docs/formats/battle-data-pack.md § Texture-pool VRAM placement).
        // PROT 1204 (the Baka Fighter / default-equipment sibling pack) stays
        // as the per-member fallback: its meshes when assembly fails, its
        // atlases (a 73-98% approximation of the band) when the texture-pool
        // decode fails. Each character's decoded battle palette overlays the
        // rows its mesh CBA samples (= 481 + slot after relocation).
        let mut party_bound = 0usize;
        // The engine built and installed each member's form at battle entry
        // (`SceneHost::ensure_battle_party_forms`, run by `SceneHost::tick`
        // for every session, headless included): the decode, the clips, the
        // art bank and records. This window adds only what is a renderer's -
        // the band pixels replayed into its battle VRAM, the GPU upload with
        // the rest-pose bake, and the facial animator's registration.
        let party = self.session.host.battle_party_forms().cloned();
        if let Some(party) = party {
            party.vram_writes.replay(&mut vram);
            for form in party.forms {
                // Rest pose: frame 0 of the assembled mesh's own idle stream
                // (the combat stance retail holds at battle start), or the
                // PROT 1203 bank's idle record for a fallback.
                // The registration-time model table draws the idle stream's
                // channels: the skeleton bones, no surplus extras, no
                // variants (`FUN_80048A08`; the per-frame posed rebuild
                // re-reads the live table).
                let masked = form.model_table.as_ref().map(|t| {
                    legaia_engine_battle::battle_attach_swap::mask_tmd(
                        &form.tmd,
                        &t.drawn_objects(usize::from(t.idle_parts)),
                    )
                });
                let rest_tmd = masked.as_ref().unwrap_or(&form.tmd);
                let vmesh = if form.rest_pose.is_empty() {
                    legaia_tmd::mesh::tmd_to_vram_mesh(rest_tmd, &form.tmd_bytes)
                } else {
                    legaia_tmd::mesh::tmd_to_vram_mesh_posed_rot(
                        rest_tmd,
                        &form.tmd_bytes,
                        &form.rest_pose,
                    )
                };
                if vmesh.indices.is_empty() {
                    continue;
                }
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
                        self.battle_rest_vmesh.insert(idx, vmesh);
                        let member = form.member;
                        self.session.host.world.actors[member].tmd_binding = Some(idx);
                        registered.push(member);
                        // Facial animation (FUN_8004C7B4): the per-tick stamp
                        // pass (`tick_battle_vram_restamps`) re-stamps the
                        // current eye/mouth frame onto the band's live face
                        // rows; the form only carries tracks when the band
                        // holds the real face-frame strip.
                        if let Some(tracks) = form.face_tracks {
                            self.battle_faces.push(BattleMemberFace::new(
                                member,
                                form.cslot,
                                tracks,
                                form.art_face_tracks,
                            ));
                        }
                        self.scene_tmd_data.push((form.tmd, form.tmd_bytes));
                        party_bound += 1;
                    }
                    Err(e) => log::warn!("play-window: party {} mesh upload: {e:#}", form.cslot),
                }
            }
        }

        // The battle display list is EXACTLY what this loader registered.
        let unregistered = unregister_non_battle_meshes(&mut self.session.host.world, &registered);
        let unregistered_count = unregistered.len();
        if bound > 0 || party_bound > 0 {
            match r.upload_vram(&vram) {
                Ok(v) => {
                    self.battle_vram_generation = Some(v.generation());
                    self.uploaded_vram = Some(v);
                }
                Err(e) => log::error!("play-window: battle VRAM re-upload: {e:#}"),
            }
            log::info!(
                "play-window: battle render bound {bound} monster + {party_bound} party mesh(es); \
                 {unregistered_count} field actor mesh(es) un-registered"
            );
        }
        // Stash the battle VRAM so a mid-battle player summon can inject its
        // creature texture into the slot its seat names
        // (`battle_stage::seat_summon_creature`).
        self.battle_vram = Some(vram);
        self.log_battle_display_list(&unregistered);
    }

    /// `LEGAIA_DIAG_BATDRAW=1`: dump the battle display list at battle entry -
    /// one line per actor slot that carries a `tmd_binding`, with its role
    /// (party ordinal / monster id / **STRAY**), its battle-world seat, the
    /// bound mesh's vertex count, and where that seat projects.
    ///
    /// Off by default. It exists because "the party mesh is bound" and "the
    /// party member is on screen" are different claims, and the startup log
    /// only ever made the first one: a bound mesh can sit behind the camera,
    /// off-frame, or - the case this was written for - under a scene prop
    /// that leaked into the fight. A `STRAY` row is a registration bug.
    ///
    /// The `clipw` / `ndc` columns are taken under the battle camera pose that
    /// is live **at registration**, i.e. before `tick_battle_camera` snaps the
    /// entry framing, so they answer "is this seat in front of a battle camera
    /// at all", not "where on screen does the player see it". The per-frame
    /// framing question is `LEGAIA_DIAG_BATCAM`'s (see `window/camera.rs`),
    /// and the per-frame mesh question is `LEGAIA_DIAG_POSE`'s.
    ///
    /// `unregistered` is the set of field slots the loader just dropped from
    /// the display list, printed so the pass is visible rather than silent.
    pub(super) fn log_battle_display_list(&self, unregistered: &[usize]) {
        if std::env::var_os("LEGAIA_DIAG_BATDRAW").is_none() {
            return;
        }
        let world = &self.session.host.world;
        let pc = world.party.party_count as usize;
        let cam = self.battle_dome_camera_mvp(4.0 / 3.0)
            * Mat4::from_scale(Vec3::splat(BATTLE_WORLD_SCALE));
        eprintln!(
            "BATDRAW display list ({} actor slots, un-registered field slots {unregistered:?})",
            world.actors.len()
        );
        for (i, a) in world.actors.iter().enumerate() {
            let Some(idx) = a.tmd_binding else { continue };
            let role = match (i < pc, a.battle_monster_id) {
                (_, Some(id)) => format!("monster id {id}"),
                (true, None) => format!("party ordinal {i}"),
                (false, None) => "STRAY (field actor)".to_string(),
            };
            let verts: usize = self
                .scene_tmd_data
                .get(idx)
                .map(|(t, _)| t.objects.iter().map(|o| o.vertices.len()).sum())
                .unwrap_or(0);
            let w = Vec3::new(
                a.move_state.world_x as f32,
                a.move_state.world_y as f32,
                a.move_state.world_z as f32,
            );
            let clip = cam * w.extend(1.0);
            eprintln!(
                "  actor{i} {role} active={} mesh={idx} verts={verts} world={w:?} \
                 clipw={:.1} ndc=({:.2},{:.2})",
                a.active,
                clip.w,
                clip.x / clip.w,
                clip.y / clip.w
            );
        }
    }
}

impl PlayWindowApp {
    /// Leave battle: restore the clean field VRAM and drop the appended
    /// battle monster meshes (the field actor table was already restored from
    /// the pre-battle snapshot, so those slots no longer reference them).
    ///
    /// That same restore is what puts the field scene's mesh bindings back
    /// after [`unregister_non_battle_meshes`] took them out of the battle
    /// display list - the two share one channel, so neither stashes anything.
    pub(in crate::window) fn exit_battle_render(&mut self) {
        if let (Some(r), Some(base)) = (self.win.renderer.as_ref(), self.cpu_vram_base.as_ref()) {
            match r.upload_vram(base) {
                Ok(v) => self.uploaded_vram = Some(v),
                Err(e) => log::error!("play-window: field VRAM restore: {e:#}"),
            }
        }
        let keep = self.battle_mesh_base.min(self.meshes.len());
        self.meshes.truncate(keep);
        self.battle_rest_vmesh.clear();
        self.scene_tmd_data
            .truncate(keep.min(self.scene_tmd_data.len()));
        let keep_color = self.battle_color_mesh_base.min(self.color_meshes.len());
        self.color_meshes.truncate(keep_color);
        // Tear down a spawned player-summon creature.
        if let Some(slot) = self.summon_actor_slot.take() {
            self.session.host.world.release_summon_seat(slot);
        }
        self.battle_vram = None;
        self.battle_vram_generation = None;
        self.battle_stage_mesh = None;
        self.battle_stage_color_mesh = None;
        self.battle_stage_spun_mesh = None;
        self.battle_stage_spun_color_mesh = None;
        self.battle_stage_shell = None;
        self.battle_ground_mesh = None;
        self.battle_ground_cue_far = None;
        self.battle_stage_outdoor = false;
        self.battle_faces.clear();
    }
}

/// Drop the `tmd_binding` of every actor slot the battle loader did **not**
/// just register, and report the slots dropped.
///
/// Retail's battle scene loader builds the fight its own actor set:
/// `FUN_800513F0` registers the backdrop, the assembled party blobs and the
/// monster meshes into `DAT_8007C018[]` and links those actors - and only
/// those - into the render OT. The field scene's actor list does not survive
/// the transition; nothing the town was drawing is still linked once the
/// arena comes up.
///
/// The port keeps ONE actor array across the transition (the world clones it
/// into `field_return` and restores it when the battle ends), so the field
/// slots arrive in the battle still carrying their scene-mesh bindings. Each
/// then draws at whatever battle-world coordinates its `move_state` happens
/// to hold - and for a scene actor that never moved that is the **origin**,
/// dead centre of the arena between the party row and the monster row. The
/// `!actor.active` gate at the draw site only catches the slots the scene
/// left inactive; an active field actor (rikuroa leaves two) walks straight
/// through it and plants a scene prop on top of the party member, which is
/// what "the party member is not visibly in the battle" turned out to be.
///
/// Un-registering here rather than filtering at the draw site keeps the rule
/// where the loader is: the battle registration decides what the battle
/// draws. Nothing is stashed for the restore - the bindings come back with
/// the field actor table (`World::end_battle`'s `field_return` restore), the
/// same channel `exit_battle_render` already relies on.
// REF: FUN_800513F0 (battle setup: the battle's own registration set)
/// The [`legaia_art::Character`] whose art tables a player-file character
/// slot (`0..=2` = Vahn / Noa / Gala) resolves against; `None` for Terra
/// (slot `3`), who has no arts catalog.
pub(in crate::window) fn unregister_non_battle_meshes(
    world: &mut legaia_engine_core::world::World,
    registered: &[usize],
) -> Vec<usize> {
    let mut dropped = Vec::new();
    for (i, a) in world.actors.iter_mut().enumerate() {
        if a.tmd_binding.is_some() && !registered.contains(&i) {
            a.tmd_binding = None;
            dropped.push(i);
        }
    }
    dropped
}

#[cfg(test)]
mod battle_display_list_tests {
    use super::unregister_non_battle_meshes;
    use legaia_engine_core::world::World;

    /// A field scene leaves live actors bound to its meshes and parked at the
    /// coordinates the scene gave them. After the battle registration only the
    /// combatant slots may still carry a binding - anything else draws inside
    /// the arena, in front of the party.
    #[test]
    fn only_the_registered_combatants_keep_a_battle_mesh() {
        let mut world = World::new();
        // Field state: every slot bound (the host's naive actor K -> mesh K
        // pre-bind) and a couple of them ACTIVE at the world origin, which is
        // exactly the shape that slipped through the draw site's
        // `!actor.active` gate.
        for (i, a) in world.actors.iter_mut().enumerate() {
            a.tmd_binding = Some(i);
        }
        world.actors[62].active = true;
        world.actors[63].active = true;
        world.enter_battle(1, 1);
        // What the loader registered this battle: party ordinal 0 + monster 1.
        let dropped = unregister_non_battle_meshes(&mut world, &[0, 1]);
        assert_eq!(world.actors[0].tmd_binding, Some(0));
        assert_eq!(world.actors[1].tmd_binding, Some(1));
        for (i, a) in world.actors.iter().enumerate().skip(2) {
            assert_eq!(a.tmd_binding, None, "slot {i} still draws in the battle");
        }
        assert!(dropped.contains(&62) && dropped.contains(&63));
        assert_eq!(dropped.len(), world.actors.len() - 2);
    }

    /// Non-vacuous: without the pass the same world keeps every field slot in
    /// the battle display list, including the two active ones at the origin.
    #[test]
    fn the_unfiltered_display_list_really_does_carry_the_strays() {
        let mut world = World::new();
        for (i, a) in world.actors.iter_mut().enumerate() {
            a.tmd_binding = Some(i);
        }
        world.actors[62].active = true;
        world.enter_battle(1, 1);
        let drawn = world
            .actors
            .iter()
            .enumerate()
            .filter(|(_, a)| a.tmd_binding.is_some() && a.active)
            .map(|(i, _)| i)
            .collect::<Vec<_>>();
        assert!(
            drawn.contains(&62),
            "the pre-fix draw predicate must admit the stray, or the fix pins nothing"
        );
        assert_eq!(world.actors[62].move_state.world_x, 0);
        assert_eq!(world.actors[62].move_state.world_z, 0);
    }

    /// A party slot the assembly failed on is un-registered too: it must show
    /// nothing at its seat, not the field mesh that happened to share its
    /// index.
    #[test]
    fn an_unbound_party_slot_does_not_fall_back_to_its_field_mesh() {
        let mut world = World::new();
        for (i, a) in world.actors.iter_mut().enumerate() {
            a.tmd_binding = Some(i);
        }
        world.enter_battle(3, 1);
        // Ordinals 0 and 2 assembled; ordinal 1 failed. Monster is slot 3.
        unregister_non_battle_meshes(&mut world, &[0, 2, 3]);
        assert_eq!(world.actors[1].tmd_binding, None);
        assert!(world.actors[0].tmd_binding.is_some());
        assert!(world.actors[2].tmd_binding.is_some());
        assert!(world.actors[3].tmd_binding.is_some());
    }
}
