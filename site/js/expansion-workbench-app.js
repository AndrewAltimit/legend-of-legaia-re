/* Expansion workbench: an expansion-pack editor over the user's own disc,
 * entirely client-side. The disc bytes are read in this tab and handed once to
 * a WASM `ExpansionWorkbench` session (crates/web-expansion), which keeps the
 * pickers' lists (retail monsters, items, spells, scenes, music) and the PROT
 * index for base-scene geometry. Nothing is uploaded.
 *
 * The pack lives here as its JSON form (`S.pack`, serde's form of
 * `ExpansionPack`), edited in place by the section editors
 * (expansion-workbench-forms.js) and the tile painter
 * (expansion-workbench-map.js). Every other answer is the session's:
 *   - `validate(pack)` a moment after each edit: the pack validator over the
 *     files exactly as they would be exported, with the disc resolving every
 *     coordinate;
 *   - `export_bundle(pack)` / `files_view(pack)` for the `.lxpack` and the
 *     per-file TOML (a file left unedited keeps its original bytes);
 *   - `xp_chart`, `check_dialog`, `map_preview`, `scene_objects` and
 *     `cutscene_timeline` for previews.
 *
 * Persistence (IndexedDB, a per-viewer convenience; every access is wrapped):
 * the working pack JSON autosaves under `legaia-expansion-workbench` /
 * `autosave` / `current`, the tree it was loaded from under `source`, and a
 * returning visitor is offered "resume". "Play in browser" stores the
 * exported bundle under database `legaia-expansion`, store `packs`, key
 * `play`, as `{ id, name, version, bytes, savedAt }`, and opens
 * `play.html?expansion=1` (docs/tooling/expansion-workbench.md).
 *
 * Imports resolve relative to THIS file (site/js/); the published example is
 * `../packs/rebirth.json`, written by site/_gen.py from packs/rebirth/.
 */

import {
  h, renderManifest, renderBattle, renderProgression, drawXpChart, renderQuests,
  renderDialog, renderCards, renderItems, renderModes,
} from './expansion-workbench-forms.js';
import { renderMaps, armPick, openMapId } from './expansion-workbench-map.js';
import { renderCutscenes } from './expansion-workbench-cutscenes.js';
import { renderAudio } from './expansion-workbench-audio.js';
import { renderArts, renderSpells } from './expansion-workbench-arts.js';
import { createPreview3d } from './expansion-workbench-preview3d.js';
import { createFind, locate } from './expansion-workbench-xref.js';
import { renderFlow } from './expansion-workbench-graph.js';
import { attachShare } from './expansion-workbench-share.js';
import { addSectionHelp } from './expansion-workbench-help.js';
import { authoringHints, hintScenes } from './expansion-workbench-hints.js';

const $ = (id) => document.getElementById(id);
const tick = () => new Promise((r) => setTimeout(r, 30));
const esc = window.escapeHtml;
const downloadFile = window.LegaiaUtil.downloadFile;

// --- IndexedDB ----------------------------------------------------------------------
function idb(db, store, mode, fn) {
  return new Promise((resolve) => {
    let req;
    try { req = indexedDB.open(db, 1); } catch (e) { resolve(null); return; }
    req.onupgradeneeded = () => {
      try { req.result.createObjectStore(store); } catch (e) { /* exists */ }
    };
    req.onerror = () => resolve(null);
    req.onsuccess = () => {
      try {
        const t = req.result.transaction(store, mode);
        const r = fn(t.objectStore(store));
        let out = null;
        if (r) r.onsuccess = () => { out = r.result; };
        t.oncomplete = () => resolve(out);
        t.onerror = () => resolve(null);
        t.onabort = () => resolve(null);
      } catch (e) {
        resolve(null);
      }
    };
  });
}
const SAVE_DB = 'legaia-expansion-workbench';
const SAVE_STORE = 'autosave';
const saveGet = (k) => idb(SAVE_DB, SAVE_STORE, 'readonly', (s) => s.get(k));
const savePut = (k, v) => idb(SAVE_DB, SAVE_STORE, 'readwrite', (s) => s.put(v, k));
const saveDel = (k) => idb(SAVE_DB, SAVE_STORE, 'readwrite', (s) => s.delete(k));
// The play page's hand-off slot.
export const PLAY_DB = 'legaia-expansion';
export const PLAY_STORE = 'packs';
export const PLAY_KEY = 'play';

// --- State ----------------------------------------------------------------------------
const S = {
  wb: null,
  cat: null,         // the disc's picker lists
  pack: null,        // the pack JSON being edited
  label: '',         // where the pack came from
  issues: [],
  hints: [],         // authoring hints (expansion-workbench-hints.js), not checks
  sceneObj: {},      // scene_objects per scene, shared by the painter and the hints
  tab: 'manifest',
  sel: {},           // split-editor selections, by file:path
  jump: null,        // { file, field } while jumping to an issue
  chart: null,
  objects: {},
  // Page-wide history: the pack JSON at each committed step. `cur` is the
  // committed present; an edit not yet committed is S.pack itself.
  hist: { past: [], future: [], cur: null, timer: null, bytes: 0 },
  clean: null,      // the pack JSON as last loaded or downloaded
  saved: { at: 0, state: 'none' },
  discName: '',     // the disc image's file name, once read
  pending: null,    // a start action waiting for the disc: { run, label }
};

const TABS = [
  ['manifest', 'Manifest', 'pack.toml', renderManifest],
  ['battle', 'Battle', 'battle.toml', renderBattle],
  ['progression', 'Progression', 'progression.toml', renderProgression],
  ['quests', 'Quests', 'quests.toml', renderQuests],
  ['dialog', 'Dialog', 'dialog.toml', renderDialog],
  ['cutscenes', 'Cutscenes', 'cutscenes.toml', renderCutscenes],
  ['cards', 'Cards', 'cards.toml', renderCards],
  ['items', 'Items', 'items.toml', renderItems],
  ['arts', 'Arts', 'arts.toml', renderArts],
  ['spells', 'Spells', 'spells.toml', renderSpells],
  ['modes', 'Modes', 'modes.toml', renderModes],
  ['audio', 'Audio', 'audio.toml', renderAudio],
  ['maps', 'Maps', 'maps/', renderMaps],
  ['flow', 'Flow', 'flow:', renderFlow],
  ['files', 'Files', null, renderFiles],
];

function tabForPath(path) {
  if (path.startsWith('maps/')) return 'maps';
  const t = TABS.find((x) => x[2] === path);
  return t ? t[0] : 'files';
}

function fileOfTab(tab) {
  if (tab === 'maps') {
    return 'maps/';
  }
  const t = TABS.find((x) => x[0] === tab);
  return t ? t[2] : null;
}

function idsOf(kind) {
  const p = S.pack;
  if (!p) return [];
  const keys = (o) => Object.keys(o || {});
  const arr = (a) => (a || []).map((x) => x.id).filter(Boolean);
  switch (kind) {
    case 'monster': return keys(p.battle.monsters);
    case 'formation': return keys(p.battle.formations);
    case 'boss': return keys(p.battle.bosses);
    case 'quest': return arr(p.quests.quests);
    case 'challenge': return arr(p.quests.challenges);
    case 'line': return keys(p.dialog.lines);
    case 'card': return arr(p.cards.cards);
    case 'booster': return arr(p.cards.boosters);
    case 'item': return keys(p.items && p.items.items);
    case 'item_set': return keys(p.items && p.items.sets);
    case 'shop': return arr(p.items && p.items.shops);
    case 'art': return keys(p.arts && p.arts.arts);
    case 'art_chain': return keys(p.arts && p.arts.chains);
    case 'spell': return keys(p.spells && p.spells.spells);
    case 'mode': return ['poker', 'card_duels', 'golf', 'races', 'prop_hunt', 'party_nights'].flatMap((k) => arr(p.modes[k]));
    case 'instance': return ['poker', 'card_duels', 'golf', 'races', 'prop_hunt'].flatMap((k) => arr(p.modes[k]));
    case 'track': return keys(p.audio && p.audio.tracks);
    case 'sound': return keys(p.audio && p.audio.sounds);
    case 'map': return keys(p.maps);
    case 'npc': return Object.values(p.maps || {}).flatMap((m) => arr(m.npcs));
    case 'cutscene': return keys(p.cutscenes && p.cutscenes.cutscenes);
    default: return [];
  }
}

