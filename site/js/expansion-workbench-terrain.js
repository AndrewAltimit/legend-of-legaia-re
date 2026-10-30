/*
 * The map painter's terrain layer: `[terrain]` of
 * docs/formats/expansion-pack-maps.md (Terrain) - the sculpted floor, its
 * ground materials, water and horizon.
 *
 * The sculpt brush writes the file's own strokes: a click adds one shape
 * (raise / lower a round brush, smooth, flatten to the ground's mean under
 * it), a drag with raise or lower adds a ridge along the drag. Nothing is
 * rasterised in the page - the heights the painter shades are the ones the
 * engine computes (`map_preview`'s `terrain`, the same heightfield the
 * floor sampler and the meshes are built from), so the relief on the
 * canvas is the floor the player walks.
 *
 * Materials, cliff, water and sky are TOML tables edited as JSON in place.
 */
import { h } from './expansion-workbench-forms.js';

export const TERRAIN_TOOL = ['sculpt', 'Sculpt', 'K'];

const MODES = [
  ['raise', 'Raise'], ['lower', 'Lower'], ['smooth', 'Smooth'], ['flatten', 'Flatten'],
];

// Brush state survives the tab re-rendering.
const B = { mode: 'raise', radius: 2.5, height: 24, strength: 1 };

/** A fresh `[terrain]` table: the engine's defaults, no strokes. */
export function defaultTerrain() {
  return {
    resolution: 4, smooth: 2, max_step: 40, seed: 0, skirt: 4,
    variation: 0.15, blend: 0.5, sun: [135, 50], shading: 0.5,
    shapes: [], materials: [], cliff: null, water: null, sky: null,
  };
}

/** Relief shading of the preview's heightfield over the painted tiles. */
export function drawTerrain(g, map, pv, cs) {
  const t = pv && pv.terrain;
  if (!map.terrain || !t || !t.heights) return;
  const step = cs / t.res;
  const off = t.skirt * t.res;
  const at = (i, j) => t.heights[Math.max(0, Math.min(t.rows - 1, j)) * t.cols + Math.max(0, Math.min(t.cols - 1, i))];
  let lo = Infinity, hi = -Infinity;
  for (const v of t.heights) { if (v < lo) lo = v; if (v > hi) hi = v; }
  const span = Math.max(1, hi - lo);
  const iw = (t.cols - 1 - 2 * off), jh = (t.rows - 1 - 2 * off);
  for (let j = 0; j < jh; j++) {
    for (let i = 0; i < iw; i++) {
      const I = i + off, J = j + off;
      const hv = at(I, J);
      // A sun from the top-left: the slope toward it lights.
      const dx = at(I + 1, J) - at(I - 1, J);
      const dz = at(I, J + 1) - at(I, J - 1);
      const lit = Math.max(-1, Math.min(1, -(dx + dz) / 48));
      const k = (hv - lo) / span;
      if (t.water != null && hv < t.water) {
        g.fillStyle = `rgba(40,90,160,${0.35 + Math.min(0.4, (t.water - hv) / 120)})`;
      } else if (lit >= 0) {
        g.fillStyle = `rgba(255,255,255,${0.08 + 0.3 * lit + 0.12 * k})`;
      } else {
        g.fillStyle = `rgba(0,0,0,${0.08 - 0.35 * lit})`;
      }
      g.fillRect(i * step, j * step, Math.ceil(step), Math.ceil(step));
    }
  }
}

/** The shape one stroke adds, from its start and end in tile units. */
function strokeShape(from, to) {
  const at = [round(from[0]), round(from[1])];
  const moved = Math.hypot(to[0] - from[0], to[1] - from[1]) > 0.5;
  const signed = B.mode === 'lower' ? -Math.abs(B.height) : Math.abs(B.height);
  if ((B.mode === 'raise' || B.mode === 'lower') && moved) {
    return { kind: 'ridge', from: at, to: [round(to[0]), round(to[1])], radius: B.radius, height: signed };
  }
  if (B.mode === 'raise' || B.mode === 'lower') return { kind: 'raise', at, radius: B.radius, height: signed };
  if (B.mode === 'smooth') return { kind: 'smooth', at, radius: B.radius, strength: B.strength };
  return { kind: 'flatten', at, radius: B.radius, strength: B.strength };
}

