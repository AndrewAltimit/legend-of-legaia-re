/* Expansion workbench: editing a map in the 3D preview.
 *
 * The preview (expansion-workbench-preview3d.js) frames the play page; with
 * an edit mode on, the frame's editing layer (js/play-view-editor-gizmos.js)
 * takes the pointer and answers with what the modder did. This side owns
 * everything that writes: it turns those answers into edits of the map
 * definition, the same `MapDef` the 2D painter edits, one step of the page
 * history each (`api.snapshot` / `api.refresh`), so a 3D edit shows in the
 * painter, its forms and the exported TOML at once, and Ctrl+Z takes it
 * back in either view.
 *
 *   - **Select**: props, decorations, NPCs, spawns, lamps, emitters,
 *     triggers, exits and encounter zones; move (tile, 32-unit sub-tile or
 *     free with Shift), rotate (props and decorations by degrees, NPCs and
 *     spawns by facing sector), scale (props and decorations; a zone's
 *     corner resizes it). Duplicate (Ctrl+D) and delete.
 *   - **Sculpt**: the terrain brushes - raise, lower, smooth, flatten (to
 *     the height where the stroke starts), ridge, ramp, plateau, noise - each
 *     drag one entry of `[terrain].shapes` (`stroke` for a drag path).
 *   - **Paint**: a material's tile character under the brush.
 *
 * The selection is shared with the 2D painter: a list of `{ type, i }`
 * (`type` a map list - `props`, `decor`, `npcs`, `spawns`, `lamps`,
 * `emitters`, `triggers`, `exits`, `encounters`). The painter's box select
 * and multi-select feed it through `select()`.
 *
 * Undo seam: every write calls `api.snapshot()` first and `api.refresh()`
 * after, which close the page history's step around it.
 */
import { h } from './expansion-workbench-forms.js';
import { layoutTools, layoutSummary, nudge } from './expansion-workbench-layout.js';

const UNIT = 128;
const TYPES = ['props', 'decor', 'npcs', 'spawns', 'lamps', 'emitters', 'triggers', 'exits', 'encounters'];
const RECTS = new Set(['emitters', 'triggers', 'exits', 'encounters']);
const BRUSHES = [
  ['raise', 'Raise'], ['lower', 'Lower'], ['smooth', 'Smooth'], ['flatten', 'Flatten'],
  ['ridge', 'Ridge'], ['ramp', 'Ramp'], ['plateau', 'Plateau'], ['noise', 'Noise'],
];

// Edit state survives the tab re-rendering.
const G = {
  mode: 'view', tool: 'move', snap: 'tile', sel: [],
  brush: { kind: 'raise', radius: 2.5, height: 24, strength: 1, char: '' },
  api: null, sentTerrain: undefined, sentObjects: '', bar: null,
  // The layout row's counts and steps (expansion-workbench-layout.js),
  // shared with the painter's.
  layout: {}, layoutMsg: '',
};

/** Run layout edit `fn(map, sel)` as one undo step over `sel`, through
 * `api` (the editor's), and keep its account for the bar. */
function runLayout(api, sel, fn) {
  const map = api.map();
  if (!map || !sel.length) return null;
  api.snapshot();
  const r = fn(map, sel);
  G.sel = (r && r.sel) || sel;
  G.layoutMsg = layoutSummary(r);
  api.selected(G.sel);
  api.refresh(true);
  renderBar();
  sync(true);
  return r;
}

/** The layout row over the shared selection, for the painter's side
 * (`sel` the painter's own when nothing is multi-selected; `painterApi`
 * the painter's `{ map(), snapshot(), refresh(), selected(list) }`). */
export function layoutRow(sel, painterApi) {
  const api = painterApi || G.api;
  if (!api) return null;
  const list = G.sel.length ? G.sel : (sel || []);
  const row = layoutTools(Object.assign(G.layout, { h, sel: () => list, run: (fn) => runLayout(api, list, fn) }));
  if (G.layoutMsg) row.append(h('span', { class: 'xw-hint' }, ' ' + G.layoutMsg));
  return row;
}

/** The list behind selection type `type` on `map` (created when asked). */
export function listOf(map, type, make) {
  if (type === 'lamps') {
    if (!map.lighting) { if (!make) return []; map.lighting = { lamps: [] }; }
    return map.lighting.lamps || (make ? (map.lighting.lamps = []) : []);
  }
  return map[type] || (make ? (map[type] = []) : []);
}

