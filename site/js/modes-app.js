/* modes-app.js - site/modes.html: the port's own game modes (Hold'em, Grid
 * Duel, golf, racing) that an expansion pack lays out in the world, played
 * in 3D from the visitor's disc.
 *
 * A game plays in the play page's runtime framed here (`play.html?embed=1
 * &expansion=1&lib=<pack>&map|scene=<where>&mode=<instance id>`), which boots
 * off the site's disc cache (js/rom-cache.js). This file only:
 *   - picks the rules (the site's sample-pack tree `packs/sample.json`, or a
 *     pack installed in the play page's library) and lists the instances
 *     laid out on a map or an overworld as cards, each with its capture
 *     from the native port when the page has one;
 *   - opens and closes the frame.
 *
 * The instance list comes from `LegaiaModes::modes_json`
 * (crates/web-minigames/src/modes_page.rs), the pack loader the engine's
 * own mode seat reads, so the cards name exactly what the frame can seat.
 *
 * URL: `?rules=sample|lib:<id>`. The chosen rules are remembered per
 * browser (localStorage; every access wrapped, the page works without it).
 *
 * Headless hooks: `window.__gm` = { rt, modes(), enter3d(id), leave3d() }.
 *
 * Classic script. */
(function () {
  'use strict';

  const $ = (id) => document.getElementById(id);
  const RULES_KEY = 'legaia-modes-rules';

  /* Captures from the native port (`legaia-engine play-window --expansion
   * packs/sample --map|--scene <where> --mode <id> --screenshot`), keyed
   * by instance id. An instance with none shows its card without a picture. */
  const SHOTS = {
    'mist-lantern-table': ['img/tiles/modes-main.jpg', "Texas Hold'em at the Mist Lantern Table: islanders seated around the felt with cards and chips"],
    'tower-high-table': ['img/tiles/modes-tower-high-table.jpg', "Hold'em at the Tower High Table: five rivals around the felt, the bet panel open on Call"],
    'haven-duel': ['img/tiles/modes-side2.jpg', 'The Village Duel: a Grid Duel board on a table outdoors, a hand of five cards in front'],
    'wardens-duel': ['img/tiles/modes-wardens-duel.jpg', "The Warden's Duel: the Grid Duel board on the tower table, the Warden across it and a hand of cards fanned out"],
    'tower-green': ['img/tiles/modes-tower-green.jpg', 'Tower Green: a mini-golf hole of green tiles in the tower, Vahn at the tee and the flag beyond a stair'],
    'drake-circuit': ['img/tiles/modes-side1.jpg', 'Drake Dash: racers at the start gate on the overworld, counting down'],
    'spiral-circuit': ['img/tiles/modes-spiral-circuit.jpg', 'The Monastery Run: Vahn at a striped gate on the overworld, six racers on the standings panel'],
    'sol-ring': ['img/tiles/modes-sol-ring.jpg', 'Sol Ring: a striped track through the tower floor, chevrons marking the lap and three racers on the board'],
  };

  /* How to play each kind: the controls from the engine's mode table
   * (docs/subsystems/expansion-modes.md#controls), button names resolved to
   * keys at render time. `{Cross}` is a button token. */
  const HOWTO = {
    'Poker': {
      blurb: "No-limit Texas Hold'em against AI seats. Last stack standing wins.",
      controls: [
        '{Left} / {Right}: Fold, Check-Call, Raise or All-in',
        '{Up} / {Down}: raise size, in big blinds',
        '{Cross}: act, or skip the showdown',
      ],
    },
    'Grid Duel': {
      blurb: 'A 3x3 card battle. Place a card next to a weaker side to flip it; most cards on the board wins.',
      controls: [
        '{Left} / {Right}: choose a hand card',
        '{Cross}: lift it, D-pad to a cell, {Cross} to place',
        '{Circle}: put it back',
      ],
    },
    'Golf': {
      blurb: 'Mini-golf against an AI golfer. Fewest strokes over the holes wins.',
      controls: [
        '{Left} / {Right}: aim (hold {Square} for fine steps)',
        '{Cross}: start the power meter, {Cross} again to strike',
        '{Circle}: back to aiming',
      ],
    },
    'Race': {
      blurb: 'Checkpoint racing: laps of a circuit, or a sprint through gates across the overworld. Run over power-ups to carry one.',
      controls: [
        '{Cross}: accelerate, {Square}: brake',
        '{Left} / {Right}: steer, {R1}: boost',
        '{Triangle}: use the power-up you carry (boost, hop, shield, slow)',
      ],
    },
    'Prop Hunt': {
      blurb: 'Hiders pose as props; hunters search. Choose a side, then play the round out.',
      controls: [
        'Choose with {Left} / {Right} and {Cross}',
        'D-pad moves; a hunter inspects with {Cross}, a hider taunts with {Triangle}',
      ],
    },
  };

  let mod = null;
  let rt = null;
  let modes = [];

  function status(text, bad) {
    const el = $('gm-status');
    el.textContent = text;
    el.classList.toggle('is-bad', !!bad);
  }

  function store(key, val) {
    try {
      if (val == null) localStorage.removeItem(key); else localStorage.setItem(key, val);
    } catch (e) { /* storage blocked: nothing remembered */ }
  }
  function recall(key) {
    try { return localStorage.getItem(key); } catch (e) { return null; }
  }

  function esc(s) {
    return String(s).replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));
  }

  function keyName(code) {
    return code.replace(/^Key/, '').replace(/^Digit/, '').replace(/^Arrow/, '')
      .replace(/^Shift(Left|Right)$/, 'Shift').replace(/^Enter$/, 'Enter');
  }

  /* `Cross` -> "Cross (Z)", the first bound key from the engine's table. */
  function button(name) {
    const keys = window.legaiaPadKeysFor ? window.legaiaPadKeysFor(name) : [];
    const dir = /^(Up|Down|Left|Right)$/.test(name); // a D-pad label prefers an arrow key
    const k = keys.find((c) => c.startsWith('Arrow') === dir) || keys[0];
    return k ? `${esc(name)} <kbd class="gm-key">${esc(keyName(k))}</kbd>` : esc(name);
  }
  function controlLine(text) {
    return esc(text).replace(/\{(\w+)\}/g, (_, b) => button(b));
  }

  /* ---------------- rules ---------------- */

  async function samplePackTree() {
    const res = await fetch(new URL('packs/sample.json', document.baseURI).href, { cache: 'no-cache' });
    if (!res.ok) throw new Error(`could not fetch packs/sample.json (${res.status})`);
    const doc = await res.json();
    const enc = new TextEncoder();
    const paths = [], data = [];
    for (const f of doc.files || []) {
      paths.push(f.path);
      data.push(f.text != null ? enc.encode(f.text) : Uint8Array.from(atob(f.base64), (c) => c.charCodeAt(0)));
    }
    return { paths, data };
  }

  async function installedPacks() {
    const lib = window.LegaiaPackLibrary;
    if (!lib || typeof lib.list !== 'function') return [];
    try { return (await lib.list()) || []; } catch (e) { return []; }
  }

  async function addInstalledOptions() {
    const sel = $('gm-rules');
    for (const rec of await installedPacks()) {
      if (!rec.bytes || rec.id === 'sample') continue;
      const opt = document.createElement('option');
      opt.value = 'lib:' + rec.id;
      opt.textContent = `${rec.name || rec.id} (installed)`;
      sel.appendChild(opt);
    }
  }

  async function useRules(value) {
    leave3d();
    let err = '';
    try {
      if (value && value.startsWith('lib:')) {
        const rec = (await installedPacks()).find((r) => 'lib:' + r.id === value);
        err = rec && rec.bytes ? rt.load_pack_bundle(new Uint8Array(rec.bytes)) : 'That pack is no longer installed.';
      } else {
        value = 'sample';
        status('Opening the sample pack ...');
        const t = await samplePackTree();
        err = rt.load_pack_tree(t.paths, t.data);
      }
    } catch (e) {
      err = String(e && e.message || e);
    }
    $('gm-rules').value = value;
    store(RULES_KEY, value === 'sample' ? null : value);
    if (err) {
      modes = [];
      list3d();
      status(err, true);
      return;
    }
    const info = JSON.parse(rt.modes_json());
    modes = info.modes || [];
    list3d();
    const n = modes.filter((m) => m.map || m.scene).length;
    status(`${n} games laid out in the world, from ${info.pack}.`);
  }

  /* ---------------- play in 3D ---------------- */

  /* Whether the site's disc cache holds a disc (js/rom-cache.js): the
   * framed play page boots off it. */
  let discReady = false;

  /* The pack id the play page opens these rules as: the sample pack (built in) or
   * a pack installed in the browser's library. */
  function playLib() {
    const rules = $('gm-rules').value;
    return rules.startsWith('lib:') ? rules.slice(4) : 'sample';
  }

  function list3d() {
    const host = $('gm-3d-cards');
    if (!rt) return; /* the engine is still loading */
    const world = modes.filter((m) => m.map || m.scene);
    if (!world.length) {
      host.innerHTML = '<p class="gm-hint">These rules lay no game out in the world.</p>';
      return;
    }
    host.innerHTML = world.map((m) => {
      const h = HOWTO[m.kind] || { blurb: '', controls: [] };
      const shot = SHOTS[m.id];
      const lines = h.controls.concat(['{Start}: pause and offer to leave']);
      return `<div class="gm-card" data-id="${esc(m.id)}">
        ${shot ? `<div class="gm-shot"><img src="${esc(shot[0])}" alt="${esc(shot[1])}" loading="lazy"></div>` : ''}
        <span class="gm-kind">${esc(m.kind)}</span>
        <h3>${esc(m.name)}</h3>
        <p>${esc(h.blurb)}</p>
        <ul>${lines.map((l) => `<li>${controlLine(l)}</li>`).join('')}</ul>
        <p class="gm-map">${m.map ? `At its place on <code>${esc(m.map)}</code>.` : `Across the overworld, <code>${esc(m.scene)}</code>.`}</p>
        <div class="gm-foot"><button class="gm-btn gm-primary" type="button" data-play3d="${esc(m.id)}"
          ${discReady ? '' : 'disabled title="Load your disc above first"'}>Play in 3D</button></div>
      </div>`;
    }).join('');
    host.querySelectorAll('[data-play3d]').forEach((b) => {
      b.addEventListener('click', () => enter3d(b.dataset.play3d));
    });
  }

  function enter3d(id) {
    const m = modes.find((x) => x.id === id);
    if (!m || !(m.map || m.scene) || !discReady) return;
    leave3d();
    const frame = $('gm-3d-frame');
    const where = m.map ? { map: m.map } : { scene: m.scene };
    const q = new URLSearchParams({ embed: '1', expansion: '1', lib: playLib(), ...where, mode: m.id });
    frame.src = 'play.html?' + q.toString();
    frame.hidden = false;
    $('gm-3d-bar').hidden = false;
    $('gm-3d-now').innerHTML = `Playing <b>${esc(m.name)}</b> in 3D. Click the picture for the keys; Start asks to leave.`;
    document.querySelectorAll('#gm-3d-cards .gm-card').forEach((c) => c.classList.toggle('is-live', c.dataset.id === id));
    frame.addEventListener('load', () => { try { frame.focus(); } catch (e) {} }, { once: true });
    frame.scrollIntoView({ behavior: 'smooth', block: 'center' });
  }

  /* Closing the frame frees its engine and its copy of the disc. */
  function leave3d() {
    const frame = $('gm-3d-frame');
    if (frame.getAttribute('src')) frame.removeAttribute('src');
    frame.hidden = true;
    $('gm-3d-bar').hidden = true;
    document.querySelectorAll('#gm-3d-cards .gm-card').forEach((c) => c.classList.remove('is-live'));
  }

  function wireDisc() {
    $('gm-3d-leave').addEventListener('click', leave3d);
    if (!window.RomCache) return;
    window.RomCache.attach($('gm-file'), {
      onLoad: (src) => {
        discReady = true;
        $('gm-disc-status').textContent = `Disc ready: ${src && src.name ? src.name : 'your image'}. Pick a game below to play it in 3D.`;
        list3d();
      },
    });
  }

  /* ---------------- boot ---------------- */

  async function boot() {
    wireDisc();
    try {
      mod = await LegaiaWasm.load();
    } catch (e) {
      status('Could not load the engine (WebAssembly).', true);
      return;
    }
    if (typeof mod.LegaiaModes !== 'function') {
      status('This build of the site has no game modes yet.', true);
      return;
    }
    if (window.legaiaAdoptPadBindings) window.legaiaAdoptPadBindings(mod);
    rt = new mod.LegaiaModes();
    await addInstalledOptions();
    const q = new URLSearchParams(location.search);
    let rules = q.get('rules') || recall(RULES_KEY) || 'sample';
    if (rules === 'rebirth') rules = 'sample'; /* the sample pack's old id */
    if (![...$('gm-rules').options].some((o) => o.value === rules)) rules = 'sample';
    await useRules(rules);
    $('gm-rules').addEventListener('change', (e) => useRules(e.target.value));

    window.__gm = {
      rt,
      modes: () => modes.slice(),
      enter3d,
      leave3d,
    };
  }

  boot();
})();
