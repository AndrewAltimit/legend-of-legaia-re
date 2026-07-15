/* vr-mode.js - WebXR `immersive-vr` mode for the site's WebGL2 3D pages.
 *
 * The world overview (three kingdom continents), the assembled full-map /
 * field-scene view (viewer + game-world pages) and the play page all draw
 * through the same `TmdRenderer.renderAssembled` path. This module lets you
 * step *inside* those scenes with a headset: stereo rendering through the
 * XRWebGLLayer, head tracking, and thumbstick locomotion.
 *
 * Design rule: the flat renderer stays the single source of geometry. Nothing
 * here uploads a mesh, compiles a shader or reorders a draw. Only two things
 * fork for XR, and both fork *around* `renderAssembled` rather than inside it:
 *
 *   1. the framebuffer + viewport (the XRWebGLLayer's fb, one eye rect per
 *      view), and
 *   2. the view-projection matrix (the XRFrame's per-eye matrices instead of
 *      the page's orbit camera).
 *
 * (1) is done by binding the layer's framebuffer, scissoring to the eye rect
 * (`renderAssembled` clears unconditionally - the scissor is what stops the
 * second eye from wiping the first) and shadowing `gl.viewport` for the
 * duration of the call. (2) is done by shadowing the global
 * `buildWorldOrbitVp` / `buildTopDownVp` (webgl-math.js) with a pass-through
 * that returns the XR matrix while a session is running. Both shadows are
 * installed only on the first `Enter VR` press and are inert when no session
 * is active, so a browser with no headset runs byte-identical code paths to
 * before this file existed.
 *
 * World scale is the one thing a VR port genuinely has to choose. The renderer
 * works in retail world units; XR works in metres. `unitsPerMeter` is the
 * bridge and it is what turns the same code into "a person standing in a town"
 * or "a giant looking down at a continent" - see docs/subsystems/vr-mode.md.
 *
 * Usage (see field-scene-view.js / world-overview-app.js / play-app.js):
 *
 *   const vr = window.LegaiaVr.attach({
 *     mount, renderer: () => r, draw: () => r.renderAssembled(draws, ext, cam),
 *     cam: () => cam, extent: () => ext, unitsPerMeter: 76,
 *   });
 *   vr.setReady(true);      // scene loaded: reveal the button (if supported)
 *   vr.end();               // force-exit (e.g. the scene is being swapped)
 *   vr.destroy();
 */
