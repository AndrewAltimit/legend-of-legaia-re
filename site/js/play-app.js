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
  const NPC_CLIP_FPS   = 15;          /* the field anim cadence (30 Hz tick, 2 ticks/frame) */

  /* Keyboard -> PSX pad bits (`legaia_engine_core::input::PadButton`). */
  const PAD = {
    ArrowUp: 0x0010, ArrowRight: 0x0020, ArrowDown: 0x0040, ArrowLeft: 0x0080,
    KeyW: 0x0010, KeyD: 0x0020, KeyS: 0x0040, KeyA: 0x0080,
    KeyZ: 0x4000,   /* Cross    - talk / confirm / advance */
    KeyX: 0x2000,   /* Circle   - cancel */
    KeyC: 0x1000,   /* Triangle */
    KeyV: 0x8000,   /* Square   */
    Enter: 0x0008,  /* Start    */
    Space: 0x4000,  /* Cross (second binding - the natural "talk" key) */
  };

  /* Keys the canvas swallows so the page doesn't scroll under the player. */
  const SWALLOW = new Set(Object.keys(PAD).concat(['Space', 'ArrowUp', 'ArrowDown',
    'ArrowLeft', 'ArrowRight']));

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
      this._attachInput();
    }

    /* ---------- scene ---------- */

    /* Boot a CDNAME scene through the engine and (re)build everything drawn.
     * Throws the engine's error message when the label doesn't resolve. */
    enter(label) {
      const state = JSON.parse(this.rt.enter_field(label));
      this._rebuild();
      this.scene = state.scene || label;
      return state;
    }

    /* Rebuild the GPU-side scene from whatever the engine currently holds.
     * Runs on entry and whenever the engine walks through a door. */
    _rebuild() {
      const rt = this.rt;
      this.renderer.clearScene();
      this.staticDraws = [];
      this.player = null;
      this.npcs = [];

      this.renderer.uploadVram(rt.field_vram_bytes());

      if (rt.field_ground_quad_count() > 0) {
        this.renderer.uploadGround(
          rt.field_ground_positions(), rt.field_ground_uvs(),
          rt.field_ground_cba_tsb(), rt.field_ground_indices());
      } else {
        this.renderer.uploadGround(new Float32Array(0), null, null, new Uint32Array(0));
      }

      /* Environment meshes, uploaded once each and instanced per placement. */
      const empty = new Set(), used = new Set();
      const ensure = (slot) => {
        if (used.has(slot)) return true;
        if (empty.has(slot)) return false;
        try { rt.field_mesh(slot); } catch (e) { empty.add(slot); return false; }
        const pos = rt.field_mesh_positions();
        const idx = rt.field_mesh_indices();
        if (!pos.length || !idx.length) { empty.add(slot); return false; }
        const flat = rt.field_mesh_flat_rgba();
        this.renderer.uploadSceneMesh(slot, pos, rt.field_mesh_uvs(),
          rt.field_mesh_cba_tsb(), idx, flat.length ? flat : null);
        used.add(slot);
        return true;
      };
      const isSky = (window.FieldSceneView && window.FieldSceneView.isSkyMesh)
        || (() => false);
      const push = (slots, pos, rots) => {
        for (let i = 0; i < slots.length; i++) {
          const s = slots[i];
          if (!ensure(s)) continue;
          /* Sky domes and kilometre-wide horizon planes read as sky only from
           * the retail in-world camera; from a follow camera inside them they
           * are a wall in front of the lens. Same classifier the full-map view
           * uses. */
          if (isSky(this.renderer.getMeshAabb(s))) continue;
          this.staticDraws.push({
            meshId: s,
            x: pos[i * 3], y: -pos[i * 3 + 1], z: pos[i * 3 + 2],
            rotY: -(rots[i] & 0xFFF) * A2R,
            scale: 1.0,
          });
        }
      };
      push(rt.field_terrain_slots(), rt.field_terrain_positions(), rt.field_terrain_rot_y());
      push(rt.field_placement_slots(), rt.field_placement_positions(), rt.field_placement_rot_y());

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

      /* NPCs: the scene's MAN placements. A conditional spawn (parked at the
       * off-map hide box) is one retail withholds until a script places it -
       * skip it, exactly as the field renderer does. */
      const cat = JSON.parse(rt.play_npc_catalog_json() || 'null');
      if (cat) {
        for (const npc of cat.npcs) {
          if (npc.conditional) continue;
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
            i: npc.i, meshId, base, objectIds: rt.play_npc_mesh_object_ids(),
            frames, frameCount: dims[0], partCount: dims[1],
            out: new Float32Array(base.length), lastFrame: -1,
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

    /* ---------- input ---------- */

    _attachInput() {
      const onKey = (e, down) => {
        if (!this.canvas.matches(':focus-within') && document.activeElement !== this.canvas) return;
        const bit = PAD[e.code];
        if (bit === undefined) return;
        if (SWALLOW.has(e.code)) e.preventDefault();
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
      let mask = 0;
      for (const k of this.held) mask |= (PAD[k] || 0);
      for (const k of this.pulse) mask |= (PAD[k] || 0);
      this.pad = mask;
    }

    /* Held-key state, for the on-screen control legend. */
    heldKeys() { return Array.from(this.held); }

    /* ---------- loop ---------- */

    start() {
      if (this.raf) return;
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

    /* One engine frame + one draw. */
    _frame() {
      const rt = this.rt;
      const advance = !this.paused || this.stepOnce;
      this.stepOnce = false;

      if (advance) {
        rt.set_camera_azimuth(azimuthUnits(this.cam.yaw));
        rt.set_pad(this.pad);
        /* The latched press edges have now been seen by exactly one tick. */
        this.pulse.clear();
        this._repack();
        let entered = '';
        try { entered = rt.tick_frame(); } catch (e) {
          console.warn('engine tick', e);
          this.stop();
          if (this.opts.onError) this.opts.onError(e.message || String(e));
          return;
        }
        if (entered) {
          /* The engine walked through a door: its scene swapped under us, so the
           * geometry has to swap too. */
          this.scene = entered;
          this._rebuild();
          if (this.opts.onScene) this.opts.onScene(entered);
        }
      }

      /* Occluder cull. Retail frames every scene with a per-scene authored
       * camera; this page has one follow camera, so a cave's roof or a house's
       * upper storey ends up between the lens and the player and fills the
       * screen. Anything whose body straddles the camera-to-player line is
       * dropped for the frame - the standard third-person fix, and the only way
       * an interior is playable without porting the authored cameras.
       *
       * The test is a sphere-vs-segment one against each mesh's AABB (already
       * on the GPU record). Mesh-local Y is PSX-down and the draw flips it, so a
       * body's world-up centre is `draw.y - cy`. */
      const pt = rt.player_transform();
      const eye = this._eye();
      const px = pt[0], py = -pt[1] + 90, pz = pt[2];
      const ex = eye[0] - px, ey = eye[1] - py, ez = eye[2] - pz;
      const segLen2 = ex * ex + ey * ey + ez * ez || 1;
      const draws = this.staticDraws.filter(d => {
        const a = this.renderer.getMeshAabb(d.meshId);
        if (!a) return true;
        const cx = d.x - a.cx, cy = d.y - a.cy, cz = d.z - a.cz;
        /* Project the body's centre onto the camera->player segment. */
        const t = ((cx - px) * ex + (cy - py) * ey + (cz - pz) * ez) / segLen2;
        if (t <= 0.06 || t >= 0.94) return true;   /* behind the player / at the lens */
        const qx = px + ex * t, qy = py + ey * t, qz = pz + ez * t;
        const d2 = (cx - qx) ** 2 + (cy - qy) ** 2 + (cz - qz) ** 2;
        /* Conservative radius: the body's largest half-extent. Overlap this
         * loose and a distant wall would blink; too tight and the roof stays. */
        const r = Math.max(a.sx, a.sy, a.sz) * 0.5;
        return d2 > r * r;
      });

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

      /* NPCs: advance each clip and draw at the world's live position. A clip is
       * a short loop, so the pose only rebuilds when the playhead actually moves
       * to a new keyframe - not once per rendered frame. */
      const nt = rt.play_npc_transforms();
      const clipFrame = Math.floor(performance.now() / 1000 * NPC_CLIP_FPS);
      for (let k = 0; k < this.npcs.length; k++) {
        const n = this.npcs[k];
        const base = n.i * 4;
        if (base + 3 >= nt.length) continue;
        const f = n.frameCount > 1 ? clipFrame % n.frameCount : 0;
        if (advance && n.frameCount > 1 && f !== n.lastFrame) {
          poseInto(n.out, n.base, n.objectIds, n.frames, n.partCount, f);
          this.renderer.updateSceneMeshPositions(n.meshId, n.out);
          n.lastFrame = f;
        }
        draws.push({
          meshId: n.meshId,
          x: nt[base], y: -nt[base + 1], z: nt[base + 2],
          rotY: -(nt[base + 3] + 2048) * A2R,
          scale: 1.0,
        });
      }

      this._followCamera(pt);
      const ext = [16384, 16384];
      this.renderer.renderAssembled(draws, ext, this.cam);

      /* FPS + HUD, sampled twice a second. */
      this._fpsFrames++;
      const now = performance.now();
      if (now - this._fpsLast >= 500) {
        this.fps = Math.round(this._fpsFrames * 1000 / (now - this._fpsLast));
        this._fpsFrames = 0;
        this._fpsLast = now;
      }
      if (this.opts.onState) {
        try { this.opts.onState(JSON.parse(rt.state_json()), this.fps); }
        catch (e) { /* a malformed frame must not kill the loop */ }
      }
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
      window.removeEventListener('keydown', this._onDown);
      window.removeEventListener('keyup', this._onUp);
      window.removeEventListener('blur', this._onBlur);
      if (this.renderer) { this.renderer.dispose(); this.renderer = null; }
    }
  }

  window.PlayView = PlayView;
})();
