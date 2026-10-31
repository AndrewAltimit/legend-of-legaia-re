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

import { h, field, grid, freshId, gateBox, listEd } from './expansion-workbench-forms.js';
import { propMeshEditor, propDetail, renderDetail, entrancesEd } from './expansion-workbench-detail.js';
import { TERRAIN_TOOL, drawTerrain, renderTerrain, sculptDown, sculptUp } from './expansion-workbench-terrain.js';
import { renderMapAudio } from './expansion-workbench-audio.js';
import { nudge } from './expansion-workbench-layout.js';
import { walkThrough, walkOnDialogs } from './expansion-workbench-hints.js';
import { mountMapEditor, editorChanged, select as editSelect, selection as editSelection, objectsIn, deleteObjects, duplicateObjects, drawSelection, clearSelection, layoutRow } from './expansion-workbench-gizmos.js';

const QUARTERS = ['tl', 'tr', 'bl', 'br'];
const QBIT = { tl: 1, tr: 2, bl: 4, br: 8 };
const SPARE_CHARS = '#.,^=~*+o%&@:;abcdefghijklmnopqrstuvwxyz0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ';
const SWATCH = ['#6b8f71', '#8a9a5b', '#b08d57', '#7d8fa6', '#a37a74', '#6f9fa0', '#9c8bb0', '#b5a26e', '#7fa86b', '#a88a9c'];
// The arrow keys' nudge, in painter columns and rows (row 0 at the top).
const ARROWS = { ArrowLeft: [-1, 0], ArrowRight: [1, 0], ArrowUp: [0, -1], ArrowDown: [0, 1] };
const TOOLS = [
  ['paint', 'Paint', 'B'], ['rect', 'Fill rect', 'R'], ['pick', 'Pick', 'I'], ['select', 'Select / move', 'V'],
  ['spawn', 'Spawn', 'S'], ['exit', 'Exit', 'E'], ['trigger', 'Trigger', 'T'], ['encounter', 'Encounters', 'N'], ['prop', 'Prop', 'P'],
  ['npc', 'NPC', 'C'], TERRAIN_TOOL,
];

