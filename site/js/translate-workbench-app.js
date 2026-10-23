/* Translation workbench: a language-pack editor over the user's own disc,
 * entirely client-side. The disc bytes are read in this tab and handed once to
 * a WASM `Workbench` session (crates/web-patcher/src/translate_workbench.rs),
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
 * Accented characters: the pack's `accents:` header (strict / fold / font)
 * decides how typed accents reach the disc, and every per-character verdict,
 * fold and cell escape comes from the session (`check` notes, `fold_line`,
 * `cells_line`, `latin_palette`), which reads the one Rust table
 * (legaia_font::latin). This file keeps no accent table of its own.
 *
 * Imports resolve relative to THIS file (site/js/), so the package at
 * site/wasm/ is `../wasm/...`; shipped packs are `../lang/<code>.yaml`.
 */

let wasmMod = null;

async function ensureWasm() {
  if (wasmMod) return wasmMod;
  wasmMod = await window.LegaiaWasm.load();
  return wasmMod;
}

const $ = (id) => document.getElementById(id);
const tick = () => new Promise((r) => setTimeout(r, 30));

const esc = window.escapeHtml;

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
  pageSize: 100,
  filtered: [],
  accents: '',        // the pack's accent mode: '' (strict), 'fold', 'font'
  accentInfo: null,   // {disc_font, preview, palette} from the session
  lastTa: null,       // the textarea the palette inserts into
  cur: null,          // index into `entries` of the line in the editor
  indexOf: new Map(), // key -> index into `entries`
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
  if (e.ud > 0) return 'undrawable';
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
    } else if (f.status === 'chars') {
      if (!(e.t && e.t.trim() && (e.ud > 0 || e.bad > 0 || e.fd > 0))) return false;
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
  e.notes = c.notes || [];
  e.ud = c.undrawable || 0;
  e.fd = e.notes.length - e.ud;
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
  if (e.rk === 'string_movable') {
    const img = codeImageOf(e.k);
    const free = img ? (img.pack_free ?? img.english_free + img.spare) : null;
    if (free != null) s += ` - moves if longer: ${free} free in ${img.image}`;
  }
  if (e.rk === 'dialog_growable') s += ' (can grow within its scene)';
  return s;
}

// The code image (executable or overlay) a ui_menu / system_text key lives in.
function codeImageOf(key) {
  const rep = S.report && S.report.code_images ? S.report : S.disc;
  if (!rep || !rep.code_images) return null;
  const m = /^ui:(\d+):/.exec(key);
  if (m) return rep.code_images.find((c) => c.prot === +m[1]) || null;
  if (key.startsWith('scus:str:')) return rep.code_images.find((c) => c.prot == null) || null;
  return null;
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
    in_place: 'fits in place', moved: 'moves to free space', grown: 'record grows',
    relocated: 'fits (scene re-packed)', relayout: 'fits (scene grows)', already_applied: 'already on the disc',
    over_budget: 'too long', no_free_run: 'no free space it can move to', rolled_back: 'rolls back to English',
    refused: 'too long for the record', not_encodable: 'not encodable', mismatch: 'not on this disc', skipped: 'skipped',
  }[o] || o;
  const bad = ['over_budget', 'no_free_run', 'rolled_back', 'refused', 'not_encodable', 'mismatch', 'skipped'].includes(o);
  const msg = S.issue.get(e.k);
  return `<span class="${bad ? 'wb-bad' : 'wb-okc'}" ${msg ? `title="${esc(msg)}"` : ''}>${esc(label)}</span>`;
}