const ctx = {
  get pack() { return S.pack; },
  // The 3D preview (expansion-workbench-preview3d.js), null until the page is up.
  get preview() { return S.preview; },
  get cat() { return S.cat; },
  get chart() { return S.chart; },
  sel: S.sel,
  ids: idsOf,
  file: () => fileOfTab(S.tab),
  jumpField: () => (S.jump && tabForPath(S.jump.file) === S.tab ? S.jump.field : null),
  jumpFile: () => (S.jump ? S.jump.file : null),
  issuePaths: () => {
    const f = fileOfTab(S.tab);
    return S.issues.filter((i) => i.severity === 'error' && (i.path === f || (f === 'maps/' && i.path.startsWith('maps/')))).map((i) => i.field);
  },
  changed: (structural) => {
    scheduleValidate();
    scheduleSave();
    if (S.preview) S.preview.changed();
    if (structural) { commitHistory(); rerender(); } else scheduleCommit();
    updateDirty();
  },
  // Page-wide undo / redo (the painter's buttons and keys come here too).
  undo: () => stepHistory(-1),
  redo: () => stepHistory(1),
  // Close the pending history step now: a 3D edit is one step of its own.
  commit: () => commitHistory(),
  // An edit still coalescing (the commit timer runs) is undoable as it is.
  canUndo: () => S.hist.past.length > 0 || S.hist.timer != null || pendingEdit(),
  canRedo: () => S.hist.future.length > 0,
  find: (q) => S.find && S.find.find(q),
  jump: (at) => jumpTo(at),
  // A tile Pick on map `map` from another tab: the Maps tab opens on that
  // map, the next click calls `done([c, r])`, and the page comes back.
  pickOnMap: (map, prompt, done) => {
    const back = S.tab;
    armPick(map, { rect: false, prompt, done: (q) => { S.tab = back; done(q); }, cancel: () => { S.tab = back; rerender(); } });
    S.tab = 'maps';
    rerender();
  },
  // The reference model (expansion-workbench-xref.js, -rename.js, -graph.js).
  xrefSites: () => xrefSites(),
  flowGraph: () => JSON.parse(S.wb.flow_graph(JSON.stringify(S.pack))),
  renamePlan: (kind, old, next) => JSON.parse(S.wb.rename_plan(JSON.stringify(S.pack), kind, old, next)),
  renameApply: (kind, old, next) => JSON.parse(S.wb.rename_id(JSON.stringify(S.pack), kind, old, next)),
  // A rename's result replaces the pack as one undo step.
  replacePack: (pack, msg) => {
    commitHistory();
    S.pack = pack;
    commitHistory();
    scheduleValidate();
    scheduleSave();
    if (S.preview) S.preview.changed();
    rerender();
    if (msg) setStatus(msg, 'ok', 'xw-export-status');
  },
  rerender: () => rerender(),
  // A fold built late (listEd's lazy body) takes the checks on its fields.
  markIssues: () => markFieldIssues(),
  checkDialog: (line) => {
    try { return JSON.parse(S.wb.check_dialog(JSON.stringify(line))); } catch (e) { return null; }
  },
  artPreview: (art, atk, def) => {
    try { return JSON.parse(S.wb.art_preview(JSON.stringify(art), atk, def)); } catch (e) { return { error: errText(e) }; }
  },
  mapPreview: (map) => {
    try { return JSON.parse(S.wb.map_preview(JSON.stringify(map))); } catch (e) { return { error: String(e) }; }
  },
  // The same from the map's JSON, off this thread when the worker is up:
  // `done(preview)` with the answer to the latest request for map `id` only
  // (an older one still waiting is dropped). Here, at once, without it.
  mapPreviewLater: (id, json, done) => mapPreviewLater(id, json, done),
  // Bumped by every change to the session's own files (an asset import):
  // a map preview reads the heightmap images among them.
  sessionGen: () => V.gen,
  cutsceneTimeline: (id, picks) => {
    try {
      return JSON.parse(S.wb.cutscene_timeline(JSON.stringify(S.pack), id, new Uint32Array(picks || [])));
    } catch (e) {
      return { error: errText(e) };
    }
  },
  sceneObjects: (scene) => {
    if (!S.sceneObj[scene]) S.sceneObj[scene] = JSON.parse(S.wb.scene_objects(scene));
    return S.sceneObj[scene];
  },
  sceneNpcs: (scene) => JSON.parse(S.wb.scene_npcs(scene)),
  // The pack's assets/ (glTF models and their PNGs for map props).
  assetPaths: () => (S.wb.asset_paths ? JSON.parse(S.wb.asset_paths()) : []),
  addAsset: (path, bytes) => { S.wb.add_asset(path, bytes); scheduleValidate(); scheduleSave(); },
  // An asset's bytes (an audio preview plays them through the browser's own
  // decoder) and an audio file's probe ({ rate, channels, frames, seconds, loop }).
  assetBytes: (path) => (S.wb.asset_bytes ? S.wb.asset_bytes(path) : new Uint8Array(0)),
  audioInfo: (path) => { try { return JSON.parse(S.wb.audio_info(path)); } catch (e) { return { error: String(e) }; } },
};

function setStatus(msg, kind, id) {
  const el = $(id || 'xw-status');
  el.textContent = msg || '';
  el.className = 'wb-status' + (kind ? ' is-' + kind : '');
}
const errText = (e) => (e && e.message ? e.message : String(e));

// --- Rendering ----------------------------------------------------------------------
function renderTabs() {
  const nav = $('xw-tabs');
  nav.innerHTML = '';
  for (const [id, label, file] of TABS) {
    const mine = S.issues.filter((i) => (file === 'maps/' ? i.path.startsWith('maps/') : file ? i.path === file : !TABS.some((t) => t[2] && (t[2] === i.path || (t[2] === 'maps/' && i.path.startsWith('maps/'))))));
    const errs = mine.filter((i) => i.severity === 'error').length;
    const warns = mine.length - errs;
    nav.append(h('button', {
      type: 'button', role: 'tab', class: 'xw-tab' + (S.tab === id ? ' is-active' : ''), 'aria-selected': String(S.tab === id),
      onclick: () => { S.tab = id; rerender(); },
    }, label, errs ? h('span', { class: 'xw-badge' }, errs) : warns ? h('span', { class: 'xw-badge is-warn' }, warns) : null));
  }
}

function rerender() {
  if (!S.pack) return;
  commitHistory();
  const wasClean = S.clean != null && S.hist.cur === S.clean;
  const m = S.pack.manifest;
  $('xw-pack-title').textContent = m.name || '(unnamed pack)';
  $('xw-pack-id').textContent = m.id || '';
  $('xw-pack-ver').textContent = `${m.version || ''}${S.label ? ' - ' + S.label : ''}`;
  renderTabs();
  if (S.preview) {
    S.preview.show(['maps', 'cutscenes', 'battle', 'cards', 'modes'].includes(S.tab));
    // The 3D editor belongs to the Maps tab.
    if (S.preview.suspendEdit) S.preview.suspendEdit(S.tab !== 'maps');
  }
  const panel = $('xw-panel');
  // Keep the panel's scroll position across a structural re-render.
  const y = window.scrollY;
  panel.innerHTML = '';
  const tab = TABS.find((t) => t[0] === S.tab);
  try {
    tab[3](ctx, panel);
    addSectionHelp(tab[0], tab[2], panel);
  } catch (e) {
    panel.append(h('div', { class: 'wb-status is-err' }, 'Editor error: ' + errText(e)));
    console.error(e);
  }
  if (S.tab === 'progression') updateChart();
  markFieldIssues();
  if (S.jump) finishJump(panel);
  else window.scrollTo(0, y);
  // An editor may fill in a default as it draws (an empty gate box): that is
  // not an edit, so the committed present absorbs it.
  S.hist.cur = JSON.stringify(S.pack);
  if (wasClean) S.clean = S.hist.cur;
  updateDirty(S.hist.cur);
  if (S.find) refreshFindSoon();
}

// The reference model's sites for the pack as it stands, cached per pack JSON.
let xrefCache = { json: null, sites: [] };
const xrefAsk = { seq: 0, json: null, done: null };

/* The Find box after an edit: a moment later, with the sites built in the
 * worker when it is up, so an edit's re-render does not wait on them. */
let findTimer = null;
function refreshFindSoon() {
  clearTimeout(findTimer);
  findTimer = setTimeout(() => {
    if (!S.find || !S.pack) return;
    const json = JSON.stringify(S.pack);
    if (xrefCache.json === json || !V.worker || !V.ready || !S.wb.xref_sites) { S.find.refresh(); return; }
    syncWorkerFiles();
    xrefAsk.seq++;
    xrefAsk.json = json;
    xrefAsk.done = () => S.find.refresh();
    V.worker.postMessage({ type: 'xref_sites', seq: xrefAsk.seq, json });
  }, 200);
}

