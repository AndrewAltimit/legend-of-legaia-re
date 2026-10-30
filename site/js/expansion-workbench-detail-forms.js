/*
 * The map detail layer's list editors as forms: decorations, scatter
 * brushes, edge runs, decals, animated materials and a prop's animators
 * (docs/formats/expansion-pack-maps.md, "Decorations", "Decals", "Animated
 * props", "Animated materials").
 *
 * Every field writes the TOML's own key into the map's JSON and carries its
 * validator path (`decor[2].tile`, `scatter[0].meshes[1].weight`), so a check
 * clicks through to it. A tile or rectangle can be picked on the painter's
 * canvas (`dctx.pickTile`), and a texture cell from the cells the base scene's
 * meshes sample (`dctx.cells()`, from `scene_objects`): the picker writes the
 * three words of the coordinate and never a texel.
 */
import { h, grid, field, listEd } from './expansion-workbench-forms.js';
import { helpLink } from './expansion-workbench-help.js';

const MAPS_DOC = '../formats/expansion-pack-maps.html';

const hex = (n) => '0x' + Number(n || 0).toString(16).toUpperCase().padStart(4, '0');
const pageLabel = (tp) => `page ${tp & 0x1F}${tp & 0x80 ? ' 8bpp' : ''}`;

/** A Pick button: the next click (or drag, for a rectangle) on the painter's
 * canvas writes `owner[key]`. */
function pickButton(dctx, owner, key, rect) {
  if (!dctx.pickTile) return null;
  return h('button', {
    type: 'button', class: 'wb-mini', title: rect ? 'Drag a rectangle on the painter' : 'Click a tile on the painter',
    onclick: () => dctx.pickTile({ rect, done: (q) => { owner[key] = rect ? q : [q[0], q[1]]; dctx.changed(true); } }),
  }, rect ? 'Pick on map' : 'Pick tile');
}

/** A tile / rectangle field with its Pick button beside it. */
export function tileField(dctx, owner, spec, path) {
  const f = field(dctx, owner, spec, path);
  const b = pickButton(dctx, owner, spec.k, spec.n === 4);
  if (b) f.append(b);
  return f;
}

/**
 * An `AtlasCell` (`{ tpage, clut, tile }`) at `owner[key]`: a picker of the
 * cells the base scene samples, the three words by hand, and the page's
 * 8 x 8 atlas drawn as a grid (marked where the scene samples it) to click a
 * cell on. Nothing of the texture itself is shown.
 */
export function cellField(dctx, owner, key, path, label) {
  const p = path ? `${path}.${key}` : key;
  const cell = owner[key] || (owner[key] = { tpage: 0, clut: 0, tile: 0 });
  const box = h('div', { class: 'xw-field is-wide xw-cell', 'data-path': p });
  box.append(h('span', {}, label || 'Texture cell', h('span', { class: 'xw-hint' }, ' a 32 x 32 cell of a texture page, through a palette, in the base scene\'s VRAM')));
  const cells = dctx.cells ? dctx.cells() : null;
  const set = (c) => { Object.assign(cell, c); dctx.changed(true); };
  if (cells && cells.length) {
    const sel = h('select', { 'aria-label': 'Cells the base scene samples' });
    const keyOf = (c) => `${c.tpage}/${c.clut}/${c.tile}`;
    const cur = keyOf(cell);
    sel.append(h('option', { value: '' }, cells.some((c) => keyOf(c) === cur) ? '(pick a cell the base scene uses)' : '(custom: not a cell the base scene samples)'));
    for (const c of cells.slice(0, 400)) {
      sel.append(h('option', { value: keyOf(c), selected: keyOf(c) === cur }, `${pageLabel(c.tpage)} - palette ${hex(c.clut)} - cell ${c.tile} (${c.prims} prims)`));
    }
    sel.addEventListener('change', () => {
      const c = cells.find((x) => keyOf(x) === sel.value);
      if (c) set({ tpage: c.tpage, clut: c.clut, tile: c.tile });
    });
    box.append(sel);
  } else {
    box.append(h('span', { class: 'xw-hint' }, cells ? 'the base scene\'s meshes sample no texture' : (dctx.cat ? 'reading the base scene ...' : 'load your disc to pick from the base scene')));
  }
  const words = h('div', { class: 'xw-cell-words' });
  words.append(grid(dctx, cell, [
    { k: 'tpage', t: 'int', label: 'Texpage word', hint: 'low byte' },
    { k: 'clut', t: 'int', label: 'Palette (CLUT word)' },
    { k: 'tile', t: 'int', label: 'Cell 0..63' },
  ], p));
  box.append(words);
  // The page's atlas: used cells marked, the chosen one outlined.
  const used = new Set((cells || []).filter((c) => c.tpage === cell.tpage && c.clut === cell.clut).map((c) => c.tile));
  const atlas = h('div', { class: 'xw-atlas', role: 'grid', 'aria-label': `Atlas of ${pageLabel(cell.tpage)}` });
  for (let t = 0; t < 64; t++) {
    atlas.append(h('button', {
      type: 'button', class: 'xw-atlas-c' + (used.has(t) ? ' is-used' : '') + (t === cell.tile ? ' is-on' : ''),
      title: `cell ${t} (column ${t % 8}, row ${Math.floor(t / 8)})${used.has(t) ? ', sampled by the base scene' : ''}`,
      onclick: () => set({ tile: t }),
    }));
  }
  box.append(atlas);
  return box;
}

