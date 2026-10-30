/* Expansion workbench: search and cross-reference over the whole pack.
 *
 * Where an id is defined, set, read and referenced is the format crate's
 * reference model (`legaia_expansion_format::xref`, through
 * `ExpansionWorkbench::xref_sites`): the same index the rename, the flow
 * graph and the validator's flow lints use, so Find, Rename and the checks
 * never disagree. A site is `{ kind, id, role, file, field }` in the
 * validator's own spelling, so a hit is a jump target exactly as a check is.
 *
 * Three questions are answered:
 *   - an id (`ogre`, `tower_intro`, a pack flag): every site naming it, by
 *     kind and role, with a Rename per kind;
 *   - a retail coordinate (`monster:109`, `item:5`, `scene:town01`,
 *     `bgm:12`): every `{ kind, id }` naming it (a walk over the JSON - a
 *     coordinate is not a pack id);
 *   - anything else: every key or value containing the text.
 * With no query the panel lists the pack's flags and counters with how
 * often each is written and read, so a flag read and never set (a gate that
 * can never open) stands out.
 */

import { h } from './expansion-workbench-forms.js';
import { openRename, kindLabel } from './expansion-workbench-rename.js';

const SECTION_FILE = { manifest: 'pack.toml' };

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

// Every leaf and every object, with its path (the coordinate and text
// searches).
function walk(node, path, out) {
  if (Array.isArray(node)) {
    node.forEach((v, i) => walk(v, path.concat(i), out));
  } else if (node && typeof node === 'object') {
    for (const [k, v] of Object.entries(node)) {
      out.push({ path: path.concat(k), key: k, value: k, isKey: true });
      walk(v, path.concat(k), out);
    }
    out.push({ path, key: path[path.length - 1], value: node, isObj: true });
  } else {
    out.push({ path, key: path[path.length - 1], value: node });
  }
}

function fieldKey(path) {
  for (let i = path.length - 1; i >= 0; i--) if (typeof path[i] === 'string') return path[i];
  return '';
}

const ROLE_ORDER = ['defines', 'sets', 'reads', 'references'];
const ROLE_LABEL = { defines: 'Defined', sets: 'Set / written by', reads: 'Read / gated by', references: 'Referenced by' };

/**
 * Every hit for `query`: `{ mode, groups: [{ kind, role, hits }] }`. `sites`
 * is the reference model's site list.
 */
export function search(pack, sites, query) {
  const q = query.trim();
  if (!pack || !q) return { mode: 'none', groups: [] };
  const coord = q.match(/^(monster|item|scene|bgm|sfx|spell|prot|npc):\s*(\S+)$/i);
  if (coord) {
    const all = [];
    walk(pack, [], all);
    const kind = coord[1].toLowerCase();
    const want = coord[2];
    const hits = all.filter((x) => x.isObj && x.value && !Array.isArray(x.value) && x.value.kind === kind
      && (String(x.value.id) === want || String(x.value.name) === want || (want.startsWith('0x') && x.value.id === parseInt(want, 16))));
    // Bare ids where a field is a retail id by name (`monster = 109` in a
    // defeat objective, `item = 0x8e` in a cutscene step).
    for (const x of all) {
      if (typeof x.value === 'number' && fieldKey(x.path) === kind && String(x.value) === want) hits.push(x);
    }
    return { mode: 'coord', groups: [{ role: 'references', hits: hits.map((x) => locate(x.path)) }] };
  }
  const exact = (sites || []).filter((s) => s.id === q);
  if (exact.length) {
    const kinds = [...new Set(exact.map((s) => s.kind))];
    const groups = [];
    for (const kind of kinds) {
      for (const role of ROLE_ORDER) {
        const hits = exact.filter((s) => s.kind === kind && s.role === role);
        if (hits.length) groups.push({ kind, role, hits });
      }
    }
    return { mode: 'id', kinds, groups };
  }
  const all = [];
  walk(pack, [], all);
  const lq = q.toLowerCase();
  const hits = all.filter((x) => !x.isObj && (
    (typeof x.value === 'string' && x.value.toLowerCase().includes(lq))
    || (typeof x.value === 'number' && String(x.value) === q)
    || (typeof x.key === 'string' && x.key.toLowerCase() === lq)))
    .map((x) => Object.assign(locate(x.path), { value: x.value }));
  return { mode: 'text', groups: [{ role: 'references', hits }] };
}

// Flags the engine raises itself: a cutscene's seen flag, a once-only
// trigger's, and the run rules' (`run.ng_plus`, `run.hard`, ...).
const ENGINE_FLAG = /^(cutscene|map|run)\./;

