/* Expansion workbench: the Arts tab (arts.toml) and the Spells tab
 * (spells.toml).
 *
 * A pack art plays the hero's own retail art clips: its form is the command
 * string (built with the arrow buttons; each hero's arm mapping labels them),
 * the segment list (a retail art per segment, from the disc's art list when
 * one is loaded) with its power bytes, and a damage preview the session runs
 * through the retail melee kernel (`art_preview`, crates/web-expansion
 * session.rs). The combo is checked against the disc's arts-name table: a
 * combo equal to a retail one never fires, and one containing a retail combo
 * loses those arrows to it. Format: docs/formats/expansion-pack-arts.md.
 */

import { h, grid, listEd, splitEd, gateBox, ELEMENTS } from './expansion-workbench-forms.js';

const joinPath = (base, k) => (base ? (typeof k === 'number' ? `${base}[${k}]` : `${base}.${k}`) : String(k));
const STATUSES = ['venom', 'toxic', 'numb', 'sleep', 'confuse', 'rot', 'curse', 'stone'];
const HEROES = ['vahn', 'noa', 'gala'];
const GLYPH = { L: '←', R: '→', U: '↑', D: '↓' };
// The weapon-hand command is Left for Vahn and Gala, Right for Noa
// (docs/subsystems/arts-command-gauge.md).
const ARM = (hero) => (hero === 'noa'
  ? { L: 'Ra-Seru', R: 'Arms', U: 'High', D: 'Low' }
  : { L: 'Arms', R: 'Ra-Seru', U: 'High', D: 'Low' });
const arrows = (s) => String(s || '').toUpperCase().replace(/[^LRUD]/g, '');

function intOr(v, d) {
  const n = parseInt(v, 10);
  return Number.isFinite(n) ? n : d;
}

// The disc's retail clips of `hero`: `[constant, name]`.
function clipOptions(ctx, hero) {
  const list = (ctx.cat && ctx.cat.art_clips) || [];
  return list.filter((c) => c.character === hero).map((c) => [c.constant, c.name]);
}

// A retail combo check: `{ equal, inside }` rows of the disc's arts table.
function comboClash(ctx, hero, cmds) {
  const rows = ((ctx.cat && ctx.cat.art_combos) || []).filter((r) => r.character === hero && r.commands.length >= 2);
  return {
    equal: rows.filter((r) => r.commands === cmds),
    inside: rows.filter((r) => r.commands !== cmds && cmds.includes(r.commands)),
  };
}

function commandBuilder(ctx, art) {
  const hero = art.character || 'vahn';
  const arm = ARM(hero);
  const wrap = h('div', { class: 'xw-box' });
  const cur = arrows(art.commands);
  const show = h('div', { class: 'xw-combo', title: cur }, cur ? cur.split('').map((c) => GLYPH[c]).join(' ') : '(chain finisher: no arrows)');
  const pads = h('div', { class: 'xw-inline wb-inline' });
  for (const c of ['L', 'U', 'D', 'R']) {
    pads.append(h('button', {
      type: 'button',
      class: 'wb-mini',
      title: `${c}: ${arm[c]}`,
      onclick: () => { art.commands = (arrows(art.commands) + c).slice(0, 9); ctx.changed(true); },
    }, `${GLYPH[c]} ${arm[c]}`));
  }
  pads.append(h('button', { type: 'button', class: 'wb-mini', onclick: () => { art.commands = arrows(art.commands).slice(0, -1); ctx.changed(true); } }, '⌫'));
  pads.append(h('button', { type: 'button', class: 'wb-mini', onclick: () => { delete art.commands; ctx.changed(true); } }, 'Clear'));
  wrap.append(h('div', { class: 'xw-box-head' }, 'Commands', h('span', { class: 'xw-hint' }, ' 2 to 9 arrows; none for a chain-only finisher')), show, pads);
  if (cur && ctx.cat && ctx.cat.art_combos) {
    const { equal, inside } = comboClash(ctx, hero, cur);
    for (const r of equal) wrap.append(h('div', { class: 'xw-note wb-bad' }, `${r.name} has this combo: the retail art matches first and this art never fires.`));
    for (const r of inside) wrap.append(h('div', { class: 'xw-note wb-bad' }, `Contains ${r.name}'s ${r.commands}: those arrows become ${r.name}.`));
    if (!equal.length && !inside.length) wrap.append(h('div', { class: 'xw-note' }, 'No retail combo of this hero inside it.'));
  } else if (cur && !(ctx.cat && ctx.cat.art_combos)) {
    wrap.append(h('div', { class: 'xw-hint' }, 'Load a disc to check the combo against the retail arts.'));
  }
  return wrap;
}

