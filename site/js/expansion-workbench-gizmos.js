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
// The page shell's mode toolbar (a cycle with the shell, which imports this
// module: the names are only called at run time).
import { registerBuildTools, syncBuildMode, setBuildMode, buildMode } from './expansion-workbench-shell.js';
import { setPainterTool } from './expansion-workbench-map.js';

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
export function frameObjects(map, sizeOf) {
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
        // The mesh's own extent where the catalog knows it (`sizeOf`, the
        // base scene's geometry pack), else the footprint or a default box.
        const ms = sizeOf ? sizeOf(map, o.mesh) : null;
        const fp = o.footprint || (ms ? [ms[0], ms[2]] : [type === 'props' ? 96 : 48, type === 'props' ? 96 : 48]);
        const hg = o.top != null ? Math.max(24, o.top) : ms ? Math.max(24, Math.min(400, ms[1])) : (type === 'props' ? 72 : 40);
        out.push({ key, kind: type === 'decor' ? 'decor' : 'props', x: cx(t[0]) + (off[0] || 0), z: cz(t[1]) + (off[2] || 0), yaw: (o.rotation || [0, 0, 0])[1] || 0, scale: o.scale || 1, size: fp, hgt: hg, solid: o.solid !== false && type === 'props' ? !!o.solid : false });
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

/* --- The build catalog -------------------------------------------------------------
 *
 * What a modder places, The Sims' build-mode way: pick a piece from the
 * catalog, a ghost of it follows the pointer in the 3D view snapped to the
 * tile grid (green where it can stand, red where it cannot), R turns it, a
 * click puts it down and the piece stays armed for the next; Escape (or a
 * right click) puts it away. The pieces:
 *
 *   - props: every mesh the pack's maps already use, named by the id of the
 *     prop that uses it (`cottage_w1` -> "cottage"), then the rest of the map's
 *     base-scene geometry pack, both sorted by size into Buildings, Large and
 *     Small props;
 *   - people: the NPCs the pack already has (their model, clip and line kept),
 *     then the base scene's retail placements by what each first says;
 *   - markers: spawn, lamp, trigger zone, encounter zone, exit.
 *
 * A placed piece copies the pack's own example where there is one (scale,
 * solid, top, light, clip), so a second cottage is the first one's twin. */

const OBJ = {};
const NPCS = {};

function sceneOf(map, mesh) {
  if (!mesh || mesh.object == null) return null;
  if (mesh.scene) return mesh.scene;
  const b = map && map.base && map.base.scene;
  return b && b.kind === 'scene' && b.name ? b.name : null;
}

/* A scene's geometry pack (`scene_objects`), read once a scene and a moment
 * later than asked (assembling a scene takes a while): null until then. */
function sceneObjects(scene) {
  if (!scene || !G.api || !G.api.sceneObjects) return null;
  if (OBJ[scene] !== undefined) return OBJ[scene];
  OBJ[scene] = null;
  setTimeout(() => {
    try { OBJ[scene] = G.api.sceneObjects(scene); } catch (e) { OBJ[scene] = { scene, objects: [] }; }
    G.catalog = null;
    renderBar();
    sync(true);
  }, 20);
  return null;
}

/** The extent `[x, y, z]` of mesh source `mesh` on `map` (scaled by
 * nothing), or null when the catalog does not know it yet. */
export function meshSize(map, mesh) {
  const sc = sceneObjects(sceneOf(map, mesh));
  const o = sc && sc.objects && sc.objects.find((x) => x.index === mesh.object);
  return o ? o.size : null;
}

function sceneNpcs(scene) {
  if (!scene || !G.api || !G.api.sceneNpcs) return null;
  if (NPCS[scene] !== undefined) return NPCS[scene];
  NPCS[scene] = null;
  setTimeout(() => {
    try { NPCS[scene] = G.api.sceneNpcs(scene); } catch (e) { NPCS[scene] = { scene, npcs: [] }; }
    G.catalog = null;
    renderBar();
  }, 40);
  return null;
}

const stem = (id) => String(id || '').replace(/(_[a-z]?\d+)+$/i, '').replace(/_/g, ' ') || 'prop';
const sizeClass = (w) => (w >= 384 ? 'Buildings' : w >= 160 ? 'Large props' : 'Small props');

/** The catalog for the open map: `[{ id, label, items }]`, each item
 * `{ id, label, kind, size, hgt, hint, make }`. */
export function catalog() {
  const api = G.api;
  const map = api && api.map();
  if (!map) return [];
  if (G.catalog && G.catalog.map === map) return G.catalog.groups;
  const pack = api.pack ? api.pack() : null;
  const groups = { Buildings: [], 'Large props': [], 'Small props': [], People: [], Markers: [] };
  const seen = new Set();
  const base = sceneOf(map, { object: 0 });
  // Props the pack already uses, on any map (a mesh named by its scene so
  // it builds on this one too).
  const uses = [];
  for (const [mid, m] of Object.entries((pack && pack.maps) || { here: map })) {
    for (const p of m.props || []) uses.push([mid, m, p]);
  }
  uses.sort((a, b) => (a[1] === map ? 0 : 1) - (b[1] === map ? 0 : 1));
  for (const [, m, p] of uses) {
    if (!p.mesh) continue;
    const scene = sceneOf(m, p.mesh);
    const mesh = p.mesh.object != null && scene ? { scene, object: p.mesh.object } : p.mesh;
    const k = JSON.stringify(mesh.object != null ? [mesh.scene, mesh.object] : mesh);
    if (seen.has(k)) continue;
    seen.add(k);
    const sz = mesh.object != null ? meshSize(map, mesh) : null;
    const sc = p.scale || 1;
    const w = sz ? Math.max(sz[0], sz[2]) * sc : 96;
    groups[sizeClass(w)].push({
      id: 'prop:' + k, label: stem(p.id), kind: 'prop', mesh, size: sz ? [sz[0] * sc, sz[2] * sc] : [96, 96],
      hgt: sz ? Math.max(24, sz[1] * sc) : 72, solid: !!p.solid, foot: footOf(map, mesh),
      hint: `as ${p.id}${sz ? `, ${Math.round(sz[0] * sc)} x ${Math.round(sz[2] * sc)} units` : ''}`,
      make: () => { const o = JSON.parse(JSON.stringify(p)); delete o.id; delete o.on; o.mesh = mesh.object != null && mesh.scene === base ? { object: mesh.object } : mesh; return o; },
    });
  }
  // The base scene's geometry the pack does not use yet.
  const so = sceneObjects(base);
  for (const o of (so && so.objects) || []) {
    const k = JSON.stringify([base, o.index]);
    if (seen.has(k) || !o.prims) continue;
    const w = Math.max(o.size[0], o.size[2]);
    groups[sizeClass(w)].push({
      id: 'prop:' + k, label: `${base} #${o.index}`, kind: 'prop', mesh: { object: o.index }, size: [o.size[0], o.size[2]],
      hgt: Math.max(24, o.size[1]), solid: w >= 160, foot: o.footprint,
      hint: `${o.size[0]} x ${o.size[2]} units, ${o.prims} prims${o.placed ? `, placed ${o.placed}x in ${base}` : ''}`,
      make: () => ({ mesh: { object: o.index }, offset: [0, 0, 0], rotation: [0, 0, 0], scale: null, solid: w >= 160 }),
    });
  }
  // People: the pack's NPCs, then the base scene's placements.
  const npcSeen = new Set();
  for (const [, m] of Object.entries((pack && pack.maps) || { here: map })) {
    for (const n of m.npcs || []) {
      if (!n.model) continue;
      const k = JSON.stringify(n.model);
      if (npcSeen.has(k)) continue;
      npcSeen.add(k);
      groups.People.push({
        id: 'npc:' + k, label: n.label || stem(n.id), kind: 'npc', size: [44, 44], hgt: 150, facing: true,
        hint: `as ${n.id}`, make: () => { const o = JSON.parse(JSON.stringify(n)); delete o.id; o.patrol = []; return o; },
      });
    }
  }
  const sn = sceneNpcs(base);
  for (const p of (sn && sn.npcs) || []) {
    const model = { kind: 'npc', scene: base, index: p.index };
    const k = JSON.stringify(model);
    if (npcSeen.has(k) || p.special) continue;
    npcSeen.add(k);
    groups.People.push({
      id: 'npc:' + k, label: p.label ? `"${p.label.slice(0, 28)}"` : `${base} #${p.index}`, kind: 'npc', size: [44, 44], hgt: 150, facing: true,
      hint: `${base} placement #${p.index} (${p.kind})`,
      make: () => ({ model: { ...model }, facing: 0, line: (api.idsOf ? api.idsOf('line')[0] : '') || '', patrol: [] }),
    });
  }
  groups.Markers.push(
    { id: 'spawn', label: 'Spawn', kind: 'spawn', size: [44, 44], hgt: 110, facing: true, hint: 'where an arrival stands', make: () => ({ facing: 0 }) },
    { id: 'lamp', label: 'Lamp', kind: 'lamp', size: [28, 28], hgt: 60, hint: 'a point light', make: () => ({ offset: [0, 96, 0], color: [255, 176, 96], radius: 640 }) },
    { id: 'trigger', label: 'Trigger zone', kind: 'zone', zone: [2, 2], list: 'triggers', hint: 'an event when the player walks in (2 x 2; drag its corner to size it)', make: () => ({ event: { kind: 'dialog', line: (api.idsOf ? api.idsOf('line')[0] : '') || '' }, once: true }) },
    { id: 'encounter', label: 'Encounter zone', kind: 'zone', zone: [4, 4], list: 'encounters', hint: 'random battles inside it (4 x 4)', make: () => ({ table: '' }) },
    { id: 'exit', label: 'Exit', kind: 'zone', zone: [1, 1], list: 'exits', hint: 'a way off the map', make: () => ({ to: { kind: 'retail', scene: { kind: 'scene', name: base || 'town01' }, facing: 0 } }) },
  );
  for (const k of ['Buildings', 'Large props', 'Small props']) groups[k].sort((a, b) => (a.label < b.label ? -1 : a.label > b.label ? 1 : 0));
  // Two meshes one name (two cottages): numbered, so each reads apart.
  const seenLabel = new Map();
  for (const items of Object.values(groups)) {
    for (const it of items) {
      const n = (seenLabel.get(it.label) || 0) + 1;
      seenLabel.set(it.label, n);
      if (n > 1) it.label = `${it.label} ${n}`;
    }
  }
  const out = Object.entries(groups).filter(([, items]) => items.length).map(([label, items]) => ({ id: label.toLowerCase().replace(/ /g, '-'), label, items }));
  G.catalog = { map, groups: out };
  return out;
}

function footOf(map, mesh) {
  const sc = sceneObjects(sceneOf(map, mesh));
  const o = sc && sc.objects && sc.objects.find((x) => x.index === mesh.object);
  return o ? o.footprint : null;
}

function findItem(id) {
  for (const g of catalog()) for (const it of g.items) if (it.id === id) return it;
  return null;
}

/* What the frame's ghost is: the armed item's shape, turned. */
function placeSpec(id) {
  const it = id && findItem(id);
  if (!it) return null;
  return { kind: it.kind, size: it.size, hgt: it.hgt || 60, yaw: G.yaw || 0, facing: !!it.facing, zone: it.zone || null, solid: it.solid, label: it.label };
}

/** Arm catalog item `id` (the 3D view's place mode with its ghost). */
export function arm(id) {
  const it = findItem(id);
  if (!it) return false;
  G.armed = id;
  if (G.mode !== 'place') G.prevMode = G.mode;
  G.mode = 'place';
  G.picked = true;
  if (it.facing) G.yaw = Math.round((G.yaw || 0) / 45) * 45;
  renderBar();
  sync();
  followShell();
  return true;
}

function note(text, bad) {
  const pv = G.api && G.api.preview;
  if (pv && pv.note) pv.note(text, bad);
}

function disarm() {
  if (G.mode !== 'place') return;
  G.mode = G.prevMode && G.prevMode !== 'place' ? G.prevMode : 'select';
  renderBar();
  sync();
  followShell();
}

/* Put the armed piece down on tile `t`, turned `yaw` (the painter's
 * degrees): one undo step; the new piece is selected. */
function placeAt(t, yaw) {
  const api = G.api;
  const map = api && api.map();
  const it = G.armed && findItem(G.armed);
  if (!map || !it || !t) return;
  api.snapshot();
  const o = it.make();
  const ids = (list) => new Set(list.map((x) => x.id).concat(api.idsOf ? api.idsOf(it.kind === 'npc' ? 'npc' : it.kind) : []));
  let type;
  if (it.kind === 'prop') {
    type = 'props';
    o.tile = [t[0], t[1]];
    o.offset = [0, (o.offset && o.offset[1]) || 0, 0];
    const rot = o.rotation || [0, 0, 0];
    const y = ((Math.round(yaw * 10) / 10) % 360 + 360) % 360;
    o.rotation = [rot[0] || 0, y > 180 ? y - 360 : y, rot[2] || 0];
    o.id = freshIdIn(it.label.replace(/[^a-z0-9]+/gi, '_').toLowerCase() || 'prop', ids(listOf(map, 'props')));
  } else if (it.kind === 'npc' || it.kind === 'spawn') {
    type = it.kind === 'npc' ? 'npcs' : 'spawns';
    o.tile = [t[0], t[1]];
    o.facing = (((Math.round((90 - yaw) / 45)) % 8) + 8) % 8;
    o.id = freshIdIn(it.kind === 'npc' ? (it.label.replace(/[^a-z0-9]+/gi, '_').toLowerCase() || 'npc') : 'spawn', ids(listOf(map, type)));
  } else if (it.kind === 'lamp') {
    type = 'lamps';
    o.tile = [t[0], t[1]];
  } else {
    type = it.list;
    const [W, H] = dims(map);
    o.rect = [t[0], t[1], Math.min(W - 1, t[0] + it.zone[0] - 1), Math.min(H - 1, t[1] + it.zone[1] - 1)];
    o.id = freshIdIn(it.id, ids(listOf(map, type)));
  }
  const list = listOf(map, type, true);
  list.push(o);
  G.sel = [{ type, i: list.length - 1 }];
  api.selected(G.sel);
  api.refresh(true);
  // The note goes to the preview's status line: a longer bar would move
  // the picture under the pointer.
  note(`Placed ${it.label} at [${t[0]}, ${t[1]}]; click again for another, Escape to stop.`);
  renderBar();
  sync(true);
}

/* A catalog tile's picture: the mesh's footprint from above (its convex
 * hull, the painter's prop outline), else a glyph for its kind. */
function thumb(it) {
  const c = h('canvas', { width: 44, height: 44, class: 'xw-piece-thumb', 'aria-hidden': 'true' });
  const g = c.getContext && c.getContext('2d');
  if (!g) return c;
  g.fillStyle = 'rgba(127,127,127,0.12)';
  g.fillRect(0, 0, 44, 44);
  const tone = { prop: '#d8c06a', npc: '#9b6bf0', spawn: '#3ccf8f', lamp: '#ffd75a', zone: '#e8a33d' }[it.kind] || '#ccc';
  g.fillStyle = tone;
  g.strokeStyle = tone;
  if (it.kind === 'prop' && it.foot && it.foot.length > 2) {
    let lo = [Infinity, Infinity], hi = [-Infinity, -Infinity];
    for (const [x, z] of it.foot) { lo = [Math.min(lo[0], x), Math.min(lo[1], z)]; hi = [Math.max(hi[0], x), Math.max(hi[1], z)]; }
    const k = 36 / Math.max(hi[0] - lo[0], hi[1] - lo[1], 1);
    g.beginPath();
    it.foot.forEach(([x, z], i) => { const px = 4 + (x - lo[0]) * k, py = 4 + (z - lo[1]) * k; if (i) g.lineTo(px, py); else g.moveTo(px, py); });
    g.closePath();
    g.globalAlpha = 0.55;
    g.fill();
    g.globalAlpha = 1;
    g.stroke();
  } else if (it.kind === 'npc' || it.kind === 'spawn') {
    g.beginPath(); g.arc(22, 13, 6, 0, Math.PI * 2); g.fill();
    g.fillRect(15, 21, 14, 16);
  } else if (it.kind === 'lamp') {
    g.beginPath(); g.arc(22, 20, 9, 0, Math.PI * 2); g.fill();
    g.fillRect(20, 29, 4, 10);
  } else if (it.kind === 'zone') {
    g.setLineDash([4, 3]); g.lineWidth = 2; g.strokeRect(7, 7, 30, 30);
  } else {
    g.fillRect(10, 10, 24, 24);
  }
  return c;
}

/** The catalog panel: a category strip and the pieces of the open
 * category as tiles (picture, name, size); a click arms one. */
export function catalogEl(into, only) {
  installCatalogCss();
  if (!into && !G.catEl) G.catEl = h('div', { class: 'xw-pieces', role: 'region', 'aria-label': 'Build catalog' });
  const el = into || G.catEl;
  el.innerHTML = '';
  const groups = catalog().filter((g) => !only || only.includes(g.id));
  if (!groups.length) return el;
  if (!groups.some((g) => g.id === G.catGroup)) G.catGroup = groups[0].id;
  const tabs = h('div', { class: 'xw-piece-tabs', role: 'tablist' });
  for (const g of groups) {
    tabs.append(h('button', {
      type: 'button', role: 'tab', class: 'wb-mini' + (g.id === G.catGroup ? ' is-on' : ''), 'aria-selected': String(g.id === G.catGroup),
      onclick: () => { G.catGroup = g.id; catalogEl(into, only); },
    }, `${g.label} (${g.items.length})`));
  }
  const q = h('input', { type: 'search', class: 'xw-piece-find', placeholder: 'Find a piece', 'aria-label': 'Find a catalog piece', value: G.catFind || '' });
  q.addEventListener('input', () => { G.catFind = q.value; paintItems(); });
  tabs.append(q);
  const items = h('div', { class: 'xw-piece-items' });
  function paintItems() {
    items.innerHTML = '';
    const f = (G.catFind || '').toLowerCase();
    const list = f ? groups.flatMap((g) => g.items).filter((it) => (it.label + ' ' + it.hint).toLowerCase().includes(f)) : (groups.find((g) => g.id === G.catGroup) || groups[0]).items;
    for (const it of list.slice(0, 160)) {
      const on = G.mode === 'place' && G.armed === it.id;
      items.append(h('button', {
        type: 'button', class: 'xw-piece' + (on ? ' is-on' : ''), title: `${it.label}: ${it.hint}. Click, then click on the map to place (R turns it, Escape stops).`,
        'data-item': it.id, onclick: () => { if (on) disarm(); else arm(it.id); },
      }, thumb(it), h('span', { class: 'xw-piece-name' }, it.label)));
    }
    if (!list.length) items.append(h('span', { class: 'xw-hint' }, 'Nothing matches.'));
  }
  paintItems();
  el.append(tabs, items);
  return el;
}

const CATALOG_CSS = `
.xw-pieces { display: grid; gap: 0.3rem; margin: 0.3rem 0; }
.xw-piece-tabs { display: flex; flex-wrap: wrap; gap: 0.25rem; align-items: center; }
.xw-piece-find { flex: 1 1 8rem; min-width: 6rem; max-width: 14rem; }
.xw-piece-items { display: flex; gap: 0.3rem; overflow-x: auto; padding-bottom: 0.25rem; scrollbar-width: thin; }
.xw-piece { flex: 0 0 auto; width: 4.6rem; display: grid; justify-items: center; gap: 0.15rem; padding: 0.25rem; border: 1px solid var(--border); border-radius: 6px; background: var(--bg-2, transparent); color: inherit; cursor: pointer; font-size: 0.72rem; line-height: 1.15; }
.xw-piece:hover, .xw-piece:focus-visible { border-color: var(--accent); }
.xw-piece.is-on { border-color: var(--accent); box-shadow: 0 0 0 1px var(--accent) inset; }
.xw-piece-name { max-width: 100%; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.xw-piece-thumb { width: 44px; height: 44px; }
`;
function installCatalogCss() {
  if (typeof document === 'undefined' || document.getElementById('xw-pieces-css')) return;
  const st = document.createElement('style');
  st.id = 'xw-pieces-css';
  st.textContent = CATALOG_CSS;
  document.head.append(st);
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
  const objs = frameObjects(map, meshSize);
  const key = JSON.stringify(objs);
  const msg = {
    mode: G.mode, tool: G.tool, snap: G.snap, brush: G.brush,
    selected: G.sel.map((s) => `${s.type}:${s.i}`),
    place: G.mode === 'place' ? placeSpec(G.armed) : null,
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
  } else if (m.type === 'xw-place') {
    placeAt(m.tile, m.yaw || 0);
  } else if (m.type === 'xw-key') {
    if (m.key === 'cancel-place') { disarm(); return; }
    if (m.key === 'digit') { try { setBuildMode(['build', 'terrain', 'objects', 'npcs', 'zones', 'test'][(m.n | 0) - 1]); } catch (e) { /* no shell */ } return; }
    if (m.key === 'place-yaw') { G.yaw = m.yaw || 0; return; }
    if (m.key === 'blocked') { note(`Not moved: ${m.why || 'a piece was dropped where it cannot stand'}.`, true); return; }
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
  bar.append(h('span', { class: 'wb-label' }, 'Build'),
    b('View', G.mode === 'view', 'The picture is only looked at: a click picks a tile', () => setMode('view')),
    b('Select', G.mode === 'select', 'Pick, move (drag), turn (R) and scale props, decorations, NPCs, spawns, lamps, emitters and zones', () => setMode('select')),
    b('Place', G.mode === 'place', 'Place a piece from the catalog below: a ghost follows the pointer, green where it fits; click to place, R turns it, Escape stops', () => setTool('place')));
  if (map && map.terrain) {
    bar.append(b('Sculpt', G.mode === 'sculpt', 'Shape the terrain with a brush ([ and ] size it)', () => setMode('sculpt')),
      b('Paint', G.mode === 'paint', 'Paint a ground material under the brush', () => setMode('paint')));
  }
  const num = (label, key, step, min, max) => {
    const i = h('input', { type: 'number', value: G.brush[key], step, min, max, 'aria-label': label, class: 'xw-num' });
    i.addEventListener('change', () => { const v = parseFloat(i.value); if (Number.isFinite(v)) { G.brush[key] = Math.max(min, Math.min(max, v)); sync(); } });
    return h('label', { class: 'xw-check' }, label, ' ', i);
  };
  if (G.mode === 'place') {
    const it = G.armed && findItem(G.armed);
    bar.append(h('span', { class: 'xw-sep' }),
      h('span', { class: 'xw-hint' }, it ? ` Placing ${it.label}: click a tile (green fits, red does not); R turns 90 degrees (Shift+R 15), Shift+wheel turns; Escape or a right click stops.` : ' Pick a piece below.'));
    if (G.layoutMsg) bar.append(h('span', { class: 'xw-hint' }, ' ' + G.layoutMsg));
  }
  if (G.mode === 'select') {
    const tool = (k, l, t) => b(l, G.tool === k, t, () => { G.tool = k; renderBar(); sync(); });
    const snap = h('select', { 'aria-label': 'Snap' },
      ...[['tile', 'Snap: tile'], ['sub', 'Snap: 32 units'], ['free', 'Free']].map(([k, l]) => h('option', { value: k, selected: G.snap === k }, l)));
    snap.addEventListener('change', () => { G.snap = snap.value; sync(); });
    bar.append(h('span', { class: 'xw-sep' }), tool('move', 'Move', 'Drag an object to move it (red where it cannot stand); the gizmo\'s arrows move on one axis, the square on both (G)'),
      tool('rotate', 'Rotate', 'Drag the ring: 15 degree steps (NPCs and spawns by facing sector); R turns a quarter turn'),
      tool('scale', 'Scale', 'Drag the corner handle; a zone\'s corner resizes it'), snap,
      b('Duplicate', false, 'Copy the selection one tile over (Ctrl+D)', duplicateSel),
      b('Delete', false, 'Remove the selection (Delete)', removeSel),
      h('span', { class: 'xw-hint' }, ` ${G.sel.length ? `${G.sel.length} selected; arrows nudge (Shift a quarter tile), R turns 90 (Shift+R 15)` : 'click an object; Shift adds'}; Shift while dragging places freely`));
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
  bar.append(h('div', { class: 'xw-3d-keys xw-hint' }, 'Camera: right drag orbits, middle drag or Space + drag pans, wheel zooms, W A S D / arrows pan, Q / E turn, T top view, F frame. Ctrl+Z / Ctrl+Y undo and redo, Delete removes.'));
  if ((G.mode === 'place' || G.mode === 'select') && G.catalogShown !== false) bar.append(catalogEl());
  if (G.shellCat && G.shellCat.el.isConnected) fillShellCatalog();
}

/** Switch the build tool: `view`, `select`, `move` / `rotate` / `scale`
 * (select with that gizmo), `place` (the catalog's last piece, else its
 * first prop), `npc` (the first person), `zone` (a trigger zone), `paint`
 * or `sculpt` (terrain maps). False when the tool does not apply. */
export function setTool(name) {
  const map = G.api && G.api.map();
  if (['move', 'rotate', 'scale'].includes(name)) { G.mode = 'select'; G.tool = name; }
  else if (name === 'view' || name === 'select') G.mode = name;
  else if (name === 'paint' || name === 'sculpt') { if (!map || !map.terrain) return false; G.mode = name; }
  else if (name === 'place' || name === 'npc' || name === 'zone') {
    const groups = catalog();
    const all = groups.flatMap((g) => g.items);
    const want = name === 'npc' ? (it) => it.kind === 'npc' : name === 'zone' ? (it) => it.kind === 'zone' : (it) => it.kind === 'prop';
    const keep = G.armed && all.find((it) => it.id === G.armed && want(it));
    const it = keep || all.find(want);
    if (!it) return false;
    if (name !== 'place') G.catGroup = (groups.find((g) => g.items.includes(it)) || {}).id;
    return arm(it.id);
  } else return false;
  G.picked = true;
  renderBar();
  sync();
  return true;
}

/** The build mode's API for the page shell (`window.xwBuild`). */
export const xwBuild = {
  setTool,
  /** `[{ id, label, items: [{ id, label, kind, size, hint }] }]` for the open map. */
  catalog: () => catalog().map((g) => ({ id: g.id, label: g.label, items: g.items.map((it) => ({ id: it.id, label: it.label, kind: it.kind, size: it.size, hint: it.hint })) })),
  /** Arm a catalog piece by id (place mode, its ghost in the 3D view). */
  arm,
  /** Put the armed piece away. */
  disarm: () => disarm(),
  /** Test play on / off (toggle with no argument): the party on the map. */
  testPlay: (on) => (G.api && G.api.preview ? G.api.preview.testPlay(on) : false),
  /** The 3D editor camera straight down, or tilted back. */
  topDown: (on) => (G.api && G.api.preview ? G.api.preview.topDown(on) : false),
  /** The build bar and the catalog panel, to place in the page. */
  barEl: () => G.bar,
  catalogEl: () => catalogEl(),
  /** Leave the catalog out of the build bar (the page shows `catalogEl` elsewhere). */
  showCatalogInBar: (on) => { G.catalogShown = !!on; renderBar(); },
  state: () => ({ mode: G.mode, tool: G.tool, armed: G.armed || null, yaw: G.yaw || 0, sel: G.sel.slice(), testing: !!(G.api && G.api.preview && G.api.preview.testing()) }),
};

/* --- The shell's mode toolbar (expansion-workbench-shell.js) ---------------------
 *
 * Build: select and move (the gizmo); Paint terrain: the painter's paint
 * tool, and on a terrain map the 3D paint brush; Place objects / NPCs /
 * Zones & exits: Place with that part of the catalog in the shell's strip;
 * Test-play: the party on the map. Escape, a right click and Stop test come
 * back to Build, and the toolbar follows (`syncBuildMode`). */
const MODE_GROUPS = { objects: ['buildings', 'large-props', 'small-props'], npcs: ['people'], zones: ['markers'] };
const PAINTER_TOOL = { build: 'select', terrain: 'paint', objects: 'select', npcs: 'select', zones: 'select' };

function shellMode() { try { return buildMode(); } catch (e) { return null; } }

const SHELL_TOOLS = {
  setMode(mode, env) {
    const pv = env.preview || (G.api && G.api.preview);
    if (mode !== 'test' && pv && pv.testing && pv.testing()) pv.testPlay(false);
    const map = G.api && G.api.map();
    let painter = PAINTER_TOOL[mode];
    if (mode === 'test') {
      if (pv) { pv.open(); pv.testPlay(true); }
    } else if (mode === 'build') {
      G.mode = 'select';
      G.tool = 'move';
    } else if (mode === 'terrain') {
      G.mode = map && map.terrain ? 'paint' : 'view';
    } else if (MODE_GROUPS[mode]) {
      if (pv && pv.open) pv.open();
      const want = MODE_GROUPS[mode];
      const groups = catalog().filter((g) => want.includes(g.id));
      const keep = G.armed && groups.some((g) => g.items.some((it) => it.id === G.armed));
      if (!groups.some((g) => g.id === G.catGroup)) G.catGroup = groups.length ? groups[0].id : G.catGroup;
      const first = groups.length && groups[0].items[0];
      if (keep) arm(G.armed);
      else if (first) arm(first.id);
      else G.mode = 'select';
    }
    G.picked = true;
    renderBar();
    sync();
    if (painter && setPainterTool(painter) !== false) env.rerender();
  },
  catalog(mode, el) {
    if (mode === 'terrain' || mode === 'test') return false;
    G.shellCat = { el, mode };
    fillShellCatalog();
    return true;
  },
};

function fillShellCatalog() {
  const { el, mode } = G.shellCat;
  el.innerHTML = '';
  if (mode === 'build') {
    el.append(h('div', { class: 'xw-hint' }, 'Click anything in the 3D view to pick it up; drag it to move it (it turns red where it cannot stand), R turns it, Delete removes it, Ctrl+Z takes any change back. Camera: right drag orbits, middle drag or Space + drag pans, the wheel zooms, T looks straight down.'));
    return;
  }
  el.append(catalogEl(h('div', { class: 'xw-pieces', role: 'region', 'aria-label': 'Build catalog' }), MODE_GROUPS[mode]));
}

/* The editor's own keys changed what it does: the shell's toolbar follows. */
function followShell() {
  const cur = shellMode();
  if (!cur) return;
  const want = G.mode === 'place' ? (() => {
    const it = G.armed && findItem(G.armed);
    return it ? (it.kind === 'npc' ? 'npcs' : it.kind === 'prop' ? 'objects' : 'zones') : cur;
  })() : (cur === 'objects' || cur === 'npcs' || cur === 'zones') ? 'build' : cur;
  if (want !== cur) syncBuildMode(want);
}

/** Mount the 3D editor for the map tab. `api`: { preview, map(), pack(),
 * mapPreview(), snapshot(), refresh(structural), undo(dir), selected(list),
 * ids(), idsOf(kind), sceneObjects(scene), sceneNpcs(scene) }. Called on
 * every render of the tab; returns the edit bar. */
export function mountMapEditor(api) {
  if (!G.api || G.api.map() !== api.map()) G.catalog = null;
  G.api = api;
  if (!G.registered) {
    G.registered = true;
    G.catalogShown = false;
    try { registerBuildTools(SHELL_TOOLS); } catch (e) { G.catalogShown = true; }
  }
  if (api.preview && api.preview.onMode) {
    api.preview.onMode((m) => {
      const cur = shellMode();
      if (m === 'player' && cur !== 'test') syncBuildMode('test');
      else if (m === 'orbit' && cur === 'test') syncBuildMode('build');
    });
  }
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
if (typeof window !== 'undefined') {
  window.__xwEdit = { state: editorState, set: setEditor, select, layout: (fn) => runLayout(G.api, G.sel, fn) };
  window.xwBuild = xwBuild;
}

/** The shell's mode toolbar (expansion-workbench-shell.js): the 3D edit
 * mode, `view`, `select`, `sculpt` or `paint` (the last two on a terrain map). */
export function setEditMode(mode) {
  if (!['view', 'select', 'sculpt', 'paint'].includes(mode) || G.mode === mode) return;
  G.mode = mode;
  G.picked = true;
  renderBar();
  sync();
}
