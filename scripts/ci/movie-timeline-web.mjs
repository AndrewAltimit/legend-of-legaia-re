#!/usr/bin/env node
// One movie start on the browser play page, as a timeline. Measurement only: everything
// is hooked from outside the page, so a player's page carries none of it.
//
//   scripts/ci/movie-timeline-web.mjs [--site site] [--fmv 0] [--route field|attract]
//       [--no-fill] [--no-fill-wait] [--evict] [--reload] [--again] [--read-delay ms]
//       [--settle frames] [--ctx-suspend ms] [--cpu-throttle n] [--gl egl|swiftshader] [--watch ms]
//       [--append out.jsonl] [--label s]
//
// Prints one JSON line per movie start (and appends it to `--append`): milliseconds from
// the request to the span resident, the install, the first decode, the first picture, the
// mixer's first sound and the first picture to move, plus the worst display-frame gap, the
// long tasks, wasm memory and the box's load. Run it once per flock of the display lock;
// the distribution is the caller's. Timing on a busy box says little: read `box`.
//
// `--route field` enters town01 and writes the movie trigger (`play_fmv_trigger`);
// `attract` sits on the title until its countdown arms MV1, as a player does.
// `--no-fill` stops the background fill of an older bundle (`--site`), so the movie's span is
// read on demand, as every bundle since reads it. `--no-fill-wait` asks while that fill is
// still running. `--evict` drops the disc from the
// OS page cache first, so the read reaches storage. `--reload` is a returning visitor: the
// disc off the page's IndexedDB cache. `--again` plays the movie a second time in the same
// page. `--settle` is how many display frames town01 runs before the trigger (default 120);
// the page's voice banks stage off the main thread's turns for some seconds after the disc
// load, and a small value lands the movie among them. `--read-delay` holds a span's read that long, to walk the install timeout.
// `--ctx-suspend` suspends the audio context just before the trigger and resumes it that
// much later: an output that is slow to start (the driver's browser has no autoplay gate).
// `--cpu-throttle` is a proxy, and says nothing of a device. "first sound" is the mixer's
// cursor passing zero, which is when samples are queued, ahead of the device's latency.

import fs from 'fs';
import path from 'path';
import { execFileSync } from 'child_process';
import { fileURLToPath } from 'url';
import { launchChromium, serveSite } from './browser-session.mjs';

const arg = (name, dflt) => {
  const i = process.argv.indexOf('--' + name);
  return i < 0 ? dflt : (process.argv[i + 1] && !process.argv[i + 1].startsWith('--') ? process.argv[i + 1] : true);
};
const REPO = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const site = path.resolve(arg('site', path.join(REPO, 'site')));
const disc = process.env.LEGAIA_DISC_BIN;
if (!disc || !fs.existsSync(disc) || !fs.existsSync(path.join(site, 'wasm', 'legaia_web_viewer_bg.wasm'))) {
  console.log('[skip] movie-timeline-web needs LEGAIA_DISC_BIN and a built site/wasm bundle');
  process.exit(0);
}
const cfg = { noFill: !!arg('no-fill'), readDelay: +arg('read-delay', 0) };
const ROUTE = arg('route', 'field');
const FMV = +arg('fmv', 0);
const WATCH = +arg('watch', 6000);
const SETTLE = +arg('settle', 120);
const THROTTLE = +arg('cpu-throttle', 0);