// Every character of the line the game will not draw as typed, marked in
// place with its reason, plus the one-click fixes that apply. The verdicts
// are the session's (encode errors + accent notes); the fixes call its fold /
// cell rewriters.
function markedText(e) {
  const errs = e.errors || [];
  const notes = e.notes || [];
  if (!errs.length && !notes.length) return '';
  const cls = new Map();
  const why = new Map();
  for (const n of notes) {
    cls.set(n.index, n.undrawable ? 'm-warn' : 'm-fold');
    why.set(n.index, n.msg);
  }
  for (const x of errs) {
    cls.set(x.index, 'm-bad');
    if (!why.has(x.index)) why.set(x.index, x.msg);
  }
  const chars = Array.from(e.t || '');
  let html = '';
  for (let i = 0; i < chars.length; i++) {
    const c = cls.get(i);
    // A `{xx}` cell escape is one note spanning four characters.
    const span = c && chars[i] === '{' && chars[i + 3] === '}' ? 4 : 1;
    const text = esc(chars.slice(i, i + span).join(''));
    html += c ? `<mark class="${c}" title="${esc(why.get(i))}">${text}</mark>` : text;
    i += span - 1;
  }
  const reasons = [];
  for (const m of why.values()) if (!reasons.includes(m)) reasons.push(m);
  const bad = errs.length || notes.some((n) => n.undrawable);
  const head = errs.length
    ? `${errs.length} character(s) will not encode`
    : (bad ? `${e.ud} character(s) will not draw as typed` : `${notes.length} character(s) fold to plain letters on import`);
  const canFold = notes.some((n) => n.fold) || errs.length;
  const canCell = notes.some((n) => n.cell && n.kind === 'accent');
  const canFont = notes.some((n) => n.cell);
  const fixes = `<span class="wb-fixes">${canFold ? '<button type="button" data-fix="fold" title="Replace every accent with its plain letter (the fold the patcher uses)">Fold to plain letters</button>' : ''}${canFont && S.accents !== 'font' ? '<button type="button" data-fix="font" title="Switch the whole pack to the accent font: the patch draws these letters">Use the accent font</button>' : ''}${canCell ? '<button type="button" data-fix="cells" title="Write each accent as its {xx} cell markup">Write as {xx} markup</button>' : ''}</span>`;
  const list = reasons.slice(0, 3).map((r) => `<li>${esc(r)}</li>`).join('') + (reasons.length > 3 ? `<li>... ${reasons.length - 3} more</li>` : '');
  return `<div class="wb-marked">${html}</div><div class="wb-info ${bad ? 'wb-bad' : ''}"><span>${head}</span>${fixes}</div><ul class="wb-reasons">${list}</ul>`;
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
// The page has one editor (`#wb-ed-body`) and a dense list of every filtered
// line beside it; picking a list row loads that line into the editor. The
// editor's info line, marks and preview use the same classes the per-row
// layout used, so `updateRowEl` serves it unchanged.
function sizeText(e) {
  const filled = e.t && e.t.trim();
  if (!filled) return `${e.room}`;
  return `${e.len == null ? '?' : e.len}/${e.room}`;
}

function liHtml(e, i, inBox, boxStart) {
  const st = statusOf(e);
  const filled = e.t && e.t.trim();
  const cls = `wb-li st-${st}${i === S.cur ? ' is-active' : ''}${inBox ? ' in-box' : ''}${boxStart ? ' box-start' : ''}`;
  return `<button type="button" class="${cls}" data-i="${i}" role="option" aria-selected="${i === S.cur}" title="${esc(e.k)}">
    <span class="wb-li-en">${esc(e.src)}</span>
    <span class="wb-li-n">${esc(sizeText(e))}</span>
    <span class="wb-li-tr${filled ? '' : ' is-empty'}">${filled ? esc(e.t) : 'untranslated'}</span>
  </button>`;
}

function updateLi(i) {
  const el = $('wb-list').querySelector(`.wb-li[data-i="${i}"]`);
  if (!el) return;
  const e = S.entries[i];
  const filled = e.t && e.t.trim();
  el.classList.remove(...Array.from(el.classList).filter((c) => c.startsWith('st-')));
  el.classList.add(`st-${statusOf(e)}`);
  el.querySelector('.wb-li-n').textContent = sizeText(e);
  const tr = el.querySelector('.wb-li-tr');
  tr.textContent = filled ? e.t : 'untranslated';
  tr.classList.toggle('is-empty', !filled);
}

// Bring a row into the list's own view. Not `scrollIntoView`: that also
// scrolls the page, which pulls the editor out from under the translator.
function scrollListTo(li) {
  const list = $('wb-list');
  const head = 26; // the sticky group heading over the top rows
  const top = li.offsetTop - list.offsetTop;
  if (top - head < list.scrollTop) list.scrollTop = Math.max(0, top - head);
  else if (top + li.offsetHeight > list.scrollTop + list.clientHeight) list.scrollTop = top + li.offsetHeight - list.clientHeight;
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
  for (const idx of slice) {
    const e = S.entries[idx];
    const g = e.s === 'scene_dialog' || e.s === 'inline_text' ? (e.g || e.c) : e.s;
    if (g !== group) {
      group = g;
      box = null;
      html += `<div class="wb-group-head">${esc(groupLabel(e))}</div>`;
    }
    const inBox = e.box != null && (S.byBox.get(e.box) || []).length > 1;
    const boxStart = inBox && e.box !== box;
    box = e.box != null ? e.box : null;
    html += liHtml(e, idx, inBox, boxStart);
  }
  list.innerHTML = html || '<div class="wb-empty">No lines match these filters.</div>';
  const pager = `<button type="button" data-p="prev" ${S.page === 0 ? 'disabled' : ''}>Previous</button>
    <span>${n ? `${start + 1}-${Math.min(n, start + S.pageSize)} of ${n} lines` : '0 lines'} (page ${S.page + 1} / ${pages})</span>
    <button type="button" data-p="next" ${S.page >= pages - 1 ? 'disabled' : ''}>Next</button>`;
  $('wb-pager').innerHTML = pager;
  list.scrollTop = 0;
  const act = list.querySelector('.wb-li.is-active');
  if (act) scrollListTo(act);
}

// Load line `i` (an index into S.entries) into the editor.
function selectLine(i, opts) {
  const o = opts || {};
  const prev = S.cur;
  S.cur = i;
  for (const el of $('wb-list').querySelectorAll('.wb-li.is-active')) {
    el.classList.remove('is-active');
    el.setAttribute('aria-selected', 'false');
  }
  const body = $('wb-ed-body');
  const ta = $('wb-ed-input');
  if (i == null || !S.entries[i]) {
    S.cur = null;
    body.hidden = true;
    $('wb-ed-empty').hidden = false;
    updatePos();
    return;
  }
  const e = S.entries[i];
  const li = $('wb-list').querySelector(`.wb-li[data-i="${i}"]`);
  if (li) {
    li.classList.add('is-active');
    li.setAttribute('aria-selected', 'true');
    scrollListTo(li);
  }
  $('wb-ed-empty').hidden = true;
  body.hidden = false;
  const lim = e.lim ? (LIMIT_LABELS[e.lim] || e.lim) : '';
  $('wb-ed-head').innerHTML = `<code>${esc(e.k)}</code>${e.c ? `<span>${esc(e.c)}</span>` : ''}<span>${esc(SECTION_LABELS[e.s] || e.s)}</span>${lim ? `<span>shown on: ${esc(lim)}</span>` : ''}`;
  $('wb-ed-src').textContent = e.src;
  ta.setAttribute('aria-label', `Translation of ${e.k}`);
  if (prev !== i || ta.value !== (e.t || '')) ta.value = e.t || '';
  renderBoxRows(e);
  updateRowEl(body, e, true);
  autoGrow(ta);
  updatePos();
  if (o.focus) {
    ta.focus({ preventScroll: true });
    ta.selectionStart = ta.selectionEnd = ta.value.length;
  }
  if (o.reveal && window.matchMedia('(max-width: 68rem)').matches) {
    $('wb-ed').scrollIntoView({ block: 'start', behavior: 'smooth' });
  }
}

function updatePos() {
  const pos = S.cur == null ? -1 : S.filtered.indexOf(S.cur);
  $('wb-ed-pos').textContent = pos >= 0 ? `line ${pos + 1} of ${S.filtered.length}` : (S.cur != null ? 'not in the current filter' : '');
  $('wb-prev').disabled = pos <= 0;
  $('wb-next').disabled = pos < 0 ? !S.filtered.length : pos >= S.filtered.length - 1;
}

// The other rows of a dialog box, as context: each one picks that row.
function renderBoxRows(e) {
  const el = $('wb-ed-box');
  const rows = e.box != null ? (S.byBox.get(e.box) || []) : [];
  if (rows.length < 2) { el.innerHTML = ''; return; }
  el.innerHTML = rows.map((x, n) => {
    const filled = x.t && x.t.trim();
    return `<button type="button" class="wb-boxrow${x === e ? ' is-active' : ''}" data-i="${S.indexOf.get(x.k)}" title="${esc(x.k)}"><span>row ${n + 1}</span><span>${esc(x.src)}</span><span>${filled ? esc(x.t) : '-'}</span></button>`;
  }).join('');
}

// Step through the filtered lines, across pages.
function stepLine(dir) {
  if (!S.filtered.length) return;
  let pos = S.cur == null ? -1 : S.filtered.indexOf(S.cur);
  if (pos < 0) pos = S.page * S.pageSize - (dir > 0 ? 1 : -1);
  const next = Math.max(0, Math.min(S.filtered.length - 1, pos + dir));
  const page = Math.floor(next / S.pageSize);
  S.cur = S.filtered[next];
  if (page !== S.page) { S.page = page; renderList(); }
  selectLine(S.filtered[next], { focus: true });
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
  ta.style.height = Math.min(ta.scrollHeight + 2, 180) + 'px';
}

function refilter(resetPage) {
  S.filtered = [];
  for (let i = 0; i < S.entries.length; i++) if (matches(S.entries[i])) S.filtered.push(i);
  if (resetPage) S.page = 0;
  // Keep the line being edited when it survives the filter; otherwise the
  // editor moves to the first line of the page.
  const keep = S.cur != null && S.filtered.includes(S.cur);
  if (keep && resetPage) S.page = Math.floor(S.filtered.indexOf(S.cur) / S.pageSize);
  renderList();
  if (keep) selectLine(S.cur);
  else selectLine(S.filtered.length ? S.filtered[S.page * S.pageSize] : null);
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
  revealWork();
}

// Scroll the list + editor into view (together they fill one screen).
function revealWork() {
  $('editor').scrollIntoView({ behavior: 'smooth', block: 'start' });
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
      if (st === 'too_long' || st === 'rolled_back' || st === 'not_encodable' || st === 'undrawable') r.bad++;
    }
    rows.set(e.s, r);
  }
  let html = '<tr><th>Section</th><th class="num">Translated</th><th class="wb-cov-meter"></th><th class="num">Problems</th></tr>';
  for (const [s, r] of rows) {
    html += `<tr class="is-link" data-section="${esc(s)}"><td>${esc(SECTION_LABELS[s] || s)}</td>
      <td class="num">${r.filled} / ${r.total}</td><td class="wb-cov-meter">${meter(0, r.filled, r.total, false)}</td>
      <td class="num ${r.bad ? 'wb-bad' : ''}">${r.bad || ''}</td></tr>`;
  }
  $('wb-coverage').innerHTML = html;
  renderProgress(rows);
}

