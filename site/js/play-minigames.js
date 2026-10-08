/* play-minigames.js - the four in-world minigames on the play page.
 *
 * Classic script (no module syntax - docs/tooling/site-shell.md). Exposes
 * `window.LegaiaPlayMinigames = { frame(rt, view, skipDraw) -> bool, ... }`;
 * `frame` returns true when a minigame owned the 3D frame, so `_frame` skips
 * its field / battle branches.
 *
 * Everything drawn here is decoded off the visitor's own disc by the engine
 * (`crates/web-viewer/src/play_minigame*.rs`), through the same presentation
 * bundle the standalone minigames page uses; the 3D renderers below are the
 * standalone page's (`minigame-muscle.js` / `minigame-baka.js` /
 * `minigame-dance.js` scene builders) re-pointed at the play runtime's
 * `play_mg_*` exports, and the slot machine is the engine's own screen-prim
 * frame. The rules
 * run in the engine off the pad word the page already routes - this file
 * reads state and draws; it binds no key.
 *
 * Layers: the 3D games draw through the page's own TmdRenderer (its
 * single-mesh `uploadMesh` / `render` path, the field's assembled scene
 * meshes untouched underneath); the dome's hub screens draw on a 2D layer
 * canvas this script inserts between the GL
 * view and the page's text overlay, so the engine's HUD lines
 * (`minigame_overlay_draws`) still read on top. */
