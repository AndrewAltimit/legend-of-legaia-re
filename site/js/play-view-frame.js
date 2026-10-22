/* play-view-frame.js - PlayView's frame.
 *
 * Part of the PlayView class (play-app.js): _frame (input, engine tick,
 * camera, the field draw), the screen-primitive and fog-volume passes, the
 * renderer state staged once ahead of every draw branch (_stageFrameState),
 * the scene clear and the field effect draws.
 *
 * Classic script. Requires play-view-constants.js and play-app.js (which
 * also holds ScreenPrimPass) first. */
(function () {
  'use strict';

  const {
    padMaskOf, azimuthUnits, PLAYER_MESH_ID, A2R, TILE_MESH_BASE,
    DYN_ACTOR_MESH_BASE, BATTLE_FX_BILLBOARD_MESH, IDENTITY_MODEL,
    FIELD_FX_PACK_MESH_BASE, BATTLE_FX_MODEL_BASE,
  } = window.LegaiaPlayShared;
  const ScreenPrimPass = window.LegaiaScreenPrimPass;

  window.LegaiaUtil.installMethods(window.PlayView, class {

    /* One display frame's sim ticks through the engine's `play_frame_ticks`,
     * then the page's follow-up its report names. Returns `false` after an
     * engine trap (the loop is already stopped). */
    _frameTicks(rt, steps) {
      /* The page speaks over the engine camera's compass azimuth only where
       * it has something the engine cannot know: the VR gaze, and its own
       * orbit vantage (`F3`, or a frame the engine handed no matrix). */
      let pad = this.pad;
      if (this._vrDrive) {
        rt.set_camera_azimuth(this._vrDrive.azimuth);
        pad = this.pad | this._vrDrive.pad;
      } else if (this.debugCamera || !this.cam.vp) {
        rt.set_camera_azimuth(azimuthUnits(this.cam.yaw));
      }
      let r;
      try { r = JSON.parse(rt.play_frame_ticks(steps, padMaskOf(this.pulse), pad)); } catch (e) {
        this._onEngineTrap('engine tick', e);
        return false;
      }
      /* A frame that ran a tick spent its edges; one that ran none keeps
       * them for the next. */
      if (r.ticks > 0) {
        this.pulse.clear();
        this._repack();
      }
      if (r.menu_opened) this._ensureMenuBlitters();
      if (r.name_committed) this._nameCommitted();
      this._serveXaStage(rt);
      /* The post-tick presentation state: the grade, tint and depth cue
       * this frame stages are the ones the ticks left. */
      if (typeof rt.play_cutscene_state_json === 'function') {
        try { this._cut = JSON.parse(rt.play_cutscene_state_json()); }
        catch (e) { /* keep the pre-tick read */ }
      }
      /* The engine entered a scene (the intro skip's hand-off, a door): its
       * geometry swapped under the page. */
      if (r.scenes && r.scenes.length) {
        try {
          this.scene = r.scenes[r.scenes.length - 1];
          this._rebuild();
          if (this.vr) this.vr.respawn();
          if (this.opts.onScene) for (const sc of r.scenes) this.opts.onScene(sc);
        } catch (e) {
          this._onEngineTrap('scene rebuild', e);
          return false;
        }
      } else if (r.player_rig_rebuilt) {
        /* The lead's rig swapped mid-scene (cave01 hands the party to
         * Noa): re-upload its whole geometry, not just the pose. */
        try { this._uploadPlayer(); } catch (e) {
          this._onEngineTrap('player rig upload', e);
          return false;
        }
      }
      /* The party wipe returned to the title (the engine started the title
       * theme): hand back to the page, which re-runs the boot title. */
      if (r.quit && this.opts.onQuit) this.opts.onQuit();
      if (r.menu_ticked) this._afterMenuTicks();
      return true;
    }

    _frame(skipDraw) {
      const rt = this.rt;
      /* Gamepad buttons enter the same held / pulse sets the keyboard
       * fills, so every reader below (menu, name entry, field tick) sees
       * one pad word whichever device pressed it. */
      if (window.legaiaPollGamepad && window.legaiaPollGamepad(this.held, this.pulse)) {
        this._repack();
      }
      const stepping = this.stepOnce;
      const advance = !this.paused || stepping;
      this.stepOnce = false;

      /* Opening-chain / cutscene presentation state (narration crawl, title
       * card, prologue grade, intro-skip availability). Read before the menu
       * and the ticks: while the timeline owns the scene the pad is frozen
       * and the pause menu stays shut, exactly as the native window gates. */
      this._cut = null;
      if (typeof rt.play_cutscene_state_json === 'function') {
        try { this._cut = JSON.parse(rt.play_cutscene_state_json()); }
        catch (e) { this._cut = null; }
      }

      /* FMV beats (STR / MDEC): the engine holds the world in
       * `SceneMode::Cutscene` while a movie is open; the movie lane slices
       * the segment out of the page's disc bytes, installs it, draws the
       * frames over the GL view and finishes the beat. The sim keeps
       * ticking - the hold is engine-side. */
      if (window.LegaiaPlayFmv) {
        try { window.LegaiaPlayFmv.service(rt, this.canvas.parentNode); }
        catch (e) { console.warn('play fmv', e); }
      }

      /* Drain this display frame's SIM steps once, before anything reads
       * them. Whichever arm owns the frame spends them - the field tick
       * below, or a modal overlay that freezes it - which is the native
       * window's own shape: it drains its ticks first and each tick routes
       * to whichever arm is active.
       *
       * THE DENOMINATION LAW, and it is shared with the native host: one
       * `World::tick` advances the simulation by exactly ONE RETAIL DISPLAY
       * FRAME. How many ticks a display frame runs is the engine's rule, not
       * this page's: `play_drain_sim_steps` is the same kernel the native
       * redraw drains through (`engine-core::frame_step::SimStepper` - whole
       * 1/60 s ticks, at most four a frame, and a longer gap dropped rather
       * than carried). Two local spellings of that rule drifted once already
       * (the native one carried its backlog, this one dropped it) - see
       * `docs/tooling/host-drift.md` and the units-per-second oracle
       * `crates/engine-core/tests/sim_cadence_wall_speed.rs`
       * (retail walk = 480 world units/second, run = 720). The JavaScript
       * accumulator below survives only for a cached wasm bundle that
       * predates the export. */
      let simSteps = 0;
      const shared = typeof rt.play_drain_sim_steps === 'function';
      if (advance) {
        const TICK_DT = 1000 / 60;
        const now = performance.now();
        if (stepping) {
          simSteps = 1;
          this._simAccum = 0;
          if (shared) rt.play_resync_sim_clock();
        } else if (shared) {
          simSteps = rt.play_drain_sim_steps(now - this._simLast);
        } else {
          this._simAccum += now - this._simLast;
          if (this._simAccum > TICK_DT * 4) this._simAccum = TICK_DT * 4;
          simSteps = Math.floor(this._simAccum / TICK_DT);
          this._simAccum -= simSteps * TICK_DT;
        }
        this._simLast = now;
      }

      /* The screens the engine has up need their blitters staged. */
      this._stageScreenBlitters();
      if (advance) {
        /* The frame's ticks, owner by owner, in the engine
         * (`play_frame_ticks` over `BootSession::claim_tick`, the order the
         * native window's `sim_tick` hands its ticks through): a host
         * screen (the pause menu, the party-wipe hold), the naming prompt,
         * the intro skip, the pause press, a menu-overlay screen, then the
         * field. What follows here is the page's own part. */
        if (!this._frameTicks(rt, simSteps)) return;
      } else {
        /* Keep the sim clock current while paused so unpausing doesn't dump the
         * accumulated wall-clock gap as a burst of catch-up ticks. */
        this._simLast = performance.now();
        /* Paused only - a modal overlay above already spent this frame's
         * drain, and its sub-tick remainder must carry or an overlay stepped
         * by `simSteps` would never step on a display above 60 Hz. */
        if (!advance && typeof rt.play_resync_sim_clock === 'function') {
          rt.play_resync_sim_clock();
        }
      }

      /* The scene's floor-height ladder is script-animated (field-VM op `0x4C`
       * nibble-9): fold its live rungs into the baked static draws so the drawn
       * ground undulates with the walk heightfield. Costs one WASM call per
       * frame and nothing else on a scene whose script never moves the ladder. */
      this._applyObjectLive(rt);
      this._applyFloorWave(rt);
      this._applyObjectParked(rt);
      this._applyObjectModels(rt);
      this._applyObjectTints(rt);
      this._applyGroundWave(rt);

      /* A camera re-centre this frame may have re-planned the windowed
       * static-object list. */
      this._syncStaticWindow(rt);

      /* Retail's visible-tile crop may have moved with the camera. */
      this._syncViewWindow(rt);
      this._syncFieldLight(rt);
      this._syncPlacementCull(rt);

      /* A script may have re-bound an NPC's mesh this frame. */
      this._rebindLiveNpcModels(rt);

      /* The per-frame READ of the engine's live pose + NPC transforms runs the
       * WASM engine too, so a trap here poisons the instance exactly like the
       * tick does - and, being outside the tick's guard, would otherwise escape
       * uncaught and freeze the loop without ever reaching recovery. Guard the
       * whole draw-build so any engine trap routes through `onError`. */
      try {
      /* Battle 3D scene: while a random encounter owns the world, the battle
       * layer (backdrop dome drawn twice, ground grid, monster + party
       * battle forms under the shared phase-scripted retail camera) replaces
       * the field draw list - the browser twin of the native redraw's
       * battle branch. `_battleFrame` returns false outside battle (and
       * restores the field VRAM texture on the exit edge), handing the
       * frame back to the field path in the `else`. The text HUD overlay
       * further below runs either way, on top. */
      /* Occlusion-fade focus is per-frame state: drop it before whichever
       * mode draws, and let the field branch below re-stage it. Battle and
       * VR first-person never fade (battle frames its own subjects; in
       * first-person the eye IS the player, nothing can sit between). */
      this.renderer.clearOcclusionFocus();
      /* The renderer's sticky per-frame state, staged ahead of EVERY draw
       * branch (see `_stageFrameState`). */
      this._stageFrameState(rt);
      /* The frame's screen-primitive backdrops draw inside the 3D pass,
       * after its clear and before its first draw (`preSceneHook`). */
      this._screenStaged = false;
      this.renderer.preSceneHook = skipDraw ? null : () => this._drawScreenPrimsBackdrop(rt);
      /* The scene clear colour too: the engine's `World::frame_clear_rgb`
       * already answers black for the four minigame modes (the native
       * window clears every venue to it), but the page applied it only on
       * the field and battle branches, so a minigame drawn through the
       * renderer's own clear (the Baka title card, before its arena is in
       * shot) kept the walked-in scene's colour - town01's brown. */
      this._applySceneClear(rt);
      /* The volumetric ground fog draws over the field and battle frames
       * only - never over a minigame venue, which owns its own VRAM and
       * camera. Raised by the two branches below after their scene draw. */
      this._fogFrameOk = false;
      if (window.LegaiaPlayMinigames && window.LegaiaPlayMinigames.frame(rt, this, skipDraw)) {
        /* An in-world minigame (casino slots, Muscle Dome, Baka Fighter,
         * dance hall) owns the 3D frame; HUD/overlay below still run. Runs
         * before the battle branch: it also handles the VRAM restore on
         * exit and the fishing prize-exchange affordance. */
      } else if (this._battleFrame(rt, skipDraw)) {
        /* Battle owns the 3D frame; FPS/HUD/overlay below still run. */
      } else {
      /* Live VRAM effects - water CLUT-walk shimmer, jou's ambient palette
       * cyclers + lightning, scripted CLUT fx. The engine mutated the scene
       * VRAM during the ticks above; re-upload the texture only when texels
       * really changed (the take call clears the engine-side flag). Guarded
       * so a cached wasm bundle predating the export still runs statically. */
      if (typeof rt.field_vram_take_dirty === 'function' && rt.field_vram_take_dirty()) {
        this.renderer.uploadVram(rt.field_vram_bytes());
      }
      /* VDF vertex morphs: env-pack meshes whose morph deltas moved this
       * tick (retail-armed ambient parts + the scene-entry pulse) get just
       * their positions re-uploaded - env mesh id == pack slot here. */
      if (typeof rt.field_morph_slots === 'function') {
        const morphSlots = rt.field_morph_slots();
        for (let i = 0; i < morphSlots.length; i++) {
          const pos = rt.field_morph_positions(morphSlots[i]);
          if (pos.length) this.renderer.updateSceneMeshPositions(morphSlots[i], pos);
        }
      }
      const pt = rt.player_transform();
      let draws;
      /* In VR first-person there is no third-person lens: the eye IS the
       * player, so nothing can "sit between" them - the fade stays off. */
      const fpLive = this._vrFp && this.vr && this.vr.isActive();
      /* Camera-occlusion fade (see-through walls): the engine's
       * `play_occlusion_fade` runs the whole frame - the shared arming
       * kernel (`field_occlusion::host_fade_armed`) over this page's terms,
       * the 5-point visibility gate against the eye (retail Y-down, so the
       * draw-frame `_eye()` Y negates on the way in) and the `FadeRamp`
       * the native window steps. It answers `[strength, centre, feet]`
       * when a focus is to be staged. The centre is the point the gate
       * tested (`field_occlusion::player_body_centre`); the feet anchor the
       * feet-line rule. A bundle predating the export draws no fade. */
      /* The screen term is the page's own screens only - it has none left;
       * the pause menu, a shop / inn / exchange and the naming prompt are
       * engine state the export folds in itself. A throw here is logged
       * once: this `try` once swallowed a ReferenceError every frame, and
       * the fade silently never staged. */
      if (typeof rt.play_occlusion_fade === 'function') {
        let f = null;
        try {
          const eye = this._eye();
          f = rt.play_occlusion_fade(eye[0], -eye[1], eye[2], !!this.occlusionFade,
            !!(fpLive || this.debugCamera), false);
        } catch (e) {
          f = null;
          if (!this._occlWarned) { this._occlWarned = true; console.warn('occlusion fade', e); }
        }
        if (f && f.length >= 7) {
          this.renderer.setOcclusionFocus([f[1], f[2], f[3]], f[0], [f[4], f[5], f[6]]);
        }
      }
      /* Every body the scene loads draws every frame, as natively: no
       * per-body occlusion cull (docs/subsystems/renderer.md). */
      draws = this.staticDraws.filter(d => !d.hidden);

      /* Player: the engine's live posed vertices + its world transform. The
       * world frame is retail's (+Y down), so the draw negates Y the way every
       * placement does. The mesh's rest pose faces -Z while the engine's heading
       * has 0 = travelling +Z, hence the half-turn. */
      /* The scene may hide the lead outright (`play_player_drawn`: the
       * opening prologue's vignette shots have none), the native actor
       * pass's skip. */
      const leadDrawn = typeof rt.play_player_drawn !== 'function' || rt.play_player_drawn();
      if (this.player && leadDrawn) {
        const posed = rt.player_mesh_positions();
        if (posed.length) this.renderer.updateSceneMeshPositions(PLAYER_MESH_ID, posed);
        const playerDraw = {
          meshId: PLAYER_MESH_ID,
          x: pt[0], y: -pt[1], z: pt[2],
          rotY: -(pt[3] + 2048) * A2R,
          scale: 1.0,
          /* Actor draw: the occlusion fade must never dissolve the player. */
          noOccl: true,
          /* The player's object-effect clip (`CC F8 C2`, play_effect_clip). */
          effectPlayer: true,
        };
        /* The player's op `4C 81` draw tint (a cutscene's fade to black, a
         * red flash) as a constant per-draw cue - the native window's
         * `player_tint_cue`. Empty while untinted. */
        const ptint = typeof rt.play_player_tint === 'function' ? rt.play_player_tint() : null;
        if (ptint && ptint.length === 4 && ptint[3] > 0) {
          playerDraw.cue = { far: [ptint[0], ptint[1], ptint[2]], nearZ: -1, farZ: 0, maxIr0: ptint[3] };
        }
        draws.push(playerDraw);
      }

      /* Animated environment props: advance each to the engine's live prop-bank
       * cursor. The windmill's sails spin continuously; a house door swings on
       * contact. A prop resting on frame 0 is left as its uploaded rest pose,
       * and one whose cursor has moved is re-posed - only when the frame
       * actually changed, not once per rendered frame. */
      if (advance && this.animProps.length) {
        const pf = rt.field_placement_frames();
        for (const p of this.animProps) {
          const f = (p.i < pf.length) ? pf[p.i] : -1;
          if (f < 0 || f === p.lastFrame) continue;
          const posed = rt.field_mesh_posed_frame_positions(p.slot, p.anim, f);
          if (posed.length) {
            this.renderer.updateSceneMeshPositions(p.meshId, posed);
            p.lastFrame = f;
          }
        }
      }

      /* NPCs: show each clip's CURRENT engine frame and draw at the world's
       * live position. The playhead lives in the engine and advances one step
       * per SIM tick (`tick_frame` -> `drive_npc_clips`), so clip cadence is
       * the retail 60 Hz-tick rate however fast the display refreshes - the
       * native window's sim-tick anim contract. The pose only rebuilds when
       * the engine's frame (or the clip itself, via an ANIMATE cue re-target -
       * the `generation` bump) actually changed. Falls back to the wall-clock
       * animator against a cached WASM without the clip-state API. */
      /* Tile board (field-VM op 0x49), synced BEFORE the NPC loop because
       * that loop consults `tileActorSlots` to skip board-owned actors -
       * syncing after it would leave the exclusion set one frame stale, which
       * is exactly one frame of ghosted tiles per board install. */
      if (typeof rt.play_tile_actor_slots === 'function') this._syncTileBoard(rt);
      /* The actor layer through the shared draw path (js/field-actors.js):
       * each clip's CURRENT engine frame (the playhead advances one step per
       * SIM tick in `tick_frame`, so clip cadence is the retail rate however
       * fast the display refreshes), op-0x4B morphs, the world's live
       * position / heading / tilt, the off-map hide skip. Board-owned actors
       * draw once per board cell through the tile pass below - drawing them
       * here too ghosts a tile at whichever cell the refresh touched last
       * (the native redraw skips the same slots, `is_tile_actor_slot`). */
      if (!this._actorApi) this._actorApi = window.LegaiaFieldActors.api(rt, 'play_npc_');
      window.LegaiaFieldActors.frame(this.renderer, this._actorApi, this.npcs, draws, {
        advance,
        skipSlot: (slot) => this.tileActorSlots.has(slot),
        /* Retail's actor cull under the visible-tile crop - the native NPC
         * pass's `npc_actor_visible` gate. */
        culled: typeof rt.play_npc_culled === 'function'
          ? rt.play_npc_culled(this.debugCamera) : null,
        /* Actor draw: exempt from the occlusion fade, like the player. */
        extra: { noOccl: true },
      });

      /* Tile board (field-VM op 0x49). A board is installed at RUNTIME by the
       * scene's script, not at scene load, so the upload is checked here per
       * frame rather than in `_rebuild` - the native window's redraw pass does
       * the same. Without this the page ran the walk state machine against a
       * board it never drew: the wall cells still blocked, so the player
       * walked into nothing and stopped.
       *
       * A cell whose mesh never uploaded is SKIPPED, not drawn at the origin
       * (the native `drained_spawn_slots` gate); the generic actor loop skips
       * board-owned slots entirely, because a tile actor's own transform only
       * carries the last repositioned cell. */
      if (typeof rt.play_tile_board_transforms === 'function') {
        /* Stride 5: slot, x, y, z, scale (TILE_DRAW_STRIDE in
         * play_tile_board.rs) - the board fade scales each tile. */
        const tb = rt.play_tile_board_transforms();
        for (let i = 0; i + 4 < tb.length; i += 5) {
          const slot = tb[i] | 0;
          if (this.tileMeshSlots.indexOf(slot) < 0) continue;
          draws.push({
            meshId: TILE_MESH_BASE + slot,
            x: tb[i + 1], y: -tb[i + 2], z: tb[i + 3],
            rotY: 0, scale: tb[i + 4],
          });
        }
      }

      /* This frame's camera. The engine resolves WHICH camera owns the frame
       * and what its retail GTE inputs are (`camera_view::resolve_field_camera`
       * - follow / op-0x45 cutscene shot / overworld walk / world-map
       * top-view), and hands back the matrix; the page uploads it. The page
       * used to run its own orbit projection here and re-map the cutscene
       * params onto it, which is a second camera model beside the engine's. */
      this._stageEngineCamera(pt);
      this._applyTerrainFacing(this.rt);

      /* This frame's field view-projection, built exactly as the renderer
       * will build it (`buildWorldOrbitVp`, or the VR/battle override). Two
       * consumers below need it before the draw: the field FX billboards
       * (camera-facing quads need the camera basis) and the field party
       * HUD's projected player Y. */
      let fieldVp = null;
      try {
        const c = this.renderer.canvas;
        fieldVp = buildWorldOrbitVp(c.width, Math.max(c.height, 1), this._ext, this.cam);
      } catch (e) { fieldVp = null; }

      /* Retail's placed-object near reject (FUN_8001ADA4's draw-kind 5
       * arm): a placed object whose origin sits within 160 units of the eye,
       * or behind it, is not drawn at all - retail has no near-plane clip, so
       * the port would otherwise paint the inside of a prop the camera stands
       * in (nilboa's mist shell). Judged by the engine's
       * `field_env::placed_origin_near_culled` kernel, the one the native
       * window asks, and only under the engine's retail camera. */
      if (fieldVp && this.cam.vp && !this.debugCamera
          && typeof rt.field_placed_near_culled === 'function') {
        const m = fieldVp;
        draws = draws.filter(d => d.placeIdx === undefined
          || !rt.field_placed_near_culled(m[3] * d.x + m[7] * d.y + m[11] * d.z + m[15]));
      }

      /* Field party-status HUD: retail's decision kernel compares the
       * lead's PROJECTED screen Y against a band (the native window's
       * `field_hud_projected_player_y`). Project the same point the native
       * pass projects - the actor origin raised 128 units - through this
       * frame's VP into 240-line stage space, and hand it to the engine. */
      if (typeof rt.play_field_hud_project === 'function') {
        const c = this.renderer.canvas;
        try { rt.play_field_hud_project(c.width, Math.max(c.height, 1)); } catch (_) {}
      } else if (fieldVp && typeof rt.set_field_player_screen_y === 'function') {
        /* Cached-bundle fallback only. The live path is the export above:
         * retail's band test reads the FOLLOW camera, and projecting through
         * this frame's draw VP put it under the cutscene shot instead. */
        const px = pt[0], py = -(pt[1] - 128), pz = pt[2];
        const m = fieldVp;
        const cy = m[1] * px + m[5] * py + m[9] * pz + m[13];
        const cw = m[3] * px + m[7] * py + m[11] * pz + m[15];
        /* -2147483648 is the engine's NO_FIELD_PROJECTION sentinel. A plain
         * negative will not do: a lead projected above the top of the stage
         * has a negative stage Y and is a number the kernel compares. */
        rt.set_field_player_screen_y(cw > 0 ? Math.round((1 - cy / cw) * 120) : -2147483648);
      }

      /* Script-spawned actors (field-VM 0x4C 0xD8): the engine hands over
       * each slot that gained a TMD once; upload its rest pose and draw it
       * from the live actor transform every frame after. The native window
       * drains the same queue in its redraw pass. */
      if (typeof rt.play_take_dynamic_mesh_slots === 'function') {
        const fresh = rt.play_take_dynamic_mesh_slots();
        /* Morph-weight actors (the same opcode) re-stage every frame: the
         * engine's envelope ramps on every tick, so their blended mesh is
         * never the one already uploaded. The native window re-uploads the
         * same slots in its redraw pass. */
        let restage = fresh;
        if (typeof rt.play_morph_weight_slots === 'function') {
          const morph = rt.play_morph_weight_slots();
          if (morph.length) restage = Array.from(new Set([...fresh, ...morph]));
        }
        for (let i = 0; i < restage.length; i++) {
          const slot = restage[i];
          try {
            if (!rt.play_dynamic_actor_mesh(slot)) continue;
            const pos = rt.play_dynamic_mesh_positions();
            const idx = rt.play_dynamic_mesh_indices();
            if (!pos.length || !idx.length) continue;
            const flat = rt.play_dynamic_mesh_flat_rgba();
            this.renderer.uploadSceneMesh(DYN_ACTOR_MESH_BASE + slot, pos,
              rt.play_dynamic_mesh_uvs(), rt.play_dynamic_mesh_cba_tsb(), idx,
              flat.length ? flat : null);
            this._dynActors.add(slot);
          } catch (e) { /* a slot with no drawable mesh just stays unlisted */ }
        }
        if (this._dynActors.size) {
          const dt = rt.play_dynamic_actor_transforms();
          for (let o = 0; o + 5 < dt.length; o += 6) {
            const slot = dt[o] | 0;
            if (!this._dynActors.has(slot) || dt[o + 5] < 0.5) continue;
            if (this.tileActorSlots.has(slot)) continue;
            draws.push({
              meshId: DYN_ACTOR_MESH_BASE + slot,
              x: dt[o + 1], y: -dt[o + 2], z: dt[o + 3],
              rotY: -(dt[o + 4] + 2048) * A2R,
              scale: 1.0,
              noOccl: true,
            });
          }
        }
      }

      /* Field effects - the effect-pool billboards, `etmd` models, summon /
       * move-FX parts and the scene's own move-VM stager parts the native
       * redraw draws on every field frame. The engine builds the geometry
       * against this frame's VP (billboards face the camera that draws
       * them); scene-pack parts reuse the env meshes already uploaded. */
      if (fieldVp && typeof rt.play_field_fx_sync === 'function') {
        this._fieldFxDraws(rt, fieldVp, draws);
      }
      this._applyEffectClips(rt, draws);
      this._applySceneClear(rt);
      this._draws = draws;
      /* `skipDraw`: a VR session owns the framebuffer and re-issues this draw
       * once per eye with the XR view matrices. */
      if (!skipDraw) this.renderer.renderAssembled(this._draws, this._ext, this.cam);
      this._fogFrameOk = true;
      }
      } catch (e) {
        this._onEngineTrap('engine draw', e);
        return;
      }

      /* Screen-space PSX primitives over the finished 3D frame: the
       * field-to-battle transition's five style bodies + backdrop + fade,
       * emitted by the shared `legaia_render_kernels::battle_intro` and ordered by
       * the shared `screen_prim` builder. Outside the try/catch above
       * only in the sense that it has its own: a shader link failure on some
       * driver must not take the whole play loop down with it. */
      /* Volumetric ground fog (the engine's `fog_volume` bank, an
       * enhancement - the native renderer's fog pass twin): over the
       * finished 3D frame, under the screen-prim layer and the HUD canvas. */
      if (!skipDraw && this._fogFrameOk) this._drawFogVolume(rt);
      /* Bloom (an enhancement, off by default - the native renderer's bloom
       * pass twin): over the fogged 3D frame, under the screen prims and HUD. */
      if (!skipDraw && this._fogFrameOk && this.bloom) this._drawBloom(rt);
      if (!skipDraw) this._drawScreenPrims(rt);

      /* FPS + HUD, sampled twice a second. */
      this._fpsFrames++;
      const now = performance.now();
      if (now - this._fpsLast >= 500) {
        this.fps = Math.round(this._fpsFrames * 1000 / (now - this._fpsLast));
        this._fpsFrames = 0;
        this._fpsLast = now;
      }
      if (this.opts.onState) {
        try {
          const st = JSON.parse(rt.state_json());
          this._hudState = st;
          this.opts.onState(st, this.fps);
        } catch (e) { /* a malformed frame must not kill the loop */ }
      }

      /* The retail pause menu / dialog reading box overlay: engine-driven,
       * blitted onto the 2D overlay canvas over the GL view. Skipped while VR
       * presents. */
      if (!skipDraw) this._drawOverlay();
      /* ... and the screen primitives that cover it. */
      this._drawScreenPrimsOver();

      /* Field-to-battle transition capture: every intro style textures its
       * geometry with the field frame the player was just looking at, so on
       * the frame the emitter arms, read back the FINISHED frame - after the
       * 3D pass, the volumetric fog, both screen-prim lists (the fog-pool
       * particles, drop shadows, light pools, fades) - and land it. The
       * engine composes that frame without the transition's own layer
       * (`tick_battle_intro`), so what is read is the field as shown, every
       * enhancement included, the native window's capture target. Read
       * earlier, straight after the 3D pass, the capture dropped the fog
       * and every screen layer. readPixels hands rows bottom-up; the
       * engine's blit flips them. The VRAM texture re-uploads at once so
       * the next frame's strips sample the capture, not stale texels.
       * One-shot per transition; the curtain's per-frame intermediate
       * afterwards rides the ordinary field_vram_take_dirty re-upload.
       * The page declares the readback every frame it draws: the engine
       * leaves the transition layer off the arming frame only for a host
       * that will read it back (a VR session, which skips this, keeps the
       * transition from its first frame). */
      if (typeof rt.play_set_intro_readback === 'function') {
        rt.play_set_intro_readback(!skipDraw);
      }
      if (!skipDraw && typeof rt.play_intro_wants_capture === 'function' &&
          rt.play_intro_wants_capture()) {
        try {
          const gl = this.renderer.gl;
          const cw = gl.drawingBufferWidth, ch = gl.drawingBufferHeight;
          const buf = new Uint8Array(cw * ch * 4);
          gl.readPixels(0, 0, cw, ch, gl.RGBA, gl.UNSIGNED_BYTE, buf);
          rt.play_intro_land_capture(buf, cw, ch);
          this.renderer.uploadVram(rt.field_vram_bytes());
        } catch (e) { /* presentation-only; keep playing without the capture */ }
      }
      /* A field-VM `43 12` copy that reads the display framebuffer (the
       * ending vignettes' photo grab into VRAM (512, 0)) waits on the drawn
       * frame: read back the FINISHED frame - after the volumetric fog and
       * both screen-prim lists, as retail's copy reads what is displayed -
       * and land it; the next field-VRAM pass runs the copy and the
       * ordinary dirty re-upload carries it. The native window's twin is
       * its redraw's `framebuffer_grab_pending` block, which reads its
       * presented target. */
      if (!skipDraw && typeof rt.play_frame_grab_pending === 'function' &&
          rt.play_frame_grab_pending()) {
        try {
          /* Read the PSX display rect, not the canvas: the largest centred
           * 4:3 rect of the drawing buffer, which is the 320 x 240 screen the
           * engine projects onto. A canvas that is not 4:3 (fullscreen, a
           * resized window) would otherwise squash or widen the photo; read
           * this way both hosts land the same texels. */
          const gl = this.renderer.gl;
          const bw = gl.drawingBufferWidth, bh = gl.drawingBufferHeight;
          const cw = Math.min(bw, Math.floor(bh * 4 / 3));
          const ch = Math.min(bh, Math.floor(cw * 3 / 4));
          const x0 = Math.floor((bw - cw) / 2), y0 = Math.floor((bh - ch) / 2);
          const buf = new Uint8Array(cw * ch * 4);
          gl.readPixels(x0, y0, cw, ch, gl.RGBA, gl.UNSIGNED_BYTE, buf);
          rt.play_land_frame_grab(buf, cw, ch);
        } catch (e) { /* presentation-only */ }
      }
    }

    /* One frame of the screen-space PSX primitive pass.
     *
     * The engine builds the whole draw list - the ordering-table sort and the
     * NDC vertex stream both come out of the shared
     * `legaia_render_kernels::screen_prim` builder, so this method has nothing to
     * order and nothing to project. It asks how many primitives the frame
     * carries, early-outs on none (every frame outside a transition), and
     * otherwise uploads three arrays and lets `ScreenPrimPass` draw the runs.
     *
     * Guarded against a cached WASM bundle predating the exports, and against
     * a driver that cannot link the pass: either way the page keeps playing
     * with the pass switched off for the session. */
    /* Upload this frame's screen primitives once and work out its three
     * spans: `[0, back)` the backdrops (drawn before the 3D scene), then
     * `[back, split)` the rest of the under list, `[split, total)` the over
     * list. Returns null when the frame carries none. */
    _stageScreenPrims(rt) {
      if (this._screenStaged) return this._screenSpans;
      this._screenStaged = true;
      this._screenSpans = null;
      if (this._screenPrimBroken) return null;
      if (typeof rt.play_screen_prim_count !== 'function') return null;
      let n = 0;
      try { n = rt.play_screen_prim_count() | 0; } catch (e) { return null; }
      if (n <= 0) return null;
      if (!this._screenPrims) this._screenPrims = new ScreenPrimPass(this.renderer);
      const runs = rt.play_screen_prim_runs();
      const total = (runs.length / 3) | 0;
      /* A cached WASM predating the split draws everything here, under
       * the text - the order this page had before it. */
      const split = (typeof rt.play_screen_prim_over_run_start === 'function')
        ? Math.min(rt.play_screen_prim_over_run_start() >>> 0, total) : total;
      const back = (typeof rt.play_screen_prim_backdrop_run_end === 'function')
        ? Math.min(rt.play_screen_prim_backdrop_run_end() >>> 0, split) : 0;
      this._screenPrims.upload(
        rt.play_screen_prim_vertex_bytes(),
        rt.play_screen_prim_indices(),
        runs,
        (typeof rt.play_fx_depth_affine === 'function') ? rt.play_fx_depth_affine() : null,
      );
      this._screenSpans = { back, split, total, backDrawn: false };
      return this._screenSpans;
    }

    /* The backdrops, from inside the 3D pass (`preSceneHook`). */
    _drawScreenPrimsBackdrop(rt) {
      try {
        const sp = this._stageScreenPrims(rt);
        if (!sp) return;
        this._screenPrims.drawRuns(0, sp.back);
        sp.backDrawn = true;
      } catch (e) {
        this._screenPrimBroken = true;
        try { console.warn('screen-prim pass disabled:', e); } catch (_) { /* no console */ }
      }
    }

    _drawScreenPrims(rt) {
      this._screenOver = null;
      try {
        const sp = this._stageScreenPrims(rt);
        this._screenStaged = false;
        if (!sp) return;
        /* A frame whose 3D pass never ran the hook (no scene drawn) still
         * draws its backdrops, here, first. */
        this._screenPrims.drawRuns(sp.backDrawn ? sp.back : 0, sp.split);
        if (sp.split < sp.total) this._screenOver = [sp.split, sp.total];
      } catch (e) {
        this._screenPrimBroken = true;
        this._screenOver = null;
        try { console.warn('screen-prim pass disabled:', e); } catch (_) { /* no console */ }
      }
    }

    /* The `over` half of the frame's screen primitives: the ones the engine
     * places AFTER the text layer (`screen_layers::compose_screen_prims`) -
     * a warp's fade, the shop and pause wipes, the cinematic bars, the
     * battle numerals, the minigame chrome. The native window presents
     * scene, `under`, 2D overlay, `over`; this page's text is a 2D canvas
     * stacked above the GL view, so on a frame that carries `over`
     * primitives the 2D layers are composited into the GL frame first
     * (`ScreenPrimPass.drawLayer`) and their DOM canvases hidden for that
     * frame. A frame without any keeps the plain DOM stack. */
    _drawScreenPrimsOver() {
      const span = this._screenOver;
      this._screenOver = null;
      const layers = [document.getElementById('play-minigame-layer'), this.menuOverlay]
        .filter(c => c && !c.hidden && c.width > 0 && c.height > 0);
      let inGl = false;
      if (span && this._screenPrims && !this._screenPrimBroken) {
        try {
          for (const c of layers) this._screenPrims.drawLayer(c);
          inGl = true;
          this._screenPrims.drawRuns(span[0], span[1]);
        } catch (e) {
          this._screenPrimBroken = true;
          inGl = false;
          try { console.warn('screen-prim pass disabled:', e); } catch (_) { /* no console */ }
        }
      }
      if (inGl !== !!this._layersInGl) {
        this._layersInGl = inGl;
        for (const c of [document.getElementById('play-minigame-layer'), this.menuOverlay]) {
          if (c) c.style.visibility = inGl ? 'hidden' : '';
        }
      }
    }

    /* One frame of the volumetric ground fog. The engine owns the bank and
     * the toggle (`rt.set_volumetric_fog`, persisted with the options); an
     * empty header is "no bank this frame". The matrix is the one
     * `renderAssembled` just drew with (`buildWorldOrbitVp` hands back
     * `cam.vp` when the engine camera is staged). Guarded against a cached
     * WASM without the exports and a driver that cannot link the pass:
     * either way the page keeps playing without fog for the session. */
    _drawFogVolume(rt) {
      if (this._fogBroken) return;
      if (typeof rt.play_fog_volume_header !== 'function'
          || typeof window.LegaiaFogVolumePass !== 'function') return;
      try {
        const c = this.renderer.canvas;
        const vp = (this.cam && this.cam.yaw != null)
          ? buildWorldOrbitVp(c.width, c.height, this._ext, this.cam)
          : null;
        if (!vp) return;
        if (!this._fogPass) this._fogPass = new window.LegaiaFogVolumePass(this.renderer.gl);
        this._fogPass.draw(rt, vp, this._battle ? this._battle.scale : 4.0,
          !!this.renderer.lastLogDepth);
      } catch (e) {
        this._fogBroken = true;
        try { console.warn('fog volume pass disabled:', e); } catch (_) { /* no console */ }
      }
    }

    /* Bloom over the finished 3D frame, at the engine's response for the
     * current mood (`play_bloom_params`). A driver that cannot link the pass
     * disables it for the session; the page keeps playing without it. */
    _drawBloom(rt) {
      if (this._bloomBroken) return;
      if (typeof rt.play_bloom_params !== 'function'
          || typeof window.LegaiaBloomPass !== 'function') return;
      try {
        if (!this._bloomPass) this._bloomPass = new window.LegaiaBloomPass(this.renderer.gl);
        this._bloomPass.draw(rt.play_bloom_params());
      } catch (e) {
        this._bloomBroken = true;
        try { console.warn('bloom pass disabled:', e); } catch (_) { /* no console */ }
      }
    }

    /* The renderer's per-frame state words: NCLIP cull, prologue colour
     * grade + depth-cue ramp, the palette-collapse half of the grade, and
     * the overworld curvature. Each is a value the engine decides for the
     * current mode, and each sticks in the renderer until it is set again.
     *
     * Called once a frame ahead of every draw branch - field, battle and the
     * in-world minigame venues - which is where the native window stages the
     * same five (`set_backface_cull`, `set_color_grade`,
     * `set_depth_cue_ramp`, `set_palette_grade`, `set_overworld_curvature`,
     * all before its mode branches). They used to be staged inside the field
     * branch alone, so a battle drew under whatever the last field frame
     * left: the field's NCLIP cull armed on the stage dome (the victory
     * orbit, whose eye leaves the dome, looked straight through a shell the
     * native window draws), and a fight entered from the overworld under
     * the overworld's screen-Y bend. See docs/tooling/host-drift.md. */
    _stageFrameState(rt) {
      /* The two opt-in render toggles (PSX rasterisation, dynamic light):
       * page-wide presentation choices, so every branch draws under the
       * checkbox state. Both off unless the player ticked them. */
      if (this.renderer.setPsxMode) this.renderer.setPsxMode(!!this.psxRender);
      if (this.renderer.setDynamicLighting) this.renderer.setDynamicLighting(!!this.dynLighting);
      if (this.renderer.setDynShadows) this.renderer.setDynShadows(this.dynShadows !== false);
      /* Log-of-w depth on every branch's perspective frames (webgl-shaders.js
       * LOG_DEPTH_GLSL): the resolution the native float reversed-Z buffer
       * has and a 24-bit one lacks. */
      if (this.renderer.setLogDepth) this.renderer.setLogDepth(true);
      /* Enhanced lighting's per-frame source: the engine's mood, picked
       * point lights and glow quads (`play_lighting_frame`), asked by the
       * renderer against the camera basis of the VP it draws with. */
      this.renderer.lightingProvider = (this.dynLighting
          && typeof rt.play_lighting_frame === 'function')
        ? (r, u) => rt.play_lighting_frame(r[0], r[1], r[2], u[0], u[1], u[2])
        : null;
      /* Retail GTE NCLIP winding rejection, from the shared engine kernel
       * (`camera_view::nclip_cull_mode`): armed for the whole field pass
       * (retail culls every field mesh's back faces - a sky dome's outer
       * shell, the opdeene prologue shot's near cave wall) and for a
       * cutscene camera on any other non-overworld mode; both-sided for
       * battle, the overworld and the minigame venues. */
      if (this.renderer.setNclipCull && typeof rt.play_render_nclip_mode === 'function') {
        let mode = 0;
        try { mode = rt.play_render_nclip_mode(); } catch (_) { mode = 0; }
        this.renderer.setNclipCull(mode);
      }
      /* Prologue colour grade + gold depth-cue ramp. No-ops on a renderer
       * without the uniforms (cached JS). */
      if (this.renderer.setColorGrade) {
        const g = this._cut && this._cut.grade;
        this.renderer.setColorGrade(g ? g.gold : null, g ? g.strength : 0);
        const c = this._cut && this._cut.cue;
        this.renderer.setDepthCue(c ? c.far : null,
          c ? c.near_z : 0, c ? c.far_z : 0, c ? c.max_ir0 : 0);
      }
      /* The prologue grade's PALETTE-COLLAPSE half: with a prologue grade
       * live, the shaders give the packet words the `4C E6` sepia rewrite
       * and this carries the op-`4C 12` screen tint. The engine composed
       * both arms already - the page just stages what
       * `play_cutscene_state_json` hands back. Without it the tint reached
       * the engine-built field-FX geometry and nothing else, so an ordinary
       * town's scene-entry fade darkened the smoke puffs over a town that
       * never faded. */
      if (this.renderer.setPaletteGrade) {
        const pg = this._cut && this._cut.palette_grade;
        this.renderer.setPaletteGrade(pg ? pg.mul : null, !!(pg && pg.on));
      }
      /* The overworld's per-vertex screen-Y bend (FUN_800271A8's table,
       * applied by retail's overworld prim leaves), scaled for this frame's
       * camera arm by the shared `overworld_curvature::frame_curve_scale`
       * kernel; `0` (flat) off the kingdom overworld. */
      if (this.renderer.setOverworldCurve && typeof rt.play_render_curve_scale === 'function') {
        let scale = 0;
        try { scale = rt.play_render_curve_scale(); } catch (_) { scale = 0; }
        /* The table's build offset in screen rows (World::overworld_curve_bias):
         * -6 once the map display has rebuilt the table, at any scale. */
        let bias = 0;
        if (typeof rt.play_render_curve_bias === 'function') {
          try { bias = rt.play_render_curve_bias(); } catch (_) { bias = 0; }
        }
        this.renderer.setOverworldCurve(scale, bias);
      }
      /* Retail's per-primitive near reject (camera_view::prim_near_cut): a
       * primitive whose mean corner depth sits near or behind the eye is not
       * drawn, where a per-pixel clip paints it across the frame. Only under
       * the engine's retail camera - never the debug orbit. */
      if (this.renderer.setPrimNear && typeof rt.play_render_prim_near === 'function') {
        let p = null;
        try { p = rt.play_render_prim_near(!this.debugCamera); } catch (_) { p = null; }
        this.renderer.setPrimNear(p);
      }
    }

    /* The clear colour is part of what a frame looks like, not a renderer
     * preference: retail black, battle included - a roofless stage shell
     * shows black above it. The engine picks it
     * (`battle_stage_clear::scene_clear`, the same selector the native window
     * renders with) and this page only applies it.
     *
     * Called on BOTH draw paths, not only the battle one: the renderer holds
     * the colour until something changes it, so setting it on battle entry
     * alone would leave the field drawing on battle sky for the rest of the
     * session. Guarded against a cached WASM with no such export, which keeps
     * the renderer's default. */
    _applySceneClear(rt) {
      try {
        if (typeof rt.play_scene_clear_color === 'function') {
          const c = rt.play_scene_clear_color();
          if (c && c.length === 4) this.renderer.clearColor = Array.from(c);
        }
      } catch (e) { /* keep the default clear */ }
    }

    /* Battle effect layer: append this frame's FX draws to `draws`.
     *
     * Two engine-composed seams, both from `crates/web-play/play_battle_fx`:
     *
     *  - the effect POOL, as one batched mesh of camera-facing textured quads
     *    (the retail FUN_801E0088 pass-2 billboards, sampling the flame atlas
     *    the battle VRAM carries). The per-billboard tinted outline is a
     *    diagnostic and is OFF unless something calls
     *    `rt.set_battle_fx_outline(true)` - retail draws no such rectangle,
     *    and leaving it on stamped an opaque red-ish box around every effect
     *    sprite in the fight. Its vertices
     *    already carry the FX camera's 4x world scale, so it draws under an
     *    identity model - the basis is derived engine-side from the same
     *    battle VP the page renders with, which is what makes the quads face
     *    the camera that actually draws them.
     *  - the 3D FX MODELS: `etmd.dat` effect meshes and the move-VM
     *    scene-graph parts (summon + battle move-FX), each with a ready model
     *    matrix. Meshes are cached per global-TMD-pool index for the fight.
     *
     * Silent no-op against a cached WASM without the FX exports. */
    /* Field twin of `_battleFxDraws`: same accessors, same billboard mesh
     * id, but the model draws split by source - a global-pool effect model
     * goes through the per-scene FX mesh cache, a scene-pack stager part
     * draws the env mesh the scene already uploaded (uploading the slot on
     * demand when no placement used it). */
    _fieldFxDraws(rt, vp, draws) {
      let verts = 0;
      try { verts = rt.play_field_fx_sync(vp); } catch (e) { return; }
      if (verts > 0) {
        this.renderer.uploadSceneMesh(BATTLE_FX_BILLBOARD_MESH,
          rt.play_battle_fx_positions(), rt.play_battle_fx_uvs(),
          rt.play_battle_fx_cba_tsb(), rt.play_battle_fx_indices(),
          rt.play_battle_fx_flat_rgba());
        draws.push({ meshId: BATTLE_FX_BILLBOARD_MESH, model: IDENTITY_MODEL, noOccl: true });
      }
      const n = rt.play_battle_fx_model_count();
      if (!n) return;
      const mats = rt.play_battle_fx_model_matrices();
      const cache = this._fieldFx;
      for (let i = 0; i < n; i++) {
        const tmd = rt.play_battle_fx_model_tmd(i);
        const scenePack = (typeof rt.play_battle_fx_model_is_scene_pack === 'function')
          && rt.play_battle_fx_model_is_scene_pack(i);
        let meshId;
        if (scenePack) {
          meshId = FIELD_FX_PACK_MESH_BASE + tmd;
          if (!cache.packMeshes.has(tmd)) {
            let ok = false;
            try {
              rt.field_mesh_posed(tmd, 0);
              const pos = rt.field_mesh_positions();
              const idx = rt.field_mesh_indices();
              if (pos.length && idx.length) {
                const flat = rt.field_mesh_flat_rgba();
                this.renderer.uploadSceneMesh(meshId, pos, rt.field_mesh_uvs(),
                  rt.field_mesh_cba_tsb(), idx, flat.length ? flat : null);
                ok = true;
              }
            } catch (e) { ok = false; }
            cache.packMeshes.set(tmd, ok);
          }
          if (!cache.packMeshes.get(tmd)) continue;
        } else {
          meshId = BATTLE_FX_MODEL_BASE + tmd;
          if (!cache.fxMeshes.has(tmd)) {
            const pos = rt.play_battle_fx_mesh_positions(tmd);
            const idx = rt.play_battle_fx_mesh_indices(tmd);
            cache.fxMeshes.set(tmd, !!(pos.length && idx.length));
            if (pos.length && idx.length) {
              this.renderer.uploadSceneMesh(meshId, pos,
                rt.play_battle_fx_mesh_uvs(tmd), rt.play_battle_fx_mesh_cba_tsb(tmd),
                idx, null);
            }
          }
          if (!cache.fxMeshes.get(tmd)) continue;
        }
        draws.push({ meshId, model: mats.subarray(i * 16, i * 16 + 16), noOccl: true });
      }
    }
  });
})();