// The list pane's progress line and the collapsed dashboard's summary.
function renderProgress(rows) {
  let total = 0, filled = 0, bad = 0;
  for (const r of rows.values()) { total += r.total; filled += r.filled; bad += r.bad; }
  const pct = total ? ((filled / total) * 100).toFixed(1) : '0.0';
  const probs = bad ? `, <span class="wb-bad">${bad} with problems</span>` : '';
  $('wb-progress').innerHTML = `<span>${filled} / ${total} translated (${pct}%)${probs}</span>${meter(0, filled, total, false)}`;
  $('wb-space-sum').textContent = `- coverage per section, name tables, label pools, scenes fullest first`;
}

function renderRegions() {
  const regions = (S.regions || (S.disc ? S.disc.name_regions : [])).map((r) => r);
  regions.sort((a, b) => (b.pack_free ?? b.english_free) - (a.pack_free ?? a.english_free) || b.total - a.total);
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
  const full = regions.filter((r) => (r.pack_free ?? r.english_free) === 0).length;
  html += `<div class="wb-note">${regions.length} regions, ${totalFree} bytes free in total${S.regions ? ' after your pack' : ' (English)'}; the roomiest are shown, ${full} are full. A longer name moves into the free bytes of any region.</div>`;
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
  html += `<div class="wb-note">A longer label moves, with every reference the game uses to reach it, into bytes its image's other labels free or the menu overlay's reserved spare room; a few pinned labels keep their slot. See "Room per category".</div>`;
  $('wb-pools').innerHTML = html;
}

