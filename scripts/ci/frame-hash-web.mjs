#!/usr/bin/env node
// Browser frame-hash oracle: does this bundle draw what that bundle draws?
//
//   scripts/ci/frame-hash-web.mjs --site site --out new.txt          # hash one site
//   scripts/ci/frame-hash-web.mjs --compare base.txt new.txt         # line for line
//   scripts/ci/frame-hash-web.mjs --site new/site --base-site old/site --out dir
//       [--scene town01,world_map,battle,cast] [--mode retail,enhanced] [--frames n]
//       [--gl swiftshader|egl] [--no-msaa] [--shots dir] [--counts out.json]
//
// Each scene runs in each mode on a fresh page, stopped off its animation loop and
// stepped one sim tick at a time through the page's own step path (`PlayView.step` then
// `_frame`) on a pad script keyed by frame number. After each step it reads the finished
// frame back and writes one line: the GL canvas, the 2D overlay canvas and the engine's
// `state_json`, each hashed, then how lit and how varied the frame is. The hashes say
// whether two runs agree and the last two columns say whether agreeing means anything: a
// scene whose frames are blank or all alike fails the run.
//
// The page's clock is virtual for the stepped stretch (`performance.now` advances 1/60 s
// a step), so anything on the page that reads it steps with the frames; `--real-clock` leaves
// it alone. See docs/tooling/frame-hash.md, which also lists what else was checked.
//
// A hardware GPU's MSAA resolve is not repeatable: the same frame on the same driver comes
// back now and then with a few pixels off by a little, so the default is SwiftShader, which
// runs the page's context as it stands and is repeatable. `--gl egl --no-msaa` is the fast
// form on the real GPU; plain `--gl egl` says it cannot be trusted.
//
// Hashes differ between GPU drivers. Compare runs from one machine and never commit one.
// A readback changes GPU timing, so this is never the timing run: frame-bench-web.mjs is.
//
// Self-checks of the oracle itself:
//   --poke scene/mode:frame  clear one pixel of that frame before reading it
//   --jitter seed            sleep between steps, so audio and disc reads move against ticks
//   --idle ms                wait at the boot prompt, so the page's prewarm lands first
//   --no-fill-wait           pick the scene without waiting for the disc's fill
//   --uncheck id,id          clear those option boxes first, to narrow a difference down
//   --no-msaa                ask for a context without MSAA, the same
//
// When two runs differ: `--shots dir --shot-every n` writes PNGs, `--raw dir` every frame's
// raw RGBA after the run, with no wait between frames. Both are Sony-derived: scratch only.
//
// `--counts out.json` skips the hashes and counts instead, over the same stepped frames:
// GL draws and every wasm export's returned bytes. cost-ratchet.py reads it.

import crypto from 'crypto';
import fs from 'fs';
import path from 'path';
import { fileURLToPath } from 'url';
import { launchChromium, rendererString, serveSite } from './browser-session.mjs';

const argv = process.argv;
const arg = (name, dflt) => {
  const i = argv.indexOf('--' + name);
  return i < 0 ? dflt : (argv[i + 1] && !argv[i + 1].startsWith('--') ? argv[i + 1] : true);
};
const REPO = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');

/* Pads are `[from, to, key]`, frames inclusive; a tap is one frame long. Each script
 * walks, so the frames differ, and the fights press confirm so a round runs. */
const walk = [[30, 110, 'ArrowDown'], [120, 200, 'ArrowUp'], [210, 290, 'ArrowLeft']];
const SCENES = {
  town01: { scene: 'town01', frames: 300, pad: walk },
  world_map: { scene: 'map01', frames: 300, pad: walk },
  battle: { scene: 'map01', frames: 600, force: true, settle: 10, tapFrom: 300, tapEvery: 40 },
  cast: { scene: 'map01', frames: 900, force: true, settle: 10, cast: [0, 138, 3] },
};
const MODES = {
  /* The page's defaults: a fresh browser profile, every enhancement at its default. */
  enhanced: [],
  /* The page's own Retail-faithful box and PSX rasterisation, the native --retail --psx-render. */
  retail: ['play-retail-mode', 'play-psx-render'],
  /* Asked for by name. PSX rasterisation under every pass that reads the screen inside the
   * frame (ambient occlusion, bloom's scene copy, the fog's depth copy). */
  'psx-enhanced': ['play-psx-render', 'play-bloom', 'play-ground-fog'],
};

