/* Expansion workbench: search and cross-reference over the whole pack.
 *
 * The pack is the page's JSON (`S.pack`, serde's form of `ExpansionPack`), so
 * one walk over it finds every place a value sits, in the validator's own
 * spelling: the file (`battle.toml`, `maps/<id>.toml`) and the dotted field
 * path inside it (`monsters.ogre.hp`, `quests[2].objectives[0]`). A hit is
 * a jump target exactly as a validation issue is - clicking one opens its
 * tab, selects its item and focuses the field.
 *
 * Three questions are answered:
 *   - free text: every key or value containing the query;
 *   - an id (`ogre`, `tower_intro`, a pack flag): where it is defined, set,
 *     read and referenced, by the shape of the place it sits in;
 *   - a retail coordinate (`monster:109`, `item:5`, `scene:town01`,
 *     `bgm:12`): every `{ kind, id }` naming it.
 * With no query the panel lists the pack's flags and counters with how
 * often each is set and read, so a flag read and never set (a gate that can
 * never open) stands out.
 */

import { h } from './expansion-workbench-forms.js';

// The file a top-level pack key exports to.
const SECTION_FILE = {
  manifest: 'pack.toml', battle: 'battle.toml', progression: 'progression.toml', quests: 'quests.toml',
  dialog: 'dialog.toml', cards: 'cards.toml', cutscenes: 'cutscenes.toml', modes: 'modes.toml',
  items: 'items.toml', audio: 'audio.toml', arts: 'arts.toml', spells: 'spells.toml',
};

// Collections whose map keys are the ids they define (file-relative path).
const KEYED = new Set([
  'monsters', 'formations', 'bosses', 'lines', 'items', 'sets', 'arts', 'chains', 'spells',
  'tracks', 'sounds', 'cutscenes', 'tracks', 'legend',
]);

// Keys whose string values (or list members) write a flag or counter.
const WRITE_KEYS = new Set(['set_flags', 'clear_flags', 'raise_flags']);
// Keys whose values read one.
const READ_KEYS = new Set(['flags', 'flags_clear', 'flags_set', 'unless_flags']);
// A step / event whose `flag` or `counter` field writes.
const WRITE_DO = new Set(['set_flag', 'clear_flag', 'counter']);

/** Split a pack-JSON path (array of keys) into `{ file, field }`. */
export function locate(path) {
  if (!path.length) return { file: 'pack.toml', field: '' };
  let file;
  let rest;
  if (path[0] === 'maps' && path.length > 1) {
    file = `maps/${path[1]}.toml`;
    rest = path.slice(2);
  } else {
    file = SECTION_FILE[path[0]] || `${path[0]}.toml`;
    rest = path.slice(1);
  }
  let field = '';
  for (const k of rest) field = typeof k === 'number' ? `${field}[${k}]` : field ? `${field}.${k}` : String(k);
  return { file, field };
}

// Every leaf and every keyed-collection key, with its path and parent.
function walk(node, path, parent, out) {
  if (Array.isArray(node)) {
    node.forEach((v, i) => walk(v, path.concat(i), node, out));
  } else if (node && typeof node === 'object') {
    const keyed = path.length && KEYED.has(path[path.length - 1]);
    for (const [k, v] of Object.entries(node)) {
      if (keyed) out.push({ path: path.concat(k), key: k, value: k, parent: node, isKey: true });
      walk(v, path.concat(k), node, out);
    }
    out.push({ path, key: path[path.length - 1], value: node, parent, isObj: true });
  } else {
    out.push({ path, key: path[path.length - 1], value: node, parent });
  }
}

// The last string key of a path (the field name a leaf sits under; a list
// member's is its list's).
function fieldKey(path) {
  for (let i = path.length - 1; i >= 0; i--) if (typeof path[i] === 'string') return path[i];
  return '';
}

// What role a string leaf equal to an id plays where it sits.
function roleOf(hit) {
  const { path, parent, isKey } = hit;
  if (isKey) return 'defined';
  const key = fieldKey(path);
  const inGate = path.some((k) => k === 'requires' || k === 'prerequisites');
  if (key === 'id' && !Array.isArray(parent)) return 'defined';
  if (WRITE_KEYS.has(key)) return 'sets';
  // A quest reward's `counters = [{ counter }]` adds to the counter.
  if (key === 'counter' && path.includes('counters')) return 'sets';
  // A quest objective of kind `counter` waits on the counter; a cutscene
  // step or map event of that kind adds to it.
  if (key === 'counter' && path.includes('objectives')) return 'reads';
  if ((key === 'flag' || key === 'counter') && parent && !Array.isArray(parent) && WRITE_DO.has(parent.do || parent.kind)) return 'sets';
  if (READ_KEYS.has(key) || inGate) return 'reads';
  if (key === 'counter') return 'reads';
  return 'references';
}

const ROLE_ORDER = ['defined', 'sets', 'reads', 'references'];
const ROLE_LABEL = { defined: 'Defined', sets: 'Set / written by', reads: 'Read / gated by', references: 'Referenced by' };

