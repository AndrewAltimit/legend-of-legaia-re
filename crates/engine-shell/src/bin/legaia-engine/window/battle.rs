//! Extracted from `window.rs` (mechanical split; behavior-preserving).

use super::*;

impl PlayWindowApp {
    /// Drain world battle events, fold each into HP / status state, and
    /// append a one-line summary to the HUD ring. Called once per simulation
    /// tick from the redraw handler.
    pub(super) fn drain_and_log_battle_events(&mut self) {
        let events = self.session.host.world.drain_battle_events();
        for ev in events {
            // Apply the gameplay-state side (currently only `ApplyArtStrike`
            // mutates HP / status; other events are visual-only here).
            self.session.host.world.fold_battle_event(&ev);
            // Surface in the HUD ring.
            if self.battle_event_log.len() >= Self::BATTLE_EVENT_LOG_CAP {
                self.battle_event_log.pop_front();
            }
            self.battle_event_log.push_back(ev.summary());
        }

        // Floating damage / heal numbers: the live battle loop resolves and
        // applies HP itself, then queues a presentation-only FX per strike.
        // Feed those into the popup model and log the magnitude (the typed
        // events above are consumed inside the live loop, so this is the
        // only place per-strike damage surfaces while live).
        let fx = self.session.host.world.drain_battle_hit_fx();
        for f in fx {
            if f.is_heal {
                self.battle_hud.push_heal(f.target_slot, f.amount);
            } else if f.is_crit {
                self.battle_hud.push_popup(
                    legaia_engine_core::battle_hud::DamagePopup::damage(f.target_slot, f.amount)
                        .crit(),
                );
            } else {
                self.battle_hud.push_damage(f.target_slot, f.amount);
            }
            if self.battle_event_log.len() >= Self::BATTLE_EVENT_LOG_CAP {
                self.battle_event_log.pop_front();
            }
            let sign = if f.is_heal { '+' } else { '-' };
            self.battle_event_log
                .push_back(format!("slot {} {}{} HP", f.target_slot, sign, f.amount));
        }

        // Battle sound cues: the art-strike outcomes resolve per-strike SFX
        // cues (kind = the SfxBank id, played directly without classify_cue).
        // Enqueue each into the director's SfxScheduler at its strike-relative
        // timing, then advance the scheduler one frame and fire any matured cue
        // through the scene VAB. Drain first so the world borrow ends before
        // the director borrow. SFX touch the SPU only - no RNG - so battle
        // determinism stays bit-exact.
        let cues = self.session.host.world.drain_battle_sfx_cues();
        if let Some(bgm) = self.session.bgm.as_mut() {
            for cue in &cues {
                bgm.enqueue_sfx(cue.kind, cue.timing_frames, cue.actor_slot, cue.target_slot);
            }
            for (id, voice) in bgm.tick_sfx_frame() {
                log::debug!("battle SFX cue {id:#04x} fired on voice {voice}");
            }
        } else {
            for cue in &cues {
                log::debug!(
                    "battle SFX cue {:#04x} @ +{} frames (actor {} -> target {}; no audio)",
                    cue.kind,
                    cue.timing_frames,
                    cue.actor_slot,
                    cue.target_slot
                );
            }
        }

        // Refresh per-slot status icons + age the popups one frame.
        if self.session.host.world.mode == SceneMode::Battle {
            for slot in 0..self.battle_hud.slots.len() as u8 {
                self.battle_hud
                    .sync_status(slot, &self.session.host.world.status_effects);
            }
        }
        self.battle_hud.tick();
    }

    /// The lead roster character's four weapon-swing AP costs (runtime slots
    /// `0xC..=0xF` → indices 0..3), from their player battle file's equipped
    /// sections. `None` when any stage of the decode fails.
    pub(super) fn lead_swing_costs(&self) -> Option<[u8; 4]> {
        let raw = self.session.host.index.entry_bytes_extended(863).ok()?;
        let pack = legaia_asset::battle_data_pack::parse(&raw).ok()?;
        let equipped: [u8; 5] = self
            .session
            .host
            .world
            .roster
            .members
            .first()
            .map(|rec| {
                let slots = rec.equipment().slots;
                [slots[0], slots[1], slots[2], slots[3], slots[4]]
            })
            .unwrap_or_default();
        let swings =
            legaia_asset::battle_char_assembly::swing_battle_animations(&raw, &pack, &equipped)
                .ok()?;
        let mut costs = [0u8; 4];
        for s in &swings {
            let i = s.slot.checked_sub(0xC)? as usize;
            if i < 4 {
                costs[i] = s.cost;
            }
        }
        Some(costs)
    }

    /// React to a `Field <-> Battle` scene-mode change once per transition:
    /// on entering battle, decode each enemy's mesh and inject it; on leaving,
    /// restore the clean field VRAM and drop the battle meshes. Called each
    /// frame before the render borrows `uploaded_vram`.
    pub(super) fn sync_battle_render(&mut self) {
        let mode = self.session.host.world.mode;
        let prev = self.prev_scene_mode.replace(mode);
        if prev == Some(mode) {
            return;
        }
        match (prev, mode) {
            (_, SceneMode::Battle) => self.enter_battle_render(),
            (Some(SceneMode::Battle), _) => self.exit_battle_render(),
            _ => {}
        }
    }

    /// Bridge the decoded monster meshes for the current battle into the draw
    /// list: inject each enemy's texture pool into a clone of the field VRAM
    /// at the loader's per-slot coords, upload the relocated mesh, and bind it
    /// to the enemy actor. Re-uploads the edited VRAM so the injected texture
    /// pages resolve.
    /// Build the current scene's battle-stage backdrop, if it has one. Returns
    /// the battle VRAM (scene + stage-dome textures resident) and the stage
    /// dome's `(Tmd, raw)`. The faithful battle backdrop is the scene's
    /// `scene_tmd_stream` half-dome (sky + mountain ring + ground); building the
    /// scene in `SceneLoadKind::Battle` makes that dome TMD + its textures
    /// resident (the Field build excludes them). `None` when the scene has no
    /// stage entry.
    pub(super) fn build_battle_stage(
        &self,
    ) -> Option<(legaia_tim::Vram, (legaia_tmd::Tmd, Vec<u8>))> {
        let scene = self.session.host.scene.as_ref()?;
        let scene_name = scene.name.clone();
        let stage_entry = *self
            .session
            .host
            .index
            .battle_stage_entries(&scene_name)
            .first()?;
        let mut shared: Vec<Scene> = Vec::new();
        for name in FIELD_SHARED_BLOCKS {
            if let Ok(s) = Scene::load(&self.session.host.index, name) {
                shared.push(s);
            }
        }
        let refs: Vec<&Scene> = shared.iter().collect();
        let (res, _) = SceneResources::build_targeted_with_options(
            scene,
            &refs,
            BuildOptions {
                kind: SceneLoadKind::Battle,
                upload_all_tims: true,
            },
        )
        .ok()?;
        // The stage dome is the leading TMD of the scene_tmd_stream stage entry.
        let dome = res.tmds.iter().find(|t| t.entry_idx == stage_entry)?;
        log::info!(
            "play-window: battle stage = scene '{scene_name}' PROT {stage_entry} \
             ({} objects)",
            dome.tmd.objects.len()
        );
        Some((res.vram.clone(), (dome.tmd.clone(), dome.raw.clone())))
    }

