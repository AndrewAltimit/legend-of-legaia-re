/* Expansion workbench: the form kit and the section editors (every tab but
 * the map painter, which is expansion-workbench-map.js).
 *
 * The editors work on the pack's JSON form (serde's form of
 * `legaia_expansion_format::ExpansionPack`, the same keys as the TOML) held
 * by the app as `ctx.pack`. A field writes straight into that object and calls
 * `ctx.changed()`; a structural edit (add, remove, rename) calls
 * `ctx.changed(true)`, which also re-renders the tab. The JSON is the only
 * state - nothing is read back from the DOM.
 *
 * Every field carries `data-path`, its dotted path inside its file in the
 * validator's own spelling (`monsters.ogre.hp`, `quests[2].objectives[0]`), so
 * a validation issue can find its field.
 *
 * A disc picker writes a coordinate (`{ kind: "monster", id }`), never a name:
 * the names it shows come from the user's disc and stay in this tab.
 */

const esc = window.escapeHtml;

export const ELEMENTS = ['Earth', 'Water', 'Fire', 'Wind', 'Thunder', 'Light', 'Dark', 'Neutral'];
const RARITIES = ['common', 'uncommon', 'rare', 'legendary'];
const CARD_ELEMENTS = ['fire', 'water', 'wind', 'earth', 'thunder', 'light', 'dark'];
const AI_TIERS = ['novice', 'regular', 'expert'];
const STATS = ['hp', 'mp', 'attack', 'defence', 'agility', 'speed', 'intelligence', 'exp', 'gold'];

// --- DOM helpers ---------------------------------------------------------------
export function h(tag, attrs, ...kids) {
  const el = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs || {})) {
    if (v == null || v === false) continue;
    if (k === 'class') el.className = v;
    else if (k === 'text') el.textContent = v;
    else if (k === 'html') el.innerHTML = v;
    else if (k.startsWith('on')) el.addEventListener(k.slice(2), v);
    else el.setAttribute(k, v === true ? '' : v);
  }
  for (const c of kids.flat()) {
    if (c == null || c === false) continue;
    el.append(c instanceof Node ? c : document.createTextNode(String(c)));
  }
  return el;
}

const joinPath = (base, k) => (base ? (typeof k === 'number' ? `${base}[${k}]` : `${base}.${k}`) : String(k));

function intOr(v, dflt) {
  const n = parseInt(v, 10);
  return Number.isFinite(n) ? n : dflt;
}
function floatOr(v, dflt) {
  const n = parseFloat(v);
  return Number.isFinite(n) ? n : dflt;
}
const csvSplit = (s) => s.split(',').map((x) => x.trim()).filter(Boolean);

// --- Disc catalog lookups ---------------------------------------------------------
function catList(ctx, which) {
  return (ctx.cat && ctx.cat[which]) || [];
}
export function monsterName(ctx, id) {
  const m = catList(ctx, 'monsters').find((x) => x.id === id);
  return m ? m.name : null;
}
export function retailMonster(ctx, id) {
  return catList(ctx, 'monsters').find((x) => x.id === id) || null;
}
function discOptions(ctx, which) {
  const list = catList(ctx, which);
  if (which === 'scenes') return list.map((s) => [s.name, `${s.name} - ${s.display}`]);
  return list.map((x) => [String(x.id), `#${x.id} ${x.name}`]);
}

// --- Fields ------------------------------------------------------------------------
// spec: { k, label, t, opts, hint, wide, ph, kind, base }
// t: text otext textarea otextarea int oint float ofloat bool sel osel csv
//    csvint lines tuple monster omonster item oitem scene oscene bgm obgm
//    monsterid itemid scenename ref oref refs stat music omusic sound osound
export function field(ctx, obj, spec, path) {
  const p = joinPath(path, spec.k);
  const t = spec.t || 'text';
  const wrap = h('label', { class: 'xw-field' + (spec.wide ? ' is-wide' : '') + (t === 'bool' ? ' xw-check' : ''), 'data-path': p });
  const set = (v, structural) => {
    if (v === undefined) delete obj[spec.k];
    else obj[spec.k] = v;
    ctx.changed(structural);
  };
  const cur = obj[spec.k];
  let input;
  const label = h('span', {}, spec.label || spec.k, spec.hint ? h('span', { class: 'xw-hint' }, ` ${spec.hint}`) : null);

  const numInput = (val, opt, isFloat) => {
    input = h('input', { type: 'number', value: val == null ? '' : val, step: isFloat ? 'any' : '1', placeholder: spec.ph || '' });
    input.addEventListener('input', () => {
      const v = input.value.trim();
      if (!v) set(opt ? null : 0);
      else set(isFloat ? floatOr(v, 0) : intOr(v, 0));
    });
  };
  const select = (opts, val, opt, onSet) => {
    input = h('select', {});
    if (opt) input.append(h('option', { value: '' }, spec.none || '(none)'));
    let found = false;
    for (const [v, l] of opts) {
      const o = h('option', { value: v }, l);
      if (String(val) === String(v)) { o.selected = true; found = true; }
      input.append(o);
    }
    if (val != null && val !== '' && !found) {
      input.append(h('option', { value: String(val), selected: true }, `${val} (not defined)`));
    }
    input.addEventListener('change', () => onSet(input.value));
  };
  const discPicker = (which, kind, opt) => {
    const val = cur && cur.id != null ? cur.id : (cur && cur.name) || null;
    const make = (v) => (kind === 'scene' ? { kind, name: v } : { kind, id: intOr(v, 0) });
    if (!catList(ctx, which).length) {
      // No disc: the coordinate by hand.
      input = h('input', { type: kind === 'scene' ? 'text' : 'number', value: val == null ? '' : val, placeholder: kind === 'scene' ? 'town01' : 'id' });
      input.addEventListener('input', () => {
        const v = input.value.trim();
        set(v ? make(v) : (opt ? null : make(kind === 'scene' ? '' : 0)));
      });
    } else {
      select(discOptions(ctx, which), val, opt, (v) => set(v ? make(v) : null));
    }
  };

  switch (t) {
    case 'text': case 'otext': case 'scenename_free':
      input = h('input', { type: 'text', value: cur == null ? '' : cur, placeholder: spec.ph || '' });
      input.addEventListener('input', () => set(input.value === '' && t === 'otext' ? null : input.value));
      break;
    case 'textarea': case 'otextarea':
      input = h('textarea', { rows: spec.rows || 3, placeholder: spec.ph || '' });
      input.value = cur == null ? '' : cur;
      input.addEventListener('input', () => set(input.value === '' && t === 'otextarea' ? null : input.value));
      break;
    case 'int': case 'oint': numInput(cur, t === 'oint', false); break;
    case 'float': case 'ofloat': numInput(cur, t === 'ofloat', true); break;
    case 'bool':
      input = h('input', { type: 'checkbox' });
      input.checked = !!cur;
      input.addEventListener('change', () => set(input.checked));
      wrap.append(input, label);
      return wrap;
    case 'sel': case 'osel':
      select(spec.opts.map((o) => (Array.isArray(o) ? o : [o, o])), cur, t === 'osel', (v) => set(v === '' ? null : (spec.num ? intOr(v, 0) : v)));
      break;
    case 'csv': case 'csvint':
      input = h('input', { type: 'text', value: (cur || []).join(', '), placeholder: spec.ph || '' });
      input.addEventListener('input', () => {
        const parts = csvSplit(input.value);
        set(t === 'csvint' ? parts.map((x) => intOr(x, 0)) : parts);
      });
      break;
    case 'lines':
      input = h('textarea', { rows: spec.rows || 5, spellcheck: 'false' });
      input.value = (cur || []).join('\n');
      input.addEventListener('input', () => set(input.value.split('\n')));
      break;
    case 'tuple': {
      input = h('div', { class: 'xw-row2' });
      const arr = Array.isArray(cur) ? cur.slice() : new Array(spec.n).fill(0);
      for (let i = 0; i < spec.n; i++) {
        const box = h('input', { type: 'number', value: arr[i] == null ? 0 : arr[i], step: spec.float ? 'any' : '1', title: (spec.names || [])[i] || '' });
        box.addEventListener('input', () => {
          arr[i] = spec.float ? floatOr(box.value, 0) : intOr(box.value, 0);
          set(arr.slice());
        });
        input.append(box);
      }
      break;
    }
    case 'monster': case 'omonster': discPicker('monsters', 'monster', t === 'omonster'); break;
    case 'item': case 'oitem': discPicker('items', 'item', t === 'oitem'); break;
    case 'scene': case 'oscene': discPicker('scenes', 'scene', t === 'oscene'); break;
    case 'bgm': case 'obgm': discPicker('bgm', 'bgm', t === 'obgm'); break;
    case 'music': case 'omusic': {
      // A retail cue by its sound-test name, a pack track, or silence
      // (audio.toml; docs/formats/expansion-pack-audio.md).
      const tracks = Object.keys((ctx.pack.audio && ctx.pack.audio.tracks) || {});
      const opts = [['silence', '(silence)']].concat(tracks.map((k) => ['t:' + k, `${k} (pack track)`]));
      const retail = discOptions(ctx, 'bgm');
      for (const [v, l] of retail) opts.push(['b:' + v, l]);
      const val = cur == null ? '' : cur.kind === 'track' ? 't:' + cur.id : cur.kind === 'silence' ? 'silence' : 'b:' + cur.id;
      if (val.startsWith('b:') && !retail.length) opts.push([val, `sound-test ${val.slice(2)}`]);
      select(opts, val, t === 'omusic', (v) => {
        if (!v) set(null);
        else if (v === 'silence') set({ kind: 'silence' });
        else if (v.startsWith('t:')) set({ kind: 'track', id: v.slice(2) });
        else set({ kind: 'bgm', id: intOr(v.slice(2), 0) });
      });
      if (!retail.length) {
        // No disc: a sound-test id by hand.
        const wrapIn = h('div', { class: 'xw-row2' }, input);
        const num = h('input', { type: 'number', min: 0, max: 80, placeholder: 'sound-test id', value: cur && cur.kind === 'bgm' ? cur.id : '' });
        num.addEventListener('input', () => { if (num.value !== '') set({ kind: 'bgm', id: intOr(num.value, 0) }); });
        wrapIn.append(num);
        input = wrapIn;
      }
      break;
    }
    case 'sound': case 'osound': {
      // A retail cue id or a pack sound.
      const sounds = Object.keys((ctx.pack.audio && ctx.pack.audio.sounds) || {});
      input = h('div', { class: 'xw-row2' });
      const kind = h('select', { 'aria-label': 'Sound kind' });
      if (t === 'osound') kind.append(h('option', { value: '' }, '(none)'));
      kind.append(h('option', { value: 'sfx' }, 'retail cue'));
      for (const k of sounds) kind.append(h('option', { value: 's:' + k }, `${k} (pack sound)`));
      kind.value = cur == null ? '' : cur.kind === 'sound' ? 's:' + cur.id : 'sfx';
      const num = h('input', { type: 'text', placeholder: 'cue id, e.g. 0x21', value: cur && cur.kind === 'sfx' ? '0x' + cur.id.toString(16).toUpperCase() : '' });
      num.disabled = !(cur && cur.kind === 'sfx');
      const parse = () => { const v = num.value.trim(); return v ? (v.toLowerCase().startsWith('0x') ? parseInt(v, 16) : parseInt(v, 10)) || 0 : 0x21; };
      kind.addEventListener('change', () => {
        const v = kind.value;
        if (!v) set(null, true);
        else if (v === 'sfx') set({ kind: 'sfx', id: parse() }, true);
        else set({ kind: 'sound', id: v.slice(2) }, true);
      });
      num.addEventListener('input', () => set({ kind: 'sfx', id: parse() }));
      input.append(kind, num);
      break;
    }
    case 'anyitem': case 'anyitemid': {
      // A retail item or a pack item: a pack key is a string, a retail item
      // a coordinate (`anyitem`) or a bare id (`anyitemid`).
      const pack = (ctx.pack.items && ctx.pack.items.items) || {};
      const opts = Object.entries(pack).map(([k, d]) => ['p:' + k, `${d.name || k} (pack)`]);
      for (const [v, l] of discOptions(ctx, 'items')) opts.push(['r:' + v, l]);
      const val = typeof cur === 'string' ? 'p:' + cur : cur == null ? '' : 'r:' + (typeof cur === 'object' ? cur.id : cur);
      if (val.startsWith('r:') && !opts.some(([v]) => v === val)) opts.push([val, `retail item ${val.slice(2)}`]);
      select(opts, val, false, (v) => {
        if (v.startsWith('p:')) set(v.slice(2));
        else if (v.startsWith('r:')) set(t === 'anyitem' ? { kind: 'item', id: intOr(v.slice(2), 0) } : intOr(v.slice(2), 0));
      });
      break;
    }
    case 'dbool':
      // A bool that is `true` when left out.
      input = h('input', { type: 'checkbox' });
      input.checked = cur !== false;
      input.addEventListener('change', () => set(input.checked ? undefined : false));
      wrap.append(input, label);
      return wrap;
    case 'chars': {
      input = h('div', { class: 'xw-inline wb-inline' });
      const have = new Set(cur || []);
      for (const c of ['vahn', 'noa', 'gala']) {
        const cb = h('input', { type: 'checkbox' });
        cb.checked = have.has(c);
        cb.addEventListener('change', () => {
          if (cb.checked) have.add(c); else have.delete(c);
          set(['vahn', 'noa', 'gala'].filter((x) => have.has(x)));
        });
        input.append(h('label', { class: 'xw-check xw-field' }, cb, c));
      }
      break;
    }
    case 'monsterid': case 'itemid': {
      const which = t === 'monsterid' ? 'monsters' : 'items';
      if (!catList(ctx, which).length) numInput(cur, false, false);
      else select(discOptions(ctx, which), cur, false, (v) => set(intOr(v, 0)));
      break;
    }
    case 'scenename':
      if (!catList(ctx, 'scenes').length) {
        input = h('input', { type: 'text', value: cur || '' });
        input.addEventListener('input', () => set(input.value));
      } else select(discOptions(ctx, 'scenes'), cur, false, (v) => set(v));
      break;
    case 'ref': case 'oref':
      select(ctx.ids(spec.kind).map((id) => [id, id]), cur, t === 'oref', (v) => set(v === '' ? (t === 'oref' ? null : '') : v));
      break;
    case 'refs': {
      input = h('div', { class: 'xw-inline wb-inline' });
      const have = new Set(cur || []);
      const ids = ctx.ids(spec.kind);
      if (!ids.length) input.append(h('span', { class: 'xw-hint' }, `no ${spec.kind} defined`));
      for (const id of ids) {
        const cb = h('input', { type: 'checkbox' });
        cb.checked = have.has(id);
        cb.addEventListener('change', () => {
          if (cb.checked) have.add(id); else have.delete(id);
          set(ids.filter((x) => have.has(x)).concat([...have].filter((x) => !ids.includes(x))));
        });
        input.append(h('label', { class: 'xw-check xw-field' }, cb, id));
      }
      break;
    }
    case 'stat': {
      // A monster stat: unset (the base's), a set value, or a multiplier.
      input = h('div', { class: 'xw-stat' });
      const mode = h('select', {}, h('option', { value: '' }, 'base'), h('option', { value: 'set' }, 'set'), h('option', { value: 'mul' }, 'x scale'));
      const num = h('input', { type: 'number', step: 'any' });
      const m = cur == null ? '' : (typeof cur === 'object' ? 'mul' : 'set');
      mode.value = m;
      num.value = cur == null ? '' : (typeof cur === 'object' ? cur.mul : cur);
      num.disabled = !m;
      const out = h('span', { class: 'xw-preview-num' });
      const base = spec.base;
      const show = () => {
        const v = obj[spec.k];
        let res = base;
        if (v != null && typeof v === 'object') res = base == null ? null : Math.round(base * v.mul);
        else if (v != null) res = v;
        out.textContent = base == null ? '' : `retail ${base} -> ${res == null ? '?' : res}`;
      };
      const apply = () => {
        num.disabled = !mode.value;
        if (!mode.value) set(null);
        else if (mode.value === 'mul') set({ mul: floatOr(num.value, 1) });
        else set(intOr(num.value, 0));
        show();
      };
      mode.addEventListener('change', () => {
        if (mode.value === 'mul' && !num.value) num.value = '1.5';
        if (mode.value === 'set' && !num.value) num.value = base == null ? '0' : String(base);
        apply();
      });
      num.addEventListener('input', apply);
      input.append(mode, num);
      wrap.append(label, input, out);
      show();
      return wrap;
    }
    default:
      input = h('span', {}, `?${t}`);
  }
  wrap.append(label, input);
  return wrap;
}