function segmentEd(ctx, box, art, p) {
  const hero = art.character || 'vahn';
  const opts = clipOptions(ctx, hero);
  listEd(ctx, box, art, 'segments', p, {
    title: 'Segments',
    hint: 'the retail clips the art plays, in order',
    add: 'Segment',
    max: 8,
    make: () => ({ art: opts.length ? opts[0][0] : 0x1b, power: ['U22'] }),
    label: (s) => {
      const o = opts.find(([v]) => v === s.art || String(s.art).toLowerCase() === String(v).toLowerCase());
      return typeof s.art === 'string' ? s.art : o ? o[1] : `0x${Number(s.art).toString(16)}`;
    },
    render: (b2, seg, p2) => {
      if (opts.length) {
        const sel = h('select', {});
        let found = false;
        for (const [v, name] of opts) {
          const on = seg.art === v || (typeof seg.art === 'string' && seg.art.toLowerCase() === name.toLowerCase());
          if (on) found = true;
          sel.append(h('option', { value: String(v), selected: on }, `0x${v.toString(16).toUpperCase()} ${name}`));
        }
        if (!found) sel.append(h('option', { value: '', selected: true }, `${seg.art} (not ${hero}'s)`));
        sel.addEventListener('change', () => { seg.art = intOr(sel.value, seg.art); ctx.changed(true); });
        b2.append(h('label', { class: 'xw-field', 'data-path': joinPath(p2, 'art') }, h('span', {}, 'Clip of'), sel));
      } else {
        b2.append(grid(ctx, seg, [{ k: 'art', t: 'int', label: 'Retail art constant', hint: '0x1B..0x32 (load a disc for names)' }], p2));
      }
      b2.append(grid(ctx, seg, [
        { k: 'power', t: 'csv', label: 'Power per beat', hint: 'U12..U28 upper, L12..L28 lower (12 18 20 22 28); empty keeps the clip\'s' },
        { k: 'beats', t: 'csvint', label: 'Beat frames', hint: 'empty keeps the clip\'s' },
      ], p2));
    },
  });
}

function previewBox(ctx, art, p) {
  const key = `arts-preview:${p}`;
  const st = ctx.sel[key] || (ctx.sel[key] = { atk: 300, def: 200 });
  const box = h('div', { class: 'xw-box' });
  const atk = h('input', { type: 'number', value: st.atk });
  const def = h('input', { type: 'number', value: st.def });
  const out = h('div', {});
  const run = () => {
    st.atk = intOr(atk.value, 0);
    st.def = intOr(def.value, 0);
    const r = ctx.artPreview(art, st.atk, st.def);
    out.innerHTML = '';
    if (!r || r.error) { out.append(h('div', { class: 'xw-hint' }, r ? r.error : 'preview unavailable')); return; }
    if (!r.hits.length) { out.append(h('div', { class: 'xw-hint' }, 'No power bytes set: the clips\' own power applies (load the battle to see it).')); return; }
    const rows = r.hits.map((x) => h('tr', {}, h('td', {}, String(x.segment + 1)), h('td', {}, x.power || '?'), h('td', {}, String(x.low)), h('td', {}, String(x.mean)), h('td', {}, String(x.high))));
    out.append(h('table', { class: 'xw-table' },
      h('thead', {}, h('tr', {}, h('th', {}, 'Seg'), h('th', {}, 'Power'), h('th', {}, 'Low'), h('th', {}, 'Mean'), h('th', {}, 'High'))),
      h('tbody', {}, ...rows, h('tr', {}, h('td', {}, ''), h('td', {}, 'Total'), h('td', {}, String(r.low)), h('td', {}, String(r.mean)), h('td', {}, String(r.high))))));
  };
  atk.addEventListener('input', run);
  def.addEventListener('input', run);
  box.append(h('div', { class: 'xw-box-head' }, 'Damage preview', h('span', { class: 'xw-hint' }, ' the retail melee kernel, 64 rolls a beat; ATK with half the weapon folded in')),
    h('div', { class: 'xw-inline wb-inline' }, h('label', { class: 'xw-field' }, h('span', {}, 'ATK'), atk), h('label', { class: 'xw-field' }, h('span', {}, 'Target DEF'), def)), out);
  run();
  return box;
}

