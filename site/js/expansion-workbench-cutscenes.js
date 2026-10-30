/* Expansion workbench: the Cutscenes tab (cutscenes.toml).
 *
 * A cutscene is a list of steps (`{ do: "move", ... }`) on a main track plus
 * named tracks that run beside it. The step-list editor here is a form per
 * step whose fields follow its `do` key (STEP_KINDS), written straight into
 * the pack JSON like every other editor. The timeline preview beside it is
 * the session's: `cutscene_timeline(pack, id, picks)` plays the cutscene on
 * the engine's own sequencer against a stand-in world and returns which step
 * ran on which frame of which track (crates/engine-expansion
 * cutscene::preview). Format: docs/formats/expansion-pack-cutscenes.md.
 */

import { h, field, grid, splitEd, gateBox } from './expansion-workbench-forms.js';

const joinPath = (base, k) => (base ? (typeof k === 'number' ? `${base}[${k}]` : `${base}.${k}`) : String(k));

// Each step kind's fields, in the form kit's spec shape. `actor` and
// `tiles` / `tile` / `focus` are this module's own pickers.
const STEP_KINDS = {
  wait: [{ k: 'frames', t: 'int', label: 'Frames', hint: '60 a second' }],
  say: [{ k: 'line', t: 'ref', kind: 'line', label: 'Line' }, { k: 'on_choice', t: 'csv', label: 'Option labels', hint: 'a label per option, in order; empty goes on' }],
  move: [{ k: 'actor', t: 'actor', label: 'Actor' }, { k: 'path', t: 'tiles', label: 'Path', hint: 'c,r c,r ... walked in order' }, { k: 'speed', t: 'oint', label: 'Speed', hint: 'world units a frame; empty = its own' }, { k: 'run', t: 'bool', label: 'Run' }, { k: 'clip', t: 'oint', label: 'Walk clip' }, { k: 'wait', t: 'bool', label: 'Wait for it' }],
  face: [{ k: 'actor', t: 'actor', label: 'Actor' }, { k: 'facing', t: 'oint', label: 'Sector', hint: '0..7; or toward' }, { k: 'toward', t: 'oactor', label: 'Toward' }],
  clip: [{ k: 'actor', t: 'actor', label: 'Actor' }, { k: 'clip', t: 'int', label: 'Clip' }, { k: 'frames', t: 'oint', label: 'Hold frames', hint: 'then the idle clip' }],
  teleport: [{ k: 'actor', t: 'actor', label: 'Actor' }, { k: 'tile', t: 'tile', label: 'Tile' }, { k: 'facing', t: 'oint', label: 'Sector' }],
  show: [{ k: 'npc', t: 'npc', label: 'NPC' }, { k: 'tile', t: 'otile', label: 'At tile' }],
  hide: [{ k: 'npc', t: 'npc', label: 'NPC' }],
  camera: [{ k: 'focus', t: 'focus', label: 'Look at' }, { k: 'yaw', t: 'ofloat', label: 'Yaw', hint: 'degrees off the follow shot' }, { k: 'pitch', t: 'ofloat', label: 'Pitch', hint: 'degrees; + looks down' }, { k: 'zoom', t: 'ofloat', label: 'Zoom', hint: 'x distance; > 1 pulls back' }, { k: 'frames', t: 'int', label: 'Frames', hint: '0 cuts' }, { k: 'wait', t: 'bool', label: 'Wait for it' }],
  camera_reset: [{ k: 'frames', t: 'int', label: 'Frames' }, { k: 'wait', t: 'bool', label: 'Wait for it' }],
  shake: [{ k: 'amplitude', t: 'int', label: 'Amplitude' }, { k: 'frames', t: 'int', label: 'Frames' }],
  fade: [{ k: 'to', t: 'sel', opts: ['black', 'white', 'clear'], label: 'To' }, { k: 'frames', t: 'int', label: 'Frames' }, { k: 'wait', t: 'bool', label: 'Wait for it' }],
  flash: [{ k: 'color', t: 'tuple', n: 3, label: 'Colour', hint: 'r g b' }, { k: 'frames', t: 'int', label: 'Frames' }],
  tint: [{ k: 'color', t: 'tuple', n: 3, label: 'Colour', hint: '0 0 0 clears' }, { k: 'frames', t: 'int', label: 'Frames' }, { k: 'mode', t: 'sel', opts: ['add', 'subtract'], label: 'Mix' }],
  set_flag: [{ k: 'flag', label: 'Pack flag' }],
  clear_flag: [{ k: 'flag', label: 'Pack flag' }],
  counter: [{ k: 'counter', label: 'Pack counter' }, { k: 'add', t: 'int', label: 'Add' }],
  label: [{ k: 'name', label: 'Label' }],
  goto: [{ k: 'label', label: 'Label' }],
  branch: [{ k: 'goto', label: 'Jump to', hint: 'when the gate holds' }, { k: 'otherwise', t: 'otext', label: 'Else to' }, { k: 'counter', t: 'otext', label: 'Counter' }, { k: 'at_least', t: 'int', label: 'At least' }],
  start_track: [{ k: 'track', t: 'track', label: 'Track' }],
  join: [{ k: 'track', t: 'track', label: 'Track' }],
  lock: [],
  unlock: [],
  battle: [{ k: 'formation', t: 'ref', kind: 'formation', label: 'Formation' }],
  boss: [{ k: 'boss', t: 'ref', kind: 'boss', label: 'Boss' }],
  give_item: [{ k: 'item', t: 'itemid', label: 'Item' }, { k: 'count', t: 'int', label: 'Count' }],
  take_item: [{ k: 'item', t: 'itemid', label: 'Item' }, { k: 'count', t: 'int', label: 'Count' }],
  gold: [{ k: 'amount', t: 'int', label: 'Gold', hint: 'negative takes' }],
  give_card: [{ k: 'card', t: 'ref', kind: 'card', label: 'Card' }],
  start_quest: [{ k: 'quest', t: 'ref', kind: 'quest', label: 'Quest' }],
  sfx: [{ k: 'id', t: 'int', label: 'Sound effect id' }],
  bgm: [{ k: 'bgm', t: 'bgm', label: 'Music' }],
  sound: [{ k: 'sound', t: 'sound', label: 'Sound', hint: 'a retail cue or an audio.toml sound' }],
  music: [{ k: 'music', t: 'music', label: 'Music', hint: 'held over the map\'s zones until a music release' }, { k: 'fade', t: 'int', label: 'Fade (frames)' }],
  music_release: [],
  end: [],
};