/** A palette word picked from the palettes the base scene samples. */
function clutField(dctx, owner, path) {
  const cells = (dctx.cells && dctx.cells()) || [];
  const counts = new Map();
  for (const c of cells) counts.set(c.clut, (counts.get(c.clut) || 0) + c.prims);
  const opts = [...counts.entries()].sort((a, b) => b[1] - a[1]).map(([cl, n]) => [String(cl), `${hex(cl)} (${n} prims)`]);
  if (!opts.length) return field(dctx, owner, { k: 'clut', t: 'int', label: 'Palette (CLUT word)' }, path);
  return field(dctx, owner, { k: 'clut', t: 'sel', num: true, opts, label: 'Palette (CLUT word)', hint: 'the palettes the base scene samples' }, path);
}

/** A mesh source (`PropMesh`) at `owner.mesh`, via the painter's prop mesh editor. */
function meshBox(dctx, owner, path) {
  owner.mesh = owner.mesh || { object: 0 };
  return dctx.meshEditor ? dctx.meshEditor(owner, path) : grid(dctx, owner.mesh, [{ k: 'object', t: 'int', label: 'Mesh index' }], `${path}.mesh`);
}

/** One section: a collapsible box claiming its JSON key's path. */
function section(box, key, title, hint, n, doc) {
  const sec = h('details', { class: 'xw-box', 'data-path': key, open: null },
    h('summary', { class: 'xw-box-head' }, `${title} (${n})`, hint ? h('span', { class: 'xw-hint' }, ' ' + hint) : null, doc ? helpLink(`${MAPS_DOC}#${doc}`, title) : null));
  box.append(sec);
  return sec;
}

const freshOf = (prefix) => (copy, arr) => {
  if (!copy.id) return;
  const taken = new Set(arr.map((x) => x.id));
  let n = 2;
  while (taken.has(`${copy.id}_${n}`)) n++;
  copy.id = `${copy.id}_${n}`;
};

const RECT4 = { t: 'tuple', n: 4, names: ['col0', 'row0', 'col1', 'row1'] };
const ORECT4 = { t: 'otuple', n: 4, names: ['col0', 'row0', 'col1', 'row1'] };

/** `[[decor]]`: one mesh at a time. */
export function decorEd(dctx, box, map) {
  const sec = section(box, 'decor', 'Decorations', 'one mesh a tile at a time; draw only', (map.decor || []).length, 'decorations');
  listEd(dctx, sec, map, 'decor', '', {
    add: 'Decoration', fold: true, dup: freshOf('decor'),
    make: () => ({ id: '', mesh: { object: 0 }, tile: [0, 0], offset: [0, 0, 0], rotation: [0, 0, 0], scale: null }),
    label: (d, i) => `${d.id || `decor ${i + 1}`} at ${(d.tile || []).join(',')}`,
    render: (b, d, p) => {
      b.append(grid(dctx, d, [{ k: 'id', label: 'Id', hint: 'optional' }], p));
      b.append(meshBox(dctx, d, p));
      const g = grid(dctx, d, [
        { k: 'offset', t: 'tuple', n: 3, names: ['x', 'y', 'z'], label: 'Offset', hint: 'from the tile centre, y up' },
        { k: 'rotation', t: 'tuple', n: 3, float: true, names: ['x', 'y', 'z'], label: 'Rotation (deg)', hint: 'y is the yaw' },
        { k: 'scale', t: 'ofloat', label: 'Scale', ph: '1' },
      ], p);
      g.prepend(tileField(dctx, d, { k: 'tile', t: 'tuple', n: 2, names: ['col', 'row'], label: 'Tile' }, p));
      b.append(g);
    },
  });
}