/* Runs in the page, before any of its scripts. */
function hooks(cfg) {
  const fresh = () => ({ ev: {}, raf: [], decodes: [], due: [], want: null, mem: {} });
  const M = window.__mt = { ...fresh(), long: [], fresh, ctx: null, xa: [], reads: [], put: [] };
  const now = () => performance.now();
  const once = (k) => { if (M.ev[k] === undefined) M.ev[k] = now(); };
  const wasmBytes = () => { try { return window.__mtMemory ? window.__mtMemory.buffer.byteLength : 0; } catch (e) { return 0; } };
  try {
    new PerformanceObserver((l) => { for (const e of l.getEntries()) M.long.push([e.startTime, e.duration]); })
      .observe({ entryTypes: ['longtask'] });
  } catch (e) { /* no reading */ }
  /* Whether the scene under the movie's overlay is still drawn. */
  for (const P of [window.WebGL2RenderingContext, window.WebGLRenderingContext]) {
    if (!P) continue;
    for (const k of ['drawArrays', 'drawElements']) {
      const f = P.prototype[k];
      P.prototype[k] = function (...a) { M.glDraws = (M.glDraws || 0) + 1; return f.apply(this, a); };
    }
  }
  const raf = window.requestAnimationFrame.bind(window);
  window.requestAnimationFrame = (cb) => raf((t) => { const s = now(); cb(t); M.raf.push([s, now() - s]); if (M.raf.length > 20000) M.raf.shift(); });
  if (window.AudioContext) {
    const Base = window.AudioContext;
    window.AudioContext = class extends Base { constructor(...a) { super(...a); M.ctx = this; } };
  }
  const grab = WebAssembly.instantiate;
  const takeMemory = (r) => {
    const inst = r.instance || r;
    if (inst.exports && inst.exports.memory) window.__mtMemory = inst.exports.memory;
    return r;
  };
  WebAssembly.instantiate = (...a) => grab(...a).then(takeMemory);
  if (WebAssembly.instantiateStreaming) {
    const grabS = WebAssembly.instantiateStreaming;
    WebAssembly.instantiateStreaming = (...a) => grabS(...a).then(takeMemory);
  }
  let cache0;
  Object.defineProperty(window, 'RomCache', { configurable: true, get: () => cache0, set: (v) => {
    const put = v.put;
    v.put = (...a) => { const e = [now(), null]; M.put.push(e); return put.apply(v, a).finally(() => { e[1] = now(); }); };
    cache0 = v;
  } });
  const hookDisc = (d) => {
    if (cfg.noFill && d.startFill) d.startFill = () => {};
    /* A reader with no fill worker: hold the movie's read in flight at the Blob, so what is
     * walked is the reader's own wait, with the read landing after a timeout included. */
    if (cfg.readDelay && !d.startFill) {
      const real = d.blob;
      d.blob = { size: real.size, slice(s, e) {
        const b = real.slice(s, e);
        const held = M.want && s === M.want.first_sector * 2352;
        return { arrayBuffer: () => (held ? new Promise((res) => setTimeout(res, cfg.readDelay)).then(() => b.arrayBuffer()) : b.arrayBuffer()) };
      } };
    }
    const heldAtTheBlob = cfg.readDelay && !d.startFill;
    const span = d.span.bind(d), prefetch = d.prefetch.bind(d);
    d.span = (lba, n) => {
      const r = span(lba, n);
      if (M.want && lba === M.want.first_sector && n === M.want.sector_count) {
        M.ev.span_asked = M.ev.span_asked ?? now();
        if (r) once('resident');
      }
      return r;
    };
    /* The one real read of a span, timed from the call to the bytes. */
    const timedRead = (lba, n) => {
      const t0 = now();
      const p = prefetch(lba, n);
      p.then(() => { M.reads.push([lba, n, t0, now() - t0]); });
      return p;
    };
    d.prefetch = (lba, n) => {
      if (M.want && lba === M.want.first_sector) {
        once('prefetch');
        M.prefetchCalls = (M.prefetchCalls || 0) + 1;
      }
      /* Only the movie's own span: the voice banks' reads pass. */
      const movie = M.want && lba === M.want.first_sector;
      if (!movie) return prefetch(lba, n);
      if (heldAtTheBlob) return timedRead(lba, n);
      if (cfg.readDelay && !d.__held) {
        d.__held = true;
        return new Promise((res) => setTimeout(res, cfg.readDelay)).then(() => timedRead(lba, n)).finally(() => { d.__held = false; });
      }
      return cfg.readDelay ? Promise.resolve() : timedRead(lba, n);
    };
  };
  let disc0;
  Object.defineProperty(window, 'LegaiaPlayDisc', { configurable: true, get: () => disc0, set: (v) => {
    const open = v.open;
    v.open = async (...a) => { const d = await open(...a); hookDisc(d); return d; };
    disc0 = v;
  } });
  const wrap = (o, name, after) => {
    const f = o[name];
    if (typeof f !== 'function') return;
    o[name] = function (...a) {
      const t0 = now();
      const r = f.apply(this, a);
      after(t0, now(), r, a);
      return r;
    };
  };
  const hookRuntime = (rt) => {
    wrap(rt, 'play_xa_install', (t0, t1, r, a) => { M.xa.push([a[0], t0, t1 - t0, a[1].length]); });
    wrap(rt, 'play_fmv_trigger', (t0) => { M.ev.trigger = t0; });
    wrap(rt, 'play_fmv_wanted_json', (t0, t1, r) => {
      if (r && r !== 'null' && !M.want) { M.want = JSON.parse(r); M.ev.wanted = t0;
        M.fill_landed_at_want = !!(window.__playDisc && (window.__playDisc.fillMs !== undefined || !window.__playDisc.startFill)); }
      if (M.want && r === 'null' && M.ev.install_start === undefined && M.ev.gave_up === undefined) M.ev.gave_up = t0;
    });
    wrap(rt, 'play_fmv_install', (t0, t1, r, a) => {
      M.ev.install_start = t0; M.ev.install_end = t1; M.install_ok = r; M.install_bytes = a[0].length;
      M.mem.wasm_after_install = wasmBytes();
      M.ctx_at_install = M.ctx ? M.ctx.state : 'none';
      M.ctx_time_at_install = M.ctx ? M.ctx.currentTime : -1;
    });
    const inst = rt.play_fmv_install;
    rt.play_fmv_install = function (s) { M.mem.wasm_before_install = wasmBytes(); return inst.call(this, s); };
    wrap(rt, 'play_fmv_audio_start', (t0, t1, r) => { M.ev.audio_start = t0; M.ev.audio_start_end = t1; M.engine_audio = r; });
    wrap(rt, 'play_fmv_audio_pcm_i16', (t0, t1, r) => { M.pcm_fallback_len = r.length; });
    wrap(rt, 'play_fmv_frame_rgba', (t0, t1, r, a) => {
      if (M.ev.first_decode === undefined) { M.ev.first_decode = t0; M.ev.first_decode_end = t1; M.glAtFirst = M.glDraws || 0; }
      M.glAtLast = M.glDraws || 0;
      if (M.decodes.length < 2000) M.decodes.push([a[0], t0, t1 - t0, r.length]);
    });
    const cursor = rt.play_fmv_audio_cursor_secs;
    wrap(rt, 'play_fmv_due_frame', (t0, t1, r, a) => {
      const c = cursor ? cursor.call(rt) : -1;
      if (c > 0) once('first_sound');
      if (r > 0) once('first_due');
      if (M.due.length < 4000) { const l = M.due[M.due.length - 1]; if (!l || l[1] !== r) M.due.push([t0, r, c, a[0]]); }
    });
    wrap(rt, 'play_fmv_finish', (t0) => { M.ev.finish = t0; });
  };
  let rt0;
  Object.defineProperty(window, '__playRuntime', { configurable: true, get: () => rt0, set: (v) => { if (v) hookRuntime(v); rt0 = v; } });
}