function riderBox(ctx, owner, key, path, title) {
  const rp = joinPath(path, key);
  const box = h('div', { class: 'xw-box', 'data-path': rp });
  const on = h('input', { type: 'checkbox' });
  on.checked = !!owner[key];
  on.addEventListener('change', () => { if (on.checked) owner[key] = { status: 'venom', chance: 25 }; else delete owner[key]; ctx.changed(true); });
  box.append(h('div', { class: 'xw-box-head' }, h('label', { class: 'xw-check xw-field' }, on, title)));
  if (owner[key]) box.append(grid(ctx, owner[key], [{ k: 'status', t: 'sel', opts: STATUSES, label: 'Status' }, { k: 'chance', t: 'int', label: 'Chance %' }], rp));
  return box;
}

function learnBox(ctx, owner, path, capture) {
  const l = owner.learn || (owner.learn = {});
  const lp = joinPath(path, 'learn');
  const box = h('div', { class: 'xw-box', 'data-path': lp });
  box.append(h('div', { class: 'xw-box-head' }, 'Learned when', h('span', { class: 'xw-hint' }, ' every set condition holds; nothing set is known at once')));
  const specs = [];
  if (capture) specs.push({ k: 'capture', t: 'oref', kind: 'monster', label: 'Captured pack monster' });
  specs.push({ k: 'level', t: 'oint', label: 'Hero level at least' }, { k: 'item', t: 'anyitemid', label: 'Item held', none: '(none)' });
  box.append(grid(ctx, l, specs, lp));
  box.append(gateBox(ctx, l, 'requires', lp, 'And the pack gate'));
  return box;
}

export function renderArts(ctx, el) {
  const s = ctx.pack.arts || (ctx.pack.arts = { arts: {}, chains: {} });
  el.append(h('p', { class: 'xw-note' }, 'arts.toml: new Tactical Arts. An art plays the hero\'s own retail art clips (a segment per clip) under the pack\'s power bytes, damage percent, element and status rider; the arts input matches it as one more record after every retail art, so retail arts keep matching. A chain plays a finisher when a turn\'s arts end with its links.'));
  el.append(h('h4', { class: 'xw-sub' }, 'Arts'));
  splitEd(ctx, el, s, 'arts', '', {
    prefix: 'art',
    add: 'Art',
    make: () => ({ character: 'vahn', name: 'New art', commands: 'RURDU', ap: 24, segments: [{ art: 0x27, power: ['L22'] }] }),
    label: (a, id) => `${id} [${a.character || 'vahn'}]`,
    render: (box, a, p) => {
      box.append(grid(ctx, a, [
        { k: 'character', t: 'sel', opts: HEROES, label: 'Hero' },
        { k: 'name', label: 'Name', hint: 'at most 20 characters' },
        { k: 'ap', t: 'int', label: 'AP', hint: 'from the Spirit gauge, 0-100' },
        { k: 'damage_pct', t: 'oint', label: 'Damage %', ph: '100' },
        { k: 'element', t: 'osel', label: 'Element', opts: ELEMENTS.slice(0, 7).map((n, i) => [String(i), n]), num: true },
        { k: 'description', t: 'textarea', label: 'Description', hint: '| breaks a line', wide: true },
      ], p));
      box.append(commandBuilder(ctx, a));
      segmentEd(ctx, box, a, p);
      box.append(previewBox(ctx, a, p));
      box.append(riderBox(ctx, a, 'rider', p, 'Status rider'));
      box.append(learnBox(ctx, a, p, false));
    },
  });
  el.append(h('h4', { class: 'xw-sub' }, 'Chains', h('span', { class: 'xw-hint' }, ' the pack\'s Super Arts')));
  splitEd(ctx, el, s, 'chains', '', {
    prefix: 'chain',
    add: 'Chain',
    make: () => ({ character: 'vahn', arts: [], finisher: ctx.ids('art')[0] || '' }),
    render: (box, c, p) => {
      box.append(grid(ctx, c, [
        { k: 'character', t: 'sel', opts: HEROES, label: 'Hero' },
        { k: 'finisher', t: 'ref', kind: 'art', label: 'Finisher (a pack art)' },
      ], p));
      const opts = [...ctx.ids('art').map((k) => ['p:' + k, `${k} (pack)`]), ...clipOptions(ctx, c.character || 'vahn').map(([v, n]) => ['r:' + v, n])];
      listEd(ctx, box, c, 'arts', p, {
        title: 'The turn ends with',
        add: 'Art',
        max: 6,
        make: () => (ctx.ids('art')[0] || 0x27),
        label: (l) => (typeof l === 'string' ? l : `0x${Number(l).toString(16).toUpperCase()}`),
        render: (b2, link, p2, i) => {
          const sel = h('select', {});
          const val = typeof link === 'string' ? 'p:' + link : 'r:' + link;
          for (const [v, l] of opts) sel.append(h('option', { value: v, selected: v === val }, l));
          if (!opts.some(([v]) => v === val)) sel.append(h('option', { value: val, selected: true }, `${link} (unknown)`));
          sel.addEventListener('change', () => {
            c.arts[i] = sel.value.startsWith('p:') ? sel.value.slice(2) : intOr(sel.value.slice(2), 0x27);
            ctx.changed(true);
          });
          b2.append(h('label', { class: 'xw-field', 'data-path': p2 }, h('span', {}, 'Art'), sel));
        },
      });
    },
  });
}

