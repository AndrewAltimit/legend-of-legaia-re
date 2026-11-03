#!/usr/bin/env node
// Frame-time benchmark of the browser play page, the sibling of frame-bench.py.
//
//   scripts/ci/frame-bench-web.mjs [--site site] [--scene town01,..] [--frames 600]
//       [--skip 120] [--json out.json] [--gl swiftshader|egl] [--profile] [--shots dir]
//       [--reenter] [--entry-profile] [--reload] [--idle ms]
//       [--vsync] [--paths] [--card file.mcr] [--card-block n] [--cpu-throttle n]
//       [--uncheck id,id] [--no-msaa] [--canvas-scale s]
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
// `--vsync` leaves the browser's frame pacing on. Without it the cadence row reads how
// fast the page can go; with it, whether it holds the display's rate.
//
// `--paths` adds the player's ways in: a New Game and a card load, each on a fresh page,
// and a second fight in one session.
//
// `--cpu-throttle n` slows the renderer's main thread n times. It is a proxy, and the
// output says so: it shows which costs scale with the CPU and says nothing of a device.
//
// `--uncheck id,id` clears those option boxes once the disc is up, `--no-msaa` asks for a
// context without MSAA, and `--canvas-scale s` draws the GL canvas at s of its size: what
// one rung of the quality ladder costs. Each is a page state a player can reach or the
// page can step to; none is a claim about a device.
//
// A browser's timers are coarse, and the frame function is only the page's main-thread
// cost: the GPU, display, audio pump and memory rows are what it leaves out. See
// docs/tooling/frame-bench.md. It prints numbers and asserts nothing.

import fs from 'fs';
import path from 'path';
import zlib from 'zlib';
import { fileURLToPath } from 'url';
import { chromiumProcesses, largestOf, launchChromium, rendererString, serveSite } from './browser-session.mjs';

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
  /* A fight, back to the field through the scene list, and the fight this row times. */
  second_fight: { scene: 'map01', battle: true, again: true },
};
const names = arg('scene') ? arg('scene').split(',') : Object.keys(SCENES);
const VSYNC = !!arg('vsync');
const THROTTLE = +arg('cpu-throttle', 0);
const DEFAULT_CARD = path.join(REPO, 'saves/library/cards/playthrough-ladder-pro00-14.mcr');
const CARD = arg('card', DEFAULT_CARD);
const UNCHECK = arg('uncheck') ? arg('uncheck').split(',') : [];
const NO_MSAA = !!arg('no-msaa');
const CANVAS_SCALE = +arg('canvas-scale', 1);

const disc = process.env.LEGAIA_DISC_BIN;
const wasmPath = path.join(site, 'wasm', 'legaia_web_viewer_bg.wasm');
if (!disc || !fs.existsSync(disc) || !fs.existsSync(wasmPath)) {
  console.log('[skip] frame-bench-web needs LEGAIA_DISC_BIN and a built site/wasm bundle');
  process.exit(0);
}

