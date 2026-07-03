//! Extracted from `window.rs` (mechanical split; behavior-preserving).

use super::*;

impl PlayWindowApp {
    pub(super) fn upload_assets(&mut self) {
        let Some(res) = self.scene_res.take() else {
            return;
        };
        let (
            vram_opt,
            font_opt,
            meshes,
            tmd_data,
            tmd_src_index,
            color_meshes,
            color_tmd_src_index,
            lo,
            hi,
            world_map_hf,
        ) = {
            let Some(r) = self.win.renderer.as_ref() else {
                self.scene_res = Some(res);
                return;
            };
            let vram = r
                .upload_vram(&res.vram)
                .map_err(|e| log::error!("VRAM upload: {e:#}"))
                .ok();
            let font = r
                .upload_font(&self.font)
                .map_err(|e| log::error!("font upload: {e:#}"))
                .ok();
            let mut meshes = Vec::new();
            let mut tmd_data: Vec<(legaia_tmd::Tmd, Vec<u8>)> = Vec::new();
            // `res.tmds` index for each pushed mesh (meshes skip empty-prim
            // TMDs, so this is the bridge back to a `ResolvedTmd`).
            let mut tmd_src_index: Vec<usize> = Vec::new();
            // Untextured (F*/G*) prop meshes + their res.tmds-index bridge,
            // parallel to `meshes` / `tmd_src_index` but on the colour pipeline.
            let mut color_meshes: Vec<UploadedColorMesh> = Vec::new();
            let mut color_tmd_src_index: Vec<usize> = Vec::new();
            let mut lo = [f32::INFINITY; 3];
            let mut hi = [f32::NEG_INFINITY; 3];
            for (src_i, rtmd) in res.tmds.iter().enumerate() {
                // Build this mesh's untextured (F*/G*) vertex-colour primitives
                // and upload them to the colour pipeline. `tmd_to_color_mesh`
                // skips textured groups, so these are DISJOINT from the VRAM-mesh
                // primitives below: a mesh carrying both textured and untextured
                // primitives now renders BOTH halves rather than only the
                // textured one (the old code built the colour mesh only when the
                // textured build came back empty, silently dropping the
                // untextured half of a mixed mesh). A mesh whose only textured
                // prims reference missing CLUTs yields an empty colour mesh AND
                // an empty filtered VRAM mesh, so it correctly stays dropped.
                let cmesh = legaia_tmd::mesh::tmd_to_color_mesh(&rtmd.tmd, &rtmd.raw);
                if !cmesh.is_empty() {
                    for p in &cmesh.positions {
                        for ax in 0..3 {
                            if p[ax] < lo[ax] {
                                lo[ax] = p[ax];
                            }
                            if p[ax] > hi[ax] {
                                hi[ax] = p[ax];
                            }
                        }
                    }
                    match r.upload_color_mesh_blended(
                        &cmesh.positions,
                        &cmesh.colors,
                        &cmesh.indices,
                        &cmesh.blend,
                    ) {
                        Ok(m) => {
                            color_meshes.push(m);
                            color_tmd_src_index.push(src_i);
                        }
                        Err(e) => log::warn!("color mesh upload skipped: {e:#}"),
                    }
                }

                // Use the VRAM-aware filter so prims whose CBA / TSB sample
                // un-uploaded regions get dropped at mesh-build time. This
                // matches the asset-viewer's cleanup and avoids the "flat
                // green CLUT[0]" shells over correctly-textured geometry
                // that the unfiltered builder produces.
                let vmesh = rtmd.build_filtered_vram_mesh(&res.vram);
                if vmesh.indices.is_empty() {
                    if std::env::var_os("LEGAIA_DIAG_PLACE").is_some() {
                        let (_, stats) = rtmd.build_filtered_vram_mesh_reasoned(&res.vram);
                        let mut missing = std::collections::BTreeSet::new();
                        let _ = legaia_tmd::mesh::tmd_to_vram_mesh_filtered(
                            &rtmd.tmd,
                            &rtmd.raw,
                            |cba, tsb, uvs| {
                                if !res.vram.prim_has_texture_data(cba, tsb, uvs) {
                                    missing.insert((cba, tsb));
                                }
                                false
                            },
                        );
                        let decoded: Vec<String> = missing
                            .iter()
                            .map(|&(cba, tsb)| {
                                format!(
                                    "cba={cba:#x} CLUT({},{}) tsb={tsb:#x} page({},{}) bpp{}",
                                    (cba & 0x3f) * 16,
                                    cba >> 6,
                                    (tsb & 0xf) * 64,
                                    ((tsb >> 4) & 1) * 256,
                                    4 << ((tsb >> 7) & 3)
                                )
                            })
                            .collect();
                        log::info!(
                            "DIAG mesh drop: res {} (entry {} off {:#x}): textured filter empty \
                             ({stats:?}, color mesh empty: {}) wants: {}",
                            src_i,
                            rtmd.entry_idx,
                            rtmd.offset,
                            cmesh.is_empty(),
                            decoded.join(" | ")
                        );
                    }
                    // No textured prims survived the VRAM filter; the colour prims
                    // (if any) were already uploaded above.
                    continue;
                }
                let (mlo, mhi) = vmesh.aabb();
                for ax in 0..3 {
                    if mlo[ax] < lo[ax] {
                        lo[ax] = mlo[ax];
                    }
                    if mhi[ax] > hi[ax] {
                        hi[ax] = mhi[ax];
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
                        tmd_data.push((rtmd.tmd.clone(), rtmd.raw.clone()));
                        meshes.push(m);
                        tmd_src_index.push(src_i);
                    }
                    Err(e) => log::warn!("TMD upload skipped: {e:#}"),
                }
            }
            // Bulk **ground** heightfield: the surface built from the `.MAP`
            // floor grid (the pack meshes are only the placed objects /
            // landmarks, not a per-cell ground mesh), textured per cell from
            // the terrain-type-keyed multi-page atlas - the heightfield
            // bakes the per-cell tile UV (`+0x14`) and page+palette
            // (`+0x15`/`+0x16..+0x18`) so grass / mountain / water cells sample
            // their own VRAM page. See docs/subsystems/world-map.md
            // "Ground texturing".
            //
            // This is NOT world-map-only: town `.MAP`s carry the same
            // `0x1000`-gated ground layer (town01: ~1.9k ground cells vs the
            // 208 `0x2000` decor tiles, most with `record[+0x10] = 0` = no
            // pack mesh), and the retail town VRAM has the terrain atlas
            // pages resident. Without this surface the town floor renders
            // as holes wherever no `0x2000` tile covers a cell.
            let mut world_map_hf: Option<UploadedVramMesh> = None;
            if let Some(scene) = self.session.host.scene.as_ref()
                && let Ok(Some(hf)) = scene.walk_heightfield(&self.session.host.index)
                && !hf.indices.is_empty()
            {
                let vmesh = heightfield_to_vram_mesh(&hf);
                match r.upload_vram_mesh(
                    &vmesh.positions,
                    &vmesh.uvs,
                    &vmesh.cba_tsb,
                    &vmesh.normals,
                    &vmesh.indices,
                ) {
                    Ok(m) => {
                        log::info!(
                            "play-window: ground heightfield {} quads ({} verts)",
                            hf.quad_count(),
                            hf.positions.len()
                        );
                        world_map_hf = Some(m);
                    }
                    Err(e) => log::warn!("heightfield upload skipped: {e:#}"),
                }
            }
            (
                vram,
                font,
                meshes,
                tmd_data,
                tmd_src_index,
                color_meshes,
                color_tmd_src_index,
                lo,
                hi,
                world_map_hf,
            )
        };
        // Kingdom slot-4 inspection wireframe (env-gated). Build the
        // world-space line geometry once at scene load; the per-frame
        // world-map render merges it into the overlay-lines buffer.
        let world_map_slot4_lines = if std::env::var_os("LEGAIA_WORLDMAP_SLOT4").is_some() {
            res.world_map_slot4.as_ref().and_then(|slot| {
                let (p, c, i) = world_map_slot4_line_geometry(slot);
                if i.is_empty() {
                    None
                } else {
                    log::info!(
                        "play-window: world-map slot-4 wireframe {} bodies, {} segments",
                        slot.bodies.len(),
                        i.len() / 2
                    );
                    Some((p, c, i))
                }
            })
        } else {
            None
        };
        // Resolve the field static-geometry placement draws: each placed
        // environment object -> its scene-pack mesh -> a world transform.
        // Built here (not per-frame) because the placement table + pack are
        // fixed for the scene; the field draw branch just replays the list.
        let field_placement_draws = self.resolve_field_placement_draws(&res, &tmd_src_index);
        // Same resolver, but bridged through the colour-mesh list: the untextured
        // props' placement transforms map to `color_meshes` indices.
        let field_placement_color_draws =
            self.resolve_field_placement_draws(&res, &color_tmd_src_index);
        let field_terrain_draws = self.resolve_field_terrain_draws(&res, &tmd_src_index);
        // Untextured ground tiles resolve through the colour-mesh bridge (the
        // textured bridge has no entry for them - they'd render as floor holes).
        let field_terrain_color_draws =
            self.resolve_field_terrain_draws(&res, &color_tmd_src_index);
        log::info!(
            "play-window: {} field terrain draws (ground layer, +{} colour tiles)",
            field_terrain_draws.len(),
            field_terrain_color_draws.len()
        );
        let world_map_terrain_draws = self.resolve_world_map_terrain_draws(&res, &tmd_src_index);
        // Field move-VM stager scene-pack TMD list: `env_tmds` (res.tmds @ the
        // scene_asset_table bundle entry, scan order) = retail `DAT_8007C018[5..]`,
        // indexed by a field stager's relative `model_sel`. Kept as its own clone
        // because `res` is consumed below; the field-FX render path looks meshes up
        // here instead of the battle `global_tmd_pool`.
        let field_stager_tmds: Vec<(legaia_tmd::Tmd, Vec<u8>)> = self
            .session
            .host
            .scene
            .as_ref()
            .and_then(|scene| {
                legaia_engine_core::scene_bundle::find_bundle(scene).map(|b| b.entry_idx())
            })
            .map(|bundle_entry| {
                res.tmds
                    .iter()
                    .filter(|t| t.entry_idx == bundle_entry)
                    .map(|t| (t.tmd.clone(), t.raw.clone()))
                    .collect()
            })
            .unwrap_or_default();
        self.field_stager_tmds = field_stager_tmds;
        // Keep a clean CPU copy of the scene VRAM so a battle can inject
        // monster textures into a throwaway clone and restore this on exit.
        self.cpu_vram_base = Some(res.vram.clone());
        if let Some(v) = vram_opt {
            self.uploaded_vram = Some(v);
        }
        if let Some(a) = font_opt {
            self.font_atlas = Some(a);
        }
        // Upload the publisher-logos atlas (if pre-decoded at boot) now
        // that the renderer is live.
        if let (Some(atlas_data), Some(r)) = (
            self.pending_publisher_logos_atlas.take(),
            self.win.renderer.as_ref(),
        ) {
            match r.upload_sprite_atlas(&atlas_data.rgba, atlas_data.width, atlas_data.height) {
                Ok(atlas) => {
                    log::info!(
                        "play-window: publisher-logos atlas uploaded ({}x{})",
                        atlas_data.width,
                        atlas_data.height
                    );
                    self.publisher_logos = Some(PublisherLogosAssets {
                        rects: atlas_data.rects,
                        atlas,
                    });
                }
                Err(e) => log::warn!("publisher-logos atlas upload skipped: {e:#}"),
            }
        }
        // Upload the title-screen atlas the same way.
        if let (Some(atlas_data), Some(r)) = (
            self.pending_title_screen_atlas.take(),
            self.win.renderer.as_ref(),
        ) {
            match r.upload_sprite_atlas(&atlas_data.rgba, atlas_data.width, atlas_data.height) {
                Ok(atlas) => {
                    log::info!(
                        "play-window: title-screen atlas uploaded ({}x{})",
                        atlas_data.width,
                        atlas_data.height
                    );
                    self.title_screen = Some(TitleScreenAssets {
                        rect: atlas_data.rect,
                        atlas,
                    });
                }
                Err(e) => log::warn!("title-screen atlas upload skipped: {e:#}"),
            }
        }
        // Upload the menu-glyph atlas the same way.
        if let (Some(atlas_data), Some(r)) = (
            self.pending_menu_glyph_atlas.take(),
            self.win.renderer.as_ref(),
        ) {
            match r.upload_sprite_atlas(&atlas_data.rgba, atlas_data.width, atlas_data.height) {
                Ok(atlas) => {
                    log::info!(
                        "play-window: menu-glyph atlas uploaded ({}x{})",
                        atlas_data.width,
                        atlas_data.height
                    );
                    self.menu_glyphs = Some(MenuGlyphAssets { atlas });
                }
                Err(e) => log::warn!("menu-glyph atlas upload skipped: {e:#}"),
            }
        }
        // Upload the save-menu UI atlas (PROT 0899) the same way.
        if let (Some(atlas_data), Some(r)) = (
            self.pending_save_menu_atlas.take(),
            self.win.renderer.as_ref(),
        ) {
            match r.upload_sprite_atlas(&atlas_data.rgba, atlas_data.width, atlas_data.height) {
                Ok(atlas) => {
                    log::info!(
                        "play-window: save-menu atlas uploaded ({}x{})",
                        atlas_data.width,
                        atlas_data.height
                    );
                    let rects = legaia_engine_render::SaveMenuAtlasRects {
                        panel_tl: atlas_data.band_panel_tl(),
                        panel_tr: atlas_data.band_panel_tr(),
                        panel_bl: atlas_data.band_panel_bl(),
                        panel_br: atlas_data.band_panel_br(),
                        panel_top: atlas_data.band_panel_top(),
                        panel_bot: atlas_data.band_panel_bot(),
                        panel_left: atlas_data.band_panel_left(),
                        panel_right: atlas_data.band_panel_right(),
                        slot1: atlas_data.band_slot1(),
                        slot2: atlas_data.band_slot2(),
                        cursor: atlas_data.band_cursor(),
                        panel_interior: atlas_data.band_panel_interior(),
                        load_empty_frame: Some(atlas_data.band_load_empty_frame()),
                        load_portrait_by_char: [
                            atlas_data.band_load_portrait(0),
                            atlas_data.band_load_portrait(1),
                            atlas_data.band_load_portrait(2),
                        ],
                    };
                    self.save_menu = Some(SaveMenuAssets { rects, atlas });
                }
                Err(e) => log::warn!("save-menu atlas upload skipped: {e:#}"),
            }
        }
        self.meshes = meshes;
        self.scene_tmd_data = tmd_data;
        self.field_terrain_draws = field_terrain_draws;
        self.field_terrain_color_draws = field_terrain_color_draws;
        self.field_placement_draws = field_placement_draws;
        self.color_meshes = color_meshes;
        self.field_placement_color_draws = field_placement_color_draws;
        self.world_map_terrain_draws = world_map_terrain_draws;
        self.ground_heightfield = world_map_hf;
        self.world_map_slot4_lines = world_map_slot4_lines;
        // World-map ocean: recover the 13-frame CLUT animation for the kingdom
        // (the ocean texture + base CLUT are already uploaded by the slot-0 TIM
        // pass). `None` off the world map, so the per-tick advance self-gates.
        self.ocean_anim = self.resolve_ocean_anim();
        if lo[0].is_finite() {
            self.scene_aabb = (lo, hi);
        }
        // Bind each uploaded mesh slot to the matching actor and wire up the
        // idle animation (record 0) when the scene carries an ANM pack for
        // that actor. Registration order: actor K → TMD slot K, mirroring
        // the retail `0x8007C018` table written by `FUN_8001E890`.
        let world = &mut self.session.host.world;
        for i in 0..self.scene_tmd_data.len() {
            world.set_actor_tmd_binding(i, i);
            if let Some(pack) = res.anm_pack_for_actor(i)
                && let Some(record_bytes) = pack.record_bytes(0)
            {
                let bone_count = self.scene_tmd_data[i].0.objects.len();
                match AnimPlayer::new(record_bytes.to_vec(), bone_count) {
                    Ok(player) => {
                        world.set_actor_animation(i, player);
                        log::info!("play-window: actor {i} animated ({bone_count} bones)");
                    }
                    Err(e) => log::warn!("play-window: actor {i} ANM init failed: {e:#}"),
                }
            }
        }
        // Bind the PLAYER's real character mesh. Town scripts install the
        // player engine-side (`install_field_player`), not via the field-VM
        // `0x4C 0xD8` spawn, so no `tmd_ref` spawn event ever fires for it -
        // and the naive pre-bind above points actor 0 at whatever scene TMD
        // shares its slot index (usually an init_data UI mesh = invisible
        // player). Resolve the lead's field mesh from the global TMD pool
        // (PROT 0874 §0, retail `DAT_8007C018[0..4]`, seeded by
        // `enter_field_scene`) and override the placeholder binding.
        self.player_color_draw = None;
        world.set_field_player_anim(None);
        if world.mode == SceneMode::Field
            && let Some(pslot) = world.player_actor_slot
        {
            let lead = world.active_party.first().copied().unwrap_or(0) as usize;
            let gtmd = world
                .global_tmd_pool
                .get(lead)
                .and_then(|s| s.as_ref())
                .map(std::sync::Arc::clone);
            if let Some(g) = gtmd {
                // The party locomotion ANM bundle (PROT 0874 §1; idle = bank
                // slot 1, walk = bank slot 0, both pinned live - see
                // `character_pack::LOCOMOTION_IDLE_SLOT` / `_WALK_SLOT`).
                // Banks cover the Vahn/Noa/Gala trio only.
                let locomotion = self
                    .session
                    .host
                    .index
                    .entry_bytes(legaia_asset::character_pack::PROT_ENTRY_INDEX)
                    .ok()
                    .and_then(|b| legaia_asset::character_pack::field_locomotion_anm(&b).ok())
                    .filter(|_| lead <= 2);
                // Rest pose: frame 0 of the character's standing-idle clip.
                // Retail caps the live object count to the clip's bone count
                // (10; groups 10/11 are equipment-swap templates, never drawn
                // - `FUN_8001E890` / `FUN_80024d78`), so truncate the disc
                // TMD's 12-object table to match before posing bone i ->
                // object i.
                // (bone_count, per-bone (translation, rotation) pairs)
                type IdlePose = (usize, Vec<([i16; 3], [i16; 3])>);
                let idle_pose: Option<IdlePose> = locomotion.as_ref().and_then(|bundle| {
                    let rec_idx = legaia_asset::character_pack::locomotion_record_index(
                        lead,
                        legaia_asset::character_pack::LOCOMOTION_IDLE_SLOT,
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
                        &vmesh.indices,
                    ) {
                        Ok(m) => {
                            let new_idx = self.meshes.len();
                            self.meshes.push(m);
                            self.scene_tmd_data
                                .push((player_tmd.clone(), g.raw.clone()));
                            world.set_actor_tmd_binding(pslot as usize, new_idx);
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
                            // Live playback: the idle/walk clip pair, ticked
                            // by the world's field step (locomotion picks the
                            // clip, the posed rebuild consumes `pose_frame`).
                            let anim = locomotion.as_ref().and_then(|bundle| {
                                let rec = |slot| {
                                    legaia_asset::character_pack::locomotion_record_index(
                                        lead, slot,
                                    )
                                };
                                let idle =
                                    legaia_engine_core::field_anim::FieldClipPlayer::from_record(
                                        bundle,
                                        rec(legaia_asset::character_pack::LOCOMOTION_IDLE_SLOT),
                                    )?;
                                let walk =
                                    legaia_engine_core::field_anim::FieldClipPlayer::from_record(
                                        bundle,
                                        rec(legaia_asset::character_pack::LOCOMOTION_WALK_SLOT),
                                    )?;
                                Some(legaia_engine_core::field_anim::FieldPlayerAnim::new(
                                    idle, walk,
                                ))
                            });
                            let animated = anim.is_some();
                            world.set_field_player_anim(anim);
                            log::info!(
                                "play-window: player (roster {lead}) -> pool mesh slot {new_idx} \
                                 ({}{}{})",
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
                    "play-window: global TMD pool has no entry for roster slot {lead}; \
                     player keeps the placeholder binding"
                );
            }
        }
        // Field NPCs + animated props: one draw per visible MAN partition-1
        // placement (see `FieldNpcDraw` for the runtime-pinned model/anim
        // resolution). Actors parked at the (16320, 16320) off-map box are
        // conditional spawns retail hides until a script places them - skip.
        self.field_npc_draws.clear();
        self.npc_clip_players.clear();
        self.npc_anim_srcs.clear();
        if world.mode == SceneMode::Field
            && let Some(r) = self.win.renderer.as_ref()
        {
            // The per-scene ANM bundle in the player-ANM frame-stream layout
            // (the pose source `FUN_8001B964` walks): the type-0x05 section
            // of the scene's first PROT slot (`player_anm::find_in_entry`;
            // field builds don't surface it through `res.anm_packs`).
            let scene_bundle = self.session.host.scene.as_ref().and_then(|s| {
                s.entries.iter().find_map(|e| {
                    legaia_asset::player_anm::find_in_entry(&e.bytes, 3)
                        .into_iter()
                        .next()
                })
            });
            let locomotion_bundle = self
                .session
                .host
                .index
                .entry_bytes(legaia_asset::character_pack::PROT_ENTRY_INDEX)
                .ok()
                .and_then(|b| legaia_asset::character_pack::field_locomotion_anm(&b).ok());
            let placements = self
                .session
                .host
                .scene
                .as_ref()
                .and_then(|s| s.field_man_payload(&self.session.host.index).ok().flatten())
                .and_then(|man| {
                    legaia_asset::man_section::parse(&man).ok().map(|mf| {
                        legaia_engine_core::man_field_scripts::classify_placements(&mf, &man)
                    })
                })
                .unwrap_or_default();
            for (p, _kind) in &placements {
                if p.world_x == 16320 && p.world_z == 16320 {
                    continue;
                }
                let src = if p.special_model {
                    world
                        .global_tmd_pool
                        .get((p.model_index - 0xF0) as usize)
                        .and_then(|s| s.as_ref())
                        .map(|g| (g.tmd.clone(), g.raw.clone()))
                } else {
                    res.tmds
                        .get(p.model_index as usize)
                        .map(|t| (t.tmd.clone(), t.raw.clone()))
                };
                let Some((mut tmd, raw)) = src else {
                    continue;
                };
                let bundle = if p.special_model {
                    locomotion_bundle.as_ref()
                } else {
                    scene_bundle.as_ref()
                };
                // Rest pose: frame 0 of the record the placement's anim byte
                // names; the object table truncates to the clip's bone count
                // (the retail count-equality contract).
                let pose: Option<Vec<([i16; 3], [i16; 3])>> = match (p.anim_id, bundle) {
                    (0, _) | (_, None) => None,
                    (id, Some(b)) => {
                        let rec_idx = (id - 1) as usize;
                        // Live playback: a looping clip player over the same
                        // record poses this NPC per frame (the rest pose
                        // below stays the frame-0 fallback / first frame).
                        if let Some(player) =
                            legaia_engine_core::field_anim::FieldClipPlayer::from_record(b, rec_idx)
                        {
                            self.npc_clip_players.insert(p.index as u8, player);
                        }
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
                let vmesh = match &pose {
                    Some(offsets) => {
                        legaia_tmd::mesh::tmd_to_vram_mesh_posed_rot(&tmd, &raw, offsets)
                    }
                    None => legaia_tmd::mesh::tmd_to_vram_mesh(&tmd, &raw),
                };
                let cmesh = match &pose {
                    Some(offsets) => {
                        legaia_tmd::mesh::tmd_to_color_mesh_posed_rot(&tmd, &raw, offsets)
                    }
                    None => legaia_tmd::mesh::tmd_to_color_mesh(&tmd, &raw),
                };
                let mesh_idx = if vmesh.indices.is_empty() {
                    None
                } else {
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
                    continue;
                }
                if self.npc_clip_players.contains_key(&(p.index as u8)) {
                    self.npc_anim_srcs
                        .insert(p.index as u8, (tmd.clone(), raw.clone()));
                }
                self.field_npc_draws.push(FieldNpcDraw {
                    slot: p.index as u8,
                    mesh_idx,
                    color_idx,
                    spawn: (p.world_x, p.world_z),
                });
            }
            if !self.field_npc_draws.is_empty() {
                log::info!(
                    "play-window: {} field NPC/prop draws ({} placements)",
                    self.field_npc_draws.len(),
                    placements.len()
                );
            }
        }
        // Initial floor snap: locomotion only re-samples the floor height on a
        // step, so an actor standing still since spawn keeps `world_y = 0`
        // while the town ground sits at a LUT-elevated tier - it renders
        // buried under (or floating over) the terrain until it first moves.
        // Snap every bound actor that still has the flat default.
        if world.follow_terrain_height && world.mode == SceneMode::Field {
            for i in 0..world.actors.len() {
                if world.actors[i].tmd_binding.is_none() {
                    continue;
                }
                let ms = &world.actors[i].move_state;
                if ms.world_y != 0 {
                    continue;
                }
                let y = world.sample_field_floor_height(ms.world_x as i32, ms.world_z as i32);
                world.actors[i].move_state.world_y = y as i16;
            }
        }
        log::info!(
            "play-window: {} meshes uploaded, VRAM {}",
            self.meshes.len(),
            if self.uploaded_vram.is_some() {
                "ready"
            } else {
                "failed"
            }
        );
    }
}
