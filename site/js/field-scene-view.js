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
 * Requires webgl-math.js + webgl-shaders.js + webgl-tmd.js to be loaded first.
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
   * mountain walls like rikuroa slot 32 are long but shallow). */
  function isSkyMesh(aabb) {
    if (!aabb) return false;
    const flatPlane = Math.min(aabb.sx, aabb.sz) < 8
      && Math.max(aabb.sx, aabb.sz) > 3000 && aabb.sy > 600;
    const domeShell = aabb.sx > 3400 && aabb.sz > 3400 && aabb.sy > 800;
    return flatPlane || domeShell;
  }

  /* World units per metre for the VR path. The field character mesh stands
   * ~130 units tall (the play page's follow camera is tuned around that), so a
   * 1.7 m human puts a metre at ~76 world units - which also makes the 128-unit
   * walkability tile a believable 1.7 m stride. At this scale a headset stands
   * *in* the town at human height. See docs/subsystems/vr-mode.md. */
  const VR_UNITS_PER_METER = 76;

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
      this.spawn = { x: 0, y: 0, z: 0 };
      /* VR: present this scene in a headset. The button is always visible;
       * without an immersive-vr device it reads "VR unavailable" and click /
       * hover explain why (secure context, runtime, browser). */
      this.vr = window.LegaiaVr ? window.LegaiaVr.attach({
        mount: o.vrMount || canvas.parentElement,
        unitsPerMeter: VR_UNITS_PER_METER,
        renderer: () => this.renderer,
        cam: () => this.cam,
        extent: () => this.ext,
        draw: () => this.renderer.renderAssembled(this.draws, this.ext, this.cam),
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

      const skySlots = new Set();
      let skyDrawsHidden = 0;
      const draws = [];
      const pushDraws = (slots, pos, rots) => {
        for (let i = 0; i < slots.length; i++) {
          const ms = slots[i];
          if (!ensureMesh(ms)) continue;
          if (isSkyMesh(this.renderer.getMeshAabb(ms))) {
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
          draws.push({
            meshId: ms,
            x: pos[i * 3], y: -pos[i * 3 + 1], z: pos[i * 3 + 2],
            rotY: rots ? -(rots[i] & 0xFFF) * Math.PI / 2048 : 0,
            scale: 1.0,
          });
        }
      };

      /* Terrain tiles first (ground layer), placed objects on top - the depth
       * test resolves overlap either way; the order just matches the native
       * draw sequence. Yaw accessors are guarded so a stale cached WASM still
       * draws (unrotated). */
      const hasRot = typeof v.field_scene_placement_rot_y === 'function';
      pushDraws(v.field_scene_terrain_slots(), v.field_scene_terrain_positions(),
        hasRot ? v.field_scene_terrain_rot_y() : null);
      const terrainCount = draws.length;
      pushDraws(v.field_scene_placement_slots(), v.field_scene_placement_positions(),
        hasRot ? v.field_scene_placement_rot_y() : null);

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

      this.state = {
        label, packCount, status, draws, hasGround, skyDrawsHidden,
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
        this.renderer.renderAssembled(this.draws, this.ext, this.cam);
        this.raf = requestAnimationFrame(tick);
      };
      this.raf = requestAnimationFrame(tick);
    }

    /* One-line summary of the loaded scene, for a status bar. */
    summary() {
      const s = this.state;
      if (!s) return '';
      const sky = s.skyDrawsHidden
        ? ` · ${s.skyDrawsHidden} sky-backdrop draw${s.skyDrawsHidden > 1 ? 's' : ''} hidden`
        : '';
      return `${s.packCount} environment meshes (${s.drawn} drawn) · ${s.status.placements} placements`
        + ` · ${s.status.terrain} terrain tiles · ${s.status.ground_quads} ground quads${sky}`;
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
          try { v.field_scene_mesh(d.meshId); } catch (e) { continue; }
          mi = v.scene_export_add_mesh(
            'mesh_' + d.meshId,
            v.field_scene_mesh_positions(), v.field_scene_mesh_uvs(),
            v.field_scene_mesh_cba_tsb(), v.field_scene_mesh_indices(),
            v.field_scene_mesh_flat_rgba());
          handles.set(d.meshId, mi);
        }
        v.scene_export_add_instance(
          mi, d.x, d.y || 0, d.z, d.rotY || 0, d.scale != null ? d.scale : 1.0);
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