// The fields a fresh step of `kind` starts with.
function makeStep(ctx, kind) {
  const s = { do: kind };
  const first = (k) => ctx.ids(k)[0] || '';
  for (const f of STEP_KINDS[kind]) {
    switch (f.t) {
      case 'int': s[f.k] = f.k === 'count' ? 1 : (f.k === 'frames' || f.k === 'fade' ? 30 : 0); break;
      case 'bool': s[f.k] = f.k === 'wait'; break;
      case 'ref': s[f.k] = first(f.kind); break;
      case 'actor': s[f.k] = 'player'; break;
      case 'npc': s[f.k] = ''; break;
      case 'tile': s[f.k] = [0, 0]; break;
      case 'tiles': s[f.k] = []; break;
      case 'tuple': s[f.k] = [255, 255, 255]; break;
      case 'csv': s[f.k] = []; break;
      case 'sel': s[f.k] = f.opts[0]; break;
      case 'track': s[f.k] = ''; break;
      case 'bgm': s[f.k] = { kind: 'bgm', id: 0 }; break;
      case 'sound': s[f.k] = { kind: 'sfx', id: 0x21 }; break;
      case 'music': s[f.k] = { kind: 'silence' }; break;
      case 'itemid': s[f.k] = 1; break;
      case 'text': s[f.k] = ''; break;
      default: break;
    }
  }
  if (kind === 'branch') s.requires = {};
  return s;
}

