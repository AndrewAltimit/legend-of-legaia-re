/* Expansion workbench: the object palette's pictures.
 *
 * Every picture is the object itself, off the visitor's own disc, drawn by the
 * renderer the 3D preview uses (`TmdRenderer`, the VRAM-mesh pipeline of
 * js/webgl-tmd.js): the validation worker assembles a scene and hands over
 * its VRAM and mesh streams (`scene_meshes`, crates/web-expansion), and one
 * shared WebGL context draws them one at a time into images.
 *
 * Scale: a catalog runs to hundreds of objects, so nothing here is eager. A
 * tile asks for its picture when it scrolls into view (`watch`), the queue
 * holds only what is still in view, and it runs a few milliseconds a turn on
 * the page's thread, giving the turn back between. A picture once drawn is
 * kept in IndexedDB (database `legaia-expansion-thumbs`, keyed by the disc,
 * scene and object), so a later visit asks the worker for nothing.
 *
 * The bytes are the user's: derived from their disc, in their browser. They
 * are never bundled, committed or uploaded, and a pack never holds them.
 */

const THUMB = 128;
// Bump to redraw every cached picture (a change of camera or background).
const THUMB_V = 1;
const DB = 'legaia-expansion-thumbs';
const STORE = 'thumbs';
// A turn of the queue: this long at most, then back to the page.
const BUDGET_MS = 6;
const REST_MS = 12;
// Whole scenes (their VRAM and meshes) kept parsed: the ghost's and the queue's.
const KEEP_SCENES = 4;

/** Parse `scene_meshes`' wire form (`catalog::pack_scene_meshes`). */
export function parseSceneMeshes(buf) {
  const bytes = new Uint8Array(buf);
  const dv = new DataView(buf);
  if (String.fromCharCode(bytes[0], bytes[1], bytes[2], bytes[3]) !== 'LXMS') throw new Error('not a scene mesh pack');
  const count = dv.getUint32(4, true);
  const vramLen = dv.getUint32(8, true);
  let at = 12;
  const heads = [];
  for (let i = 0; i < count; i++) {
    heads.push([dv.getUint32(at, true), dv.getUint32(at + 4, true), dv.getUint32(at + 8, true)]);
    at += 12;
  }
  const pad = () => { at = (at + 3) & ~3; };
  const vram = bytes.slice(at, at + vramLen);
  at += vramLen;
  pad();
  const meshes = heads.map(([verts, inds, flatLen]) => {
    const positions = new Float32Array(buf.slice(at, at + verts * 12));
    at += verts * 12;
    const uvs = bytes.slice(at, at + verts * 2);
    at += verts * 2;
    pad();
    const cbaTsb = new Uint16Array(buf.slice(at, at + verts * 4));
    at += verts * 4;
    const indices = new Uint32Array(buf.slice(at, at + inds * 4));
    at += inds * 4;
    const flat = bytes.slice(at, at + flatLen);
    at += flatLen;
    pad();
    return { positions, uvs, cbaTsb, indices, flat };
  });
  return { vram, meshes };
}

/** A mesh's box: `{ lo, hi, size, center, radius }`, or null when it is empty. */
export function meshBox(m) {
  const p = m.positions;
  if (!p.length || !m.indices.length) return null;
  const lo = [Infinity, Infinity, Infinity], hi = [-Infinity, -Infinity, -Infinity];
  for (let i = 0; i < p.length; i += 3) {
    for (let k = 0; k < 3; k++) {
      if (p[i + k] < lo[k]) lo[k] = p[i + k];
      if (p[i + k] > hi[k]) hi[k] = p[i + k];
    }
  }
  const size = [hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]];
  const center = [(lo[0] + hi[0]) / 2, (lo[1] + hi[1]) / 2, (lo[2] + hi[2]) / 2];
  const radius = Math.max(1, Math.hypot(size[0], size[1], size[2]) / 2);
  return { lo, hi, size, center, radius };
}