export function grid(ctx, obj, specs, path) {
  const g = h('div', { class: 'xw-grid' });
  for (const s of specs) g.append(field(ctx, obj, s, path));
  return g;
}

// --- Lists -----------------------------------------------------------------------------
// An array of objects, every item shown in its own box.
export function listEd(ctx, parent, owner, key, path, o) {
  if (!Array.isArray(owner[key])) owner[key] = [];
  const arr = owner[key];
  const p = joinPath(path, key);
  const wrap = h('div', { 'data-path': p });
  if (o.title) wrap.append(h('h4', { class: 'xw-sub' }, o.title, o.hint ? h('span', { class: 'xw-hint' }, ` ${o.hint}`) : null));
  arr.forEach((item, i) => {
    const ip = joinPath(p, i);
    const box = h('div', { class: 'xw-box', 'data-path': ip });
    const head = h('div', { class: 'xw-box-head' }, o.label ? o.label(item, i) : `#${i + 1}`, h('span', { class: 'xw-sp' }));
    if (i > 0) head.append(h('button', { type: 'button', class: 'wb-mini', title: 'Move up', onclick: () => { arr.splice(i - 1, 0, arr.splice(i, 1)[0]); ctx.changed(true); } }, '↑'));
    head.append(h('button', { type: 'button', class: 'wb-mini', onclick: () => { arr.splice(i, 1); ctx.changed(true); } }, 'Remove'));
    box.append(head);
    o.render(box, item, ip, i);
    wrap.append(box);
  });
  if (!(o.max && arr.length >= o.max)) {
    wrap.append(h('button', { type: 'button', class: 'wb-mini', onclick: () => { arr.push(o.make()); ctx.changed(true); } }, `+ ${o.add || 'Add'}`));
  }
  parent.append(wrap);
}

// A keyed map (`monsters`, `lines`, ...) or an id-keyed array (`quests`), as a
// list beside the selected item's form. `o.arrayId` marks an array whose items
// carry their id in an `id` field.
export function splitEd(ctx, parent, owner, key, path, o) {
  const isArr = !!o.arrayId;
  if (owner[key] == null) owner[key] = isArr ? [] : {};
  const coll = owner[key];
  const p = joinPath(path, key);
  const keys = isArr ? coll.map((_, i) => i) : Object.keys(coll);
  const idOf = (k) => (isArr ? (coll[k].id || `#${k + 1}`) : k);
  const selKey = `${ctx.file()}:${p}`;
  // A pending jump selects the item the issue is in.
  const jump = ctx.jumpField();
  if (jump) {
    for (const k of keys) {
      const ip = joinPath(p, k);
      if (jump === ip || jump.startsWith(ip + '.') || jump.startsWith(ip + '[')) ctx.sel[selKey] = k;
    }
  }
  let cur = ctx.sel[selKey];
  if (!keys.some((k) => String(k) === String(cur))) cur = keys[0];
  const wrap = h('div', { class: 'xw-split', 'data-path': p });
  const list = h('div', { class: 'xw-list' });
  const errs = ctx.issuePaths();
  for (const k of keys) {
    const ip = joinPath(p, k);
    const bad = errs.some((f) => f === ip || f.startsWith(ip + '.') || f.startsWith(ip + '['));
    list.append(h('button', {
      type: 'button',
      class: 'xw-li' + (String(k) === String(cur) ? ' is-active' : '') + (bad ? ' has-err' : ''),
      onclick: () => { ctx.sel[selKey] = k; ctx.rerender(); },
    }, o.label ? o.label(coll[k], idOf(k)) : idOf(k)));
  }
  const add = h('button', {
    type: 'button',
    class: 'wb-mini',
    onclick: () => {
      const id = freshId(o.prefix || key.replace(/s$/, ''), isArr ? coll.map((x) => x.id) : Object.keys(coll));
      if (isArr) { coll.push(Object.assign(o.make(), { id })); ctx.sel[selKey] = coll.length - 1; } else { coll[id] = o.make(); ctx.sel[selKey] = id; }
      ctx.changed(true);
    },
  }, `+ ${o.add || 'Add'}`);
  const left = h('div', {}, list, add);
  const right = h('div', {});
  if (cur == null) {
    right.append(h('div', { class: 'wb-empty' }, o.empty || 'Nothing here yet.'));
  } else {
    const ip = joinPath(p, cur);
    const item = coll[cur];
    const box = h('div', { 'data-path': ip });
    // Id / key editor.
    const idIn = h('input', { type: 'text', value: idOf(cur), spellcheck: 'false' });
    idIn.addEventListener('change', () => {
      const nv = idIn.value.trim();
      if (!nv || nv === idOf(cur)) return;
      if (isArr) { item.id = nv; } else {
        if (coll[nv]) { idIn.value = cur; return; }
        // Keep the key order: rebuild the map with the renamed key in place.
        const entries = Object.entries(coll);
        for (const k2 of Object.keys(coll)) delete coll[k2];
        for (const [k2, v2] of entries) coll[k2 === cur ? nv : k2] = v2;
        ctx.sel[selKey] = nv;
        if (o.onRename) o.onRename(cur, nv);
      }
      ctx.changed(true);
    });
    const del = h('button', {
      type: 'button',
      class: 'wb-mini',
      onclick: () => { if (isArr) coll.splice(cur, 1); else delete coll[cur]; ctx.changed(true); },
    }, 'Delete');
    box.append(h('div', { class: 'xw-box-head' }, h('label', { class: 'xw-field', 'data-path': isArr ? joinPath(ip, 'id') : ip }, h('span', {}, o.idLabel || 'Id'), idIn), h('span', { class: 'xw-sp' }), del));
    o.render(box, item, ip, cur);
    right.append(box);
  }
  wrap.append(left, right);
  parent.append(wrap);
}