// The actors a cutscene can name: the player, its map's NPCs, or a retail slot.
function actorIds(ctx, cs) {
  const map = cs.map && ctx.pack.maps ? ctx.pack.maps[cs.map] : null;
  return ['player'].concat(map ? (map.npcs || []).map((n) => n.id).filter(Boolean) : []);
}

const tileText = (t) => (Array.isArray(t) ? t.join(',') : '');
const parseTile = (v) => {
  const p = v.split(',').map((x) => parseInt(x, 10));
  return p.length === 2 && p.every(Number.isFinite) ? p : null;
};

// One field of a step: the form kit's, or one of this module's pickers.
function stepField(ctx, cs, step, spec, path) {
  const p = joinPath(path, spec.k);
  const wrap = (input) => h('label', { class: 'xw-field', 'data-path': p }, h('span', {}, spec.label, spec.hint ? h('span', { class: 'xw-hint' }, ` ${spec.hint}`) : null), input);
  const set = (v, structural) => {
    if (v === undefined || v === null) step[spec.k] = null;
    else step[spec.k] = v;
    ctx.changed(structural);
  };
  const select = (opts, val, optional) => {
    const sel = h('select', {});
    if (optional) sel.append(h('option', { value: '' }, '(none)'));
    let found = false;
    for (const v of opts) {
      const o = h('option', { value: v }, v);
      if (v === val) { o.selected = true; found = true; }
      sel.append(o);
    }
    if (val && !found) sel.append(h('option', { value: val, selected: true }, `${val} (not on the map)`));
    sel.addEventListener('change', () => set(sel.value === '' ? null : sel.value));
    return sel;
  };
  switch (spec.t) {
    case 'actor': case 'oactor': {
      if (!cs.map) {
        const input = h('input', { type: 'text', value: step[spec.k] || '', placeholder: 'player or retail:<slot>' });
        input.addEventListener('input', () => set(input.value.trim() === '' && spec.t === 'oactor' ? null : input.value.trim()));
        return wrap(input);
      }
      return wrap(select(actorIds(ctx, cs), step[spec.k], spec.t === 'oactor'));
    }
    case 'npc': return wrap(select(actorIds(ctx, cs).filter((a) => a !== 'player'), step[spec.k], false));
    case 'track': return wrap(select(Object.keys(cs.tracks || {}), step[spec.k], false));
    case 'tile': case 'otile': {
      const input = h('input', { type: 'text', value: tileText(step[spec.k]), placeholder: 'c,r' });
      input.addEventListener('change', () => set(input.value.trim() ? parseTile(input.value.trim()) : (spec.t === 'otile' ? null : [0, 0])));
      return wrap(input);
    }
    case 'tiles': {
      // `to` folds into the path: the editor writes every stop as `path`.
      const stops = (step.path || []).concat(step.to ? [step.to] : []);
      const input = h('input', { type: 'text', value: stops.map(tileText).join(' '), placeholder: 'c,r c,r', spellcheck: 'false' });
      input.addEventListener('change', () => {
        step.path = input.value.trim().split(/\s+/).filter(Boolean).map(parseTile).filter(Boolean);
        step.to = null;
        ctx.changed();
      });
      return wrap(input);
    }
    case 'focus': {
      const f = step.focus;
      const mode = f == null ? '' : (f.actor != null ? 'actor' : 'tile');
      const kind = h('select', {}, h('option', { value: '' }, '(keep)'), h('option', { value: 'tile', selected: mode === 'tile' }, 'a tile'), h('option', { value: 'actor', selected: mode === 'actor' }, 'an actor'));
      kind.addEventListener('change', () => {
        step.focus = kind.value === 'tile' ? { tile: [0, 0] } : kind.value === 'actor' ? { actor: 'player' } : null;
        ctx.changed(true);
      });
      const box = h('div', { class: 'xw-row2' }, kind);
      if (mode === 'tile') {
        const t = h('input', { type: 'text', value: tileText(f.tile), placeholder: 'c,r' });
        t.addEventListener('change', () => { const v = parseTile(t.value.trim()); if (v) { f.tile = v; ctx.changed(); } });
        box.append(t);
      } else if (mode === 'actor') {
        const ids = actorIds(ctx, cs);
        const a = h('select', {}, ...ids.map((id) => h('option', { value: id, selected: id === f.actor }, id)));
        a.addEventListener('change', () => { f.actor = a.value; ctx.changed(); });
        box.append(a);
      }
      return wrap(box);
    }
    default:
      return field(ctx, step, spec, path);
  }
}

