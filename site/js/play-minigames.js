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
  const ANIM_FPS = 14;                /* Baka clip rate */
  const HUD_W = 320, HUD_H = 240;     /* retail stage */

  /* Baka anim record slots (minigame-baka.js): 0 idle, 1..3 attacks, 4
   * special, 5 hit, 8 win flourish. */
  const ACT = { IDLE: 0, ATTACK1: 1, ATTACK2: 2, ATTACK3: 3, SPECIAL: 4, HIT: 5, WIN: 8 };
  /* Player battle-form clip slots (minigame-muscle.js P_ANIM). */
  const P_ANIM = { IDLE: 0, HIT: 2, KO: 4 };

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
    r.render(sc.cam.yaw, sc.cam.pitch, sc.cam.distance, 0, 0, sc.center, sc.radius, sc.fov);
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
    for (let i = 0; i < a.scene.paylines.length; i++) {
      const l = a.scene.paylines[i];
      const p = slotProject(l.a[0], l.a[1], l.a[2]);
      const q = slotProject(l.b[0], l.b[1], l.b[2]);
      g.strokeStyle = (i === winLine) ? 'rgba(255,255,128,0.95)' : 'rgba(190,190,190,0.35)';
      g.lineWidth = (i === winLine) ? 2 : 1;
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

  function pickMonsterClips(anims) {
    const byTag = (t) => anims.findIndex(a => a.action_id === t);
    const idle = byTag(0);
    let attack = byTag(0x21);
    if (attack < 0) attack = byTag(0x20);
    if (attack < 0) attack = anims.findIndex(a => a.action_id >= 0x20);
    if (attack < 0) attack = anims.length > 1 ? 1 : idle;
    let hit = byTag(2);
    if (hit < 0) hit = byTag(3);
    let ko = byTag(4);
    if (ko < 0) ko = hit;
    return { idle: Math.max(idle, 0), attack, hit: hit < 0 ? idle : hit, ko };
  }

  /* The retail battle ground grid (minigame-muscle.js groundBuffers). */
  function groundBuffers() {
    const out = { pos: [], uvs: [], ct: [], flat: [], idx: [] };
    const CELL = 0x200, SUB = 0x100, N = 14;
    const CBA = 0x77C0, TSB = 0x000D;
    for (let cz = -N; cz < N; cz++) for (let cx = -N; cx < N; cx++) {
      for (let sr = 0; sr < 2; sr++) for (let sc = 0; sc < 2; sc++) {
        const x0 = cx * CELL + sc * SUB, x1 = x0 + SUB;
        const z0 = cz * CELL + sr * SUB, z1 = z0 + SUB;
        const u0 = 192 + sc * 32, u1 = u0 + 31;
        const v0 = 192 + sr * 32, v1 = v0 + 31;
        const base = out.pos.length / 3;
        out.pos.push(x0, 0, z0, x1, 0, z0, x1, 0, z1, x0, 0, z1);
        out.uvs.push(u0, v0, u1, v0, u1, v1, u0, v1);
        for (let k = 0; k < 4; k++) { out.ct.push(CBA, TSB); out.flat.push(128, 128, 128, 255); }
        out.idx.push(base, base + 1, base + 2, base, base + 2, base + 3);
      }
    }
    return out;
  }
  function floorBuffers(extent) {
    const out = { pos: [], uvs: [], ct: [], flat: [], idx: [] };
    const T = Math.max(160, Math.round(extent / 4));
    const N = 12;
    for (let iz = -N; iz < N; iz++) for (let ix = -N; ix < N; ix++) {
      const dark = ((ix + iz) & 1) === 0;
      const c = dark ? [34, 36, 44] : [48, 52, 62];
      const base = out.pos.length / 3;
      const x0 = ix * T, x1 = x0 + T, z0 = iz * T, z1 = z0 + T;
      out.pos.push(x0, 0, z0, x1, 0, z0, x1, 0, z1, x0, 0, z1);
      for (let k = 0; k < 4; k++) { out.uvs.push(0, 0); out.ct.push(0, 0); out.flat.push(c[0], c[1], c[2], 0); }
      out.idx.push(base, base + 1, base + 2, base, base + 2, base + 3);
    }
    return out;
  }

  function muscleBuild(rt, view, info) {
    const m = info.muscle || {};
    const monsterId = m.monster_id, charSlot = m.char_slot | 0;
    if (monsterId == null) return null;
    if (!rt.play_mg_muscle_scene_ready || !rt.play_mg_muscle_scene_ready(monsterId, charSlot)) return null;
    const P = {
      pos: rt.play_mg_muscle_fighter_positions(charSlot),
      uvs: rt.play_mg_muscle_fighter_uvs(charSlot),
      ct: rt.play_mg_muscle_fighter_cba_tsb(charSlot),
      idx: rt.play_mg_muscle_fighter_indices(charSlot),
      oid: rt.play_mg_muscle_fighter_object_ids(charSlot),
      flat: rt.play_mg_muscle_fighter_flat_rgba(charSlot),
      parts: rt.play_mg_muscle_fighter_part_count(charSlot),
    };
    const M = {
      pos: rt.play_mg_muscle_monster_positions(monsterId),
      uvs: rt.play_mg_muscle_monster_uvs(monsterId),
      ct: rt.play_mg_muscle_monster_cba_tsb(monsterId),
      idx: rt.play_mg_muscle_monster_indices(monsterId),
      oid: rt.play_mg_muscle_monster_object_ids(monsterId),
      flat: rt.play_mg_muscle_monster_flat_rgba(monsterId),
      parts: rt.play_mg_muscle_monster_part_count(monsterId),
    };
    if (!P.pos.length || !M.pos.length) return null;
    const pAnims = parse(() => rt.play_mg_muscle_fighter_anims_json(charSlot)) || [];
    const pClip = (slot) => {
      const row = pAnims.find(a => a.slot === slot);
      if (!row || !row.frame_count) return null;
      const frames = rt.play_mg_muscle_fighter_pose_frames(charSlot, slot, P.parts);
      if (!frames.length) return null;
      return { frames, frameCount: row.frame_count, parts: P.parts, rate: Math.max(1, (row.rate || 1) * 2) };
    };
    const mAnims = parse(() => rt.play_mg_muscle_monster_anims_json(monsterId)) || [];
    const mPick = pickMonsterClips(mAnims);
    const mClip = (index) => {
      if (index < 0 || index >= mAnims.length) return null;
      const a = mAnims[index];
      const frames = rt.play_mg_muscle_monster_pose_frames(monsterId, index, M.parts);
      if (!frames.length) return null;
      return { frames, frameCount: a.frame_count, parts: M.parts, rate: Math.max(1, (a.rate || 1) * 2) };
    };
    const pIdle = pClip(P_ANIM.IDLE), pHit = pClip(P_ANIM.HIT);
    const clips = [
      { idle: pIdle, hit: pHit || pIdle, ko: pClip(P_ANIM.KO) || pHit || pIdle,
        byCmd: Object.fromEntries([12, 13, 14, 15].map(c => [c, pClip(c)])) },
      { idle: mClip(mPick.idle), hit: mClip(mPick.hit), attack: mClip(mPick.attack), ko: mClip(mPick.ko) },
    ];
    if (!clips[0].idle || !clips[1].idle) return null;
    const extP = poseExtent(P, clips[0].idle), extM = poseExtent(M, clips[1].idle);
    const gap = (extP.half + extM.half) * 1.5 + 120;
    const arenaPos = rt.play_mg_muscle_arena_positions();
    const arena = arenaPos.length ? {
      pos: arenaPos, uvs: rt.play_mg_muscle_arena_uvs(), ct: rt.play_mg_muscle_arena_cba_tsb(),
      flat: rt.play_mg_muscle_arena_flat_rgba(), idx: rt.play_mg_muscle_arena_indices(),
    } : null;
    const statics = arena ? [arena, groundBuffers()] : [floorBuffers(gap)];
    const buf = concatBuffers([P, M].concat(statics));
    const spreadZ = !!arena;
    const sc = {
      kind: 'muscle', P, M, nP: P.pos.length / 3, clips,
      base: buf.pos.slice(), out: buf.pos,
      dx: spreadZ ? [0, 0] : [-gap / 2, gap / 2],
      dz: spreadZ ? [-gap / 2, gap / 2] : [0, 0],
      yaw: spreadZ ? [0, Math.PI] : [Math.PI / 2, -Math.PI / 2],
      act: [{ clip: clips[0].idle, start: 0, loop: true }, { clip: clips[1].idle, start: 0, loop: true }],
      cam: { yaw: spreadZ ? Math.PI / 2 : 0.0, pitch: 0.14, distance: spreadZ ? 2.1 : 1.75 },
      center: [spreadZ ? 260 : 0, -Math.max(extP.height, extM.height) * 0.42, 0],
      radius: gap * 0.95 + Math.max(extP.half, extM.half) * 0.6,
      tick: 0, turnsSeen: 0, timers: [], phase: '',
    };
    if (!takeRenderer(view, rt.play_mg_muscle_vram(monsterId, charSlot), buf, { semiTwoPass: true })) return null;
    return sc;
  }

  function musclePlay(sc, fi, clip, hold) {
    if (!clip) return;
    sc.act[fi] = { clip, start: sc.tick, loop: false, hold: !!hold };
  }

  function muscleFrame(rt, view, skipDraw) {
    const sc = S.scene;
    const st = parse(() => rt.play_mg_muscle_state_json());
    if (sc && st && st.live) {
      sc.tick++;
      /* Resolved turn: replay its plays as swings, defender flinch on the
       * connect, 34 ticks per event (the standalone page's cadence). */
      if (st.turns_resolved !== sc.turnsSeen) {
        sc.turnsSeen = st.turns_resolved;
        let at = 0;
        for (const ev of (st.plays || [])) {
          const attacker = ev.attacker | 0, defender = attacker ^ 1;
          const swing = attacker === 0 ? (sc.clips[0].byCmd[ev.cmd] || sc.clips[0].idle) : sc.clips[1].attack;
          const hit = defender === 0 ? sc.clips[0].hit : sc.clips[1].hit;
          sc.timers.push({ at: sc.tick + at, fn: () => musclePlay(sc, attacker, swing) });
          if (ev.damage > 0) sc.timers.push({ at: sc.tick + at + 12, fn: () => musclePlay(sc, defender, hit) });
          at += 34;
        }
      }
      if (st.phase !== sc.phase) {
        sc.phase = st.phase;
        if (st.phase === 'won') musclePlay(sc, 1, sc.clips[1].ko, true);
        if (st.phase === 'lost') musclePlay(sc, 0, sc.clips[0].ko, true);
      }
      const due = sc.timers.filter(t => t.at <= sc.tick);
      sc.timers = sc.timers.filter(t => t.at > sc.tick);
      for (const t of due) t.fn();
      for (let fi = 0; fi < 2; fi++) {
        const a = sc.act[fi];
        let clip = a.clip || sc.clips[fi].idle;
        let frame;
        if (a.loop) {
          frame = Math.floor((sc.tick - a.start) * clip.rate / 16) % clip.frameCount;
        } else {
          frame = Math.floor((sc.tick - a.start) * clip.rate / 16);
          if (frame >= clip.frameCount) {
            if (a.hold) frame = clip.frameCount - 1;
            else { sc.act[fi] = { clip: sc.clips[fi].idle, start: sc.tick, loop: true }; clip = sc.clips[fi].idle; frame = 0; }
          }
        }
        const f = fi === 0 ? sc.P : sc.M;
        poseInto(sc.out, sc.base, f.oid, clip, frame, fi === 0 ? 0 : sc.nP, sc.dx[fi], sc.yaw[fi], sc.dz[fi]);
      }
    }
    if (skipDraw) return;
    if (sc) renderScene(view, sc); else clearGl(view);
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
      const s = hubSheet(rt, q.sheet, q.pal);
      if (!s) continue;
      g.drawImage(s, q.u, q.v, q.w, q.h, q.x * sx, q.y * sy, q.dw * sx, q.dh * sy);
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

  /* ================================================================== */
  /* Baka Fighter: the two fighters over the PROT 1203 stage wall + a floor
   * tiled from the wall's own dominant face (minigame-baka.js). */

  function bakaStageBuffers(rt, clearance) {
    const stage = { pos: [], uvs: [], ct: [], idx: [], flat: [] };
    const zBack = -Math.max(360, (clearance || 0) * 1.9);
    let wallNearZ = zBack;
    const sp = Array.from(rt.play_mg_baka_stage_positions(0));
    if (sp.length) {
      for (let i = 0; i < sp.length; i += 3) {
        sp[i] = -sp[i];
        sp[i + 2] = zBack - sp[i + 2];
        if (sp[i + 2] > wallNearZ) wallNearZ = sp[i + 2];
      }
      const base = stage.pos.length / 3;
      stage.pos.push(...sp);
      stage.uvs.push(...rt.play_mg_baka_stage_uvs(0));
      stage.ct.push(...rt.play_mg_baka_stage_cba_tsb(0));
      stage.flat.push(...rt.play_mg_baka_stage_flat_rgba(0));
      for (const ix of rt.play_mg_baka_stage_indices(0)) stage.idx.push(base + ix);
    }
    /* Floor: the wall's dominant textured face tiled on y = 0. */
    let best = null, bestArea = 0;
    for (let t = 0; t + 2 < stage.idx.length; t += 3) {
      const a = stage.idx[t], b = stage.idx[t + 1], c = stage.idx[t + 2];
      if (stage.flat[a * 4 + 3] === 0) continue;
      const us = [stage.uvs[a * 2], stage.uvs[b * 2], stage.uvs[c * 2]];
      const vs = [stage.uvs[a * 2 + 1], stage.uvs[b * 2 + 1], stage.uvs[c * 2 + 1]];
      const xs = [stage.pos[a * 3], stage.pos[b * 3], stage.pos[c * 3]];
      const ys = [stage.pos[a * 3 + 1], stage.pos[b * 3 + 1], stage.pos[c * 3 + 1]];
      const ww = Math.max(...xs) - Math.min(...xs);
      const wh = Math.max(...ys) - Math.min(...ys);
      if (ww * wh <= bestArea) continue;
      bestArea = ww * wh;
      best = { u0: Math.min(...us), u1: Math.max(...us), v0: Math.min(...vs), v1: Math.max(...vs),
        cba: stage.ct[a * 2], tsb: stage.ct[a * 2 + 1], tw: Math.max(64, ww), th: Math.max(64, wh) };
    }
    if (best) {
      const X0 = -1750, X1 = 1750, Z0 = wallNearZ, Z1 = 520;
      const nx = Math.ceil((X1 - X0) / best.tw), nz = Math.ceil((Z1 - Z0) / best.th);
      for (let iz = 0; iz < nz; iz++) for (let ix = 0; ix < nx; ix++) {
        const x0 = X0 + ix * best.tw, x1 = Math.min(x0 + best.tw, X1);
        const z0 = Z0 + iz * best.th, z1 = Math.min(z0 + best.th, Z1);
        const base = stage.pos.length / 3;
        stage.pos.push(x0, 0, z0, x1, 0, z0, x1, 0, z1, x0, 0, z1);
        stage.uvs.push(best.u0, best.v0, best.u1, best.v0, best.u1, best.v1, best.u0, best.v1);
        for (let k = 0; k < 4; k++) { stage.ct.push(best.cba, best.tsb); stage.flat.push(128, 128, 128, 255); }
        stage.idx.push(base, base + 1, base + 2, base, base + 2, base + 3);
      }
    }
    return stage;
  }

  function bakaBuild(rt, view, info) {
    const b = info.baka || {};
    const playerChar = b.player_char | 0, opponent = b.opponent | 0;
    /* Roster rows 0..2 are the party-side fighters (one shared pack, side
     * 0); the ladder rows 3..16 carry their own packs (side 1). */
    const oppSide = b.opponent_side | 0;
    if (!opponent) return null;
    if (!rt.play_mg_baka_presentation_ready || !rt.play_mg_baka_presentation_ready()) return null;
    const facing = parse(() => rt.play_mg_baka_duel_facing_json())
      || { player: { side: -1, facing: 1 }, opponent: { side: 1, facing: -1 } };
    const side = (s, id) => {
      const pos = rt.play_mg_baka_fighter_positions(s, id);
      if (!pos.length) return null;
      return {
        pos, uvs: rt.play_mg_baka_fighter_uvs(s, id), ct: rt.play_mg_baka_fighter_cba_tsb(s, id),
        idx: rt.play_mg_baka_fighter_indices(s, id), oid: rt.play_mg_baka_fighter_object_ids(s, id),
        flat: rt.play_mg_baka_fighter_flat_rgba(s, id), parts: rt.play_mg_baka_fighter_part_count(s, id),
      };
    };
    const P = side(0, playerChar), O = side(oppSide, opponent);
    if (!P || !O) return null;
    const cache = new Map();
    const clipFor = (fi, action) => {
      const key = fi + ':' + action;
      if (!cache.has(key)) {
        const s = fi === 0 ? 0 : oppSide, id = fi === 0 ? playerChar : opponent, parts = fi === 0 ? P.parts : O.parts;
        const dims = rt.play_mg_baka_anim_dims(s, id, action);
        let clip = null;
        if (dims[0] && dims[1]) {
          const frames = rt.play_mg_baka_anim_pose_frames(s, id, action, parts);
          if (frames.length) clip = { frames, frameCount: dims[1], parts };
        }
        cache.set(key, clip);
      }
      return cache.get(key);
    };
    const idleP = clipFor(0, ACT.IDLE), idleO = clipFor(1, ACT.IDLE);
    if (!idleP || !idleO) return null;
    const halfP = poseExtent(P, idleP).half, halfO = poseExtent(O, idleO).half;
    const gap = Math.max(halfP, halfO) * 2.4;
    const stage = bakaStageBuffers(rt, Math.max(halfP, halfO));
    const buf = concatBuffers([P, O, stage]);
    const sc = {
      kind: 'baka', P, O, nP: P.pos.length / 3, clipFor, facing,
      base: buf.pos.slice(), out: buf.pos, gap,
      center: [0, -halfP * 0.8, 0],
      radius: gap * 0.95 + Math.max(halfP, halfO) * 0.4,
      cam: { yaw: 0.0, pitch: 0.1, distance: 1.7 },
      action: [{ id: ACT.IDLE, start: 0, loop: true }, { id: ACT.IDLE, start: 0, loop: true }],
      tick: 0, lastKey: '', victory: null, overSeen: false,
    };
    if (!takeRenderer(view, rt.play_mg_baka_duel_vram(opponent), buf, {})) return null;
    return sc;
  }

  function bakaPlay(sc, fi, actionId, hold) {
    const c = sc.clipFor(fi, actionId);
    sc.action[fi] = c ? { id: actionId, start: sc.tick, loop: actionId === ACT.IDLE, hold: !!hold }
                      : { id: ACT.IDLE, start: sc.tick, loop: true };
  }

  function bakaFrame(rt, view, skipDraw) {
    const sc = S.scene;
    const st = parse(() => rt.play_mg_baka_state_json());
    if (sc && st && st.live) {
      sc.tick++;
      if (st.last) {
        const key = JSON.stringify(st.last) + ':' + st.round;
        if (key !== sc.lastKey) {
          sc.lastKey = key;
          const l = st.last;
          if (!l.draw) {
            const winner = l.winner, loser = 1 - l.winner;
            const t = st.chosen && st.chosen[winner];
            const atk = l.special ? ACT.SPECIAL : t === 2 ? ACT.ATTACK2 : t === 3 ? ACT.ATTACK3 : ACT.ATTACK1;
            bakaPlay(sc, winner, atk);
            bakaPlay(sc, loser, ACT.HIT);
          } else {
            bakaPlay(sc, 0, ACT.ATTACK1);
            bakaPlay(sc, 1, ACT.ATTACK1);
          }
        }
      }
      if (st.phase === 'match_over' && !sc.overSeen && st.winner != null) {
        sc.overSeen = true;
        sc.victory = { fi: st.winner, step: 0, nextAt: sc.tick + 12 };
        bakaPlay(sc, 1 - st.winner, ACT.HIT, true);
      }
      if (sc.victory) {
        const v = sc.victory;
        if (sc.tick >= v.nextAt) {
          if (v.step < 5) {
            bakaPlay(sc, v.fi, [ACT.ATTACK1, ACT.ATTACK3, ACT.ATTACK2][v.step % 3]);
            v.nextAt = sc.tick + 34; v.step++;
          } else sc.victory = null;
        }
      }
      for (let fi = 0; fi < 2; fi++) {
        const a = sc.action[fi];
        const c = sc.clipFor(fi, a.id);
        if (!a.loop && !a.hold && c) {
          const f = Math.floor((sc.tick - a.start) * (ANIM_FPS / 60));
          if (f >= c.frameCount) sc.action[fi] = { id: ACT.IDLE, start: sc.tick, loop: true };
        }
      }
      const poseFighter = (fi, f, vertBase, dx, yaw) => {
        const a = sc.action[fi];
        const c = sc.clipFor(fi, a.id) || sc.clipFor(fi, ACT.IDLE);
        if (!c) return;
        const rawF = Math.floor((sc.tick - a.start) * (ANIM_FPS / 60));
        const frame = a.loop ? rawF % c.frameCount : Math.min(rawF, c.frameCount - 1);
        poseInto(sc.out, sc.base, f.oid, c, frame, vertBase, dx, yaw, 0);
      };
      const F = sc.facing;
      poseFighter(0, sc.P, 0, F.player.side * sc.gap / 2, F.player.facing * Math.PI / 2);
      poseFighter(1, sc.O, sc.nP, F.opponent.side * sc.gap / 2, F.opponent.facing * Math.PI / 2);
    }
    if (skipDraw) return;
    if (sc) renderScene(view, sc); else clearGl(view);
  }

  /* ================================================================== */
  /* Dance: Noa + the hall's dancer NPCs over the baked `other7` hall, posed
   * off the scene's choreography bundle (minigame-dance.js). */

  function danceBuild(rt, view) {
    if (!rt.play_mg_dance_body_ready || !rt.play_mg_dance_body_ready()) return null;
    const count = rt.play_mg_dance_body_count();
    if (!count) return null;
    const cast = parse(() => rt.play_mg_dance_cast_json());
    if (!cast) return null;
    const dancers = [];
    for (let d = 0; d < count; d++) {
      const pos = rt.play_mg_dance_body_positions(d);
      if (!pos.length) return null;
      dancers.push({
        pos, uvs: rt.play_mg_dance_body_uvs(d), ct: rt.play_mg_dance_body_cba_tsb(d),
        idx: rt.play_mg_dance_body_indices(d), oid: rt.play_mg_dance_body_object_ids(d),
        flat: rt.play_mg_dance_body_flat_rgba(d), parts: rt.play_mg_dance_body_part_count(d),
        kind: cast.dancers[d] ? cast.dancers[d].kind : 0,
      });
    }
    const clip = (d, c, parts) => {
      const dims = rt.play_mg_dance_body_anim_dims(d, c);
      if (!dims[0] || !dims[1]) return null;
      const frames = rt.play_mg_dance_body_pose_frames(d, c, parts);
      if (!frames.length) return null;
      const meta = (cast.dancers[d].clips || [])[c] || {};
      return { frames, frameCount: dims[1], parts, rate: meta.rate || 8 };
    };
    const clips = dancers.map((f, d) => {
      const per = [];
      for (let c = 0; c < (cast.dancers[d].clips || []).length; c++) per.push(clip(d, c, f.parts));
      return per;
    });
    const anim = dancers.map(() => ({ cursor: 0, move: null }));
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
    const halfOf = (f, cl) => {
      const c = cl[0] || cl[1];
      if (!c) return 200;
      return poseExtent(f, c).half;
    };
    const maxHalf = Math.max.apply(null, dancers.map((f, d) => halfOf(f, clips[d])));
    const human = rt.play_mg_dance_body_human_index();
    const humanX = cast.dancers[human] ? cast.dancers[human].x : 0;
    const dx = dancers.map((_, d) => cast.dancers[d] ? (cast.dancers[d].x - humanX) : 0);
    const spread = Math.max.apply(null, dx.map(Math.abs)) || maxHalf;
    const parts = dancers.slice();
    if (env) parts.push(env);
    if (markers) parts.push(markers);
    const buf = concatBuffers(parts);
    const sc = {
      kind: 'dance', dancers, clips, anim, moves: cast.moves, dx,
      vertBases: buf.bases.slice(0, dancers.length),
      markerBase: markers ? buf.bases[dancers.length + (env ? 1 : 0)] : -1,
      base: buf.pos.slice(), out: buf.pos,
      lastBeat: -1, rivalTri: {}, human,
      cam: env ? { yaw: Math.PI, pitch: 0.24, distance: 2.7 } : { yaw: 0.0, pitch: 0.12, distance: 1.9 },
      center: [0, -maxHalf * (env ? 1.6 : 0.85), 0],
      radius: spread * 1.15 + maxHalf * 1.05,
      fov: env ? 0.85 : undefined,
      faceYaw: env ? 0 : Math.PI,
    };
    const flags = env ? { cullBackfaces: true, cullFrontFace: 'ccw', semiTwoPass: true } : {};
    if (!takeRenderer(view, rt.play_mg_dance_body_vram(), buf, flags)) return null;
    return sc;
  }

  function danceTrigger(sc, d, clipId) {
    if (!sc.clips[d] || !sc.clips[d][clipId]) return;
    sc.anim[d].move = clipId;
    sc.anim[d].cursor = 0;
  }
  function danceAdvance(sc, d, live) {
    const a = sc.anim[d];
    const loopId = live ? 1 : 0;
    let clipId = a.move !== null ? a.move : loopId;
    let c = sc.clips[d][clipId] || sc.clips[d][loopId] || sc.clips[d][0];
    if (!c) return { clip: null, frame: 0 };
    const last = c.frameCount * 16 - 1;
    if (a.move !== null && a.cursor >= last) {
      a.move = null; a.cursor = 0; clipId = loopId;
      c = sc.clips[d][clipId] || sc.clips[d][0];
      if (!c) return { clip: null, frame: 0 };
    }
    const frame = Math.min(a.cursor >> 4, c.frameCount - 1);
    a.cursor += c.rate;
    if (a.move === null && a.cursor > last) a.cursor = 0;
    return { clip: c, frame };
  }

  function danceFrame(rt, view, skipDraw) {
    const sc = S.scene;
    const st = parse(() => rt.play_mg_dance_state_json());
    if (sc) {
      const live = !!(st && st.live);
      const chart = live ? parse(() => rt.play_mg_dance_chart_json()) : null;
      const rivals = (st && st.rivals) || [];
      if (live && chart && sc.moves) {
        const beat = st.beat | 0;
        const rivalOf = (d) => rivals.find(r => r.kind === sc.dancers[d].kind);
        for (let d = 0; d < sc.dancers.length; d++) {
          if (d === sc.human) continue;
          const rv = rivalOf(d);
          if (!rv) continue;
          const lane = Math.min(rv.lane | 0, chart.rows.length - 1);
          const was = sc.rivalTri[d];
          sc.rivalTri[d] = rv.triangles;
          if (was !== undefined && rv.triangles < was) {
            danceTrigger(sc, d, sc.moves.beat[Math.min(lane, 2)]);
          } else if (beat !== sc.lastBeat) {
            const rowc = chart.rows[lane];
            const sym = rowc ? rowc[beat % rowc.length] : 0;
            if (sym === 1) danceTrigger(sc, d, sc.moves.seq_square[Math.min(lane, 2)]);
            else if (sym === 2) danceTrigger(sc, d, sc.moves.seq_circle[Math.min(lane, 2)]);
          }
        }
        /* The human's own judged press: the sequence move on a hit. */
        if (st.judged != null && beat !== sc.lastBeat) {
          const lane = Math.min(st.lane | 0, 2);
          if (st.judged === 1) danceTrigger(sc, sc.human, sc.moves.seq_square[lane]);
          else if (st.judged === 2) danceTrigger(sc, sc.human, sc.moves.seq_circle[lane]);
          else if (st.judged === 3) danceTrigger(sc, sc.human, sc.moves.beat[lane]);
        }
        sc.lastBeat = beat;
      } else {
        sc.lastBeat = -1; sc.rivalTri = {};
      }
      for (let d = 0; d < sc.dancers.length; d++) {
        const adv = danceAdvance(sc, d, live);
        if (!adv.clip) continue;
        poseInto(sc.out, sc.base, sc.dancers[d].oid, adv.clip, adv.frame, sc.vertBases[d], sc.dx[d], sc.faceYaw, 0);
      }
      if (sc.markerBase >= 0) {
        const mp = rt.play_mg_dance_marker_step(1);
        if (mp.length) sc.out.set(mp, sc.markerBase * 3);
      }
    }
    if (skipDraw) return;
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
      if (info.game === 'muscle') S.scene = muscleBuild(rt, view, info);
      else if (info.game === 'baka') S.scene = bakaBuild(rt, view, info);
      else if (info.game === 'dance') S.scene = danceBuild(rt, view);
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
      /* The arena hub outlives the leg: the INTERVAL + tally screen and the
       * re-entered hub's ringside still play after the dome has handed the
       * field back, so they are drawn over it here. */
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