function boxNow() {
  const stat = () => {
    const f = fs.readFileSync('/proc/stat', 'latin1').split('\n')[0].trim().split(/\s+/).slice(1).map(Number);
    return { busy: f[0] + f[1] + f[2] + f[5] + f[6] + f[7], all: f.reduce((a, x) => a + x, 0) };
  };
  return { stat, load: () => fs.readFileSync('/proc/loadavg', 'latin1').split(' ').slice(0, 3).map(Number) };
}

function row(M, box0, box1, extra) {
  const e = M.ev;
  const from = e.trigger ?? e.wanted;
  const rel = (t) => (t === undefined ? null : +(t - from).toFixed(1));
  const draws = M.decodes.filter((d) => d[3] > 0);
  const firstMove = draws.find((d) => d[0] > 0);
  const firstPaint = e.first_decode_end === undefined ? undefined : (M.raf.find((r) => r[0] > e.first_decode_end) || [undefined])[0];
  const end = from + WATCH;
  const gaps = M.raf.filter((r) => r[0] >= from && r[0] < end).map((r) => r[0]);
  let worst = 0, worstAt = 0;
  for (let i = 1; i < gaps.length; i++) if (gaps[i] - gaps[i - 1] > worst) { worst = gaps[i] - gaps[i - 1]; worstAt = gaps[i - 1]; }
  /* Longest the picture held still once it was up. */
  let hold = 0, holdFrom = 0;
  for (let i = 1; i < draws.length; i++) if (draws[i][1] - draws[i - 1][1] > hold) { hold = draws[i][1] - draws[i - 1][1]; holdFrom = draws[i - 1][0]; }
  const during = M.raf.filter((r) => firstPaint !== undefined && r[0] > firstPaint && r[0] < end).map((r) => r[1]).sort((a, b) => a - b);
  const rafTask = during.length ? { n: during.length, p50: +during[during.length >> 1].toFixed(1), max: +during[during.length - 1].toFixed(1) } : null;
  const dec = draws.slice(0, 60).map((d) => d[2]).sort((a, b) => a - b);
  const jump = M.due.filter((d) => d[1] > 0)[0];
  return {
    ...extra,
    wanted: M.want && { first_sector: M.want.first_sector, sector_count: M.want.sector_count, path: M.want.path },
    fill_landed_at_want: M.fill_landed_at_want,
    prefetch_calls: M.prefetchCalls || 0,
    ms: {
      wanted: rel(e.wanted), gave_up: rel(e.gave_up), prefetch: rel(e.prefetch), resident: rel(e.resident),
      install_start: rel(e.install_start), install: e.install_end === undefined ? null : +(e.install_end - e.install_start).toFixed(1),
      audio_start: rel(e.audio_start), audio_start_call: e.audio_start === undefined ? null : +(e.audio_start_end - e.audio_start).toFixed(1),
      first_decode: rel(e.first_decode), first_decode_call: e.first_decode === undefined ? null : +(e.first_decode_end - e.first_decode).toFixed(1),
      first_picture: rel(firstPaint), first_sound: rel(e.first_sound), first_motion: firstMove ? rel(firstMove[1]) : null,
      worst_raf_gap: +worst.toFixed(1), worst_raf_gap_at: rel(worstAt), worst_hold: +hold.toFixed(1), worst_hold_after_frame: holdFrom,
      decode_p50: dec.length ? +dec[dec.length >> 1].toFixed(1) : null, decode_max: dec.length ? +dec[dec.length - 1].toFixed(1) : null,
    },
    first_due: jump ? { frame: jump[1], cursor: +jump[2].toFixed(3), wall: +jump[3].toFixed(3) } : null,
    engine_audio: M.engine_audio, ctx_at_install: M.ctx_at_install, ctx_time_at_install: M.ctx_time_at_install,
    pcm_fallback_len: M.pcm_fallback_len, install_ok: M.install_ok, install_bytes: M.install_bytes,
    frames_drawn: draws.length, gl_draws_during_movie: (M.glAtLast || 0) - (M.glAtFirst || 0),
    raf_task_ms: rafTask, finished: e.finish !== undefined,
    wasm_mb: { before: +(M.mem.wasm_before_install / 1048576 || 0).toFixed(0), after: +(M.mem.wasm_after_install / 1048576 || 0).toFixed(0) },
    span_reads: (M.reads || []).filter((x) => M.want && x[0] === M.want.first_sector).map((x) => [rel(x[2]), +x[3].toFixed(1)]),
    rom_cache_put: (M.put || []).map((x) => [rel(x[0]), x[1] === null ? null : rel(x[1])]),
    xa_installs: M.xa.map((x) => [x[0].replace(/^.*\//, ''), rel(x[1]), +x[2].toFixed(0), +(x[3] / 1048576).toFixed(1)]),
    long_tasks: M.long.filter((l) => l[0] + l[1] >= from && l[0] < end).map((l) => [rel(l[0]), +l[1].toFixed(0)]),
    box: { load: box1.load, cpu_busy_pct: +(100 * (box1.busy - box0.busy) / Math.max(1, box1.all - box0.all)).toFixed(1) },
  };
}

const { stat, load } = boxNow();
if (arg('evict')) {
  /* Drops clean pages only; needs no privilege and leaves the file alone. */
  execFileSync('python3', ['-I', '-c', 'import os,sys;f=os.open(sys.argv[1],os.O_RDONLY);os.posix_fadvise(f,0,0,os.POSIX_FADV_DONTNEED)', disc]);
}
const server = await serveSite(site);
const browser = await launchChromium({ gl: arg('gl', 'egl'), vsync: true });
const context = await browser.newContext({ viewport: { width: 1280, height: 900 } });
const page = await context.newPage();
const errors = [];
page.on('pageerror', (e) => errors.push(e.message));
const logs = [];
page.on('console', (m) => { const t = m.text(); if (/^(fmv|cutscene):/.test(t)) logs.push(t); });
await page.addInitScript(hooks, cfg);
if (THROTTLE) await (await context.newCDPSession(page)).send('Emulation.setCPUThrottlingRate', { rate: THROTTLE });
const extra = { settle: SETTLE, label: arg('label', ''), route: ROUTE, no_fill: cfg.noFill, evict: !!arg('evict'), reload: !!arg('reload'), throttle: THROTTLE, read_delay: cfg.readDelay, ctx_suspend: +arg('ctx-suspend', 0) };
const out = [];
const emit = (r) => {
  const line = JSON.stringify(r);
  console.log(line);
  if (arg('append')) fs.appendFileSync(arg('append'), line + '\n');
};
try {
  await page.goto(`http://127.0.0.1:${server.port}/play.html`);
  await (await page.$('input[type=file]')).setInputFiles(disc);
  await page.waitForFunction(() => !!window.__playRuntime, null, { timeout: 300000 });
  const waitFill = () => page.waitForFunction(() => window.__playDisc && (window.__playDisc.fillMs !== undefined || !window.__playDisc.startFill), null, { timeout: 180000 });
  if (arg('reload')) {
    if (!cfg.noFill) await waitFill();
    /* The cache's put lands after the first read. */
    await page.waitForFunction(async () => !!(await window.RomCache.meta()), null, { timeout: 180000, polling: 500 });
    await page.reload();
    await page.waitForFunction(() => !!window.__playRuntime, null, { timeout: 300000 });
  }
  if (!cfg.noFill && !arg('no-fill-wait')) await waitFill();
  const fillMs = await page.evaluate(() => window.__playDisc.fillMs ?? null);
  extra.fill_ms = fillMs;
  if (ROUTE === 'attract') {
    await page.evaluate(() => { document.querySelector('[data-mode="newgame"]').click(); });
  } else {
    await page.evaluate(() => { document.querySelector('[data-scene="town01"]').click(); });
    await page.waitForFunction((n) => { const M = window.__mt; M.rafAtClick = M.rafAtClick ?? M.raf.length; return M.raf.length - M.rafAtClick >= n; }, SETTLE, { timeout: 60000, polling: 'raf' });
  }
  const once = async (pass) => {
    const box0 = stat();
    if (ROUTE === 'attract' && pass === 0) {
      await page.waitForFunction(() => !!window.__mt.want, null, { timeout: 240000, polling: 50 });
    } else {
      const ok = await page.evaluate(async ({ id, hold }) => {
        const M = window.__mt;
        if (hold && M.ctx) { await M.ctx.suspend(); setTimeout(() => M.ctx.resume(), hold); }
        Object.assign(M, M.fresh());
        return window.__playRuntime.play_fmv_trigger(id);
      }, { id: FMV, hold: +arg('ctx-suspend', 0) });
      if (!ok) throw new Error('play_fmv_trigger refused');
    }
    /* From the request's clock, not this driver's: a stalled page answers late. */
    await page.waitForFunction((w) => {
      const M = window.__mt, f = M.ev.trigger ?? M.ev.wanted;
      return f !== undefined && performance.now() - f > w && (M.ev.finish !== undefined || M.ev.install_end !== undefined || performance.now() - f > 20000);
    }, WATCH, { timeout: 120000 + cfg.readDelay, polling: 200 });
    const M = await page.evaluate(() => { const M = window.__mt; return { ev: M.ev, xa: M.xa, reads: M.reads, put: M.put, raf: M.raf, decodes: M.decodes, due: M.due, want: M.want, mem: M.mem, long: M.long,
      prefetchCalls: M.prefetchCalls, glAtFirst: M.glAtFirst, glAtLast: M.glAtLast, fill_landed_at_want: M.fill_landed_at_want, engine_audio: M.engine_audio, ctx_at_install: M.ctx_at_install,
      ctx_time_at_install: M.ctx_time_at_install, pcm_fallback_len: M.pcm_fallback_len, install_ok: M.install_ok, install_bytes: M.install_bytes,
      skipped: window.__playRuntime.play_fmv_id() < 0 }; });
    const box1 = { ...stat(), load: load() };
    emit({ ...row(M, box0, box1, { ...extra, pass }), skipped_or_gone: M.skipped, errors: errors.slice(), logs: logs.splice(0) });
  };
  await once(0);
  if (arg('again')) {
    await page.evaluate(() => window.__playRuntime.play_fmv_finish());
    await page.waitForFunction(() => !window.LegaiaPlayFmv.active(), null, { timeout: 30000 });
    await page.waitForTimeout(1500);
    await once(1);
  }
} catch (e) {
  emit({ ...extra, failed: String(e), errors, logs });
} finally {
  await browser.close();
  server.stop();
}
