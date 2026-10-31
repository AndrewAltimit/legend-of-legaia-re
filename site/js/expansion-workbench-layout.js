/* Expansion workbench: layout helpers for a map's objects.
 *
 * Pure edits of a `MapDef` over a selection (`{ type, i }`, the list the 3D
 * editor and the painter share, expansion-workbench-gizmos.js): snap to the
 * tile grid, align, distribute, repeat in a row, fill between two objects,
 * and mirror across the map's middle. Positions are in tile units - column
 * `c` spans `[c, c + 1)`, so a tile centre is `c + 0.5` - and an object
 * that carries an `offset` (props, decorations, lamps) keeps a fraction of
 * a tile in it; NPCs and spawns stand on whole tiles; a zone (`rect`)
 * moves by whole tiles.
 *
 * One prop a tile: a prop never moves onto a tile another prop holds - it
 * stays and is counted in `skipped`, as is a copy with nowhere to go. Each
 * function returns `{ moved, made, skipped, sel }`: the objects it moved,
 * the copies it made, the ones it left, and the selection afterwards.
 */

export const LAYOUT_TYPES = ['props', 'decor', 'npcs', 'spawns', 'lamps', 'emitters', 'triggers', 'exits', 'encounters'];
const OFFSET = new Set(['props', 'decor', 'lamps']);
const TURNS = new Set(['props', 'decor']);
const FACES = new Set(['npcs', 'spawns']);

function listOf(map, type, make) {
  if (type === 'lamps') {
    if (!map.lighting) { if (!make) return []; map.lighting = { lamps: [] }; }
    return map.lighting.lamps || (make ? (map.lighting.lamps = []) : []);
  }
  return map[type] || (make ? (map[type] = []) : []);
}

/** The map's size in tiles, `[width, height]`. */
export function mapSize(map) {
  const rows = (map.tiles && map.tiles.rows) || [];
  return [rows.reduce((m, r) => Math.max(m, [...r].length), 0), rows.length];
}

/** An object's position in tile units (its centre). */
export function posOf(type, o) {
  if (o.rect) {
    const q = o.rect;
    return [(Math.min(q[0], q[2]) + Math.max(q[0], q[2]) + 1) / 2, (Math.min(q[1], q[3]) + Math.max(q[1], q[3]) + 1) / 2];
  }
  const t = o.tile || [0, 0];
  const off = OFFSET.has(type) ? (o.offset || [0, 0, 0]) : [0, 0, 0];
  return [t[0] + 0.5 + (off[0] || 0) / 128, t[1] + 0.5 + (off[2] || 0) / 128];
}

const i16 = (v) => Math.max(-32768, Math.min(32767, Math.round(v)));
const clampTo = (v, n) => Math.max(0, Math.min(n - 1, v));

/* Where position `[x, z]` puts an object of `type`: its tile and offset,
 * or for a zone the whole-tile shift of its rectangle. */
function placement(map, type, o, x, z) {
  const [W, H] = mapSize(map);
  if (o.rect) {
    const [cx, cz] = posOf(type, o);
    const q = o.rect;
    let dc = Math.round(x - cx), dr = Math.round(z - cz);
    dc = Math.max(-Math.min(q[0], q[2]), Math.min(W - 1 - Math.max(q[0], q[2]), dc));
    dr = Math.max(-Math.min(q[1], q[3]), Math.min(H - 1 - Math.max(q[1], q[3]), dr));
    return { rect: [q[0] + dc, q[1] + dr, q[2] + dc, q[3] + dr], dc, dr };
  }
  const c = clampTo(Math.floor(x), W), r = clampTo(Math.floor(z), H);
  if (!OFFSET.has(type)) return { tile: [c, r] };
  const off = o.offset || [0, 0, 0];
  return { tile: [c, r], offset: [i16((x - c - 0.5) * 128), off[1] || 0, i16((z - r - 0.5) * 128)] };
}

