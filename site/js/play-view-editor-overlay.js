/* play-view-editor-overlay.js - PlayView's expansion-editor overlay.
 *
 * Part of the PlayView class (play-app.js). The expansion workbench's 3D
 * preview is this page embedded (`play.html?xwpreview=1`), drawing the pack
 * map through the ordinary scene build; this file adds what an editor needs
 * on top of it, and nothing that changes the game picture:
 *
 *   - translucent volumes and markers for the active map's walls, zones,
 *     triggers and exits, spawns, NPCs and props, plus the selection the
 *     workbench's 2D painter holds and a cutscene's camera path, built from
 *     the engine's own reading of the map (`rt.expansion_overlay_json`: the
 *     definition the engine runs, each tile's floor from its floor sampler);
 *   - `pickTile(clientX, clientY)`: the map tile under a screen point,
 *     ray-cast against those floors through the frame's own view-projection;
 *   - the orbit focus the preview's free camera turns around
 *     (`editorFocus`, read by `_stageEngineCamera` under the debug orbit).
 *
 * The volumes draw after the 3D pass (and the fog): once without the depth
 * test, faint, so a marker behind a wall still shows where it is, then with
 * it, in the scene's log-depth space (LOG_DEPTH_GLSL), so what is in front
 * reads solid. Vertices are retail Y-down world, so the matrix carries this
 * page's Y flip, as the glow pass's does.
 *
 * Classic script. Requires webgl-math.js, webgl-shaders.js and play-app.js
 * first. */