/* Runs before the page's scripts. */
function instrument(cfg) {
  const B = (window.__hash = { virtual: false, vt: 0, draws: 0, glCalls: 0, exports: {}, on: false });
  const real = performance.now.bind(performance);
  if (!cfg.realClock) performance.now = () => (B.virtual ? B.vt : real());
  if (cfg.noMsaa) {
    const get = HTMLCanvasElement.prototype.getContext;
    HTMLCanvasElement.prototype.getContext = function (kind, attrs) {
      return get.call(this, kind, /^webgl/.test(kind) ? { ...attrs, antialias: false } : attrs);
    };
  }
  if (!cfg.counts) return;
  const wrap = (proto) => {
    for (const k of Object.getOwnPropertyNames(proto)) {
      const d = Object.getOwnPropertyDescriptor(proto, k);
      if (!d || typeof d.value !== 'function') continue;
      const draw = /^(drawArrays|drawElements|drawRangeElements|multiDraw)/.test(k);
      const f = d.value;
      proto[k] = function (...a) {
        if (B.on) { B.glCalls++; if (draw) B.draws++; }
        return f.apply(this, a);
      };
    }
  };
  if (window.WebGL2RenderingContext) wrap(WebGL2RenderingContext.prototype);
  B.wrapRuntime = (rt) => {
    const proto = Object.getPrototypeOf(rt);
    for (const k of Object.getOwnPropertyNames(proto)) {
      const d = Object.getOwnPropertyDescriptor(proto, k);
      if (k === 'constructor' || !d || typeof d.value !== 'function') continue;
      const f = d.value;
      proto[k] = function (...a) {
        const r = f.apply(this, a);
        if (B.on) {
          const e = (B.exports[k] = B.exports[k] || { calls: 0, bytes: 0 });
          e.calls++;
          if (typeof r === 'string') e.bytes += r.length;
          else if (r && r.byteLength !== undefined) e.bytes += r.byteLength;
        }
        return r;
      };
    }
  };
}

