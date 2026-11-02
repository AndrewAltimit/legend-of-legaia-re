/* play-view-editor-gizmos.js - PlayView's expansion-editor editing layer.
 *
 * Part of the PlayView class (play-app.js), beside the editor overlay
 * (play-view-editor-overlay.js). The workbench's 3D preview frames this page
 * (`play.html?xwpreview=1`); with an edit mode on (`xw-edit` from the
 * workbench, expansion-workbench-gizmos.js on its side) the picture takes
 * the editor's pointer:
 *
 *   - **select**: a click picks the object under it (Shift or Ctrl adds),
 *     and the selection gets a gizmo - arrows and a plane square to move,
 *     a ring to turn, a corner handle to scale (a zone's corner resizes
 *     it). A drag snaps to the tile or the 32-unit sub-tile grid; Shift
 *     places freely. Objects stand on the ground the frame samples.
 *   - **sculpt**: a ring cursor of the brush's radius (and an inner ring at
 *     its strength) rides the ground; a drag lays the brush's stroke.
 *   - **paint**: the ring paints the tiles under it with a material.
 *
 * Nothing here edits the pack. A finished drag posts the result to the
 * workbench (`xw-transform`, `xw-sculpt`, `xw-paint`, `xw-select`), which
 * writes the pack, keeps its undo and rebuilds the map. Until the rebuild
 * lands the frame draws a cheap local proxy: the objects at their new poses,
 * the sculpted ground as a tinted patch at its new heights (the stroke run
 * over the frame's copy of the heightfield), the painted tiles tinted.
 *
 * Heights: the workbench sends the heightfield `map_preview` reads back
 * (the engine's own build, `[terrain]` maps) and the frame samples it the
 * way `Heightfield::sample_world` does (a quad split along b-c); a tile map
 * samples the overlay's per-tile floors. Retail world, Y down.
 *
 * Classic script. Requires webgl-math.js, webgl-shaders.js, play-app.js and
 * play-view-editor-overlay.js first. */
