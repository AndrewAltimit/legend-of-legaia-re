//! Scene entry: `enter_field` and its JsValue-free core, the New Game slate,
//! and the page-facing render state rebuilt for the entered scene (map, NPC
//! clips, player seat and rig).

use super::*;

#[wasm_bindgen]
impl LegaiaRuntime {
    /// Boot a named CDNAME scene (e.g. `"town01"`) and assemble everything the
    /// page draws. This is the real field entry: the scene's assets, the
    /// walkability grid + elevation overrides, the MAN system script, the player
    /// install, the encounter session. World-map labels (`map01`..`map03`) route
    /// through the world-map entry, which installs the overworld controller
    /// instead.
    ///
    /// Returns the same JSON as [`Self::state_json`]. Throws when the disc isn't
    /// loaded or the label is unknown.
    pub fn enter_field(&mut self, name: &str) -> Result<String, JsValue> {
        self.enter_field_core(name, false)
            .map_err(|e| JsValue::from_str(&e))
    }

    /// Establish a fresh New Game slate - the browser twin of the native
    /// `BootSession::begin_new_game`: `World::begin_new_game` (flags, money,
    /// bag, clock, pending transitions) plus the SCUS starting party + bag.
    /// The page used to enter `opdeene` without this, so after a Continue,
    /// a card import or a picker visit a "New game" kept the old roster,
    /// gold and story flags.
    pub fn begin_new_game(&mut self) {
        // With a disc loaded this is the session's own slate
        // (`BootSession::begin_new_game_slate`: the seeded world, and the
        // title theme stopped so the prologue's own BGM or its scripted
        // silence owns the audio).
        if let Some(session) = self.scene_host.session_mut() {
            session.begin_new_game_slate();
            return;
        }
        if let Some(d) = self.scene_host.director_mut() {
            use legaia_engine_scene_host::scene_host::BgmDirector;
            d.stop();
        }
        self.world.begin_new_game();
    }
}

