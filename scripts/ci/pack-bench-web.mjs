#!/usr/bin/env node
// What an expansion pack costs in the browser, the sibling of frame-bench-web.mjs.
//
//   scripts/ci/pack-bench-web.mjs [--site site] [--maps genesis_1f,isles_beach,isles_village]
//       [--repeat 5] [--cpu-throttle n] [--profile] [--json out.json]
//   scripts/ci/pack-bench-web.mjs --entry [--site site] [--map isles_beach] [--cpu-throttle n]
//       [--profile] [--json out.json]
//
// The default is the workbench's painter: opens the sample pack over the disc named by
// LEGAIA_DISC_BIN, picks a brush off each map and paints single tiles with it through the
// canvas's own mouse events, and reads per edit how long the edit held the page, its first
// frame, the `map_preview` round trip and the preview drawn on the canvas. Each map also
// runs `map_preview` on the page's thread, for its size and cost alone.
//
// `--entry` is the play page: enables the sample pack through the library row, then enters
// the map through the scene list, and reads the time to its first frame by wasm export.
//
// `--cpu-throttle n` slows the renderer's main thread n times, and neither the workbench's
// validation worker nor anything else: a proxy, labelled one. A row of the painter's that
// is the worker's own (`worker_ms`) is not throttled, so read `page_ms` for how it scales.
// Prints numbers and asserts nothing; docs/tooling/frame-bench.md says how to read them.

import fs from 'fs';
import path from 'path';
import { fileURLToPath } from 'url';
import { launchChromium, serveSite } from './browser-session.mjs';

const arg = (name, dflt) => {
  const i = process.argv.indexOf('--' + name);
  return i < 0 ? dflt : (process.argv[i + 1] && !process.argv[i + 1].startsWith('--') ? process.argv[i + 1] : true);
};
const REPO = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const site = path.resolve(arg('site', path.join(REPO, 'site')));
const THROTTLE = +arg('cpu-throttle', 0);
const MAPS = arg('maps', 'genesis_1f,isles_beach,isles_village').split(',');
const REPEAT = +arg('repeat', 5);
const MAP = arg('map', 'isles_beach');
const disc = process.env.LEGAIA_DISC_BIN;
if (!disc || !fs.existsSync(disc) || !fs.existsSync(path.join(site, 'wasm', 'legaia_web_viewer_bg.wasm'))) {
  console.log('[skip] pack-bench-web needs LEGAIA_DISC_BIN and a built site/wasm bundle');
  process.exit(0);
}

const median = (rows, key) => {
  const a = rows.map((r) => r[key]).filter((x) => x != null).sort((x, y) => x - y);
  return a.length ? +a[a.length >> 1].toFixed(1) : null;
};

function printProfile(profile) {
  const byId = new Map(profile.nodes.map((n) => [n.id, n]));
  const parent = new Map();
  for (const n of profile.nodes) for (const c of n.children || []) parent.set(c, n.id);
  const name = (id) => byId.get(id).callFrame.functionName || '(anon)';
  const self = new Map();
  const inclusive = new Map();
  profile.samples.forEach((id, i) => {
    const d = profile.timeDeltas[i];
    self.set(name(id), (self.get(name(id)) || 0) + d);
    const seen = new Set();
    for (let x = id; x != null; x = parent.get(x)) {
      const k = name(x);
      if (!seen.has(k)) { seen.add(k); inclusive.set(k, (inclusive.get(k) || 0) + d); }
    }
  });
  for (const [title, m] of [['self', self], ['inclusive', inclusive]]) {
    console.log(`  ${title} ms:`);
    for (const [k, v] of [...m.entries()].sort((a, b) => b[1] - a[1]).slice(0, 25)) {
      console.log(`  ${(v / 1000).toFixed(1).padStart(9)} ${k.slice(0, 80)}`);
    }
  }
}

async function openPage(browser, url) {
  const page = await (await browser.newContext({ viewport: { width: 1400, height: 1000 } })).newPage();
  page.on('pageerror', (e) => console.error('PAGEERR', e.message));
  const cdp = await page.context().newCDPSession(page);
  await page.goto(url);
  return { page, cdp };
}

async function throttle(cdp) {
  if (!THROTTLE) return;
  console.log(`[proxy] main thread throttled ${THROTTLE}x (Emulation.setCPUThrottlingRate): not the worker, not a device measure`);
  await cdp.send('Emulation.setCPUThrottlingRate', { rate: THROTTLE });
}

async function startProfile(cdp) {
  if (!arg('profile')) return;
  await cdp.send('Profiler.enable');
  await cdp.send('Profiler.setSamplingInterval', { interval: 500 });
  await cdp.send('Profiler.start');
}

async function stopProfile(cdp) {
  if (!arg('profile')) return;
  printProfile((await cdp.send('Profiler.stop')).profile);
}

/* One map's brush and the tiles it paints: the second commonest legend character, onto
 * tiles of the commonest. */