/* The stepped stretch, in the page. */
async function stepRun(cfg) {
  const B = window.__hash;
  const rt = window.__playRuntime;
  const sc = cfg.sc;
  if (cfg.counts) B.wrapRuntime(rt);
  const btn = document.querySelector(`[data-scene="${sc.scene}"]`);
  if (!btn) throw new Error('no scene button ' + sc.scene);
  for (const id of cfg.boxes) {
    const el = document.getElementById(id);
    if (!el) throw new Error('no option box ' + id);
    if (!el.checked) el.click();
  }
  for (const id of cfg.uncheck || []) {
    const el = document.getElementById(id);
    if (!el) throw new Error('no option box ' + id);
    if (el.checked) el.click();
  }
  /* Entry and the stop share a task, so no animation frame runs between them. */
  B.vt = 1e6;
  B.virtual = true;
  btn.click();
  const view = window.__playView;
  view.stop();
  view.setPaused(true);

  const words32 = (a, b) => { /* FNV-1a and a second mix over 32-bit words: one word changed always shows in the first. */
    let h1 = 0x811c9dc5 | 0, h2 = 0x9747b28c | 0;
    for (let i = 0; i < a.length; i++) {
      const w = a[i];
      h1 = Math.imul(h1 ^ w, 0x01000193);
      h2 = Math.imul(h2 ^ w, 0x5bd1e995);
      h2 ^= h2 >>> 13;
    }
    const hex = (x) => (x >>> 0).toString(16).padStart(8, '0');
    return b ? hex(h1) + hex(h2) : hex(h1);
  };
  const stringHash = (s) => {
    const a = new Uint32Array(s.length);
    for (let i = 0; i < s.length; i++) a[i] = s.charCodeAt(i);
    return words32(a, true);
  };
  const send = (type, code) => window.dispatchEvent(new KeyboardEvent(type, { code, key: code, bubbles: true }));
  const padAt = (n) => {
    for (const [from, to, key] of sc.pad || []) {
      if (n === from) send('keydown', key);
      if (n === to + 1) send('keyup', key);
    }
    if (sc.tapEvery && n >= sc.tapFrom) {
      const at = (n - sc.tapFrom) % sc.tapEvery;
      if (at === 0) send('keydown', 'KeyZ');
      if (at === 1) send('keyup', 'KeyZ');
    }
  };
  const lines = [];
  const raws = [];
  let rand = cfg.jitter | 0;
  const nextRand = () => { rand = Math.imul(rand ^ 0x5bd1e995, 0x01000193) + 12345 | 0; return (rand >>> 8) & 0xFFFF; };
  const positions = new Set();
  let battleFrame = null;
  for (let n = 0; n < cfg.frames; n++) {
    if (n === 0) B.firstState = rt.state_json();
    if (sc.force && n === sc.settle) {
      if (sc.cast) rt.debug_seed_inflight_cast(...sc.cast);
      rt.debug_force_battle(-1);
    }
    padAt(n);
    B.vt += 1000 / 60;
    /* Counts cover the frame alone: the page's timers (the card's write-back, the XA
     * banks landing) fire whenever they fire, between frames. */
    B.on = !!cfg.counts;
    view.step();
    view._frame();
    B.on = false;
    if (view._battle && battleFrame === null) battleFrame = n;
    const st = rt.state_json();
    try { const p = JSON.parse(st).player; positions.add(p.x + ',' + p.z); } catch (e) { /* no player */ }
    if (!cfg.counts) {
      const gl = view.renderer.gl;
      const w = gl.drawingBufferWidth, h = gl.drawingBufferHeight;
      const prev = gl.getParameter(gl.FRAMEBUFFER_BINDING);
      gl.bindFramebuffer(gl.FRAMEBUFFER, null);
      if (cfg.poke && cfg.poke === n) {
        /* The self-check: one pixel of what the frame drew, changed before the read. */
        const was = new Uint8Array(4);
        gl.readPixels(w >> 1, h >> 1, 1, 1, gl.RGBA, gl.UNSIGNED_BYTE, was);
        gl.enable(gl.SCISSOR_TEST);
        gl.scissor(w >> 1, h >> 1, 1, 1);
        gl.clearColor(((was[0] + 1) & 255) / 255, was[1] / 255, was[2] / 255, 1);
        gl.clear(gl.COLOR_BUFFER_BIT);
        gl.disable(gl.SCISSOR_TEST);
      }
      const px = new Uint8Array(w * h * 4);
      gl.readPixels(0, 0, w, h, gl.RGBA, gl.UNSIGNED_BYTE, px);
      gl.bindFramebuffer(gl.FRAMEBUFFER, prev);
      const words = new Uint32Array(px.buffer);
      /* How much the frame is showing, off a sample of it. */
      const colours = new Set();
      let lit = 0, seen = 0;
      for (let i = 0; i < words.length; i += 17) {
        const c = words[i] & 0xFFFFFF;
        seen++;
        if (c) lit++;
        if (colours.size < 4096) colours.add(c);
      }
      let ov = '-';
      const o = view.menuOverlay;
      if (o) {
        const d = o.getContext('2d').getImageData(0, 0, o.width, o.height).data;
        ov = words32(new Uint32Array(d.buffer, d.byteOffset, d.byteLength >> 2), true);
      }
      lines.push(`${cfg.label} ${String(n).padStart(4, '0')} gl=${words32(words, true)} ov=${ov} st=${stringHash(st)} lit=${(lit / seen).toFixed(3)} col=${colours.size}`);
      if (cfg.shotEvery && n % cfg.shotEvery === 0) await window.__hashShot(view.canvas.toDataURL('image/png'), n);
      if (cfg.raw) raws.push(px);
    }
    /* A step in a task of its own now and then: the audio pump and the disc's reads run
     * where a player's would, between frames. */
    if (cfg.jitter) {
      if (nextRand() % 3 === 0) await new Promise((res) => setTimeout(res, nextRand() % 30));
    } else if (n % 20 === 19) {
      await new Promise((res) => setTimeout(res, 0));
    }
  }
  for (const [, , key] of sc.pad || []) send('keyup', key);
  /* Written after the run: a screenshot's wait between frames can hide what a run without one shows. */
  for (let n = 0; n < raws.length; n++) {
    let bin = '';
    const px = raws[n];
    for (let i = 0; i < px.length; i += 0x8000) bin += String.fromCharCode.apply(null, px.subarray(i, i + 0x8000));
    await window.__hashRaw(btoa(bin), n);
  }
  return { lines, positions: positions.size, battleFrame, frames: cfg.frames,
    canvas: [view.canvas.width, view.canvas.height], draws: B.draws, glCalls: B.glCalls, exports: B.exports,
    audio: rt.audio_stats_json() };
}

