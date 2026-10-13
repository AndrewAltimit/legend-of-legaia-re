/* play-view-field-sync.js - PlayView's per-frame field state sync.
 *
 * Part of the PlayView class (play-app.js): what the engine reports about
 * the field scene each frame and the renderer has to follow - the static
 * placement window and cull, the field light, the view window, ground and
 * floor waves, and the live (moved / turned), parked, re-modelled and tinted
 * objects.
 *
 * Classic script. Requires play-app.js first. */
(function () {
  'use strict';

  window.LegaiaUtil.installMethods(window.PlayView, class {

    /* One engine frame + one draw (the draw is skipped while VR presents). */
    /* Re-derive every static draw's Y from the engine's LIVE floor-height
     * ladder.
     *
     * The sixteen-rung ladder a scene loads from its MAN header is not static:
     * field-VM op `0x4C` nibble-9 sub-`0xE` rewrites all sixteen rungs and
     * sub-`0..2` sets one oscillating every frame, which is the travelling
     * wave under `jou`'s organic Seru interior. Retail's per-cell terrain
     * emitters re-read the ladder every frame; this page bakes its draw
     * positions once per scene, so the engine hands back a per-draw Y offset
     * instead (`field_floor_wave_offsets`, terrain draws then placement draws)
     * and this folds it in. The array comes back EMPTY while the ladder sits
     * where the scene loaded it, so the common scene pays one call and no
     * iteration; `_floorWaveLive` is what returns the draws to their baked Y on
     * the falling edge.
     *
     * The page's world frame negates retail Y, so the offset subtracts. */
    /* The sub-area window sweep's placements live on the engine's windowed
     * static-object list (retail FUN_801D7B50, re-planned on every camera
     * re-centre). The engine answers which placement draws are live through
     * the same `field_env::placed_draw_live` kernel the native window asks;
     * the page re-reads that mask only when the list or the retail-windowing
     * option changed. With windowing off (the default) every entry is live. */
    _syncStaticWindow(rt) {
      if (typeof rt.field_placement_live !== 'function') return;
      const stamp = rt.field_static_window_stamp();
      if (stamp === this._staticWindowStamp) return;
      this._staticWindowStamp = stamp;
      const live = rt.field_placement_live();
      for (const d of this.staticDraws) {
        if (d.placeIdx === undefined) continue;
        d.winHidden = d.placeIdx < live.length && live[d.placeIdx] === 0;
        d.hidden = d.winHidden || !!d.cullHidden || !!d.parkHidden;
      }
    }

    /* Every placed object is a field actor, and retail's actor tick culls it
     * (FUN_801D79E8: outside the region box, or outside the visible tile
     * window widened by its record's cull radius) - the draw walk then skips
     * it. The engine answers through the same
     * `field_view_window::placed_actor_visible` kernel the native window asks;
     * the mask is empty while the visible-tile crop does not apply, and the
     * page re-reads it only when the cull view moves. */
    _syncPlacementCull(rt) {
      if (typeof rt.field_placement_cull_stamp !== 'function') return;
      const stamp = rt.field_placement_cull_stamp(this.debugCamera);
      if (stamp === this._placementCullStamp) return;
      this._placementCullStamp = stamp;
      const culled = stamp ? rt.field_placement_culled(this.debugCamera) : [];
      for (const d of this.staticDraws) {
        if (d.placeIdx === undefined) continue;
        d.cullHidden = d.placeIdx < culled.length && culled[d.placeIdx] === 1;
        d.hidden = !!d.winHidden || d.cullHidden || !!d.parkHidden;
      }
    }

    /* Retail's visible-tile crop (FUN_801F7088 and the ground emitters it
     * calls): only the cells the camera's tile window reaches are drawn. The
     * engine answers through the same `field_view_window` kernel the native
     * window asks - a per-terrain-draw mask plus a cropped ground index list -
     * and the page re-reads both only when the rectangle's stamp moves. It
     * applies at retail framing only (the camera-distance preset at Retail, no
     * drag / zoom, and never under `F3`), so the default page draws the map
     * whole. */
    /* Upload one lit env copy: slot `rec.slot` shaded at the draw rotation
     * `(rec.rx, rec.ry, rec.rz)` under the world's live field light. */
    _uploadLitEnvMesh(rt, rec) {
      try {
        if (rec.anim) rt.field_mesh_posed_lit(rec.slot, rec.anim, rec.rx, rec.ry, rec.rz);
        else rt.field_mesh_lit(rec.slot, rec.rx, rec.ry, rec.rz);
      }
      catch (e) { return false; }
      const pos = rt.field_mesh_positions();
      const idx = rt.field_mesh_indices();
      if (!pos.length || !idx.length) return false;
      const flat = rt.field_mesh_flat_rgba();
      this.renderer.uploadSceneMesh(rec.meshId, pos, rt.field_mesh_uvs(),
        rt.field_mesh_cba_tsb(), idx, flat.length ? flat : null);
      return true;
    }

    /* Re-shade the lit env copies when the live field light moves (op
     * 4C 8A - koin3's cutscene records drop it to black and back). */
    _syncFieldLight(rt) {
      if (!this.litEnvMeshes || !this.litEnvMeshes.length
          || typeof rt.field_light_key !== 'function') return;
      const key = rt.field_light_key();
      if (key === this._fieldLightKey) return;
      this._fieldLightKey = key;
      for (const rec of this.litEnvMeshes) this._uploadLitEnvMesh(rt, rec);
    }

    _syncViewWindow(rt) {
      if (typeof rt.field_view_window_stamp !== 'function') return;
      const stamp = rt.field_view_window_stamp(this.debugCamera);
      if (stamp === this._viewWindowStamp) return;
      this._viewWindowStamp = stamp;
      const live = rt.field_terrain_live(this.debugCamera);
      for (const d of this.staticDraws) {
        if (d.terrainIdx === undefined) continue;
        d.hidden = d.terrainIdx < live.length && live[d.terrainIdx] === 0;
      }
      if (this.renderer.setGroundIndices && rt.field_ground_quad_count() > 0) {
        this.renderer.setGroundIndices(rt.field_ground_indices_cropped(this.debugCamera));
      }
    }

    /* The ground surface itself under the live ladder: retail's ground pass
     * (PROT 0900 `FUN_801F6D48`) takes each cell's four corner tiers through
     * the ladder on every frame it draws, so the walk ground deforms per
     * vertex - jouina's pulsing path, concnow's flesh pits - not just the
     * meshes standing on it. `field_ground_live_positions` comes back empty
     * on a frame the ladder did not move. */
    _applyGroundWave(rt) {
      if (!rt.field_ground_live_positions || !this.renderer.updateGroundPositions) return;
      const pos = rt.field_ground_live_positions();
      if (!pos.length) return;
      this.renderer.updateGroundPositions(pos,
        (typeof rt.field_ground_flat_refs === 'function') ? rt.field_ground_flat_refs() : null);
    }

    _applyFloorWave(rt) {
      if (!rt.field_floor_wave_offsets) return;
      const wave = rt.field_floor_wave_offsets();
      if (!wave.length) {
        if (!this._floorWaveLive) return;
        for (const d of this.staticDraws) {
          if (d.floorIdx === undefined || d.liveModel) continue;
          d.y = d.baseY;
          if (d.model) d.model[13] = d.y;
        }
        this._floorWaveLive = false;
        return;
      }
      for (const d of this.staticDraws) {
        if (d.floorIdx === undefined || d.floorIdx >= wave.length || d.liveModel) continue;
        d.y = d.baseY - wave[d.floorIdx];
        if (d.model) d.model[13] = d.y;
      }
      this._floorWaveLive = true;
    }

    /* A placed object a script has moved (an `A3` seat, an op-`4C 42` lift -
     * chitei2's falling boulder) or turned (op 0x38, `4C 48` - town0c's exit
     * rocks) draws at its actor, as retail's case-5 draw does. The engine
     * composes the whole model - spawn transform, floor offset, turn about
     * the draw's origin, displacement - through the kernel the native window
     * runs (`field_env::live_placed_model`) and hands it over in the page's
     * frame: 16 floats per placement, all zero for one nothing has touched,
     * EMPTY while nothing is live. The page does no matrix math here; it
     * adopts the model or returns the draw to the one it baked. Runs BEFORE
     * `_applyFloorWave`, which leaves a live draw's Y to the engine and
     * re-seats a draw this pass has just released. */
    _applyObjectLive(rt) {
      if (!rt.field_placement_live_models) return;
      const live = rt.field_placement_live_models();
      if (!live.length && !this._objectLive) return;
      let released = false;
      for (const d of this.staticDraws) {
        if (d.placeIdx === undefined) continue;
        const k = d.placeIdx * 16;
        if (k + 15 < live.length && live[k + 15] !== 0) {
          if (!d.liveModel) {
            d.liveModel = true;
            d.bakedModel = d.model || null;
          }
          d.model = live.subarray(k, k + 16);
          d.x = d.model[12];
          d.y = d.model[13];
          d.z = d.model[14];
        } else if (d.liveModel) {
          d.liveModel = false;
          if (d.bakedModel) d.model = d.bakedModel; else delete d.model;
          d.bakedModel = null;
          d.x = d.baseX;
          d.z = d.baseZ;
          released = true;
        }
      }
      /* A released draw's Y is the floor pass's again. */
      if (released) this._floorWaveLive = true;
      this._objectLive = live.length > 0;
    }

    /* A camera-facing terrain / decoration cell (record flags +0x12 & 0x380:
     * rugi's candle glows, the vell forest trees) is rebuilt against this
     * frame's engine camera: retail's decoration pass FUN_801F7088 drops the
     * flagged axes from the camera rotation before the cell's own. The engine
     * hands back [terrain index, 9 column-major floats] per cell - the linear
     * part in the page frame - or an EMPTY array (no such cell, or the page
     * drew through its own orbit, when every cell returns to its baked
     * model). The native window rebuilds the same draws
     * (`gte::decoration_cell_basis`). Runs after the camera is staged. */
    _applyTerrainFacing(rt) {
      if (!rt || typeof rt.field_terrain_facing !== 'function') return;
      const vp = this.cam && this.cam.vp;
      const m = vp ? rt.field_terrain_facing(vp) : [];
      if (!m.length && !this._terrainFacingLive) return;
      if (!this._terrainByIdx) {
        this._terrainByIdx = new Map();
        for (const d of this.staticDraws) {
          if (d.terrainIdx !== undefined) this._terrainByIdx.set(d.terrainIdx, d);
        }
      }
      const seen = new Set();
      for (let k = 0; k + 9 < m.length; k += 10) {
        const d = this._terrainByIdx.get(m[k]);
        if (!d) continue;
        if (d.facingBase === undefined) d.facingBase = d.model || null;
        d.model = new Float32Array([
          m[k + 1], m[k + 2], m[k + 3], 0,
          m[k + 4], m[k + 5], m[k + 6], 0,
          m[k + 7], m[k + 8], m[k + 9], 0,
          d.x, d.y, d.z, 1,
        ]);
        seen.add(d);
      }
      if (this._terrainFacingLive) {
        for (const d of this._terrainFacingLive) {
          if (seen.has(d)) continue;
          if (d.facingBase) d.model = d.facingBase; else delete d.model;
          delete d.facingBase;
        }
      }
      this._terrainFacingLive = seen.size ? seen : null;
    }

    /* A placed object a script parks at the hide box after the scene built
     * its draws (rugi's entry script runs `A3 06 7F 7F` on the stone block the
     * opened wall leaves behind) stops drawing: retail's case-5 draw reads the
     * actor, which now stands off the map. The engine hands back a
     * per-placement mask (1 = parked) or an EMPTY array while none is;
     * `_objectParkedLive` clears the flags on the falling edge. The native
     * window skips the same set (`World::hidden_object_records`). */
    _applyObjectParked(rt) {
      if (!rt.field_placement_parked) return;
      const m = rt.field_placement_parked();
      if (!m.length && !this._objectParkedLive) return;
      for (const d of this.staticDraws) {
        if (d.placeIdx === undefined) continue;
        const parked = d.placeIdx < m.length && m[d.placeIdx] === 1;
        if (parked === !!d.parkHidden) continue;
        d.parkHidden = parked;
        d.hidden = !!d.winHidden || !!d.cullHidden || parked;
      }
      this._objectParkedLive = m.length > 0;
    }

    /* A placed object whose actor carries a draw tint (op `4C 81`: the
     * `+0x74` colour / `+0x78` blend retail stages as the far colour and
     * IR0 - chitei2's hologram panels go black once the generator is down)
     * draws with a constant per-draw cue. The engine hands back a
     * per-placement [r, g, b, ir0] or an EMPTY array while nothing is
     * tinted. The native window stages the same table
     * (`World::object_draw_tints`) on its placed draws. */
    /* A placed object whose scripted-motion stream swapped its model (op
     * 0x0E - koin3's video wall cycles its panels) draws with the swapped-in
     * env slot's mesh. The engine hands back a per-placement slot (-1 =
     * none) or an EMPTY array while no stream has swapped one; the native
     * window draws through the same table (`World::object_live_models`). */
    _applyObjectModels(rt) {
      if (!rt.field_placement_models || !this._ensureEnvMesh) return;
      const m = rt.field_placement_models();
      if (!m.length && !this._objectModelsLive) return;
      for (const d of this.staticDraws) {
        if (d.placeIdx === undefined || d.baseMeshId === undefined) continue;
        const slot = d.placeIdx < m.length ? m[d.placeIdx] : -1;
        let id = d.baseMeshId;
        if (slot >= 0) {
          const swapped = this._ensureEnvMesh(slot);
          if (swapped >= 0) id = swapped;
        }
        d.meshId = id;
      }
      this._objectModelsLive = m.length > 0;
    }

    _applyObjectTints(rt) {
      if (!rt.field_placement_tints) return;
      const t = rt.field_placement_tints();
      if (!t.length && !this._objectTintsLive) return;
      for (const d of this.staticDraws) {
        if (d.placeIdx === undefined) continue;
        const k = d.placeIdx * 4;
        if (k + 3 < t.length && t[k + 3] > 0) {
          d.cue = { far: [t[k], t[k + 1], t[k + 2]], nearZ: -1, farZ: 0, maxIr0: t[k + 3] };
        } else if (d.cue) {
          delete d.cue;
        }
      }
      this._objectTintsLive = t.length > 0;
    }

    /* Object-effect clip (field-VM `4C C2` raises an actor's `+0x42`):
     * retail clips that actor's mesh to the slab its object-effect row
     * stages. Each placed object (`placeIdx`) and NPC (`npcSlot`) draw asks
     * the engine's `play_effect_clip` for its mesh-space slab, handing the
     * page model it will draw with; the native window stages the same
     * kernel (`World::object_effect_mesh_clip`) on its draws. One cheap
     * `play_effect_clip_live` call a frame while no actor has it raised. */
    _applyEffectClips(rt, draws) {
      if (typeof rt.play_effect_clip_live !== 'function') return;
      let live = false;
      try { live = rt.play_effect_clip_live(); } catch (e) { live = false; }
      if (!live && !this._effectClipsLive) return;
      for (const d of draws) {
        let kind = -1, key = 0;
        if (d.placeIdx !== undefined) { kind = 0; key = d.placeIdx; }
        else if (d.npcSlot !== undefined) { kind = 1; key = d.npcSlot; }
        else if (d.effectPlayer) { kind = 2; key = 0; }
        if (kind < 0) continue;
        if (!live) { delete d.effectClip; continue; }
        const m = this.renderer.sceneMeshes && this.renderer.sceneMeshes.get(d.meshId);
        if (!m) continue;
        const model = this.renderer._placementModel(d, m);
        let c = null;
        try { c = rt.play_effect_clip(kind, key >>> 0, model); } catch (e) { c = null; }
        if (c && c.length >= 6) d.effectClip = c; else delete d.effectClip;
      }
      this._effectClipsLive = live;
    }

    /* Scripted mesh re-bind (the scripted-motion VM's op `0x0E`): the engine
     * records the actor's new model id per placement slot, and
     * `play_npc_live_model` reports it per catalog entry (`-1` = still the
     * spawn model). An NPC's mesh is uploaded once at scene build, so a swap
     * the script makes later needs this re-upload; the engine's own cache key
     * carries the bound id, so asking for an unswapped NPC costs one call and
     * hands back the same mesh. The native window's twin is
     * `rebind_live_npc_models` in its asset uploader - same world field, same
     * `SceneModelBank` resolve.
     *
     * The same re-upload follows a move in the mesh's object cut
     * (`play_npc_mesh_cut`): retail draws as many objects as the actor's clip
     * has bones, and a placement whose first clip came from a later ANIMATE
     * cue (rikuroa's party Noa) was built uncut - the native window cuts it
     * at pose time. */
    _rebindLiveNpcModels(rt) {
      if (!this._actorApi) this._actorApi = window.LegaiaFieldActors.api(rt, 'play_npc_');
      window.LegaiaFieldActors.rebind(this.renderer, this._actorApi, this.npcs);
    }
  });
})();