/* The tiles props stand on, but for the selection's own (they move). */
function propTiles(map, except) {
  const out = new Set();
  listOf(map, 'props').forEach((p, i) => {
    if (!except.has(`props:${i}`) && p.tile) out.add(`${p.tile[0]},${p.tile[1]}`);
  });
  return out;
}

/* Move every `[s, x, z]` of `moves`; returns the moved and skipped counts. */
function applyMoves(map, moves) {
  const taken = propTiles(map, new Set(moves.map(([s]) => `${s.type}:${s.i}`)));
  let moved = 0, skipped = 0;
  for (const [s, x, z] of moves) {
    const o = listOf(map, s.type)[s.i];
    if (!o) continue;
    const at = placement(map, s.type, o, x, z);
    if (s.type === 'props') {
      const k = `${at.tile[0]},${at.tile[1]}`;
      if (taken.has(k)) { skipped++; if (o.tile) taken.add(`${o.tile[0]},${o.tile[1]}`); continue; }
      taken.add(k);
    }
    const before = JSON.stringify([o.tile, o.offset, o.rect]);
    if (at.rect) {
      o.rect = at.rect;
    } else {
      const dc = at.tile[0] - o.tile[0], dr = at.tile[1] - o.tile[1];
      o.tile = at.tile;
      if (at.offset) o.offset = at.offset;
      if (o.patrol && (dc || dr)) o.patrol = o.patrol.map((t) => [t[0] + dc, t[1] + dr]);
    }
    if (JSON.stringify([o.tile, o.offset, o.rect]) !== before) moved++;
  }
  return { moved, skipped };
}

function live(map, sel) {
  return (sel || []).filter((s) => s && LAYOUT_TYPES.includes(s.type) && listOf(map, s.type)[s.i]);
}

/** Put each selected object on its nearest tile centre (no offset). */
export function snapToGrid(map, sel) {
  const ss = live(map, sel);
  const r = applyMoves(map, ss.map((s) => {
    const [x, z] = posOf(s.type, listOf(map, s.type)[s.i]);
    return [s, Math.floor(x) + 0.5, Math.floor(z) + 0.5];
  }));
  return { ...r, made: 0, sel: ss };
}

/** Line the selection up on `axis` (`'x'` columns, `'z'` rows) at its
 * lowest (`'min'`), highest (`'max'`) or middle (`'mid'`) position. */
export function align(map, sel, axis, where) {
  const ss = live(map, sel);
  const k = axis === 'z' ? 1 : 0;
  const ps = ss.map((s) => posOf(s.type, listOf(map, s.type)[s.i]));
  if (ps.length < 2) return { moved: 0, made: 0, skipped: 0, sel: ss };
  const vs = ps.map((p) => p[k]);
  const to = where === 'min' ? Math.min(...vs) : where === 'max' ? Math.max(...vs) : vs.reduce((a, b) => a + b, 0) / vs.length;
  const r = applyMoves(map, ss.map((s, n) => (k ? [s, ps[n][0], to] : [s, to, ps[n][1]])));
  return { ...r, made: 0, sel: ss };
}

/** Space the selection evenly on `axis` between its first and last
 * object, in their order along it. Three objects at least. */
export function distribute(map, sel, axis) {
  const ss = live(map, sel);
  const k = axis === 'z' ? 1 : 0;
  if (ss.length < 3) return { moved: 0, made: 0, skipped: 0, sel: ss };
  const items = ss.map((s) => ({ s, p: posOf(s.type, listOf(map, s.type)[s.i]) })).sort((a, b) => a.p[k] - b.p[k]);
  const a = items[0].p[k], b = items[items.length - 1].p[k];
  const r = applyMoves(map, items.map((it, n) => {
    const v = a + (b - a) * n / (items.length - 1);
    return k ? [it.s, it.p[0], v] : [it.s, v, it.p[1]];
  }));
  return { ...r, made: 0, sel: ss };
}

/** Move the selection by `dc` columns and `dr` rows (tile units; a
 * fraction moves only what carries an offset, a zone and an NPC by whole
 * tiles). The arrow keys' nudge in the painter and the 3D editor. */
