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
import { renderMaps } from './expansion-workbench-map.js';
import { renderCutscenes } from './expansion-workbench-cutscenes.js';
import { renderAudio } from './expansion-workbench-audio.js';
import { renderArts, renderSpells } from './expansion-workbench-arts.js';
import { createPreview3d } from './expansion-workbench-preview3d.js';
import { createFind } from './expansion-workbench-xref.js';
import { attachShare } from './expansion-workbench-share.js';
import { addSectionHelp } from './expansion-workbench-help.js';

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
  canUndo: () => S.hist.past.length > 0 || pendingEdit(),
  canRedo: () => S.hist.future.length > 0,
  find: (q) => S.find && S.find.find(q),
  rerender: () => rerender(),
  checkDialog: (line) => {
    try { return JSON.parse(S.wb.check_dialog(JSON.stringify(line))); } catch (e) { return null; }
  },
  artPreview: (art, atk, def) => {
    try { return JSON.parse(S.wb.art_preview(JSON.stringify(art), atk, def)); } catch (e) { return { error: errText(e) }; }
  },
  mapPreview: (map) => {
    try { return JSON.parse(S.wb.map_preview(JSON.stringify(map))); } catch (e) { return { error: String(e) }; }
  },
  cutsceneTimeline: (id, picks) => {
    try {
      return JSON.parse(S.wb.cutscene_timeline(JSON.stringify(S.pack), id, new Uint32Array(picks || [])));
    } catch (e) {
      return { error: errText(e) };
    }
  },
  sceneObjects: (scene) => JSON.parse(S.wb.scene_objects(scene)),
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
  if (S.preview) S.preview.show(['maps', 'cutscenes', 'battle', 'cards'].includes(S.tab));
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
  if (S.jump) finishJump(panel);
  else window.scrollTo(0, y);
  // An editor may fill in a default as it draws (an empty gate box): that is
  // not an edit, so the committed present absorbs it.
  S.hist.cur = JSON.stringify(S.pack);
  if (wasClean) S.clean = S.hist.cur;
  updateDirty();
  if (S.find) S.find.refresh();
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
  if (H.cur == null) { H.cur = now; return; }
  if (now === H.cur) return;
  H.past.push(H.cur);
  H.bytes += H.cur.length;
  H.cur = now;
  H.future = [];
  while (H.past.length > HIST_STEPS || (H.bytes > HIST_BYTES && H.past.length > 1)) H.bytes -= H.past.shift().length;
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

function updateDirty() {
  const el = $('xw-dirty');
  if (!el || !S.pack) return;
  const dirty = S.clean != null && JSON.stringify(S.pack) !== S.clean;
  el.hidden = !dirty;
  const u = $('xw-undo');
  const r = $('xw-redo');
  if (u) u.disabled = !ctx.canUndo();
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

function renderIssues() {
  const box = $('xw-issues');
  box.innerHTML = '';
  const errs = S.issues.filter((i) => i.severity === 'error').length;
  const warns = S.issues.length - errs;
  $('xw-issue-sum').textContent = `${errs} error(s), ${warns} warning(s)${S.cat ? '' : ' - load a disc to check coordinates'}`;
  if (!S.issues.length) { box.append(h('div', { class: 'xw-ok' }, 'No issues: the pack loads.')); return; }
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

let validateTimer = null;
function scheduleValidate() {
  clearTimeout(validateTimer);
  validateTimer = setTimeout(runValidate, 250);
}
function runValidate() {
  if (!S.wb || !S.pack) return;
  try {
    S.issues = JSON.parse(S.wb.validate(JSON.stringify(S.pack)));
  } catch (e) {
    S.issues = [{ severity: 'error', path: '(workbench)', field: '', message: errText(e) }];
  }
  renderIssues();
  renderTabs();
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
    try { ok = await savePut('current', { json: JSON.stringify(S.pack), label: S.label, savedAt: at }); } catch (e) { ok = null; }
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
  box.innerHTML = `<span>You have an unsaved pack in this browser: <strong>${esc(pack.manifest.name || pack.manifest.id)}</strong> (${esc(when)}).</span>
    <button type="button" class="wb-button" id="xw-resume-go">Resume it</button>
    <button type="button" class="wb-button wb-button-ghost" id="xw-resume-drop">Discard</button>`;
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
    showPack(pack, saved.label || 'resumed', `Resumed ${pack.manifest.name || pack.manifest.id}.`);
  };
  $('xw-resume-drop').onclick = async () => {
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
    S.wb = new mod.ExpansionWorkbench();
    S.cat = JSON.parse(S.wb.open_disc(buf));
    setStatus(`Disc read: ${file.name} - ${S.cat.monsters.length} monsters, ${S.cat.items.length} items, ${S.cat.scenes.length} scenes. Load a pack or start fresh.`, 'ok');
    await offerResume();
  } catch (e) {
    setStatus('Error: ' + errText(e), 'err');
  }
}

function needDisc() {
  if (S.wb) return false;
  setStatus('Choose your disc image first.', 'err');
  return true;
}

function showPack(pack, label, msg) {
  S.pack = pack;
  S.label = label;
  S.sel = {};
  ctx.sel = S.sel;
  $('xw-main').hidden = false;
  $('xw-empty').hidden = true;
  resetHistory();
  runValidate();
  rerender();
  resetHistory();
  setStatus(msg, 'ok');
  scheduleSave();
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
  showPack(res.pack, label, `${label}: ${res.files.length} file(s) loaded.`);
}

async function loadTree(tree, label) {
  S.wb.begin_files();
  for (const [p, b] of Object.entries(tree)) S.wb.add_file(p, b);
  await loadResult(JSON.parse(S.wb.finish_files()), label, tree);
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
      setStatus(`Started ${as.name} as a clone of Rebirth: every section is Rebirth's, the manifest is yours. Unchanged files keep Rebirth's comments.`, 'ok');
    }
  } catch (e) {
    setStatus('Error: ' + errText(e), 'err');
  }
}