function xrefSites() {
  if (!S.wb || !S.pack || !S.wb.xref_sites) return [];
  const json = JSON.stringify(S.pack);
  if (xrefCache.json !== json) {
    try { xrefCache = { json, sites: JSON.parse(S.wb.xref_sites(json)).sites }; } catch (e) { xrefCache = { json, sites: [] }; }
  }
  return xrefCache.sites;
}

// --- History, dirty state ---------------------------------------------------------------
const HIST_STEPS = 200;
const HIST_BYTES = 96 * 1024 * 1024;

function pendingEdit() {
  return S.pack != null && S.hist.cur != null && JSON.stringify(S.pack) !== S.hist.cur;
}

// Typing coalesces: a burst of input events is one step.
function scheduleCommit() {
  clearTimeout(S.hist.timer);
  S.hist.timer = setTimeout(commitHistory, 600);
}

function commitHistory() {
  clearTimeout(S.hist.timer);
  S.hist.timer = null;
  if (!S.pack) return;
  const now = JSON.stringify(S.pack);
  const H = S.hist;
  if (H.cur == null) { H.cur = now; updateDirty(now); return; }
  if (now === H.cur) { updateDirty(now); return; }
  H.past.push(H.cur);
  H.bytes += H.cur.length;
  H.cur = now;
  H.future = [];
  while (H.past.length > HIST_STEPS || (H.bytes > HIST_BYTES && H.past.length > 1)) H.bytes -= H.past.shift().length;
  updateDirty(now);
}

function resetHistory() {
  S.hist.past = [];
  S.hist.future = [];
  S.hist.bytes = 0;
  S.hist.cur = S.pack ? JSON.stringify(S.pack) : null;
  S.clean = S.hist.cur;
}

function stepHistory(dir) {
  if (!S.pack) return false;
  commitHistory();
  const H = S.hist;
  const from = dir < 0 ? H.past : H.future;
  const to = dir < 0 ? H.future : H.past;
  if (!from.length) return false;
  to.push(H.cur);
  H.cur = from.pop();
  H.bytes = H.past.reduce((n, x) => n + x.length, 0);
  S.pack = JSON.parse(H.cur);
  scheduleValidate();
  scheduleSave();
  if (S.preview) S.preview.changed();
  rerender();
  setStatus(dir < 0 ? `Undone (${H.past.length} more to undo).` : `Redone (${H.future.length} more to redo).`, 'ok', 'xw-export-status');
  return true;
}

/* `json`, when the caller has it, is the pack's JSON as it is now (the
 * history's present just taken): it saves serialising the pack again. */
function updateDirty(json) {
  const el = $('xw-dirty');
  if (!el || !S.pack) return;
  // While a burst of typing coalesces the pack is not serialised on every
  // key (a few hundred kilobytes of JSON): an edit underway counts as a
  // change, and the commit that closes the burst decides.
  const now = () => (json != null ? json : (json = JSON.stringify(S.pack)));
  const dirty = S.clean != null && (S.hist.timer != null || now() !== S.clean);
  el.hidden = !dirty;
  const u = $('xw-undo');
  const r = $('xw-redo');
  // Undo: a step behind, an edit coalescing, or one not yet committed.
  if (u) u.disabled = !(S.hist.past.length > 0 || S.hist.timer != null || (S.hist.cur != null && now() !== S.hist.cur));
  if (r) r.disabled = !ctx.canRedo();
  const sv = $('xw-saved');
  if (sv) {
    const st = S.saved;
    sv.textContent = st.state === 'saving' ? 'Autosaving ...'
      : st.state === 'saved' ? `Autosaved in this browser ${new Date(st.at).toLocaleTimeString()}`
        : st.state === 'failed' ? 'Autosave unavailable (no browser storage): download the .lxpack to keep your work'
          : '';
    sv.className = 'xw-saved' + (st.state === 'failed' ? ' is-err' : '');
  }
}

// Each check on the open tab marks the field it names (the longest
// data-path that contains its field) until it is fixed: a red edge and the
// message under the field, so a jump's flash is not the only sign.
// On the Maps tab the open map's checks are marked (its panel shows one
// map of several; the others' stay in the Checks list).
function markFieldIssues() {
  const panel = $('xw-panel');
  if (!panel) return;
  for (const e of panel.querySelectorAll('.xw-has-issue')) e.classList.remove('xw-has-issue', 'is-warning');
  for (const e of panel.querySelectorAll('.xw-issue-msg')) e.remove();
  let file = fileOfTab(S.tab);
  if (file === 'maps/') file = openMapId() ? `maps/${openMapId()}.toml` : null;
  if (!file) return;
  const els = [...panel.querySelectorAll('[data-path]')].map((e) => [e, e.getAttribute('data-path')]).filter(([, p]) => p);
  for (const i of S.issues) {
    if (i.path !== file || !i.field) continue;
    let best = null;
    let bestLen = -1;
    for (const [e, p] of els) {
      if ((i.field === p || i.field.startsWith(p + '.') || i.field.startsWith(p + '[')) && p.length > bestLen) { best = e; bestLen = p.length; }
    }
    if (!best) continue;
    const warn = i.severity !== 'error';
    // Red wins: an element with any error is not drawn as a warning.
    const hadError = best.classList.contains('xw-has-issue') && !best.classList.contains('is-warning');
    best.classList.add('xw-has-issue');
    best.classList.toggle('is-warning', warn && !hadError);
    // A bare control or a grid takes the edge only: a message inside would
    // break its layout (the Checks list still says it).
    if (best.matches('input,select,textarea,.xw-split,.xw-grid,.xw-list-ed,details')) continue;
    const rest = i.field.slice(bestLen).replace(/^[.]/, '');
    best.append(h('span', { class: 'xw-issue-msg' + (warn ? ' is-warning' : '') }, rest ? h('code', {}, rest + ' ') : null, i.message));
  }
}

function finishJump(panel) {
  const field = S.jump.field || '';
  S.jump = null;
  let best = null;
  let bestLen = -1;
  for (const el of panel.querySelectorAll('[data-path]')) {
    const p = el.getAttribute('data-path');
    if ((field === p || field.startsWith(p + '.') || field.startsWith(p + '[') || p === '') && p.length > bestLen) {
      best = el;
      bestLen = p.length;
    }
  }
  if (!best) return;
  for (let d = best.closest('details'); d; d = d.parentElement && d.parentElement.closest('details')) d.open = true;
  best.scrollIntoView({ block: 'center' });
  best.classList.add('xw-flash');
  setTimeout(() => best.classList.remove('xw-flash'), 1600);
  const input = best.matches('input,select,textarea') ? best : best.querySelector('input,select,textarea');
  if (input) input.focus({ preventScroll: true });
}

function jumpTo(issue) {
  S.tab = tabForPath(issue.path);
  S.jump = { file: issue.path, field: issue.field };
  rerender();
}

/* The authoring hints over the pack as it is: walk-through props and
 * walk-on text. A walk-through judgement needs the base scene's geometry
 * pack; one not built yet is built a scene at a time while the page is
 * idle, and the hints redraw as each lands. */
function computeHints() {
  if (!S.pack) { S.hints = []; return; }
  try {
    S.hints = authoringHints(S.pack, (scene) => S.sceneObj[scene] || null);
  } catch (e) {
    S.hints = [];
  }
  if (!S.cat || !S.wb || S.hintLoad) return;
  const todo = hintScenes(S.pack).filter((sc) => !S.sceneObj[sc]);
  if (!todo.length) return;
  // The worker builds it off this thread when it is up.
  if (V.worker && V.ready) {
    S.hintLoad = todo[0];
    V.worker.postMessage({ type: 'scene_objects', scene: todo[0] });
    return;
  }
  S.hintLoad = setTimeout(() => {
    S.hintLoad = null;
    try { S.sceneObj[todo[0]] = JSON.parse(S.wb.scene_objects(todo[0])); } catch (e) { S.sceneObj[todo[0]] = { scene: todo[0], objects: [] }; }
    computeHints();
    renderIssues();
  }, 400);
}

