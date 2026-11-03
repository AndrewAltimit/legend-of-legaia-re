#!/usr/bin/env node
// Frame-time benchmark of the browser play page, the sibling of frame-bench.py.
//
//   scripts/ci/frame-bench-web.mjs [--site site] [--scene town01,..] [--frames 600]
//       [--skip 120] [--json out.json] [--gl swiftshader|egl] [--profile] [--shots dir]
//       [--reenter] [--entry-profile] [--reload] [--idle ms]
//
// Serves `site/` itself, loads the disc named by LEGAIA_DISC_BIN through the page's
// own file input, enters each scene through the page's own scene list and times the
// page's frame function, with every wasm export and the GL calls wrapped in counters.
// Needs a built bundle (scripts/ci/build-wasm.sh) and playwright-core; PLAYWRIGHT_CORE
// names its directory when `~/.npm/_npx` does not hold one. A bundle built with
// `web-viewer`'s `bench-alloc` feature also reports wasm allocations per frame.
//
// `--reenter` enters each scene a second time, right after the first entry, and reports
// that too: what a door back into a scene costs. `--entry-profile` samples each scene
// entry and each battle's first frames and prints time by function, self and inclusive.
//
// `--reload` reloads the page once the disc is up and times a returning visitor's load,
// the disc coming from the page's own cache.
//
// `--idle` waits that long at the boot-mode prompt before the first pick, as a player
// reading it does: the page prewarms the first entry then.
//
// A browser's timers are coarse, and the frame function is only the page's main-thread
// cost: the GPU process's own work is not in it. It prints numbers and asserts nothing.

import { createRequire } from 'module';
import { spawn } from 'child_process';
import fs from 'fs';
import os from 'os';
import path from 'path';
import zlib from 'zlib';
import { fileURLToPath } from 'url';

const arg = (name, dflt) => {
  const i = process.argv.indexOf('--' + name);
  return i < 0 ? dflt : (process.argv[i + 1] && !process.argv[i + 1].startsWith('--') ? process.argv[i + 1] : true);
};
const REPO = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const site = path.resolve(arg('site', path.join(REPO, 'site')));
const FRAMES = +arg('frames', 600);
const SKIP = +arg('skip', 120);
const GL = arg('gl', 'egl');
const SCENES = {
  town01: { scene: 'town01', pad: 'walk' },
  bio_castle: { scene: 'jouina' },
  world_map: { scene: 'map01' },
  battle: { scene: 'map01', battle: true },
  summon_cast: { scene: 'map01', battle: true, cast: [0, 138, 3] },
};
const names = arg('scene') ? arg('scene').split(',') : Object.keys(SCENES);

const disc = process.env.LEGAIA_DISC_BIN;
const wasmPath = path.join(site, 'wasm', 'legaia_web_viewer_bg.wasm');
if (!disc || !fs.existsSync(disc) || !fs.existsSync(wasmPath)) {
  console.log('[skip] frame-bench-web needs LEGAIA_DISC_BIN and a built site/wasm bundle');
  process.exit(0);
}

function playwrightCore() {
  const home = os.homedir();
  const roots = process.env.PLAYWRIGHT_CORE ? [process.env.PLAYWRIGHT_CORE] : [];
  const npx = path.join(home, '.npm/_npx');
  if (fs.existsSync(npx)) {
    for (const d of fs.readdirSync(npx)) roots.push(path.join(npx, d, 'node_modules/playwright-core'));
  }
  for (const r of roots) if (fs.existsSync(path.join(r, 'package.json'))) return createRequire(import.meta.url)(r);
  throw new Error('playwright-core not found; set PLAYWRIGHT_CORE');
}

