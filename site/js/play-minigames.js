/* play-minigames.js - the four in-world minigames on the play page.
 *
 * Classic script (no module syntax - docs/tooling/site-shell.md). Exposes
 * `window.LegaiaPlayMinigames = { frame(rt, view, skipDraw) -> bool, ... }`;
 * `frame` returns true when a minigame owned the 3D frame, so `_frame` skips
 * its field / battle branches.
 *
 * Everything drawn here is decoded off the visitor's own disc by the engine
 * (`crates/web-viewer/src/play_minigame*.rs`), through the same presentation
 * bundle the standalone minigames page uses; the renderers below are the
 * standalone page's (`site/_content/minigames.html` slotRender,
 * `minigame-muscle.js` / `minigame-baka.js` / `minigame-dance.js` scene
 * builders) re-pointed at the play runtime's `play_mg_*` exports. The rules
 * run in the engine off the pad word the page already routes - this file
 * reads state and draws; it binds no key.
 *
 * Layers: the 3D games draw through the page's own TmdRenderer (its
 * single-mesh `uploadMesh` / `render` path, the field's assembled scene
 * meshes untouched underneath); the slot machine and the dome's hub
 * screens draw on a 2D layer canvas this script inserts between the GL
 * view and the page's text overlay, so the engine's HUD lines
 * (`minigame_overlay_draws`) still read on top. */