/** Every editable object of `map` with its world pose, for the frame. */
export function frameObjects(map) {
  const o0 = (map.tiles && map.tiles.origin) || [0, 0];
  const cx = (c) => (o0[0] + c) * UNIT + UNIT / 2;
  const cz = (r) => (o0[1] + r) * UNIT + UNIT / 2;
  const out = [];
  for (const type of TYPES) {
    listOf(map, type).forEach((o, i) => {
      const key = `${type}:${i}`;
      if (RECTS.has(type)) {
        const q = o.rect || [0, 0, 0, 0];
        const [c0, r0, c1, r1] = [Math.min(q[0], q[2]), Math.min(q[1], q[3]), Math.max(q[0], q[2]), Math.max(q[1], q[3])];
        out.push({ key, kind: type === 'emitters' ? 'emitter' : type, rect: [(o0[0] + c0) * UNIT, (o0[1] + r0) * UNIT, (o0[0] + c1 + 1) * UNIT, (o0[1] + r1 + 1) * UNIT], hgt: 14 });
        return;
      }
      const t = o.tile || [0, 0];
      const off = o.offset || [0, 0, 0];
      if (type === 'props' || type === 'decor') {
        const fp = o.footprint || [type === 'props' ? 96 : 48, type === 'props' ? 96 : 48];
        out.push({ key, kind: type === 'decor' ? 'decor' : 'props', x: cx(t[0]) + (off[0] || 0), z: cz(t[1]) + (off[2] || 0), yaw: (o.rotation || [0, 0, 0])[1] || 0, scale: o.scale || 1, size: fp, hgt: o.top != null ? Math.max(24, o.top) : (type === 'props' ? 72 : 40) });
      } else if (type === 'lamps') {
        out.push({ key, kind: 'lamp', x: cx(t[0]) + (off[0] || 0), z: cz(t[1]) + (off[2] || 0), size: [28, 28], hgt: 50 + Math.max(0, off[1] || 0), noRotate: true, noScale: true });
      } else {
        // NPCs and spawns stand on a tile and face a sector (0 faces +Z):
        // the painter's yaw sense is `90 - sector * 45`.
        out.push({ key, kind: type, x: cx(t[0]), z: cz(t[1]), yaw: 90 - (o.facing || 0) * 45, size: [44, 44], hgt: type === 'npcs' ? 150 : 110, tileOnly: true, facing: true, noScale: true });
      }
    });
  }
  return out;
}

function dims(map) {
  const rows = map.tiles.rows || [];
  return [rows.reduce((m, r) => Math.max(m, [...r].length), 0), rows.length];
}

/** Apply the frame's absolute poses (`xw-transform` edits) to `map`. */
export function applyTransforms(map, edits) {
  const o0 = map.tiles.origin || [0, 0];
  const [W, H] = dims(map);
  const clamp = (v, n) => Math.max(0, Math.min(n - 1, v));
  const moving = new Set(edits.map((e) => e.key));
  const propTiles = new Set();
  listOf(map, 'props').forEach((p, i) => { if (!moving.has(`props:${i}`)) propTiles.add(`${p.tile[0]},${p.tile[1]}`); });
  for (const e of edits) {
    const [type, si] = e.key.split(':');
    const o = listOf(map, type)[+si];
    if (!o) continue;
    if (RECTS.has(type) && e.rect) {
      const c0 = clamp(Math.round(e.rect[0] / UNIT) - o0[0], W), r0 = clamp(Math.round(e.rect[1] / UNIT) - o0[1], H);
      const c1 = clamp(Math.round(e.rect[2] / UNIT) - o0[0] - 1, W), r1 = clamp(Math.round(e.rect[3] / UNIT) - o0[1] - 1, H);
      o.rect = [Math.min(c0, c1), Math.min(r0, r1), Math.max(c0, c1), Math.max(r0, r1)];
      continue;
    }
    if (e.x == null) continue;
    let c = clamp(Math.floor(e.x / UNIT) - o0[0], W), r = clamp(Math.floor(e.z / UNIT) - o0[1], H);
    if (type === 'props' && propTiles.has(`${c},${r}`)) {
      // One prop a tile: keep its tile and reach over with the offset.
      [c, r] = o.tile;
    }
    if (type === 'props') propTiles.add(`${c},${r}`);
    const centre = [(o0[0] + c) * UNIT + UNIT / 2, (o0[1] + r) * UNIT + UNIT / 2];
    const i16 = (v) => Math.max(-32768, Math.min(32767, Math.round(v)));
    if (type === 'npcs' || type === 'spawns') {
      const dc = c - o.tile[0], dr = r - o.tile[1];
      o.tile = [c, r];
      if (o.patrol && (dc || dr)) o.patrol = o.patrol.map((t) => [t[0] + dc, t[1] + dr]);
      if (e.yaw != null) o.facing = (((Math.round((90 - e.yaw) / 45)) % 8) + 8) % 8;
      continue;
    }
    o.tile = [c, r];
    const off = o.offset || [0, 0, 0];
    o.offset = [i16(e.x - centre[0]), off[1] || 0, i16(e.z - centre[1])];
    if (type === 'lamps') continue;
    if (e.yaw != null) {
      const rot = o.rotation || [0, 0, 0];
      let y = Math.round(e.yaw * 10) / 10;
      y = ((y % 360) + 360) % 360;
      o.rotation = [rot[0] || 0, y > 180 ? y - 360 : y, rot[2] || 0];
    }
    if (e.scale != null) o.scale = Math.abs(e.scale - 1) < 1e-3 ? null : e.scale;
  }
}