// A step list: one box per step, its kind switchable in place.
function stepsEd(ctx, parent, cs, owner, path, title) {
  if (!Array.isArray(owner.steps)) owner.steps = [];
  const steps = owner.steps;
  const sp = joinPath(path, 'steps');
  const wrap = h('div', { 'data-path': sp });
  wrap.append(h('h4', { class: 'xw-sub' }, title, h('span', { class: 'xw-hint' }, ` ${steps.length} step(s)`)));
  steps.forEach((step, i) => {
    const ip = joinPath(sp, i);
    const box = h('div', { class: 'xw-box xw-step', 'data-path': ip });
    const kind = h('select', { 'aria-label': `Step ${i + 1}` }, ...Object.keys(STEP_KINDS).map((k) => h('option', { value: k, selected: step.do === k }, k.replace('_', ' '))));
    kind.addEventListener('change', () => { steps[i] = makeStep(ctx, kind.value); ctx.changed(true); });
    const head = h('div', { class: 'xw-box-head' }, h('span', { class: 'xw-hint' }, `${i + 1}.`), kind, h('span', { class: 'xw-sp' }));
    if (i > 0) head.append(h('button', { type: 'button', class: 'wb-mini', title: 'Move up', onclick: () => { steps.splice(i - 1, 0, steps.splice(i, 1)[0]); ctx.changed(true); } }, '↑'));
    head.append(h('button', { type: 'button', class: 'wb-mini', title: 'Insert a step above', onclick: () => { steps.splice(i, 0, makeStep(ctx, 'wait')); ctx.changed(true); } }, '+ above'));
    head.append(h('button', { type: 'button', class: 'wb-mini', onclick: () => { steps.splice(i, 1); ctx.changed(true); } }, 'Remove'));
    box.append(head);
    const g = h('div', { class: 'xw-grid' });
    for (const spec of STEP_KINDS[step.do] || []) g.append(stepField(ctx, cs, step, spec, ip));
    box.append(g);
    if (step.do === 'branch') box.append(gateBox(ctx, step, 'requires', ip, 'The gate'));
    wrap.append(box);
  });
  const addKind = h('select', { 'aria-label': 'New step kind' }, ...Object.keys(STEP_KINDS).map((k) => h('option', { value: k }, k.replace('_', ' '))));
  wrap.append(h('div', { class: 'xw-inline' }, addKind, h('button', { type: 'button', class: 'wb-mini', onclick: () => { steps.push(makeStep(ctx, addKind.value)); ctx.changed(true); } }, '+ Step')));
  parent.append(wrap);
}

// Where the camera looks over a cutscene, from its camera steps and the
// frames the timeline put them at: keys `{ f, t }` (frame, map tile), the
// camera easing from one to the next over a step's `frames`. A step with no
// focus keeps the last; an actor focus is the actor's tile on the map (the
// player's is the first spawn); a reset returns to the player.
export function cameraKeys(cs, map, rows) {
  const spawn = (map.spawns && map.spawns[0] && map.spawns[0].tile) || [0, 0];
  const actorTile = (a) => {
    if (!a || a === 'player') return spawn;
    const n = (map.npcs || []).find((x) => x.id === a);
    return n ? n.tile : spawn;
  };
  let last = spawn;
  const keys = [{ f: 0, t: spawn }];
  for (const r of rows) {
    if (r.kind !== 'camera' && r.kind !== 'camera_reset') continue;
    const list = r.track ? ((cs.tracks || {})[r.track] || {}).steps : cs.steps;
    const st = list && list[r.step];
    if (!st) continue;
    let to = last;
    if (r.kind === 'camera_reset') to = spawn;
    else if (st.focus) to = st.focus.tile || actorTile(st.focus.actor);
    keys.push({ f: r.frame, t: last }, { f: r.frame + (st.frames || 0), t: to });
    last = to;
  }
  return keys;
}

