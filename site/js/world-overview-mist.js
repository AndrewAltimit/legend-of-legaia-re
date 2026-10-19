/* world-overview-mist.js - the Mist over a kingdom, drawn the way retail
 * draws it on the overworld: the field fog pool.
 *
 * Retail has no separate "Mist" renderer on the world map. The kingdom's
 * scene system script raises the ambient-particle gate (field-VM op 4C 30)
 * and rewrites each MAN section-4 fog region's enable byte from the inverse
 * of its story flag (op 4C C1), and the fog pool then spawns additive
 * cloud half-sheets inside every still-enabled region near the player:
 * spawner FUN_801D629C, walk FUN_8003F348, per-particle update FUN_8003F3FC,
 * half-sheet emitter FUN_8003F86C. The regions' flags are the per-area Mist
 * lifts (0x51A..0x521 and siblings), so a region stops drawing when its
 * area's Mist is cleared. docs/subsystems/field-ambient-fx.md (the fog pool
 * and "The pool on the kingdom overworld") and
 * docs/subsystems/world-overview-viewer.md#the-mist carry the provenance;
 * crates/engine-field/src/fog_particles.rs is the engine's port.
 *
 * What this page keeps from retail: the region table off the disc, the
 * first-region-wins test, the spawn record (drift heading + speed, height,
 * grey, age rate), the brightness ramp, the half-width growth, the two
 * half-sheets with their staged UV rows, the sheet art (PROT 0874 section 2
 * page (448, 0) through CLUT (0, 473)), the 5-bit texture blend and the
 * additive (ABR 1) composite. What it changes: retail's pool only lives in
 * the player's visible-tile window under a 0x48 cap; here the same density
 * (cap and spawn rate scaled by area) covers the whole continent, as if the
 * player stood everywhere at once. The overworld spawner's camera-depth
 * drop and the player-proximity ageing key on retail's walk camera and
 * player, which this orbit view has neither of, so they are not modelled.
 *
 * Classic script; loads before world-overview-app.js. */
