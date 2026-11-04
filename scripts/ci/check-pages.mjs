#!/usr/bin/env node
// Headless page checks over a built site: what a green `build-wasm.sh` does not say.
//
//   scripts/ci/check-pages.mjs [--site site] [--out dir] [--tabs 4] [--only play,modes]
//       [--gl swiftshader|egl] [--no-disc]
//
// Every generated page under `site/` loads with no uncaught exception, no console error
// and no failed same-origin request. Then the pages that run the bundle run it: the loader
// instantiates and exposes each runtime's export, the modes page lists its cards, and, when
// LEGAIA_DISC_BIN names an image, the play page takes the disc through its own file input,
// enters town01 and draws frames that are not blank (a screenshot lands in --out; it holds
// disc-derived pixels, so keep --out under target/).
//
// Needs a built bundle (scripts/ci/build-wasm.sh) and playwright-core; PLAYWRIGHT_CORE names
// its directory when `~/.npm/_npx` does not hold one. Exit 1 with every failure listed; a
// missing bundle or browser is exit 1 too, since a page check that did not run proves nothing.

import { createRequire } from 'module';
import { spawn } from 'child_process';
import fs from 'fs';
import os from 'os';
import path from 'path';
import { fileURLToPath } from 'url';

const arg = (name, dflt) => {
  const i = process.argv.indexOf('--' + name);
  return i < 0 ? dflt : (process.argv[i + 1] && !process.argv[i + 1].startsWith('--') ? process.argv[i + 1] : true);
};
const REPO = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const site = path.resolve(arg('site', path.join(REPO, 'site')));
const outDir = path.resolve(arg('out', path.join(REPO, 'target', 'check-pages')));
const TABS = +arg('tabs', 4);
const GL = arg('gl', 'egl');
const only = arg('only') ? String(arg('only')).split(',') : null;
const disc = arg('no-disc') ? null : process.env.LEGAIA_DISC_BIN;

// Text a page logs at the error level on its way to working: the disc-less prompt's probe.
const BENIGN_CONSOLE = [
  /favicon/,
];
// What each bundle must export, by a page that holds it, and the path it must come from:
// a page on the wrong bundle still runs if that bundle has the exports, so the path counts.
const BUNDLE_CHECKS = [
  { page: 'play.html', from: '/wasm/play/legaia_web_play_bundle_bg.wasm', exports: ['LegaiaRuntime', 'LegaiaMinigames', 'LegaiaModes'] },
  { page: 'minigames.html', from: '/wasm/play/legaia_web_play_bundle_bg.wasm', exports: ['LegaiaMinigames'] },
  { page: 'modes.html', from: '/wasm/play/legaia_web_play_bundle_bg.wasm', exports: ['LegaiaModes'] },
  { page: 'viewer.html', from: '/wasm/legaia_web_viewer_bg.wasm', exports: ['LegaiaViewer', 'LegaiaRuntime'] },
  { page: 'tooling/rom-patcher.html', from: '/wasm/legaia_web_viewer_bg.wasm', exports: ['patch_rom'] },
  { page: 'tooling/expansion-workbench.html', from: '/wasm/legaia_web_viewer_bg.wasm', exports: ['ExpansionWorkbench'] },
];
// A uniform frame compresses to almost nothing; a drawn scene does not.
const NOT_BLANK_BYTES = 20000;
const TOWN_FRAMES = 120;

function die(msg) {
  console.log(`[check-pages] FAILED: ${msg}`);
  process.exit(1);
}

function playwrightCore() {
  const home = os.homedir();
  const roots = process.env.PLAYWRIGHT_CORE ? [process.env.PLAYWRIGHT_CORE] : [];
  const npx = path.join(home, '.npm/_npx');
  if (fs.existsSync(npx)) {
    for (const d of fs.readdirSync(npx)) roots.push(path.join(npx, d, 'node_modules/playwright-core'));
  }
  for (const r of roots) if (fs.existsSync(path.join(r, 'package.json'))) return createRequire(import.meta.url)(r);
  return die('playwright-core not found; set PLAYWRIGHT_CORE');
}

