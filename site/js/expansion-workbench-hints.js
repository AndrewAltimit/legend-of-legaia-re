/* Expansion workbench: authoring hints.
 *
 * Advice on a map that is not a defect, so the validator does not carry it
 * (a pack with hints still validates with no warning): each hint reads like
 * a check - `{ severity: 'hint', path, field, message, kind }` - and lists
 * under the Checks as its own group, a click selecting what it names.
 *
 *   - **walk-through** (`kind: 'walk'`): a large prop or decoration the
 *     player walks through - a prop not `solid`, with no standable `top`,
 *     on a tile its style leaves open; a decoration (which never blocks) on
 *     an open tile. "Large" is the mesh's box from the base scene's
 *     geometry pack (`scene_objects`' `size`, times the scale): wider than
 *     half a tile both ways and taller than a step, standing on the floor.
 *     A glTF or PROT mesh has no size here and is not judged.
 *   - **text** (`kind: 'text'`): walk-on dialogue. A dialog trigger that is
 *     not `press` stops the player when they step in, and one that is not
 *     `once` as well does so on every visit: each of those is a hint to make
 *     it a `press` (examine) trigger or a `toast` (a line that stops
 *     nothing). A map with more than `TEXT_BUSY` walk-on dialogs, once or
 *     not, gets one hint of its own.
 */

/** Walk-on dialogs on one map before the map itself is flagged as busy. */
export const TEXT_BUSY = 3;
/** Half a tile: a mesh wider than this both ways reads as solid. */
const WIDE = 64;
/** A step: a mesh taller than this is not walked over. */
const TALL = 48;

const QBIT = { tl: 1, tr: 2, bl: 4, br: 8 };

function wallMask(st) {
  if (!st || st.walls == null || st.walls === 'none') return 0;
  if (st.walls === 'all') return 15;
  return (st.walls || []).reduce((m, q) => m | (QBIT[q] || 0), 0);
}

function charAt(map, t) {
  const row = (map.tiles && map.tiles.rows && map.tiles.rows[t[1]]) || '';
  return [...row][t[0]];
}

/** The base scene a map's base-pack mesh indices name. */
export function baseSceneOf(map) {
  const s = map.base && map.base.scene;
  return s && s.kind === 'scene' && s.name ? s.name : null;
}

/* A mesh's box `[x, y, z]` from the geometry packs `objectsFor` has, or
 * null when it is not known (an asset, a PROT mesh, a pack not loaded). */
function meshSize(mesh, base, objectsFor) {
  if (!mesh || typeof mesh !== 'object') return null;
  let scene = null, index = null;
  if (mesh.object != null && mesh.scene == null) { scene = base; index = mesh.object; }
  else if (mesh.object != null && mesh.scene) { scene = typeof mesh.scene === 'string' ? mesh.scene : (mesh.scene.name || null); index = mesh.object; }
  if (!scene || index == null) return null;
  const objs = objectsFor(scene);
  const o = objs && objs.objects && objs.objects.find((x) => x.index === index);
  return o ? o.size : null;
}

/** The scenes whose geometry packs the walk-through hint reads for `pack`. */
export function hintScenes(pack) {
  const out = new Set();
  for (const map of Object.values((pack && pack.maps) || {})) {
    const base = baseSceneOf(map);
    for (const o of [...(map.props || []), ...(map.decor || [])]) {
      const m = o.mesh || {};
      if (m.object != null && m.scene == null && base) out.add(base);
      else if (m.object != null && m.scene) out.add(typeof m.scene === 'string' ? m.scene : m.scene.name);
    }
  }
  out.delete(undefined);
  out.delete(null);
  return [...out];
}

/* Whether a mesh box at `scale`, lifted `lift` off the floor, blocks. */
function large(size, scale, lift) {
  if (!size) return false;
  const s = scale || 1;
  return size[0] * s >= WIDE && size[2] * s >= WIDE && size[1] * s >= TALL && (lift || 0) < TALL;
}

/** The walk-through objects of `map`: `[{ type, i, size }]`. */
export function walkThrough(map, objectsFor) {
  const base = baseSceneOf(map);
  const legend = (map.tiles && map.tiles.legend) || {};
  const out = [];
  const open = (t) => wallMask(legend[charAt(map, t)]) !== 15;
  (map.props || []).forEach((p, i) => {
    if (p.solid || p.top != null || !p.tile || !open(p.tile)) return;
    const size = meshSize(p.mesh, base, objectsFor);
    if (large(size, p.scale, (p.offset || [])[1])) out.push({ type: 'props', i, size });
  });
  (map.decor || []).forEach((d, i) => {
    if (!d.tile || !open(d.tile)) return;
    const size = meshSize(d.mesh, base, objectsFor);
    if (large(size, d.scale, (d.offset || [])[1])) out.push({ type: 'decor', i, size });
  });
  return out;
}

/** The walk-on dialog triggers of `map`: `[{ i, repeats }]`. */
export function walkOnDialogs(map) {
  const out = [];
  (map.triggers || []).forEach((t, i) => {
    const kind = t.event && t.event.kind;
    if (kind === 'dialog' && !t.press) out.push({ i, repeats: !t.once });
  });
  return out;
}

/** Every hint over `pack`; `objectsFor(scene)` is the geometry pack of a
 * scene when it is loaded, else null (that scene's meshes are skipped). */
export function authoringHints(pack, objectsFor) {
  const hints = [];
  const get = objectsFor || (() => null);
  for (const [id, map] of Object.entries((pack && pack.maps) || {})) {
    const path = `maps/${id}.toml`;
    for (const w of walkThrough(map, get)) {
      const o = (w.type === 'props' ? map.props : map.decor)[w.i];
      const what = w.type === 'props' ? 'prop' : 'decoration';
      const box = `${Math.round(w.size[0] * (o.scale || 1))} x ${Math.round(w.size[2] * (o.scale || 1))}, ${Math.round(w.size[1] * (o.scale || 1))} tall`;
      hints.push({
        severity: 'hint', kind: 'walk', path, field: `${w.type}[${w.i}]`,
        message: w.type === 'props'
          ? `the player walks through ${what} ${o.id || '#' + w.i} (${box}): tick Solid, give it a standable top, or stand it on a wall tile`
          : `the player walks through ${what} ${o.id || '#' + w.i} (${box}): decorations never block - make it a solid prop, or put it on a wall tile`,
      });
    }
    const talk = walkOnDialogs(map);
    for (const t of talk) {
      if (!t.repeats) continue;
      const tr = map.triggers[t.i];
      hints.push({
        severity: 'hint', kind: 'text', path, field: `triggers[${t.i}]`,
        message: `walk-on dialog ${tr.id || '#' + t.i} stops the player on every visit: make it On the action button (an examine), a toast, or Once ever`,
      });
    }
    if (talk.length > TEXT_BUSY) {
      hints.push({
        severity: 'hint', kind: 'text', path, field: 'triggers',
        message: `${talk.length} walk-on dialogs on this map: walking about keeps stopping for text; keep the story beats and make the rest examines or toasts`,
      });
    }
  }
  return hints;
}