export function nudge(map, sel, dc, dr) {
  const ss = live(map, sel);
  const [W, H] = mapSize(map);
  let off = 0;
  const moves = [];
  for (const s of ss) {
    const o = listOf(map, s.type)[s.i];
    const [x, z] = posOf(s.type, o);
    // Off the map's edge the object stays where it is.
    const inside = o.rect
      ? Math.min(o.rect[0], o.rect[2]) + Math.round(dc) >= 0 && Math.max(o.rect[0], o.rect[2]) + Math.round(dc) < W
        && Math.min(o.rect[1], o.rect[3]) + Math.round(dr) >= 0 && Math.max(o.rect[1], o.rect[3]) + Math.round(dr) < H
      : x + dc >= 0 && x + dc < W && z + dr >= 0 && z + dr < H;
    if (inside) moves.push([s, x + dc, z + dr]); else off++;
  }
  const r = applyMoves(map, moves);
  return { ...r, skipped: r.skipped + off, made: 0, sel: ss };
}

function freshId(stem, used) {
  const base = String(stem || 'obj').replace(/_\d+$/, '');
  let n = 1;
  while (used.has(`${base}_${n}`)) n++;
  used.add(`${base}_${n}`);
  return `${base}_${n}`;
}

/* Copies of `s` at each of `spots` (`[x, z]`, tile units), turned by
 * `turn(o)` when given. A prop copy whose tile holds a prop is skipped. */
function copyAt(map, s, spots, turn) {
  const src = listOf(map, s.type)[s.i];
  const list = listOf(map, s.type, true);
  const used = new Set(list.map((x) => x.id).filter(Boolean));
  const taken = propTiles(map, new Set());
  const [W, H] = mapSize(map);
  const made = [];
  let skipped = 0;
  for (const [x, z] of spots) {
    if (x < 0 || z < 0 || x >= W || z >= H) { skipped++; continue; }
    const o = JSON.parse(JSON.stringify(src));
    if (o.id) o.id = freshId(o.id, used);
    const at = placement(map, s.type, o, x, z);
    if (at.rect) {
      o.rect = at.rect;
      if (o.patrol) o.patrol = o.patrol.map((t) => [t[0] + at.dc, t[1] + at.dr]);
    } else {
      if (s.type === 'props') {
        const k = `${at.tile[0]},${at.tile[1]}`;
        if (taken.has(k)) { skipped++; continue; }
        taken.add(k);
      }
      const dc = at.tile[0] - (o.tile || [0, 0])[0], dr = at.tile[1] - (o.tile || [0, 0])[1];
      o.tile = at.tile;
      if (at.offset) o.offset = at.offset;
      if (o.patrol) o.patrol = o.patrol.map((t) => [t[0] + dc, t[1] + dr]);
    }
    if (turn) turn(o);
    list.push(o);
    made.push({ type: s.type, i: list.length - 1 });
  }
  return { made, skipped };
}

/** `count` copies of each selected object, each `step` (`[dx, dz]`, in
 * tiles) on from the one before: a colonnade from one pillar. */
export function repeatRow(map, sel, count, step) {
  const ss = live(map, sel);
  const n = Math.max(0, Math.min(64, Math.floor(count) || 0));
  const out = [];
  let skipped = 0;
  for (const s of ss) {
    const [x, z] = posOf(s.type, listOf(map, s.type)[s.i]);
    const spots = Array.from({ length: n }, (_, k) => [x + step[0] * (k + 1), z + step[1] * (k + 1)]);
    const r = copyAt(map, s, spots);
    out.push(...r.made);
    skipped += r.skipped;
  }
  return { moved: 0, made: out.length, skipped, sel: ss.concat(out) };
}

/** Fill the line between two selected objects with copies of the first so
 * that `total` stand evenly along it, both ends included: six pillars
 * along a wall from the first and the last. */
