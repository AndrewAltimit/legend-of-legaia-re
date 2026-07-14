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
  /* VR world scale - same anchor as the full-map view: the ~130-unit character
   * mesh is a 1.7 m human, so a metre is ~76 world units and the headset stands
   * in the town at human height. See docs/subsystems/vr-mode.md. */
  const VR_UNITS_PER_METER = 76;

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

  /* ---------- occluder cull (see the note in `_frame`) ---------- */

  /* Drop a body that sits between the lens and the player. This is an OCCLUSION
   * test, never a distance / budget one - set it to `false` and every body a
   * scene loads is drawn every frame, unconditionally. */
  const OCCLUDER_CULL = true;

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
      /* Last frame's draw list + world extent, kept on the instance so the VR
       * loop can re-issue the same draw once per eye without re-ticking the
       * engine. */
      this._draws = [];
      this._ext = [16384, 16384];
      this._attachInput();

      /* VR: walk the live scene in a headset. The engine keeps ticking (the XR
       * frame loop drives it), so NPCs move and the keyboard still steers the
       * character; the headset is a free-flying camera in the running world.
       * Button stays hidden unless an immersive-vr device is reported. */
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
        draw: () => this.renderer.renderAssembled(this._draws, this._ext, this.cam),
        /* Spawn where the third-person camera sits (behind the character,
         * looking at them), feet on the character's floor. */
        start: () => {
          const eye = this._eye();
          const pt = this.rt.player_transform();
          return {
            x: eye[0], y: -pt[1], z: eye[2],
            yaw: Math.PI - this.cam.yaw,
          };
        },
        onEnter: () => this.stop(),
        onExit: () => this.start(),
      }) : null;
    }

    /* ---------- scene ---------- */

    /* Boot a CDNAME scene through the engine and (re)build everything drawn.
     * Throws the engine's error message when the label doesn't resolve. */
    enter(label) {
      const state = JSON.parse(this.rt.enter_field(label));
      this._rebuild();
      this.scene = state.scene || label;
      if (this.vr) {
        this.vr.setReady(true);
        /* A live session survives a scene swap (same canvas / GL context) - just
         * re-place the viewer in the new map. */
        this.vr.respawn();
      }
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
      const isSky = (window.FieldSceneView && window.FieldSceneView.isSkyMesh)
        || (() => false);
      const push = (slots, pos, rots, anims) => {
        for (let i = 0; i < slots.length; i++) {
          const meshId = ensure(slots[i], anims ? anims[i] : 0);
          if (meshId < 0) continue;
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

    /* One engine frame + one draw (the draw is skipped while VR presents). */
    _frame(skipDraw) {
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
          if (this.vr) this.vr.respawn();
          if (this.opts.onScene) this.opts.onScene(entered);
        }
      }

      /* Occluder cull - EXACT, and off by default.
       *
       * Nothing is culled for distance / budget reasons: every body a scene
       * loads stays resident and drawn. The only thing this page ever wants to
       * drop is a body that literally sits between the lens and the player (a
       * cave roof, a house's upper storey), because the page has one follow
       * camera where retail had per-scene authored ones.
       *
       * The old test bounded each body by a SPHERE of its longest half-extent
       * and dropped it whenever that sphere came near the camera-to-player
       * line. A floor slab or a staircase is wide and thin, so its sphere is
       * enormous (a 1024-unit-wide floor gets a 512-unit sphere) - the line
       * swept into those spheres constantly while walking and whole floors,
       * stairs and wall sections blinked out. It also placed the body's centre
       * at `d.x - a.cx` (the local centroid *subtracted*, un-rotated), so the
       * sphere often wasn't even where the body was.
       *
       * The test is now an exact segment-vs-world-AABB (slab) intersection
       * against the body's real box (`d.box`, baked once per placement in
       * `_rebuild`), and only the stretch of the segment strictly between the
       * lens and the player counts. A body you stand on, walk past, or climb
       * cannot satisfy it; only one the camera genuinely looks through does.
       * `OCCLUDER_CULL = false` disables even that and draws every body,
       * always. */
      const pt = rt.player_transform();
      let draws;
      if (OCCLUDER_CULL) {
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
      this._draws = draws;
      /* `skipDraw`: a VR session owns the framebuffer and re-issues this draw
       * once per eye with the XR view matrices. */
      if (!skipDraw) this.renderer.renderAssembled(this._draws, this._ext, this.cam);

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
      if (this.vr) { this.vr.destroy(); this.vr = null; }
      window.removeEventListener('keydown', this._onDown);
      window.removeEventListener('keyup', this._onUp);
      window.removeEventListener('blur', this._onBlur);
      if (this.renderer) { this.renderer.dispose(); this.renderer = null; }
    }
  }

  window.PlayView = PlayView;
})();