async function hashSite(site, opts) {
  const { port, stop } = await serveSite(site);
  const browser = await launchChromium({ gl: opts.gl, vsync: false });
  const header = [`# site ${site}`];
  const wasm = path.join(site, 'wasm', 'legaia_web_viewer_bg.wasm');
  header.push(`# wasm sha256 ${crypto.createHash('sha256').update(fs.readFileSync(wasm)).digest('hex')}`);
  const stampPath = path.join(site, 'wasm', 'SOURCE_STAMP.json');
  if (fs.existsSync(stampPath)) {
    const stamp = JSON.parse(fs.readFileSync(stampPath));
    header.push(`# wasm profile ${stamp.profile} source ${String(stamp.source_stamp).slice(0, 16)}`);
  }
  header.push(`# gl ${opts.gl}${opts.noMsaa ? ', no MSAA' : ''}`);
  header.push(`# clock ${opts.realClock ? 'real' : 'virtual'}${opts.jitter ? `, jitter ${opts.jitter}` : ''}${opts.idle ? `, idle ${opts.idle}` : ''}${opts.noFillWait ? ', no fill wait' : ''}`);
  const lines = [];
  const counts = {};
  const problems = [];
  for (const name of opts.scenes) {
    for (const mode of opts.modes) {
      const sc = SCENES[name];
      if (!sc) throw new Error(`unknown scene ${name}`);
      const context = await browser.newContext({ viewport: { width: 1280, height: 900 } });
      const page = await context.newPage();
      page.on('pageerror', (e) => problems.push(`${name}/${mode}: page error: ${e.message}`));
      const label = `${name} ${mode}`;
      if (opts.shots) {
        fs.mkdirSync(opts.shots, { recursive: true });
        await page.exposeFunction('__hashShot', (url, n) => {
          fs.writeFileSync(path.join(opts.shots, `${name}-${mode}-${String(n).padStart(4, '0')}.png`),
            Buffer.from(url.split(',')[1], 'base64'));
        });
      }
      if (opts.raw) {
        fs.mkdirSync(opts.raw, { recursive: true });
        await page.exposeFunction('__hashRaw', (b64, n) => {
          fs.writeFileSync(path.join(opts.raw, `${name}-${mode}-${String(n).padStart(4, '0')}.rgba`), Buffer.from(b64, 'base64'));
        });
      }
      const cfg = { sc, label, raw: !!opts.raw, uncheck: opts.uncheck, noMsaa: opts.noMsaa, frames: opts.frames || sc.frames, boxes: MODES[mode], realClock: opts.realClock,
        counts: opts.counts, jitter: opts.jitter, shotEvery: opts.shots ? (+opts.shotEvery || 100) : 0,
        poke: opts.poke && opts.poke.key === `${name}/${mode}` ? opts.poke.frame : undefined };
      await page.addInitScript(instrument, cfg);
      await page.goto(`http://127.0.0.1:${port}/play.html`);
      if (lines.length === 0) header.push(`# renderer ${await rendererString(page)}`);
      await (await page.$('input[type=file]')).setInputFiles(process.env.LEGAIA_DISC_BIN);
      await page.waitForFunction(() => !!window.__playRuntime, null, { timeout: 300000 });
      /* The XA and movie bytes resident, as a player some seconds in has them. */
      if (!opts.noFillWait) {
        await page.waitForFunction(() => window.__playDisc && window.__playDisc.fillMs !== undefined, null, { timeout: 120000 });
      }
      if (opts.idle) await page.waitForTimeout(opts.idle);
      const r = await page.evaluate(stepRun, cfg);
      lines.push(...r.lines);
      /* Would agreeing mean anything? */
      if (!opts.counts) {
        const gl = r.lines.map((l) => /gl=(\w+)/.exec(l)[1]);
        const distinct = new Set(gl).size;
        const lit = r.lines.map((l) => +/lit=([\d.]+)/.exec(l)[1]);
        const blank = lit.filter((x) => x < 0.02).length;
        const col = r.lines.map((l) => +/col=(\d+)/.exec(l)[1]);
        const say = `${label}: ${r.frames} frames, ${distinct} distinct, ${blank} blank, median lit ${lit.slice().sort((a, b) => a - b)[lit.length >> 1]}, `
          + `median colours ${col.slice().sort((a, b) => a - b)[col.length >> 1]}, ${r.positions} player positions, canvas ${r.canvas.join('x')}`
          + `${sc.force ? `, battle up at frame ${r.battleFrame}` : ''}`;
        console.error(say);
        header.push(`# ${say}`);
        if (distinct < r.frames / 10) problems.push(`${label}: VACUOUS, only ${distinct} distinct frames of ${r.frames}`);
        if (blank > r.frames / 2) problems.push(`${label}: VACUOUS, ${blank} blank frames of ${r.frames}`);
        if (sc.pad && r.positions < 10) problems.push(`${label}: VACUOUS, the pad script moved the player through ${r.positions} positions`);
        if (sc.force && r.battleFrame === null) problems.push(`${label}: VACUOUS, the battle scene never came up`);
      } else {
        const bytes = Object.values(r.exports).reduce((a, e) => a + e.bytes, 0);
        counts[`${name}/${mode}`] = { frames: r.frames, gl_draws: r.draws, gl_calls: r.glCalls, wasm_bytes: bytes,
          battle_frame: r.battleFrame };
      }
      await context.close();
    }
  }
  await browser.close();
  stop();
  return { text: header.concat(lines).join('\n') + '\n', counts, problems };
}

