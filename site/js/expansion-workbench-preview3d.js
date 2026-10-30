/* Expansion workbench: the 3D map preview and the Play here loop.
 *
 * The preview is not a second renderer. It is the play page itself, framed
 * as `play.html?xwpreview=1` (js/play-expansion-live.js on its side): its own
 * LegaiaRuntime reads the disc from the site's shared disc cache, loads the
 * working pack's `.lxpack` and enters the map through the play page's entry
 * path, so the picture is the engine's scene build for that map - its tiles,
 * walls, props, decorations, animators, emitters, materials, lights, mood
 * and fog - drawn by the play page's renderer, with the editor's volumes on
 * top (js/play-view-editor-overlay.js).
 *
 * After an edit (debounced) the working pack is exported again and posted to
 * the frame, which hot-reloads it (`LegaiaRuntime::reload_expansion`: the
 * same activation, the map re-entered with the player where they stood).
 * What is not an edit stays a message and rebuilds nothing: the selection,
 * the overlay layers, the camera, a cutscene's path.
 *
 * Between rebuilds the volumes follow the editor at once: the painter hands
 * over each `map_preview` it computes (`setMapPreview`), and the frame draws
 * the tile grid at its corner heights and a draft of the edited map - its
 * object lists, and the tiles whose walls, void or floor the edit changed -
 * over the engine's reading
 * of the last build, until the rebuild lands and replaces it.
 *
 * The view switch lays the editor out: **2D** (the dock above the tab, the
 * painter under it), **Split** (the picture in a column beside the tab,
 * sticky while the forms scroll) and **3D** (the picture as large as the
 * window allows, the painter's canvas put away and its forms under it).
 * The frame is never moved in the page - moving an iframe reloads it - so
 * the layouts are classes on the page (`xw-view-split`, `xw-view-3d`) and
 * the style below.
 *
 * Play here posts the same bytes on BroadcastChannel `legaia-expansion-live`:
 * an open play tab on the pack hot-reloads in place and answers; with no
 * answer the bundle goes to the play page's IndexedDB slot (database
 * `legaia-expansion`, store `packs`, key `play`) and a play tab opens on the
 * map (`play.html?expansion=1&map=<id>`).
 */

const CHANNEL = 'legaia-expansion-live';
const LAYERS = [
  ['grid', 'Grid'], ['walls', 'Walls'], ['zones', 'Encounter zones'], ['triggers', 'Triggers + exits'],
  ['npcs', 'NPCs'], ['spawns', 'Spawns'], ['props', 'Props'],
];
const RELOAD_MS = 450;
const VIEWS = [
  ['2d', '2D', 'The painter, with the 3D preview docked above the tab'],
  ['split', 'Split', 'The painter and the 3D view side by side: the picture stays in view while the forms scroll'],
  ['3d', '3D', 'The 3D view as large as the window allows; the painter\'s canvas is put away and its forms sit under the picture'],
];
const VIEW_KEY = 'legaia-xw-view';

/* The layouts of the view switch. The page's own style (the HTML) sizes
 * the dock for 2D; these rules only rearrange it, so the page keeps one
 * place for its look. */
