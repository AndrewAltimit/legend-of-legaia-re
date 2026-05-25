/* world-overview-app.js — viewer controller for the world-overview page.
 *
 * Extracted from site/world-overview.html (and its _content/ mirror) for
 * file modularity. Loads as a classic global script after webgl-{shaders,
 * math,tmd}.js. The top-level IIFE was preserved verbatim so the DOM
 * dependencies (the canvas + control IDs in the page) keep working
 * unchanged. Bump the ?v= query param when shipping a breaking method
 * change so users don't hit a stale cached copy.
 */

(async function () {
  /* ---------- PROT ranges per kingdom (from CDNAME map) ----------- */
  const KINGDOM_PROT = {
    drake:   { start: 85,  end: 93,  label: 'Drake Kingdom',   cdname: 'map01' },
    sebucus: { start: 244, end: 253, label: 'Sebucus Islands', cdname: 'map02' },
    karisto: { start: 391, end: 398, label: 'Karisto Kingdom', cdname: 'map03' },
  };

  /* ---------- Retail camera defaults (from .mc{1,2,3}) -------------
   * Captured per-kingdom from mednafen save states in walk-view mode.
   * The X/Z scrolls live at `_DAT_80089120` / `_DAT_80089118` and are
   * stored as negated map-origin coordinates - the engine writes
   * `_DAT_80089118 = -(int)*(short *)(actor + 0x14)` etc., so the world
   * camera target is the negation of the raw cell. The captured saves
   * are all in walk-view (DAT_801F2B94 = 0), so these reflect the
   * load-time spawn rather than an interactively-scrolled dev-menu
   * camera; they're still useful as per-kingdom anchor points.
   *   mednafen-state world-map-camera --table <save> ...
   * regenerates these values. */
  const KINGDOM_CAM = {
    drake:   { centerX: 8832, centerZ: 8832, zoom: 0x170 },
    sebucus: { centerX: 7936, centerZ: 9344, zoom: 0x170 },
    karisto: { centerX: 7936, centerZ: 9088, zoom: 0x170 },
  };
  /* Top-view camera anchors per kingdom. The "lock to retail top-view"
   * button slams the world-cam to these centres + sets the zoom so the
   * frame matches the in-game world-map camera. Values mirror the
   * walk-view load-time spawn anchors decoded by `mednafen-state
   * world-map-camera --table <save>` against per-kingdom saves; the
   * dev-menu's free-camera top-view (DAT_801F2B94 != 0) starts from the
   * same anchor before user input scrolls it, so these double as
   * top-view defaults until a captured dev-menu state refines them. */
  const KINGDOM_TOPVIEW_CAM = {
    drake:   { centerX: 8832, centerZ: 8832, azimuth: 0x0000, zoom: 0x0170 },
    sebucus: { centerX: 7936, centerZ: 9344, azimuth: 0x0000, zoom: 0x0170 },
    karisto: { centerX: 7936, centerZ: 9088, azimuth: 0x0000, zoom: 0x0170 },
  };
  /* Class-conditional target footprints for the unplaced-slot1 grid.
   * Each unplaced TMD is normalised so its larger XZ extent maps to this
   * target, then centroid-anchored at its assigned grid slot. Numbers
   * are world units (Drake is 16320x16320). The renderer falls back to
   * MESH_SCALE when normalisation is disabled or the AABB collapses to
   * zero. Matches the four classes in slot1_classification.toml. */
  const UNPLACED_TARGET_FOOTPRINT = {
    landmark:   600,
    decoration: 200,
    ground_tile: 1200,
    unknown:    600,
  };
  /* Per-kingdom baseline fog tint (BGR555 -> linear RGB). Used as the
   * fragment shader's fallback when a captured LUT entry is zero, so the
   * "fog" toggle still produces a kingdom-flavoured gradient before the
   * user supplies fog_probe.lut.bin. The values are mid-tier samples of
   * each kingdom's dev-menu top-view sky colour. */
  const KINGDOM_FOG_TINT = {
    drake:   { r: 0.18, g: 0.22, b: 0.34 },
    sebucus: { r: 0.16, g: 0.26, b: 0.32 },
    karisto: { r: 0.26, g: 0.20, b: 0.22 },
  };
  /* Initial fog defaults: matches the retail probe shape (Z_far = Z >> 5
   * over a ~16384-unit reference). Overwritten when the user uploads a
   * fog_probe.lut.bin via the fog-LUT picker. */
  const DEFAULT_FOG_PARAMS = { enable: 0, zShift: 5, farRef: 12000, color: null };
  const LIST_NAMES = {
    0: 'player/system', 1: 'entities', 2: 'background',
    3: 'reserve_3', 4: 'reserve_4', 5: 'reserve_5', 6: 'extra',
  };
  const LIST_COLORS = { 0: '#56B6F5', 1: '#79D279', 2: '#C0C0C0', 6: '#E07028' };

  /* ---------- DOM refs ----------- */
  const $file       = document.getElementById('wo-disc-file');
  const $status     = document.getElementById('wo-status');
  const $canvas3d   = document.getElementById('wo-canvas3d');
  const $canvas2d   = document.getElementById('wo-canvas2d');
  const $statsDl    = document.getElementById('wo-stats-dl');
  const $listCounts = document.getElementById('wo-list-counts');
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
  const $fogEnable  = document.getElementById('wo-fog-enable');
  const $showTerrain = document.getElementById('wo-show-terrain');
  const $showLandmarks = document.getElementById('wo-show-landmarks');
  const $lockTopView = document.getElementById('wo-lock-topview');
  let   scatterDots = [];   /* { x, y, p } screen-space hit-test entries */
  let   viewMode    = 'world';  /* 'world' (assembled scene from disc) | 'mesh' (single TMD inspector) */
  let   fogLut      = null;     /* Uint16Array(512) BGR555 entries, or null */
  let   fogParams   = { ...DEFAULT_FOG_PARAMS };

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
  const cam = { yaw: 0, pitch: 0.25, distance: 2.5, panX: 0, panY: 0, autoRotate: true };
  /* Top-down camera state (world units). Reset per-kingdom to fit the world. */
  const worldCam = { centerX: 8160, centerZ: 8160, halfWidth: 9000, halfHeight: 9000, pitch: 0 };

  /* ---------- Placement JSON (always available) ----------- */
  try {
    placements = await fetch('world-overview.json').then(r => r.json());
  } catch (e) {
    placements = null;
  }
  updateSnapshot();
  drawScatter();

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
      worldCam.pitch = 0;
    } else {
      resetCam();
    }
  });
  $autoRot.addEventListener('change', () => { cam.autoRotate = $autoRot.checked; });

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
    /* Unplaced-TMD toggle, normalisation, fog, top-view lock only make
     * sense in world mode (mesh inspector already walks the whole pack
     * one mesh at a time). */
    const $unplacedWrap = document.getElementById('wo-show-unplaced-wrap');
    const $normalizeWrap = document.getElementById('wo-normalize-unplaced-wrap');
    const $fogWrap = document.getElementById('wo-fog-wrap');
    const $terrainWrap = document.getElementById('wo-show-terrain-wrap');
    const $landmarksWrap = document.getElementById('wo-show-landmarks-wrap');
    /* The unplaced-slot-1 layout grid only drives the legacy
     * overview-frame placement layer, which is hidden while the viewer
     * shows the walk-view continent terrain only - so keep its toggles
     * hidden regardless of view mode. */
    if ($unplacedWrap)  $unplacedWrap.hidden  = true;
    if ($normalizeWrap) $normalizeWrap.hidden = true;
    if ($fogWrap)       $fogWrap.hidden       = meshOnly;
    if ($terrainWrap)   $terrainWrap.hidden   = meshOnly;
    if ($landmarksWrap) $landmarksWrap.hidden = meshOnly;
    if ($lockTopView)   $lockTopView.hidden   = meshOnly;
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
  if ($fogEnable) {
    $fogEnable.addEventListener('change', () => {
      fogParams.enable = $fogEnable.checked ? 1 : 0;
      pushFogParamsToRenderer();
    });
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
      worldCam.pitch = 0;
    });
  }

  /* Push the latest (fogLut, fogParams) snapshot into the GL renderer.
   * Safe to call even when glRenderer isn't a TmdRenderer (mesh-inspector
   * mode binds the same class, so uploadFogLut always exists). */
  function pushFogParamsToRenderer() {
    if (!glRenderer || typeof glRenderer.uploadFogLut !== 'function') return;
    /* Compose the color, preferring (in order):
     *   1. explicit param override (fogParams.color from the UI picker)
     *   2. per-kingdom captured haze from world-overview.json
     *      (`fog_color` field, populated by
     *      scripts/mednafen/resolve_bulk_terrain.py when the save state
     *      has an active FUN_801E3E00 atmospheric-tick actor)
     *   3. KINGDOM_FOG_TINT hardcoded fallback (eyeballed defaults). */
    let color = fogParams.color;
    if (!color && placements?.[currentKingdom]?.fog_color) {
      const f = placements[currentKingdom].fog_color;
      color = { r: f.r, g: f.g, b: f.b };
    }
    if (!color) {
      color = KINGDOM_FOG_TINT[currentKingdom]
        || { r: 0.20, g: 0.20, b: 0.30 };
    }
    glRenderer.uploadFogLut(fogLut, {
      enable: fogParams.enable,
      zShift: fogParams.zShift,
      farRef: fogParams.farRef,
      color,
    });
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
   * has decompressed slot 0 and `legaia_web_viewer::ocean::find_ocean_assets`
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
        // ES module import paths resolve relative to the importing JS
        // file (site/js/), not the host HTML page. wasm-pack output
        // lives at site/wasm/.
        wasmMod = await import('../wasm/legaia_web_viewer.js');
        await wasmMod.default();
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
      /* Auto-extract the world-map fog LUT from SCUS. The runtime
       * adds these per-Z scalar entries to vertex SXY+offset words in
       * the overlay leaves at 0x801F7644..0x801F8690; the WebGL port
       * reuses them as the per-tier fade curve and mixes the diffuse
       * term toward the per-kingdom haze color from KINGDOM_FOG_TINT.
       * An empty return means SCUS extraction failed (raw PROT.DAT,
       * regional variant, modded disc) - the kingdom-tinted fallback
       * still works without it. */
      try {
        const raw = viewer.fog_lut_bytes();
        if (raw && raw.length >= 4096) {
          fogLut = new Uint16Array(raw.buffer, raw.byteOffset, raw.length / 2);
          if ($fogLutStatus) {
            $fogLutStatus.textContent =
              `Fog LUT: auto-extracted from SCUS_942.54 (${raw.length} B, ${fogLut.length} per-Z scalar entries).`;
          }
        } else if ($fogLutStatus) {
          $fogLutStatus.textContent =
            'Fog LUT: not located in SCUS — kingdom-tinted fallback active.';
        }
      } catch (err) {
        console.warn('fog_lut_bytes:', err);
      }
      /* Populate the whole-Legaia quick-travel section from SCUS data
       * the viewer parsed during load_disc(). Failures are silent: the
       * section just stays in its "load a disc" state. */
      try {
        const json = viewer.worldmap_menu_json();
        const menu = JSON.parse(json);
        if (menu) renderQuickTravel(menu);
      } catch (e) {
        console.warn('worldmap_menu_json:', e);
      }
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

  /* Status line for the auto-extracted fog LUT. The disc-load path
   * populates this; there is no manual upload affordance because the
   * SCUS bytes are the authoritative source. */
  const $fogLutStatus = document.getElementById('wo-fog-lut-status');

  /* ---------- Mesh navigation ----------- */
  function selectKingdom(k) {
    currentKingdom = k;
    history.replaceState(null, '', '#' + k);
    document.querySelectorAll('.wo-tab').forEach(b => b.classList.toggle('is-active', b.dataset.k === k));
    currentMeshIdx = 0;
    updateSnapshot();
    drawScatter();
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
    if (rafId !== null) { cancelAnimationFrame(rafId); rafId = null; }
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
    /* Top-view-lock + fog become available in world-view mode; mesh
     * inspector mode hides them again. */
    if ($lockTopView) $lockTopView.hidden = false;
    pushFogParamsToRenderer();
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
    /* Upload a single pack mesh on demand; return true if its mesh data
     * is renderable (some slots have non-textured flat-shaded prims that
     * the WebGL path skips). */
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
      glRenderer.uploadSceneMesh(ms, positions, uvs, cba, idx);
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

    /* Walk-frame placed landmarks: the slot-1 pack meshes FUN_8003A55C
     * stamps on the continent (flags & 0x4), resolved by the WASM
     * `build_walk_placements` into the SAME col*128 world frame as the
     * heightfield ground above - so they overlay the terrain correctly,
     * unlike the legacy overview-frame `world-overview.json` placements
     * (kept behind SHOW_LEGACY_PLACEMENTS below). Each landmark draws its
     * pack mesh at world (x, y, z) with scale 1 and the shared (1,-1,1)
     * Y-flip; world Y comes from the floor-height LUT so the mesh sits on
     * the heightfield. */
    let walkLandmarkCount = 0;
    const showLandmarks = !$showLandmarks || $showLandmarks.checked;
    if (showLandmarks) {
      const wpCount = viewer.walk_placement_count();
      if (wpCount > 0) {
        const slots = viewer.walk_placement_slots();
        const pos = viewer.walk_placement_positions();
        for (let i = 0; i < wpCount; i++) {
          const ms = slots[i];
          if (!ensureMeshUploaded(ms)) continue;
          drawPlacements.push({
            meshId: ms,
            x: pos[i * 3],
            y: pos[i * 3 + 1],
            z: pos[i * 3 + 2],
            scale: 1.0,
            kind: 'walk_landmark',
            class: 'landmark',
          });
          walkLandmarkCount++;
        }
      }
    }
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
    worldCam.pitch = 0;
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
        ? ` + ${walkLandmarkCount} placed landmarks`
        : '';
      $meshLabel.textContent = groundQuads > 0
        ? `${k.cdname} - ${groundQuads}-cell continent heightfield (walk-view terrain)${landmarkSummary}`
        : (walkLandmarkCount > 0
            ? `${k.cdname} - ${walkLandmarkCount} placed landmarks (no terrain resolved)`
            : `${k.cdname} - no walk-view terrain resolved`);
    }
    $meshInfo.textContent =
      `top-down view; world ${ext[0]} x ${ext[1]} units; scroll to zoom, drag to pan`;
    attachTopDownControls(fresh);
    const tick = () => {
      glRenderer.renderAssembled(drawPlacements, ext, worldCam);
      rafId = requestAnimationFrame(tick);
    };
    rafId = requestAnimationFrame(tick);
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
    const retail = KINGDOM_CAM[currentKingdom];
    /* When the continent ground heightfield is present it's the dominant
     * geometry, so frame the whole continent (centred on its XZ centroid,
     * sized to cover its extent + a margin) rather than the sparse
     * landmark cluster. The "lock to retail top-view" button still gives
     * the retail spawn framing on demand. */
    if (groundAabb && groundAabb.sx > 0 && groundAabb.sz > 0) {
      worldCam.centerX = groundAabb.cx;
      worldCam.centerZ = groundAabb.cz;
      worldCam.halfWidth  = groundAabb.sx / 2 + 1000;
      worldCam.halfHeight = groundAabb.sz / 2 + 1000;
      return;
    }
    if (drawPlacements && drawPlacements.length > 0) {
      let xmin = Infinity, xmax = -Infinity, zmin = Infinity, zmax = -Infinity;
      for (const p of drawPlacements) {
        if (p.x < xmin) xmin = p.x;
        if (p.x > xmax) xmax = p.x;
        if (p.z < zmin) zmin = p.z;
        if (p.z > zmax) zmax = p.z;
      }
      if (retail) {
        worldCam.centerX = retail.centerX;
        worldCam.centerZ = retail.centerZ;
        const dx = Math.max(retail.centerX - xmin, xmax - retail.centerX);
        const dz = Math.max(retail.centerZ - zmin, zmax - retail.centerZ);
        worldCam.halfWidth  = Math.max(dx, 1500) + 1500;
        worldCam.halfHeight = Math.max(dz, 1500) + 1500;
      } else {
        worldCam.centerX = (xmin + xmax) / 2;
        worldCam.centerZ = (zmin + zmax) / 2;
        worldCam.halfWidth  = Math.max((xmax - xmin) / 2, 1500) + 1500;
        worldCam.halfHeight = Math.max((zmax - zmin) / 2, 1500) + 1500;
      }
    } else if (retail) {
      worldCam.centerX = retail.centerX;
      worldCam.centerZ = retail.centerZ;
      worldCam.halfWidth = ext[0] / 2 + 1000;
      worldCam.halfHeight = ext[1] / 2 + 1000;
    } else {
      worldCam.centerX = ext[0] / 2;
      worldCam.centerZ = ext[1] / 2;
      worldCam.halfWidth = ext[0] / 2 + 1000;
      worldCam.halfHeight = ext[1] / 2 + 1000;
    }
  }

  /* Pan / zoom controls for the top-down camera. Replaces the orbit
   * controls in `attachCameraControls` for world-view mode. */
  function attachTopDownControls(canvas) {
    let dragging = false, lastX = 0, lastY = 0;
    canvas.addEventListener('mousedown', e => {
      dragging = true; lastX = e.clientX; lastY = e.clientY;
    });
    window.addEventListener('mouseup', () => { dragging = false; });
    window.addEventListener('mousemove', e => {
      if (!dragging) return;
      const dx = e.clientX - lastX, dy = e.clientY - lastY;
      lastX = e.clientX; lastY = e.clientY;
      /* Drag -> camera grabs the map (content follows the cursor).
       * Convert pixel deltas to world units via the current camera
       * half-extents, through the basis buildTopDownVp uses (180 deg
       * rotation + horizontal mirror): screen-right = world +X,
       * screen-down = world -Z. */
      const sx = (worldCam.halfWidth  * 2) / canvas.width;
      const sy = (worldCam.halfHeight * 2) / canvas.height;
      worldCam.centerX -= dx * sx;
      worldCam.centerZ += dy * sy;
    });
    canvas.addEventListener('wheel', e => {
      e.preventDefault();
      const f = Math.exp(e.deltaY * 0.001);
      worldCam.halfWidth  = Math.max(200, Math.min(20000, worldCam.halfWidth  * f));
      worldCam.halfHeight = Math.max(200, Math.min(20000, worldCam.halfHeight * f));
    }, { passive: false });
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
    glRenderer.uploadMesh(positions, uvs, cbaTsb, indices);
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

  /* ---------- Snapshot stats + scatter ----------- */

  function updateSnapshot() {
    if (!placements || !placements[currentKingdom]) {
      $statsDl.innerHTML = '<dt>data</dt><dd>placement JSON missing</dd>';
      $listCounts.innerHTML = '';
      if ($nameList) $nameList.innerHTML = '';
      return;
    }
    const k = placements[currentKingdom];
    const placed = k.world_placed_count ?? k.placements.filter(p => !p.script_positioned).length;
    const scripted = k.script_positioned_count ?? (k.placements.length - placed);
    const packCount = k.tmd_pack ? k.tmd_pack.count : (k.tmd_count ?? '?');
    const globalRefs = k.placements.filter(p => p.tmd_slot >= 0xF0).length;
    $statsDl.innerHTML = `
      <dt>cdname</dt>          <dd>${k.cdname || k.label}</dd>
      <dt>PROT base</dt>       <dd>${k.prot_base ?? '?'}</dd>
      <dt>records</dt>         <dd>${k.placements.length} total</dd>
      <dt>world-placed</dt>    <dd>${placed}</dd>
      <dt>script-positioned</dt><dd>${scripted}</dd>
      <dt>TMD pack</dt>        <dd>${packCount} meshes (slot 1, type 0x02)</dd>
      <dt>global TMD refs</dt> <dd>${globalRefs} placement(s) use the global pool (slot &ge; 0xF0) - rendered as placeholders pending disc-side global mesh bundle</dd>
    `;
    const counts = {};
    k.placements.forEach(p => { if (!p.script_positioned) counts[p.list] = (counts[p.list] || 0) + 1; });
    $listCounts.innerHTML = Object.entries(counts).sort((a,b)=>+a[0]-+b[0]).map(([li, n]) =>
      `<li><span class="legend-swatch swatch-list-${li}"></span>list ${li} (${LIST_NAMES[li]||'?'}): ${n}</li>`
    ).join('');
    if ($nameList) {
      const named = k.placements
        .filter(p => !p.script_positioned && p.name && p.name.trim())
        .slice(0, 60);
      $nameList.innerHTML = named.map(p =>
        `<li><span class="legend-swatch swatch-list-${p.list}"></span><span class="wo-name">${escapeHtml(p.name)}</span> <span class="wo-coord">(${p.pos[0]}, ${p.pos[2]})</span></li>`
      ).join('');
    }
  }
  function escapeHtml(s) {
    return String(s).replace(/[&<>"']/g, c => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));
  }
  function drawScatter() {
    if (!$canvas2d) return;
    const ctx = $canvas2d.getContext('2d');
    const W = $canvas2d.width, H = $canvas2d.height;
    ctx.fillStyle = '#0a0a1a'; ctx.fillRect(0, 0, W, H);
    scatterDots = [];
    if (!placements || !placements[currentKingdom]) return;
    const k = placements[currentKingdom];
    const [wx, wz] = k.world_extent || [16320, 16320];
    /* grid */
    ctx.strokeStyle = 'rgba(120,130,200,0.15)';
    for (let i = 0; i <= 8; i++) {
      const t = i/8;
      ctx.beginPath();
      ctx.moveTo(t*W, 0); ctx.lineTo(t*W, H);
      ctx.moveTo(0, t*H); ctx.lineTo(W, t*H);
      ctx.stroke();
    }
    /* placements */
    k.placements.forEach(p => {
      if (p.script_positioned) return;
      const x = p.pos[0] / wx * W;
      const y = p.pos[2] / wz * H;
      const color = LIST_COLORS[p.list] || '#FFF';
      ctx.beginPath();
      ctx.arc(x, y, 3, 0, 2*Math.PI);
      ctx.fillStyle = color; ctx.fill();
      scatterDots.push({ x, y, p });
    });
  }

  /* ---------- Quick-travel menu overlay (whole Legaia) ---------- */
  /* Visual style guide for the 20-record overlay - subtle but readable. */
  const QT_DOT_COLOR = '#F4B41A';
  const QT_DOT_RADIUS = 5;
  let qtRecords = null;  /* { names: string[], placements: [{name_idx, ...}] } */
  let qtScreen = [];     /* [{ x, y, p, name }] hit-test entries in canvas coords */

  function renderQuickTravel(menu) {
    qtRecords = menu;
    const $qtCanvas   = document.getElementById('wo-qt-canvas');
    const $qtNameList = document.getElementById('wo-qt-namelist');
    if (!$qtCanvas || !$qtNameList) return;
    /* Sidebar: 16 names with placement counts (sub-entries per landmark). */
    const counts = new Array(menu.names.length).fill(0);
    menu.placements.forEach(p => { counts[p.name_idx]++; });
    $qtNameList.innerHTML = menu.names.map((name, i) => {
      const c = counts[i];
      const tag = c === 0
        ? '<span style="color:#888">(no record)</span>'
        : `<span class="wo-coord">x${c}</span>`;
      return `<li><span class="wo-name">[0x${i.toString(16).padStart(2,'0').toUpperCase()}] ${escapeHtml(name)}</span> ${tag}</li>`;
    }).join('');
    /* Canvas: scale menu_x/menu_y (PSX menu screen space, ~128x128) to
     * canvas pixels. menu_x range observed 22..102, menu_y 13..108. */
    drawQuickTravelCanvas();
  }
  function drawQuickTravelCanvas() {
    const $qtCanvas = document.getElementById('wo-qt-canvas');
    if (!$qtCanvas || !qtRecords) return;
    const ctx = $qtCanvas.getContext('2d');
    const W = $qtCanvas.width, H = $qtCanvas.height;
    ctx.fillStyle = '#0a0a1a'; ctx.fillRect(0, 0, W, H);
    /* Margin + scale from a 0..128 menu-coord box to canvas. */
    const margin = 30;
    const aW = W - margin * 2;
    const aH = H - margin * 2;
    const sx = aW / 128;
    const sy = aH / 128;
    /* Grid */
    ctx.strokeStyle = 'rgba(120,130,200,0.12)';
    for (let i = 0; i <= 8; i++) {
      ctx.beginPath();
      ctx.moveTo(margin + (i/8)*aW, margin);
      ctx.lineTo(margin + (i/8)*aW, margin + aH);
      ctx.moveTo(margin, margin + (i/8)*aH);
      ctx.lineTo(margin + aW, margin + (i/8)*aH);
      ctx.stroke();
    }
    qtScreen = [];
    /* Draw dots first so labels render on top. */
    qtRecords.placements.forEach(p => {
      const x = margin + p.menu_x * sx;
      const y = margin + p.menu_y * sy;
      ctx.beginPath();
      ctx.arc(x, y, QT_DOT_RADIUS, 0, 2*Math.PI);
      ctx.fillStyle = QT_DOT_COLOR; ctx.fill();
      ctx.strokeStyle = 'rgba(0,0,0,0.5)'; ctx.lineWidth = 1; ctx.stroke();
      qtScreen.push({ x, y, p, name: qtRecords.names[p.name_idx] || '?' });
    });
    /* Label one entry per landmark (the first record we see for each
     * name_idx). De-dupes the closely-grouped sub-entry triplets. */
    ctx.font = '11px monospace';
    ctx.fillStyle = '#ddd';
    ctx.textBaseline = 'middle';
    const labeled = new Set();
    qtScreen.forEach(d => {
      if (labeled.has(d.p.name_idx)) return;
      labeled.add(d.p.name_idx);
      const label = d.name;
      const tx = d.x + QT_DOT_RADIUS + 4;
      const tw = ctx.measureText(label).width;
      /* Background pill so labels stay readable over the grid. */
      ctx.fillStyle = 'rgba(10,10,26,0.78)';
      ctx.fillRect(tx - 2, d.y - 7, tw + 4, 14);
      ctx.fillStyle = '#ddd';
      ctx.fillText(label, tx, d.y);
    });
  }
  /* Hover tooltip over the quick-travel canvas. */
  const $qtCanvasEl = document.getElementById('wo-qt-canvas');
  const $qtHover    = document.getElementById('wo-qt-hover');
  if ($qtCanvasEl && $qtHover) {
    $qtCanvasEl.addEventListener('mousemove', ev => {
      const rect = $qtCanvasEl.getBoundingClientRect();
      const mx = (ev.clientX - rect.left) * ($qtCanvasEl.width / rect.width);
      const my = (ev.clientY - rect.top) * ($qtCanvasEl.height / rect.height);
      let best = null, bestD = Infinity;
      for (const d of qtScreen) {
        const dx = d.x - mx, dy = d.y - my;
        const r2 = dx*dx + dy*dy;
        if (r2 < bestD) { bestD = r2; best = d; }
      }
      if (best && bestD < 96) {
        const p = best.p;
        $qtHover.textContent =
          `${best.name}  -  rec ${p.index}  menu (${p.menu_x}, ${p.menu_y})  ` +
          `scene_id=0x${p.scene_id.toString(16).padStart(4,'0').toUpperCase()}  ` +
          `flag=0x${p.discovery_flag.toString(16).padStart(2,'0').toUpperCase()}`;
      } else {
        $qtHover.innerHTML = '&nbsp;';
      }
    });
    $qtCanvasEl.addEventListener('mouseleave', () => { $qtHover.innerHTML = '&nbsp;'; });
  }
  /* Initial placeholder. */
  (function paintQtPlaceholder() {
    const $c = document.getElementById('wo-qt-canvas');
    if (!$c) return;
    const ctx = $c.getContext('2d');
    ctx.fillStyle = '#0a0a1a'; ctx.fillRect(0, 0, $c.width, $c.height);
    ctx.fillStyle = '#888'; ctx.font = '13px monospace';
    ctx.textAlign = 'center';
    ctx.fillText('Load a disc above to see the in-game world-map menu', $c.width / 2, $c.height / 2 - 10);
    ctx.fillText('(parsed live from SCUS_942.54)', $c.width / 2, $c.height / 2 + 10);
  })();

  /* Hover tooltip over the scatter */
  if ($canvas2d) {
    $canvas2d.addEventListener('mousemove', ev => {
      const rect = $canvas2d.getBoundingClientRect();
      const mx = (ev.clientX - rect.left) * ($canvas2d.width / rect.width);
      const my = (ev.clientY - rect.top) * ($canvas2d.height / rect.height);
      let best = null, bestD = Infinity;
      for (const d of scatterDots) {
        const dx = d.x - mx, dy = d.y - my;
        const r2 = dx*dx + dy*dy;
        if (r2 < bestD) { bestD = r2; best = d; }
      }
      if (best && bestD < 64) {
        const p = best.p;
        let src = '';
        if (p.tmd_source) {
          if (p.tmd_source.kind === 'scene_tmd_pack') {
            src = `  pack[${p.tmd_source.pack_slot}] nobj=${p.tmd_source.nobj} (${p.tmd_source.body_bytes}B)`;
          } else if (p.tmd_source.kind === 'global_pool') {
            src = `  global[${p.tmd_source.global_index}]`;
          }
        }
        const slot = `0x${p.tmd_slot.toString(16).padStart(2,'0').toUpperCase()}`;
        const cls = p.class ? `  class=${p.class}` : '';
        const note = p.class_note ? `  ${p.class_note}` : '';
        $hover.textContent = `${p.name || '(unnamed)'}  -  (${p.pos[0]}, ${p.pos[2]})  tmd_slot=${slot}${src}${cls}${note}`;
      } else {
        $hover.innerHTML = '&nbsp;';
      }
    });
    $canvas2d.addEventListener('mouseleave', () => { $hover.innerHTML = '&nbsp;'; });
  }
})();