export function fillBetween(map, sel, total) {
  const ss = live(map, sel);
  if (ss.length !== 2) return { moved: 0, made: 0, skipped: 0, sel: ss };
  const n = Math.max(2, Math.min(64, Math.floor(total) || 2));
  const a = posOf(ss[0].type, listOf(map, ss[0].type)[ss[0].i]);
  const b = posOf(ss[1].type, listOf(map, ss[1].type)[ss[1].i]);
  const spots = [];
  for (let k = 1; k < n - 1; k++) spots.push([a[0] + (b[0] - a[0]) * k / (n - 1), a[1] + (b[1] - a[1]) * k / (n - 1)]);
  const r = copyAt(map, ss[0], spots);
  return { moved: 0, made: r.made.length, skipped: r.skipped, sel: ss.concat(r.made) };
}

/* The turn a mirror on `axis` gives an object: a yaw measured from +X
 * towards +Z (the 3D editor's: a facing sector `f` is `90 - 45 f`). */
function mirrorTurn(axis) {
  return (o, type) => {
    if (TURNS.has(type) && o.rotation) {
      let y = axis === 'x' ? 180 - (o.rotation[1] || 0) : -(o.rotation[1] || 0);
      y = ((y % 360) + 540) % 360 - 180;
      o.rotation = [o.rotation[0] || 0, Math.round(y * 10) / 10 || 0, o.rotation[2] || 0];
    }
    if (FACES.has(type) && o.facing != null) {
      o.facing = (((axis === 'x' ? -o.facing : 4 - o.facing) % 8) + 8) % 8;
    }
  };
}

/** Mirror the selection across the map's middle on `axis` (`'x'` flips
 * columns, left to right; `'z'` flips rows). With `copy` the originals
 * stay and mirrored copies are made (one on the axis itself is not). */
export function mirror(map, sel, axis, copy) {
  const ss = live(map, sel);
  const [W, H] = mapSize(map);
  const k = axis === 'z' ? 1 : 0;
  const flip = (p) => (k ? [p[0], H - p[1]] : [W - p[0], p[1]]);
  const turn = mirrorTurn(axis);
  const flipPatrol = (o) => { if (o.patrol) o.patrol = o.patrol.map((t) => (k ? [t[0], H - 1 - t[1]] : [W - 1 - t[0], t[1]])); };
  if (copy) {
    const out = [];
    let skipped = 0;
    for (const s of ss) {
      const o = listOf(map, s.type)[s.i];
      const p = posOf(s.type, o);
      const q = flip(p);
      if (Math.abs(q[0] - p[0]) < 1e-6 && Math.abs(q[1] - p[1]) < 1e-6) { skipped++; continue; }
      const r = copyAt(map, s, [q], (c) => {
        turn(c, s.type);
        if (c.patrol) {
          // The copy's patrol was shifted with it; mirror the original's.
          c.patrol = JSON.parse(JSON.stringify(o.patrol));
          flipPatrol(c);
        }
      });
      out.push(...r.made);
      skipped += r.skipped;
    }
    return { moved: 0, made: out.length, skipped, sel: ss.concat(out) };
  }
  const moves = ss.map((s) => [s, ...flip(posOf(s.type, listOf(map, s.type)[s.i]))]);
  const patrols = ss.map((s) => JSON.stringify(listOf(map, s.type)[s.i].patrol || null));
  const r = applyMoves(map, moves);
  ss.forEach((s, n) => {
    const o = listOf(map, s.type)[s.i];
    turn(o, s.type);
    if (o.patrol) { o.patrol = JSON.parse(patrols[n]); flipPatrol(o); }
  });
  return { ...r, made: 0, sel: ss };
}

/** The layout buttons for a selection, as one toolbar row. `env`:
 * `{ h, sel() -> [{ type, i }], map(), run(fn) }`; `run` closes one undo
 * step around `fn(map, sel)` (which returns a result above) and reports
 * it. Disabled buttons say what they need. */