const VIEW_CSS = `
body.xw-view-split .xw-edpane { display: grid; grid-template-columns: minmax(0, 1fr) minmax(0, 1.3fr); column-gap: 0.6rem; align-items: start; }
body.xw-view-split .xw-edpane > .xw-tabs { grid-column: 1 / -1; }
body.xw-view-split .xw-edpane > .xw-panel { grid-column: 1; grid-row: 2; min-width: 0; }
body.xw-view-split .xw-edpane > .xw-3d:not([hidden]) { grid-column: 2; grid-row: 2; position: sticky; top: calc(var(--topbar-h, 0px) + 0.4rem); z-index: 2; max-height: calc(100vh - var(--topbar-h, 0px) - 0.8rem); overflow: auto; border: 1px solid var(--border); border-radius: 8px; }
body.xw-view-split .xw-edpane > .xw-3d .xw-3d-frame { max-width: none; width: min(100%, calc((100vh - var(--topbar-h, 0px) - 12rem) * 4 / 3)); }
body.xw-view-3d .xw-edpane > .xw-3d .xw-3d-frame { max-width: none; width: min(100%, calc((100vh - var(--topbar-h, 0px) - 9rem) * 4 / 3)); }
body.xw-view-split .xw-painter { grid-template-columns: minmax(0, 1fr); }
body.xw-view-split .xw-canvas-wrap { height: min(48vh, 32rem); }
body.xw-view-3d .xw-painter .xw-canvas-wrap, body.xw-view-3d .xw-painter .xw-canvas-wrap ~ .xw-note { display: none; }
.xw-3d-views { display: inline-flex; gap: 0; }
.xw-3d-views .wb-mini { border-radius: 0; margin: 0; }
.xw-3d-views .wb-mini:first-child { border-radius: 4px 0 0 4px; }
.xw-3d-views .wb-mini:last-child { border-radius: 0 4px 4px 0; }
`;

function installViewCss() {
  if (document.getElementById('xw-view-css')) return;
  const st = document.createElement('style');
  st.id = 'xw-view-css';
  st.textContent = VIEW_CSS;
  document.head.append(st);
}

/* The tile grid's corner heights (world units up) for the frame: the
 * synthetic image's tiers, or on a terrain map its heightfield sampled at
 * each tile corner as \`Heightfield::sample_world\` does (bilinear here: a
 * corner sits on a heightfield vertex). */
export function gridCorners(map, pv) {
  if (!pv || pv.error || !pv.corners) return null;
  const W = pv.width, H = pv.height;
  const t = map && map.terrain ? pv.terrain : null;
  // Ramps are filled on a tier map; a terrain map slopes everywhere and
  // shows its relief itself.
  if (!t || !t.heights || !t.origin) return { width: W, height: H, corners: pv.corners, slopes: true };
  const o0 = (map.tiles && map.tiles.origin) || [0, 0];
  const at = (i, j) => t.heights[Math.max(0, Math.min(t.rows - 1, j)) * t.cols + Math.max(0, Math.min(t.cols - 1, i))];
  const sample = (x, z) => {
    const fi = (x - t.origin[0]) / t.step, fj = (z - t.origin[1]) / t.step;
    const i = Math.floor(fi), j = Math.floor(fj), u = fi - i, v = fj - j;
    return Math.round((at(i, j) * (1 - u) + at(i + 1, j) * u) * (1 - v) + (at(i, j + 1) * (1 - u) + at(i + 1, j + 1) * u) * v);
  };
  const corners = new Array(W * H);
  for (let r = 0; r < H; r++) {
    for (let c = 0; c < W; c++) {
      const x0 = (o0[0] + c) * 128, z0 = (o0[1] + r) * 128;
      corners[r * W + c] = [sample(x0, z0), sample(x0 + 128, z0), sample(x0, z0 + 128), sample(x0 + 128, z0 + 128)];
    }
  }
  return { width: W, height: H, corners, slopes: false };
}

/* The draft of the edited map for the frame: the object lists as the
 * definition holds them, and the tiles whose walls, void or floor changed
 * since \`built\` (the \`map_preview\` of the pack the last rebuild was sent),
 * as \`[index, walls, void, floor]\` - only those, so a tile the edit left
 * alone keeps the engine's own reading (its sampled floor, its walls). A
 * changed tile's floor is its corners' mean (world units up); a terrain
 * map's floor is its heightfield, which the engine samples, so there only
 * walls and void are drafted. */
