/*
 * The map painter's detail layer: where a prop's mesh comes from (the base
 * scene, another scene, a PROT coordinate, or a glTF model imported into the
 * pack's assets/), its animators and light, and the map's decorations,
 * brushes, edge runs, lighting, animated materials and fog.
 *
 * The fields are the TOML of docs/formats/expansion-pack-maps.md one to one:
 * a prop's mesh and light as forms, the list sections as JSON editors
 * (parsed on every change, the error shown in place), so whatever a modder
 * writes by hand round-trips through the page.
 */
import { h, grid } from './expansion-workbench-forms.js';

const MESH_KINDS = [
  ['object', 'base scene mesh'],
  ['scene', 'another scene\'s mesh'],
  ['tmd', 'PROT coordinate'],
  ['asset', 'glTF model (pack asset)'],
];

function meshKind(m) {
  if (!m) return 'object';
  if (m.asset != null) return 'asset';
  if (m.scene != null) return 'scene';
  if (m.tmd != null) return 'tmd';
  return 'object';
}

/** Import a .glb / .gltf / .png file into the pack's assets/models/. */
function importButton(ctx, onDone) {
  const input = h('input', { type: 'file', accept: '.glb,.gltf,.png', 'aria-label': 'Import a model' });
  input.addEventListener('change', async () => {
    const f = input.files && input.files[0];
    if (!f || !ctx.addAsset) return;
    const bytes = new Uint8Array(await f.arrayBuffer());
    const path = 'models/' + f.name.replace(/[^A-Za-z0-9._-]/g, '_');
    ctx.addAsset(path, bytes);
    onDone(path);
  });
  return h('label', { class: 'xw-field' }, h('span', {}, 'Import glTF', h('span', { class: 'xw-hint' }, ' into assets/models/ (stays in your browser)')), input);
}

/** A prop's mesh source: kind picker plus the fields of that kind.
 * `basePicker(owner, key)` is the painter's base-pack mesh list. */
export function propMeshEditor(ctx, mctx, prop, path, basePicker) {
  const box = h('div', { 'data-path': `${path}.mesh` });
  const kind = meshKind(prop.mesh);
  const sel = h('select', { 'aria-label': 'Mesh source' }, ...MESH_KINDS.map(([k, l]) => h('option', { value: k, selected: k === kind }, l)));
  sel.addEventListener('change', () => {
    const k = sel.value;
    prop.mesh = k === 'scene' ? { scene: 'town01', object: 0 }
      : k === 'tmd' ? { tmd: { kind: 'tmd', prot: 0, index: 0 } }
      : k === 'asset' ? { asset: (ctx.assetPaths ? ctx.assetPaths().find((p) => /\.(glb|gltf)$/i.test(p)) : '') || '' }
      : { object: 0 };
    mctx.changed(true);
  });
  box.append(h('label', { class: 'xw-field' }, h('span', {}, 'Mesh from', h('span', { class: 'xw-hint' }, ' another scene\'s textures are copied into free VRAM at load')), sel));
  const m = prop.mesh || {};
  if (kind === 'object') box.append(basePicker(m, 'object'));
  if (kind === 'scene') box.append(grid(mctx, m, [{ k: 'scene', t: 'scenename', label: 'Scene' }, { k: 'object', t: 'int', label: 'Mesh index', hint: 'in that scene\'s geometry pack' }], `${path}.mesh`));
  if (kind === 'tmd') box.append(grid(mctx, m.tmd, [{ k: 'prot', t: 'int', label: 'PROT entry' }, { k: 'index', t: 'int', label: 'Mesh index' }], `${path}.mesh.tmd`));
  if (kind === 'asset') {
    const paths = ctx.assetPaths ? ctx.assetPaths().filter((p) => /\.(glb|gltf)$/i.test(p)) : [];
    box.append(grid(mctx, m, [
      paths.length ? { k: 'asset', t: 'sel', opts: paths, label: 'Model' } : { k: 'asset', label: 'Model path', hint: 'relative to assets/' },
      { k: 'colors', t: 'osel', num: true, opts: [['16', '16 colours (4bpp)'], ['256', '256 colours (8bpp)']], label: 'Palette', none: '256 (default)' },
    ], `${path}.mesh`));
    box.append(importButton(ctx, (p) => { m.asset = p; mctx.changed(true); }));
  }
  return box;
}

