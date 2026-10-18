/* world-overview-app.js - viewer controller for the world-overview page.
 *
 * Extracted from site/world-overview.html (and its _content/ mirror) for
 * file modularity. Loads as a classic global script after webgl-{shaders,
 * math,tmd}.js. The top-level IIFE was preserved verbatim so the DOM
 * dependencies (the canvas + control IDs in the page) keep working
 * unchanged. Bump the ?v= query param when shipping a breaking method
 * change so users don't hit a stale cached copy.
 */

(async function () {
  /* ---------- Per-kingdom data (world-overview-kingdoms.js) ----------- */
  const {
    KINGDOM_PROT, KINGDOM_CAM, KINGDOM_TOPVIEW_CAM, UNPLACED_TARGET_FOOTPRINT,
    KINGDOM_WALK_EYE, KINGDOM_MIST,
  } = window.LegaiaWorldOverviewKingdoms;
  /* ---------- Top-down map (world-overview-minimap.js) ---------------- */
  const { createMinimap, classifyGround } = window.LegaiaWorldOverviewMinimap;
  /* ---------- Ground sampler + camera framing (world-overview-ground.js) */
  const { buildSampler, frameCamera } = window.LegaiaWorldOverviewGround;


  /* ---------- DOM refs ----------- */
  const $file       = document.getElementById('wo-disc-file');
  const $status     = document.getElementById('wo-status');
  const $canvas3d   = document.getElementById('wo-canvas3d');
  const $canvas2d   = document.getElementById('wo-canvas2d');
  const $statsDl    = document.getElementById('wo-stats-dl');
  const $legend     = document.getElementById('wo-map-legend');
  const $meshLabel  = document.getElementById('wo-mesh-label');
  const $meshInfo   = document.getElementById('wo-mesh-info');
  const $prev       = document.getElementById('wo-prev-mesh');
  const $next       = document.getElementById('wo-next-mesh');
  const $autoRot    = document.getElementById('wo-auto-rotate');
  const $resetCam   = document.getElementById('wo-reset-cam');
  const $hover      = document.getElementById('wo-hover');
  const $nameList   = document.getElementById('wo-name-list');
  const $modeWorld  = document.getElementById('wo-mode-world');
  const $modeMesh   = document.getElementById('wo-mode-mesh');
  const $autoRotWrap = document.getElementById('wo-auto-rotate-wrap');
  const $showUnplaced = document.getElementById('wo-show-unplaced');
  const $normalizeUnplaced = document.getElementById('wo-normalize-unplaced');
  const $hazeEnable = document.getElementById('wo-haze-enable');
  const $mistEnable = document.getElementById('wo-mist-enable');
  const $mistStatus = document.getElementById('wo-mist-status');
  const $showTerrain = document.getElementById('wo-show-terrain');
  const $showLandmarks = document.getElementById('wo-show-landmarks');
  const $lockTopView = document.getElementById('wo-lock-topview');
  const $exportGlb  = document.getElementById('wo-export-glb');
  /* Top-down map: the per-kingdom plot data gathered from the disc the last
   * time that kingdom's assembled scene loaded, keyed by tab id. */
  const minimap = $canvas2d ? createMinimap({ canvas: $canvas2d, hover: $hover }) : null;
  const minimapData = {};
  let   viewMode    = 'world';  /* 'world' (assembled scene from disc) | 'mesh' (single TMD inspector) */
  /* The Mist overlay (world-overview-mist.js) for the loaded kingdom, and
   * the fog sheet art read off the disc once per load. */
  let   mist        = null;
  let   mistSheet   = null;

  /* Authored default-state suppression for script-managed walk stamps.
   * Drake has two golden-bridge (mesh 6) stamps: record 441, a plain
   * decoration at the road crossing (12224, 6336), and record 349, a
   * placed+interactive record (flags 0x0017) whose grid cell sits over
   * the river at (10688, 5312). Retail spawns record 349 as an actor
   * (live render-list capture: rec 349 -> pool 11) and runs its MAN
   * interaction script's leading ops at spawn (FUN_8003A55C's inline
   * stepper), which manage where/whether it rests - the raw grid cell
   * is not its retail resting draw position, and retail's default map
   * shows a single bridge at the record-441 site. The static viewer
   * can't run MAN scripts, so the grid-cell draw is suppressed here
   * (the same reason the legacy JSON layer skips `script_positioned`
   * placements). Keyed by kingdom tab id; matched on
   * (pack mesh, world x, world z). */
  const WALK_STAMP_SUPPRESS = {
    drake: [{ mesh: 6, x: 10688, z: 5312 }],
  };

  /* ---------- State ----------- */
  let viewer = null;          /* LegaiaViewer (WASM) - created after load */
  let wasmMod = null;
  let allEntries = [];        /* viewer.entry_list_json output */
  let kingdomSlots = { drake: [], sebucus: [], karisto: [] };
  let currentKingdom = (location.hash || '#drake').slice(1);
  if (!KINGDOM_PROT[currentKingdom]) currentKingdom = 'drake';
  let currentMeshIdx = 0;     /* index into kingdomSlots[currentKingdom] */
  let placements = null;      /* world-overview.json */
  let glRenderer = null;
  let rafId = null;
  /* Draw list of the currently-assembled world view, kept so the .glb
   * export can bake exactly what's on screen. Null until a kingdom's
   * world view has loaded (and while in mesh-inspector mode). */
  let worldExportState = null;
  const cam = { yaw: 0, pitch: 0.25, distance: 2.5, panX: 0, panY: 0, autoRotate: true };
  /* World camera state (world units). Reset per-kingdom to fit the world.
   * `yaw`/`pitch` drive the perspective orbit camera (buildWorldOrbitVp):
   * pitch = 0 is straight-down, larger tilts toward the horizon; the
   * default tilt makes the 3D terrain relief legible on load, matching
   * the retail world map being a real 3D perspective view (GameShark
   * camera moves in retail confirm it). "Lock to retail top-view" and
   * the reset button snap pitch back to their respective framings. */
  const WORLD_DEFAULT_PITCH = 0.85;
  const worldCam = {
    centerX: 8160, centerZ: 8160, halfWidth: 9000, halfHeight: 9000,
    yaw: 0, pitch: WORLD_DEFAULT_PITCH,
  };
  /* Testing hook: the headless-browser verification harness steers the
   * camera to known world coordinates through this reference. Not used
   * by any page code. */
  window.__woCam = worldCam;

  /* ---------- VR (WebXR immersive-vr) -----------------------------
   * A kingdom is a 16320-unit continent; at 2000 world units per metre it
   * becomes an ~8 m diorama, and the headset's 1.6 m eye height puts the
   * viewer 3200 world units above the sea plane - a giant looking down at the
   * map, which is the readable framing for a whole continent. The thumbstick
   * grips rescale it live, so 1:1 "stand on the world map" is a squeeze away.
   * See docs/subsystems/vr-mode.md. */
  const VR_UNITS_PER_METER = 2000;
  /* "On the ground" first-person mode: human scale, standing on the continent
   * terrain. 76 world units per metre is the character-height anchor every
   * human-scale page shares (the ~130-unit player mesh as a 1.7 m adult), so a
   * metre here means the same thing it means in a town. */
  const VR_GROUND_UNITS_PER_METER = 76;
  let vr = null;             /* LegaiaVr handle (null before DOM wiring) */
  let worldTick = null;      /* the flat render loop, so VR can pause/resume it */
  let worldDrawState = null; /* { draws, ext } of the assembled kingdom */
  let groundSampler = null;  /* (x, z) -> terrain surface Y (world up), or null */

  /* The terrain-height sampler over the walk heightfield the kingdom load
   * uploaded (world-overview-ground.js), or null without one. */
  function buildGroundSampler() {
    if (!viewer || typeof viewer.walk_ground_positions !== 'function') return null;
    let pos;
    try { pos = viewer.walk_ground_positions(); } catch (e) { return null; }
    return buildSampler(pos);
  }
  /* Headless-verification hook (same spirit as `__woCam`): the mock-XR
   * harness asserts the VR ground mode stands ON this surface. Not used by
   * page code. */
  window.__woGround = (x, z) => (groundSampler ? groundSampler(x, z) : null);

  /* ---------- Placement JSON (always available) ----------- */
  try {
    placements = await fetch('world-overview.json').then(r => r.json());
  } catch (e) {
    placements = null;
  }
  updateSnapshot();
  refreshMinimap();

  /* Render a "load a disc" placeholder on first paint so the canvas
   * isn't an empty black square. Default mode is 'world' (assembled scene
   * from disc); if the user has no disc yet, drawWorldPlaceholder() draws
   * a friendly hint. drawWorldPlaceholder is hoisted as a function
   * declaration. */
  queueMicrotask(() => drawWorldPlaceholder());

  /* ---------- Camera controls ----------- */
  function attachCameraControls(canvas) {
    let dragging = false, lastX = 0, lastY = 0;
    canvas.addEventListener('mousedown', e => { dragging = true; lastX = e.clientX; lastY = e.clientY; cam.autoRotate = false; $autoRot.checked = false; });
    window.addEventListener('mouseup', () => { dragging = false; });
    window.addEventListener('mousemove', e => {
      if (!dragging) return;
      const dx = e.clientX - lastX, dy = e.clientY - lastY;
      lastX = e.clientX; lastY = e.clientY;
      cam.yaw   += dx * 0.01;
      cam.pitch  = Math.max(-1.4, Math.min(1.4, cam.pitch + dy * 0.01));
    });
    canvas.addEventListener('wheel', e => {
      e.preventDefault();
      const f = Math.exp(e.deltaY * 0.001);
      cam.distance = Math.max(0.5, Math.min(20, cam.distance * f));
    }, { passive: false });
    const keys = new Set();
    window.addEventListener('keydown', e => {
      if (e.target.tagName === 'INPUT') return;
      const k = e.key.toLowerCase();
      if ('wasd'.includes(k)) keys.add(k);
    });
    window.addEventListener('keyup', e => {
      const k = e.key.toLowerCase();
      keys.delete(k);
    });
    return keys;
  }
  function resetCam() {
    cam.yaw = 0; cam.pitch = 0.25; cam.distance = 2.5;
    cam.panX = 0; cam.panY = 0; cam.autoRotate = true;
    $autoRot.checked = true;
  }
  $resetCam.addEventListener('click', () => {
    if (viewMode === 'world') {
      /* Frame the whole kingdom again, anchored on the retail camera
       * centre when one is known. */
      const k = placements?.[currentKingdom];
      const ext = k?.world_extent || [16320, 16320];
      const retail = KINGDOM_CAM[currentKingdom];
      if (retail) {
        worldCam.centerX = retail.centerX;
        worldCam.centerZ = retail.centerZ;
      } else {
        worldCam.centerX = ext[0] / 2;
        worldCam.centerZ = ext[1] / 2;
      }
      worldCam.halfWidth = ext[0] / 2 + 1000;
      worldCam.halfHeight = ext[1] / 2 + 1000;
      worldCam.yaw = 0;
      worldCam.pitch = WORLD_DEFAULT_PITCH;
    } else {
      resetCam();
    }
  });
  $autoRot.addEventListener('change', () => { cam.autoRotate = $autoRot.checked; });

  /* ---------- Continent .glb export ----------- */
  function downloadBytes(bytes, filename) {
    const blob = new Blob([bytes], { type: 'model/gltf-binary' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = filename;
    a.click();
    setTimeout(() => URL.revokeObjectURL(url), 5000);
  }
  /* Bake the assembled continent (terrain heightfield + placed landmark /
   * decoration stamps, textures baked from VRAM) into a binary glTF and
   * download it. Feeds the WASM exporter the exact mesh buffers + per-draw
   * transforms the on-screen renderAssembled loop uses, so the file matches
   * the view (ocean backdrop excluded - it's an animated screen effect). */
  async function exportWorldGlb() {
    if (!viewer || !worldExportState || viewMode !== 'world') return;
    if (typeof viewer.scene_export_begin !== 'function') return;
    const prevLabel = $exportGlb.textContent;
    $exportGlb.disabled = true;
    $exportGlb.textContent = 'baking…';
    /* Let the label paint before the synchronous WASM bake. */
    await new Promise((r) => setTimeout(r, 30));
    try {
      const k = KINGDOM_PROT[currentKingdom];
      viewer.scene_export_begin(k ? k.cdname : currentKingdom);
      viewer.scene_export_set_vram(viewer.pack_vram_bytes());
      const none = new Uint8Array(0);
      if (worldExportState.hasGround && (!$showTerrain || $showTerrain.checked)) {
        /* Ground vertices are already in world coords (PSX pre-flip frame);
         * the renderer draws them with diag(1,-1,1) and no translation, so
         * an identity instance reproduces it (the exporter bakes the flip). */
        const gi = viewer.scene_export_add_mesh(
          'ground',
          viewer.walk_ground_positions(), viewer.walk_ground_uvs(),
          viewer.walk_ground_cba_tsb(), viewer.walk_ground_indices(), none);
        viewer.scene_export_add_instance(gi, 0, 0, 0, 0, 1.0);
      }
      const meshHandles = new Map();
      for (const p of worldExportState.draws) {
        let mi = meshHandles.get(p.meshId);
        if (mi === undefined) {
          try { viewer.pack_mesh(p.meshId); } catch (e) { continue; }
          /* Same hybrid packet-colour stream ensureMeshUploaded hands the
           * shader - the export bakes it into COLOR_0 (and the flag picks
           * textured-modulate vs untextured-fill per vertex), so the file
           * carries the `texel * colour / 128` shading the canvas draws
           * AND the vertex-colour roofs. Omitting it exported raw texels. */
          mi = viewer.scene_export_add_mesh(
            'mesh_' + p.meshId,
            viewer.pack_mesh_positions(), viewer.pack_mesh_uvs(),
            viewer.pack_mesh_cba_tsb(), viewer.pack_mesh_indices(),
            (typeof viewer.pack_mesh_flat_rgba === 'function')
              ? viewer.pack_mesh_flat_rgba() : none);
          meshHandles.set(p.meshId, mi);
        }
        const scale = (p.scale != null) ? p.scale : MESH_SCALE;
        let x = p.x, y = p.y || 0, z = p.z;
        if (p.anchor === 'centroid' && glRenderer) {
          /* Legacy layout-grid stamps pivot on the mesh AABB centroid;
           * fold that into the translation (placementModelCentered math). */
          const aabb = glRenderer.getMeshAabb(p.meshId);
          if (aabb) {
            const c = Math.cos(p.rotY || 0), s = Math.sin(p.rotY || 0);
            x = scale * (-c * aabb.cx + s * aabb.cz) + p.x;
            y = scale * aabb.cy;
            z = scale * (-s * aabb.cx - c * aabb.cz) + p.z;
          }
        }
        viewer.scene_export_add_instance(mi, x, y, z, p.rotY || 0, scale);
      }
      const glb = viewer.scene_export_finish();
      if (!glb || glb.length === 0) {
        $exportGlb.textContent = 'nothing to export';
        setTimeout(() => { $exportGlb.textContent = prevLabel; }, 1500);
        return;
      }
      downloadBytes(glb, `legaia-${currentKingdom}-continent.glb`);
      $exportGlb.textContent = prevLabel;
    } catch (err) {
      console.warn('glb export failed:', err);
      $exportGlb.textContent = 'export failed';
      setTimeout(() => { $exportGlb.textContent = prevLabel; }, 1500);
    } finally {
      $exportGlb.disabled = false;
      if ($exportGlb.textContent === 'baking…') $exportGlb.textContent = prevLabel;
    }
  }
  if ($exportGlb) $exportGlb.addEventListener('click', exportWorldGlb);

  /* ---------- VR ----------- */
  /* Attached once; the button lives in the canvas control bar (which survives
   * the per-load canvas swap) and is always visible - it arms when the browser
   * reports an immersive-vr device AND a kingdom is assembled, and otherwise
   * explains what is missing on hover / click. */
  if (window.LegaiaVr) {
    vr = window.LegaiaVr.attach({
      mount: document.querySelector('.wo-canvas-controls'),
      unitsPerMeter: VR_UNITS_PER_METER,
      renderer: () => glRenderer,
      cam: () => worldCam,
      extent: () => (worldDrawState ? worldDrawState.ext : [16320, 16320]),
      draw: () => {
        stepKingdomClut();
        glRenderer.renderAssembled(worldDrawState.draws, worldDrawState.ext, worldCam);
      },
      modes: [
        /* Diorama: stand on the sea plane at the continent's framing centre -
         * at 2000 units/m that is 3.2 m above an ~8 m tabletop continent. */
        {
          id: 'diorama', label: 'Diorama',
          unitsPerMeter: VR_UNITS_PER_METER,
          start: () => ({ x: worldCam.centerX, y: 0, z: worldCam.centerZ }),
        },
        /* On the ground: human scale, feet snapped to the terrain surface
         * under the viewer every frame (walk the continent; no free-fly
         * altitude). Spawns on the terrain under the framing centre. */
        {
          id: 'ground', label: 'On the ground',
          unitsPerMeter: VR_GROUND_UNITS_PER_METER,
          start: () => ({ x: worldCam.centerX, y: 0, z: worldCam.centerZ }),
          groundHeight: (x, z) => (groundSampler ? groundSampler(x, z) : 0),
        },
      ],
      onEnter: () => {
        if (rafId !== null) { cancelAnimationFrame(rafId); rafId = null; }
      },
      onExit: () => {
        if (rafId === null && worldTick) rafId = requestAnimationFrame(worldTick);
      },
    });
  }

  /* ---------- View-mode toggle ----------- */
  function setViewMode(mode) {
    if (!['world', 'mesh'].includes(mode)) return;
    if (mode === viewMode) return;
    viewMode = mode;
    $modeWorld.classList.toggle('is-active', mode === 'world');
    $modeMesh .classList.toggle('is-active', mode === 'mesh');
    $modeWorld.setAttribute('aria-checked', mode === 'world');
    $modeMesh .setAttribute('aria-checked', mode === 'mesh');
    const meshOnly = (mode === 'mesh');
    $prev.hidden = !meshOnly;
    $next.hidden = !meshOnly;
    $autoRotWrap.hidden = !meshOnly;
    /* Unplaced-TMD toggle, normalisation, haze, Mist, top-view lock only make
     * sense in world mode (mesh inspector already walks the whole pack
     * one mesh at a time). */
    const $unplacedWrap = document.getElementById('wo-show-unplaced-wrap');
    const $normalizeWrap = document.getElementById('wo-normalize-unplaced-wrap');
    const $hazeWrap = document.getElementById('wo-haze-wrap');
    const $mistWrap = document.getElementById('wo-mist-wrap');
    const $terrainWrap = document.getElementById('wo-show-terrain-wrap');
    const $landmarksWrap = document.getElementById('wo-show-landmarks-wrap');
    /* The unplaced-slot-1 layout grid only drives the legacy
     * overview-frame placement layer, which is hidden while the viewer
     * shows the walk-view continent terrain only - so keep its toggles
     * hidden regardless of view mode. */
    if ($unplacedWrap)  $unplacedWrap.hidden  = true;
    if ($normalizeWrap) $normalizeWrap.hidden = true;
    if ($hazeWrap)      $hazeWrap.hidden      = meshOnly;
    if ($mistWrap)      $mistWrap.hidden      = meshOnly;
    if ($terrainWrap)   $terrainWrap.hidden   = meshOnly;
    if ($landmarksWrap) $landmarksWrap.hidden = meshOnly;
    if ($lockTopView)   $lockTopView.hidden   = meshOnly;
    if ($exportGlb)     $exportGlb.hidden     = true; /* loadWorldView unhides */
    $resetCam.hidden = false;
    /* Re-enter the active kingdom so the right path renders. */
    selectKingdom(currentKingdom);
  }
  $modeWorld.addEventListener('click', () => setViewMode('world'));
  $modeMesh .addEventListener('click', () => setViewMode('mesh'));
  if ($showUnplaced) {
    $showUnplaced.addEventListener('change', () => {
      if (viewMode === 'world') {
        /* Rerun the assembled-scene path so the unplaced grid appears or
         * clears. The world view only reads `$showUnplaced.checked` once
         * per load, not per-frame, so a checkbox flip has to retrigger. */
        selectKingdom(currentKingdom);
      }
    });
  }
  if ($normalizeUnplaced) {
    $normalizeUnplaced.addEventListener('change', () => {
      /* Re-run the layout pass so the per-class footprint normalisation
       * (and the centroid-anchor flag) is re-applied or dropped. */
      if (viewMode === 'world') selectKingdom(currentKingdom);
    });
  }
  if ($hazeEnable) {
    $hazeEnable.addEventListener('change', () => pushHazeToRenderer());
  }
  if ($mistEnable) {
    $mistEnable.addEventListener('change', () => updateMistStatus());
  }
  const $oceanEnable = document.getElementById('wo-ocean-enable');
  if ($oceanEnable) {
    $oceanEnable.addEventListener('change', () => {
      pushOceanParamsToRenderer();
    });
  }
  if ($showTerrain) {
    $showTerrain.addEventListener('change', () => {
      /* The ground draw flag is read per-frame, so a flip takes effect
       * without re-entering the kingdom. */
      if (glRenderer && typeof glRenderer.setGroundEnable === 'function') {
        glRenderer.setGroundEnable($showTerrain.checked);
      }
    });
  }
  if ($showLandmarks) {
    $showLandmarks.addEventListener('change', () => {
      /* The walk-frame landmark draw list is built once per kingdom load,
       * so a flip has to re-enter the kingdom to add or clear the layer. */
      if (viewMode === 'world') selectKingdom(currentKingdom);
    });
  }
  if ($lockTopView) {
    $lockTopView.addEventListener('click', () => {
      /* Prefer the per-kingdom anchor captured into world-overview.json
       * (`topview_cam`, sourced by `mednafen-state world-map-camera` from
       * the user's per-kingdom save state). Falls back to the hardcoded
       * KINGDOM_TOPVIEW_CAM defaults if the JSON field is missing. */
      const fromJson = placements?.[currentKingdom]?.topview_cam;
      const tv = fromJson
        ? { centerX: fromJson.cam_x, centerZ: fromJson.cam_z,
            azimuth: fromJson.azimuth, zoom: fromJson.zoom }
        : KINGDOM_TOPVIEW_CAM[currentKingdom];
      if (!tv) return;
      worldCam.centerX = tv.centerX;
      worldCam.centerZ = tv.centerZ;
      /* Top-view zoom field encodes a half-extent multiplier; the retail
       * dev-menu enters at zoom=0x170 which maps roughly to 1.0x of the
       * kingdom extent. Match that here so the lock action visually
       * matches the dev-menu spawn framing. */
      const k = placements?.[currentKingdom];
      const ext = k?.world_extent || [16320, 16320];
      worldCam.halfWidth  = ext[0] / 2;
      worldCam.halfHeight = ext[1] / 2;
      worldCam.yaw = 0;
      worldCam.pitch = 0;
    });
  }

  /* Push the distance-haze state into the GL renderer: retail's overworld
   * walk-view depth cue, with the kingdom's walk eye depth and pitch
   * (KINGDOM_WALK_EYE) and the camera centre as the virtual walk focus. */
  function pushHazeToRenderer() {
    if (!glRenderer || typeof glRenderer.setDistanceHaze !== 'function') return;
    const eye = KINGDOM_WALK_EYE[currentKingdom] || KINGDOM_WALK_EYE.drake;
    glRenderer.setDistanceHaze({
      enable: $hazeEnable ? $hazeEnable.checked : false,
      eyeZ: eye.eyeZ,
      scale: 6 * Math.cos(eye.pitch * Math.PI * 2 / 4096),
      origin: null,
    });
  }
  /* The "Mist:" status line: what the loaded kingdom's fog-region table
   * holds and what the overlay is drawing. */
  function updateMistStatus() {
    if (!$mistStatus) return;
    if (!mist) {
      $mistStatus.textContent = mistSheet
        ? 'Mist: load a kingdom in the assembled view to read its fog regions.'
        : 'Mist: fog sheet art not read from the disc (PROT 0874 missing).';
      return;
    }
    const st = mist.stats();
    const regs = mist.regions();
    const flags = [...new Set(regs.map(r => '0x' + r.flag.toString(16).toUpperCase()))];
    const on = $mistEnable && $mistEnable.checked;
    $mistStatus.textContent =
      `Mist: ${st.regions} fog regions in this kingdom's MAN (story flags ${flags.join(', ')}); `
      + `${st.mistTiles} tiles under Mist before it lifts`
      + (on ? `, ${st.live} sheets live of a ${st.cap} cap.` : ' - tick "Mist" to draw it.');
  }
  /* Push the latest per-kingdom ocean colour + enable flag into the GL
   * renderer. Composes the colour preferring the captured
   * `ocean_color_normalized` field in `world-overview.json`
   * (CLUT-sampled from the user's save state by
   * `scripts/mednafen/resolve_bulk_terrain.py::pick_ocean_color`) over
   * the hardcoded fallback. The checkbox in the canvas controls toggles
   * the pass; when off, the ocean plane is skipped entirely (canvas
   * clear-colour shows through). */
  function pushOceanParamsToRenderer() {
    if (!glRenderer || typeof glRenderer.setOceanColor !== 'function') return;
    const enable = $oceanEnable ? ($oceanEnable.checked ? 1 : 0) : 1;
    const ocn = placements?.[currentKingdom]?.ocean_color;
    let color;
    if (ocn && Array.isArray(ocn.ocean_color_normalized)
        && ocn.ocean_color_normalized.length === 3) {
      const [r, g, b] = ocn.ocean_color_normalized;
      color = { r, g, b };
    } else {
      /* Hardcoded fallback (royal blue close to #1F2466). Matches the
       * three retail saves we've sampled so far. */
      color = { r: 0.122, g: 0.141, b: 0.392 };
    }
    glRenderer.setOceanColor(color, enable, 0);
  }
  /* Hand the disc-side ocean assets (texture + 13-frame CLUT animation
   * table) to the renderer. Called after `viewer.set_scene_kingdom`
   * has decompressed slot 0 and `legaia_scene_formats::ocean::find_ocean_assets`
   * has run. When the kingdom isn't a world-map one (or the disc isn't
   * loaded yet), the accessors return empty buffers and we leave the
   * solid-colour fallback in place. */
  function pushOceanAssetsToRenderer() {
    if (!viewer || !glRenderer || typeof glRenderer.setOceanAssets !== 'function') {
      return;
    }
    const tex = viewer.ocean_texture_bytes();
    const frames = viewer.ocean_animation_frames();
    if (!tex || tex.byteLength === 0 || !frames || frames.byteLength === 0) {
      glRenderer.setOceanAssets(null, null);
      return;
    }
    /* tileWorldSize=256: one ocean-texture wrap covers 256 world units.
     * Each kingdom is 16320 world units across (~64 wraps), matching
     * the retail tile pitch in the prim pool. */
    glRenderer.setOceanAssets(tex, frames, 256);
  }

  /* The kingdom's slot-5 CLUT-walk shimmer (ocean head + the river and
   * shoreline cells beside it), stepped by the engine kernel every play host
   * runs (`viewer.kingdom_clut_tick` -> `ClutWalkAnim`). set_scene_kingdom
   * parks the walker's source strips into the kingdom VRAM; without the walk
   * the destination CLUTs keep another TIM's palette or zeros, and those
   * zero entries are transparent texels the backdrop plane shows through -
   * water sliding under the land as the camera turns. */
  let clutWalkActive = false;
  let clutLast;
  let clutAccum = 0;
  function startKingdomClutWalk() {
    clutWalkActive = !!(viewer && typeof viewer.kingdom_clut_walker_entries === 'function'
      && viewer.kingdom_clut_walker_entries() > 0);
    clutLast = undefined;
    clutAccum = 0;
    if (glRenderer && typeof glRenderer.setOceanWalkerDriven === 'function') {
      glRenderer.setOceanWalkerDriven(clutWalkActive);
      if (clutWalkActive) syncKingdomVram();
    }
  }
  function syncKingdomVram() {
    const bytes = viewer.pack_vram_bytes();
    glRenderer.uploadVram(bytes);
    glRenderer.setOceanClutFromVram(bytes);
  }
  /* Wall-clock vsyncs, not frames: the loop fires at the display rate. */
  function stepKingdomClut() {
    if (!clutWalkActive || !viewer || !glRenderer) return;
    const VSYNC_MS = 1000 / 60;
    const now = performance.now();
    if (clutLast === undefined) clutLast = now;
    clutAccum = Math.min(clutAccum + (now - clutLast), VSYNC_MS * 4);
    clutLast = now;
    const vsyncs = Math.floor(clutAccum / VSYNC_MS);
    if (vsyncs <= 0) return;
    clutAccum -= vsyncs * VSYNC_MS;
    if (viewer.kingdom_clut_tick(vsyncs)) syncKingdomVram();
  }
  /* Default UI matches default viewMode = 'world' (assembled scene from
   * disc). Mesh-inspector chrome stays hidden; reset-camera stays visible
   * (used by the top-down camera). */
  $prev.hidden = true; $next.hidden = true; $autoRotWrap.hidden = true;
  $resetCam.hidden = false;

  /* Empty-state placeholder painted on first load when no disc has been
   * uploaded yet. */
  function drawWorldPlaceholder() {
    if (rafId !== null) { cancelAnimationFrame(rafId); rafId = null; }
    if (glRenderer) { glRenderer.dispose(); glRenderer = null; }
    const fresh = freshCanvas();
    if (!fresh) return;
    const ctx = fresh.getContext('2d');
    ctx.fillStyle = '#0a0a1a';
    ctx.fillRect(0, 0, fresh.width, fresh.height);
    ctx.fillStyle = '#888'; ctx.font = '14px monospace';
    ctx.textAlign = 'center';
    ctx.fillText('Load a disc image above to render this kingdom\'s assembled scene.', fresh.width / 2, fresh.height / 2 - 8);
    ctx.fillStyle = '#666';
    ctx.fillText('No save state required.', fresh.width / 2, fresh.height / 2 + 14);
    $meshLabel.textContent = `${KINGDOM_PROT[currentKingdom].cdname} - waiting for disc`;
    $meshInfo.textContent = 'Load a disc image above to see the kingdom\'s landmark TMDs assembled in 3D.';
  }

  /* ---------- Load disc + WASM ----------- */
  /* Wires to the file input's `change` event below: as soon as the user
   * picks a disc image the page starts the WASM load + classify pass
   * (so there's no separate "Load" button to click). The same path is
   * also reachable programmatically via `loadDisc()` if we ever add a
   * "reload" affordance. */
  async function loadDisc(src) {
    /* `src` is a fresh File pick or a RomCache cache-backed source; both
     * expose name / size / arrayBuffer(). Fall back to the input's
     * current file if called with no argument (legacy reload path). */
    const f = src || $file.files[0];
    if (!f) { $status.textContent = 'Pick a .bin or .dat file first.'; return; }
    const prog = window.LoadProgress ? LoadProgress.create($status) : null;
    try {
      $status.textContent = `Reading ${f.name} (${(f.size/1024/1024).toFixed(1)} MB) ...`;
      const bytes = prog
        ? await prog.read(f, `Reading ${f.name}`)
        : new Uint8Array(await f.arrayBuffer());
      if (prog) prog.indeterminate('Initialising WASM decoder…');
      if (!wasmMod) {
        wasmMod = await LegaiaWasm.load();
      }
      $status.textContent = 'Classifying PROT entries ...';
      if (prog) { prog.indeterminate('Parsing PROT.DAT + classifying entries…'); await prog.paint(); }
      /* CRITICAL: replace the canvas before constructing the viewer.
       *
       * `load_disc` ends with `render_current()`, which for any TIM-only
       * entry calls `getContext("2d")` on the canvas. If the canvas was
       * previously bound to webgl2 (a prior disc load, or any
       * loadWorldView()/loadMesh() that ran in this session), that 2D
       * context request returns null and `load_disc` throws with
       * "no 2d context...".
       *
       * Wiping any pre-existing GL renderer + replacing the canvas with
       * a fresh DOM element gives load_disc a clean 2D-capable surface.
       * The WebGL2 context is rebound later by loadWorldView() /
       * loadMesh() on the fresh canvas. */
      freshCanvas();
      viewer = new wasmMod.LegaiaViewer('wo-canvas3d');
      const count = viewer.load_disc(bytes);
      allEntries = JSON.parse(viewer.entry_list_json());

      /* Bucket the entries per kingdom */
      kingdomSlots = { drake: [], sebucus: [], karisto: [] };
      for (let i = 0; i < allEntries.length; i++) {
        const e = allEntries[i];
        if (!e.has_tmd) continue;
        for (const k of Object.keys(KINGDOM_PROT)) {
          const r = KINGDOM_PROT[k];
          if (e.prot_index >= r.start && e.prot_index <= r.end) {
            kingdomSlots[k].push(i);
            break;
          }
        }
      }
      const tot = Object.values(kingdomSlots).reduce((a,b)=>a+b.length, 0);
      $status.textContent = `Loaded ${f.name} - ${count} viewable PROT entries, ${tot} TMDs in world-map blocks (Drake: ${kingdomSlots.drake.length}, Sebucus: ${kingdomSlots.sebucus.length}, Karisto: ${kingdomSlots.karisto.length}).`;
      if (prog) prog.done(`${count} PROT entries, ${tot} world-map TMDs.`);
      /* The Mist's sheet art: PROT 0874 section 2's (448, 0) page cells
       * through CLUT (0, 473) - the texels retail's fog pool samples. */
      mistSheet = null;
      try {
        if (typeof viewer.mist_sheet_rgba === 'function') {
          const rgba = viewer.mist_sheet_rgba();
          if (rgba && rgba.length > 0) mistSheet = rgba;
        }
      } catch (err) {
        console.warn('mist_sheet_rgba:', err);
      }
      updateMistStatus();
      $prev.disabled = false;
      $next.disabled = false;
      currentMeshIdx = 0;
      /* selectKingdom dispatches to loadWorldView or loadMesh based on viewMode. */
      selectKingdom(currentKingdom);
    } catch (err) {
      console.error(err);
      $status.textContent = 'Load failed: ' + (err.message || err);
      if (prog) prog.fail('Load failed: ' + (err.message || err));
    }
  }
  /* Cache the picked disc and auto-load it on return visits / reloads.
   * RomCache wires the input's change event (caching the pick) and, when
   * a disc is already cached, calls loadDisc(cachedSource) on init. */
  if (window.RomCache) {
    RomCache.attach($file, { onLoad: (f) => loadDisc(f) });
  } else {
    $file.addEventListener('change', () => { if ($file.files[0]) loadDisc(); });
  }

  /* ---------- Mesh navigation ----------- */
  function selectKingdom(k) {
    currentKingdom = k;
    history.replaceState(null, '', '#' + k);
    document.querySelectorAll('.wo-tab').forEach(b => b.classList.toggle('is-active', b.dataset.k === k));
    currentMeshIdx = 0;
    updateSnapshot();
    refreshMinimap();
    if (!viewer) {
      /* No disc loaded yet. Both 'world' and 'mesh' modes need the WASM
       * viewer + a parsed PROT TOC. Show the friendly placeholder so the
       * canvas doesn't sit empty until the user uploads a disc. */
      drawWorldPlaceholder();
      return;
    }
    if (viewMode === 'world') {
      loadWorldView();
    } else if (kingdomSlots[k].length > 0) {
      loadMesh();
    } else {
      $meshLabel.textContent = `${KINGDOM_PROT[k].cdname}: no viewable TMDs`;
    }
  }

  /* Freshen the canvas (WebGL context is single-shot per canvas, and we
   * also need a clean depth buffer when switching kingdoms or modes). */
  function freshCanvas() {
    /* The canvas (and with it the GL context an XR layer is bound to) is about
     * to be replaced - a live session cannot survive that. */
    if (vr) { vr.end(); vr.setReady(false); }
    worldTick = null;
    worldDrawState = null;
    groundSampler = null;
    if (rafId !== null) { cancelAnimationFrame(rafId); rafId = null; }
    if (mist) { mist.dispose(); mist = null; }
    if (glRenderer) { glRenderer.dispose(); glRenderer = null; }
    const old = document.getElementById('wo-canvas3d');
    if (!old) return null;
    const fresh = document.createElement('canvas');
    fresh.id = 'wo-canvas3d';
    fresh.className = 'viewer-canvas';
    fresh.width = old.width || 800;
    fresh.height = old.height || 600;
    old.replaceWith(fresh);
    return fresh;
  }

  /* Assembled top-down world view. Uses the kingdom 7-asset bundle:
   * slot 0 (TIM_LIST) -> shared VRAM, slot 1 (TMD pack) -> per-placement
   * mesh. The pack_slot for each placement is precomputed in
   * world-overview.json's `tmd_source` block. */
  function loadWorldView() {
    if (!viewer) return;
    worldExportState = null;
    if ($exportGlb) $exportGlb.hidden = true;
    const k = placements?.[currentKingdom];
    if (!k) {
      $meshLabel.textContent = 'placement JSON missing';
      return;
    }
    const fresh = freshCanvas();
    if (!fresh) return;
    try {
      glRenderer = new window.TmdRenderer(fresh);
    } catch (err) {
      $meshInfo.textContent = 'WebGL2 init failed: ' + (err.message || err);
      return;
    }
    /* Top-view-lock, haze and Mist become available in world-view mode;
     * mesh inspector mode hides them again. */
    if ($lockTopView) $lockTopView.hidden = false;
    pushHazeToRenderer();
    pushOceanParamsToRenderer();
    /* Load kingdom bundle (TIM_LIST -> VRAM, TMD pack parsed). */
    let packCount;
    try {
      packCount = viewer.set_scene_kingdom(k.prot_base);
    } catch (err) {
      $meshInfo.textContent = 'set_scene_kingdom failed: ' + (err.message || err);
      return;
    }
    glRenderer.uploadVram(viewer.pack_vram_bytes());
    /* Upload the disc-side ocean texture + 13-frame CLUT animation
     * table now that slot 0 has been decompressed. */
    pushOceanAssetsToRenderer();
    startKingdomClutWalk();
    /* Walk-view continent ground: the procedural heightfield surface the
     * native engine draws (per-cell terrain-atlas textured from the same
     * slot-0 VRAM uploaded above). set_scene_kingdom already built it;
     * upload it once here and let renderAssembled draw it under the
     * landmark placements. */
    const groundQuads = viewer.walk_ground_quad_count();
    if (groundQuads > 0) {
      glRenderer.uploadGround(
        viewer.walk_ground_positions(),
        viewer.walk_ground_uvs(),
        viewer.walk_ground_cba_tsb(),
        viewer.walk_ground_indices(),
      );
    } else {
      glRenderer.uploadGround(new Float32Array(0), null, null, new Uint32Array(0));
    }
    glRenderer.setGroundEnable(!$showTerrain || $showTerrain.checked);
    /* Terrain-height sampler for the VR "on the ground" mode. */
    groundSampler = groundQuads > 0 ? buildGroundSampler() : null;
    /* The Mist: the kingdom MAN's fog regions drawn through retail's fog
     * pool rules (world-overview-mist.js), over the same depth buffer. */
    if (mistSheet && typeof viewer.walk_mist_regions === 'function'
        && window.LegaiaWorldOverviewMist) {
      try {
        mist = window.LegaiaWorldOverviewMist.create(glRenderer.gl);
        mist.setSheet(mistSheet);
        const km = KINGDOM_MIST[currentKingdom] || { flagsSet: [], yControl: 60 };
        const sampler = groundSampler;
        mist.setKingdom({
          regions: viewer.walk_mist_regions(k.prot_base),
          flagsSet: km.flagsSet,
          yControl: km.yControl,
          groundUp: (x, z) => (sampler ? sampler(x + 64, z + 64) : 0),
        });
      } catch (err) {
        console.warn('mist:', err);
        mist = null;
      }
    }
    updateMistStatus();
    /* The top-down map plots this same load: gathered here, off the
     * accessors rather than the draw list, so the "landmarks + trees"
     * toggle hides the 3D layer without emptying the map. */
    minimapData[currentKingdom] = gatherMinimapData();
    refreshMinimap();
    /* Upload a single pack mesh on demand; return true if its mesh data
     * is renderable. The accessors hand back the HYBRID stream (the shared
     * `build_hybrid_pack_mesh` kernel): the textured prims first, then the
     * untextured F* / G* vertex-colour prims retail draws through the same
     * per-prim dispatch - Rim Elm's four hut roofs are 24 gouraud triangles
     * that a textured-only upload left as open rings. */
    const used = new Set();
    function ensureMeshUploaded(ms) {
      if (ms < 0 || ms >= packCount) return false;
      if (used.has(ms)) return true;
      viewer.pack_mesh(ms);
      const positions = viewer.pack_mesh_positions();
      const uvs = viewer.pack_mesh_uvs();
      const cba = viewer.pack_mesh_cba_tsb();
      const idx = viewer.pack_mesh_indices();
      if (positions.length === 0 || idx.length === 0) return false;
      /* Packet colours, `[r, g, b, flag]` per vertex: flag 255 = textured,
       * the shader modulates the texel by the RGB (`texel * colour / 128`,
       * retail's whole lighting model); flag 0 = untextured, the shader
       * fills with the RGB (renderAssembled turns the fill branch on per
       * mesh that carries the stream). */
      glRenderer.uploadSceneMesh(ms, positions, uvs, cba, idx,
        viewer.pack_mesh_flat_rgba());
      used.add(ms);
      return true;
    }

    /* Legacy placement layers (MAN-table landmarks, live-RAM actors,
     * unplaced slot-1 TMD layout grid). These are positioned in the
     * top-down OVERVIEW coordinate frame, which is misaligned with the
     * walk-view heightfield the viewer now draws as the real continent.
     * Until they're re-derived in the walk frame, show terrain only.
     * Flip this to re-enable the old layer (the code below is preserved
     * so the snapshot panel + toggles keep working when it returns). */
    const SHOW_LEGACY_PLACEMENTS = false;
    const drawPlacements = [];

    /* Walk-frame pack-mesh stamps, two disjoint layers merged by the WASM
     * `build_walk_placements` into the SAME col*128 world frame as the
     * heightfield ground above:
     *   - placed landmarks (FUN_8003A55C, flags & 0x4), and
     *   - the decoration layer (walk-visible cells stamping a nonzero
     *     record[+0x10] mesh with the mesh-drawn flag 0x2 and no placed
     *     flag - the crossed-quad billboard trees, mountain groups, and
     *     props; ~210-300 cells per kingdom).
     * They overlay the terrain correctly, unlike the legacy overview-frame
     * `world-overview.json` placements (kept behind SHOW_LEGACY_PLACEMENTS
     * below). Each stamp draws its pack mesh at world (x, y, z) with scale
     * 1 and the shared (1,-1,1) Y-flip; world Y comes from the
     * floor-height LUT so the mesh sits on the heightfield. */
    let walkLandmarkCount = 0;
    const showLandmarks = !$showLandmarks || $showLandmarks.checked;
    if (showLandmarks) {
      const suppress = WALK_STAMP_SUPPRESS[currentKingdom] || [];
      const wpCount = viewer.walk_placement_count();
      if (wpCount > 0) {
        const slots = viewer.walk_placement_slots();
        const pos = viewer.walk_placement_positions();
        /* Authored per-placement yaw (object record +0x0A, PSX 4096-per-rev
         * units) - the Sebucus island bridges' quarter-turns and the
         * decoration layer's per-tree variety. Retail's matrix builder
         * (FUN_80026988) maps local +Z to (sin, cos) in world XZ, the
         * opposite yaw sense of placementModelScaledY, hence the negation.
         * Guarded so a stale cached WASM without the accessor still draws
         * (unrotated). */
        const rots = (typeof viewer.walk_placement_rot_y === 'function')
          ? viewer.walk_placement_rot_y() : null;
        /* 1 = placed object, 0 = decoration-layer stamp. */
        const layer = (typeof viewer.walk_placement_placed === 'function')
          ? viewer.walk_placement_placed() : null;
        for (let i = 0; i < wpCount; i++) {
          const ms = slots[i];
          if (suppress.some((s) =>
            s.mesh === ms && s.x === pos[i * 3] && s.z === pos[i * 3 + 2])) {
            continue;
          }
          if (!ensureMeshUploaded(ms)) continue;
          /* The WASM returns retail-frame world Y (PSX +Y down: the stamp
           * anchor is `-lut[nibble] + y_off`, the heightfield's PRE-flip
           * stored-vertex frame). placementModelScaledY flips only the
           * mesh-local geometry, not the translation - while the ground
           * bakes `-lut` into its vertices and gets the renderer's
           * (1,-1,1) model, landing at `+lut` (up). Negate the placement Y
           * so stamps sit ON the terrain instead of mirrored below it -
           * the same negation the viewer.html full-map path applies.
           * Kingdom LUT heights are small, so the un-negated artifact was
           * a subtle sink/hover that doubled with the cell height. */
          drawPlacements.push({
            meshId: ms,
            x: pos[i * 3],
            y: -pos[i * 3 + 1],
            z: pos[i * 3 + 2],
            rotY: rots ? -(rots[i] & 0xFFF) * Math.PI / 2048 : 0,
            scale: 1.0,
            kind: 'walk_landmark',
            class: 'landmark',
            /* A decoration-layer stamp: the distance haze cues it by its
             * origin toward 0xD0 (FUN_801F69D8); placed landmarks are not
             * that sweep's and take no cue. */
            hazeDeco: !!(layer && layer[i] === 0),
          });
          walkLandmarkCount++;
        }
      }
    }
    /* Headless-verification hook (same spirit as `__woCam`): the walk
     * stamps actually queued this render, so browser tests can assert on
     * placement contents. Not used by page code. */
    window.__woWalkStamps = drawPlacements.filter(
      (p) => p.kind === 'walk_landmark');
    /* MAN-table placements at the disc-encoded world coordinates. Counts
     * by source so the snapshot panel surfaces how many placements made
     * it onto the GPU vs were dropped for the global-pool gap. */
    const manSlots = new Set();
    const renderCounts = { scene_pack: 0, global_pool: 0, skipped: 0 };
    for (const p of (SHOW_LEGACY_PLACEMENTS ? k.placements : [])) {
      if (p.script_positioned) continue;
      if (!p.tmd_source) {
        renderCounts.skipped++;
        continue;
      }
      if (p.tmd_source.kind === 'scene_tmd_pack') {
        const ms = p.tmd_source.pack_slot;
        if (!ensureMeshUploaded(ms)) {
          renderCounts.skipped++;
          continue;
        }
        manSlots.add(ms);
        drawPlacements.push({
          meshId: ms, x: p.pos[0], z: p.pos[2],
          kind: 'man', class: p.class || 'landmark', name: p.name,
        });
        renderCounts.scene_pack++;
        continue;
      }
      if (p.tmd_source.kind === 'global_pool') {
        /* Global TMD references (slot >= 0xF0) resolve through retail's
         * DAT_8007C018 table - the disc-side global mesh pool is not yet
         * bundled into world-overview.json. Until that pipeline lands,
         * stamp a placeholder using the kingdom pack's smallest TMD
         * (slot 0 - typically a ground tile) so the position is visible.
         * Tagged 'global_pool' so the snapshot panel can call them out. */
        if (!ensureMeshUploaded(0)) {
          renderCounts.skipped++;
          continue;
        }
        manSlots.add(0);
        drawPlacements.push({
          meshId: 0, x: p.pos[0], z: p.pos[2],
          kind: 'global_pool',
          class: p.class || 'landmark',
          name: p.name,
          global_index: p.tmd_source.global_index,
        });
        renderCounts.global_pool++;
        continue;
      }
      renderCounts.skipped++;
    }
    /* Live-RAM actor placements (resolve_actor_tmds.py /
     * scripts/mednafen/resolve_bulk_terrain.py output). These pin
     * actor-positioned slots that MAN doesn't reference. The new mednafen
     * resolver also tags each entry with `kind: 'bulk_terrain'` or
     * 'man_actor' so we can split the draw list into a separate
     * `bulk_terrain` track (rendered with class-conditioned colour).
     *
     * `bulk_terrain_placements` is a strict subset of `live_placements`
     * (kind === 'bulk_terrain'). We honour `live_placements` directly so
     * the legacy class colouring still works, and treat bulk_terrain as
     * a tagged sub-bucket. */
    const liveSlots = new Set();
    const livePlacements = SHOW_LEGACY_PLACEMENTS ? (k.live_placements || []) : [];
    for (const lp of livePlacements) {
      const ms = lp.pack_slot;
      if (!ensureMeshUploaded(ms)) continue;
      liveSlots.add(ms);
      drawPlacements.push({
        meshId: ms, x: lp.pos[0], z: lp.pos[2],
        kind: lp.kind === 'bulk_terrain' ? 'bulk_terrain' : 'live',
        class: lp.class || 'landmark',
        name: lp.man_record_name,
      });
    }

    /* Optional: render the unplaced slot-1 TMDs. MAN surfaces only a
     * subset of the kingdom pack; the rest are positioned by the
     * runtime field-VM via actor lists. Until the disc-side init-blob
     * walker lands we lay these out by classification:
     *   landmark    - row south of the world bounds (sorted by slot)
     *   ground_tile - stacked at kingdom centre
     *   decoration  - row north of the world bounds
     *   npc_token   - hidden
     *   unknown     - canonical grid east of the world bounds
     * The class field comes from world-overview/slot1_classification.toml
     * and is rolled into world-overview.json by the build pipeline. */
    const ext = k.world_extent || [16320, 16320];
    const unplacedShown = { landmark: 0, decoration: 0, ground_tile: 0,
                            npc_token: 0, unknown: 0 };
    const normalize = !$normalizeUnplaced || $normalizeUnplaced.checked;
    /* Layout helper: given an uploaded mesh's AABB + class target
     * footprint, compute the per-placement world scale that maps the
     * mesh's larger XZ extent onto the target. Returns the legacy
     * MESH_SCALE constant when normalisation is disabled or the AABB
     * is degenerate (zero-extent mesh).
     *
     * MESH_SCALE = 6.0 (from webgl-tmd.js) is the legacy constant scale
     * factor; we hard-code the same value here so we don't need to plumb
     * a getter through the renderer. */
    const LEGACY_MESH_SCALE = 6.0;
    function classScale(aabb, cls) {
      if (!normalize) return LEGACY_MESH_SCALE;
      if (!aabb) return LEGACY_MESH_SCALE;
      const footprint = Math.max(aabb.sx, aabb.sz);
      if (footprint <= 0) return LEGACY_MESH_SCALE;
      const target = UNPLACED_TARGET_FOOTPRINT[cls]
        ?? UNPLACED_TARGET_FOOTPRINT.unknown;
      return target / footprint;
    }
    /* Helper for layout: each unplaced placement uses centroid anchoring
     * + per-class scale. Returns the new draw record (does NOT push it). */
    function buildUnplacedDraw(u, cls, x, z) {
      const aabb = glRenderer.getMeshAabb(u.pack_slot);
      const scale = classScale(aabb, cls);
      return {
        meshId: u.pack_slot, x, z, rotY: 0,
        kind: 'unplaced', class: cls,
        slot: u.pack_slot, note: u.note,
        scale,
        anchor: normalize ? 'centroid' : 'origin',
      };
    }
    if (SHOW_LEGACY_PLACEMENTS && $showUnplaced && $showUnplaced.checked) {
      /* Drop entries the live extract already placed in real
       * coordinates - drawing them twice produces visual duplicates. */
      const unplacedAll = (k.unplaced_slot1_tmds || [])
        .filter(u => !liveSlots.has(u.pack_slot));
      const byClass = { landmark: [], decoration: [], ground_tile: [],
                        npc_token: [], unknown: [] };
      for (const u of unplacedAll) {
        const cls = u.class || 'unknown';
        (byClass[cls] || byClass.unknown).push(u);
      }
      /* Landmarks: sort by slot, lay in a row south of the world. Spacing
       * is the larger of (a) the world width / count - so a 25-landmark
       * row spans the kingdom - and (b) 1.4 x the class target footprint
       * so neighbouring meshes don't visually collide after scaling. */
      const landmarks = byClass.landmark;
      landmarks.sort((a, b) => a.pack_slot - b.pack_slot);
      {
        const target = UNPLACED_TARGET_FOOTPRINT.landmark;
        const pitch = Math.max(ext[0] / Math.max(landmarks.length, 1),
                               target * 1.4);
        const rowSpan = pitch * landmarks.length;
        const xStart = (ext[0] - rowSpan) / 2;
        for (let i = 0; i < landmarks.length; i++) {
          const u = landmarks[i];
          if (!ensureMeshUploaded(u.pack_slot)) continue;
          const x = xStart + (i + 0.5) * pitch;
          const z = ext[1] + Math.max(1500, target);
          drawPlacements.push(buildUnplacedDraw(u, 'landmark', x, z));
          unplacedShown.landmark++;
        }
      }
      /* Decorations: row north of the world, same spacing rule. */
      byClass.decoration.sort((a, b) => a.pack_slot - b.pack_slot);
      {
        const target = UNPLACED_TARGET_FOOTPRINT.decoration;
        const pitch = Math.max(ext[0] / Math.max(byClass.decoration.length, 1),
                               target * 1.5);
        const rowSpan = pitch * byClass.decoration.length;
        const xStart = (ext[0] - rowSpan) / 2;
        for (let i = 0; i < byClass.decoration.length; i++) {
          const u = byClass.decoration[i];
          if (!ensureMeshUploaded(u.pack_slot)) continue;
          const x = xStart + (i + 0.5) * pitch;
          const z = -Math.max(1500, target);
          drawPlacements.push(buildUnplacedDraw(u, 'decoration', x, z));
          unplacedShown.decoration++;
        }
      }
      /* Ground tiles: spread in a small grid west of the kingdom so
       * each silhouette is visible. The runtime tiles them via the
       * overlay-routed dispatch; this layout is the closest we can
       * approximate without porting the bulk emit mechanism. */
      {
        const tiles = byClass.ground_tile;
        const target = UNPLACED_TARGET_FOOTPRINT.ground_tile;
        const pitch = target * 1.2;
        const cols = Math.max(1, Math.ceil(Math.sqrt(tiles.length)));
        const gridSpan = pitch * cols;
        const xBase = -gridSpan - 1500;
        const zBase = (ext[1] - gridSpan) / 2;
        for (let i = 0; i < tiles.length; i++) {
          const u = tiles[i];
          if (!ensureMeshUploaded(u.pack_slot)) continue;
          const col = i % cols;
          const row = Math.floor(i / cols);
          const x = xBase + (col + 0.5) * pitch;
          const z = zBase + (row + 0.5) * pitch;
          drawPlacements.push(buildUnplacedDraw(u, 'ground_tile', x, z));
          unplacedShown.ground_tile++;
        }
      }
      /* npc_token: skipped on purpose (the count is reported in the
       * status line so the user can confirm). */
      unplacedShown.npc_token = byClass.npc_token.length;
      /* Unknown: grid east of the world. Same normalisation rule. */
      const unknowns = byClass.unknown;
      unknowns.sort((a, b) => a.pack_slot - b.pack_slot);
      {
        const target = UNPLACED_TARGET_FOOTPRINT.unknown;
        const pitch = target * 1.4;
        const ucols = Math.max(1, Math.ceil(Math.sqrt(unknowns.length)));
        const uxBase = ext[0] + 1500;
        for (let i = 0; i < unknowns.length; i++) {
          const u = unknowns[i];
          if (!ensureMeshUploaded(u.pack_slot)) continue;
          const col = i % ucols;
          const row = Math.floor(i / ucols);
          const x = uxBase + (col + 0.5) * pitch;
          const z = (row + 0.5) * pitch;
          drawPlacements.push(buildUnplacedDraw(u, 'unknown', x, z));
          unplacedShown.unknown++;
        }
      }
    }
    frameWorldCam(drawPlacements, ext, glRenderer.getGroundAabb());
    worldCam.yaw = 0;
    worldCam.pitch = WORLD_DEFAULT_PITCH;
    const unplacedShownTotal = unplacedShown.landmark
      + unplacedShown.decoration + unplacedShown.ground_tile
      + unplacedShown.unknown;
    const placedShownCount = drawPlacements.length - unplacedShownTotal;
    const unplacedSummary = unplacedShownTotal === 0 ? '' :
      ` + ${unplacedShownTotal} unplaced (`
      + [
          unplacedShown.landmark    ? `${unplacedShown.landmark}L` : null,
          unplacedShown.ground_tile ? `${unplacedShown.ground_tile}G` : null,
          unplacedShown.decoration  ? `${unplacedShown.decoration}D` : null,
          unplacedShown.unknown     ? `${unplacedShown.unknown}?` : null,
        ].filter(Boolean).join(' ')
      + (unplacedShown.npc_token ? `, ${unplacedShown.npc_token} hidden` : '')
      + ')';
    /* Split live actors into bulk_terrain (positioned by some non-MAN
     * runtime path - the FieldVM prescript chain via FUN_801DE840, the
     * world-map overlay's actor-spawn calls into FUN_80024C88, etc.)
     * vs man_actor (positioned via the standard MAN-record walker at
     * FUN_8003A1E4). The 'kind' tag comes from the mednafen resolver in
     * scripts/mednafen/resolve_bulk_terrain.py; older PCSX-Redux runs
     * miss it and fall under man_actor for back-compat. */
    const bulkPlacementsLive = livePlacements.filter(
      lp => lp.kind === 'bulk_terrain'
    );
    const manPlacementsLive = livePlacements.filter(
      lp => lp.kind !== 'bulk_terrain'
    );
    const liveSummary = liveSlots.size === 0 ? '' :
      ` + ${livePlacements.length} live-RAM actors `
      + `(${bulkPlacementsLive.length} bulk_terrain, `
      + `${manPlacementsLive.length} man_actor; `
      + `${liveSlots.size} slots)`;
    /* Global-pool placeholders render at MAN coords with a generic
     * scene-pack stand-in mesh; surface the count so the gap is visible. */
    const globalPoolSummary = renderCounts.global_pool === 0 ? '' :
      ` + ${renderCounts.global_pool} global-pool placeholders`;
    const skippedSummary = renderCounts.skipped === 0 ? '' :
      ` (${renderCounts.skipped} skipped)`;
    if (SHOW_LEGACY_PLACEMENTS) {
      const terrainSummary = groundQuads > 0
        ? ` + ${groundQuads}-cell ground heightfield`
        : '';
      $meshLabel.textContent =
        `${k.cdname} - ${placedShownCount} placements (${used.size} unique meshes, ${packCount}-TMD pack)`
        + terrainSummary + liveSummary + globalPoolSummary + unplacedSummary + skippedSummary;
    } else {
      const landmarkSummary = walkLandmarkCount > 0
        ? ` + ${walkLandmarkCount} landmarks/decorations (trees, mountains; ${used.size} meshes)`
        : '';
      $meshLabel.textContent = groundQuads > 0
        ? `${k.cdname} - ${groundQuads}-cell continent heightfield (walk-view terrain)${landmarkSummary}`
        : (walkLandmarkCount > 0
            ? `${k.cdname} - ${walkLandmarkCount} placed landmarks (no terrain resolved)`
            : `${k.cdname} - no walk-view terrain resolved`);
    }
    $meshInfo.textContent =
      `3D perspective view; world ${ext[0]} x ${ext[1]} units; `
      + `drag to pan, right-drag (or shift+drag) to rotate/tilt, scroll to zoom`;
    attachWorldControls(fresh);
    /* Arm the .glb export with what this load actually put on screen.
     * Guarded on the WASM builder existing so a stale cached module
     * (before scene_export_* landed) just leaves the button hidden. */
    worldExportState = { draws: drawPlacements, hasGround: groundQuads > 0 };
    if ($exportGlb) {
      $exportGlb.hidden = (typeof viewer.scene_export_begin !== 'function');
    }
    worldDrawState = { draws: drawPlacements, ext };
    if (vr) vr.setReady(true);
    let mistStatusAt = 0;
    worldTick = () => {
      stepKingdomClut();
      glRenderer.renderAssembled(worldDrawState.draws, worldDrawState.ext, worldCam);
      if (mist && $mistEnable && $mistEnable.checked) {
        mist.draw(glRenderer.lastVp);
        const now = performance.now();
        if (now - mistStatusAt > 1000) { mistStatusAt = now; updateMistStatus(); }
      }
      rafId = requestAnimationFrame(worldTick);
    };
    rafId = requestAnimationFrame(worldTick);
  }

  /* Frame the top-down camera on the placement cluster.
   *
   * If a retail camera anchor is known for the current kingdom
   * (KINGDOM_CAM), centre on that instead of the placement-cluster
   * average and widen halfWidth/halfHeight to enclose both the anchor
   * and the cluster. This keeps the viewer's initial view aligned with
   * the retail spawn (where the kingdom's exit drops the player onto
   * the world map) while still showing every placed landmark. */
  function frameWorldCam(drawPlacements, ext, groundAabb) {
    Object.assign(
      worldCam,
      frameCamera(KINGDOM_CAM[currentKingdom], drawPlacements, ext, groundAabb),
    );
  }

  /* Pan / orbit / zoom controls for the world camera. Left-drag pans the
   * map (yaw-aware); right-drag or shift+left-drag orbits (yaw + tilt);
   * wheel zooms. Shared with viewer.html's full-map mode
   * (attachWorldOrbitControls in webgl-math.js); replaces the
   * mesh-inspector orbit controls in `attachCameraControls` for
   * world-view mode. */
  function attachWorldControls(canvas) {
    attachWorldOrbitControls(canvas, worldCam, { minHalf: 200, maxHalf: 20000 });
  }
  function loadMesh() {
    if (!viewer) return;
    const slots = kingdomSlots[currentKingdom];
    if (slots.length === 0) return;
    const slot = slots[currentMeshIdx];
    const fresh = freshCanvas();
    if (!fresh) return;
    viewer.set_slot(slot);
    const entry = allEntries[slot];
    const positions = viewer.mesh_positions();
    const uvs = viewer.mesh_uvs();
    const cbaTsb = viewer.mesh_cba_tsb();
    const indices = viewer.mesh_indices();
    const bounds = viewer.mesh_bounds();
    const vram = viewer.current_vram_bytes();
    $meshLabel.textContent = `[${currentMeshIdx+1}/${kingdomSlots[currentKingdom].length}] PROT ${entry.prot_index} - ${entry.class}`;
    $meshInfo.textContent = `${positions.length/3} verts, ${indices.length/3} tris`;
    if (positions.length === 0 || indices.length === 0) {
      $meshInfo.textContent += ' (no textured prims - flat-shaded TMD)';
      return;
    }
    try {
      glRenderer = new window.TmdRenderer(fresh);
    } catch (err) {
      $meshInfo.textContent += ' (WebGL2 init failed)';
      return;
    }
    glRenderer.uploadVram(vram);
    glRenderer.uploadMesh(positions, uvs, cbaTsb, indices,
      viewer.mesh_flat_rgba());
    const center = [bounds[0], bounds[1], bounds[2]];
    const radius = bounds[3];
    attachCameraControls(fresh);
    let last = performance.now();
    const tick = (now) => {
      const dt = (now - last) / 1000;
      last = now;
      if (cam.autoRotate) cam.yaw += dt * 0.6;
      glRenderer.render(cam.yaw, cam.pitch, cam.distance, cam.panX, cam.panY, center, radius);
      rafId = requestAnimationFrame(tick);
    };
    rafId = requestAnimationFrame(tick);
  }
  $prev.addEventListener('click', () => {
    if (!viewer) return;
    const n = kingdomSlots[currentKingdom].length;
    if (n === 0) return;
    currentMeshIdx = (currentMeshIdx - 1 + n) % n;
    loadMesh();
  });
  $next.addEventListener('click', () => {
    if (!viewer) return;
    const n = kingdomSlots[currentKingdom].length;
    if (n === 0) return;
    currentMeshIdx = (currentMeshIdx + 1) % n;
    loadMesh();
  });
  window.addEventListener('keydown', e => {
    if (e.target.tagName === 'INPUT') return;
    if (e.key === 'ArrowRight' || e.key === 'n') $next.click();
    else if (e.key === 'ArrowLeft' || e.key === 'p') $prev.click();
  });

  /* ---------- Tabs ----------- */
  document.querySelectorAll('.wo-tab').forEach(btn => {
    btn.addEventListener('click', () => selectKingdom(btn.dataset.k));
  });
  /* Apply initial active tab */
  document.querySelectorAll('.wo-tab').forEach(b => b.classList.toggle('is-active', b.dataset.k === currentKingdom));

  /* ---------- Snapshot stats + top-down map ----------- */

  /* Everything the top-down map plots for the loaded kingdom, read off the
   * same WASM accessors the assembled scene draws from - so the two views
   * cannot disagree about where anything is. Stamps the 3D scene suppresses
   * (WALK_STAMP_SUPPRESS) are left out here too and counted. */
  function gatherMinimapData() {
    const ground = classifyGround(viewer.walk_ground_positions(), viewer.walk_ground_cba_tsb());
    if (!ground) return null;
    const suppress = WALK_STAMP_SUPPRESS[currentKingdom] || [];
    const stamps = [];
    let suppressed = 0;
    const n = viewer.walk_placement_count();
    const slots = n > 0 ? viewer.walk_placement_slots() : [];
    const pos = n > 0 ? viewer.walk_placement_positions() : [];
    /* Guarded like walk_placement_rot_y: a stale cached WASM without the
     * accessor still plots, as one undivided family. */
    const layer = (n > 0 && typeof viewer.walk_placement_placed === 'function')
      ? viewer.walk_placement_placed() : null;
    for (let i = 0; i < n; i++) {
      const x = pos[i * 3], z = pos[i * 3 + 2];
      if (suppress.some((s) => s.mesh === slots[i] && s.x === x && s.z === z)) {
        suppressed++;
        continue;
      }
      stamps.push({ x, z, slot: slots[i], placed: layer ? layer[i] === 1 : false });
    }
    let labels = null;
    if (typeof viewer.world_map_labels_json === 'function') {
      try {
        labels = JSON.parse(viewer.world_map_labels_json())
          .map((l) => ({ x: l.world_x, z: l.world_z, name: l.name, index: l.index }));
      } catch (e) { labels = null; }
    }
    return {
      ground, stamps, suppressed,
      labels: labels || [],
      hasLabels: labels !== null,
      hasLayers: layer !== null,
    };
  }

  function refreshMinimap() {
    if (!minimap) return;
    const title = KINGDOM_PROT[currentKingdom].label;
    const data = minimapData[currentKingdom] || null;
    const hint = viewer
      ? ['Open the assembled scene to plot', title]
      : ['Load a disc image to plot', title];
    const drawn = minimap.show(currentKingdom, title, data, hint);
    renderMinimapLegend(data, drawn.labelsDrawn);
  }

  /* The legend under the map: one row per dot family, in the dot's own
   * colour and shape, with its count - and one row for what is deliberately
   * not plotted, so nothing goes missing silently. */
  function renderMinimapLegend(data, labelsDrawn) {
    if ($legend) {
      if (!data) {
        $legend.innerHTML = '';
      } else {
        const objects = data.stamps.filter((s) => s.placed).length;
        const rows = [];
        const hidden = data.labels.length - labelsDrawn;
        rows.push(`<li><span class="wo-key wo-key-place"></span>named places: ${data.labels.length}` +
          (data.hasLabels ? '' : ' (this site bundle predates the label reader)') +
          (hidden > 0 ? ` (${hidden} unlabelled - hover it)` : '') + '</li>');
        if (data.hasLayers) {
          rows.push(`<li title="Objects the game spawns on the map: gates, towers, bridges, rail."><span class="wo-key wo-key-object"></span>placed objects: ${objects}</li>`);
          rows.push(`<li title="Trees, mountain groups and small props."><span class="wo-key wo-key-deco"></span>decorations: ${data.stamps.length - objects}</li>`);
        } else {
          rows.push(`<li><span class="wo-key wo-key-deco"></span>map stamps: ${data.stamps.length}</li>`);
        }
        const k = placements?.[currentKingdom];
        const scripted = k ? (k.script_positioned_count ?? 0) : 0;
        const omitted = [];
        if (scripted) omitted.push(`${scripted} script-positioned actor${scripted === 1 ? '' : 's'}`);
        if (data.suppressed) omitted.push(`${data.suppressed} script-managed stamp${data.suppressed === 1 ? '' : 's'}`);
        if (omitted.length) {
          rows.push(`<li class="wo-key-note">not plotted, no fixed position: ${omitted.join(', ')}</li>`);
        }
        $legend.innerHTML = rows.join('');
      }
    }
    if ($nameList) {
      $nameList.innerHTML = '';
      if (!data) {
        $nameList.innerHTML = '<li class="wo-key-note">Place names are read from your disc.</li>';
        return;
      }
      for (const l of data.labels) {
        const li = document.createElement('li');
        li.innerHTML = `<span class="wo-key wo-key-place"></span><span class="wo-name">${escapeHtml(l.name)}</span> ` +
          `<span class="wo-coord">(${l.x}, ${l.z})</span>`;
        li.addEventListener('pointerenter', () => minimap.highlightPlace(l));
        li.addEventListener('pointerleave', () => minimap.highlightPlace(null));
        li.addEventListener('click', () => minimap.highlightPlace(l));
        $nameList.appendChild(li);
      }
    }
  }

  function updateSnapshot() {
    if (!placements || !placements[currentKingdom]) {
      $statsDl.innerHTML = '<dt>data</dt><dd>placement JSON missing</dd>';
      return;
    }
    const k = placements[currentKingdom];
    const fixed = k.world_placed_count ?? k.placements.filter(p => !p.script_positioned).length;
    const scripted = k.script_positioned_count ?? (k.placements.length - fixed);
    const packCount = k.tmd_pack ? k.tmd_pack.count : (k.tmd_count ?? '?');
    const globalRefs = k.placements.filter(p => p.tmd_slot >= 0xF0).length;
    $statsDl.innerHTML = `
      <dt>kingdom</dt>         <dd>${escapeHtml(k.label)} <span class="wo-coord" title="internal scene id">${escapeHtml(k.cdname || '')}</span></dd>
      <dt>PROT base</dt>       <dd>${k.prot_base ?? '?'}</dd>
      <dt>mesh pack</dt>       <dd>${packCount} meshes</dd>
      <dt>scripted actors</dt> <dd>${k.placements.length} (${fixed} at a fixed tile, ${scripted} positioned by script; ${globalRefs} use a party model)</dd>
    `;
  }
})();