/** The objects whose tile (or zone) lies in tile rect `q` (inclusive). */
export function objectsIn(map, q) {
  const [c0, r0, c1, r1] = [Math.min(q[0], q[2]), Math.min(q[1], q[3]), Math.max(q[0], q[2]), Math.max(q[1], q[3])];
  const out = [];
  for (const type of TYPES) {
    listOf(map, type).forEach((o, i) => {
      if (o.rect) {
        const z = o.rect;
        const inside = Math.min(z[0], z[2]) >= c0 && Math.max(z[0], z[2]) <= c1 && Math.min(z[1], z[3]) >= r0 && Math.max(z[1], z[3]) <= r1;
        if (inside) out.push({ type, i });
      } else if (o.tile && o.tile[0] >= c0 && o.tile[0] <= c1 && o.tile[1] >= r0 && o.tile[1] <= r1) {
        out.push({ type, i });
      }
    });
  }
  return out;
}

/** Remove `sel` from `map`. */
export function deleteObjects(map, sel) {
  const by = {};
  for (const s of sel) (by[s.type] = by[s.type] || []).push(s.i);
  for (const [type, idx] of Object.entries(by)) {
    const list = listOf(map, type);
    for (const i of [...new Set(idx)].sort((a, b) => b - a)) list.splice(i, 1);
  }
}

function freshIdIn(base, used) {
  let n = 1;
  const stem = String(base || 'obj').replace(/_\d+$/, '');
  while (used.has(`${stem}_${n}`)) n++;
  const id = `${stem}_${n}`;
  used.add(id);
  return id;
}

/** Copy `sel` one tile over (right, else down); returns the copies'
 * selection. Ids are made unique; a prop's copy goes to a free tile. */
export function duplicateObjects(map, sel, usedIds) {
  const [W, H] = dims(map);
  const out = [];
  const propTiles = new Set(listOf(map, 'props').map((p) => `${p.tile[0]},${p.tile[1]}`));
  for (const s of sel) {
    const src = listOf(map, s.type)[s.i];
    if (!src) continue;
    const o = JSON.parse(JSON.stringify(src));
    if (o.id != null) {
      const used = new Set([...(usedIds || []), ...listOf(map, s.type).map((x) => x.id)]);
      o.id = freshIdIn(o.id, used);
    }
    const shift = (dc, dr) => {
      if (o.tile) o.tile = [Math.min(W - 1, o.tile[0] + dc), Math.min(H - 1, o.tile[1] + dr)];
      if (o.rect) o.rect = [o.rect[0] + dc, o.rect[1] + dr, o.rect[2] + dc, o.rect[3] + dr].map((v, k) => Math.min((k % 2 ? H : W) - 1, v));
      if (o.patrol) o.patrol = o.patrol.map((t) => [t[0] + dc, t[1] + dr]);
    };
    if (s.type === 'props') {
      // The nearest free tile, spiralling out.
      let done = false;
      for (let ring = 1; ring < 6 && !done; ring++) {
        for (let dr = -ring; dr <= ring && !done; dr++) {
          for (let dc = -ring; dc <= ring && !done; dc++) {
            const c = src.tile[0] + dc, r = src.tile[1] + dr;
            if (c < 0 || r < 0 || c >= W || r >= H || propTiles.has(`${c},${r}`)) continue;
            o.tile = [c, r];
            propTiles.add(`${c},${r}`);
            done = true;
          }
        }
      }
      if (!done) continue;
    } else {
      shift(src.tile && src.tile[0] + 1 < W ? 1 : 0, src.tile && src.tile[0] + 1 >= W ? 1 : 0);
    }
    const list = listOf(map, s.type, true);
    list.push(o);
    out.push({ type: s.type, i: list.length - 1 });
  }
  return out;
}