// The quest prerequisite shape: a quest's start test, and the `requires`
// gate of a map's entrances, exits, triggers and NPCs.
export const GATE_FIELDS = [
  { k: 'quests', t: 'refs', kind: 'quest', label: 'Quests done', wide: true },
  { k: 'min_level', t: 'oint', label: 'Party level at least' },
  { k: 'flags', t: 'csv', label: 'Pack flags set' },
  { k: 'flags_clear', t: 'csv', label: 'Pack flags clear' },
  { k: 'bosses', t: 'refs', kind: 'boss', label: 'Once-only bosses beaten', wide: true },
  { k: 'bosses_unbeaten', t: 'refs', kind: 'boss', label: 'Once-only bosses not beaten', wide: true },
  { k: 'story_flags', t: 'csvint', label: 'Retail story flags set' },
  { k: 'story_flags_clear', t: 'csvint', label: 'Retail story flags clear' },
];

// A boxed gate editor over `owner[key]`, created on first use.
export function gateBox(ctx, owner, key, path, title) {
  const gate = owner[key] || (owner[key] = {});
  const gp = joinPath(path, key);
  return h('div', { class: 'xw-box', 'data-path': gp }, h('div', { class: 'xw-box-head' }, title), grid(ctx, gate, GATE_FIELDS, gp));
}

export function freshId(prefix, taken) {
  const base = prefix.replace(/[^a-z0-9_]/g, '_') || 'item';
  for (let n = 1; ; n++) {
    const id = `${base}_${n}`;
    if (!taken.includes(id)) return id;
  }
}

// --- Section editors -----------------------------------------------------------------------
export function renderManifest(ctx, el) {
  const m = ctx.pack.manifest;
  el.append(h('p', { class: 'xw-note' }, 'pack.toml: who made the pack and what it needs. The id namespaces every content id in the pack.'));
  el.append(grid(ctx, m, [
    { k: 'id', label: 'Pack id', hint: '[a-z0-9_-]' },
    { k: 'name', label: 'Name' },
    { k: 'version', label: 'Version', hint: 'semver' },
    { k: 'license', label: 'License' },
    { k: 'homepage', t: 'otext', label: 'Homepage', ph: 'https://' },
    { k: 'authors', t: 'csv', label: 'Authors', hint: 'comma-separated', wide: true },
    { k: 'description', t: 'textarea', label: 'Description', wide: true, rows: 4 },
    { k: 'tags', t: 'csv', label: 'Tags' },
    { k: 'requires', t: 'csv', label: 'Requires', hint: 'pack ids' },
    { k: 'conflicts', t: 'csv', label: 'Conflicts', hint: 'pack ids' },
  ]));
  el.append(h('p', { class: 'xw-note' }, `Schema ${m.schema}, disc ${m.disc}.`));
}

function itemChanceEd(ctx, box, owner, key, path, label) {
  const p = joinPath(path, key);
  const row = h('div', { class: 'xw-box', 'data-path': p });
  const on = h('input', { type: 'checkbox' });
  on.checked = owner[key] != null;
  on.addEventListener('change', () => {
    owner[key] = on.checked ? { item: { kind: 'item', id: (catList(ctx, 'items')[0] || { id: 1 }).id }, chance_pct: 10 } : null;
    ctx.changed(true);
  });
  row.append(h('div', { class: 'xw-box-head' }, h('label', { class: 'xw-check xw-field' }, on, label)));
  if (owner[key]) row.append(grid(ctx, owner[key], [{ k: 'item', t: 'anyitem', label: 'Item' }, { k: 'chance_pct', t: 'int', label: 'Chance %' }], p));
  box.append(row);
}

function monsterRefSlot(ctx, f, i, path) {
  // A formation slot: a pack monster id (string) or a retail one (DiscRef).
  const cur = f.monsters[i];
  const sel = h('select', {});
  const opts = [['', '(empty seat)']];
  for (const id of ctx.ids('monster')) opts.push([`p:${id}`, `${id} (pack)`]);
  for (const m of catList(ctx, 'monsters')) opts.push([`d:${m.id}`, `#${m.id} ${m.name} (retail)`]);
  const val = cur == null ? '' : (typeof cur === 'string' ? `p:${cur}` : `d:${cur.id}`);
  let found = false;
  for (const [v, l] of opts) {
    const o = h('option', { value: v }, l);
    if (v === val) { o.selected = true; found = true; }
    sel.append(o);
  }
  if (!found && val) sel.append(h('option', { value: val, selected: true }, `${val.slice(2)} (not defined)`));
  sel.addEventListener('change', () => {
    const v = sel.value;
    if (!v) f.monsters.splice(i, 1);
    else f.monsters[i] = v.startsWith('p:') ? v.slice(2) : { kind: 'monster', id: intOr(v.slice(2), 1) };
    ctx.changed(!v);
  });
  return h('label', { class: 'xw-field', 'data-path': `${path}.monsters[${i}]` }, h('span', {}, `Seat ${i + 1}`), sel);
}

// --- Boss scripts ---------------------------------------------------------------------------
// `[monsters.<key>.script]`: the phase editor. Moves pick among the base
// monster's own swings (`attack = n`, its attack list) and spell-table ids;
// docs/formats/expansion-pack-battle.md#boss-scripts.
const TARGET_RULES = [['random', 'Random'], ['lowest_hp', 'Lowest HP'], ['highest_attack', 'Highest attack'], ['healer', 'Healer first']];
const STATUS_NAMES = ['venom', 'toxic', 'numb', 'sleep', 'confuse', 'rot', 'curse', 'stone'];
const PHASE_STATS = ['attack', 'defence', 'agility', 'speed', 'intelligence'];

function spellName(ctx, id) {
  const s = catList(ctx, 'spells').find((x) => x.id === id);
  return s ? s.name : null;
}

// One move: a swing of the base's or a spell, and how it is picked.
function moveEd(ctx, box, mv, p, base) {
  const kind = h('select', {}, h('option', { value: 'attack' }, 'Swing (base attack)'), h('option', { value: 'spell' }, 'Spell'));
  kind.value = mv.spell != null ? 'spell' : 'attack';
  kind.addEventListener('change', () => {
    if (kind.value === 'spell') { delete mv.attack; delete mv.hits; mv.spell = mv.spell || 0x81; } else { delete mv.spell; mv.attack = 0; }
    ctx.changed(true);
  });
  const row = h('div', { class: 'xw-grid' }, h('label', { class: 'xw-field' }, h('span', {}, 'Move'), kind));
  if (kind.value === 'attack') {
    const swings = (base && base.swings) || [];
    if (swings.length) {
      row.append(field(ctx, mv, { k: 'attack', t: 'sel', num: true, label: 'Swing', opts: swings.map((c, i) => [String(i), `swing ${i} (AGL ${c})`]) }, p));
    } else row.append(field(ctx, mv, { k: 'attack', t: 'int', label: 'Swing', hint: 'index into the base attack list' }, p));
    row.append(field(ctx, mv, { k: 'hits', t: 'oint', label: 'Hits', hint: '1..8', ph: '1' }, p));
  } else if (catList(ctx, 'spells').length) {
    row.append(field(ctx, mv, { k: 'spell', t: 'sel', num: true, label: 'Spell', opts: catList(ctx, 'spells').filter((x) => x.id > 0).map((x) => [String(x.id), `0x${x.id.toString(16).toUpperCase().padStart(2, '0')} ${x.name}`]) }, p));
  } else row.append(field(ctx, mv, { k: 'spell', t: 'int', label: 'Spell id' }, p));
  row.append(
    field(ctx, mv, { k: 'weight', t: 'int', label: 'Weight' }, p),
    field(ctx, mv, { k: 'target', t: 'osel', label: 'Target', none: '(phase rule)', opts: TARGET_RULES }, p),
    field(ctx, mv, { k: 'cooldown', t: 'int', label: 'Cooldown', hint: 'own turns' }, p),
    field(ctx, mv, { k: 'charge', t: 'bool', label: 'Telegraphed (charges a turn first)' }, p),
  );
  if (mv.charge) {
    row.append(field(ctx, mv, { k: 'charge_line', t: 'oref', kind: 'line', label: 'Charge banner', none: '(gathering power)' }, p));
    row.append(field(ctx, mv, { k: 'charge_sound', t: 'osound', label: 'Charge sound', hint: 'the telegraph\'s cue' }, p));
  }
  box.append(row);
}

// Summons: pack monsters or retail ones, into free monster seats.
function summonEd(ctx, box, ph, p) {
  if (!Array.isArray(ph.summon)) ph.summon = [];
  const seats = h('div', { class: 'xw-grid', 'data-path': `${p}.summon` });
  const n = Math.min(4, ph.summon.length + 1);
  for (let i = 0; i < n; i++) {
    const cur = ph.summon[i];
    const sel = h('select', {});
    const opts = [['', i < ph.summon.length ? '(remove)' : '(none)']];
    for (const id of ctx.ids('monster')) opts.push([`p:${id}`, `${id} (pack)`]);
    for (const m of catList(ctx, 'monsters')) opts.push([`d:${m.id}`, `#${m.id} ${m.name} (retail)`]);
    const val = cur == null ? '' : (typeof cur === 'string' ? `p:${cur}` : `d:${cur.id}`);
    for (const [v, l] of opts) sel.append(h('option', { value: v, selected: v === val }, l));
    sel.addEventListener('change', () => {
      const v = sel.value;
      if (!v) ph.summon.splice(i, 1);
      else ph.summon[i] = v.startsWith('p:') ? v.slice(2) : { kind: 'monster', id: intOr(v.slice(2), 1) };
      ctx.changed(true);
    });
    seats.append(h('label', { class: 'xw-field', 'data-path': `${p}.summon[${i}]` }, h('span', {}, `Summon ${i + 1}`), sel));
  }
  box.append(h('h4', { class: 'xw-sub' }, 'Summons', h('span', { class: 'xw-hint' }, ' seated as the next round opens')), seats);
}