export function draftDefs(map, pv, built) {
  if (!map || !pv || pv.error) return null;
  const d = { width: pv.width, height: pv.height };
  for (const k of ['spawns', 'exits', 'triggers', 'encounters', 'props', 'npcs']) d[k] = map[k] || [];
  const cells = [];
  if (built && !built.error && built.width === pv.width && built.height === pv.height) {
    const terrain = !!map.terrain;
    for (let i = 0; i < pv.width * pv.height; i++) {
      const q = pv.corners[i], b = built.corners[i];
      const moved = !terrain && (q[0] !== b[0] || q[1] !== b[1] || q[2] !== b[2] || q[3] !== b[3]);
      if (pv.walls[i] === built.walls[i] && !!pv.void[i] === !!built.void[i] && !moved) continue;
      cells.push([i, pv.walls[i], pv.void[i] ? 1 : 0, moved ? Math.round((q[0] + q[1] + q[2] + q[3]) / 4) : null]);
    }
  }
  d.cells = cells;
  return d;
}

export function createPreview3d(env) {
  // env: { exportBundle() -> Uint8Array (throws on errors), pack(), storePlay(bytes, map) -> Promise, playUrl(map) }
  const P = {
    open: false, ready: false, visible: false, frame: null, map: null, spawn: '',
    dirty: true, timer: null, sent: null, layers: { grid: true, walls: true, zones: true, triggers: true, npcs: true, spawns: true, props: false },
    select: null, path: null, marker: null, mode: 'orbit', onPick: null, stats: null, last: null, live: false, liveTimer: null,
    auto: true, painter: true, arrival: false, edits: 0, builtEdits: -1, view: '2d', grid: null, defs: null, overlayKey: '',
  };
  try { P.view = VIEWS.some(([k]) => k === localStorage.getItem(VIEW_KEY)) ? localStorage.getItem(VIEW_KEY) : '2d'; } catch (e) { P.view = '2d'; }
  installViewCss();
  let channel = null;
  try { channel = new BroadcastChannel(CHANNEL); } catch (e) { channel = null; }

  const el = document.createElement('div');
  el.className = 'xw-3d';
  el.hidden = true;
  const head = document.createElement('div');
  head.className = 'xw-3d-head';
  const body = document.createElement('div');
  body.className = 'xw-3d-body';
  body.hidden = true;
  const status = document.createElement('div');
  status.className = 'xw-3d-status';
  el.append(head, body);

  const btn = (label, title, fn) => {
    const b = document.createElement('button');
    b.type = 'button';
    b.className = 'wb-mini';
    b.textContent = label;
    if (title) b.title = title;
    b.addEventListener('click', fn);
    return b;
  };
  const toggleBtn = btn('Open 3D preview', 'Render this map as the engine builds it, in the play page framed here. It reads your disc from the site\'s disc cache: the first open takes a few seconds.', () => setOpen(!P.open));
  const orbitBtn = btn('Orbit', 'Free camera: drag to turn, wheel to zoom, right-drag to pan', () => setMode('orbit'));
  const playerBtn = btn('Player', 'The field camera: the follow view the game draws the map with', () => setMode('player'));
  const fitBtn = btn('Fit map', 'Frame the whole map', () => {
    setMode('orbit', true);
    post({ type: 'xw-camera', mode: 'orbit', focus: null, painter: P.painter });
  });
  const walkBtn = btn('Walk from selection', 'Stand the player on the selected tile and switch to the field camera: walk with the arrow keys in the picture', () => {
    const t = selTile();
    if (!t) return;
    post({ type: 'xw-seat', tile: t });
    setMode('player');
  });
  const reloadBtn = btn('Reload', 'Rebuild the map from the pack now', () => { P.dirty = true; flush(); });
  const layerBox = document.createElement('span');
  layerBox.className = 'xw-3d-layers';
  for (const [k, label] of LAYERS) {
    const lab = document.createElement('label');
    lab.className = 'xw-check';
    const cb = document.createElement('input');
    cb.type = 'checkbox';
    cb.checked = !!P.layers[k];
    cb.dataset.layer = k;
    cb.addEventListener('change', () => { P.layers[k] = cb.checked; sendOverlay(); });
    lab.append(cb, document.createTextNode(label));
    layerBox.append(lab);
  }
  const title = document.createElement('strong');
  title.textContent = '3D preview';
  const viewBox = document.createElement('span');
  viewBox.className = 'xw-3d-views';
  viewBox.setAttribute('role', 'group');
  viewBox.setAttribute('aria-label', 'Map view');
  const viewBtns = {};
  for (const [k, label, tip] of VIEWS) {
    const b = btn(label, tip, () => setView(k));
    b.dataset.view = k;
    viewBtns[k] = b;
    viewBox.append(b);
  }
  const ctrls = document.createElement('span');
  ctrls.className = 'xw-3d-ctrls';
  ctrls.hidden = true;
  const autoLab = document.createElement('label');
  autoLab.className = 'xw-check';
  autoLab.title = 'Rebuild after each edit (off: only Reload rebuilds - for a big map)';
  const autoCb = document.createElement('input');
  autoCb.type = 'checkbox';
  autoCb.checked = true;
  autoCb.addEventListener('change', () => { P.auto = autoCb.checked; if (P.auto && P.dirty) schedule(); });
  autoLab.append(autoCb, document.createTextNode('Auto'));
  const check = (label, title, on, fn) => {
    const lab = document.createElement('label');
    lab.className = 'xw-check';
    lab.title = title;
    const cb = document.createElement('input');
    cb.type = 'checkbox';
    cb.checked = on;
    cb.addEventListener('change', () => fn(cb.checked));
    lab.append(cb, document.createTextNode(label));
    return lab;
  };
  /* The game looks at a map from its low-Z side with the world's own
   * handedness, which is the painter's grid mirrored (the painter draws row 0
   * at the top). Match painter turns the orbit camera to the high-Z side and
   * mirrors the picture, so the 3D view reads as the grid does: row 0 at the
   * top, columns left to right. The Player camera is always the game's. */
  const painterLab = check('Match painter', 'Orbit view oriented as the painter draws the grid (row 0 at the top, columns left to right); off shows the map with the game\'s own handedness', true, (on) => {
    P.painter = on;
    post({ type: 'xw-camera', mode: P.mode, painter: on });
  });
  /* A preview entry skips the map's arrival cutscene and place-name banner;
   * ticked, the map is entered again as a player arrives. */
  const arrivalLab = check('Arrival', 'Play the map\'s arrival cutscene and name banner on entry, as a player arriving sees them (re-enters the map at its spawn)', false, (on) => {
    P.arrival = on;
    post({ type: 'xw-arrival', on });
    if (on && P.sent) { P.sent = null; P.dirty = true; flush(); }
  });
  ctrls.append(orbitBtn, playerBtn, fitBtn, walkBtn, reloadBtn, autoLab, painterLab, arrivalLab);
  // The map editor's bar (expansion-workbench-gizmos.js), under the head.
  const editSlot = document.createElement('div');
  editSlot.className = 'xw-3d-editslot';
  head.append(title, viewBox, toggleBtn, ctrls, layerBox, status);
  body.before(editSlot);
  layerBox.hidden = true;

  /* The status line: the last build's or action's message, then the
   * frame's cost (refreshed each second without losing the message). */
  let msg = '';
  let msgBad = false;
  function setStatus(text, bad) {
    msg = text || '';
    msgBad = !!bad;
    paintStatus();
  }
  function paintStatus() {
    status.textContent = msg + (P.stats && P.open ? statsText() : '');
    status.classList.toggle('is-err', msgBad);
  }

  function selTile() {
    const s = P.select;
    if (!s) return null;
    if (s.tile) return s.tile;
    if (s.rect) return [Math.min(s.rect[0], s.rect[2]), Math.min(s.rect[1], s.rect[3])];
    return null;
  }

  function post(msg, transfer) {
    if (P.frame && P.frame.contentWindow && P.ready) {
      P.frame.contentWindow.postMessage(msg, location.origin, transfer || []);
    }
  }

  function setMode(mode, refit) {
    P.mode = mode;
    orbitBtn.classList.toggle('is-on', mode === 'orbit');
    playerBtn.classList.toggle('is-on', mode === 'player');
    if (!refit) post({ type: 'xw-camera', mode, painter: P.painter });
    if (mode === 'player' && P.frame) P.frame.focus();
  }

  /* The layout: 2D, Split or 3D (see the header). Split and 3D open the
   * preview; the choice is kept per browser. Off the Maps tab the page
   * stays as 2D lays it out. */
  function setView(view) {
    P.view = view;
    try { localStorage.setItem(VIEW_KEY, view); } catch (e) { /* no storage */ }
    if (view !== '2d' && !P.open) setOpen(true);
    applyView();
    if (view !== '2d' && P.open) el.scrollIntoView({ block: 'nearest' });
  }
  function applyView() {
    const on = P.visible && P.mapsTab !== false;
    document.body.classList.toggle('xw-view-split', on && P.view === 'split');
    document.body.classList.toggle('xw-view-3d', on && P.view === '3d');
    for (const [k, b] of Object.entries(viewBtns)) b.classList.toggle('is-on', k === P.view);
    viewBox.hidden = P.mapsTab === false;
  }

  function setOpen(on) {
    P.open = on;
    toggleBtn.textContent = on ? 'Close 3D preview' : 'Open 3D preview';
    body.hidden = !on;
    ctrls.hidden = !on;
    editSlot.hidden = !on || !editSlot.firstChild || !!P.editOff;
    layerBox.hidden = !on;
    if (on && !P.frame) {
      const f = document.createElement('iframe');
      f.className = 'xw-3d-frame';
      f.title = '3D map preview';
      f.src = new URL('../play.html?xwpreview=1', import.meta.url).href;
      P.frame = f;
      P.ready = false;
      P.dirty = true;
      body.append(f);
      setStatus('Starting the engine and reading your disc ...');
    } else if (!on && P.frame) {
      // Closing frees the frame's engine and its copy of the disc.
      P.frame.remove();
      P.frame = null;
      P.ready = false;
      P.sent = null;
      setStatus('');
      // No picture to lay out beside the painter.
      if (P.view !== '2d') { P.view = '2d'; applyView(); }
    }
    setMode(P.mode);
  }

  function pack() { return env.pack(); }

  /* Export and post the pack to the frame now. */
  function flush() {
    clearTimeout(P.timer);
    if (!P.open || !P.ready || !P.visible || !P.map || !pack()) return;
    if (!pack().maps || !pack().maps[P.map]) { setStatus(`No map ${P.map} in the pack.`, true); return; }
    /* The painter's preview of the edit is still to come: let its draft
     * reach the frame first (the rebuild then holds the thread for a
     * while), once. */
    if (P.sent && P.pvNowEdits !== P.edits && !P.deferred) {
      P.deferred = true;
      P.timer = setTimeout(flush, 250);
      return;
    }
    P.deferred = false;
    let bytes;
    try {
      bytes = env.exportBundle();
    } catch (e) {
      setStatus(`Showing the last good build: the pack has errors (${String(e && e.message ? e.message : e).split('\n')[0]}).`, true);
      return;
    }
    const first = !P.sent;
    P.sent = { map: P.map };
    // The tiles the draft compares against are this build's: the latest
    // preview when no edit came after it, else the next one to arrive.
    P.builtEdits = P.edits;
    P.pvBuilt = P.pvNow && P.pvNowEdits === P.edits ? P.pvNow : null;
    // The draft sent with the overlay after the build: nothing beyond it.
    const def = pack().maps[P.map];
    if (P.pvBuilt && def) P.defs = draftDefs(def, P.pvNow, P.pvBuilt);
    P.dirty = false;
    P.t0 = performance.now();
    setStatus(first ? `Entering ${P.map} ...` : `Rebuilding ${P.map} ...`);
    P.lastKind = first ? 'xw-load' : 'xw-reload';
    post({ type: P.lastKind, id: pack().manifest.id, map: P.map, spawn: P.spawn, bytes }, [bytes.buffer]);
  }

  /* The rebuild runs on this page's main thread, so a map that takes a
   * while to build waits for a longer pause in the editing: the delay grows
   * with the last build's time. With Auto off only Reload rebuilds. */
  function schedule() {
    clearTimeout(P.timer);
    if (!P.auto && P.sent) return;
    // The last hot reload's time (a first entry builds more and says
    // nothing about the next edit).
    const last = P.reloadMs || 0;
    P.timer = setTimeout(flush, Math.max(RELOAD_MS, Math.min(5000, last * 1.5)));
  }

  function sendOverlay() {
    post({ type: 'xw-overlay', layers: P.layers, select: P.select, path: P.path, marker: P.marker, grid: P.grid, defs: P.defs });
  }

  window.addEventListener('message', (ev) => {
    if (!P.frame || ev.source !== P.frame.contentWindow || ev.origin !== location.origin) return;
    const m = ev.data || {};
    if (m.type === 'xw-ready') {
      P.ready = true;
      P.sent = null;
      post({ type: 'xw-arrival', on: P.arrival });
      syncRun();
      sendEdit();
      flush();
    } else if (m.type === 'xw-loaded') {
      const ms = Math.round(performance.now() - (P.t0 || performance.now()));
      P.builds = (P.builds || 0) + 1;
      if (m.ok && P.lastKind === 'xw-reload') P.reloadMs = ms;
      P.last = { ok: m.ok, ms, engineMs: m.engineMs, map: m.map, n: P.builds };
      if (m.busy) {
        // A fight or a screen owns the frame: try again shortly.
        setStatus('The preview is in a fight or a screen: the edit goes in once it is back on the field.');
        P.dirty = true;
        clearTimeout(P.timer);
        P.timer = setTimeout(flush, 1500);
      } else if (!m.ok) {
        setStatus(`The engine refused the pack: ${(m.errors || []).slice(0, 2).join(' | ')}`, true);
        if (!m.map) P.sent = null;
      } else {
        setStatus(`${m.map}: built in ${ms} ms${m.engineMs != null ? ` (engine ${m.engineMs} ms)` : ''}.`);
      }
      sendOverlay();
      sendEdit();
      post({ type: 'xw-camera', mode: P.mode, painter: P.painter });
      syncRun();
      if (m.ok && P.pending) sendPending();
    } else if (m.type === 'xw-pick') {
      P.select = { tile: m.tile };
      if (P.onPick) P.onPick(m.tile[0], m.tile[1]);
    } else if (m.type === 'xw-action') {
      const what = { cutscene: `Cutscene ${m.id}`, fight: `Fight ${m.id}`, cards: 'The card binder' }[m.what] || m.what;
      setStatus(m.ok ? `${what}: running in the preview (field camera; Orbit to look around).${m.what === 'cards' ? ` ${m.added} card(s) put in its collection.` : ''}`
        : `${what} did not start in the preview (another scene or fight may own it).`, !m.ok);
    } else if (m.type === 'xw-stats') {
      P.stats = m;
      paintStatus();
    } else if (/^xw-(select|transform|sculpt|paint|key)$/.test(m.type)) {
      // The frame's editing layer answered (expansion-workbench-gizmos.js).
      if (P.onEdit) P.onEdit(m);
    }
  });

  /* The 3D editor's state for the frame (`xw-edit`): kept whole, so a new
   * frame or a rebuilt map gets all of it again. */
  function sendEdit() {
    if (P.edit) post(Object.assign({ type: 'xw-edit' }, P.edit, P.editOff ? { mode: 'view' } : {}));
  }

  /* An action for the frame's engine (a cutscene, a fight, the binder),
   * sent once the map it runs on is built. With no map chosen yet, the
   * pack's first map stands in. */
  function act(msg) {
    if (!P.map && pack() && pack().maps) {
      const first = Object.keys(pack().maps)[0];
      if (first) { P.map = first; P.dirty = true; schedule(); }
    }
    P.pending = msg;
    if (P.ready && P.sent && !P.dirty) sendPending();
  }

  function sendPending() {
    const msg = P.pending;
    P.pending = null;
    setMode('player', true);
    post(msg);
  }

  function statsText() {
    const s = P.stats;
    return s ? ` Frame ${s.frameMs.toFixed(1)} ms (worst ${s.worstMs.toFixed(1)}), ${Math.round(s.fps)} fps.` : '';
  }

  /* --- Play here --- */
  function playHere(map) {
    let bytes;
    try { bytes = env.exportBundle(); } catch (e) { return Promise.reject(e); }
    const id = pack().manifest.id;
    return new Promise((resolve) => {
      const nonce = Math.random().toString(36).slice(2);
      let done = false;
      const finish = async (answer) => {
        if (done) return;
        done = true;
        if (channel) channel.removeEventListener('message', onMsg);
        if (answer) { resolve({ reloaded: true, answer }); return; }
        // No play tab on this pack answered: hand off through the slot.
        await env.storePlay(bytes);
        window.open(env.playUrl(map), 'legaia-play');
        resolve({ reloaded: false });
      };
      const onMsg = (ev) => {
        const m = ev.data || {};
        if (m.nonce === nonce && m.type === 'reloaded') finish(m);
      };
      if (!channel) { finish(null); return; }
      channel.addEventListener('message', onMsg);
      // Structured clone copies the bytes; keep ours for the slot fallback.
      channel.postMessage({ type: 'reload', nonce, id, map, bytes });
      setTimeout(() => finish(null), 700);
    });
  }

  /* Live: every edit goes to an open play tab too. */
  function pushLive() {
    clearTimeout(P.liveTimer);
    if (!P.live || !channel || !pack()) return;
    P.liveTimer = setTimeout(() => {
      let bytes;
      try { bytes = env.exportBundle(); } catch (e) { return; }
      channel.postMessage({ type: 'reload', nonce: 'live', id: pack().manifest.id, map: P.map, bytes });
    }, RELOAD_MS * 2);
  }

  /* Run the frame's loop only while the dock is on screen and the painter
   * is not mid-stroke: the frame shares this page's main thread. */
  let onScreen = true;
  let held = false;
  const syncRun = () => post({ type: 'xw-run', on: P.visible && P.open && onScreen && !held });
  if (typeof IntersectionObserver === 'function') {
    new IntersectionObserver((es) => {
      onScreen = es.some((e) => e.isIntersecting);
      syncRun();
    }).observe(body);
  }

  return {
    el,
    /* The painter holds the frame still through a stroke. */
    hold(on) {
      if (held === !!on) return;
      held = !!on;
      syncRun();
    },
    /* The tab now showing: the preview runs on the Maps and Cutscenes tabs. */
    show(on) {
      P.visible = !!on;
      el.hidden = !on;
      applyView();
      syncRun();
      if (P.visible && P.dirty) schedule();
    },
    /* The map the editor is on (and the spawn to enter it at). */
    setMap(map, spawn) {
      if (map === P.map) return;
      P.map = map;
      P.spawn = spawn || '';
      P.select = null;
      P.path = null;
      P.marker = null;
      P.grid = null;
      P.defs = null;
      P.overlayKey = '';
      P.lastPv = null;
      P.pvNow = null;
      P.pvBuilt = null;
      P.dirty = true;
      schedule();
    },
    /* The painter's latest \`map_preview\` of the map it is on: the grid and
     * the draft the frame draws until the next rebuild lands. Sent only
     * when it changed. */
    setMapPreview(map, pv) {
      // A new preview object is a new computation (the painter makes one a
      // moment after each edit); the same one has been sent.
      if (!map || !pv || pv.error || pv === P.lastPv) return;
      P.lastPv = pv;
      const grid = gridCorners(map, pv);
      P.pvNow = pv;
      P.pvNowEdits = P.edits;
      // The first preview after a build with no edit since it describes
      // exactly the pack that build was sent.
      if (!P.pvBuilt && P.builtEdits === P.edits) P.pvBuilt = pv;
      const defs = draftDefs(map, pv, P.pvBuilt);
      const key = JSON.stringify([grid, defs]);
      if (key === P.overlayKey) return;
      P.overlayKey = key;
      P.grid = grid;
      P.defs = defs;
      post({ type: 'xw-overlay', grid, defs });
    },
    /* The pack was edited. */
    changed() {
      P.edits = (P.edits || 0) + 1;
      P.dirty = true;
      if (P.visible) schedule();
      pushLive();
    },
    /* The painter's selection: `{ tile }`, `{ rect }` or null. */
    select(sel) {
      const k = JSON.stringify(sel || null);
      if (k === JSON.stringify(P.select || null)) return;
      P.select = sel || null;
      sendOverlay();
    },
    onPick(fn) { P.onPick = fn; },
    /* A cutscene's camera path on the map, and the scrub point. */
    setPath(path, marker) {
      const first = !P.path && path;
      P.path = path || null;
      P.marker = marker || null;
      sendOverlay();
      // A path is read from above.
      if (first && P.mode !== 'orbit') setMode('orbit');
    },
    /* Play cutscene `id` in the frame, once the map it plays on is built. */
    playCutscene(id) { act({ type: 'xw-cutscene', id }); },
    /* Start a pack fight (`kind` "formation" or "boss") in the frame. */
    fight(kind, id) { act({ type: 'xw-fight', kind, id }); },
    /* Every pack card into the frame's collection, and its binder open. */
    cards() { act({ type: 'xw-cards' }); },
    /* The 3D editor: its state for the frame (merged), the handler of the
     * frame's answers, and its bar. */
    edit(msg) {
      P.edit = Object.assign(P.edit || {}, msg);
      if (P.ready) post(Object.assign({ type: 'xw-edit' }, msg, P.editOff ? { mode: 'view' } : {}));
    },
    /* Off the Maps tab the 3D editor stands down: the frame drops to the
     * view mode and the edit bar hides; back on the tab both come back as
     * they were. */
    suspendEdit(off) {
      off = !!off;
      // The view switch belongs to the Maps tab as the editor does.
      P.mapsTab = !off;
      applyView();
      if (!off && P.visible && P.view !== '2d' && !P.open) setOpen(true);
      if (P.editOff === off) return;
      P.editOff = off;
      editSlot.hidden = off || !editSlot.firstChild || !P.open;
      if (P.ready) sendEdit();
    },
    onEdit(fn) { P.onEdit = fn; },
    setEditBar(bar) {
      if (bar && bar.parentNode !== editSlot) { editSlot.innerHTML = ''; editSlot.append(bar); }
      editSlot.hidden = !bar || !P.open || !!P.editOff;
    },
    isOpen: () => P.open,
    open: () => { if (!P.open) setOpen(true); },
    playHere,
    setLive(on) { P.live = !!on; pushLive(); },
    /* Headless-verification hook. */
    setView,
    state: () => ({ view: P.view, open: P.open, ready: P.ready, map: P.map, last: P.last, stats: P.stats, select: P.select, dirty: P.dirty, mode: P.mode, painter: P.painter, arrival: P.arrival, status: status.textContent }),
  };
}