(function () {
  'use strict';

  const HUD_W = 320, HUD_H = 240;     /* retail stage */

  const S = {
    game: null,        /* 'slot' | 'muscle' | 'baka' | 'dance' | null */
    gen: -1,
    layer: null,       /* the 2D layer canvas */
    layerCtx: null,
    slotVram: false,   /* the slot art pack is the renderer's VRAM */
    scene: null,       /* the live 3D scene for muscle / baka / dance */
    savedFlags: null,  /* TmdRenderer flags to restore on exit */
    hubSheets: {},     /* "sheet:pal" -> canvas */
    hubDims: {},
    prize: null,       /* the fishing prize-exchange panel */
  };

  /* shared helper: site/js/site-util.js */
  const rgbaCanvas = window.LegaiaUtil.rgbaCanvas;

  function parse(fn) {
    try { return JSON.parse(fn()); } catch (e) { return null; }
  }

  /* The 2D layer between the GL view and the page's text overlay. Sized to
   * the overlay canvas, same absolute placement (`.play-menu-overlay`). */
  function ensureLayer(view) {
    if (S.layer) return S.layer;
    const ov = view.menuOverlay;
    if (!ov || !ov.parentNode) return null;
    const c = document.createElement('canvas');
    c.className = 'play-menu-overlay';
    c.id = 'play-minigame-layer';
    c.width = ov.width; c.height = ov.height;
    c.hidden = true;
    ov.parentNode.insertBefore(c, ov);
    S.layer = c;
    S.layerCtx = c.getContext('2d');
    return c;
  }

  function showLayer(view, on) {
    const c = ensureLayer(view);
    if (!c) return null;
    if (view.menuOverlay && (c.width !== view.menuOverlay.width || c.height !== view.menuOverlay.height)) {
      c.width = view.menuOverlay.width; c.height = view.menuOverlay.height;
    }
    c.hidden = !on;
    if (!on) S.layerCtx.clearRect(0, 0, c.width, c.height);
    return c;
  }

  function clearGl(view) {
    const r = view.renderer;
    if (!r || !r.gl) return;
    const gl = r.gl;
    gl.viewport(0, 0, gl.drawingBufferWidth, gl.drawingBufferHeight);
    gl.clearColor(0, 0, 0, 1);
    gl.clear(gl.COLOR_BUFFER_BIT | gl.DEPTH_BUFFER_BIT);
  }

  /* shared helper: site/js/site-util.js */
  const poseInto = window.LegaiaUtil.poseClipInto;

  /* Half-extent + height of a rest pose (camera framing / spacing). */
  function poseExtent(f, clip) {
    const out = new Float32Array(f.pos);
    poseInto(out, f.pos, f.oid, clip, 0, 0, 0, 0, 0);
    let lo = Infinity, hi = -Infinity, top = 0;
    for (let i = 0; i < out.length; i += 3) {
      if (out[i] < lo) lo = out[i];
      if (out[i] > hi) hi = out[i];
      if (-out[i + 1] > top) top = -out[i + 1];
    }
    return { half: (hi - lo) / 2 || 200, height: top || 400 };
  }

  /* Concatenate `parts` ([{pos,uvs,ct,flat,idx}]) into one vertex set. */
  function concatBuffers(parts) {
    let n = 0;
    const bases = [];
    for (const p of parts) { bases.push(n); n += p.pos.length / 3; }
    const pos = new Float32Array(n * 3);
    const uvs = new Uint8Array(n * 2);
    const ct = new Uint16Array(n * 2);
    const flat = new Uint8Array(n * 4);
    const idx = [];
    parts.forEach((p, i) => {
      const b = bases[i];
      pos.set(p.pos, b * 3);
      uvs.set(p.uvs, b * 2);
      ct.set(p.ct, b * 2);
      if (p.flat && p.flat.length) flat.set(p.flat, b * 4);
      else for (let k = 0; k < p.pos.length / 3; k++) flat.set([128, 128, 128, 255], (b + k) * 4);
      for (const ix of p.idx) idx.push(ix + b);
    });
    return { pos, uvs, ct, flat, idx: new Uint32Array(idx), bases };
  }

  /* Take over the page renderer for a 3D minigame: remember its flags,
   * upload the game's VRAM + combined mesh. */
  function takeRenderer(view, vram, buf, flags) {
    const r = view.renderer;
    if (!r) return false;
    if (!S.savedFlags) {
      S.savedFlags = {
        cullBackfaces: r.cullBackfaces, cullFrontFace: r.cullFrontFace,
        semiTwoPass: r.semiTwoPass,
      };
    }
    r.cullBackfaces = !!(flags && flags.cullBackfaces);
    if (flags && flags.cullFrontFace) r.cullFrontFace = flags.cullFrontFace;
    r.semiTwoPass = !!(flags && flags.semiTwoPass);
    if (vram && vram.length) r.uploadVram(vram);
    r.uploadMesh(buf.pos, buf.uvs, buf.ct, buf.idx, buf.flat);
    return true;
  }

  function releaseRenderer(rt, view) {
    const r = view.renderer;
    if (r && S.savedFlags) {
      r.cullBackfaces = S.savedFlags.cullBackfaces;
      r.cullFrontFace = S.savedFlags.cullFrontFace;
      r.semiTwoPass = S.savedFlags.semiTwoPass;
    }
    S.savedFlags = null;
    /* Drop the single-mesh buffer so a stale minigame mesh can never draw
     * through the inspector path again. */
    if (r && r.uploadMesh) {
      try {
        r.uploadMesh(new Float32Array(0), new Uint8Array(0), new Uint16Array(0),
          new Uint32Array(0), null);
      } catch (e) { /* nothing to drop */ }
    }
    if (r && typeof rt.field_vram_bytes === 'function') {
      try { r.uploadVram(rt.field_vram_bytes()); } catch (e) { /* keep going */ }
    }
  }

  function renderScene(view, sc) {
    const r = view.renderer;
    if (!r) return;
    r.updatePositions(sc.out);
    /* A scene carrying an engine view-projection (the dance hall's entry
     * camera) draws through it instead of the orbit framing. */
    r.mvpOverride = sc.vp && sc.vp.length === 16 ? Float32Array.from(sc.vp) : null;
    /* The engine projection carries the retail screen-X mirror the orbit
     * framing does not, so its front faces wind the other way. */
    const front = r.cullFrontFace;
    if (r.mvpOverride) r.cullFrontFace = front === 'ccw' ? 'cw' : 'ccw';
    r.render(sc.cam.yaw, sc.cam.pitch, sc.cam.distance, 0, 0, sc.center, sc.radius, sc.fov);
    r.cullFrontFace = front;
    r.mvpOverride = null;
  }

  /* ================================================================== */
  /* Slot machine. The engine draws the whole frame - cabinet mesh, reels,
   * furniture, dot matrix, coin HUD and paylines - through the shared
   * `ui_slot_cabinet` builder into the page's screen-prim pass, the same
   * primitive list the native window draws, sampling the machine's art pack.
   * This file only puts that art pack up as the renderer's VRAM for the
   * visit (the teardown hands the field's back) and clears the 3D view. A
   * disc whose resident set does not decode draws the engine's status rows
   * over a cleared view instead; there is no second, hand-composed machine. */

  function slotFrame(rt, view, skipDraw) {
    const st = parse(() => rt.play_mg_slot_state_json());
    if (!st || !st.live) return;
    showLayer(view, false);
    const ready = typeof rt.play_mg_slot_cabinet_ready === 'function' && rt.play_mg_slot_cabinet_ready();
    const r = view.renderer;
    if (ready && r && !S.slotVram) {
      try { r.uploadVram(rt.play_mg_slot_vram()); S.slotVram = true; } catch (e) { /* keep going */ }
    }
    if (!skipDraw) clearGl(view);
  }

  /* ================================================================== */
  /* Muscle Dome: arena backdrop + ground grid, the lead's assembled battle
   * form and the ladder's monster, posed from their own clip banks. */

  /* The dome's 3D surface is the engine's (MuscleDomeSurface::frame, the
   * call the native window makes too): the seat, the choreography, the pose
   * and the camera. The page uploads the buffers on a generation change and
   * the posed positions every frame. */
  function muscleUpload(rt, view, gen) {
    const pos = rt.play_mg_muscle_scene_positions();
    if (!pos.length) return null;
    const buf = {
      pos,
      uvs: rt.play_mg_muscle_scene_uvs(),
      ct: rt.play_mg_muscle_scene_cba_tsb(),
      flat: rt.play_mg_muscle_scene_flat_rgba(),
      idx: rt.play_mg_muscle_scene_indices(),
    };
    /* The arena's lamp glow is semi-transparent (ABE) prims. */
    if (!takeRenderer(view, rt.play_mg_muscle_scene_vram(), buf, { semiTwoPass: true })) return null;
    return { kind: 'muscle', gen };
  }

  function muscleBuild(rt, view) {
    if (typeof rt.play_mg_muscle_scene_frame !== 'function') return null;
    const gen = rt.play_mg_muscle_scene_frame();
    return gen < 0 ? null : muscleUpload(rt, view, gen);
  }

  function muscleFrame(rt, view, skipDraw) {
    const gen = typeof rt.play_mg_muscle_scene_frame === 'function'
      ? rt.play_mg_muscle_scene_frame() : -1;
    if (gen >= 0 && (!S.scene || S.scene.gen !== gen)) S.scene = muscleUpload(rt, view, gen);
    if (skipDraw) return;
    const r = view.renderer;
    if (gen < 0 || !S.scene || !r) {
      clearGl(view);
    } else {
      r.updatePositions(rt.play_mg_muscle_scene_positions());
      const c = r.canvas;
      const vp = rt.play_mg_muscle_scene_vp(c.width / Math.max(c.height, 1));
      r.mvpOverride = vp.length === 16 ? Float32Array.from(vp) : null;
      r.render(0, 0, 1, 0, 0, [0, 0, 0], 1);
      r.mvpOverride = null;
    }
    /* Hub screens (intro card / ROUND banner) over the arena. */
    drawHubQuads(rt, view);
  }

  /* ================================================================== */
  /* Fishing: the pond scene (`other1`) with the party on its shore seats,
   * framed by the venue camera - the engine's FishingSurface, the kernel
   * the native window draws too. The HUD, rod and line ride play_fishing's
   * own passes over it. */
  function fishingUpload(rt, view, gen) {
    const pos = rt.play_mg_fishing_scene_positions();
    if (!pos.length) return null;
    const buf = {
      pos,
      uvs: rt.play_mg_fishing_scene_uvs(),
      ct: rt.play_mg_fishing_scene_cba_tsb(),
      flat: rt.play_mg_fishing_scene_flat_rgba(),
      idx: rt.play_mg_fishing_scene_indices(),
    };
    /* The pond's water sheets are semi-transparent (ABE) prims. */
    if (!takeRenderer(view, rt.play_mg_fishing_scene_vram(), buf, { semiTwoPass: true })) return null;
    return { kind: 'fishing', gen };
  }

  function fishingFrame(rt, view, skipDraw) {
    const gen = typeof rt.play_mg_fishing_scene_frame === 'function'
      ? rt.play_mg_fishing_scene_frame() : -1;
    if (gen >= 0 && (!S.scene || S.scene.gen !== gen)) S.scene = fishingUpload(rt, view, gen);
    if (skipDraw) return;
    const r = view.renderer;
    if (gen < 0 || !S.scene || !r) {
      clearGl(view);
      return;
    }
    r.updatePositions(rt.play_mg_fishing_scene_positions());
    const c = r.canvas;
    const vp = rt.play_mg_fishing_scene_vp(c.width / Math.max(c.height, 1));
    r.mvpOverride = vp.length === 16 ? Float32Array.from(vp) : null;
    /* The venue camera carries the retail screen-X mirror, so its front
     * faces wind the other way (as the dance hall's engine camera does). */
    const front = r.cullFrontFace;
    if (r.mvpOverride) r.cullFrontFace = front === 'ccw' ? 'cw' : 'ccw';
    r.render(0, 0, 1, 0, 0, [0, 0, 0], 1);
    r.cullFrontFace = front;
    r.mvpOverride = null;
  }

  /* The PROT 0977 hub screens: engine-placed quads over the two hub page
   * sheets, blitted at retail 320x240 coordinates scaled to the layer. */
  function hubSheet(rt, sheet, pal) {
    const key = sheet + ':' + pal;
    if (S.hubSheets[key] !== undefined) return S.hubSheets[key];
    let c = null;
    try {
      let dims = S.hubDims[sheet];
      if (!dims) { dims = rt.play_mg_muscle_hub_sheet_dims(sheet); S.hubDims[sheet] = dims; }
      const rgba = rt.play_mg_muscle_hub_sheet_rgba(sheet, pal);
      if (dims && dims.length === 2) c = rgbaCanvas(rgba, dims[0], dims[1]);
    } catch (e) { c = null; }
    S.hubSheets[key] = c;
    return c;
  }
  /* PSX semi-transparency for a hub quad's `abr` (null = opaque): 0 is
   * `B/2 + F/2`, 1 `B + F`, 3 `B + F/4` - canvas composites (a transparent
   * texel has alpha 0 and adds nothing). 2 is `B - F`, which a 2D canvas has
   * no composite for: `draw` receives `true` and must hand in the sprite's
   * black silhouette (`hubSilhouette`) instead. Every ABR-2 hub quad samples
   * one of the all-white knockout palettes the variant-2 emitter bump
   * selects (7 under 6, 9 under 8, ...), and the arena uploads those
   * STP-set, so `B - white` clamps to black under every texel - which is the
   * silhouette, exactly, at full fade. */
  function withAbr(g, abr, draw) {
    if (abr == null) return draw(false);
    if (abr === 2) return draw(true);
    g.save();
    if (abr === 0) g.globalAlpha *= 0.5;
    else {
      g.globalCompositeOperation = 'lighter';
      if (abr === 3) g.globalAlpha *= 0.25;
    }
    try { return draw(false); } finally { g.restore(); }
  }
  /* The black silhouette of a decoded sheet (every opaque texel -> black,
   * alpha kept), cached per sheet canvas. */
  const hubSilhouettes = new WeakMap();
  function hubSilhouette(c) {
    if (!c) return c;
    let s = hubSilhouettes.get(c);
    if (!s) {
      s = document.createElement('canvas');
      s.width = c.width; s.height = c.height;
      const sg = s.getContext('2d');
      sg.drawImage(c, 0, 0);
      sg.globalCompositeOperation = 'source-in';
      sg.fillStyle = '#000';
      sg.fillRect(0, 0, s.width, s.height);
      hubSilhouettes.set(c, s);
    }
    return s;
  }
  function drawHubQuads(rt, view) {
    if (typeof rt.play_mg_muscle_hub_quads_json !== 'function') return false;
    const m = parse(() => rt.play_mg_muscle_hub_quads_json());
    const quads = (m && m.ok && m.quads) || [];
    if (!quads.length) {
      /* Only hide a layer that is up: this runs on every field frame too. */
      if (S.game !== 'slot' && S.layer && !S.layer.hidden) showLayer(view, false);
      return false;
    }
    const layer = showLayer(view, true);
    if (!layer) return false;
    const g = S.layerCtx;
    const sx = layer.width / HUD_W, sy = layer.height / HUD_H;
    g.imageSmoothingEnabled = false;
    g.clearRect(0, 0, layer.width, layer.height);
    for (const q of quads) {
      /* The first visit's backdrop shade (FUN_801D1610): retail's
       * subtractive Gouraud ramp, applied to what is already down; black
       * bands at alpha f / 255 only when the layer cannot be read back. */
      if (q.shade) {
        if (subtractShade(g, q, sx, sy)) continue;
        const bands = 16;
        for (let b = 0; b < bands; b++) {
          const y0 = q.y + Math.floor(q.dh * b / bands);
          const y1 = q.y + Math.floor(q.dh * (b + 1) / bands);
          const f = q.top + (q.bottom - q.top) * ((b + 0.5) / bands);
          if (y1 <= y0 || f <= 0) continue;
          g.fillStyle = 'rgba(0,0,0,' + (f / 255) + ')';
          g.fillRect(q.x * sx, y0 * sy, q.dw * sx, (y1 - y0) * sy);
        }
        continue;
      }
      const s = hubSheet(rt, q.sheet, q.pal);
      if (!s) continue;
      /* The quad's Gouraud colour (`texel * c / 128`): the hub screens'
       * fade in / out is this colour, not an alpha - the native window's
       * sprite pass modulates by it, and leaving it out drew every banner at
       * full strength through its whole fade. */
      const mod = q.rgb ? window.LegaiaUtil.modulatedSprite(s, q.u, q.v, q.w, q.h, q.rgb[0], q.rgb[1]) : null;
      withAbr(g, q.abr, (sub) => (mod && !sub)
        ? g.drawImage(mod, 0, 0, q.w, q.h, q.x * sx, q.y * sy, q.dw * sx, q.dh * sy)
        : g.drawImage(sub ? hubSilhouette(s) : s, q.u, q.v, q.w, q.h,
          q.x * sx, q.y * sy, q.dw * sx, q.dh * sy));
      /* The ringside still is an opaque packet modulated by its fade level
       * (`texel * c / 128`): below neutral that is the image darkened
       * toward black, which a black fill at `1 - c/128` reproduces. */
      if (typeof q.bright === 'number' && q.bright < 128) {
        g.fillStyle = 'rgba(0,0,0,' + (1 - q.bright / 128) + ')';
        g.fillRect(q.x * sx, q.y * sy, q.dw * sx, q.dh * sy);
      }
    }
    return true;
  }

  /* The first visit's backdrop shade (FUN_801D1610): an untextured Gouraud
   * quad drawn with ABR 2 (tpage 0x46, `B - F`), top corners 0x64, bottom 0.
   * A 2D canvas has no subtractive composite, so the page does the equation
   * itself on the pixels already down: per canvas row, F is the ramp at that
   * row's logical y and every channel drops by F, clamped at 0. `sx` / `sy`
   * map logical 320x240 pixels to canvas pixels. Returns false when the
   * canvas cannot be read back (the caller then falls back to black bands). */
  function subtractShade(g, q, sx, sy) {
    const cw = g.canvas.width, ch = g.canvas.height;
    const x0 = Math.max(0, Math.round(q.x * sx)), y0 = Math.max(0, Math.round(q.y * sy));
    const x1 = Math.min(cw, Math.round((q.x + q.dw) * sx));
    const y1 = Math.min(ch, Math.round((q.y + q.dh) * sy));
    const w = x1 - x0, h = y1 - y0;
    if (w <= 0 || h <= 0 || !(q.dh > 0)) return true;
    let img;
    try { img = g.getImageData(x0, y0, w, h); } catch (e) { return false; }
    const d = img.data;
    for (let r = 0; r < h; r++) {
      const ly = (y0 + r + 0.5) / sy - q.y;
      const f = Math.round(q.top + (q.bottom - q.top) * (ly / q.dh));
      if (f <= 0) continue;
      for (let i = r * w * 4, e = i + w * 4; i < e; i += 4) {
        d[i] = d[i] > f ? d[i] - f : 0;
        d[i + 1] = d[i + 1] > f ? d[i + 1] - f : 0;
        d[i + 2] = d[i + 2] > f ? d[i + 2] - f : 0;
      }
    }
    g.putImageData(img, x0, y0);
    return true;
  }

  /* ================================================================== */
  /* Baka Fighter: the engine's duel surface (`engine-core::baka_duel_scene`
   * through the `play_mg_baka_scene_*` exports) - the fighters posed by the
   * duel's own clip clocks, the special's afterimage ghosts, the four arena
   * walls and the floor grid, drawn under the arena camera's ready
   * view-projection. The native window uploads the same buffers and draws
   * them with the same matrix; nothing is posed or framed here. */

  function bakaUpload(rt, view, gen) {
    const pos = rt.play_mg_baka_scene_positions();
    if (!pos.length) return null;
    const buf = {
      pos,
      uvs: rt.play_mg_baka_scene_uvs(),
      ct: rt.play_mg_baka_scene_cba_tsb(),
      flat: rt.play_mg_baka_scene_flat_rgba(),
      idx: rt.play_mg_baka_scene_indices(),
    };
    /* The arena's lamp glow is semi-transparent (ABE) prims: the two-pass
     * draw keeps them from painting opaque. */
    if (!takeRenderer(view, rt.play_mg_baka_scene_vram(), buf, { semiTwoPass: true })) return null;
    return { kind: 'baka', gen, attrGen: bakaAttrGen(rt) };
  }

  /* The scene's attribute generation: the impact effect's flip-book cells
   * and fades rewrite UVs / CBA-TSB / colours between VRAM generations. */
  function bakaAttrGen(rt) {
    return typeof rt.play_mg_baka_scene_attr_generation === 'function'
      ? rt.play_mg_baka_scene_attr_generation() : 0;
  }

  function bakaBuild(rt, view) {
    if (typeof rt.play_mg_baka_scene_frame !== 'function') return null;
    const gen = rt.play_mg_baka_scene_frame();
    return gen < 0 ? null : bakaUpload(rt, view, gen);
  }

  function bakaFrame(rt, view, skipDraw) {
    if (typeof rt.play_mg_baka_scene_frame !== 'function') {
      if (!skipDraw) clearGl(view);
      return;
    }
    const gen = rt.play_mg_baka_scene_frame();
    if (gen < 0) {
      if (!skipDraw) clearGl(view);
      return;
    }
    /* A new generation is a new seated opponent: re-read the static
     * buffers and the duel VRAM. */
    if (!S.scene || S.scene.gen !== gen) S.scene = bakaUpload(rt, view, gen);
    if (skipDraw) return;
    const r = view.renderer;
    if (!S.scene || !r) {
      clearGl(view);
      return;
    }
    const ag = bakaAttrGen(rt);
    if (S.scene.attrGen !== ag) {
      /* Same buffers, new attributes: re-upload the mesh, keep the VRAM. */
      r.uploadMesh(rt.play_mg_baka_scene_positions(), rt.play_mg_baka_scene_uvs(),
        rt.play_mg_baka_scene_cba_tsb(), rt.play_mg_baka_scene_indices(),
        rt.play_mg_baka_scene_flat_rgba());
      S.scene.attrGen = ag;
    }
    r.updatePositions(rt.play_mg_baka_scene_positions());
    const c = r.canvas;
    const vp = rt.play_mg_baka_scene_vp(c.width / Math.max(c.height, 1));
    r.mvpOverride = vp.length === 16 ? Float32Array.from(vp) : null;
    r.render(0, 0, 1, 0, 0, [0, 0, 0], 1);
    r.mvpOverride = null;
  }

  /* ================================================================== */
  /* Dance: the run's own floor over the baked `other7` hall. The bodies are
   * the engine's cast surface (`engine-core::dance_cast_scene` through the
   * `play_mg_dance_scene_*` exports) - the same kernel the native window
   * poses them with - so the cast follows the run's mode (qualifier, finals,
   * the how-to's Disco King, free play's six) and the page never picks one. */

  function danceBuild(rt, view, gen) {
    if (!rt.play_mg_dance_body_ready || !rt.play_mg_dance_body_ready()) return null;
    if (gen < 0) return null;
    const cast = {
      pos: rt.play_mg_dance_scene_positions(), uvs: rt.play_mg_dance_scene_uvs(),
      ct: rt.play_mg_dance_scene_cba_tsb(), idx: rt.play_mg_dance_scene_indices(),
      flat: rt.play_mg_dance_scene_flat_rgba(),
    };
    if (!cast.pos.length) return null;
    let env = null;
    const ep = rt.play_mg_dance_env_positions();
    if (ep.length) {
      env = { pos: ep, uvs: rt.play_mg_dance_env_uvs(), ct: rt.play_mg_dance_env_cba_tsb(),
        idx: rt.play_mg_dance_env_indices(), flat: rt.play_mg_dance_env_flat_rgba() };
    }
    let markers = null;
    if (rt.play_mg_dance_marker_tiles() > 0) {
      const mp = rt.play_mg_dance_marker_positions();
      if (mp.length) {
        markers = { pos: mp, uvs: rt.play_mg_dance_marker_uvs(), ct: rt.play_mg_dance_marker_cba_tsb(),
          idx: rt.play_mg_dance_marker_indices(), flat: rt.play_mg_dance_marker_flat_rgba() };
      }
    }
    const parts = [cast];
    if (env) parts.push(env);
    if (markers) parts.push(markers);
    const buf = concatBuffers(parts);
    const sc = {
      kind: 'dance', gen,
      markerBase: markers ? buf.bases[1 + (env ? 1 : 0)] : -1,
      /* Where the hall's triangles sit in the combined index list, so each
       * frame can draw only the ones the PSX GPU would (hallIndices). */
      hall: env ? {
        full: buf.idx, castLen: cast.idx.length,
        end: cast.idx.length + env.idx.length, base: buf.bases[1], key: null,
      } : null,
      out: buf.pos,
      /* Orbit fallback only - the engine camera frames the hall (`vp`). */
      cam: { yaw: Math.PI, pitch: 0.24, distance: 2.7 },
      center: [0, -400, 0], radius: 900, fov: 0.85,
    };
    const flags = env ? { cullBackfaces: true, cullFrontFace: 'ccw', semiTwoPass: true } : {};
    if (!takeRenderer(view, rt.play_mg_dance_body_vram(), buf, flags)) return null;
    return sc;
  }

  /* The hall under the engine camera draws only the triangles the PSX GPU
   * would: one spanning more than 1023 x 511 screen pixels is refused whole,
   * which keeps the stage-entrance curtain out of the camera track's far
   * poses. The subset is the engine's (`play_mg_dance_env_visible_indices`,
   * the `dance_venue::psx_gpu_visible_indices` kernel the native window cuts
   * its hall with); swapped in only when it changes. */
  function hallIndexKey(a) {
    let h = 0x811c9dc5 ^ a.length;
    for (let i = 0; i < a.length; i++) h = Math.imul(h ^ a[i], 0x01000193);
    return a.length + ':' + (h >>> 0);
  }

  function hallIndices(rt, view, sc) {
    const hall = sc.hall;
    const r = view.renderer;
    if (!hall || !r || !r.updateIndices
      || typeof rt.play_mg_dance_env_visible_indices !== 'function') return;
    const vis = sc.vp && sc.vp.length === 16 ? rt.play_mg_dance_env_visible_indices() : null;
    const key = vis ? hallIndexKey(vis) : 'full';
    if (key === hall.key) return;
    hall.key = key;
    if (!vis) { r.updateIndices(hall.full); return; }
    const tail = hall.full.length - hall.end;
    const out = new Uint32Array(hall.castLen + vis.length + tail);
    out.set(hall.full.subarray(0, hall.castLen), 0);
    for (let i = 0; i < vis.length; i++) out[hall.castLen + i] = vis[i] + hall.base;
    out.set(hall.full.subarray(hall.end), hall.castLen + vis.length);
    r.updateIndices(out);
  }

  /* Pose this frame through the engine surface and build on first sight. */
  function danceSceneFrame(rt) {
    return typeof rt.play_mg_dance_scene_frame === 'function'
      ? rt.play_mg_dance_scene_frame() : -1;
  }

  function danceFrame(rt, view, skipDraw) {
    let sc = S.scene;
    const gen = danceSceneFrame(rt);
    /* A new generation is a new cast (another mode's floor): rebuild. */
    if (gen >= 0 && (!sc || sc.gen !== gen)) {
      sc = danceBuild(rt, view, gen);
      S.scene = sc;
    }
    if (sc && gen >= 0) {
      sc.out.set(rt.play_mg_dance_scene_positions(), 0);
      if (sc.markerBase >= 0) {
        const mp = rt.play_mg_dance_marker_step(1);
        if (mp.length) sc.out.set(mp, sc.markerBase * 3);
      }
    }
    if (skipDraw) return;
    /* The hall frames through the dance entry's own camera (FUN_801CEF54's
     * staged pose, from the engine kernel the native window draws with). */
    if (sc && typeof rt.play_mg_dance_venue_vp === 'function' && view.renderer) {
      const c = view.renderer.canvas;
      sc.vp = rt.play_mg_dance_venue_vp(c.width / Math.max(c.height, 1));
    }
    if (sc) hallIndices(rt, view, sc);
    if (sc) renderScene(view, sc); else clearGl(view);
  }

  /* ================================================================== */
  /* Fishing prize exchange. The sub-screen is ENGINE state
   * (`World::minigames.fishing_exchange`) the HUD compose draws on the
   * canvas every frame it is open - the same screen the native window draws
   * - and this click panel is only its input surface: the button toggles it
   * through the shared input kernel (`play_fishing_exchange_input`), the
   * venue tabs switch its page, Buy puts the cursor on a row and buys
   * (`play_fishing_prize_buy`, which leaves the screen open). Retail reaches
   * the exchange through the venue clerk; the page offers it as a button
   * beside the frame. */

  function ensurePrizePanel(view) {
    if (S.prize) return S.prize;
    const ov = view.menuOverlay;
    if (!ov || !ov.parentNode) return null;
    const wrap = ov.parentNode;
    const btn = document.createElement('button');
    btn.type = 'button';
    btn.id = 'play-fishing-prizes-btn';
    btn.textContent = 'Prize exchange';
    /* Bottom-left: the top-right corner is the fishing HUD's own lure
     * counter (`Left: N`), which a corner button there covered. */
    btn.style.cssText = 'position:absolute;bottom:8px;left:8px;z-index:3;font:600 12px/1 system-ui,sans-serif;'
      + 'padding:6px 10px;border-radius:6px;border:1px solid #6c7a90;background:#1b2230;color:#e8ecf3;'
      + 'cursor:pointer;display:none;';
    const panel = document.createElement('div');
    panel.id = 'play-fishing-prizes';
    panel.style.cssText = 'position:absolute;bottom:40px;left:8px;z-index:3;width:300px;max-height:70%;overflow:auto;'
      + 'font:12px/1.4 system-ui,sans-serif;background:rgba(12,16,24,0.94);color:#e8ecf3;'
      + 'border:1px solid #6c7a90;border-radius:8px;padding:10px;display:none;';
    wrap.appendChild(btn);
    wrap.appendChild(panel);
    const p = { btn, panel, open: false, venue: 0, rt: null };
    btn.addEventListener('click', () => {
      /* Toggle the engine's sub-screen; the panel follows it (prizeFrame). */
      if (p.rt && typeof p.rt.play_fishing_exchange_input === 'function') {
        try { p.rt.play_fishing_exchange_input(0); } catch (e) { /* refused */ }
        syncPrizeOpen(p);
      } else {
        p.open = !p.open;
      }
      renderPrizePanel(p);
    });
    S.prize = p;
    return p;
  }

  /* Mirror the engine's open / venue state into the panel. Returns true when
   * it changed (the panel then re-renders). A bundle predating the export
   * keeps the panel's own flag. */
  function syncPrizeOpen(p) {
    const rt = p.rt;
    if (!rt || typeof rt.play_fishing_exchange_state_json !== 'function') return false;
    const st = parse(() => rt.play_fishing_exchange_state_json());
    if (!st) return false;
    const open = !!st.open;
    const venue = open ? (st.venue | 0) : p.venue;
    const changed = open !== p.open || venue !== p.venue;
    p.open = open;
    p.venue = venue;
    return changed;
  }

  function renderPrizePanel(p) {
    const rt = p.rt;
    p.panel.style.display = p.open ? 'block' : 'none';
    if (!p.open || !rt) return;
    const data = parse(() => rt.play_fishing_prizes_json(p.venue));
    const h = [];
    const tab = (v, label) => `<button type="button" data-venue="${v}" style="margin-right:6px;padding:3px 8px;`
      + `border-radius:4px;border:1px solid #6c7a90;background:${p.venue === v ? '#3a4d6e' : '#1b2230'};`
      + `color:#e8ecf3;cursor:pointer">${label}</button>`;
    h.push('<div style="margin-bottom:6px">' + tab(0, 'Buma') + tab(1, 'Vidna')
      + '<button type="button" data-close="1" style="float:right;padding:3px 8px;border-radius:4px;'
      + 'border:1px solid #6c7a90;background:#1b2230;color:#e8ecf3;cursor:pointer">Close</button></div>');
    if (!data) {
      h.push('<div>The exchange pages did not decode from this disc.</div>');
    } else {
      h.push(`<div style="margin-bottom:6px"><b>${data.points}</b> points</div>`);
      h.push('<table style="width:100%;border-collapse:collapse">');
      data.rows.forEach((r, i) => {
        const tag = r.one_time ? (r.available ? 'one-time' : 'sold / n/a') : 'each';
        h.push(`<tr style="opacity:${r.available ? 1 : 0.5}"><td style="padding:2px 4px">${r.name}</td>`
          + `<td style="padding:2px 4px;text-align:right">${r.price} pts</td>`
          + `<td style="padding:2px 4px">${tag} (own ${r.owned})</td>`
          + `<td style="padding:2px 4px"><button type="button" data-buy="${i}" ${r.available ? '' : 'disabled'} `
          + 'style="padding:2px 8px;border-radius:4px;border:1px solid #6c7a90;background:#1b2230;color:#e8ecf3;'
          + 'cursor:pointer">Buy</button></td></tr>');
      });
      h.push('</table>');
    }
    p.panel.innerHTML = h.join('');
    p.panel.querySelectorAll('button[data-venue]').forEach(b => b.addEventListener('click', () => {
      const want = +b.dataset.venue;
      if (want !== p.venue && typeof rt.play_fishing_exchange_input === 'function') {
        try { rt.play_fishing_exchange_input(3); } catch (e) { /* refused */ }
        syncPrizeOpen(p);
      } else {
        p.venue = want;
      }
      renderPrizePanel(p);
    }));
    p.panel.querySelectorAll('button[data-buy]').forEach(b => b.addEventListener('click', () => {
      try { rt.play_fishing_prize_buy(p.venue, +b.dataset.buy); } catch (e) { /* refused */ }
      syncPrizeOpen(p);
      renderPrizePanel(p);
    }));
    const close = p.panel.querySelector('button[data-close]');
    if (close) close.addEventListener('click', () => {
      if (typeof rt.play_fishing_exchange_input === 'function') {
        try { rt.play_fishing_exchange_input(0); } catch (e) { /* refused */ }
        syncPrizeOpen(p);
      } else {
        p.open = false;
      }
      renderPrizePanel(p);
    });
  }

  function prizeFrame(rt, view) {
    if (typeof rt.play_fishing_active !== 'function' || typeof rt.play_fishing_prizes_json !== 'function') return;
    const p = ensurePrizePanel(view);
    if (!p) return;
    p.rt = rt;
    let active = false;
    try { active = !!rt.play_fishing_active(); } catch (e) { active = false; }
    p.btn.style.display = active ? 'block' : 'none';
    if (!active && p.open) { p.open = false; renderPrizePanel(p); return; }
    if (syncPrizeOpen(p)) renderPrizePanel(p);
  }

  /* ================================================================== */

  function teardown(rt, view) {
    if (S.game && (S.game !== 'slot' || S.slotVram)) releaseRenderer(rt, view);
    S.slotVram = false;
    S.game = null;
    S.gen = -1;
    S.scene = null;
    showLayer(view, false);
  }

  function build(rt, view, info) {
    S.game = info.game;
    S.gen = info.gen;
    S.scene = null;
    S.hubSheets = {};
    S.hubDims = {};
    if (info.game === 'slot') return;
    /* The pond builds off the scene host's own disc index, not the art. */
    if (info.game === 'fishing') return;
    if (!info.art) return;
    try {
      if (info.game === 'muscle') S.scene = muscleBuild(rt, view);
      else if (info.game === 'baka') S.scene = bakaBuild(rt, view);
      else if (info.game === 'dance') S.scene = danceBuild(rt, view, danceSceneFrame(rt));
    } catch (e) {
      try { console.warn('play-minigames: scene build failed', e); } catch (_) { /* no console */ }
      S.scene = null;
    }
  }

  /* One frame. Returns true when a minigame owns the 3D view. */
  function frame(rt, view, skipDraw) {
    if (!rt || !view || typeof rt.play_mg_game_json !== 'function') return false;
    prizeFrame(rt, view);
    const info = parse(() => rt.play_mg_game_json());
    if (!info || !info.game) {
      if (S.game) teardown(rt, view);
      /* Between legs the arena keeps the frame (the engine stays in the
       * dome mode, so `muscleFrame` draws the hub over a cleared view); this
       * only clears a hub layer left up when the field comes back. */
      drawHubQuads(rt, view);
      if (typeof rt.play_mg_take_vram_restore === 'function' && rt.play_mg_take_vram_restore()
          && view.renderer && typeof rt.field_vram_bytes === 'function') {
        try { view.renderer.uploadVram(rt.field_vram_bytes()); } catch (e) { /* keep going */ }
      }
      return false;
    }
    if (info.game !== S.game || info.gen !== S.gen) {
      if (S.game) teardown(rt, view);
      build(rt, view, info);
    }
    try {
      if (info.game === 'slot') slotFrame(rt, view, skipDraw);
      else if (info.game === 'muscle') muscleFrame(rt, view, skipDraw);
      else if (info.game === 'baka') bakaFrame(rt, view, skipDraw);
      else if (info.game === 'dance') danceFrame(rt, view, skipDraw);
      else if (info.game === 'fishing') fishingFrame(rt, view, skipDraw);
    } catch (e) {
      try { console.warn('play-minigames: frame failed', e); } catch (_) { /* no console */ }
      if (!skipDraw) clearGl(view);
    }
    return true;
  }

  window.LegaiaPlayMinigames = {
    frame,
    /* Which game owns the view (`null` outside one). */
    active: () => S.game,
    /* The fishing prize-exchange panel, for a page control that wants to
     * open it directly. */
    openPrizes(rt, view) {
      const p = ensurePrizePanel(view);
      if (!p) return;
      p.rt = rt; p.open = true; renderPrizePanel(p);
    },
  };
})();