function renderIssues() {
  const box = $('xw-issues');
  box.innerHTML = '';
  const errs = S.issues.filter((i) => i.severity === 'error').length;
  const warns = S.issues.length - errs;
  const hints = S.hints || [];
  $('xw-issue-sum').textContent = S.checking && !S.issues.length ? 'checking ...'
    : `${errs} error(s), ${warns} warning(s)${hints.length ? `, ${hints.length} hint(s)` : ''}${S.cat ? '' : ' - load a disc to check coordinates'}`;
  if (hints.length) box.append(renderHints(hints));
  if (!S.issues.length) {
    box.append(S.checking ? h('div', { class: 'xw-dim' }, 'Checking the pack ...') : h('div', { class: 'xw-ok' }, 'No issues: the pack loads.'));
    return;
  }
  const by = new Map();
  for (const i of S.issues) {
    if (!by.has(i.path)) by.set(i.path, []);
    by.get(i.path).push(i);
  }
  for (const [path, list] of by) {
    box.append(h('div', { class: 'xw-issue-file' }, path));
    for (const i of list) {
      box.append(h('button', { type: 'button', class: 'xw-issue is-' + i.severity, onclick: () => jumpTo(i) },
        i.field ? h('code', {}, i.field + ' ') : null, i.message));
    }
  }
}

/* The hints as a fold under the checks: not errors, not warnings - a pack
 * with hints exports and validates as it is. A click opens what it names
 * (a prop, a trigger) selected in the painter. */
function renderHints(hints) {
  const walk = hints.filter((i) => i.kind === 'walk').length;
  const text = hints.length - walk;
  const det = h('details', { class: 'xw-hints' });
  try { det.open = localStorage.getItem('xw-hints-open') !== '0'; } catch (e) { det.open = true; }
  det.addEventListener('toggle', () => { try { localStorage.setItem('xw-hints-open', det.open ? '1' : '0'); } catch (e) { /* no storage */ } });
  det.append(h('summary', {}, `Hints: ${[walk ? `${walk} walk-through` : '', text ? `${text} walk-on text` : ''].filter(Boolean).join(', ')}`,
    h('span', { class: 'xw-hint' }, ' advice, not checks: the pack exports as it is')));
  let path = null;
  for (const i of hints) {
    if (i.path !== path) { path = i.path; det.append(h('div', { class: 'xw-issue-file' }, path)); }
    det.append(h('button', { type: 'button', class: 'xw-issue is-hint', 'data-kind': i.kind, onclick: () => jumpTo(i) },
      i.field ? h('code', {}, i.field + ' ') : null, i.message));
  }
  return det;
}

// The path of the JSON value under character `at` of JSON.stringify(v): the
// same output, written here with each value's span recorded.
function pathAtOffset(v, at) {
  let pos = 0;
  let hit = null;
  const walk = (x, path) => {
    const start = pos;
    if (Array.isArray(x)) {
      pos += 1;
      x.forEach((e, i) => {
        if (i) pos += 1;
        walk(e === undefined || typeof e === 'function' ? null : e, path.concat(i));
      });
      pos += 1;
    } else if (x && typeof x === 'object') {
      pos += 1;
      let first = true;
      for (const [k, e] of Object.entries(x)) {
        if (e === undefined || typeof e === 'function') continue;
        if (!first) pos += 1;
        first = false;
        pos += JSON.stringify(k).length + 1;
        walk(e, path.concat(k));
      }
      pos += 1;
    } else {
      pos += JSON.stringify(x).length;
    }
    // The innermost span holding `at` wins: children are visited first.
    if (!hit && at >= start && at < Math.max(pos, start + 1)) hit = path;
  };
  walk(v, []);
  return hit;
}

// The session reports a value its types refuse (a 999 in a u8 field) as the
// whole pack failing to parse, at a column of the JSON it was handed. That
// column names the value: turn it into an ordinary check on that field.
function placeParseError(issue) {
  const m = issue.path === '(workbench)' && /^the editor's pack does not parse: (.*) at line 1 column (\d+)$/.exec(issue.message);
  if (!m) return issue;
  // serde's column is at or just past the value's last character.
  const col = parseInt(m[2], 10);
  const a = pathAtOffset(S.pack, col - 1) || [];
  const b = pathAtOffset(S.pack, col - 2) || [];
  const path = b.length > a.length ? b : a;
  if (!path || !path.length) return issue;
  const at = locate(path);
  const RANGE = { u8: '0 to 255', i8: '-128 to 127', u16: '0 to 65535', i16: '-32768 to 32767', u32: '0 to 4294967295' };
  const why = m[1].replace(/expected (u8|i8|u16|i16|u32)\b/, (_, t) => `expected a whole number from ${RANGE[t]}`);
  return { severity: 'error', path: at.file, field: at.field, message: `not a value this field takes: ${why}` };
}

/* The validator runs a moment after each edit. It costs a few hundred
 * milliseconds on a pack of Rebirth's size, so it runs in a worker
 * (expansion-workbench-validate-worker.js) on a second session built from
 * this one's disc files and pack files: typing never waits for it. An
 * answer for a pack that has changed since is dropped (a newer one is on
 * its way). Without the worker (an older bundle, a worker that failed) it
 * runs here. An export or a hand-off still validates here, at once. */
const V = { worker: null, ready: false, seq: 0, gen: 0, synced: -1, edited: false, ms: [], bundleSeq: 0, bundles: new Map(), bundleMs: [], previewSeq: 0, previews: new Map(), previewMs: [] };

// Count every change to the session's own files (the loaded tree, assets,
// documents), so the worker is handed them again before its next check.
function trackSession(wb) {
  for (const k of ['begin_files', 'add_file', 'finish_files', 'finish_files_unchecked', 'load_bundle', 'import_bundle', 'start_fresh', 'add_asset', 'set_doc']) {
    const f = wb[k];
    if (typeof f !== 'function') continue;
    wb[k] = function (...a) { V.gen++; return f.apply(wb, a); };
  }
  return wb;
}

function stopValidator() {
  if (V.worker) V.worker.terminate();
  V.worker = null;
  V.ready = false;
  if (typeof S.hintLoad === 'string') S.hintLoad = null;
  // A bundle still owed by the worker is built here instead.
  const owed = [...V.bundles.values()];
  V.bundles.clear();
  for (const b of owed) {
    try { b.res(S.wb.export_bundle(b.json)); } catch (e) { b.rej(e); }
  }
  // ... and the Find box's sites and a map preview.
  if (xrefAsk.done) { const done = xrefAsk.done; xrefAsk.done = null; done(); }
  const waiting = [...V.previews.values()];
  V.previews.clear();
  for (const p of waiting) {
    let pv;
    try { pv = JSON.parse(S.wb.map_preview(p.json)); } catch (e) { pv = { error: String(e) }; }
    p.done(pv);
  }
}

/* Hand the worker the session's files when they changed since it had them. */
function syncWorkerFiles() {
  if (V.synced !== V.gen) {
    V.worker.postMessage({ type: 'files', data: S.wb.session_files() });
    V.synced = V.gen;
  }
}

/* `map_preview` in the worker (see ctx.mapPreviewLater). */
function mapPreviewLater(id, json, done) {
  if (!V.worker || !V.ready) {
    let pv;
    try { pv = JSON.parse(S.wb.map_preview(json)); } catch (e) { pv = { error: String(e) }; }
    done(pv);
    return;
  }
  syncWorkerFiles();
  const seq = ++V.previewSeq;
  V.previews.set(id, { seq, done, json });
  V.worker.postMessage({ type: 'map_preview', seq, map: id, json });
}

/* The `.lxpack` of the pack as it is now, built in the worker when it is
 * up (the 3D preview's rebuilds; the page's thread is not held), else
 * here. Rejects as `export_bundle` throws while the pack has an error. */
function exportBundleAsync() {
  const json = JSON.stringify(S.pack);
  if (!V.worker || !V.ready) return new Promise((res) => res(S.wb.export_bundle(json)));
  syncWorkerFiles();
  const seq = ++V.bundleSeq;
  return new Promise((res, rej) => {
    V.bundles.set(seq, { res, rej, json });
    V.worker.postMessage({ type: 'bundle', seq, json });
  });
}