(function () {
  'use strict';

  /* ---------- small vec/mat helpers (column-major, like webgl-math.js) --- */

  function mul(a, b) {
    /* webgl-math.js's mulMat4 is a global; fall back if it ever moves. */
    if (typeof window.mulMat4 === 'function') return window.mulMat4(a, b);
    const out = new Float32Array(16);
    for (let r = 0; r < 4; r++) {
      for (let c = 0; c < 4; c++) {
        let s = 0;
        for (let k = 0; k < 4; k++) s += a[k * 4 + r] * b[c * 4 + k];
        out[c * 4 + r] = s;
      }
    }
    return out;
  }

  /* World -> XR-space transform for a player standing at `p` (world units,
   * +Y up - the frame `renderAssembled` draws in) facing `yaw`, with `upm`
   * world units per metre.
   *
   *   W = MirrorX * RotY(-yaw) * Scale(1/upm) * Translate(-p)
   *
   * The mirror is not cosmetic: both flat projections mirror screen X (the
   * retail horizontal flip - `buildWorldOrbitVp` negates P[0]) and the
   * fragment shader compensates with a hardcoded `u_normal_sign = -1`. An
   * unmirrored XR matrix would flip the handedness of the screen-space
   * derivatives the shader builds its lighting normal from, and every surface
   * would collapse to the ambient floor. Mirroring the *world* instead of the
   * projection keeps the shading correct and keeps both eyes consistent, so
   * stereo is unaffected. */
  function worldToXr(p, yaw, upm) {
    const s = 1 / upm;
    const c = Math.cos(yaw), sn = Math.sin(yaw);
    /* MirrorX * RotY(-yaw) * Scale(s), pre-multiplied, then translation:
     *   x_xr = s * (-cos(yaw)*dx + sin(yaw)*dz)
     *   y_xr = s * dy
     *   z_xr = s * ( sin(yaw)*dx + cos(yaw)*dz)   with d = p - origin */
    const r00 = -c * s, r02 = sn * s;
    const r20 = sn * s, r22 = c * s;
    return new Float32Array([
      r00, 0, r20, 0,
      0, s, 0, 0,
      r02, 0, r22, 0,
      -(r00 * p[0] + r02 * p[2]),
      -(s * p[1]),
      -(r20 * p[0] + r22 * p[2]),
      1,
    ]);
  }

  /* Inverse of the rotation/scale half of `worldToXr`: an XR-space direction
   * (metres) -> a world-space direction (world units). */
  function xrDirToWorld(d, yaw, upm) {
    const c = Math.cos(yaw), s = Math.sin(yaw);
    return [
      upm * (-c * d[0] + s * d[2]),
      upm * d[1],
      upm * (s * d[0] + c * d[2]),
    ];
  }

  /* Head forward (XR space, horizontal) from the viewer pose orientation. */
  function headForward(q) {
    const fx = -2 * (q.x * q.z + q.w * q.y);
    const fz = -(1 - 2 * (q.x * q.x + q.y * q.y));
    const l = Math.hypot(fx, fz);
    return l > 1e-4 ? [fx / l, 0, fz / l] : [0, 0, -1];
  }

  /* ---------- gamepad reading (xr-standard) ------------------------------ */

  const DEAD = 0.18;
  function axis(gp, i) {
    if (!gp || !gp.axes || gp.axes.length <= i) return 0;
    const v = gp.axes[i];
    return Math.abs(v) < DEAD ? 0 : v;
  }
  /* xr-standard puts the thumbstick on axes 2/3 and leaves 0/1 for a
   * touchpad; older/simpler mappings only expose 0/1. Prefer 2/3 when the
   * device has them. */
  function stick(gp) {
    if (!gp || !gp.axes) return [0, 0];
    if (gp.axes.length >= 4) return [axis(gp, 2), axis(gp, 3)];
    return [axis(gp, 0), axis(gp, 1)];
  }
  function pressed(gp, i) {
    return !!(gp && gp.buttons && gp.buttons[i] && gp.buttons[i].pressed);
  }

  /* ---------- global shadows (installed lazily, inert without a session) -- */

  let vpOverride = null;      /* Float32Array(16) while an eye is drawing */
  let hooksInstalled = false;

  function installHooks() {
    if (hooksInstalled) return;
    hooksInstalled = true;
    /* `buildWorldOrbitVp` / `buildTopDownVp` are top-level function
     * declarations in webgl-math.js (a classic script), so they live on the
     * global object and `renderAssembled`'s free-variable reference resolves
     * through it at call time - reassigning the property is enough. */
    for (const name of ['buildWorldOrbitVp', 'buildTopDownVp']) {
      const orig = window[name];
      if (typeof orig !== 'function') continue;
      window[name] = function (w, h, ext, cam) {
        return vpOverride || orig(w, h, ext, cam);
      };
    }
  }

  /* Ask for an XR-compatible GL context up front, so the renderers built after
   * the support probe resolves never need `makeXRCompatible()` (which is
   * allowed to drop the context when it has to move to another adapter).
   * Installed only once `immersive-vr` is known to be supported, so a browser
   * without a headset gets the untouched `getContext`. */
  let ctxHookInstalled = false;
  function installContextHook() {
    if (ctxHookInstalled || typeof HTMLCanvasElement === 'undefined') return;
    ctxHookInstalled = true;
    const orig = HTMLCanvasElement.prototype.getContext;
    HTMLCanvasElement.prototype.getContext = function (type, attrs) {
      if (type === 'webgl2' || type === 'webgl') {
        attrs = Object.assign({}, attrs || {}, { xrCompatible: true });
      }
      return orig.call(this, type, attrs);
    };
  }

  /* ---------- support probe ---------------------------------------------- */

  let supportPromise = null;
  function supported() {
    if (supportPromise) return supportPromise;
    supportPromise = (async () => {
      const xr = navigator.xr;
      if (!xr || typeof xr.isSessionSupported !== 'function') return false;
      let ok = false;
      try { ok = await xr.isSessionSupported('immersive-vr'); } catch (e) { ok = false; }
      if (ok) installContextHook();
      return !!ok;
    })();
    return supportPromise;
  }

  /* ---------- button chrome ---------------------------------------------- */

  const STYLE_ID = 'legaia-vr-style';
  function ensureStyle() {
    if (document.getElementById(STYLE_ID)) return;
    const st = document.createElement('style');
    st.id = STYLE_ID;
    st.textContent = `
.legaia-vr-btn {
  background: var(--bg-card, #1a1c2c); color: var(--text, #dfe3ff);
  border: 1px solid var(--accent, #7f9cff); border-radius: 3px;
  padding: 0.28rem 0.6rem; margin-left: 0.4rem;
  font-family: var(--font-mono, monospace); font-size: 0.74rem; cursor: pointer;
}
.legaia-vr-btn:hover { color: var(--accent, #7f9cff); }
.legaia-vr-btn[disabled] { opacity: 0.45; cursor: not-allowed; }
.legaia-vr-btn.is-active { background: var(--accent, #7f9cff); color: #0d0f1c; }
.legaia-vr-mode-btn {
  background: var(--bg-card, #1a1c2c); color: var(--text, #dfe3ff);
  border: 1px solid var(--accent, #7f9cff); border-radius: 3px;
  padding: 0.28rem 0.6rem; margin-left: 0.4rem;
  font-family: var(--font-mono, monospace); font-size: 0.74rem; cursor: pointer;
}
.legaia-vr-mode-btn:hover { color: var(--accent, #7f9cff); }
`;
    document.head.appendChild(st);
  }

  /* ---------- the session ------------------------------------------------- */

  const DEFAULTS = {
    unitsPerMeter: 76,      /* field/town default - see docs/subsystems/vr-mode.md */
    moveSpeed: 2.2,         /* metres/second at 1x */
    flySpeed: 1.6,
    snapDeg: 30,
    minUnitsPerMeter: 8,
    maxUnitsPerMeter: 20000,
    syncCamCenter: true,    /* write the VR position into cam.center{X,Z} */
    eyeHeightHint: 1.6,     /* only used when the device gives no floor space */
    label: 'VR',
  };

  /* The live session's handle - at most one per page (WebXR allows one
   * immersive session at a time). */
  let current = null;

  class VrHandle {
    constructor(cfg) {
      this.cfg = Object.assign({}, DEFAULTS, cfg);
      this.session = null;
      this.ready = false;
      this.upm = this.cfg.unitsPerMeter;
      this.pos = [0, 0, 0];       /* world units, +Y up; the XR floor origin */
      this.yaw = 0;
      this.home = null;
      this.snapLatch = false;
      this.btn = null;
      this.modeBtn = null;
      /* Optional viewing modes (e.g. diorama vs first-person). Each mode may
       * override unitsPerMeter (number or function), eyeHeightHint, start(),
       * and add anchor() / groundHeight() / drive() - see `_mode`. When absent
       * the handle behaves exactly as before modes existed. */
      this.modes = Array.isArray(cfg.modes) && cfg.modes.length ? cfg.modes : null;
      this.modeIdx = 0;
      this.onEndBound = () => this._teardown();
      this._buildButton();
      this._buildModeButton();
      supported().then((ok) => {
        this.supportedFlag = ok;
        this._sync();
      });
    }

    /* The active mode record ({} when the host configured no modes). */
    _mode() { return this.modes ? this.modes[this.modeIdx] : {}; }

    /* Select a viewing mode by index or id. Live sessions are re-placed in
     * the scene at the new mode's scale/anchor immediately. */
    setMode(sel) {
      if (!this.modes) return;
      const idx = typeof sel === 'number'
        ? sel : this.modes.findIndex(m => m.id === sel);
      if (idx < 0 || idx >= this.modes.length) return;
      this.modeIdx = idx;
      if (this.modeBtn) {
        const m = this.modes[idx];
        this.modeBtn.textContent = 'VR: ' + (m.label || m.id);
      }
      if (this.cfg.onMode) this.cfg.onMode(this.modes[idx].id);
      if (this.session) {
        this._placeStart();
        this._pushDepthRange();
      }
    }

    _buildButton() {
      const mount = this.cfg.mount;
      if (!mount) return;
      ensureStyle();
      /* One button per mount, even if the host rebuilds its view (the viewer
       * makes a fresh FieldSceneView per full-map load). */
      const stale = mount.querySelector('.legaia-vr-btn');
      if (stale) stale.remove();
      const b = document.createElement('button');
      b.type = 'button';
      b.className = 'legaia-vr-btn';
      b.hidden = true;
      b.textContent = 'Enter VR';
      b.title = 'View this scene in a VR headset (WebXR immersive-vr). '
        + 'Left stick moves, right stick turns / flies, grips scale the world.';
      b.addEventListener('click', () => {
        if (this.session) this.end();
        else this.enter().catch(err => this._fail(err));
      });
      mount.appendChild(b);
      this.btn = b;
    }

    /* Mode toggle - only when the host offers more than one viewing mode.
     * Sits next to the Enter VR button, cycles through the mode list, and is
     * usable both before entry and while a session is live. */
    _buildModeButton() {
      const mount = this.cfg.mount;
      if (!mount || !this.modes || this.modes.length < 2) return;
      ensureStyle();
      const stale = mount.querySelector('.legaia-vr-mode-btn');
      if (stale) stale.remove();
      const b = document.createElement('button');
      b.type = 'button';
      b.className = 'legaia-vr-mode-btn';
      b.hidden = true;
      const m0 = this.modes[this.modeIdx];
      b.textContent = 'VR: ' + (m0.label || m0.id);
      b.title = 'Toggle the VR viewing mode (diorama / first-person).';
      b.addEventListener('click', () => {
        this.setMode((this.modeIdx + 1) % this.modes.length);
      });
      mount.appendChild(b);
      this.modeBtn = b;
    }

    /* The host tells us when there is actually something to look at. */
    setReady(on) {
      this.ready = !!on;
      this._sync();
    }

    /* The host swapped the geometry under a live session (the game-world page's
     * town navigator does this): re-place the viewer in the new scene. */
    respawn() {
      if (!this.session) return;
      this._placeStart();
      this._pushDepthRange();
    }

    _sync() {
      if (!this.btn) return;
      const show = !!(this.supportedFlag && this.ready);
      this.btn.hidden = !show;
      this.btn.textContent = this.session ? 'Exit VR' : 'Enter VR';
      this.btn.classList.toggle('is-active', !!this.session);
      if (this.modeBtn) this.modeBtn.hidden = !show;
    }

    _fail(err) {
      const msg = (err && err.message) || String(err);
      if (this.cfg.onError) this.cfg.onError(msg);
      else console.warn('[vr]', msg);
      this._teardown();
    }

    isActive() { return !!this.session; }

    async enter() {
      if (this.session) return;
      /* Only one immersive session can exist per page; if some other view on
       * this page left one running, take it down first. */
      if (current && current !== this) current.end();
      const renderer = this.cfg.renderer();
      if (!renderer || !renderer.gl) throw new Error('no renderer to present');
      const gl = renderer.gl;
      installHooks();

      /* The context is normally already XR-compatible (`installContextHook`
       * asks for it at creation, once the support probe has said yes), but a
       * renderer built before the probe resolved needs the retro-fit call.
       *
       * It is raced against a timeout on purpose: the promise is specified to
       * settle, but a browser with `navigator.xr` and no backing XR runtime can
       * leave it pending forever (headless Chromium does exactly this), and a
       * hung await would wedge the button. If the context really cannot present,
       * the XRWebGLLayer constructor below throws and `_fail` surfaces it. */
      if (typeof gl.makeXRCompatible === 'function') {
        try {
          await Promise.race([
            gl.makeXRCompatible(),
            new Promise(res => setTimeout(res, 1500)),
          ]);
        } catch (e) { /* the layer constructor is the real gate */ }
      }
      const session = await navigator.xr.requestSession('immersive-vr', {
        optionalFeatures: ['local-floor', 'bounded-floor'],
      });
      this.session = session;
      current = this;
      this.gl = gl;

      const Layer = window.XRWebGLLayer;
      const layer = new Layer(session, gl);
      session.updateRenderState({ baseLayer: layer });
      this.layer = layer;

      /* `local-floor` puts the XR origin on the physical floor, which is what
       * makes the eye height come out right for free (head ~1.6 m above the
       * origin -> `1.6 * unitsPerMeter` world units above the scene's floor).
       * `local` is the fallback; there the head sits at the origin, so lift
       * the world anchor by the hinted eye height instead. */
      let space = null;
      let floorRelative = true;
      try {
        space = await session.requestReferenceSpace('local-floor');
      } catch (e) {
        space = await session.requestReferenceSpace('local');
        floorRelative = false;
      }
      this.space = space;
      this.floorRelative = floorRelative;

      this._placeStart();
      this._pushDepthRange();
      this.camBackup = this._snapshotCam();

      session.addEventListener('end', this.onEndBound);
      if (this.cfg.onEnter) this.cfg.onEnter();
      /* Tell the host which viewing mode the session opened in, so its
       * per-mode state (first-person flags, engine input routing) is armed
       * before the first XR frame. */
      if (this.cfg.onMode && this.modes) this.cfg.onMode(this._mode().id);
      this.lastT = 0;
      session.requestAnimationFrame((t, f) => this._frame(t, f));
      this._sync();
    }

    end() {
      if (this.session) {
        try { this.session.end(); } catch (e) { this._teardown(); }
      }
    }

    /* Spawn pose. The active mode (or the host) may hand us one; otherwise
     * stand at the flat camera's look-at target, facing the way the flat
     * camera faces (the orbit camera at yaw psi looks along (-sin psi,
     * cos psi), and our yaw 0 faces world -Z, hence `PI - psi`). */
    _placeStart() {
      const mode = this._mode();
      const cam = this.cfg.cam ? this.cfg.cam() : null;
      const startFn = mode.start || this.cfg.start;
      const start = startFn ? startFn() : null;
      const px = start ? start.x : (cam ? cam.centerX : 0);
      const pz = start ? start.z : (cam ? cam.centerZ : 0);
      let py = start ? start.y : 0;
      const yaw = start && start.yaw != null
        ? start.yaw
        : Math.PI - ((cam && cam.yaw) || 0);
      /* Per-mode world scale; a function lets the host derive it live (e.g.
       * from the measured player-mesh height). */
      const upmRaw = mode.unitsPerMeter != null
        ? mode.unitsPerMeter : this.cfg.unitsPerMeter;
      this.upm = Math.max(1e-6,
        typeof upmRaw === 'function' ? upmRaw() : upmRaw);
      /* Ground-locked modes stand on the terrain surface under the spawn. */
      if (typeof mode.groundHeight === 'function') py = mode.groundHeight(px, pz);
      const eyeHint = mode.eyeHeightHint != null
        ? mode.eyeHeightHint : this.cfg.eyeHeightHint;
      this.pos = [px, py + (this.floorRelative ? 0 : eyeHint * this.upm), pz];
      this.yaw = yaw;
      this.home = { pos: this.pos.slice(), yaw, upm: this.upm };
    }

    /* Depth range in metres, sized from the world extent at the current scale
     * so a whole continent stays inside the frustum without wrecking depth
     * precision on a room-scale town. */
    _pushDepthRange() {
      const ext = this.cfg.extent ? this.cfg.extent() : [16384, 16384];
      const spanM = Math.max(ext[0] || 0, ext[1] || 0) / this.upm;
      this.session.updateRenderState({
        depthNear: 0.05,
        depthFar: Math.max(50, spanM * 2.5 + 20),
      });
    }

    _snapshotCam() {
      const cam = this.cfg.cam ? this.cfg.cam() : null;
      return cam ? { centerX: cam.centerX, centerY: cam.centerY, centerZ: cam.centerZ } : null;
    }

    _restoreCam() {
      const cam = this.cfg.cam ? this.cfg.cam() : null;
      if (cam && this.camBackup) Object.assign(cam, this.camBackup);
    }

    _teardown() {
      const s = this.session;
      this.session = null;
      if (current === this) current = null;
      if (s) s.removeEventListener('end', this.onEndBound);
      vpOverride = null;
      this.layer = null;
      this.space = null;
      this._restoreCam();
      if (this.cfg.onExit) this.cfg.onExit();
      this._sync();
    }

    /* ---------- per-XRFrame ---------------------------------------------- */

    _frame(t, frame) {
      const session = this.session;
      if (!session || frame.session !== session) return;
      session.requestAnimationFrame((tt, ff) => this._frame(tt, ff));

      const dt = this.lastT ? Math.min(0.1, (t - this.lastT) / 1000) : 0;
      this.lastT = t;

      const pose = frame.getViewerPose(this.space);
      if (!pose) return;

      this._locomote(session, pose, dt);
      if (this.cfg.update) this.cfg.update(dt);

      /* An anchored mode (first-person) pins the rig's floor origin to a live
       * game entity every frame - read AFTER cfg.update so the rig tracks the
       * post-tick position (the engine, not the headset, owns the walk). */
      const anchorFn = this._mode().anchor;
      if (anchorFn) {
        const a = anchorFn();
        if (a) { this.pos[0] = a.x; this.pos[1] = a.y; this.pos[2] = a.z; }
      }

      /* Keep the ocean backdrop quad and the fog origin centred on the viewer -
       * both read cam.center{X,Z} inside renderAssembled. The play page opts
       * out (its follow camera owns the record). */
      if (this.cfg.syncCamCenter && this.cfg.cam) {
        const cam = this.cfg.cam();
        if (cam) { cam.centerX = this.pos[0]; cam.centerZ = this.pos[2]; }
      }

      const gl = this.gl;
      const layer = (session.renderState && session.renderState.baseLayer) || this.layer;
      const W = worldToXr(this.pos, this.yaw, this.upm);

      const realViewport = gl.viewport.bind(gl);
      gl.bindFramebuffer(gl.FRAMEBUFFER, layer.framebuffer);
      gl.enable(gl.SCISSOR_TEST);
      try {
        for (const view of pose.views) {
          const vp = layer.getViewport(view);
          if (!vp || vp.width === 0 || vp.height === 0) continue;
          /* `renderAssembled` clears the whole bound framebuffer; the scissor
           * confines that clear (and every draw) to this eye's rect, so the
           * second eye does not wipe the first. */
          gl.scissor(vp.x, vp.y, vp.width, vp.height);
          /* ...and it sets its own full-canvas viewport, so shadow the call. */
          gl.viewport = () => realViewport(vp.x, vp.y, vp.width, vp.height);
          gl.viewport();
          vpOverride = mul(view.projectionMatrix, mul(view.transform.inverse.matrix, W));
          this.cfg.draw();
        }
      } finally {
        vpOverride = null;
        delete gl.viewport;
        gl.disable(gl.SCISSOR_TEST);
      }
    }

    /* Thumbstick locomotion. Left stick = head-relative walk/fly-forward,
     * right stick X = snap turn, right stick Y = altitude, trigger = sprint,
     * grips = scale the world, A/X = respawn at the entry pose. */
    _locomote(session, pose, dt) {
      if (this.cfg.locomotion === 'none' || dt <= 0) return;
      const mode = this._mode();
      let mx = 0, mz = 0, turn = 0, lift = 0, boost = 1, scale = 0, reset = false;
      let trig = false, aBtn = false, bBtn = false;
      for (const src of session.inputSources) {
        const gp = src.gamepad;
        if (!gp) continue;
        const [sx, sy] = stick(gp);
        if (src.handedness === 'right') {
          turn += sx;
          lift += -sy;
          if (pressed(gp, 1)) scale += 1;
          if (pressed(gp, 4)) { reset = true; aBtn = true; }
          if (pressed(gp, 5)) bBtn = true;
        } else {
          mx += sx;
          mz += -sy;
          if (pressed(gp, 1)) scale -= 1;
        }
        if (pressed(gp, 0)) { boost = 4; trig = true; }
      }

      /* Snap turn (latched): comfort default, no continuous rotation. Shared
       * by every mode - in first-person it turns the rig around the anchor. */
      if (Math.abs(turn) > 0.7) {
        if (!this.snapLatch) {
          this.yaw += Math.sign(turn) * this.cfg.snapDeg * Math.PI / 180;
          this.snapLatch = true;
        }
      } else if (Math.abs(turn) < 0.4) {
        this.snapLatch = false;
      }

      const q = pose.transform.orientation;
      const f = headForward(q);

      /* First-person drive mode: the rig is anchored to a game entity and the
       * PAGE moves that entity through the engine (so collision / walkability
       * still applies). Hand it the stick, the gaze's world-space forward and
       * the button states; no free-fly integration happens here. */
      if (typeof mode.drive === 'function') {
        const fw = xrDirToWorld(f, this.yaw, 1);
        mode.drive({
          x: mx, z: mz, dt,
          forward: [fw[0], fw[2]],
          buttons: { trigger: trig, a: aBtn, b: bBtn },
        });
        return;
      }

      if (reset && this.home) {
        this.pos = this.home.pos.slice();
        this.yaw = this.home.yaw;
        if (this.upm !== this.home.upm) { this.upm = this.home.upm; this._pushDepthRange(); }
        return;
      }

      const grounded = typeof mode.groundHeight === 'function';
      const r = [-f[2], 0, f[0]];
      const spd = this.cfg.moveSpeed * boost * dt;
      const dxr = [
        (f[0] * mz + r[0] * mx) * spd,
        /* Ground-locked modes walk the surface: no free-fly altitude. */
        grounded ? 0 : lift * this.cfg.flySpeed * boost * dt,
        (f[2] * mz + r[2] * mx) * spd,
      ];
      const dw = xrDirToWorld(dxr, this.yaw, this.upm);
      this.pos[0] += dw[0];
      this.pos[1] += dw[1];
      this.pos[2] += dw[2];
      if (grounded) this.pos[1] = mode.groundHeight(this.pos[0], this.pos[2]);

      /* Scale the world around the head, not the feet, so growing/shrinking
       * doesn't shove the scene through the viewer. */
      if (scale !== 0) {
        const next = Math.max(this.cfg.minUnitsPerMeter,
          Math.min(this.cfg.maxUnitsPerMeter, this.upm * Math.exp(scale * 1.2 * dt)));
        if (next !== this.upm) {
          const p = pose.transform.position;
          const unit = xrDirToWorld([p.x, p.y, p.z], this.yaw, 1);
          const head = [
            this.pos[0] + unit[0] * this.upm,
            this.pos[1] + unit[1] * this.upm,
            this.pos[2] + unit[2] * this.upm,
          ];
          this.upm = next;
          this.pos = [
            head[0] - unit[0] * next,
            head[1] - unit[1] * next,
            head[2] - unit[2] * next,
          ];
          this._pushDepthRange();
        }
      }
    }

    destroy() {
      this.end();
      if (this.btn) { this.btn.remove(); this.btn = null; }
      if (this.modeBtn) { this.modeBtn.remove(); this.modeBtn = null; }
    }
  }

  window.LegaiaVr = {
    supported,
    attach(cfg) { return new VrHandle(cfg); },
    /* Verification hooks (same spirit as the pages' `__woCam` / `__fsState`):
     * the headless mock-XR harness reads the live session's pose and the pure
     * transform kernels through these. Not used by page code. */
    active: () => current,
    /* One-call debug snapshot of the live session's rig, for the mock-XR
     * verification harness (`window.__vrDebug()`). */
    debug: () => {
      const h = current;
      if (!h || !h.session) return null;
      return {
        mode: h.modes ? h._mode().id : 'default',
        pos: Array.from(h.pos),
        yaw: h.yaw,
        upm: h.upm,
        floorRelative: h.floorRelative,
      };
    },
    _worldToXr: worldToXr,
    _xrDirToWorld: xrDirToWorld,
  };
  window.__vrDebug = window.LegaiaVr.debug;
})();