impl LegaiaRuntime {
    /// JsValue-free body of [`Self::enter_field`]. `resume` marks a card
    /// load's entry ([`crate::resume`]), whose save the shared kernel
    /// applies after this returns; `false` for a picker / door /
    /// opening-chain entry.
    pub(crate) fn enter_field_core(&mut self, name: &str, resume: bool) -> Result<String, String> {
        let host = self
            .scene_host
            .host_mut()
            .ok_or_else(|| "enter_field: call load_disc first".to_string())?;
        // Faithful-play arming, matching the native play-window's flags:
        // dialogue through the field VM (so branch handlers - flag sets,
        // GIVE_ITEM, scene changes - actually execute), retail's leading-edge
        // wall footprint, solid NPC bodies, per-step terrain follow, and NPCs
        // walking their MAN-authored routes.
        host.world.toggles.use_vm_dialogue = true;
        // The page lands its drawn frame for a framebuffer-reading `43 12`
        // copy (`play_land_frame_grab`), as the native window does.
        host.world.enable_frame_grab(true);
        host.world.locomotion.follow_terrain_height = true;
        host.world.locomotion.leading_edge_wall_probes = true;
        host.world.npcs.solid = true;
        host.world.npcs.animate = true;
        // Free-roam story staging for PICKER entries only - the engine's one
        // rule (`World::stage_picker_entry`), shared with the native
        // `--scene` entry. The opening chain's legs and the prologue skip's
        // `town01` re-enter through here too, and a card Load's resume is not
        // a picker visit either.
        host.world.stage_picker_entry(name, resume);
        // A picked scene seats the party where the disc's doors land them
        // (`SceneHost::arm_picker_entry_seat`, the native `--scene` entry's
        // rule too). The arm stands down for the opening chain's legs and a
        // cutscene-owned entry; a card Load's resume carries its own seat.
        if !resume {
            host.arm_picker_entry_seat(name);
        }
        // The page's runtime twins of the native launch switches
        // (`crate::play_toggles`).
        self.apply_page_toggles();
        let host = self
            .scene_host
            .host_mut()
            .ok_or_else(|| "enter_field: call load_disc first".to_string())?;
        let world_map = legaia_engine_scene::scene::is_world_map_scene(name);
        if world_map {
            host.enter_world_map_scene(name)
                .map_err(|e| format!("enter_field({name}): {e:#}"))?;
            // A picked overworld starts in walk mode with the retail top-view
            // debug camera reachable through its own chord
            // (`_DAT_8007B98C`); a Load or a New Game does not arm it. The
            // engine's one rule, which the native `--scene` entry asks too.
            if !resume {
                host.world.arm_picker_world_map_debug();
            }
        } else {
            host.enter_field_scene(name, 0)
                .map_err(|e| format!("enter_field({name}): {e:#}"))?;
        }
        // The scene host cleared the WORLD's camera state (timeline, op-0x45
        // params); this is the engine camera's half, which only an in-world
        // `SceneEntered` tick otherwise runs. Without it a scene picked
        // mid-cutscene kept the interrupted shot's globals and focus latch
        // and was framed by the old scene's camera. The native
        // `BootSession::enter_field_live` is the paired site; the glide
        // interpolator is this host's own and goes with it.
        self.scene_host.camera_mut().reset_for_scene_entry();
        self.cutscene_glide.reset();
        if !world_map {
            // Retail reaches the field through the mode table, not through a
            // call: whoever wants the field stores `MAIN INIT` (2) and mode
            // 2's handler stages the field overlay, calls the per-scene
            // initializer and hands the word to `MAIN MODE` at `0x80025E50`.
            // `BootSession::enter_field_live` performs exactly this pair, and
            // the overworld's entry (`enter_world_map_live`) performs
            // neither - so the branch above is the same branch the native
            // host takes. Without it this page's word arrived at `MAIN MODE`
            // through `adopt_world_mode` alone, one frame late and with no
            // INIT frame in the trace at all.
            let plan = self.seat_enter(legaia_engine_mode::mode::GameMode::MainInit);
            debug_assert!(
                matches!(plan, Some(legaia_engine_mode::mode::ModeInitPlan::Stage(st))
                    if st.overlay_entry == legaia_engine_title::title_overlay::FIELD_SCENE_INIT_PC),
                "MAIN INIT's plan should name the per-scene initializer"
            );
        }
        if self.live_battles {
            self.arm_live_battles(name);
        }
        self.rebuild_render_state().map_err(js_error_text)?;
        // The seat heuristic is for interactive free-roam entry; the opening
        // chain's cutscene legs stage their own tableau (the timeline owns
        // actor placement) and must not have the anchor relocated under it.
        // A card Load's resume is not free-roam entry either: the party
        // stands where the save says (`SceneHost::arm_resume_seat`), as on
        // the native window, which has no relocation at all. Relocating it
        // moved five of the library saves' parties across their maps.
        let in_opening = self.scene_host.host().is_some_and(|h| {
            h.world.cutscene.opening_chain_active || h.world.cutscene_timeline_active()
        });
        if !in_opening && !resume {
            self.seat_player();
        }
        // A deliberate scene boot restages BGM from scratch: clear the dedupe
        // latch, so the scene's own op-`0x35` start is honoured (and re-stages
        // its track's bank) even if it names the track already playing. No
        // scene bank is staged: retail loads a bank only with its track
        // (`legaia_engine_scene_host::scene_host::SCENE_LOCAL_BGM_FALLBACK_ID`).
        // New scene -> drop any SFX cues still queued for the old one (the
        // native boot's `clear_sfx` on scene entry).
        self.on_scene_change_audio();
        if let Some(d) = self.scene_host.director_mut() {
            d.last_started = None;
        }
        // A card Load's score hand-off lands here: stop whatever was
        // sounding (the title theme), then restore the track the entry
        // started, so the scene's own start of that track finds it already
        // sounding.
        self.run_pending_bgm_handoff();
        Ok(self.state_json())
    }
}

/// A `JsValue` error as text, without touching the JS runtime off-wasm
/// (where every `JsValue` accessor panics) - the testable cores return
/// `String` errors.
fn js_error_text(e: JsValue) -> String {
    #[cfg(target_arch = "wasm32")]
    {
        e.as_string()
            .unwrap_or_else(|| "rebuild_render_state failed".to_string())
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = e;
        "rebuild_render_state failed".to_string()
    }
}

