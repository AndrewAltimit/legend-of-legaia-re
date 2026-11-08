/* Single-model orbit view with a real on-disc animation player.
 *
 * Wraps the shared `TmdRenderer` (the same R16UI paletted-VRAM pipeline the
 * world-overview and full-map views use) with orbit / drag / zoom controls and
 * a per-frame skeletal poser. Used by the characters page (Vahn / Noa / Gala,
 * field + battle forms) and the NPC catalog - the two pages that show one
 * posed figure at a time.
 *
 * **Why the poser is mandatory, not decorative.** A Legaia character TMD ships
 * its vertices in *object-local* space: each object's coordinates are relative
 * to its own joint, not to the model origin. Drawn raw, the parts collapse into
 * a pile at the origin. The assembled figure is
 *
 *     v_world = R_bone . v_object_local + T_bone
 *
 * with (R, T) read per (bone, frame) from the ANM record. That is exactly the
 * retail per-object GTE pipeline: FUN_8001B964 loads the actor base matrix,
 * FUN_8001BE80 pushes the bone's decoded T through MVMVA to land the object's
 * world translation and composes the bone's Rz.Ry.Rx onto the matrix, then
 * FUN_8002735C draws that object's object-local vertices as M.v + TR. There is
 * no pivot / centroid subtraction - the TMD authors each object so its local
 * origin IS the joint.
 *
 * Requires webgl-math.js + webgl-shaders.js + webgl-tmd.js first.
 *
 *   const view = new MeshView(canvas, { onSpin: (on) => updateLabel(on) });
 *   view.uploadVram(bytes);
 *   view.setMesh(positions, uvs, cbaTsb, indices, bounds, objectIds, flatRgba);
 *   view.setAnimation({ partCount, frameCount, frames });   // null = rest
 *   view.setPlaying(true);
 *
 * **Between keyframes.** Retail does not hold a keyframe until the next one:
 * the battle anim tick (`FUN_80047430`) moves a 12.4 cursor once a battle
 * frame and the decoder (`FUN_8004998C`) blends toward the next keyframe by
 * the cursor's sixteenths. The view has two readings of a clip, both through
 * the WASM `PoseSampler` (crates/web-viewer/src/pose_sampler.rs), so the
 * blend is the engine's own:
 *   - smooth (the default, an enhancement): the blend at a continuous
 *     position, re-posed every display frame;
 *   - retail stepping: the retail cursor, advanced once per battle frame
 *     (`meta.retailStep` vsyncs, default 2) and decoded at its sixteenth.
 * The choice is one per visitor (`MeshView.smoothPref`, localStorage), shared
 * by every page that uses this view. Without the sampler (an older bundle) the
 * view steps whole keyframes, as it always did.
 */