// One phase (or the enrage): when it begins, what changes, its moves.
function phaseEd(ctx, box, ph, p, base, opts) {
  const when = ph.when || (ph.when = {});
  const wp = `${p}.when`;
  if (opts.enrage) box.append(grid(ctx, when, [{ k: 'round', t: 'oint', label: 'Enrage at round', hint: 'the first round is 1' }], wp));
  else if (!opts.first) {
    box.append(h('p', { class: 'xw-hint' }, 'Begins when any condition set holds.'));
    box.append(grid(ctx, when, [
      { k: 'hp_below_pct', t: 'oint', label: 'HP below %' },
      { k: 'round', t: 'oint', label: 'At round' },
      { k: 'allies_down', t: 'oint', label: 'Allies down' },
    ], wp));
  }
  box.append(grid(ctx, ph, [
    { k: 'line', t: 'oref', kind: 'line', label: 'Banner line', none: '(none)' },
    { k: 'target', t: 'osel', label: 'Targeting', none: '(script rule)', opts: TARGET_RULES },
    { k: 'element', t: 'osel', label: 'Element', none: '(keep)', opts: ELEMENTS.map((n, i) => [String(i), n]), num: true },
    { k: 'heal_pct', t: 'oint', label: 'Heal % of max HP' },
    { k: 'music', t: 'omusic', label: 'Music from here', hint: 'cross-fades as the phase begins' },
    { k: 'sound', t: 'osound', label: 'Sound', hint: 'played as it begins' },
  ], p));
  if (!opts.first) {
    const g = h('div', { class: 'xw-grid' });
    for (const st of PHASE_STATS) g.append(field(ctx, ph, { k: st, t: 'stat', label: st }, p));
    box.append(h('h4', { class: 'xw-sub' }, 'Stat changes', h('span', { class: 'xw-hint' }, ' on the live stat as the phase begins')), g);
    summonEd(ctx, box, ph, p);
  }
  listEd(ctx, box, ph, 'moves', p, {
    title: 'Moves',
    hint: opts.first ? 'picked by weight each turn' : 'empty keeps the moves before',
    add: 'Move',
    max: 12,
    make: () => ({ attack: 0, weight: 1, charge: false, cooldown: 0 }),
    label: (mv) => (mv.spell != null ? `spell ${spellName(ctx, mv.spell) || mv.spell}` : `swing ${mv.attack}${mv.hits > 1 ? ' x' + mv.hits : ''}`) + (mv.charge ? ' (telegraphed)' : ''),
    render: (b2, mv, p2) => moveEd(ctx, b2, mv, p2, base),
  });
}

function bossScriptEd(ctx, box, m, p, base) {
  const sp = `${p}.script`;
  const wrap = h('div', { class: 'xw-box', 'data-path': sp });
  const on = h('input', { type: 'checkbox' });
  on.checked = m.script != null;
  on.addEventListener('change', () => {
    m.script = on.checked ? { target: 'random', phases: [{ when: {}, moves: [{ attack: 0, weight: 1, charge: false, cooldown: 0 }], summon: [] }], immune: [] } : null;
    ctx.changed(true);
  });
  wrap.append(h('div', { class: 'xw-box-head' }, h('label', { class: 'xw-check xw-field' }, on, 'Boss script'), h('span', { class: 'xw-hint' }, ' its own AI: phases, telegraphs, summons')));
  const sc = m.script;
  if (!sc) { box.append(wrap); return; }
  wrap.append(grid(ctx, sc, [{ k: 'target', t: 'sel', label: 'Targeting', opts: TARGET_RULES }], sp));
  // Immunities.
  const have = new Set(sc.immune || []);
  const imm = h('div', { class: 'xw-inline wb-inline', 'data-path': `${sp}.immune` });
  for (const st of STATUS_NAMES) {
    const cb = h('input', { type: 'checkbox' });
    cb.checked = have.has(st);
    cb.addEventListener('change', () => {
      if (cb.checked) have.add(st); else have.delete(st);
      sc.immune = STATUS_NAMES.filter((x) => have.has(x));
      ctx.changed();
    });
    imm.append(h('label', { class: 'xw-check xw-field' }, cb, st));
  }
  wrap.append(h('h4', { class: 'xw-sub' }, 'Immune to'), imm);
  // Last stand.
  const ls = h('input', { type: 'checkbox' });
  ls.checked = sc.last_stand != null;
  ls.addEventListener('change', () => { sc.last_stand = ls.checked ? {} : null; ctx.changed(true); });
  wrap.append(h('label', { class: 'xw-check xw-field' }, ls, 'Last stand (survives one lethal blow at 1 HP)'));
  if (sc.last_stand) wrap.append(grid(ctx, sc.last_stand, [{ k: 'line', t: 'oref', kind: 'line', label: 'Last-stand banner', none: '(default)' }], `${sp}.last_stand`));
  listEd(ctx, wrap, sc, 'phases', sp, {
    title: 'Phases',
    hint: 'in order; the first opens the fight',
    add: 'Phase',
    max: 8,
    make: () => ({ when: { hp_below_pct: 50 }, moves: [], summon: [] }),
    label: (ph, i) => {
      if (i === 0) return 'Phase 1 (opening)';
      const w = ph.when || {};
      const c = [w.hp_below_pct != null ? `HP < ${w.hp_below_pct}%` : null, w.round != null ? `round ${w.round}` : null, w.allies_down != null ? `${w.allies_down} allies down` : null].filter(Boolean);
      return `Phase ${i + 1}${c.length ? ' - ' + c.join(' or ') : ''}`;
    },
    render: (b2, ph, p2, i) => phaseEd(ctx, b2, ph, p2, base, { first: i === 0 }),
  });
  // The enrage timer.
  const en = h('input', { type: 'checkbox' });
  en.checked = sc.enrage != null;
  en.addEventListener('change', () => { sc.enrage = en.checked ? { when: { round: 12 }, moves: [], summon: [] } : null; ctx.changed(true); });
  wrap.append(h('label', { class: 'xw-check xw-field' }, en, 'Enrage timer'));
  if (sc.enrage) {
    const eb = h('div', { class: 'xw-box', 'data-path': `${sp}.enrage` });
    phaseEd(ctx, eb, sc.enrage, `${sp}.enrage`, base, { enrage: true });
    wrap.append(eb);
  }
  box.append(wrap);
}

export function renderBattle(ctx, el) {
  const b = ctx.pack.battle;
  el.append(h('p', { class: 'xw-note' }, 'battle.toml: monsters derived from a retail record, formations of up to four, per-scene encounter overrides and bosses. A derived monster keeps its base\'s mesh, animation and attacks; only the numbers change.'));
  el.append(h('h4', { class: 'xw-sub' }, 'Monsters'));
  splitEd(ctx, el, b, 'monsters', '', {
    prefix: 'monster',
    add: 'Monster',
    make: () => ({ base: { kind: 'monster', id: (catList(ctx, 'monsters')[0] || { id: 1 }).id } }),
    label: (m, id) => `${id}${m.name ? ' - ' + m.name : ''}`,
    render: (box, m, p) => {
      const base = m.base && retailMonster(ctx, m.base.id);
      box.append(grid(ctx, m, [
        { k: 'base', t: 'monster', label: 'Base (retail record)' },
        { k: 'name', t: 'otext', label: 'Name', ph: base ? `(retail: ${base.name})` : '' },
        { k: 'element', t: 'osel', label: 'Element', none: base ? `(base: ${ELEMENTS[base.element] || base.element})` : '(base)', opts: ELEMENTS.map((n, i) => [String(i), n]), num: true },
        { k: 'spells', t: 'csvint', label: 'Spells', hint: 'ids; empty keeps the base' },
      ], p));
      const g = h('div', { class: 'xw-grid' });
      for (const s of STATS) g.append(field(ctx, m, { k: s, t: 'stat', label: s, base: base ? base[s] : null }, p));
      box.append(h('h4', { class: 'xw-sub' }, 'Stats'), g);
      itemChanceEd(ctx, box, m, 'drop', p, 'Drop');
      itemChanceEd(ctx, box, m, 'steal', p, 'Steal');
      bossScriptEd(ctx, box, m, p, base);
    },
  });

  el.append(h('h4', { class: 'xw-sub' }, 'Formations', h('span', { class: 'xw-hint' }, ' up to four monsters')));
  splitEd(ctx, el, b, 'formations', '', {
    prefix: 'formation',
    add: 'Formation',
    make: () => ({ label: '', monsters: [] }),
    label: (f, id) => `${id} (${(f.monsters || []).length})`,
    render: (box, f, p) => {
      box.append(grid(ctx, f, [{ k: 'label', label: 'Label' }, { k: 'scripted', t: 'bool', label: 'Scripted (fixed stats, no random boost)' }], p));
      const seats = h('div', { class: 'xw-grid', 'data-path': `${p}.monsters` });
      const n = Math.min(4, f.monsters.length + 1);
      for (let i = 0; i < n; i++) seats.append(monsterRefSlot(ctx, f, i, p));
      box.append(seats);
    },
  });

  listEd(ctx, el, b, 'encounters', '', {
    title: 'Encounter overrides',
    hint: 'per retail scene',
    add: 'Encounter override',
    make: () => ({ scene: (catList(ctx, 'scenes')[0] || { name: 'town01' }).name, formations: [], retail_weight: 0 }),
    label: (e) => e.scene,
    render: (box, e, p) => {
      box.append(grid(ctx, e, [
        { k: 'scene', t: 'scenename', label: 'Scene' },
        { k: 'retail_weight', t: 'int', label: 'Retail weight', hint: '0 replaces the retail table' },
        { k: 'steps', t: 'tuple', n: 2, label: 'Steps between fights', hint: '[min, max]' },
      ], p));
      listEd(ctx, box, e, 'formations', p, {
        add: 'Formation',
        make: () => ({ formation: ctx.ids('formation')[0] || '', weight: 1 }),
        label: (w) => w.formation,
        render: (b2, w, p2) => b2.append(grid(ctx, w, [{ k: 'formation', t: 'ref', kind: 'formation', label: 'Formation' }, { k: 'weight', t: 'int', label: 'Weight' }], p2)),
      });
    },
  });

  el.append(h('h4', { class: 'xw-sub' }, 'Bosses'));
  splitEd(ctx, el, b, 'bosses', '', {
    prefix: 'boss',
    add: 'Boss',
    make: () => ({ formation: ctx.ids('formation')[0] || '', no_escape: true, reward_items: [], reward_gold: 0, once: true }),
    render: (box, bo, p) => {
      box.append(grid(ctx, bo, [
        { k: 'formation', t: 'ref', kind: 'formation', label: 'Formation' },
        { k: 'music', t: 'omusic', label: 'Music', hint: 'retail or a pack track; audio.toml music.boss when unset' },
        { k: 'reward_gold', t: 'int', label: 'Reward gold' },
        { k: 'no_escape', t: 'bool', label: 'No escape' },
        { k: 'once', t: 'bool', label: 'Once only (recorded in the save)' },
      ], p));
      listEd(ctx, box, bo, 'reward_items', p, {
        title: 'Reward items',
        add: 'Item',
        make: () => ({ item: { kind: 'item', id: (catList(ctx, 'items')[0] || { id: 1 }).id }, count: 1 }),
        render: (b2, it, p2) => b2.append(grid(ctx, it, [{ k: 'item', t: 'anyitem', label: 'Item' }, { k: 'count', t: 'int', label: 'Count' }], p2)),
      });
    },
  });
}