export function layoutTools(env) {
  const { h } = env;
  const n = env.sel().length;
  const box = h('div', { class: 'xw-tools xw-layout', role: 'toolbar', 'aria-label': 'Layout' });
  const b = (label, title, need, fn) => {
    const ok = n >= need;
    return h('button', { type: 'button', class: 'wb-mini', title: ok ? title : `${title} (select ${need} or more)`, disabled: !ok, onclick: () => env.run(fn) }, label);
  };
  const count = h('input', { type: 'number', min: 1, max: 64, step: 1, value: env.count || 5, class: 'xw-num', 'aria-label': 'Copies' });
  const dx = h('input', { type: 'number', min: -32, max: 32, step: 0.5, value: env.dx != null ? env.dx : 2, class: 'xw-num', 'aria-label': 'Step across (tiles)' });
  const dz = h('input', { type: 'number', min: -32, max: 32, step: 0.5, value: env.dz || 0, class: 'xw-num', 'aria-label': 'Step down (tiles)' });
  const copy = h('input', { type: 'checkbox', checked: env.mirrorCopy !== false, 'aria-label': 'Mirror as copies' });
  const remember = () => { env.count = +count.value; env.dx = +dx.value; env.dz = +dz.value; env.mirrorCopy = copy.checked; };
  for (const i of [count, dx, dz, copy]) i.addEventListener('change', remember);
  const num = (v, d) => (Number.isFinite(+v) ? +v : d);
  box.append(h('span', { class: 'wb-label' }, 'Layout'),
    b('Snap', 'Put each selected object on its tile centre (no offset)', 1, (m, s) => snapToGrid(m, s)),
    b('Align rows', 'Give the selection one row: the middle of theirs', 2, (m, s) => align(m, s, 'z', 'mid')),
    b('Align cols', 'Give the selection one column: the middle of theirs', 2, (m, s) => align(m, s, 'x', 'mid')),
    b('Spread across', 'Space the selection evenly left to right between its outer two', 3, (m, s) => distribute(m, s, 'x')),
    b('Spread down', 'Space the selection evenly top to bottom between its outer two', 3, (m, s) => distribute(m, s, 'z')),
    h('span', { class: 'xw-sep' }),
    h('label', { class: 'xw-check', title: 'How many copies (Repeat), or how many in all, ends included (Fill between)' }, 'Count ', count),
    h('label', { class: 'xw-check', title: 'Repeat: tiles between copies, across (+ right) and down (+ down)' }, 'Step ', dx, dz),
    b('Repeat', 'Copies of the selection in a row, Step apart', 1, (m, s) => repeatRow(m, s, num(count.value, 5), [num(dx.value, 2), num(dz.value, 0)])),
    b('Fill between', 'Select two objects: copies of the first so that Count stand evenly from one to the other', 2, (m, s) => (s.length === 2 ? fillBetween(m, s, num(count.value, 5)) : { moved: 0, made: 0, skipped: 0, sel: s, why: 'select exactly two' })),
    h('span', { class: 'xw-sep' }),
    b('Mirror L-R', 'Mirror across the map\'s middle column (left to right)', 1, (m, s) => mirror(m, s, 'x', copy.checked)),
    b('Mirror T-B', 'Mirror across the map\'s middle row (top to bottom)', 1, (m, s) => mirror(m, s, 'z', copy.checked)),
    h('label', { class: 'xw-check', title: 'Keep the originals and mirror copies of them' }, copy, ' as copies'));
  return box;
}

/** A one-line account of a layout result, for the status line. */
export function layoutSummary(r) {
  if (!r) return '';
  if (r.why) return `Layout: ${r.why}.`;
  const parts = [];
  if (r.moved) parts.push(`${r.moved} moved`);
  if (r.made) parts.push(`${r.made} made`);
  if (r.skipped) parts.push(`${r.skipped} left where they were (a prop already stands there, or off the map)`);
  return `Layout: ${parts.length ? parts.join(', ') : 'nothing to change'}.`;
}