/** Paint tile character `ch` on `tiles` ([c, r] each). */
export function paintTiles(map, tiles, ch) {
  const rows = map.tiles.rows;
  let changed = false;
  for (const [c, r] of tiles) {
    if (r < 0 || r >= rows.length) continue;
    const row = [...rows[r]];
    if (c < 0 || c >= row.length || row[c] === ch || row[c] === ' ') continue;
    row[c] = ch;
    rows[r] = row.join('');
    changed = true;
  }
  return changed;
}

/** The painter's multi-selection outline, drawn over its canvas. */
export function drawSelection(g, map, cs) {
  if (G.sel.length < 2) return;
  g.save();
  g.strokeStyle = '#ffffff';
  g.lineWidth = 2;
  g.setLineDash([4, 2]);
  for (const s of G.sel) {
    const o = listOf(map, s.type)[s.i];
    if (!o) continue;
    if (o.rect) {
      const q = o.rect;
      const x0 = Math.min(q[0], q[2]) * cs, y0 = Math.min(q[1], q[3]) * cs;
      g.strokeRect(x0 + 1, y0 + 1, (Math.abs(q[2] - q[0]) + 1) * cs - 2, (Math.abs(q[3] - q[1]) + 1) * cs - 2);
    } else if (o.tile) {
      const off = o.offset || [0, 0, 0];
      const x = (o.tile[0] + 0.5) * cs + (off[0] || 0) / UNIT * cs, y = (o.tile[1] + 0.5) * cs + (off[2] || 0) / UNIT * cs;
      g.strokeRect(x - cs * 0.45, y - cs * 0.45, cs * 0.9, cs * 0.9);
    }
  }
  g.restore();
}

/** The current selection (`{ type, i }` list). */
export function selection() { return G.sel.slice(); }

/** Set the selection (from the painter); the frame follows. */
export function select(list) {
  G.sel = (list || []).filter((s) => s && TYPES.includes(s.type));
  G.layoutMsg = '';
  renderBar();
  sync(true);
}

/* Send the frame what it edits: mode, brush, objects, selection, and the
 * heightfield when it changed. `force` re-sends everything. */
function sync(force) {
  const api = G.api;
  if (!api || !api.preview) return;
  const map = api.map();
  if (!map) return;
  const objs = frameObjects(map);
  const key = JSON.stringify(objs);
  const msg = {
    mode: G.mode, tool: G.tool, snap: G.snap, brush: G.brush,
    selected: G.sel.map((s) => `${s.type}:${s.i}`),
  };
  if (force || key !== G.sentObjects) { msg.objects = objs; G.sentObjects = key; }
  const pv = api.mapPreview();
  const t = pv && !pv.error && map.terrain ? pv.terrain : null;
  if (force || t !== G.sentTerrain) {
    msg.terrain = t && t.origin ? t : null;
    G.sentTerrain = t;
  }
  api.preview.edit(msg);
}