// --- Progression -------------------------------------------------------------------------------
export function renderProgression(ctx, el) {
  const pr = ctx.pack.progression;
  el.append(h('p', { class: 'xw-note' }, 'progression.toml: the XP curve, EXP and gold multipliers, enemy level scaling and level-up rewards.'));
  el.append(grid(ctx, pr, [
    { k: 'xp_mul', t: 'ofloat', label: 'EXP multiplier', ph: '1.0' },
    { k: 'gold_mul', t: 'ofloat', label: 'Gold multiplier', ph: '1.0' },
  ], ''));
  const curve = pr.xp_curve || (pr.xp_curve = { kind: 'retail' });
  const cbox = h('div', { class: 'xw-box', 'data-path': 'xp_curve' });
  const kind = h('select', {}, ...['retail', 'scaled', 'banded', 'explicit'].map((k) => h('option', { value: k, selected: curve.kind === k }, k)));
  kind.addEventListener('change', () => {
    const k = kind.value;
    pr.xp_curve = k === 'scaled' ? { kind: k, mul: 0.5 } : k === 'banded' ? { kind: k, bands: [{ from_level: 60, mul: 0.5 }] } : k === 'explicit' ? { kind: k, levels: (ctx.chart && ctx.chart.retail) || [] } : { kind: 'retail' };
    ctx.changed(true);
  });
  cbox.append(h('div', { class: 'xw-box-head' }, h('label', { class: 'xw-field' }, h('span', {}, 'XP curve'), kind)));
  if (curve.kind === 'scaled') cbox.append(grid(ctx, curve, [{ k: 'mul', t: 'float', label: 'Every step x' }], 'xp_curve'));
  if (curve.kind === 'banded') {
    listEd(ctx, cbox, curve, 'bands', 'xp_curve', {
      add: 'Band',
      make: () => ({ from_level: 50, mul: 0.5 }),
      label: (b) => `from level ${b.from_level}`,
      render: (b2, band, p2) => b2.append(grid(ctx, band, [{ k: 'from_level', t: 'int', label: 'From level' }, { k: 'mul', t: 'float', label: 'Step x' }], p2)),
    });
  }
  if (curve.kind === 'explicit') cbox.append(grid(ctx, curve, [{ k: 'levels', t: 'csvint', label: 'Cumulative XP for levels 2..99', wide: true }], 'xp_curve'));
  const chartBox = h('div', { id: 'xw-xp-chart' });
  cbox.append(chartBox);
  el.append(cbox);

  const sbox = h('div', { class: 'xw-box', 'data-path': 'enemy_scaling' });
  const on = h('input', { type: 'checkbox' });
  on.checked = pr.enemy_scaling != null;
  on.addEventListener('change', () => {
    pr.enemy_scaling = on.checked ? { from_level: 50, per_level: 0.02, max_mul: 1.5, scope: 'pack', rewards: false } : null;
    ctx.changed(true);
  });
  sbox.append(h('div', { class: 'xw-box-head' }, h('label', { class: 'xw-check xw-field' }, on, 'Enemy level scaling')));
  if (pr.enemy_scaling) {
    sbox.append(grid(ctx, pr.enemy_scaling, [
      { k: 'from_level', t: 'int', label: 'From party level' },
      { k: 'per_level', t: 'float', label: 'Per level', hint: 'x per level past it' },
      { k: 'max_mul', t: 'float', label: 'At most x' },
      { k: 'scope', t: 'sel', opts: [['pack', 'pack fights'], ['all', 'every fight']], label: 'Applies to' },
      { k: 'rewards', t: 'bool', label: 'Scale EXP and gold too' },
    ], 'enemy_scaling'));
  }
  el.append(sbox);

  listEd(ctx, el, pr, 'level_rewards', '', {
    title: 'Level-up rewards',
    add: 'Reward',
    make: () => ({ level: 50, items: [] }),
    label: (r) => `level ${r.level}`,
    render: (box, r, p) => {
      box.append(grid(ctx, r, [{ k: 'level', t: 'int', label: 'Level' }, { k: 'character', t: 'oint', label: 'Character slot', ph: 'any' }], p));
      listEd(ctx, box, r, 'items', p, {
        add: 'Item',
        make: () => ({ item: { kind: 'item', id: (catList(ctx, 'items')[0] || { id: 1 }).id }, count: 1 }),
        render: (b2, it, p2) => b2.append(grid(ctx, it, [{ k: 'item', t: 'anyitem', label: 'Item' }, { k: 'count', t: 'int', label: 'Count' }], p2)),
      });
    },
  });
}

// Cumulative XP to each level, retail beside the pack's, as an inline SVG
// line chart with a crosshair readout. `chart` is the session's `xp_chart`.
export function drawXpChart(host, chart) {
  if (!host) return;
  host.innerHTML = '';
  if (!chart) return;
  const W = 640, H = 240, L = 58, R = 14, T = 12, B = 30;
  const levels = chart.retail.length + 1; // levels 2..99
  const max = Math.max(1, ...chart.retail, ...chart.pack);
  const x = (i) => L + (i / (levels - 2)) * (W - L - R);
  const y = (v) => T + (1 - v / max) * (H - T - B);
  const line = (arr) => arr.map((v, i) => `${i ? 'L' : 'M'}${x(i).toFixed(1)},${y(v).toFixed(1)}`).join('');
  const fmt = (v) => (v >= 1e6 ? (v / 1e6).toFixed(1) + 'M' : v >= 1e3 ? Math.round(v / 1e3) + 'k' : String(v));
  let grid = '';
  for (let k = 0; k <= 4; k++) {
    const v = (max * k) / 4;
    grid += `<line x1="${L}" x2="${W - R}" y1="${y(v)}" y2="${y(v)}" stroke="var(--line-soft)" stroke-width="1"/>`;
    grid += `<text x="${L - 6}" y="${y(v) + 4}" text-anchor="end">${fmt(Math.round(v))}</text>`;
  }
  for (const lv of [2, 20, 40, 60, 80, 99]) grid += `<text x="${x(lv - 2)}" y="${H - 10}" text-anchor="middle">${lv}</text>`;
  const last = levels - 2;
  const svg = `<svg class="xw-chart" viewBox="0 0 ${W} ${H}" role="img" aria-label="Cumulative XP to each level, retail and this pack">
    ${grid}
    <path d="${line(chart.retail)}" fill="none" stroke="var(--text-dim)" stroke-width="2" stroke-dasharray="5 4"/>
    <path d="${line(chart.pack)}" fill="none" stroke="var(--accent)" stroke-width="2"/>
    <text x="${x(last) - 8}" y="${Math.max(T + 10, y(chart.retail[last]) + 4)}" text-anchor="end">retail ${fmt(chart.retail[last])}</text>
    <text x="${x(last) - 8}" y="${Math.min(H - B - 4, y(chart.pack[last]) + 16)}" text-anchor="end">pack ${fmt(chart.pack[last])}</text>
    <line class="xw-xh" x1="0" x2="0" y1="${T}" y2="${H - B}" stroke="var(--border-hi)" stroke-width="1" visibility="hidden"/>
    <rect x="${L}" y="${T}" width="${W - L - R}" height="${H - T - B}" fill="transparent" class="xw-hit"/>
  </svg>`;
  host.innerHTML = `<div class="xw-legend"><span><i style="background:var(--text-dim)"></i>retail</span><span><i style="background:var(--accent)"></i>this pack</span><span class="xw-readout"></span></div>${svg}`;
  const el = host.querySelector('svg');
  const xh = el.querySelector('.xw-xh');
  const read = host.querySelector('.xw-readout');
  const hit = el.querySelector('.xw-hit');
  hit.addEventListener('mousemove', (ev) => {
    const r = el.getBoundingClientRect();
    const px = ((ev.clientX - r.left) / r.width) * W;
    const i = Math.max(0, Math.min(last, Math.round(((px - L) / (W - L - R)) * (levels - 2))));
    xh.setAttribute('x1', x(i));
    xh.setAttribute('x2', x(i));
    xh.setAttribute('visibility', 'visible');
    read.textContent = `level ${i + 2}: retail ${chart.retail[i].toLocaleString()} XP, pack ${chart.pack[i].toLocaleString()} XP`;
  });
  hit.addEventListener('mouseleave', () => { xh.setAttribute('visibility', 'hidden'); read.textContent = ''; });
  if (chart.problems && chart.problems.length) host.append(h('div', { class: 'xw-note wb-bad' }, chart.problems.join('; ')));
}

// --- Quests -----------------------------------------------------------------------------------
const GOALS = {
  defeat_formation: [{ k: 'formation', t: 'ref', kind: 'formation', label: 'Formation' }, { k: 'count', t: 'int', label: 'Count' }],
  defeat_monster: [{ k: 'monster', t: 'monsterid', label: 'Retail monster' }, { k: 'count', t: 'int', label: 'Count' }],
  win_battles: [{ k: 'count', t: 'int', label: 'Count' }],
  win_boss: [{ k: 'boss', t: 'oref', kind: 'boss', label: 'Boss' }],
  enter_scene: [{ k: 'scene', t: 'scenename', label: 'Scene' }],
  reach_level: [{ k: 'level', t: 'int', label: 'Level' }],
  collect_item: [{ k: 'item', t: 'anyitemid', label: 'Item' }, { k: 'count', t: 'int', label: 'Count' }, { k: 'hand_in', t: 'bool', label: 'Hand in' }],
  talk: [{ k: 'npc', label: 'NPC id' }],
  win_mode: [{ k: 'mode', t: 'ref', kind: 'mode', label: 'Mode' }, { k: 'count', t: 'int', label: 'Count' }],
  counter: [{ k: 'counter', label: 'Counter' }, { k: 'at_least', t: 'int', label: 'At least' }],
  flag: [{ k: 'flag', label: 'Pack flag' }],
};
const GOAL_DEFAULTS = {
  defeat_formation: (ctx) => ({ formation: ctx.ids('formation')[0] || '', count: 1 }),
  defeat_monster: () => ({ monster: 1, count: 1 }),
  win_battles: () => ({ count: 1 }),
  win_boss: (ctx) => ({ boss: ctx.ids('boss')[0] || null }),
  enter_scene: () => ({ scene: 'town01' }),
  reach_level: () => ({ level: 10 }),
  collect_item: () => ({ item: 1, count: 1, hand_in: false }),
  talk: () => ({ npc: '' }),
  win_mode: (ctx) => ({ mode: ctx.ids('mode')[0] || '', count: 1 }),
  counter: () => ({ counter: '', at_least: 1 }),
  flag: () => ({ flag: '' }),
};

function rewardsEd(ctx, box, owner, path) {
  const r = owner.rewards || (owner.rewards = {});
  const p = joinPath(path, 'rewards');
  const rb = h('div', { class: 'xw-box', 'data-path': p }, h('div', { class: 'xw-box-head' }, 'Rewards'));
  rb.append(grid(ctx, r, [
    { k: 'gold', t: 'int', label: 'Gold' },
    { k: 'xp', t: 'int', label: 'XP' },
    { k: 'set_flags', t: 'csv', label: 'Set pack flags' },
    { k: 'cards', t: 'refs', kind: 'card', label: 'Cards', wide: true },
  ], p));
  listEd(ctx, rb, r, 'items', p, {
    add: 'Item',
    make: () => ({ item: (catList(ctx, 'items')[0] || { id: 1 }).id, count: 1 }),
    render: (b2, it, p2) => b2.append(grid(ctx, it, [{ k: 'item', t: 'anyitemid', label: 'Item' }, { k: 'count', t: 'int', label: 'Count' }], p2)),
  });
  listEd(ctx, rb, r, 'counters', p, {
    add: 'Counter',
    make: () => ({ counter: 'counter', add: 1 }),
    render: (b2, c, p2) => b2.append(grid(ctx, c, [{ k: 'counter', label: 'Counter' }, { k: 'add', t: 'int', label: 'Add' }], p2)),
  });
  box.append(rb);
}