/** `[[scatter]]`: a seeded brush over a rectangle. */
export function scatterEd(dctx, box, map) {
  const sec = section(box, 'scatter', 'Scatter brushes', 'a seeded brush: the same file paints the same floor', (map.scatter || []).length, 'decorations');
  listEd(dctx, sec, map, 'scatter', '', {
    add: 'Scatter brush', fold: true, dup: freshOf('scatter'),
    make: () => ({ id: '', rect: [0, 0, 0, 0], meshes: [{ mesh: { object: 0 }, weight: 1, scale: null, y: 0 }], density: 1, seed: 0, jitter: 40, yaw: [0, 360], scale: [1, 1], tilt: 0, on: '', keep_clear: true, cluster: null }),
    label: (s, i) => `${s.id || `brush ${i + 1}`} - ${(s.meshes || []).length} mesh(es), density ${s.density}`,
    render: (b, s, p) => {
      const g = grid(dctx, s, [
        { k: 'id', label: 'Id' },
        { k: 'density', t: 'float', label: 'Density', hint: 'meshes a tile; the fraction is a chance' },
        { k: 'seed', t: 'int', label: 'Seed' },
        { k: 'jitter', t: 'int', label: 'Jitter', hint: 'world units off the centre; 0..64 keeps the tile' },
        { k: 'yaw', t: 'tuple', n: 2, float: true, names: ['min', 'max'], label: 'Yaw range (deg)' },
        { k: 'scale', t: 'tuple', n: 2, float: true, names: ['min', 'max'], label: 'Scale range' },
        { k: 'tilt', t: 'float', label: 'Tilt (deg)', hint: 'off upright, each way' },
        { k: 'on', label: 'Paints on', hint: 'legend characters; empty: every floor tile', ph: '.,' },
        { k: 'keep_clear', t: 'dbool', label: 'Keep spawns, exits, NPCs and props clear' },
      ], p);
      g.prepend(tileField(dctx, s, Object.assign({ k: 'rect', label: 'Tiles' }, RECT4), p));
      b.append(g);
      const cp = `${p}.cluster`;
      const cb = h('div', { class: 'xw-box', 'data-path': cp });
      const on = h('input', { type: 'checkbox' });
      on.checked = s.cluster != null;
      on.addEventListener('change', () => { s.cluster = on.checked ? { centre: null, radius: 0, falloff: 2 } : null; dctx.changed(true); });
      cb.append(h('div', { class: 'xw-box-head' }, h('label', { class: 'xw-check xw-field' }, on, 'Cluster'), h('span', { class: 'xw-hint' }, 'dense at a centre, thinning to the rim')));
      if (s.cluster) {
        cb.append(grid(dctx, s.cluster, [
          { k: 'centre', t: 'otuple', n: 2, float: true, names: ['col', 'row'], label: 'Centre', hint: 'empty: the rectangle\'s' },
          { k: 'radius', t: 'float', label: 'Radius (tiles)', hint: '0: the farthest corner' },
          { k: 'falloff', t: 'float', label: 'Falloff' },
        ], cp));
      }
      b.append(cb);
      listEd(dctx, b, s, 'meshes', p, {
        title: 'Meshes', hint: 'picked by weight', add: 'Mesh', dup: true,
        make: () => ({ mesh: { object: 0 }, weight: 1, scale: null, y: 0 }),
        label: (m, i) => `mesh ${i + 1}, weight ${m.weight}`,
        render: (b2, m, p2) => {
          b2.append(meshBox(dctx, m, p2));
          b2.append(grid(dctx, m, [
            { k: 'weight', t: 'int', label: 'Weight' },
            { k: 'scale', t: 'otuple', n: 2, float: true, names: ['min', 'max'], label: 'Own scale range', hint: 'empty: the brush\'s' },
            { k: 'y', t: 'int', label: 'Height', hint: 'above the floor' },
          ], p2));
        },
      });
    },
  });
}