function startValidator(parts) {
  stopValidator();
  if (typeof Worker !== 'function' || !parts || !parts.length) return;
  let w;
  try {
    w = new Worker(new URL('./expansion-workbench-validate-worker.js', import.meta.url), { type: 'module' });
  } catch (e) {
    return;
  }
  V.worker = w;
  V.synced = -1;
  const v = window.LEGAIA_WASM_V || '0';
  w.onmessage = (ev) => {
    const m = ev.data || {};
    if (w !== V.worker) return;
    if (m.type === 'disc') {
      V.ready = true;
      // Anything edited while the worker was starting.
      if (S.pack) scheduleValidate();
    } else if (m.type === 'issues') {
      V.ms.push(m.ms);
      if (V.ms.length > 20) V.ms.shift();
      // Stale when another check was asked for since (an edit came after).
      if (m.seq !== V.seq || V.edited) return;
      applyIssues(JSON.parse(m.issues));
    } else if (m.type === 'scene_objects') {
      if (!S.sceneObj[m.scene]) {
        try { S.sceneObj[m.scene] = m.error ? { scene: m.scene, objects: [] } : JSON.parse(m.json); } catch (e) { S.sceneObj[m.scene] = { scene: m.scene, objects: [] }; }
      }
      S.hintLoad = null;
      computeHints();
      renderIssues();
    } else if (m.type === 'xref_sites') {
      if (m.seq !== xrefAsk.seq) return;
      const done = xrefAsk.done;
      xrefAsk.done = null;
      if (m.json != null) {
        try { xrefCache = { json: xrefAsk.json, sites: JSON.parse(m.json).sites }; } catch (e) { /* built here on demand */ }
      }
      if (done) done();
    } else if (m.type === 'map_preview') {
      const p = V.previews.get(m.map);
      if (!p || p.seq !== m.seq || m.dropped) return;
      V.previews.delete(m.map);
      V.previewMs.push(m.ms);
      if (V.previewMs.length > 20) V.previewMs.shift();
      p.done(m.preview);
    } else if (m.type === 'bundle') {
      const b = V.bundles.get(m.seq);
      if (!b) return;
      V.bundles.delete(m.seq);
      if (m.error != null) { b.rej(new Error(m.error)); return; }
      V.bundleMs.push(m.ms);
      if (V.bundleMs.length > 20) V.bundleMs.shift();
      b.res(m.bytes);
    } else if (m.type === 'error') {
      console.warn('expansion workbench: the validation worker stopped (' + m.message + '); validating on the page');
      stopValidator();
      if (S.pack) scheduleValidate();
    }
  };
  w.onerror = (e) => {
    if (w !== V.worker) return;
    console.warn('expansion workbench: the validation worker failed to start; validating on the page', e.message || '');
    stopValidator();
    if (S.pack) scheduleValidate();
  };
  w.postMessage({
    type: 'init',
    glue: new URL('../wasm/legaia_web_viewer.js?v=' + v, import.meta.url).href,
    bin: new URL('../wasm/legaia_web_viewer_bg.wasm?v=' + v, import.meta.url).href,
  });
  w.postMessage({ type: 'disc', parts: parts.buffer }, [parts.buffer]);
}

let validateTimer = null;
function scheduleValidate() {
  V.edited = true;
  V.current = false;
  clearTimeout(validateTimer);
  validateTimer = setTimeout(validateSoon, 250);
}

function validateSoon() {
  if (!S.wb || !S.pack) return;
  if (!V.worker || !V.ready) { runValidate(); return; }
  syncWorkerFiles();
  V.edited = false;
  V.seq++;
  V.worker.postMessage({ type: 'validate', seq: V.seq, json: JSON.stringify(S.pack) });
}

function runValidate() {
  if (!S.wb || !S.pack) return;
  // A worker answer still on its way is for this pack or an older one.
  V.seq++;
  let issues;
  try {
    issues = JSON.parse(S.wb.validate(JSON.stringify(S.pack)));
  } catch (e) {
    issues = [{ severity: 'error', path: '(workbench)', field: '', message: errText(e) }];
  }
  applyIssues(issues);
}

function applyIssues(raw) {
  // Every path here answers the pack as it is now (a stale worker answer
  // never reaches it), until the next edit.
  V.current = true;
  S.checking = false;
  try {
    S.issues = raw.map(placeParseError);
  } catch (e) {
    S.issues = [{ severity: 'error', path: '(workbench)', field: '', message: errText(e) }];
  }
  computeHints();
  renderIssues();
  renderTabs();
  markFieldIssues();
  if (S.tab === 'progression') updateChart();
  if (S.share) S.share.refresh();
}

function updateChart() {
  try {
    S.chart = JSON.parse(S.wb.xp_chart(JSON.stringify(S.pack.progression)));
  } catch (e) {
    S.chart = null;
  }
  drawXpChart(document.getElementById('xw-xp-chart'), S.chart);
}

function renderFiles(c, el) {
  el.append(h('p', { class: 'xw-note' }, 'The pack as the files it exports to. A browser cannot save a folder, so take a file at a time here, or the whole pack as one .lxpack. Files you did not change keep their original text and comments.'));
  let files;
  try {
    files = JSON.parse(S.wb.files_view(JSON.stringify(S.pack)));
  } catch (e) {
    el.append(h('div', { class: 'wb-status is-err' }, errText(e)));
    return;
  }
  for (const f of files) {
    const pre = f.text != null ? h('pre', { class: 'xw-pre' }, f.text) : h('div', { class: 'xw-note' }, `binary, ${f.size} bytes`);
    const name = f.path.split('/').pop();
    const box = h('details', { class: 'xw-box', 'data-path': '' },
      h('summary', { class: 'xw-box-head' }, h('code', {}, f.path), h('span', { class: 'xw-hint' }, ` ${f.size} bytes`)),
      h('div', { class: 'xw-tools' },
        f.text != null ? h('button', {
          type: 'button', class: 'wb-mini', onclick: async (ev) => {
            try { await navigator.clipboard.writeText(f.text); ev.target.textContent = 'Copied'; } catch (e) { ev.target.textContent = 'Copy failed'; }
          },
        }, 'Copy') : null,
        f.text != null ? h('button', { type: 'button', class: 'wb-mini', onclick: () => downloadFile(new Blob([f.text], { type: 'text/plain' }), name) }, 'Download') : null),
      pre);
    el.append(box);
  }
}

// --- Persistence ----------------------------------------------------------------------
let saveTimer = null;
function scheduleSave() {
  clearTimeout(saveTimer);
  if (S.saved.state !== 'failed') { S.saved.state = 'saving'; updateDirty(); }
  saveTimer = setTimeout(async () => {
    if (!S.pack) return;
    const at = Date.now();
    let ok = null;
    const json = JSON.stringify(S.pack);
    const edited = S.clean == null || json !== S.clean;
    const rec = { json, label: S.label, savedAt: at, tab: S.tab, edited, clean: edited ? S.clean : null };
    try { ok = await savePut('current', rec); } catch (e) { ok = null; }
    // A put resolves `undefined`-ish on success (the key), null on failure.
    S.saved = ok === null ? { at, state: 'failed' } : { at, state: 'saved' };
    updateDirty();
  }, 700);
}

async function offerResume() {
  const box = $('xw-resume');
  let saved = null;
  try { saved = await saveGet('current'); } catch (e) { saved = null; }
  if (!saved || !saved.json) { box.hidden = true; return; }
  let pack;
  try { pack = JSON.parse(saved.json); } catch (e) { box.hidden = true; return; }
  const when = new Date(saved.savedAt).toLocaleString();
  const name = esc(pack.manifest.name || pack.manifest.id);
  // An autosave from before `edited` was recorded counts as edited.
  const edited = saved.edited !== false;
  box.innerHTML = edited
    ? `<span>You have unsaved changes to <strong>${name}</strong> in this browser (${esc(when)}).</span>
    <button type="button" class="wb-button" id="xw-resume-go">Resume editing</button>
    <button type="button" class="wb-button wb-button-ghost" id="xw-resume-drop">Discard changes</button>`
    : `<span>Pick up where you left off: <strong>${name}</strong> (${esc(saved.label || 'pack')}, ${esc(when)}).</span>
    <button type="button" class="wb-button" id="xw-resume-go">Reopen</button>
    <button type="button" class="wb-button wb-button-ghost" id="xw-resume-drop">Dismiss</button>`;
  box.hidden = false;
  $('xw-resume-go').onclick = async () => {
    box.hidden = true;
    const src = await saveGet('source');
    if (src && src.bundle) {
      S.wb.load_bundle(src.bundle);
    } else if (src && src.files) {
      S.wb.begin_files();
      for (const [p, b] of Object.entries(src.files)) S.wb.add_file(p, b);
      S.wb.finish_files();
    } else {
      S.wb.start_fresh(pack.manifest.id, pack.manifest.name);
    }
    showPack(pack, saved.label || 'resumed', `Resumed ${pack.manifest.name || pack.manifest.id}${edited ? ' with its unsaved changes' : ''}.`,
      { tab: saved.tab, clean: edited ? (saved.clean || '') : null });
  };
  $('xw-resume-drop').onclick = async () => {
    if (edited && !window.confirm(`Discard the unsaved changes to ${pack.manifest.name || pack.manifest.id}? This cannot be undone.`)) return;
    box.hidden = true;
    await saveDel('current');
    await saveDel('source');
  };
}