impl LegaiaRuntime {
    /// Rebuild the page-facing render state for the scene the host now holds:
    /// the assembled map, the lead's posed mesh, the NPC catalog. Runs on scene
    /// entry and on every door the engine walks through.
    pub(super) fn rebuild_render_state(&mut self) -> Result<(), JsValue> {
        // A scene swap drops the previous scene's script-spawned actors
        // with it (the MAN loader's retire sweep); the page re-uploads
        // whatever the new scene spawns.
        self.pending_dynamic_mesh_slots.clear();
        self.dynamic_mesh_slots.clear();
        self.scene_aabb = None;
        self.dynamic_mesh_cur = None;
        self.field = None;
        self.player = None;
        self.actors.clear();
        self.scene_anm = None;
        self.locomotion_anm = None;
        self.field_vram_anim = None;
        let Some(host) = self.scene_host.host() else {
            return Ok(());
        };
        let (Some(scene), Some(res)) = (host.scene.as_ref(), host.resources.as_ref()) else {
            return Err(JsValue::from_str(
                "enter: the scene loaded but built no resources",
            ));
        };
        let name = scene.name.clone();
        let is_world_map = legaia_engine_scene::scene::is_world_map_scene(&name);
        let follow = host.world.object_floor_follow_records();
        self.field = Some(crate::play::build_field_render(
            &host.index,
            scene,
            res,
            is_world_map,
            &host.world.hidden_object_records(),
            &host.world.object_render_scales(),
            &|r, x, z| host.world.object_floor_follow_y(&follow, r, x, z),
        ));
        // Volumetric fog: where the ground is, so the bank lies on it and
        // spills off its rims - the call the native window makes at its
        // scene load (`install_fog_ground`), over the same static draws:
        // the story-hidden placements already gone, each at its render scale.
        if let Some(host) = self.scene_host.host_mut()
            && let (Some(scene), Some(res)) = (host.scene.as_ref(), host.resources.as_ref())
            && let Some(fr) = self.field.as_ref()
        {
            let field = legaia_engine_scene::fog_ground::build_for_scene(
                &fr.terrain,
                &fr.placements,
                &fr.placement_scales,
                res,
                scene,
                &host.index,
            );
            host.world.fog_volume.set_ground_field(&scene.name, field);
        }
        // The scene's actors, bound to the world by the step the native
        // window runs too (`scene_actors::bind_scene_actors`): the lead's
        // locomotion clips, the NPC clip cursors, the floor snap. It hands
        // back the pose sources (the scene bundle poses the MAN NPCs and the
        // bound placed props; the locomotion bundle the global-pool
        // specials), the NPC catalog and the clip players the actor layer
        // draws from; the lead's mesh is this page's own.
        self.bind_scene_actors();
        self.build_player_rig();
        // CLUT-walk shimmer (water / waterfall scenes): parse the bundle's
        // type-6 walker table and park its source strips into the host's
        // VRAM - this must land before the page's post-entry
        // `field_vram_bytes` upload, which is why it lives in the rebuild.
        // The ambient move-VM tree needs no sibling here: the scene host
        // spawned it into the live world at scene entry, and
        // `step_field_vram_fx` drains it against the same VRAM.
        self.field_vram_anim = None;
        if let Some(host) = self.scene_host.host_mut()
            && let (Some(scene), Some(res)) = (host.scene.as_ref(), host.resources.as_mut())
        {
            let frame_step = host.world.clock.frame_step.max(1);
            // One resolve + park for every host
            // (`legaia_engine_scene::clut_walk_anim::ClutWalkAnim::install`):
            // the type-6 table on a field scene, the kingdom's slot-5 table
            // on an overworld, the Drake complement rows, and the legacy
            // ocean-head fallback.
            if let Some(install) = legaia_engine_scene::clut_walk_anim::ClutWalkAnim::install(
                scene,
                &host.index,
                &mut res.vram,
            ) {
                if install.ocean_fallback {
                    crate::console_log(
                        "play: no slot-5 CLUT-walk table in the kingdom bundle; \
                         falling back to the legacy ocean-head cycle",
                    );
                }
                for (x, y) in &install.missing_cells {
                    crate::console_log(&format!(
                        "play: CLUT-walk source cell ({x}, {y}) has no VRAM data \
                         (strip residency gap)"
                    ));
                }
                self.field_vram_anim = Some(crate::field_scene_anim::FieldSceneAnim::clut_only(
                    install.anim,
                    frame_step,
                ));
            }
        }
        Ok(())
    }

    /// Bind the entered scene's actors
    /// ([`legaia_engine_session::scene_actors::bind_scene_actors`]) and
    /// adopt what it resolved into the actor layer
    /// ([`crate::field_actors::FieldActors::adopt`]).
    fn bind_scene_actors(&mut self) {
        let Some(host) = self.scene_host.host_mut() else {
            self.actors.clear();
            return;
        };
        let mut b = legaia_engine_session::scene_actors::bind_scene_actors(host);
        self.scene_anm = b.scene_anm.take();
        self.locomotion_anm = b.locomotion_anm.take();
        self.actors.adopt(b);
    }