/** `[[edges]]`: a mesh along the floor's wall edges. */
export function edgesEd(dctx, box, map) {
  const sec = section(box, 'edges', 'Edge runs', 'a mesh repeated along the walls, facing into the room', (map.edges || []).length, 'decorations');
  listEd(dctx, sec, map, 'edges', '', {
    add: 'Edge run', fold: true, dup: freshOf('edges'),
    make: () => ({ id: '', mesh: { object: 0 }, walls: '', on: '', rect: null, every: 1, phase: 0, inset: 8, y: 0, yaw: 0, scale: null, jitter: 0, yaw_jitter: 0, scale_range: null, seed: 0 }),
    label: (e, i) => `${e.id || `run ${i + 1}`}${e.every > 1 ? `, every ${e.every}` : ''}`,
    render: (b, e, p) => {
      b.append(grid(dctx, e, [{ k: 'id', label: 'Id' }], p));
      b.append(meshBox(dctx, e, p));
      const g = grid(dctx, e, [
        { k: 'walls', label: 'Wall characters', hint: 'empty: walls, void and the border' },
        { k: 'on', label: 'Floor characters', hint: 'empty: every floor' },
        { k: 'every', t: 'int', label: 'Every Nth edge tile' },
        { k: 'phase', t: 'int', label: 'Phase' },
        { k: 'inset', t: 'int', label: 'Inset', hint: 'from the wall face' },
        { k: 'y', t: 'int', label: 'Height' },
        { k: 'yaw', t: 'float', label: 'Extra yaw (deg)' },
        { k: 'scale', t: 'ofloat', label: 'Scale', ph: '1' },
        { k: 'jitter', t: 'int', label: 'Slide jitter', hint: 'world units along the wall' },
        { k: 'yaw_jitter', t: 'float', label: 'Yaw jitter (deg)' },
        { k: 'scale_range', t: 'otuple', n: 2, float: true, names: ['min', 'max'], label: 'Scale range', hint: 'overrides scale' },
        { k: 'seed', t: 'int', label: 'Seed' },
      ], p);
      g.prepend(tileField(dctx, e, Object.assign({ k: 'rect', label: 'Only inside', hint: 'empty: the whole map' }, ORECT4), p));
      b.append(g);
    },
  });
}

/** `[[decals]]`: flat textured polygons on the floor. */
export function decalsEd(dctx, box, map) {
  const sec = section(box, 'decals', 'Decals', 'a texture cell laid flat on the floor: placed once, or brushed over a rectangle', (map.decals || []).length, 'decals');
  listEd(dctx, sec, map, 'decals', '', {
    add: 'Decal', fold: true, dup: freshOf('decals'),
    make: () => ({ id: '', cell: { tpage: 0, clut: 0, tile: 0 }, cells: [], span: 1, tile: [0, 0], rect: null, density: 0.3, seed: 0, on: '', offset: [0, 2, 0], jitter: 32, size: [96, 96], size_jitter: 0, yaw: [0, 360], sides: 4, color: [128, 128, 128], semi: false }),
    label: (d, i) => `${d.id || `decal ${i + 1}`} - ${d.rect ? 'brush' : `at ${(d.tile || []).join(',')}`}`,
    render: (b, d, p) => {
      b.append(grid(dctx, d, [{ k: 'id', label: 'Id' }], p));
      // Placed once (a tile) or a brush (a rectangle).
      const mode = h('select', { 'aria-label': 'Placement' }, h('option', { value: 'tile', selected: !d.rect }, 'placed once on a tile'), h('option', { value: 'rect', selected: !!d.rect }, 'a brush over a rectangle'));
      mode.addEventListener('change', () => {
        if (mode.value === 'rect') { d.rect = d.tile ? [d.tile[0], d.tile[1], d.tile[0], d.tile[1]] : [0, 0, 0, 0]; d.tile = null; } else { d.tile = d.rect ? [d.rect[0], d.rect[1]] : [0, 0]; d.rect = null; }
        dctx.changed(true);
      });
      b.append(h('label', { class: 'xw-field' }, h('span', {}, 'Placement'), mode));
      b.append(cellField(dctx, d, 'cell', p, 'Texture cell'));
      const g = grid(dctx, d, [
        { k: 'span', t: 'int', label: 'Span', hint: 'cells a side, right and down' },
        { k: 'size', t: 'tuple', n: 2, names: ['w', 'd'], label: 'Size', hint: 'world units' },
        { k: 'sides', t: 'int', label: 'Sides', hint: '4 rectangle, 5..16 a polygon' },
        { k: 'yaw', t: 'tuple', n: 2, float: true, names: ['min', 'max'], label: 'Yaw range (deg)', hint: 'once: the first' },
        { k: 'offset', t: 'tuple', n: 3, names: ['x', 'y', 'z'], label: 'Offset', hint: 'y up' },
        { k: 'color', t: 'tuple', n: 3, names: ['r', 'g', 'b'], label: 'Colour', hint: '128 128 128 draws the texel as it is' },
        { k: 'semi', t: 'bool', label: 'Blend with the floor' },
      ], p);
      g.prepend(d.rect
        ? tileField(dctx, d, Object.assign({ k: 'rect', label: 'Brush tiles' }, RECT4), p)
        : tileField(dctx, d, { k: 'tile', t: 'tuple', n: 2, names: ['col', 'row'], label: 'Tile' }, p));
      b.append(g);
      if (d.rect) {
        b.append(grid(dctx, d, [
          { k: 'density', t: 'float', label: 'Density', hint: '0..8 a tile' },
          { k: 'seed', t: 'int', label: 'Seed' },
          { k: 'on', label: 'Paints on', hint: 'legend characters' },
          { k: 'jitter', t: 'int', label: 'Jitter' },
          { k: 'size_jitter', t: 'float', label: 'Size jitter', hint: '1 +- this' },
        ], p));
        const cp = `${p}.cells`;
        if (!Array.isArray(d.cells)) d.cells = [];
        const cbx = h('div', { 'data-path': cp }, h('h4', { class: 'xw-sub' }, 'More cells', h('span', { class: 'xw-hint' }, ' the brush picks among these and the cell above')));
        d.cells.forEach((c, k) => {
          const row = h('div', { class: 'xw-box' });
          row.append(h('div', { class: 'xw-box-head' }, `cell ${k + 2}`, h('span', { class: 'xw-sp' }),
            h('button', { type: 'button', class: 'wb-mini', onclick: () => { d.cells.splice(k, 1); dctx.changed(true); } }, 'Remove')));
          row.append(cellField(dctx, d.cells, k, cp, `Cell ${k + 2}`));
          cbx.append(row);
        });
        cbx.append(h('button', { type: 'button', class: 'wb-mini', onclick: () => { d.cells.push(Object.assign({}, d.cell)); dctx.changed(true); } }, '+ Cell'));
        b.append(cbx);
      }
    },
  });
}