function idb() {
  return new Promise((res) => {
    let rq;
    try { rq = indexedDB.open(DB, 1); } catch (e) { res(null); return; }
    rq.onupgradeneeded = () => { rq.result.createObjectStore(STORE); };
    rq.onsuccess = () => res(rq.result);
    rq.onerror = () => res(null);
    rq.onblocked = () => res(null);
  });
}

/**
 * The picture service. `env`: `{ ask(scene) -> Promise<ArrayBuffer>, discKey()
 * -> string }`. `ask` is the worker's `scene_meshes` (or the page's own).
 */
export function createThumbs(env) {
  const T = {
    urls: new Map(),      // key -> object URL
    waits: new Map(),     // key -> { scene, object, tiles: Set<img> }
    scenes: new Map(),    // scene -> Promise<{ vram, meshes }>, oldest first
    failed: new Set(),
    r: null, canvas: null, loadedVram: null,
    timer: null, dbp: null, drawn: 0, cached: 0,
    io: null,
  };
  const db = () => T.dbp || (T.dbp = idb());
  const keyOf = (scene, object) => `${env.discKey()}|${THUMB_V}|${scene}|${object}`;

  function renderer() {
    if (T.r) return T.r;
    /* global TmdRenderer */
    T.canvas = document.createElement('canvas');
    T.canvas.width = T.canvas.height = THUMB;
    T.r = new TmdRenderer(T.canvas);
    T.r.useFlatColors = true;
    T.r.clearColor = [0.13, 0.15, 0.18, 1];
    return T.r;
  }

  /** A scene's parsed meshes and VRAM (kept for the last few scenes). */
  function scene(name) {
    let p = T.scenes.get(name);
    if (p) {
      T.scenes.delete(name);
      T.scenes.set(name, p);
      return p;
    }
    p = env.ask(name).then(parseSceneMeshes);
    T.scenes.set(name, p);
    p.catch(() => T.scenes.delete(name));
    while (T.scenes.size > KEEP_SCENES) T.scenes.delete(T.scenes.keys().next().value);
    return p;
  }

  function show(key, url) {
    const w = T.waits.get(key);
    T.waits.delete(key);
    if (w) for (const img of w.tiles) { img.src = url; img.classList.add('is-ready'); }
  }

  /* Draw one object into the shared context; the copy out runs in this task,
   * while the drawing buffer is still valid. */
  function draw(w, data) {
    const m = data.meshes[w.object];
    const box = m && meshBox(m);
    if (!box) { T.failed.add(w.key); noPicture(w); return; }
    const r = renderer();
    if (T.loadedVram !== data.vram) { r.uploadVram(data.vram); T.loadedVram = data.vram; }
    r.uploadMesh(m.positions, m.uvs, m.cbaTsb, m.indices, m.flat.length ? m.flat : null);
    // A three-quarter view from above, the whole box in frame.
    r.render(-0.6, 0.5, 1.75, 0, 0, box.center, box.radius);
    const key = w.key;
    T.canvas.toBlob((blob) => {
      if (!blob) { noPicture(w); return; }
      const url = URL.createObjectURL(blob);
      T.urls.set(key, url);
      T.drawn++;
      show(key, url);
      db().then((d) => {
        if (!d) return;
        try { d.transaction(STORE, 'readwrite').objectStore(STORE).put(blob, key); } catch (e) { /* no room: redrawn next time */ }
      });
    }, 'image/webp', 0.9);
  }

  function noPicture(w) {
    T.waits.delete(w.key);
    for (const img of w.tiles) img.closest('.xw-piece')?.classList.add('is-blank');
  }

  /* The queue: what is still in view, a scene at a time so the context keeps
   * its VRAM, one short turn at a time. */
  function pump() {
    if (T.timer || T.busy) return;
    T.timer = setTimeout(turn, REST_MS);
  }
  async function turn() {
    T.timer = null;
    T.busy = true;
    const t0 = performance.now();
    while (performance.now() - t0 < BUDGET_MS) {
      const w = nextWait();
      if (!w) break;
      w.drawing = true;
      try {
        // The scene's bytes come from the worker: waiting on them costs the
        // page nothing.
        const q = scene(w.scene);
        const data = q.ready || (q.ready = await q);
        draw(w, data);
      } catch (e) {
        // No picture (no WebGL2, a scene that does not assemble): the tile
        // keeps its name and still places.
        console.warn('palette thumbnail', w.key, e);
        T.waits.delete(w.key);
      }
    }
    T.busy = false;
    if (nextWait()) pump();
  }
  function nextWait() {
    let any = null;
    for (const w of T.waits.values()) {
      if (w.drawing || !w.looked) continue;
      for (const img of w.tiles) if (!img.isConnected) w.tiles.delete(img);
      if (!w.tiles.size) { T.waits.delete(w.key); continue; }
      if (!w.visible) continue;
      if (w.scene === T.lastScene) return w;
      if (!any) any = w;
    }
    if (any) T.lastScene = any.scene;
    return any;
  }

  /* A tile came into view: its picture from memory, the cache, or the queue. */
  function want(img) {
    const { scene: sc, object } = img.dataset;
    const key = keyOf(sc, object);
    const url = T.urls.get(key);
    if (url) { img.src = url; img.classList.add('is-ready'); return; }
    let w = T.waits.get(key);
    if (w) { w.tiles.add(img); w.visible = true; if (w.looked) pump(); return; }
    w = { key, scene: sc, object: +object, tiles: new Set([img]), visible: true };
    T.waits.set(key, w);
    db().then((d) => {
      if (!d) return null;
      return new Promise((res) => {
        const rq = d.transaction(STORE).objectStore(STORE).get(key);
        rq.onsuccess = () => res(rq.result || null);
        rq.onerror = () => res(null);
      });
    }).then((blob) => {
      w.looked = true;
      if (blob) {
        const u = URL.createObjectURL(blob);
        T.urls.set(key, u);
        T.cached++;
        show(key, u);
      } else pump();
    });
  }

  function observer() {
    if (T.io || typeof IntersectionObserver !== 'function') return T.io;
    T.io = new IntersectionObserver((es) => {
      for (const e of es) {
        const img = e.target;
        if (e.isIntersecting) {
          want(img);
        } else {
          const w = T.waits.get(keyOf(img.dataset.scene, img.dataset.object));
          if (w && !w.drawing) w.visible = false;
          if (!img.isConnected) T.io.unobserve(img);
        }
      }
    }, { rootMargin: '120px' });
    return T.io;
  }

  return {
    /** Have tile image `img` (`data-scene`, `data-object`) drawn once in view. */
    watch(img) {
      const io = observer();
      if (io) io.observe(img); else want(img);
    },
    /** One object's streams and its scene's VRAM (the ghost's): a promise. */
    mesh(sceneName, object) {
      return scene(sceneName).then((d) => {
        const m = d.meshes[object];
        return m ? { mesh: m, vram: d.vram, box: meshBox(m) } : null;
      });
    },
    /** A whole scene's objects as `{ index, size, tris }`, for browsing a
     * scene the mesh catalog has not labelled. */
    objects(sceneName) {
      return scene(sceneName).then((d) => d.meshes.map((m, index) => {
        const b = meshBox(m);
        return b ? { index, size: [Math.round(b.size[0]), Math.round(b.size[2]), Math.round(b.size[1])], tris: m.indices.length / 3 } : null;
      }).filter(Boolean));
    },
    /** Forget every picture of this browser (the disc was patched). */
    async clear() {
      T.urls.clear();
      const d = await db();
      if (d) await new Promise((res) => { const q = d.transaction(STORE, 'readwrite').objectStore(STORE).clear(); q.onsuccess = q.onerror = () => res(); });
    },
    /** Headless-verification hook. */
    stats: () => ({ drawn: T.drawn, cached: T.cached, waiting: T.waits.size, memory: T.urls.size }),
  };
}