    /// Put the player somewhere they can actually stand.
    ///
    /// Scene entry seats them at the retail **cold-boot spawn** - the fixed
    /// camera-window centre `FIELD_COLD_SPAWN_XZ` that `FUN_801D6704` uses on a
    /// non-warp entry. That is the right answer for `town01`, the one scene
    /// retail cold-boots into; every other scene is normally *entered through a
    /// door*, which overrides X/Z with the transition's entry tile. Dropping into
    /// one from the scene picker has no door to supply that, so the engine seats
    /// it on a door arrival other scenes' `0x3F` ops name
    /// (`SceneHost::arm_picker_entry_seat`) and this pass leaves that seat
    /// alone. A scene no door leads into keeps the cold spawn, which can land
    /// outside the map entirely (a cave whose floor is nowhere near it).
    ///
    /// So: keep the cold spawn when it is walkable and inside the scene's
    /// populated area, and otherwise fall back to the walkable terrain tile
    /// closest to the middle of that area. Then sample the floor under the final
    /// position - the locomotion step is what normally does that, so without it
    /// the first frame would draw the character sunk into an elevated tier.
    fn seat_player(&mut self) {
        let Some(host) = self.scene_host.host_mut() else {
            return;
        };
        // A door arrival the engine seated the picker entry on is a
        // retail-authored landing it has already validated against the
        // walkability grid; this page-side heuristic is only for the scenes
        // that fell back to the cold-entry seat.
        if matches!(
            host.picker_seat(),
            Some(legaia_engine_scene_host::scene_host::PickerSeat::Arrival { .. })
        ) {
            return;
        }
        let Some(slot) = host.world.player_actor_slot.map(|s| s as usize) else {
            return;
        };
        // Candidate seats, each `(x, z, drawn_y)`.
        //
        // The **walk-ground heightfield** is the scene's actual floor, so it is
        // the first choice: a cave's terrain-*tile* meshes are its rock walls,
        // not its ground, and seating on one of those buries the player inside a
        // boulder. Scenes with no resolvable floor grid fall back to the tile /
        // placement draws.
        //
        // Each candidate carries the height it is *drawn* at, which is not always
        // what the floor sampler reports (a scene whose floor lives in its
        // meshes rather than in the floor-height LUT samples as 0), so the seat
        // prefers the sampler and falls back to the drawn height.
        let tiles: Vec<(i32, i32, i32)> = self
            .field
            .as_ref()
            .map(|f| match f.ground.as_ref() {
                // Every 16th vertex is plenty: the grid is 128-unit tiles and a
                // 4000-quad heightfield would otherwise cost a needless scan.
                Some(hf) => hf
                    .positions
                    .iter()
                    .step_by(16)
                    .map(|p| (p[0] as i32, p[2] as i32, p[1] as i32))
                    .collect(),
                None => f
                    .terrain
                    .iter()
                    .chain(f.placements.iter())
                    .map(|d| (d.world_x, d.world_z, d.world_y))
                    .collect(),
            })
            .unwrap_or_default();
        let (sx, sz) = match host.world.actors.get(slot) {
            Some(a) => (a.move_state.world_x as i32, a.move_state.world_z as i32),
            None => return,
        };
        let mut x = sx;
        let mut z = sz;
        let mut y = host.world.sample_field_floor_height(sx, sz);
        if !tiles.is_empty() {
            let dist2 = |a: (i32, i32), b: (i32, i32)| {
                let (dx, dz) = ((a.0 - b.0) as i64, (a.1 - b.1) as i64);
                dx * dx + dz * dz
            };
            let n = tiles.len() as i64;
            let cx = (tiles.iter().map(|t| t.0 as i64).sum::<i64>() / n) as i32;
            let cz = (tiles.iter().map(|t| t.1 as i64).sum::<i64>() / n) as i32;
            // "Inside the map" = the cold spawn has floor under it. Measuring
            // that against the *nearest* ground tile, not the map centre, keeps
            // a perfectly good spawn near the edge of a big scene - and moving a
            // player who did not need moving is not free: the relocation target
            // can be a walk-on trigger tile (a town exit), which the engine would
            // fire the moment the first tick crosses onto it.
            let nearest = tiles
                .iter()
                .map(|&t| dist2((t.0, t.1), (sx, sz)))
                .min()
                .unwrap_or(i64::MAX);
            let cold_ok =
                !host.world.field_tile_is_wall(sx as i16, sz as i16) && nearest < 1200 * 1200;
            if !cold_ok {
                let mut best: Option<((i32, i32, i32), i64)> = None;
                for &t in &tiles {
                    if host.world.field_tile_is_wall(t.0 as i16, t.1 as i16) {
                        continue;
                    }
                    // Never seat onto a walk-on trigger tile: the first tick
                    // would fire it, and the scene the player just picked would
                    // warp out from under them (a town exit does exactly this).
                    if host.tile_has_walk_on_trigger(t.0 as i16, t.1 as i16) {
                        continue;
                    }
                    let d = dist2((t.0, t.1), (cx, cz));
                    if best.is_none_or(|(_, bd)| d < bd) {
                        best = Some((t, d));
                    }
                }
                if let Some(((bx, bz, by), _)) = best {
                    x = bx;
                    z = bz;
                    // Prefer the sampler when it has an answer (a town's floor
                    // tiers do come from the LUT); fall back to the height the
                    // tile is actually drawn at.
                    let sampled = host.world.sample_field_floor_height(bx, bz);
                    y = if sampled != 0 { sampled } else { by };
                }
            }
        }
        if let Some(p) = host.world.actors.get_mut(slot) {
            p.move_state.world_x = x as i16;
            p.move_state.world_z = z as i16;
            p.move_state.world_y = y as i16;
        }
    }

