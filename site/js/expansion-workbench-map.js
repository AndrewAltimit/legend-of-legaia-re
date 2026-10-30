/* Expansion workbench: the 2D tile painter for `maps/<id>.toml`.
 *
 * The painter edits a `MapDef` (legaia_expansion_format::map) 1:1: its canvas
 * is `tiles.rows` (one character per tile) and its palette is `tiles.legend`,
 * so a painted map and a hand-written one are the same file. Spawns, exits,
 * trigger and encounter zones, props and NPCs are placed on the same grid,
 * in map-local tiles `[column, row]` (column = world +X, row = world +Z).
 *
 * An NPC's model is a retail placement by coordinate (`{ kind = "npc",
 * scene, index }`); the picker shows the first line each placement says,
 * which is the only name retail gives an NPC, and the pack stores only the
 * coordinate.
 *
 * What a tile means to the engine is not re-derived here. The session's
 * `map_preview` builds the synthetic field image the engine itself enters and
 * reads back each tile's floor tier and wall quarters and each cell's four
 * corner heights; the height overlay draws those. That is where the corner
 * rule shows: the floor sampler reads a tile's tier at its low-X / low-Z
 * corner and interpolates across the cell, so a raised block slopes down
 * along its right and bottom edges (docs/formats/expansion-pack-maps.md,
 * "Tiers are corner heights").
 */

import { h, field, grid, freshId, gateBox } from './expansion-workbench-forms.js';
import { propMeshEditor, propDetail, renderDetail } from './expansion-workbench-detail.js';

const QUARTERS = ['tl', 'tr', 'bl', 'br'];
const QBIT = { tl: 1, tr: 2, bl: 4, br: 8 };
const SPARE_CHARS = '#.,^=~*+o%&@:;abcdefghijklmnopqrstuvwxyz0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ';
const SWATCH = ['#6b8f71', '#8a9a5b', '#b08d57', '#7d8fa6', '#a37a74', '#6f9fa0', '#9c8bb0', '#b5a26e', '#7fa86b', '#a88a9c'];
const TOOLS = [
  ['paint', 'Paint', 'B'], ['rect', 'Fill rect', 'R'], ['pick', 'Pick', 'I'], ['select', 'Select / move', 'V'],
  ['spawn', 'Spawn', 'S'], ['exit', 'Exit', 'E'], ['trigger', 'Trigger', 'T'], ['encounter', 'Encounters', 'N'], ['prop', 'Prop', 'P'],
  ['npc', 'NPC', 'C'],
];

// View state survives the tab re-rendering on every structural edit.
const V = {
  map: null, tool: 'paint', brush: '.', cell: 22, heights: true, sel: null,
  scroll: {}, undo: {}, redo: {}, preview: {}, previewTimer: null, objects: {}, objectsPending: null,
  propObject: 0, drag: null, npcs: {}, npcModel: null,
};

function legendKeys(map) {
  return Object.keys(map.tiles.legend || {});
}

function dims(map) {
  const rows = map.tiles.rows || [];
  return { w: rows.reduce((m, r) => Math.max(m, [...r].length), 0), h: rows.length };
}

function charAt(map, c, r) {
  const row = [...(map.tiles.rows[r] || '')];
  return c < row.length ? row[c] : ' ';
}

function setChar(map, c, r, ch) {
  const { w } = dims(map);
  const row = [...(map.tiles.rows[r] || '')];
  while (row.length < Math.max(w, c + 1)) row.push(' ');
  if (row[c] === ch) return false;
  row[c] = ch;
  map.tiles.rows[r] = row.join('');
  return true;
}

function syncSize(map) {
  if (map.size != null) {
    const { w, h: ht } = dims(map);
    map.size = [w, ht];
  }
}

function colourOf(map, ch) {
  if (ch === ' ') return null;
  const st = map.tiles.legend[ch];
  if (!st) return '#d04040';
  if (st.walls === 'all') return '#3a3f47';
  const i = legendKeys(map).indexOf(ch);
  return SWATCH[(i < 0 ? 0 : i) % SWATCH.length];
}

function wallMask(st) {
  if (!st || st.walls == null || st.walls === 'none') return 0;
  if (st.walls === 'all') return 15;
  return (st.walls || []).reduce((m, q) => m | (QBIT[q] || 0), 0);
}

function snapshot(ctx, id) {
  (V.undo[id] = V.undo[id] || []).push(JSON.stringify(ctx.pack.maps[id]));
  if (V.undo[id].length > 200) V.undo[id].shift();
  V.redo[id] = [];
}

function restore(ctx, id, json) {
  const m = ctx.pack.maps[id];
  for (const k of Object.keys(m)) delete m[k];
  Object.assign(m, JSON.parse(json));
}

function undo(ctx, dir) {
  const id = V.map;
  const from = dir < 0 ? V.undo : V.redo;
  const to = dir < 0 ? V.redo : V.undo;
  const s = (from[id] || []).pop();
  if (!s) return;
  (to[id] = to[id] || []).push(JSON.stringify(ctx.pack.maps[id]));
  restore(ctx, id, s);
  V.sel = null;
  ctx.changed(true);
}

function objectsAt(map, c, r) {
  const out = [];
  const inRect = (q) => c >= Math.min(q[0], q[2]) && c <= Math.max(q[0], q[2]) && r >= Math.min(q[1], q[3]) && r <= Math.max(q[1], q[3]);
  (map.npcs || []).forEach((p, i) => { if (p.tile[0] === c && p.tile[1] === r) out.push({ type: 'npcs', i }); });
  (map.props || []).forEach((p, i) => { if (p.tile[0] === c && p.tile[1] === r) out.push({ type: 'props', i }); });
  (map.spawns || []).forEach((p, i) => { if (p.tile[0] === c && p.tile[1] === r) out.push({ type: 'spawns', i }); });
  for (const t of ['exits', 'triggers', 'encounters']) (map[t] || []).forEach((z, i) => { if (inRect(z.rect)) out.push({ type: t, i }); });
  return out;
}

function baseScene(map) {
  const s = map.base && map.base.scene;
  return s && s.kind === 'scene' && s.name ? s.name : null;
}