function paintPlan(id) {
  const m = window.__xwState.pack.maps[id];
  const { rows, legend } = m.tiles;
  const count = {};
  for (const r of rows) for (const ch of r) count[ch] = (count[ch] || 0) + 1;
  const best = Object.keys(count).filter((c) => legend[c]).sort((a, b) => count[b] - count[a]);
  const brush = best[1];
  let pick = null;
  const paint = [];
  for (let r = 1; r < rows.length - 1; r++) {
    for (let c = 1; c < rows[0].length - 1; c++) {
      if (!pick && rows[r][c] === brush) pick = [c, r];
      else if (paint.length < 32 && rows[r][c] === best[0] && (c + r) % 5 === 0) paint.push([c, r]);
    }
  }
  return { brush, onto: best[0], pick, paint, width: rows[0].length };
}

async function painter(browser, port, out) {
  const { page, cdp } = await openPage(browser, `http://127.0.0.1:${port}/tooling/expansion-workbench.html`);
  await page.setInputFiles('#xw-file', disc);
  await page.waitForFunction(() => {
    const S = window.__xwState;
    return S && S.wb && S.wb.has_disc() && window.__xwValidator.ready;
  }, null, { timeout: 300000 });
  await page.evaluate(() => document.getElementById('xw-sample').click());
  await page.waitForFunction(() => {
    const p = window.__xwState.pack;
    return p && Object.keys(p.maps || {}).length > 0;
  }, null, { timeout: 120000 });
  await page.evaluate(() => {
    const S = window.__xwState;
    S.tab = 'maps';
    window.__xwCtx.rerender();
    // The sample pack opens in the 3D view: the painter's canvas beside it.
    S.preview.setView('split');
  });
  await page.waitForTimeout(3000);
  await throttle(cdp);
  await page.evaluate(() => {
    const ctx = window.__xwCtx;
    const later = ctx.mapPreviewLater;
    window.__pb = { rows: [], t0: 0 };
    const T = window.__pb;
    ctx.mapPreviewLater = (id, json, done) => {
      const asked = performance.now();
      later(id, json, (pv) => {
        const landed = performance.now();
        done(pv);
        T.rows.push({ asked: asked - T.t0, landed: landed - T.t0, drawn: performance.now() - T.t0 });
      });
    };
  });
  const press = (key) => page.evaluate((k) => document.dispatchEvent(new KeyboardEvent('keydown', { key: k, bubbles: true })), key);
  const click = (tile, width) => page.evaluate(({ t, w }) => {
    const cv = document.querySelector('.xw-canvas-wrap canvas');
    const b = cv.getBoundingClientRect();
    const cs = cv.width / w;
    const at = { clientX: b.left + (t[0] + 0.5) * cs, clientY: b.top + (t[1] + 0.5) * cs, button: 0, bubbles: true };
    cv.dispatchEvent(new MouseEvent('mousedown', at));
    document.dispatchEvent(new MouseEvent('mouseup', at));
  }, { t: tile, w: width });
  for (const id of MAPS) {
    await page.evaluate((m) => {
      const sel = document.querySelector('select[aria-label="Map"]');
      sel.value = m;
      sel.dispatchEvent(new Event('change'));
    }, id);
    await page.waitForTimeout(2500);
    const plan = await page.evaluate(paintPlan, id);
    await press('i');
    await click(plan.pick, plan.width);
    await page.waitForTimeout(200);
    await press('b');
    await page.waitForTimeout(800);
    await startProfile(cdp);
    const rows = [];
    for (let i = 0; i < Math.min(REPEAT, plan.paint.length); i++) {
      rows.push(await page.evaluate(async ({ t, w, m }) => {
        const T = window.__pb;
        const V = window.__xwValidator;
        const cv = document.querySelector('.xw-canvas-wrap canvas');
        const b = cv.getBoundingClientRect();
        const cs = cv.width / w;
        const at = { clientX: b.left + (t[0] + 0.5) * cs, clientY: b.top + (t[1] + 0.5) * cs, button: 0, bubbles: true };
        T.rows.length = 0;
        T.t0 = performance.now();
        cv.dispatchEvent(new MouseEvent('mousedown', at));
        const down = performance.now() - T.t0;
        await new Promise((res) => requestAnimationFrame(() => res()));
        const tile = performance.now() - T.t0;
        // The press ends: the stroke's commit and the tab's re-render.
        const t1 = performance.now();
        document.dispatchEvent(new MouseEvent('mouseup', at));
        const up = performance.now() - t1;
        const end = performance.now() + 4000;
        while (performance.now() < end && !T.rows.length) await new Promise((res) => setTimeout(res, 5));
        const painted = window.__xwState.pack.maps[m].tiles.rows[t[1]][t[0]];
        return { down, tile, up, painted, ...(T.rows[0] || {}), worker: V.previewMs[V.previewMs.length - 1], hook: V.previewDrawnMs ? V.previewDrawnMs[V.previewDrawnMs.length - 1] : null };
      }, { t: plan.paint[i], w: plan.width, m: id }));
      // Past the 3D preview's rebuild of this edit, which holds the page's thread.
      await page.waitForTimeout(4000);
    }
    await stopProfile(cdp);
    const alone = await page.evaluate((m) => {
      const S = window.__xwState;
      const json = JSON.stringify(S.pack.maps[m]);
      const times = [];
      let bytes = 0;
      for (let i = 0; i < 5; i++) {
        const t = performance.now();
        bytes = S.wb.map_preview(json).length;
        times.push(performance.now() - t);
      }
      times.sort((a, b) => a - b);
      return { json_bytes: json.length, preview_bytes: bytes, page_ms: times[2] };
    }, id);
    const row = {
      brush: `${plan.brush} onto ${plan.onto}`,
      edits: rows.filter((r) => r.painted === plan.brush).length,
      previews: rows.filter((r) => r.drawn != null).length,
      down_ms: median(rows, 'down'),
      tile_on_frame_ms: median(rows, 'tile'),
      up_ms: median(rows, 'up'),
      asked_ms: median(rows, 'asked'),
      landed_ms: median(rows, 'landed'),
      preview_drawn_ms: median(rows, 'drawn'),
      page_hook_ms: median(rows, 'hook'),
      worker_ms: median(rows, 'worker'),
      ...alone,
    };
    out.painter[id] = row;
    console.log(id, JSON.stringify(row));
  }
}