// Runs in the page, before any of its scripts.
function instrument() {
  const B = (window.__bench = { t0: performance.now(), wasmLoad: null, gl: {}, exports: {}, on: false, frames: [] });
  // GL calls, counted on the context's own prototype.
  const wrapGl = (proto) => {
    for (const k of Object.getOwnPropertyNames(proto)) {
      const d = Object.getOwnPropertyDescriptor(proto, k);
      if (!d || typeof d.value !== 'function') continue;
      if (!/^(draw|bind|use|uniform|buffer|tex|enable|disable|blend|depth|vertexAttrib|clear|framebuffer|readPixels|create|delete)/.test(k)) continue;
      const f = d.value;
      proto[k] = function (...a) {
        if (B.on) B.gl[k] = (B.gl[k] || 0) + 1;
        return f.apply(this, a);
      };
    }
  };
  if (window.WebGL2RenderingContext) wrapGl(WebGL2RenderingContext.prototype);
  if (window.WebGLRenderingContext) wrapGl(WebGLRenderingContext.prototype);
  // Every export of the runtime, timed; strings count their length.
  B.wrapRuntime = (rt) => {
    const proto = Object.getPrototypeOf(rt);
    if (proto.__wrapped) return;
    proto.__wrapped = true;
    for (const k of Object.getOwnPropertyNames(proto)) {
      const d = Object.getOwnPropertyDescriptor(proto, k);
      if (k === 'constructor' || !d || typeof d.value !== 'function') continue;
      const f = d.value;
      proto[k] = function (...a) {
        if (!B.on) return f.apply(this, a);
        const t = performance.now();
        const r = f.apply(this, a);
        const dt = performance.now() - t;
        const e = (B.exports[k] = B.exports[k] || { calls: 0, ms: 0, bytes: 0 });
        e.calls++;
        e.ms += dt;
        if (typeof r === 'string') e.bytes += r.length;
        else if (r && r.byteLength !== undefined) e.bytes += r.byteLength;
        return r;
      };
    }
  };
  // The page's frame.
  B.wrapView = (view) => {
    if (view.__benchWrapped) return;
    view.__benchWrapped = true;
    const f = view._frame;
    B.calls = 0;
    view._frame = function (...a) {
      const t = performance.now();
      const r = f.apply(this, a);
      B.calls++;
      if (B.on) B.frames.push(performance.now() - t);
      else if (B.firstFrame === undefined) B.firstFrame = performance.now() - t;
      else B.entryWorst = Math.max(B.entryWorst || 0, performance.now() - t);
      return r;
    };
  };
  // The module's memory, which a wasm instance never gives back once grown.
  for (const k of ['instantiate', 'instantiateStreaming']) {
    const f = WebAssembly[k];
    WebAssembly[k] = async function (...a) {
      const r = await f.apply(WebAssembly, a);
      const inst = r.instance || r;
      if (inst.exports && inst.exports.memory) B.memory = inst.exports.memory;
      return r;
    };
  }
  // The disc image read off its blob.
  const readAs = FileReader.prototype.readAsArrayBuffer;
  FileReader.prototype.readAsArrayBuffer = function (blob) {
    const t = performance.now();
    if (blob.size > (1 << 24)) {
      this.addEventListener('load', () => { B.fileRead = performance.now() - t; });
    }
    return readAs.call(this, blob);
  };
  // Main-thread stalls, by the browser's own account.
  B.longTasks = [];
  try {
    new PerformanceObserver((l) => {
      for (const e of l.getEntries()) B.longTasks.push(e.duration);
    }).observe({ entryTypes: ['longtask'] });
  } catch (e) { /* none */ }
}

function summarise(b, frames) {
  frames.sort((x, y) => x - y);
  const n = frames.length || 1;
  const sum = frames.reduce((a, x) => a + x, 0);
  const pick = (q) => frames[Math.min(n - 1, Math.floor(n * q))] || 0;
  const exports = Object.entries(b.exports)
    .map(([k, e]) => ({ k, ms: e.ms / n, calls: e.calls / n, kb: e.bytes / n / 1024 }))
    .sort((x, y) => y.ms - x.ms);
  const gl = Object.entries(b.gl).map(([k, c]) => [k, c / n]).sort((x, y) => y[1] - x[1]);
  return { frames: frames.length, avg_ms: sum / n, p50_ms: pick(0.5), p99_ms: pick(0.99), max_ms: pick(1),
    first_ms: b.firstFrame, entry_worst_ms: b.entryWorst || 0, exports, gl };
}

