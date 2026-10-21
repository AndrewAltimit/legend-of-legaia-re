/* play-view-scene-build.js - PlayView's scene upload.
 *
 * Part of the PlayView class (play-app.js): _rebuild, which uploads a scene
 * on entry - VRAM, the static map and ground, the player and NPC meshes -
 * and the tile-board mesh sync.
 *
 * Classic script. Requires play-view-constants.js and play-app.js first. */
(function () {
  'use strict';

  const {
    A2R, PLAYER_MESH_ID, NPC_MESH_BASE, TILE_MESH_BASE,
  } = window.LegaiaPlayShared;

  window.LegaiaUtil.installMethods(window.PlayView, class {

    /* Rebuild the GPU-side scene from whatever the engine currently holds.
     * Runs on entry and whenever the engine walks through a door. */
    _rebuild() {
      /* Per-scene FX mesh caches + dynamic actor set (mesh ids are per
       * scene upload; a rebuild replaces the whole scene mesh table). */
      this._fieldFx = { fxMeshes: new Map(), packMeshes: new Map() };
      this._dynActors = new Set();
      const rt = this.rt;
      this.renderer.clearScene();
      this.staticDraws = [];
      this._terrainByIdx = null;
      this._terrainFacingLive = null;
      this._staticWindowStamp = undefined;
      this._viewWindowStamp = undefined;
      this._placementCullStamp = undefined;
      this.player = null;
      this.npcs = [];
      this.tileMeshSlots = [];   /* board-owned actor slots with an uploaded mesh */
      this.tileActorSlots = new Set();   /* every board-owned slot, drawn or not */
      this.animProps = [];
      this._battle = null;   /* scene swap invalidates the battle upload */
      /* Scene-owned caption image (the prologue's baked TIM): re-resolve on
       * the next draw that needs it. */
      this._captionBlit = undefined;

      this.renderer.uploadVram(rt.field_vram_bytes());

      if (rt.field_ground_quad_count() > 0) {
        this.renderer.uploadGround(
          rt.field_ground_positions(), rt.field_ground_uvs(),
          rt.field_ground_cba_tsb(), rt.field_ground_indices(),
          (typeof rt.field_ground_flat_refs === 'function')
            ? rt.field_ground_flat_refs() : null);
      } else {
        this.renderer.uploadGround(new Float32Array(0), null, null, new Uint32Array(0));
      }

      /* Environment meshes, uploaded once per (slot, anim) pair and instanced
       * per placement. `anim` selects the frame-0 **posed** variant of the
       * slot's mesh: a placed prop whose object bind names a clip is a
       * multi-object mesh whose parts are that clip's bones - cupboard doors
       * only sit on the cabinet's front face, and windmill sails on their hub,
       * once the pose is applied (the WASM side falls back to the raw mesh
       * when the pose can't resolve, exactly as the native window does). */
      const POSED_MESH_BASE = 700000;   /* + slot*256 + anim */
      const empty = new Set(), used = new Set();
      const ensure = (slot, anim) => {
        const meshId = anim ? POSED_MESH_BASE + slot * 256 + anim : slot;
        const key = meshId;
        if (used.has(key)) return meshId;
        if (empty.has(key)) return -1;
        try { rt.field_mesh_posed(slot, anim || 0); }
        catch (e) { empty.add(key); return -1; }
        const pos = rt.field_mesh_positions();
        const idx = rt.field_mesh_indices();
        if (!pos.length || !idx.length) { empty.add(key); return -1; }
        const flat = rt.field_mesh_flat_rgba();
        this.renderer.uploadSceneMesh(meshId, pos, rt.field_mesh_uvs(),
          rt.field_mesh_cba_tsb(), idx, flat.length ? flat : null);
        used.add(key);
        return meshId;
      };
      /* The env-slot uploader, kept for `_applyObjectModels` (a motion
       * stream's op-0x0E model swap draws a placed object with another
       * slot's mesh). */
      this._ensureEnvMesh = (slot) => ensure(slot, 0);
      /* Per-animated-placement mesh id space (above the shared env-slot ids and
       * the posed frame-0 variants, below the player / NPC ids). One mesh per
       * animated placement so two props sharing an env slot can sit on
       * different clip frames. */
      const ANIM_PROP_BASE = 800000;
      const uploadPosedInstance = (meshId, slot, anim) => {
        try { rt.field_mesh_posed(slot, anim); }
        catch (e) { return false; }
        const pos = rt.field_mesh_positions();
        const idx = rt.field_mesh_indices();
        if (!pos.length || !idx.length) return false;
        const flat = rt.field_mesh_flat_rgba();
        this.renderer.uploadSceneMesh(meshId, pos, rt.field_mesh_uvs(),
          rt.field_mesh_cba_tsb(), idx, flat.length ? flat : null);
        return true;
      };
      /* Light-source rows (TMD group flags 0x10..0x17): retail shades each
       * corner through the GTE light against the draw's world normal, so a
       * slot carrying them gets one shaded copy per draw rotation - the
       * `field_lit_mesh` kernel the native window runs. `_syncFieldLight`
       * re-shades them when the live light changes (op 4C 8A). */
      const LIT_MESH_BASE = 1400000;
      this.litEnvMeshes = [];
      this._fieldLightKey = (typeof rt.field_light_key === 'function') ? rt.field_light_key() : '';
      const litSlot = new Map(), litIds = new Map();
      const ensureLit = (slot, ry, rx, rz) => {
        if (typeof rt.field_mesh_lit !== 'function') return -1;
        let has = litSlot.get(slot);
        if (has === undefined) {
          has = !!rt.field_mesh_has_lit_rows(slot);
          litSlot.set(slot, has);
        }
        if (!has) return -1;
        const rec = { meshId: 0, slot, rx: rx & 0xFFF, ry: ry & 0xFFF, rz: rz & 0xFFF };
        const key = `${slot}:${rec.rx}:${rec.ry}:${rec.rz}`;
        const known = litIds.get(key);
        if (known !== undefined) return known;
        rec.meshId = LIT_MESH_BASE + this.litEnvMeshes.length;
        if (!this._uploadLitEnvMesh(rt, rec)) return -1;
        this.litEnvMeshes.push(rec);
        litIds.set(key, rec.meshId);
        return rec.meshId;
      };
      /* `floorBase` is where this list starts inside the concatenated
       * floor-wave offset array (`rt.field_floor_wave_offsets()`, terrain then
       * placements), so a draw the loop below SKIPS - a mesh with
       * no renderable prims - does not shift every later draw's rung. */
      const push = (slots, pos, rots, anims, rotsX, rotsZ, floorBase, placed, scales, decoStart) => {
        for (let i = 0; i < slots.length; i++) {
          const anim = anims ? anims[i] : 0;
          let meshId, animRec = null;
          if (anim) {
            /* Animated prop: its own instance, uploaded at the rest pose
             * (frame 0) and re-posed per frame from the engine's live cursor. */
            meshId = ANIM_PROP_BASE + i;
            /* A prop carrying light-source rows is shaded at frame 0 and
             * at its own rotation (`field_mesh_posed_lit`); re-poses move
             * its positions only. */
            const litRec = (typeof rt.field_mesh_posed_lit === 'function'
              && rt.field_mesh_has_lit_rows(slots[i]))
              ? { meshId, slot: slots[i], anim,
                  rx: rotsX ? (rotsX[i] & 0xFFF) : 0, ry: rots ? (rots[i] & 0xFFF) : 0,
                  rz: rotsZ ? (rotsZ[i] & 0xFFF) : 0 }
              : null;
            if (litRec) {
              if (!this._uploadLitEnvMesh(rt, litRec)) continue;
              this.litEnvMeshes.push(litRec);
            } else if (!uploadPosedInstance(meshId, slots[i], anim)) continue;
            animRec = { meshId, i, slot: slots[i], anim, lastFrame: 0 };
          } else {
            meshId = ensureLit(slots[i], rots ? rots[i] : 0,
              rotsX ? rotsX[i] : 0, rotsZ ? rotsZ[i] : 0);
            if (meshId < 0) meshId = ensure(slots[i], 0);
            if (meshId < 0) continue;
          }
          /* Sky domes and kilometre-wide horizon planes are scene geometry
           * like any other placement: retail draws them, and so does the
           * native window, whose field pass has no sky classifier. The page
           * used to drop them here (the full-map viewer's classifier), which
           * left the opdeene prologue's sepia sky and ridge line a navy void
           * behind the tableau. */
          const draw = {
            meshId,
            x: pos[i * 3], y: -pos[i * 3 + 1], z: pos[i * 3 + 2],
            rotY: -(rots[i] & 0xFFF) * A2R,
            /* The bind record's render scale (`actor[+0x72]`, retail's
             * case-5 `ScaleMatrix`) - 1.0 for nearly every placement. */
            scale: (scales && scales.length > i) ? scales[i] : 1.0,
          };
          /* A placement with an authored X/Z tilt cannot go through the
           * yaw-only path: that builder's negated-yaw convention is a
           * cancellation specific to Ry (see webgl-math.js). Hand it a whole
           * model matrix composed in retail's Rx*Ry*Rz order instead - the
           * same composition crates/engine-render battle_intro::
           * placement_rotation applies natively. Most placements are pure
           * yaw and keep the cheaper path. */
          const rx = rotsX ? (rotsX[i] & 0xFFF) : 0;
          const rz = rotsZ ? (rotsZ[i] & 0xFFF) : 0;
          if (rx || rz) {
            draw.model = placementModelEuler(
              draw.x, draw.y, draw.z,
              rx * A2R, (rots[i] & 0xFFF) * A2R, rz * A2R, draw.scale);
          }
          /* Floor-wave bookkeeping: the rung index this draw's Y came from,
           * and the Y the scene's baked ladder gave it. `_applyFloorWave`
           * re-derives `y` from these when the ladder moves. */
          if (floorBase !== undefined) {
            draw.floorIdx = floorBase + i;
            draw.baseY = draw.y;
            draw.eulerY = (rx || rz)
              ? [rx * A2R, (rots[i] & 0xFFF) * A2R, rz * A2R] : null;
          }
          /* Placed layer only: the index into the engine's per-placement
           * live mask (`_syncStaticWindow`). */
          if (placed) {
            draw.placeIdx = i;
            /* The mesh the placement draws with no model swap live. */
            if (!anim) draw.baseMeshId = meshId;
            /* Where the record put it, for `_applyObjectLive`'s release. */
            draw.baseX = draw.x;
            draw.baseZ = draw.z;
            /* Overworld decorations (the placed list past the landmarks):
             * retail's decoration sweep hazes each one toward 0xD0 by its
             * origin's camera depth - staged per draw by renderAssembled
             * (`decoCue`, the native `world_map_deco_start` twin). */
            if (decoStart !== undefined && i >= decoStart) draw.decoCue = true;
          } else {
            /* Terrain layer only: the index into the engine's visible-tile
             * crop mask (`_syncViewWindow`). This was the `else` of the
             * decoration test, so every non-decoration PLACED draw took a
             * terrain index too, and at retail framing the crop hid
             * placements by the mask entry of an unrelated terrain tile. */
            draw.terrainIdx = i;
          }
          this.staticDraws.push(draw);
          if (animRec) this.animProps.push(animRec);
        }
      };
      const terrainSlots = rt.field_terrain_slots();
      /* Terrain cells carry the record's authored pitch / roll too (vell's
       * trees, bubu1's tilted slabs); the native window composes all three. */
      push(terrainSlots, rt.field_terrain_positions(), rt.field_terrain_rot_y(), null,
        rt.field_terrain_rot_x ? rt.field_terrain_rot_x() : null,
        rt.field_terrain_rot_z ? rt.field_terrain_rot_z() : null, 0);
      push(rt.field_placement_slots(), rt.field_placement_positions(), rt.field_placement_rot_y(),
        rt.field_placement_anim_ids(),
        rt.field_placement_rot_x ? rt.field_placement_rot_x() : null,
        rt.field_placement_rot_z ? rt.field_placement_rot_z() : null,
        terrainSlots.length, true,
        rt.field_placement_scales ? rt.field_placement_scales() : null,
        rt.field_decoration_start ? rt.field_decoration_start() : undefined);
      this._floorWaveLive = false;
      this._objectLive = false;
      this._objectTintsLive = false;

      /* Player: geometry once, positions re-uploaded per frame from the pose. */
      this._uploadPlayer();

      /* NPCs: the scene's MAN placements. Upload EVERY placement, parked
       * ones included, and let the per-frame draw skip anyone whose live
       * position is the off-map hide box - the native window's upload rule
       * (`upload_assets`). A header-parked placement is exactly the actor a
       * cutscene seats mid-visit (`A3 <ch> x z`, `CC <ch> 37` - Maya's first
       * meeting in bylon, Noa / Gala materializing beside the player), long
       * after this bind ran; skipping it here left the actor meshless for the
       * whole visit while the engine had it standing on stage. */
      /* The shared actor draw path (js/field-actors.js) over this page's
       * `play_npc_*` exports - the same path, over the same Rust actor layer,
       * the map viewer runs. */
      this._actorApi = window.LegaiaFieldActors.api(rt, 'play_npc_');
      this.npcs = window.LegaiaFieldActors.upload(this.renderer, this._actorApi, NPC_MESH_BASE);

      /* Frame the camera on the player straight away so the first painted frame
       * is already looking at them. */
      this._stageEngineCamera();
    }

    /* Upload a scene mesh for every board-owned actor slot the engine reports,
     * once each. Cheap when no board is installed: the engine returns an empty
     * slot list and this is two array reads.
     *
     * `tileActorSlots` is the exclusion set the generic actor loop consults -
     * the browser twin of the native redraw pass's `is_tile_actor_slot` skip.
     * Teardown (the script closing the board) empties both lists, so a second
     * board in the same scene re-uploads rather than drawing the first one's
     * meshes at the new cells. */
    /* Upload the lead's rig geometry. Runs at scene build, and again when
     * the engine reports a mid-scene rig rebuild (`player_rig_rebuilt`: a
     * lead change, a `CC F8 50` re-stage) - the per-frame pose only
     * replaces positions, so a new rig needs its indices / UVs / texture
     * words too. The native window re-uploads on the same signal. */
    _uploadPlayer() {
      const rt = this.rt;
      this.player = null;
      if (!rt.player_has_mesh()) return;
      const base = rt.player_mesh_positions();
      const idx = rt.player_mesh_indices();
      if (!base.length || !idx.length) return;
      const flat = rt.player_mesh_flat_rgba();
      this.renderer.uploadSceneMesh(PLAYER_MESH_ID, base, rt.player_mesh_uvs(),
        rt.player_mesh_cba_tsb(), idx, flat.length ? flat : null);
      this.player = { verts: base.length / 3 };
    }

    _syncTileBoard(rt) {
      const owned = rt.play_tile_actor_slots();
      if (!owned.length) {
        if (this.tileMeshSlots.length) this.tileMeshSlots = [];
        if (this.tileActorSlots.size) this.tileActorSlots = new Set();
        return;
      }
      this.tileActorSlots = new Set(Array.from(owned, (s) => s | 0));
      const want = rt.play_tile_board_slots();
      for (let i = 0; i < want.length; i++) {
        const slot = want[i] | 0;
        if (this.tileMeshSlots.indexOf(slot) >= 0) continue;
        let ok = true;
        try { rt.play_tile_actor_mesh(slot); } catch (e) { ok = false; }
        if (!ok) continue;
        const pos = rt.play_tile_actor_mesh_positions();
        const idx = rt.play_tile_actor_mesh_indices();
        if (!pos.length || !idx.length) continue;
        const flat = rt.play_tile_actor_mesh_flat_rgba();
        this.renderer.uploadSceneMesh(TILE_MESH_BASE + slot, pos,
          rt.play_tile_actor_mesh_uvs(), rt.play_tile_actor_mesh_cba_tsb(),
          idx, flat.length ? flat : null);
        this.tileMeshSlots.push(slot);
      }
    }
  });
})();
