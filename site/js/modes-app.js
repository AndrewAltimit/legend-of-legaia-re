/* modes-app.js - site/modes.html: the port's own game modes (Hold'em, Grid
 * Duel, golf, racing, prop hunt) played in the browser with no disc.
 *
 * The runtime is `LegaiaModes` (crates/web-minigames/src/modes_page.rs), a
 * World-free shell over the same mode seat the play hosts run
 * (`legaia_engine_expansion::modes::PackModeSeat`). This file only:
 *   - picks the rules (the built-in practice set, the site's Rebirth tree
 *     `packs/rebirth.json`, an installed pack from the play page's library,
 *     or a picked .lxpack) and lists the instances as cards;
 *   - steps the seat at 60 Hz off one pad word (keyboard through the
 *     engine's own binding table in pad-bindings.js, a controller, or the
 *     on-screen pad);
 *   - draws the seat's screen: `frame_json` hands flat quads and unmeasured
 *     labels in the 320x240 stage, drawn here on a 2D canvas.
 *
 * URL: `?rules=builtin|rebirth&mode=<instance id>` opens straight into a
 * mode. The chosen rules are remembered per browser (localStorage; every
 * access wrapped, the page works without it).
 *
 * Headless hooks: `window.__gm` = { rt, state(), enter(id), pad(mask, frames) }.
 *
 * Classic script. */