    /// Resolve the lead's field-form mesh out of the global TMD pool (PROT 0874
    /// §0, seeded by `enter_field_scene`). The clips the world ticks into the
    /// player actor's `pose_frame` are the world's, bound by
    /// `scene_actors::bind_lead_rig`.
    ///
    /// Mirrors the native play-window's player bind: the disc TMD's object table
    /// is truncated to the clip's bone count (retail caps the live object count
    /// at 10 - groups 10/11 are equipment-swap templates and are never drawn), so
    /// bone `i` poses object `i`.
    /// REF: FUN_8001E890
    pub(super) fn build_player_rig(&mut self) {
        let Some(host) = self.scene_host.host_mut() else {
            return;
        };
        // The overworld draws the lead's field form too (retail walks the
        // same mesh across the continent; the native window binds it on
        // both), so a kingdom scene keeps the rig.
        if !matches!(host.world.mode, SceneMode::Field | SceneMode::WorldMap) {
            return;
        }
        let roster_lead = usize::from(host.world.field_leader());
        // The lead's field form, or the model a `CC F8 50` re-staged the
        // player onto - one resolution for both hosts
        // (`SceneHost::player_rig_mesh`).
        let Some(g) = host.player_rig_mesh() else {
            crate::console_log(&format!(
                "play: no rig mesh for roster slot {roster_lead} (model {:?})",
                host.world.locomotion.player_live_model
            ));
            return;
        };
        // The party locomotion bundle (PROT 0874 §1) banks the Vahn / Noa / Gala
        // trio only. The bone cap follows the MESH's slot (a scene-bank model
        // has none); the clip player stays the roster lead's, whose settle
        // pick binds scene records itself while the party-bank bit is down.
        let locomotion_bank = host
            .index
            .entry_bytes(legaia_actor_formats::character_pack::PROT_ENTRY_INDEX)
            .ok()
            .and_then(|b| legaia_actor_formats::character_pack::field_locomotion_anm(&b).ok());
        let lead = g.party_slot.unwrap_or(usize::MAX);
        let rec = |slot| legaia_actor_formats::character_pack::locomotion_record_index(lead, slot);
        let bones = locomotion_bank
            .as_ref()
            .filter(|_| lead <= 2)
            .and_then(|bundle| {
                let idx = rec(legaia_actor_formats::character_pack::LOCOMOTION_IDLE_SLOT);
                bundle.record(idx).ok().map(|r| r.bone_count as usize)
            });
        let mut tmd = g.tmd.clone();
        if let Some(b) = bones {
            tmd.objects.truncate(b);
        }
        let (base, object_ids, shading) =
            legaia_tmd::mesh::tmd_to_vram_mesh_field_hybrid(&tmd, &g.raw);
        if base.indices.is_empty() {
            crate::console_log("play: the lead's field mesh has no renderable prims");
            return;
        }
        let flat = crate::packet_color::hybrid(&base, &shading);
        let posed = Vec::with_capacity(base.positions.len() * 3);
        self.player = Some(PlayerRig {
            base,
            object_ids,
            flat,
            posed,
        });
        self.player_rig_gen = self.player_rig_gen.wrapping_add(1);
    }
}