async function exportBundle() {
  return S.wb.export_bundle(JSON.stringify(S.pack));
}

// The play page's slot, written by Play in browser and Play here.
function storePlay(bytes) {
  const m = S.pack.manifest;
  return idb(PLAY_DB, PLAY_STORE, 'readwrite', (s) => s.put({ id: m.id, name: m.name, version: m.version, bytes, savedAt: Date.now() }, PLAY_KEY));
}

// Play here: the map the Maps tab is on, in a play tab - hot-reloaded in
// place when one is open on the pack.
async function playHere() {
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
  try {
    const bytes = await exportBundle();
    const m = S.pack.manifest;
    const ok = await idb(PLAY_DB, PLAY_STORE, 'readwrite', (s) => s.put({ id: m.id, name: m.name, version: m.version, bytes, savedAt: Date.now() }, PLAY_KEY));
    if (ok === null && !window.indexedDB) throw new Error('this browser has no IndexedDB');
    setStatus(`Stored ${m.id}.lxpack for the play page.`, 'ok', 'xw-export-status');
    window.open(new URL('../play.html?expansion=1', import.meta.url).href, '_blank', 'noopener');
  } catch (e) {
    setStatus('Not stored: ' + errText(e), 'err', 'xw-export-status');
  }
}

// --- Info tips (same behaviour as the translation workbench's) ---------------------------
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
  $('xw-rebirth').addEventListener('click', () => loadRebirth());
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
    const box = $('xw-new');
    box.hidden = !box.hidden;
    if (!box.hidden) $('xw-new-id').focus();
  });
  $('xw-new-go').addEventListener('click', newPack);
  $('xw-new-cancel').addEventListener('click', () => { $('xw-new').hidden = true; });
  $('xw-dl').addEventListener('click', downloadBundle);
  $('xw-undo').addEventListener('click', () => stepHistory(-1));
  $('xw-redo').addEventListener('click', () => stepHistory(1));
  $('xw-keys').addEventListener('click', () => { $('xw-keyhelp').hidden = !$('xw-keyhelp').hidden; });
  S.find = createFind($('xw-findbox'), { pack: () => S.pack, jump: jumpTo });
  document.addEventListener('keydown', onShortcut);
  window.addEventListener('beforeunload', (ev) => {
    // The autosave keeps the work; warn only when it could not.
    if (S.pack && S.saved.state === 'failed' && S.clean != null && JSON.stringify(S.pack) !== S.clean) { ev.preventDefault(); ev.returnValue = ''; }
  });
  initRest();
}

async function downloadBundle() {
  try {
    const bytes = await exportBundle();
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

async function newPack() {
  if (needDisc()) return;
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

function initRest() {
  $('xw-toml').addEventListener('click', () => { S.tab = 'files'; rerender(); });
  $('xw-play').addEventListener('click', playInBrowser);
  $('xw-play-here').addEventListener('click', playHere);
  $('xw-live').addEventListener('change', (ev) => S.preview && S.preview.setLive(ev.target.checked));
  S.preview = createPreview3d({
    pack: () => S.pack,
    exportBundle: () => S.wb.export_bundle(JSON.stringify(S.pack)),
    storePlay,
    playUrl: (map) => new URL(`../play.html?expansion=1&map=${encodeURIComponent(map)}`, import.meta.url).href,
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
// ... and the editors' context: `__xwCtx.changed(true)` after a scripted edit.
window.__xwCtx = ctx;