// View state survives the tab re-rendering on every structural edit.
const V = {
  map: null, tool: 'paint', brush: '.', cell: 22, heights: true, walk: true, sel: null,
  scroll: {}, preview: {}, previewTimer: null, objects: {}, objectsPending: null, pick: null,
  propObject: 0, drag: null, npcs: {}, npcModel: null,
  // The engine's collision reading per map, from the 3D preview's builds.
  collide: false, blocked: {},
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

// Every edit, a stroke included, is a step of the page's own history.
function undo(ctx, dir) {
  V.sel = null;
  clearSelection();
  if (dir < 0) ctx.undo(); else ctx.redo();
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
  // Decorations and decals as the map bakes them (from the preview's
  // expansion of every decor / scatter / edges / decals entry).
  if (good && pv.decor && pv.decor.length) {
    for (const [dx, dz, kind] of pv.decor) {
      g.fillStyle = kind ? 'rgba(150,200,255,0.55)' : 'rgba(120,220,140,0.75)';
      const r = Math.max(1.5, cs * (kind ? 0.12 : 0.08));
      g.beginPath();
      g.arc(dx * cs, dz * cs, r, 0, Math.PI * 2);
      g.fill();
    }
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
  if (good) drawTerrain(g, map, pv, cs);
  if (V.drag && V.drag.rect) {
    const [c0, r0, c1, r1] = V.drag.rect;
    zone([c0, r0, c1, r1], '#ffffff', 'rgba(255,255,255,0.15)', [3, 3]);
  }
  // A box select being dragged, and a selection of several objects.
  if (V.drag && V.drag.box) {
    const [c0, r0, c1, r1] = V.drag.box;
    zone([c0, r0, c1, r1], '#ffffff', 'rgba(255,255,255,0.08)', [2, 2]);
  }
  if (V.collide) drawCollision(g, w, ht, cs);
  drawSelection(g, map, cs);
  if (V.walk) drawWalkThrough(g, map, cs);
  // The tile last clicked here or in the 3D preview, with nothing selected.
  if (!V.sel && V.cursor && V.cursor.map === V.map) {
    const [c, r] = V.cursor.tile;
    g.strokeStyle = '#ffffff';
    g.lineWidth = 2;
    g.setLineDash([3, 2]);
    g.strokeRect(c * cs + 1, r * cs + 1, cs - 2, cs - 2);
    g.setLineDash([]);
  }
}

// --- The tab ------------------------------------------------------------------------
/**
 * Arm a Pick from another tab (a podium's tile on the Modes tab): select map
 * `mapId` and make the next click on its canvas call `req.done([c, r])`;
 * `req.cancel()` runs on Cancel / Esc. The caller switches to the Maps tab.
 */
/** The map the tab has open (null before it first draws). */
export function openMapId() { return V.map || null; }

export function armPick(mapId, req) {
  V.map = mapId;
  V.sel = null;
  V.pick = req;
}

export function renderMaps(ctx, el) {
  const cancelPick = () => { const req = V.pick; V.pick = null; if (req && req.cancel) req.cancel(); else ctx.rerender(); };
  const maps = ctx.pack.maps || (ctx.pack.maps = {});
  const ids = Object.keys(maps);
  const jump = ctx.jumpFile();
  if (jump && jump.startsWith('maps/')) {
    V.map = jump.slice(5, -5);
    // A jump to an object's field selects the object; to a legend entry's, the entry.
    const f = ctx.jumpField() || '';
    const o = f.match(/^(props|npcs|spawns|exits|triggers|encounters)\[(\d+)\]/);
    if (o) V.sel = { type: o[1], i: parseInt(o[2], 10) };
    const l = f.match(/^tiles\.legend\.(.)/u);
    if (l) V.brush = l[1];
  }
  if (!ids.includes(V.map)) V.map = ids[0] || null;

  // Map chooser.
  const chooser = h('select', { 'aria-label': 'Map' }, ...ids.map((id) => h('option', { value: id, selected: id === V.map }, id)));
  chooser.addEventListener('change', () => { V.map = chooser.value; V.sel = null; clearSelection(); ctx.rerender(); });
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
      // With every reference (exits, cutscenes, modes, objectives, engine
      // flags), as one undo step: the reference model's rename.
      if (ctx.renameApply) {
        const res = ctx.renameApply('map', V.map, nv);
        if (!res.ok) { ren.value = V.map; ren.title = res.error.message; return; }
        V.map = nv;
        ctx.replacePack(res.pack, `Renamed map to ${nv}: ${res.changes.length} field(s).`);
        return;
      }
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
  // The 3D preview (expansion-workbench-preview3d.js) follows the painter's
  // selection: the selected object's tile or zone, else the last tile clicked.
  const pv = ctx.preview;
  const selShape = () => {
    const o = V.sel && map[V.sel.type] && map[V.sel.type][V.sel.i];
    if (o && o.rect) return { rect: o.rect };
    if (o && o.tile) return { tile: o.tile };
    return V.cursor && V.cursor.map === V.map ? { tile: V.cursor.tile } : null;
  };
  // It also takes each map_preview the painter computes: the tile grid and a
  // draft of the edit, drawn at once while the rebuild runs.
  const redraw = () => {
    draw(ctx, cv);
    if (pv) { pv.select(selShape()); if (pv.setMapPreview) pv.setMapPreview(map, V.preview[V.map]); }
    editorChanged();
  };
  // Edits from the side forms redraw the canvas without a re-render.
  const mctx = Object.create(ctx);
  mctx.changed = (structural) => { ctx.changed(structural); if (!structural) { schedulePreview(ctx, redraw); redraw(); } };
  // The side boxes' undo step (the terrain's heightmap import).
  mctx.snapshot = () => { if (ctx.commit) ctx.commit(); };

  // Settings.
  const settings = h('details', { class: 'xw-box', open: ids.length === 1 && !(map.tiles.rows || []).length ? true : null });
  settings.append(h('summary', { class: 'xw-box-head' }, `Map settings: ${map.name || V.map}`));
  settings.append(grid(mctx, map, [
    { k: 'name', label: 'Name' },
    { k: 'camera', t: 'sel', opts: ['default', 'retail', 'far', 'farther'], label: 'Camera' },
    { k: 'bgm', t: 'obgm', label: 'Music', hint: 'the Sound box below may name a pack track and zones' },
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
  // Arrival cutscenes (cutscenes.toml): the first unseen one whose gate holds plays.
  listEd(ctx, settings, map, 'enter_cutscenes', path, {
    title: 'Arrival cutscenes',
    hint: 'the first unseen one whose gate holds plays on arrival',
    add: 'Arrival cutscene',
    make: () => ({ cutscene: ctx.ids('cutscene')[0] || '', requires: {}, repeat: false }),
    label: (e) => e.cutscene || '(none)',
    render: (b, e, p2) => {
      b.append(grid(ctx, e, [
        { k: 'cutscene', t: 'ref', kind: 'cutscene', label: 'Cutscene' },
        { k: 'repeat', t: 'bool', label: 'Every arrival, not only until seen' },
      ], p2));
      b.append(gateBox(ctx, e, 'requires', p2, 'Plays when'));
    },
  });
  entrancesEd(ctx, settings, map);
  const { w, h: ht } = dims(map);
  const rw = h('input', { type: 'number', value: w, min: 1, max: 120, 'aria-label': 'Width' });
  const rh = h('input', { type: 'number', value: ht, min: 1, max: 120, 'aria-label': 'Height' });
  const resize = h('button', {
    type: 'button', class: 'wb-mini', onclick: () => {
      const nw = Math.max(1, Math.min(120, parseInt(rw.value, 10) || w));
      const nh = Math.max(1, Math.min(120, parseInt(rh.value, 10) || ht));
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
    h('button', { type: 'button', class: 'wb-mini', title: 'Undo (Ctrl+Z)', disabled: !ctx.canUndo(), onclick: () => undo(ctx, -1) }, 'Undo'),
    h('button', { type: 'button', class: 'wb-mini', title: 'Redo (Ctrl+Y)', disabled: !ctx.canRedo(), onclick: () => undo(ctx, 1) }, 'Redo'),
    h('span', { class: 'xw-sep' }), h('label', { class: 'xw-check xw-field' }, hcb, 'Heights'),
    walkToggle(redraw), collisionToggle(() => redraw(), pv));

  const wrap = h('div', { class: 'xw-canvas-wrap', tabindex: '0' }, cv);
  if (V.refocus) {
    V.refocus = false;
    setTimeout(() => { if (wrap.isConnected) wrap.focus({ preventScroll: true }); }, 0);
  }
  const hover = h('div', { class: 'xw-hover' });
  const note = h('p', { class: 'xw-note' }, 'Heights: the number is each tile\'s floor tier; lighter is higher. The floor reads a tile\'s tier at its top-left corner (low X, low Z) and slopes across the cell to its neighbours\' corners, so a cell is flat only where its four corners agree. Hatched cells slope: along a raised block\'s top and left edges the cell outside it ramps up onto it, and along its bottom and right edges its own last row and column ramp down. Drag with the right or middle button (or hold Space) to pan; Ctrl+wheel zooms.');
  const pickBar = V.pick ? h('div', { class: 'xw-pickbar', role: 'status' }, V.pick.prompt || (V.pick.rect ? 'Drag a rectangle on the map to fill the field.' : 'Click a tile on the map to fill the field.'),
    h('button', { type: 'button', class: 'wb-mini', onclick: cancelPick }, 'Cancel (Esc)')) : null;
  const left = h('div', {}, tools, pickBar, wrap, hover, note);
  const side = h('div', {});
  el.append(h('div', { class: 'xw-painter' }, left, side));
  // The detail layer's list forms are wide: they sit under the painter.
  const below = h('div', { class: 'xw-below' });
  el.append(below);

  renderPalette(ctx, mctx, side, map);
  renderSelection(ctx, mctx, side, map);
  renderDetail(ctx, mctx, below, map, {
    meshEditor: (owner, p) => propMeshEditor(ctx, mctx, owner, p, (m, k) => meshPicker(mctx, m, k, `${p}.mesh`, map)),
    cells: () => { const o = V.objects[baseScene(map)]; return o && !o.error ? (o.cells || []) : null; },
    // A detail form's Pick: the next click (a drag for a rectangle) on the canvas.
    pickTile: (req) => {
      V.pick = req;
      ctx.rerender();
      const w2 = document.querySelector('.xw-canvas-wrap');
      if (w2) w2.scrollIntoView({ block: 'nearest' });
    },
  });
  renderTerrain(ctx, mctx, side, map);
  renderMapAudio(ctx, mctx, side, map);

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
    V.cursor = { map: V.map, tile: [c, r] };
    if (V.pick) {
      if (V.pick.rect) { V.drag = { rect: [c, r, c, r], pick: V.pick }; redraw(); return; }
      const req = V.pick;
      V.pick = null;
      req.done([c, r]);
      return;
    }
    if (pv) pv.hold(true);
    const t = V.tool;
    if (t === 'paint') {
      V.drag = { paint: true };
      if (setChar(map, c, r, V.brush)) { syncSize(map); mctx.changed(); }
    } else if (t === 'sculpt') {
      const b = cv.getBoundingClientRect();
      V.drag = sculptDown(map, (ev.clientX - b.left) / V.cell, (ev.clientY - b.top) / V.cell);
    } else if (t === 'pick') {
      V.brush = charAt(map, c, r);
      ctx.rerender();
    } else if (t === 'rect' || t === 'exit' || t === 'trigger' || t === 'encounter') {
      V.drag = { rect: [c, r, c, r], kind: t };
      redraw();
    } else if (t === 'select') {
      const hit = objectsAt(map, c, r);
      if (ev.shiftKey || ev.ctrlKey || ev.metaKey) {
        // Shift adds to (or takes from) the selection; on empty ground a
        // drag adds a box.
        if (hit.length) {
          const cur = editSelection();
          const k = cur.findIndex((x) => x.type === hit[0].type && x.i === hit[0].i);
          if (k >= 0) cur.splice(k, 1); else cur.push(hit[0]);
          editSelect(cur);
          V.sel = cur[0] || null;
        } else {
          V.drag = { box: [c, r, c, r], add: true };
        }
      } else if (hit.length) {
        const cur = editSelection();
        const inSel = cur.some((x) => x.type === hit[0].type && x.i === hit[0].i);
        V.sel = hit[0];
        if (!inSel) editSelect([hit[0]]);
        V.drag = { move: true, from: [c, r], group: inSel && cur.length > 1 ? cur : null };
      } else {
        // A drag on empty ground selects what its box holds.
        V.sel = null;
        editSelect([]);
        V.drag = { box: [c, r, c, r] };
      }
      renderSelection(ctx, mctx, side, map);
      redraw();
    } else if (t === 'spawn') {
      map.spawns.push({ id: freshId('spawn', map.spawns.map((s) => s.id)), tile: [c, r], facing: 0 });
      V.sel = { type: 'spawns', i: map.spawns.length - 1 };
      V.tool = 'select';
      ctx.changed(true);
    } else if (t === 'npc') {
      const scene = baseScene(map) || 'town01';
      const model = V.npcModel && V.npcModel.scene === scene ? V.npcModel : { kind: 'npc', scene, index: 1 };
      map.npcs.push({ id: freshId('npc', ctx.ids('npc')), tile: [c, r], facing: 0, model: { ...model }, line: ctx.ids('line')[0] || '', patrol: [] });
      V.sel = { type: 'npcs', i: map.npcs.length - 1 };
      V.tool = 'select';
      ctx.changed(true);
    } else if (t === 'prop') {
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
    } else if (d.sculpt) {
      const b = cv.getBoundingClientRect();
      d.to = [(ev.clientX - b.left) / V.cell, (ev.clientY - b.top) / V.cell];
    } else if (d.rect) {
      d.rect[2] = c; d.rect[3] = r;
      redraw();
    } else if (d.box) {
      d.box[2] = c; d.box[3] = r;
      redraw();
    } else if (d.move && V.sel && (c !== d.from[0] || r !== d.from[1])) {
      const dc = c - d.from[0], dr = r - d.from[1];
      for (const s of d.group || [V.sel]) {
        const o = s.type === 'lamps' ? ((map.lighting || {}).lamps || [])[s.i] : (map[s.type] || [])[s.i];
        if (!o) continue;
        if (o.tile) o.tile = [o.tile[0] + dc, o.tile[1] + dr];
        if (o.patrol) o.patrol = o.patrol.map((t) => [t[0] + dc, t[1] + dr]);
        if (o.rect) o.rect = [o.rect[0] + dc, o.rect[1] + dr, o.rect[2] + dc, o.rect[3] + dr];
      }
      d.from = [c, r];
      d.moved = true;
      mctx.changed();
    }
  };
  const onUp = () => {
    if (!cv.isConnected) return;
    if (pv) pv.hold(false);
    const d = V.drag;
    V.drag = null;
    wrap.classList.remove('is-panning');
    if (!d) return;
    if (d.sculpt) {
      if (sculptUp(map, d)) ctx.changed(true);
      return;
    }
    if (d.box) {
      const found = objectsIn(map, d.box);
      const list = d.add ? editSelection().concat(found.filter((f) => !editSelection().some((x) => x.type === f.type && x.i === f.i))) : found;
      editSelect(list);
      V.sel = list[0] || null;
      renderSelection(ctx, mctx, side, map);
      redraw();
      return;
    }
    if (d.rect) {
      const [c0, r0, c1, r1] = d.rect;
      const q = [Math.min(c0, c1), Math.min(r0, r1), Math.max(c0, c1), Math.max(r0, r1)];
      if (d.pick) { V.pick = null; d.pick.done(q); return; }
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
    } else if (d.paint || (d.move && d.moved)) {
      // The tab draws again; the canvas takes the keys back (the arrows'
      // nudge, Delete). A click that moved nothing changes nothing.
      V.refocus = true;
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
    if (k === 'escape' && V.pick) { cancelPick(); return; }
    if ((k === 'delete' || k === 'backspace') && (V.sel || editSelection().length)) {
      ev.preventDefault();
      const many = editSelection();
      if (many.length) deleteObjects(map, many); else map[V.sel.type].splice(V.sel.i, 1);
      V.sel = null;
      editSelect([]);
      ctx.changed(true);
      return;
    }
    if ((ev.ctrlKey || ev.metaKey) && k === 'd' && (V.sel || editSelection().length)) {
      ev.preventDefault();
      const copies = duplicateObjects(map, editSelection().length ? editSelection() : [V.sel], ctx.ids('npc'));
      editSelect(copies);
      V.sel = copies[0] || null;
      ctx.changed(true);
      return;
    }
    if (ev.ctrlKey || ev.metaKey || ev.altKey) return;
    const arrow = ARROWS[ev.key];
    if (arrow && (V.sel || editSelection().length) && wrap.contains(document.activeElement)) {
      // Arrows nudge the selection a tile; Shift a quarter tile (32 units).
      ev.preventDefault();
      const many = editSelection().length ? editSelection() : [V.sel];
      const step = ev.shiftKey ? 0.25 : 1;
      if (ctx.commit) ctx.commit();
      const r = nudge(map, many, arrow[0] * step, arrow[1] * step);
      if (!r.moved) return;
      // One undo step a press; the canvas keeps the focus for the next.
      mctx.changed();
      if (ctx.commit) ctx.commit();
      renderSelection(ctx, mctx, side, map);
      editorChanged();
      return;
    }
    if (k === 'escape' && (V.sel || editSelection().length)) {
      V.sel = null;
      editSelect([]);
      renderSelection(ctx, mctx, side, map);
      redraw();
      return;
    }
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

  // A click in the 3D preview selects here: the first object on the tile
  // (as the Select tool picks), else the tile itself.
  if (pv) {
    pv.setMap(V.map, '');
    // The 3D preview's editing layer (expansion-workbench-gizmos.js): its
    // writes go through this painter's undo and redraw.
    mountMapEditor({
      preview: pv,
      map: () => ctx.pack.maps[V.map],
      mapPreview: () => V.preview[V.map],
      // One undo step a 3D edit: the page history's (ctx.commit), and the
      // painter's own stack where the page has none.
      snapshot: () => { if (ctx.commit) ctx.commit(); },
      refresh: (structural) => {
        if (structural) { ctx.changed(true); return; }
        mctx.changed();
        if (ctx.commit) ctx.commit();
        renderSelection(ctx, mctx, side, map);
      },
      undo: (dir) => undo(ctx, dir),
      selected: (list) => {
        V.sel = list[0] || null;
        renderSelection(ctx, mctx, side, map);
        draw(ctx, cv);
      },
      ids: () => ctx.ids('npc'),
    });
    pv.onCollision((m) => {
      V.blocked[m.map] = m;
      if (V.collide && cv.isConnected && m.map === V.map) redraw();
    });
    pv.onPick((c, r) => {
      if (!cv.isConnected) return;
      V.cursor = { map: V.map, tile: [c, r] };
      const hit = objectsAt(map, c, r);
      V.sel = hit.length ? hit[0] : null;
      editSelect(V.sel ? [V.sel] : []);
      hover.textContent = describe(ctx, map, c, r);
      renderSelection(ctx, mctx, side, map);
      redraw();
      const x = (c + 0.5) * V.cell, y = (r + 0.5) * V.cell;
      if (x < wrap.scrollLeft || x > wrap.scrollLeft + wrap.clientWidth || y < wrap.scrollTop || y > wrap.scrollTop + wrap.clientHeight) {
        wrap.scrollLeft = x - wrap.clientWidth / 2;
        wrap.scrollTop = y - wrap.clientHeight / 2;
      }
    });
  }
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

/* The walk-through check (expansion-workbench-hints.js) on the canvas: a
 * large prop or decoration the player walks through gets a red cross-hatched
 * tile, and a walk-on dialog trigger an amber corner mark (a repeating one
 * a filled mark). The same objects list as hints under the Checks. */
function drawWalkThrough(g, map, cs) {
  const bad = walkThrough(map, (sc) => V.objects[sc] || null);
  g.save();
  g.strokeStyle = '#ff4d4d';
  g.lineWidth = 2;
  for (const w of bad) {
    const o = (w.type === 'props' ? map.props : map.decor)[w.i];
    const off = o.offset || [0, 0, 0];
    const x = (o.tile[0] + 0.5 + (off[0] || 0) / 128) * cs, y = (o.tile[1] + 0.5 + (off[2] || 0) / 128) * cs;
    const r = cs * 0.42;
    g.setLineDash([]);
    g.strokeRect(x - r, y - r, 2 * r, 2 * r);
    g.beginPath();
    g.moveTo(x - r, y - r); g.lineTo(x + r, y + r);
    g.moveTo(x + r, y - r); g.lineTo(x - r, y + r);
    g.stroke();
  }
  for (const t of walkOnDialogs(map)) {
    const q = map.triggers[t.i].rect || [0, 0, 0, 0];
    const x = Math.min(q[0], q[2]) * cs, y = Math.min(q[1], q[3]) * cs;
    const k = Math.max(6, cs * 0.35);
    g.fillStyle = t.repeats ? 'rgba(255,190,60,0.95)' : 'rgba(255,190,60,0.45)';
    g.beginPath();
    g.moveTo(x, y); g.lineTo(x + k, y); g.lineTo(x, y + k);
    g.closePath();
    g.fill();
  }
  g.restore();
}

/* The quarters the engine's collision grid closes past the tile's own
 * walls (solid props, decorations, terrain walls), from the 3D preview's
 * last build of this map: red, a quarter tile each. */
function drawCollision(g, w, ht, cs) {
  const c = V.blocked[V.map];
  if (!c || c.width !== w || c.height !== ht) return;
  const hh = cs / 2;
  g.save();
  g.fillStyle = 'rgba(232, 52, 52, 0.55)';
  g.strokeStyle = 'rgba(120, 10, 10, 0.9)';
  g.lineWidth = 1;
  for (let r = 0; r < ht; r++) {
    for (let col = 0; col < w; col++) {
      const k = r * w + col;
      const extra = (c.blocked[k] & 15) & ~(c.walls[k] || 0);
      if (!extra) continue;
      for (const [q, bit] of Object.entries(QBIT)) {
        if (!(extra & bit)) continue;
        const x = col * cs + (q[1] === 'r' ? hh : 0), y = r * cs + (q[0] === 'b' ? hh : 0);
        g.fillRect(x + 1, y + 1, hh - 2, hh - 2);
        g.strokeRect(x + 1.5, y + 1.5, hh - 3, hh - 3);
      }
    }
  }
  g.restore();
}

function collisionToggle(redraw, pv) {
  const cb = h('input', { type: 'checkbox' });
  cb.checked = V.collide;
  cb.addEventListener('change', () => {
    V.collide = cb.checked;
    // The marks come from the engine: open the preview to get them.
    if (V.collide && pv && !V.blocked[V.map] && !pv.isOpen()) pv.open();
    redraw();
  });
  return h('label', { class: 'xw-check xw-field', title: 'Mark in red what the player bumps into beyond the painted walls - solid props, decorations and terrain - as the engine\'s collision grid has it after the 3D preview\'s last build (it opens the preview)' }, cb, 'Collision');
}

function walkToggle(redraw) {
  const cb = h('input', { type: 'checkbox' });
  cb.checked = V.walk;
  cb.addEventListener('change', () => { V.walk = cb.checked; redraw(); });
  return h('label', { class: 'xw-check xw-field', title: 'Mark large props and decorations the player walks through (red cross) and walk-on dialog triggers (amber corner; solid when it repeats on every visit). The Checks list them as hints.' }, cb, 'Walk-through');
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

/** The borrowed placement's clips by measured pose (`scene_npcs`'
 * `postures`, legaia_web_expansion::npc_posture): which ids stand, walk,
 * sit (and need a seat under them) or crouch. Empty until the disc's list
 * has loaded. */
function clipPostureHint(model) {
  const list = model && V.npcs[model.scene];
  const row = list && (list.postures || []).find((r) => r.index === model.index);
  const el = h('p', { class: 'xw-hint', 'data-path': 'clip-postures' });
  if (!row) return el;
  const part = (label, ids) => (ids && ids.length ? `${label}: ${ids.join(', ')}` : '');
  const own = row.clip ? `own clip ${row.clip}` : 'no clip of its own';
  const parts = [part('standing', row.standing), part('walking', row.walking),
    part('seated (put a seat under it)', row.seated), part('crouched', row.crouched)].filter(Boolean);
  el.textContent = `Clips by measured pose (${own}): ${parts.join('; ')}.`;
  return el;
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
      legend[nv] = st;
      delete legend[ch];
      map.tiles.rows = map.tiles.rows.map((row) => [...row].map((x) => (x === ch ? nv : x)).join(''));
      V.brush = nv;
      ctx.changed(true);
    });
    ed.append(h('div', { class: 'xw-tools' }, h('span', { class: 'wb-label' }, 'Character'), keyIn,
      h('button', {
        type: 'button', class: 'wb-mini', onclick: () => {
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
    h('button', { type: 'button', class: 'wb-mini', onclick: () => { map[sel.type].splice(sel.i, 1); V.sel = null; ctx.changed(true); } }, 'Delete')));
  // Snap, align, spread, repeat, fill between and mirror over the
  // selection (the multi-selection when there is one).
  const lay = layoutRow([sel], {
    map: () => map,
    snapshot: () => { if (ctx.commit) ctx.commit(); },
    refresh: () => ctx.changed(true),
    selected: (list) => { editSelect(list.length > 1 ? list : []); V.sel = list[0] || null; },
  });
  if (lay) box.append(lay);
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
    box.append(gateBox(mctx, o, 'requires', p, 'Open when'));
    box.append(h('div', { class: 'xw-box-head' }, 'While shut'));
    box.append(grid(mctx, o, [
      { k: 'locked_line', t: 'oref', kind: 'line', label: 'Says why', hint: 'on each step onto the shut exit: "The seal holds."' },
      { k: 'locked_toast', t: 'bool', label: 'As a toast', hint: 'a one-row notice at the top; the player is not stopped' },
    ], p));
    box.append(h('p', { class: 'xw-note' }, !o.requires
      ? 'The exit has no "Open when" gate, so it is never shut and this line is never said.'
      : o.locked_line
        ? (o.locked_toast ? 'A toast: the player reads why and keeps walking. Only the line\'s first page shows (40 characters).' : 'A dialog box: it stops the player on every step onto the exit. Tick As a toast for a lighter touch.')
        : 'No line: the shut exit just does nothing when stepped on.'));
  } else if (sel.type === 'triggers') {
    box.append(grid(mctx, o, [{ k: 'id', label: 'Id' }, rect], p));
    // How it fires: on entry, or as an examine (`press`).
    const how = h('select', { 'aria-label': 'Fires' },
      h('option', { value: 'walk', selected: !o.press }, 'when the player steps on it'),
      h('option', { value: 'press', selected: !!o.press }, 'when the player examines it (action button)'));
    how.addEventListener('change', () => { if (how.value === 'press') o.press = true; else delete o.press; ctx.changed(true); });
    box.append(h('label', { class: 'xw-field', 'data-path': `${p}.press` }, h('span', {}, 'Fires', h('span', { class: 'xw-hint' }, ' an examine fires while standing on it or facing it from the next tile')), how));
    box.append(grid(mctx, o, [{ k: 'once', t: 'bool', label: 'Once ever', hint: 'fires the first time only; a flag remembers it across visits and saves' }], p));
    const ev = o.event;
    const EVENTS = {
      battle: [{ k: 'formation', t: 'ref', kind: 'formation', label: 'Formation' }],
      boss: [{ k: 'boss', t: 'ref', kind: 'boss', label: 'Boss' }],
      dialog: [{ k: 'line', t: 'ref', kind: 'line', label: 'Line', hint: 'a dialog box: the player stops and reads' }],
      toast: [{ k: 'line', t: 'ref', kind: 'line', label: 'Line', hint: 'its first page as a one-row notice; the player keeps walking' }],
      talk: [{ k: 'npc', label: 'NPC id', hint: 'a quest giver / talk objective' }],
      start_quest: [{ k: 'quest', t: 'ref', kind: 'quest', label: 'Quest' }],
      set_flag: [{ k: 'flag', label: 'Pack flag' }],
      counter: [{ k: 'counter', label: 'Pack counter' }, { k: 'add', t: 'int', label: 'Add' }],
      card_dealer: [{ k: 'dealer', label: 'Card dealer', hint: 'a cards.toml dealer id; empty = the first' }],
      shop: [{ k: 'shop', t: 'ref', kind: 'shop', label: 'Item shop', hint: 'an items.toml shop' }],
      challenge: [{ k: 'challenge', t: 'ref', kind: 'challenge', label: 'Challenge' }, { k: 'start', t: 'bool', label: 'Start its battle' }],
      mode: [{ k: 'mode', t: 'ref', kind: 'mode', label: 'Mode instance' }],
      rest: [{ k: 'cost', t: 'int', label: 'Gold cost' }, { k: 'save', t: 'bool', label: 'Then offer a save' }],
      cutscene: [{ k: 'cutscene', t: 'ref', kind: 'cutscene', label: 'Cutscene' }],
      sound: [{ k: 'sound', t: 'sound', label: 'Sound', hint: 'a retail cue or an audio.toml sound' }],
    };
    const EVENT_LABEL = { dialog: 'dialog (a box that stops the player)', toast: 'toast (a notice that stops nothing)' };
    const kind = h('select', {}, ...Object.keys(EVENTS).map((k) => h('option', { value: k, selected: ev.kind === k }, EVENT_LABEL[k] || k.replace('_', ' '))));
    kind.addEventListener('change', () => {
      const k = kind.value;
      const first = (kd) => ctx.ids(kd)[0] || '';
      o.event = { kind: k };
      for (const f of EVENTS[k]) {
        o.event[f.k] = f.t === 'ref' ? first(f.kind) : f.t === 'bool' ? false : f.t === 'int' ? (f.k === 'cost' ? 0 : 1) : f.t === 'sound' ? { kind: 'sfx', id: 0x21 } : (k === 'talk' ? first('npc') : '');
      }
      ctx.changed(true);
    });
    box.append(h('label', { class: 'xw-field', 'data-path': `${p}.event` }, h('span', {}, 'Raises'), kind));
    box.append(grid(mctx, ev, EVENTS[ev.kind] || [], `${p}.event`));
    // What the player meets, in a line: walk-on text is the thing to keep rare.
    if (ev.kind === 'dialog' || ev.kind === 'toast') {
      const say = ev.kind === 'toast'
        ? (o.press ? 'Examined, it shows a toast: a light touch for flavour text.' : 'Stepping on it shows a toast: the player reads it in passing and keeps walking.')
        : o.press
          ? 'Examined, it opens a dialog box: the player asked for it, so it may run long.'
          : o.once
            ? 'Stepping on it opens a dialog box, the first time only: keep these for story beats.'
            : 'Stepping on it opens a dialog box on every visit: that stops the player each time they pass. Make it an examine, a toast, or Once ever.';
      box.append(h('p', { class: 'xw-note' + (ev.kind === 'dialog' && !o.press && !o.once ? ' is-warn' : '') }, say));
    }
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
      { k: 'cutscene', t: 'oref', kind: 'cutscene', label: 'Plays (cutscene)', hint: 'in place of its line' },
      { k: 'scenery', t: 'bool', label: 'Scenery (plays its clip; no talk, no collision)' },
    ], p));
    box.append(clipPostureHint(o.model));
    box.append(gateBox(mctx, o, 'requires', p, 'Talks when'));
    box.append(gateBox(mctx, o, 'after', p, 'After (switches to the second line)'));
    box.append(gateBox(mctx, o, 'present', p, 'Stands on the map when (else a cutscene brings it on)'));
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