export function renderQuests(ctx, el) {
  const q = ctx.pack.quests;
  el.append(h('p', { class: 'xw-note' }, 'quests.toml: quests (prerequisites, objectives, rewards) and challenges (rule sets over one battle). Pickers offer only ids the pack defines.'));
  el.append(h('h4', { class: 'xw-sub' }, 'Quests'));
  splitEd(ctx, el, q, 'quests', '', {
    arrayId: true,
    prefix: 'quest',
    add: 'Quest',
    make: () => ({ title: 'New quest', description: '', ordered: true, objectives: [], prerequisites: {}, rewards: {}, repeat: {} }),
    label: (it, id) => `${id}${it.title ? ' - ' + it.title : ''}`,
    render: (box, it, p) => {
      box.append(grid(ctx, it, [
        { k: 'title', label: 'Title' },
        { k: 'giver', t: 'otext', label: 'Giver (NPC id)' },
        { k: 'description', t: 'textarea', label: 'Description', wide: true },
        { k: 'auto_start', t: 'bool', label: 'Starts by itself' },
        { k: 'ordered', t: 'bool', label: 'Objectives in order' },
        { k: 'start_dialog', t: 'oref', kind: 'line', label: 'Start line' },
        { k: 'active_dialog', t: 'oref', kind: 'line', label: 'While active' },
        { k: 'complete_dialog', t: 'oref', kind: 'line', label: 'Complete line' },
        { k: 'start_battle', t: 'oref', kind: 'formation', label: 'Start battle' },
        { k: 'start_boss', t: 'oref', kind: 'boss', label: 'Start boss' },
        { k: 'start_cutscene', t: 'oref', kind: 'cutscene', label: 'Start cutscene' },
        { k: 'complete_cutscene', t: 'oref', kind: 'cutscene', label: 'Complete cutscene' },
      ], p));
      box.append(gateBox(ctx, it, 'prerequisites', p, 'Prerequisites'));
      listEd(ctx, box, it, 'objectives', p, {
        title: 'Objectives',
        add: 'Objective',
        make: () => Object.assign({ kind: 'win_battles' }, GOAL_DEFAULTS.win_battles(ctx)),
        label: (o) => o.text || o.kind,
        render: (b2, o, p2) => {
          const ks = h('select', {}, ...Object.keys(GOALS).map((k) => h('option', { value: k, selected: o.kind === k }, k)));
          ks.addEventListener('change', () => {
            const text = o.text;
            for (const k of Object.keys(o)) delete o[k];
            Object.assign(o, { kind: ks.value }, GOAL_DEFAULTS[ks.value](ctx));
            if (text) o.text = text;
            ctx.changed(true);
          });
          b2.append(h('div', { class: 'xw-grid' }, h('label', { class: 'xw-field', 'data-path': `${p2}.kind` }, h('span', {}, 'Kind'), ks), field(ctx, o, { k: 'text', t: 'otext', label: 'Journal text' }, p2)));
          b2.append(grid(ctx, o, GOALS[o.kind] || [], p2));
        },
      });
      rewardsEd(ctx, box, it, p);
      const rp = joinPath(p, 'repeat');
      box.append(h('div', { class: 'xw-box', 'data-path': rp }, h('div', { class: 'xw-box-head' }, 'Repeat'), grid(ctx, it.repeat || (it.repeat = {}), [
        { k: 'repeatable', t: 'bool', label: 'Repeatable' },
        { k: 'cooldown_seconds', t: 'int', label: 'Cooldown (s)' },
        { k: 'max_completions', t: 'oint', label: 'At most' },
      ], rp)));
    },
  });

  el.append(h('h4', { class: 'xw-sub' }, 'Challenges'));
  splitEd(ctx, el, q, 'challenges', '', {
    arrayId: true,
    prefix: 'challenge',
    add: 'Challenge',
    make: () => ({ title: 'New challenge', description: '', rules: {}, score: {}, rewards: {} }),
    label: (it, id) => `${id}${it.title ? ' - ' + it.title : ''}`,
    render: (box, it, p) => {
      box.append(grid(ctx, it, [
        { k: 'title', label: 'Title' },
        { k: 'formation', t: 'oref', kind: 'formation', label: 'Formation', none: '(the next battle)' },
        { k: 'description', t: 'textarea', label: 'Description', wide: true },
      ], p));
      const rp = joinPath(p, 'rules');
      box.append(h('div', { class: 'xw-box', 'data-path': rp }, h('div', { class: 'xw-box-head' }, 'Rules'), grid(ctx, it.rules || (it.rules = {}), [
        { k: 'turn_limit', t: 'oint', label: 'Round limit' },
        { k: 'no_items', t: 'bool', label: 'No items' },
        { k: 'no_magic', t: 'bool', label: 'No Ra-Seru magic' },
        { k: 'solo', t: 'oint', label: 'Solo character slot' },
        { k: 'level_cap', t: 'oint', label: 'Level cap' },
        { k: 'enemy_scale_permille', t: 'oint', label: 'Enemy scale (1/1000)' },
      ], rp)));
      const sp = joinPath(p, 'score');
      box.append(h('div', { class: 'xw-box', 'data-path': sp }, h('div', { class: 'xw-box-head' }, 'Score'), grid(ctx, it.score || (it.score = {}), [
        { k: 'base', t: 'int', label: 'Base' },
        { k: 'turn_bonus', t: 'int', label: 'Per round spare' },
        { k: 'level_bonus', t: 'int', label: 'Per level under the cap' },
      ], sp)));
      rewardsEd(ctx, box, it, p);
    },
  });
}

// --- Dialog --------------------------------------------------------------------------------
function rowsView(rowsJson, limit) {
  const box = h('div', { class: 'xw-rows' });
  (rowsJson || []).forEach((page, pi) => {
    page.forEach((r, ri) => {
      const cls = r.issues.length ? 'is-bad' : r.long ? 'is-long' : '';
      const div = h('div', { class: (ri === 0 && pi > 0 ? 'xw-page ' : '') + cls }, h('span', {}, `${r.chars}`), h('span', {}, r.text));
      if (r.long) div.title = `over ${limit} characters: dialog boxes do not wrap`;
      if (r.issues.length) div.title = r.issues.join('\n');
      box.append(div);
    });
  });
  return box;
}

export function renderDialog(ctx, el) {
  const d = ctx.pack.dialog;
  el.append(h('p', { class: 'xw-note' }, 'dialog.toml: each line of text is one row of the dialog box, a blank line starts a page. Rows do not wrap: keep them near 36 characters. Text is in the translation markup; anything the game font cannot draw is an error.'));
  splitEd(ctx, el, d, 'lines', '', {
    prefix: 'line',
    add: 'Line',
    idLabel: 'Line id',
    make: () => ({ text: 'Hello.', choices: [] }),
    label: (l, id) => `${id}${l.speaker ? ' (' + l.speaker + ')' : ''}`,
    onRename: (from, to) => renameRefs(ctx.pack, 'line', from, to),
    render: (box, line, p) => {
      box.append(grid(ctx, line, [{ k: 'speaker', t: 'otext', label: 'Speaker' }], p));
      const tf = field(ctx, line, { k: 'text', t: 'textarea', label: 'Text', wide: true, rows: 7 }, p);
      const check = h('div', { class: 'xw-dialog-check' });
      tf.append(check);
      box.append(tf);
      const recheck = () => {
        const res = ctx.checkDialog(line);
        check.innerHTML = '';
        if (!res) return;
        check.append(rowsView(res.pages, res.row_limit));
        res.choices.forEach((c, k) => {
          check.append(h('div', { class: 'xw-hint' }, `option ${k + 1}`), rowsView(c.label, res.row_limit));
          if (c.reply) check.append(rowsView(c.reply, res.row_limit));
        });
        check.append(h('div', { class: res.error ? 'wb-status is-err' : 'wb-status is-ok' }, res.error ? res.error : `compiles to ${res.bytes} byte(s)`));
      };
      box.addEventListener('input', recheck);
      recheck();
      listEd(ctx, box, line, 'choices', p, {
        title: 'Choices',
        hint: 'none, or two to four',
        add: 'Choice',
        max: 4,
        make: () => ({ label: 'Yes.' }),
        label: (c) => c.label,
        render: (b2, c, p2) => b2.append(grid(ctx, c, [
          { k: 'label', label: 'Label' },
          { k: 'next', t: 'oref', kind: 'line', label: 'Then line' },
          { k: 'reply', t: 'otextarea', label: 'Reply', wide: true },
          { k: 'set_flags', t: 'csv', label: 'Set pack flags' },
          { k: 'clear_flags', t: 'csv', label: 'Clear pack flags' },
          { k: 'battle', t: 'oref', kind: 'formation', label: 'Then battle' },
          { k: 'boss', t: 'oref', kind: 'boss', label: 'Then boss' },
          { k: 'challenge', t: 'oref', kind: 'challenge', label: 'Arm challenge' },
          { k: 'mode', t: 'oref', kind: 'mode', label: 'Then mode' },
        ], p2)),
      });
    },
  });
}

// Rename a dialogue line's references across the pack (quests, choices, map
// triggers), so renaming in the editor does not strand them.
function renameRefs(pack, kind, from, to) {
  if (kind !== 'line') return;
  for (const q of (pack.quests && pack.quests.quests) || []) {
    for (const k of ['start_dialog', 'active_dialog', 'complete_dialog']) if (q[k] === from) q[k] = to;
  }
  for (const l of Object.values((pack.dialog && pack.dialog.lines) || {})) {
    for (const c of l.choices || []) if (c.next === from) c.next = to;
  }
  for (const m of Object.values(pack.maps || {})) {
    for (const t of m.triggers || []) if (t.event && t.event.kind === 'dialog' && t.event.line === from) t.event.line = to;
  }
}