// Generated pages: every .html outside the sources and the bundle.
function pages() {
  const out = [];
  const walk = (dir) => {
    for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
      if (e.name.startsWith('_') || e.name === 'wasm' || e.name === 'lang') continue;
      const p = path.join(dir, e.name);
      if (e.isDirectory()) walk(p);
      else if (e.name.endsWith('.html')) out.push(path.relative(site, p));
    }
  };
  walk(site);
  return out.sort();
}

function watch(page, base, failures, labelOf) {
  const label = { toString: () => (typeof labelOf === 'function' ? labelOf() : labelOf) };
  page.on('pageerror', (e) => failures.push(`${label}: uncaught ${e.message}`));
  page.on('console', (m) => {
    if (m.type() !== 'error') return;
    const text = m.text();
    if (BENIGN_CONSOLE.some((re) => re.test(text) || re.test(m.location().url || ''))) return;
    failures.push(`${label}: console error ${text.slice(0, 200)} (${m.location().url || 'no location'})`);
  });
  page.on('requestfailed', (r) => {
    if (r.url().startsWith(base) && !/net::ERR_(ABORTED|FAILED)/.test(r.failure()?.errorText || '')) {
      failures.push(`${label}: request failed ${r.url().slice(base.length)} ${r.failure()?.errorText}`);
    }
  });
  page.on('response', (r) => {
    if (r.url().startsWith(base) && r.status() >= 400 && !/favicon/.test(r.url())) {
      failures.push(`${label}: HTTP ${r.status()} ${r.url().slice(base.length)}`);
    }
  });
}