/* First disagreement per scene and mode, by column. Returns how many frames differ. */
export function compareTexts(aText, bText) {
  const parse = (t) => t.split('\n').filter((l) => l && !l.startsWith('#'));
  const header = (t, key) => (t.split('\n').find((l) => l.startsWith(`# ${key} `)) || '').slice(key.length + 3);
  const a = parse(aText), b = parse(bText);
  const out = [];
  if (header(aText, 'renderer') !== header(bText, 'renderer')) {
    out.push(`WARNING: different renderers, hashes will not agree: ${header(aText, 'renderer')} / ${header(bText, 'renderer')}`);
  }
  if (a.length !== b.length) out.push(`line count differs: ${a.length} / ${b.length}`);
  const groups = new Map();
  const n = Math.min(a.length, b.length);
  let bad = 0;
  for (let i = 0; i < n; i++) {
    const fa = a[i].split(' '), fb = b[i].split(' ');
    const key = `${fa[0]}/${fa[1]}`;
    if (!groups.has(key)) groups.set(key, { frames: 0, gl: 0, ov: 0, st: 0, first: null });
    const g = groups.get(key);
    g.frames++;
    if (fa[0] !== fb[0] || fa[1] !== fb[1] || fa[2] !== fb[2]) {
      out.push(`line ${i + 1} is a different frame: ${a[i]} / ${b[i]}`);
      bad++;
      continue;
    }
    let differs = false;
    for (const [col, j] of [['gl', 3], ['ov', 4], ['st', 5]]) {
      if (fa[j] !== fb[j]) {
        g[col]++;
        differs = true;
        if (g.first === null) g.first = `frame ${fa[2]} ${col}: ${fa[j]} / ${fb[j]}`;
      }
    }
    if (differs) bad++;
  }
  for (const [key, g] of groups) {
    out.push(`${key}: ${g.frames} frames; differ gl ${g.gl}, overlay ${g.ov}, state ${g.st}${g.first ? `; first at ${g.first}` : ''}`);
  }
  return { bad: bad + Math.abs(a.length - b.length), report: out.join('\n'), frames: n };
}