// --- Cards ---------------------------------------------------------------------------------
export function renderCards(ctx, el) {
  const c = ctx.pack.cards;
  el.append(h('p', { class: 'xw-note' }, 'cards.toml: trading cards (a retail monster as the art), booster sets, where boosters drop, and what the shops pay per rarity.'));
  c.sell_prices = c.sell_prices || { common: 0, uncommon: 0, rare: 0, legendary: 0 };
  el.append(h('h4', { class: 'xw-sub' }, 'Sell prices'), grid(ctx, c.sell_prices, RARITIES.map((r) => ({ k: r, t: 'int', label: r })), 'sell_prices'));
  el.append(h('h4', { class: 'xw-sub' }, 'Cards'));
  splitEd(ctx, el, c, 'cards', '', {
    arrayId: true,
    prefix: 'card',
    add: 'Card',
    make: () => ({ name: 'New card', flavor: '', rarity: 'common', sides: [1, 1, 1, 1], art: null }),
    label: (it, id) => `${id} [${it.rarity}]`,
    render: (box, it, p) => box.append(grid(ctx, it, [
      { k: 'name', label: 'Name' },
      { k: 'rarity', t: 'sel', opts: RARITIES, label: 'Rarity' },
      { k: 'element', t: 'osel', opts: CARD_ELEMENTS, label: 'Element' },
      { k: 'art', t: 'omonster', label: 'Art (retail monster)' },
      { k: 'sides', t: 'tuple', n: 4, names: ['top', 'right', 'bottom', 'left'], label: 'Sides', hint: 'top right bottom left' },
      { k: 'sell', t: 'oint', label: 'Sell price', ph: 'by rarity' },
      { k: 'flavor', t: 'textarea', label: 'Flavour text', wide: true },
    ], p)),
  });
  el.append(h('h4', { class: 'xw-sub' }, 'Boosters'));
  splitEd(ctx, el, c, 'boosters', '', {
    arrayId: true,
    prefix: 'booster',
    add: 'Booster',
    make: () => ({ name: 'New booster', cards_per_pack: 5, pool: [], weights: { common: 70, uncommon: 23, rare: 6, legendary: 1 } }),
    render: (box, it, p) => {
      box.append(grid(ctx, it, [
        { k: 'name', label: 'Name' },
        { k: 'cards_per_pack', t: 'int', label: 'Cards per pack' },
        { k: 'guaranteed', t: 'osel', opts: RARITIES, label: 'Guaranteed one at least' },
        { k: 'price', t: 'oint', label: 'Shop price' },
        { k: 'pool', t: 'refs', kind: 'card', label: 'Pool (empty: every card)', wide: true },
      ], p));
      const wp = joinPath(p, 'weights');
      box.append(h('h4', { class: 'xw-sub' }, 'Rarity weights'), grid(ctx, it.weights || (it.weights = {}), RARITIES.map((r) => ({ k: r, t: 'int', label: r })), wp));
    },
  });
  listEd(ctx, el, c, 'rewards', '', {
    title: 'Drops',
    add: 'Drop rule',
    make: () => ({ source: { kind: 'monster', id: 1 }, booster: ctx.ids('booster')[0] || null, chance_percent: 100, count: 1 }),
    label: (r) => `${r.source.kind} ${r.source.id}`,
    render: (box, r, p) => {
      const sp = joinPath(p, 'source');
      const ks = h('select', {}, ...['monster', 'formation', 'mode'].map((k) => h('option', { value: k, selected: r.source.kind === k }, k)));
      ks.addEventListener('change', () => { r.source = ks.value === 'monster' ? { kind: 'monster', id: 1 } : { kind: ks.value, id: ctx.ids(ks.value)[0] || '' }; ctx.changed(true); });
      const src = r.source.kind === 'monster'
        ? field(ctx, r.source, { k: 'id', t: 'monsterid', label: 'Monster' }, sp)
        : field(ctx, r.source, { k: 'id', t: 'ref', kind: r.source.kind, label: r.source.kind }, sp);
      box.append(h('div', { class: 'xw-grid' }, h('label', { class: 'xw-field', 'data-path': sp }, h('span', {}, 'Source'), ks), src));
      box.append(grid(ctx, r, [
        { k: 'booster', t: 'oref', kind: 'booster', label: 'Booster' },
        { k: 'card', t: 'oref', kind: 'card', label: 'Or one card' },
        { k: 'chance_percent', t: 'int', label: 'Chance %' },
        { k: 'count', t: 'int', label: 'Count' },
      ], p));
    },
  });
}

// --- Items ---------------------------------------------------------------------------------
const ITEM_KINDS = ['consumable', 'weapon', 'armor', 'helmet', 'boots', 'accessory', 'key'];
const STATUSES = ['venom', 'toxic', 'numb', 'sleep', 'confuse', 'rot', 'curse', 'stone'];
const EFFECTS = {
  heal_hp: [{ k: 'amount', t: 'int', label: 'HP' }, { k: 'full', t: 'bool', label: 'Full' }],
  heal_mp: [{ k: 'amount', t: 'int', label: 'MP' }, { k: 'full', t: 'bool', label: 'Full' }],
  revive: [{ k: 'percent', t: 'int', label: 'HP %' }],
  cure: [{ k: 'status', t: 'sel', opts: STATUSES, label: 'Status' }],
  cure_all: [],
  damage: [{ k: 'amount', t: 'int', label: 'Damage' }],
  as_item: [{ k: 'item', t: 'itemid', label: 'Retail item', hint: 'runs its own effect' }],
};
const EFFECT_MAKE = {
  heal_hp: () => ({ kind: 'heal_hp', amount: 500 }),
  heal_mp: () => ({ kind: 'heal_mp', amount: 100 }),
  revive: () => ({ kind: 'revive', percent: 50 }),
  cure: () => ({ kind: 'cure', status: 'venom' }),
  cure_all: () => ({ kind: 'cure_all' }),
  damage: () => ({ kind: 'damage', amount: 1000 }),
  as_item: () => ({ kind: 'as_item', item: 0x8e }),
};

function effectEd(ctx, box, it, p) {
  const ep = joinPath(p, 'effect');
  const eb = h('div', { class: 'xw-box', 'data-path': ep });
  const ks = h('select', {}, ...Object.keys(EFFECTS).map((k) => h('option', { value: k, selected: (it.effect || {}).kind === k }, k.replace('_', ' '))));
  ks.addEventListener('change', () => { it.effect = EFFECT_MAKE[ks.value](); ctx.changed(true); });
  if (!it.effect) it.effect = EFFECT_MAKE.heal_hp();
  eb.append(h('div', { class: 'xw-box-head' }, h('label', { class: 'xw-field' }, h('span', {}, 'Effect'), ks)));
  eb.append(grid(ctx, it.effect, EFFECTS[it.effect.kind] || [], ep));
  eb.append(grid(ctx, it, [
    { k: 'all_party', t: 'bool', label: 'Whole party' },
    { k: 'field', t: 'dbool', label: 'Usable in the field' },
    { k: 'battle', t: 'dbool', label: 'Usable in battle' },
  ], p));
  box.append(eb);
}

export function renderItems(ctx, el) {
  const s = ctx.pack.items || (ctx.pack.items = { items: {}, sets: {}, shops: [] });
  el.append(h('p', { class: 'xw-note' }, 'items.toml: new consumables and equipment, equipment sets and the pack\'s shops. A pack item lives beside the retail id space; drops, steals, rewards and shop rows pick a retail item or a pack item. An item borrows a retail item\'s look as its icon (and, for a weapon, its battle mesh).'));
  el.append(h('h4', { class: 'xw-sub' }, 'Items'));
  splitEd(ctx, el, s, 'items', '', {
    prefix: 'item',
    add: 'Item',
    make: () => ({ name: 'New item', kind: 'consumable', price: 100, effect: EFFECT_MAKE.heal_hp() }),
    label: (it, id) => `${id} [${it.kind || 'consumable'}]`,
    render: (box, it, p) => {
      const kind = it.kind || 'consumable';
      box.append(grid(ctx, it, [
        { k: 'name', label: 'Name', hint: 'at most 20 characters' },
        { k: 'kind', t: 'sel', opts: ITEM_KINDS, label: 'Kind' },
        { k: 'icon', t: 'oitem', label: 'Icon / look (retail item)' },
        { k: 'price', t: 'int', label: 'Buy price', hint: '0: not sold' },
        { k: 'sell', t: 'oint', label: 'Sell price', ph: 'half the price' },
        { k: 'description', t: 'textarea', label: 'Description', hint: '| breaks a line', wide: true },
      ], p));
      if (kind === 'consumable') {
        effectEd(ctx, box, it, p);
      } else if (kind !== 'key') {
        box.append(h('h4', { class: 'xw-sub' }, 'While worn'));
        box.append(grid(ctx, it, [
          { k: 'characters', t: 'chars', label: 'Worn by', hint: 'none: everyone', wide: true },
          { k: 'attack', t: 'int', label: 'ATK' },
          { k: 'defence', t: 'int', label: 'DEF (both)' },
          { k: 'udf', t: 'int', label: 'UDF' },
          { k: 'ldf', t: 'int', label: 'LDF' },
          { k: 'speed', t: 'int', label: 'SPD' },
          { k: 'intelligence', t: 'int', label: 'INT', hint: 'menu only, as retail head gear' },
          { k: 'element', t: 'osel', label: 'Guards element', opts: ELEMENTS.slice(0, 7).map((n, i) => [String(i), n]), num: true },
          { k: 'passives', t: 'csvint', label: 'Passives', hint: 'accessory passive indices 0-63' },
          { k: 'set', t: 'oref', kind: 'item_set', label: 'Set' },
        ], p));
      }
    },
  });
  el.append(h('h4', { class: 'xw-sub' }, 'Sets'));
  splitEd(ctx, el, s, 'sets', '', {
    prefix: 'set',
    add: 'Set',
    make: () => ({ name: 'New set', bonuses: [{ pieces: 2, attack: 5 }] }),
    render: (box, set, p) => {
      box.append(grid(ctx, set, [{ k: 'name', label: 'Name' }], p));
      listEd(ctx, box, set, 'bonuses', p, {
        title: 'Tiers',
        add: 'Tier',
        make: () => ({ pieces: 2 }),
        label: (b) => `${b.pieces} pieces`,
        render: (b2, b, p2) => b2.append(grid(ctx, b, [
          { k: 'pieces', t: 'int', label: 'Pieces worn' },
          { k: 'attack', t: 'int', label: 'ATK' },
          { k: 'defence', t: 'int', label: 'DEF (both)' },
          { k: 'speed', t: 'int', label: 'SPD' },
          { k: 'intelligence', t: 'int', label: 'INT' },
          { k: 'passives', t: 'csvint', label: 'Passives' },
        ], p2)),
      });
    },
  });
  el.append(h('h4', { class: 'xw-sub' }, 'Shops', h('span', { class: 'xw-hint' }, ' opened by a map shop trigger or a shop NPC')));
  splitEd(ctx, el, s, 'shops', '', {
    arrayId: true,
    prefix: 'shop',
    add: 'Shop',
    make: () => ({ name: 'New shop', stock: [] }),
    render: (box, shop, p) => {
      box.append(grid(ctx, shop, [{ k: 'name', label: 'Name' }], p));
      // Bare rows become `{ item }` so each can take a price and a gate.
      shop.stock = (shop.stock || []).map((e) => (e && typeof e === 'object' && 'item' in e ? e : { item: e }));
      listEd(ctx, box, shop, 'stock', p, {
        title: 'Stock',
        add: 'Row',
        make: () => ({ item: ctx.ids('item')[0] || (catList(ctx, 'items')[0] || { id: 0x77 }).id }),
        render: (b2, row, p2) => {
          b2.append(grid(ctx, row, [
            { k: 'item', t: 'anyitemid', label: 'Item' },
            { k: 'price', t: 'oint', label: 'Price', ph: 'the item\'s' },
          ], p2));
          b2.append(gateBox(ctx, row, 'requires', p2, 'Listed when'));
        },
      });
    },
  });
}