(function () {
  'use strict';

  const A2R = (Math.PI * 2) / 4096;   /* PSX angle units -> radians */
  const SYM_PX = 64;                  /* a reel symbol cell, in texels */
  const HUD_W = 320, HUD_H = 240;     /* retail stage */

  const S = {
    game: null,        /* 'slot' | 'muscle' | 'baka' | 'dance' | null */
    gen: -1,
    layer: null,       /* the 2D layer canvas */
    layerCtx: null,
    slot: null,        /* decoded slot art + scene */
    slotCaption: null,
    slotFrame: null,   /* 640x240 offscreen framebuffer */
    scene: null,       /* the live 3D scene for muscle / baka / dance */
    savedFlags: null,  /* TmdRenderer flags to restore on exit */
    hubSheets: {},     /* "sheet:pal" -> canvas */
    hubDims: {},
    prize: null,       /* the fishing prize-exchange panel */
  };

  function rgbaCanvas(bytes, w, h) {
    if (!bytes || bytes.length !== w * h * 4) return null;
    const c = document.createElement('canvas');
    c.width = w; c.height = h;
    c.getContext('2d').putImageData(new ImageData(new Uint8ClampedArray(bytes), w, h), 0, 0);
    return c;
  }

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

  /* ------------------------------------------------------------------ */
  /* Shared pose kernel: per-object Rz.Ry.Rx . v + T, then a world yaw about
   * Y and an (dx, dz) floor offset (minigame-muscle.js poseInto). */
  function poseInto(out, base, oids, clip, frame, vertBase, dx, yaw, dz) {
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
    const wsin = Math.sin(yaw || 0), wcos = Math.cos(yaw || 0);
    const n = oids.length;
    for (let v = 0; v < n; v++) {
      const vi = (vertBase + v) * 3;
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
      const wx = x * wcos + z * wsin;
      const wz = -x * wsin + z * wcos;
      out[vi] = wx + (dx || 0);
      out[vi + 1] = y;
      out[vi + 2] = wz + (dz || 0);
    }
  }

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
  /* Slot machine: the standalone page's 2D renderer of the retail 3D scene
   * (GTE projection replicated; every position / cell / palette read off the
   * overlay's rodata and PROT 1200 through the engine). */

  function slotLoad(rt) {
    S.slot = null;
    S.slotCaption = null;
    if (!rt.play_mg_slot_art_ready || !rt.play_mg_slot_art_ready()) return;
    const symbols = [];
    for (let s = 0; s < 10; s++) symbols.push(rgbaCanvas(rt.play_mg_slot_symbol_rgba(s), SYM_PX, SYM_PX));
    const numbers = [];
    for (let n = 1; n <= 10; n++) numbers.push(rgbaCanvas(rt.play_mg_slot_bonus_number_rgba(n), SYM_PX, SYM_PX));
    const pages = {};
    const page = (p, pal) => {
      const k = p + ':' + pal;
      if (!(k in pages)) {
        const w = rt.play_mg_slot_page_width(p) || 256;
        pages[k] = rgbaCanvas(rt.play_mg_slot_page_rgba(p, pal), w, 256);
      }
      return pages[k];
    };
    const scene = rt.play_mg_slot_scene_ready() ? parse(() => rt.play_mg_slot_scene_json()) : null;
    S.slot = {
      symbols, numbers,
      digits: rgbaCanvas(rt.play_mg_slot_digits_rgba(), 64 + 10 * 16, 16),
      panel: rgbaCanvas(rt.play_mg_slot_panel_rgba(), 127, 239),
      page,
      scene: scene && scene.ok ? scene : null,
      marquee: parse(() => rt.play_mg_slot_marquee_json()),
      tick: 0,
    };
    if (!S.slotFrame) {
      S.slotFrame = document.createElement('canvas');
      S.slotFrame.width = 640; S.slotFrame.height = 240;
    }
  }

  const BONUS_VALUE_BASE = 0x10;
  function slotFaceFor(value) {
    const a = S.slot;
    return value >= BONUS_VALUE_BASE ? a.numbers[value - BONUS_VALUE_BASE] : a.symbols[value];
  }
  function slotScale(z) {
    const P = S.slot.scene.proj;
    return P.sx0 * P.z0 / (P.z0 + z);
  }
  function slotProject(x, y, z) {
    const P = S.slot.scene.proj, s = slotScale(z);
    return [P.ofx + s * x, P.ofy + (s / P.aspect) * y];
  }
  function slotBillboard(hw, hh, z) {
    const k = slotScale(z) / S.slot.scene.proj.xscale;
    return [hw * k, hh * k];
  }
  function slotDrawBillboard(g, img, cell, pos, half, alpha) {
    if (!img) return;
    const [cx, cy] = slotProject(pos[0], pos[1], pos[2]);
    const [hw, hh] = slotBillboard(half[0], half[1], pos[2]);
    g.globalAlpha = alpha === undefined ? 1 : alpha;
    g.drawImage(img, cell[0], cell[1], cell[2], cell[3], cx - hw, cy - hh, hw * 2, hh * 2);
    g.globalAlpha = 1;
  }
  function slotTexTri(g, img, s0, s1, s2, t0, t1, t2) {
    g.save();
    g.beginPath();
    const cx = (s0[0] + s1[0] + s2[0]) / 3, cy = (s0[1] + s1[1] + s2[1]) / 3;
    const gr = (p) => [cx + (p[0] - cx) * 1.02, cy + (p[1] - cy) * 1.02];
    const [a, b, c] = [gr(s0), gr(s1), gr(s2)];
    g.moveTo(a[0], a[1]); g.lineTo(b[0], b[1]); g.lineTo(c[0], c[1]); g.closePath();
    g.clip();
    const d = (t1[0] - t0[0]) * (t2[1] - t0[1]) - (t2[0] - t0[0]) * (t1[1] - t0[1]);
    if (Math.abs(d) > 1e-6) {
      const m11 = ((s1[0] - s0[0]) * (t2[1] - t0[1]) - (s2[0] - s0[0]) * (t1[1] - t0[1])) / d;
      const m12 = ((s1[1] - s0[1]) * (t2[1] - t0[1]) - (s2[1] - s0[1]) * (t1[1] - t0[1])) / d;
      const m21 = ((s2[0] - s0[0]) * (t1[0] - t0[0]) - (s1[0] - s0[0]) * (t2[0] - t0[0])) / d;
      const m22 = ((s2[1] - s0[1]) * (t1[0] - t0[0]) - (s1[1] - s0[1]) * (t2[0] - t0[0])) / d;
      g.transform(m11, m12, m21, m22,
        s0[0] - m11 * t0[0] - m21 * t0[1],
        s0[1] - m12 * t0[0] - m22 * t0[1]);
      g.drawImage(img, 0, 0);
    }
    g.restore();
  }

  /* The reel cylinders, as FUN_801d0fa8 builds them (see the standalone
   * page for the derivation of the payline face + shade). */
  function slotDrawReels(g, positions, strips) {
    const R = S.slot.scene.reels;
    const FULL = R.angle_full;
    const sinT = (a) => Math.sin(((a % FULL + FULL) % FULL) / FULL * Math.PI * 2);
    const cosT = (a) => Math.cos(((a % FULL + FULL) % FULL) / FULL * Math.PI * 2);
    const ry = (a) => (4096 * sinT(a) * -R.y_radius) / 4096;
    const rz = (a) => (4096 * cosT(a)) / (1 << R.z_shift);
    const shade = (z) => Math.max(0, Math.min(R.shade_max,
      R.shade_max - Math.floor((z + R.shade_bias) * R.shade_gain / 512)));
    let paylineFace = 0, nearest = Infinity;
    for (let f = 0; f < R.faces; f++) {
      const z = rz(R.angle_base + f * R.angle_step + R.angle_step / 2);
      if (z < nearest) { nearest = z; paylineFace = f; }
    }
    for (let r = 0; r < 3; r++) {
      const pos = positions[r];
      const row0 = Math.floor(((pos >> 8) % R.strip_len + R.strip_len) % R.strip_len);
      const frac = ((pos % 256) + 256) % 256;
      const strip = strips[r];
      if (!strip || !strip.length) continue;
      const x0 = R.x[r], x1 = x0 + R.w;
      for (let f = 0; f < R.faces; f++) {
        const aTop = R.angle_base + frac + f * R.angle_step;
        const aBot = aTop + R.angle_step;
        const zT = rz(aTop), zB = rz(aBot);
        const sT = shade(zT), sB = shade(zB);
        const yT = ry(aTop), yB = ry(aBot);
        const p00 = slotProject(x0, yT, zT), p10 = slotProject(x1, yT, zT);
        const p01 = slotProject(x0, yB, zB), p11 = slotProject(x1, yB, zB);
        const idx = ((row0 + paylineFace - f) % R.strip_len + R.strip_len) % R.strip_len;
        const sym = slotFaceFor(strip[idx]);
        if (!sym) continue;
        slotTexTri(g, sym, p00, p10, p01, [0, 0], [SYM_PX, 0], [0, SYM_PX]);
        slotTexTri(g, sym, p10, p11, p01, [SYM_PX, 0], [SYM_PX, SYM_PX], [0, SYM_PX]);
        g.save();
        g.beginPath();
        g.moveTo(p00[0], p00[1]); g.lineTo(p10[0], p10[1]);
        g.lineTo(p11[0], p11[1]); g.lineTo(p01[0], p01[1]); g.closePath();
        g.clip();
        const mT = Math.min(1, sT / R.shade_neutral), mB = Math.min(1, sB / R.shade_neutral);
        const grad = g.createLinearGradient(0, (p00[1] + p10[1]) / 2, 0, (p01[1] + p11[1]) / 2);
        const lvl = (m) => `rgb(${Math.round(m * 255)},${Math.round(m * 255)},${Math.round(m * 255)})`;
        grad.addColorStop(0, lvl(mT));
        grad.addColorStop(1, lvl(mB));
        g.globalCompositeOperation = 'multiply';
        g.fillStyle = grad;
        g.fill();
        if (sT > R.shade_neutral || sB > R.shade_neutral) {
          const bT = Math.max(0, (sT - R.shade_neutral) / R.shade_neutral);
          const bB = Math.max(0, (sB - R.shade_neutral) / R.shade_neutral);
          const gb = g.createLinearGradient(0, (p00[1] + p10[1]) / 2, 0, (p01[1] + p11[1]) / 2);
          gb.addColorStop(0, `rgba(255,255,255,${bT * 0.35})`);
          gb.addColorStop(1, `rgba(255,255,255,${bB * 0.35})`);
          g.globalCompositeOperation = 'lighter';
          g.fillStyle = gb;
          g.fill();
        }
        g.restore();
      }
    }
  }

  function slotMsgBits(id) {
    const sc = S.slot.scene;
    const bits = sc.msgBits || (sc.msgBits = sc.messages.map(m => m.bitmap.split(',').map(Number)));
    return bits[id];
  }
  function slotBlitMsg(buf, id, col, row) {
    const m = S.slot.scene.messages[id];
    if (!m) return;
    const bm = slotMsgBits(id), D = S.slot.scene.dots;
    for (let r = 0; r < m.h; r++) {
      const dr = row + r;
      if (dr < 0 || dr >= D.rows) continue;
      for (let c = 0; c < m.w; c++) {
        const dc = col + c;
        if (dc < 0 || dc >= D.cols) continue;
        buf[dc * D.rows + dr] = bm[r * m.w + c];
      }
    }
  }
  function slotBlitScroll(buf, id, sx) {
    const m = S.slot.scene.messages[id];
    if (!m) return;
    const bm = slotMsgBits(id), D = S.slot.scene.dots;
    for (let col = 0; col < D.cols; col++) {
      const mc = sx + col;
      if (mc < 0 || mc >= m.w) continue;
      for (let row = 0; row < Math.min(D.rows, m.h); row++) {
        buf[col * D.rows + row] = bm[row * m.w + mc];
      }
    }
  }
  /* FUN_801cfff0's message pass: tally / payout caption / round pips /
   * attract legend, in retail's branch order. */
  function slotMarqueeBuffer(st, bonus, tick) {
    const D = S.slot.scene.dots, M = S.slot.marquee;
    const buf = new Uint8Array(D.cols * D.rows);
    if (!M) return buf;
    const tallyRow = (tally) => {
      for (let r = 0; r < 3; r++) slotBlitMsg(buf, M.number_base + tally[r], M.tally_cols[r], 0);
      slotBlitMsg(buf, M.times, M.times_cols[0], 0);
      slotBlitMsg(buf, M.times, M.times_cols[1], 0);
    };
    const cap = S.slotCaption;
    if (cap) {
      if (cap.age < 0) { tallyRow(cap.tally); return buf; }
      const n = cap.payout;
      const row = Math.min(cap.age - M.payout_slide_rows, 0);
      const d = M.number_base;
      if (n > 999) slotBlitMsg(buf, d + Math.floor(n / 1000), M.payout_digit_cols[0], row);
      if (n > 99) slotBlitMsg(buf, d + Math.floor((n % 1000) / 100), M.payout_digit_cols[1], row);
      if (n > 9) slotBlitMsg(buf, d + Math.floor((n % 100) / 10), M.payout_digit_cols[2], row);
      slotBlitMsg(buf, d + (n % 10), M.payout_digit_cols[3], row);
      slotBlitMsg(buf, M.coins, M.payout_coins_col, row);
      return buf;
    }
    if (bonus && bonus.active) {
      if (st.phase === 'stopping' || st.phase === 'payout') {
        tallyRow(bonus.tally);
      } else {
        const left = bonus.rounds_left;
        for (let i = 0; i < 3; i++) {
          slotBlitMsg(buf, left > (2 - i) ? M.pip_on : M.pip_off, M.pip_cols[i], 0);
        }
      }
      return buf;
    }
    slotBlitScroll(buf, 0, (tick % 0x94) - 0x4a);
    return buf;
  }
  function slotDrawMarqueeDots(g, buf, tick) {
    const D = S.slot.scene.dots;
    const swatch = S.slot.page(D.page, D.blink_palettes[tick & 1]);
    if (!swatch) return;
    for (let col = 0; col < D.cols; col++) {
      for (let row = 0; row < D.rows; row++) {
        const nib = buf[col * D.rows + row];
        if (!nib) continue;
        const [px, py] = slotProject(D.x0 + col * D.dx, D.y0 + row * D.dy, D.z);
        g.drawImage(swatch, nib * D.u_per_nibble, 0, D.size, D.size, Math.round(px), Math.round(py), 2, 2);
      }
    }
  }
  function slotDrawCoins(g, value) {
    const a = S.slot;
    if (!a || !a.digits) return;
    g.drawImage(a.digits, 0, 0, 64, 16, 560 - 32, 160 - 8, 64, 16);
    const s = String(Math.max(0, Math.min(99999, value))).padStart(5, '0');
    let dx = 546 + (s.length - 1) * 10 - (s.length - 1) * 16;
    for (const ch of s) {
      const d = ch.charCodeAt(0) - 48;
      g.drawImage(a.digits, 64 + d * 16, 0, 16, 16, dx, 168, 16, 16);
      dx += 16;
    }
  }
  /* The cabinet body: the measured composition (the mesh is PROT 1200
   * descriptor 1; the standalone page paints it the same way and says so). */
  function slotDrawCabinet(g) {
    g.fillStyle = '#2c2c2c';
    g.fillRect(28, 19, 460, 199);
    let gr = g.createLinearGradient(28, 0, 488, 0);
    gr.addColorStop(0, '#505050'); gr.addColorStop(0.5, '#888888'); gr.addColorStop(1, '#505050');
    g.fillStyle = gr; g.fillRect(28, 19, 460, 7);
    gr = g.createLinearGradient(28, 0, 488, 0);
    gr.addColorStop(0, '#2a2a2a'); gr.addColorStop(0.5, '#383838'); gr.addColorStop(1, '#2a2a2a');
    g.fillStyle = gr; g.fillRect(28, 26, 460, 40);
    g.fillStyle = 'rgb(0,0,72)'; g.fillRect(116, 24, 274, 34);
    gr = g.createLinearGradient(0, 66, 0, 197);
    gr.addColorStop(0, 'rgb(40,32,32)'); gr.addColorStop(0.37, 'rgb(136,48,48)');
    gr.addColorStop(0.75, 'rgb(56,24,24)'); gr.addColorStop(1, 'rgb(24,24,24)');
    g.fillStyle = gr; g.fillRect(36, 66, 434, 131);
    g.fillStyle = '#383838'; g.fillRect(28, 66, 8, 131);
    g.fillStyle = '#404040'; g.fillRect(470, 66, 18, 131);
    gr = g.createLinearGradient(0, 197, 0, 218);
    gr.addColorStop(0, 'rgb(16,16,16)'); gr.addColorStop(1, 'rgb(96,96,96)');
    g.fillStyle = gr; g.fillRect(28, 197, 460, 21);
  }

  const SLOT_TALLY_HOLD = 26, SLOT_CAPTION_FRAMES = 110;

  function slotRender(rt, st, bonus) {
    const cv = S.slotFrame, g = cv.getContext('2d');
    g.imageSmoothingEnabled = false;
    g.fillStyle = '#000000';
    g.fillRect(0, 0, cv.width, cv.height);
    const a = S.slot;
    if (!a || !a.scene) {
      g.fillStyle = '#8a8a99';
      g.font = '12px monospace';
      g.fillText(a ? 'the scene graph (PROT 0975) did not decode - symbol ids only'
                   : 'art pack (PROT 1200) did not decode - symbol ids only', 14, 20);
      for (let r = 0; r < 3; r++) for (let row = 0; row < 3; row++) {
        g.fillStyle = row === 1 ? '#e8e8f0' : '#6a6a78';
        g.font = (row === 1 ? 'bold ' : '') + '20px monospace';
        g.fillText('#' + st.window[r][row], 190 + r * 90, 90 + row * 45);
      }
      return;
    }
    const C = a.scene.cells;
    const winLine = (st.last && st.last.line != null) ? st.last.line : null;
    slotDrawCabinet(g);
    const winTop = 54, winBot = 197;
    g.fillStyle = '#000000';
    const R = a.scene.reels;
    for (let r = 0; r < 3; r++) {
      const [rx0] = slotProject(R.x[r], 0, -512);
      const [rx1] = slotProject(R.x[r] + R.w, 0, -512);
      g.fillRect(rx0 - 3, winTop, rx1 - rx0 + 6, winBot - winTop);
    }
    g.save();
    g.beginPath();
    g.rect(112, winTop, 336, winBot - winTop);
    g.clip();
    const strips = [0, 1, 2].map(r => rt.play_mg_slot_strip(r));
    slotDrawReels(g, rt.play_mg_slot_reel_pos(), strips);
    g.restore();
    /* The paylines: the engine's ported payline pass (FUN_801d3380) picks
     * each line's colour, lit state and semi-transparency; the page runs the
     * RTPS projection it leaves caller-side and strokes the segment. */
    let lines = [];
    try { lines = JSON.parse(rt.play_mg_slot_payline_prims_json(winLine == null ? -1 : winLine)); }
    catch (e) { lines = []; }
    for (const l of lines) {
      /* Endpoints projected by the engine (slot_machine::projected_paylines),
       * the same segments the native window draws; the page-side projection
       * is only the fallback for a bundle that predates the fields. */
      const p = l.sa || slotProject(l.a[0], l.a[1], l.a[2]);
      const q = l.sb || slotProject(l.b[0], l.b[1], l.b[2]);
      g.strokeStyle = `rgba(${l.rgb[0]},${l.rgb[1]},${l.rgb[2]},${l.semi ? 0.5 : 1})`;
      g.lineWidth = l.lit ? 2 : 1;
      g.beginPath(); g.moveTo(p[0], p[1]); g.lineTo(q[0], q[1]); g.stroke();
    }
    a.scene.medallions.forEach((m) => {
      const img = a.page(C.medallion_page, (C.medallion_clut_base + m.art) & 0x3F);
      slotDrawBillboard(g, img, C.medallion, m.pos, C.medallion_half);
    });
    a.scene.lamps.forEach((m, i) => {
      const img = a.page(C.lamp_page, C.lamp_palette);
      slotDrawBillboard(g, img, i === winLine ? C.lamp_lit : C.lamp_unlit, m.pos, C.lamp_half);
    });
    a.scene.pedestals.forEach((p, r) => {
      const stopped = st.stopped > r;
      const pal = (stopped ? C.pedestal_clut_stopped : C.pedestal_clut_spinning) + r;
      const img = a.page(C.pedestal_page, pal & 0x3F);
      const cell = (stopped ? C.pedestal_cells_stopped : C.pedestal_cells)[r];
      slotDrawBillboard(g, img, cell, p.pos, C.pedestal_half);
    });
    a.scene.marquee.forEach((m) => {
      const img = a.page(C.marquee_page, m.clut);
      slotDrawBillboard(g, img, m.cell, m.pos, m.half);
    });
    slotDrawMarqueeDots(g, slotMarqueeBuffer(st, bonus, a.tick), a.tick);
    if (a.panel) g.drawImage(a.panel, 560 - 63, 128 - 119);
    slotDrawCoins(g, st.balance);
  }

  function slotFrame(rt, view, skipDraw) {
    const st = parse(() => rt.play_mg_slot_state_json());
    if (!st || !st.live) return;
    const bonus = parse(() => rt.play_mg_slot_bonus_json());
    if (S.slot) S.slot.tick = (st.tick | 0);
    /* A bonus round just paid: hold its finished tally, then the caption. */
    if (st.credited > 0 && st.credited_bonus && bonus) {
      S.slotCaption = { payout: st.credited, tally: bonus.tally.slice(), age: -SLOT_TALLY_HOLD };
    } else if (S.slotCaption) {
      S.slotCaption.age++;
      if (S.slotCaption.age > SLOT_CAPTION_FRAMES || st.phase === 'spinning') S.slotCaption = null;
    }
    if (skipDraw) return;
    clearGl(view);
    const layer = showLayer(view, true);
    if (!layer || !S.slotFrame) return;
    slotRender(rt, st, bonus);
    const g = S.layerCtx;
    g.imageSmoothingEnabled = false;
    g.clearRect(0, 0, layer.width, layer.height);
    g.drawImage(S.slotFrame, 0, 0, 640, 240, 0, 0, layer.width, layer.height);
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
      withAbr(g, q.abr, (sub) =>
        g.drawImage(sub ? hubSilhouette(s) : s, q.u, q.v, q.w, q.h,
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
    btn.style.cssText = 'position:absolute;top:8px;right:8px;z-index:3;font:600 12px/1 system-ui,sans-serif;'
      + 'padding:6px 10px;border-radius:6px;border:1px solid #6c7a90;background:#1b2230;color:#e8ecf3;'
      + 'cursor:pointer;display:none;';
    const panel = document.createElement('div');
    panel.id = 'play-fishing-prizes';
    panel.style.cssText = 'position:absolute;top:40px;right:8px;z-index:3;width:300px;max-height:70%;overflow:auto;'
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
    if (S.game && S.game !== 'slot') releaseRenderer(rt, view);
    S.game = null;
    S.gen = -1;
    S.scene = null;
    S.slotCaption = null;
    showLayer(view, false);
  }

  function build(rt, view, info) {
    S.game = info.game;
    S.gen = info.gen;
    S.scene = null;
    S.hubSheets = {};
    S.hubDims = {};
    if (info.game === 'slot') { slotLoad(rt); return; }
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
