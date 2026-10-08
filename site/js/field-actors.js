/* The field actor layer's draw path - the scene's MAN-placed actors (NPCs,
 * chests, save crystals, story actors), posed and placed every frame off the
 * engine's world. One JS path for both surfaces that draw a live field scene:
 * the play page (over `LegaiaRuntime`'s `play_npc_*` exports) and the map
 * viewer's FieldSceneView (over `LegaiaViewer`'s `field_scene_npc_*` exports).
 * Both export families are thin delegates of the one Rust type
 * `web-viewer::field_actors::FieldActors`, so the two pages differ only in
 * which world they read.
 *
 *   const api = LegaiaFieldActors.api(rt, 'play_npc_');
 *   const recs = LegaiaFieldActors.upload(renderer, api, MESH_BASE);
 *   LegaiaFieldActors.rebind(renderer, api, recs);              // per frame
 *   LegaiaFieldActors.frame(renderer, api, recs, draws, opts);  // per frame
 *
 * Requires webgl-math.js (placementModelEuler).
 */
(function () {
  'use strict';

  const A2R = Math.PI * 2 / 4096;     /* PSX 12-bit angle -> radians */
  /* The wall-clock animator's rate, for a WASM build without the clip-state
   * exports (the engine's world-ticked clips replace it). */
  const NPC_CLIP_FPS = 15;
  /* Floats per actor in a `draw_poses` export (FieldActors::DRAW_POSE_STRIDE). */
  const DRAW_POSE_STRIDE = 8;
  /* Ints per actor in a `clip_states` export (FieldActors::CLIP_STATE_STRIDE). */
  const CLIP_STATE_STRIDE = 4;

  /* Pose an object-local mesh into `out` from one frame of a clip: per bone,
   * `Rz . Ry . Rx . v + T`. A character TMD's vertices are relative to their
   * own joint, so without this the parts pile on the origin. */
  function poseInto(out, base, objectIds, frames, partCount, frameIdx) {
    const ff = ((frameIdx % (frames.length / (partCount * 6))) + (frames.length / (partCount * 6)))
      % (frames.length / (partCount * 6));
    const sin = new Float32Array(partCount * 3);
    const cos = new Float32Array(partCount * 3);
    const tr  = new Float32Array(partCount * 3);
    for (let p = 0; p < partCount; p++) {
      const o = (ff * partCount + p) * 6;
      for (let k = 0; k < 3; k++) {
        const a = frames[o + 3 + k] * A2R;
        sin[p * 3 + k] = Math.sin(a);
        cos[p * 3 + k] = Math.cos(a);
        tr[p * 3 + k]  = frames[o + k];
      }
    }
    const n = base.length / 3;
    for (let v = 0; v < n; v++) {
      const o = objectIds[v];
      /* An object past the clip's bone count is not drawn: retail draws as
       * many objects as the clip has bones. The engine cuts the mesh to that
       * count (`mesh_cut`, re-uploaded by `rebind` when a cue binds a clip),
       * so this only guards a pose read in the frame between a cue and the
       * re-upload: surplus objects collapse to a point rather than litter the
       * actor's feet. */
      if (partCount > 0 && o >= partCount) {
        out[v * 3] = 0;
        out[v * 3 + 1] = 0;
        out[v * 3 + 2] = 0;
        continue;
      }
      if (o >= partCount) {
        out[v * 3] = base[v * 3];
        out[v * 3 + 1] = base[v * 3 + 1];
        out[v * 3 + 2] = base[v * 3 + 2];
        continue;
      }
      const sx = sin[o * 3],     cxx = cos[o * 3];
      const sy = sin[o * 3 + 1], cyy = cos[o * 3 + 1];
      const sz = sin[o * 3 + 2], czz = cos[o * 3 + 2];
      let x = base[v * 3], y = base[v * 3 + 1], z = base[v * 3 + 2];
      let ny = y * cxx - z * sx, nz = y * sx + z * cxx; y = ny; z = nz;
      let nx = x * cyy + z * sy;  nz = -x * sy + z * cyy; x = nx; z = nz;
      nx = x * czz - y * sz;      ny = x * sz + y * czz;  x = nx; y = ny;
      out[v * 3]     = x + tr[o * 3];
      out[v * 3 + 1] = y + tr[o * 3 + 1];
      out[v * 3 + 2] = z + tr[o * 3 + 2];
    }
  }

  /* Bind one export family by prefix. A missing export comes back as
   * `null`, so a stale cached WASM degrades the way the play page always
   * has (no clip states -> wall-clock animator, and so on). */
  function api(obj, prefix) {
    const f = (name) => (typeof obj[prefix + name] === 'function')
      ? obj[prefix + name].bind(obj) : null;
    return {
      catalog_json: f('catalog_json'),
      mesh: f('mesh'),
      mesh_positions: f('mesh_positions'),
      mesh_uvs: f('mesh_uvs'),
      mesh_cba_tsb: f('mesh_cba_tsb'),
      mesh_indices: f('mesh_indices'),
      mesh_object_ids: f('mesh_object_ids'),
      mesh_flat_rgba: f('mesh_flat_rgba'),
      mesh_cut: f('mesh_cut'),
      live_model: f('live_model'),
      pose_frames: f('pose_frames'),
      pose_dims: f('pose_dims'),
      draw_poses: f('draw_poses'),
      tints: f('tints'),
      clip_states: f('clip_states'),
      live_bones: f('live_bones'),
      morph_states: f('morph_states'),
      morph_base: f('morph_base'),
    };
  }

  /* Upload every catalogued actor's mesh once (ids `meshBase + i`) and pose
   * each to frame 0 - an unposed multi-object character is a heap of limbs
   * at the origin. Uploads EVERY placement, parked ones included: a
   * header-parked placement is exactly the actor a cutscene seats mid-visit,
   * and the per-frame draw skips anyone at the off-map hide box. */
  function upload(renderer, a, meshBase) {
    const recs = [];
    if (!a.catalog_json) return recs;
    const cat = JSON.parse(a.catalog_json() || 'null');
    if (!cat) return recs;
    for (const npc of cat.npcs) {
      let ok = true;
      try { a.mesh(npc.i); } catch (e) { ok = false; }
      if (!ok) continue;
      const base = a.mesh_positions();
      const idx = a.mesh_indices();
      if (!base.length || !idx.length) continue;
      const flat = a.mesh_flat_rgba();
      const meshId = meshBase + npc.i;
      renderer.uploadSceneMesh(meshId, base, a.mesh_uvs(),
        a.mesh_cba_tsb(), idx, flat.length ? flat : null);
      const frames = a.pose_frames(npc.i);
      const dims = a.pose_dims(npc.i);
      const rec = {
        i: npc.i, slot: npc.slot, meshId, base,
        meshCut: a.mesh_cut ? a.mesh_cut(npc.i) : -1,
        objectIds: a.mesh_object_ids(),
        frames, frameCount: dims[0], partCount: dims[1],
        out: new Float32Array(base.length), lastFrame: -1, lastGen: -1,
      };
      if (rec.frameCount > 0) {
        poseInto(rec.out, rec.base, rec.objectIds, rec.frames, rec.partCount, 0);
        renderer.updateSceneMeshPositions(meshId, rec.out);
      }
      recs.push(rec);
    }
    return recs;
  }

  /* Scripted mesh re-bind (the scripted-motion VM's op `0x0E`) and mesh
   * re-cut: re-upload an actor whose live model id or object cut moved since
   * its mesh was built. The engine's own cache key carries both, so asking
   * for an unchanged actor costs one call. */
  function rebind(renderer, a, recs) {
    if (!a.live_model || !recs) return;
    for (const rec of recs) {
      const id = a.live_model(rec.i);
      const modelMoved = id >= 0 && id !== rec.liveModel;
      const cut = a.mesh_cut ? a.mesh_cut(rec.i) : rec.meshCut;
      if (!modelMoved && cut === rec.meshCut) continue;
      if (modelMoved) rec.liveModel = id;
      rec.meshCut = cut;
      let ok = true;
      try { a.mesh(rec.i); } catch (e) { ok = false; }
      if (!ok) continue;
      const base = a.mesh_positions();
      const idx = a.mesh_indices();
      if (!base.length || !idx.length) continue;
      const flat = a.mesh_flat_rgba();
      renderer.uploadSceneMesh(rec.meshId, base, a.mesh_uvs(),
        a.mesh_cba_tsb(), idx, flat.length ? flat : null);
      /* The pose buffers are sized by the mesh, so they go with it. */
      rec.base = base;
      rec.objectIds = a.mesh_object_ids();
      rec.out = new Float32Array(base.length);
      rec.lastFrame = -1;
      rec.lastGen = -1;
      /* A morph staged on the old base must be re-read onto the new one. */
      rec.morphGen = undefined;
    }
  }

  /* One frame of the actor layer: re-pose each actor to the engine's current
   * clip frame (only when it changed), fold in an op-0x4B morph, and push a
   * draw at the world's live position / heading / tilt. `opts`:
   * `advance` (the page is not paused - drives the wall-clock fallback),
   * `skipSlot(slot)` (an actor another pass draws, e.g. a tile-board cell),
   * `extra` (fields merged into each draw). */
  function frame(renderer, a, recs, draws, opts) {
    if (!recs || !recs.length || !a.draw_poses) return;
    const o = opts || {};
    /* The engine's draw pose per actor (`World::field_npc_draw_pose`, the
     * native window's NPC transform): DRAW_POSE_STRIDE floats each -
     * visible, x, y (Y-up), z, yaw, pitch, roll (12-bit units), scale. The
     * page decides nothing about the transform: not the hide, not the
     * heading composition, not the render scale. */
    const np = a.draw_poses();
    /* Op `4C 81` draw tints, `[r, g, b, ir0]` per actor (empty while none
     * is tinted): a constant per-draw cue, the native NPC draw's twin. */
    const ntint = a.tints ? a.tints() : null;
    const clipStates = a.clip_states ? a.clip_states() : null;
    /* A per-entry morph generation that moves whenever the slot's staged
     * deltas do; on a move the object-local base is swapped for the
     * engine's morphed one and re-posed. `-1` = never armed. */
    const morphStates = a.morph_states ? a.morph_states() : null;
    const clipFrame = Math.floor(performance.now() / 1000 * NPC_CLIP_FPS);
    for (let k = 0; k < recs.length; k++) {
      const n = recs[k];
      const base = n.i * DRAW_POSE_STRIDE;
      if (base + DRAW_POSE_STRIDE > np.length) continue;
      /* Not drawn: parked in the off-map hide box, or at zero render scale. */
      if (!np[base]) continue;
      if (o.skipSlot && o.skipSlot(n.slot | 0)) continue;
      let morphMoved = false;
      if (morphStates && n.i < morphStates.length) {
        const mg = morphStates[n.i];
        if (mg >= 0 && mg !== n.morphGen) {
          const mb = a.morph_base(n.i);
          if (mb.length === n.base.length) {
            n.base = mb;
            morphMoved = true;
          }
          n.morphGen = mg;
        }
      }
      let posed = false;
      const cs = n.i * CLIP_STATE_STRIDE;
      if (clipStates && cs + CLIP_STATE_STRIDE <= clipStates.length) {
        const f = clipStates[cs];
        /* Re-target generation plus the look key's two halves
         * (`ActorLook::pose_key_bits`): a head turning on a held frame. */
        const gen = clipStates[cs + 1] + ':' + clipStates[cs + 2] + ':' + clipStates[cs + 3];
        if (f >= 0 && (morphMoved || f !== n.lastFrame || gen !== n.lastGen)) {
          const bones = a.live_bones(n.i);
          if (bones.length) {
            poseInto(n.out, n.base, n.objectIds, bones, bones.length / 6, 0);
            renderer.updateSceneMeshPositions(n.meshId, n.out);
            n.lastFrame = f; n.lastGen = gen;
            posed = true;
          }
        }
      } else if ((o.advance || morphMoved) && n.frameCount > 1) {
        const f = clipFrame % n.frameCount;
        if (morphMoved || f !== n.lastFrame) {
          poseInto(n.out, n.base, n.objectIds, n.frames, n.partCount, f);
          renderer.updateSceneMeshPositions(n.meshId, n.out);
          n.lastFrame = f;
          posed = true;
        }
      }
      /* A clip-less entry: its frame-0 rest pose (or its bare object-local
       * mesh) re-staged with the morphed base. */
      if (morphMoved && !posed) {
        if (n.frameCount > 0) {
          poseInto(n.out, n.base, n.objectIds, n.frames, n.partCount, 0);
          renderer.updateSceneMeshPositions(n.meshId, n.out);
        } else {
          renderer.updateSceneMeshPositions(n.meshId, n.base);
        }
      }
      const yaw = np[base + 4], pitch = np[base + 5], roll = np[base + 6];
      const actorDraw = Object.assign({
        meshId: n.meshId,
        x: np[base + 1], y: np[base + 2], z: np[base + 3],
        rotY: -yaw * A2R,
        scale: np[base + 7],
        /* The placement slot, for per-actor engine lookups (the
         * object-effect clip, `play_effect_clip`). */
        npcSlot: n.slot | 0,
      }, o.extra || {});
      const tk = n.i * 4;
      if (ntint && tk + 3 < ntint.length && ntint[tk + 3] > 0) {
        actorDraw.cue = { far: [ntint[tk], ntint[tk + 1], ntint[tk + 2]], nearZ: -1, farZ: 0, maxIr0: ntint[tk + 3] };
      }
      /* A tilted actor carries all three of retail's authored angles,
       * composed together (`FUN_8001ADA4` reads X at `+0`, Y at `+2`, Z at
       * `+4`); the yaw-only builder cannot express that, so it takes the
       * whole `Rx * Ry * Rz` model. */
      if (pitch || roll) {
        actorDraw.rotX = pitch * A2R;
        actorDraw.rotZ = roll * A2R;
        actorDraw.model = placementModelEuler(
          actorDraw.x, actorDraw.y, actorDraw.z,
          actorDraw.rotX, yaw * A2R, actorDraw.rotZ, actorDraw.scale);
      }
      draws.push(actorDraw);
    }
  }

  window.LegaiaFieldActors = { poseInto, api, upload, rebind, frame };
})();