// --- Drawing ------------------------------------------------------------------------
function draw(ctx, cv) {
  const map = ctx.pack.maps[V.map];
  if (!map || !cv) return;
  const { w, h: ht } = dims(map);
  const cs = V.cell;
  cv.width = Math.max(1, w * cs);
  cv.height = Math.max(1, ht * cs);
  const g = cv.getContext('2d');
  g.clearRect(0, 0, cv.width, cv.height);
  const pv = V.preview[V.map];
  const good = pv && !pv.error && pv.width === w && pv.height === ht;
  const ladder = good ? pv.ladder : null;
  const top = ladder ? Math.max(1, ...ladder.map(Math.abs)) : 1;
  for (let r = 0; r < ht; r++) {
    for (let c = 0; c < w; c++) {
      const ch = charAt(map, c, r);
      const x = c * cs, y = r * cs;
      const col = colourOf(map, ch);
      if (!col) {
        g.fillStyle = (c + r) % 2 ? '#1b1d22' : '#23262c';
        g.fillRect(x, y, cs, cs);
        continue;
      }
      g.fillStyle = col;
      g.fillRect(x, y, cs, cs);
      const st = map.tiles.legend[ch];
      const tier = good ? pv.tier[r * w + c] : (st ? st.height || 0 : 0);
      if (V.heights && ladder && ladder[tier]) {
        // Higher floors lighter.
        g.fillStyle = `rgba(255,255,255,${Math.min(0.45, (Math.abs(ladder[tier]) / top) * 0.45)})`;
        g.fillRect(x, y, cs, cs);
      }
      const mask = good ? pv.walls[r * w + c] : wallMask(st);
      if (mask && mask !== 15) {
        g.fillStyle = 'rgba(20,22,26,0.85)';
        const hh = cs / 2;
        if (mask & 1) g.fillRect(x, y, hh, hh);
        if (mask & 2) g.fillRect(x + hh, y, hh, hh);
        if (mask & 4) g.fillRect(x, y + hh, hh, hh);
        if (mask & 8) g.fillRect(x + hh, y + hh, hh, hh);
      } else if (mask === 15 && st && st.walls !== 'all') {
        g.fillStyle = 'rgba(20,22,26,0.85)';
        g.fillRect(x, y, cs, cs);
      }
      if (V.heights && good && !pv.void[r * w + c] && !pv.flat[r * w + c] && mask !== 15) {
        // A sloped cell: its four corners disagree.
        g.save();
        g.beginPath();
        g.rect(x, y, cs, cs);
        g.clip();
        g.strokeStyle = 'rgba(255,240,180,0.55)';
        g.lineWidth = 1;
        for (let k = -cs; k < cs; k += 5) { g.beginPath(); g.moveTo(x + k, y + cs); g.lineTo(x + k + cs, y); g.stroke(); }
        g.restore();
      }
      if (st && st.wall) {
        // A visible wall: the box or mesh the tile draws.
        g.strokeStyle = 'rgba(200,210,225,0.7)';
        g.lineWidth = 1;
        g.strokeRect(x + 2.5, y + 2.5, cs - 5, cs - 5);
      }
      if (V.heights && cs >= 18 && mask !== 15) {
        g.fillStyle = 'rgba(0,0,0,0.7)';
        g.font = `${Math.round(cs * 0.42)}px monospace`;
        g.fillText(String(tier), x + 2, y + cs * 0.45);
      }
    }
  }
  if (cs >= 8) {
    g.strokeStyle = 'rgba(0,0,0,0.25)';
    g.lineWidth = 1;
    g.beginPath();
    for (let c = 0; c <= w; c++) { g.moveTo(c * cs + 0.5, 0); g.lineTo(c * cs + 0.5, ht * cs); }
    for (let r = 0; r <= ht; r++) { g.moveTo(0, r * cs + 0.5); g.lineTo(w * cs, r * cs + 0.5); }
    g.stroke();
  }
  const zone = (q, stroke, fill, dash) => {
    const x0 = Math.min(q[0], q[2]) * cs, y0 = Math.min(q[1], q[3]) * cs;
    const x1 = (Math.max(q[0], q[2]) + 1) * cs, y1 = (Math.max(q[1], q[3]) + 1) * cs;
    g.fillStyle = fill;
    g.fillRect(x0, y0, x1 - x0, y1 - y0);
    g.setLineDash(dash || []);
    g.strokeStyle = stroke;
    g.lineWidth = 2;
    g.strokeRect(x0 + 1, y0 + 1, x1 - x0 - 2, y1 - y0 - 2);
    g.setLineDash([]);
  };
  const isSel = (type, i) => V.sel && V.sel.type === type && V.sel.i === i;
  (map.encounters || []).forEach((z, i) => zone(z.rect, isSel('encounters', i) ? '#fff' : '#e0605a', 'rgba(224,96,90,0.12)', [4, 3]));
  (map.triggers || []).forEach((z, i) => zone(z.rect, isSel('triggers', i) ? '#fff' : '#e6a23c', 'rgba(230,162,60,0.18)'));
  (map.exits || []).forEach((z, i) => zone(z.rect, isSel('exits', i) ? '#fff' : '#4aa3e8', 'rgba(74,163,232,0.25)'));
  if (good && pv.tops) {
    // Standable prop tops: what action movement stands on.
    g.setLineDash([2, 2]);
    g.strokeStyle = '#f0e6a0';
    g.lineWidth = 1;
    for (const [c0, r0, c1, r1] of pv.tops) g.strokeRect(c0 * cs + 0.5, r0 * cs + 0.5, (c1 - c0) * cs - 1, (r1 - r0) * cs - 1);
    g.setLineDash([]);
  }
  const objs = V.objects[baseScene(map)];
  (map.props || []).forEach((p, i) => {
    const cx = (p.tile[0] + 0.5) * cs + ((p.offset || [0])[0] / 128) * cs;
    const cy = (p.tile[1] + 0.5) * cs + ((p.offset || [0, 0, 0])[2] / 128) * cs;
    const obj = objs && p.mesh && p.mesh.object != null ? objs.objects[p.mesh.object] : null;
    g.strokeStyle = isSel('props', i) ? '#fff' : '#d9c46a';
    g.fillStyle = 'rgba(217,196,106,0.35)';
    g.lineWidth = 2;
    g.beginPath();
    if (obj && obj.footprint.length >= 3) {
      const yaw = (((p.rotation || [0, 0, 0])[1] || 0) * Math.PI) / 180;
      const [cs0, sn] = [Math.cos(yaw), Math.sin(yaw)];
      obj.footprint.forEach(([vx, vz], k) => {
        const rx = vx * cs0 - vz * sn, rz = vx * sn + vz * cs0;
        const px = cx + (rx / 128) * cs, py = cy + (rz / 128) * cs;
        if (k) g.lineTo(px, py); else g.moveTo(px, py);
      });
      g.closePath();
    } else {
      const d = cs * 0.35;
      g.moveTo(cx, cy - d); g.lineTo(cx + d, cy); g.lineTo(cx, cy + d); g.lineTo(cx - d, cy); g.closePath();
    }
    g.fill();
    g.stroke();
  });
  (map.spawns || []).forEach((s, i) => {
    const cx = (s.tile[0] + 0.5) * cs, cy = (s.tile[1] + 0.5) * cs;
    g.fillStyle = i === 0 ? '#3ecf8e' : '#2a9d6f';
    g.strokeStyle = isSel('spawns', i) ? '#fff' : '#0b2a1d';
    g.lineWidth = 2;
    g.beginPath();
    g.arc(cx, cy, cs * 0.32, 0, Math.PI * 2);
    g.fill();
    g.stroke();
    if (cs >= 16) {
      g.fillStyle = '#06231f';
      g.font = `bold ${Math.round(cs * 0.4)}px monospace`;
      g.textAlign = 'center';
      g.fillText(String(s.facing || 0), cx, cy + cs * 0.14);
      g.textAlign = 'left';
    }
  });
  (map.npcs || []).forEach((n, i) => {
    const at = (t) => [(t[0] + 0.5) * cs, (t[1] + 0.5) * cs];
    const [cx, cy] = at(n.tile);
    if ((n.patrol || []).length) {
      g.strokeStyle = 'rgba(186,140,255,0.7)';
      g.setLineDash([4, 3]);
      g.lineWidth = 2;
      g.beginPath();
      g.moveTo(cx, cy);
      for (const t of n.patrol) g.lineTo(...at(t));
      g.closePath();
      g.stroke();
      g.setLineDash([]);
    }
    g.fillStyle = '#9b6cf0';
    g.strokeStyle = isSel('npcs', i) ? '#fff' : '#2a1650';
    g.lineWidth = 2;
    g.beginPath();
    g.rect(cx - cs * 0.3, cy - cs * 0.3, cs * 0.6, cs * 0.6);
    g.fill();
    g.stroke();
    // The facing sector, as a tick toward it (sector 0 faces +Z, down).
    const a = ((n.facing || 0) * Math.PI) / 4;
    g.strokeStyle = '#f4ecff';
    g.beginPath();
    g.moveTo(cx, cy);
    g.lineTo(cx + Math.sin(a) * cs * 0.45, cy + Math.cos(a) * cs * 0.45);
    g.stroke();
  });
  if (V.drag && V.drag.rect) {
    const [c0, r0, c1, r1] = V.drag.rect;
    zone([c0, r0, c1, r1], '#ffffff', 'rgba(255,255,255,0.15)', [3, 3]);
  }
}