/** The pack's flags and counters: `[{ name, kind, sets, reads, engine }]`. */
export function flagIndex(sites) {
  const by = new Map();
  for (const s of sites || []) {
    if (s.kind !== 'flag' && s.kind !== 'counter') continue;
    const k = `${s.kind}:${s.id}`;
    const e = by.get(k) || { name: s.id, kind: s.kind, sets: 0, reads: 0, engine: s.kind === 'flag' && ENGINE_FLAG.test(s.id) };
    if (s.role === 'sets') e.sets++; else e.reads++;
    by.set(k, e);
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
 * `o.pack()` the pack, `o.sites()` the reference model's sites, `o.jump({
 * path, field })` a jump, and `o.rename` the rename calls (`plan`, `apply`,
 * `done`).
 */
export function createFind(host, o) {
  const input = h('input', { type: 'search', id: 'xw-find', placeholder: 'id, flag, text, monster:109 ...', 'aria-label': 'Find in the pack', spellcheck: 'false', autocomplete: 'off' });
  const out = h('div', { class: 'xw-find-out', role: 'region', 'aria-live': 'polite' });
  host.append(h('div', { class: 'xw-find-row' }, input), out);
  let timer = null;
  const renameButton = (kind, id) => h('button', {
    type: 'button', class: 'wb-mini', title: `Rename this ${kindLabel(kind)} and every reference to it`,
    onclick: (ev) => {
      const old = out.querySelector('.xw-rename');
      if (old) old.remove();
      const box = ev.target.closest('.xw-find-kind');
      openRename(box || out, {
        kind, id, plan: o.rename.plan, apply: o.rename.apply, jump: o.jump,
        done: (res, msg) => { o.rename.done(res, msg); input.value = res.newId; render(); },
      });
    },
  }, 'Rename');
  const render = () => {
    out.innerHTML = '';
    const pack = o.pack();
    if (!pack) return;
    const sites = o.sites();
    const q = input.value;
    if (!q.trim()) {
      const flags = flagIndex(sites);
      if (!flags.length) { out.append(h('div', { class: 'xw-dim' }, 'Type an id, a pack flag or any text. monster:109, item:5 or scene:town01 find a retail coordinate.')); return; }
      out.append(h('div', { class: 'xw-find-head' }, `Pack flags and counters (${flags.length})`));
      for (const f of flags) {
        const unset = f.reads && !f.sets && !f.engine;
        const warn = f.engine && !f.sets ? ' set by the engine' : unset ? ' never set' : f.sets && !f.reads ? ' never read' : '';
        out.append(h('button', {
          type: 'button', class: 'xw-hit' + (unset ? ' is-warn' : ''), title: unset ? 'Read by a gate and set nowhere in the pack: the gate never opens' : '',
          onclick: () => { input.value = f.name; render(); },
        }, h('code', {}, f.name), h('span', { class: 'xw-dim' }, ` ${f.kind === 'counter' ? 'counter, ' : ''}set ${f.sets}, read ${f.reads}${warn ? ' -' + warn : ''}`)));
      }
      return;
    }
    const res = search(pack, sites, q);
    const total = res.groups.reduce((n, g) => n + g.hits.length, 0);
    out.append(h('div', { class: 'xw-find-head' }, total ? `${total} place(s)` + (res.mode === 'id' ? ' naming this id' : res.mode === 'coord' ? ' naming this coordinate' : ' matching') : 'Nothing found.'));
    let kindBox = null;
    let lastKind = null;
    for (const g of res.groups) {
      if (!g.hits.length) continue;
      let into = out;
      if (res.mode === 'id') {
        if (g.kind !== lastKind) {
          lastKind = g.kind;
          const engine = g.kind === 'flag' && ENGINE_FLAG.test(q.trim());
          kindBox = h('div', { class: 'xw-find-kind' },
            h('div', { class: 'xw-find-role' }, kindLabel(g.kind), ' ', engine ? h('span', { class: 'xw-dim' }, '(raised by the engine)') : renameButton(g.kind, q.trim())));
          out.append(kindBox);
        }
        into = kindBox;
        into.append(h('div', { class: 'xw-dim' }, ROLE_LABEL[g.role]));
      }
      for (const x of g.hits.slice(0, 300)) {
        into.append(h('button', { type: 'button', class: 'xw-hit', onclick: () => o.jump({ path: x.file, field: x.field }) },
          h('span', { class: 'xw-hit-file' }, x.file), ' ', h('code', {}, x.field || '(file)'),
          res.mode === 'text' ? h('span', { class: 'xw-dim' }, ' ' + snippet(x.value)) : null));
      }
      if (g.hits.length > 300) into.append(h('div', { class: 'xw-dim' }, `... and ${g.hits.length - 300} more`));
    }
  };
  input.addEventListener('input', () => { clearTimeout(timer); timer = setTimeout(render, 120); });
  return {
    input,
    refresh: render,
    find: (q) => { input.value = q; render(); input.focus(); },
  };
}
