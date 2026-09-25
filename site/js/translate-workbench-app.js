/* Translation workbench: a language-pack editor over the user's own disc,
 * entirely client-side. The disc bytes are read in this tab and handed once to
 * a WASM `Workbench` session (crates/web-viewer/src/translate_workbench.rs),
 * which keeps the parsed disc, its export, the importer's name planner and the
 * retail font resident. Nothing is uploaded.
 *
 * Every number shown comes from that session - rooms, growth paths, scene
 * fits and pixel widths are the importer's and the font measurer's own; this
 * file never re-derives a budget rule. Costs, per the session:
 *   - `set_translation(key, text)` per keystroke (encode + measure one line);
 *   - `scene_fit(prot, relayout)` debounced after an edit in a `man:` line;
 *   - `names_fit()` debounced after an edit in a SCUS name;
 *   - `space_report(relayout)` once per "Check against my disc".
 *
 * Edits live in page state (`S.entries[i].t`), never read back from the DOM:
 * filtering and paging re-render rows from state, so an edit on a hidden row
 * is never lost. The working translations autosave to IndexedDB (a per-viewer
 * convenience; every access is wrapped) and a returning visitor is offered
 * "resume".
 *
 * Imports resolve relative to THIS file (site/js/), so the package at
 * site/wasm/ is `../wasm/...`; shipped packs are `../lang/<code>.yaml`.
 */

let wasmMod = null;

async function ensureWasm() {
  if (wasmMod) return wasmMod;
  const v = window.LEGAIA_WASM_V || '0';
  wasmMod = await import('../wasm/legaia_web_viewer.js?v=' + v);
  await wasmMod.default(new URL('../wasm/legaia_web_viewer_bg.wasm?v=' + v, import.meta.url));
  return wasmMod;
}

const $ = (id) => document.getElementById(id);
const tick = () => new Promise((r) => setTimeout(r, 30));

function esc(s) {
  return String(s == null ? '' : s)
    .replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
}

function download(text, name) {
  const blob = new Blob([text], { type: 'text/yaml' });
  const url = URL.createObjectURL(blob);
  const a = document.createElement('a');
  a.href = url;
  a.download = name;
  document.body.appendChild(a);
  a.click();
  a.remove();
  setTimeout(() => URL.revokeObjectURL(url), 4000);
}

function readFileText(file) {
  return new Promise((resolve, reject) => {
    const r = new FileReader();
    r.onload = () => resolve(r.result);
    r.onerror = () => reject(r.error || new Error('read failed'));
    r.readAsText(file);
  });
}

const SECTION_LABELS = {
  items: 'Item names',
  item_types: 'Item types',
  spells: 'Spell names',
  arts: 'Arts names',
  accessory_passives: 'Accessory effects',
  party_names: 'Party names',
  scene_dialog: 'Scene dialog',
  inline_text: 'Dungeon / event text',
  ui_menu: 'Menu labels',
  system_text: 'System messages',
  place_names: 'Place names',
  monster_names: 'Monster names',
};

const LIMIT_LABELS = {
  field_dialog_row: 'dialog row',
  field_dialog_row_beside_page_hand: 'dialog row beside the page hand',
  item_list_name: 'item list',
  status_magic_name: 'magic list',
  status_moves_name: 'moves list',
  party_name: 'name entry',
  battle_intro_enemy_label: 'battle enemy label',
};

// --- Autosave (IndexedDB; per-viewer convenience only) ---------------------
const SAVE_DB = 'legaia-translate-workbench';
const SAVE_STORE = 'autosave';