// The camera's tile at frame `f` (fractional between keys).
export function cameraAt(keys, f) {
  let p = keys[0].t;
  for (let i = 1; i < keys.length; i++) {
    const a = keys[i - 1], b = keys[i];
    if (f <= a.f) break;
    if (f >= b.f) { p = b.t; continue; }
    const k = b.f > a.f ? (f - a.f) / (b.f - a.f) : 1;
    return [a.t[0] + (b.t[0] - a.t[0]) * k, a.t[1] + (b.t[1] - a.t[1]) * k];
  }
  return p;
}

// The timeline preview: the session plays the cutscene and lists its steps by frame.
function timeline(ctx, parent, id, cs) {
  const box = h('div', { class: 'xw-box xw-timeline' });
  const picks = h('input', { type: 'text', placeholder: 'e.g. 0, 1', spellcheck: 'false', 'aria-label': 'Options picked at each menu' });
  const out = h('div', {});
  const run = () => {
    out.innerHTML = '';
    const list = picks.value.split(',').map((x) => parseInt(x, 10)).filter(Number.isFinite);
    const res = ctx.cutsceneTimeline(id, list);
    if (!res || res.error) {
      out.append(h('div', { class: 'wb-status is-err' }, res ? res.error : 'no preview'));
      return;
    }
    const summary = `${res.ended ? 'Ends' : 'Still running'} at frame ${res.frames} (${(res.frames / 60).toFixed(1)} s)`
      + (res.fight ? `, then ${res.fight}` : '')
      + (res.flags.length ? `; raises ${res.flags.join(', ')}` : '');
    out.append(h('div', { class: res.ended ? 'wb-status is-ok' : 'wb-status is-err' }, summary));
    const table = h('table', { class: 'xw-table' }, h('thead', {}, h('tr', {}, h('th', {}, 'frame'), h('th', {}, 's'), h('th', {}, 'track'), h('th', {}, 'step'), h('th', {}, 'what'))));
    const body = h('tbody', {});
    for (const r of res.rows) {
      body.append(h('tr', {}, h('td', {}, String(r.frame)), h('td', {}, r.seconds.toFixed(1)), h('td', {}, r.track || 'main'), h('td', {}, `${r.step + 1}. ${r.kind}`), h('td', {}, r.text)));
    }
    table.append(body);
    out.append(table);
    // On its map in the 3D preview: the camera's path, scrubbed by frame.
    const map = cs && cs.map && ctx.pack.maps && ctx.pack.maps[cs.map];
    const pv = ctx.preview;
    if (!map || !pv) return;
    const keys = cameraKeys(cs, map, res.rows);
    const path = [];
    for (const k of keys) {
      const l = path[path.length - 1];
      if (!l || l[0] !== k.t[0] || l[1] !== k.t[1]) path.push(k.t);
    }
    const slider = h('input', { type: 'range', min: 0, max: Math.max(1, res.frames), value: 0, 'aria-label': 'Scrub the timeline' });
    const at = h('span', { class: 'xw-preview-num' }, 'frame 0');
    const scrub = () => {
      const f = parseInt(slider.value, 10) || 0;
      at.textContent = `frame ${f} (${(f / 60).toFixed(1)} s)`;
      // The step running at that frame.
      let cur = -1;
      res.rows.forEach((r, i) => { if (r.frame <= f) cur = i; });
      body.querySelectorAll('tr').forEach((tr, i) => tr.classList.toggle('is-now', i === cur));
      pv.setPath(path, cameraAt(keys, f));
    };
    slider.addEventListener('input', scrub);
    out.append(h('div', { class: 'xw-tools xw-scrub' },
      h('span', { class: 'wb-label' }, `On ${cs.map} in 3D`),
      h('button', { type: 'button', class: 'wb-mini', onclick: () => { pv.open(); pv.setMap(cs.map, ''); scrub(); } }, 'Show the camera path'),
      slider, at,
      h('button', { type: 'button', class: 'wb-mini', title: 'Play the cutscene in the 3D preview, through the engine', onclick: () => { pv.open(); pv.setMap(cs.map, ''); pv.playCutscene(id); } }, 'Play in 3D')));
  };
  box.append(
    h('div', { class: 'xw-box-head' }, 'Timeline preview', h('span', { class: 'xw-sp' }),
      h('label', { class: 'xw-field' }, h('span', {}, 'Menu picks', h('span', { class: 'xw-hint' }, ' option per menu, in order')), picks),
      h('button', { type: 'button', class: 'wb-mini', onclick: run }, 'Play')),
    h('p', { class: 'xw-note' }, 'The engine\'s own sequencer, against a stand-in world: walks take their distance over their pace, a line takes a reading time from its length. The frame of every step is the sequencer\'s; how long a walk or a line takes is an estimate.'),
    out,
  );
  parent.append(box);
  run();
}