(function () {
  'use strict';

  const $ = (id) => document.getElementById(id);
  const STAGE_W = 320, STAGE_H = 240;
  const STEP_MS = 1000 / 60;
  const RULES_KEY = 'legaia-modes-rules';

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
      blurb: 'Checkpoint racing. Finish the laps first.',
      controls: [
        '{Cross}: accelerate, {Square}: brake',
        '{Left} / {Right}: steer, {R1}: boost',
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
  let stage = 'closed';
  let live = null;            /* the instance id in play */
  const held = new Set();     /* KeyboardEvent.codes + 'Gamepad:<Button>' */
  let touchMask = 0;
  let acc = 0, last = 0, raf = 0;

  const canvas = $('gm-canvas');
  const ctx = canvas.getContext('2d');

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

  async function rebirthTree() {
    const res = await fetch(new URL('packs/rebirth.json', document.baseURI).href, { cache: 'no-cache' });
    if (!res.ok) throw new Error(`could not fetch packs/rebirth.json (${res.status})`);
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
      if (!rec.bytes || rec.id === 'rebirth') continue;
      const opt = document.createElement('option');
      opt.value = 'lib:' + rec.id;
      opt.textContent = `${rec.name || rec.id} (installed)`;
      sel.appendChild(opt);
    }
  }

  async function useRules(value) {
    leave();
    let err = '';
    try {
      if (value === 'rebirth') {
        status('Opening Rebirth ...');
        const t = await rebirthTree();
        err = rt.load_pack_tree(t.paths, t.data);
      } else if (value && value.startsWith('lib:')) {
        const rec = (await installedPacks()).find((r) => 'lib:' + r.id === value);
        err = rec && rec.bytes ? rt.load_pack_bundle(new Uint8Array(rec.bytes)) : 'That pack is no longer installed.';
      } else {
        rt.use_builtin();
        value = 'builtin';
      }
    } catch (e) {
      err = String(e && e.message || e);
    }
    if (err) {
      rt.use_builtin();
      value = 'builtin';
    }
    $('gm-rules').value = value;
    store(RULES_KEY, value === 'rebirth' ? null : value);
    listModes();
    const info = JSON.parse(rt.modes_json());
    const from = info.source === 'pack' ? info.pack : 'the practice set';
    if (err) status(`${err} - showing the practice set.`, true);
    else status(`${info.modes.length} modes from ${from}. Pick one and press Play.`);
  }

  async function openBundle(file) {
    leave();
    const bytes = new Uint8Array(await file.arrayBuffer());
    const err = rt.load_pack_bundle(bytes);
    if (err) { status(`${file.name}: ${err}`, true); return; }
    const sel = $('gm-rules');
    let opt = sel.querySelector('option[value="file"]');
    if (!opt) { opt = document.createElement('option'); opt.value = 'file'; sel.appendChild(opt); }
    opt.textContent = `${file.name} (this visit)`;
    sel.value = 'file';
    listModes();
    const info = JSON.parse(rt.modes_json());
    status(`${info.modes.length} modes from ${info.pack}. Pick one and press Play.`);
  }

  /* ---------------- play in 3D ---------------- */

  /* Whether the site's disc cache holds a disc (js/rom-cache.js): the
   * framed play page boots off it. */
  let discReady = false;

  /* The pack id the play page opens these rules as: Rebirth (built in) or
   * a pack installed in the browser's library; '' for the practice set or
   * a picked file, which the play page cannot open. */
  function playLib() {
    const rules = $('gm-rules').value;
    return rules === 'rebirth' ? 'rebirth' : (rules.startsWith('lib:') ? rules.slice(4) : '');
  }

  function list3d() {
    const host = $('gm-3d-cards');
    const lib = playLib();
    const world = lib ? modes.filter((m) => m.map) : [];
    if (!world.length) {
      host.innerHTML = `<p class="gm-hint">${lib
        ? 'These rules lay no game out on a map.'
        : 'The practice set has no games in the world. Choose Rebirth (or an installed pack) as the rules below.'}</p>`;
      return;
    }
    host.innerHTML = world.map((m) => {
      const h = HOWTO[m.kind] || { blurb: '' };
      return `<div class="gm-card" data-id="${esc(m.id)}">
        <span class="gm-kind">${esc(m.kind)}</span>
        <h3>${esc(m.name)}</h3>
        <p>${esc(h.blurb)}</p>
        <p class="gm-map">At its place on <code>${esc(m.map)}</code>.</p>
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
    const lib = playLib();
    if (!m || !m.map || !lib || !discReady) return;
    leave();
    const frame = $('gm-3d-frame');
    const q = new URLSearchParams({ embed: '1', expansion: '1', lib, map: m.map, mode: m.id });
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

  function listModes() {
    modes = JSON.parse(rt.modes_json()).modes || [];
    list3d();
    const host = $('gm-cards');
    host.innerHTML = modes.map((m) => {
      const h = HOWTO[m.kind] || { blurb: '', controls: [] };
      const lines = h.controls.concat(['{Start}: pause and offer to leave']);
      return `<div class="gm-card" data-id="${esc(m.id)}">
        <span class="gm-kind">${esc(m.kind)}</span>
        <h3>${esc(m.name)}</h3>
        <p>${esc(h.blurb)}</p>
        <ul>${lines.map((l) => `<li>${controlLine(l)}</li>`).join('')}</ul>
        ${m.map ? `<p class="gm-map">Also in 3D above, at its place on <code>${esc(m.map)}</code>.</p>` : ''}
        <div class="gm-foot"><button class="gm-btn gm-primary" type="button" data-play="${esc(m.id)}">Play flat</button></div>
      </div>`;
    }).join('');
    host.querySelectorAll('[data-play]').forEach((b) => {
      b.addEventListener('click', () => enter(b.getAttribute('data-play')));
    });
  }

  /* ---------------- the seat ---------------- */

  function enter(id) {
    leave3d();
    const err = rt.enter(id);
    if (err) { status(err, true); return false; }
    live = id;
    held.clear();
    touchMask = 0;
    if (window.legaiaResetGamepad) window.legaiaResetGamepad();
    stage = 'playing';
    const m = modes.find((x) => x.id === id);
    $('gm-now').innerHTML = `<b>${esc(m ? m.name : id)}</b> &middot; ${esc(m ? m.kind : '')}`;
    $('gm-stage-wrap').hidden = false;
    document.querySelectorAll('.gm-card').forEach((c) => c.classList.toggle('is-live', c.getAttribute('data-id') === id));
    status(`Playing ${m ? m.name : id}. Start pauses; Cross on the pause screen leaves.`);
    canvas.focus({ preventScroll: true });
    $('gm-stage-wrap').scrollIntoView({ block: 'start', behavior: 'smooth' });
    start();
    return true;
  }

  function leave() {
    if (rt) rt.leave();
    stage = 'closed';
    live = null;
    $('gm-stage-wrap').hidden = true;
    document.querySelectorAll('.gm-card.is-live').forEach((c) => c.classList.remove('is-live'));
    if (raf) { cancelAnimationFrame(raf); raf = 0; }
  }

  function padWord() {
    const keys = window.legaiaPadMaskOf ? window.legaiaPadMaskOf(held) : 0;
    return (keys | touchMask) & 0xFFFF;
  }

  function step() {
    stage = rt.tick(padWord());
    if (stage === 'closed') {
      const was = live;
      leave();
      const m = modes.find((x) => x.id === was);
      status(m ? `Left ${m.name}. Pick another mode, or play it again.` : 'Pick a mode.');
    }
  }

  function start() {
    if (raf) return;
    last = performance.now();
    acc = 0;
    const frame = (now) => {
      raf = 0;
      if (stage === 'closed') return;
      if (window.legaiaPollGamepad) window.legaiaPollGamepad(held, null);
      acc += Math.min(now - last, 250);
      last = now;
      while (acc >= STEP_MS && stage !== 'closed') { step(); acc -= STEP_MS; }
      if (stage === 'closed') return;
      draw();
      raf = requestAnimationFrame(frame);
    };
    raf = requestAnimationFrame(frame);
  }

  /* ---------------- drawing ---------------- */

  function rgba(r, g, b, a) { return `rgba(${r},${g},${b},${(a / 255).toFixed(3)})`; }

  /* A label held to its box (`max_w` stage px, 0 = none), cut and marked
   * `..` the way the engine's `fit_text` cuts it in the dialog font. */
  function fitLabel(text, maxW) {
    if (!maxW || ctx.measureText(text).width <= maxW) return text;
    if (ctx.measureText('..').width > maxW) return '';
    for (let n = text.length - 1; n > 0; n--) {
      const head = text.slice(0, n).trimEnd() + '..';
      if (ctx.measureText(head).width <= maxW) return head;
    }
    return '..';
  }

  function draw() {
    const f = JSON.parse(rt.frame_json());
    const sx = canvas.width / STAGE_W, sy = canvas.height / STAGE_H;
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.globalAlpha = 1;
    ctx.fillStyle = '#000';
    ctx.fillRect(0, 0, canvas.width, canvas.height);
    ctx.setTransform(sx, 0, 0, sy, 0, 0);
    for (const p of f.prims) {
      /* POLY_F4 order: TL, TR, BL, BR -> path TL, TR, BR, BL. */
      ctx.globalAlpha = p[12] ? 0.5 : 1;
      ctx.fillStyle = `rgb(${p[8]},${p[9]},${p[10]})`;
      ctx.beginPath();
      ctx.moveTo(p[0], p[1]);
      ctx.lineTo(p[2], p[3]);
      ctx.lineTo(p[6], p[7]);
      ctx.lineTo(p[4], p[5]);
      ctx.closePath();
      ctx.fill();
    }
    ctx.globalAlpha = 1;
    /* Labels: the dialog font is disc data, so a web font stands in at the
     * dialog font's size (a 12px line; LINE = 14 stage px between rows). */
    ctx.font = '600 10px "Trebuchet MS", "Segoe UI", system-ui, sans-serif';
    ctx.textBaseline = 'top';
    for (const l of f.labels) {
      ctx.textAlign = l[7] === 1 ? 'center' : l[7] === 2 ? 'right' : 'left';
      ctx.fillStyle = rgba(l[3], l[4], l[5], l[6]);
      ctx.fillText(fitLabel(l[0], l[8]), l[1], l[2] + 1.5);
    }
  }

  /* ---------------- input ---------------- */

  function playing() { return stage !== 'closed'; }

  window.addEventListener('keydown', (e) => {
    if (!playing()) return;
    const t = e.target;
    if (t && (t.tagName === 'INPUT' || t.tagName === 'SELECT' || t.tagName === 'TEXTAREA')) return;
    if (window.legaiaPadSwallows && window.legaiaPadSwallows(e.code)) e.preventDefault();
    held.add(e.code);
  });
  window.addEventListener('keyup', (e) => { held.delete(e.code); });
  window.addEventListener('blur', () => {
    held.clear();
    touchMask = 0;
    if (window.legaiaResetGamepad) window.legaiaResetGamepad();
  });

  function wirePad() {
    $('gm-pad').querySelectorAll('button[data-btn]').forEach((b) => {
      const name = b.getAttribute('data-btn');
      const down = (e) => {
        e.preventDefault();
        touchMask |= window.legaiaPadButton ? window.legaiaPadButton(name) : 0;
        b.classList.add('is-down');
        try { b.setPointerCapture(e.pointerId); } catch (err) { /* old browser */ }
      };
      const up = () => {
        touchMask &= ~(window.legaiaPadButton ? window.legaiaPadButton(name) : 0);
        b.classList.remove('is-down');
      };
      b.addEventListener('pointerdown', down);
      b.addEventListener('pointerup', up);
      b.addEventListener('pointercancel', up);
      b.addEventListener('lostpointercapture', up);
    });
  }

  /* ---------------- boot ---------------- */

  async function boot() {
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
    rt.set_seed(Date.now() % 4294967296);
    $('gm-keys').innerHTML = `Keyboard: arrows move, ${button('Cross')}, ${button('Circle')}, `
      + `${button('Square')}, ${button('Triangle')}, ${button('R1')}, ${button('Start')}. A controller works too.`;
    wirePad();
    wireDisc();
    await addInstalledOptions();
    const q = new URLSearchParams(location.search);
    /* Rebirth by default: its games are the ones laid out in the world,
     * which the page leads with. */
    let rules = q.get('rules') || recall(RULES_KEY) || 'rebirth';
    if (![...$('gm-rules').options].some((o) => o.value === rules)) rules = 'builtin';
    await useRules(rules);
    const want = q.get('mode');
    if (want && modes.some((m) => m.id === want)) enter(want);

    $('gm-rules').addEventListener('change', (e) => useRules(e.target.value));
    $('gm-pack-file').addEventListener('change', (e) => {
      const f = e.target.files && e.target.files[0];
      if (f) openBundle(f);
      e.target.value = '';
    });
    $('gm-leave').addEventListener('click', () => {
      const m = modes.find((x) => x.id === live);
      leave();
      status(m ? `Left ${m.name}.` : 'Pick a mode.');
    });

    window.__gm = {
      rt,
      state: () => JSON.parse(rt.state_json()),
      modes: () => modes.slice(),
      enter,
      /* Hold `mask` for `frames` frames, then release for one. */
      pad(mask, frames) {
        for (let i = 0; i < (frames || 1); i++) { touchMask = mask; step(); }
        touchMask = 0; if (playing()) step();
        if (playing()) draw();
        return stage;
      },
    };
  }

  boot();
})();