// --- The tab ------------------------------------------------------------------------
export function renderMaps(ctx, el) {
  const maps = ctx.pack.maps || (ctx.pack.maps = {});
  const ids = Object.keys(maps);
  const jump = ctx.jumpFile();
  if (jump && jump.startsWith('maps/')) V.map = jump.slice(5, -5);
  if (!ids.includes(V.map)) V.map = ids[0] || null;

  // Map chooser.
  const chooser = h('select', { 'aria-label': 'Map' }, ...ids.map((id) => h('option', { value: id, selected: id === V.map }, id)));
  chooser.addEventListener('change', () => { V.map = chooser.value; V.sel = null; ctx.rerender(); });
  const addBtn = h('button', {
    type: 'button', class: 'wb-mini', onclick: () => {
      const id = freshId('map', ids);
      maps[id] = newMap(ctx);
      V.map = id;
      ctx.changed(true);
    },
  }, '+ New map');
  const head = h('div', { class: 'xw-tools' }, h('span', { class: 'wb-label' }, 'Map'), chooser, addBtn);
  if (V.map) {
    const ren = h('input', { type: 'text', value: V.map, size: 18, 'aria-label': 'Map id (file name)', spellcheck: 'false' });
    ren.addEventListener('change', () => {
      const nv = ren.value.trim();
      if (!nv || nv === V.map || maps[nv]) { ren.value = V.map; return; }
      maps[nv] = maps[V.map];
      delete maps[V.map];
      for (const m of Object.values(maps)) for (const e of m.exits || []) if (e.to && e.to.kind === 'map' && e.to.map === V.map) e.to.map = nv;
      V.map = nv;
      ctx.changed(true);
    });
    head.append(h('span', { class: 'xw-sep' }), h('span', { class: 'xw-hint' }, 'file maps/'), ren, h('span', { class: 'xw-hint' }, '.toml'),
      h('button', { type: 'button', class: 'wb-mini', onclick: () => { delete maps[V.map]; V.map = null; ctx.changed(true); } }, 'Delete map'));
  }
  el.append(h('p', { class: 'xw-note' }, 'maps/<id>.toml: a walkable field scene laid out tile by tile over a retail base scene, whose textures, palettes and geometry pack stay on your disc.'), head);
  if (!V.map) { el.append(h('div', { class: 'wb-empty' }, 'No maps yet: add one.')); return; }
  const map = maps[V.map];
  map.tiles = map.tiles || { origin: [16, 16], rows: [], legend: {} };
  map.tiles.legend = map.tiles.legend || {};
  map.tiles.rows = map.tiles.rows || [];
  for (const k of ['props', 'npcs', 'spawns', 'exits', 'triggers', 'encounters']) map[k] = map[k] || [];
  const path = '';

  const cv = h('canvas', {});
  const redraw = () => draw(ctx, cv);
  // Edits from the side forms redraw the canvas without a re-render.
  const mctx = Object.create(ctx);
  mctx.changed = (structural) => { ctx.changed(structural); if (!structural) { schedulePreview(ctx, redraw); redraw(); } };

  // Settings.
  const settings = h('details', { class: 'xw-box', open: ids.length === 1 && !(map.tiles.rows || []).length ? true : null });
  settings.append(h('summary', { class: 'xw-box-head' }, `Map settings: ${map.name || V.map}`));
  settings.append(grid(mctx, map, [
    { k: 'name', label: 'Name' },
    { k: 'camera', t: 'sel', opts: ['default', 'retail', 'far', 'farther'], label: 'Camera' },
    { k: 'bgm', t: 'obgm', label: 'Music' },
    { k: 'floor_heights', t: 'csvint', label: 'Floor heights', hint: 'tier 0, 1, ... world units up; empty takes the base scene\'s' },
  ], path));
  const base = map.base || (map.base = { scene: { kind: 'scene', name: '' }, geometry: 'none' });
  const bctx = Object.create(mctx);
  bctx.changed = (s) => { mctx.changed(s); loadObjects(ctx, baseScene(map), () => ctx.rerender()); };
  settings.append(grid(bctx, base, [
    { k: 'scene', t: 'scene', label: 'Base scene' },
    { k: 'geometry', t: 'sel', opts: [['none', 'none (map ground only)'], ['keep', 'keep the base scene\'s geometry']], label: 'Base geometry' },
  ], 'base'));
  settings.append(grid(mctx, map.tiles, [{ k: 'origin', t: 'tuple', n: 2, label: 'Origin on the 128x128 field grid', hint: '[column, row]' }], 'tiles'));
  const { w, h: ht } = dims(map);
  const rw = h('input', { type: 'number', value: w, min: 1, max: 120, 'aria-label': 'Width' });
  const rh = h('input', { type: 'number', value: ht, min: 1, max: 120, 'aria-label': 'Height' });
  const resize = h('button', {
    type: 'button', class: 'wb-mini', onclick: () => {
      const nw = Math.max(1, Math.min(120, parseInt(rw.value, 10) || w));
      const nh = Math.max(1, Math.min(120, parseInt(rh.value, 10) || ht));
      snapshot(ctx, V.map);
      const rows = [];
      for (let r = 0; r < nh; r++) {
        const row = [...(map.tiles.rows[r] || '')].slice(0, nw);
        while (row.length < nw) row.push(' ');
        rows.push(row.join(''));
      }
      map.tiles.rows = rows;
      syncSize(map);
      ctx.changed(true);
    },
  }, 'Resize');
  const sizeOn = h('input', { type: 'checkbox' });
  sizeOn.checked = map.size != null;
  sizeOn.addEventListener('change', () => { map.size = sizeOn.checked ? [w, ht] : null; ctx.changed(); });
  settings.append(h('div', { class: 'xw-tools' }, h('span', { class: 'wb-label' }, 'Size'), rw, 'x', rh, resize,
    h('label', { class: 'xw-check xw-field' }, sizeOn, 'write size = [w, h] (checked against the rows)')));
  el.append(settings);

  // Tool bar.
  const tools = h('div', { class: 'xw-tools', role: 'toolbar', 'aria-label': 'Painter tools' });
  for (const [id, label, key] of TOOLS) {
    tools.append(h('button', { type: 'button', class: 'wb-mini' + (V.tool === id ? ' is-on' : ''), title: `${label} (${key})`, onclick: () => { V.tool = id; ctx.rerender(); } }, label));
  }
  const hcb = h('input', { type: 'checkbox' });
  hcb.checked = V.heights;
  hcb.addEventListener('change', () => { V.heights = hcb.checked; redraw(); });
  tools.append(h('span', { class: 'xw-sep' }),
    h('button', { type: 'button', class: 'wb-mini', title: 'Zoom out', onclick: () => zoom(-1) }, '−'),
    h('button', { type: 'button', class: 'wb-mini', title: 'Zoom in', onclick: () => zoom(1) }, '+'),
    h('button', { type: 'button', class: 'wb-mini', title: 'Undo (Ctrl+Z)', disabled: !(V.undo[V.map] || []).length, onclick: () => undo(ctx, -1) }, 'Undo'),
    h('button', { type: 'button', class: 'wb-mini', title: 'Redo (Ctrl+Y)', disabled: !(V.redo[V.map] || []).length, onclick: () => undo(ctx, 1) }, 'Redo'),
    h('span', { class: 'xw-sep' }), h('label', { class: 'xw-check xw-field' }, hcb, 'Heights'));

  const wrap = h('div', { class: 'xw-canvas-wrap', tabindex: '0' }, cv);
  const hover = h('div', { class: 'xw-hover' });
  const note = h('p', { class: 'xw-note' }, 'Heights: the number is each tile\'s floor tier; lighter is higher. The floor reads a tile\'s tier at its top-left corner (low X, low Z) and slopes across the cell to its neighbours\' corners, so a cell is flat only where its four corners agree. Hatched cells slope: along a raised block\'s top and left edges the cell outside it ramps up onto it, and along its bottom and right edges its own last row and column ramp down. Drag with the right or middle button (or hold Space) to pan; Ctrl+wheel zooms.');
  const left = h('div', {}, tools, wrap, hover, note);
  const side = h('div', {});
  el.append(h('div', { class: 'xw-painter' }, left, side));

  renderPalette(ctx, mctx, side, map);
  renderSelection(ctx, mctx, side, map);
  renderDetail(ctx, mctx, side, map);

  function zoom(d) {
    const sizes = [6, 8, 10, 14, 18, 22, 28, 36, 48];
    let i = sizes.indexOf(V.cell);
    if (i < 0) i = 5;
    V.cell = sizes[Math.max(0, Math.min(sizes.length - 1, i + d))];
    redraw();
  }

  // Input.
  const tileOf = (ev) => {
    const r = cv.getBoundingClientRect();
    return [Math.floor((ev.clientX - r.left) / V.cell), Math.floor((ev.clientY - r.top) / V.cell)];
  };
  let space = false;
  wrap.addEventListener('keydown', (ev) => { if (ev.code === 'Space') { space = true; ev.preventDefault(); } });
  wrap.addEventListener('keyup', (ev) => { if (ev.code === 'Space') space = false; });
  cv.addEventListener('contextmenu', (ev) => ev.preventDefault());
  cv.addEventListener('mousedown', (ev) => {
    wrap.focus({ preventScroll: true });
    if (ev.button === 1 || ev.button === 2 || space) {
      V.drag = { pan: true, x: ev.clientX, y: ev.clientY, sl: wrap.scrollLeft, st: wrap.scrollTop };
      wrap.classList.add('is-panning');
      ev.preventDefault();
      return;
    }
    if (ev.button !== 0) return;
    const [c, r] = tileOf(ev);
    const { w: mw, h: mh } = dims(map);
    if (c < 0 || r < 0 || c >= mw || r >= mh) return;
    const t = V.tool;
    if (t === 'paint') {
      snapshot(ctx, V.map);
      V.drag = { paint: true };
      if (setChar(map, c, r, V.brush)) { syncSize(map); mctx.changed(); }
    } else if (t === 'pick') {
      V.brush = charAt(map, c, r);
      ctx.rerender();
    } else if (t === 'rect' || t === 'exit' || t === 'trigger' || t === 'encounter') {
      V.drag = { rect: [c, r, c, r], kind: t };
      redraw();
    } else if (t === 'select') {
      const hit = objectsAt(map, c, r);
      V.sel = hit.length ? hit[0] : null;
      if (V.sel) { snapshot(ctx, V.map); V.drag = { move: true, from: [c, r] }; }
      renderSelection(ctx, mctx, side, map);
      redraw();
    } else if (t === 'spawn') {
      snapshot(ctx, V.map);
      map.spawns.push({ id: freshId('spawn', map.spawns.map((s) => s.id)), tile: [c, r], facing: 0 });
      V.sel = { type: 'spawns', i: map.spawns.length - 1 };
      V.tool = 'select';
      ctx.changed(true);
    } else if (t === 'npc') {
      snapshot(ctx, V.map);
      const scene = baseScene(map) || 'town01';
      const model = V.npcModel && V.npcModel.scene === scene ? V.npcModel : { kind: 'npc', scene, index: 1 };
      map.npcs.push({ id: freshId('npc', ctx.ids('npc')), tile: [c, r], facing: 0, model: { ...model }, line: ctx.ids('line')[0] || '', patrol: [] });
      V.sel = { type: 'npcs', i: map.npcs.length - 1 };
      V.tool = 'select';
      ctx.changed(true);
    } else if (t === 'prop') {
      snapshot(ctx, V.map);
      map.props.push({ id: freshId('prop', map.props.map((p) => p.id)), mesh: { object: V.propObject }, tile: [c, r], offset: [0, 0, 0], rotation: [0, 0, 0], scale: null, solid: false });
      V.sel = { type: 'props', i: map.props.length - 1 };
      ctx.changed(true);
    }
  });
  const onMove = (ev) => {
    if (!cv.isConnected) return;
    const d = V.drag;
    if (d && d.pan) {
      wrap.scrollLeft = d.sl - (ev.clientX - d.x);
      wrap.scrollTop = d.st - (ev.clientY - d.y);
      return;
    }
    const [c, r] = tileOf(ev);
    const { w: mw, h: mh } = dims(map);
    const inside = c >= 0 && r >= 0 && c < mw && r < mh;
    if (inside) hover.textContent = describe(ctx, map, c, r);
    if (!d || !inside) return;
    if (d.paint) {
      if (setChar(map, c, r, V.brush)) { syncSize(map); mctx.changed(); }
    } else if (d.rect) {
      d.rect[2] = c; d.rect[3] = r;
      redraw();
    } else if (d.move && V.sel && (c !== d.from[0] || r !== d.from[1])) {
      const dc = c - d.from[0], dr = r - d.from[1];
      const o = map[V.sel.type][V.sel.i];
      if (o.tile) o.tile = [o.tile[0] + dc, o.tile[1] + dr];
      if (o.patrol) o.patrol = o.patrol.map((t) => [t[0] + dc, t[1] + dr]);
      if (o.rect) o.rect = [o.rect[0] + dc, o.rect[1] + dr, o.rect[2] + dc, o.rect[3] + dr];
      d.from = [c, r];
      mctx.changed();
    }
  };
  const onUp = () => {
    if (!cv.isConnected) return;
    const d = V.drag;
    V.drag = null;
    wrap.classList.remove('is-panning');
    if (!d) return;
    if (d.rect) {
      const [c0, r0, c1, r1] = d.rect;
      const q = [Math.min(c0, c1), Math.min(r0, r1), Math.max(c0, c1), Math.max(r0, r1)];
      snapshot(ctx, V.map);
      if (d.kind === 'rect') {
        for (let r = q[1]; r <= q[3]; r++) for (let c = q[0]; c <= q[2]; c++) setChar(map, c, r, V.brush);
        syncSize(map);
        mctx.changed();
        return;
      }
      const list = map[d.kind + 's'];
      const id = freshId(d.kind, list.map((z) => z.id));
      if (d.kind === 'exit') list.push({ id, rect: q, to: { kind: 'retail', scene: { kind: 'scene', name: baseScene(map) || 'town01' }, facing: 0 } });
      if (d.kind === 'trigger') list.push({ id, rect: q, event: { kind: 'dialog', line: ctx.ids('line')[0] || '' }, once: true });
      if (d.kind === 'encounter') list.push({ id, rect: q, table: '' });
      V.sel = { type: d.kind + 's', i: list.length - 1 };
      ctx.changed(true);
    } else if (d.move || d.paint) {
      ctx.changed(true);
    }
  };
  // Replace the previous render's document listeners.
  if (V.listeners) {
    document.removeEventListener('mousemove', V.listeners.move);
    document.removeEventListener('mouseup', V.listeners.up);
    document.removeEventListener('keydown', V.listeners.key);
  }
  const onKey = (ev) => {
    if (!cv.isConnected || ev.defaultPrevented) return;
    const t = ev.target;
    if (t && (t.tagName === 'INPUT' || t.tagName === 'TEXTAREA' || t.tagName === 'SELECT')) return;
    const k = ev.key.toLowerCase();
    if ((ev.ctrlKey || ev.metaKey) && k === 'z') { ev.preventDefault(); undo(ctx, ev.shiftKey ? 1 : -1); return; }
    if ((ev.ctrlKey || ev.metaKey) && k === 'y') { ev.preventDefault(); undo(ctx, 1); return; }
    if ((k === 'delete' || k === 'backspace') && V.sel) {
      ev.preventDefault();
      snapshot(ctx, V.map);
      map[V.sel.type].splice(V.sel.i, 1);
      V.sel = null;
      ctx.changed(true);
      return;
    }
    if (ev.ctrlKey || ev.metaKey || ev.altKey) return;
    const tool = TOOLS.find((x) => x[2].toLowerCase() === k);
    if (tool) { V.tool = tool[0]; ctx.rerender(); }
  };
  V.listeners = { move: onMove, up: onUp, key: onKey };
  document.addEventListener('mousemove', onMove);
  document.addEventListener('mouseup', onUp);
  document.addEventListener('keydown', onKey);
  wrap.addEventListener('wheel', (ev) => {
    if (!ev.ctrlKey) return;
    ev.preventDefault();
    zoom(ev.deltaY < 0 ? 1 : -1);
  }, { passive: false });
  wrap.addEventListener('scroll', () => { V.scroll[V.map] = [wrap.scrollLeft, wrap.scrollTop]; });

  loadObjects(ctx, baseScene(map), () => { renderSelection(ctx, mctx, side, map); redraw(); });
  if (!V.preview[V.map]) V.preview[V.map] = ctx.mapPreview(map);
  redraw();
  const sc = V.scroll[V.map];
  if (sc) requestAnimationFrame(() => { wrap.scrollLeft = sc[0]; wrap.scrollTop = sc[1]; });
  schedulePreview(ctx, redraw);
}

