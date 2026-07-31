/* Play the port in the browser.
 *
 * The WASM side (`LegaiaRuntime`) is the *engine*: a real `SceneHost` running
 * the field VM, the free-movement controller against the scene's walkability
 * grid, floor-height sampling, NPC motion, the interaction probe, the dialogue
 * runner. This module is the *shell*: keyboard -> PSX pad word, one engine tick
 * per animation frame, and a draw of whatever the engine reports.
 *
 * Rendering reuses the shared `TmdRenderer` scene path (the same plumbing the
 * world-overview continents and the viewer's full-map button run on):
 *
 *   - static map   - terrain tiles + placed objects + the ground heightfield,
 *                    uploaded once per scene;
 *   - player       - one scene mesh whose positions are re-uploaded each frame
 *                    from the engine's live pose (idle / walk locomotion clip);
 *   - NPCs         - one scene mesh each, posed from the scene's ANM bundle and
 *                    drawn at the world's live NPC position / heading.
 *
 * Requires webgl-math.js + webgl-shaders.js + webgl-tmd.js + field-scene-view.js
 * (for the shared sky-mesh classifier) to be loaded first.
 */
(function () {
  'use strict';

  const A2R = Math.PI * 2 / 4096;     /* PSX 12-bit angle -> radians */
  const PLAYER_MESH_ID = 900000;      /* scene-mesh id space above any env slot */
  const NPC_MESH_BASE  = 910000;
  const TILE_MESH_BASE = 920000;   /* one mesh per board-owned actor slot */
  /* Battle 3D layer: 930000 backdrop, +1 ground grid, +2 the effect-pool
   * billboard batch, +16+i actor meshes, +256+tmd the 3D FX model cache
   * (keyed by the engine's global-TMD-pool index, so one upload per distinct
   * effect mesh per fight). */
  const BATTLE_MESH_BASE = 930000;
  const BATTLE_FX_BILLBOARD_MESH = BATTLE_MESH_BASE + 2;
  const BATTLE_FX_MODEL_BASE = BATTLE_MESH_BASE + 256;
  /* Identity model matrix for draws whose transform the engine already folded
   * into the vertex stream (the FX billboard batch). */
  const IDENTITY_MODEL = new Float32Array([
    1, 0, 0, 0,
    0, 1, 0, 0,
    0, 0, 1, 0,
    0, 0, 0, 1,
  ]);
  /* Wall-clock NPC anim fallback rate, used ONLY against a cached WASM
   * without the engine clip-state API. The live path reads each clip's
   * current frame from the engine, whose playhead advances in sim-tick time
   * (60 Hz ticks, 2 ticks per clip frame = 30 clip fps, the retail cadence). */
  const NPC_CLIP_FPS   = 15;
  /* VR world scale - same anchor as the full-map view: the ~130-unit character
   * mesh is a 1.7 m human, so a metre is ~76 world units and the headset stands
   * in the town at human height. See docs/subsystems/vr-mode.md. */
  const VR_UNITS_PER_METER = 76;
  /* First-person mode derives its scale from the LIVE player mesh instead of
   * the 130-unit rule of thumb: Vahn is an adult-human ~1.7 m, so
   * units/metre = measured mesh height / 1.7. Eyes sit a little below the top
   * of the head (~94% of standing height) - that is what the `local`
   * reference-space fallback lifts the rig by; a `local-floor` device takes
   * the real headset height instead. */
  const VAHN_HEIGHT_M = 1.7;
  const VR_FP_EYE_HEIGHT_M = 1.6;
  const VR_FP_FALLBACK_MESH_HEIGHT = 130;

  /* Keyboard -> PSX pad bits: the shared table in `js/pad-bindings.js`, read
   * out of the engine (`Mapping::web_default` through `pad_bindings_json`).
   * Not a table in this file, and not a table in any page file - see that
   * module's header for what happened the last time it was. */
  const adoptPadBindings = window.legaiaAdoptPadBindings;
  const padMaskOf = window.legaiaPadMaskOf;
  const padTable = () => window.legaiaPadTable();


  /* The retail menu's own clock. Its timers (the save screen's "Now checking"
   * beat, every slide-in) are counted in 60 Hz frames, so the menu ticks on
   * this fixed step rather than once per animation frame - the page's own
   * frame rate is well below 60 on a heavy scene. */
  const MENU_TICK_MS = 1000 / 60;
  /* Most catch-up ticks one frame may replay (a backgrounded tab can hand us
   * an arbitrarily large gap). */
  const MENU_TICK_MAX_CATCHUP = 8;

  /* Blits sub-rects of a source atlas (font glyphs or the menu-chrome sheet)
   * with a per-quad RGBA multiply tint, over a 2D canvas. The retail pause menu
   * emits its geometry as `{ dst, src, color }` quads (the shipped
   * `legaia-engine-ui` draw builders); this is the browser twin of the native
   * window's textured-quad overlay pass.
   *
   * The tint is a colour MULTIPLY that preserves the source alpha - identity for
   * white (`(1,1,1)`), so plain chrome blits straight through, while the
   * whitewashed font atlas takes the ink colour and the navy filigree takes its
   * darkening tint. Multiplied full-atlas copies are cached per colour, so the
   * per-quad cost is a single `drawImage`. */
  class AtlasBlitter {
    constructor(rgba, w, h) {
      this.w = w; this.h = h;
      this.base = document.createElement('canvas');
      this.base.width = w; this.base.height = h;
      const bctx = this.base.getContext('2d');
      bctx.putImageData(new ImageData(new Uint8ClampedArray(rgba), w, h), 0, 0);
      this.cache = new Map();
    }
    _tinted(col) {
      const r = Math.round(col[0] * 255), g = Math.round(col[1] * 255), b = Math.round(col[2] * 255);
      if (r === 255 && g === 255 && b === 255) return this.base;   /* identity multiply */
      const key = (r << 16) | (g << 8) | b;
      let cv = this.cache.get(key);
      if (cv) return cv;
      cv = document.createElement('canvas');
      cv.width = this.w; cv.height = this.h;
      const cx = cv.getContext('2d');
      cx.drawImage(this.base, 0, 0);
      cx.globalCompositeOperation = 'multiply';
      cx.fillStyle = 'rgb(' + r + ',' + g + ',' + b + ')';
      cx.fillRect(0, 0, this.w, this.h);
      /* Restore the source alpha the flat fill just clobbered on the transparent
       * texels, so only the glyph / sprite footprint remains. */
      cx.globalCompositeOperation = 'destination-in';
      cx.drawImage(this.base, 0, 0);
      cx.globalCompositeOperation = 'source-over';
      this.cache.set(key, cv);
      return cv;
    }
    blit(ctx, draws) {
      if (!draws) return;
      for (const d of draws) {
        const s = d.src, dst = d.dst, col = d.color;
        if (!s || !dst || s[2] <= 0 || s[3] <= 0 || dst[2] <= 0 || dst[3] <= 0) continue;
        ctx.globalAlpha = col ? col[3] : 1;
        ctx.drawImage(this._tinted(col || [1, 1, 1, 1]),
          s[0], s[1], s[2], s[3], dst[0], dst[1], dst[2], dst[3]);
      }
      ctx.globalAlpha = 1;
    }
  }

  /* The engine takes the camera's azimuth and quantises it to a quarter turn to
   * remap the d-pad ("up" walks away from the camera, "right" walks screen-right).
   *
   * The sense is **opposite** to this camera's `yaw`. Working the shared orbit
   * projection's basis out: the eye sits at
   * `target + d(sinP.sinY, cosP, -sinP.cosY)`, so the camera's right axis is
   * `(-cosY, 0, -sinY)` - and the projection then mirrors screen X (the retail
   * horizontal flip), which lands screen-right on world `(cosY, 0, sinY)` and
   * screen-up on `(-sinY, 0, cosY)`. The engine's quadrant table
   * (`decode_field_direction`) rotates the other way, so the azimuth it wants is
   * `-yaw`. Feed it `+yaw` and the controls come out correct at yaw 0 and
   * inverted at a quarter turn - which is exactly the bug this negation fixes. */
  function azimuthUnits(yaw) {
    const u = Math.round(-yaw / (Math.PI * 2) * 4096) % 4096;
    return (u + 4096) % 4096;
  }

  /* ---------- occluder cull (see the note in `_frame`) ---------- */

  /* Occluder cull, DISABLED. The native renderer draws the whole scene every
   * frame with no distance / frustum / occlusion culling (docs/subsystems/
   * renderer.md), and the browser matches it. The per-frame lens->player slab
   * test below bounded each body by an axis-aligned box over whole terrain
   * tiles / walls / buildings, so neighbouring bodies blinked out as the camera
   * orbited or the player walked (the reported "meshes cull while walking"
   * symptom). Leave this `false`; every body a scene loads is drawn every
   * frame, unconditionally. */
  const OCCLUDER_CULL = false;

  /* World AABB of one placement: the mesh's local box carried through the draw
   * model `T(x, y, z) . Ry(rotY) . diag(sc, -sc, sc)` (`placementModelScaledY`).
   * Baked once per placement so the per-frame occluder test is a slab
   * intersection against the body's REAL box - not a bounding sphere of its
   * longest axis, which for a floor slab or a staircase is hundreds of units of
   * empty space and is what used to blink them out. */
  function placementWorldBox(aabb, d) {
    if (!aabb) return null;
    const sc = (d.scale != null) ? d.scale : 1.0;
    const c = Math.cos(d.rotY || 0), s = Math.sin(d.rotY || 0);
    const ax = Math.abs(c), az = Math.abs(s);
    const cx = d.x + sc * (c * aabb.cx + s * aabb.cz);
    const cy = d.y - sc * aabb.cy;               /* the model flips Y */
    const cz = d.z + sc * (-s * aabb.cx + c * aabb.cz);
    const hx = sc * (ax * aabb.sx + az * aabb.sz) * 0.5;
    const hy = sc * aabb.sy * 0.5;
    const hz = sc * (az * aabb.sx + ax * aabb.sz) * 0.5;
    return [cx - hx, cy - hy, cz - hz, cx + hx, cy + hy, cz + hz];
  }

  /* Does the segment `p + t*e`, `t in (OCC_T_MIN, OCC_T_MAX)`, pierce the world
   * box? Standard slab test. `t = 0` is the PLAYER and `t = 1` is the LENS, so
   * the trims are asymmetric: the first `OCC_T_MIN` of the segment is skipped so
   * the body the player stands on / walks past is never an occluder, while the
   * lens end runs all the way to `1` - a body the camera is *inside* (a cliff
   * face, a cave roof) is exactly what has to go. */
  const OCC_T_MIN = 0.12, OCC_T_MAX = 1.0;
  function segmentHitsBox(px, py, pz, ex, ey, ez, box) {
    if (!box) return false;
    let t0 = OCC_T_MIN, t1 = OCC_T_MAX;
    const p = [px, py, pz], e = [ex, ey, ez];
    for (let i = 0; i < 3; i++) {
      const lo = box[i], hi = box[i + 3];
      if (Math.abs(e[i]) < 1e-6) {
        if (p[i] < lo || p[i] > hi) return false;   /* parallel and outside */
        continue;
      }
      let ta = (lo - p[i]) / e[i], tb = (hi - p[i]) / e[i];
      if (ta > tb) { const s = ta; ta = tb; tb = s; }
      if (ta > t0) t0 = ta;
      if (tb < t1) t1 = tb;
      if (t0 > t1) return false;
    }
    return true;
  }

  /* Pose an object-local mesh into `out` from one frame of a clip: per bone,
   * `Rz . Ry . Rx . v + T`. Identical to the WASM-side player pose (and the
   * monster / character pages' animators) - a character TMD's vertices are
   * relative to their own joint, so without this the parts pile on the origin. */
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

  class PlayView {
    /* `runtime` is the WASM LegaiaRuntime (disc already loaded); `canvas` an
     * unused <canvas> in the DOM. `opts.onState` fires once per frame with the
     * engine's state JSON (already parsed) for the HUD. */
    constructor(runtime, canvas, opts) {
      if (typeof window.TmdRenderer === 'undefined') {
        throw new Error('TmdRenderer global missing (webgl-tmd.js not loaded?)');
      }
      this.rt = runtime;
      /* One binding table, read from the engine rather than typed here. */
      adoptPadBindings(runtime);
      this.canvas = canvas;
      this.renderer = new window.TmdRenderer(canvas);
      this.opts = opts || {};
      this.raf = 0;
      this.paused = false;
      this.stepOnce = false;
      this.pad = 0;
      this.held = new Set();
      /* Keys that went down since the last engine tick. A tap shorter than one
       * frame (the browser delivers keydown+keyup between two animation frames)
       * would otherwise be sampled as "never pressed" - and the engine's
       * just-pressed edge, which is what talking to an NPC rides on, would never
       * fire. Latching the edge here means a tap always lands on exactly one
       * tick, however fast it was. */
      this.pulse = new Set();
      this.scene = null;
      this.staticDraws = [];
      this.player = null;    /* { basePositions } */
      this.npcs = [];        /* [{ meshId, base, objectIds, frames, partCount, frameCount, out }] */
      this.tileMeshSlots = [];
      this.tileActorSlots = new Set();
      /* Animated environment props (a placement whose object bind names a
       * clip: the Rim Elm windmill, swinging house doors). Each gets its own
       * mesh instance so its clip advances independently; re-posed per frame
       * from the engine's live prop-bank cursor. See `_frame`. */
      this.animProps = [];   /* [{ meshId, i, slot, anim, lastFrame }] */
      /* Battle 3D scene, `{ gen, scale, backdrop, ground, actors }` while the
       * engine is in a battle whose render state built (see `_battleFrame`);
       * null otherwise. `_camBeforeBattle` restores the field framing after. */
      this._battle = null;
      this._camBeforeBattle = null;
      /* Retail pause menu (Start): the state + navigation live in the engine
       * (`LegaiaRuntime::play_menu_*`), which serves the byte-pinned window
       * chrome + font glyphs as `{ dst, src, color }` quads. This page owns only
       * the two atlas textures + a 2D overlay canvas the quads blit onto; the
       * field freezes while the menu is up (retail's `SceneMode::Menu`). */
      this.menuOverlay = (opts && opts.menuOverlay) || null;   /* <canvas> over the GL view */
      this._menuCtx = null;
      this._menuFont = null;      /* AtlasBlitter for the dialog-font atlas */
      this._menuChrome = undefined;  /* AtlasBlitter | null (null once resolved to "no chrome") */
      this._overlayActive = false;
      /* Whether this session CAN draw the retail reading box on the overlay
       * canvas: the engine exports the draw builder and the font atlas is
       * uploaded. The page's DOM `.play-dialog` fallback keys off this.
       *
       * A *capability*, deliberately, not a per-frame "did we draw it". The
       * flag used to be the latter, and it produced a visible defect: the HUD
       * DOM update runs earlier in `_frame` than `_drawOverlay`, so on the
       * frame a box opened the page read last frame's value (false), unhid the
       * DOM box - which CSS anchors at `bottom: 5%` - and only hid it again a
       * frame later, once the canvas had drawn the real box at the TOP
       * (`dialog_reading_box_layout`: `(0x26, 0x10, ...)`, FUN_801D84D0). One
       * frame of an empty-looking window at the bottom, on every box open and
       * every page break. A capability cannot race a draw: it settles once,
       * before the first conversation, and never flickers. */
      this.dialogCanvasCapable = false;
      /* Last engine state handed to the HUD - lets the menu gate on Field
       * mode / no-dialog before opening. */
      this._hudState = null;
      /* Follow camera. `halfWidth` is the ortho-equivalent half-window the shared
       * orbit projection consumes (smaller = closer); 520 frames a ~130-unit
       * character at roughly the on-screen height retail's follow camera gives
       * them. */
      this.cam = {
        centerX: 0, centerY: 0, centerZ: 0,
        halfWidth: 520, halfHeight: 520,
        yaw: 0, pitch: 0.62,
      };
      this.fps = 0;
      this._fpsAccum = 0;
      this._fpsFrames = 0;
      this._fpsLast = performance.now();
      /* Fixed-timestep sim clock. The engine's field / motion VMs are authored
       * for a fixed 60 Hz tick (retail's vsync-locked field loop; the native
       * window drives it off a wall-clock accumulator, `Window::drain_ticks`,
       * `TICK_DT = 1/60`). requestAnimationFrame fires at the DISPLAY refresh -
       * 120/144 Hz on a high-refresh monitor - so ticking once per rAF ran the
       * whole world (NPC walkers included) at 2-2.4x speed, which is the "NPCs
       * zip around" symptom. Accumulate real elapsed time and run only as many
       * 1/60 s ticks as have actually elapsed (capped so a stall can't spiral). */
      this._simAccum = 0;
      this._simLast = performance.now();
      /* Last frame's draw list + world extent, kept on the instance so the VR
       * loop can re-issue the same draw once per eye without re-ticking the
       * engine. */
      this._draws = [];
      this._ext = [16384, 16384];
      this._attachInput();

      /* Measured standing height of the player mesh (world units), refreshed
       * per scene in `_rebuild`; drives the first-person world scale. */
      this.playerHeight = VR_FP_FALLBACK_MESH_HEIGHT;
      /* While the VR first-person mode is live: `_vrFp` filters the player
       * mesh out of the eye draws (you are inside it) and disables the
       * occluder cull (there is no third-person lens to occlude); `_vrDrive`
       * carries this frame's VR-stick pad word + gaze azimuth into the engine
       * tick. */
      this._vrFp = false;
      this._vrDrive = null;
      this._vrPrecise = false;

      /* VR: walk the live scene in a headset. The engine keeps ticking (the XR
       * frame loop drives it), so NPCs move and the keyboard still steers the
       * character. Two viewing modes (toggle button next to Enter VR):
       *   - Spectator: the headset is a free-flying camera in the running
       *     world, spawned where the follow camera sits.
       *   - First-person: the rig is anchored at the player's position at eye
       *     height ("what Vahn sees"); the left stick drives the REAL player
       *     through the engine's collision / walkability grid.
       * The button is always visible; without an immersive-vr device it reads
       * "VR unavailable" and click / hover explain why. */
      this.vr = window.LegaiaVr ? window.LegaiaVr.attach({
        mount: (this.opts.vrMount || document.querySelector('.play-btn-row')
          || canvas.parentElement),
        unitsPerMeter: VR_UNITS_PER_METER,
        renderer: () => this.renderer,
        cam: () => this.cam,
        extent: () => this._ext,
        /* The follow camera owns cam.center* every frame - don't fight it. */
        syncCamCenter: false,
        update: () => this._frame(true),
        draw: () => {
          const draws = this._vrFp
            ? this._draws.filter(d => d.meshId !== PLAYER_MESH_ID)
            : this._draws;
          this.renderer.renderAssembled(draws, this._ext, this.cam);
        },
        modes: [
          /* Spawn where the third-person camera sits (behind the character,
           * looking at them), feet on the character's floor. */
          {
            id: 'spectator', label: 'Spectator',
            unitsPerMeter: VR_UNITS_PER_METER,
            start: () => {
              const eye = this._eye();
              const pt = this.rt.player_transform();
              return {
                x: eye[0], y: -pt[1], z: eye[2],
                yaw: Math.PI - this.cam.yaw,
              };
            },
          },
          /* First-person: floor origin pinned to the player's feet, world
           * scaled so the measured mesh height reads as a 1.7 m adult. The
           * spawn faces the player's current heading (engine heading 0 =
           * travelling +Z = world dir (sin, cos); the rig's yaw 0 faces -Z
           * through the mirrored world transform, hence the half-turn). */
          {
            id: 'first-person', label: 'First-person',
            unitsPerMeter: () => this._measurePlayerHeight() / VAHN_HEIGHT_M,
            eyeHeightHint: VR_FP_EYE_HEIGHT_M,
            start: () => {
              const pt = this.rt.player_transform();
              return {
                x: pt[0], y: -pt[1], z: pt[2],
                yaw: Math.PI + pt[3] * A2R,
              };
            },
            anchor: () => {
              const pt = this.rt.player_transform();
              return { x: pt[0], y: -pt[1], z: pt[2] };
            },
            drive: (d) => this._vrDriveInput(d),
          },
        ],
        onMode: (id) => this._setVrMode(id),
        onEnter: () => this.stop(),
        onExit: () => { this._setVrMode(null); this.start(); },
      }) : null;
    }

    /* Standing height of the LIVE posed player mesh (world Y extent), the
     * first-person scale anchor. Measured lazily at VR placement time - at
     * `_rebuild` the just-uploaded geometry is still the unposed object-local
     * vertex pile (parts relative to their own joints, ~half the standing
     * height); once the engine has posed a frame the extent is the real
     * standing figure (~130 units). Cached on the instance for the harness. */
    _measurePlayerHeight() {
      let h = 0;
      try {
        const pos = this.rt.player_mesh_positions();
        let yMin = Infinity, yMax = -Infinity;
        for (let i = 1; i < pos.length; i += 3) {
          if (pos[i] < yMin) yMin = pos[i];
          if (pos[i] > yMax) yMax = pos[i];
        }
        h = yMax - yMin;
      } catch (e) { /* fall through to the anchor constant */ }
      /* Guard well above the ~64-unit unposed pile: a not-yet-posed mesh (or
       * a failed accessor) falls back to the 130-unit standing anchor. */
      this.playerHeight = (Number.isFinite(h) && h > 90)
        ? h : VR_FP_FALLBACK_MESH_HEIGHT;
      return this.playerHeight;
    }

    /* The VR mode toggled (or the session ended, id = null). Arm / disarm the
     * first-person state and hand the engine's input path back to the
     * keyboard when leaving first-person. */
    _setVrMode(id) {
      this._vrFp = (id === 'first-person');
      if (!this._vrFp) {
        this._vrDrive = null;
        if (this._vrPrecise) {
          this._vrPrecise = false;
          if (typeof this.rt.set_precise_movement === 'function') {
            this.rt.set_precise_movement(false);
            this.rt.set_left_stick(0, 0);
          }
        }
      }
    }

    /* One VR first-person input sample (called once per XR frame by the VR
     * module's drive hook). Routes the left stick into the ENGINE's
     * free-movement controller - the same collision-checked path the keyboard
     * uses - by (a) pointing the engine's camera azimuth along the gaze so
     * "stick forward" walks where the user looks, and (b) feeding the stick
     * as the analog axes of the engine's precise-locomotion decode (falling
     * back to 8-way d-pad bits on a stale cached WASM without that API).
     * Trigger / A = Cross (talk / confirm), B = Circle (cancel). */
    _vrDriveInput(d) {
      /* Azimuth a makes screen-up walk along world (sin a, cos a). */
      const azRad = Math.atan2(d.forward[0], d.forward[1]);
      const azimuth = ((Math.round(azRad / (Math.PI * 2) * 4096) % 4096) + 4096) % 4096;
      let pad = 0;
      if (d.buttons.trigger || d.buttons.a) pad |= 0x4000;  /* Cross */
      if (d.buttons.b) pad |= 0x2000;                       /* Circle */
      const hasPrecise = typeof this.rt.set_precise_movement === 'function'
        && typeof this.rt.set_left_stick === 'function';
      if (hasPrecise) {
        if (!this._vrPrecise) {
          this.rt.set_precise_movement(true);
          this._vrPrecise = true;
        }
        const clamp = (v) => Math.max(-127, Math.min(127, Math.round(v * 127)));
        /* PSX stick: +Y is DOWN; our z is forward(+). */
        this.rt.set_left_stick(clamp(d.x), clamp(-d.z));
      } else if (Math.hypot(d.x, d.z) > 0.3) {
        const ang = Math.atan2(d.x, d.z);   /* 0 = forward, + = right */
        const oct = ((Math.round(ang / (Math.PI / 4)) % 8) + 8) % 8;
        const DIR = [0x0010, 0x0030, 0x0020, 0x0060,
          0x0040, 0x00C0, 0x0080, 0x0090];
        pad |= DIR[oct];
      }
      this._vrDrive = { pad, azimuth };
    }

    /* ---------- scene ---------- */

    /* Boot a CDNAME scene through the engine and (re)build everything drawn.
     * Throws the engine's error message when the label doesn't resolve. */
    enter(label) {
      const state = JSON.parse(this.rt.enter_field(label));
      this._rebuild();
      this.scene = state.scene || label;
      /* Demo tile board (`?tileboard=1`): the browser's twin of the native
       * window's `LEGAIA_TILE_BOARD_DEMO=1`, which no browser can set. No
       * retail scene installs a board, so without a trigger the per-cell
       * draw pass is unreachable on this host. */
      if (/[?&]tileboard=1\b/.test(location.search)
          && typeof this.rt.play_install_demo_tile_board === 'function') {
        try { this.rt.play_install_demo_tile_board(); } catch (e) {}
      }
      if (this.vr) {
        this.vr.setReady(true);
        /* A live session survives a scene swap (same canvas / GL context) - just
         * re-place the viewer in the new map. */
        this.vr.respawn();
      }
      return state;
    }

    /* Swap in a freshly-instantiated engine (after a WASM trap poisoned the
     * previous one) and re-enter `label`, then resume the loop. Reuses the
     * existing GL renderer + canvas + input listeners - only the engine handle
     * changes, so every callback that reads `this.rt` picks up the new one.
     * The page's `recoverRuntime` builds the fresh runtime from cached disc
     * bytes and drives this (Bug-3 recovery). */
    recover(newRuntime, label) {
      this.rt = newRuntime;
      this.stop();               /* the trapped loop is dead; clear its raf id */
      this._vrDrive = null;
      this._audioUp = false;     /* fresh engine has no audio output yet */
      const st = this.enter(label);
      this.start();
      return st;
    }

    /* Rebuild the GPU-side scene from whatever the engine currently holds.
     * Runs on entry and whenever the engine walks through a door. */
    _rebuild() {
      const rt = this.rt;
      this.renderer.clearScene();
      this.staticDraws = [];
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
          rt.field_ground_cba_tsb(), rt.field_ground_indices());
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
      const isSky = (window.FieldSceneView && window.FieldSceneView.isSkyMesh)
        || (() => false);
      const push = (slots, pos, rots, anims) => {
        for (let i = 0; i < slots.length; i++) {
          const anim = anims ? anims[i] : 0;
          let meshId, animRec = null;
          if (anim) {
            /* Animated prop: its own instance, uploaded at the rest pose
             * (frame 0) and re-posed per frame from the engine's live cursor. */
            meshId = ANIM_PROP_BASE + i;
            if (!uploadPosedInstance(meshId, slots[i], anim)) continue;
            animRec = { meshId, i, slot: slots[i], anim, lastFrame: 0 };
          } else {
            meshId = ensure(slots[i], 0);
            if (meshId < 0) continue;
          }
          /* Sky domes and kilometre-wide horizon planes read as sky only from
           * the retail in-world camera; from a follow camera inside them they
           * are a wall in front of the lens. Same classifier the full-map view
           * uses. */
          const aabb = this.renderer.getMeshAabb(meshId);
          if (isSky(aabb)) continue;
          const draw = {
            meshId,
            x: pos[i * 3], y: -pos[i * 3 + 1], z: pos[i * 3 + 2],
            rotY: -(rots[i] & 0xFFF) * A2R,
            scale: 1.0,
          };
          /* World box for the occluder test, baked once (see `_frame`). */
          draw.box = placementWorldBox(aabb, draw);
          this.staticDraws.push(draw);
          if (animRec) this.animProps.push(animRec);
        }
      };
      push(rt.field_terrain_slots(), rt.field_terrain_positions(), rt.field_terrain_rot_y(), null);
      push(rt.field_placement_slots(), rt.field_placement_positions(), rt.field_placement_rot_y(),
        rt.field_placement_anim_ids());

      /* Player: geometry once, positions re-uploaded per frame from the pose. */
      if (rt.player_has_mesh()) {
        const base = rt.player_mesh_positions();
        const idx = rt.player_mesh_indices();
        if (base.length && idx.length) {
          const flat = rt.player_mesh_flat_rgba();
          this.renderer.uploadSceneMesh(PLAYER_MESH_ID, base, rt.player_mesh_uvs(),
            rt.player_mesh_cba_tsb(), idx, flat.length ? flat : null);
          this.player = { verts: base.length / 3 };
        }
      }

      /* NPCs: the scene's MAN placements. The scene-entry spawn-prologue
       * pre-run (engine-side, retail FUN_8003A1E4) can SEAT a header-parked
       * placement into the town per story state, and can PARK a header-placed
       * one at the off-map hide box - so upload every placement except one
       * that is header-parked AND still parked live (the native window's
       * upload rule), and let the per-frame draw skip anyone whose live
       * position is the hide box. */
      this._hideXZ = (typeof rt.field_offmap_hide_xz === 'function')
        ? rt.field_offmap_hide_xz() : 16320;
      const cat = JSON.parse(rt.play_npc_catalog_json() || 'null');
      if (cat) {
        const nt0 = rt.play_npc_transforms();
        for (const npc of cat.npcs) {
          const b4 = npc.i * 4;
          const liveParked = (b4 + 3 >= nt0.length)
            || (nt0[b4] === this._hideXZ && nt0[b4 + 2] === this._hideXZ);
          if (npc.conditional && liveParked) continue;
          let ok = true;
          try { rt.play_npc_mesh(npc.i); } catch (e) { ok = false; }
          if (!ok) continue;
          const base = rt.play_npc_mesh_positions();
          const idx = rt.play_npc_mesh_indices();
          if (!base.length || !idx.length) continue;
          const flat = rt.play_npc_mesh_flat_rgba();
          const meshId = NPC_MESH_BASE + npc.i;
          this.renderer.uploadSceneMesh(meshId, base, rt.play_npc_mesh_uvs(),
            rt.play_npc_mesh_cba_tsb(), idx, flat.length ? flat : null);
          const frames = rt.play_npc_pose_frames(npc.i);
          const dims = rt.play_npc_pose_dims(npc.i);
          const rec = {
            i: npc.i, slot: npc.slot, meshId, base,
            objectIds: rt.play_npc_mesh_object_ids(),
            frames, frameCount: dims[0], partCount: dims[1],
            out: new Float32Array(base.length), lastFrame: -1, lastGen: -1,
          };
          /* Pose to frame 0 immediately: an unposed multi-object character is a
           * heap of limbs at the origin, which is worse than not drawing it. */
          if (rec.frameCount > 0) {
            poseInto(rec.out, rec.base, rec.objectIds, rec.frames, rec.partCount, 0);
            this.renderer.updateSceneMeshPositions(meshId, rec.out);
          }
          this.npcs.push(rec);
        }
      }

      /* Frame the camera on the player straight away so the first painted frame
       * is already looking at them. */
      this._followCamera();
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

    /* ---------- input ---------- */

    _attachInput() {
      const onKey = (e, down) => {
        if (!this.canvas.matches(':focus-within') && document.activeElement !== this.canvas) return;
        const table = padTable();
        if (!table || table[e.code] === undefined) return;
        if (window.legaiaPadSwallows(e.code)) e.preventDefault();
        if (down) { this.held.add(e.code); this.pulse.add(e.code); }
        else this.held.delete(e.code);
        this._repack();
      };
      this._onDown = (e) => onKey(e, true);
      this._onUp = (e) => onKey(e, false);
      window.addEventListener('keydown', this._onDown);
      window.addEventListener('keyup', this._onUp);
      /* Blur drops every held key - otherwise tabbing away mid-walk leaves the
       * player marching into a wall forever. */
      this._onBlur = () => { this.held.clear(); this.pulse.clear(); this.pad = 0; };
      window.addEventListener('blur', this._onBlur);
      this.canvas.addEventListener('blur', this._onBlur);

      /* Camera orbit: drag to swing around the player, wheel to zoom. The engine
       * is told the new azimuth each frame, so "up" always walks away from the
       * camera - turning the camera turns the controls with it. */
      let dragging = false, lastX = 0, lastY = 0;
      this.canvas.addEventListener('pointerdown', (e) => {
        dragging = true; lastX = e.clientX; lastY = e.clientY;
        this.canvas.focus();
        this.canvas.setPointerCapture(e.pointerId);
        /* Autoplay backstop: a click on the canvas is a user gesture, so init
         * the BGM output here too (the scene-pick click already tries) and
         * always resume - browsers open the AudioContext suspended. Scene BGM
         * then feeds itself through the engine's per-tick op-0x35 routing. */
        this._enableAudio();
      });
      this.canvas.addEventListener('pointerup', (e) => {
        dragging = false;
        try { this.canvas.releasePointerCapture(e.pointerId); } catch (_) {}
      });
      this.canvas.addEventListener('pointermove', (e) => {
        if (!dragging) return;
        this.cam.yaw += (e.clientX - lastX) * 0.006;
        this.cam.pitch = Math.max(0.12, Math.min(1.35,
          this.cam.pitch + (e.clientY - lastY) * 0.004));
        lastX = e.clientX; lastY = e.clientY;
      });
      this.canvas.addEventListener('wheel', (e) => {
        e.preventDefault();
        const f = e.deltaY > 0 ? 1.12 : 0.89;
        this.cam.halfWidth = Math.max(220, Math.min(6000, this.cam.halfWidth * f));
        this.cam.halfHeight = this.cam.halfWidth;
      }, { passive: false });
    }

    /* Held keys OR the not-yet-consumed press edges -> one pad word. */
    _repack() {
      this.pad = padMaskOf(this.held) | padMaskOf(this.pulse);
    }

    /* Held-key state, for the on-screen control legend. */
    heldKeys() { return Array.from(this.held); }

    /* ---------- sound effects ---------- */

    /* Fire the cue mapped to a named event. The event -> cue-id map and each
     * id's provenance (`disc` = traced to a retail ring write, `site` = the
     * port's pick) live in the engine (`play_sfx_events_json`), so the page
     * names behaviour and never a cue number. Silent no-op until the disc's
     * descriptor table and program bank have staged, and against a cached WASM
     * that predates the channel. */
    sfxEvent(name) {
      if (typeof this.rt.play_sfx_event !== 'function') return false;
      try { return !!this.rt.play_sfx_event(name); } catch (e) { return false; }
    }

    /* The SFX channel's state (`{descriptors, bank_prot, vab_staged, fired,
     * last_cue, idle_voices, pending}`), or `null`. */
    sfxState() {
      if (typeof this.rt.play_sfx_state_json !== 'function') return null;
      try { return JSON.parse(this.rt.play_sfx_state_json()); }
      catch (e) { return null; }
    }

    /* ---------- fishing minigame ---------- */

    /* Is a fishing session live on the engine's world this frame? */
    fishingActive() {
      if (typeof this.rt.play_fishing_active !== 'function') return false;
      try { return !!this.rt.play_fishing_active(); } catch (e) { return false; }
    }

    /* Start / leave the fishing minigame. The engine suspends the current
     * scene mode and restores it on exit, banking the session's points into the
     * world's persistent pool - so the field is exactly where it was left and
     * the point total survives. Returns `true` on a state change. */
    toggleFishing() {
      const rt = this.rt;
      if (typeof rt.play_fishing_start !== 'function') return false;
      try {
        if (this.fishingActive()) {
          rt.play_fishing_stop();
          /* Drop the HUD immediately rather than waiting for the next layer to
           * claim the overlay. */
          if (this._menuCtx && this.menuOverlay) {
            this._menuCtx.clearRect(0, 0, this.menuOverlay.width, this.menuOverlay.height);
            this._overlayActive = false;
          }
          return true;
        }
        return !!rt.play_fishing_start();
      } catch (e) { return false; }
    }

    /* The live session's readout (`{live, phase, cast_power, tension, ...}`), or
     * `{live:false}`. */
    fishingState() {
      if (typeof this.rt.play_fishing_state_json !== 'function') return { live: false };
      try { return JSON.parse(this.rt.play_fishing_state_json()); }
      catch (e) { return { live: false }; }
    }

    /* ---------- retail pause menu (engine-driven) ---------- */

    /* Drive the retail pause menu from this frame's just-pressed edges (the
     * `pulse` set, read before the engine tick consumes it). The state machine
     * + navigation live in the engine (`play_menu_input`); this only opens it on
     * Start, closes it on Start, and forwards the remaining edges. Returns `true`
     * while the menu is up, so `_frame` freezes the field - retail holds the
     * world in SceneMode::Menu. */
    _updateFieldMenu() {
      const rt = this.rt;
      const p = this.pulse;
      /* Test the Start *bit*, not a key name: Start answers to Enter and
       * Space, and a rebinding moves it again. Keying off the literal made
       * this handler the one place the engine's table did not reach. */
      const startEdge = (padMaskOf(p) & window.legaiaPadButton('Start')) !== 0;
      let open;
      try { open = rt.play_menu_is_open(); } catch (e) { return false; }
      if (!open) {
        if (startEdge && this._canOpenFieldMenu()) {
          try { rt.play_menu_open(); } catch (e) { return false; }
          /* The engine can REFUSE - `play_menu_open` declines while a dialogue
           * engagement owns the player (`World::dialogue_owns_input`), which is
           * retail's engaged-bit branch. Take none of the follow-up on a
           * refusal: no confirm blip, no swallowed pad edge, no menu clock. */
          let opened = false;
          try { opened = rt.play_menu_is_open(); } catch (e) {}
          if (!opened) return false;
          this.sfxEvent('menu_confirm');
          this._ensureMenuBlitters();
          /* Start the menu clock now: whatever wall-clock gap preceded the
           * open is not menu time. */
          this._menuClock = performance.now();
          p.clear();
          this._repack();
          return rt.play_menu_is_open();
        }
        return false;
      }
      /* Menu up: Start toggles it shut; every other edge is the engine's. */
      if (startEdge) {
        try { rt.play_menu_close(); } catch (e) {}
        this.sfxEvent('menu_cancel');
      } else {
        let edge = 0;
        edge |= padMaskOf(p);
        /* Cue the engine's own blips off this frame's edges: a direction is a
         * cursor move, Cross a confirm, Circle a cancel. The cue ids and their
         * provenance come from the engine (`play_sfx_events_json`) - the page
         * never hard-codes one.
         *
         * These sound again. They were silent for a while by the engine's
         * choice: retail's three ids are pinned (`FUN_80032A44`), but the port
         * keyed them an octave below retail, which is why menu navigation
         * played thuds. That pitch is measured and fixed. The engine still
         * counts every request (`menu_cue_requests` in `play_sfx_state_json`)
         * alongside `queued`, so keep firing the events either way - the wiring
         * is what stays measurable. See `play_sfx::CUE_MENU_CURSOR` for the one
         * inexactness left, which is a bank choice rather than a pitch. */
        if (edge) {
          const DIRS = 0x0010 | 0x0020 | 0x0040 | 0x0080;
          if (edge & 0x4000) this.sfxEvent('menu_confirm');
          else if (edge & 0x2000) this.sfxEvent('menu_cancel');
          else if (edge & DIRS) this.sfxEvent('menu_cursor');
        }
        /* Tick EVERY frame, edge or not, and tick at 60 Hz.
         *
         * The menu is not purely input-driven: the save screen's "Now
         * checking" dialog counts down a retail frame timer and its slide-ins
         * ramp per tick. Gating the tick on a keypress freezes the card read
         * forever; ticking once per rAF stretches a ~2 s beat to ~12 s when
         * the page is running at 10 fps. So run the menu on its own 60 Hz
         * clock and catch up whole ticks - the same reason retail scales its
         * slide increments by the frame-skip factor `DAT_1f800393` (see
         * docs/subsystems/save-screen.md), keeping the animation's real-time
         * speed constant however slow the frame is.
         *
         * The edge is delivered on the first tick only; the catch-up ticks
         * pass 0 so one keypress can never register twice. */
        const now = performance.now();
        if (!this._menuClock) this._menuClock = now;
        let ticks = Math.floor((now - this._menuClock) / MENU_TICK_MS);
        /* Cap the catch-up so a backgrounded tab can't spend a whole second
         * of wall clock replaying menu frames on return. */
        if (ticks > MENU_TICK_MAX_CATCHUP) {
          ticks = MENU_TICK_MAX_CATCHUP;
          this._menuClock = now;
        } else {
          this._menuClock += ticks * MENU_TICK_MS;
        }
        /* Always at least one tick, so an edge is never dropped. */
        for (let i = 0, n = Math.max(1, ticks); i < n; i++) {
          try { rt.play_menu_input(i === 0 ? edge : 0); } catch (e) {}
        }
        /* An in-canvas Load off a memory card lands the save's party in the
         * world, but the scene it was written in is the page's to enter
         * (`enter()` owns scene assembly). The engine parks the label; hand
         * it over the frame it appears. */
        let scene = '';
        try { scene = rt.play_menu_take_load_scene(); } catch (e) {}
        if (scene && typeof this.opts.onCardLoad === 'function') {
          this.opts.onCardLoad(scene);
        }
      }
      /* The menu owns every edge while it is up - clear them so none leak into
       * the frozen field on the next tick. */
      p.clear();
      this._repack();
      try { return rt.play_menu_is_open(); } catch (e) { return false; }
    }

    /* Drive the opening name-entry overlay from this frame's just-pressed edges.
     *
     * Like the field shop this has no open/close key of its own: the `town01`
     * establishing timeline's pinned op-0x49 opens it and the SM's own Select ->
     * "Is this name okay?" -> Yes closes it, at which point the engine writes the
     * name into the party record and the suspended opening script resumes. So
     * there is nothing to toggle - just forward edges while it is up.
     *
     * One tick per frame is right: the overlay steps exactly one cell / glyph per
     * press, and the only frame-counted thing on it (the caret blink) is advanced
     * inside `name_entry_input` because the field tick is frozen under it.
     *
     * Returns `true` while the overlay is up, so `_frame` freezes the field -
     * the naming prompt is modal, as it is natively. */
    _updateNameEntry() {
      const rt = this.rt;
      if (typeof rt.name_entry_is_active !== 'function') return false;
      let open;
      try { open = rt.name_entry_is_active(); } catch (e) { return false; }
      if (!open) return false;
      this._ensureMenuBlitters();
      let edge = 0;
      edge |= padMaskOf(this.pulse);
      let committed = false;
      try { committed = rt.name_entry_input(edge); } catch (e) {}
      if (committed && typeof this.opts.onNamed === 'function') {
        let name = '';
        try { name = rt.party_display_name(0); } catch (e) {}
        this.opts.onNamed(name);
      }
      /* The overlay owns every edge while it is up - clear them so none leak
       * into the frozen field on the next tick. */
      this.pulse.clear();
      this._repack();
      try { return rt.name_entry_is_active(); } catch (e) { return false; }
    }

    /* Drive an open field shop from this frame's just-pressed edges.
     *
     * Unlike the pause menu this has no open/close key: the field VM opens it
     * (a merchant's op-0x49 sub-0 record) and the player's **Exit** row closes
     * it, at which point the engine resumes the suspended script. So there is
     * nothing to toggle here - just forward edges while it is up.
     *
     * Also unlike the pause menu, one tick per frame is right: the shop has no
     * frame-counted animation to keep on a wall clock, so it needs no catch-up
     * clock of its own.
     *
     * Returns `true` while the shop is up, so `_frame` freezes the field. */
    _updateFieldShop() {
      const rt = this.rt;
      if (typeof rt.play_shop_is_open !== 'function') return false;
      let open;
      try { open = rt.play_shop_is_open(); } catch (e) { return false; }
      if (!open) return false;
      this._ensureMenuBlitters();
      let edge = 0;
      edge |= padMaskOf(this.pulse);
      try { rt.play_shop_input(edge); } catch (e) {}
      /* The shop owns every edge while it is up - clear them so none leak into
       * the frozen field on the next tick. */
      this.pulse.clear();
      this._repack();
      try { return rt.play_shop_is_open(); } catch (e) { return false; }
    }

    /* Start only opens the menu in ordinary field play - not on the world map,
     * in battle, or while a dialogue box is up (Start is inert there in
     * retail). */
    _canOpenFieldMenu() {
      let mode = '';
      try { mode = this.rt.scene_mode(); } catch (e) { return false; }
      if (mode !== 'Field') return false;
      if (this._hudState && this._hudState.dialog) return false;
      /* The opening chain / a narration beat owns the scene - Start is inert. */
      if (this._cut && (this._cut.locked || this._cut.chain)) return false;
      return true;
    }

    /* Upload the pause-menu atlases (font glyphs + the disc's menu-chrome sheet)
     * as `AtlasBlitter`s the first time the menu opens. Idempotent; the chrome
     * blitter stays `null` on a PROT.DAT-only load (glyphs only, no gold frame). */
    _ensureMenuBlitters() {
      if (this._menuFont && this._menuChrome !== undefined) return;
      const rt = this.rt;
      try {
        const fd = rt.play_menu_font_dims();
        if (!this._menuFont && fd && fd.length === 2 && fd[0] > 0 && fd[1] > 0) {
          const rgba = rt.play_menu_font_rgba();
          if (rgba && rgba.length) this._menuFont = new AtlasBlitter(rgba, fd[0], fd[1]);
        }
        if (this._menuChrome === undefined) {
          if (rt.play_menu_has_chrome()) {
            const cd = rt.play_menu_chrome_dims();
            const rgba = rt.play_menu_chrome_rgba();
            if (cd && cd.length === 2 && cd[0] > 0 && rgba && rgba.length) {
              this._menuChrome = new AtlasBlitter(rgba, cd[0], cd[1]);
            } else {
              this._menuChrome = null;
            }
          } else {
            this._menuChrome = null;
          }
        }
      } catch (e) { console.warn('play menu: atlas upload', e); this._menuChrome = null; }
    }

    /* Fishing HUD layer. Returns `true` when it drew (a session is live), so
     * `_drawOverlay` can stop there.
     *
     * The text quads come from the engine's shared draw-list consumer
     * (`fishing_hud_draws_for`, the same call the native window makes) and are
     * blitted from the font atlas like every other overlay layer. The `bars`
     * channel is separate for one reason: the fishing sprite page is the one
     * asset in the chain nobody has decoded, so the consumer's atlas is blind
     * and it drops the gauge fills. Their geometry still comes from the engine
     * (the ported cap/body/cap frame), so this only fills the rect it is told
     * to. */
    _drawFishingHud(ctx, ov) {
      if (typeof this.rt.play_fishing_hud_json !== 'function') return false;
      let hud = null;
      try { hud = JSON.parse(this.rt.play_fishing_hud_json(ov.width, ov.height)); }
      catch (e) { return false; }
      if (!hud || !hud.open) return false;
      this._ensureMenuBlitters();
      ctx.clearRect(0, 0, ov.width, ov.height);
      /* Gauge fills first, so the digit rows read on top of them. Coordinates
       * are retail 320x240 stage pixels; `stage` is the engine's own
       * origin/scale for this surface, the same transform its text quads were
       * already scaled by. */
      const st = hud.stage || [0, 0, 1];
      for (const b of (hud.bars || [])) {
        if (b.w <= 0 || b.h <= 0) continue;
        ctx.fillStyle = 'rgb(' + b.rgb[0] + ',' + b.rgb[1] + ',' + b.rgb[2] + ')';
        ctx.fillRect(st[0] + b.x * st[2], st[1] + b.y * st[2],
          Math.max(1, b.w * st[2]), Math.max(1, b.h * st[2]));
      }
      if (this._menuFont) this._menuFont.blit(ctx, hud.texts);
      this._overlayActive = true;
      return true;
    }

    /* Blit the current pause-menu OR retail-dialog draw lists onto the 2D
     * overlay canvas: the gold 9-slice / filigree chrome from the menu sheet,
     * then the font glyphs. A no-op (and a one-shot clear) when neither is up.
     *
     * The pause menu blacks the whole surface (retail suppresses the 3D draw
     * under it); the dialog reading box does NOT - retail draws it over the
     * live, still-running field, so only the box quads paint. */
    _drawOverlay() {
      const ov = this.menuOverlay;
      if (!ov) return;
      const ctx = this._menuCtx || (this._menuCtx = ov.getContext('2d'));
      this._drawOverlayBase(ctx, ov);
      /* Developer-menu overlay (the visitor's explicit opt-in - the browser
       * twin of the native LEGAIA_DEV_MENU surface). Painted last so it rides
       * whatever the base layers drew, the same way the native window folds
       * its dev draws into the field HUD's list; the engine returns
       * `open:false` while the opt-in is off or the pause menu owns the
       * screen, so this is a no-op on the shipped default. */
      if (typeof this.rt.play_dev_menu_draws_json === 'function') {
        let dev = null;
        try { dev = JSON.parse(this.rt.play_dev_menu_draws_json(ov.width, ov.height)); }
        catch (e) { dev = null; }
        if (dev && dev.open) {
          this._ensureMenuBlitters();
          if (this._menuFont) this._menuFont.blit(ctx, dev.texts);
          this._overlayActive = true;
        }
      }
    }

    _drawOverlayBase(ctx, ov) {
      /* PSX UI is nearest-neighbour: the native wgpu overlay samples the atlas
       * with no filtering, so the integer-scaled tiles butt edge-to-edge. Canvas
       * 2D defaults to bilinear (`imageSmoothingEnabled` true), which bleeds a
       * half-texel across every tile boundary - the visible seams in the 9-slice
       * chrome's repeated fill. Force nearest so the repeat is seamless and the
       * glyphs stay crisp, matching native. */
      ctx.imageSmoothingEnabled = false;
      /* Settle the reading-box capability once, ahead of every early return
       * below, so it is already true before the first conversation and cannot
       * flip with whatever else owns the overlay this frame. Both halves have
       * to hold: the engine must export the builder (a cached WASM predating
       * it does not), and the font atlas must have uploaded, because without
       * `_menuFont` the blit paints no glyphs. `_ensureMenuBlitters` is
       * idempotent and returns immediately once built. */
      if (!this.dialogCanvasCapable
          && typeof this.rt.play_dialog_draws_json === 'function') {
        this._ensureMenuBlitters();
        if (this._menuFont) this.dialogCanvasCapable = true;
      }
      let open = false;
      try { open = this.rt.play_menu_is_open(); } catch (e) {}
      if (open) {
        this._overlayActive = true;
        this._ensureMenuBlitters();
        let draws;
        try { draws = JSON.parse(this.rt.play_menu_draws_json(ov.width, ov.height)); }
        catch (e) { return; }
        if (!draws || !draws.open) return;
        /* Native blacks the whole framebuffer while the pause menu is up
         * (`boot_ui.is_active()` clears to black + suppresses every 3D draw) - the
         * frozen scene is NOT visible around the windows. This overlay canvas sits
         * over the GL view, so paint it fully opaque black first, then blit the
         * menu on top: same result as native's black backdrop. */
        ctx.clearRect(0, 0, ov.width, ov.height);
        ctx.globalAlpha = 1;
        ctx.fillStyle = '#000';
        ctx.fillRect(0, 0, ov.width, ov.height);
        if (this._menuChrome) this._menuChrome.blit(ctx, draws.sprites);
        if (this._menuFont) this._menuFont.blit(ctx, draws.texts);
        return;
      }
      /* Opening name-entry overlay (`town01`'s naming prompt). Modal, so it is
       * checked before the shop / dialog layers - but like them it composites
       * over the live scene rather than blacking it: retail draws the grid and
       * name-field windows over the frozen establishing shot. */
      let naming = null;
      if (typeof this.rt.name_entry_draws_json === 'function') {
        try { naming = JSON.parse(this.rt.name_entry_draws_json(ov.width, ov.height)); }
        catch (e) { naming = null; }
      }
      if (naming && naming.open) {
        this._ensureMenuBlitters();
        ctx.clearRect(0, 0, ov.width, ov.height);
        if (this._menuChrome) this._menuChrome.blit(ctx, naming.sprites);
        if (this._menuFont) this._menuFont.blit(ctx, naming.texts);
        this._overlayActive = true;
        return;
      }

      /* Fishing minigame HUD (the retail persistent + catch rows through the
       * shared `fishing_hud_draws_for` consumer, plus the gauge frames the
       * undecoded sprite page cannot fill). Checked before the shop / dialog
       * layers because fishing is a mode *suspend*: while it runs, the field
       * underneath is frozen and nothing else can be up. Composites over the
       * live scene rather than blacking it. */
      if (this._drawFishingHud(ctx, ov)) return;

      /* Field merchant panel + post-action banners (level-up, Seru capture).
       * Same builders as the native window (`shop_draws_for`,
       * `level_up_draws_for`, `capture_banner_draws_for`); like the dialog box
       * they composite over the live field rather than blacking it. Sits above
       * the dialog check because a merchant's box closes before the shop
       * opens, and a banner should not be hidden by one. */
      let shop = null;
      if (typeof this.rt.play_overlay_draws_json === 'function') {
        try { shop = JSON.parse(this.rt.play_overlay_draws_json(ov.width, ov.height)); }
        catch (e) { shop = null; }
      }
      /* This layer does NOT own the frame: the battle HUD rides it, and a
       * reading box can be up during battle (the sparring fight talks over
       * the running battle). Returning here suppressed the canvas reading box
       * for every in-battle line, so the page fell back to the DOM
       * `.play-dialog` strip - CSS-positioned near the BOTTOM - while the
       * engine's own box geometry says top (`dialog_reading_box_layout`,
       * FUN_801D84D0). So blit and fall through to the dialog layer. */
      let overlayDrew = false;
      if (shop && shop.open) {
        this._ensureMenuBlitters();
        ctx.clearRect(0, 0, ov.width, ov.height);
        if (this._menuChrome) this._menuChrome.blit(ctx, shop.sprites);
        if (this._menuFont) this._menuFont.blit(ctx, shop.texts);
        this._overlayActive = true;
        overlayDrew = true;
      }

      /* Retail dialog reading box (field NPC / event message): the engine
       * serves the byte-pinned chrome + glyph quads; blit them over the live
       * GL view. The DOM text box stands in only where this whole path is
       * unavailable (`dialogCanvasCapable` false - a cached WASM without the
       * export, or no font atlas); it is never a per-frame alternative to
       * this, because the two are seated differently and a page that switches
       * between them mid-conversation shows the box move. */
      let dlg = null;
      if (typeof this.rt.play_dialog_draws_json === 'function') {
        try { dlg = JSON.parse(this.rt.play_dialog_draws_json(ov.width, ov.height)); }
        catch (e) { dlg = null; }
      }
      if (dlg && dlg.open) {
        this._ensureMenuBlitters();
        /* Only clear when nothing has painted this frame - the shop / battle
         * layer above already cleared and its quads must survive. */
        if (!overlayDrew) ctx.clearRect(0, 0, ov.width, ov.height);
        if (this._menuChrome) this._menuChrome.blit(ctx, dlg.sprites);
        if (this._menuFont) this._menuFont.blit(ctx, dlg.texts);
        this._overlayActive = true;
        return;
      }
      if (overlayDrew) return;
      /* Opening-cutscene narration crawl / title card / "It was the Seru."
       * caption: font-atlas text quads + one faded image quad over the live
       * 3D prologue scene. */
      let cutDrew = false;
      if (this._cut && (this._cut.narration || this._cut.card
          || this._cut.caption_alpha > 0.001)
          && typeof this.rt.play_cutscene_text_draws_json === 'function') {
        let txt = null;
        try { txt = JSON.parse(this.rt.play_cutscene_text_draws_json(ov.width, ov.height)); }
        catch (e) { txt = null; }
        this._ensureMenuBlitters();
        ctx.clearRect(0, 0, ov.width, ov.height);
        if (txt && txt.open && this._menuFont) {
          this._menuFont.blit(ctx, txt.texts);
          cutDrew = true;
        }
        /* Caption image (a baked TIM): centered horizontally, mid-screen
         * ~y110 of the PSX 240-line frame, scaled by h/240, faded by the
         * engine's alpha - the native window's caption quad. */
        if (this._cut.caption_alpha > 0.001) {
          if (this._captionBlit === undefined) {
            this._captionBlit = null;
            try {
              const cd = this.rt.cutscene_caption_dims();
              if (cd && cd[0] > 0 && cd[1] > 0) {
                const rgba = this.rt.cutscene_caption_rgba();
                if (rgba && rgba.length) {
                  this._captionBlit = new AtlasBlitter(rgba, cd[0], cd[1]);
                }
              }
            } catch (e) { this._captionBlit = null; }
          }
          if (this._captionBlit) {
            const scale = ov.height / 240;
            const dw = Math.round(this._captionBlit.w * scale);
            const dh = Math.round(this._captionBlit.h * scale);
            const dx = Math.round((ov.width - dw) / 2);
            const dy = Math.round((110 / 240) * ov.height - dh / 2);
            this._captionBlit.blit(ctx, [{
              dst: [dx, dy, dw, dh],
              src: [0, 0, this._captionBlit.w, this._captionBlit.h],
              color: [1, 1, 1, Math.min(1, this._cut.caption_alpha)],
            }]);
            cutDrew = true;
          }
        }
        if (cutDrew) { this._overlayActive = true; return; }
      }
      if (this._overlayActive) {
        ctx.clearRect(0, 0, ov.width, ov.height);
        this._overlayActive = false;
      }
    }

    /* ---------- loop ---------- */

    start() {
      if (this.raf) return;
      if (this.vr && this.vr.isActive()) return;   /* the XR loop is driving */
      const tick = () => {
        this.raf = requestAnimationFrame(tick);
        this._frame();
      };
      this.raf = requestAnimationFrame(tick);
    }

    stop() {
      if (this.raf) cancelAnimationFrame(this.raf);
      this.raf = 0;
    }

    setPaused(on) { this.paused = !!on; }
    step() { this.stepOnce = true; }

    /* Bring up the WebAudio BGM backend from inside a user gesture (browser
     * autoplay policy). Idempotent: initialises the output once, then always
     * resumes the AudioContext (browsers open it suspended even in a gesture).
     * The scene's music plays itself - every engine tick routes the field VM's
     * op-0x35 BGM events into the output - so there is nothing to start here.
     * Tolerant of a stale cached WASM without the audio methods. */
    _enableAudio() {
      const rt = this.rt;
      if (!rt || typeof rt.audio_init !== 'function') return;
      try {
        if (typeof rt.audio_ready === 'function' ? !rt.audio_ready() : !this._audioUp) {
          rt.audio_init();
          this._audioUp = true;
        }
        if (typeof rt.audio_resume === 'function') rt.audio_resume();
        /* `audio_init` parks the engine's default gain on the GainNode, which
         * would silently override wherever the page's volume slider is sitting
         * (and does, after a trap-recovery rebuild). Re-assert the control. */
        if (typeof window.__playApplyVolume === 'function') window.__playApplyVolume();
        /* Same for the dev-menu opt-in: a rebuilt runtime starts with it off,
         * so re-assert whatever the page checkbox says. */
        if (typeof window.__playApplyDevMenu === 'function') window.__playApplyDevMenu();
      } catch (e) { console.warn('play audio enable', e); }
    }

    /* A WASM trap (or any throw from an engine call) during the frame poisons
     * the engine instance. Stop the dead loop and hand the message to the page,
     * whose `onError` rebuilds a fresh runtime from cached disc bytes and
     * resumes - no page reload (Bug-3 recovery). Unifies every in-frame engine
     * call (tick, scene rebuild, per-frame pose reads, draw) onto one path so
     * none can escape uncaught and freeze the loop. */
    _onEngineTrap(where, e) {
      console.warn(where, e);
      this.stop();
      if (this.opts.onError) this.opts.onError((e && e.message) || String(e));
    }

    /* One engine frame + one draw (the draw is skipped while VR presents). */
    _frame(skipDraw) {
      const rt = this.rt;
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

      /* Field pause menu (Start): consumes this frame's edges and, while up,
       * freezes the field. Must run before the tick reads the pad. */
      const menuOpen = this._updateFieldMenu();
      /* Field merchant (field-VM op 0x49 sub-0). The shop suspends the script
       * on the engine side, so the field must not advance under it either. */
      const shopOpen = menuOpen ? false : this._updateFieldShop();
      /* Opening name-entry prompt (the `town01` timeline's op 0x49). Suspends
       * the script the same way, and is modal over everything else. */
      const namingOpen = (menuOpen || shopOpen) ? false : this._updateNameEntry();

      if (advance && !menuOpen && !shopOpen && !namingOpen) {
        /* Run the engine at a fixed 60 Hz regardless of the display refresh
         * (see the `_simAccum` note in the constructor). `Step 1 frame` forces
         * exactly one tick; free play consumes the real elapsed time. */
        const TICK_DT = 1000 / 60;
        let steps;
        if (stepping) {
          steps = 1;
          this._simAccum = 0;
          this._simLast = performance.now();
        } else {
          const now = performance.now();
          this._simAccum += now - this._simLast;
          this._simLast = now;
          /* Cap the backlog so a long stall (hidden tab, GC pause) can't
           * unleash a burst of catch-up ticks - the native window caps at
           * 4 ticks/frame the same way. */
          if (this._simAccum > TICK_DT * 4) this._simAccum = TICK_DT * 4;
          steps = Math.floor(this._simAccum / TICK_DT);
          this._simAccum -= steps * TICK_DT;
        }
        for (let s = 0; s < steps; s++) {
          /* Retail prologue intro-skip (FUN_801D1344): while the opening
           * chain plays, a Cross press skips the whole remaining opening to
           * town01 - available mid-narration too. The engine returns the
           * target label once; enter it like a door. */
          if (this._cut && this._cut.chain
              && (padMaskOf(this.pulse) & window.legaiaPadButton('Cross'))
              && typeof rt.play_take_prologue_handoff === 'function') {
            let target = '';
            try { target = rt.play_take_prologue_handoff(true); } catch (e) {}
            if (target) {
              try {
                rt.enter_field(target);
                this.scene = target;
                this._rebuild();
                if (this.vr) this.vr.respawn();
                if (this.opts.onScene) this.opts.onScene(target);
              } catch (e) {
                this._onEngineTrap('prologue handoff', e);
                return;
              }
              this.pulse.clear();
              this._repack();
              break;
            }
          }
          /* Party wipe: the engine raises game over and the overlay draws the
           * panel. This used to swallow every confirm key into a bare Retry,
           * with the drawn cursor pinned to row 1 - a picture of a menu. The
           * panel is a live `GameOverSession` on both hosts now, so the pad
           * edge routes into it and the picked row comes back out. Retail's
           * destination on a wipe is unpinned, so the panel itself is an
           * engine presentation - see docs/subsystems/battle.md. Handled
           * before the pad is fed in so the same press does not also walk the
           * player. */
          if (typeof rt.is_game_over === 'function' && rt.is_game_over()) {
            const edge = padMaskOf(this.pulse);
            this.pulse.clear();
            this._repack();
            let picked = '';
            try { picked = rt.game_over_input(edge); } catch (e) {}
            /* Continue opened the retail save-select on the card rack (the
             * shared pause-menu Load row); the menu loop below drives it from
             * the next frame. Quit hands back to the page, which re-runs the
             * boot title. */
            if (picked === 'quit' && this.opts.onQuit) this.opts.onQuit();
            break;
          }
          /* VR first-person owns the azimuth (the gaze) and merges its stick
           * pad word over the keyboard's; otherwise the follow camera rules. */
          const lockedPad = this._cut && this._cut.locked;
          if (this._vrDrive) {
            rt.set_camera_azimuth(this._vrDrive.azimuth);
            rt.set_pad(lockedPad ? 0 : (this.pad | this._vrDrive.pad));
          } else {
            rt.set_camera_azimuth(azimuthUnits(this.cam.yaw));
            rt.set_pad(lockedPad ? 0 : this.pad);
          }
          /* A tap's just-pressed edge fires on the first tick of this frame
           * only; later catch-up ticks see the held set, so a one-frame tap
           * lands as exactly one edge however many ticks the frame runs. */
          this.pulse.clear();
          this._repack();
          let entered = '';
          try { entered = rt.tick_frame(); } catch (e) {
            this._onEngineTrap('engine tick', e);
            return;
          }
          if (entered) {
            /* The engine walked through a door: its scene swapped under us, so
             * the geometry has to swap too. A trap while rebuilding the new
             * scene's geometry is just as fatal as one in the tick, so route it
             * through recovery too. */
            try {
              this.scene = entered;
              this._rebuild();
              if (this.vr) this.vr.respawn();
              if (this.opts.onScene) this.opts.onScene(entered);
            } catch (e) {
              this._onEngineTrap('scene rebuild', e);
              return;
            }
            /* Don't keep feeding this frame's input into the freshly-loaded
             * scene - resume ticking it next frame. */
            break;
          }
        }
      } else {
        /* Keep the sim clock current while paused so unpausing doesn't dump the
         * accumulated wall-clock gap as a burst of catch-up ticks. */
        this._simLast = performance.now();
      }

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
      if (this._battleFrame(rt, skipDraw)) {
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
      /* Occluder cull, DISABLED (`OCCLUDER_CULL = false`; see the note at its
       * definition). Even the exact segment-vs-world-AABB form culled legit
       * bodies: the boxes are axis-aligned over whole terrain tiles / walls /
       * buildings, so as the camera orbited or the player walked, the
       * lens->player segment pierced a neighbour's box and blinked it out. The
       * native renderer draws the whole scene unconditionally, and this page
       * matches it - the branch stays for reference but is never taken. */
      const pt = rt.player_transform();
      let draws;
      /* In VR first-person there is no third-person lens: the eye IS the
       * player, so nothing can "sit between" them - draw everything. */
      const fpLive = this._vrFp && this.vr && this.vr.isActive();
      if (OCCLUDER_CULL && !fpLive) {
        const eye = this._eye();
        const px = pt[0], py = -pt[1] + 90, pz = pt[2];
        const ex = eye[0] - px, ey = eye[1] - py, ez = eye[2] - pz;
        draws = this.staticDraws.filter(
          d => !segmentHitsBox(px, py, pz, ex, ey, ez, d.box));
      } else {
        draws = this.staticDraws.slice();
      }

      /* Player: the engine's live posed vertices + its world transform. The
       * world frame is retail's (+Y down), so the draw negates Y the way every
       * placement does. The mesh's rest pose faces -Z while the engine's heading
       * has 0 = travelling +Z, hence the half-turn. */
      if (this.player) {
        const posed = rt.player_mesh_positions();
        if (posed.length) this.renderer.updateSceneMeshPositions(PLAYER_MESH_ID, posed);
        draws.push({
          meshId: PLAYER_MESH_ID,
          x: pt[0], y: -pt[1], z: pt[2],
          rotY: -(pt[3] + 2048) * A2R,
          scale: 1.0,
        });
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
      const nt = rt.play_npc_transforms();
      const clipStates = (typeof rt.play_npc_clip_states === 'function')
        ? rt.play_npc_clip_states() : null;
      const clipFrame = Math.floor(performance.now() / 1000 * NPC_CLIP_FPS);
      for (let k = 0; k < this.npcs.length; k++) {
        const n = this.npcs[k];
        const base = n.i * 4;
        if (base + 3 >= nt.length) continue;
        /* Story-parked actor (spawn-prologue MoveTo to the off-map hide box,
         * or a cutscene hide): not drawn - retail parks despawned actors at
         * the far-corner sentinel tile precisely so they never render. */
        if (nt[base] === this._hideXZ && nt[base + 2] === this._hideXZ) continue;
        /* Board-owned actor: it draws once per board cell through the tile
         * pass below, and its own transform holds only the last repositioned
         * cell - drawing it here too ghosts a tile at whichever cell the
         * refresh touched last. The native redraw pass skips the same slots
         * (`is_tile_actor_slot`). */
        if (this.tileActorSlots.has(n.slot | 0)) continue;
        if (clipStates && n.i * 2 + 1 < clipStates.length) {
          const f = clipStates[n.i * 2], gen = clipStates[n.i * 2 + 1];
          if (f >= 0 && (f !== n.lastFrame || gen !== n.lastGen)) {
            const bones = rt.play_npc_live_bones(n.i);
            if (bones.length) {
              poseInto(n.out, n.base, n.objectIds, bones, bones.length / 6, 0);
              this.renderer.updateSceneMeshPositions(n.meshId, n.out);
              n.lastFrame = f; n.lastGen = gen;
            }
          }
        } else if (advance && n.frameCount > 1) {
          const f = clipFrame % n.frameCount;
          if (f !== n.lastFrame) {
            poseInto(n.out, n.base, n.objectIds, n.frames, n.partCount, f);
            this.renderer.updateSceneMeshPositions(n.meshId, n.out);
            n.lastFrame = f;
          }
        }
        draws.push({
          meshId: n.meshId,
          x: nt[base], y: -nt[base + 1], z: nt[base + 2],
          rotY: -(nt[base + 3] + 2048) * A2R,
          scale: 1.0,
        });
      }

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
        const tb = rt.play_tile_board_transforms();
        for (let i = 0; i + 3 < tb.length; i += 4) {
          const slot = tb[i] | 0;
          if (this.tileMeshSlots.indexOf(slot) < 0) continue;
          draws.push({
            meshId: TILE_MESH_BASE + slot,
            x: tb[i + 1], y: -tb[i + 2], z: tb[i + 3],
            rotY: 0, scale: 1.0,
          });
        }
      }

      /* Cutscene camera: while a timeline runs, aim the orbit camera from
       * the engine's staged op-0x45 params (the native `cutscene_view`
       * decode) instead of following the player. Mapped onto the page's
       * orbit projection: focus -> target, pitch/yaw -> orbit angles, and
       * the framing half-height from the eye depth x the PSX projection
       * (half-screen 120 px over the staged H focal length). */
      let cutsceneCam = false;
      if (this._cut && typeof rt.play_cutscene_camera_json === 'function') {
        try {
          const cc = JSON.parse(rt.play_cutscene_camera_json());
          if (cc && cc.active) {
            cutsceneCam = true;
            this.cam.centerX = cc.focus[0];
            this.cam.centerY = -cc.focus[1] + 60;
            this.cam.centerZ = cc.focus[2];
            this.cam.yaw = -cc.yaw;
            this.cam.pitch = Math.max(0.12, Math.min(1.35, cc.pitch));
            /* Roll (op-0x45 slot 2) tilts the frame about the view ray.
             * Negated for the same reason the yaw above is: the orbit
             * projection mirrors screen X (`buildWorldOrbitVp` negates P[0]),
             * which reverses the on-screen sense of both angles. */
            this.cam.roll = -(cc.roll || 0);
            const half = Math.abs(cc.tr[2]) * 120 / Math.max(cc.h, 1);
            this.cam.halfWidth = Math.max(220, Math.min(6000, half));
            this.cam.halfHeight = this.cam.halfWidth;
          }
        } catch (e) { /* keep the follow camera */ }
      }
      if (!cutsceneCam) { this.cam.roll = 0; this._followCamera(pt); }
      /* Prologue colour grade + gold depth-cue ramp (the native window's
       * per-frame set_color_grade / set_depth_cue_ramp staging). No-ops on
       * a renderer without the uniforms (cached JS). */
      if (this.renderer.setColorGrade) {
        const g = this._cut && this._cut.grade;
        this.renderer.setColorGrade(g ? g.gold : null, g ? g.strength : 0);
        const c = this._cut && this._cut.cue;
        this.renderer.setDepthCue(c ? c.far : null,
          c ? c.near_z : 0, c ? c.far_z : 0, c ? c.max_ir0 : 0);
      }
      this._draws = draws;
      /* `skipDraw`: a VR session owns the framebuffer and re-issues this draw
       * once per eye with the XR view matrices. */
      if (!skipDraw) this.renderer.renderAssembled(this._draws, this._ext, this.cam);
      }
      } catch (e) {
        this._onEngineTrap('engine draw', e);
        return;
      }

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
    }

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
        /* Drop the battle VP override whenever battle isn't drawing, so the
         * field frame is back on the orbit projection even if the battle
         * state was torn down elsewhere (scene swap, trap recovery). */
        if (this.cam.vp) this.cam.vp = null;
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
      const b = this._battle;
      if (!b) return false;

      const draws = [];
      /* Backdrop (both copies pre-appended engine-side) + ground grid draw
       * at raw battle world coordinates; actors compose the retail 4x world
       * scale (base matrix 0x8007BF10), pre-scaled here because the page's
       * per-draw `scale` only scales the mesh, not its translation. */
      if (b.backdrop) draws.push({ meshId: b.backdrop, x: 0, y: 0, z: 0, rotY: 0, scale: 1.0 });
      /* The grid rides the stage's own GTE depth cue (`DAT_80078C1C` outdoor
       * table / indoor grey), as a PER-DRAW cue - nothing else in the frame
       * fogs. The engine resolved the far colour + ramp window at battle
       * entry; the page just attaches it. */
      if (b.ground) {
        draws.push({ meshId: b.ground, x: 0, y: 0, z: 0, rotY: 0, scale: 1.0, cue: b.groundCue });
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
      for (let i = 0; i < b.actors.length; i++) {
        const a = b.actors[i];
        const o = i * 5;
        if (!a || o + 4 >= tf.length || tf[o + 4] < 0.5) continue;   /* inactive */
        if (a.objectIds.length) {
          /* Live battle pose (idle loop / hit reaction / staged swing), the
           * same per-bone R.v+T composition every other animator here runs. */
          const pose = rt.play_battle_actor_pose(i);
          if (pose.length) {
            poseInto(a.out, a.base, a.objectIds, pose, pose.length / 6, 0);
            this.renderer.updateSceneMeshPositions(a.meshId, a.out);
          }
        }
        const c = (cursor && cursor.length >= (i + 1) * 6) ? i * 6 : -1;
        draws.push({
          meshId: a.meshId,
          /* Raw retail translation (Y-down, like the native actor_model) -
           * the battle VP carries no world negation, its trailing Y-flip
           * cancels the placement model's, so world Y goes through as-is. */
          x: tf[o] * S, y: tf[o + 1] * S, z: tf[o + 2] * S,
          /* Enemy meshes rest facing +Z; the enemy side carries the
           * half-turn toward the party (the native actor_model rule). */
          rotY: tf[o + 3] > 0.5 ? Math.PI : 0,
          scale: (c >= 0) ? S * cursor[c + 5] : S,
          cue: (c >= 0 && cursor[c] > 0.5)
            ? {
              far: [cursor[c + 1], cursor[c + 2], cursor[c + 3]],
              nearZ: -1, farZ: 0, maxIr0: cursor[c + 4],
            }
            : null,
        });
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

      this._draws = draws;
      if (!skipDraw) this.renderer.renderAssembled(draws, this._ext, this.cam);
      return true;
    }

    /* Battle effect layer: append this frame's FX draws to `draws`.
     *
     * Two engine-composed seams, both from `crates/web-viewer/play_battle_fx`:
     *
     *  - the effect POOL, as one batched mesh of camera-facing textured quads
     *    (the retail FUN_801E0088 pass-2 billboards, sampling the flame atlas
     *    the battle VRAM carries) plus tinted outline strips. Its vertices
     *    already carry the FX camera's 4x world scale, so it draws under an
     *    identity model - the basis is derived engine-side from the same
     *    battle VP the page renders with, which is what makes the quads face
     *    the camera that actually draws them.
     *  - the 3D FX MODELS: `etmd.dat` effect meshes and the move-VM
     *    scene-graph parts (summon + battle move-FX), each with a ready model
     *    matrix. Meshes are cached per global-TMD-pool index for the fight.
     *
     * Silent no-op against a cached WASM without the FX exports. */
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
      const up = (meshId, pos, uvs, ct, idx, flat) => {
        if (!pos.length || !idx.length) return 0;
        this.renderer.uploadSceneMesh(meshId, pos, uvs, ct, idx,
          (flat && flat.length) ? flat : null);
        return meshId;
      };
      const b = {
        gen,
        scale: (typeof rt.play_battle_world_scale === 'function')
          ? rt.play_battle_world_scale() : 4.0,
        backdrop: 0, ground: 0, actors: [],
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
      b.ground = up(BATTLE_MESH_BASE + 1, rt.play_battle_ground_positions(),
        rt.play_battle_ground_uvs(), rt.play_battle_ground_cba_tsb(),
        rt.play_battle_ground_indices(), null);
      const n = rt.play_battle_actor_count();
      for (let i = 0; i < n; i++) {
        const base = rt.play_battle_actor_positions(i);
        const idx = rt.play_battle_actor_indices(i);
        if (!base.length || !idx.length) { b.actors.push(null); continue; }
        const meshId = BATTLE_MESH_BASE + 16 + i;
        this.renderer.uploadSceneMesh(meshId, base, rt.play_battle_actor_uvs(i),
          rt.play_battle_actor_cba_tsb(i), idx, null);
        const rec = {
          meshId, base,
          objectIds: rt.play_battle_actor_object_ids(i),
          out: new Float32Array(base.length),
        };
        /* Pose to the rest/idle frame immediately: an unposed multi-part
         * character is a heap of limbs at the origin. (Empty objectIds =
         * the engine uploaded a statically-posed fallback mesh.) */
        if (rec.objectIds.length) {
          const pose = rt.play_battle_actor_pose(i);
          if (pose.length) {
            poseInto(rec.out, rec.base, rec.objectIds, pose, pose.length / 6, 0);
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

    /* Where the shared orbit projection puts the eye for the current camera
     * (`buildWorldOrbitVp`'s own formula, minus the aspect letterbox, which only
     * matters for framing). The occluder cull needs the eye in world space. */
    _eye() {
      const FOV_Y = 0.9;
      const dist = Math.max(this.cam.halfHeight / Math.tan(FOV_Y / 2), 1);
      const sy = Math.sin(this.cam.yaw), cy = Math.cos(this.cam.yaw);
      const sp = Math.sin(this.cam.pitch), cp = Math.cos(this.cam.pitch);
      return [
        this.cam.centerX + dist * sp * sy,
        this.cam.centerY + dist * cp,
        this.cam.centerZ - dist * sp * cy,
      ];
    }

    /* Keep the camera on the player: same target, user-controlled orbit. */
    _followCamera(pt) {
      const t = pt || this.rt.player_transform();
      this.cam.centerX = t[0];
      /* Retail Y is down-positive, and the draw frame flips it; target a little
       * above the floor so the camera looks at the character, not their feet. */
      this.cam.centerY = -t[1] + 60;
      this.cam.centerZ = t[2];
    }

    dispose() {
      this.stop();
      if (this.vr) { this.vr.destroy(); this.vr = null; }
      window.removeEventListener('keydown', this._onDown);
      window.removeEventListener('keyup', this._onUp);
      window.removeEventListener('blur', this._onBlur);
      if (this.renderer) { this.renderer.dispose(); this.renderer = null; }
    }
  }

  window.PlayView = PlayView;
  /* Shared by the boot-title controller in play.html, which renders the retail
   * title card onto the same overlay canvas before a scene exists. */
  window.LegaiaAtlasBlitter = AtlasBlitter;
})();