export function renderCutscenes(ctx, el) {
  const pack = ctx.pack;
  pack.cutscenes = pack.cutscenes || { cutscenes: {} };
  el.append(h('p', { class: 'xw-note' }, 'cutscenes.toml: scripted scenes - steps a small sequencer runs in order (walk, turn, camera, fade, say, branch, fight), with named tracks running beside the main one. A map trigger (kind cutscene), a map\'s arrival list, an NPC or a quest moment plays one. Start skips a cutscene once it has been seen.'));
  splitEd(ctx, el, pack.cutscenes, 'cutscenes', '', {
    prefix: 'cutscene',
    add: 'Cutscene',
    idLabel: 'Cutscene id',
    make: () => ({ title: 'New cutscene', map: null, skippable: true, lock_player: true, letterbox: true, steps: [{ do: 'wait', frames: 30 }], tracks: {} }),
    label: (c, id) => `${id}${c.map ? ' @ ' + c.map : ''}`,
    render: (box, cs, p, id) => {
      box.append(grid(ctx, cs, [
        { k: 'title', label: 'Title' },
        { k: 'map', t: 'oref', kind: 'map', label: 'Plays on map', hint: 'its NPCs and tiles; empty = any scene' },
        { k: 'skippable', t: 'bool', label: 'Start skips it once seen' },
        { k: 'lock_player', t: 'bool', label: 'Hold the player' },
        { k: 'letterbox', t: 'bool', label: 'Letterbox' },
      ], p));
      stepsEd(ctx, box, cs, cs, p, 'Main track');
      cs.tracks = cs.tracks || {};
      for (const name of Object.keys(cs.tracks)) {
        const tp = joinPath(joinPath(p, 'tracks'), name);
        const tb = h('div', { class: 'xw-box', 'data-path': tp });
        tb.append(h('div', { class: 'xw-box-head' }, `Track "${name}"`, h('span', { class: 'xw-sp' }),
          h('button', { type: 'button', class: 'wb-mini', onclick: () => { delete cs.tracks[name]; ctx.changed(true); } }, 'Delete track')));
        stepsEd(ctx, tb, cs, cs.tracks[name], tp, 'Steps');
        box.append(tb);
      }
      const tn = h('input', { type: 'text', placeholder: 'track name', spellcheck: 'false', 'aria-label': 'New track name' });
      box.append(h('div', { class: 'xw-inline' }, tn, h('button', {
        type: 'button', class: 'wb-mini', onclick: () => {
          const n = tn.value.trim().replace(/[^a-z0-9_]/g, '_');
          if (!n || cs.tracks[n]) return;
          cs.tracks[n] = { steps: [] };
          ctx.changed(true);
        },
      }, '+ Track')));
      timeline(ctx, box, id, cs);
    },
  });
}