function newMap(ctx) {
  const scene = (ctx.cat && ctx.cat.scenes && ctx.cat.scenes[0]) ? ctx.cat.scenes[0].name : 'town01';
  return {
    name: 'New map', size: null,
    base: { scene: { kind: 'scene', name: scene }, geometry: 'none' },
    floor_heights: [0, 64],
    tiles: {
      origin: [16, 16],
      rows: ['#########', '#.......#', '#.......#', '#.......#', '#########'],
      legend: { '#': { label: 'wall', walls: 'all', height: 0, ground: null }, '.': { label: 'floor', height: 0, ground: null } },
    },
    props: [], npcs: [], spawns: [{ id: 'start', tile: [4, 2], facing: 0 }], exits: [], triggers: [], encounters: [],
    bgm: null, camera: 'default',
  };
}

function schedulePreview(ctx, redraw) {
  clearTimeout(V.previewTimer);
  V.previewTimer = setTimeout(() => {
    const map = ctx.pack.maps && ctx.pack.maps[V.map];
    if (!map) return;
    V.preview[V.map] = ctx.mapPreview(map);
    redraw();
  }, 140);
}

function loadObjects(ctx, scene, done) {
  if (!scene || V.objects[scene] || !ctx.cat || V.objectsPending === scene) return;
  V.objectsPending = scene;
  // Assembling a scene takes a moment; let the page paint first.
  setTimeout(() => {
    try {
      V.objects[scene] = ctx.sceneObjects(scene);
    } catch (e) {
      V.objects[scene] = { scene, entry: null, objects: [], error: String(e && e.message ? e.message : e) };
    }
    V.objectsPending = null;
    done();
  }, 30);
}