// Runs in the page, before any of its scripts.
function instrument(noMsaa) {
  const B = (window.__bench = { t0: performance.now(), wasmLoad: null, gl: {}, exports: {}, on: false, frames: [], gaps: [], lastStart: null });
  if (noMsaa) {
    const get = HTMLCanvasElement.prototype.getContext;
    HTMLCanvasElement.prototype.getContext = function (kind, attrs) {
      return get.call(this, kind, /^webgl/.test(kind) ? { ...attrs, antialias: false } : attrs);
    };
  }
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
  // What the GL contexts were asked to hold, by object: a tally of the page's requests,
  // not the driver's account. The canvases' own buffers are not in it.
  const held = (B.glHeld = { buffer: new Map(), texture: new Map(), renderbuffer: new Map(), unknownFormats: 0 });
  const PIXEL_BYTES = { 0x8229: 1, 0x822B: 2, 0x8051: 3, 0x8058: 4, 0x8C43: 4, 0x8059: 4, 0x8057: 2, 0x8056: 2, 0x8D62: 2,
    0x822D: 2, 0x822F: 4, 0x881A: 8, 0x822E: 4, 0x8230: 8, 0x8814: 16, 0x8C3A: 4, 0x8232: 1, 0x8231: 1, 0x8234: 2, 0x8233: 2,
    0x8236: 4, 0x8235: 4, 0x8238: 2, 0x8237: 2, 0x823A: 4, 0x8D7C: 4, 0x8D82: 4, 0x8D76: 8, 0x8D70: 16,
    0x81A5: 2, 0x81A6: 4, 0x8CAC: 4, 0x88F0: 4, 0x8CAD: 8, 0x8D48: 1 };
  const pixelBytes = (internal, format, type) => {
    if (PIXEL_BYTES[internal]) return PIXEL_BYTES[internal];
    /* An unsized format: its type says. */
    const channels = { 0x1908: 4, 0x1907: 3, 0x1909: 1, 0x190A: 2, 0x1906: 1 }[internal] || { 0x1908: 4, 0x1907: 3 }[format];
    const size = { 0x1401: 1, 0x1406: 4, 0x140B: 2, 0x8D61: 2, 0x1403: 2, 0x1405: 4 }[type];
    if (channels && size) return channels * size;
    held.unknownFormats++;
    return 4;
  };
  const bound = new WeakMap();
  const stateOf = (gl) => {
    if (!bound.has(gl)) bound.set(gl, { buffers: {}, unit: 0x84C0, textures: {}, renderbuffer: null });
    return bound.get(gl);
  };
  const levelSize = (w, h, d, level) => Math.max(1, w >> level) * Math.max(1, h >> level) * Math.max(1, d >> level);
  const wrapHeld = (proto) => {
    const on = (name, fn) => {
      const f = proto[name];
      if (!f) return;
      proto[name] = function (...a) { fn(this, a); return f.apply(this, a); };
    };
    on('bindBuffer', (gl, [t, b]) => { stateOf(gl).buffers[t] = b; });
    on('bindBufferBase', (gl, [t, , b]) => { stateOf(gl).buffers[t] = b; });
    on('bindBufferRange', (gl, [t, , b]) => { stateOf(gl).buffers[t] = b; });
    on('activeTexture', (gl, [u]) => { stateOf(gl).unit = u; });
    on('bindTexture', (gl, [t, x]) => { const s = stateOf(gl); s.textures[s.unit + ':' + t] = x; });
    on('bindRenderbuffer', (gl, [, x]) => { stateOf(gl).renderbuffer = x; });
    on('bufferData', (gl, [t, data, , offset, length]) => {
      const b = stateOf(gl).buffers[t];
      if (!b) return;
      const n = typeof data === 'number' ? data : length ? length * (data.BYTES_PER_ELEMENT || 1)
        : (data ? data.byteLength - (offset || 0) * (data.BYTES_PER_ELEMENT || 1) : 0);
      held.buffer.set(b, n);
    });
    const texOf = (gl, target) => {
      const s = stateOf(gl);
      const t = target >= 0x8515 && target <= 0x851A ? 0x8513 : target;
      return s.textures[s.unit + ':' + t];
    };
    const setLevel = (gl, target, level, bytes) => {
      const x = texOf(gl, target);
      if (!x) return;
      if (!held.texture.has(x)) held.texture.set(x, new Map());
      held.texture.get(x).set(target + ':' + level, bytes);
    };
    on('texImage2D', (gl, a) => {
      const [t, level, internal] = a;
      if (a.length >= 8) {
        setLevel(gl, t, level, a[3] * a[4] * pixelBytes(internal, a[6], a[7]));
      } else {
        const src = a[5];
        const w = src.videoWidth || src.naturalWidth || src.displayWidth || src.width || 0;
        const h = src.videoHeight || src.naturalHeight || src.displayHeight || src.height || 0;
        setLevel(gl, t, level, w * h * pixelBytes(internal, a[3], a[4]));
      }
    });
    on('texImage3D', (gl, a) => setLevel(gl, a[0], a[1], a[3] * a[4] * a[5] * pixelBytes(a[2], a[7], a[8])));
    on('texStorage2D', (gl, [t, levels, internal, w, h]) => {
      for (let l = 0; l < levels; l++) setLevel(gl, t, l, levelSize(w, h, 1, l) * pixelBytes(internal));
    });
    on('texStorage3D', (gl, [t, levels, internal, w, h, d]) => {
      /* An array's layer count holds at every level; a volume's halves. */
      const layers = t === 0x8C1A;
      for (let l = 0; l < levels; l++) {
        setLevel(gl, t, l, levelSize(w, h, layers ? 1 : d, l) * (layers ? d : 1) * pixelBytes(internal));
      }
    });
    on('renderbufferStorage', (gl, [, internal, w, h]) => {
      const x = stateOf(gl).renderbuffer;
      if (x) held.renderbuffer.set(x, w * h * pixelBytes(internal));
    });
    on('renderbufferStorageMultisample', (gl, [, samples, internal, w, h]) => {
      const x = stateOf(gl).renderbuffer;
      if (x) held.renderbuffer.set(x, w * h * Math.max(1, samples) * pixelBytes(internal));
    });
    on('deleteBuffer', (gl, [x]) => held.buffer.delete(x));
    on('deleteTexture', (gl, [x]) => held.texture.delete(x));
    on('deleteRenderbuffer', (gl, [x]) => held.renderbuffer.delete(x));
  };
  if (window.WebGL2RenderingContext) wrapHeld(WebGL2RenderingContext.prototype);
  if (window.WebGLRenderingContext) wrapHeld(WebGLRenderingContext.prototype);
  B.glHeldBytes = () => {
    let buffers = 0, textures = 0, renderbuffers = 0;
    for (const n of held.buffer.values()) buffers += n;
    for (const levels of held.texture.values()) for (const n of levels.values()) textures += n;
    for (const n of held.renderbuffer.values()) renderbuffers += n;
    return { buffers, textures, renderbuffers, unknownFormats: held.unknownFormats };
  };
  // The frame function's commands on the GPU's clock, off the timer query, where the
  // driver has one. The query spans its begin to its end, so where a flush falls between
  // them it includes the wait for the main thread's next commands: an upper bound on GPU
  // work, whose lower edge is the frame function itself. Above that edge, the GPU is what
  // is behind.
  const gpu = (B.gpu = { ext: undefined, pending: [], free: [], ms: [], disjoint: 0, active: null });
  const gpuBegin = (gl) => {
    if (!gl) return;
    if (gpu.ext === undefined) gpu.ext = gl.getExtension('EXT_disjoint_timer_query_webgl2') || null;
    if (!gpu.ext || gpu.active) return;
    const q = gpu.free.pop() || gl.createQuery();
    gl.beginQuery(gpu.ext.TIME_ELAPSED_EXT, q);
    gpu.active = q;
  };
  const gpuEnd = (gl) => {
    if (!gpu.active) return;
    gl.endQuery(gpu.ext.TIME_ELAPSED_EXT);
    gpu.pending.push(gpu.active);
    gpu.active = null;
  };
  B.gpuPoll = (gl) => {
    if (!gl || !gpu.ext) return;
    while (gpu.pending.length && gl.getQueryParameter(gpu.pending[0], gl.QUERY_RESULT_AVAILABLE)) {
      const q = gpu.pending.shift();
      /* A disjoint read means some timings in flight are not to be trusted. */
      if (gl.getParameter(gpu.ext.GPU_DISJOINT_EXT)) gpu.disjoint++;
      else gpu.ms.push(gl.getQueryParameter(q, gl.QUERY_RESULT) / 1e6);
      gpu.free.push(q);
    }
  };
  // The audio pump's main-thread time: the output's report handler, or the fallback's
  // buffer callback. Worklet reports are message events on the node's port.
  B.pump = { ms: 0, calls: 0, max: 0 };
  const wrapHandler = (proto, prop) => {
    const d = proto && Object.getOwnPropertyDescriptor(proto, prop);
    if (!d || !d.set) return;
    Object.defineProperty(proto, prop, { configurable: true, get: d.get, set(fn) {
      d.set.call(this, fn && function (...a) {
        const t = performance.now();
        try { return fn.apply(this, a); } finally {
          if (B.on) {
            const dt = performance.now() - t;
            B.pump.ms += dt; B.pump.calls++; B.pump.max = Math.max(B.pump.max, dt);
          }
        }
      });
    } });
  };
  wrapHandler(window.MessagePort && MessagePort.prototype, 'onmessage');
  wrapHandler(window.ScriptProcessorNode && ScriptProcessorNode.prototype, 'onaudioprocess');
  // The context the output opens, whose state says whether any of it runs.
  if (window.AudioContext) {
    const Base = window.AudioContext;
    window.AudioContext = class extends Base {
      constructor(...a) { super(...a); B.audioCtx = this; }
    };
  }
  // Where the running animation-frame task began: a New Game lands inside one.
  const raf = window.requestAnimationFrame.bind(window);
  window.requestAnimationFrame = (cb) => raf((t) => { B.taskStart = performance.now(); cb(t); });
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
      const gl = this.renderer && this.renderer.gl;
      const on = B.on;
      if (on) {
        /* The display's cadence: frame function start to frame function start. */
        if (B.lastStart !== null) B.gaps.push(t - B.lastStart);
        B.lastStart = t;
        B.gpuPoll(gl);
        gpuBegin(gl);
      }
      let r;
      try { r = f.apply(this, a); } finally { if (on) gpuEnd(gl); }
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
  /* Display cadence, and how far past one display frame the worst gaps ran. */
  const gaps = (b.gaps || []).slice().sort((x, y) => x - y);
  const atGap = (q) => gaps[Math.min(gaps.length - 1, Math.floor(gaps.length * q))] || 0;
  const cadence = { p50_ms: atGap(0.5), p99_ms: atGap(0.99), max_ms: atGap(1),
    over_33ms: gaps.filter((x) => x > 1000 / 30).length, samples: gaps.length };
  const gpus = (b.gpuMs || []).slice().sort((x, y) => x - y);
  const atGpu = (q) => gpus[Math.min(gpus.length - 1, Math.floor(gpus.length * q))] || 0;
  const gpu = gpus.length ? { avg_ms: gpus.reduce((a, x) => a + x, 0) / gpus.length, p50_ms: atGpu(0.5),
    p99_ms: atGpu(0.99), max_ms: atGpu(1), samples: gpus.length, disjoint_drops: b.gpuDisjoint || 0 } : null;
  return { cadence, gpu, frames: frames.length, avg_ms: sum / n, p50_ms: pick(0.5), p99_ms: pick(0.99), max_ms: pick(1),
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

/* Run in the page: every export counted, and allocations where the bundle counts them. */
async function wrapForBench() {
  window.__bench.wrapRuntime(window.__playRuntime);
  const m = await window.LegaiaWasm.load();
  if (m.bench_alloc_totals) window.__wasmAllocTotals = () => m.bench_alloc_totals();
}

/* The page's whole footprint, by who holds it. */
async function memoryRow(page) {
  const m = await page.evaluate(() => {
    const B = window.__bench;
    const disc = window.__playDisc;
    const canvas = document.getElementById('play-canvas');
    const gl = window.__playView && window.__playView.renderer && window.__playView.renderer.gl;
    return { wasm_mb: B.memory ? B.memory.buffer.byteLength / 1048576 : null,
      disc_resident_mb: disc ? disc.blocks.reduce((a, b) => a + (b.end - b.start), 0) / 1048576 : null,
      disc_filled: !!(disc && disc.fillMs !== undefined),
      js_heap_mb: performance.memory ? performance.memory.usedJSHeapSize / 1048576 : null,
      gl_held: B.glHeldBytes(),
      canvas: canvas && [canvas.width, canvas.height, !!(gl && gl.getContextAttributes().antialias)] };
  });
  const procs = chromiumProcesses();
  const renderer = largestOf(procs, 'renderer');
  const gpu = largestOf(procs, 'gpu-process');
  return { ...m, renderer_rss_mb: renderer && renderer.rssMb, renderer_peak_mb: renderer && renderer.peakMb,
    gpu_process_rss_mb: gpu && gpu.rssMb };
}

function printRows(s) {
  const c = s.cadence, m = s.main_thread, mem = s.memory;
  if (s.gpu === null) {
    console.log('  gpu span: no reading');
  } else if (s.gpu) {
    console.log(`  gpu span: avg ${s.gpu.avg_ms.toFixed(2)} p50 ${s.gpu.p50_ms.toFixed(2)} p99 ${s.gpu.p99_ms.toFixed(2)} max ${s.gpu.max_ms.toFixed(1)} ms `
      + `over ${s.gpu.samples} frames${s.gpu.disjoint_drops ? `, ${s.gpu.disjoint_drops} dropped as disjoint` : ''}`);
  }
  if (c) console.log(`  cadence: gap p50 ${c.p50_ms.toFixed(2)} p99 ${c.p99_ms.toFixed(2)} max ${c.max_ms.toFixed(1)} ms; ${c.over_33ms} of ${c.samples} over 33 ms`);
  if (m) {
    console.log(`  main thread per frame: tasks ${m.task_ms_per_frame.toFixed(2)} ms = frame function ${m.frame_fn_ms_per_frame.toFixed(2)} + outside ${m.outside_ms_per_frame.toFixed(2)}; `
      + `of which audio pump ${m.audio_pump_ms_per_frame.toFixed(2)} (${m.audio_pump_calls} calls, worst ${m.audio_pump_max_ms.toFixed(1)})`);
  }
  if (s.audio_edge) {
    const a = s.audio_edge;
    console.log(`  audio across the battle edge: ${a.dry_frames} dry frames (${(a.dry_frames * 1000 / a.rate).toFixed(0)} ms), `
      + `${a.played_frames} played; queue target ${a.target_before} -> ${a.target_after}; ${a.path}, context ${a.context}`);
  } else if (s.audio_edge === null) {
    console.log('  audio across the battle edge: no reading, the output never came up');
  }
  if (s.audio_steady) {
    const a = s.audio_steady;
    console.log(`  audio over the timed frames: ${a.dry_frames} dry frames (${(a.dry_frames * 1000 / a.rate).toFixed(0)} ms), `
      + `${a.played_frames} played (${(a.played_frames * 1000 / a.rate).toFixed(0)} ms); queue target ${a.target_before} -> ${a.target_after}; context ${a.context}`);
  }
  if (mem) {
    const mb = (x) => (x === null || x === undefined ? '-' : x.toFixed(0));
    const h = mem.gl_held;
    console.log(`  memory MB: renderer RSS ${mb(mem.renderer_rss_mb)} (peak ${mb(mem.renderer_peak_mb)}), gpu process RSS ${mb(mem.gpu_process_rss_mb)}; `
      + `wasm ${mb(mem.wasm_mb)}, disc resident ${mb(mem.disc_resident_mb)}${mem.disc_filled ? '' : ' (fill not landed)'}, JS heap ${mb(mem.js_heap_mb)}; `
      + `GL asked for: buffers ${mb(h.buffers / 1048576)}, textures ${mb(h.textures / 1048576)}, renderbuffers ${mb(h.renderbuffers / 1048576)}`
      + `${h.unknownFormats ? ` (${h.unknownFormats} of unknown format, counted at 4 bytes)` : ''}; canvas ${mem.canvas ? mem.canvas.join('x').replace('xtrue', ' MSAA').replace('xfalse', '') : '-'}`);
  }
}

/* A player's way in, on a fresh page: 'new_game' through the title, 'card_load' through
 * the saves bar. `entry_ms` is the synchronous stretch a player sits through, from the
 * task the start lands in to the end of the page's rebuild; a picker row's `entry_ms` is
 * the same stretch for the pick. */
async function playerPath(browser, port, kind, errors) {
  if (kind === 'card_load' && !fs.existsSync(CARD)) {
    console.log(`${kind}: no reading, no card at ${CARD} (--card names one)`);
    return { reading: false, why: 'no card image' };
  }
  const context = await browser.newContext({ viewport: { width: 1280, height: 900 } });
  const page = await context.newPage();
  page.on('pageerror', (e) => { errors.push(e.message); console.error('PAGEERR', e.message); });
  await page.addInitScript(instrument, NO_MSAA);
  if (THROTTLE) await (await context.newCDPSession(page)).send('Emulation.setCPUThrottlingRate', { rate: THROTTLE });
  await page.goto(`http://127.0.0.1:${port}/play.html`);
  await (await page.$('input[type=file]')).setInputFiles(disc);
  await page.waitForFunction(() => !!window.__playRuntime, null, { timeout: 300000 });
  await page.evaluate(wrapForBench);
  await page.evaluate(() => {
    const B = window.__bench;
    B.wrapView(window.PlayView.prototype);
    const Proto = window.PlayView.prototype;
    const adopt = Proto._adopt;
    Proto._adopt = function (...a) {
      const r = adopt.apply(this, a);
      if (B.entryMs === undefined) B.entryMs = performance.now() - B.taskStart;
      return r;
    };
    const rt = window.__playRuntime;
    const imp = rt.import_card_save;
    rt.import_card_save = function (...a) { B.taskStart = performance.now(); B.cardFrom = true; return imp.apply(this, a); };
    B.exports = {};
    B.on = false;
  });
  let block = null;
  if (kind === 'new_game') {
    await page.evaluate(() => { document.querySelector('[data-mode="newgame"]').click(); });
    /* Past the logos, the title's Start, its New Game row and the rules screen. */
    const started = Date.now();
    for (let i = 0; Date.now() - started < 60000; i++) {
      if (await page.evaluate(() => window.__bench.entryMs !== undefined)) break;
      const code = i % 2 ? 'KeyZ' : 'Enter';
      await page.evaluate((c) => {
        for (const type of ['keydown', 'keyup']) window.dispatchEvent(new KeyboardEvent(type, { code: c, key: c, bubbles: true }));
      }, code);
      await page.waitForTimeout(250);
    }
  } else {
    const pickers = await page.$$('input[type=file]');
    let picker = null;
    for (const h of pickers) if (/\.mcr/.test(await h.getAttribute('accept') || '')) picker = h;
    if (!picker) throw new Error('no save-file input on the page');
    await picker.setInputFiles(CARD);
    /* A card of several saves asks which. */
    const started = Date.now();
    while (Date.now() - started < 30000) {
      block = await page.evaluate((want) => {
        if (window.__bench.entryMs !== undefined) return 'landed';
        const buttons = [...document.querySelectorAll('#save-status button')];
        if (!buttons.length) return null;
        const b = want === null ? buttons[0] : buttons.find((x) => x.textContent.startsWith(`Block ${want} `)) || buttons[0];
        const label = b.textContent;
        b.click();
        return label;
      }, arg('card-block') === undefined ? null : +arg('card-block'));
      if (block) break;
      await page.waitForTimeout(100);
    }
  }
  const r = await page.evaluate(async (SKIP) => {
    const B = window.__bench;
    const started = performance.now();
    while (B.entryMs === undefined && performance.now() - started < 30000) await new Promise((res) => setTimeout(res, 50));
    if (B.entryMs === undefined) return null;
    const at = B.calls;
    while (B.calls - at < SKIP && performance.now() - started < 60000) await new Promise((res) => setTimeout(res, 20));
    const view = window.__playView;
    return { entry_ms: B.entryMs, first_ms: B.firstFrame, entry_worst_ms: B.entryWorst || 0, frames_after: B.calls - at,
      scene: view && view.scene };
  }, SKIP);
  if (!r) {
    console.log(`${kind}: no reading, the page never entered a scene`);
    await context.close();
    return { reading: false, why: 'no scene entered' };
  }
  const row = { reading: true, ...r, memory: await memoryRow(page), card_block: block };
  console.log(`${kind}: entry ${r.entry_ms.toFixed(0)} ms, first ${(r.first_ms || 0).toFixed(0)}, worst of the next ${r.frames_after} ${r.entry_worst_ms.toFixed(0)}; landed in ${r.scene}${block ? `; card ${path.basename(CARD)}: ${block}` : ''}`);
  printRows({ memory: row.memory });
  await context.close();
  return row;
}

async function main() {
  const { port, stop } = await serveSite(site);
  const browser = await launchChromium({ gl: GL, vsync: VSYNC });
  const out = { wasm: {}, startup: {}, scenes: {}, paths: {}, pacing: VSYNC ? 'vsync' : 'uncapped',
    cpu_throttle: THROTTLE ? { rate: THROTTLE, proxy: true } : null };
  const raw = fs.readFileSync(wasmPath);
  out.wasm = { bytes: raw.length, gzip: zlib.gzipSync(raw, { level: 6 }).length };
  const glue = fs.readFileSync(path.join(site, 'wasm', 'legaia_web_viewer.js'));
  out.wasm.glue_bytes = glue.length;

  const page = await (await browser.newContext({ viewport: { width: 1280, height: 900 } })).newPage();
  const errors = [];
  page.on('pageerror', (e) => { errors.push(e.message); console.error('PAGEERR', e.message); });
  await page.addInitScript(instrument, NO_MSAA);
  if (THROTTLE) {
    console.log(`[proxy] main thread throttled ${THROTTLE}x (Emulation.setCPUThrottlingRate): a proxy, not a device measure`);
    await (await page.context().newCDPSession(page)).send('Emulation.setCPUThrottlingRate', { rate: THROTTLE });
  }
  const t0 = Date.now();
  await page.goto(`http://127.0.0.1:${port}/play.html`);
  out.startup.page_load_ms = Date.now() - t0;
  out.renderer = await rendererString(page);
  out.gpu_timer = await page.evaluate(() => {
    const gl = document.createElement('canvas').getContext('webgl2');
    return !!(gl && gl.getExtension('EXT_disjoint_timer_query_webgl2'));
  });
  console.log(`[bench] renderer: ${out.renderer}`);
  console.log(`[bench] GPU time: ${out.gpu_timer ? 'EXT_disjoint_timer_query_webgl2' : 'no timer query on this driver, display cadence only'}; `
    + `cadence: ${VSYNC ? 'vsync on, the rate a display would get' : 'uncapped, requestAnimationFrame as fast as the page goes'}`);
  // Module fetch + compile + instantiate, alone (the loader caches its promise).
  out.startup.wasm_instantiate_ms = await page.evaluate(async () => {
    const t = performance.now();
    await window.LegaiaWasm.load();
    return performance.now() - t;
  });
  /* The disc load, by export. */
  await page.evaluate(async () => {
    const m = await window.LegaiaWasm.load();
    for (const k of ['load_disc', 'load_disc_image', 'load_disc_spans']) {
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
  if (arg('reload')) {
    /* A returning visitor: the page again, its cached disc auto-loading. */
    const tr = Date.now();
    await page.reload();
    await page.waitForFunction(() => !!window.__playRuntime, null, { timeout: 300000 });
    out.startup.reload_disc_ready_ms = Date.now() - tr;
    out.startup.reload_file_read_ms = await page.evaluate(() => window.__bench.fileRead);
  }
  await page.evaluate(wrapForBench);
  if (UNCHECK.length) {
    await page.evaluate((ids) => {
      for (const id of ids) {
        const el = document.getElementById(id);
        if (!el) throw new Error('no option box ' + id);
        if (el.checked) el.click();
      }
    }, UNCHECK);
    out.unchecked = UNCHECK;
  }
  if (arg('idle')) await page.waitForTimeout(+arg('idle'));
  const cdp = await page.context().newCDPSession(page);
  /* The main thread's whole account: the renderer's own task time, of which the
   * frame function is a part. */
  await cdp.send('Performance.enable');
  let taskAt = 0, taskSpan = 0;
  await page.exposeFunction('__benchTask', async (phase) => {
    const { metrics } = await cdp.send('Performance.getMetrics');
    const now = metrics.find((m) => m.name === 'TaskDuration').value * 1000;
    if (phase === 'start') taskAt = now;
    else taskSpan = now - taskAt;
  });
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
    const r = await page.evaluate(async ({ sc, FRAMES, SKIP, reenter, canvasScale }) => {
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
      if (canvasScale !== 1) {
        const c = view.canvas;
        c.width = Math.round(960 * canvasScale);
        c.height = Math.round(720 * canvasScale);
      }
      const wait = (pred) => new Promise((res) => {
        const check = () => (pred() ? res() : setTimeout(check, 20));
        check();
      });
      const after = (n) => { const at = B.calls; return wait(() => B.calls - at >= n); };
      let firstFightEnded = null;
      if (sc.again) {
        /* The fight before this row's, played to its end the way a player's is: a
         * party strong enough and the confirm button, results screens included. If
         * it will not end, the way back to the field is the scene list. */
        await after(10);
        window.__playRuntime.cheat_apply('level', 99, '');
        window.__playRuntime.debug_force_battle(-1);
        const t = performance.now();
        while (performance.now() - t < 90000) {
          if (view._battle) break;
          await new Promise((res) => setTimeout(res, 50));
        }
        const tw = performance.now();
        while (view._battle && performance.now() - tw < 90000) {
          for (const type of ['keydown', 'keyup']) {
            window.dispatchEvent(new KeyboardEvent(type, { code: 'KeyZ', key: 'z', bubbles: true }));
          }
          await new Promise((res) => setTimeout(res, 120));
        }
        firstFightEnded = !view._battle;
        if (!firstFightEnded) btn.click();
      }
      /* The output comes up a moment after the pick. A battle edge read before it
       * is playing could not go dry. */
      const audio = () => {
        let stats = null;
        try { stats = JSON.parse(window.__playRuntime.audio_stats_json()); } catch (e) { /* down */ }
        return stats && Object.assign(stats, { context: B.audioCtx ? B.audioCtx.state : 'none' });
      };
      let audio0 = null;
      if (sc.battle) {
        /* Settled in the field first, so the battle edge's frame stands alone. */
        await after(10);
        const t = performance.now();
        while (performance.now() - t < 3000) {
          audio0 = audio();
          if (audio0 && audio0.played > 0) break;
          await new Promise((res) => setTimeout(res, 50));
        }
        await window.__benchBattleProfile('start');
      }
      if (sc.cast) window.__playRuntime.debug_seed_inflight_cast(...sc.cast);
      B.firstFrame = undefined; B.entryWorst = 0;
      if (sc.battle) window.__playRuntime.debug_force_battle(-1);
      const before = B.calls;
      let battleUp = !sc.battle;
      if (sc.battle) {
        /* Past the swirl: the battle edge lands on the intro's last frames, and the
         * intro runs on sim ticks. A frame count alone ends inside it on a page that
         * draws faster than the sim ticks. */
        const t = performance.now();
        await wait(() => !!view._battle || performance.now() - t > 30000);
        battleUp = !!view._battle;
        const landed = B.calls;
        await wait(() => B.calls - landed >= SKIP);
        await window.__benchBattleProfile('stop');
      }
      await wait(() => B.calls - before >= SKIP);
      let audioEdge = null;
      if (sc.battle) {
        /* The reports a stall stacked up are read in order, after it. */
        await new Promise((res) => setTimeout(res, 300));
        const a1 = audio();
        audioEdge = audio0 && a1 && { dry_frames: a1.dry_frames - audio0.dry_frames, played_frames: a1.played - audio0.played,
          rate: a1.rate, target_before: audio0.target, target_after: a1.target, path: a1.path, context: a1.context };
      }
      if (sc.pad === 'walk') {
        const send = (code, type) => window.dispatchEvent(new KeyboardEvent(type, { code, key: code, bubbles: true }));
        B.walk = setInterval(() => {
          const c = (B.walkN = (B.walkN || 0) + 1) % 2 ? 'ArrowDown' : 'ArrowUp';
          send(c === 'ArrowDown' ? 'ArrowUp' : 'ArrowDown', 'keyup');
          send(c, 'keydown');
        }, 3500);
      }
      const alloc0 = window.__wasmAllocTotals ? window.__wasmAllocTotals() : null;
      const steady0 = audio();
      await window.__benchTask('start');
      B.gaps = []; B.lastStart = null; B.gpu.ms = []; B.gpu.disjoint = 0;
      B.pump = { ms: 0, calls: 0, max: 0 };
      B.on = true;
      const start = B.calls;
      const tw = performance.now();
      await wait(() => B.calls - start >= FRAMES);
      const wall = performance.now() - tw;
      B.on = false;
      const steady1 = audio();
      await window.__benchTask('stop');
      /* The queries still in flight land within a few frames. */
      {
        const gl = view.renderer && view.renderer.gl;
        const t = performance.now();
        while (B.gpu.pending.length && performance.now() - t < 1000) {
          await new Promise((res) => setTimeout(res, 20));
          B.gpuPoll(gl);
        }
      }
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
        gaps: B.gaps, gpuMs: B.gpu.ms, gpuDisjoint: B.gpu.disjoint, pump: B.pump, audioEdge, battleUp, firstFightEnded,
        audioSteady: steady0 && steady1 && { dry_frames: steady1.dry_frames - steady0.dry_frames, played_frames: steady1.played - steady0.played,
          rate: steady1.rate, target_before: steady0.target, target_after: steady1.target, context: steady1.context },
        longtasks: ls, heap: performance.memory ? performance.memory.usedJSHeapSize : 0,
        state: view.scene, battle: typeof view.rt.state_json === 'function' ? view.rt.state_json().slice(0, 200) : '' };
    }, { sc, FRAMES, SKIP, reenter: !!arg('reenter'), canvasScale: CANVAS_SCALE });
    if (arg('shots')) {
      fs.mkdirSync(arg('shots'), { recursive: true });
      await page.screenshot({ path: path.join(arg('shots'), `${name}.png`) });
    }
    const frameSum = r.frames.reduce((a, x) => a + x, 0);
    const s = summarise({ exports: r.exports, gl: r.gl, firstFrame: r.firstFrame, entryWorst: r.entryWorst,
      gaps: r.gaps, gpuMs: r.gpuMs, gpuDisjoint: r.gpuDisjoint }, r.frames);
    s.main_thread = { task_ms_per_frame: taskSpan / FRAMES, frame_fn_ms_per_frame: frameSum / FRAMES,
      outside_ms_per_frame: (taskSpan - frameSum) / FRAMES, audio_pump_ms_per_frame: r.pump.ms / FRAMES,
      audio_pump_calls: r.pump.calls, audio_pump_max_ms: r.pump.max };
    if (sc.battle) {
      s.audio_edge = r.audioEdge;
      s.battle_up = r.battleUp;
      if (r.firstFightEnded !== null) {
        s.first_fight_ended = r.firstFightEnded;
        console.log(`  ${name}: the fight before this one ${r.firstFightEnded ? 'was won and left through its results' : 'did not end in 90 s; back to the field by the scene list'}`);
      }
      if (!r.battleUp) console.log(`  ${name}: NOT A BATTLE ROW, the page's battle scene never came up in 30 s`);
    }
    s.audio_steady = r.audioSteady;
    s.memory = await memoryRow(page);
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
      + `allocs/frame ${s.allocs_per_frame === undefined ? '-' : s.allocs_per_frame.toFixed(0)}; heap ${s.heap_mb.toFixed(0)} MB; long tasks ${s.longtasks}, worst ${s.longtask_max.toFixed(0)} ms`);
    console.log('  exports: ' + s.exports.slice(0, 14).map((e) => `${e.k} ${e.ms.toFixed(2)}ms/${e.calls.toFixed(1)}x${e.kb >= 1 ? '/' + e.kb.toFixed(0) + 'KB' : ''}`).join(', '));
    console.log('  entry: ' + r.entryExports.map(([k, ms, c]) => `${k} ${ms}ms/${c}x`).join(', '));
    console.log('  gl: ' + s.gl.slice(0, 12).map(([k, c]) => `${k} ${c.toFixed(0)}`).join(', '));
    printRows(s);
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
      /* Self time outside the frame function, by the function that spends it. */
      const inFrame = (id) => {
        for (let at = id; at !== undefined; at = parents.get(at)) {
          if (/_frame$/.test(byId.get(at).callFrame.functionName)) return true;
        }
        return false;
      };
      const outside = new Map();
      profile.samples.forEach((id, i) => {
        const k = byId.get(id).callFrame.functionName || '(anon)';
        if (k === '(idle)' || k === '(root)' || inFrame(id)) return;
        outside.set(k, (outside.get(k) || 0) + dts[i]);
      });
      s.outside_frame_profile = [...outside.entries()].sort((x, y) => y[1] - x[1]).slice(0, 12)
        .map(([k, v]) => [k, +(v * 100 / tot).toFixed(1)]);
      console.log('  outside frame function, self%: ' + s.outside_frame_profile.map(([k, v]) => `${k} ${v}`).join(', '));
    }
  }
  /* Can the dry-frame row read anything here? A stall long past the queue must. */
  out.audio_self_check = await page.evaluate(async () => {
    const rt = window.__playRuntime;
    const B = window.__bench;
    let a0 = null;
    try { a0 = JSON.parse(rt.audio_stats_json()); } catch (e) { /* down */ }
    if (!a0) return { reading: false, why: 'no audio output on the page' };
    const t = performance.now();
    while (performance.now() - t < 1000); /* eslint-disable-line no-empty */
    await new Promise((res) => setTimeout(res, 500));
    const a1 = JSON.parse(rt.audio_stats_json());
    return { reading: a1.dry_frames > a0.dry_frames, dry_frames: a1.dry_frames - a0.dry_frames,
      played_frames: a1.played - a0.played, context: B.audioCtx ? B.audioCtx.state : 'none', path: a1.path };
  });
  const c = out.audio_self_check;
  console.log(c.reading
    ? `[bench] dry-frame self-check: a 1000 ms stall read ${c.dry_frames} dry frames (${c.path}, context ${c.context}): the row reads something`
    : `[bench] dry-frame self-check FAILED, the dry-frame rows prove nothing here: ${JSON.stringify(c)}`);
  /* And the GL tally, on objects whose size is known. */
  out.gl_tally_check = await page.evaluate(() => {
    const B = window.__bench;
    const gl = document.createElement('canvas').getContext('webgl2');
    const sum = () => { const h = B.glHeldBytes(); return h.buffers + h.textures + h.renderbuffers; };
    const at = sum();
    const tex = gl.createTexture();
    gl.bindTexture(gl.TEXTURE_2D, tex);
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA8, 256, 256, 0, gl.RGBA, gl.UNSIGNED_BYTE, null);
    const buf = gl.createBuffer();
    gl.bindBuffer(gl.ARRAY_BUFFER, buf);
    gl.bufferData(gl.ARRAY_BUFFER, new Float32Array(1000), gl.STATIC_DRAW);
    const rb = gl.createRenderbuffer();
    gl.bindRenderbuffer(gl.RENDERBUFFER, rb);
    gl.renderbufferStorageMultisample(gl.RENDERBUFFER, 4, gl.RGBA8, 64, 64);
    const up = sum() - at;
    gl.deleteTexture(tex); gl.deleteBuffer(buf); gl.deleteRenderbuffer(rb);
    return { want: 256 * 256 * 4 + 4000 + 64 * 64 * 4 * 4, got: up, back: sum() - at };
  });
  {
    const g = out.gl_tally_check;
    console.log(g.want === g.got && g.back === 0
      ? `[bench] GL tally self-check: ${g.got} bytes asked for and let go, as known`
      : `[bench] GL tally self-check FAILED: ${JSON.stringify(g)}`);
  }
  await page.context().close();
  if (arg('paths')) {
    for (const kind of ['new_game', 'card_load']) {
      out.paths[kind] = await playerPath(browser, port, kind, errors);
    }
  }
  console.log(JSON.stringify({ wasm: out.wasm, startup: out.startup, renderer: out.renderer, errors }));
  if (arg('json')) fs.writeFileSync(arg('json'), JSON.stringify(out, null, 1));
  await browser.close();
  stop();
}

main().catch((e) => { console.error(e); process.exit(1); });