/** A JSON editor for `owner[key]` (a list or table section). */
function jsonEditor(mctx, owner, key, label, hint, empty) {
  const ta = h('textarea', { rows: 6, spellcheck: 'false', class: 'xw-json', 'aria-label': label });
  ta.value = JSON.stringify(owner[key] == null ? empty : owner[key], null, 1);
  const err = h('div', { class: 'xw-note' });
  ta.addEventListener('change', () => {
    try {
      const v = JSON.parse(ta.value || 'null');
      if (v == null || (Array.isArray(v) && !v.length)) delete owner[key];
      else owner[key] = v;
      err.textContent = '';
      mctx.changed(false);
    } catch (e) {
      err.textContent = 'Not JSON: ' + (e && e.message ? e.message : e);
    }
  });
  return h('div', { class: 'xw-field is-wide', 'data-path': key }, h('span', {}, label, h('span', { class: 'xw-hint' }, ' ' + hint)), ta, err);
}

/** A prop's animators and light. */
export function propDetail(mctx, prop, path) {
  const box = h('div', { 'data-path': path });
  box.append(jsonEditor(mctx, prop, 'animate', 'Animators', 'spin / sway / bob / orbit / pulse, e.g. [{"kind":"spin","period":300}]', []));
  const on = h('input', { type: 'checkbox' });
  on.checked = !!prop.light;
  on.addEventListener('change', () => {
    prop.light = on.checked ? { color: [255, 176, 96], radius: 640, height: 96, flicker: 0, period: 24 } : undefined;
    if (!on.checked) delete prop.light;
    mctx.changed(true);
  });
  box.append(h('label', { class: 'xw-field xw-check' }, on, h('span', {}, 'Carries a light', h('span', { class: 'xw-hint' }, ' enhanced lighting only'))));
  if (prop.light) {
    box.append(grid(mctx, prop.light, [
      { k: 'color', t: 'tuple', n: 3, label: 'Colour', hint: 'r g b, 0..255' },
      { k: 'radius', t: 'int', label: 'Reach' },
      { k: 'height', t: 'int', label: 'Height', hint: 'above the prop' },
      { k: 'flicker', t: 'float', label: 'Flicker 0..1' },
      { k: 'period', t: 'int', label: 'Flicker period (frames)' },
    ], `${path}.light`));
  }
  return box;
}

/** The map's detail sections, and quick adds for a brush and a lamp. */
export function renderDetail(ctx, mctx, side, map) {
  const old = side.querySelector('.xw-detailbox');
  if (old) old.remove();
  const box = h('div', { class: 'xw-box xw-detailbox' });
  const [w, hgt] = [Math.max(0, ...map.tiles.rows.map((r) => [...r].length)), map.tiles.rows.length];
  box.append(h('div', { class: 'xw-box-head' }, 'Detail layer', h('span', { class: 'xw-sp' }),
    h('button', {
      type: 'button', class: 'wb-mini', onclick: () => {
        (map.scatter = map.scatter || []).push({ id: `brush_${map.scatter.length + 1}`, rect: [0, 0, Math.max(0, w - 1), Math.max(0, hgt - 1)], density: 0.3, seed: Date.now() % 100000, jitter: 40, scale: [0.6, 1.0], meshes: [{ mesh: { object: 0 }, weight: 1 }] });
        ctx.changed(true);
      },
    }, '+ Scatter brush'),
    h('button', {
      type: 'button', class: 'wb-mini', onclick: () => {
        map.lighting = map.lighting || {};
        (map.lighting.lamps = map.lighting.lamps || []).push({ tile: [Math.floor(w / 2), Math.floor(hgt / 2)], offset: [0, 96, 0], color: [255, 176, 96], radius: 640 });
        ctx.changed(true);
      },
    }, '+ Lamp')));
  box.append(h('p', { class: 'xw-note' }, 'Decorations draw only and stack any number a tile; they bake into one mesh per tile when the map loads. A brush is seeded, so the same file paints the same floor.'));
  box.append(jsonEditor(mctx, map, 'decor', 'Decorations', '[{mesh, tile, offset, rotation, scale}]', []));
  box.append(jsonEditor(mctx, map, 'scatter', 'Scatter brushes', '[{rect, meshes, density, seed, jitter, yaw, scale, tilt, on, keep_clear}]', []));
  box.append(jsonEditor(mctx, map, 'edges', 'Edge runs', '[{mesh, walls, on, every, phase, inset, y, yaw, scale}]', []));
  box.append(jsonEditor(mctx, map, 'lighting', 'Lighting', '{mood: day|dusk|night|cave, lamps: [...]}', {}));
  box.append(jsonEditor(mctx, map, 'materials', 'Animated materials', '[{kind: cycle|hsv|scroll, ...}]', []));
  box.append(jsonEditor(mctx, map, 'fog', 'Fog', '{regions: [{rect, angle, spread, speed}]}', {}));
  if (ctx.assetPaths) {
    const paths = ctx.assetPaths();
    box.append(h('p', { class: 'xw-note' }, paths.length ? `Pack assets: ${paths.join(', ')}` : 'No pack assets yet: import a glTF model on a prop to add one.'));
  }
  side.append(box);
}