/* Time by function, callees included. */
function inclusiveTableOf(profile, n) {
  const byId = new Map(profile.nodes.map((x) => [x.id, x]));
  const parent = new Map();
  for (const x of profile.nodes) for (const c of x.children || []) parent.set(c, x.id);
  const name = (x) => x.callFrame.functionName || '(anon)';
  const incl = new Map();
  profile.samples.forEach((id, i) => {
    const seen = new Set();
    for (let at = id; at !== undefined; at = parent.get(at)) {
      const k = name(byId.get(at));
      if (seen.has(k)) continue;
      seen.add(k);
      incl.set(k, (incl.get(k) || 0) + profile.timeDeltas[i]);
    }
  });
  return [...incl.entries()].sort((x, y) => y[1] - x[1]).filter(([k]) => !k.startsWith('(')).slice(0, n)
    .map(([k, v]) => `${k} ${(v / 1000).toFixed(0)}`).join('\n          ');
}

async function main() {
  const server = spawn('python3', ['-u', '-m', 'http.server', '0', '--bind', '127.0.0.1', '--directory', site],
    { stdio: ['ignore', 'pipe', 'pipe'] });
  const port = await new Promise((res, rej) => {
    const on = (d) => { const m = /port (\d+)/.exec(String(d)); if (m) res(+m[1]); };
    server.stdout.on('data', on); server.stderr.on('data', on);
    setTimeout(() => rej(new Error('no server')), 10000);
  });
  const stop = () => { try { server.kill(); } catch (e) { /* gone */ } };
  process.on('exit', stop);

  const { chromium } = playwrightCore();
  const dirs = fs.readdirSync(path.join(os.homedir(), '.cache/ms-playwright')).filter((d) => /^chromium-/.test(d)).sort();
  const dir = path.join(os.homedir(), '.cache/ms-playwright', dirs[dirs.length - 1]);
  const exe = [path.join(dir, 'chrome-linux/chrome'), path.join(dir, 'chrome-linux64/chrome')].find((p) => fs.existsSync(p));
  const glArgs = GL === 'swiftshader'
    ? ['--use-gl=angle', '--use-angle=swiftshader', '--enable-unsafe-swiftshader']
    : ['--use-gl=angle', '--use-angle=gl-egl', '--ignore-gpu-blocklist'];
  const browser = await chromium.launch({ executablePath: exe, headless: true,
    args: [...glArgs, '--enable-webgl', '--disable-frame-rate-limit', '--disable-gpu-vsync', '--autoplay-policy=no-user-gesture-required'] });
  const out = { wasm: {}, startup: {}, scenes: {} };
  const raw = fs.readFileSync(wasmPath);
  out.wasm = { bytes: raw.length, gzip: zlib.gzipSync(raw, { level: 6 }).length };
  const glue = fs.readFileSync(path.join(site, 'wasm', 'legaia_web_viewer.js'));
  out.wasm.glue_bytes = glue.length;

  const page = await (await browser.newContext({ viewport: { width: 1280, height: 900 } })).newPage();
  const errors = [];
  page.on('pageerror', (e) => { errors.push(e.message); console.error('PAGEERR', e.message); });
  await page.addInitScript(instrument);
  const t0 = Date.now();
  await page.goto(`http://127.0.0.1:${port}/play.html`);
  out.startup.page_load_ms = Date.now() - t0;
  out.renderer = await page.evaluate(() => {
    const gl = document.createElement('canvas').getContext('webgl2');
    const x = gl && gl.getExtension('WEBGL_debug_renderer_info');
    return x ? gl.getParameter(x.UNMASKED_RENDERER_WEBGL) : 'unknown';
  });
  // Module fetch + compile + instantiate, alone (the loader caches its promise).
  out.startup.wasm_instantiate_ms = await page.evaluate(async () => {
    const t = performance.now();
    await window.LegaiaWasm.load();
    return performance.now() - t;
  });
  /* The disc load, by export. */
  await page.evaluate(async () => {
    const m = await window.LegaiaWasm.load();
    for (const k of ['load_disc', 'load_disc_image']) {
      const f = m.LegaiaRuntime.prototype[k];
      if (!f) continue;
      m.LegaiaRuntime.prototype[k] = function (...a) {
        const t = performance.now();
        const r = f.apply(this, a);
        window.__bench.loadDisc = performance.now() - t;
        return r;
      };
    }
  });
  const fileInput = await page.$('input[type=file]');
  const t1 = Date.now();
  const cdpLoad = await page.context().newCDPSession(page);
  if (arg('entry-profile')) {
    await cdpLoad.send('Profiler.enable');
    await cdpLoad.send('Profiler.setSamplingInterval', { interval: 1000 });
    await cdpLoad.send('Profiler.start');
  }
  await fileInput.setInputFiles(disc);
  await page.waitForFunction(() => !!window.__playRuntime, null, { timeout: 300000 });
  out.startup.disc_ready_ms = Date.now() - t1;
  if (arg('entry-profile')) {
    const { profile } = await cdpLoad.send('Profiler.stop');
    console.log('  disc load inclusive ms:\n          ' + inclusiveTableOf(profile, 40));
  }
  out.startup.load_disc_export_ms = await page.evaluate(() => window.__bench.loadDisc);
  out.startup.file_read_ms = await page.evaluate(() => window.__bench.fileRead);
  out.startup.wasm_memory_mb = await page.evaluate(() => {
    const m = window.__bench.memory;
    return m ? m.buffer.byteLength / 1048576 : null;
  });
  await page.evaluate(async () => {
    window.__bench.wrapRuntime(window.__playRuntime);
    const m = await window.LegaiaWasm.load();
    if (m.bench_alloc_totals) window.__wasmAllocTotals = () => m.bench_alloc_totals();
  });

  if (arg('reload')) {
    /* A returning visitor: the page again, its cached disc auto-loading. */
    const tr = Date.now();
    await page.reload();
    await page.waitForFunction(() => !!window.__playRuntime, null, { timeout: 300000 });
    out.startup.reload_disc_ready_ms = Date.now() - tr;
    out.startup.reload_file_read_ms = await page.evaluate(() => window.__bench.fileRead);
  }
  if (arg('idle')) await page.waitForTimeout(+arg('idle'));
  const cdp = await page.context().newCDPSession(page);
  /* A CPU profile of the scene entry alone, stopped from the page. */
  const inclusiveTable = inclusiveTableOf;
  const profiling = () => arg('profile') || arg('entry-profile');
  const selfTable = (profile, n) => {
    const byId = new Map(profile.nodes.map((x) => [x.id, x]));
    const self = new Map();
    profile.samples.forEach((id, i) => {
      const f = byId.get(id).callFrame;
      const k = f.functionName || '(anon)';
      self.set(k, (self.get(k) || 0) + profile.timeDeltas[i]);
    });
    const tot = [...self.values()].reduce((a, x) => a + x, 0);
    return [...self.entries()].sort((x, y) => y[1] - x[1]).slice(0, n)
      .map(([k, v]) => `${k} ${(v / 1000).toFixed(0)}ms`).join(', ') + ` (total ${(tot / 1000).toFixed(0)}ms)`;
  };
  await page.exposeFunction('__benchEntered', async () => {
    if (!profiling()) return;
    const { profile } = await cdp.send('Profiler.stop');
    console.log('  entry self: ' + selfTable(profile, 24));
    if (arg('entry-profile')) console.log('  entry inclusive ms:\n          ' + inclusiveTable(profile, 60));
  });
  /* The battle edge's own: from the force to the first frames after it. */
  await page.exposeFunction('__benchBattleProfile', async (phase) => {
    if (!arg('entry-profile')) return;
    if (phase === 'start') {
      await cdp.send('Profiler.enable');
      await cdp.send('Profiler.setSamplingInterval', { interval: 500 });
      await cdp.send('Profiler.start');
    } else {
      const { profile } = await cdp.send('Profiler.stop');
      console.log('  battle entry self: ' + selfTable(profile, 24));
      console.log('  battle entry inclusive ms:\n          ' + inclusiveTable(profile, 60));
    }
  });
  for (const name of names) {
    const sc = SCENES[name];
    if (profiling()) {
      await cdp.send('Profiler.enable');
      await cdp.send('Profiler.setSamplingInterval', { interval: 500 });
      await cdp.send('Profiler.start');
    }
    const r = await page.evaluate(async ({ sc, FRAMES, SKIP, reenter }) => {
      const B = window.__bench;
      B.on = false; B.frames = []; B.exports = {}; B.gl = {}; B.firstFrame = undefined; B.entryWorst = 0;
      const btn = document.querySelector(`[data-scene="${sc.scene}"]`);
      if (!btn) throw new Error('no scene button ' + sc.scene);
      const t = performance.now();
      B.on = true;
      btn.click();
      B.on = false;
      const entry = performance.now() - t;
      let reentry = null;
      if (reenter) {
        const t2 = performance.now();
        btn.click();
        reentry = performance.now() - t2;
      }
      const entryExports = Object.entries(B.exports)
        .map(([k, e]) => [k, +e.ms.toFixed(1), e.calls])
        .sort((x, y) => y[1] - x[1]).slice(0, 8);
      B.exports = {}; B.gl = {}; B.frames = [];
      if (window.__benchEntered) await window.__benchEntered();
      const view = window.__playView;
      B.wrapView(view);
      const wait = (pred) => new Promise((res) => {
        const check = () => (pred() ? res() : setTimeout(check, 20));
        check();
      });
      if (sc.battle) {
        /* Settled in the field first, so the battle edge's frame stands alone. */
        const settled = B.calls;
        await wait(() => B.calls - settled >= 10);
        await window.__benchBattleProfile('start');
      }
      if (sc.cast) window.__playRuntime.debug_seed_inflight_cast(...sc.cast);
      B.firstFrame = undefined; B.entryWorst = 0;
      if (sc.battle) window.__playRuntime.debug_force_battle(-1);
      const before = B.calls;
      if (sc.battle) {
        /* Past the swirl: the battle edge lands on the intro's last frames. */
        await wait(() => B.calls - before >= SKIP);
        await window.__benchBattleProfile('stop');
      }
      await wait(() => B.calls - before >= SKIP);
      if (sc.pad === 'walk') {
        const send = (code, type) => window.dispatchEvent(new KeyboardEvent(type, { code, key: code, bubbles: true }));
        B.walk = setInterval(() => {
          const c = (B.walkN = (B.walkN || 0) + 1) % 2 ? 'ArrowDown' : 'ArrowUp';
          send(c === 'ArrowDown' ? 'ArrowUp' : 'ArrowDown', 'keyup');
          send(c, 'keydown');
        }, 3500);
      }
      const alloc0 = window.__wasmAllocTotals ? window.__wasmAllocTotals() : null;
      B.on = true;
      const start = B.calls;
      const tw = performance.now();
      await wait(() => B.calls - start >= FRAMES);
      const wall = performance.now() - tw;
      B.on = false;
      if (B.walk) {
        clearInterval(B.walk);
        B.walk = null;
        /* A key left down would walk the next scene through its doors. */
        for (const code of ['ArrowUp', 'ArrowDown']) {
          window.dispatchEvent(new KeyboardEvent('keyup', { code, key: code, bubbles: true }));
        }
      }
      const alloc1 = window.__wasmAllocTotals ? window.__wasmAllocTotals() : null;
      const ls = B.longTasks.slice();
      B.longTasks.length = 0;
      return { entryExports, entry_ms: entry, reentry_ms: reentry, wall_ms: wall, frames: B.frames, firstFrame: B.firstFrame, entryWorst: B.entryWorst,
        exports: B.exports, gl: B.gl, alloc: alloc0 && [alloc1[0] - alloc0[0], alloc1[1] - alloc0[1]],
        longtasks: ls, heap: performance.memory ? performance.memory.usedJSHeapSize : 0,
        state: view.scene, battle: typeof view.rt.state_json === 'function' ? view.rt.state_json().slice(0, 200) : '' };
    }, { sc, FRAMES, SKIP, reenter: !!arg('reenter') });
    if (arg('shots')) {
      fs.mkdirSync(arg('shots'), { recursive: true });
      await page.screenshot({ path: path.join(arg('shots'), `${name}.png`) });
    }
    const s = summarise({ exports: r.exports, gl: r.gl, firstFrame: r.firstFrame, entryWorst: r.entryWorst }, r.frames);
    s.entry_ms = r.entry_ms;
    s.reentry_ms = r.reentry_ms;
    s.entry_exports = r.entryExports;
    s.fps = FRAMES / (r.wall_ms / 1000);
    s.longtasks = r.longtasks.length;
    s.longtask_max = r.longtasks.reduce((a, x) => Math.max(a, x), 0);
    if (r.alloc) { s.allocs_per_frame = r.alloc[0] / FRAMES; s.alloc_kb_per_frame = r.alloc[1] / FRAMES / 1024; }
    s.heap_mb = r.heap / 1048576;
    out.scenes[name] = s;
    console.log(`${name}: frame avg ${s.avg_ms.toFixed(2)} p50 ${s.p50_ms.toFixed(2)} p99 ${s.p99_ms.toFixed(2)} max ${s.max_ms.toFixed(1)} ms; `
      + `entry ${s.entry_ms.toFixed(0)} ms${s.reentry_ms === null ? '' : `, re-entry ${s.reentry_ms.toFixed(0)}`}, first ${(s.first_ms || 0).toFixed(0)}, worst-entry ${s.entry_worst_ms.toFixed(0)}; ${s.fps.toFixed(0)} fps; `
      + `allocs/frame ${s.allocs_per_frame === undefined ? '-' : s.allocs_per_frame.toFixed(0)}; heap ${s.heap_mb.toFixed(0)} MB; long tasks ${s.longtasks}`);
    console.log('  exports: ' + s.exports.slice(0, 14).map((e) => `${e.k} ${e.ms.toFixed(2)}ms/${e.calls.toFixed(1)}x${e.kb >= 1 ? '/' + e.kb.toFixed(0) + 'KB' : ''}`).join(', '));
    console.log('  entry: ' + r.entryExports.map(([k, ms, c]) => `${k} ${ms}ms/${c}x`).join(', '));
    console.log('  gl: ' + s.gl.slice(0, 12).map(([k, c]) => `${k} ${c.toFixed(0)}`).join(', '));
    if (arg('profile')) {
      // Self time by function, garbage collector included.
      await cdp.send('Profiler.enable');
      await cdp.send('Profiler.setSamplingInterval', { interval: 500 });
      await cdp.send('Profiler.start');
      await page.waitForTimeout(6000);
      const { profile } = await cdp.send('Profiler.stop');
      const byId = new Map(profile.nodes.map((n) => [n.id, n]));
      const self = new Map();
      const dts = profile.timeDeltas;
      profile.samples.forEach((id, i) => {
        const n = byId.get(id);
        const k = `${n.callFrame.functionName || '(anon)'} ${path.basename(n.callFrame.url || '')}:${n.callFrame.lineNumber}`;
        self.set(k, (self.get(k) || 0) + dts[i]);
      });
      const tot = [...self.values()].reduce((a, x) => a + x, 0);
      // The call chain of the heaviest function outside the browser itself.
      const parents = new Map();
      for (const n of profile.nodes) for (const c of n.children || []) parents.set(c, n.id);
      const nodeSelf = new Map();
      profile.samples.forEach((id, i) => nodeSelf.set(id, (nodeSelf.get(id) || 0) + dts[i]));
      const heavy = [...nodeSelf.entries()].sort((x, y) => y[1] - x[1])
        .filter(([id]) => !byId.get(id).callFrame.functionName.startsWith('(')).slice(0, 3);
      s.chains = heavy.map(([id]) => {
        const c = [];
        for (let at = id; at !== undefined && c.length < 12; at = parents.get(at)) c.push(byId.get(at).callFrame.functionName);
        return c.join(' < ');
      });
      console.log('  chains: ' + s.chains.join('\n          '));
      s.profile = [...self.entries()].sort((x, y) => y[1] - x[1]).slice(0, 25)
        .map(([k, v]) => [k, +(v * 100 / tot).toFixed(1)]);
      console.log('  self%: ' + s.profile.map(([k, v]) => `${k} ${v}`).join('\n         '));
    }
  }
  console.log(JSON.stringify({ wasm: out.wasm, startup: out.startup, renderer: out.renderer, errors }));
  if (arg('json')) fs.writeFileSync(arg('json'), JSON.stringify(out, null, 1));
  await browser.close();
  stop();
}

main().catch((e) => { console.error(e); process.exit(1); });