// --- Loading ----------------------------------------------------------------------------
async function ensureWasm() {
  const mod = await window.LegaiaWasm.load();
  if (typeof mod.ExpansionWorkbench !== 'function') {
    throw new Error('this copy of the site\'s WebAssembly bundle predates the expansion workbench. Rebuild it (scripts/ci/build-wasm.sh) and reload the page.');
  }
  return mod;
}

async function openDisc(file) {
  setStatus(`Reading ${file.name} (nothing is uploaded) ...`);
  try {
    const mod = await ensureWasm();
    const buf = new Uint8Array(await file.arrayBuffer());
    setStatus('Reading the monster archive, names and scenes on your disc ...');
    await tick();
    if (S.wb) { try { S.wb.free(); } catch (e) { /* already freed */ } }
    S.wb = trackSession(new mod.ExpansionWorkbench());
    // The validation worker opens its own session from the disc files this
    // one keeps (an older bundle has no worker: validation stays here).
    const keep = typeof S.wb.open_disc_keep === 'function';
    S.cat = JSON.parse(keep ? S.wb.open_disc_keep(buf) : S.wb.open_disc(buf));
    if (keep) startValidator(S.wb.take_disc_parts());
    S.discName = file.name;
    setStatus(`Disc read: ${file.name} - ${S.cat.monsters.length} monsters, ${S.cat.items.length} items, ${S.cat.scenes.length} scenes.${S.pack ? '' : ' Pick a starting point below.'}`, 'ok');
    updateStart();
    updateSetup();
    // A start chosen before the disc was in runs now, in place of the
    // resume offer (the choice was the newer intent).
    const pending = S.pending;
    S.pending = null;
    if (pending) { await pending.run(); return; }
    await offerResume();
  } catch (e) {
    setStatus('Error: ' + errText(e), 'err');
  }
}

function needDisc() {
  if (S.wb) return false;
  setStatus('Choose your disc image first.', 'err');
  focusDisc();
  return true;
}

// The disc picker, unfolded and focused.
function focusDisc() {
  const setup = $('xw-setup');
  if (setup) setup.open = true;
  const input = $('xw-file');
  input.scrollIntoView({ block: 'center' });
  input.focus({ preventScroll: true });
}

// A start action before the disc is read waits for it, instead of
// refusing: the newcomer who clicks a pack first and picks the disc second
// gets the pack.
function whenDisc(label, run, card) {
  if (S.wb) return run();
  S.pending = { label, run, card };
  setStatus(`Choose your disc image: ${label} as soon as it is read.`, 'ok');
  updateStart();
  focusDisc();
  return null;
}

// The start panel's step 1 says whether the disc is in; a queued choice
// stays outlined until it runs.
function updateStart() {
  const step = $('xw-start-disc');
  const text = $('xw-start-disc-text');
  if (!step || !text) return;
  step.classList.toggle('is-done', !!S.wb);
  text.textContent = S.wb
    ? `Disc read: ${S.discName || 'your disc'}. It stays in this tab.`
    : 'Choose your disc image above. It is read in this tab and never uploaded.';
  const box = $('xw-empty');
  box.classList.toggle('is-waiting', !!S.pending);
  for (const c of box.querySelectorAll('.xw-start-card')) c.classList.toggle('is-queued', !!S.pending && c.dataset.start === S.pending.card);
}

// The disc and pack pickers fold to a one-line summary once a pack is open.
function updateSetup() {
  const setup = $('xw-setup');
  if (!setup) return;
  const st = $('xw-setup-state');
  const parts = [];
  if (S.discName) parts.push(S.discName);
  if (S.pack) parts.push(`${S.pack.manifest.name || S.pack.manifest.id} (${S.label || 'pack'})`);
  if (st) st.textContent = parts.join(' - ');
  setup.classList.toggle('is-foldable', !!S.pack);
}

function showPack(pack, label, msg, opts) {
  const o = opts || {};
  S.pack = pack;
  S.label = label;
  S.sel = {};
  ctx.sel = S.sel;
  if (o.tab && TABS.some((t) => t[0] === o.tab)) S.tab = o.tab;
  $('xw-main').hidden = false;
  $('xw-empty').hidden = true;
  $('xw-new').hidden = true;
  resetHistory();
  // The checks of a pack just opened: in the worker when it is up (the
  // list the load itself made stands until they land), else here.
  if (V.worker && V.ready) {
    applyIssues(o.issues || []);
    V.current = false;
    if (!S.issues.length) { S.checking = true; renderIssues(); }
    clearTimeout(validateTimer);
    validateSoon();
  } else {
    runValidate();
  }
  rerender();
  resetHistory();
  // A resumed pack keeps its unsaved state: `clean` is what it was opened as.
  if (o.clean != null) { S.clean = o.clean; updateDirty(); }
  setStatus(msg, 'ok');
  scheduleSave();
  updateSetup();
  const setup = $('xw-setup');
  if (setup) setup.open = false;
  // The editor, not the page's header, fills the screen.
  const work = $('xw-main');
  const top = work.getBoundingClientRect().top + window.scrollY - (parseFloat(getComputedStyle(document.documentElement).getPropertyValue('--topbar-h')) || 56) - 8;
  window.scrollTo({ top: Math.max(0, top) });
}

async function loadResult(res, label, tree) {
  if (!res.pack) {
    S.issues = res.issues;
    renderIssues();
    const first = res.issues.find((i) => i.severity === 'error');
    setStatus(`${label} does not load: ${first ? `${first.path}: ${first.message}` : 'unknown error'}`, 'err');
    return;
  }
  $('xw-resume').hidden = true;
  // What a resume reloads the session from: the tree, or the bundle.
  await savePut('source', Object.assign({ label, savedAt: Date.now() }, tree.bundle ? { bundle: tree.bundle } : { files: tree }));
  showPack(res.pack, label, `${label}: ${res.files.length} file(s) loaded.`, { issues: res.issues });
}

async function loadTree(tree, label) {
  S.wb.begin_files();
  for (const [p, b] of Object.entries(tree)) S.wb.add_file(p, b);
  // With the worker up the checks run there (showPack asks for them): the
  // load itself only says why a tree does not load.
  const quick = V.worker && V.ready && typeof S.wb.finish_files_unchecked === 'function';
  await loadResult(JSON.parse(quick ? S.wb.finish_files_unchecked() : S.wb.finish_files()), label, tree);
}

async function loadRebirth(as) {
  if (needDisc()) return;
  setStatus('Fetching the Rebirth example ...');
  try {
    const res = await fetch(new URL('../packs/rebirth.json', import.meta.url).href);
    if (!res.ok) throw new Error(`could not load rebirth.json (${res.status})`);
    const doc = await res.json();
    const enc = new TextEncoder();
    const tree = {};
    for (const f of doc.files) {
      tree[f.path] = f.text != null ? enc.encode(f.text) : Uint8Array.from(atob(f.base64), (c) => c.charCodeAt(0));
    }
    await loadTree(tree, as && as.id ? `clone of Rebirth` : 'Rebirth example');
    if (as && as.id && S.pack) {
      // A clone: the same content as a pack of the modder's own.
      Object.assign(S.pack.manifest, { id: as.id, name: as.name, version: '0.1.0' });
      ctx.changed(true);
      // A clone has never been downloaded: it starts unsaved, and its first
      // step (the new manifest) is not an undo back into Rebirth.
      S.hist.past = [];
      S.hist.bytes = 0;
      S.clean = '';
      S.label = 'clone of Rebirth';
      updateDirty();
      updateSetup();
      setStatus(`Started ${as.name} as a clone of Rebirth: every section is Rebirth's, the manifest is yours. Unchanged files keep Rebirth's comments.`, 'ok');
    }
  } catch (e) {
    setStatus('Error: ' + errText(e), 'err');
  }
}

// An export, a play or a hand-off is refused while the pack has an error:
// say so and open the first error's field rather than only quoting it.
function blockedByErrors(what) {
  // The checks already answer the pack as it stands unless an edit came
  // after them.
  if (!V.current) {
    clearTimeout(validateTimer);
    runValidate();
  }
  const errs = S.issues.filter((i) => i.severity === 'error');
  if (!errs.length) return false;
  S.issueAt = 0;
  jumpTo(errs[0]);
  setStatus(`${what}: the pack has ${errs.length} error(s). The first is highlighted below; F8 steps through the rest.`, 'err', 'xw-export-status');
  return true;
}

