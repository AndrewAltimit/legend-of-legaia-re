/* Fishing - the retail presentation layer.
 *
 * Everything simulated here is the game's own machinery, decoded from the
 * visitor's disc at runtime by `LegaiaMinigames`
 * (crates/web-viewer/src/minigames_fishing.rs + minigames_fishing_scene.rs):
 *
 *   - the RULES are the ported retail loop (engine-core::fishing): the
 *     casting-power oscillator, the per-frame band roll and its cutoffs, the
 *     four reel-cadence gesture templates read out of the overlay rodata
 *     (a match stores its template id AS the cast band and fires the
 *     "Good!" splash), the strike-credit roll, the venue-hardwired band-4
 *     gate behind each pond's rarest fish, the spawn-table species lookup
 *     (lure row x band column), the tension tug-of-war and the catch
 *     scoring into the persistent point pool;
 *   - the 3D STAGE is the fishing venue's own field scene (the `other1`
 *     bundle: the pond, pier and shore props assembled through the shared
 *     field-scene kernel) with the lead's real field body standing on the
 *     shore, posed by his standing-idle locomotion clip (PROT 0874);
 *   - the HUD layout is the ported overlay draw list
 *     (engine-ui::ui_fishing): the persistent best/points rows, the catch
 *     readouts, the depth/tension/power gauge bars with the retail fill
 *     arithmetic and colour ramps, and the five banner animators on their
 *     traced slide/hold/fade ramps.
 *
 * Approximated (the page's note says so): the fishing sprite page (the HUD
 * glyph atlas) is undecoded, so glyph ids draw as labelled text; the retail
 * camera framing and the angler's shore anchor are fitted against the
 * scene's own bounds; the line/lure/ripple overlay is drawn as projected
 * 2D geometry (retail draws the line as projected segments too - the
 * overlay's own 3-D clip helpers - but its exact anchor points are not
 * pinned).
 */