const CATEGORY_LABELS = {
  items: 'Item, spell, arts names', monster_names: 'Monster names', place_names: 'Place names',
  ui_menu: 'Menu labels', system_text: 'System messages',
};

function renderCategories() {
  if (!S.disc || !S.disc.categories) return;
  const after = new Map();
  if (S.report && S.report.categories) for (const c of S.report.categories) after.set(c.category, c.free_pack);
  let html = `<tr><th>Category</th><th class="num">Strings</th><th class="num">Can grow</th>
    <th class="num">Free</th><th class="num">Spare</th>${S.report ? '<th class="num">Left</th>' : ''}</tr>`;
  for (const c of S.disc.categories) {
    const tip = `${c.carrier}; reached by ${c.addressing}. Longer: ${c.growth}.` +
      (c.competing.length ? ` Shared with: ${c.competing.join('; ')}.` : '');
    const left = after.get(c.category);
    html += `<tr title="${esc(tip)}"><td>${esc(CATEGORY_LABELS[c.category] || c.category)}</td>
      <td class="num">${c.strings}</td><td class="num">${c.growable}</td>
      <td class="num">${c.free_english}</td><td class="num">${c.spare_bytes || ''}</td>
      ${S.report ? `<td class="num">${left ?? ''}</td>` : ''}</tr>`;
  }
  $('wb-categories').innerHTML = html;
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
  renderCategories();
  renderRegions();
  renderMonsters();
  renderPools();
  renderScenes();
}