const round = (v) => Math.round(v * 4) / 4;

/** Start a sculpt stroke at fractional tile `(x, z)`; `null` with no terrain. */
export function sculptDown(map, x, z) {
  if (!map.terrain) return null;
  return { sculpt: true, from: [x, z], to: [x, z] };
}

/** Finish a sculpt stroke: append its shape. Returns whether it did. */
export function sculptUp(map, d) {
  if (!d || !d.sculpt || !map.terrain) return false;
  (map.terrain.shapes || (map.terrain.shapes = [])).push(strokeShape(d.from, d.to));
  return true;
}

function num(label, value, onSet, attrs = {}) {
  const i = h('input', { type: 'number', value, ...attrs, 'aria-label': label });
  i.addEventListener('change', () => { const v = parseFloat(i.value); if (Number.isFinite(v)) onSet(v); });
  return h('label', { class: 'xw-field' }, h('span', {}, label), i);
}

function jsonBox(mctx, owner, key, label, hint, empty) {
  const ta = h('textarea', { rows: 5, spellcheck: 'false', class: 'xw-json', 'aria-label': label });
  ta.value = JSON.stringify(owner[key] == null ? empty : owner[key], null, 1);
  const err = h('div', { class: 'xw-note' });
  ta.addEventListener('change', () => {
    try {
      const v = JSON.parse(ta.value || 'null');
      owner[key] = v;
      err.textContent = '';
      mctx.changed(true);
    } catch (e) {
      err.textContent = String(e.message || e);
    }
  });
  return h('label', { class: 'xw-field' }, h('span', {}, label, h('span', { class: 'xw-hint' }, ` ${hint}`)), ta, err);
}

/** Import a grayscale PNG as a `heightmap` stroke: the file goes under
 * `assets/terrain/`, the stroke names it, black at `Low` and white at
 * `High`, stretched over the whole map. */
function heightmapImport(ctx, mctx, t) {
  const st = { lo: 0, hi: 160 };
  const file = h('input', { type: 'file', accept: 'image/png', 'aria-label': 'Heightmap PNG' });
  const note = h('span', { class: 'xw-hint' }, '');
  file.addEventListener('change', async () => {
    const f = file.files && file.files[0];
    if (!f) return;
    const bytes = new Uint8Array(await f.arrayBuffer());
    const name = f.name.replace(/[^A-Za-z0-9._-]/g, '_').toLowerCase();
    const path = `terrain/${name.endsWith('.png') ? name : name + '.png'}`;
    if (mctx.snapshot) mctx.snapshot();
    ctx.addAsset(path, bytes);
    (t.shapes || (t.shapes = [])).push({ kind: 'heightmap', image: path, height: [st.lo, st.hi], strength: 1 });
    note.textContent = ` added assets/${path}`;
    ctx.changed(true);
  });
  return h('div', { class: 'xw-tools' }, h('span', { class: 'wb-label' }, 'Heightmap'), file,
    num('Low', st.lo, (v) => { st.lo = v; }, { step: 8 }), num('High', st.hi, (v) => { st.hi = v; }, { step: 8 }), note);
}