const MATERIAL_MAKE = {
  cycle: (c) => ({ kind: 'cycle', clut: c.clut || 0, start: 0, count: 16, period: 8, reverse: false }),
  hsv: (c) => ({ kind: 'hsv', clut: c.clut || 0, start: 0, count: 16, period: 120, value: 0, hue: 360 }),
  scroll: (c) => ({ kind: 'scroll', cell: { tpage: c.tpage || 0, clut: c.clut || 0, tile: c.tile || 0 }, dx: 1, dy: 0, period: 8 }),
};

/** The map's `[[materials]]` (animated texture words; the terrain's own
 * materials are the Terrain box's). */
export function materialsEd(dctx, box, map) {
  const sec = section(box, 'materials', 'Animated materials', 'palette cycling, hue cycling, texel scrolling', (map.materials || []).length, 'animated-materials');
  listEd(dctx, sec, map, 'materials', '', {
    add: 'Material', fold: true, dup: true,
    make: () => MATERIAL_MAKE.cycle(((dctx.cells && dctx.cells()) || [])[0] || {}),
    label: (m, i) => `${i + 1}. ${m.kind}${m.kind === 'scroll' ? '' : ` palette ${hex(m.clut)}`}, every ${m.period} frames`,
    render: (b, m, p) => {
      const kind = h('select', { 'aria-label': 'Kind' }, ...[['cycle', 'palette cycle'], ['hsv', 'hue and brightness cycle'], ['scroll', 'texel scroll']].map(([k, l]) => h('option', { value: k, selected: m.kind === k }, l)));
      kind.addEventListener('change', () => {
        const fresh = MATERIAL_MAKE[kind.value](m.cell || m);
        for (const k of Object.keys(m)) delete m[k];
        Object.assign(m, fresh);
        dctx.changed(true);
      });
      b.append(h('label', { class: 'xw-field', 'data-path': `${p}.kind` }, h('span', {}, 'Kind'), kind));
      const from = { k: 'from', t: 'otext', label: 'From scene', hint: 'a scene a carried-in mesh samples; empty: the base scene', ph: '(the base scene)' };
      const period = { k: 'period', t: 'int', label: 'Period', hint: 'frames a step, at least 1' };
      if (m.kind === 'scroll') {
        b.append(cellField(dctx, m, 'cell', p, 'Cell to scroll'));
        b.append(grid(dctx, m, [
          { k: 'dx', t: 'int', label: 'dx', hint: 'texels a step' },
          { k: 'dy', t: 'int', label: 'dy' },
          period, from,
          { k: 'rect', t: 'otuple', n: 4, names: ['x', 'y', 'w', 'h'], label: 'VRAM rectangle', hint: 'halfwords; empty: the cell\'s' },
        ], p));
      } else {
        const g = grid(dctx, m, [
          { k: 'start', t: 'int', label: 'First entry' },
          { k: 'count', t: 'int', label: 'Entries' },
          period,
        ].concat(m.kind === 'cycle' ? [
          { k: 'reverse', t: 'bool', label: 'Rotate the other way' },
        ] : [
          { k: 'value', t: 'int', label: 'Brightness swing', hint: '0..255' },
          { k: 'hue', t: 'float', label: 'Hue sweep (deg)', hint: '0 pulses brightness only' },
        ]).concat([from]), p);
        g.prepend(clutField(dctx, m, p));
        b.append(g);
      }
    },
  });
}

