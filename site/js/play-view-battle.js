/* play-view-battle.js - PlayView's battle branch.
 *
 * Part of the PlayView class (play-app.js): the battle frame (_battleFrame),
 * the battle effect draws and the battle scene upload.
 *
 * Classic script. Requires play-view-constants.js and play-app.js first. */
(function () {
  'use strict';

  const {
    poseInto, BATTLE_FX_BILLBOARD_MESH, IDENTITY_MODEL,
    BATTLE_FX_MODEL_BASE, BATTLE_MESH_BASE, BATTLE_BACKDROP_SPUN_MESH,
  } = window.LegaiaPlayShared;

  /* Collapse the vertices of the objects a battle body leaves out this frame
   * to a point - the page's not-drawn convention. The list is the engine's:
   * the objects the per-channel model table does not name for the playing
   * stream's channels (FUN_80048A08 after the mesh-variant swap
   * FUN_8004CCD4). Returns whether anything was hidden. */
  function hideObjects(out, objectIds, hidden) {
    if (!hidden || !hidden.length) return false;
    const set = new Set(hidden);
    for (let v = 0; v < objectIds.length; v++) {
      if (set.has(objectIds[v])) {
        out[v * 3] = 0;
        out[v * 3 + 1] = 0;
        out[v * 3 + 2] = 0;
      }
    }
    return true;
  }

  /* The engine's hide list for battle mesh `i` (or its ghost `g`); empty on
   * a cached WASM without the export. */
  function hiddenOf(rt, i, g) {
    try {
      if (g === undefined) {
        return (typeof rt.play_battle_actor_hidden === 'function')
          ? rt.play_battle_actor_hidden(i) : null;
      }
      return (typeof rt.play_battle_actor_ghost_hidden === 'function')
        ? rt.play_battle_actor_ghost_hidden(i, g) : null;
    } catch (e) { return null; }
  }

  window.LegaiaUtil.installMethods(window.PlayView, class {

    /* ---------- battle 3D scene ---------- */

    /* One battle frame: upload the battle scene on its generation edge, pose
     * each bound actor from the engine's live battle-animation `pose_frame`,
     * take the engine-built battle view-projection (cam.vp), and draw. Returns
     * `false` outside battle (restoring the field VRAM texture on the exit
     * edge) so `_frame` runs its normal field path; `false` too when the
     * engine built no battle render (no monsters decoded) - the field scene
     * then keeps drawing behind the battle HUD, the old behaviour. Guarded
     * against a cached WASM without the battle exports. */
    _battleFrame(rt, skipDraw) {
      if (typeof rt.play_battle_active !== 'function') return false;
      let active = false;
      try { active = !!rt.play_battle_active(); } catch (e) { return false; }
      if (!active) {
        /* Drop the battle VP override whenever battle isn't drawing, even if
         * the battle state was torn down elsewhere (scene swap, trap
         * recovery). The field branch re-stages its own `cam.vp` from the
         * engine camera the same frame. */
        if (this.cam.vp) this.cam.vp = null;
        /* The field ground heightfield is the renderer's own retained pass
         * (`uploadGround`), not a member of the draw list: hand it back on
         * every non-battle frame, not only on the teardown edge below - a
         * scene swap out of a fight clears `_battle` in `_rebuild` first,
         * and the edge then never fires. */
        this.renderer.groundEnable = true;
        if (this._battle) {
          /* Battle just ended: drop the battle scene and restore the field
           * VRAM texture. The engine's field-side VRAM was never touched -
           * the battle worked on a throwaway copy, the native exit contract. */
          this._battle = null;
          this.renderer.uploadVram(rt.field_vram_bytes());
          /* Hand the orbit camera back the way the fight found it. */
          if (this._camBeforeBattle) {
            Object.assign(this.cam, this._camBeforeBattle);
            this._camBeforeBattle = null;
          }
        }
        return false;
      }
      const gen = rt.play_battle_generation();
      if (!this._battle || this._battle.gen !== gen) this._uploadBattleScene(rt, gen);
      /* The field scene's ground heightfield (`uploadGround` in `_rebuild`)
       * is a RETAINED renderer pass that `renderAssembled` draws before any
       * placement, whatever the draw list holds - so a battle over a field
       * scene with ground cells drew that field terrain through the battle
       * camera, textured from the battle VRAM: the flat yellow strips and
       * the stray grass / gravel patches around the monster. The native
       * battle pass draws the stage dome, the ground grid, the actors and
       * the FX only. Gate the field pass off for as long as the battle
       * draws; the non-battle branch above turns it back on. */
      this.renderer.setGroundEnable(false);
      const b = this._battle;
      if (!b) return false;
      /* Mid-battle VRAM re-stamps (facial animation, status-effect actor
       * recolour, effect CLUT stage): the engine mutated the battle VRAM
       * this tick; re-upload only when texels really changed. Guarded so a
       * cached bundle predating the export still runs. */
      if (typeof rt.play_battle_vram_take_dirty === 'function' && rt.play_battle_vram_take_dirty()) {
        const vram = rt.play_battle_vram_bytes();
        if (vram.length) this.renderer.uploadVram(vram);
        if (typeof rt.play_battle_vram_serial === 'function') b.vramSerial = rt.play_battle_vram_serial();
      } else if (typeof rt.play_battle_vram_serial === 'function' &&
                 b.vramSerial != null && rt.play_battle_vram_serial() !== b.vramSerial) {
        /* Residency guard - the page-side half of the native
         * `check_battle_vram_residency`: the engine's battle VRAM moved
         * past the copy the GPU holds (a re-stamp the dirty edge did not
         * report, or an upload the page missed), so re-upload it. */
        const vram = rt.play_battle_vram_bytes();
        if (vram.length) this.renderer.uploadVram(vram);
        b.vramSerial = rt.play_battle_vram_serial();
      }

      const draws = [];
      /* Backdrop (both copies pre-appended engine-side) + ground grid draw
       * at raw battle world coordinates; actors compose the retail 4x world
       * scale (base matrix 0x8007BF10), pre-scaled here because the page's
       * per-draw `scale` only scales the mesh, not its translation. */
      /* The backdrop pair's own depth cue (`FUN_80050120`'s `+0x78` ramp):
       * pulled toward black through a summon close-up, off the draw at full
       * weight (PROT 0903's fire tunnel). Guarded against a cached WASM
       * without the export. */
      let backdropCue = null;
      let backdropHidden = false;
      if (typeof rt.play_battle_backdrop_cue_json === 'function') {
        try {
          const c = JSON.parse(rt.play_battle_backdrop_cue_json());
          if (c && c.hidden) backdropHidden = true;
          else if (c && c.far) {
            backdropCue = { far: c.far, nearZ: c.near_z, farZ: c.far_z, maxIr0: c.max_ir0 };
          }
        } catch (e) { /* draw uncued */ }
      }
      if (b.backdrop && !backdropHidden) {
        draws.push({ meshId: b.backdrop, x: 0, y: 0, z: 0, rotY: 0, scale: 1.0, cue: backdropCue });
      }
      /* The backdrop's drawn slot 1 (on a stage that keeps object 1, the
       * horizon mist ribbon) turns about Y every battle frame, so it is one
       * copy drawn under the two per-frame models the engine composes -
       * the native window's two draws. */
      if (b.backdropSpun && !backdropHidden
          && typeof rt.play_battle_backdrop_spun_models === 'function') {
        const models = rt.play_battle_backdrop_spun_models();
        /* Copy A culls its back faces (the camera stands outside the
         * ribbon's ring, so its near quads drop); a mirrored copy B draws
         * both sides (`backdrop_copy_nclip`). Guarded against a cached WASM. */
        const spunNclip = (typeof rt.play_battle_backdrop_spun_nclip === 'function')
          ? rt.play_battle_backdrop_spun_nclip() : null;
        for (let o = 0, k = 0; o + 16 <= models.length; o += 16, k++) {
          draws.push({
            meshId: b.backdropSpun, model: models.slice(o, o + 16), cue: backdropCue,
            nclip: (spunNclip && k < spunNclip.length) ? spunNclip[k] : undefined,
          });
        }
      }
      /* The grid rides the stage's own GTE depth cue (`DAT_80078C1C` outdoor
       * table / indoor grey), as a PER-DRAW cue - nothing else in the frame
       * fogs. The engine resolved the far colour + ramp window at battle
       * entry; the page just attaches it. */
      if (b.ground) {
        /* Battle ambient (`0x8007B7B0`): a summon close-up ramps it down, and
         * the grid's packet colour and cue far colour both follow it. Re-read
         * both only when the engine's key moves - the limb-key pattern.
         * Guarded against a cached WASM without the export. */
        if (typeof rt.play_battle_ground_ambient_key === 'function') {
          const ambientKey = rt.play_battle_ground_ambient_key();
          if (ambientKey !== b.groundAmbientKey) {
            if (b.groundAmbientKey !== undefined) {
              this.renderer.updateSceneMeshFlat(b.ground, rt.play_battle_ground_flat_rgba());
            }
            try {
              const cue = JSON.parse(rt.play_battle_ground_cue_json());
              if (cue && cue.far) {
                b.groundCue = {
                  far: cue.far, nearZ: cue.near_z, farZ: cue.far_z, maxIr0: cue.max_ir0,
                };
              }
            } catch (e) { /* keep the last cue */ }
            b.groundAmbientKey = ambientKey;
          }
        }
        /* A cast holding `ctx[+0x278]` bit 0 on the floor ambient takes the
         * grid off the screen (`World::battle_ground_grid_drawn`). Guarded
         * against a cached WASM without the export. */
        const groundDrawn = (typeof rt.play_battle_ground_drawn !== 'function')
          || rt.play_battle_ground_drawn();
        if (groundDrawn) {
          /* Retail links every grid quad into one far ordering-table entry
           * (`0x3FEC >> otshift`): the floor never hides what is drawn after
           * it - the native host's `pin_to_far_bucket`. */
          draws.push({
            meshId: b.ground, x: 0, y: 0, z: 0, rotY: 0, scale: 1.0, cue: b.groundCue,
            farBucket: true,
          });
        }
      }
      const S = b.scale;
      const tf = rt.play_battle_actor_transforms();
      /* Target-select cursor: the engine resolves `FUN_801DA6B4`'s three
       * render words into a ready [enable, far rgb, max_ir0, model scale] row
       * per actor - the pointed-at monster pulses bright, the rest dim, and
       * the q12 render scale composes about the actor origin. Guarded against
       * a cached WASM without the export. */
      let cursor = null;
      try {
        if (typeof rt.play_battle_actor_cursor === 'function') {
          cursor = rt.play_battle_actor_cursor();
        }
      } catch (e) { /* no cursor tint this frame */ }
      /* Retail draws a battle body single-sided unless its colour word
       * carries the double-sided bit (BattleActorDrawPlan::nclip_mode): one
       * NCLIP word per actor, riding the placement over the battle pass's
       * both-sided word. Guarded against a cached WASM without the export. */
      let nclip = null;
      try {
        if (typeof rt.play_battle_actor_nclip === 'function') {
          nclip = rt.play_battle_actor_nclip();
        }
      } catch (e) { /* frame word only */ }
      /* A body the defeat fade is sinking draws clipped at the stage floor
       * (its node's `+0x42`, World::battle_actor_floor_clip): six floats per
       * actor, `[m0, m1, m2, lo, hi, enable]`, all zero when unclipped.
       * Guarded against a cached WASM without the export. */
      let floorClips = null;
      try {
        if (typeof rt.play_battle_actor_floor_clips === 'function') {
          floorClips = rt.play_battle_actor_floor_clips();
        }
      } catch (e) { /* draw unclipped */ }
      for (let i = 0; i < b.actors.length; i++) {
        const a = b.actors[i];
        const o = i * 5;
        if (!a || o + 4 >= tf.length || tf[o + 4] < 0.5) continue;   /* inactive */
        if (a.objectIds.length) {
          /* Live battle pose (idle loop / hit reaction / staged swing), the
           * same per-bone R.v+T composition every other animator here runs. */
          const pose = rt.play_battle_actor_pose(i);
          /* The playing clip's morph lanes (FUN_800495C8): object-local
           * positions with the lanes' deltas blended in, posed in place of
           * the rest upload this frame. Empty = no lane active. Guarded
           * against a cached WASM without the export. */
          let src = a.base;
          if (typeof rt.play_battle_actor_morph === 'function') {
            const m = rt.play_battle_actor_morph(i);
            if (m.length === a.base.length) src = m;
          }
          const hidden = hiddenOf(rt, i);
          if (pose.length) {
            poseInto(a.out, src, a.objectIds, pose, pose.length / 6, 0);
            hideObjects(a.out, a.objectIds, hidden);
            this.renderer.updateSceneMeshPositions(a.meshId, a.out);
          } else if (hidden && hidden.length) {
            a.out.set(src);
            hideObjects(a.out, a.objectIds, hidden);
            this.renderer.updateSceneMeshPositions(a.meshId, a.out);
          } else if (src !== a.base || a.morphed) {
            this.renderer.updateSceneMeshPositions(a.meshId, src);
          }
          a.morphed = src !== a.base || !!(hidden && hidden.length && !pose.length);
        }
        /* Rot limb dimming (FUN_80048A08's per-object colour rule): re-send
         * the actor's packet colours only when the engine's dimmed set
         * changes. Guarded against a cached WASM without the export. */
        if (typeof rt.play_battle_actor_limb_key === 'function') {
          const limbKey = rt.play_battle_actor_limb_key(i);
          if (limbKey !== (a.limbKey || 0)) {
            this.renderer.updateSceneMeshFlat(a.meshId, rt.play_battle_actor_limb_rgba(i));
            a.limbKey = limbKey;
          }
        }
        /* Whole-mesh blend (the near-camera ghost pass FUN_8004DC68, the
         * capture / defeat fade): re-send the actor's TSB stream with the
         * colour word's ABE + ABR ORed in when the engine's blend changes.
         * Guarded against a cached WASM without the export. */
        if (typeof rt.play_battle_actor_blend_key === 'function') {
          const blendKey = rt.play_battle_actor_blend_key(i);
          if (blendKey !== (a.blendKey || 0)) {
            this.renderer.updateSceneMeshCbaTsb(a.meshId, rt.play_battle_actor_blend_cba_tsb(i));
            a.blendKey = blendKey;
          }
        }
        /* The shatter renderer (the body's `+0x1BA` word; FUN_80029888):
         * the engine's shattered object-local corners, posed with the live
         * pose into a de-indexed companion mesh drawn in the body's place.
         * Its NCLIP word is already the both-sides one
         * (`play_battle_actor_nclip`). Guarded against a cached WASM. */
        let drawMeshId = a.meshId;
        if (a.objectIds.length && typeof rt.play_battle_actor_shatter === 'function') {
          const sp = rt.play_battle_actor_shatter(i);
          if (sp.length) {
            /* The companion carries the body's live colour and blend
             * streams (the Rot limb dimming, the defeat fade's additive
             * word), so it is rebuilt whenever either moves. */
            const shardKey = (a.limbKey || 0) + ':' + (a.blendKey || 0);
            if (!a.shard || a.shard.out.length !== sp.length || a.shard.key !== shardKey) {
              const ct = (a.blendKey && typeof rt.play_battle_actor_blend_cba_tsb === 'function')
                ? rt.play_battle_actor_blend_cba_tsb(i) : rt.play_battle_actor_cba_tsb(i);
              const fl = (a.limbKey && typeof rt.play_battle_actor_limb_rgba === 'function')
                ? rt.play_battle_actor_limb_rgba(i)
                : ((typeof rt.play_battle_actor_flat_rgba === 'function')
                  ? rt.play_battle_actor_flat_rgba(i) : null);
              a.shard = window.LegaiaFieldActors.shardMesh(this.renderer,
                BATTLE_MESH_BASE + 128 + i, rt.play_battle_actor_indices(i),
                rt.play_battle_actor_uvs(i), ct, fl, a.objectIds);
              a.shard.key = shardKey;
            }
            const pose = rt.play_battle_actor_pose(i);
            if (pose.length) {
              poseInto(a.shard.out, sp, a.shard.objectIds, pose, pose.length / 6, 0);
              hideObjects(a.shard.out, a.shard.objectIds, hiddenOf(rt, i));
              this.renderer.updateSceneMeshPositions(a.shard.meshId, a.shard.out);
            } else {
              this.renderer.updateSceneMeshPositions(a.shard.meshId, sp);
            }
            drawMeshId = a.shard.meshId;
          }
        }
        const c = (cursor && cursor.length >= (i + 1) * 6) ? i * 6 : -1;
        draws.push({
          meshId: drawMeshId,
          /* Retail translation with the frame flip on its Y (`T(F pos)`, like
           * the native actor_model): the battle VP ends in a Y-flip, and the
           * placement model's own flip only turns the mesh, so a body's
           * height `+0x36` (Y-down) is negated here - unnegated, a killed
           * monster the defeat fade sinks rose out of the floor instead. */
          x: tf[o] * S, y: -tf[o + 1] * S, z: tf[o + 2] * S,
          /* Every mesh rests facing +Z and turns by the live battle facing
           * `+0x46` (a seated monster's half-turn, a fleeing party's back).
           * The engine sends glam's yaw; this placement model takes it
           * negated (see placementModelScaledY). */
          rotY: -tf[o + 3],
          scale: (c >= 0) ? S * cursor[c + 5] : S,
          nclip: (nclip && i < nclip.length) ? nclip[i] : undefined,
          effectClip: (floorClips && floorClips.length >= (i + 1) * 6
            && floorClips[i * 6 + 5] > 0.5)
            ? floorClips.subarray(i * 6, i * 6 + 6) : undefined,
          cue: (c >= 0 && cursor[c] > 0.5)
            ? {
              far: [cursor[c + 1], cursor[c + 2], cursor[c + 3]],
              nearZ: -1, farZ: 0, maxIr0: cursor[c + 4],
            }
            : null,
        });
        /* Arts after-image ghosts: the engine's ghost plan (rate-scheduled
         * pose-history depths + per-character colour with the per-ghost
         * decay) drawn as additive copies behind the body - the saturated
         * cue (nearZ -1, maxIr0 1) is the walk's full weight, so every
         * texel is modulated by the ghost RGB (`texel * rgb / 128`) in
         * place of its packet colour, and the mesh's ABE|ABR1 prims ride the blend
         * pass with `strictDepth` (gl.LESS), so a ghost pose coincident
         * with the live body is rejected fragment-for-fragment instead of
         * additively washing the whole mesh - the retail look of a trail
         * in a deeper OT bucket than the body. */
        if (a.ghostMeshIds && a.ghostMeshIds.length && a.objectIds.length
            && typeof rt.play_battle_actor_ghosts === 'function') {
          const gh = rt.play_battle_actor_ghosts(i);
          /* 7 floats per ghost: pushed position, RGB, and the eye-push
           * scale k (the engine already moved the position to
           * eye + k*(pos - eye); multiplying the draw scale by k completes
           * the scale-about-the-eye, so the silhouette is unchanged while
           * the depth sits behind the live body). */
          const STRIDE = 7;
          const count = Math.min(gh.length / STRIDE, a.ghostMeshIds.length);
          for (let g = 0; g < count; g++) {
            const pose = rt.play_battle_actor_ghost_pose(i, g);
            if (!pose.length) continue;
            poseInto(a.out, a.base, a.objectIds, pose, pose.length / 6, 0);
            hideObjects(a.out, a.objectIds, hiddenOf(rt, i, g));
            this.renderer.updateSceneMeshPositions(a.ghostMeshIds[g], a.out);
            const j = g * STRIDE;
            draws.push({
              meshId: a.ghostMeshIds[g],
              x: gh[j] * S, y: gh[j + 1] * S, z: gh[j + 2] * S,
              rotY: -tf[o + 3],
              scale: S * gh[j + 6],
              /* Single-sided, as the live body (`BattleGhostDraw::
               * nclip_mode`): both shells additive read twice retail's
               * ghost. Guarded against a cached WASM. */
              nclip: (typeof rt.play_battle_actor_ghost_nclip === 'function')
                ? rt.play_battle_actor_ghost_nclip(i, g) : undefined,
              strictDepth: true,
              cue: {
                far: [gh[j + 3], gh[j + 4], gh[j + 5]],
                nearZ: -1, farZ: 0, maxIr0: 1,
              },
            });
          }
        }
      }
      this._battleFxDraws(rt, b, draws);

      /* Camera: the engine hands the page a READY view-projection - the
       * shared retail phase script's live pose (dialogue close-up / far
       * menu framing + idle orbit / per-character submenu close-up)
       * projected through `battle_cam_script::battle_vp`, the exact matrix
       * the native window renders with. `cam.vp` overrides the orbit
       * construction in buildWorldOrbitVp (and still yields to the VR
       * per-eye override), so the old orbit-remap fudge is gone. Guarded
       * against a cached WASM without the export: the previous camera is
       * kept rather than re-approximated. */
      try {
        if (typeof rt.play_battle_camera_vp === 'function') {
          const c = this.renderer.canvas;
          const vp = rt.play_battle_camera_vp(c.width / Math.max(c.height, 1));
          if (vp && vp.length === 16) this.cam.vp = Float32Array.from(vp);
        }
      } catch (e) { /* keep the previous camera */ }

      this._applySceneClear(rt);
      this._draws = draws;
      if (!skipDraw) this.renderer.renderAssembled(draws, this._ext, this.cam);
      this._fogFrameOk = true;
      return true;
    }

    _battleFxDraws(rt, b, draws) {
      if (typeof rt.play_battle_fx_sync !== 'function') return;
      const c = this.renderer.canvas;
      const aspect = c.width / Math.max(c.height, 1);
      let verts = 0;
      try { verts = rt.play_battle_fx_sync(aspect); } catch (e) { return; }
      if (verts > 0) {
        this.renderer.uploadSceneMesh(BATTLE_FX_BILLBOARD_MESH,
          rt.play_battle_fx_positions(), rt.play_battle_fx_uvs(),
          rt.play_battle_fx_cba_tsb(), rt.play_battle_fx_indices(),
          rt.play_battle_fx_flat_rgba());
        draws.push({ meshId: BATTLE_FX_BILLBOARD_MESH, model: IDENTITY_MODEL });
      }
      const n = rt.play_battle_fx_model_count();
      if (!n) return;
      const mats = rt.play_battle_fx_model_matrices();
      for (let i = 0; i < n; i++) {
        const tmd = rt.play_battle_fx_model_tmd(i);
        const meshId = BATTLE_FX_MODEL_BASE + tmd;
        if (!b.fxMeshes.has(tmd)) {
          const pos = rt.play_battle_fx_mesh_positions(tmd);
          const idx = rt.play_battle_fx_mesh_indices(tmd);
          /* Cache the miss too: a pool slot that yields no geometry must not
           * be re-decoded every frame for the life of the fight. */
          b.fxMeshes.set(tmd, !!(pos.length && idx.length));
          if (pos.length && idx.length) {
            this.renderer.uploadSceneMesh(meshId, pos,
              rt.play_battle_fx_mesh_uvs(tmd), rt.play_battle_fx_mesh_cba_tsb(tmd),
              idx, null);
          }
        }
        if (!b.fxMeshes.get(tmd)) continue;
        draws.push({ meshId, model: mats.subarray(i * 16, i * 16 + 16) });
      }
    }

    /* Upload the freshly-built battle scene: the battle VRAM (stage + flame
     * atlas + monster/party texture bands), the backdrop, the ground grid
     * and one mesh per bound battle actor. Actor slots that fail to decode
     * keep a null placeholder so the array stays index-parallel with the
     * engine's transform/pose exports. */
    _uploadBattleScene(rt, gen) {
      this._battle = null;
      const vram = rt.play_battle_vram_bytes();
      if (!vram.length) return;
      this.renderer.uploadVram(vram);
      const vramSerial = (typeof rt.play_battle_vram_serial === 'function')
        ? rt.play_battle_vram_serial() : null;
      const up = (meshId, pos, uvs, ct, idx, flat) => {
        if (!pos.length || !idx.length) return 0;
        this.renderer.uploadSceneMesh(meshId, pos, uvs, ct, idx,
          (flat && flat.length) ? flat : null);
        return meshId;
      };
      const b = {
        gen,
        vramSerial,
        scale: (typeof rt.play_battle_world_scale === 'function')
          ? rt.play_battle_world_scale() : 4.0,
        backdrop: 0, backdropSpun: 0, ground: 0, actors: [],
        /* global-TMD-pool index -> "this slot uploaded geometry", so an FX
         * model decodes once per fight (and a dud slot is not retried). */
        fxMeshes: new Map(),
        /* The ground grid's per-stage depth cue, in the renderer's per-draw
         * cue shape. Resolved engine-side (`play_battle_ground_cue_json` =
         * the SCUS outdoor table / indoor grey), null when no grid is up. */
        groundCue: null,
      };
      try {
        const cue = JSON.parse(rt.play_battle_ground_cue_json());
        if (cue && cue.far) {
          b.groundCue = {
            far: cue.far, nearZ: cue.near_z, farZ: cue.far_z, maxIr0: cue.max_ir0,
          };
        }
      } catch (e) { /* an unreadable cue just leaves the grid uncued */ }
      b.backdrop = up(BATTLE_MESH_BASE, rt.play_battle_backdrop_positions(),
        rt.play_battle_backdrop_uvs(), rt.play_battle_backdrop_cba_tsb(),
        rt.play_battle_backdrop_indices(), rt.play_battle_backdrop_flat_rgba());
      /* Guarded against a cached WASM without the exports. */
      if (typeof rt.play_battle_backdrop_spun_positions === 'function') {
        b.backdropSpun = up(BATTLE_BACKDROP_SPUN_MESH,
          rt.play_battle_backdrop_spun_positions(), rt.play_battle_backdrop_spun_uvs(),
          rt.play_battle_backdrop_spun_cba_tsb(), rt.play_battle_backdrop_spun_indices(),
          rt.play_battle_backdrop_spun_flat_rgba());
      }
      b.ground = up(BATTLE_MESH_BASE + 1, rt.play_battle_ground_positions(),
        rt.play_battle_ground_uvs(), rt.play_battle_ground_cba_tsb(),
        rt.play_battle_ground_indices(),
        (typeof rt.play_battle_ground_flat_rgba === 'function')
          ? rt.play_battle_ground_flat_rgba() : null);
      const n = rt.play_battle_actor_count();
      for (let i = 0; i < n; i++) {
        const base = rt.play_battle_actor_positions(i);
        const idx = rt.play_battle_actor_indices(i);
        if (!base.length || !idx.length) { b.actors.push(null); continue; }
        const meshId = BATTLE_MESH_BASE + 16 + i;
        /* Actors carry their packet colours like every other TMD draw - the
         * shader's `texel * colour / 128` is retail's only lighting term, so
         * an actor uploaded without them reads flat against a modulated
         * backdrop. Older bundles have no export: fall back to null. */
        const actorFlat = (typeof rt.play_battle_actor_flat_rgba === 'function')
          ? rt.play_battle_actor_flat_rgba(i) : null;
        this.renderer.uploadSceneMesh(meshId, base, rt.play_battle_actor_uvs(i),
          rt.play_battle_actor_cba_tsb(i), idx,
          (actorFlat && actorFlat.length) ? actorFlat : null);
        const rec = {
          meshId, base,
          objectIds: rt.play_battle_actor_object_ids(i),
          out: new Float32Array(base.length),
          ghostMeshIds: [],
        };
        /* Arts after-image ghosts (retail FUN_80049348, plan from the shared
         * `World::battle_ghost_draws`): two extra copies of the actor mesh on
         * the body's own textured path, every prim raised to
         * semi-transparent additive (the ABE bit and ABR mode 1 ORed into
         * its TSB, as FUN_80043390 ORs the ghost word's mode byte into each
         * packet), posed per frame from the engine's pose-history ring.
         * Retail's ghost is not a flat fill: the walk hands the ordinary
         * object draw a colour word and a full weight, and the prim bank it
         * selects still emits textured packets, so the ghost is the body's
         * texture modulated by the ghost colour - the per-draw cue below.
         * Guarded so a cached WASM without the export just skips the
         * layer. */
        if (rec.objectIds.length
            && typeof rt.play_battle_actor_ghosts === 'function') {
          const gcb = rt.play_battle_actor_cba_tsb(i);
          const ghostCt = new Uint16Array(gcb.length);
          for (let k = 0; k + 1 < gcb.length; k += 2) {
            ghostCt[k] = gcb[k];
            ghostCt[k + 1] = gcb[k + 1] | 0x8020;   /* ABE | ABR 1 (B + F) */
          }
          for (let g = 0; g < 2; g++) {
            const gid = up(BATTLE_MESH_BASE + 32 + i * 2 + g, base,
              rt.play_battle_actor_uvs(i), ghostCt, idx,
              (actorFlat && actorFlat.length) ? actorFlat : null);
            if (gid) rec.ghostMeshIds.push(gid);
          }
        }
        /* Pose to the rest/idle frame immediately: an unposed multi-part
         * character is a heap of limbs at the origin. (Empty objectIds =
         * the engine uploaded a statically-posed fallback mesh.) */
        if (rec.objectIds.length) {
          const pose = rt.play_battle_actor_pose(i);
          if (pose.length) {
            poseInto(rec.out, rec.base, rec.objectIds, pose, pose.length / 6, 0);
            hideObjects(rec.out, rec.objectIds, hiddenOf(rt, i));
            this.renderer.updateSceneMeshPositions(meshId, rec.out);
          }
        }
        b.actors.push(rec);
      }
      /* Remember the field camera so battle exit can restore its framing. */
      if (!this._camBeforeBattle) {
        this._camBeforeBattle = Object.assign({}, this.cam);
      }
      this._battle = b;
    }
  });
})();