// --- Modes ---------------------------------------------------------------------------------
export function renderModes(ctx, el) {
  const m = ctx.pack.modes;
  el.append(h('p', { class: 'xw-note' }, 'modes.toml: instances of the engine\'s new modes. The rules live in the engine; a mode here picks the numbers and the layout.'));
  const ai = { k: 'ai_tier', t: 'sel', opts: AI_TIERS, label: 'AI' };
  const loc = { k: 'location', t: 'oscene', label: 'Location' };
  const sections = [
    ['poker', 'Poker tables', () => ({ name: 'New table', seats: 4, buy_in: 1000, small_blind: 10, big_blind: 20, currency: 'gold', chip_value: 1, ai_tier: 'regular' }), [
      { k: 'name', label: 'Name' }, loc,
      { k: 'seats', t: 'int', label: 'Seats' }, { k: 'buy_in', t: 'int', label: 'Buy-in' },
      { k: 'small_blind', t: 'int', label: 'Small blind' }, { k: 'big_blind', t: 'int', label: 'Big blind' },
      { k: 'currency', t: 'sel', opts: ['gold', 'coins'], label: 'Currency' }, { k: 'chip_value', t: 'int', label: 'Chip value' },
      ai, { k: 'max_hands', t: 'oint', label: 'Max hands' }, { k: 'blind_double_every', t: 'oint', label: 'Blinds double every' },
    ]],
    ['card_duels', 'Card duels', () => ({ name: 'New duel', deck: [], ai_tier: 'novice' }), [
      { k: 'name', label: 'Name' }, ai,
      { k: 'deck', t: 'refs', kind: 'card', label: 'Opponent deck', wide: true },
      { k: 'elemental_cells', t: 'bool', label: 'Elemental cells' }, { k: 'chain', t: 'bool', label: 'Chain captures' },
      { k: 'stakes', t: 'bool', label: 'Stakes (win a card)' }, { k: 'win_booster', t: 'oref', kind: 'booster', label: 'Win booster' },
    ]],
    ['golf', 'Golf courses', () => ({ name: 'New course', holes: [{ par: 2, rows: ['#####', '#T.O#', '#####'], heights: [], max_strokes: 10 }] }), [
      { k: 'name', label: 'Name' }, loc,
    ]],
    ['races', 'Race tracks', () => ({ name: 'New track', laps: 3, points: [[0, 0], [800, 0], [800, 800], [0, 800]], half_width: 90, boost_segments: [], racers: 4, ai_tier: 'regular' }), [
      { k: 'name', label: 'Name' }, loc,
      { k: 'laps', t: 'int', label: 'Laps' }, { k: 'racers', t: 'int', label: 'Racers' },
      { k: 'half_width', t: 'int', label: 'Half width' }, ai,
      { k: 'boost_segments', t: 'csvint', label: 'Boost segments' },
    ]],
    ['prop_hunt', 'Prop hunt arenas', () => ({ name: 'New arena', props: [], decoys: [], bounds: [0, 0, 960, 640], blockers: [], rounds: 4, hunters: 1, players: 4, hide_frames: 1800, round_frames: 10800, hunter_health: 100, miss_penalty: 10, ai_tier: 'regular' }), [
      { k: 'name', label: 'Name' }, loc,
      { k: 'players', t: 'int', label: 'Players' }, { k: 'hunters', t: 'int', label: 'Hunters' },
      { k: 'rounds', t: 'int', label: 'Rounds' }, ai,
      { k: 'hide_frames', t: 'int', label: 'Hide frames' }, { k: 'round_frames', t: 'int', label: 'Round frames' },
      { k: 'hunter_health', t: 'int', label: 'Hunter health' }, { k: 'miss_penalty', t: 'int', label: 'Miss penalty' },
      { k: 'bounds', t: 'tuple', n: 4, label: 'Bounds', hint: 'x0 y0 x1 y1' },
    ]],
  ];
  for (const [key, title, make, specs] of sections) {
    el.append(h('h4', { class: 'xw-sub' }, title));
    splitEd(ctx, el, m, key, '', {
      arrayId: true,
      prefix: key.replace(/s$/, ''),
      add: title.replace(/s$/, '').toLowerCase(),
      make,
      render: (box, it, p) => {
        box.append(grid(ctx, it, specs, p));
        if (key === 'golf') {
          box.append(h('p', { class: 'xw-note' }, 'A hole is a character grid: # wall, . fairway, , rough, s sand, w water, > slope, T tee, O cup. Heights are one digit per cell.'));
          listEd(ctx, box, it, 'holes', p, {
            add: 'Hole',
            make: () => ({ par: 3, rows: ['#####', '#T.O#', '#####'], heights: [], max_strokes: 10 }),
            label: (hole, i) => `hole ${i + 1}, par ${hole.par}`,
            render: (b2, hole, p2) => b2.append(grid(ctx, hole, [
              { k: 'par', t: 'int', label: 'Par' }, { k: 'max_strokes', t: 'int', label: 'Max strokes' },
              { k: 'rows', t: 'lines', label: 'Rows' }, { k: 'heights', t: 'lines', label: 'Heights' },
            ], p2)),
          });
        }
        if (key === 'races') box.append(pointsField(ctx, it, p));
        if (key === 'prop_hunt') {
          listEd(ctx, box, it, 'props', p, {
            title: 'Props', add: 'Prop',
            make: () => ({ id: 'prop', radius: 12, mesh: null }),
            label: (pr) => pr.id,
            render: (b2, pr, p2) => b2.append(grid(ctx, pr, [{ k: 'id', label: 'Id' }, { k: 'radius', t: 'int', label: 'Radius' }], p2)),
          });
          listEd(ctx, box, it, 'decoys', p, {
            title: 'Decoys', add: 'Decoy',
            make: () => ({ prop: (it.props[0] || { id: '' }).id, x: 0, y: 0 }),
            label: (d) => `${d.prop} at ${d.x}, ${d.y}`,
            render: (b2, d, p2) => b2.append(grid(ctx, d, [{ k: 'prop', t: 'sel', opts: it.props.map((pr) => pr.id), label: 'Prop' }, { k: 'x', t: 'int', label: 'X' }, { k: 'y', t: 'int', label: 'Y' }], p2)),
          });
          box.append(blockersField(ctx, it, p));
        }
      },
    });
  }
  renderPartyNights(ctx, el, m);
}

// A party night: a playlist of short rounds of the instances above, scored by
// placement (docs/subsystems/netplay.md, "Party night").
function renderPartyNights(ctx, el, m) {
  el.append(h('h4', { class: 'xw-sub' }, 'Party nights'));
  el.append(h('p', { class: 'xw-note' }, 'A night chains short rounds of the instances above into one scoreboard: points by placement, catch-up rules, prizes for the top places. Played alone from a map trigger, or hosted for a room.'));
  splitEd(ctx, el, m, 'party_nights', '', {
    arrayId: true,
    prefix: 'night',
    add: 'party night',
    make: () => ({ name: 'New night', order: 'fixed', rounds: [], points: [10, 6, 4, 2], last_place_picks: false, double_final: true, players: 4, rivals: [], ai_tier: 'regular', prizes: [] }),
    render: (box, it, p) => {
      box.append(grid(ctx, it, [
        { k: 'name', label: 'Name' }, { k: 'location', t: 'oscene', label: 'Location' },
        { k: 'order', t: 'sel', opts: ['fixed', 'shuffle'], label: 'Order' },
        { k: 'points', t: 'csvint', label: 'Points by place', hint: '1st first' },
        { k: 'last_place_picks', t: 'bool', label: 'Last place picks the next round' },
        { k: 'double_final', t: 'bool', label: 'Final round scores double' },
        { k: 'players', t: 'int', label: 'Seats played alone' },
        { k: 'rivals', t: 'csv', label: 'Rival names' },
        { k: 'ai_tier', t: 'sel', opts: AI_TIERS, label: 'Rival AI' },
      ], p));
      listEd(ctx, box, it, 'rounds', p, {
        title: 'Rounds', add: 'Round',
        make: () => ({ mode: ctx.ids('instance')[0] || '', name: '', holes: [], double: false, wager: 0 }),
        label: (r, i) => `${i + 1}. ${r.name || r.mode}`,
        render: (b2, r, p2) => b2.append(grid(ctx, r, [
          { k: 'mode', t: 'ref', kind: 'instance', label: 'Instance' }, { k: 'name', label: 'Title' },
          { k: 'hands', t: 'oint', label: 'Poker: hands' }, { k: 'stack', t: 'oint', label: 'Poker: stack' },
          { k: 'holes', t: 'csvint', label: 'Golf: holes', hint: '1-based; empty plays all' },
          { k: 'laps', t: 'oint', label: 'Race: laps' }, { k: 'rounds', t: 'oint', label: 'Prop hunt: rounds' },
          { k: 'double', t: 'bool', label: 'Double points' }, { k: 'wager', t: 'int', label: 'Wager %', hint: '0..50' },
        ], p2)),
      });
      listEd(ctx, box, it, 'prizes', p, {
        title: 'Prizes', add: 'Prize',
        make: () => ({ place: (it.prizes || []).length + 1, gold: 0, coins: 0, boosters: [], cards: [] }),
        label: (z) => `place ${z.place}`,
        render: (b2, z, p2) => b2.append(grid(ctx, z, [
          { k: 'place', t: 'int', label: 'Place' }, { k: 'gold', t: 'int', label: 'Gold' }, { k: 'coins', t: 'int', label: 'Coins' },
          { k: 'boosters', t: 'refs', kind: 'booster', label: 'Boosters', wide: true },
          { k: 'cards', t: 'refs', kind: 'card', label: 'Cards', wide: true },
        ], p2)),
      });
    },
  });
}

function pointsField(ctx, it, p) {
  const ta = h('textarea', { rows: 6, spellcheck: 'false' });
  ta.value = (it.points || []).map((q) => q.join(', ')).join('\n');
  ta.addEventListener('input', () => {
    it.points = ta.value.split('\n').map((l) => l.split(',').map((x) => intOr(x.trim(), NaN))).filter((q) => q.length === 2 && q.every(Number.isFinite));
    ctx.changed();
  });
  return h('label', { class: 'xw-field is-wide', 'data-path': `${p}.points` }, h('span', {}, 'Centre-line points', h('span', { class: 'xw-hint' }, ' one "x, z" per line, a closed loop')), ta);
}

function blockersField(ctx, it, p) {
  const ta = h('textarea', { rows: 4, spellcheck: 'false' });
  ta.value = (it.blockers || []).map((q) => q.join(', ')).join('\n');
  ta.addEventListener('input', () => {
    it.blockers = ta.value.split('\n').map((l) => l.split(',').map((x) => intOr(x.trim(), NaN))).filter((q) => q.length === 4 && q.every(Number.isFinite));
    ctx.changed();
  });
  return h('label', { class: 'xw-field is-wide', 'data-path': `${p}.blockers` }, h('span', {}, 'Blockers', h('span', { class: 'xw-hint' }, ' one "x0, y0, x1, y1" per line')), ta);
}

export { esc };