window.MgFishing = (function () {
  'use strict';

  const SCALE = 2;                    /* retail 320x240 -> 640x480 canvas */
  const A2R = (Math.PI * 2) / 4096;   /* PSX angle units -> radians */

  /* Labels for the undecoded HUD glyph ids (the one non-retail asset - the
   * fishing sprite page - substituted by text; ids from ui_fishing). */
  const GLYPH_LABEL = {
    0x1a: 'BEST',   0x1c: 'POINTS',
    0x0b: 'LINE',   0x10: 'm',
    0x0a: 'CAST',   0x0e: 'POWER %',
    0x08: 'DEPTH',  0x09: 'TENSION',
    /* banner glyphs (the slide/hold/fade ramps are the traced ones) */
    0x07: 'HIT!!',  0x0d: 'GET!!',  0x19: 'MISS',  0x0c: '—',
    0x416: 'Good!', 0x816: '',      /* splash pair draws as one label */
  };
  const CAPTION_TEXT = {
    lure0: 'Light Lure', lure1: 'Normal Lure', lure2: 'Heavy Lure',
    lures_left: 'Lures', lure_suffix: 'left',
  };

  function create(api, canvas, glCanvas) {
    const g = canvas.getContext('2d');
    g.imageSmoothingEnabled = false;

    let scene = null;      /* the 3D stage (null = flat fallback) */
    let ripples = [];      /* {x, z, t} world-space ripple rings */
    let splashT = 0;       /* splash burst frames left */
    let jumpT = 0;         /* landed-fish jump arc frames left */

    /* ------------- the 3D stage ------------- */

    /* Fitted shore anchors, tuned against the assembled venue map. The
     * `other1` bundle carries TWO pond areas: the fenced fishing deck on
     * the grassy pond (mountains backdrop, the authored fishing platform)
     * and the rocky-shore blue pond. Which pond each venue id plays is a
     * fitted assignment - retail's spot / camera live in runtime globals
     * (see minigames_fishing_scene.rs). `yaw` spins the angler's rest
     * facing (-Z) toward the water; facing = (-sin yaw, -cos yaw). */
    const VENUE_ANCHORS = [
      { x: 4370, z: 10280, yaw: Math.PI },  /* venue 0: the fenced deck */
      { x: 6000, z: 8000, yaw: 0 },         /* venue 1: the rocky shore */
    ];

    function facingOf(anchor) {
      return [-Math.sin(anchor.yaw || 0), -Math.cos(anchor.yaw || 0)];
    }

    /* Re-anchor the angler + camera for a venue (also the initial framing). */
    function applyVenue(venue) {
      const b = scene;
      if (!b) return;
      b.venue = venue;
      const a = VENUE_ANCHORS[venue & 1];
      b.anchor = { x: a.x, z: a.z, yaw: a.yaw,
                   y: api.fishing_scene_height_at(a.x, a.z) };
      const [fx, fz] = facingOf(b.anchor);
      /* Camera behind the angler, looking with him across the water: the
       * orbit centre sits a short way out over the water so the angler stays
       * in the lower third of the frame. */
      b.defCam = { yaw: a.yaw, pitch: 0.22, distance: 0.55 };
      b.cam = Object.assign({}, b.defCam);
      b.center = [
        b.anchor.x + fx * 700,
        b.anchor.y - 350,
        b.anchor.z + fz * 700,
      ];
    }

    function build3D() {
      if (!glCanvas || !window.TmdRenderer) return null;
      if (!api.fishing_scene_ready || !api.fishing_scene_ready()) return null;
      const info = JSON.parse(api.fishing_scene_info_json());
      if (!info) return null;

      const env = {
        pos: api.fishing_scene_positions(),
        uvs: api.fishing_scene_uvs(),
        ct: api.fishing_scene_cba_tsb(),
        idx: api.fishing_scene_indices(),
        flat: api.fishing_scene_flat_rgba(),
      };
      if (!env.pos.length) return null;

      const player = info.player ? {
        pos: api.fishing_player_positions(),
        uvs: api.fishing_player_uvs(),
        ct: api.fishing_player_cba_tsb(),
        idx: api.fishing_player_indices(),
        oid: api.fishing_player_object_ids(),
        flat: api.fishing_player_flat_rgba(),
        parts: api.fishing_player_part_count(),
      } : null;
      const dims = api.fishing_player_idle_dims();
      const idle = (player && dims[0] && dims[1])
        ? { frames: api.fishing_player_idle_frames(), parts: dims[0], frameCount: dims[1], rate: 8 }
        : null;

      /* Combined buffer: [player verts][env verts]; only the player half is
       * re-posed per frame. */
      const pCount = player ? player.pos.length / 3 : 0;
      const eCount = env.pos.length / 3;
      const total = pCount + eCount;
      const pos = new Float32Array(total * 3);
      const uvs = new Uint8Array(total * 2);
      const ct = new Uint16Array(total * 2);
      const flat = new Uint8Array(total * 4);
      const idxArr = [];
      if (player) {
        pos.set(player.pos, 0);
        uvs.set(player.uvs, 0);
        ct.set(player.ct, 0);
        flat.set(player.flat, 0);
        for (const ix of player.idx) idxArr.push(ix);
      }
      pos.set(env.pos, pCount * 3);
      uvs.set(env.uvs, pCount * 2);
      ct.set(env.ct, pCount * 2);
      if (env.flat.length) flat.set(env.flat, pCount * 4);
      else flat.fill(255, pCount * 4);
      for (const ix of env.idx) idxArr.push(ix + pCount);
      const idx = new Uint32Array(idxArr);

      const renderer = new window.TmdRenderer(glCanvas);
      renderer.uploadVram(api.fishing_scene_vram());
      renderer.uploadMesh(pos, uvs, ct, idx, flat);
      /* The venue's water sheets / glow props are ABE prims; draw them
       * additively instead of as opaque slabs (same as the dance hall). */
      renderer.semiTwoPass = true;

      const [lo, hi] = info.aabb;
      const spanX = hi[0] - lo[0], spanZ = hi[2] - lo[2];
      const radius = Math.max(spanX, spanZ) * 0.28;
      const scene_ = {
        renderer,
        base: pos.slice(),
        out: pos,
        playerCount: pCount,
        parts: player ? player.parts : 0,
        oid: player ? player.oid : null,
        idle,
        cursor: 0,
        anchor: { x: 0, y: 0, z: 0, yaw: 0 },
        venue: -1,
        aabb: info.aabb,
        defCam: { yaw: 0, pitch: 0.22, distance: 0.55 },
        cam: { yaw: 0, pitch: 0.22, distance: 0.55 },
        center: [0, 0, 0],
        radius,
        fov: 0.8,
      };
      attachOrbit(scene_);
      return scene_;
    }

    function loadAssets() {
      try {
        scene = build3D();
        if (scene) applyVenue(0);
      } catch (e) {
        console.warn('MgFishing: 3D stage unavailable -', e);
        scene = null;
      }
      return !!(api.fishing_pond_ready && api.fishing_pond_ready());
    }

    /* Pose the angler at the shore anchor with his idle clip, then render. */
    function render3D(st) {
      const b = scene;
      if (!b) return;
      if (b.idle && b.oid) {
        const c = b.idle;
        const frame = Math.min(b.cursor >> 4, c.frameCount - 1);
        b.cursor += c.rate;
        if (b.cursor > c.frameCount * 16 - 1) b.cursor = 0;
        poseWorld(b.out, b.base, b.oid, c, frame, b.anchor);
        b.renderer.updatePositions(b.out);
      }
      b.renderer.render(b.cam.yaw, b.cam.pitch, b.cam.distance,
                        0, 0, b.center, b.radius, b.fov);
    }

    function attachOrbit(sc) {
      const c = glCanvas;
      let drag = false, lx = 0, ly = 0;
      c.addEventListener('pointerdown', (e) => {
        drag = true; lx = e.clientX; ly = e.clientY;
        c.setPointerCapture(e.pointerId);
      });
      c.addEventListener('pointerup', (e) => {
        drag = false; try { c.releasePointerCapture(e.pointerId); } catch (_) { /* */ }
      });
      c.addEventListener('pointermove', (e) => {
        if (!drag) return;
        sc.cam.yaw -= (e.clientX - lx) * 0.006;
        sc.cam.pitch = Math.max(-1.0, Math.min(1.2,
          sc.cam.pitch - (e.clientY - ly) * 0.006));
        lx = e.clientX; ly = e.clientY;
      });
      c.addEventListener('dblclick', () => { sc.cam = Object.assign({}, sc.defCam); });
      c.addEventListener('wheel', (e) => {
        e.preventDefault();
        sc.cam.distance = Math.max(0.4, Math.min(6,
          sc.cam.distance * (e.deltaY > 0 ? 1.1 : 0.9)));
      }, { passive: false });
    }

    /* Project a world-space point through the same MVP the 3D pass uses;
     * returns [x, y, visible] in HUD-canvas pixels. */
    function project(p) {
      const b = scene;
      if (!b) return null;
      const m = buildMvp(b.cam.yaw, b.cam.pitch, b.cam.distance, 0, 0,
                         b.center, b.radius, canvas.width, canvas.height, b.fov);
      const x = p[0], y = p[1], z = p[2];
      const cx = m[0] * x + m[4] * y + m[8] * z + m[12];
      const cy = m[1] * x + m[5] * y + m[9] * z + m[13];
      const cw = m[3] * x + m[7] * y + m[11] * z + m[15];
      if (cw <= 0.0001) return null;
      return [
        (cx / cw + 1) / 2 * canvas.width,
        (1 - (cy / cw + 1) / 2) * canvas.height,
        true,
      ];
    }

    /* The lure's world position for the live line record: out along the
     * angler's facing into the water, pulled back in as the record drops
     * (an on-screen reading of DAT_801d927c - the retail projection vector
     * is runtime state). */
    function lureWorld(st) {
      const b = scene;
      if (!b) return null;
      const t = Math.max(0, (st.record - 300)) / 1300;
      const dist = 350 + t * 1500;
      const [fx, fz] = facingOf(b.anchor);
      const lat = (st.lateral || 0) * 0.6;
      /* Lateral dart pushes perpendicular to the facing. */
      return [
        b.anchor.x + fx * dist + fz * lat,
        b.anchor.y - 40,
        b.anchor.z + fz * dist - fx * lat,
      ];
    }

    /* Rod-tip world position (over the angler's shoulder). */
    function rodTip() {
      const b = scene;
      if (!b) return null;
      const [fx, fz] = facingOf(b.anchor);
      return [b.anchor.x + fx * 70, b.anchor.y - 430, b.anchor.z + fz * 70];
    }

    /* ------------- events from the session ------------- */

    function onEvents(events, st) {
      for (const e of events) {
        if (e.e === 'splash') {
          splashT = 40;
          const lw = lureWorld(st);
          if (lw) ripples.push({ x: lw[0], z: lw[2], t: 0 });
        } else if (e.e === 'hooked') {
          splashT = 24;
          const lw = lureWorld(st);
          if (lw) ripples.push({ x: lw[0], z: lw[2], t: 0 });
        } else if (e.e === 'landed') {
          jumpT = 50;
        }
      }
    }

    /* ------------- HUD drawing ------------- */

    function hudText(text, x, y, opts) {
      if (!text) return;
      opts = opts || {};
      g.save();
      g.font = (opts.bold ? 'bold ' : '') + (opts.px || 8) * SCALE + 'px monospace';
      g.textBaseline = 'middle';
      g.textAlign = opts.align || 'left';
      const a = (opts.b === undefined ? 128 : opts.b) / 128;
      g.fillStyle = opts.color || ('rgba(255,255,255,' + Math.min(1, a) + ')');
      if (opts.shadow !== false) {
        g.save();
        g.fillStyle = 'rgba(0,0,0,0.6)';
        g.fillText(text, x * SCALE + 1, y * SCALE + 1);
        g.restore();
      }
      g.fillText(text, x * SCALE, y * SCALE);
      g.restore();
    }

    /* Draw one item of the ported HUD draw list (retail 320x240 coords). */
    function drawHudItem(it) {
      if (it.t === 'digit') {
        hudText(String(it.d), it.x, it.y, { b: it.b, bold: true });
      } else if (it.t === 'cap') {
        hudText(CAPTION_TEXT[it.k] || '', it.x, it.y, { color: '#cfe0ef' });
      } else if (it.t === 'glyph') {
        const label = GLYPH_LABEL[it.id];
        if (label === undefined) return;
        const banner = it.layer === 0;
        hudText(label, it.x, it.y, {
          b: it.b,
          bold: banner,
          px: banner ? 14 : 8,
          align: banner ? 'center' : 'left',
          color: banner ? '#ffd77a' : '#9fb6c9',
        });
      } else if (it.t === 'bar') {
        const rgb = 'rgb(' + it.rgb[0] + ',' + it.rgb[1] + ',' + it.rgb[2] + ')';
        g.save();
        if (it.axis === 'h') {
          const w = (it.end_x - it.x) * SCALE, h = 8 * SCALE;
          g.fillStyle = 'rgba(0,0,0,0.55)';
          g.fillRect(it.x * SCALE, it.y * SCALE, w, h);
          g.strokeStyle = 'rgba(255,255,255,0.5)';
          g.strokeRect(it.x * SCALE + 0.5, it.y * SCALE + 0.5, w, h);
          g.fillStyle = rgb;
          g.fillRect((it.x + 4) * SCALE, (it.y + 1) * SCALE,
                     Math.max(0, it.fill) * SCALE, h - 2 * SCALE);
        } else {
          const h = (it.end_y - it.y) * SCALE, w = 8 * SCALE;
          g.fillStyle = 'rgba(0,0,0,0.55)';
          g.fillRect(it.x * SCALE, it.y * SCALE, w, h);
          g.strokeStyle = 'rgba(255,255,255,0.5)';
          g.strokeRect(it.x * SCALE + 0.5, it.y * SCALE + 0.5, w, h);
          /* The power bar fills UPWARD from the bottom cap. */
          const fh = Math.max(0, it.fill) * SCALE;
          g.fillStyle = rgb;
          g.fillRect((it.x + 1) * SCALE, it.y * SCALE + h - 4 - fh,
                     w - 2 * SCALE, fh);
        }
        g.restore();
      }
    }

    /* The projected line / lure / ripple / fish overlay. */
    function drawWaterOverlay(st) {
      if (!scene || !st || !st.live) return;
      const active = st.phase === 'flight' || st.phase === 'waiting'
        || st.phase === 'hooked';
      const lw = active ? lureWorld(st) : null;
      const lp = lw ? project(lw) : null;
      const tip = project(rodTip());

      /* Ripples (splash + wander) expand and fade in world space. */
      ripples = ripples.filter(r => r.t < 46);
      for (const r of ripples) {
        r.t++;
        const p = project([r.x, (scene.anchor.y - 30), r.z]);
        if (!p) continue;
        g.save();
        g.strokeStyle = 'rgba(220,240,255,' + (0.55 * (1 - r.t / 46)) + ')';
        g.lineWidth = 1.5;
        g.beginPath();
        g.ellipse(p[0], p[1], 3 + r.t * 1.6, (3 + r.t * 1.6) * 0.38, 0, 0, Math.PI * 2);
        g.stroke();
        g.restore();
      }

      if (lp && tip) {
        /* The line: rod tip -> a sagging midpoint -> the lure (retail draws
         * it as projected segments through the overlay's own clip helpers). */
        g.save();
        g.strokeStyle = 'rgba(235,235,245,0.85)';
        g.lineWidth = 1;
        g.beginPath();
        g.moveTo(tip[0], tip[1]);
        const mx = (tip[0] + lp[0]) / 2, my = Math.max(tip[1], lp[1]) + 14;
        g.quadraticCurveTo(mx, my, lp[0], lp[1]);
        g.stroke();

        /* The bobber. */
        const bob = Math.sin(performance.now() / 300) * 2;
        g.fillStyle = st.phase === 'hooked' ? '#ff5f4a' : '#ffd0d0';
        g.beginPath();
        g.arc(lp[0], lp[1] + bob, 4, 0, Math.PI * 2);
        g.fill();
        g.fillStyle = '#f5f5f5';
        g.beginPath();
        g.arc(lp[0], lp[1] + bob - 3, 2.2, 0, Math.PI * 2);
        g.fill();

        /* The hooked fish: a shadow under the surface, darting with the
         * fight's lateral push and sinking with the line depth. */
        if (st.phase === 'hooked') {
          const depth = 6 + (st.depth / 4096) * 26;
          const wig = Math.sin(performance.now() / 90) * 4;
          g.fillStyle = 'rgba(10,30,45,0.55)';
          g.beginPath();
          g.ellipse(lp[0] + wig, lp[1] + depth, 16, 5, 0, 0, Math.PI * 2);
          g.fill();
        }

        /* Splash burst frames. */
        if (splashT > 0) {
          splashT--;
          g.strokeStyle = 'rgba(255,255,255,' + (splashT / 40) + ')';
          g.lineWidth = 2;
          for (let i = 0; i < 3; i++) {
            const a = (i / 3) * Math.PI * 2 + splashT / 6;
            g.beginPath();
            g.moveTo(lp[0], lp[1]);
            g.lineTo(lp[0] + Math.cos(a) * (12 + (40 - splashT)),
                     lp[1] - Math.abs(Math.sin(a)) * (10 + (40 - splashT) / 2));
            g.stroke();
          }
        }

        /* Landed: the catch arcs out of the water. */
        if (jumpT > 0) {
          jumpT--;
          const t = 1 - jumpT / 50;
          const jy = lp[1] - Math.sin(t * Math.PI) * 60;
          g.save();
          g.translate(lp[0] + t * 30, jy);
          g.rotate(t * 2.2);
          g.fillStyle = '#9fd4ef';
          g.beginPath();
          g.ellipse(0, 0, 13, 5, 0, 0, Math.PI * 2);
          g.fill();
          g.fillStyle = '#7ab7d8';
          g.beginPath();
          g.moveTo(11, 0); g.lineTo(19, -5); g.lineTo(19, 5); g.closePath();
          g.fill();
          g.restore();
        }
        g.restore();
      }
    }

    /* One full frame: 3D pass below, HUD + overlay above. `hud` is the
     * ported draw-item list for this frame (already JSON-parsed). */
    function draw(st, hud) {
      const W = canvas.width, H = canvas.height;
      if (scene) {
        if (st && st.live && st.venue !== scene.venue) applyVenue(st.venue);
        render3D(st);
        g.clearRect(0, 0, W, H);
      } else {
        /* No WebGL / scene: flat water backdrop, and the page's note says
         * the venue didn't decode. */
        const grad = g.createLinearGradient(0, 0, 0, H);
        grad.addColorStop(0, '#123a55');
        grad.addColorStop(1, '#0a2033');
        g.fillStyle = grad;
        g.fillRect(0, 0, W, H);
      }
      drawWaterOverlay(st);
      if (hud) for (const it of hud) drawHudItem(it);
    }

    return {
      loadAssets, draw, onEvents,
      sceneOk() { return !!scene; },
      camInfo() {
        return scene
          ? { cam: Object.assign({}, scene.cam), center: scene.center.slice(),
              radius: scene.radius, anchor: Object.assign({}, scene.anchor) }
          : null;
      },
      setCam(c) { if (scene && c) Object.assign(scene.cam, c); },
      setAnchor(a) {
        if (!scene || !a) return;
        Object.assign(scene.anchor, a);
        if (a.x !== undefined || a.z !== undefined) {
          scene.anchor.y = api.fishing_scene_height_at(scene.anchor.x, scene.anchor.z);
        }
      },
      setCenter(c) { if (scene && c) scene.center = c.slice(); },
    };
  }

  /* Pose `base` (combined buffer, angler in verts [0, oid.length)) through
   * the idle clip at `frame`, then translate the whole figure to the shore
   * `anchor` (+ yaw about Y). Same per-object composition as the dance /
   * baka posers (R . v + T per bone, world transform on top). */
  function poseWorld(out, base, oids, clip, frame, anchor) {
    const pc = clip.parts, f = clip.frames;
    const ff = ((frame % clip.frameCount) + clip.frameCount) % clip.frameCount;
    const sin = new Float32Array(pc * 3), cos = new Float32Array(pc * 3);
    const tr = new Float32Array(pc * 3);
    for (let p = 0; p < pc; p++) {
      const o = (ff * pc + p) * 6;
      for (let k = 0; k < 3; k++) {
        const a = f[o + 3 + k] * A2R;
        sin[p * 3 + k] = Math.sin(a);
        cos[p * 3 + k] = Math.cos(a);
        tr[p * 3 + k] = f[o + k];
      }
    }
    const wy = anchor.yaw || 0;
    const wsin = Math.sin(wy), wcos = Math.cos(wy);
    const n = oids.length;
    for (let v = 0; v < n; v++) {
      const vi = v * 3;
      const o = oids[v];
      let x = base[vi], y = base[vi + 1], z = base[vi + 2];
      if (o < pc) {
        const sx = sin[o * 3], cxx = cos[o * 3];
        const sy = sin[o * 3 + 1], cyy = cos[o * 3 + 1];
        const sz = sin[o * 3 + 2], czz = cos[o * 3 + 2];
        let ny = y * cxx - z * sx, nz = y * sx + z * cxx; y = ny; z = nz;
        let nx = x * cyy + z * sy; nz = -x * sy + z * cyy; x = nx; z = nz;
        nx = x * czz - y * sz; ny = x * sz + y * czz; x = nx; y = ny;
        x += tr[o * 3]; y += tr[o * 3 + 1]; z += tr[o * 3 + 2];
      }
      const rx = x * wcos + z * wsin;
      const rz = -x * wsin + z * wcos;
      out[vi] = rx + anchor.x;
      out[vi + 1] = y + (anchor.y || 0);
      out[vi + 2] = rz + anchor.z;
    }
  }

  return { create };
})();