// --- Accents --------------------------------------------------------------------
const MODE_LABEL = { '': 'strict', fold: 'fold', font: 'font' };

function renderAccents() {
  if (!S.wb) return;
  let info = null;
  try { info = JSON.parse(S.wb.accent_info()); } catch (e) { info = null; }
  S.accentInfo = info;
  S.accents = info ? info.mode : '';
  $('wb-accent-mode').value = MODE_LABEL[S.accents] || 'strict';
  const disc = info && info.disc_font;
  const note = {
    applied: 'Your disc already carries the accent font.',
    partial: 'Your disc carries a different font patch in some accent cells.',
    absent: 'Your disc has the retail font: it draws plain letters only.',
  }[disc] || '';
  const how = {
    '': 'Every accent is reported until you fold it or switch modes.',
    fold: 'On import every accent is written as its plain letter (Épée becomes Epee).',
    font: 'On import the patch also draws accented letters into the game font, built from your disc\'s own letters, and every accent is kept. The previews below use that font.',
  }[S.accents] || '';
  $('wb-accent-note').textContent = `${note} ${how}`;
  renderPalette();
  renderCharStats();
}

function renderPalette() {
  const info = S.accentInfo;
  if (!info || !info.palette) return;
  const sel = $('wb-pal-lang');
  if (!sel.options.length) {
    sel.innerHTML = info.palette.map((l) => `<option value="${esc(l.code)}">${esc(l.name)}</option>`).join('');
    const guess = (languageOf() || '').toLowerCase().split('-')[0];
    if (info.palette.some((l) => l.code === guess)) sel.value = guess;
  }
  const lang = info.palette.find((l) => l.code === sel.value) || info.palette[0];
  $('wb-palette').innerHTML = lang.chars.map((c) => {
    const drawn = c.cell && (S.accents === 'font' || info.disc_font === 'applied');
    const t = drawn ? `draws in the accent font as ${c.cell}` : `folds to "${c.fold}"${c.cell ? ` (the accent font draws it as ${c.cell})` : ' (no glyph, even in the accent font)'}`;
    return `<button type="button" class="wb-pal-key ${drawn ? 'is-drawn' : ''}" data-ch="${esc(c.ch)}" title="${esc(t)}">${esc(c.ch)}</button>`;
  }).join('');
}