async function entry(browser, port, out) {
  const { page, cdp } = await openPage(browser, `http://127.0.0.1:${port}/play.html?autoquality=0`);
  await page.evaluate(() => window.LegaiaWasm.load());
  await (await page.$('input[type=file]')).setInputFiles(disc);
  await page.waitForFunction(() => !!window.__playRuntime, null, { timeout: 300000 });
  await page.waitForTimeout(4000);
  await throttle(cdp);
  await page.evaluate(() => {
    const P = Object.getPrototypeOf(window.__playRuntime);
    const E = (window.__pb = { on: false, ms: {}, calls: {} });
    for (const k of Object.getOwnPropertyNames(P)) {
      const d = Object.getOwnPropertyDescriptor(P, k);
      if (!d || typeof d.value !== 'function' || k === 'constructor') continue;
      const f = d.value;
      P[k] = function (...a) {
        if (!E.on) return f.apply(this, a);
        const t = performance.now();
        const r = f.apply(this, a);
        E.ms[k] = (E.ms[k] || 0) + performance.now() - t;
        E.calls[k] = (E.calls[k] || 0) + 1;
        return r;
      };
    }
  });
  const top = () => `Object.entries(window.__pb.ms).map(([k, v]) => [k, +v.toFixed(1), window.__pb.calls[k]]).sort((a, b) => b[1] - a[1]).slice(0, 12)`;
  out.enable = await page.evaluate(async (topExpr) => {
    const E = window.__pb;
    const row = [...document.querySelectorAll('button')].find((b) => /^enable/i.test(b.textContent.trim()));
    if (!row) throw new Error('no Enable row');
    E.on = true;
    const t = performance.now();
    row.click();
    const end = t + 120000;
    while (performance.now() < end && !document.querySelector('[data-pack-map]')) await new Promise((res) => setTimeout(res, 5));
    E.on = false;
    const r = { to_scene_list_ms: performance.now() - t, top: eval(topExpr) };
    E.ms = {};
    E.calls = {};
    return r;
  }, top());
  console.log('enable', JSON.stringify(out.enable));
  await startProfile(cdp);
  out.entry = await page.evaluate(async ({ map, topExpr }) => {
    const E = window.__pb;
    const btn = document.querySelector(`[data-pack-map="${map}"]`);
    if (!btn) throw new Error('no scene row ' + map);
    E.on = true;
    const t = performance.now();
    btn.click();
    const click = performance.now() - t;
    const frames = [];
    let last = performance.now();
    await new Promise((res) => {
      const f = () => {
        const now = performance.now();
        frames.push(+(now - last).toFixed(1));
        last = now;
        if (frames.length >= 6) res();
        else requestAnimationFrame(f);
      };
      requestAnimationFrame(f);
    });
    E.on = false;
    return { click_ms: click, frames_ms: frames, first_frame_ms: click + frames[0] + frames[1], top: eval(topExpr) };
  }, { map: MAP, topExpr: top() });
  console.log('entry', MAP, JSON.stringify(out.entry));
  await stopProfile(cdp);
}

async function main() {
  const { port, stop } = await serveSite(site);
  const browser = await launchChromium({ gl: 'egl', vsync: true });
  const out = { cpu_throttle: THROTTLE ? { rate: THROTTLE, proxy: true } : null, painter: {} };
  try {
    if (arg('entry')) await entry(browser, port, out);
    else await painter(browser, port, out);
  } finally {
    await browser.close();
    stop();
  }
  if (arg('json')) fs.writeFileSync(arg('json'), JSON.stringify(out, null, 1));
}

main().catch((e) => { console.error(e); process.exit(1); });