async function exportBundle() {
  return S.wb.export_bundle(JSON.stringify(S.pack));
}

// The play hand-off goes through the play page's pack library
// (js/play-pack-library.js): the bundle is installed there under the pack's
// id - an older version replaced, as an install from a file does - and made
// the active pack, and the play page opens on it (`?expansion=1&lib=<id>`).
// The old `play` slot is still written, for a play page that predates the
// library.
async function installInLibrary(bytes) {
  const m = S.pack.manifest;
  const lib = window.LegaiaPackLibrary;
  await idb(PLAY_DB, PLAY_STORE, 'readwrite', (s) => s.put({ id: m.id, name: m.name, version: m.version, bytes, savedAt: Date.now() }, PLAY_KEY));
  if (!lib) return { installed: false, prev: null };
  const prev = await lib.get(m.id);
  const rel = m.cover && m.cover.startsWith('assets/') ? m.cover.slice(7) : null;
  let cover = null;
  try { cover = rel ? S.wb.asset_bytes(rel) : null; } catch (e) { cover = null; }
  const ok = await lib.put(lib.recordFor(bytes, m, cover));
  if (ok === null && !window.indexedDB) return { installed: false, prev };
  await lib.setActive(m.id);
  return { installed: true, prev };
}

function playLibUrl(map) {
  const id = encodeURIComponent(S.pack.manifest.id);
  const q = map ? `&map=${encodeURIComponent(map)}` : '';
  return new URL(`../play.html?expansion=1&lib=${id}${q}`, import.meta.url).href;
}

// Play here's fallback with no play tab open: the library, then a tab.
function storePlay(bytes) {
  return installInLibrary(bytes);
}

// Play here: the map the Maps tab is on, in a play tab - hot-reloaded in
// place when one is open on the pack.
async function playHere() {
  if (blockedByErrors('Not sent to a play tab')) return;
  const map = S.preview && S.preview.state().map;
  if (!map) {
    setStatus('Open a map on the Maps tab first.', 'err', 'xw-export-status');
    return;
  }
  try {
    const r = await S.preview.playHere(map);
    if (r.reloaded) {
      const a = r.answer;
      setStatus(a.ok
        ? (a.reentered ? `Reloaded in the open play tab on ${a.map} (${a.ms} ms); the player kept their place.` : `Reloaded in the open play tab${a.map ? ` (it is on ${a.map})` : ''}.`)
        : `The play tab refused the pack: ${(a.errors || []).slice(0, 2).join(' | ')}`, a.ok ? 'ok' : 'err', 'xw-export-status');
    } else {
      setStatus(`No play tab was open on this pack: opened one on ${map}.`, 'ok', 'xw-export-status');
    }
  } catch (e) {
    setStatus('Not sent: ' + errText(e), 'err', 'xw-export-status');
  }
}

async function playInBrowser() {
  if (blockedByErrors('Not played')) return;
  try {
    const bytes = await exportBundle();
    const m = S.pack.manifest;
    const r = await installInLibrary(bytes);
    if (!r.installed && !window.indexedDB) throw new Error('this browser has no IndexedDB');
    const was = r.prev && r.prev.version && r.prev.version !== m.version ? ` (updated from ${r.prev.version})` : '';
    setStatus(`${m.name} ${m.version} is in the play library and active${was}; the play page opens on it.`, 'ok', 'xw-export-status');
    window.open(playLibUrl(null), '_blank', 'noopener');
  } catch (e) {
    setStatus('Not stored: ' + errText(e), 'err', 'xw-export-status');
  }
}

function setupInfoTips() {
  const openTips = () => document.querySelectorAll('.info-tip.is-open');
  const close = (tip) => { tip.classList.remove('is-open'); tip.setAttribute('aria-expanded', 'false'); };
  document.addEventListener('click', (e) => {
    const tip = e.target.closest('.info-tip');
    if (!tip) { openTips().forEach(close); return; }
    if (e.target.closest('a')) return;
    e.preventDefault();
    e.stopPropagation();
    const open = !tip.classList.contains('is-open');
    openTips().forEach((t) => { if (t !== tip) close(t); });
    tip.classList.toggle('is-open', open);
    tip.setAttribute('aria-expanded', String(open));
  });
  document.addEventListener('keydown', (e) => {
    if (e.key === 'Escape') openTips().forEach(close);
  });
}

// --- Wiring --------------------------------------------------------------------------
function init() {
  setupInfoTips();
  const fileInput = $('xw-file');
  if (window.RomCache) {
    window.RomCache.attach(fileInput, { onLoad: (f) => openDisc(f) });
  } else {
    fileInput.addEventListener('change', () => {
      const f = fileInput.files && fileInput.files[0];
      if (f) openDisc(f);
    });
  }
  $('xw-rebirth').addEventListener('click', () => whenDisc('the Rebirth example opens', () => loadRebirth(), 'rebirth'));
  // The start panel's cards.
  $('xw-empty').addEventListener('click', (ev) => {
    const b = ev.target.closest('[data-start]');
    if (!b) return;
    const what = b.dataset.start;
    if (what === 'rebirth') whenDisc('the Rebirth example opens', () => loadRebirth(), 'rebirth');
    else if (what === 'new') openNewForm();
    else if (what === 'bundle') { if (!needDisc()) $('xw-bundle-file').click(); }
    else if (what === 'folder') { if (!needDisc()) $('xw-dir-file').click(); }
  });
  $('xw-bundle-file').addEventListener('change', async (ev) => {
    const f = ev.target.files && ev.target.files[0];
    ev.target.value = '';
    if (!f || needDisc()) return;
    // Through the import gates (expansion-workbench-share.js): a refused
    // bundle says which gate and why.
    await S.share.importBundle(new Uint8Array(await f.arrayBuffer()), f.name);
  });
  $('xw-dir-file').addEventListener('change', async (ev) => {
    const list = [...(ev.target.files || [])];
    ev.target.value = '';
    if (!list.length || needDisc()) return;
    const tree = {};
    for (const f of list) {
      const rel = f.webkitRelativePath || f.name;
      // Skip what a pack folder carries beside its files (VCS, OS litter).
      if (/(^|\/)(\.git|\.DS_Store|Thumbs\.db)(\/|$)/.test(rel)) continue;
      tree[rel] = new Uint8Array(await f.arrayBuffer());
    }
    const root = Object.keys(tree)[0] ? Object.keys(tree)[0].split('/')[0] : 'folder';
    await loadTree(tree, `folder ${root}`);
  });
  $('xw-fresh').addEventListener('click', () => {
    if ($('xw-new').hidden) openNewForm();
    else $('xw-new').hidden = true;
  });
  $('xw-new-go').addEventListener('click', newPack);
  $('xw-new').addEventListener('keydown', (ev) => {
    if (ev.key === 'Enter' && ev.target.tagName === 'INPUT') { ev.preventDefault(); newPack(); }
    if (ev.key === 'Escape') $('xw-new').hidden = true;
  });
  $('xw-new-cancel').addEventListener('click', () => { $('xw-new').hidden = true; });
  $('xw-dl').addEventListener('click', downloadBundle);
  $('xw-undo').addEventListener('click', () => stepHistory(-1));
  $('xw-redo').addEventListener('click', () => stepHistory(1));
  $('xw-keys').addEventListener('click', () => { $('xw-keyhelp').hidden = !$('xw-keyhelp').hidden; });
  S.find = createFind($('xw-findbox'), {
    pack: () => S.pack,
    sites: () => xrefSites(),
    jump: jumpTo,
    rename: { plan: ctx.renamePlan, apply: ctx.renameApply, done: (res, msg) => ctx.replacePack(res.pack, msg) },
  });
  document.addEventListener('keydown', onShortcut);
  window.addEventListener('beforeunload', (ev) => {
    // The autosave keeps the work; warn only when it could not.
    if (S.pack && S.saved.state === 'failed' && S.clean != null && JSON.stringify(S.pack) !== S.clean) { ev.preventDefault(); ev.returnValue = ''; }
  });
  initRest();
}