(function () {
  'use strict';

  const VS = `#version 300 es
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
  const FS = `#version 300 es
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

  const COL = {
    x: [0.95, 0.32, 0.30, 0.95], z: [0.30, 0.55, 0.98, 0.95], plane: [0.98, 0.86, 0.30, 0.75],
    ring: [0.35, 0.92, 0.45, 0.9], scale: [0.98, 0.62, 0.20, 0.95], hot: [1, 1, 1, 1],
    sel: [1, 1, 1, 0.42], ghost: [1, 1, 1, 0.16], hover: [0.85, 0.95, 1, 0.3],
    lamp: [1.0, 0.85, 0.35, 0.75], emitter: [0.45, 0.9, 0.95, 0.35], decor: [0.45, 0.85, 0.45, 0.55],
    brush: [1, 1, 1, 0.9], inner: [1, 0.85, 0.4, 0.8], up: [0.55, 0.85, 0.45, 0.55], down: [0.85, 0.45, 0.35, 0.55],
    paint: [0.45, 0.95, 1.0, 0.5],
    ok: [0.35, 0.95, 0.45, 0.95], okFill: [0.35, 0.95, 0.45, 0.35], bad: [1.0, 0.3, 0.25, 0.95], badFill: [1.0, 0.3, 0.25, 0.4],
  };
  const FLIP = new Float32Array([1, 0, 0, 0, 0, -1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1]);
  /* The Select tool's keys in the frame (the painter's letters are its own). */
  const TOOL_KEYS = { w: 'move', e: 'rotate', r: 'scale' };
  /* An arrow key's direction on screen (client pixels, Y down). */
  const ARROW_DIR = { ArrowLeft: [-1, 0], ArrowRight: [1, 0], ArrowUp: [0, -1], ArrowDown: [0, 1] };

  function inv4(m) {
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
    o[0] = (a[5] * b11 - a[6] * b10 + a[7] * b09) * det; o[1] = (a[2] * b10 - a[1] * b11 - a[3] * b09) * det;
    o[2] = (a[13] * b05 - a[14] * b04 + a[15] * b03) * det; o[3] = (a[10] * b04 - a[9] * b05 - a[11] * b03) * det;
    o[4] = (a[6] * b08 - a[4] * b11 - a[7] * b07) * det; o[5] = (a[0] * b11 - a[2] * b08 + a[3] * b07) * det;
    o[6] = (a[14] * b02 - a[12] * b05 - a[15] * b01) * det; o[7] = (a[8] * b05 - a[10] * b02 + a[11] * b01) * det;
    o[8] = (a[4] * b10 - a[5] * b08 + a[7] * b06) * det; o[9] = (a[1] * b08 - a[0] * b10 - a[3] * b06) * det;
    o[10] = (a[12] * b04 - a[13] * b02 + a[15] * b00) * det; o[11] = (a[9] * b02 - a[8] * b04 - a[11] * b00) * det;
    o[12] = (a[5] * b07 - a[4] * b09 - a[6] * b06) * det; o[13] = (a[0] * b09 - a[1] * b07 + a[2] * b06) * det;
    o[14] = (a[13] * b01 - a[12] * b03 - a[14] * b00) * det; o[15] = (a[8] * b03 - a[9] * b01 + a[10] * b00) * det;
    return o;
  }
  function xf(m, x, y, z) {
    const w = m[3] * x + m[7] * y + m[11] * z + m[15];
    return [(m[0] * x + m[4] * y + m[8] * z + m[12]) / w, (m[1] * x + m[5] * y + m[9] * z + m[13]) / w,
      (m[2] * x + m[6] * y + m[10] * z + m[14]) / w, w];
  }

  /* --- The stroke kernels, the format's (legaia_expansion_format::terrain)
   * re-run over the frame's copy for the proxy only. --- */
  const cosFall = (d, r) => (r <= 0 || d >= r ? 0 : 0.5 * (1 + Math.cos(Math.PI * d / r)));
  const smooth = (e0, e1, x) => { const t = Math.max(0, Math.min(1, (x - e0) / (e1 - e0))); return t * t * (3 - 2 * t); };
  function segDist(p, a, b) {
    const dx = b[0] - a[0], dz = b[1] - a[1];
    const l2 = dx * dx + dz * dz;
    const t = l2 > 0 ? Math.max(0, Math.min(1, ((p[0] - a[0]) * dx + (p[1] - a[1]) * dz) / l2)) : 0;
    return [Math.hypot(p[0] - a[0] - dx * t, p[1] - a[1] - dz * t), t];
  }
  function pathDist(p, path) {
    if (path.length === 1) return Math.hypot(p[0] - path[0][0], p[1] - path[0][1]);
    let d = Infinity;
    for (let k = 0; k + 1 < path.length; k++) d = Math.min(d, segDist(p, path[k], path[k + 1])[0]);
    return d;
  }

  /* Run `shape` over heights `h` (Float32Array, cols x rows) inside the
   * vertex box [i0, j0, i1, j1]; `uv(i, j)` is the vertex's map-local tile
   * coordinate. Noise previews as no change (its rectangle draws instead). */
  function runShape(h, cols, box, uv, s) {
    const [i0, j0, i1, j1] = box;
    const each = (f) => { for (let j = j0; j <= j1; j++) for (let i = i0; i <= i1; i++) { const k = j * cols + i; h[k] = f(uv(i, j), h[k], i, j); } };
    const blurred = () => {
      const out = new Float32Array(h);
      for (let pass = 0; pass < 3; pass++) {
        const src = new Float32Array(out);
        for (let j = j0; j <= j1; j++) for (let i = i0; i <= i1; i++) {
          let sum = 0;
          for (let dj = -1; dj <= 1; dj++) for (let di = -1; di <= 1; di++) {
            const ii = Math.max(i0, Math.min(i1, i + di)), jj = Math.max(j0, Math.min(j1, j + dj));
            sum += src[jj * cols + ii];
          }
          out[j * cols + i] = sum / 9;
        }
      }
      return out;
    };
    const st = s.strength == null ? 1 : s.strength;
    switch (s.kind) {
      case 'raise': each((p, v) => v + s.height * cosFall(Math.hypot(p[0] - s.at[0], p[1] - s.at[1]), s.radius)); break;
      case 'ridge': each((p, v) => v + s.height * cosFall(segDist(p, s.from, s.to)[0], s.radius)); break;
      case 'ramp': each((p, v) => {
        const [d, t] = segDist(p, s.from, s.to);
        const m = 1 - smooth(s.radius * 0.6, Math.max(0.01, s.radius), d);
        return v + (s.height[0] + (s.height[1] - s.height[0]) * t - v) * m;
      }); break;
      case 'plateau': {
        const [c0, r0, c1, r1] = s.rect;
        const x0 = Math.min(c0, c1), z0 = Math.min(r0, r1), x1 = Math.max(c0, c1) + 1, z1 = Math.max(r0, r1) + 1;
        each((p, v) => {
          const dx = Math.max(x0 - p[0], p[0] - x1, 0), dz = Math.max(z0 - p[1], p[1] - z1, 0);
          const m = 1 - smooth(0, Math.max(0.01, s.edge == null ? 0.5 : s.edge), Math.hypot(dx, dz));
          return v + (s.height - v) * m;
        });
        break;
      }
      case 'flatten': each((p, v) => v + ((s.height == null ? v : s.height) - v)
        * Math.max(0, Math.min(1, cosFall(Math.hypot(p[0] - s.at[0], p[1] - s.at[1]), s.radius) * st))); break;
      case 'smooth': {
        const b = blurred();
        each((p, v, i, j) => v + (b[j * cols + i] - v) * Math.max(0, Math.min(1, cosFall(Math.hypot(p[0] - s.at[0], p[1] - s.at[1]), s.radius) * st)));
        break;
      }
      case 'stroke': {
        const op = s.op || 'raise';
        if (op === 'raise') each((p, v) => v + (s.height || 0) * st * cosFall(pathDist(p, s.path), s.radius));
        else if (op === 'flatten') each((p, v) => v + ((s.height == null ? v : s.height) - v) * Math.max(0, Math.min(1, cosFall(pathDist(p, s.path), s.radius) * st)));
        else {
          const b = blurred();
          each((p, v, i, j) => v + (b[j * cols + i] - v) * Math.max(0, Math.min(1, cosFall(pathDist(p, s.path), s.radius) * st)));
        }
        break;
      }
      default: break;
    }
  }

  /* Triangle soup, 7 floats a vertex (xyz retail Y-down, rgba). */
  class Soup {
    constructor() { this.v = []; }
    tri(a, b, c, col) { for (const p of [a, b, c]) this.v.push(p[0], p[1], p[2], col[0], col[1], col[2], col[3]); }
    quad(a, b, c, d, col) { this.tri(a, b, c, col); this.tri(a, c, d, col); }
    box(x0, z0, x1, z1, y0, hgt, col) {
      const y1 = y0 - hgt;
      const p = (x, y, z) => [x, y, z];
      this.quad(p(x0, y1, z0), p(x1, y1, z0), p(x1, y1, z1), p(x0, y1, z1), col);
      this.quad(p(x0, y0, z0), p(x0, y1, z0), p(x0, y1, z1), p(x0, y0, z1), col);
      this.quad(p(x1, y0, z0), p(x1, y0, z1), p(x1, y1, z1), p(x1, y1, z0), col);
      this.quad(p(x0, y0, z0), p(x1, y0, z0), p(x1, y1, z0), p(x0, y1, z0), col);
      this.quad(p(x0, y0, z1), p(x0, y1, z1), p(x1, y1, z1), p(x1, y0, z1), col);
    }
    /* A box turned `yaw` radians about its centre (the painter's sense:
     * +X turns toward +Z). */
    obox(cx, cz, hw, hd, y0, hgt, yaw, col) {
      const c = Math.cos(yaw), s = Math.sin(yaw);
      const at = (lx, lz, y) => [cx + lx * c - lz * s, y, cz + lx * s + lz * c];
      const y1 = y0 - hgt;
      const k = [[-hw, -hd], [hw, -hd], [hw, hd], [-hw, hd]];
      const top = k.map(([a, b]) => at(a, b, y1)), bot = k.map(([a, b]) => at(a, b, y0));
      this.quad(top[0], top[1], top[2], top[3], col);
      for (let i = 0; i < 4; i++) { const n = (i + 1) % 4; this.quad(bot[i], bot[n], top[n], top[i], col); }
    }
    ribbon(a, b, w, col) {
      const dx = b[0] - a[0], dz = b[2] - a[2];
      const l = Math.hypot(dx, dz) || 1;
      const nx = -dz / l * w / 2, nz = dx / l * w / 2;
      this.quad([a[0] + nx, a[1], a[2] + nz], [b[0] + nx, b[1], b[2] + nz], [b[0] - nx, b[1], b[2] - nz], [a[0] - nx, a[1], a[2] - nz], col);
    }
  }

  window.LegaiaUtil.installMethods(window.PlayView, class {

    /* The workbench's edit state: `{ mode, tool, snap, objects, selected,
     * brush, terrain }` (expansion-workbench-gizmos.js). Fields left out
     * keep their value. */
    setEditorEdit(msg) {
      const E = this._ed || (this._ed = { mode: 'view', tool: 'move', snap: 'tile', objects: [], selected: [], brush: null, terrain: null });
      for (const k of ['mode', 'tool', 'snap', 'objects', 'selected', 'brush', 'place']) if (msg[k] !== undefined) E[k] = msg[k];
      if (E.mode !== 'place') E.ghost = null;
      if (msg.terrain !== undefined) {
        const t = msg.terrain;
        E.terrain = t && t.heights ? Object.assign({}, t, { h: Float32Array.from(t.heights) }) : null;
        // A heightfield of another size (resolution, skirt) ends the proxy.
        if (E.proxy && (!E.terrain || E.proxy.h.length !== E.terrain.h.length)) E.proxy = null;
      }
      // Fresh objects from the workbench supersede the drag's proxy poses.
      if (msg.objects !== undefined) E.poses = null;
      this.syncEditorHud();
      this._edDirty = true;
    }

    /* The party HUD stays off the frame while an edit mode is on, and
     * under the orbit camera (the editor's view of the map, not the
     * player's); the field camera shows it as the game does. */
    syncEditorHud() {
      const E = this._ed;
      if (this.rt && typeof this.rt.set_editor_hud_hidden === 'function') {
        try { this.rt.set_editor_hud_hidden(!!this.debugCamera || !!(E && E.mode !== 'view')); } catch (_) { /* no runtime yet */ }
      }
    }

    /* The map was rebuilt: what the proxy stood in for now draws itself. */
    editorRebuilt() {
      if (this._ed) { this._ed.proxy = null; this._ed.paintProxy = null; this._edDirty = true; }
    }

    /* `post(msg)` reaches the workbench. Called once by
     * play-expansion-live.js with the canvas ready.
     *
     * While the preview is an editor (`editorNoPlayer`: no Test play
     * running) the picture is the editor's alone: the lead is not drawn,
     * no key reaches the pad or the page's hotkeys, and the mouse drives
     * the editor camera - right drag orbits, middle drag (or Space with the
     * left button) pans, the wheel zooms - and the edit mode's left button.
     * Test play hands the canvas back to the game's own input. */
    attachEditorEditing(post) {
      if (this._edAttached) return;
      this._edAttached = true;
      this._edPost = post;
      const cv = this.canvas;
      const consume = (e) => { e.stopImmediatePropagation(); e.preventDefault(); };
      this._edSpace = false;
      cv.addEventListener('contextmenu', (e) => { if (this.editorNoPlayer) e.preventDefault(); });
      cv.addEventListener('pointerdown', (e) => {
        if (!this.editorNoPlayer) return;
        consume(e);
        try { cv.focus({ preventScroll: true }); } catch (_) { cv.focus(); }
        try { cv.setPointerCapture(e.pointerId); } catch (_) { /* no capture */ }
        const E = this._ed;
        if (e.button === 2 || e.button === 1 || (e.button === 0 && (this._edSpace || e.altKey))) {
          this._edCam = { kind: e.button === 2 || (e.button === 0 && e.altKey) ? 'orbit' : 'pan', x: e.clientX, y: e.clientY, x0: e.clientX, y0: e.clientY, b: e.button };
          return;
        }
        if (e.button !== 0) return;
        if (E && E.mode !== 'view' && this._editorMap && this._edDown(e)) return;
        // A left click on nothing: picks the tile (View) or drops the
        // selection (the edit modes) on release.
        this._edCam = { kind: 'click', x: e.clientX, y: e.clientY, x0: e.clientX, y0: e.clientY, b: 0 };
      }, { capture: true });
      cv.addEventListener('pointermove', (e) => {
        if (!this.editorNoPlayer) return;
        consume(e);
        if (this._edCam) { this._edCamMove(e); return; }
        const E = this._ed;
        if (!E || !this._editorMap) return;
        if (this._edDrag) { this._edMove(e); return; }
        if (E.mode !== 'view') this._edHover(e);
        // Placing: the keys (R turns the piece, Escape stops) belong to the
        // picture the pointer is over, unless the page is taking text.
        if (E.mode === 'place' && document.activeElement !== cv) {
          let typing = false;
          try { const a = window.parent && window.parent.document.activeElement; typing = !!(a && /^(INPUT|TEXTAREA|SELECT)$/.test(a.tagName)); } catch (_) { typing = false; }
          if (!typing) { try { cv.focus({ preventScroll: true }); } catch (_) { /* no focus */ } }
        }
      }, { capture: true });
      const up = (e) => {
        if (!this.editorNoPlayer) return;
        consume(e);
        try { cv.releasePointerCapture(e.pointerId); } catch (_) { /* no capture */ }
        const c = this._edCam;
        if (c) {
          this._edCam = null;
          const still = Math.hypot(e.clientX - c.x0, e.clientY - c.y0) <= 4;
          if (still) this._edClick(e, c);
          return;
        }
        if (this._edDrag) this._edUp(e);
      };
      cv.addEventListener('pointerup', up, { capture: true });
      cv.addEventListener('pointercancel', up, { capture: true });
      cv.addEventListener('pointerleave', () => { if (this._ed) { this._ed.cursor = null; this._ed.ghost = null; this._edDirty = true; } });
      cv.addEventListener('wheel', (e) => {
        if (!this.editorNoPlayer) return;
        const E = this._ed;
        // Shift with the wheel turns the piece being placed (or the
        // selection) a quarter turn a notch; the plain wheel zooms.
        if (E && e.shiftKey && (E.mode === 'place' || (E.mode === 'select' && E.selected.length))) {
          consume(e);
          this._edRotate(e.deltaY > 0 ? 90 : -90);
          return;
        }
        consume(e);
        const f = e.deltaY > 0 ? 1.12 : 0.89;
        this.cam.halfWidth = Math.max(220, Math.min(this.maxOrbitHalf || 16000, this.cam.halfWidth * f));
        this.cam.halfHeight = this.cam.halfWidth;
      }, { capture: true, passive: false });
      const key = (e, down) => this._edKey(e, down);
      window.addEventListener('keydown', (e) => key(e, true), { capture: true });
      window.addEventListener('keyup', (e) => key(e, false), { capture: true });
      window.addEventListener('blur', () => { this._edSpace = false; });
    }

    /* The editor's keys (window capture: nothing below sees a key the
     * editor takes, so the pad never walks the lead). Function keys other
     * than F3 (the debug camera, which the editor owns) pass to the page. */
    _edKey(e, down) {
      if (!this.editorNoPlayer) return;
      const k = e.key;
      if (/^F\d+$/.test(k) && k !== 'F3') return;
      const t = e.target;
      if (t && (t.tagName === 'INPUT' || t.tagName === 'TEXTAREA' || t.tagName === 'SELECT')) return;
      e.stopImmediatePropagation();
      if (k === ' ' || e.code === 'Space') { this._edSpace = down; e.preventDefault(); return; }
      if (!down) return;
      e.preventDefault();
      const E = this._ed || { mode: 'view', selected: [] };
      const post = this._edPost;
      const ctrl = e.ctrlKey || e.metaKey;
      const lk = k.toLowerCase();
      if (ctrl && (lk === 'z' || lk === 'y' || lk === 'd')) { post({ type: 'xw-key', key: lk, ctrl: true, shift: e.shiftKey }); return; }
      if (ctrl) return;
      if (k === 'Delete' || k === 'Backspace') { post({ type: 'xw-key', key: 'Delete' }); return; }
      if (k === '[' || k === ']') { post({ type: 'xw-key', key: k }); return; }
      if (k === 'Escape') {
        if (E.mode === 'place') post({ type: 'xw-key', key: 'cancel-place' });
        else post({ type: 'xw-key', key: 'deselect' });
        return;
      }
      if (/^[1-6]$/.test(k)) { post({ type: 'xw-key', key: 'digit', n: +k }); return; }
      if (lk === 'r') { this._edRotate(e.shiftKey ? 15 : 90); return; }
      if (lk === 't') { this.editorTopDown(!this._edTop); return; }
      if (lk === 'f') { this._edFrame(); return; }
      if (lk === 'q' || lk === 'e') {
        this.cam.yaw += (lk === 'q' ? -1 : 1) * Math.PI / 4 * (this.mirrorView ? -1 : 1);
        return;
      }
      if (lk === 'g') { post({ type: 'xw-key', key: 'tool', tool: 'move' }); return; }
      // Arrows: nudge a selection along the map axis closest on screen;
      // with nothing selected they pan, as W A S D always do.
      const arrow = ARROW_DIR[k];
      if (arrow && E.mode === 'select' && E.selected.length && this.debugCamera) {
        const n = this._edArrowAxis(k);
        if (n) post({ type: 'xw-key', key: 'nudge', dc: n[0], dr: n[1], fine: e.shiftKey });
        return;
      }
      const pan = arrow || { w: [0, -1], a: [-1, 0], s: [0, 1], d: [1, 0] }[lk];
      // A key moves the view the way it points (a drag moves the ground
      // with the pointer, so the key's step is the drag's reversed).
      if (pan) this._edPanBy(-pan[0] * 60, -pan[1] * 60);
    }

    /* The editor camera's drag: orbit (yaw and tilt) or pan. */
    _edCamMove(e) {
      const c = this._edCam;
      const dx = e.clientX - c.x, dy = e.clientY - c.y;
      c.x = e.clientX; c.y = e.clientY;
      if (c.kind === 'orbit') {
        this.cam.yaw += dx * 0.006 * (this.mirrorView ? -1 : 1);
        this.cam.pitch = Math.max(0.02, Math.min(1.35, this.cam.pitch + dy * 0.004));
        this._edTop = false;
      } else if (c.kind === 'pan') {
        this._edPanBy(-dx, -dy);
      }
    }

    /* Pan the orbit focus by a screen offset (client pixels): the ground
     * moves with the pointer. */
    _edPanBy(dxPx, dyPx) {
      if (!this.editorFocus) return;
      const cv = this.canvas;
      const dx = dxPx * (this.mirrorView ? -1 : 1);
      const k = (this.cam.halfWidth * 2) / Math.max(cv.clientWidth, 1);
      const sy = Math.sin(this.cam.yaw), cy = Math.cos(this.cam.yaw);
      /* Screen X is mirrored on this camera (buildWorldOrbitVp), and
       * mirrored back by the canvas under Match painter. */
      this.editorFocus[0] -= (dx * cy + dyPx * sy) * k;
      this.editorFocus[2] -= (dx * sy - dyPx * cy) * k;
    }

    /* Straight down on the map (and back to the tilt it had). */
    editorTopDown(on) {
      if (on && !this._edTop) {
        this._edTopPrev = this.cam.pitch;
        this.cam.pitch = 0.02;
        this.cam.yaw = Math.round(this.cam.yaw / (Math.PI / 2)) * (Math.PI / 2);
      } else if (!on && this._edTop) {
        this.cam.pitch = this._edTopPrev || 0.62;
      }
      this._edTop = !!on;
      if (this._edPost) this._edPost({ type: 'xw-key', key: 'topdown', on: this._edTop });
    }

    /* The selection framed, else the whole map. */
    _edFrame() {
      const P = this._ed && this._ed.selected.length ? this._edPivot() : null;
      if (P) {
        const t = this._edTile(P.x, P.z);
        this.focusEditorTile(t[0], t[1]);
        this.cam.halfWidth = this.cam.halfHeight = Math.max(500, Math.min(this.cam.halfWidth, 1400));
      } else {
        const keep = [this.cam.yaw, this.cam.pitch];
        this.focusEditorTile();
        [this.cam.yaw, this.cam.pitch] = keep;
      }
    }

    /* A click with no travel: the left button picks a tile (View), drops
     * the selection (the edit modes); the right one puts the piece being
     * placed down. */
    _edClick(e, c) {
      const E = this._ed;
      if (c.b === 2) {
        if (E && E.mode === 'place') this._edPost({ type: 'xw-key', key: 'cancel-place' });
        return;
      }
      if (c.kind !== 'click') return;
      if (!E || E.mode === 'view') {
        const t = this.pickTile(e.clientX, e.clientY);
        if (t) {
          this.setEditorOverlay({ select: { tile: t } });
          this._edPost({ type: 'xw-pick', tile: t });
        }
      } else if (E.mode === 'select' && E.selected.length && !(e.shiftKey || e.ctrlKey || e.metaKey)) {
        E.selected = [];
        this._edPost({ type: 'xw-select', keys: [] });
        this._edDirty = true;
      }
    }

    /* Turn the piece being placed, or the selection, by `deg` (the
     * painter's sense). NPCs and spawns turn by facing sector. */
    _edRotate(deg) {
      const E = this._ed;
      if (!E) return;
      if (E.mode === 'place' && E.place) {
        const step = E.place.facing ? 45 : 1;
        E.place.yaw = (((Math.round(((E.place.yaw || 0) + deg) / step) * step) % 360) + 360) % 360;
        this._edPost({ type: 'xw-key', key: 'place-yaw', yaw: E.place.yaw });
        this._edDirty = true;
        return;
      }
      if (E.mode !== 'select' || !E.selected.length) return;
      const P = this._edPivot();
      if (!P) return;
      const edits = [];
      for (const o of P.sel) {
        if (o.rect || o.noRotate) continue;
        const p = this._edPose(o);
        const d = o.facing ? Math.sign(deg) * Math.max(45, Math.abs(deg) >= 90 ? 90 : 45) : deg;
        edits.push({ key: o.key, x: p.x, z: p.z, yaw: (((p.yaw || 0) + d) % 360 + 360) % 360 });
      }
      if (!edits.length) return;
      E.poses = Object.assign({}, E.poses || {});
      for (const ed of edits) E.poses[ed.key] = Object.assign({}, this._edPose(P.sel.find((o) => o.key === ed.key)), { yaw: ed.yaw });
      this._edDirty = true;
      this._edPost({ type: 'xw-transform', edits });
    }

    /* --- Geometry --- */

    _edVp() {
      const c = this.renderer.canvas;
      return mulMat4(buildWorldOrbitVp(c.width, c.height, this._ext, this.cam), FLIP);
    }

    /* The map step `[dc, dr]` (one of the four axis directions) whose
     * direction on screen is closest to arrow key `key`, from the map's
     * middle; null for another key or with no map. */
    _edArrowAxis(key) {
      const want = ARROW_DIR[key];
      const m = this._editorMap;
      if (!want || !m) return null;
      const vp = this._edVp();
      const c = this.editorTileCentre(m.width / 2, m.height / 2);
      const o = c && this._edScreen(c, vp);
      if (!o) return null;
      let best = null, bestDot = -Infinity;
      for (const [dc, dr] of [[1, 0], [-1, 0], [0, 1], [0, -1]]) {
        const s = this._edScreen([c[0] + dc * m.unit, c[1], c[2] + dr * m.unit], vp);
        if (!s) continue;
        const vx = s[0] - o[0], vy = s[1] - o[1], len = Math.hypot(vx, vy) || 1;
        const dot = (vx * want[0] + vy * want[1]) / len;
        if (dot > bestDot) { bestDot = dot; best = [dc, dr]; }
      }
      return best;
    }

    /* The ray under client point (x, y), retail world. */
    _edRay(cx, cy) {
      const cv = this.renderer.canvas;
      const r = cv.getBoundingClientRect();
      /* The preview's Match painter mirrors the canvas (`mirrorView`). */
      const m = this.mirrorView ? -1 : 1;
      const nx = (((cx - r.left) / r.width) * 2 - 1) * m, ny = 1 - ((cy - r.top) / r.height) * 2;
      const inv = inv4(this._edVp());
      if (!inv) return null;
      const a = xf(inv, nx, ny, -1), b = xf(inv, nx, ny, 1);
      return { o: [a[0], a[1], a[2]], d: [b[0] - a[0], b[1] - a[1], b[2] - a[2]] };
    }

    /* Client point of a world point, null behind the eye. */
    _edScreen(p, vp) {
      const q = xf(vp || this._edVp(), p[0], p[1], p[2]);
      if (q[3] <= 0) return null;
      const r = this.renderer.canvas.getBoundingClientRect();
      const x = this.mirrorView ? -q[0] : q[0];
      return [r.left + (x + 1) / 2 * r.width, r.top + (1 - q[1]) / 2 * r.height];
    }

    /* The ground's retail Y (down) at world (x, z): the heightfield the
     * workbench sent (with the stroke's proxy over it), else the tile's
     * floor. */
    _edGround(x, z) {
      const E = this._ed;
      const t = E && E.terrain;
      if (t) {
        const h = (E.proxy && E.proxy.h) || t.h;
        const lx = x - t.origin[0], lz = z - t.origin[1];
        const fi = lx / t.step, fj = lz / t.step;
        const i = Math.max(0, Math.min(t.cols - 2, Math.floor(fi))), j = Math.max(0, Math.min(t.rows - 2, Math.floor(fj)));
        const fx = Math.max(0, Math.min(1, fi - i)), fz = Math.max(0, Math.min(1, fj - j));
        const at = (a, b) => h[b * t.cols + a];
        const A = at(i, j), B = at(i + 1, j), C = at(i, j + 1), D = at(i + 1, j + 1);
        const y = fx + fz <= 1 ? A + (B - A) * fx + (C - A) * fz : D + (C - D) * (1 - fx) + (B - D) * (1 - fz);
        return -y;
      }
      const m = this._editorMap;
      if (!m) return 0;
      const c = Math.max(0, Math.min(m.width - 1, Math.floor(x / m.unit) - m.origin[0]));
      const r = Math.max(0, Math.min(m.height - 1, Math.floor(z / m.unit) - m.origin[1]));
      return m.floor[r * m.width + c] || 0;
    }

    /* Where the ray under the point meets the ground: [x, y, z] or null. */
    _edGroundHit(cx, cy) {
      const ray = this._edRay(cx, cy);
      if (!ray) return null;
      /* March only the stretch of the ray between the highest and the
       * lowest ground (a fine step there, not a coarse one over the whole
       * far-plane-long ray, which stepped over raised tiles). */
      const [yTop, yBot] = this._edGroundSpan();
      let ta = 0, tb = 1;
      if (Math.abs(ray.d[1]) > 1e-9) {
        const t1 = (yTop - 2 - ray.o[1]) / ray.d[1], t2 = (yBot + 2 - ray.o[1]) / ray.d[1];
        ta = Math.max(0, Math.min(t1, t2));
        tb = Math.min(1, Math.max(t1, t2));
        if (tb <= ta) return null;
      }
      const len = Math.hypot(ray.d[0], ray.d[1], ray.d[2]) * (tb - ta) || 1;
      const n = Math.min(4000, Math.max(8, Math.ceil(len / 8)));
      let prev = ta;
      for (let k = 1; k <= n; k++) {
        const t = ta + (tb - ta) * k / n;
        const p = [ray.o[0] + ray.d[0] * t, ray.o[1] + ray.d[1] * t, ray.o[2] + ray.d[2] * t];
        if (p[1] >= this._edGround(p[0], p[2])) {
          let lo = prev, hi = t;
          for (let s = 0; s < 12; s++) {
            const mid = (lo + hi) / 2;
            const q = [ray.o[0] + ray.d[0] * mid, ray.o[1] + ray.d[1] * mid, ray.o[2] + ray.d[2] * mid];
            if (q[1] >= this._edGround(q[0], q[2])) hi = mid; else lo = mid;
          }
          return [ray.o[0] + ray.d[0] * hi, ray.o[1] + ray.d[1] * hi, ray.o[2] + ray.d[2] * hi];
        }
        prev = t;
      }
      return null;
    }

    /* The ground's retail Y range `[top, bottom]` (Y down): the
     * heightfield's or the tile floors', kept with the data it reads. */
    _edGroundSpan() {
      const E = this._ed;
      const t = E && E.terrain;
      const src = t ? ((E.proxy && E.proxy.h) || t.h) : (this._editorMap && this._editorMap.floor);
      if (!src) return [-4096, 4096];
      if (this._edSpanOf !== src) {
        let lo = Infinity, hi = -Infinity;
        for (const v of src) { const y = t ? -v : v; if (y < lo) lo = y; if (y > hi) hi = y; }
        this._edSpanOf = src;
        this._edSpan = Number.isFinite(lo) ? [lo, hi] : [-4096, 4096];
      }
      return this._edSpan;
    }

    /* Where the ray meets the level plane at retail Y `y`. */
    _edPlaneHit(cx, cy, y) {
      const ray = this._edRay(cx, cy);
      if (!ray || Math.abs(ray.d[1]) < 1e-6) return null;
      const t = (y - ray.o[1]) / ray.d[1];
      if (t < 0) return null;
      return [ray.o[0] + ray.d[0] * t, y, ray.o[2] + ray.d[2] * t];
    }

    _edTile(x, z) {
      const m = this._editorMap;
      return [x / m.unit - m.origin[0], z / m.unit - m.origin[1]];
    }

    /* An object's pose: its proxy pose during and after a drag, else the
     * workbench's. */
    _edPose(o) {
      const p = this._ed.poses && this._ed.poses[o.key];
      return p || o;
    }

    /* An object's box: [cx, cz, hw, hd, hgt, yaw] (rect objects their
     * rectangle). */
    _edBox(o) {
      const p = this._edPose(o);
      if (p.rect) {
        const [x0, z0, x1, z1] = p.rect;
        return { cx: (x0 + x1) / 2, cz: (z0 + z1) / 2, hw: (x1 - x0) / 2, hd: (z1 - z0) / 2, hgt: o.hgt || 16, yaw: 0 };
      }
      const k = p.scale || 1;
      return { cx: p.x, cz: p.z, hw: (o.size ? o.size[0] : 40) / 2 * k, hd: (o.size ? o.size[1] : 40) / 2 * k, hgt: (o.hgt || 60) * k, yaw: (p.yaw || 0) * Math.PI / 180 };
    }

    /* The object under the point, or null: the nearest point object whose
     * box the ray passes through, else the smallest zone holding the
     * ground point under it. */
    _edPickObject(cx, cy) {
      const ray = this._edRay(cx, cy);
      if (!ray) return null;
      let best = null;
      for (const o of this._ed.objects || []) {
        if (o.rect) continue;
        const b = this._edBox(o);
        const y0 = this._edGround(b.cx, b.cz);
        // Slab test in the box's own frame (turned by -yaw).
        const c = Math.cos(-b.yaw), s = Math.sin(-b.yaw);
        const loc = (p) => { const dx = p[0] - b.cx, dz = p[2] - b.cz; return [dx * c - dz * s, p[1], dx * s + dz * c]; };
        const o0 = loc(ray.o), o1 = loc([ray.o[0] + ray.d[0], ray.o[1] + ray.d[1], ray.o[2] + ray.d[2]]);
        const d = [o1[0] - o0[0], o1[1] - o0[1], o1[2] - o0[2]];
        // A small object is padded so it stays clickable from afar.
        const pad = Math.max(10, (this.cam.halfWidth || 1000) * 0.012);
        const lo = [-b.hw - pad, y0 - b.hgt - pad, -b.hd - pad], hi = [b.hw + pad, y0 + pad, b.hd + pad];
        let t0 = 0, t1 = 1;
        for (let a = 0; a < 3; a++) {
          if (Math.abs(d[a]) < 1e-9) { if (o0[a] < lo[a] || o0[a] > hi[a]) { t0 = 2; break; } continue; }
          let ta = (lo[a] - o0[a]) / d[a], tb = (hi[a] - o0[a]) / d[a];
          if (ta > tb) [ta, tb] = [tb, ta];
          t0 = Math.max(t0, ta); t1 = Math.min(t1, tb);
        }
        if (t0 <= t1 && (!best || t0 < best.t)) best = { o, t: t0 };
      }
      if (best) return best.o;
      const g = this._edGroundHit(cx, cy);
      if (!g) return null;
      let zone = null;
      for (const o of this._ed.objects || []) {
        if (!o.rect) continue;
        const [x0, z0, x1, z1] = this._edPose(o).rect;
        if (g[0] < x0 || g[0] > x1 || g[2] < z0 || g[2] > z1) continue;
        const area = (x1 - x0) * (z1 - z0);
        if (!zone || area < zone.area) zone = { o, area };
      }
      return zone && zone.o;
    }

    /* The selection's pivot on the ground and the gizmo's size. */
    _edPivot() {
      const E = this._ed;
      const sel = (E.objects || []).filter((o) => E.selected.includes(o.key));
      if (!sel.length) return null;
      let x = 0, z = 0;
      for (const o of sel) { const b = this._edBox(o); x += b.cx; z += b.cz; }
      x /= sel.length; z /= sel.length;
      const g = Math.max(60, Math.min(1200, (this.cam.halfWidth || 1000) * 0.22));
      return { x, z, y: this._edGround(x, z), g, sel };
    }

    /* Which gizmo handle is under the point: 'x' | 'z' | 'plane' | 'ring'
     * | 'scale' | null. */
    _edHandle(cx, cy) {
      const E = this._ed;
      const P = this._edPivot();
      if (!P) return null;
      const vp = this._edVp();
      const y = P.y - 6;
      const sp = (p) => this._edScreen(p, vp);
      const near = (a, b, tol) => {
        const A = sp(a), B = sp(b);
        if (!A || !B) return false;
        const dx = B[0] - A[0], dy = B[1] - A[1];
        const l2 = dx * dx + dy * dy || 1;
        const t = Math.max(0, Math.min(1, ((cx - A[0]) * dx + (cy - A[1]) * dy) / l2));
        return Math.hypot(cx - A[0] - dx * t, cy - A[1] - dy * t) < tol;
      };
      if (E.tool === 'move') {
        const q = P.g * 0.32;
        const sq = sp([P.x + q, y, P.z + q]);
        if (sq && Math.hypot(cx - sq[0], cy - sq[1]) < 12) return 'plane';
        if (near([P.x + P.g * 0.15, y, P.z], [P.x + P.g * 1.15, y, P.z], 9)) return 'x';
        if (near([P.x, y, P.z + P.g * 0.15], [P.x, y, P.z + P.g * 1.15], 9)) return 'z';
      } else if (E.tool === 'rotate') {
        for (let k = 0; k < 48; k++) {
          const a0 = k / 48 * Math.PI * 2, a1 = (k + 1) / 48 * Math.PI * 2;
          if (near([P.x + Math.cos(a0) * P.g, y, P.z + Math.sin(a0) * P.g], [P.x + Math.cos(a1) * P.g, y, P.z + Math.sin(a1) * P.g], 8)) return 'ring';
        }
      } else if (E.tool === 'scale') {
        const corner = this._edScaleHandle(P);
        const s = sp([corner[0], y, corner[1]]);
        if (s && Math.hypot(cx - s[0], cy - s[1]) < 13) return 'scale';
      }
      return null;
    }

    /* The scale handle's world (x, z): a zone's far corner, else the
     * pivot's diagonal. */
    _edScaleHandle(P) {
      const o = P.sel.length === 1 ? P.sel[0] : null;
      const p = o && this._edPose(o);
      if (p && p.rect) return [p.rect[2], p.rect[3]];
      return [P.x + P.g * 0.75, P.z + P.g * 0.75];
    }

    /* --- Input --- */

    _edDown(e) {
      const E = this._ed;
      if (E.mode === 'place') {
        const g = this._edGhostAt(e.clientX, e.clientY);
        E.ghost = g;
        this._edDirty = true;
        if (g && g.ok) this._edPost({ type: 'xw-place', tile: g.tile, yaw: (E.place && E.place.yaw) || 0 });
        return true;
      }
      if (E.mode === 'sculpt' || E.mode === 'paint') {
        const hit = this._edGroundHit(e.clientX, e.clientY);
        if (!hit) return false;
        const t = this._edTile(hit[0], hit[2]);
        this._edDrag = { brush: true, path: [t], start: t, h0: -hit[1], last: t };
        if (E.mode === 'paint') this._edPaintAt(t, true);
        else this._edProxy();
        this._edDirty = true;
        return true;
      }
      // Select mode: a handle of the selection first, then an object.
      const handle = this._edHandle(e.clientX, e.clientY);
      let P = this._edPivot();
      if (!handle) {
        const o = this._edPickObject(e.clientX, e.clientY);
        if (!o) return false;
        const add = e.shiftKey || e.ctrlKey || e.metaKey;
        let sel = E.selected.slice();
        if (add) sel = sel.includes(o.key) ? sel.filter((k) => k !== o.key) : sel.concat(o.key);
        else if (!sel.includes(o.key)) sel = [o.key];
        E.selected = sel;
        this._edPost({ type: 'xw-select', keys: sel });
        this._edDirty = true;
        if (add) return true;
        P = this._edPivot();
      }
      if (!P) return true;
      const kind = handle || (E.tool === 'move' ? 'plane' : E.tool === 'rotate' ? 'ring' : 'scale');
      const start = this._edPlaneHit(e.clientX, e.clientY, P.y);
      if (!start) return true;
      const orig = {};
      for (const o of P.sel) orig[o.key] = Object.assign({}, this._edPose(o));
      this._edDrag = { kind, start, P, orig, moved: false };
      return true;
    }

    _edMove(e) {
      const d = this._edDrag;
      const E = this._ed;
      if (d.brush) {
        const hit = this._edGroundHit(e.clientX, e.clientY);
        if (!hit) return;
        const t = this._edTile(hit[0], hit[2]);
        d.cur = t;
        E.cursor = { x: hit[0], z: hit[2] };
        const b = E.brush || {};
        if (Math.hypot(t[0] - d.last[0], t[1] - d.last[1]) >= Math.max(0.15, (b.radius || 2) * 0.25)) {
          d.path.push(t);
          d.last = t;
        }
        if (E.mode === 'paint') this._edPaintAt(t, false);
        else this._edProxy();
        this._edDirty = true;
        return;
      }
      const q = this._edPlaneHit(e.clientX, e.clientY, d.P.y);
      if (!q) return;
      d.moved = true;
      const free = e.shiftKey;
      const poses = {};
      const P = d.P;
      if (d.kind === 'x' || d.kind === 'z' || d.kind === 'plane') {
        let dx = q[0] - d.start[0], dz = q[2] - d.start[2];
        if (d.kind === 'x') dz = 0;
        if (d.kind === 'z') dx = 0;
        // Snap the first object's position; the rest keep their offsets.
        const first = d.orig[P.sel[0].key];
        const rectish = P.sel.some((o) => o.rect || o.tileOnly);
        const grid = rectish ? 128 : free ? 1 : E.snap === 'sub' ? 32 : E.snap === 'free' ? 1 : 128;
        const ref = first.rect ? [first.rect[0], first.rect[1]] : [first.x, first.z];
        const base = first.rect || rectish ? 0 : 64;
        const snapTo = (v) => Math.round((v - base) / grid) * grid + base;
        dx = snapTo(ref[0] + dx) - ref[0];
        dz = snapTo(ref[1] + dz) - ref[1];
        if (rectish) { dx = Math.round(dx / 128) * 128; dz = Math.round(dz / 128) * 128; }
        for (const o of P.sel) {
          const p = d.orig[o.key];
          poses[o.key] = p.rect
            ? Object.assign({}, p, { rect: [p.rect[0] + dx, p.rect[1] + dz, p.rect[2] + dx, p.rect[3] + dz] })
            : Object.assign({}, p, { x: p.x + dx, z: p.z + dz });
        }
      } else if (d.kind === 'ring') {
        const a0 = Math.atan2(d.start[2] - P.z, d.start[0] - P.x);
        const a1 = Math.atan2(q[2] - P.z, q[0] - P.x);
        let dyaw = (a1 - a0) * 180 / Math.PI;
        const step = P.sel.some((o) => o.facing) ? 45 : free ? 1 : 15;
        dyaw = Math.round(dyaw / step) * step;
        const r = dyaw * Math.PI / 180, c = Math.cos(r), s = Math.sin(r);
        for (const o of P.sel) {
          const p = d.orig[o.key];
          if (p.rect || o.noRotate) { poses[o.key] = p; continue; }
          // Turned about the pivot as a group, each about itself too.
          const lx = p.x - P.x, lz = p.z - P.z;
          const turned = P.sel.length > 1 ? { x: P.x + lx * c - lz * s, z: P.z + lx * s + lz * c } : {};
          poses[o.key] = Object.assign({}, p, turned, { yaw: ((p.yaw || 0) + dyaw) % 360 });
        }
      } else if (d.kind === 'scale') {
        const one = P.sel.length === 1 ? P.sel[0] : null;
        if (one && d.orig[one.key].rect) {
          const p = d.orig[one.key];
          const m = this._editorMap;
          const cx = Math.max(p.rect[0] + m.unit, Math.round(q[0] / m.unit) * m.unit);
          const cz = Math.max(p.rect[1] + m.unit, Math.round(q[2] / m.unit) * m.unit);
          poses[one.key] = Object.assign({}, p, { rect: [p.rect[0], p.rect[1], cx, cz] });
        } else {
          const r0 = Math.hypot(d.start[0] - P.x, d.start[2] - P.z) || 1;
          let k = Math.hypot(q[0] - P.x, q[2] - P.z) / r0;
          if (!free) k = Math.round(k * 20) / 20;
          for (const o of P.sel) {
            const p = d.orig[o.key];
            poses[o.key] = o.noScale || p.rect ? p : Object.assign({}, p, { scale: Math.max(0.1, Math.min(8, Math.round((p.scale || 1) * k * 100) / 100)) });
          }
        }
      }
      E.poses = Object.assign({}, E.poses || {}, poses);
      d.poses = poses;
      // Where a moved prop, NPC or spawn cannot stand (red while dragged).
      if (d.kind !== 'scale') {
        const skip = new Set(Object.keys(poses));
        const m = this._editorMap;
        d.blocked = [];
        for (const o of P.sel) {
          if (o.rect || !['props', 'npcs', 'spawns'].includes(o.kind)) continue;
          const p = poses[o.key];
          const c = Math.floor(p.x / m.unit) - m.origin[0], r = Math.floor(p.z / m.unit) - m.origin[1];
          const pl = { kind: o.kind === 'props' ? 'prop' : 'npc', size: o.size ? [o.size[0] * (p.scale || 1), o.size[1] * (p.scale || 1)] : null, solid: o.solid };
          const why = this._edBlocked(pl, c, r, p.yaw || 0, skip);
          if (why) { d.blocked.push(o.key); d.why = `${o.key} at [${c}, ${r}]: ${why}`; }
        }
      }
      this._edDirty = true;
    }

    _edUp() {
      const d = this._edDrag;
      this._edDrag = null;
      const E = this._ed;
      if (d.brush) {
        if (E.mode === 'paint') {
          const tiles = Object.values(E.paintProxy || {}).map((v) => v.t);
          if (tiles.length) this._edPost({ type: 'xw-paint', tiles, char: (E.brush || {}).char });
        } else {
          const shape = this._edShape(d, true);
          if (shape) this._edPost({ type: 'xw-sculpt', shape });
        }
        this._edDirty = true;
        return;
      }
      if (!d.moved || !d.poses) return;
      // Dropped where a piece cannot stand: everything goes back.
      if (d.blocked && d.blocked.length) {
        E.poses = null;
        this._edDirty = true;
        this._edPost({ type: 'xw-key', key: 'blocked', why: d.why || '' });
        return;
      }
      const edits = Object.entries(d.poses).map(([key, p]) => ({ key, x: p.x, z: p.z, yaw: p.yaw, scale: p.scale, rect: p.rect }));
      this._edPost({ type: 'xw-transform', edits });
    }

    _edHover(e) {
      const E = this._ed;
      if (E.mode === 'place') {
        E.ghost = this._edGhostAt(e.clientX, e.clientY);
        this._edDirty = true;
        return;
      }
      if (E.mode === 'sculpt' || E.mode === 'paint') {
        const hit = this._edGroundHit(e.clientX, e.clientY);
        E.cursor = hit ? { x: hit[0], z: hit[2] } : null;
      } else {
        const h = this._edHandle(e.clientX, e.clientY);
        const o = h ? null : this._edPickObject(e.clientX, e.clientY);
        E.hot = h;
        E.hover = o ? o.key : null;
      }
      this._edDirty = true;
    }

    /* The shape the brush stroke `d` lays (the file's own entry). */
    _edShape(d, final) {
      const E = this._ed;
      const b = E.brush || {};
      const r4 = (v) => Math.round(v * 4) / 4;
      const pt = (t) => [r4(t[0]), r4(t[1])];
      const R = Math.max(0.25, b.radius || 2), H = b.height == null ? 24 : b.height, S = b.strength == null ? 1 : b.strength;
      const path = d.path.map(pt);
      const from = pt(d.start), to = pt(d.cur || d.last);
      const moved = Math.hypot(to[0] - from[0], to[1] - from[1]) > 0.5 || path.length > 1;
      const rect = () => {
        const a = [Math.floor(d.start[0]), Math.floor(d.start[1])], c = [Math.floor((d.cur || d.last)[0]), Math.floor((d.cur || d.last)[1])];
        const m = this._editorMap;
        const cl = (v, n) => Math.max(0, Math.min(n - 1, v));
        return [cl(Math.min(a[0], c[0]), m.width), cl(Math.min(a[1], c[1]), m.height), cl(Math.max(a[0], c[0]), m.width), cl(Math.max(a[1], c[1]), m.height)];
      };
      const ground = (t) => -this._edGroundRaw(t);
      switch (b.kind) {
        case 'raise': case 'lower': {
          const h = (b.kind === 'lower' ? -Math.abs(H) : Math.abs(H));
          return moved ? { kind: 'stroke', op: 'raise', path, radius: R, height: h, strength: S } : { kind: 'raise', at: from, radius: R, height: Math.round(h * S) };
        }
        case 'smooth': return moved ? { kind: 'stroke', op: 'smooth', path, radius: R, strength: S } : { kind: 'smooth', at: from, radius: R, strength: S };
        case 'flatten': {
          const level = Math.round(d.h0);
          return moved ? { kind: 'stroke', op: 'flatten', path, radius: R, height: level, strength: S } : { kind: 'flatten', at: from, radius: R, height: level, strength: S };
        }
        case 'ridge': return moved || !final ? { kind: 'ridge', from, to, radius: R, height: H } : null;
        case 'ramp': return moved || !final ? { kind: 'ramp', from, to, radius: R, height: [Math.round(ground(d.start)), Math.round(ground(d.cur || d.last) + H)] } : null;
        case 'plateau': return { kind: 'plateau', rect: rect(), height: Math.round(d.h0 + H), edge: Math.max(0.2, Math.round(R * 0.25 * 4) / 4) };
        case 'noise': return { kind: 'noise', rect: rect(), amplitude: Math.abs(H), scale: R };
        default: return null;
      }
    }

    /* The heightfield's height (up) at map-local tile point `t`, without
     * the proxy. */
    _edGroundRaw(t) {
      const E = this._ed;
      const m = this._editorMap;
      const keep = E.proxy;
      E.proxy = null;
      const y = this._edGround((t[0] + m.origin[0]) * m.unit, (t[1] + m.origin[1]) * m.unit);
      E.proxy = keep;
      return y;
    }

    /* Re-run the stroke so far over a copy of the heightfield. */
    _edProxy() {
      const E = this._ed;
      const t = E.terrain;
      const d = this._edDrag;
      if (!t || !d) return;
      const shape = this._edShape(d, false);
      if (!shape) return;
      const m = this._editorMap;
      const R = shape.radius || 2;
      // The vertex box the stroke can reach.
      let pts = shape.path || [shape.at || shape.from, shape.to].filter(Boolean);
      if (shape.rect) pts = [[shape.rect[0] - 1, shape.rect[1] - 1], [shape.rect[2] + 2, shape.rect[3] + 2]];
      let u0 = Infinity, v0 = Infinity, u1 = -Infinity, v1 = -Infinity;
      for (const p of pts) { u0 = Math.min(u0, p[0]); v0 = Math.min(v0, p[1]); u1 = Math.max(u1, p[0]); v1 = Math.max(v1, p[1]); }
      const toI = (u) => Math.round((u + t.skirt) * t.res);
      const pad = R + 1;
      const box = [Math.max(0, toI(u0 - pad)), Math.max(0, toI(v0 - pad)), Math.min(t.cols - 1, toI(u1 + pad)), Math.min(t.rows - 1, toI(v1 + pad))];
      const h = new Float32Array(t.h);
      runShape(h, t.cols, box, (i, j) => [i / t.res - t.skirt, j / t.res - t.skirt], shape);
      // `base`: the heights before the stroke, which the patch is tinted
      // against until the rebuild (a newer heightfield already holds it).
      E.proxy = { h, box, shape, base: t.h };
      void m;
    }

    /* Mark the tiles under the brush at map-local `t` painted. */
    _edPaintAt(t, first) {
      const E = this._ed;
      const m = this._editorMap;
      if (first) E.paintProxy = {};
      const R = Math.max(0.5, (E.brush && E.brush.radius) || 1);
      for (let r = Math.floor(t[1] - R); r <= Math.ceil(t[1] + R); r++) {
        for (let c = Math.floor(t[0] - R); c <= Math.ceil(t[0] + R); c++) {
          if (c < 0 || r < 0 || c >= m.width || r >= m.height || m.void[r * m.width + c]) continue;
          if (Math.hypot(c + 0.5 - t[0], r + 0.5 - t[1]) > R) continue;
          E.paintProxy[`${c},${r}`] = { t: [c, r] };
        }
      }
    }

    /* --- Placing --- */

    /* The piece being placed (`E.place`: `{ kind, size, hgt, yaw,
     * facing, zone }`) at the tile under the point: `{ tile, x, z, ok,
     * why }`, or null off the map. */
    _edGhostAt(cx, cy) {
      const E = this._ed;
      const m = this._editorMap;
      const pl = E && E.place;
      if (!pl || !m) return null;
      const hit = this._edGroundHit(cx, cy);
      if (!hit) return null;
      const c = Math.floor(hit[0] / m.unit) - m.origin[0], r = Math.floor(hit[2] / m.unit) - m.origin[1];
      if (c < 0 || r < 0 || c >= m.width || r >= m.height) return null;
      const x = (m.origin[0] + c) * m.unit + m.unit / 2, z = (m.origin[1] + r) * m.unit + m.unit / 2;
      const why = this._edBlocked(pl, c, r, pl.yaw || 0, null);
      return { tile: [c, r], x, z, ok: !why, why };
    }

    /* Why piece `pl` cannot stand at tile (c, r) turned `yaw`, or '' when
     * it can: off the map or over void, another prop on its tile (one prop
     * a tile), a solid prop's footprint over another solid one's, a person
     * on a wall block or on another person's tile. `skip` holds the keys being moved. */
    _edBlocked(pl, c, r, yaw, skip) {
      const E = this._ed;
      const m = this._editorDrawnMap ? this._editorDrawnMap() : this._editorMap;
      if (!m) return 'no map';
      const W = m.width, H = m.height;
      const at = (cc, rr) => rr * W + cc;
      if (c < 0 || r < 0 || c >= W || r >= H || m.void[at(c, r)]) return 'off the map';
      if (pl.zone) return '';
      const u = m.unit;
      const cxw = (m.origin[0] + c) * u + u / 2, czw = (m.origin[1] + r) * u + u / 2;
      const quarter = Math.round(((yaw % 180) + 180) % 180 / 90) % 2 === 1;
      const sz = pl.size || [96, 96];
      const hw = (quarter ? sz[1] : sz[0]) / 2, hd = (quarter ? sz[0] : sz[1]) / 2;
      const box = [cxw - hw, czw - hd, cxw + hw, czw + hd];
      if (pl.kind === 'prop') {
        // The tiles under the footprint stand on the map. (Walls under it
        // are not in the way: a building's own tiles are often walled.)
        const c0 = Math.floor(box[0] / u) - m.origin[0], c1 = Math.floor((box[2] - 1) / u) - m.origin[0];
        const r0 = Math.floor(box[1] / u) - m.origin[1], r1 = Math.floor((box[3] - 1) / u) - m.origin[1];
        for (let rr = r0; rr <= r1; rr++) {
          for (let cc = c0; cc <= c1; cc++) {
            if (cc < 0 || rr < 0 || cc >= W || rr >= H || m.void[at(cc, rr)]) return 'its footprint leaves the map';
          }
        }
      } else if (m.walls[at(c, r)] === 15) {
        return 'a wall fills the tile';
      }
      const shrink = 10;
      for (const o of E.objects || []) {
        if (o.rect || (skip && skip.has(o.key))) continue;
        const p = this._edPose(o);
        const oc = Math.floor(p.x / u) - m.origin[0], or = Math.floor(p.z / u) - m.origin[1];
        if (pl.kind === 'prop' && o.kind === 'props') {
          if (oc === c && or === r) return 'another prop holds the tile';
          if (pl.solid === false || o.solid === false) continue;
          const b = this._edBox(o);
          const q = Math.round((((b.yaw * 180 / Math.PI) % 180) + 180) % 180 / 90) % 2 === 1;
          const ow = (q ? b.hd : b.hw), od = (q ? b.hw : b.hd);
          if (box[0] + shrink < b.cx + ow && box[2] - shrink > b.cx - ow && box[1] + shrink < b.cz + od && box[3] - shrink > b.cz - od) return 'it overlaps another prop';
        } else if (pl.kind !== 'prop' && (o.kind === 'npcs' || o.kind === 'spawns') && oc === c && or === r) {
          return 'someone already stands there';
        }
      }
      return '';
    }

    /* --- Drawing --- */

    _edBuild() {
      const E = this._ed;
      const m = this._editorMap;
      const soup = new Soup();
      if (!E || !m || E.mode === 'view') return soup;
      const ground = (x, z) => this._edGround(x, z);
      if (E.mode === 'select') {
        for (const o of E.objects || []) {
          const b = this._edBox(o);
          const y0 = ground(b.cx, b.cz);
          const sel = E.selected.includes(o.key);
          if (sel && E.poses && E.poses[o.key]) {
            // The pose the drag started from, faint.
            const keep = E.poses;
            E.poses = null;
            const ob = this._edBox(o);
            E.poses = keep;
            soup.obox(ob.cx, ob.cz, ob.hw, ob.hd, ground(ob.cx, ob.cz), ob.hgt, ob.yaw, COL.ghost);
          }
          const col = sel ? COL.sel : o.key === E.hover ? COL.hover : COL[o.kind] || null;
          if (!col) continue;
          if (o.rect && !sel && o.key !== E.hover) continue;
          if (o.rect) {
            // A zone: its outline laid over the ground, a fence a little high.
            const [x0, z0, x1, z1] = this._edPose(o).rect;
            const edge = (ax, az, bx, bz) => {
              const n = Math.max(1, Math.ceil(Math.hypot(bx - ax, bz - az) / 32));
              for (let k = 0; k < n; k++) {
                const p0 = [ax + (bx - ax) * k / n, 0, az + (bz - az) * k / n];
                const p1 = [ax + (bx - ax) * (k + 1) / n, 0, az + (bz - az) * (k + 1) / n];
                p0[1] = ground(p0[0], p0[2]) - 6; p1[1] = ground(p1[0], p1[2]) - 6;
                soup.ribbon(p0, p1, 10, sel ? COL.hot : col);
                soup.quad(p0, p1, [p1[0], p1[1] - 18, p1[2]], [p0[0], p0[1] - 18, p0[2]], col);
              }
            };
            edge(x0, z0, x1, z0); edge(x1, z0, x1, z1); edge(x1, z1, x0, z1); edge(x0, z1, x0, z0);
            continue;
          }
          soup.obox(b.cx, b.cz, b.hw, b.hd, y0 + 1, b.hgt, b.yaw, col);
          if (sel && !o.rect && !o.noRotate) {
            // Its facing, a tick on the floor.
            const a = b.yaw, L = Math.max(b.hw, b.hd) + 30;
            soup.ribbon([b.cx, y0 - 3, b.cz], [b.cx + Math.cos(a) * L, y0 - 3, b.cz + Math.sin(a) * L], 8, COL.sel);
          }
        }
        const P = this._edPivot();
        if (P) {
          const y = P.y - 6, g = P.g, w = g * 0.06;
          const hot = (k) => (E.hot === k || (this._edDrag && this._edDrag.kind === k) ? COL.hot : null);
          if (E.tool === 'move') {
            soup.ribbon([P.x + g * 0.15, y, P.z], [P.x + g, y, P.z], w, hot('x') || COL.x);
            soup.tri([P.x + g * 1.18, y, P.z], [P.x + g, y, P.z - w * 1.8], [P.x + g, y, P.z + w * 1.8], hot('x') || COL.x);
            soup.ribbon([P.x, y, P.z + g * 0.15], [P.x, y, P.z + g], w, hot('z') || COL.z);
            soup.tri([P.x, y, P.z + g * 1.18], [P.x + w * 1.8, y, P.z + g], [P.x - w * 1.8, y, P.z + g], hot('z') || COL.z);
            const q = g * 0.32, s = g * 0.1;
            soup.quad([P.x + q - s, y, P.z + q - s], [P.x + q + s, y, P.z + q - s], [P.x + q + s, y, P.z + q + s], [P.x + q - s, y, P.z + q + s], hot('plane') || COL.plane);
          } else if (E.tool === 'rotate') {
            for (let k = 0; k < 48; k++) {
              const a0 = k / 48 * Math.PI * 2, a1 = (k + 1) / 48 * Math.PI * 2;
              soup.ribbon([P.x + Math.cos(a0) * g, y, P.z + Math.sin(a0) * g], [P.x + Math.cos(a1) * g, y, P.z + Math.sin(a1) * g], w, hot('ring') || COL.ring);
            }
          } else if (E.tool === 'scale') {
            const [hx, hz] = this._edScaleHandle(P);
            soup.ribbon([P.x, y, P.z], [hx, y, hz], w * 0.6, COL.scale);
            const s = g * 0.08;
            soup.box(hx - s, hz - s, hx + s, hz + s, y + s, 2 * s, hot('scale') || COL.scale);
          }
        }
      }
      // The piece being placed: its box at the snapped tile, green where it
      // can stand and red where it cannot, its footprint's tiles on the
      // ground and its facing.
      if (E.mode === 'place' && E.ghost && E.place) {
        const g = E.ghost, pl = E.place;
        const col = g.ok ? COL.okFill : COL.badFill;
        const line = g.ok ? COL.ok : COL.bad;
        const y0 = ground(g.x, g.z);
        if (pl.zone) {
          const n = pl.zone;
          const x0 = g.x - m.unit / 2, z0 = g.z - m.unit / 2;
          const x1 = x0 + m.unit * n[0], z1 = z0 + m.unit * n[1];
          soup.quad([x0, y0 - 4, z0], [x1, y0 - 4, z0], [x1, y0 - 4, z1], [x0, y0 - 4, z1], col);
          soup.ribbon([x0, y0 - 6, z0], [x1, y0 - 6, z0], 10, line); soup.ribbon([x1, y0 - 6, z0], [x1, y0 - 6, z1], 10, line);
          soup.ribbon([x1, y0 - 6, z1], [x0, y0 - 6, z1], 10, line); soup.ribbon([x0, y0 - 6, z1], [x0, y0 - 6, z0], 10, line);
        } else {
          const yaw = (pl.yaw || 0) * Math.PI / 180;
          const sz = pl.size || [96, 96];
          soup.obox(g.x, g.z, sz[0] / 2, sz[1] / 2, y0 + 1, pl.hgt || 80, yaw, col);
          const a = yaw, L = Math.max(sz[0], sz[1]) / 2 + 40;
          soup.ribbon([g.x, y0 - 4, g.z], [g.x + Math.cos(a) * L, y0 - 4, g.z + Math.sin(a) * L], 12, line);
          const u = m.unit, x0 = g.x - u / 2, z0 = g.z - u / 2;
          soup.ribbon([x0, y0 - 5, z0], [x0 + u, y0 - 5, z0], 8, line); soup.ribbon([x0 + u, y0 - 5, z0], [x0 + u, y0 - 5, z0 + u], 8, line);
          soup.ribbon([x0 + u, y0 - 5, z0 + u], [x0, y0 - 5, z0 + u], 8, line); soup.ribbon([x0, y0 - 5, z0 + u], [x0, y0 - 5, z0], 8, line);
        }
      }
      // A move drag: the dragged pieces' tiles, red where one cannot stand.
      if (E.mode === 'select' && this._edDrag && this._edDrag.blocked) {
        for (const k of this._edDrag.blocked) {
          const o = (E.objects || []).find((x) => x.key === k);
          if (!o) continue;
          const b = this._edBox(o);
          soup.obox(b.cx, b.cz, b.hw, b.hd, ground(b.cx, b.cz) + 1, b.hgt, b.yaw, COL.badFill);
        }
      }
      // The sculpt proxy: the stroke's patch at its new heights, tinted by
      // which way it moved.
      const t = E.terrain;
      if (E.proxy && t) {
        const { h, box, base } = E.proxy;
        const [i0, j0, i1, j1] = box;
        const V = (i, j) => [t.origin[0] + i * t.step, -h[j * t.cols + i] - 2, t.origin[1] + j * t.step];
        for (let j = j0; j < j1; j++) {
          for (let i = i0; i < i1; i++) {
            const k = [j * t.cols + i, j * t.cols + i + 1, (j + 1) * t.cols + i, (j + 1) * t.cols + i + 1];
            const dh = k.reduce((s, q) => s + h[q] - base[q], 0) / 4;
            if (Math.abs(dh) < 0.5) continue;
            const tint = dh > 0 ? COL.up : COL.down;
            const col = [tint[0], tint[1], tint[2], Math.min(0.75, 0.25 + Math.abs(dh) / 60)];
            const a = V(i, j), b = V(i + 1, j), c = V(i, j + 1), d = V(i + 1, j + 1);
            soup.tri(a, c, b, col);
            soup.tri(b, c, d, col);
          }
        }
      }
      if (E.proxy && E.proxy.shape && E.proxy.shape.rect) {
        const [c0, r0, c1, r1] = E.proxy.shape.rect;
        for (let r = r0; r <= r1; r++) for (let c = c0; c <= c1; c++) {
          const x = (c + m.origin[0]) * m.unit, z = (r + m.origin[1]) * m.unit;
          const y = ground(x + m.unit / 2, z + m.unit / 2) - 4;
          soup.quad([x + 4, y, z + 4], [x + m.unit - 4, y, z + 4], [x + m.unit - 4, y, z + m.unit - 4], [x + 4, y, z + m.unit - 4], COL.paint);
        }
      }
      if (E.paintProxy) {
        for (const { t: [c, r] } of Object.values(E.paintProxy)) {
          const x = (c + m.origin[0]) * m.unit, z = (r + m.origin[1]) * m.unit;
          const q = (a, b) => [x + a, ground(x + a, z + b) - 3, z + b];
          const u = m.unit;
          soup.quad(q(2, 2), q(u - 2, 2), q(u - 2, u - 2), q(2, u - 2), COL.paint);
        }
      }
      // The brush ring and its strength ring.
      if ((E.mode === 'sculpt' || E.mode === 'paint') && E.cursor) {
        const b = E.brush || {};
        const R = Math.max(0.25, b.radius || 2) * m.unit;
        const ring = (rad, col, w) => {
          const n = 64;
          for (let k = 0; k < n; k++) {
            const a0 = k / n * Math.PI * 2, a1 = (k + 1) / n * Math.PI * 2;
            const p0 = [E.cursor.x + Math.cos(a0) * rad, 0, E.cursor.z + Math.sin(a0) * rad];
            const p1 = [E.cursor.x + Math.cos(a1) * rad, 0, E.cursor.z + Math.sin(a1) * rad];
            p0[1] = ground(p0[0], p0[2]) - 4;
            p1[1] = ground(p1[0], p1[2]) - 4;
            soup.ribbon(p0, p1, w, col);
          }
        };
        ring(R, COL.brush, Math.max(4, R * 0.03));
        if (E.mode === 'sculpt') ring(R * Math.max(0.05, Math.min(1, b.strength == null ? 1 : b.strength)), COL.inner, Math.max(3, R * 0.02));
        const y = ground(E.cursor.x, E.cursor.z);
        soup.box(E.cursor.x - 5, E.cursor.z - 5, E.cursor.x + 5, E.cursor.z + 5, y, 30, COL.brush);
      }
      return soup;
    }

    /* Draw the editing layer over the finished frame (after the overlay). */
    _drawEditorGizmos() {
      const E = this._ed;
      if (!E || E.mode === 'view' || !this.renderer || !this._editorMap) return;
      const gl = this.renderer.gl;
      if (!this._edProg) {
        try { this._edProg = compileProgram(gl, VS, FS); } catch (e) { console.warn('editor gizmos disabled:', e); this._ed = null; return; }
        const p = this._edProg;
        this._edLoc = {
          mvp: gl.getUniformLocation(p, 'u_mvp'), log: gl.getUniformLocation(p, 'u_log_depth_on'),
          alpha: gl.getUniformLocation(p, 'u_alpha'), pos: gl.getAttribLocation(p, 'a_position'), col: gl.getAttribLocation(p, 'a_color'),
        };
        this._edVao = gl.createVertexArray();
        this._edBuf = gl.createBuffer();
        gl.bindVertexArray(this._edVao);
        gl.bindBuffer(gl.ARRAY_BUFFER, this._edBuf);
        gl.enableVertexAttribArray(this._edLoc.pos);
        gl.vertexAttribPointer(this._edLoc.pos, 3, gl.FLOAT, false, 28, 0);
        gl.enableVertexAttribArray(this._edLoc.col);
        gl.vertexAttribPointer(this._edLoc.col, 4, gl.FLOAT, false, 28, 12);
        gl.bindVertexArray(null);
        this._edDirty = true;
      }
      // The gizmo scales with the camera: rebuild when it moves too.
      const camKey = `${this.cam.halfWidth}|${this.cam.yaw}|${this.cam.pitch}`;
      if (this._edDirty || camKey !== this._edCamKey) {
        this._edCamKey = camKey;
        const v = this._edBuild().v;
        this._edCount = v.length / 7;
        gl.bindBuffer(gl.ARRAY_BUFFER, this._edBuf);
        gl.bufferData(gl.ARRAY_BUFFER, new Float32Array(v), gl.DYNAMIC_DRAW);
        this._edDirty = false;
      }
      if (!this._edCount) return;
      const L = this._edLoc;
      gl.useProgram(this._edProg);
      gl.bindVertexArray(this._edVao);
      gl.uniformMatrix4fv(L.mvp, false, this._edVp());
      gl.uniform1i(L.log, this.renderer.lastLogDepth ? 1 : 0);
      gl.enable(gl.BLEND);
      gl.blendEquation(gl.FUNC_ADD);
      gl.blendFunc(gl.SRC_ALPHA, gl.ONE_MINUS_SRC_ALPHA);
      gl.disable(gl.CULL_FACE);
      gl.depthMask(false);
      // Through everything faint, then solid where in front: a gizmo is
      // always reachable.
      gl.disable(gl.DEPTH_TEST);
      gl.uniform1f(L.alpha, 0.45);
      gl.drawArrays(gl.TRIANGLES, 0, this._edCount);
      gl.enable(gl.DEPTH_TEST);
      gl.depthFunc(gl.LEQUAL);
      gl.uniform1f(L.alpha, 1.0);
      gl.drawArrays(gl.TRIANGLES, 0, this._edCount);
      gl.depthMask(true);
      gl.disable(gl.BLEND);
      gl.bindVertexArray(null);
      gl.useProgram(this.renderer.program);
    }
  });

  const P = window.PlayView.prototype;
  /* The workbench's preview stays silent while it is an editor: the BGM
   * mixer runs in a ScriptProcessorNode on this page's main thread - the
   * workbench's own, the frame being same-origin - and was most of a drag's
   * cost (the frame's own time under a drag, measured). Its first Test play
   * (`editorAudio`) brings the sound up. */
  const enableAudio = P._enableAudio;
  if (typeof enableAudio === 'function' && /[?&]xwpreview=1\b/.test(location.search)) {
    P._enableAudio = function () {
      if (!this.editorAudio) return undefined;
      return enableAudio.call(this);
    };
  }
  /* Draw the editing layer with the overlay (play-view-frame.js calls the
   * overlay's draw after the fog pass). */
  const drawOverlay = P._drawEditorOverlay;
  P._drawEditorOverlay = function () {
    drawOverlay.call(this);
    this._drawEditorGizmos();
  };
  window.LegaiaEditorGizmos = { runShape };
})();