function idb(mode, fn) {
  return new Promise((resolve) => {
    let req;
    try { req = indexedDB.open(SAVE_DB, 1); } catch (e) { resolve(null); return; }
    req.onupgradeneeded = () => {
      try { req.result.createObjectStore(SAVE_STORE); } catch (e) { /* exists */ }
    };
    req.onerror = () => resolve(null);
    req.onsuccess = () => {
      try {
        const t = req.result.transaction(SAVE_STORE, mode);
        const r = fn(t.objectStore(SAVE_STORE));
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
const saveGet = () => idb('readonly', (s) => s.get('current'));
const savePut = (v) => idb('readwrite', (s) => s.put(v, 'current'));
const saveClear = () => idb('readwrite', (s) => s.delete('current'));

// --- State -----------------------------------------------------------------
const S = {
  wb: null,
  entries: [],        // rows from Workbench.entries(); `t` is the live edit
  byKey: new Map(),
  byBox: new Map(),   // dialog box id -> rows in box order
  disc: null,         // disc-only space tables
  outcome: new Map(), // key -> importer outcome (latest check or fast path)
  issue: new Map(),   // key -> importer message
  sceneFit: new Map(),// prot -> scene row from scene_fit / the report
  regions: null,      // name regions after the pack (names_fit)
  report: null,       // last full check
  filter: { section: '', status: '', q: '', group: '', groupLabel: '' },
  page: 0,
  pageSize: 40,
  filtered: [],
};

function setStatus(msg, kind) {
  const el = $('wb-status');
  el.textContent = msg || '';
  el.className = 'wb-status' + (kind ? ' is-' + kind : '');
}
function setCheckStatus(msg, kind) {
  const el = $('wb-check-status');
  el.textContent = msg || '';
  el.className = 'wb-status' + (kind ? ' is-' + kind : '');
}

function languageOf() {
  return ($('wb-lang').value || '').trim() || 'xx';
}

// The key's PROT entry for `man:` / `raw:` keys.
function protOf(key) {
  const m = /^(man|raw):(\d+):/.exec(key);
  return m ? { kind: m[1], prot: +m[2] } : null;
}

// --- Status of one row -------------------------------------------------------
function statusOf(e) {
  if (!e.t || !e.t.trim()) return 'untranslated';
  if (e.bad > 0) return 'not_encodable';
  const o = S.outcome.get(e.k);
  if (o === 'rolled_back') return 'rolled_back';
  if (o === 'over_budget' || o === 'no_free_run' || o === 'refused') return 'too_long';
  if (e.len != null) {
    if ((e.rk === 'string_fixed' || e.rk === 'field' || e.rk === 'dialog_fixed') && e.len > e.room) return 'too_long';
    if (e.rk === 'monster' && e.cap != null && e.len > e.cap) return 'too_long';
  }
  if (e.ovpx > 0) return 'too_wide';
  return 'fits';
}

function matches(e) {
  const f = S.filter;
  if (f.section && e.s !== f.section) return false;
  if (f.group) {
    if (f.group === 'section:monster_names') {
      if (e.s !== 'monster_names') return false;
    } else if (e.g !== f.group) return false;
  }
  if (f.status) {
    const st = statusOf(e);
    if (f.status === 'translated') {
      if (st === 'untranslated') return false;
    } else if (f.status === 'too_wide') {
      if (!(e.ovpx > 0)) return false;
    } else if (st !== f.status) return false;
  }
  if (f.q) {
    const q = f.q;
    if (!(e.k.toLowerCase().includes(q) || (e.src || '').toLowerCase().includes(q) ||
      (e.t || '').toLowerCase().includes(q) || (e.c || '').toLowerCase().includes(q))) return false;
  }
  return true;
}

// --- Row checks ----------------------------------------------------------------
function applyCheck(e, c) {
  if (!c) return;
  e.len = c.len;
  e.bad = (c.errors || []).length;
  e.errors = c.errors || [];
  e.px = c.px;
  e.ovpx = c.over_px || 0;
  e.unres = c.unresolved || 0;
}

function roomText(e) {
  const len = e.t && e.t.trim() ? e.len : null;
  const shown = len == null ? (e.t && e.t.trim() ? '?' : '-') : len;
  let s = `${shown} / ${e.room} bytes`;
  if (e.rk === 'monster' && e.cap != null) s += ` (grows to ${e.cap})`;
  if (e.rk === 'name_movable') {
    const ri = e.g && e.g.startsWith('region:') ? +e.g.slice(7) : null;
    const r = ri != null && S.regions ? S.regions[ri] : null;
    const free = r ? r.pack_free : (ri != null && S.disc ? S.disc.name_regions[ri].english_free : null);
    if (free != null) s += ` - moves if longer: ${free} free in its table`;
  }
  if (e.rk === 'dialog_growable') s += ' (can grow within its scene)';
  return s;
}

function widthText(e, px) {
  if (px == null) return '';
  const lim = e.lim ? S.limits.get(e.lim) : null;
  const unres = e.unres ? ' (at least; a name inside is unknown here)' : '';
  if (!lim) return `${px} px${unres}`;
  const where = LIMIT_LABELS[e.lim] || e.lim;
  const over = px > lim.max_px;
  return `<span class="${over ? 'wb-warnc' : ''}">${px} / ${lim.max_px} px on the ${esc(where)}${over ? ` - ${px - lim.max_px} px too wide` : ''}</span>${unres}`;
}

function outcomeText(e) {
  const o = S.outcome.get(e.k);
  if (!o || o === 'untranslated') return '';
  const label = {
    in_place: 'fits in place', moved: 'moves to free table space', grown: 'record grows',
    relocated: 'fits (scene re-packed)', relayout: 'fits (scene grows)', already_applied: 'already on the disc',
    over_budget: 'too long', no_free_run: 'no free space in the name tables', rolled_back: 'rolls back to English',
    refused: 'too long for the record', not_encodable: 'not encodable', mismatch: 'not on this disc', skipped: 'skipped',
  }[o] || o;
  const bad = ['over_budget', 'no_free_run', 'rolled_back', 'refused', 'not_encodable', 'mismatch', 'skipped'].includes(o);
  const msg = S.issue.get(e.k);
  return `<span class="${bad ? 'wb-bad' : 'wb-okc'}" ${msg ? `title="${esc(msg)}"` : ''}>${esc(label)}</span>`;
}

function markedText(e) {
  if (!e.errors || !e.errors.length) return '';
  const bad = new Set(e.errors.map((x) => x.index));
  const chars = Array.from(e.t || '');
  const html = chars.map((ch, i) => (bad.has(i) ? `<mark>${esc(ch)}</mark>` : esc(ch))).join('');
  const why = esc(e.errors[0].msg);
  return `<div class="wb-marked" title="${why}">${html}</div><div class="wb-info wb-bad">${e.errors.length} character(s) the game cannot draw: ${why}</div>`;
}

// The rows of a preview: the line's whole box for dialog, else the line.
function previewRows(e) {
  if (e.box != null && S.byBox.has(e.box)) {
    return S.byBox.get(e.box).map((x) => (x.t && x.t.trim() ? x.t : x.src));
  }
  return [e.t && e.t.trim() ? e.t : e.src];
}

function drawPreview(e, canvas, note) {
  if (!S.wb || !canvas) return;
  const rows = previewRows(e);
  let img;
  // A box draws against its first row's width (the rows beside the page hand
  // carry their own, narrower limit in each row's info line).
  const drawKey = e.box != null && S.byBox.has(e.box) ? S.byBox.get(e.box)[0].k : e.k;
  try { img = S.wb.render_preview(drawKey, rows.join('\n')); } catch (err) { img = null; }
  if (!img) { canvas.hidden = true; return; }
  canvas.hidden = false;
  canvas.width = img.w;
  canvas.height = img.h;
  canvas.style.width = (img.w * 2) + 'px';
  const ctx = canvas.getContext('2d');
  ctx.putImageData(new ImageData(new Uint8ClampedArray(img.rgba), img.w, img.h), 0, 0);
  if (note) {
    const lim = img.limit_px;
    if (e.box == null) {
      note.innerHTML = lim ? '' : 'no width limit is known for this screen';
    } else {
      note.innerHTML = img.width_px > lim
        ? `<span class="wb-warnc">this box's widest row is ${img.width_px} px - it runs past the ${lim} px edge (red)</span>`
        : `this box as the game draws it (widest row ${img.width_px} px)`;
    }
  }
}

// --- Editor rendering ------------------------------------------------------------
function rowHtml(e, i) {
  const st = statusOf(e);
  const lim = e.lim ? (LIMIT_LABELS[e.lim] || e.lim) : '';
  return `<div class="wb-row st-${st}" data-i="${i}">
    <div class="wb-row-head"><code>${esc(e.k)}</code>${e.c ? `<span>${esc(e.c)}</span>` : ''}<span>${esc(SECTION_LABELS[e.s] || e.s)}</span>${lim ? `<span>shown on: ${esc(lim)}</span>` : ''}</div>
    <div class="wb-src">${esc(e.src)}</div>
    <div class="wb-edit"><textarea rows="1" spellcheck="true" aria-label="Translation of ${esc(e.k)}">${esc(e.t)}</textarea>
      <div class="wb-info"><span class="wb-room">${esc(roomText(e))}</span><span class="wb-out">${outcomeText(e)}</span><span class="wb-width">${widthText(e, e.t && e.t.trim() ? e.px : e.spx)}</span></div>
      <div class="wb-errs">${markedText(e)}</div>
      <div class="wb-preview"><canvas></canvas><span class="wb-info wb-pv-note"></span></div>
    </div>
  </div>`;
}

function renderList() {
  const list = $('wb-list');
  const n = S.filtered.length;
  const pages = Math.max(1, Math.ceil(n / S.pageSize));
  S.page = Math.min(S.page, pages - 1);
  const start = S.page * S.pageSize;
  const slice = S.filtered.slice(start, start + S.pageSize);
  let html = '';
  let group = null;
  let box = null;
  let open = false;
  for (const idx of slice) {
    const e = S.entries[idx];
    const g = e.s === 'scene_dialog' || e.s === 'inline_text' ? (e.g || e.c) : e.s;
    if (g !== group) {
      if (open) { html += '</div>'; open = false; }
      group = g;
      box = null;
      html += `<div class="wb-group-head">${esc(groupLabel(e))}</div>`;
    }
    if (e.box != null) {
      if (e.box !== box) {
        if (open) html += '</div>';
        box = e.box;
        const rows = (S.byBox.get(box) || []).length;
        html += `<div class="wb-box"><div class="wb-box-head"><span>box of ${rows} row${rows === 1 ? '' : 's'}</span></div>`;
        open = true;
      }
    } else if (open) {
      html += '</div>';
      open = false;
    }
    html += rowHtml(e, idx);
  }
  if (open) html += '</div>';
  list.innerHTML = html || '<div class="wb-empty">No lines match these filters.</div>';
  for (const el of list.querySelectorAll('.wb-row')) {
    const e = S.entries[+el.dataset.i];
    drawPreview(e, el.querySelector('canvas'), el.querySelector('.wb-pv-note'));
    autoGrow(el.querySelector('textarea'));
  }
  const pager = `<button type="button" data-p="prev" ${S.page === 0 ? 'disabled' : ''}>Previous</button>
    <span>${n ? `${start + 1}-${Math.min(n, start + S.pageSize)} of ${n} lines` : '0 lines'} (page ${S.page + 1} / ${pages})</span>
    <button type="button" data-p="next" ${S.page >= pages - 1 ? 'disabled' : ''}>Next</button>`;
  $('wb-pager-top').innerHTML = pager;
  $('wb-pager-bottom').innerHTML = pager;
}

function groupLabel(e) {
  if (e.g && (e.g.startsWith('scene:') || e.g.startsWith('carrier:'))) {
    return `${e.c || '?'} (PROT ${e.g.split(':')[1]})`;
  }
  return SECTION_LABELS[e.s] || e.s;
}

function autoGrow(ta) {
  if (!ta) return;
  ta.style.height = 'auto';
  ta.style.height = Math.min(ta.scrollHeight + 2, 240) + 'px';
}

function refilter(resetPage) {
  S.filtered = [];
  for (let i = 0; i < S.entries.length; i++) if (matches(S.entries[i])) S.filtered.push(i);
  if (resetPage) S.page = 0;
  renderList();
  const chip = $('wb-f-group');
  if (S.filter.group) {
    chip.hidden = false;
    chip.innerHTML = `Showing: ${esc(S.filter.groupLabel)} <button type="button" aria-label="Clear">&times;</button>`;
  } else {
    chip.hidden = true;
  }
}

function focusGroup(group, label) {
  S.filter.group = group;
  S.filter.groupLabel = label;
  S.filter.section = '';
  $('wb-f-section').value = '';
  refilter(true);
  $('editor').scrollIntoView({ behavior: 'smooth' });
}

// --- Dashboard -------------------------------------------------------------------
function meter(en, pack, total, over) {
  const pct = (v) => Math.max(0, Math.min(100, total ? (v / total) * 100 : 0)).toFixed(1);
  const packBar = pack == null ? '' : `<span class="${over ? 'm-over' : 'm-pack'}" style="width:${pct(pack)}%"></span>`;
  const enBar = pack == null ? `<span class="m-en" style="width:${pct(en)}%"></span>` : '';
  return `<div class="wb-meter">${enBar}${packBar}</div>`;
}

function renderCoverage() {
  const rows = new Map();
  for (const e of S.entries) {
    const r = rows.get(e.s) || { total: 0, filled: 0, bad: 0 };
    r.total++;
    if (e.t && e.t.trim()) {
      r.filled++;
      const st = statusOf(e);
      if (st === 'too_long' || st === 'rolled_back' || st === 'not_encodable') r.bad++;
    }
    rows.set(e.s, r);
  }
  let html = '<tr><th>Section</th><th class="num">Translated</th><th></th><th class="num">Problems</th></tr>';
  for (const [s, r] of rows) {
    html += `<tr class="is-link" data-section="${esc(s)}"><td>${esc(SECTION_LABELS[s] || s)}</td>
      <td class="num">${r.filled} / ${r.total}</td><td>${meter(0, r.filled, r.total, false)}</td>
      <td class="num ${r.bad ? 'wb-bad' : ''}">${r.bad || ''}</td></tr>`;
  }
  $('wb-coverage').innerHTML = html;
}

function renderRegions() {
  const regions = (S.regions || (S.disc ? S.disc.name_regions : [])).map((r) => r);
  regions.sort((a, b) => (a.pack_free ?? a.english_free) - (b.pack_free ?? b.english_free) || b.total - a.total);
  const vaHex = (v) => '0x' + (v >>> 0).toString(16);
  let html = '';
  for (const r of regions.slice(0, 12)) {
    const used = r.pack_used ?? r.english_used;
    const free = r.pack_free ?? r.english_free;
    html += `<div class="wb-bar" data-group="region:${r.index}" data-label="name table ${vaHex(r.start_va)}">
      <span class="wb-bar-label">table ${vaHex(r.start_va)}</span>${meter(r.english_used, S.regions ? used : null, r.total, false)}
      <span class="wb-bar-num">${free} free / ${r.total}</span></div>`;
  }
  const totalFree = regions.reduce((a, r) => a + (r.pack_free ?? r.english_free), 0);
  html += `<div class="wb-note">${regions.length} regions, ${totalFree} bytes free in total${S.regions ? ' after your pack' : ' (English)'}; tightest shown.</div>`;
  $('wb-regions').innerHTML = html;
}

function renderMonsters() {
  let n = 0, inplace = 0, grow = 0, over = 0;
  for (const e of S.entries) {
    if (e.s !== 'monster_names' || !(e.t && e.t.trim())) continue;
    n++;
    if (e.len == null) continue;
    if (e.len <= e.room) inplace++;
    else if (e.cap != null && e.len <= e.cap) grow++;
    else over++;
  }
  const cap = S.disc && S.disc.monsters.length ? S.disc.monsters[0].longest : 15;
  $('wb-monsters').innerHTML = `${n} translated: ${inplace} in place, ${grow} grow their record` +
    (over ? `, <span class="wb-bad">${over} too long</span>` : '') +
    ` <div class="wb-note">Room is 7, 11 or 15 bytes per record; up to ${cap} by growing it.</div>`;
}

function renderPools() {
  if (!S.disc) return;
  const packBytes = new Map();
  if (S.report) for (const p of S.report.pools) packBytes.set(p.index, p.pack_bytes);
  let html = '';
  const pools = S.disc.pools.slice().sort((a, b) => a.slack - b.slack);
  for (const p of pools.slice(0, 8)) {
    const pb = packBytes.get(p.index);
    html += `<div class="wb-bar" data-group="pool:${p.index}" data-label="${esc(p.label)} labels">
      <span class="wb-bar-label">${esc(p.label)}</span>${meter(p.english_bytes, pb ?? null, p.room_bytes, pb != null && pb > p.room_bytes)}
      <span class="wb-bar-num">${p.slack} spare bytes${pb != null ? `, pack ${pb} / ${p.room_bytes}` : ''}</span></div>`;
  }
  html += `<div class="wb-note">Each label keeps its own fixed slot; the spare bytes are padding a longer label can use one string at a time.</div>`;
  $('wb-pools').innerHTML = html;
}

function sceneRows() {
  const rows = [];
  if (!S.disc) return rows;
  const filled = new Map();
  for (const e of S.entries) {
    if (e.g && e.t && e.t.trim()) filled.set(e.g, (filled.get(e.g) || 0) + 1);
  }
  for (const sc of S.disc.scenes) {
    const fit = S.sceneFit.get(sc.prot);
    rows.push({
      group: `scene:${sc.prot}`, name: sc.scene || '?', prot: sc.prot, lines: sc.lines,
      filled: filled.get(`scene:${sc.prot}`) || 0, room: sc.footprint,
      used: fit && fit.written_len != null ? fit.written_len : null,
      over: fit ? (fit.path === 'none' && fit.full_overflow ? fit.full_overflow : 0) : 0,
      rolled: fit ? fit.rolled_back.length : 0, grows: fit ? fit.relayout_sectors : null, kind: 'scene',
    });
  }
  for (const c of S.disc.carriers) {
    rows.push({
      group: `carrier:${c.prot}`, name: c.scene || '?', prot: c.prot, lines: c.lines,
      filled: filled.get(`carrier:${c.prot}`) || 0, room: c.footprint + (c.sector_slack || 0), used: null,
      over: 0, rolled: 0, grows: null, kind: 'dungeon',
    });
  }
  const full = (r) => (r.used != null ? r.used / r.room : (r.filled ? 0.999 : 0)) + r.rolled;
  rows.sort((a, b) => full(b) - full(a) || b.filled - a.filled);
  return rows;
}

let showAllScenes = false;
function renderScenes() {
  const rows = sceneRows();
  const shown = showAllScenes ? rows : rows.slice(0, 15);
  let html = `<div class="wb-scroll"><table class="wb-table"><tr><th>Scene</th><th class="num">Lines</th><th class="num">Translated</th>
    <th>Compressed size / space</th><th class="num">Rolled back</th></tr>`;
  for (const r of shown) {
    const used = r.used != null ? `${r.used} / ${r.room}` : (r.kind === 'dungeon' ? `${r.room} incl. sector slack` : (r.filled ? 'not measured yet' : `${r.room} (English fills it)`));
    html += `<tr class="is-link" data-group="${r.group}" data-label="${esc(r.name)} (PROT ${r.prot})">
      <td>${esc(r.name)} <span class="wb-bar-num">${r.prot}${r.kind === 'dungeon' ? ', dungeon text' : ''}</span></td>
      <td class="num">${r.lines}</td><td class="num">${r.filled}</td>
      <td>${r.used != null ? meter(0, r.used, r.room, r.rolled > 0) : ''}<span class="wb-bar-num">${used}${r.grows ? `, grows ${r.grows} sector(s)` : ''}</span></td>
      <td class="num ${r.rolled ? 'wb-bad' : ''}">${r.rolled || ''}</td></tr>`;
  }
  html += '</table></div>';
  if (rows.length > 15) {
    html += `<button type="button" class="wb-button wb-button-ghost wb-more" id="wb-scenes-more">${showAllScenes ? 'Show fewer' : `Show all ${rows.length}`}</button>`;
  }
  $('wb-scenes').innerHTML = html;
}

function renderDashboard() {
  renderCoverage();
  renderRegions();
  renderMonsters();
  renderPools();
  renderScenes();
}

// --- Fast paths after edits ------------------------------------------------------
const pendingScenes = new Set();
let sceneTimer = null;
let namesTimer = null;
let saveTimer = null;
let dashTimer = null;

function applySceneFit(fit) {
  if (!fit || !fit.scene) return;
  S.sceneFit.set(fit.scene.prot, fit.scene);
  for (const row of fit.entries) {
    S.outcome.set(row.key, row.outcome);
    if (row.issue) S.issue.set(row.key, row.issue); else S.issue.delete(row.key);
  }
}

function scheduleSceneFit(prot) {
  pendingScenes.add(prot);
  clearTimeout(sceneTimer);
  sceneTimer = setTimeout(() => {
    const relayout = $('wb-relayout').checked;
    for (const p of pendingScenes) {
      try { applySceneFit(JSON.parse(S.wb.scene_fit(p, relayout))); } catch (e) { console.warn(e); }
    }
    pendingScenes.clear();
    refreshVisibleRows();
    renderScenes();
    renderCoverage();
  }, 700);
}

function applyNamesFit() {
  try {
    const nf = JSON.parse(S.wb.names_fit());
    S.regions = nf.regions;
    for (const e of S.entries) {
      if (e.k.startsWith('scus:') && S.outcome.has(e.k)) S.outcome.delete(e.k);
    }
    for (const row of nf.entries) {
      S.outcome.set(row.key, row.outcome);
      if (row.issue) S.issue.set(row.key, row.issue); else S.issue.delete(row.key);
    }
  } catch (e) { console.warn(e); }
}

function scheduleNamesFit() {
  clearTimeout(namesTimer);
  namesTimer = setTimeout(() => {
    applyNamesFit();
    refreshVisibleRows();
    renderRegions();
    renderCoverage();
  }, 400);
}

function scheduleSave() {
  clearTimeout(saveTimer);
  saveTimer = setTimeout(async () => {
    if (!S.wb) return;
    try {
      await savePut({ savedAt: Date.now(), json: S.wb.translations() });
    } catch (e) { /* storage unavailable: nothing to do */ }
  }, 1500);
}

function scheduleDash() {
  clearTimeout(dashTimer);
  dashTimer = setTimeout(() => { renderCoverage(); renderMonsters(); }, 500);
}

// Re-draw the info line + preview of every row on the page (after a fit).
function refreshVisibleRows() {
  for (const el of $('wb-list').querySelectorAll('.wb-row')) updateRowEl(el, S.entries[+el.dataset.i], false);
}

function updateRowEl(el, e, redraw) {
  el.className = `wb-row st-${statusOf(e)}`;
  el.querySelector('.wb-room').textContent = roomText(e);
  el.querySelector('.wb-out').innerHTML = outcomeText(e);
  el.querySelector('.wb-width').innerHTML = widthText(e, e.t && e.t.trim() ? e.px : e.spx);
  el.querySelector('.wb-errs').innerHTML = markedText(e);
  if (redraw) drawPreview(e, el.querySelector('canvas'), el.querySelector('.wb-pv-note'));
}

function onEdit(el, e, text) {
  e.t = text;
  let c = null;
  try { c = JSON.parse(S.wb.set_translation(e.k, text)); } catch (err) { console.warn(err); }
  applyCheck(e, c);
  if (!text.trim()) S.outcome.delete(e.k);
  updateRowEl(el, e, true);
  // The other rows of the same dialog box draw this line in their preview.
  if (e.box != null) {
    for (const other of $('wb-list').querySelectorAll('.wb-row')) {
      const o = S.entries[+other.dataset.i];
      if (o !== e && o.box === e.box) drawPreview(o, other.querySelector('canvas'), other.querySelector('.wb-pv-note'));
    }
  }
  const p = protOf(e.k);
  if (p && p.kind === 'man') scheduleSceneFit(p.prot);
  if (e.k.startsWith('scus:')) scheduleNamesFit();
  scheduleSave();
  scheduleDash();
}

// --- Loading -----------------------------------------------------------------------
function ingestEntries() {
  const data = JSON.parse(S.wb.entries());
  S.entries = data.entries;
  S.limits = new Map(data.limits.map((l) => [l.context, l]));
  S.byKey = new Map();
  S.byBox = new Map();
  for (const e of S.entries) {
    S.byKey.set(e.k, e);
    if (e.box != null) {
      const b = S.byBox.get(e.box);
      if (b) b.push(e); else S.byBox.set(e.box, [e]);
    }
  }
  $('wb-lang').value = data.language || '';
  $('wb-contrib').value = (data.contributors || []).join(', ');
  S.outcome.clear();
  S.issue.clear();
  S.sceneFit.clear();
  S.report = null;
  S.regions = null;
  $('wb-issues').hidden = true;
  setCheckStatus('');
}

function fillSectionFilter() {
  const seen = [];
  for (const e of S.entries) if (!seen.includes(e.s)) seen.push(e.s);
  $('wb-f-section').innerHTML = '<option value="">All sections</option>' +
    seen.map((s) => `<option value="${esc(s)}">${esc(SECTION_LABELS[s] || s)}</option>`).join('');
}

async function afterPackChange(msg) {
  ingestEntries();
  applyNamesFit();
  fillSectionFilter();
  $('wb-main').hidden = false;
  renderDashboard();
  refilter(true);
  setStatus(msg, 'ok');
  const filled = S.entries.some((e) => e.t && e.t.trim());
  if (filled) await runCheck(true);
}

async function openDisc(file) {
  setStatus(`Reading ${file.name} (nothing is uploaded) ...`);
  try {
    const mod = await ensureWasm();
    const buf = new Uint8Array(await file.arrayBuffer());
    setStatus('Reading the text on your disc (a few seconds) ...');
    await tick();
    if (S.wb) { try { S.wb.free(); } catch (e) { /* already freed */ } }
    S.wb = mod.Workbench.open(buf);
    S.disc = JSON.parse(S.wb.disc_report());
    S.wb.start_fresh('xx');
    await afterPackChange(`Disc read: ${file.name}. Load a pack or start fresh.`);
    $('wb-lang').value = '';
    await offerResume();
  } catch (e) {
    setStatus('Error: ' + (e && e.message ? e.message : e), 'err');
  }
}

async function offerResume() {
  const box = $('wb-resume');
  let saved = null;
  try { saved = await saveGet(); } catch (e) { saved = null; }
  if (!saved || !saved.json) { box.hidden = true; return; }
  let info;
  try { info = JSON.parse(saved.json); } catch (e) { box.hidden = true; return; }
  const n = Object.keys(info.t || {}).length;
  if (!n) { box.hidden = true; return; }
  const when = new Date(saved.savedAt).toLocaleString();
  box.innerHTML = `<span>You have an unsaved <strong>${esc(info.language)}</strong> pack in this browser (${n} line(s), ${esc(when)}).</span>
    <button type="button" class="wb-button" id="wb-resume-go">Resume it</button>
    <button type="button" class="wb-button wb-button-ghost" id="wb-resume-drop">Discard</button>`;
  box.hidden = false;
  $('wb-resume-go').onclick = async () => {
    box.hidden = true;
    try {
      const k = S.wb.load_translations(saved.json);
      await afterPackChange(`Resumed ${k} line(s) of your ${info.language} pack.`);
    } catch (e) {
      setStatus('Error: ' + (e && e.message ? e.message : e), 'err');
    }
  };
  $('wb-resume-drop').onclick = async () => {
    box.hidden = true;
    await saveClear();
  };
}

async function loadPackText(yaml, label) {
  if (!S.wb) { setStatus('Choose your disc image first.', 'err'); return; }
  setStatus(`Loading ${label} ...`);
  await tick();
  try {
    const r = JSON.parse(S.wb.load_pack(yaml));
    $('wb-resume').hidden = true;
    let msg = `${label}: ${r.merged} line(s) loaded (${r.language}).`;
    if (r.unknown) msg += ` ${r.unknown} line(s) are keyed to text your disc does not carry and were dropped.`;
    await afterPackChange(msg);
    scheduleSave();
  } catch (e) {
    setStatus('Error: ' + (e && e.message ? e.message : e), 'err');
  }
}

// --- Check -----------------------------------------------------------------------
async function runCheck(auto) {
  if (!S.wb) return;
  const btn = $('wb-check');
  btn.disabled = true;
  const relayout = $('wb-relayout').checked;
  setCheckStatus(`Checking the whole pack against your disc${relayout ? ', with more room for dialog' : ''} ...`);
  await tick();
  try {
    const t0 = performance.now();
    const rep = JSON.parse(S.wb.space_report(relayout));
    S.report = rep;
    S.outcome.clear();
    S.issue.clear();
    for (const e of rep.entries) {
      if (e.outcome) S.outcome.set(e.key, e.outcome);
      if (e.issue) S.issue.set(e.key, e.issue);
    }
    S.sceneFit.clear();
    for (const sc of rep.scenes) S.sceneFit.set(sc.prot, sc);
    const s = rep.summary;
    const o = s.outcomes || {};
    const landed = ['in_place', 'moved', 'grown', 'relocated', 'relayout', 'already_applied']
      .reduce((a, k) => a + (o[k] || 0), 0);
    const skipped = s.filled - landed;
    let msg = `${landed} of ${s.filled} translated line(s) would be patched, ${skipped} would stay English`;
    if (s.scenes_rolled_back) msg += `; ${s.scenes_rolled_back} scene(s) roll lines back`;
    if (s.relayout_sectors) msg += `; the relayout grows ${s.relayout_entries} scene(s) by ${s.relayout_sectors} sector(s)`;
    msg += ` (${((performance.now() - t0) / 1000).toFixed(1)} s)`;
    for (const r of rep.reasons || []) msg += `\n  ${r.count} skipped: ${r.reason}`;
    setCheckStatus(msg, skipped ? '' : 'ok');
    $('wb-issues').hidden = !rep.entries.some((e) => e.issue);
    renderDashboard();
    refilter(false);
  } catch (e) {
    setCheckStatus('Error: ' + (e && e.message ? e.message : e), 'err');
  } finally {
    btn.disabled = false;
  }
  return auto;
}

function issuesCsv() {
  const q = (v) => '"' + String(v == null ? '' : v).replace(/"/g, '""') + '"';
  const rows = ['key,outcome,message'];
  for (const e of (S.report ? S.report.entries : [])) {
    if (e.issue) rows.push([q(e.key), q(e.outcome), q(e.issue)].join(','));
  }
  return rows.join('\n') + '\n';
}

// --- Info tips (same behaviour as the ROM patcher's) ---------------------------------
function setupInfoTips() {
  const openTips = () => document.querySelectorAll('.info-tip.is-open');
  const close = (tip) => { tip.classList.remove('is-open'); tip.setAttribute('aria-expanded', 'false'); };
  const position = (tip) => {
    tip.classList.remove('tip-align-right', 'tip-align-left');
    const r = tip.getBoundingClientRect();
    const half = Math.min(384, window.innerWidth * 0.78) / 2;
    const mid = r.left + r.width / 2;
    if (mid + half > window.innerWidth - 12) tip.classList.add('tip-align-right');
    else if (mid - half < 12) tip.classList.add('tip-align-left');
  };
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
    if (open) position(tip);
  });
  document.addEventListener('keydown', (e) => {
    if (e.key === 'Escape') openTips().forEach(close);
  });
  document.addEventListener('mouseover', (e) => {
    const tip = e.target.closest && e.target.closest('.info-tip');
    if (tip) position(tip);
  });
}

// --- Wiring --------------------------------------------------------------------------
function init() {
  setupInfoTips();
  const fileInput = $('wb-file');
  if (window.RomCache) {
    window.RomCache.attach(fileInput, { onLoad: (f) => openDisc(f) });
  } else {
    fileInput.addEventListener('change', () => {
      const f = fileInput.files && fileInput.files[0];
      if (f) openDisc(f);
    });
  }

  $('wb-shipped').addEventListener('change', async (ev) => {
    const lang = ev.target.value;
    if (!lang) return;
    try {
      const res = await fetch(new URL(`../lang/${lang}.yaml`, import.meta.url).href);
      if (!res.ok) throw new Error(`could not load ${lang}.yaml (${res.status})`);
      await loadPackText(await res.text(), `published ${lang} pack`);
    } catch (e) {
      setStatus('Error: ' + (e && e.message ? e.message : e), 'err');
    }
    ev.target.value = '';
  });

  $('wb-pack-file').addEventListener('change', async (ev) => {
    const f = ev.target.files && ev.target.files[0];
    if (!f) return;
    await loadPackText(await readFileText(f), f.name);
    ev.target.value = '';
  });

  $('wb-fresh').addEventListener('click', async () => {
    if (!S.wb) { setStatus('Choose your disc image first.', 'err'); return; }
    const lang = languageOf();
    S.wb.start_fresh(lang);
    await afterPackChange(`Started an empty ${lang} pack. Set the language code above.`);
    $('wb-lang').value = lang === 'xx' ? '' : lang;
  });

  const syncMeta = () => {
    if (S.wb) S.wb.set_meta(languageOf(), $('wb-contrib').value, '');
    scheduleSave();
  };
  $('wb-lang').addEventListener('change', syncMeta);
  $('wb-contrib').addEventListener('change', syncMeta);

  $('wb-dl-working').addEventListener('click', () => {
    if (!S.wb) return;
    syncMeta();
    download(S.wb.working_pack(), `legaia_${languageOf()}.working.yaml`);
    setCheckStatus(`Downloaded legaia_${languageOf()}.working.yaml - it holds the game's English text: keep it to yourself.`, 'ok');
  });
  $('wb-dl-share').addEventListener('click', () => {
    if (!S.wb) return;
    syncMeta();
    download(S.wb.shareable_pack(), `legaia_${languageOf()}.yaml`);
    setCheckStatus(`Downloaded legaia_${languageOf()}.yaml - your translations only, safe to share. Patch a disc with it on the ROM patcher page.`, 'ok');
  });
  $('wb-check').addEventListener('click', () => runCheck(false));
  $('wb-issues').addEventListener('click', () => {
    download(issuesCsv(), `legaia_${languageOf()}.skipped.csv`);
  });

  $('wb-f-section').addEventListener('change', (ev) => {
    S.filter.section = ev.target.value;
    S.filter.group = '';
    refilter(true);
  });
  $('wb-f-status').addEventListener('change', (ev) => { S.filter.status = ev.target.value; refilter(true); });
  let qTimer = null;
  $('wb-f-search').addEventListener('input', (ev) => {
    clearTimeout(qTimer);
    qTimer = setTimeout(() => { S.filter.q = ev.target.value.trim().toLowerCase(); refilter(true); }, 200);
  });
  $('wb-f-group').addEventListener('click', (ev) => {
    if (ev.target.closest('button')) { S.filter.group = ''; refilter(true); }
  });

  const pagerClick = (ev) => {
    const b = ev.target.closest('button[data-p]');
    if (!b) return;
    S.page += b.dataset.p === 'next' ? 1 : -1;
    renderList();
    if (ev.currentTarget.id === 'wb-pager-bottom') $('wb-list').scrollIntoView();
  };
  $('wb-pager-top').addEventListener('click', pagerClick);
  $('wb-pager-bottom').addEventListener('click', pagerClick);

  $('wb-list').addEventListener('input', (ev) => {
    const ta = ev.target.closest('textarea');
    if (!ta) return;
    const el = ta.closest('.wb-row');
    const e = S.entries[+el.dataset.i];
    autoGrow(ta);
    onEdit(el, e, ta.value);
  });

  // Dashboard rows filter the editor.
  document.querySelector('.wb-dash').addEventListener('click', (ev) => {
    if (ev.target.closest('.info-tip')) return;
    if (ev.target.id === 'wb-scenes-more') { showAllScenes = !showAllScenes; renderScenes(); return; }
    const sec = ev.target.closest('[data-section]');
    if (sec) { focusGroup('', ''); S.filter.section = sec.dataset.section; $('wb-f-section').value = sec.dataset.section; refilter(true); return; }
    const g = ev.target.closest('[data-group]');
    if (g) { focusGroup(g.dataset.group, g.dataset.label || g.dataset.group); return; }
    if (ev.target.closest('#wb-monsters')) focusGroup('section:monster_names', 'monster names');
  });
}

if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', init);
else init();

// Headless-verification hook: the page state (no disc bytes).
window.__wbState = S;