(function () {
  'use strict';

  /* Retail constants (crates/engine-field/src/fog_particles.rs). */
  const FOG_CAP_RAISED = 0x48;          /* overworld cap (MAN[1] & 1) */
  const FOG_OVERWORLD_LIFT = 0x28;
  const FOG_SHEET_HEIGHT = 0x80;
  const FOG_WIDTH_BASE = 0x180;
  const FOG_FADE_IN_END = 0x400;
  const FOG_FADE_OUT_START = 0xC00;
  const FRAME_DT = 2;                   /* DAT_1F800393: 30 fps game frames */
  const FRAME_SEC = 2 / 60;
  /* The emitter: 24 draws a frame, a 1-in-16 burst of (rand & 3) + 1. */
  const EMITTER_DRAWS = 24;
  /* map01's visible-tile window (-18, -12, 18, 32): 36 x 44 tiles. The pool
   * density (cap per window) and spawn rate scale from it to the map. */
  const WINDOW_TILES = 36 * 44;
  const MAP_TILES = 128;
  const SHEET_W = 64;
  const SHEET_H = 48;
  /* Overworld near cull (SZ 0x310) in this view's 1x eye units. */
  const NEAR_W = 0x310 / 6;

  /* BIOS rand(): 15 bits. Not retail's seed - a frame of retail fog is one
   * draw of the rand() stream, so a frame comparison scores placement, not
   * presence (field-ambient-fx.md). */
  const rand = () => (Math.random() * 0x8000) | 0;

  const VS = `#version 300 es
precision highp float;
layout(location = 0) in vec3 a_center;
layout(location = 1) in vec2 a_corner;
layout(location = 2) in vec2 a_uv;
layout(location = 3) in float a_c8;
uniform mat4 u_vp;
uniform vec2 u_proj;
out vec2 v_uv;
out float v_c8;
void main() {
  vec4 clip = u_vp * vec4(a_center, 1.0);
  /* A view-space billboard: the corner offsets ride the screen axes at the
   * particle's depth, like FUN_8003F86C's RTPT of (dx, 0, 0x80). */
  clip.xy += a_corner * u_proj;
  gl_Position = clip;
  v_uv = a_uv;
  v_c8 = a_c8;
}`;

  const FS = `#version 300 es
precision highp float;
uniform sampler2D u_sheet;
in vec2 v_uv;
in float v_c8;
out vec4 o_color;
void main() {
  vec4 t = texture(u_sheet, v_uv);
  if (t.a < 0.5) discard;
  /* The GPU's 5-bit texture blend: (t5 * c8) >> 7, saturated at 31. */
  vec3 t5 = floor(t.rgb * 31.0 + 0.5);
  vec3 o5 = min(floor(t5 * v_c8 / 128.0), vec3(31.0));
  o_color = vec4(o5 / 31.0, 1.0);
}`;

  function compile(gl, type, src) {
    const sh = gl.createShader(type);
    gl.shaderSource(sh, src);
    gl.compileShader(sh);
    if (!gl.getShaderParameter(sh, gl.COMPILE_STATUS)) {
      throw new Error('mist shader: ' + gl.getShaderInfoLog(sh));
    }
    return sh;
  }

  /* `regions` is the flat u16 list of walk_mist_regions (9 words each). */
  function decodeRegions(flat) {
    const out = [];
    for (let i = 0; i + 8 < flat.length; i += 9) {
      out.push({
        x0: flat[i + 1], z0: flat[i + 2], x1: flat[i + 3], z1: flat[i + 4],
        angleBase: flat[i + 5], angleSpread: flat[i + 6], speed: flat[i + 7],
        flag: flat[i + 8],
      });
    }
    return out;
  }

  function brightness(age) {
    let s;
    if (age < FOG_FADE_IN_END) s = age >> 2;
    else if (age <= FOG_FADE_OUT_START) s = 0x100;
    else s = 0xFF - ((age - FOG_FADE_OUT_START) >> 2);
    return s < 0 ? -1 : Math.min(s, 0xFF);
  }

  function create(gl) {
    const prog = gl.createProgram();
    gl.attachShader(prog, compile(gl, gl.VERTEX_SHADER, VS));
    gl.attachShader(prog, compile(gl, gl.FRAGMENT_SHADER, FS));
    gl.linkProgram(prog);
    if (!gl.getProgramParameter(prog, gl.LINK_STATUS)) {
      throw new Error('mist link: ' + gl.getProgramInfoLog(prog));
    }
    const locVp = gl.getUniformLocation(prog, 'u_vp');
    const locProj = gl.getUniformLocation(prog, 'u_proj');
    const locSheet = gl.getUniformLocation(prog, 'u_sheet');
    const vao = gl.createVertexArray();
    const buf = gl.createBuffer();
    const tex = gl.createTexture();
    const STRIDE = 8; /* center xyz, corner xy, uv, c8 */
    gl.bindVertexArray(vao);
    gl.bindBuffer(gl.ARRAY_BUFFER, buf);
    gl.enableVertexAttribArray(0);
    gl.vertexAttribPointer(0, 3, gl.FLOAT, false, STRIDE * 4, 0);
    gl.enableVertexAttribArray(1);
    gl.vertexAttribPointer(1, 2, gl.FLOAT, false, STRIDE * 4, 12);
    gl.enableVertexAttribArray(2);
    gl.vertexAttribPointer(2, 2, gl.FLOAT, false, STRIDE * 4, 20);
    gl.enableVertexAttribArray(3);
    gl.vertexAttribPointer(3, 1, gl.FLOAT, false, STRIDE * 4, 28);
    gl.bindVertexArray(null);

    let hasSheet = false;
    let regions = [];
    let flagsSet = new Set();
    let groundUp = () => 0;
    let yControl = 60;
    let cap = 0;
    let drawsPerFrame = 0;
    let mistTiles = 0;
    let particles = [];
    let live = 0;
    let lastTs = 0;
    let acc = 0;
    let verts = new Float32Array(0);

    /* First region whose open box holds the tile; null when none or when
     * that region is disabled (the spawner stops at the first hit). */
    function regionAt(tx, tz) {
      for (const r of regions) {
        if (r.x0 < tx && tx < r.x1 && r.z0 < tz && tz < r.z1) {
          return flagsSet.has(r.flag) ? null : r;
        }
      }
      return null;
    }

    function recount() {
      mistTiles = 0;
      for (let tz = 0; tz < MAP_TILES; tz++) {
        for (let tx = 0; tx < MAP_TILES; tx++) if (regionAt(tx, tz)) mistTiles++;
      }
      cap = Math.round(FOG_CAP_RAISED * mistTiles / WINDOW_TILES);
      drawsPerFrame = Math.round(EMITTER_DRAWS * MAP_TILES * MAP_TILES / WINDOW_TILES);
      particles = [];
      live = 0;
    }

    /* FUN_801D629C at tile (tx, tz), overworld arm. */
    function spawn(tx, tz) {
      if (tx < 0 || tz < 0 || tx >= MAP_TILES || tz >= MAP_TILES) return;
      const r = regionAt(tx, tz);
      if (!r || live >= cap) return;
      const base = r.angleBase << 4;
      const spread = r.angleSpread << 4;
      const angle = (base + (((rand() & 0xFFF) * spread) >> 12)) & 0xFFF;
      const y = -(rand() & 0x7F);
      const grey = rand() & 0x7F;
      const rate = (rand() & 7) + 8;
      const th = angle * Math.PI * 2 / 4096;
      const s = Math.round(Math.sin(th) * 4096);
      const c = Math.round(Math.cos(th) * 4096);
      particles.push({
        age: 0, rate, grey,
        vx: -((s * r.speed) >> 14), vz: -((c * r.speed) >> 14),
        x: (tx << 7) << 4, z: (tz << 7) << 4,
        y: y - FOG_OVERWORLD_LIFT,
        floor: groundUp(tx << 7, tz << 7),
      });
    }

    /* One game frame: the emitter's bursts, then the pool walk's age +
     * drift (FUN_8003F3FC; the colour is taken from the pre-update age). */
    function step() {
      for (let i = 0; i < drawsPerFrame; i++) {
        if ((rand() & 0xF) !== 0) continue;
        const n = (rand() & 3) + 1;
        const tx = rand() % MAP_TILES;
        const tz = rand() % MAP_TILES;
        for (let k = 0; k < n; k++) spawn(tx, tz);
      }
      const keep = [];
      for (const p of particles) {
        const b = brightness(p.age);
        const wx = p.x >> 4, wz = p.z >> 4;
        if (b < 0 || wx < 0 || wz < 0 || wx >= MAP_TILES * 128 || wz >= MAP_TILES * 128) continue;
        /* grey * tint * brightness >> 15 with the neutral 0x80 tint. */
        p.c8 = (p.grey * 0x80 * b) >> 15;
        p.drawX = wx; p.drawZ = wz;
        p.x += p.vx * FRAME_DT;
        p.z += p.vz * FRAME_DT;
        p.age += p.rate * FRAME_DT;
        keep.push(p);
      }
      particles = keep;
      live = particles.length;
    }

    function buildVerts(vp) {
      const need = particles.length * 12 * STRIDE;
      if (verts.length < need) verts = new Float32Array(need);
      let o = 0;
      const put = (cx, cy, cz, dx, dy, u, v, c8) => {
        verts[o++] = cx; verts[o++] = cy; verts[o++] = cz;
        verts[o++] = dx; verts[o++] = dy;
        verts[o++] = u; verts[o++] = v; verts[o++] = c8;
      };
      for (const p of particles) {
        if (!(p.c8 > 0)) continue;
        /* Height: retail's y less the camera vertical offset, which the
         * kingdom's system script eases to (control word - player floor);
         * the floor here is the particle's own tile. Y-up. */
        const cy = p.floor + yControl - p.y;
        const w = vp[3] * p.drawX + vp[7] * cy + vp[11] * p.drawZ + vp[15];
        if (w < NEAR_W) continue;
        const hw = (FOG_WIDTH_BASE + (p.age >> 4)) >> 1;
        /* Left half: staged UV row 1 (sheet rows 24..47); right: row 0. */
        for (const [x0, x1, r0] of [[-2 * hw, 0, 24], [0, 2 * hw, 0]]) {
          const u0 = 0.5 / SHEET_W, u1 = 63.5 / SHEET_W;
          const vt = (r0 + 0.5) / SHEET_H, vb = (r0 + 23.5) / SHEET_H;
          const yt = FOG_SHEET_HEIGHT;
          put(p.drawX, cy, p.drawZ, x0, yt, u0, vt, p.c8);
          put(p.drawX, cy, p.drawZ, x1, yt, u1, vt, p.c8);
          put(p.drawX, cy, p.drawZ, x0, 0, u0, vb, p.c8);
          put(p.drawX, cy, p.drawZ, x1, yt, u1, vt, p.c8);
          put(p.drawX, cy, p.drawZ, x1, 0, u1, vb, p.c8);
          put(p.drawX, cy, p.drawZ, x0, 0, u0, vb, p.c8);
        }
      }
      return o / STRIDE;
    }

    return {
      /* The sheet art (mist_sheet_rgba, 64 x 48 RGBA). */
      setSheet(rgba) {
        if (!rgba || rgba.length < SHEET_W * SHEET_H * 4) { hasSheet = false; return; }
        gl.bindTexture(gl.TEXTURE_2D, tex);
        gl.pixelStorei(gl.UNPACK_ALIGNMENT, 1);
        gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA8, SHEET_W, SHEET_H, 0, gl.RGBA,
          gl.UNSIGNED_BYTE, rgba);
        gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.NEAREST);
        gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.NEAREST);
        gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
        gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
        hasSheet = true;
      },
      /* A kingdom: its region list, the story flags taken as set, the
       * terrain sampler (world-up Y) and the camera-offset control word. */
      setKingdom(opts) {
        regions = decodeRegions(opts.regions || []);
        flagsSet = new Set(opts.flagsSet || []);
        groundUp = opts.groundUp || (() => 0);
        yControl = opts.yControl || 60;
        recount();
        /* Pre-run the pool to its steady state so the first frame shows
         * the bank, not an empty map filling up. */
        for (let i = 0; i < 240; i++) step();
      },
      regions() { return regions; },
      stats() { return { regions: regions.length, mistTiles, cap, live }; },
      /* Advance on wall-clock time and draw over the finished scene (same
       * framebuffer + depth buffer). `vp` is the scene's view-projection. */
      draw(vp) {
        if (!hasSheet || !vp || regions.length === 0) return;
        const now = performance.now() / 1000;
        if (lastTs === 0) lastTs = now;
        acc = Math.min(acc + (now - lastTs), 0.5);
        lastTs = now;
        while (acc >= FRAME_SEC) { step(); acc -= FRAME_SEC; }
        const n = buildVerts(vp);
        if (n === 0) return;
        gl.useProgram(prog);
        gl.uniformMatrix4fv(locVp, false, vp);
        gl.uniform2f(locProj,
          Math.hypot(vp[0], vp[4], vp[8]), Math.hypot(vp[1], vp[5], vp[9]));
        gl.activeTexture(gl.TEXTURE0);
        gl.bindTexture(gl.TEXTURE_2D, tex);
        gl.uniform1i(locSheet, 0);
        gl.bindVertexArray(vao);
        gl.bindBuffer(gl.ARRAY_BUFFER, buf);
        gl.bufferData(gl.ARRAY_BUFFER, verts.subarray(0, n * STRIDE), gl.STREAM_DRAW);
        gl.enable(gl.DEPTH_TEST);
        gl.depthFunc(gl.LEQUAL);
        gl.depthMask(false);
        gl.enable(gl.BLEND);
        gl.blendEquation(gl.FUNC_ADD);
        gl.blendFunc(gl.ONE, gl.ONE);
        gl.drawArrays(gl.TRIANGLES, 0, n);
        gl.disable(gl.BLEND);
        gl.depthMask(true);
        gl.bindVertexArray(null);
      },
      dispose() {
        gl.deleteBuffer(buf);
        gl.deleteVertexArray(vao);
        gl.deleteTexture(tex);
        gl.deleteProgram(prog);
      },
    };
  }

  window.LegaiaWorldOverviewMist = { create, decodeRegions };
})();