async function downloadBundle() {
  // Built in the worker when it is up; the export refuses a pack with an
  // error, and only then do the checks run here to open the first one.
  if (V.current && blockedByErrors('Not downloaded')) return;
  try {
    let bytes;
    try {
      bytes = await exportBundleAsync();
    } catch (e) {
      if (blockedByErrors('Not downloaded')) return;
      throw e;
    }
    const name = `${S.pack.manifest.id || 'pack'}.lxpack`;
    downloadFile(new Blob([bytes], { type: 'application/octet-stream' }), name);
    setStatus(`Downloaded ${name} (${bytes.length} bytes). It holds no game data: share it freely.`, 'ok', 'xw-export-status');
    S.clean = JSON.stringify(S.pack);
    updateDirty();
  } catch (e) {
    setStatus('Not exported: ' + errText(e), 'err', 'xw-export-status');
  }
}

// --- New pack from a template ----------------------------------------------------------------
// The minimal template: one of everything a first pack wires together - a
// derived monster in a formation, a dialogue line, a quest to win that
// fight, and a small map whose trigger zones show the line and start the
// fight. Every id is the template's own; the disc supplies the base scene.
function minimalPack(pack) {
  const cat = S.cat || {};
  const mon = (cat.monsters && cat.monsters[0]) || { id: 1 };
  const scene = (cat.scenes && cat.scenes[0] && cat.scenes[0].name) || 'town01';
  pack.battle.monsters = { first_monster: { base: { kind: 'monster', id: mon.id }, name: 'First Monster', hp: { mul: 1.5 } } };
  pack.battle.formations = { first_fight: { label: 'First Fight', monsters: ['first_monster'] } };
  pack.dialog.lines = { welcome: { speaker: 'Guide', text: 'Welcome to my expansion.\nStep onto the red tiles to fight.' } };
  pack.quests.quests = [{
    id: 'first_quest', title: 'First Quest', description: 'Win the first fight.', auto_start: true, ordered: true,
    objectives: [{ kind: 'defeat_formation', formation: 'first_fight', count: 1, text: 'Win the first fight' }],
    rewards: { gold: 100 },
  }];
  pack.maps = {
    first_map: {
      name: 'First Map', size: null,
      base: { scene: { kind: 'scene', name: scene }, geometry: 'none' },
      floor_heights: [0, 64],
      tiles: {
        origin: [16, 16],
        rows: ['###########', '#.........#', '#.........#', '#.........#', '#.........#', '###########'],
        legend: { '#': { label: 'wall', walls: 'all', height: 0, ground: null }, '.': { label: 'floor', height: 0, ground: null } },
      },
      props: [], npcs: [], exits: [], encounters: [],
      spawns: [{ id: 'start', tile: [2, 2], facing: 0 }],
      triggers: [
        { id: 'greeting', rect: [3, 2, 3, 2], event: { kind: 'dialog', line: 'welcome' }, once: true },
        { id: 'fight', rect: [7, 2, 8, 3], event: { kind: 'battle', formation: 'first_fight' } },
      ],
      bgm: null, camera: 'default',
    },
  };
  return pack;
}

function openNewForm() {
  const box = $('xw-new');
  box.hidden = false;
  box.scrollIntoView({ block: 'nearest' });
  $('xw-new-id').focus();
  $('xw-new-id').select();
}

async function newPack() {
  if (!S.wb) {
    $('xw-new').hidden = true;
    whenDisc('your pack is created', newPack, 'new');
    return;
  }
  const id = ($('xw-new-id').value.trim() || 'my_pack').toLowerCase().replace(/[^a-z0-9_-]/g, '_');
  const name = $('xw-new-name').value.trim() || 'My expansion';
  const tpl = $('xw-new-tpl').value;
  $('xw-new').hidden = true;
  if (tpl === 'rebirth') {
    await loadRebirth({ id, name });
    return;
  }
  let pack = JSON.parse(S.wb.start_fresh(id, name));
  if (tpl === 'minimal') pack = minimalPack(pack);
  saveDel('source');
  showPack(pack, 'new pack', tpl === 'minimal'
    ? `Started ${name} from the minimal template: a monster, a fight, a line, a quest and a map that ties them together.`
    : `Started ${name}, an empty pack: add content tab by tab.`);
}

// --- Keyboard shortcuts --------------------------------------------------------------------------
const TEXT_INPUT = (t) => t && ((t.tagName === 'INPUT' && !['checkbox', 'radio', 'button', 'range', 'file'].includes(t.type)) || t.tagName === 'TEXTAREA' || t.isContentEditable);

function jumpIssue(dir) {
  const list = S.issues.filter((i) => i.severity === 'error').concat(S.issues.filter((i) => i.severity !== 'error'));
  if (!list.length) return;
  S.issueAt = ((S.issueAt == null ? (dir > 0 ? -1 : 0) : S.issueAt) + dir + list.length) % list.length;
  jumpTo(list[S.issueAt]);
}

function onShortcut(ev) {
  if (!S.pack || $('xw-main').hidden) return;
  const k = ev.key.toLowerCase();
  const mod = ev.ctrlKey || ev.metaKey;
  // Undo / redo: a text field keeps the browser's own while it has focus.
  if (mod && (k === 'z' || k === 'y') && !TEXT_INPUT(ev.target)) {
    ev.preventDefault();
    stepHistory(k === 'y' || ev.shiftKey ? 1 : -1);
    return;
  }
  if (mod && k === 's') { ev.preventDefault(); downloadBundle(); return; }
  if ((mod && k === 'k') || (k === '/' && !TEXT_INPUT(ev.target))) {
    ev.preventDefault();
    S.find.input.focus();
    S.find.input.select();
    return;
  }
  if (k === 'f8') { ev.preventDefault(); jumpIssue(ev.shiftKey ? -1 : 1); return; }
  if (ev.altKey && !mod && /^[0-9]$/.test(ev.key)) {
    const i = ev.key === '0' ? 9 : parseInt(ev.key, 10) - 1;
    if (TABS[i]) { ev.preventDefault(); S.tab = TABS[i][0]; rerender(); }
    return;
  }
  if (ev.altKey && !mod && (ev.key === '[' || ev.key === ']')) {
    ev.preventDefault();
    const i = TABS.findIndex((t) => t[0] === S.tab);
    S.tab = TABS[(i + (ev.key === ']' ? 1 : -1) + TABS.length) % TABS.length][0];
    rerender();
    return;
  }
  if (k === '?' && !TEXT_INPUT(ev.target)) { ev.preventDefault(); $('xw-keyhelp').hidden = !$('xw-keyhelp').hidden; return; }
  if (k === 'escape') { $('xw-keyhelp').hidden = true; }
}

// The pack bar's More menu (a phone folds the secondary buttons into it):
// a press inside runs the button and closes the menu, as does a press
// outside or Escape.
function setupMoreMenu() {
  const btn = $('xw-more');
  const box = $('xw-more-items');
  if (!btn || !box) return;
  const set = (on) => { box.classList.toggle('is-open', on); btn.setAttribute('aria-expanded', String(on)); };
  btn.addEventListener('click', (ev) => { ev.stopPropagation(); set(!box.classList.contains('is-open')); });
  box.addEventListener('click', (ev) => { if (ev.target.closest('button')) set(false); });
  document.addEventListener('click', (ev) => { if (!box.contains(ev.target) && ev.target !== btn) set(false); });
  document.addEventListener('keydown', (ev) => { if (ev.key === 'Escape') set(false); });
}

function initRest() {
  setupMoreMenu();
  $('xw-toml').addEventListener('click', () => { S.tab = 'files'; rerender(); });
  $('xw-play').addEventListener('click', playInBrowser);
  $('xw-play-here').addEventListener('click', playHere);
  $('xw-live').addEventListener('change', (ev) => S.preview && S.preview.setLive(ev.target.checked));
  S.preview = createPreview3d({
    pack: () => S.pack,
    exportBundle: () => S.wb.export_bundle(JSON.stringify(S.pack)),
    exportBundleAsync,
    storePlay,
    playUrl: playLibUrl,
  });
  $('xw-panel').before(S.preview.el);
  S.share = attachShare({
    S,
    changed: (structural) => ctx.changed(structural),
    setStatus,
    errText,
    downloadFile,
    loaded: loadResult,
    loadTree,
    needDisc,
    exportBundle,
    play: playInBrowser,
  });
}

if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', init);
else init();

// Headless-verification hook: the page state (no disc bytes).
window.__xwState = S;
// ... the validation worker's state (headless checks: `__xwValidator.ready`, `.ms`).
window.__xwValidator = V;
// ... and the editors' context: `__xwCtx.changed(true)` after a scripted edit.
window.__xwCtx = ctx;