const TARGETS = ['one_enemy', 'all_enemies', 'one_ally', 'all_allies'];

function spellOptions(ctx) {
  return ((ctx.cat && ctx.cat.spells) || []).filter((s) => s.id >= 0x81 && s.id <= 0xa0).map((s) => [s.id, s.name.trim()]);
}

function growthBox(sp) {
  const base = sp.power == null ? 100 : sp.power;
  const g = sp.growth || 0;
  const cells = [];
  for (let l = 1; l <= 9; l++) cells.push(h('td', {}, `${base + g * (l - 1)}%`));
  return h('div', { class: 'xw-box' }, h('div', { class: 'xw-box-head' }, 'Power by magic level', h('span', { class: 'xw-hint' }, ' of the base cast\'s rolled damage or restore')),
    h('table', { class: 'xw-table' }, h('thead', {}, h('tr', {}, ...[1, 2, 3, 4, 5, 6, 7, 8, 9].map((l) => h('th', {}, `Lv ${l}`)))), h('tbody', {}, h('tr', {}, ...cells))));
}

export function renderSpells(ctx, el) {
  const s = ctx.pack.spells || (ctx.pack.spells = { spells: {} });
  el.append(h('p', { class: 'xw-note' }, 'spells.toml: new Seru magic. A spell casts as its base retail Seru - its summon, creature and camera - with the pack\'s MP, power, element, target and rider, and keeps its own magic level. Capturing the named pack monster teaches it.'));
  const opts = spellOptions(ctx);
  splitEd(ctx, el, s, 'spells', '', {
    prefix: 'spell',
    add: 'Spell',
    make: () => ({ name: 'New Seru', base: 0x81, power: 150 }),
    label: (sp, id) => {
      const b = opts.find(([v]) => v === sp.base);
      return `${id} [${b ? b[1] : '0x' + Number(sp.base).toString(16)}]`;
    },
    render: (box, sp, p) => {
      const baseField = opts.length
        ? { k: 'base', t: 'sel', opts: opts.map(([v, n]) => [String(v), `0x${v.toString(16).toUpperCase()} ${n}`]), num: true, label: 'Cast as (retail Seru)' }
        : { k: 'base', t: 'int', label: 'Cast as (retail Seru id)', hint: '0x81..0xA0 (load a disc for names)' };
      box.append(grid(ctx, sp, [
        { k: 'name', label: 'Name', hint: 'at most 20 characters' },
        baseField,
        { k: 'mp', t: 'oint', label: 'MP', ph: 'the base\'s' },
        { k: 'power', t: 'oint', label: 'Power %', ph: '100' },
        { k: 'growth', t: 'oint', label: 'Growth % / level', ph: '0' },
        { k: 'element', t: 'osel', label: 'Element', opts: ELEMENTS.map((n, i) => [String(i), n]), num: true, none: '(the creature\'s)' },
        { k: 'target', t: 'osel', label: 'Target', opts: TARGETS, none: '(the base\'s)' },
        { k: 'characters', t: 'chars', label: 'Learned by', hint: 'none: everyone', wide: true },
        { k: 'description', t: 'textarea', label: 'Description', hint: '| breaks a line', wide: true },
      ], p));
      box.append(growthBox(sp));
      box.append(riderBox(ctx, sp, 'status', p, 'Status rider'));
      box.append(learnBox(ctx, sp, p, true));
    },
  });
}