// The {ce:NN} symbol palette: buttons and icons drawn from the disc's own
// sprites (names from legaia_translate::symbols). Inserts the
// hex token the export writes; the name alias is shown as a hint.
function renderSymbols() {
  const el = $('wb-symbols');
  const field = el.closest('.wb-sym-field');
  if (!S.wb || typeof S.wb.symbols !== 'function') { field.hidden = true; return; }
  let syms = [];
  try { syms = Array.from(S.wb.symbols()); } catch (e) { syms = []; }
  field.hidden = !syms.length;
  el.innerHTML = syms.map((s, i) => {
    const pic = s.rgba ? `<canvas data-sym="${i}"></canvas>` : `<span>#${s.index - 0x0b}</span>`;
    const t = `${s.name}: ${s.token}, or type ${s.alias}`;
    return `<button type="button" class="wb-sym-key" data-ch="${esc(s.token)}" title="${esc(t)}" aria-label="${esc(s.name)}">${pic}<code>${esc(s.token.slice(4, 6))}</code></button>`;
  }).join('');
  for (const c of el.querySelectorAll('canvas[data-sym]')) {
    const s = syms[+c.dataset.sym];
    c.width = s.w;
    c.height = s.h;
    c.style.width = (s.w * 2) + 'px';
    c.style.height = (s.h * 2) + 'px';
    c.getContext('2d').putImageData(new ImageData(new Uint8ClampedArray(s.rgba), s.w, s.h), 0, 0);
  }
}

function renderCharStats() {
  if (!S.wb) return;
  let st = null;
  try { st = JSON.parse(S.wb.char_stats()); } catch (e) { st = null; }
  const el = $('wb-char-stats');
  if (!st) { el.textContent = ''; return; }
  let html = st.undrawable_lines
    ? `<span class="wb-bad">${st.undrawable_lines} line(s) contain ${st.undrawable_chars} character(s) the game will not draw as typed</span>`
    : '<span class="wb-okc">Every translated line draws as typed</span>';
  if (st.fold_lines) html += `; ${st.fold_lines} line(s) fold accents to plain letters on import`;
  if (st.top && st.top.length) html += `. Most frequent: ${st.top.slice(0, 10).map((t) => `<code>${esc(t.char)}</code>&times;${t.count}`).join(' ')}`;
  if (st.undrawable_lines || st.fold_lines) html += ' <button type="button" class="wb-button wb-button-ghost wb-mini" id="wb-show-chars">Show these lines</button>';
  el.innerHTML = html;
}

// Re-run the live check of every translated line (after the accent mode
// changes, every verdict and width can change).
function recheckAll() {
  for (const e of S.entries) {
    if (!e.t || !e.t.trim()) continue;
    try { applyCheck(e, JSON.parse(S.wb.check(e.k, e.t))); } catch (err) { console.warn(err); }
  }
}

async function setAccentMode(mode) {
  if (!S.wb) return;
  S.wb.set_accents(mode);
  recheckAll();
  renderAccents();
  refilter(false);
  renderCoverage();
  scheduleSave();
  if (S.report) setCheckStatus('The accent mode changed - run "Check against my disc" again for the scene fits.', '');
}