    pub(super) fn enter_battle_render(&mut self) {
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
        // Build the battle-stage backdrop (the scene's scene_tmd_stream
        // half-dome). Its VRAM (scene + stage-dome textures resident) becomes
        // the battle base, so the dome renders textured behind the actors;
        // fall back to the field VRAM when the scene has no stage.
        let stage = self.build_battle_stage();
        let base = match &stage {
            Some((sv, _)) => sv.clone(),
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
        if let Err(e) = legaia_engine_core::scene::upload_flame_atlas_into_vram(
            &self.session.host.index,
            &mut vram,
            true,
        ) {
            log::warn!("play-window: flame-atlas VRAM upload skipped: {e:#}");
        }
        self.battle_mesh_base = self.meshes.len();
        // Upload the stage dome mesh (drawn as the backdrop). Its textures live
        // in the stage VRAM, so build it unfiltered (all textured prims are
        // resident). Appended after `battle_mesh_base`, so battle exit truncates
        // it away with the monster meshes.
        self.battle_stage_mesh = None;
        if let Some((_, (tmd, raw))) = &stage {
            let vmesh = legaia_tmd::mesh::tmd_to_vram_mesh(tmd, raw);
            if !vmesh.indices.is_empty()
                && let Ok(m) = r.upload_vram_mesh(
                    &vmesh.positions,
                    &vmesh.uvs,
                    &vmesh.cba_tsb,
                    &vmesh.normals,
                    &vmesh.indices,
                )
            {
                self.battle_stage_mesh = Some(self.meshes.len());
                self.meshes.push(m);
                self.scene_tmd_data.push((tmd.clone(), raw.clone()));
                // Flat tiled ground grid under the actors (retail's
                // `func_0x801d02c0` grass grid). Reuse the dome's grass texel so
                // it samples real grass from the battle VRAM; drawn with the
                // actor camera so the party stands on it.
                self.battle_ground_mesh = None;
                if let Some(grid) = build_battle_ground_grid(&vmesh)
                    && let Ok(gm) = r.upload_vram_mesh(
                        &grid.positions,
                        &grid.uvs,
                        &grid.cba_tsb,
                        &grid.normals,
                        &grid.indices,
                    )
                {
                    self.battle_ground_mesh = Some(self.meshes.len());
                    self.meshes.push(gm);
                    self.scene_tmd_data.push((tmd.clone(), raw.clone())); // keep meshes/data aligned
                }
            }
        }
        let mut bound = 0usize;
        for (actor_idx, monster_id, slot) in monsters {
            let mesh = match legaia_asset::monster_archive::mesh(&archive, monster_id) {
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
            if vmesh.indices.is_empty() {
                continue;
            }
            match r.upload_vram_mesh(
                &vmesh.positions,
                &vmesh.uvs,
                &vmesh.cba_tsb,
                &vmesh.normals,
                &vmesh.indices,
            ) {
                Ok(m) => {
                    let idx = self.meshes.len();
                    self.meshes.push(m);
                    // Keep `scene_tmd_data` length-parallel with `meshes`.
                    self.scene_tmd_data.push((tmd, mesh.tmd_bytes().to_vec()));
                    self.session.host.world.actors[actor_idx].tmd_binding = Some(idx);
                    // Record the texture slot so the posed-animation rebuild can
                    // re-apply the per-slot CBA/TSB relocation (the raw-TMD posed
                    // mesh otherwise carries the nominal on-disc addresses and
                    // samples the wrong VRAM page → untextured/white).
                    self.session.host.world.actors[actor_idx].battle_tex_slot = Some(slot);
                    // Attach the monster's idle clip so its limbs move in
                    // battle. The clip's part count should equal the TMD object
                    // count (one rigid transform per object); MonsterAnimPlayer
                    // tolerates a mismatch (extra objects stay at rest).
                    match legaia_asset::monster_archive::idle_animation(&archive, monster_id) {
                        Ok(Some(idle)) => {
                            if let Some(player) =
                                legaia_engine_core::battle_anim::MonsterAnimPlayer::new(&idle)
                            {
                                self.session
                                    .host
                                    .world
                                    .set_actor_battle_animation(actor_idx, player);
                            }
                        }
                        Ok(None) => {}
                        Err(e) => {
                            log::warn!("play-window: monster {monster_id} idle anim decode: {e:#}")
                        }
                    }
                    // Install the full archive-order action-clip set so the
                    // hit-reaction family (action tags 2..5, the retail
                    // `+0x1EF` map) can play when this monster takes damage.
                    match legaia_asset::monster_archive::animations(&archive, monster_id) {
                        Ok(Some(anims)) if !anims.is_empty() => {
                            let clips: Vec<_> = anims.into_iter().map(Some).collect();
                            self.session.host.world.set_actor_battle_action_clips(
                                actor_idx,
                                std::sync::Arc::new(clips),
                            );
                        }
                        Ok(_) => {}
                        Err(e) => {
                            log::warn!("play-window: monster {monster_id} action clips: {e:#}")
                        }
                    }
                    bound += 1;
                }
                Err(e) => log::warn!("play-window: monster {monster_id} mesh upload: {e:#}"),
            }
        }

        // Load the REAL battle party meshes and bind them to the party actor
        // slots. The faithful source is the retail battle loader's per-character
        // ASSEMBLY: each member's mesh is spliced from their player battle file's
        // equipment-id sections (`legaia_asset::battle_char_assembly`, extraction
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
        let party_count = self.session.host.world.party_count as usize;
        if party_count > 0
            && let Ok(pack_raw) = self
                .session
                .host
                .index
                .entry_bytes_extended(legaia_asset::battle_char_pack::PROT_ENTRY_INDEX)
            && let Ok(pack) = legaia_asset::battle_char_pack::parse(&pack_raw)
        {
            // Upload the 7 character atlases (256x256 4bpp + their CLUT rows)
            // at their declared authoring rects (the Baka Fighter layout the
            // fallback meshes sample).
            for atlas in &pack.atlases {
                if let Ok(tim) = legaia_tim::parse(&atlas.tim_bytes) {
                    vram.upload_tim(&tim);
                }
            }
            // The battle party mesh is a set of object-local TMD pieces (head,
            // torso, limbs) - NOT pre-assembled. The retail battle sockets the
            // ASSEMBLED mesh with the character's own idle keyframe stream from
            // record[0] of the same player file (monster-format `[parts]
            // [frames][9-byte TRS]`, parts = skeleton bones; equipment extras
            // ride their attach bone - `assembled_party_battle_mesh` returns it
            // expanded per object). PROT 1203 is NOT that source: its banks are
            // authored against PROT 1204's own object order, which differs from
            // the assembled blob's sorted bone-tag order per character - posing
            // the assembled mesh from 1203 mis-sockets joints and splits the
            // duplicate equipment pieces apart. 1203 stays as the rest-pose
            // source for the 1204 FALLBACK mesh only (banks Vahn 0-8 /
            // Noa 9-17 / Gala 18-26, idle = each bank's first record; bone i =
            // 1204 object i).
            let battle_anm = self
                .session
                .host
                .index
                .entry_bytes_extended(1203)
                .ok()
                .and_then(|raw| {
                    [6usize, 3, 5, 7].iter().find_map(|&dc| {
                        legaia_asset::player_anm::find_in_entry(&raw, dc)
                            .into_iter()
                            .next()
                    })
                });
            // Per-member: assemble the battle mesh from the occupying
            // character's player file (fallback: the static PROT 1204 slot),
            // overlay its battle palette onto the CLUT rows the mesh samples,
            // upload, bind to the party actor. The retail rule (live-verified
            // for all four characters, incl. Terra): the CHARACTER picks the
            // content - player file + palette = extraction PROT `863 +
            // char_slot` (raw TOC 0x361-0x364; see docs/formats/cdname.md
            // numbering space) - while the present-party ORDINAL picks the
            // runtime texture band (`relocate_tsb_cba` x = 0x200 + i*0x80,
            // CLUT row 481 + i). `World::active_party` supplies the mapping;
            // empty = the identity Vahn/Noa/Gala default.
            for member in 0..party_count.min(3) {
                let cslot = self.session.host.world.party_roster_slot(member);
                // (tmd, raw bytes, per-object idle clip). The assembled path
                // poses from the player file's own idle stream (already
                // expanded so channel i drives object i, extras included);
                // the fallback 1204 mesh has no stream (`None`) and poses
                // from PROT 1203 with the identity object->bone mapping.
                let mut source: Option<(
                    legaia_tmd::Tmd,
                    Vec<u8>,
                    Option<legaia_asset::monster_archive::MonsterAnimation>,
                )> = None;
                let mut tex_uploads: Vec<legaia_asset::battle_char_assembly::TextureUpload> =
                    Vec::new();
                let mut action_clips: Option<
                    Vec<Option<legaia_asset::monster_archive::MonsterAnimation>>,
                > = None;
                let mut art_bank: Option<
                    Vec<Option<legaia_asset::monster_archive::MonsterAnimation>>,
                > = None;
                let mut face_tracks: Option<Vec<Option<legaia_asset::face_anim::FaceTracks>>> =
                    None;
                let mut art_face_tracks: Vec<Option<legaia_asset::face_anim::FaceTracks>> =
                    Vec::new();
                if let Some((asm, uploads, idle, clips, bank, faces, art_faces)) =
                    self.assembled_party_battle_mesh(cslot, member)
                {
                    tex_uploads = uploads;
                    action_clips = Some(clips);
                    art_bank = Some(bank);
                    face_tracks = Some(faces);
                    art_face_tracks = art_faces;
                    match legaia_tmd::parse(&asm.tmd) {
                        Ok(tmd) => source = Some((tmd, asm.tmd, Some(idle))),
                        Err(e) => log::warn!(
                            "play-window: party {cslot} assembled TMD parse: {e:#} \
                             (falling back to PROT 1204)"
                        ),
                    }
                }
                let assembled = source.is_some();
                if source.is_none()
                    && let Some(slot) = pack.slot(cslot)
                    && let Ok(tmd) = legaia_tmd::parse(&slot.tmd_bytes)
                {
                    source = Some((tmd, slot.tmd_bytes.clone(), None));
                }
                let Some((tmd, tmd_bytes, idle_anim)) = source else {
                    continue;
                };
                // Band pixels for the relocated mesh: the equipped sections'
                // texture pools + record[0] image blocks at the pinned
                // FUN_80052FA0 placement; the 1204 atlas pair approximates
                // the band only when the pool decode failed. Fallback 1204
                // meshes sample the authoring rects uploaded above instead.
                if assembled {
                    if tex_uploads.is_empty() {
                        // Approximation content = the CHARACTER's atlas pair;
                        // destination = the MEMBER ordinal's runtime band.
                        for half in 0..2usize {
                            if let Some(atlas) = pack.atlases.get(cslot * 2 + half)
                                && let Ok(tim) = legaia_tim::parse(&atlas.tim_bytes)
                            {
                                let img = &tim.image;
                                vram.write_block(
                                    512 + (member * 2 + half) as u16 * 64,
                                    256,
                                    img.fb_w,
                                    img.h,
                                    &img.data,
                                );
                            }
                        }
                    } else {
                        for u in &tex_uploads {
                            vram.write_block(u.fb_x(), u.fb_y(), u.rect.w, u.rect.h, &u.pixels);
                            if !u.clut.is_empty() {
                                vram.write_clut_row(u.clut_x, u.clut_row(), &u.clut_bytes());
                            }
                        }
                    }
                }
                // Rest pose: frame 0 of the assembled mesh's own idle stream
                // (the combat stance retail holds at battle start). Fallback
                // 1204 meshes pose from PROT 1203 instead - banks (per
                // docs/formats/character-mesh.md): records 0-8 = Vahn
                // (15-bone), 9-17 = Noa (16), 18-26 = Gala (15); the FIRST
                // record of each bank is that character's idle rest pose,
                // bone i driving 1204 object i.
                let bone_offsets: Vec<([i16; 3], [i16; 3])> = match (&idle_anim, &battle_anm) {
                    (Some(anim), _) => anim
                        .frames
                        .first()
                        .map(|f0| {
                            f0.iter()
                                .map(|p| {
                                    ([p.tx, p.ty, p.tz], [p.rx as i16, p.ry as i16, p.rz as i16])
                                })
                                .collect()
                        })
                        .unwrap_or_default(),
                    (None, Some(b)) => {
                        // Idle record per 1203 bank. The banks cover the
                        // Vahn/Noa/Gala trio only - a Terra fallback mesh
                        // (no bank) renders unposed.
                        match [0usize, 9, 18].get(cslot) {
                            Some(&rec) => (0..tmd.objects.len())
                                .map(|o| match b.bone_transform(rec, 0, o) {
                                    Some(t) => (
                                        [t.t_x as i16, t.t_y as i16, t.t_z as i16],
                                        [t.r_x as i16, t.r_y as i16, t.r_z as i16],
                                    ),
                                    None => ([0; 3], [0; 3]),
                                })
                                .collect(),
                            None => Vec::new(),
                        }
                    }
                    (None, None) => Vec::new(),
                };
                let vmesh = if bone_offsets.is_empty() {
                    legaia_tmd::mesh::tmd_to_vram_mesh(&tmd, &tmd_bytes)
                } else {
                    legaia_tmd::mesh::tmd_to_vram_mesh_posed_rot(&tmd, &tmd_bytes, &bone_offsets)
                };
                if vmesh.indices.is_empty() {
                    continue;
                }
                // Rows the mesh CBA samples, and (for collect) the columns.
                let mut rows: Vec<u16> =
                    vmesh.cba_tsb.iter().map(|c| (c[0] >> 6) & 0x1FF).collect();
                rows.sort_unstable();
                rows.dedup();
                let mut cols: Vec<u16> = vmesh.cba_tsb.iter().map(|c| (c[0] & 0x3F) * 16).collect();
                cols.sort_unstable();
                cols.dedup();
                // Decode + overlay the battle palette from the character's own
                // player file. Vahn (863) = byte-exact parse_record; the other
                // characters (864..866, incl. Terra) = equipment-robust collect.
                let pal = match cslot {
                    0 => self
                        .session
                        .host
                        .index
                        .entry_bytes_extended(863)
                        .ok()
                        .and_then(|f| {
                            let rec0 = legaia_asset::battle_char_palette::find_record0(&f)?;
                            legaia_asset::battle_char_palette::parse_record(&f, rec0).ok()
                        }),
                    1..=3 => self
                        .session
                        .host
                        .index
                        .entry_bytes_extended(863 + cslot as u32)
                        .ok()
                        .and_then(|f| {
                            legaia_asset::battle_char_palette::collect_palette(&f, 0, &cols).ok()
                        }),
                    _ => None,
                };
                if let Some(pal) = pal {
                    // STP-set palette bands onto each row the mesh CBA samples.
                    for &row in &rows {
                        for band in &pal.bands {
                            let bytes: Vec<u8> = band
                                .vram_words()
                                .iter()
                                .flat_map(|w| w.to_le_bytes())
                                .collect();
                            vram.write_clut_row(band.base, row, &bytes);
                        }
                    }
                }
                match r.upload_vram_mesh(
                    &vmesh.positions,
                    &vmesh.uvs,
                    &vmesh.cba_tsb,
                    &vmesh.normals,
                    &vmesh.indices,
                ) {
                    Ok(m) => {
                        let idx = self.meshes.len();
                        self.meshes.push(m);
                        self.scene_tmd_data.push((tmd, tmd_bytes));
                        self.session.host.world.actors[member].tmd_binding = Some(idx);
                        // Assembled meshes loop their own idle clip, exactly
                        // like monsters (the per-frame posed path rebuilds
                        // from the relocated TMD bytes, so texture
                        // addressing stays in the slot band).
                        if let Some(anim) = &idle_anim
                            && let Some(player) =
                                legaia_engine_core::battle_anim::MonsterAnimPlayer::new(anim)
                        {
                            self.session
                                .host
                                .world
                                .set_actor_battle_animation(member, player);
                        }
                        // Hand the SM pose hook the full action-clip set
                        // (record[0] slots + the equipment-spliced swings at
                        // 0xC..0xF) so ready / recover / defeat AND the
                        // staged attack-band swings play their real streams.
                        if let Some(clips) = action_clips.take() {
                            self.session
                                .host
                                .world
                                .set_actor_battle_action_clips(member, std::sync::Arc::new(clips));
                        }
                        // And the art bank, so staged ids >= 0x10 resolve
                        // through the ME-archive streams (FUN_8004AD80).
                        if let Some(bank) = art_bank.take().filter(|b| !b.is_empty()) {
                            self.session
                                .host
                                .world
                                .set_actor_battle_art_bank(member, std::sync::Arc::new(bank));
                        }
                        // Facial animation (FUN_8004C7B4): register the
                        // member's per-action face tracks so the per-tick
                        // stamp pass (`tick_battle_face_stamps`) re-stamps
                        // the current eye/mouth frame onto the band's live
                        // face rows. Only meaningful when the band holds
                        // the REAL texture-pool pixels (the face-frame
                        // strip the stamps copy from); the retail animator
                        // covers chars 0..2 (Terra is skipped) on bands
                        // 0..2.
                        if assembled
                            && !tex_uploads.is_empty()
                            && cslot < legaia_asset::face_anim::FACE_CHAR_COUNT
                            && member < legaia_asset::face_anim::FACE_SLOT_COUNT
                            && let Some(tracks) = face_tracks.take()
                        {
                            self.battle_faces.push(BattleMemberFace {
                                actor_slot: member,
                                char_index: cslot,
                                tracks,
                                art_tracks: std::mem::take(&mut art_face_tracks),
                                last_stamps: None,
                                art_counter: None,
                            });
                        }
                        party_bound += 1;
                    }
                    Err(e) => log::warn!("play-window: party {cslot} mesh upload: {e:#}"),
                }
            }
        }

        if bound > 0 || party_bound > 0 {
            match r.upload_vram(&vram) {
                Ok(v) => {
                    self.battle_vram_generation = Some(v.generation());
                    self.uploaded_vram = Some(v);
                }
                Err(e) => log::error!("play-window: battle VRAM re-upload: {e:#}"),
            }
            log::info!(
                "play-window: battle render bound {bound} monster + {party_bound} party mesh(es)"
            );
        }
        // Stash the battle VRAM + the monster-slot count so a mid-battle player
        // summon can inject its creature texture into the next free slot.
        self.battle_tex_slots_used = (bound as u8).min(4);
        self.battle_vram = Some(vram);
    }

    /// Assemble one party member's battle mesh the way the retail battle
    /// loader does: read the character's player battle file (extraction PROT
    /// `863 + cslot`, the `data\battle\PLAYER<n>` files), select its five
    /// equipment sections by the roster record's equipped item ids
    /// (`+0x196..+0x19A`), splice them into the merged TMD
    /// (`legaia_asset::battle_char_assembly`), then apply the
    /// registration-time TSB/CBA relocation into the present-party
    /// ORDINAL's runtime VRAM band (`relocate_tsb_cba` over `band`;
    /// texpages `512 + 128*band` / `+64` at `y = 256`, CLUT row
    /// `481 + band` - the live-verified retail rule: the band follows the
    /// party position, the content follows the character). Also decodes
    /// the matching band
    /// *pixels* - the equipped sections' texture pools + record[0] image
    /// blocks at the pinned placement (`character_texture_uploads`; empty
    /// with a warning when that decode fails, so the caller can fall back
    /// to the 1204 atlas approximation) - and the character's **idle battle
    /// animation** from record[0] of the same file
    /// (`idle_battle_animation`, the retail pose source for the assembled
    /// mesh), expanded per assembled object (`expand_animation_for_objects`
    /// over `anm_bones`, so channel `i` drives TMD object `i`, equipment
    /// extras riding their attach bone). `None` (with a warning) when the
    /// mesh assembly or the idle-stream decode fails - the caller falls
    /// back to the slot's static PROT 1204 mesh, whose rest pose
    /// (PROT 1203, identity object->bone) is known-correct.
    pub(super) fn assembled_party_battle_mesh(
        &self,
        cslot: usize,
        band: usize,
    ) -> Option<AssembledPartyMesh> {
        let prot = 863 + cslot as u32;
        let raw = match self.session.host.index.entry_bytes_extended(prot) {
            Ok(raw) => raw,
            Err(e) => {
                log::warn!("play-window: party {cslot} player file (PROT {prot}): {e:#}");
                return None;
            }
        };
        let pack = match legaia_asset::battle_data_pack::parse(&raw) {
            Ok(pack) => pack,
            Err(e) => {
                log::warn!("play-window: party {cslot} player-file pack parse: {e:#}");
                return None;
            }
        };
        // Equipped item ids from the canonical roster record; an absent or
        // zeroed record assembles the all-default (unequipped) sections.
        let equipped: [u8; 5] = self
            .session
            .host
            .world
            .roster
            .members
            .get(cslot)
            .map(|rec| {
                let slots = rec.equipment().slots;
                [slots[0], slots[1], slots[2], slots[3], slots[4]]
            })
            .unwrap_or_default();
        let mut asm =
            match legaia_asset::battle_char_assembly::assemble_character(&raw, &pack, &equipped) {
                Ok(asm) => asm,
                Err(e) => {
                    log::warn!(
                        "play-window: party {cslot} battle-mesh assembly \
                         (equipped {equipped:02x?}): {e:#}"
                    );
                    return None;
                }
            };
        if let Err(e) =
            legaia_asset::battle_char_assembly::relocate_tsb_cba(&mut asm.tmd, band as u8)
        {
            log::warn!("play-window: party {cslot} TSB/CBA relocation: {e:#}");
            return None;
        }
        // The assembled mesh's pose source: the character's own idle stream
        // (record[0] action slot 0), expanded so channel i drives object i.
        let idle = match legaia_asset::battle_char_assembly::idle_battle_animation(&raw) {
            Ok(Some(anim)) => legaia_asset::battle_char_assembly::expand_animation_for_objects(
                &anim,
                &asm.anm_bones,
            ),
            Ok(None) => {
                log::warn!("play-window: party {cslot} player file carries no idle stream");
                return None;
            }
            Err(e) => {
                log::warn!("play-window: party {cslot} idle-stream decode: {e:#}");
                return None;
            }
        };
        // Band pixels at the pinned FUN_80052FA0 placement. A failure here
        // degrades to the 1204 atlas approximation, not to a mesh fallback.
        let uploads = legaia_asset::battle_char_assembly::character_texture_uploads(
            &raw, &pack, &equipped, band as u8,
        )
        .unwrap_or_else(|e| {
            log::warn!("play-window: party {cslot} texture-pool decode: {e:#}");
            Vec::new()
        });
        // Every populated action-stream slot, expanded the same way as the
        // idle: the battle-action SM's pose hook switches between these
        // (ready / recover / defeat one-shots over the idle loop).
        let mut clips: Vec<Option<legaia_asset::monster_archive::MonsterAnimation>> =
            vec![None; legaia_asset::battle_char_assembly::ACTION_SLOT_COUNT];
        // Per-action facial keyframe tracks (entry +0x8C eyes / +0x98
        // mouth), keyed like `clips` by the playing clip's action_id; the
        // per-frame facial animator (`tick_battle_face_stamps`) looks the
        // playing clip's tracks up here. Record[0] entries first, the
        // equipment-spliced swing entries' tracks land on slots 0xC..0xF
        // in the swing loop below.
        let mut faces: Vec<Option<legaia_asset::face_anim::FaceTracks>> =
            vec![None; legaia_asset::battle_char_assembly::ACTION_SLOT_COUNT];
        match legaia_asset::face_anim::battle_face_tracks(&raw) {
            Ok(t) => faces = t,
            Err(e) => log::warn!("play-window: party {cslot} face-track decode: {e:#}"),
        }
        match legaia_asset::battle_char_assembly::battle_animations(&raw) {
            Ok(anims) => {
                for a in &anims {
                    if let Some(slot) = clips.get_mut(a.action_id as usize) {
                        *slot = Some(
                            legaia_asset::battle_char_assembly::expand_animation_for_objects(
                                a,
                                &asm.anm_bones,
                            ),
                        );
                    }
                }
            }
            Err(e) => log::warn!("play-window: party {cslot} action-stream decode: {e:#}"),
        }
        // The four direction-command weapon swings spliced from the equipped
        // sections (runtime slots 0xC..0xF) - per-equipment animations the
        // attack chain stages as anim ids 0x0C..0x0F.
        match legaia_asset::battle_char_assembly::swing_battle_animations(&raw, &pack, &equipped) {
            Ok(swings) => {
                for s in &swings {
                    if let Some(slot) = clips.get_mut(s.slot as usize) {
                        *slot = Some(
                            legaia_asset::battle_char_assembly::expand_animation_for_objects(
                                &s.anim,
                                &asm.anm_bones,
                            ),
                        );
                    }
                    if let Some(face) = faces.get_mut(s.slot as usize) {
                        *face = s.face;
                    }
                }
            }
            Err(e) => log::warn!("play-window: party {cslot} swing decode: {e:#}"),
        }
        // The art-animation bank (record[0] +0x58): each record's keyframe
        // stream resolves through the character's readef.DAT "ME" archive
        // (main slot 3*char+1, base slot 3*char+2 for rate_alt == 0xFF
        // records). The staged-anim commit materializes bank record
        // `id - 0x10` into dynamic slot 0x10/0x11 (FUN_8004AD80).
        let (art_bank, art_faces) = self.party_art_bank(&raw, cslot, &asm.anm_bones);
        Some((asm, uploads, idle, clips, art_bank, faces, art_faces))
    }

    /// Decode one character's art-animation bank into commit-ready clips
    /// plus the records' embedded-entry face tracks (both indexed by bank
    /// record). Failures degrade per record (a `None` slot commits as a
    /// zero-length clip) or to an empty bank with a warning.
    pub(super) fn party_art_bank(
        &self,
        raw: &[u8],
        cslot: usize,
        anm_bones: &[u8],
    ) -> (
        Vec<Option<legaia_asset::monster_archive::MonsterAnimation>>,
        Vec<Option<legaia_asset::face_anim::FaceTracks>>,
    ) {
        let record0 = match legaia_asset::battle_char_assembly::decode_record0(raw) {
            Ok(r) => r,
            Err(e) => {
                log::warn!("play-window: party {cslot} record[0] decode for art bank: {e:#}");
                return (Vec::new(), Vec::new());
            }
        };
        let records = match legaia_asset::battle_char_assembly::art_animation_bank(&record0) {
            Ok(r) => r,
            Err(e) => {
                log::warn!("play-window: party {cslot} art-bank parse: {e:#}");
                return (Vec::new(), Vec::new());
            }
        };
        // The embedded entries' face tracks (record +0xB0 / +0xBC) come
        // straight off the bank records - no ME archive involved.
        let faces: Vec<Option<legaia_asset::face_anim::FaceTracks>> =
            records.iter().map(|r| r.face).collect();
        // readef.DAT (extraction PROT 894) - the battle side-band file whose
        // 0x10800-byte slots carry the per-character "ME" stream archives.
        let readef = match self.session.host.index.entry_bytes_extended(894) {
            Ok(b) => b,
            Err(e) => {
                log::warn!("play-window: readef.DAT (PROT 894) read: {e:#}");
                return (Vec::new(), faces);
            }
        };
        let main = legaia_asset::battle_char_assembly::art_me_archive(&readef, cslot, false);
        let base = legaia_asset::battle_char_assembly::art_me_archive(&readef, cslot, true);
        let mut bank = vec![None; records.len()];
        for rec in &records {
            let archive = if rec.uses_base_archive() {
                &base
            } else {
                &main
            };
            let Ok(archive) = archive else { continue };
            match legaia_asset::battle_char_assembly::art_animation(rec, archive) {
                Ok(anim) => {
                    bank[rec.index] = Some(
                        legaia_asset::battle_char_assembly::expand_animation_for_objects(
                            &anim, anm_bones,
                        ),
                    );
                }
                Err(e) => log::warn!(
                    "play-window: party {cslot} art record {} ({}): {e:#}",
                    rec.index,
                    rec.name
                ),
            }
        }
        (bank, faces)
    }

    /// Spawn the player Seru-magic summon as a battle creature, the faithful
    /// render (the summon reuses its namesake `battle_data` enemy creature -
    /// see `summon::summon_creature_id`). Loads that creature's mesh + texture
    /// (`battle_render_mesh`) into a free battle texture slot and binds it to a
    /// high actor slot with its idle [`MonsterAnimPlayer`], so the existing
    /// battle render animates + textures it exactly like an enemy. Replaces the
    /// move-VM `SummonScene` stand-in for the visual. No-op outside battle.
    pub(super) fn spawn_summon_creature(&mut self, spell_id: u8) {
        if self.session.host.world.mode != SceneMode::Battle {
            return;
        }
        let Some(archive) = self.monster_archive_bytes() else {
            return;
        };
        let Some(creature) = legaia_engine_core::summon::summon_creature_id(spell_id, &archive)
        else {
            return;
        };
        let Some(mut vram) = self.battle_vram.clone() else {
            return;
        };
        let Some(r) = self.win.renderer.as_ref() else {
            return;
        };
        let mesh = match legaia_asset::monster_archive::mesh(&archive, creature) {
            Ok(Some(m)) => m,
            _ => return,
        };
        let Ok(tmd) = legaia_tmd::parse(mesh.tmd_bytes()) else {
            return;
        };
        // Inject the creature texture into the next free battle slot.
        let tex_slot = self.battle_tex_slots_used.min(4);
        let Some(vmesh) = mesh.battle_render_mesh(tex_slot, &mut vram) else {
            return;
        };
        if vmesh.indices.is_empty() {
            return;
        }
        let uploaded = match r.upload_vram_mesh(
            &vmesh.positions,
            &vmesh.uvs,
            &vmesh.cba_tsb,
            &vmesh.normals,
            &vmesh.indices,
        ) {
            Ok(m) => m,
            Err(e) => {
                log::warn!("play-window: summon mesh upload: {e:#}");
                return;
            }
        };
        let idx = self.meshes.len();
        self.meshes.push(uploaded);
        self.scene_tmd_data.push((tmd, mesh.tmd_bytes().to_vec()));
        match r.upload_vram(&vram) {
            Ok(v) => {
                // A mid-battle summon spawn is a legitimate battle-VRAM
                // refresh: re-stamp the expected resident generation.
                self.battle_vram_generation = Some(v.generation());
                self.uploaded_vram = Some(v);
            }
            Err(e) => log::error!("play-window: summon VRAM re-upload: {e:#}"),
        }
        // Keep the stashed battle VRAM in sync with what's now resident, so
        // later battle-VRAM refreshes (the residency heal, the per-frame
        // face stamps) don't drop the injected summon texture.
        self.battle_vram = Some(vram);

        // Seat the summon in a free high actor slot (>= 8) so it never collides
        // with the party/monster battle slots. Place it on the party side
        // (`enter_battle` seats party at negative Z, enemies at positive Z),
        // in front of the party and clearly clear of the enemy cluster, so
        // the battle camera frames it distinct from the enemies it attacks.
        let slot = self
            .summon_actor_slot
            .unwrap_or_else(|| 8 + (self.session.host.world.party_count as usize));
        self.summon_actor_slot = Some(slot);
        if let Some(a) = self.session.host.world.actors.get_mut(slot) {
            a.active = true;
            a.tmd_binding = Some(idx);
            a.battle_tex_slot = Some(tex_slot);
            a.move_state.world_x = 0;
            a.move_state.world_y = 0;
            a.move_state.world_z = -350;
        }
        if let Ok(Some(idle)) = legaia_asset::monster_archive::idle_animation(&archive, creature)
            && let Some(player) = legaia_engine_core::battle_anim::MonsterAnimPlayer::new(&idle)
        {
            self.session
                .host
                .world
                .set_actor_battle_animation(slot, player);
        }
        log::info!(
            "play-window: summon spell {spell_id:#04x} -> battle_data creature {creature} \
             (mesh slot {idx}, tex slot {tex_slot}, actor slot {slot})"
        );
    }

    /// Leave battle: restore the clean field VRAM and drop the appended
    /// battle monster meshes (the field actor table was already restored from
    /// the pre-battle snapshot, so those slots no longer reference them).
    pub(super) fn exit_battle_render(&mut self) {
        if let (Some(r), Some(base)) = (self.win.renderer.as_ref(), self.cpu_vram_base.as_ref()) {
            match r.upload_vram(base) {
                Ok(v) => self.uploaded_vram = Some(v),
                Err(e) => log::error!("play-window: field VRAM restore: {e:#}"),
            }
        }
        let keep = self.battle_mesh_base.min(self.meshes.len());
        self.meshes.truncate(keep);
        self.scene_tmd_data
            .truncate(keep.min(self.scene_tmd_data.len()));
        // Tear down a spawned player-summon creature.
        if let Some(slot) = self.summon_actor_slot.take()
            && let Some(a) = self.session.host.world.actors.get_mut(slot)
        {
            a.active = false;
            a.tmd_binding = None;
            a.battle_tex_slot = None;
            a.battle_animation = None;
            a.pose_frame = None;
        }
        self.battle_vram = None;
        self.battle_vram_generation = None;
        self.battle_tex_slots_used = 0;
        self.battle_stage_mesh = None;
        self.battle_ground_mesh = None;
        self.battle_faces.clear();
    }

    /// Lazily read the static face-frame tables out of the boot source's
    /// `SCUS_942.54` (`legaia_asset::face_anim::FaceFrameTables`). A failed
    /// attempt (disc-free run / unparsable executable) is remembered so the
    /// probe only runs once; facial animation is simply skipped then.
    pub(super) fn load_face_tables(&mut self) {
        if self.face_tables_attempted {
            return;
        }
        self.face_tables_attempted = true;
        use legaia_engine_core::Vfs;
        let scus = if let Some(root) = self.extracted_root.as_deref() {
            legaia_engine_core::DirVfs::new(root)
                .ok()
                .and_then(|v| v.read("SCUS_942.54").ok())
        } else if let Some(disc) = self.disc_path.as_deref() {
            legaia_engine_core::DiscVfs::open(disc)
                .ok()
                .and_then(|v| v.read("SCUS_942.54").ok())
        } else {
            None
        };
        self.face_tables = scus
            .as_deref()
            .and_then(legaia_asset::face_anim::FaceFrameTables::from_scus);
        self.art_mouth_tables = scus
            .as_deref()
            .and_then(legaia_asset::face_anim::ArtMouthTables::from_scus);
        if self.face_tables.is_none() {
            log::info!(
                "play-window: SCUS face-frame tables unavailable - battle faces stay neutral"
            );
        }
    }

    /// Lazily read the spell/seru display-name table from the boot SCUS so the
    /// seru-trade overlay can label each offer. Cached after the first read;
    /// a disc-free run leaves it `None` (offers fall back to "Seru NN").
    pub(super) fn ensure_seru_names(&mut self) {
        if self.seru_names.is_some() {
            return;
        }
        use legaia_engine_core::Vfs;
        let scus = if let Some(root) = self.extracted_root.as_deref() {
            legaia_engine_core::DirVfs::new(root)
                .ok()
                .and_then(|v| v.read("SCUS_942.54").ok())
        } else if let Some(disc) = self.disc_path.as_deref() {
            legaia_engine_core::DiscVfs::open(disc)
                .ok()
                .and_then(|v| v.read("SCUS_942.54").ok())
        } else {
            None
        };
        self.seru_names = scus
            .as_deref()
            .and_then(legaia_asset::spell_names::SpellNameTable::from_scus);
    }

    /// Per-tick battle facial animation: re-stamp each registered party
    /// member's current eye + mouth face frame onto the band's live face
    /// rows, exactly like the retail per-frame animator. The playing clip's
    /// `action_id` selects the member's face tracks, its integer keyframe
    /// cursor is the frame counter, and the selected frames are VRAM-to-VRAM
    /// copies from the band's face-frame strip (`Vram::move_image`, the
    /// `MoveImage` stamp). During the victory window - the battle ended in
    /// a monster wipe while a member still plays a dynamic-art-slot clip
    /// (staged id `0x11..=0x18`, e.g. the killing art) - the mouth records
    /// come from the static `0x80077E80` override table and the animator
    /// clocks on the halved victory counter instead (the win-quote mouth
    /// flap). The GPU texture is only re-uploaded on a frame
    /// whose stamp set differs from the previous one (retail re-issues
    /// identical `MoveImage`s every frame; the visible result is the same).
    // PORT: FUN_80047430 (facial-animator dispatch): per visible party
    // node, call the stamp pass with (band slot, char index, cursor
    // keyframes, playing action entry), skipping char 3 (Terra) and bands
    // >= 3. The stamp-selection half is `FaceFrameTables::stamps_with_art_window`
    // (PORT: FUN_8004C7B4 in legaia_asset::face_anim).
    pub(super) fn tick_battle_face_stamps(&mut self) {
        use legaia_asset::face_anim::{ART_BAND_FIRST, ART_BAND_LAST, ArtMouthOverride};
        use legaia_engine_vm::battle_action::BattleEndCause;
        if self.session.host.world.mode != SceneMode::Battle || self.battle_faces.is_empty() {
            return;
        }
        let Some(tables) = self.face_tables.as_ref() else {
            return;
        };
        let art_tables = self.art_mouth_tables.as_ref();
        let Some(vram) = self.battle_vram.as_mut() else {
            return;
        };
        // Retail gate 1 of the victory-window mouth override: the
        // battle-end signal `DAT_8007BD71 == 0xFE` raised by a monster
        // wipe (the SM `0x5A` arm; the engine mirror is the
        // `BattleActionHost::battle_end` latch). Gates 2/3 - the victory
        // sequencer's phase halfword `ctx+0x6CE != 0` and the celebration
        // flag `DAT_8007BD60 & 0x80` (which the party-wipe path clears) -
        // are the retail victory presentation's internal progress flags;
        // the engine has no victory sequencer, so "the won battle is
        // still on screen" stands in for them. Escapes also raise 0xFE
        // but never set the celebration flag, so they stay excluded.
        let victory_window =
            self.session.host.world.battle_end == Some(BattleEndCause::MonsterWipe);
        let mut changed = false;
        for mf in &mut self.battle_faces {
            let Some(actor) = self.session.host.world.actors.get(mf.actor_slot) else {
                continue;
            };
            // No player yet = the rest pose: behaves like clip frame 0 of
            // the (track-less) idle, i.e. the neutral face.
            let (action_id, frame) = actor
                .battle_animation
                .as_ref()
                .map(|p| (p.action_id(), p.current_frame()))
                .unwrap_or((0, 0));
            // Track source. Staged ids >= 0x10 are art-bank clips: retail
            // materializes bank record `id - 0x10` and installs its
            // embedded entry (record +0x24) as the action-table pointer
            // (FUN_8004AD80), so the animator reads THAT entry's tracks
            // (record +0xB0 eyes / +0xBC mouth). Below the art base, the
            // playing clip's action slot picks the record[0] / swing entry.
            let tracks = if action_id >= legaia_asset::battle_char_assembly::ART_ANIM_ID_BASE {
                mf.art_tracks
                    .get(
                        (action_id - legaia_asset::battle_char_assembly::ART_ANIM_ID_BASE) as usize,
                    )
                    .and_then(|t| t.as_ref())
            } else {
                mf.tracks.get(action_id as usize).and_then(|t| t.as_ref())
            };
            // Retail gate 4: the member's last-staged anim id
            // (`actor[+0x1DB]`; the engine's art-bank clips carry it as
            // their `action_id`) sits in the dynamic-art-slot band
            // `0x11..=0x18`. Open the override window, clocking the
            // member's `gp+0x9EA` mirror from 0; closed, the counter
            // resets.
            let art_mouth = if victory_window
                && (ART_BAND_FIRST..=ART_BAND_LAST).contains(&action_id)
                && let Some(track) = art_tables.and_then(|t| t.track(mf.char_index, action_id))
            {
                let counter = mf.art_counter.unwrap_or(0);
                mf.art_counter = Some(counter.saturating_add(1));
                Some(ArtMouthOverride { track, counter })
            } else {
                mf.art_counter = None;
                None
            };
            // The retail mouth-neutral gate: character-record word `+0xF8`
            // bit 0x2000 = ability bitfield (`+0xF4`) bit 45 (passive 0x2D
            // Rage, Evil Medallion). Read it off the occupying character's
            // rebuilt ability bytes (byte 5 bit 0x20).
            let world = &self.session.host.world;
            let force_neutral_mouth = world
                .roster
                .members
                .get(world.party_roster_slot(mf.actor_slot))
                .map(|m| m.ability_bits()[5] & 0x20 != 0)
                .unwrap_or(false);
            let stamps = tables.stamps_with_art_window(
                mf.char_index,
                mf.actor_slot,
                tracks,
                frame,
                art_mouth,
                force_neutral_mouth,
            );
            if mf.last_stamps.as_deref() != Some(&stamps) {
                for s in &stamps {
                    vram.move_image(s.src_x, s.src_y, s.w, s.h, s.dst_x, s.dst_y);
                }
                mf.last_stamps = Some(stamps);
                changed = true;
            }
        }
        if !changed {
            return;
        }
        if let (Some(r), Some(vram)) = (self.win.renderer.as_ref(), self.battle_vram.as_ref()) {
            match r.upload_vram(vram) {
                Ok(v) => {
                    // A face re-stamp is a legitimate battle-VRAM refresh:
                    // move the expected resident generation along with it.
                    self.battle_vram_generation = Some(v.generation());
                    self.uploaded_vram = Some(v);
                }
                Err(e) => log::error!("play-window: face-stamp VRAM re-upload: {e:#}"),
            }
        }
    }

    /// Residency guard: while a battle texture is expected to be GPU-resident,
    /// verify no other path re-uploaded VRAM over it this frame. The
    /// white-speckle party bug (a background CLUT animator re-uploading the
    /// field snapshot mid-battle) was invisible to every CPU-side VRAM oracle
    /// because they never check *which* upload the draw samples. On a
    /// violation: log loudly, fail debug builds, and self-heal by re-uploading
    /// the stashed battle VRAM.
    pub(super) fn check_battle_vram_residency(&mut self) {
        if self.session.host.world.mode != SceneMode::Battle {
            return;
        }
        let Some(expected) = self.battle_vram_generation else {
            return;
        };
        let current = self.uploaded_vram.as_ref().map(|v| v.generation());
        if current == Some(expected) {
            return;
        }
        log::error!(
            "play-window: battle VRAM clobbered mid-battle (expected upload \
             generation {expected}, GPU holds {current:?}); re-uploading the \
             battle texture"
        );
        debug_assert!(
            false,
            "battle VRAM residency violated: expected generation {expected}, found {current:?}"
        );
        if let (Some(r), Some(vram)) = (self.win.renderer.as_ref(), self.battle_vram.as_ref()) {
            match r.upload_vram(vram) {
                Ok(v) => {
                    self.battle_vram_generation = Some(v.generation());
                    self.uploaded_vram = Some(v);
                }
                Err(e) => log::error!("play-window: battle VRAM re-upload (heal): {e:#}"),
            }
        }
    }
}