function loadNpcs(ctx, scene, done) {
  if (!scene || V.npcs[scene] || !ctx.cat) return;
  try {
    V.npcs[scene] = ctx.sceneNpcs(scene);
  } catch (e) {
    V.npcs[scene] = { scene, npcs: [], error: String(e && e.message ? e.message : e) };
  }
  done();
}

/** The NPC model picker: the placements of `scene` a pack NPC can borrow,
 * labelled with what each says (shown, not stored). */
function npcModelPicker(ctx, model, onPick) {
  const list = V.npcs[model.scene];
  const wrap = h('div', {});
  wrap.append(field(ctx, model, { k: 'scene', t: 'scenename', label: 'Model scene', hint: 'the map\'s base scene, unless a party member' }, ''));
  if (!list || !list.npcs.length) {
    wrap.append(field(ctx, model, { k: 'index', t: 'int', label: 'Placement', hint: list && list.error ? list.error : (ctx.cat ? 'reading the scene...' : 'load your disc to list NPCs') }, ''));
    return wrap;
  }
  const sel = h('select', { 'aria-label': 'NPC model' }, ...list.npcs.map((p) => h('option', { value: p.index, selected: p.index === model.index },
    `#${p.index} ${p.kind}${p.special ? ' (party/global)' : ''}${p.label ? ' - "' + p.label + '"' : ''}`)));
  sel.addEventListener('change', () => { model.index = parseInt(sel.value, 10); onPick(); });
  wrap.append(h('label', { class: 'xw-field' }, h('span', {}, 'Model (retail placement)'), sel));
  return wrap;
}