async function main() {
  if (arg('compare')) {
    const i = argv.indexOf('--compare');
    const { bad, report, frames } = compareTexts(fs.readFileSync(argv[i + 1], 'utf8'), fs.readFileSync(argv[i + 2], 'utf8'));
    console.log(report);
    console.log(bad ? `[frame-hash] DIFFER: ${bad} of ${frames} frames` : `[frame-hash] AGREE: ${frames} frames`);
    process.exit(bad ? 1 : 0);
  }
  const site = path.resolve(arg('site', path.join(REPO, 'site')));
  const disc = process.env.LEGAIA_DISC_BIN;
  if (!disc || !fs.existsSync(disc) || !fs.existsSync(path.join(site, 'wasm', 'legaia_web_viewer_bg.wasm'))) {
    console.log('[skip] frame-hash-web needs LEGAIA_DISC_BIN and a built site/wasm bundle');
    process.exit(0);
  }
  let poke = null;
  if (arg('poke')) {
    const m = /^(\w+\/\w+):(\d+)$/.exec(arg('poke'));
    if (!m) throw new Error('--poke scene/mode:frame');
    poke = { key: m[1], frame: +m[2] };
  }
  const opts = {
    scenes: arg('scene') ? arg('scene').split(',') : Object.keys(SCENES),
    modes: arg('mode') ? arg('mode').split(',') : ['retail', 'enhanced'],
    frames: +arg('frames', 0),
    gl: arg('gl', 'swiftshader'),
    realClock: !!arg('real-clock'),
    jitter: +arg('jitter', 0),
    idle: +arg('idle', 0),
    noFillWait: !!arg('no-fill-wait'),
    counts: !!arg('counts'),
    shots: arg('shots'),
    shotEvery: arg('shot-every'),
    raw: arg('raw'),
    noMsaa: !!arg('no-msaa'),
    uncheck: arg('uncheck') ? arg('uncheck').split(',') : [],
    poke,
  };
  if (opts.gl === 'egl' && !opts.noMsaa && !opts.counts) {
    console.error('[frame-hash] warning: hardware MSAA is not repeatable; expect a stray frame to differ. Use --no-msaa or the default SwiftShader');
  }
  if (opts.counts) {
    const { counts, problems } = await hashSite(site, opts);
    fs.writeFileSync(arg('counts'), JSON.stringify(counts, null, 1));
    for (const p of problems) console.error(p);
    process.exit(problems.length ? 1 : 0);
  }
  const out = arg('out');
  if (!out) throw new Error('--out names where the hash lines go');
  if (arg('base-site')) {
    fs.mkdirSync(out, { recursive: true });
    const base = await hashSite(path.resolve(arg('base-site')), opts);
    const nw = await hashSite(site, opts);
    fs.writeFileSync(path.join(out, 'base.txt'), base.text);
    fs.writeFileSync(path.join(out, 'new.txt'), nw.text);
    const { bad, report, frames } = compareTexts(base.text, nw.text);
    console.log(report);
    const problems = [...base.problems, ...nw.problems];
    for (const p of problems) console.log(p);
    console.log(bad ? `[frame-hash] DIFFER: ${bad} of ${frames} frames` : `[frame-hash] AGREE: ${frames} frames`);
    process.exit(bad || problems.length ? 1 : 0);
  }
  const r = await hashSite(site, opts);
  fs.mkdirSync(path.dirname(path.resolve(out)), { recursive: true });
  fs.writeFileSync(out, r.text);
  for (const p of r.problems) console.log(p);
  console.log(`[frame-hash] wrote ${out}`);
  process.exit(r.problems.length ? 1 : 0);
}

main().catch((e) => { console.error(e); process.exit(1); });