/** The side panel: terrain on/off, the brush, the water level, the tables. */
export function renderTerrain(ctx, mctx, side, map) {
  const box = h('details', { class: 'xw-box', open: map.terrain ? true : null });
  box.append(h('summary', {}, 'Terrain'));
  const on = h('input', { type: 'checkbox' });
  on.checked = !!map.terrain;
  on.addEventListener('change', () => {
    map.terrain = on.checked ? defaultTerrain() : null;
    ctx.changed(true);
  });
  box.append(h('label', { class: 'xw-check xw-field' }, on, 'Sculpted floor (the tiers become its base; walls follow steep slopes and deep water)'));
  if (!map.terrain) {
    side.append(box);
    return;
  }
  const t = map.terrain;
  const mode = h('select', { 'aria-label': 'Brush' }, ...MODES.map(([k, l]) => h('option', { value: k, selected: k === B.mode }, l)));
  mode.addEventListener('change', () => { B.mode = mode.value; });
  box.append(h('p', { class: 'xw-note' }, 'Pick the Sculpt tool (K): click to stamp the brush, drag with Raise or Lower to lay a ridge. Each stroke is one entry of shapes, applied in order.'));
  box.append(h('div', { class: 'xw-tools' },
    h('label', { class: 'xw-field' }, h('span', {}, 'Brush'), mode),
    num('Radius (tiles)', B.radius, (v) => { B.radius = Math.max(0.25, v); }, { step: 0.25, min: 0.25 }),
    num('Height', B.height, (v) => { B.height = v; }, { step: 2 }),
    num('Strength', B.strength, (v) => { B.strength = Math.max(0, Math.min(1, v)); }, { step: 0.1, min: 0, max: 1 })));
  const undoStroke = h('button', {
    type: 'button', class: 'wb-mini', disabled: !(t.shapes || []).length,
    onclick: () => { t.shapes.pop(); ctx.changed(true); },
  }, 'Remove last stroke');
  box.append(h('div', { class: 'xw-tools' }, undoStroke,
    h('span', { class: 'xw-hint' }, ` ${(t.shapes || []).length} strokes`)));
  box.append(h('div', { class: 'xw-tools' },
    num('Resolution', t.resolution, (v) => { t.resolution = [1, 2, 4].includes(v) ? v : 4; ctx.changed(true); }, { min: 1, max: 4 }),
    num('Smooth passes', t.smooth, (v) => { t.smooth = Math.max(0, Math.round(v)); ctx.changed(true); }, { min: 0, max: 16 }),
    num('Walkable step', t.max_step, (v) => { t.max_step = Math.max(1, v); ctx.changed(true); }, { min: 1 }),
    num('Skirt (tiles)', t.skirt, (v) => { t.skirt = Math.max(0, Math.min(12, Math.round(v))); ctx.changed(true); }, { min: 0, max: 12 })));
  box.append(h('div', { class: 'xw-tools' },
    num('Boundary detail', t.detail || 1, (v) => { t.detail = [1, 2, 4].includes(v) ? v : 1; ctx.changed(true); }, { min: 1, max: 4, title: 'Sub-quads a side on a material boundary: 1, 2 or 4 (finer edges, more triangles)' })));
  box.append(heightmapImport(ctx, mctx, t));
  const water = h('input', { type: 'checkbox' });
  water.checked = !!t.water;
  water.addEventListener('change', () => {
    t.water = water.checked ? { level: -8, cells: [{ tpage: 9, clut: 31880, tile: 56 }] } : null;
    ctx.changed(true);
  });
  box.append(h('div', { class: 'xw-tools' }, h('label', { class: 'xw-check xw-field' }, water, 'Water'),
    t.water ? num('Water level', t.water.level, (v) => { t.water.level = v; ctx.changed(true); }, { step: 2 }) : ''));
  box.append(jsonBox(mctx, t, 'materials', 'Materials', '[{ name, chars, cells: [{ tpage, clut, tile }], tint, rotate, lift, roughness }]', []));
  box.append(jsonBox(mctx, t, 'cliff', 'Cliff', '{ cells, slope, tint } - the texture of steep faces', null));
  if (t.water) box.append(jsonBox(mctx, t, 'water', 'Water table', '{ level, cells, shallow, deep, depth, foam, foam_color, wade, surf }', null));
  box.append(jsonBox(mctx, t, 'sky', 'Horizon', '{ tpage, clut, rect, radius, span, segments, repeat, color }', null));
  box.append(jsonBox(mctx, t, 'shapes', 'Shapes', 'raise, ridge, plateau, ramp, flatten, smooth, noise', []));
  side.append(box);
}