/* The frame answered. */
function onFrame(m) {
  G.log = (G.log || []).concat(m.type).slice(-20);
  const api = G.api;
  if (!api) return;
  const map = api.map();
  if (!map) return;
  if (m.type === 'xw-select') {
    G.sel = (m.keys || []).map((k) => { const [type, i] = k.split(':'); return { type, i: +i }; });
    G.layoutMsg = '';
    api.selected(G.sel);
    renderBar();
  } else if (m.type === 'xw-transform') {
    api.snapshot();
    applyTransforms(map, m.edits || []);
    api.refresh();
  } else if (m.type === 'xw-sculpt') {
    if (!map.terrain || !m.shape) return;
    api.snapshot();
    (map.terrain.shapes || (map.terrain.shapes = [])).push(m.shape);
    api.refresh(true);
  } else if (m.type === 'xw-paint') {
    if (!m.char) return;
    api.snapshot();
    if (paintTiles(map, m.tiles || [], m.char)) api.refresh();
  } else if (m.type === 'xw-key') {
    if (m.key === 'Delete' || m.key === 'Backspace') removeSel();
    else if (m.ctrl && m.key === 'd') duplicateSel();
    else if (m.ctrl && m.key === 'z') api.undo(m.shift ? 1 : -1);
    else if (m.ctrl && m.key === 'y') api.undo(1);
    else if (m.key === 'nudge' && G.sel.length) {
      // One tile, or a quarter (32 units) with Shift or the 32-unit snap.
      const step = m.fine || G.snap === 'sub' ? 0.25 : 1;
      runLayout(api, G.sel, (map, sel) => nudge(map, sel, (m.dc | 0) * step, (m.dr | 0) * step));
      G.layoutMsg = '';
      renderBar();
    } else if (m.key === 'tool' && ['move', 'rotate', 'scale'].includes(m.tool)) {
      G.tool = m.tool;
      renderBar();
      sync();
    } else if (m.key === 'deselect' && G.sel.length) {
      G.sel = [];
      api.selected(G.sel);
      renderBar();
      sync(true);
    }
    else if (m.key === '[' || m.key === ']') {
      G.brush.radius = Math.max(0.25, Math.min(16, G.brush.radius * (m.key === ']' ? 1.25 : 0.8)));
      renderBar();
      sync();
    }
  }
}

function removeSel() {
  const api = G.api;
  if (!G.sel.length) return;
  api.snapshot();
  deleteObjects(api.map(), G.sel);
  G.sel = [];
  api.selected(G.sel);
  api.refresh(true);
}

function duplicateSel() {
  const api = G.api;
  if (!G.sel.length) return;
  api.snapshot();
  G.sel = duplicateObjects(api.map(), G.sel, api.ids ? api.ids() : []);
  api.selected(G.sel);
  api.refresh(true);
}

/* The materials a paint brush offers: each legend character, named by the
 * terrain material that claims it. */
function paintChoices(map) {
  const mats = (map.terrain && map.terrain.materials) || [];
  return Object.entries(map.tiles.legend || {})
    .filter(([, st]) => st && st.walls !== 'all')
    .map(([ch, st]) => {
      const m = mats.find((x) => (x.chars || '').includes(ch));
      return [ch, `${ch}  ${m ? m.name || 'material' : st.label || 'tile'}`];
    });
}