function insertChar(ch) {
  const ta = S.lastTa && document.body.contains(S.lastTa) ? S.lastTa : null;
  if (!ta) { setStatus('Click into a translation first, then pick a character.', ''); return; }
  const a = ta.selectionStart ?? ta.value.length;
  const b = ta.selectionEnd ?? a;
  ta.value = ta.value.slice(0, a) + ch + ta.value.slice(b);
  ta.focus();
  ta.selectionStart = ta.selectionEnd = a + ch.length;
  ta.dispatchEvent(new Event('input', { bubbles: true }));
}

function applyFix(e, fix) {
  if (fix === 'font') { setAccentMode('font'); return; }
  const mod = wasmMod;
  if (!mod) return;
  const text = fix === 'fold' ? mod.fold_line(e.t || '') : mod.cells_line(e.t || '');
  const ta = $('wb-ed-input');
  ta.value = text;
  autoGrow(ta);
  onEdit(e, text);
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
  dashTimer = setTimeout(() => { renderCoverage(); renderMonsters(); renderCharStats(); }, 500);
}

// Re-draw every list row's status and the editor (after a fit).
function refreshVisibleRows() {
  for (const el of $('wb-list').querySelectorAll('.wb-li')) updateLi(+el.dataset.i);
  if (S.cur != null) updateRowEl($('wb-ed-body'), S.entries[S.cur], false);
}

function updateRowEl(el, e, redraw) {
  el.className = `wb-row st-${statusOf(e)}`;
  el.querySelector('.wb-room').textContent = roomText(e);
  el.querySelector('.wb-out').innerHTML = outcomeText(e);
  el.querySelector('.wb-width').innerHTML = widthText(e, e.t && e.t.trim() ? e.px : e.spx);
  el.querySelector('.wb-errs').innerHTML = markedText(e);
  if (redraw) drawPreview(e, el.querySelector('.wb-preview canvas'), el.querySelector('.wb-pv-note'));
}

function onEdit(e, text) {
  e.t = text;
  let c = null;
  try { c = JSON.parse(S.wb.set_translation(e.k, text)); } catch (err) { console.warn(err); }
  applyCheck(e, c);
  if (!text.trim()) S.outcome.delete(e.k);
  updateRowEl($('wb-ed-body'), e, true);
  updateLi(S.indexOf.get(e.k));
  if (e.box != null) renderBoxRows(e);
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
  S.indexOf = new Map();
  S.entries.forEach((e, i) => S.indexOf.set(e.k, i));
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
  S.cur = null;
  $('wb-issues').hidden = true;
  setCheckStatus('');
}

function fillSectionFilter() {
  const seen = [];
  for (const e of S.entries) if (!seen.includes(e.s)) seen.push(e.s);
  $('wb-f-section').innerHTML = '<option value="">All sections</option>' +
    seen.map((s) => `<option value="${esc(s)}">${esc(SECTION_LABELS[s] || s)}</option>`).join('');
}

async function afterPackChange(msg, reveal) {
  ingestEntries();
  applyNamesFit();
  fillSectionFilter();
  $('wb-main').hidden = false;
  renderAccents();
  renderSymbols();
  renderDashboard();
  refilter(true);
  setStatus(msg, 'ok');
  const filled = S.entries.some((e) => e.t && e.t.trim());
  if (filled) await runCheck(true);
  if (reveal) revealWork();
}