const ANIM_FIELDS = {
  spin: [{ k: 'axis', t: 'sel', opts: ['x', 'y', 'z'], label: 'Axis' }, { k: 'period', t: 'int', label: 'Period', hint: 'frames a turn; negative turns back' }],
  sway: [{ k: 'axis', t: 'sel', opts: ['x', 'y', 'z'], label: 'Axis' }, { k: 'degrees', t: 'float', label: 'Degrees', hint: 'either side' }, { k: 'period', t: 'int', label: 'Period' }],
  bob: [{ k: 'amplitude', t: 'int', label: 'Amplitude', hint: 'world units' }, { k: 'period', t: 'int', label: 'Period' }],
  orbit: [{ k: 'radius', t: 'int', label: 'Radius' }, { k: 'period', t: 'int', label: 'Period', hint: 'negative circles back' }],
  pulse: [{ k: 'amount', t: 'float', label: 'Amount', hint: 'scale 1 +- this' }, { k: 'period', t: 'int', label: 'Period' }],
};
const ANIM_MAKE = {
  spin: () => ({ kind: 'spin', axis: 'y', period: 300, phase: 0 }),
  sway: () => ({ kind: 'sway', axis: 'z', degrees: 8, period: 120, phase: 0 }),
  bob: () => ({ kind: 'bob', amplitude: 10, period: 150, phase: 0 }),
  orbit: () => ({ kind: 'orbit', radius: 32, period: 240, phase: 0 }),
  pulse: () => ({ kind: 'pulse', amount: 0.08, period: 90, phase: 0 }),
};

/** A prop's `animate` list: procedural animators, composed in order. */
export function animatorsEd(dctx, box, prop, path) {
  const sec = h('details', { class: 'xw-box', 'data-path': `${path}.animate`, open: (prop.animate || []).length ? true : null },
    h('summary', { class: 'xw-box-head' }, `Animators (${(prop.animate || []).length})`, h('span', { class: 'xw-hint' }, ' compose in order; a pure function of the frame'), helpLink(`${MAPS_DOC}#animated-props`, 'animated props')));
  listEd(dctx, sec, prop, 'animate', path, {
    add: 'Animator', fold: true, dup: true,
    make: ANIM_MAKE.spin,
    label: (a, i) => `${i + 1}. ${a.kind}, period ${a.period}`,
    render: (b, a, p) => {
      const kind = h('select', { 'aria-label': 'Kind' }, ...Object.keys(ANIM_MAKE).map((k) => h('option', { value: k, selected: a.kind === k }, k)));
      kind.addEventListener('change', () => {
        const fresh = ANIM_MAKE[kind.value]();
        fresh.period = a.period || fresh.period;
        fresh.phase = a.phase || 0;
        for (const k of Object.keys(a)) delete a[k];
        Object.assign(a, fresh);
        dctx.changed(true);
      });
      const g = grid(dctx, a, (ANIM_FIELDS[a.kind] || []).concat([{ k: 'phase', t: 'float', label: 'Phase', hint: 'fraction of the period' }]), p);
      g.prepend(h('label', { class: 'xw-field', 'data-path': `${p}.kind` }, h('span', {}, 'Kind'), kind));
      b.append(g);
    },
  });
  box.append(sec);
}