(function () {
  'use strict';

  const EDITOR_VS = `#version 300 es
precision highp float;
uniform mat4 u_mvp;
in vec3 a_position;
in vec4 a_color;
out vec4 v_color;
out float v_depth_w;
void main() {
  gl_Position = u_mvp * vec4(a_position, 1.0);
  v_color = a_color;
  v_depth_w = gl_Position.w;
}
`;

  const EDITOR_FS = `#version 300 es
precision highp float;
in vec4 v_color;
in float v_depth_w;
uniform int u_log_depth_on;
uniform float u_alpha;
out vec4 o_color;
${LOG_DEPTH_GLSL}
void main() {
  gl_FragDepth = u_log_depth_on != 0 ? logDepthOfW(v_depth_w) : gl_FragCoord.z;
  o_color = vec4(v_color.rgb, v_color.a * u_alpha);
}
`;

  /* Layer colours, RGBA 0..1: the 2D painter's (expansion-workbench-map.js). */
  const COLOR = {
    walls: [0.72, 0.80, 0.95, 0.26],
    encounters: [0.88, 0.38, 0.35, 0.28],
    triggers: [0.90, 0.64, 0.24, 0.36],
    exits: [0.29, 0.64, 0.91, 0.40],
    spawns: [0.24, 0.81, 0.56, 0.75],
    npcs: [0.61, 0.42, 0.94, 0.70],
    props: [0.85, 0.77, 0.42, 0.45],
    select: [1.00, 1.00, 1.00, 0.30],
    grid: [1.00, 1.00, 1.00, 0.22],
    slope: [0.98, 0.78, 0.30, 0.20],
    path: [1.00, 0.95, 0.55, 0.85],
    marker: [1.00, 0.30, 0.20, 0.90],
  };
  /* Default layer set: every volume but the props (the props draw as
   * themselves), and the tile grid. */
  const DEFAULT_LAYERS = { grid: true, walls: true, zones: true, triggers: true, npcs: true, spawns: true, props: false };
  /* What a workbench draft may replace in the engine's reading of the map
   * (`defs`, see `setEditorOverlay`). */
  const DRAFT_LISTS = ['spawns', 'exits', 'triggers', 'encounters', 'props', 'npcs'];

  function invert4(m) {
    const a = m, o = new Float32Array(16);
    const b00 = a[0] * a[5] - a[1] * a[4], b01 = a[0] * a[6] - a[2] * a[4];
    const b02 = a[0] * a[7] - a[3] * a[4], b03 = a[1] * a[6] - a[2] * a[5];
    const b04 = a[1] * a[7] - a[3] * a[5], b05 = a[2] * a[7] - a[3] * a[6];
    const b06 = a[8] * a[13] - a[9] * a[12], b07 = a[8] * a[14] - a[10] * a[12];
    const b08 = a[8] * a[15] - a[11] * a[12], b09 = a[9] * a[14] - a[10] * a[13];
    const b10 = a[9] * a[15] - a[11] * a[13], b11 = a[10] * a[15] - a[11] * a[14];
    let det = b00 * b11 - b01 * b10 + b02 * b09 + b03 * b08 - b04 * b07 + b05 * b06;
    if (!det) return null;
    det = 1 / det;
    o[0] = (a[5] * b11 - a[6] * b10 + a[7] * b09) * det;
    o[1] = (a[2] * b10 - a[1] * b11 - a[3] * b09) * det;
    o[2] = (a[13] * b05 - a[14] * b04 + a[15] * b03) * det;
    o[3] = (a[10] * b04 - a[9] * b05 - a[11] * b03) * det;
    o[4] = (a[6] * b08 - a[4] * b11 - a[7] * b07) * det;
    o[5] = (a[0] * b11 - a[2] * b08 + a[3] * b07) * det;
    o[6] = (a[14] * b02 - a[12] * b05 - a[15] * b01) * det;
    o[7] = (a[8] * b05 - a[10] * b02 + a[11] * b01) * det;
    o[8] = (a[4] * b10 - a[5] * b08 + a[7] * b06) * det;
    o[9] = (a[1] * b08 - a[0] * b10 - a[3] * b06) * det;
    o[10] = (a[12] * b04 - a[13] * b02 + a[15] * b00) * det;
    o[11] = (a[9] * b02 - a[8] * b04 - a[11] * b00) * det;
    o[12] = (a[5] * b07 - a[4] * b09 - a[6] * b06) * det;
    o[13] = (a[0] * b09 - a[1] * b07 + a[2] * b06) * det;
    o[14] = (a[13] * b01 - a[12] * b03 - a[14] * b00) * det;
    o[15] = (a[8] * b03 - a[9] * b01 + a[10] * b00) * det;
    return o;
  }

  function project(m, x, y, z) {
    const w = m[3] * x + m[7] * y + m[11] * z + m[15];
    return [
      (m[0] * x + m[4] * y + m[8] * z + m[12]) / w,
      (m[1] * x + m[5] * y + m[9] * z + m[13]) / w,
      (m[2] * x + m[6] * y + m[10] * z + m[14]) / w,
    ];
  }

  /* Triangle soup builder: 7 floats a vertex (xyz retail Y-down, rgba). */
  class Soup {
    constructor() { this.v = []; }
    tri(a, b, c, col) {
      for (const p of [a, b, c]) this.v.push(p[0], p[1], p[2], col[0], col[1], col[2], col[3]);
    }
    quad(a, b, c, d, col) { this.tri(a, b, c, col); this.tri(a, c, d, col); }
    /* An axis-aligned box from floor `y0` up `hgt` units (retail Y is down,
     * so its top is `y0 - hgt`). `skip` drops faces: bit 0 -x, 1 +x, 2 -z,
     * 3 +z, 4 bottom. */
    box(x0, z0, x1, z1, y0, hgt, col, skip) {
      const s = skip || 0;
      const y1 = y0 - hgt;
      const p = (x, y, z) => [x, y, z];
      this.quad(p(x0, y1, z0), p(x1, y1, z0), p(x1, y1, z1), p(x0, y1, z1), col);
      if (!(s & 16)) this.quad(p(x0, y0, z0), p(x0, y0, z1), p(x1, y0, z1), p(x1, y0, z0), col);
      if (!(s & 1)) this.quad(p(x0, y0, z0), p(x0, y1, z0), p(x0, y1, z1), p(x0, y0, z1), col);
      if (!(s & 2)) this.quad(p(x1, y0, z0), p(x1, y0, z1), p(x1, y1, z1), p(x1, y1, z0), col);
      if (!(s & 4)) this.quad(p(x0, y0, z0), p(x1, y0, z0), p(x1, y1, z0), p(x0, y1, z0), col);
      if (!(s & 8)) this.quad(p(x0, y0, z1), p(x0, y1, z1), p(x1, y1, z1), p(x1, y0, z1), col);
    }
    /* A flat quad over a tile, `lift` units above the floor. */
    slab(x0, z0, x1, z1, y, lift, col) {
      const yy = y - lift;
      this.quad([x0, yy, z0], [x1, yy, z0], [x1, yy, z1], [x0, yy, z1], col);
    }
    /* A ribbon from a to b, `w` wide, flat in X/Z. */
    ribbon(a, b, w, col) {
      const dx = b[0] - a[0], dz = b[2] - a[2];
      const l = Math.hypot(dx, dz) || 1;
      const nx = -dz / l * w / 2, nz = dx / l * w / 2;
      this.quad([a[0] + nx, a[1], a[2] + nz], [b[0] + nx, b[1], b[2] + nz],
        [b[0] - nx, b[1], b[2] - nz], [a[0] - nx, a[1], a[2] - nz], col);
    }
  }

  window.LegaiaUtil.installMethods(window.PlayView, class {

    /* Read the active pack map back from the engine (after an entry or a
     * reload) and rebuild the volumes. `null` off a pack map. */
    refreshEditorOverlay() {
      let ov = null;
      try {
        if (typeof this.rt.expansion_overlay_json === 'function') {
          ov = JSON.parse(this.rt.expansion_overlay_json());
        }
      } catch (e) { ov = null; }
      this._editorMap = ov;
      /* The engine's reading supersedes a draft of the same edits; the
       * workbench sends the draft of any later ones again. */
      this._editorDraft = null;
      this._buildEditorOverlay();
      return ov;
    }

    /* What the workbench asks for: `{ layers, select, path, marker, grid,
     * defs }`.
     * `layers` maps a layer name to on / off; `select` is
     * `{ tile: [c, r] }` or `{ rect: [c0, r0, c1, r1] }`; `path` is a list
     * of map tiles a cutscene's camera visits and `marker` the point on it
     * the timeline is scrubbed to (`[c, r]`, fractional).
     *
     * `grid` is the tile grid with its corner heights, from the workbench's
     * `map_preview` (the synthetic field image the engine enters, or the
     * terrain's heightfield): `{ width, height, corners }`, `corners` four
     * heights a tile (world units up; low-X low-Z, high-X low-Z, low-X
     * high-Z, high-X high-Z) - and `slopes`, whether a ramp cell is filled
     * (a tier map; a terrain map slopes everywhere). `defs` is the workbench's draft of the map
     * as edited since the last rebuild - the object lists of the map
     * definition, and `cells`, `[index, walls, void, floor]` for each
     * tile the edit changed (floor world units up, `null` to keep the
     * engine's) - drawn in place of the engine's
     * reading until the rebuild lands, so an edit shows at once. Either is
     * ignored when its size is not the built map's. */
    setEditorOverlay(opts) {
      const o = opts || {};
      if (o.grid !== undefined) this._editorGrid = o.grid;
      if (o.defs !== undefined) this._editorDraft = o.defs;
      this._editorOpts = {
        layers: Object.assign({}, DEFAULT_LAYERS, (this._editorOpts && this._editorOpts.layers) || {}, o.layers || {}),
        select: o.select !== undefined ? o.select : (this._editorOpts && this._editorOpts.select),
        path: o.path !== undefined ? o.path : (this._editorOpts && this._editorOpts.path),
        marker: o.marker !== undefined ? o.marker : (this._editorOpts && this._editorOpts.marker),
      };
      this._buildEditorOverlay();
    }

    /* World-space centre of map tile (c, r), retail frame: [x, floorY, z]. */
    editorTileCentre(c, r) {
      const m = this._editorMap;
      if (!m) return null;
      const ci = Math.max(0, Math.min(m.width - 1, Math.floor(c)));
      const ri = Math.max(0, Math.min(m.height - 1, Math.floor(r)));
      const u = m.unit;
      return [(m.origin[0] + c) * u + u / 2, m.floor[ri * m.width + ci] || 0, (m.origin[1] + r) * u + u / 2];
    }

    /* The map the volumes draw: the engine's reading with the workbench's
     * draft over it, when the draft is of a map that size. */
    _editorDrawnMap() {
      const m = this._editorMap;
      const d = this._editorDraft;
      if (!m || !d || d.width !== m.width || d.height !== m.height) return m;
      const out = Object.assign({}, m);
      for (const k of DRAFT_LISTS) if (Array.isArray(d[k])) out[k] = d[k];
      if (Array.isArray(d.cells) && d.cells.length) {
        out.walls = m.walls.slice();
        out.void = m.void.slice();
        out.floor = m.floor.slice();
        for (const [i, walls, vd, floor] of d.cells) {
          if (i < 0 || i >= m.width * m.height) continue;
          out.walls[i] = walls;
          out.void[i] = vd;
          if (floor != null) out.floor[i] = -floor;
        }
      }
      return out;
    }

    _buildEditorOverlay() {
      const m = this._editorDrawnMap();
      const o = this._editorOpts || { layers: DEFAULT_LAYERS };
      const L = o.layers || DEFAULT_LAYERS;
      const soup = new Soup();
      if (m) {
        const u = m.unit, W = m.width, H = m.height;
        const tx = (c) => (m.origin[0] + c) * u;
        const tz = (r) => (m.origin[1] + r) * u;
        const floorAt = (c, r) => m.floor[r * W + c] || 0;
        const isVoid = (c, r) => c < 0 || r < 0 || c >= W || r >= H || !!m.void[r * W + c];
        const full = (c, r) => !isVoid(c, r) && m.walls[r * W + c] === 15;
        const WALL_H = 96;
        /* The tile grid at its corner heights: each tile's low-X and low-Z
         * edges (and the far edges of the last column and row) as thin
         * ribbons, and a cell whose corners disagree - a ramp - filled. */
        const g = this._editorGrid;
        if (L.grid && g && g.width === W && g.height === H && g.corners && g.corners.length >= W * H) {
          const LIFT = 3, LW = 5;
          const k = (c, r) => g.corners[r * W + c];
          for (let r = 0; r < H; r++) {
            for (let c = 0; c < W; c++) {
              if (isVoid(c, r)) continue;
              const q = k(c, r);
              if (!q) continue;
              const p00 = [tx(c), -q[0] - LIFT, tz(r)], p10 = [tx(c + 1), -q[1] - LIFT, tz(r)];
              const p01 = [tx(c), -q[2] - LIFT, tz(r + 1)], p11 = [tx(c + 1), -q[3] - LIFT, tz(r + 1)];
              soup.ribbon(p00, p10, LW, COLOR.grid);
              soup.ribbon(p00, p01, LW, COLOR.grid);
              if (c === W - 1 || isVoid(c + 1, r)) soup.ribbon(p10, p11, LW, COLOR.grid);
              if (r === H - 1 || isVoid(c, r + 1)) soup.ribbon(p01, p11, LW, COLOR.grid);
              if (g.slopes && (q[0] !== q[1] || q[0] !== q[2] || q[0] !== q[3])) soup.quad(p00, p10, p11, p01, COLOR.slope);
            }
          }
        }
        if (L.walls) {
          for (let r = 0; r < H; r++) {
            for (let c = 0; c < W; c++) {
              if (isVoid(c, r)) continue;
              const mask = m.walls[r * W + c];
              if (!mask) continue;
              const y = floorAt(c, r);
              if (mask === 15) {
                /* Faces against a neighbouring full wall are inside the
                 * block: leave them out (a dense map stays light). */
                const skip = (full(c - 1, r) ? 1 : 0) | (full(c + 1, r) ? 2 : 0)
                  | (full(c, r - 1) ? 4 : 0) | (full(c, r + 1) ? 8 : 0) | 16;
                soup.box(tx(c), tz(r), tx(c + 1), tz(r + 1), y, WALL_H, COLOR.walls, skip);
                continue;
              }
              const hh = u / 2;
              if (mask & 1) soup.box(tx(c), tz(r), tx(c) + hh, tz(r) + hh, y, WALL_H, COLOR.walls, 16);
              if (mask & 2) soup.box(tx(c) + hh, tz(r), tx(c + 1), tz(r) + hh, y, WALL_H, COLOR.walls, 16);
              if (mask & 4) soup.box(tx(c), tz(r) + hh, tx(c) + hh, tz(r + 1), y, WALL_H, COLOR.walls, 16);
              if (mask & 8) soup.box(tx(c) + hh, tz(r) + hh, tx(c + 1), tz(r + 1), y, WALL_H, COLOR.walls, 16);
            }
          }
        }
        const rectSlab = (q, col, lift) => {
          const c0 = Math.max(0, Math.min(q[0], q[2])), c1 = Math.min(W - 1, Math.max(q[0], q[2]));
          const r0 = Math.max(0, Math.min(q[1], q[3])), r1 = Math.min(H - 1, Math.max(q[1], q[3]));
          for (let r = r0; r <= r1; r++) {
            for (let c = c0; c <= c1; c++) {
              if (isVoid(c, r)) continue;
              soup.slab(tx(c) + 3, tz(r) + 3, tx(c + 1) - 3, tz(r + 1) - 3, floorAt(c, r), lift, col);
            }
          }
        };
        if (L.zones) for (const z of m.encounters || []) rectSlab(z.rect, COLOR.encounters, 4);
        if (L.triggers) {
          for (const z of m.triggers || []) rectSlab(z.rect, COLOR.triggers, 8);
          for (const z of m.exits || []) rectSlab(z.rect, COLOR.exits, 10);
        }
        const pillar = (t, col, w, hgt) => {
          if (!t || isVoid(t[0], t[1])) return;
          const cx = tx(t[0]) + u / 2, cz = tz(t[1]) + u / 2;
          soup.box(cx - w / 2, cz - w / 2, cx + w / 2, cz + w / 2, floorAt(t[0], t[1]), hgt, col);
        };
        if (L.spawns) {
          for (const s of m.spawns || []) {
            pillar(s.tile, COLOR.spawns, 36, 150);
            /* The facing: an arrow on the floor, sector n = n * 45 degrees
             * (0 faces +Z). */
            const t = s.tile;
            if (!t || isVoid(t[0], t[1])) continue;
            const a = (s.facing || 0) * Math.PI / 4;
            const cx = tx(t[0]) + u / 2, cz = tz(t[1]) + u / 2, y = floorAt(t[0], t[1]) - 6;
            const fx = Math.sin(a), fz = Math.cos(a);
            soup.tri([cx + fx * 60, y, cz + fz * 60], [cx - fz * 26, y, cz + fx * 26], [cx + fz * 26, y, cz - fx * 26], COLOR.spawns);
          }
        }
        if (L.npcs) {
          for (const n of m.npcs || []) {
            pillar(n.tile, COLOR.npcs, 30, 180);
            const route = n.patrol || [];
            /* The route loops back to its first stop. */
            for (let i = 0; route.length > 1 && i < route.length; i++) {
              const a = route[i], b = route[(i + 1) % route.length];
              const pa = this.editorTileCentre(a[0], a[1]), pb = this.editorTileCentre(b[0], b[1]);
              if (pa && pb) soup.ribbon([pa[0], pa[1] - 5, pa[2]], [pb[0], pb[1] - 5, pb[2]], 12, COLOR.npcs);
            }
          }
        }
        if (L.props) {
          for (const p of m.props || []) {
            const t = p.tile;
            if (!t || isVoid(t[0], t[1])) continue;
            const [fw, fd] = p.footprint || [96, 96];
            const cx = tx(t[0]) + u / 2 + ((p.offset && p.offset[0]) || 0);
            const cz = tz(t[1]) + u / 2 + ((p.offset && p.offset[2]) || 0);
            soup.box(cx - fw / 2, cz - fd / 2, cx + fw / 2, cz + fd / 2, floorAt(t[0], t[1]), p.top != null ? p.top : 24, COLOR.props);
          }
        }
        /* The painter's selection. */
        const sel = o.select;
        if (sel && sel.rect) {
          const q = sel.rect;
          const c0 = Math.min(q[0], q[2]), c1 = Math.max(q[0], q[2]);
          const r0 = Math.min(q[1], q[3]), r1 = Math.max(q[1], q[3]);
          let y = 0;
          if (!isVoid(c0, r0)) y = floorAt(c0, r0);
          soup.box(tx(c0) - 4, tz(r0) - 4, tx(c1 + 1) + 4, tz(r1 + 1) + 4, y + 2, 140, COLOR.select);
        } else if (sel && sel.tile && sel.tile[0] >= 0 && sel.tile[1] >= 0 && sel.tile[0] < W && sel.tile[1] < H) {
          const [c, r] = sel.tile;
          soup.box(tx(c) - 4, tz(r) - 4, tx(c + 1) + 4, tz(r + 1) + 4, floorAt(c, r) + 2, 140, COLOR.select);
        }
        /* A cutscene's camera path, and where the timeline is scrubbed to. */
        const path = o.path || [];
        const LIFT = 70;
        for (let i = 0; i + 1 < path.length; i++) {
          const a = this.editorTileCentre(path[i][0], path[i][1]);
          const b = this.editorTileCentre(path[i + 1][0], path[i + 1][1]);
          if (a && b) soup.ribbon([a[0], a[1] - LIFT, a[2]], [b[0], b[1] - LIFT, b[2]], 18, COLOR.path);
        }
        for (const t of path) {
          const a = this.editorTileCentre(t[0], t[1]);
          if (a) soup.box(a[0] - 14, a[2] - 14, a[0] + 14, a[2] + 14, a[1] - LIFT + 14, 28, COLOR.path);
        }
        if (o.marker) {
          const a = this.editorTileCentre(o.marker[0], o.marker[1]);
          if (a) soup.box(a[0] - 26, a[2] - 26, a[0] + 26, a[2] + 26, a[1], 170, COLOR.marker);
        }
      }
      this._editorVerts = soup.v.length ? new Float32Array(soup.v) : null;
      this._editorVertCount = soup.v.length / 7;
      this._editorDirty = true;
    }

    /* Draw the volumes over the finished 3D frame (play-view-frame.js
     * calls this after the fog pass, field frames only). */
    _drawEditorOverlay() {
      if (!this.editorOverlayOn || !this._editorVerts || !this.renderer) return;
      const gl = this.renderer.gl;
      if (!this._editorProg) {
        try { this._editorProg = compileProgram(gl, EDITOR_VS, EDITOR_FS); }
        catch (e) { this.editorOverlayOn = false; console.warn('editor overlay disabled:', e); return; }
        const p = this._editorProg;
        this._editorLoc = {
          mvp: gl.getUniformLocation(p, 'u_mvp'),
          log: gl.getUniformLocation(p, 'u_log_depth_on'),
          alpha: gl.getUniformLocation(p, 'u_alpha'),
          pos: gl.getAttribLocation(p, 'a_position'),
          col: gl.getAttribLocation(p, 'a_color'),
        };
        this._editorVao = gl.createVertexArray();
        this._editorBuf = gl.createBuffer();
        gl.bindVertexArray(this._editorVao);
        gl.bindBuffer(gl.ARRAY_BUFFER, this._editorBuf);
        const L = this._editorLoc;
        gl.enableVertexAttribArray(L.pos);
        gl.vertexAttribPointer(L.pos, 3, gl.FLOAT, false, 28, 0);
        gl.enableVertexAttribArray(L.col);
        gl.vertexAttribPointer(L.col, 4, gl.FLOAT, false, 28, 12);
        gl.bindVertexArray(null);
        this._editorDirty = true;
      }
      const L = this._editorLoc;
      const c = this.renderer.canvas;
      const vp = buildWorldOrbitVp(c.width, c.height, this._ext, this.cam);
      const flipY = new Float32Array([1, 0, 0, 0, 0, -1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1]);
      gl.useProgram(this._editorProg);
      gl.bindVertexArray(this._editorVao);
      if (this._editorDirty) {
        gl.bindBuffer(gl.ARRAY_BUFFER, this._editorBuf);
        gl.bufferData(gl.ARRAY_BUFFER, this._editorVerts, gl.STATIC_DRAW);
        this._editorDirty = false;
      }
      gl.uniformMatrix4fv(L.mvp, false, mulMat4(vp, flipY));
      gl.uniform1i(L.log, this.renderer.lastLogDepth ? 1 : 0);
      gl.enable(gl.BLEND);
      gl.blendEquation(gl.FUNC_ADD);
      gl.blendFunc(gl.SRC_ALPHA, gl.ONE_MINUS_SRC_ALPHA);
      gl.disable(gl.CULL_FACE);
      gl.depthMask(false);
      /* Faint and through everything, then solid where nothing is in front. */
      gl.disable(gl.DEPTH_TEST);
      gl.uniform1f(L.alpha, 0.28);
      gl.drawArrays(gl.TRIANGLES, 0, this._editorVertCount);
      gl.enable(gl.DEPTH_TEST);
      gl.depthFunc(gl.LEQUAL);
      gl.uniform1f(L.alpha, 1.0);
      gl.drawArrays(gl.TRIANGLES, 0, this._editorVertCount);
      gl.depthMask(true);
      gl.disable(gl.BLEND);
      gl.bindVertexArray(null);
      gl.useProgram(this.renderer.program);
    }

    /* The map tile under client point (x, y), `null` off the map: the ray
     * through the pixel, through the inverse of the frame's own
     * view-projection, marched until it passes under a tile's floor. */
    pickTile(clientX, clientY) {
      const m = this._editorMap;
      if (!m || !this.renderer) return null;
      const cv = this.renderer.canvas;
      const rect = cv.getBoundingClientRect();
      let nx = ((clientX - rect.left) / rect.width) * 2 - 1;
      /* The preview's Match painter mirrors the canvas (CSS). */
      if (this.mirrorView) nx = -nx;
      const ny = 1 - ((clientY - rect.top) / rect.height) * 2;
      const vp = buildWorldOrbitVp(cv.width, cv.height, this._ext, this.cam);
      const flipY = new Float32Array([1, 0, 0, 0, 0, -1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1]);
      const inv = invert4(mulMat4(vp, flipY));
      if (!inv) return null;
      const a = project(inv, nx, ny, -1), b = project(inv, nx, ny, 1);
      const u = m.unit;
      const dir = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
      const len = Math.hypot(dir[0], dir[1], dir[2]) || 1;
      const step = 16;
      const n = Math.min(20000, Math.ceil(len / step));
      for (let i = 0; i <= n; i++) {
        const t = i / n;
        const x = a[0] + dir[0] * t, y = a[1] + dir[1] * t, z = a[2] + dir[2] * t;
        const c = Math.floor(x / u) - m.origin[0], r = Math.floor(z / u) - m.origin[1];
        if (c < 0 || r < 0 || c >= m.width || r >= m.height) continue;
        const k = r * m.width + c;
        if (m.void[k]) continue;
        /* A wall block stops the ray at its top. */
        const top = m.floor[k] - (m.walls[k] === 15 ? 96 : 0);
        if (y >= top) return [c, r];
      }
      return null;
    }

    /* Aim the orbit camera at map tile (c, r) (fractional), or at the whole
     * map with no argument. */
    focusEditorTile(c, r) {
      const m = this._editorMap;
      if (!m) return;
      if (c == null) {
        c = m.width / 2; r = m.height / 2;
        const span = Math.max(m.width, m.height) * m.unit;
        this.cam.halfWidth = this.cam.halfHeight = Math.max(600, Math.min(16000, span * 0.7));
        /* Seen from the map's low-Z side, as the field camera looks: the
         * painter's columns keep their left and right, and its row 0 is
         * the near edge - the painter's grid mirrored top to bottom, since
         * the view mirrors screen X. Under the preview's Match painter
         * (`matchPainter`, with the canvas mirrored back) from the high-Z
         * side instead: row 0 the far edge, at the top, as painted. */
        this.cam.yaw = this.matchPainter ? Math.PI : 0;
        this.cam.pitch = 0.62;
      }
      const p = this.editorTileCentre(c, r);
      if (p) this.editorFocus = [p[0], -p[1], p[2]];
    }
  });
})();