async function openDisc(file) {
  setStatus(`Reading ${file.name} (nothing is uploaded) ...`);
  try {
    const mod = await ensureWasm();
    // A locally served site/wasm/ is uncommitted build output; one built
    // before the workbench existed loads fine but has no session type.
    if (typeof mod.Workbench !== 'function') {
      setStatus('This copy of the site\'s WebAssembly bundle predates the workbench. '
        + 'Rebuild it (scripts/ci/build-wasm.sh) and reload the page.', 'err');
      return;
    }
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
      await afterPackChange(`Resumed ${k} line(s) of your ${info.language} pack.`, true);
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
    await afterPackChange(msg, true);
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
    await afterPackChange(`Started an empty ${lang} pack. Set the language code above.`, true);
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

  $('wb-pager').addEventListener('click', (ev) => {
    const b = ev.target.closest('button[data-p]');
    if (!b) return;
    S.page += b.dataset.p === 'next' ? 1 : -1;
    renderList();
    selectLine(S.filtered[S.page * S.pageSize]);
  });

  $('wb-accent-mode').addEventListener('change', (ev) => setAccentMode(ev.target.value));
  $('wb-pal-lang').addEventListener('change', renderPalette);
  $('wb-palette').addEventListener('mousedown', (ev) => {
    // Keep the textarea's caret: insert on mousedown, before focus moves.
    const k = ev.target.closest('[data-ch]');
    if (!k) return;
    ev.preventDefault();
    insertChar(k.dataset.ch);
  });
  $('wb-palette').addEventListener('keydown', (ev) => {
    const k = ev.target.closest('[data-ch]');
    if (k && (ev.key === 'Enter' || ev.key === ' ')) { ev.preventDefault(); insertChar(k.dataset.ch); }
  });
  $('wb-symbols').addEventListener('mousedown', (ev) => {
    const k = ev.target.closest('[data-ch]');
    if (!k) return;
    ev.preventDefault();
    insertChar(k.dataset.ch);
  });
  $('wb-symbols').addEventListener('keydown', (ev) => {
    const k = ev.target.closest('[data-ch]');
    if (k && (ev.key === 'Enter' || ev.key === ' ')) { ev.preventDefault(); insertChar(k.dataset.ch); }
  });
  $('wb-char-stats').addEventListener('click', (ev) => {
    if (ev.target.id !== 'wb-show-chars') return;
    S.filter.status = 'chars';
    $('wb-f-status').value = 'chars';
    refilter(true);
    revealWork();
  });
  const ta = $('wb-ed-input');
  S.lastTa = ta;
  $('wb-list').addEventListener('click', (ev) => {
    const li = ev.target.closest('.wb-li');
    if (li) selectLine(+li.dataset.i, { focus: true, reveal: true });
  });
  $('wb-ed-box').addEventListener('click', (ev) => {
    const b = ev.target.closest('.wb-boxrow');
    if (b && !b.classList.contains('is-active')) selectLine(+b.dataset.i, { focus: true });
  });
  $('wb-ed-body').addEventListener('click', (ev) => {
    const b = ev.target.closest('button[data-fix]');
    if (b && S.cur != null) applyFix(S.entries[S.cur], b.dataset.fix);
  });
  ta.addEventListener('input', () => {
    if (S.cur == null) return;
    autoGrow(ta);
    onEdit(S.entries[S.cur], ta.value);
  });
  $('wb-prev').addEventListener('click', () => stepLine(-1));
  $('wb-next').addEventListener('click', () => stepLine(1));
  // Ctrl+Enter / Alt+Down: next line; Ctrl+Shift+Enter / Alt+Up: previous.
  // Bound on the document so they also work with focus in the list or on a
  // palette key, but only while the workbench is open.
  document.addEventListener('keydown', (ev) => {
    if ($('wb-main').hidden || ev.defaultPrevented) return;
    let dir = 0;
    if (ev.key === 'Enter' && (ev.ctrlKey || ev.metaKey)) dir = ev.shiftKey ? -1 : 1;
    else if (ev.altKey && !ev.ctrlKey && !ev.metaKey && ev.key === 'ArrowDown') dir = 1;
    else if (ev.altKey && !ev.ctrlKey && !ev.metaKey && ev.key === 'ArrowUp') dir = -1;
    if (!dir) return;
    const t = ev.target;
    if (t !== ta && !(t.closest && t.closest('.wb-work'))) return;
    ev.preventDefault();
    stepLine(dir);
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