function describe(ctx, map, c, r) {
  const ch = charAt(map, c, r);
  const st = map.tiles.legend[ch];
  const pv = V.preview[V.map];
  const { w } = dims(map);
  let s = `[${c}, ${r}] '${ch}'${st && st.label ? ' ' + st.label : ch === ' ' ? ' void' : ''}`;
  if (pv && !pv.error && pv.width === w) {
    const i = r * w + c;
    const k = pv.corners[i];
    s += `  tier ${pv.tier[i]} (${pv.ladder[pv.tier[i]]} up)  walls ${QUARTERS.filter((q) => pv.walls[i] & QBIT[q]).join('+') || 'none'}`;
    s += `  corners ${k.join('/')} ${pv.flat[i] ? 'flat' : 'slope'}`;
  }
  const hits = objectsAt(map, c, r).map((o) => `${o.type.replace(/s$/, '')} ${map[o.type][o.i].id || '#' + o.i}`);
  if (hits.length) s += `  | ${hits.join(', ')}`;
  return s;
}

// --- Legend palette -----------------------------------------------------------------
function renderPalette(ctx, mctx, side, map) {
  const legend = map.tiles.legend;
  const box = h('div', { class: 'xw-box', 'data-path': 'tiles.legend' });
  box.append(h('div', { class: 'xw-box-head' }, 'Legend', h('span', { class: 'xw-sp' }), h('button', {
    type: 'button', class: 'wb-mini', onclick: () => {
      const used = new Set(legendKeys(map));
      const ch = [...SPARE_CHARS].find((x) => !used.has(x));
      if (!ch) return;
      legend[ch] = { label: 'new', height: 0, ground: null };
      V.brush = ch;
      ctx.changed(true);
    },
  }, '+ Entry')));
  const pal = h('div', { class: 'xw-palette' });
  const counts = {};
  for (const row of map.tiles.rows) for (const ch of row) counts[ch] = (counts[ch] || 0) + 1;
  for (const ch of [' ', ...legendKeys(map)]) {
    const st = legend[ch];
    const col = colourOf(map, ch);
    pal.append(h('button', {
      type: 'button', class: 'xw-swatch' + (V.brush === ch ? ' is-active' : ''), onclick: () => { V.brush = ch; if (V.tool !== 'rect') V.tool = 'paint'; ctx.rerender(); },
    }, h('code', {}, ch === ' ' ? '␣' : ch), h('i', { style: col ? `background:${col}` : 'background:repeating-linear-gradient(45deg,#1b1d22 0 4px,#2a2d34 4px 8px)' }),
    h('span', {}, `${st ? st.label || '' : 'void'}${st && st.height ? ` (tier ${st.height})` : ''} - ${counts[ch] || 0}`)));
  }
  box.append(pal);
  const ch = V.brush;
  const st = legend[ch];
  if (st) {
    const p = `tiles.legend.${ch}`;
    const ed = h('div', { 'data-path': p });
    const keyIn = h('input', { type: 'text', value: ch, maxlength: 1, size: 2, 'aria-label': 'Character' });
    keyIn.addEventListener('change', () => {
      const nv = [...keyIn.value][0];
      if (!nv || nv === ch || legend[nv] || nv === ' ') { keyIn.value = ch; return; }
      snapshot(ctx, V.map);
      legend[nv] = st;
      delete legend[ch];
      map.tiles.rows = map.tiles.rows.map((row) => [...row].map((x) => (x === ch ? nv : x)).join(''));
      V.brush = nv;
      ctx.changed(true);
    });
    ed.append(h('div', { class: 'xw-tools' }, h('span', { class: 'wb-label' }, 'Character'), keyIn,
      h('button', {
        type: 'button', class: 'wb-mini', onclick: () => {
          snapshot(ctx, V.map);
          delete legend[ch];
          map.tiles.rows = map.tiles.rows.map((row) => [...row].map((x) => (x === ch ? ' ' : x)).join(''));
          V.brush = ' ';
          ctx.changed(true);
        },
      }, 'Delete (tiles become void)')));
    const lctx = Object.create(mctx);
    lctx.changed = (s) => mctx.changed(s);
    ed.append(grid(lctx, st, [{ k: 'label', label: 'Label' }], 'tiles.legend.' + ch));
    // Walls: none / all / some quarters.
    const mask = wallMask(st);
    const q = h('div', { class: 'xw-quarters', 'aria-label': 'Wall quarters' });
    for (const name of QUARTERS) {
      const cb = h('input', { type: 'checkbox' });
      cb.checked = !!(mask & QBIT[name]);
      cb.addEventListener('change', () => {
        const on = QUARTERS.filter((n) => (n === name ? cb.checked : !!(wallMask(st) & QBIT[n])));
        if (on.length === 0) delete st.walls;
        else if (on.length === 4) st.walls = 'all';
        else st.walls = on;
        ctx.changed(true);
      });
      q.append(h('label', { title: name }, cb, h('span', {}, name)));
    }
    ed.append(h('label', { class: 'xw-field', 'data-path': `${p}.walls` }, h('span', {}, 'Wall quarters', h('span', { class: 'xw-hint' }, ' a tile is four 64x64 sub-cells')), q));
    const ladder = (V.preview[V.map] && V.preview[V.map].ladder) || map.floor_heights || [];
    ed.append(grid(lctx, st, [{ k: 'height', t: 'sel', num: true, opts: Array.from({ length: 16 }, (_, i) => [String(i), `tier ${i}${ladder[i] != null ? ` (${ladder[i]} up)` : ''}`]), label: 'Floor tier' }], p));
    // Ground.
    const g = st.ground;
    const kind = h('select', {}, ...['', 'texture', 'base_cell', 'mesh'].map((k) => h('option', { value: k, selected: (g ? g.kind : '') === k }, k || 'nothing')));
    kind.addEventListener('change', () => {
      const k = kind.value;
      st.ground = k === 'texture' ? { kind: k, tpage: 0, clut: 0, tile: 0 } : k === 'base_cell' ? { kind: k, cell: [0, 0] } : k === 'mesh' ? { kind: k, index: 0 } : null;
      ctx.changed(true);
    });
    ed.append(h('label', { class: 'xw-field', 'data-path': `${p}.ground` }, h('span', {}, 'Ground'), kind));
    if (g && g.kind === 'texture') ed.append(grid(lctx, g, [{ k: 'tpage', t: 'int', label: 'Texture page' }, { k: 'clut', t: 'int', label: 'Palette (CLUT word)' }, { k: 'tile', t: 'int', label: 'Atlas cell 0..63' }], `${p}.ground`));
    if (g && g.kind === 'base_cell') ed.append(grid(lctx, g, [{ k: 'cell', t: 'tuple', n: 2, label: 'Base scene cell', hint: '[column, row] on its field grid' }], `${p}.ground`));
    if (g && g.kind === 'mesh') ed.append(meshPicker(lctx, g, 'index', `${p}.ground`, map));
    // Visible wall: a generated box or a base-scene mesh standing on the tile.
    const wl = st.wall;
    const wkind = h('select', {}, ...[['', 'nothing'], ['box', 'a textured box'], ['mesh', 'a base-scene mesh']].map(([k, label]) => h('option', { value: k, selected: (wl ? wl.kind : '') === k }, label)));
    wkind.addEventListener('change', () => {
      const k = wkind.value;
      st.wall = k === 'box' ? { kind: k, height: 128, tpage: 0, clut: 0, tile: 0 } : k === 'mesh' ? { kind: k, index: 0 } : null;
      ctx.changed(true);
    });
    ed.append(h('label', { class: 'xw-field', 'data-path': `${p}.wall` }, h('span', {}, 'Visible wall', h('span', { class: 'xw-hint' }, ' draws only; collision is the quarters above')), wkind));
    if (wl && wl.kind === 'box') ed.append(grid(lctx, wl, [{ k: 'height', t: 'int', label: 'Height (world units)' }, { k: 'tpage', t: 'int', label: 'Texture page' }, { k: 'clut', t: 'int', label: 'Palette (CLUT word)' }, { k: 'tile', t: 'int', label: 'Atlas cell 0..63' }], `${p}.wall`));
    if (wl && wl.kind === 'mesh') ed.append(meshPicker(lctx, wl, 'index', `${p}.wall`, map));
    box.append(ed);
  }
  side.append(box);
}