function renderBar() {
  const bar = G.bar;
  const api = G.api;
  if (!bar || !api) return;
  bar.innerHTML = '';
  const map = api.map();
  const b = (label, on, title, fn) => h('button', { type: 'button', class: 'wb-mini' + (on ? ' is-on' : ''), title, onclick: fn }, label);
  const setMode = (m) => { G.mode = m; G.picked = true; renderBar(); sync(); };
  bar.append(h('span', { class: 'wb-label' }, 'Edit'),
    b('View', G.mode === 'view', 'The picture is only looked at: a click picks a tile', () => setMode('view')),
    b('Select', G.mode === 'select', 'Pick, move, turn and scale props, decorations, NPCs, spawns, lamps, emitters and zones', () => setMode('select')));
  if (map && map.terrain) {
    bar.append(b('Sculpt', G.mode === 'sculpt', 'Shape the terrain with a brush ([ and ] size it)', () => setMode('sculpt')),
      b('Paint', G.mode === 'paint', 'Paint a ground material under the brush', () => setMode('paint')));
  }
  const num = (label, key, step, min, max) => {
    const i = h('input', { type: 'number', value: G.brush[key], step, min, max, 'aria-label': label, class: 'xw-num' });
    i.addEventListener('change', () => { const v = parseFloat(i.value); if (Number.isFinite(v)) { G.brush[key] = Math.max(min, Math.min(max, v)); sync(); } });
    return h('label', { class: 'xw-check' }, label, ' ', i);
  };
  if (G.mode === 'select') {
    const tool = (k, l, t) => b(l, G.tool === k, t, () => { G.tool = k; renderBar(); sync(); });
    const snap = h('select', { 'aria-label': 'Snap' },
      ...[['tile', 'Snap: tile'], ['sub', 'Snap: 32 units'], ['free', 'Free']].map(([k, l]) => h('option', { value: k, selected: G.snap === k }, l)));
    snap.addEventListener('change', () => { G.snap = snap.value; sync(); });
    bar.append(h('span', { class: 'xw-sep' }), tool('move', 'Move', 'Arrows move on one axis, the square on both; drag an object to move it (W)'),
      tool('rotate', 'Rotate', 'Drag the ring: 15 degree steps (NPCs and spawns by facing sector) (E)'),
      tool('scale', 'Scale', 'Drag the corner handle; a zone\'s corner resizes it (R)'), snap,
      b('Duplicate', false, 'Copy the selection one tile over (Ctrl+D)', duplicateSel),
      b('Delete', false, 'Remove the selection (Delete)', removeSel),
      h('span', { class: 'xw-hint' }, ` ${G.sel.length ? `${G.sel.length} selected; arrows nudge (Shift a quarter tile)` : 'click an object; Shift adds'}; Shift while dragging places freely; W / E / R move, rotate, scale`));
    if (G.sel.length) {
      bar.append(layoutTools(Object.assign(G.layout, { h, sel: () => G.sel, run: (fn) => runLayout(api, G.sel, fn) })));
      if (G.layoutMsg) bar.append(h('span', { class: 'xw-hint' }, ' ' + G.layoutMsg));
    }
  } else if (G.mode === 'sculpt') {
    const kind = h('select', { 'aria-label': 'Brush' }, ...BRUSHES.map(([k, l]) => h('option', { value: k, selected: G.brush.kind === k }, l)));
    kind.addEventListener('change', () => { G.brush.kind = kind.value; sync(); });
    bar.append(h('span', { class: 'xw-sep' }), kind, num('Radius', 'radius', 0.25, 0.25, 16), num('Height', 'height', 2, -400, 400),
      num('Strength', 'strength', 0.1, 0, 1),
      h('span', { class: 'xw-hint' }, ' drag to stroke; flatten levels to where you start; ridge, ramp, plateau and noise drag from one point to another'));
  } else if (G.mode === 'paint') {
    const ch = paintChoices(map);
    if (!ch.some(([k]) => k === G.brush.char)) G.brush.char = ch.length ? ch[0][0] : '';
    const pick = h('select', { 'aria-label': 'Material' }, ...ch.map(([k, l]) => h('option', { value: k, selected: G.brush.char === k }, l)));
    pick.addEventListener('change', () => { G.brush.char = pick.value; sync(); });
    bar.append(h('span', { class: 'xw-sep' }), pick, num('Radius', 'radius', 0.25, 0.5, 16));
  }
}

/** Mount the 3D editor for the map tab. `api`: { preview, map(),
 * mapPreview(), snapshot(), refresh(structural), undo(dir), selected(list),
 * ids() }. Called on every render of the tab; returns the edit bar. */
export function mountMapEditor(api) {
  G.api = api;
  if (!api.preview) return null;
  if (!G.bar) G.bar = h('div', { class: 'xw-tools xw-3d-edit', role: 'toolbar', 'aria-label': '3D editing' });
  api.preview.onEdit(onFrame);
  api.preview.setEditBar(G.bar);
  /* Split and 3D are for working in the picture: they open in Select
   * (unless a mode was picked by hand), 2D in View as before. */
  if (api.preview.onView) {
    const byView = (view) => {
      if (G.picked || view === '2d' || G.mode !== 'view') return;
      G.mode = 'select';
      renderBar();
      sync(true);
    };
    api.preview.onView(byView);
    byView(api.preview.state().view);
  }
  renderBar();
  sync(true);
  return G.bar;
}

/** The map or its preview changed: send the frame what moved. */
export function editorChanged() { sync(false); }

/** Drop the edit selection (a map switch, an undo). */
export function clearSelection() { G.sel = []; }

/** Headless-verification hook. */
export const editorState = () => ({ mode: G.mode, tool: G.tool, snap: G.snap, sel: G.sel.slice(), brush: { ...G.brush }, log: (G.log || []).slice() });
export function setEditor(o) {
  Object.assign(G, o.mode ? { mode: o.mode } : {}, o.tool ? { tool: o.tool } : {}, o.snap ? { snap: o.snap } : {});
  if (o.brush) Object.assign(G.brush, o.brush);
  renderBar();
  sync(true);
}

// Headless-verification hook.
if (typeof window !== 'undefined') window.__xwEdit = { state: editorState, set: setEditor, select, layout: (fn) => runLayout(G.api, G.sel, fn) };
