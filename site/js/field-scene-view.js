/* Assembled full-map view: load a CDNAME scene through the engine's real
 * loaders and render the map it assembles into.
 *
 * The WASM side does the work (`set_scene_field`): field VRAM pre-pass, LZS
 * environment mesh pack, `.MAP` object-grid placements, the floor-height LUT,
 * the walk-ground heightfield. This module streams that into the shared
 * `TmdRenderer`'s instanced scene-mesh path - the same plumbing the
 * world-overview page uses for the kingdom continents.
 *
 * Shared by the game-world page (its town navigator swaps scenes in place) and
 * the asset viewer (its per-entry "full map" button). One instance owns one
 * canvas + one renderer + one set of camera controls for its whole lifetime and
 * `load()` is re-entrant, so swapping scenes doesn't leak GL objects or stack
 * event listeners.
 *
 * Requires webgl-math.js + webgl-shaders.js + webgl-tmd.js + field-actors.js to
 * be loaded first.
 *
 *   const view = new FieldSceneView(wasmViewer, canvasEl);
 *   const st = view.load('town01');   // throws on failure
 *   view.dispose();
 */
(function () {
  'use strict';

  /* Sky-dome / horizon-backdrop classifier. Open maps place their sky as
   * environment meshes too: a hemispherical cloud shell over the whole map
   * (rikuroa slot 37, town01 slot 84) and kilometre-wide vertical horizon
   * planes (town01 slot 85 spans 17920 units). Under the retail in-world camera
   * they read as sky; under this view's assembled camera they draw ON TOP of
   * the terrain and hide the map, so they're excluded from the draw list (and
   * hence from the framing AABB).
   *
   * Calibrated against the disc: a *backdrop plane* is a near-zero-depth
   * vertical sheet wider than any real wall (town cliffs are thick, and flat
   * floor slabs are horizontal); a *dome shell* is huge on BOTH horizontal axes
   * AND tall (interior floor slabs like korb3's 3584-unit carpets are flat;
   * mountain walls like rikuroa slot 32 are long but shallow) AND sparse -
   * every genuine shell in the corpus is 42-51 verts stretched over a
   * 7k-18k span (teien slot 3, town01 slots 84/85, rikuroa slot 37), while
   * real geometry that big is dense: kor5 slot 3 - the town's ENTIRE tiled
   * paving + walkway floor, 459 verts over 4224x3680 - matched the old
   * AABB-only arm and the whole plaza floor silently vanished into the sky
   * bucket. `vertCount` 0/undefined (metadata unavailable) keeps the
   * pre-guard behaviour. */
  function isSkyMesh(aabb, vertCount) {
    if (!aabb) return false;
    /* `vertCount` is TmdRenderer.getMeshVertexCount = positions.length / 3 -
     * the UPLOADED per-primitive-corner count, because legaia_tmd emits one
     * position per corner with no dedup. That is ~4x the TMD vertex count the
     * 96 was calibrated against (kor5 slot 3: 459 TMD verts, 1714 uploaded),
     * so nearly every sky shell scored as dense and got drawn - the "extra
     * sky domes" symptom. Corpus separation in uploaded counts: shells run
     * 48..480, the densest real geometry clearing the AABB arm starts at 582,
     * and kor5's plaza paving (what this guard exists for) is 1714. */
    const sparse = !vertCount || vertCount <= 512;
    const flatPlane = Math.min(aabb.sx, aabb.sz) < 8
      && Math.max(aabb.sx, aabb.sz) > 3000 && aabb.sy > 600;
    const domeShell = aabb.sx > 3400 && aabb.sz > 3400 && aabb.sy > 800 && sparse;
    return flatPlane || domeShell;
  }

  /* World units per metre for the VR path. The field character mesh stands
   * ~130 units tall (the play page's follow camera is tuned around that), so a
   * 1.7 m human puts a metre at ~76 world units - which also makes the 128-unit
   * walkability tile a believable 1.7 m stride. At this scale a headset stands
   * *in* the town at human height. See docs/subsystems/vr-mode.md. */
  const VR_UNITS_PER_METER = 76;

  /* Mesh-id space of the actor layer (the play page's NPC_MESH_BASE). */
  const NPC_MESH_BASE = 910000;

  class FieldSceneView {
    /* `viewer` is the WASM LegaiaViewer; `canvas` an existing <canvas> already
     * in the DOM (it must not have been used for a 2D context - a canvas can
     * only ever bind one context type). `opts`: minHalf / maxHalf zoom clamps,
     * `vrMount` (element the "Enter VR" button is appended to). */
    constructor(viewer, canvas, opts) {
      if (typeof window.TmdRenderer === 'undefined') {
        throw new Error('TmdRenderer global missing (webgl-tmd.js not loaded?)');
      }
      this.viewer = viewer;
      this.canvas = canvas;
      this.renderer = new window.TmdRenderer(canvas);
      this.raf = 0;
      this.state = null;
      /* `yaw` present -> renderAssembled picks the perspective orbit camera
       * (buildWorldOrbitVp), the same projection the world-overview page uses,
       * so the shared pan/pivot/zoom controls behave identically here.
       * Mutated in place by the controls and by each load()'s re-framing, so
       * the controls only ever bind once. */
      this.cam = {
        centerX: 0, centerZ: 0,
        halfWidth: 4000, halfHeight: 4000,
        yaw: 0, pitch: 0.65,
      };
      /* Wider zoom clamp than the kingdom default - scenes range from
       * single-room interiors to whole towns. */
      const o = opts || {};
      attachWorldOrbitControls(canvas, this.cam, {
        minHalf: o.minHalf != null ? o.minHalf : 150,
        maxHalf: o.maxHalf != null ? o.maxHalf : 40000,
      });

      /* Live draw list + world extent of the loaded scene, kept on the instance
       * so the VR loop can re-issue the exact same draw the flat loop does. */
      this.draws = [];
      this.ext = [16384, 16384];
      /* {walker_entries, ambient_parts} once field_scene_anim_init reports
       * animation sources for the loaded scene; null otherwise. */
      this.anim = null;
      this.spawn = { x: 0, y: 0, z: 0 };
      /* The scene's MAN-placed actors (NPCs, chests, story actors), drawn
       * through the play page's own actor path (js/field-actors.js) over the
       * live world. `showActors` is the page's toggle. */
      this.actors = [];
      this.actorApi = null;
      this.showActors = o.showActors != null ? !!o.showActors : true;
      /* VR: present this scene in a headset. The button is always visible;
       * without an immersive-vr device it reads "VR unavailable" and click /
       * hover explain why (secure context, runtime, browser). */
      this.vr = window.LegaiaVr ? window.LegaiaVr.attach({
        mount: o.vrMount || canvas.parentElement,
        unitsPerMeter: VR_UNITS_PER_METER,
        renderer: () => this.renderer,
        cam: () => this.cam,
        extent: () => this.ext,
        draw: () => { this.stepAnim(); this.renderer.renderAssembled(this.frameDraws(), this.ext, this.cam); },
        /* Stand in the middle of the built-up area, on its floor, facing the
         * way the flat camera faces. */
        start: () => ({ x: this.spawn.x, y: this.spawn.y, z: this.spawn.z }),
        onEnter: () => this.stop(),
        onExit: () => this.resume(),
      }) : null;
    }

    /* Assemble and start rendering a CDNAME scene. Re-entrant: the previous
     * scene's GL meshes are released first. Returns the assembled state (also
     * kept as `this.state`); throws if the WASM loader rejects the label. */
    load(label) {
      this.stop();
      const v = this.viewer;
      this.renderer.clearScene();

      const packCount = v.set_scene_field(label);
      const status = JSON.parse(v.field_scene_status_json());

      /* Animation: the bundle's CLUT-walk table (water/waterfall shimmer)
       * plus the scene itself, entered live and headless through the
       * engine's scene host (engine-core `scene_live::LiveScene`). Its world
       * runs the scene's scripts, so everything the play page animates
       * moves here off the same world state: the floor-height ladder (the
       * placed / terrain draws and the walk ground follow it - concnow's
       * entry script installs a whole new one), the placed-prop clips (the
       * windmill), the ambient move-VM tree and scripted VRAM effects, and
       * the VDF vertex morphs. Init parks the walker source strips into the
       * WASM-side VRAM, so it must run BEFORE the first texture upload. */
      this.anim = null;
      this._animLast = undefined;
      this._animAccum = 0;
      this._floorWaveLive = false;
      if (typeof v.field_scene_anim_init === 'function') {
        try {
          const a = JSON.parse(v.field_scene_anim_init());
          if (a.walker_entries > 0 || a.ambient_parts > 0 || a.live) this.anim = a;
        } catch (e) { /* animation is optional; the static map still renders */ }
      }
      this.renderer.uploadVram(v.field_scene_vram_bytes());

      /* Ground heightfield may be absent - some interiors floor entirely with
       * terrain-tile meshes. Passing empties clears any previous scene's. */
      const hasGround = v.field_scene_ground_quad_count() > 0;
      if (hasGround) {
        this.renderer.uploadGround(
          v.field_scene_ground_positions(),
          v.field_scene_ground_uvs(),
          v.field_scene_ground_cba_tsb(),
          v.field_scene_ground_indices(),
          /* Each sloped far-bucket cell's marker: retail draws it under
           * everything (field_ground::flat_refs). Absent on an older bundle. */
          (typeof v.field_scene_ground_flat_refs === 'function')
            ? v.field_scene_ground_flat_refs() : null,
        );
      } else {
        this.renderer.uploadGround(new Float32Array(0), null, null, new Uint32Array(0));
      }

      /* Upload each referenced environment mesh once. A mesh is the WASM-side
       * hybrid: VRAM-filtered textured prims plus the untextured flat/gouraud
       * vertex-colour prims (flat_rgba non-empty), so colour-only props
       * (fences, rocks) render instead of being skipped - the browser sibling
       * of the engine's colour-mesh pipeline. Slots with no renderable prims of
       * either kind are skipped. */
      const used = new Set();
      const empty = new Set();
      const ensureMesh = (ms) => {
        if (used.has(ms)) return true;
        if (empty.has(ms)) return false;
        try { v.field_scene_mesh(ms); } catch (e) { empty.add(ms); return false; }
        const positions = v.field_scene_mesh_positions();
        const indices = v.field_scene_mesh_indices();
        if (positions.length === 0 || indices.length === 0) { empty.add(ms); return false; }
        const flat = v.field_scene_mesh_flat_rgba();
        this.renderer.uploadSceneMesh(
          ms, positions, v.field_scene_mesh_uvs(),
          v.field_scene_mesh_cba_tsb(), indices,
          flat.length ? flat : null,
        );
        used.add(ms);
        return true;
      };

      /* A placed prop whose object bind names a clip is a multi-object mesh
       * whose parts are that clip's bones (windmill sails on their hub,
       * cupboard doors on the cabinet): it gets its own mesh instance,
       * uploaded at the clip's rest pose and re-posed per frame from the live
       * world's prop cursor - the play page's ANIM_PROP_BASE scheme. */
      const ANIM_PROP_BASE = 800000;
      const hasPosed = typeof v.field_scene_mesh_posed === 'function';
      const uploadPosed = (meshId, slot, anim) => {
        try { v.field_scene_mesh_posed(slot, anim); } catch (e) { return false; }
        const positions = v.field_scene_mesh_positions();
        const indices = v.field_scene_mesh_indices();
        if (!positions.length || !indices.length) return false;
        const flat = v.field_scene_mesh_flat_rgba();
        this.renderer.uploadSceneMesh(meshId, positions, v.field_scene_mesh_uvs(),
          v.field_scene_mesh_cba_tsb(), indices, flat.length ? flat : null);
        return true;
      };
      const skySlots = new Set();
      let skyDrawsHidden = 0;
      const draws = [];
      this.animProps = [];
      /* `floorBase` is where this list starts inside the engine's
       * concatenated floor-wave offset array (terrain draws, then
       * placements), so a draw skipped below does not shift later rungs. */
      /* Light-source rows (TMD group flags 0x10..0x17) are shaded by the
       * field light against the draw's world normal - the play hosts'
       * `field_lit_mesh` kernel - so a slot carrying them gets one shaded
       * copy per (slot, pose, rotation). */
      const LIT_MESH_BASE = 900000;
      const hasLit = typeof v.field_scene_mesh_posed_lit === 'function';
      const litSlot = new Map(), litIds = new Map();
      let litCount = 0;
      const ensureLit = (slot, anim, rx, ry, rz, inst) => {
        if (!hasLit) return -1;
        let has = litSlot.get(slot);
        if (has === undefined) {
          has = !!v.field_scene_mesh_has_lit_rows(slot);
          litSlot.set(slot, has);
        }
        if (!has) return -1;
        /* A posed prop re-poses per instance, so it never shares an upload. */
        const key = `${slot}:${anim}:${rx & 0xFFF}:${ry & 0xFFF}:${rz & 0xFFF}${anim ? ":" + inst : ""}`;
        const known = litIds.get(key);
        if (known !== undefined) return known;
        try { v.field_scene_mesh_posed_lit(slot, anim, rx & 0xFFF, ry & 0xFFF, rz & 0xFFF); }
        catch (e) { return -1; }
        const positions = v.field_scene_mesh_positions();
        const indices = v.field_scene_mesh_indices();
        if (!positions.length || !indices.length) return -1;
        const flat = v.field_scene_mesh_flat_rgba();
        const id = LIT_MESH_BASE + litCount++;
        this.renderer.uploadSceneMesh(id, positions, v.field_scene_mesh_uvs(),
          v.field_scene_mesh_cba_tsb(), indices, flat.length ? flat : null);
        litIds.set(key, id);
        return id;
      };
      const pushDraws = (slots, pos, rots, rotsX, rotsZ, anims, floorBase) => {
        for (let i = 0; i < slots.length; i++) {
          const anim = (anims && hasPosed) ? anims[i] : 0;
          let ms = slots[i];
          const lit = ensureLit(slots[i], anim, rotsX ? rotsX[i] : 0,
            rots ? rots[i] : 0, rotsZ ? rotsZ[i] : 0, i);
          if (lit >= 0) {
            ms = lit;
          } else if (anim) {
            ms = ANIM_PROP_BASE + i;
            if (!uploadPosed(ms, slots[i], anim)) continue;
          } else if (!ensureMesh(ms)) {
            continue;
          }
          if (isSkyMesh(this.renderer.getMeshAabb(ms),
                        this.renderer.getMeshVertexCount
                          ? this.renderer.getMeshVertexCount(ms) : 0)) {
            skySlots.add(ms);
            skyDrawsHidden++;
            continue;
          }
          /* The WASM returns retail-frame world Y (PSX +Y down: elevated tiles
           * are NEGATIVE). placementModelScaledY flips only the mesh-local
           * geometry, not the translation - while the ground heightfield bakes
           * `-lut` into its vertices and gets the renderer's (1,-1,1) model,
           * landing at `+lut` (up). Negate the placement Y so objects sit ON
           * their floor tiles instead of mirrored below them (visible as sunken
           * buildings on elevated maps like Rim Elm's cliff).
           * rotY: the record's authored yaw (+0x0A, PSX 4096-per-rev); retail's
           * yaw sense is opposite placementModelScaledY's, hence the negation. */
          const draw = {
            meshId: ms,
            x: pos[i * 3], y: -pos[i * 3 + 1], z: pos[i * 3 + 2],
            rotY: rots ? -(rots[i] & 0xFFF) * Math.PI / 2048 : 0,
            scale: 1.0,
            /* The env-pack slot + clip, for the .glb baker. */
            slot: slots[i], anim,
            /* The record angles a lit copy was shaded at (null = plain). */
            litRot: lit >= 0 ? [rotsX ? rotsX[i] & 0xFFF : 0, rots ? rots[i] & 0xFFF : 0,
              rotsZ ? rotsZ[i] & 0xFFF : 0] : null,
            /* Floor-wave bookkeeping: this draw's index into the offset
             * array and the Y the shipped ladder gave it. */
            floorIdx: floorBase + i, baseY: -pos[i * 3 + 1],
          };
          /* A placement with an authored X/Z tilt cannot go through the
           * yaw-only path above: that builder's negated-yaw convention is a
           * cancellation specific to Ry (see webgl-math.js). Hand it a whole
           * model matrix composed in retail's Rx*Ry*Rz order instead - the
           * same composition the native shell applies through
           * engine-render's battle_intro::placement_rotation, and the same
           * one the play page applies. Most placements are pure yaw and keep
           * the cheaper path. */
          const rx = rotsX ? (rotsX[i] & 0xFFF) : 0;
          const rz = rotsZ ? (rotsZ[i] & 0xFFF) : 0;
          if (rx || rz) {
            const A2R = Math.PI / 2048;
            draw.model = placementModelEuler(
              draw.x, draw.y, draw.z,
              rx * A2R, (rots ? rots[i] & 0xFFF : 0) * A2R, rz * A2R, 1.0);
            /* The raw record angles, for the .glb baker's tilted entry. */
            draw.tilt = [rx, rots ? rots[i] & 0xFFF : 0, rz];
          }
          draws.push(draw);
          if (anim) this.animProps.push({ meshId: ms, i, slot: slots[i], anim, lastFrame: 0 });
        }
      };

      /* Terrain tiles first (ground layer), placed objects on top - the depth
       * test resolves overlap either way; the order just matches the native
       * draw sequence. Rotation accessors are guarded so a stale cached WASM
       * still draws (unrotated). */
      const hasRot = typeof v.field_scene_placement_rot_y === 'function';
      const hasTilt = typeof v.field_scene_placement_rot_x === 'function';
      const terrainSlots = v.field_scene_terrain_slots();
      pushDraws(terrainSlots, v.field_scene_terrain_positions(),
        hasRot ? v.field_scene_terrain_rot_y() : null,
        hasTilt ? v.field_scene_terrain_rot_x() : null,
        hasTilt ? v.field_scene_terrain_rot_z() : null,
        null, 0);
      const terrainCount = draws.length;
      pushDraws(v.field_scene_placement_slots(), v.field_scene_placement_positions(),
        hasRot ? v.field_scene_placement_rot_y() : null,
        hasTilt ? v.field_scene_placement_rot_x() : null,
        hasTilt ? v.field_scene_placement_rot_z() : null,
        hasPosed ? v.field_scene_placement_anim_ids() : null,
        terrainSlots.length);

      /* Frame the camera on the assembled geometry (ground AABB if present,
       * else the draw cluster). */
      let xmin = Infinity, xmax = -Infinity, zmin = Infinity, zmax = -Infinity;
      for (const p of draws) {
        if (p.x < xmin) xmin = p.x; if (p.x > xmax) xmax = p.x;
        if (p.z < zmin) zmin = p.z; if (p.z > zmax) zmax = p.z;
      }
      const gAabb = this.renderer.getGroundAabb();
      if (gAabb && gAabb.sx > 0) {
        xmin = Math.min(xmin, gAabb.cx - gAabb.sx / 2);
        xmax = Math.max(xmax, gAabb.cx + gAabb.sx / 2);
        zmin = Math.min(zmin, gAabb.cz - gAabb.sz / 2);
        zmax = Math.max(zmax, gAabb.cz + gAabb.sz / 2);
      }
      if (!Number.isFinite(xmin)) { xmin = 0; xmax = 16384; zmin = 0; zmax = 16384; }
      const spanX = Math.max(xmax - xmin, 1024);
      const spanZ = Math.max(zmax - zmin, 1024);
      const ext = [spanX + 1024, spanZ + 1024];
      this.cam.centerX = (xmin + xmax) / 2;
      this.cam.centerZ = (zmin + zmax) / 2;
      this.cam.halfWidth = spanX / 2 + 512;
      this.cam.halfHeight = spanZ / 2 + 512;
      this.cam.yaw = 0;
      this.cam.pitch = 0.65;

      /* VR spawn point: the middle of the *built* area, taken as the component-
       * wise median of the placed objects (houses, props, NPC anchors) - NOT
       * the framing AABB centre. A field map is a 128x128-tile grid whose town
       * usually occupies a corner of it, so the AABB centre lands on empty
       * ground with the buildings a hundred metres away; the median lands in
       * the village. Y comes from the same set: a placement's world Y *is* the
       * floor tile it stands on. Terrain tiles are excluded (they tile the
       * whole grid and would drag the median back to the centre). */
      const built = draws.slice(terrainCount);
      const src = built.length ? built : draws;
      const med = (key) => {
        if (!src.length) return 0;
        const s = src.map(d => d[key]).sort((a, b) => a - b);
        return s[Math.floor((s.length - 1) / 2)];
      };
      this.spawn = { x: med('x'), y: med('y'), z: med('z') };

      /* The actor layer: uploaded once per scene (every catalogued
       * placement, parked ones included - the per-frame draw skips anyone at
       * the off-map hide box), posed each frame off the live world. */
      this.actors = [];
      this.actorApi = null;
      if (window.LegaiaFieldActors && typeof v.field_scene_npc_catalog_json === 'function'
          && this.anim && this.anim.live) {
        this.actorApi = window.LegaiaFieldActors.api(v, 'field_scene_npc_');
        this.actors = window.LegaiaFieldActors.upload(this.renderer, this.actorApi, NPC_MESH_BASE);
      }

      this.state = {
        label, packCount, status, draws, hasGround, skyDrawsHidden,
        actors: this.actors.length,
        drawn: new Set(draws.map(d => d.meshId)).size,
        drawnSlots: Array.from(new Set(draws.map(d => d.meshId))).sort((a, b) => a - b),
        emptySlots: Array.from(empty).sort((a, b) => a - b),
        skySlots: Array.from(skySlots).sort((a, b) => a - b),
        cam: this.cam,
        meshAabbs: Object.fromEntries(
          Array.from(used).map(ms => [ms, this.renderer.getMeshAabb(ms)])),
      };

      this.draws = draws;
      this.ext = ext;
      if (this.vr) {
        this.vr.setReady(true);
        /* A live headset session survives a scene swap (same canvas, same GL
         * context, same renderer) - just re-place the viewer in the new map. */
        if (this.vr.isActive()) {
          this.vr.respawn();
          return this.state;
        }
      }
      this.resume();
      return this.state;
    }

    /* (Re)start the flat render loop. Idempotent; a no-op while a VR session
     * owns the renderer (the XR frame loop draws instead). */
    resume() {
      if (this.raf || !this.renderer) return;
      if (this.vr && this.vr.isActive()) return;
      const tick = () => {
        this.stepAnim();
        this.renderer.renderAssembled(this.frameDraws(), this.ext, this.cam);
        this.raf = requestAnimationFrame(tick);
      };
      this.raf = requestAnimationFrame(tick);
    }

    /* Advance the scene's VRAM animation by however many retail vsyncs of
     * wall clock elapsed and re-upload the VRAM texture when texels changed
     * (CLUT-walk shimmer fires every few game ticks; the ambient palette
     * cyclers every game tick while lit). Called from both the flat loop and
     * (via LegaiaVr's draw callback path) each XR frame - both of which fire
     * at the DISPLAY rate, so the vsync count must come from the wall clock,
     * not the callback count: ticking 1 per rAF ran the palette animations
     * at 2-2.4x retail on a 120/144 Hz monitor (and up to 4x in a 240 Hz
     * headset), the same class of bug the play page's `_simAccum` governor
     * fixed for the world tick. No-op for scenes with no animation sources. */
    stepAnim() {
      if (!this.anim) return;
      const v = this.viewer;
      if (typeof v.field_scene_anim_tick !== 'function') return;
      const VSYNC_MS = 1000 / 60;
      const now = performance.now();
      if (this._animLast === undefined) this._animLast = now;
      this._animAccum = (this._animAccum || 0) + (now - this._animLast);
      this._animLast = now;
      /* Cap the backlog so a hidden tab doesn't unleash a catch-up burst. */
      if (this._animAccum > VSYNC_MS * 4) this._animAccum = VSYNC_MS * 4;
      const vsyncs = Math.floor(this._animAccum / VSYNC_MS);
      if (vsyncs <= 0) return;
      this._animAccum -= vsyncs * VSYNC_MS;
      if (v.field_scene_anim_tick(vsyncs)) {
        this.renderer.uploadVram(v.field_scene_vram_bytes());
      }
      /* VDF vertex morphs (jou's flesh-ground pulse, rikuroa's generator
       * sacs): the ambient world reports which env-pack meshes' deltas
       * moved this tick; re-upload just those meshes' positions. */
      if (typeof v.field_scene_morph_slots === 'function') {
        const slots = v.field_scene_morph_slots();
        for (let i = 0; i < slots.length; i++) {
          const pos = v.field_scene_morph_positions(slots[i]);
          if (pos.length) this.renderer.updateSceneMeshPositions(slots[i], pos);
        }
      }
      this._applyFloorWave();
      this._applyGroundWave();
      this._applyPropFrames();
    }

    /* The floor-height ladder the scene's scripts animate (field-VM op 0x4C
     * nibble 9). Every terrain / placed draw was resolved against the ladder
     * the scene ships; the engine hands back a per-draw Y offset under the
     * live one (`field_scene_floor_wave_offsets`, the play page's kernel),
     * empty while the two agree. The page frame negates retail Y. */
    _applyFloorWave() {
      const v = this.viewer;
      if (typeof v.field_scene_floor_wave_offsets !== 'function') return;
      const wave = v.field_scene_floor_wave_offsets();
      if (!wave.length) {
        if (!this._floorWaveLive) return;
        for (const d of this.draws) {
          if (d.floorIdx === undefined) continue;
          d.y = d.baseY;
          if (d.model) d.model[13] = d.y;
        }
        this._floorWaveLive = false;
        return;
      }
      for (const d of this.draws) {
        if (d.floorIdx === undefined || d.floorIdx >= wave.length) continue;
        d.y = d.baseY - wave[d.floorIdx];
        if (d.model) d.model[13] = d.y;
      }
      this._floorWaveLive = true;
    }

    /* The walk ground under the same live ladder: retail's ground pass takes
     * each cell's corner tiers through it every frame (jouina's pulsing
     * path, concnow's flesh pits). Empty on a frame the ladder did not
     * move. */
    _applyGroundWave() {
      const v = this.viewer;
      if (typeof v.field_scene_ground_live_positions !== 'function'
          || !this.renderer.updateGroundPositions || !this.state || !this.state.hasGround) return;
      const pos = v.field_scene_ground_live_positions();
      if (pos.length) this.renderer.updateGroundPositions(pos, null);
    }

    /* Animated props: re-pose each to the live world's prop-bank cursor when
     * its frame changed (the windmill's sails turn). */
    _applyPropFrames() {
      const v = this.viewer;
      if (!this.animProps || !this.animProps.length
          || typeof v.field_scene_placement_frames !== 'function') return;
      const pf = v.field_scene_placement_frames();
      for (const p of this.animProps) {
        const f = (p.i < pf.length) ? pf[p.i] : -1;
        if (f < 0 || f === p.lastFrame) continue;
        const posed = v.field_scene_mesh_posed_frame_positions(p.slot, p.anim, f);
        if (posed.length) {
          this.renderer.updateSceneMeshPositions(p.meshId, posed);
          p.lastFrame = f;
        }
      }
    }

    /* This frame's draw list: the static map plus, when shown, the actor
     * layer posed and placed off the live world. */
    frameDraws() {
      if (!this.showActors || !this.actors.length || !this.actorApi) return this.draws;
      const out = this.draws.slice();
      window.LegaiaFieldActors.rebind(this.renderer, this.actorApi, this.actors);
      window.LegaiaFieldActors.frame(this.renderer, this.actorApi, this.actors, out,
        { advance: true });
      return out;
    }

    /* Show / hide the actor layer. */
    setShowActors(on) {
      this.showActors = !!on;
    }

    /* One-line summary of the loaded scene, for a status bar. */
    summary() {
      const s = this.state;
      if (!s) return '';
      const sky = s.skyDrawsHidden
        ? ` · ${s.skyDrawsHidden} sky-backdrop draw${s.skyDrawsHidden > 1 ? 's' : ''} hidden`
        : '';
      const anim = this.anim
        ? ` · animated (${this.anim.live ? 'live scene, ' : ''}${this.anim.walker_entries} CLUT walkers, ${this.anim.ambient_parts} ambient fx)`
        : '';
      const actors = s.actors ? ` · ${s.actors} actors` : '';
      return `${s.packCount} environment meshes (${s.drawn} drawn) · ${s.status.placements} placements${actors}`
        + ` · ${s.status.terrain} terrain tiles · ${s.status.ground_quads} ground quads${sky}${anim}`;
    }

    /* Feed the assembled draw list to the WASM .glb exporter and return the
     * bytes (empty Uint8Array when nothing is drawable). Bakes the same meshes
     * + transforms this view renders, so the file matches the screen. */
    exportGlb() {
      const v = this.viewer;
      const s = this.state;
      if (!s || typeof v.scene_export_begin !== 'function') return new Uint8Array(0);
      const none = new Uint8Array(0);
      v.scene_export_begin(s.label);
      v.scene_export_set_vram(v.field_scene_vram_bytes());
      if (s.hasGround) {
        const gi = v.scene_export_add_mesh(
          'ground',
          v.field_scene_ground_positions(), v.field_scene_ground_uvs(),
          v.field_scene_ground_cba_tsb(), v.field_scene_ground_indices(), none);
        v.scene_export_add_instance(gi, 0, 0, 0, 0, 1.0);
      }
      const handles = new Map();
      for (const d of s.draws) {
        let mi = handles.get(d.meshId);
        if (mi === undefined) {
          try {
            if (d.litRot) v.field_scene_mesh_posed_lit(d.slot, d.anim || 0, ...d.litRot);
            else if (d.anim) v.field_scene_mesh_posed(d.slot, d.anim);
            else v.field_scene_mesh(d.meshId);
          } catch (e) { continue; }
          mi = v.scene_export_add_mesh(
            'mesh_' + d.meshId,
            v.field_scene_mesh_positions(), v.field_scene_mesh_uvs(),
            v.field_scene_mesh_cba_tsb(), v.field_scene_mesh_indices(),
            v.field_scene_mesh_flat_rgba());
          handles.set(d.meshId, mi);
        }
        const sc = d.scale != null ? d.scale : 1.0;
        /* A tilted record (its own model matrix on screen) keeps its pitch /
         * roll in the file: the yaw-only entry would stand it upright. */
        if (d.tilt && typeof v.scene_export_add_instance_euler === 'function') {
          v.scene_export_add_instance_euler(mi, d.x, d.y || 0, d.z, ...d.tilt, sc);
        } else {
          v.scene_export_add_instance(mi, d.x, d.y || 0, d.z, d.rotY || 0, sc);
        }
      }
      return v.scene_export_finish() || new Uint8Array(0);
    }

    /* Halt the render loop but keep the renderer + controls alive for the next
     * load(). */
    stop() {
      if (this.raf) cancelAnimationFrame(this.raf);
      this.raf = 0;
    }

    dispose() {
      this.stop();
      if (this.vr) { this.vr.destroy(); this.vr = null; }
      if (this.renderer) {
        this.renderer.dispose();
        this.renderer = null;
      }
      this.state = null;
    }
  }

  window.FieldSceneView = FieldSceneView;
  window.FieldSceneView.isSkyMesh = isSkyMesh;
})();
