/* Expansion workbench: the validator off the page's main thread.
 *
 * A module worker holding a second `ExpansionWorkbench` session
 * (crates/web-expansion) that only validates. The page opens it once the
 * disc is read and hands it:
 *   - `init`: the wasm glue and binary URLs (the page's own, `?v=` and all);
 *   - `disc`: the disc files the page's session kept (`take_disc_parts`:
 *     PROT.DAT, CDNAME.TXT, SCUS_942.54), transferred, so the worker builds
 *     the same catalog without the image;
 *   - `files`: the page session's loaded tree, canonical forms, assets and
 *     documents (`session_files`), whenever they change;
 *   - `validate`: the pack JSON, answered with the issue list and its cost;
 *   - `bundle`: the pack JSON, answered with its `.lxpack` bytes
 *     (transferred) or the export's refusal - what the 3D preview rebuilds
 *     from, built here so the page's thread is not held;
 *   - `scene_objects`: a scene's geometry pack summary (the authoring hints'
 *     mesh sizes), answered as JSON or an error;
 *   - `map_preview`: one map's JSON, answered with the painter's
 *     `map_preview` of it, parsed (or `{ error }`);
 *   - `xref_sites`: the pack JSON, answered with the reference model's
 *     sites (the Find box's flag list), as JSON.
 *
 * Messages run in order, one at a time, with two exceptions that keep the
 * queue from running work nobody will read: a `validate` or `xref_sites`
 * drops any older one still waiting (the page reads only its latest),
 * and a `map_preview` drops an older one of the same map and runs ahead of
 * the validations and bundles waiting - the painter is waiting on it - but
 * never ahead of `init`, `disc` or `files`, so it still sees the files sent
 * before it. A dropped request is answered `{ dropped: true }`. A failure
 * is answered as `error` and the page falls back to its own thread.
 */
let wb = null;
const queue = [];
let busy = false;
const BARRIER = new Set(['init', 'disc', 'files']);

async function handle(m) {
  if (m.type === 'init') {
    const mod = await import(m.glue);
    await mod.default({ module_or_path: m.bin });
    if (typeof mod.ExpansionWorkbench !== 'function') throw new Error('the bundle has no ExpansionWorkbench');
    wb = new mod.ExpansionWorkbench();
    if (typeof wb.open_disc_parts !== 'function') throw new Error('the bundle predates the validation worker');
    self.postMessage({ type: 'ready' });
  } else if (m.type === 'disc') {
    const t0 = performance.now();
    wb.open_disc_parts(new Uint8Array(m.parts));
    self.postMessage({ type: 'disc', ms: Math.round(performance.now() - t0) });
  } else if (m.type === 'files') {
    wb.set_session_files(new Uint8Array(m.data));
  } else if (m.type === 'validate') {
    const t0 = performance.now();
    const issues = wb.validate(m.json);
    self.postMessage({ type: 'issues', seq: m.seq, issues, ms: Math.round(performance.now() - t0) });
  } else if (m.type === 'bundle') {
    const t0 = performance.now();
    let bytes;
    try {
      bytes = wb.export_bundle(m.json);
    } catch (e) {
      self.postMessage({ type: 'bundle', seq: m.seq, error: String(e && e.message ? e.message : e) });
      return;
    }
    self.postMessage({ type: 'bundle', seq: m.seq, bytes, ms: Math.round(performance.now() - t0) }, [bytes.buffer]);
  } else if (m.type === 'map_preview') {
    const t0 = performance.now();
    let preview;
    try { preview = JSON.parse(wb.map_preview(m.json)); } catch (e) { preview = { error: String(e && e.message ? e.message : e) }; }
    self.postMessage({ type: 'map_preview', seq: m.seq, map: m.map, preview, ms: Math.round(performance.now() - t0) });
  } else if (m.type === 'xref_sites') {
    let json = null, error = null;
    try { json = wb.xref_sites(m.json); } catch (e) { error = String(e && e.message ? e.message : e); }
    self.postMessage({ type: 'xref_sites', seq: m.seq, json, error });
  } else if (m.type === 'scene_objects') {
    let json = null, error = null;
    try { json = wb.scene_objects(m.scene); } catch (e) { error = String(e && e.message ? e.message : e); }
    self.postMessage({ type: 'scene_objects', scene: m.scene, json, error });
  }
}

/* The next request to run: the first `map_preview` not behind a barrier,
 * else the head of the queue. */
function next() {
  for (let i = 0; i < queue.length; i++) {
    if (BARRIER.has(queue[i].type)) break;
    if (queue[i].type === 'map_preview') return queue.splice(i, 1)[0];
  }
  return queue.shift();
}

async function pump() {
  if (busy) return;
  busy = true;
  while (queue.length) {
    const m = next();
    try {
      await handle(m);
    } catch (e) {
      self.postMessage({ type: 'error', of: m.type, seq: m.seq, message: String(e && e.message ? e.message : e) });
    }
  }
  busy = false;
}

self.onmessage = (ev) => {
  const m = ev.data || {};
  if (m.type === 'validate' || m.type === 'xref_sites' || m.type === 'map_preview') {
    for (let i = queue.length - 1; i >= 0; i--) {
      const q = queue[i];
      if (BARRIER.has(q.type)) break;
      if (q.type === m.type && (m.type !== 'map_preview' || q.map === m.map)) {
        queue.splice(i, 1);
        if (m.type === 'map_preview') self.postMessage({ type: 'map_preview', seq: q.seq, map: q.map, dropped: true });
      }
    }
  }
  queue.push(m);
  pump();
};
