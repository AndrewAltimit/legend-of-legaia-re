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
 *     mesh sizes), answered as JSON or an error.
 *
 * Messages run strictly in order (one promise chain), so a `validate` sent
 * after `files` sees those files. A failure is answered as `error` and the
 * page falls back to validating on its own thread.
 */
let wb = null;
let chain = Promise.resolve();

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
  } else if (m.type === 'scene_objects') {
    let json = null, error = null;
    try { json = wb.scene_objects(m.scene); } catch (e) { error = String(e && e.message ? e.message : e); }
    self.postMessage({ type: 'scene_objects', scene: m.scene, json, error });
  }
}

self.onmessage = (ev) => {
  const m = ev.data || {};
  chain = chain.then(() => handle(m)).catch((e) => {
    self.postMessage({ type: 'error', of: m.type, seq: m.seq, message: String(e && e.message ? e.message : e) });
  });
};