/** Every hit for `query` in `pack`: `{ mode, groups: [{ role, hits }] }`. */
export function search(pack, query) {
  const q = query.trim();
  if (!pack || !q) return { mode: 'none', groups: [] };
  const all = [];
  walk(pack, [], null, all);
  const coord = q.match(/^(monster|item|scene|bgm|sfx|spell|prot|npc):\s*(\S+)$/i);
  if (coord) {
    const kind = coord[1].toLowerCase();
    const want = coord[2];
    const hits = all.filter((x) => x.isObj && x.value && !Array.isArray(x.value) && x.value.kind === kind
      && (String(x.value.id) === want || String(x.value.name) === want || (want.startsWith('0x') && x.value.id === parseInt(want, 16))));
    // Bare ids where a field is a retail id by name (`monster = 109` in a
    // defeat objective, `item = 0x8e` in a cutscene step).
    for (const x of all) {
      if (typeof x.value === 'number' && fieldKey(x.path) === kind && String(x.value) === want) hits.push(x);
    }
    return { mode: 'coord', groups: [{ role: 'references', hits }] };
  }
  // An exact id: the hits whose string value (or key) is the query.
  const exact = all.filter((x) => !x.isObj && typeof x.value === 'string' && x.value === q);
  if (exact.length) {
    const groups = ROLE_ORDER.map((role) => ({ role, hits: exact.filter((x) => roleOf(x) === role) })).filter((g) => g.hits.length);
    return { mode: 'id', groups };
  }
  const lq = q.toLowerCase();
  const hits = all.filter((x) => !x.isObj && (
    (typeof x.value === 'string' && x.value.toLowerCase().includes(lq))
    || (typeof x.value === 'number' && String(x.value) === q)
    || (typeof x.key === 'string' && x.key.toLowerCase() === lq)));
  return { mode: 'text', groups: [{ role: 'references', hits }] };
}

// Flags the engine raises itself: a cutscene's seen flag, a once-only
// trigger's, and the run rules' (`run.ng_plus`, `run.hard`, ...).
const ENGINE_FLAG = /^(cutscene|map|run)\./;

/** The pack's flags and counters: `[{ name, sets, reads, engine }]`. */
export function flagIndex(pack) {
  const all = [];
  if (pack) walk(pack, [], null, all);
  const by = new Map();
  for (const x of all) {
    if (x.isObj || x.isKey || typeof x.value !== 'string' || !x.value) continue;
    const key = fieldKey(x.path);
    const isFlagField = WRITE_KEYS.has(key) || READ_KEYS.has(key) || key === 'flag' || key === 'counter';
    if (!isFlagField) continue;
    const role = roleOf(x);
    const e = by.get(x.value) || { name: x.value, sets: 0, reads: 0, engine: ENGINE_FLAG.test(x.value) };
    if (role === 'sets') e.sets++; else e.reads++;
    by.set(x.value, e);
  }
  return [...by.values()].sort((a, b) => a.name.localeCompare(b.name));
}

const snippet = (v) => {
  if (v == null) return '';
  const s = typeof v === 'string' ? v : JSON.stringify(v);
  return s.length > 60 ? s.slice(0, 57) + '...' : s;
};

/**
 * The Find panel: an input and its results. `host` is the element to fill;
 * `o.pack()` the pack, `o.jump({ path, field })` a jump.
 */
export function createFind(host, o) {
  const input = h('input', { type: 'search', id: 'xw-find', placeholder: 'id, flag, text, monster:109 ...', 'aria-label': 'Find in the pack', spellcheck: 'false', autocomplete: 'off' });
  const out = h('div', { class: 'xw-find-out', role: 'region', 'aria-live': 'polite' });
  host.append(h('div', { class: 'xw-find-row' }, input), out);
  let timer = null;
  const render = () => {
    out.innerHTML = '';
    const pack = o.pack();
    if (!pack) return;
    const q = input.value;
    if (!q.trim()) {
      const flags = flagIndex(pack);
      if (!flags.length) { out.append(h('div', { class: 'xw-dim' }, 'Type an id, a pack flag or any text. monster:109, item:5 or scene:town01 find a retail coordinate.')); return; }
      out.append(h('div', { class: 'xw-find-head' }, `Pack flags and counters (${flags.length})`));
      for (const f of flags) {
        const unset = f.reads && !f.sets && !f.engine;
        const warn = f.engine && !f.sets ? ' set by the engine' : unset ? ' never set' : f.sets && !f.reads ? ' never read' : '';
        out.append(h('button', {
          type: 'button', class: 'xw-hit' + (unset ? ' is-warn' : ''), title: unset ? 'Read by a gate and set nowhere in the pack: the gate never opens' : '',
          onclick: () => { input.value = f.name; render(); },
        }, h('code', {}, f.name), h('span', { class: 'xw-dim' }, ` set ${f.sets}, read ${f.reads}${warn ? ' -' + warn : ''}`)));
      }
      return;
    }
    const res = search(pack, q);
    const total = res.groups.reduce((n, g) => n + g.hits.length, 0);
    out.append(h('div', { class: 'xw-find-head' }, total ? `${total} place(s)` + (res.mode === 'id' ? ' naming this id' : res.mode === 'coord' ? ' naming this coordinate' : ' matching') : 'Nothing found.'));
    for (const g of res.groups) {
      if (!g.hits.length) continue;
      if (res.mode === 'id') out.append(h('div', { class: 'xw-find-role' }, ROLE_LABEL[g.role]));
      for (const x of g.hits.slice(0, 300)) {
        const at = locate(x.path);
        out.append(h('button', { type: 'button', class: 'xw-hit', onclick: () => o.jump({ path: at.file, field: at.field }) },
          h('span', { class: 'xw-hit-file' }, at.file), ' ', h('code', {}, at.field || '(file)'),
          res.mode === 'text' ? h('span', { class: 'xw-dim' }, ' ' + snippet(x.value)) : null));
      }
      if (g.hits.length > 300) out.append(h('div', { class: 'xw-dim' }, `... and ${g.hits.length - 300} more`));
    }
  };
  input.addEventListener('input', () => { clearTimeout(timer); timer = setTimeout(render, 120); });
  return {
    input,
    refresh: render,
    find: (q) => { input.value = q; render(); input.focus(); },
  };
}