async function main() {
  for (const f of [['wasm', 'legaia_web_viewer_bg.wasm'], ['wasm', 'play', 'legaia_web_play_bundle_bg.wasm']]) {
    if (!fs.existsSync(path.join(site, ...f))) die(`no ${f.join('/')} under ${site}; run scripts/ci/build-wasm.sh`);
  }
  const list = pages();
  if (!list.includes('play.html')) die(`no generated pages under ${site}; run python3 site/_gen.py`);
  fs.mkdirSync(outDir, { recursive: true });

  const server = spawn('python3', ['-u', '-m', 'http.server', '0', '--bind', '127.0.0.1', '--directory', site],
    { stdio: ['ignore', 'pipe', 'pipe'] });
  process.on('exit', () => { try { server.kill(); } catch (e) { /* gone */ } });
  const port = await new Promise((res, rej) => {
    const on = (d) => { const m = /port (\d+)/.exec(String(d)); if (m) res(+m[1]); };
    server.stdout.on('data', on);
    server.stderr.on('data', on);
    setTimeout(() => rej(new Error('no server')), 10000);
  });
  const base = `http://127.0.0.1:${port}/`;

  const { chromium } = playwrightCore();
  const pw = path.join(os.homedir(), '.cache/ms-playwright');
  const dirs = fs.existsSync(pw) ? fs.readdirSync(pw).filter((d) => /^chromium-/.test(d)).sort() : [];
  if (!dirs.length) die('no Chromium under ~/.cache/ms-playwright');
  const dir = path.join(pw, dirs[dirs.length - 1]);
  const exe = [path.join(dir, 'chrome-linux/chrome'), path.join(dir, 'chrome-linux64/chrome')].find((p) => fs.existsSync(p));
  const glArgs = GL === 'swiftshader'
    ? ['--use-gl=angle', '--use-angle=swiftshader', '--enable-unsafe-swiftshader']
    : ['--use-gl=angle', '--use-angle=gl-egl', '--ignore-gpu-blocklist'];
  const browser = await chromium.launch({ executablePath: exe, headless: true,
    args: [...glArgs, '--enable-webgl', '--autoplay-policy=no-user-gesture-required'] });

  const failures = [];
  const want = (name) => !only || only.includes(name);

  // 1. Every page loads clean.
  if (want('all')) {
    const queue = list.slice();
    const context = await browser.newContext({ viewport: { width: 1280, height: 900 } });
    await Promise.all(Array.from({ length: TABS }, async () => {
      const page = await context.newPage();
      let label = '';
      watch(page, base, failures, () => label);
      while (queue.length) {
        label = queue.shift();
        try {
          await page.goto(base + label, { waitUntil: 'load', timeout: 120000 });
        } catch (e) {
          failures.push(`${label}: ${e.message.split('\n')[0]}`);
        }
        // Stop a page's own work before the next, so its errors stay under its name.
        await page.goto('about:blank').catch(() => {});
      }
      await page.close();
    }));
    await context.close();
    console.log(`[check-pages] loaded ${list.length} pages`);
  }

  // 2. The bundle runs.
  if (want('bundle')) {
    for (const c of BUNDLE_CHECKS) {
      const context = await browser.newContext();
      const page = await context.newPage();
      watch(page, base, failures, `bundle on ${c.page}`);
      const fetched = [];
      page.on('request', (r) => { if (r.url().includes('.wasm')) fetched.push(new URL(r.url()).pathname); });
      await page.goto(base + c.page, { waitUntil: 'load' });
      const missing = await page.evaluate(async (names) => {
        const m = await window.LegaiaWasm.load();
        return names.filter((n) => !m[n]);
      }, c.exports);
      if (missing.length) failures.push(`bundle on ${c.page}: no export ${missing.join(', ')}`);
      if (!fetched.includes(c.from)) failures.push(`bundle on ${c.page}: fetched ${fetched.join(', ') || 'no module'}, not ${c.from}`);
      else if (fetched.some((u) => u !== c.from)) failures.push(`bundle on ${c.page}: also fetched ${fetched.filter((u) => u !== c.from).join(', ')}`);
      await context.close();
    }
    console.log(`[check-pages] each bundle instantiates on its pages (${BUNDLE_CHECKS.length} pages)`);
  }

  // 3. The modes page, which needs no disc.
  if (want('modes')) {
    const context = await browser.newContext({ viewport: { width: 1280, height: 900 } });
    const page = await context.newPage();
    watch(page, base, failures, 'modes.html');
    await page.goto(base + 'modes.html', { waitUntil: 'load' });
    await page.waitForFunction(() => document.querySelectorAll('#gm-3d-cards .gm-card').length > 0, null, { timeout: 120000 })
      .catch(() => failures.push('modes.html: no mode cards'));
    await context.close();
    console.log('[check-pages] modes page lists its cards');
  }

  // 4. A disc in the play page, a scene entered, frames drawn.
  if (want('play')) {
    if (!disc || !fs.existsSync(disc)) {
      console.log('[skip] LEGAIA_DISC_BIN unset: the play page was not played');
    } else {
      const context = await browser.newContext({ viewport: { width: 1280, height: 900 } });
      const page = await context.newPage();
      watch(page, base, failures, 'play.html');
      await page.goto(base + 'play.html', { waitUntil: 'load' });
      await (await page.$('input[type=file]')).setInputFiles(disc);
      await page.waitForFunction(() => !!window.__playRuntime, null, { timeout: 300000 });
      const drawn = await page.evaluate(async (frames) => {
        const btn = document.querySelector('[data-scene="town01"]');
        if (!btn) return { error: 'no town01 scene button' };
        btn.click();
        const view = window.__playView;
        const f = view._frame;
        let calls = 0;
        view._frame = function (...a) { calls++; return f.apply(this, a); };
        const t0 = performance.now();
        while (calls < frames && performance.now() - t0 < 120000) await new Promise((r) => setTimeout(r, 50));
        return { calls, scene: view.scene };
      }, TOWN_FRAMES);
      if (drawn.error) failures.push(`play.html: ${drawn.error}`);
      else if (drawn.calls < TOWN_FRAMES) failures.push(`play.html: ${drawn.calls} frames in two minutes`);
      const wrap = await page.$('.play-canvas-wrap');
      if (!wrap) {
        failures.push('play.html: no .play-canvas-wrap');
      } else {
        const shot = await wrap.screenshot({ path: path.join(outDir, 'play-town01.png') });
        if (shot.length < NOT_BLANK_BYTES) failures.push(`play.html: town01 frame looks blank (${shot.length} byte PNG)`);
      }
      await context.close();
      console.log(`[check-pages] play page entered town01, ${drawn.calls} frames`);
    }
  }

  await browser.close();
  server.kill();
  const unique = [...new Set(failures)];
  for (const f of unique) console.log(`  ${f}`);
  if (unique.length) {
    console.log(`[check-pages] FAILED: ${unique.length} problem(s) over ${list.length} pages`);
    process.exit(1);
  }
  console.log(`[check-pages] OK: ${list.length} pages`);
}

main().catch((e) => die(e.stack || e.message));