(function () {
  'use strict';

  /* Retail clips run at ~14 fps. */
  const ANIM_FPS = 14;
  /* PSX angle units -> radians (4096 = one revolution). */
  const A2R = (Math.PI * 2) / 4096;
  /* Display vsyncs per second the retail cursor is clocked on. */
  const VSYNC_HZ = 60;
  /* Visitor preference: smooth (interpolated) playback, default on. */
  const SMOOTH_KEY = 'legaia-anim-smooth';
  function readSmoothPref() {
    try { return window.localStorage.getItem(SMOOTH_KEY) !== '0'; } catch (e) { return true; }
  }
  /* Every live view, so one toggle flips them all. */
  const VIEWS = new Set();

  class MeshView {
    /* `opts.onSpin(isSpinning)` fires whenever auto-rotate flips, so the page
     * can re-label its spin button. */
    constructor(canvas, opts) {
      if (typeof window.TmdRenderer === 'undefined') {
        throw new Error('TmdRenderer global missing (webgl-tmd.js not loaded?)');
      }
      const o = opts || {};
      this.canvas = canvas;
      this.renderer = new window.TmdRenderer(canvas);
      this.onSpin = o.onSpin || null;
      this.onFrame = null;   /* once per keyframe index */
      this.onPose = null;    /* after every posed sample (smooth: every display frame) */
      this.defaultCam = Object.assign(
        { yaw: -0.4, pitch: 0.1, distance: 2.6, autoRotate: true }, o.cam || {});
      this.zoom = Object.assign({ min: 1.4, max: 8 }, o.zoom || {});
      this.cam = Object.assign({}, this.defaultCam);

      this.indexCount = 0;
      this.center = [0, 0, 0];
      this.radius = 1;

      this._basePos = null;   /* pristine Float32Array(vertices * 3) */
      this._objIds = null;    /* Uint32Array, per-vertex object index */
      this.anim = null;
      /* After-image trail config: { tint: [r,g,b], echoes: [{delay, alpha}] }
       * or null. Echo poses are re-derived per frame from the clip itself
       * (pose(frame - delay)), so no ring buffer of copies is needed. */
      this.trail = null;
      this._ghostBufs = [];   /* per-echo Float32Array scratch poses */
      this._frameOverride = -1;
      this.playing = false;
      /* Playback-rate multiplier over the clip's own fps (1 = retail rate). */
      this.speed = 1;
      this._startMs = 0;
      this._lastPosedFrame = -1;
      /* Smooth (interpolated) playback vs retail stepping; see the header. */
      this.smooth = readSmoothPref();
      this._lastSample = null;   /* the last sampled cursor, to skip re-posing */
      VIEWS.add(this);

      this._loop = this._loop.bind(this);
      this._raf = 0;
      this._attachControls();
      this._raf = requestAnimationFrame(this._loop);
    }

    uploadVram(vramBytes) {
      this._vram = vramBytes;
      this.renderer.uploadVram(vramBytes);
    }

    /* `flatRgba` (optional) carries per-vertex [r, g, b, textured_flag] so the
     * untextured flat/gouraud prims render in their real colours; omit it to
     * leave the renderer in pure-textured mode.
     *
     * The buffers are retained (not just uploaded) so `exportGlb` can hand the
     * exact same geometry to the WASM glTF baker. */
    setMesh(positions, uvs8, cbaTsb, indices, bounds, objectIds, flatRgba) {
      this.renderer.uploadMesh(positions, uvs8, cbaTsb, indices, flatRgba || null);
      this.indexCount = indices.length;
      this.center = [bounds[0], bounds[1], bounds[2]];
      this.radius = bounds[3] || 1;
      this._basePos = positions;
      this._objIds = objectIds || null;
      this._uvs = uvs8;
      this._cbaTsb = cbaTsb;
      this._indices = indices;
      this._flat = flatRgba || null;
      this.anim = null;
      this.playing = false;
      this._frameOverride = -1;
      this.renderer.ghostTrail = null;
      this._ghostBufs = [];
    }

    /* Bake the model currently on screen into a textured binary glTF, through
     * the WASM scene exporter (which renders every (CLUT, texpage) pair the
     * mesh samples into one atlas and remaps the UVs).
     *
     * Exports the **posed** vertices when a clip is loaded - i.e. the assembled
     * figure at the frame being shown - rather than the raw object-local parts,
     * which would arrive in the file as a heap at the origin. Returns an empty
     * Uint8Array when there's nothing to export (or the WASM build predates the
     * exporter). */
    exportGlb(viewer, name) {
      if (!this._basePos || !this._indices || !this._indices.length) return new Uint8Array(0);
      if (!viewer || typeof viewer.scene_export_begin !== 'function') return new Uint8Array(0);
      const positions = this.anim ? this.anim.out : this._basePos;
      const none = new Uint8Array(0);
      viewer.scene_export_begin(name);
      viewer.scene_export_set_vram(this._vram || none);
      const mi = viewer.scene_export_add_mesh(
        name, positions, this._uvs, this._cbaTsb, this._indices, this._flat || none);
      viewer.scene_export_add_instance(mi, 0, 0, 0, 0, 1.0);
      return viewer.scene_export_finish() || new Uint8Array(0);
    }

    /* `meta`: { partCount, frameCount, frames } where `frames` is an
     * Int32Array(frameCount * partCount * 6) of absolute per-(frame, bone)
     * [tx, ty, tz, rx, ry, rz]. Pass null / a 1-frame clip to drop back to the
     * mesh's own rest geometry. Optional `meta.fps` overrides the display
     * default - clips that carry a retail rate byte play at 7.5 * rate
     * (FUN_80047430 advances rate/8 keyframes per 60 Hz tick). */
    setAnimation(meta) {
      if (!meta || !meta.frameCount || meta.frameCount < 1 ||
          !this._objIds || !this._basePos) {
        this.anim = null;
        this.playing = false;
        this.renderer.ghostTrail = null;
        if (this._basePos) this.renderer.updatePositions(this._basePos);
        return;
      }
      if (this.anim && this.anim.sampler) this.anim.sampler.free();
      this.anim = {
        out: new Float32Array(this._basePos),
        partCount: meta.partCount,
        frameCount: meta.frameCount,
        frames: meta.frames,
        fps: meta.fps || ANIM_FPS,
        /* Vsyncs per battle frame the retail cursor steps on. */
        retailStep: Math.max(1, meta.retailStep | 0 || 2),
        sampler: null,
        parts: new Float32Array(meta.partCount * 6),
        /* Opt-out: `loop: false` plays the clip once and holds its last pose,
         * firing `onEnd` once. A looping clip (the default) has no end. */
        loop: meta.loop !== false,
        onEnd: typeof meta.onEnd === 'function' ? meta.onEnd : null,
        ended: false,
        /* Opt-in: fit the camera over EVERY frame of the clip rather than
         * frame 0 only - action clips (battle arts) translate the actor far
         * from the rest stance mid-clip. */
        fitAll: !!meta.fitAll,
      };
      this._startMs = performance.now();
      this._frameOverride = 0;
      this._lastPosedFrame = -1;
      this._lastSample = null;
      this._poseAt(0);
      this._refit();
      this._attachSampler(this.anim);
    }

    /* Build the WASM sampler for clip `A` once the module is in hand (the
     * pages that use this view have already loaded it, so this resolves on
     * the next microtask). Until then, and on a bundle without it, the view
     * steps whole keyframes. */
    _attachSampler(A) {
      if (!window.LegaiaWasm || A.frameCount < 2) return;
      window.LegaiaWasm.load().then((mod) => {
        if (this.anim !== A || typeof mod.PoseSampler !== 'function') return;
        try {
          A.sampler = new mod.PoseSampler(Int32Array.from(A.frames), A.partCount, A.frameCount);
        } catch (e) {
          console.warn('mesh-view: no pose sampler', e);
        }
        this._lastSample = null;
      }).catch(() => {});
    }

    /* Smooth (interpolated) playback on / off for this view; the page's
     * toggle goes through `MeshView.setSmoothAll`. */
    setSmooth(on) {
      this.smooth = !!on;
      this._lastSample = null;
    }

    /* Re-fit the camera on the assembled pose. Frame 0 of a clip is the
     * actor's rest stance, and it can sit far from the raw TMD's centroid.
     * With `fitAll` the bounds accumulate across the whole clip (each frame
     * posed once, without uploading), so a flip / lunge stays in frame. */
    _refit() {
      if (!this.anim) return;
      const ids = this._objIds, pc = this.anim.partCount;
      const lo = [Infinity, Infinity, Infinity];
      const hi = [-Infinity, -Infinity, -Infinity];
      const accum = () => {
        const o = this.anim.out, n = o.length / 3;
        for (let v = 0; v < n; v++) {
          if (ids[v] >= pc) continue;
          for (let k = 0; k < 3; k++) {
            const x = o[v * 3 + k];
            if (x < lo[k]) lo[k] = x;
            if (x > hi[k]) hi[k] = x;
          }
        }
      };
      if (this.anim.fitAll && this.anim.frameCount > 1) {
        for (let f = 0; f < this.anim.frameCount; f++) {
          this._poseAt(f, false);
          accum();
        }
        this._poseAt(0);
      } else {
        accum();
      }
      if (lo[0] === Infinity) return;
      this.center = [(lo[0]+hi[0])/2, (lo[1]+hi[1])/2, (lo[2]+hi[2])/2];
      this.radius = Math.max(1, Math.hypot(
        (hi[0]-lo[0])/2, (hi[1]-lo[1])/2, (hi[2]-lo[2])/2));
    }

    /* Effective clip rate in frames per second (clip fps x speed). */
    _rate() {
      return (this.anim && this.anim.fps || ANIM_FPS) * (this.speed || 1);
    }

    setPlaying(on) {
      if (!this.anim) { this.playing = false; return; }
      this.playing = !!on;
      if (this.playing) {
        /* Resume from the scrubbed frame (or the last posed one), not 0. */
        const from = this._frameOverride >= 0 ? this._frameOverride
          : (this._lastPosedFrame >= 0 ? this._lastPosedFrame : 0);
        this._startMs = performance.now() - (from / this._rate()) * 1000;
        this._frameOverride = -1;
      }
    }

    setFrame(f) {
      if (!this.anim) return;
      this._frameOverride = f | 0;
      this.playing = false;
      this._lastPosedFrame = this._frameOverride;
      this._poseAt(this._frameOverride);
    }

    /* Current display frame: the scrubbed frame while paused, else the last
     * frame the loop posed (-1 before any pose). */
    currentFrame() {
      return this._frameOverride >= 0 ? this._frameOverride : this._lastPosedFrame;
    }

    /* Pause and move `delta` frames (wrapping). */
    step(delta) {
      if (!this.anim) return;
      const n = this.anim.frameCount;
      const cur = Math.max(0, this.currentFrame());
      this.setFrame((((cur + (delta | 0)) % n) + n) % n);
    }

    /* Change the playback multiplier without a visible jump: the clock is
     * rebased so the frame on screen stays where it is. */
    setSpeed(mult) {
      const m = Number(mult) > 0 ? Number(mult) : 1;
      if (m === this.speed) return;
      const cur = Math.max(0, this.currentFrame());
      this.speed = m;
      if (this.playing) this._startMs = performance.now() - (cur / this._rate()) * 1000;
    }

    /* Pose frame `frameIdx` into A.out. `upload` (default true) pushes the
     * posed positions to the GPU; _refit's fitAll sweep poses without it. */
    _poseAt(frameIdx, upload) {
      const A = this.anim;
      if (!A) return;
      this._poseInto(frameIdx, A.out);
      if (upload !== false) {
        this.renderer.updatePositions(A.out);
        this._updateTrail(frameIdx);
      }
    }

    /* Pose frame `frameIdx` of the current clip into `out` (a
     * Float32Array the size of the base positions), without uploading. */
    _poseInto(frameIdx, out) {
      const A = this.anim;
      if (!A) return;
      const ff = ((frameIdx % A.frameCount) + A.frameCount) % A.frameCount;
      this._poseParts(A.frames, ff * A.partCount * 6, out);
    }

    /* Pose the mesh from per-part `[tx, ty, tz, rx, ry, rz]` rows starting
     * at `src[at]` (a keyframe of the clip, or a sampler's blended row) into
     * `out`. */
    _poseParts(src, at, out) {
      const A = this.anim;
      const base = this._basePos;
      const ids = this._objIds, n = base.length / 3;
      /* Precompute each bone's sin/cos + translation once per frame. */
      const sin = new Float32Array(A.partCount * 3);
      const cos = new Float32Array(A.partCount * 3);
      const tr = new Float32Array(A.partCount * 3);
      const f = src;
      for (let p = 0; p < A.partCount; p++) {
        const o = at + p * 6;
        for (let k = 0; k < 3; k++) {
          const a = f[o + 3 + k] * A2R;
          sin[p*3+k] = Math.sin(a);
          cos[p*3+k] = Math.cos(a);
          tr[p*3+k] = f[o + k];
        }
      }
      for (let v = 0; v < n; v++) {
        const o = ids[v];
        if (o >= A.partCount) {
          /* A vertex whose object the clip doesn't cover has no joint to hang
           * from; collapse it rather than leave it floating in object space. */
          out[v*3] = 0; out[v*3+1] = 0; out[v*3+2] = 0;
          continue;
        }
        const sx = sin[o*3],   cxx = cos[o*3];
        const sy = sin[o*3+1], cyy = cos[o*3+1];
        const sz = sin[o*3+2], czz = cos[o*3+2];
        let x = base[v*3], y = base[v*3+1], z = base[v*3+2];
        /* Rz . Ry . Rx . v + T */
        let ny = y*cxx - z*sx, nz = y*sx + z*cxx; y = ny; z = nz;
        let nx = x*cyy + z*sy;   nz = -x*sy + z*cyy; x = nx; z = nz;
        nx = x*czz - y*sz;       ny = x*sz + y*czz;  x = nx; y = ny;
        out[v*3]   = x + tr[o*3];
        out[v*3+1] = y + tr[o*3+1];
        out[v*3+2] = z + tr[o*3+2];
      }
    }

    /* Configure the after-image trail: `{ tint: [r, g, b], echoes:
     * [{ delay, alpha }, ...] }` (delays in clip keyframes) or null to turn
     * it off. Takes effect on the next posed frame. */
    setTrail(opts) {
      this.trail = opts || null;
      if (!this.trail) this.renderer.ghostTrail = null;
    }

    /* Rebuild the renderer's ghost passes for display frame `frameIdx`: one
     * delayed pose per configured echo. Echoes before the clip's first frame
     * are skipped, so a trail "grows in" at the start instead of wrapping a
     * stale pose from the clip's tail. */
    _updateTrail(frameIdx) {
      const T = this.trail, A = this.anim;
      if (!T || !A || !T.echoes || !T.echoes.length) {
        this.renderer.ghostTrail = null;
        return;
      }
      const passes = [];
      for (let i = 0; i < T.echoes.length; i++) {
        const e = T.echoes[i];
        const gf = frameIdx - e.delay;
        if (gf < 0) continue;
        let buf = this._ghostBufs[i];
        if (!buf || buf.length !== this._basePos.length) {
          buf = new Float32Array(this._basePos.length);
          this._ghostBufs[i] = buf;
        }
        this._poseInto(gf, buf);
        passes.push({ positions: buf, tint: T.tint, alpha: e.alpha });
      }
      this.renderer.ghostTrail = passes.length
        ? { passes, restore: A.out }
        : null;
    }

    setSpin(on) {
      this.cam.autoRotate = !!on;
      if (this.onSpin) this.onSpin(this.cam.autoRotate);
    }

    resetView() {
      this.cam = Object.assign({}, this.defaultCam);
      if (this.onSpin) this.onSpin(this.cam.autoRotate);
    }

    _attachControls() {
      const c = this.canvas;
      let dragging = false, lastX = 0, lastY = 0;
      const onDown = (e) => {
        dragging = true; lastX = e.clientX; lastY = e.clientY;
        this.setSpin(false);
        c.setPointerCapture(e.pointerId);
      };
      const onUp = (e) => {
        dragging = false;
        c.releasePointerCapture(e.pointerId);
      };
      const onMove = (e) => {
        if (!dragging) return;
        this.cam.yaw   -= (e.clientX - lastX) * 0.008;
        this.cam.pitch -= (e.clientY - lastY) * 0.008;
        const lim = Math.PI / 2 - 0.05;
        this.cam.pitch = Math.max(-lim, Math.min(lim, this.cam.pitch));
        lastX = e.clientX; lastY = e.clientY;
      };
      const onWheel = (e) => {
        e.preventDefault();
        this.cam.distance *= e.deltaY > 0 ? 1.1 : 0.9;
        this.cam.distance = Math.max(this.zoom.min, Math.min(this.zoom.max, this.cam.distance));
      };
      const onDbl = () => this.resetView();
      c.addEventListener('pointerdown', onDown);
      c.addEventListener('pointerup', onUp);
      c.addEventListener('pointermove', onMove);
      c.addEventListener('wheel', onWheel, { passive: false });
      c.addEventListener('dblclick', onDbl);
    }

    /* Stop drawing while the page shows another view (no RAF, no GPU work)
     * and resume where the camera was. */
    setSuspended(on) {
      const s = !!on;
      if (s === !!this._suspended) return;
      this._suspended = s;
      if (s) {
        if (this._raf) cancelAnimationFrame(this._raf);
        this._raf = 0;
      } else if (!this._raf) {
        this._raf = requestAnimationFrame(this._loop);
      }
    }

    _loop() {
      const dpr = window.devicePixelRatio || 1;
      const w = (this.canvas.clientWidth || 640) * dpr;
      const h = (this.canvas.clientHeight || 480) * dpr;
      if (this.canvas.width !== w || this.canvas.height !== h) {
        this.canvas.width = w;
        this.canvas.height = h;
      }
      if (this.cam.autoRotate) this.cam.yaw += 0.006;
      if (this.anim && this.playing && this.anim.frameCount > 1) {
        this._advance((performance.now() - this._startMs) / 1000);
      }
      if (this._suspended) { this._raf = 0; return; }
      /* Orbit UX: drag rotates rather than pans, so panX/panY stay 0. */
      this.renderer.render(
        this.cam.yaw, this.cam.pitch, this.cam.distance,
        0, 0, this.center, this.radius);
      this._raf = requestAnimationFrame(this._loop);
    }

    /* Pose the clip `secs` seconds into playback. With a sampler the pose is
     * the blend at the smooth position, or at the retail cursor; without one,
     * the whole keyframe. `onFrame` fires once per keyframe either way. */
    _advance(secs) {
      const A = this.anim;
      const last = A.frameCount - 1;
      const raw = secs * this._rate();
      /* A one-shot stops on the last keyframe: never blends back to frame 0. */
      const t = A.loop ? raw : Math.min(raw, last);
      const f = A.loop ? Math.floor(t) % A.frameCount : Math.floor(t);
      if (A.sampler) {
        let key;
        if (this.smooth) {
          key = t;
          A.sampler.sample_smooth(t, A.parts);
        } else {
          /* The retail cursor: whole battle frames of vsyncs, each moving it
           * the clip rate's sixteenths (15 * rate keyframes a second at the
           * normal rate is 4 * rate sixteenths a vsync). */
          const vs = Math.floor(secs * VSYNC_HZ * (this.speed || 1) + 1e-6);
          const gf = vs - (vs % A.retailStep);
          key = Math.floor(gf * (A.fps * 16) / VSYNC_HZ + 1e-6);
          if (!A.loop) key = Math.min(key, last * 16);
          if (key === this._lastSample) return;
          A.sampler.sample_retail(key, A.parts);
        }
        this._lastSample = key;
        this._poseParts(A.parts, 0, A.out);
        this.renderer.updatePositions(A.out);
        this._updateTrail(f);
        if (typeof this.onPose === 'function') this.onPose();
      } else if (f !== this._lastPosedFrame) {
        this._poseAt(f);
        if (typeof this.onPose === 'function') this.onPose();
      }
      if (f !== this._lastPosedFrame) {
        this._lastPosedFrame = f;
        if (typeof this.onFrame === 'function') this.onFrame(f);
      }
      if (!A.loop && raw >= last && !A.ended) {
        A.ended = true;
        this.playing = false;
        this._frameOverride = last;
        if (A.onEnd) A.onEnd();
      }
    }

    /* Draw the current model once at the default camera (or `yaw`) and copy
     * the frame into a 2D `target` canvas - a posed still, so a page with
     * several models can show them all from one WebGL context. The copy runs
     * in the same task as the draw, while the drawing buffer is still valid. */
    snapshot(target, yaw) {
      if (!target || !this.indexCount) return false;
      const c = this.canvas;
      const w = target.width, h = target.height;
      if (c.width !== w || c.height !== h) { c.width = w; c.height = h; }
      this.renderer.render(
        yaw != null ? yaw : this.defaultCam.yaw, this.defaultCam.pitch,
        this.defaultCam.distance, 0, 0, this.center, this.radius);
      const ctx = target.getContext('2d');
      ctx.clearRect(0, 0, w, h);
      ctx.drawImage(c, 0, 0, w, h);
      return true;
    }

    dispose() {
      if (this._raf) cancelAnimationFrame(this._raf);
      this._raf = 0;
      VIEWS.delete(this);
      if (this.anim && this.anim.sampler) this.anim.sampler.free();
    }
  }

  /* The visitor's smooth-playback preference (default on). */
  MeshView.smoothPref = readSmoothPref;

  /* Flip smooth playback on every live view and remember the choice. */
  MeshView.setSmoothAll = function (on) {
    try { window.localStorage.setItem(SMOOTH_KEY, on ? '1' : '0'); } catch (e) { /* private mode */ }
    VIEWS.forEach((v) => v.setSmooth(on));
  };

  /* Wire a checkbox as the page's "smooth motion" toggle: checked = the
   * interpolated enhancement, unchecked = retail stepping. */
  MeshView.attachSmoothToggle = function (box) {
    if (!box) return;
    box.checked = readSmoothPref();
    box.addEventListener('change', () => MeshView.setSmoothAll(box.checked));
  };

  /* Wire a button to "bake the model on screen into a .glb and download it".
   * `getName()` supplies the filename stem (and the glTF node name); it's a
   * callback because the selected model changes under the button.
   *
   * The bake is synchronous inside WASM, so the label is repainted first -
   * otherwise a large model freezes the button mid-click with no feedback. */
  MeshView.attachGlbButton = function (btn, getView, getViewer, getName) {
    if (!btn) return;
    btn.addEventListener('click', async () => {
      const view = getView();
      const viewer = getViewer();
      if (!view || !viewer) return;
      const prev = btn.textContent;
      btn.disabled = true;
      btn.textContent = 'baking…';
      await new Promise(r => setTimeout(r, 30));
      let msg = null;
      try {
        const name = getName() || 'model';
        const glb = view.exportGlb(viewer, name);
        if (!glb || glb.length === 0) {
          msg = 'no mesh';
        } else {
          const url = URL.createObjectURL(new Blob([glb], { type: 'model/gltf-binary' }));
          const a = document.createElement('a');
          a.href = url;
          a.download = `${name}.glb`;
          a.click();
          setTimeout(() => URL.revokeObjectURL(url), 5000);
        }
      } catch (err) {
        console.warn('glb export failed:', err);
        msg = 'export failed';
      }
      btn.textContent = msg || prev;
      btn.disabled = false;
      if (msg) setTimeout(() => { btn.textContent = prev; }, 1500);
    });
  };

  window.MeshView = MeshView;
})();