function meshPicker(ctx, owner, key, path, map) {
  const objs = V.objects[baseScene(map)];
  if (!objs || !objs.objects.length) {
    return field(ctx, owner, { k: key, t: 'int', label: 'Mesh', hint: objs && objs.error ? objs.error : (ctx.cat ? 'reading the base scene...' : 'load your disc to list meshes') }, path);
  }
  return field(ctx, owner, {
    k: key, t: 'sel', num: true, label: 'Mesh (base geometry pack)',
    opts: objs.objects.map((o) => [String(o.index), `#${o.index} - ${o.prims} prims, ${o.size[0]}x${o.size[2]}${o.placed ? `, placed ${o.placed}x` : ''}`]),
  }, path);
}

// --- The selected object's form ------------------------------------------------------
function renderSelection(ctx, mctx, side, map) {
  const old = side.querySelector('.xw-selbox');
  if (old) old.remove();
  const box = h('div', { class: 'xw-box xw-selbox' });
  if (V.tool === 'prop') {
    const o = { object: V.propObject };
    const pctx = Object.create(mctx);
    pctx.changed = () => { V.propObject = o.object; };
    box.append(h('div', { class: 'xw-box-head' }, 'New prop'), meshPicker(pctx, o, 'object', '', map), h('p', { class: 'xw-note' }, 'Click a tile to place the mesh there.'));
  }
  if (V.tool === 'npc') {
    const scene = baseScene(map) || 'town01';
    if (!V.npcModel || V.npcModel.scene !== scene) V.npcModel = { kind: 'npc', scene, index: 1 };
    loadNpcs(ctx, scene, () => {});
    const nctx = Object.create(mctx);
    nctx.changed = () => {};
    box.append(h('div', { class: 'xw-box-head' }, 'New NPC'), npcModelPicker(nctx, V.npcModel, () => {}), h('p', { class: 'xw-note' }, 'Click a tile to stand the NPC there.'));
  }
  const sel = V.sel && map[V.sel.type] && map[V.sel.type][V.sel.i] ? V.sel : null;
  if (!sel) {
    if (V.tool !== 'prop' && V.tool !== 'npc') box.append(h('div', { class: 'wb-empty' }, 'Select an object (Select tool) or place one.'));
    side.append(box);
    return;
  }
  const o = map[sel.type][sel.i];
  const p = `${sel.type}[${sel.i}]`;
  box.setAttribute('data-path', p);
  box.append(h('div', { class: 'xw-box-head' }, `${sel.type.replace(/s$/, '')} ${o.id || ''}`, h('span', { class: 'xw-sp' }),
    h('button', { type: 'button', class: 'wb-mini', onclick: () => { snapshot(ctx, V.map); map[sel.type].splice(sel.i, 1); V.sel = null; ctx.changed(true); } }, 'Delete')));
  const rect = { k: 'rect', t: 'tuple', n: 4, label: 'Tiles', hint: '[col0, row0, col1, row1]' };
  if (sel.type === 'spawns') {
    box.append(grid(mctx, o, [{ k: 'id', label: 'Id' }, { k: 'tile', t: 'tuple', n: 2, label: 'Tile' },
      { k: 'facing', t: 'sel', num: true, opts: Array.from({ length: 8 }, (_, i) => [String(i), `sector ${i} (${i * 45} deg)`]), label: 'Facing' }], p));
    if (sel.i === 0) box.append(h('p', { class: 'xw-note' }, 'The first spawn is where the map is entered by default.'));
  } else if (sel.type === 'exits') {
    box.append(grid(mctx, o, [{ k: 'id', label: 'Id' }, rect], p));
    const to = o.to;
    const kind = h('select', {}, h('option', { value: 'retail', selected: to.kind === 'retail' }, 'a retail scene'), h('option', { value: 'map', selected: to.kind === 'map' }, 'another map of this pack'));
    kind.addEventListener('change', () => {
      o.to = kind.value === 'map' ? { kind: 'map', map: Object.keys(ctx.pack.maps).find((k) => k !== V.map) || V.map, spawn: '' } : { kind: 'retail', scene: { kind: 'scene', name: 'town01' }, facing: 0 };
      ctx.changed(true);
    });
    box.append(h('label', { class: 'xw-field', 'data-path': `${p}.to` }, h('span', {}, 'Leads to'), kind));
    if (to.kind === 'retail') {
      box.append(grid(mctx, to, [{ k: 'scene', t: 'scene', label: 'Scene' }, { k: 'tile', t: 'tuple', n: 2, label: 'Arrival tile', hint: 'its field grid; untouched = the scene\'s own door' }, { k: 'facing', t: 'int', label: 'Facing sector' }], `${p}.to`));
    } else {
      box.append(grid(mctx, to, [{ k: 'map', t: 'sel', opts: Object.keys(ctx.pack.maps), label: 'Map' }, { k: 'spawn', label: 'Spawn', ph: 'first' }], `${p}.to`));
    }
    box.append(grid(mctx, o, [{ k: 'locked_line', t: 'oref', kind: 'line', label: 'Said while shut', hint: 'a sealed stair saying why' }], p));
    box.append(gateBox(mctx, o, 'requires', p, 'Open when'));
  } else if (sel.type === 'triggers') {
    box.append(grid(mctx, o, [{ k: 'id', label: 'Id' }, rect, { k: 'once', t: 'bool', label: 'Once per visit' }], p));
    const ev = o.event;
    const EVENTS = {
      battle: [{ k: 'formation', t: 'ref', kind: 'formation', label: 'Formation' }],
      boss: [{ k: 'boss', t: 'ref', kind: 'boss', label: 'Boss' }],
      dialog: [{ k: 'line', t: 'ref', kind: 'line', label: 'Line' }],
      talk: [{ k: 'npc', label: 'NPC id', hint: 'a quest giver / talk objective' }],
      start_quest: [{ k: 'quest', t: 'ref', kind: 'quest', label: 'Quest' }],
      set_flag: [{ k: 'flag', label: 'Pack flag' }],
      counter: [{ k: 'counter', label: 'Pack counter' }, { k: 'add', t: 'int', label: 'Add' }],
      card_dealer: [{ k: 'dealer', label: 'Card dealer', hint: 'a cards.toml dealer id; empty = the first' }],
      shop: [{ k: 'shop', t: 'ref', kind: 'shop', label: 'Item shop', hint: 'an items.toml shop' }],
      challenge: [{ k: 'challenge', t: 'ref', kind: 'challenge', label: 'Challenge' }, { k: 'start', t: 'bool', label: 'Start its battle' }],
      mode: [{ k: 'mode', t: 'ref', kind: 'mode', label: 'Mode instance' }],
      rest: [{ k: 'cost', t: 'int', label: 'Gold cost' }, { k: 'save', t: 'bool', label: 'Then offer a save' }],
    };
    const kind = h('select', {}, ...Object.keys(EVENTS).map((k) => h('option', { value: k, selected: ev.kind === k }, k.replace('_', ' '))));
    kind.addEventListener('change', () => {
      const k = kind.value;
      const first = (kd) => ctx.ids(kd)[0] || '';
      o.event = { kind: k };
      for (const f of EVENTS[k]) {
        o.event[f.k] = f.t === 'ref' ? first(f.kind) : f.t === 'bool' ? false : f.t === 'int' ? (f.k === 'cost' ? 0 : 1) : (k === 'talk' ? first('npc') : '');
      }
      ctx.changed(true);
    });
    box.append(h('label', { class: 'xw-field', 'data-path': `${p}.event` }, h('span', {}, 'Raises'), kind));
    box.append(grid(mctx, ev, EVENTS[ev.kind] || [], `${p}.event`));
    box.append(gateBox(mctx, o, 'requires', p, 'Fires when'));
  } else if (sel.type === 'encounters') {
    box.append(grid(mctx, o, [{ k: 'id', label: 'Id' }, rect, { k: 'table', label: 'Encounter table' }], p));
  } else if (sel.type === 'props') {
    box.append(grid(mctx, o, [{ k: 'id', label: 'Id' }, { k: 'tile', t: 'tuple', n: 2, label: 'Tile' }], p));
    box.append(propMeshEditor(ctx, mctx, o, p, (m, k) => meshPicker(mctx, m, k, `${p}.mesh`, map)));
    box.append(grid(mctx, o, [
      { k: 'offset', t: 'tuple', n: 3, label: 'Offset', hint: 'x y z, y up' },
      { k: 'rotation', t: 'tuple', n: 3, float: true, label: 'Rotation (deg)', hint: 'y is the yaw' },
      { k: 'scale', t: 'ofloat', label: 'Scale' },
      { k: 'solid', t: 'bool', label: 'Solid (closes its tile)' },
      { k: 'top', t: 'oint', label: 'Standable top', hint: 'world units up; action movement stands on it' },
      { k: 'footprint', t: 'tuple', n: 2, label: 'Top footprint', hint: '[x, z] world units; untouched = the tile' },
    ], p));
    box.append(propDetail(mctx, o, p));
  } else if (sel.type === 'npcs') {
    box.append(grid(mctx, o, [{ k: 'id', label: 'Id', hint: 'the quest talk key' }, { k: 'label', label: 'Label' }, { k: 'tile', t: 'tuple', n: 2, label: 'Tile' },
      { k: 'facing', t: 'sel', num: true, opts: Array.from({ length: 8 }, (_, i) => [String(i), `sector ${i} (${i * 45} deg)`]), label: 'Facing' }], p));
    o.model = o.model || { kind: 'npc', scene: baseScene(map) || 'town01', index: 1 };
    loadNpcs(ctx, o.model.scene, () => renderSelection(ctx, mctx, side, map));
    const nctx = Object.create(mctx);
    nctx.changed = (s) => { loadNpcs(ctx, o.model.scene, () => {}); mctx.changed(s); };
    box.append(npcModelPicker(nctx, o.model, () => mctx.changed(true)));
    box.append(grid(mctx, o, [
      { k: 'clip', t: 'oint', label: 'Idle clip', hint: 'ANM record + 1; empty keeps the placement\'s' },
      { k: 'walk_clip', t: 'oint', label: 'Walk clip', hint: 'played on patrol legs' },
      { k: 'line', t: 'oref', kind: 'line', label: 'Says (dialog line)' },
      { k: 'shop', t: 'otext', label: 'Shop', hint: 'an items.toml shop or a cards.toml dealer id; talking opens its counter' },
      { k: 'line_after', t: 'oref', kind: 'line', label: 'Says instead, once "after" holds' },
    ], p));
    box.append(gateBox(mctx, o, 'requires', p, 'Talks when'));
    box.append(gateBox(mctx, o, 'after', p, 'After (switches to the second line)'));
    const pat = h('input', { type: 'text', value: (o.patrol || []).map((t) => t.join(',')).join(' '), placeholder: 'c,r c,r ...', spellcheck: 'false', 'aria-label': 'Patrol waypoints' });
    pat.addEventListener('change', () => {
      o.patrol = pat.value.trim().split(/\s+/).filter(Boolean).map((t) => t.split(',').map((x) => parseInt(x, 10) || 0).slice(0, 2)).filter((t) => t.length === 2);
      mctx.changed(true);
    });
    box.append(h('label', { class: 'xw-field', 'data-path': `${p}.patrol` }, h('span', {}, 'Patrol', h('span', { class: 'xw-hint' }, ' tiles it walks to and back from, in order')), pat));
    box.append(h('p', { class: 'xw-note' }, 'Talking to the NPC says its line and counts as talking to its id: a quest whose giver or talk objective names that id advances.'));
  }
  side.append(box);
}
