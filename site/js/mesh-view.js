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
 */
(function () {
  'use strict';

  /* Retail clips run at ~14 fps. */
  const ANIM_FPS = 14;
  /* PSX angle units -> radians (4096 = one revolution). */
  const A2R = (Math.PI * 2) / 4096;

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
      this.onFrame = null;
      this.defaultCam = Object.assign(
        { yaw: 0.4, pitch: 0.1, distance: 2.6, autoRotate: true }, o.cam || {});
      this.zoom = Object.assign({ min: 1.4, max: 8 }, o.zoom || {});
      this.cam = Object.assign({}, this.defaultCam);

      this.indexCount = 0;
      this.center = [0, 0, 0];
      this.radius = 1;

      this._basePos = null;   /* pristine Float32Array(vertices * 3) */
      this._objIds = null;    /* Uint32Array, per-vertex object index */
      this.anim = null;
      this._frameOverride = -1;
      this.playing = false;
      this._startMs = 0;
      this._lastPosedFrame = -1;

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
     * mesh's own rest geometry. */
    setAnimation(meta) {
      if (!meta || !meta.frameCount || meta.frameCount < 1 ||
          !this._objIds || !this._basePos) {
        this.anim = null;
        this.playing = false;
        if (this._basePos) this.renderer.updatePositions(this._basePos);
        return;
      }
      this.anim = {
        out: new Float32Array(this._basePos),
        partCount: meta.partCount,
        frameCount: meta.frameCount,
        frames: meta.frames,
      };
      this._startMs = performance.now();
      this._frameOverride = 0;
      this._lastPosedFrame = -1;
      this._poseAt(0);
      this._refit();
    }

    /* Re-fit the camera on the assembled pose. Frame 0 of a clip is the
     * actor's rest stance, and it can sit far from the raw TMD's centroid. */
    _refit() {
      if (!this.anim) return;
      const o = this.anim.out, n = o.length / 3;
      const ids = this._objIds, pc = this.anim.partCount;
      const lo = [Infinity, Infinity, Infinity];
      const hi = [-Infinity, -Infinity, -Infinity];
      for (let v = 0; v < n; v++) {
        if (ids[v] >= pc) continue;
        for (let k = 0; k < 3; k++) {
          const x = o[v * 3 + k];
          if (x < lo[k]) lo[k] = x;
          if (x > hi[k]) hi[k] = x;
        }
      }
      if (lo[0] === Infinity) return;
      this.center = [(lo[0]+hi[0])/2, (lo[1]+hi[1])/2, (lo[2]+hi[2])/2];
      this.radius = Math.max(1, Math.hypot(
        (hi[0]-lo[0])/2, (hi[1]-lo[1])/2, (hi[2]-lo[2])/2));
    }

    setPlaying(on) {
      if (!this.anim) { this.playing = false; return; }
      this.playing = !!on;
      if (this.playing) {
        this._startMs = performance.now() - (this._frameOverride >= 0
          ? (this._frameOverride / ANIM_FPS) * 1000
          : 0);
        this._frameOverride = -1;
      }
    }

    setFrame(f) {
      if (!this.anim) return;
      this._frameOverride = f | 0;
      this.playing = false;
      this._poseAt(this._frameOverride);
    }

    _poseAt(frameIdx) {
      const A = this.anim;
      if (!A) return;
      const ff = ((frameIdx % A.frameCount) + A.frameCount) % A.frameCount;
      const base = this._basePos;
      const out = A.out;
      const ids = this._objIds, n = base.length / 3;
      /* Precompute each bone's sin/cos + translation once per frame. */
      const sin = new Float32Array(A.partCount * 3);
      const cos = new Float32Array(A.partCount * 3);
      const tr = new Float32Array(A.partCount * 3);
      const f = A.frames;
      for (let p = 0; p < A.partCount; p++) {
        const o = (ff * A.partCount + p) * 6;
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
      this.renderer.updatePositions(out);
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
        const t = (performance.now() - this._startMs) / 1000 * ANIM_FPS;
        const f = Math.floor(t) % this.anim.frameCount;
        if (f !== this._lastPosedFrame) {
          this._poseAt(f);
          this._lastPosedFrame = f;
          if (typeof this.onFrame === 'function') this.onFrame(f);
        }
      }
      /* Orbit UX: drag rotates rather than pans, so panX/panY stay 0. */
      this.renderer.render(
        this.cam.yaw, this.cam.pitch, this.cam.distance,
        0, 0, this.center, this.radius);
      this._raf = requestAnimationFrame(this._loop);
    }

    dispose() {
      if (this._raf) cancelAnimationFrame(this._raf);
      this._raf = 0;
    }
  }

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
