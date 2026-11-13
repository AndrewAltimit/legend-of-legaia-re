#!/usr/bin/env node
// Headless page checks over a built site: what a green `build-wasm.sh` does not say.
//
//   scripts/ci/check-pages.mjs [--site site] [--out dir] [--tabs 4] [--only phone,play,modes]
//       [--gl swiftshader|egl] [--no-disc]
//
// Every generated page under `site/` loads with no uncaught exception, no console error
// and no failed same-origin request, and lays out on a 360px phone with nothing wider than
// the screen and the drawer's zone strip clear of the top bar (`phone`). Then the pages that run the bundle run it: the loader
// instantiates and exposes each runtime's export, the modes page lists its cards, and, when
// LEGAIA_DISC_BIN names an image, the play page takes the disc through its own file input,
// enters town01 and draws frames that are not blank (a screenshot lands in --out; it holds
// disc-derived pixels, so keep --out under target/). `controls` is what only a browser has: a
// file that is not the game refused in words (no disc needed), then with a disc the on-screen
// pad of a touch-only device, a controller on the standard mapping, fullscreen, sound after
// the first gesture, and controllers plugged in mid-game (a non-standard pad's hat, a second
// pad, the unplug notice, a Key Config rebind made with a controller).
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
// The narrowest phone the site is laid out for, and what the phone pass measures there:
// the document's width with the outermost elements past the right edge that no scroller
// or fixed layer holds, then the drawer's zone strip against the top bar.
const PHONE = [360, 780];
// The screen's width is passed in: a mobile viewport widens its layout viewport to
// the widest content and zooms the page out, so `innerWidth` grows with the defect.
const PHONE_PROBE = async (W) => {
  const held = (el) => {
    // The body's own `overflow-x: hidden` holds nothing: it only cuts the
    // overflow off where a reader cannot scroll to it.
    for (let a = el; a && a !== document.body; a = a.parentElement) {
      const cs = getComputedStyle(a);
      if (cs.position === 'fixed') return true;
      if (a !== el && /auto|scroll|hidden|clip/.test(cs.overflowX)) return true;
    }
    return !!el.closest('details:not([open]) > :not(summary)');
  };
  const name = (el) => el.tagName.toLowerCase() + (el.id ? '#' + el.id : el.classList.length ? '.' + [...el.classList].slice(0, 2).join('.') : '');
  const wide = [];
  for (const el of document.querySelectorAll('body *')) {
    const r = el.getBoundingClientRect();
    if (!r.width || r.right <= W + 1 || held(el)) continue;
    const p = el.parentElement && el.parentElement.getBoundingClientRect();
    if (p && p.right > W + 1 && !held(el.parentElement)) continue;
    wide.push(`${name(el)} to ${Math.round(r.right)}px`);
    if (wide.length >= 4) break;
  }
  let drawer = '';
  const toggle = document.querySelector('.sidebar-toggle');
  if (toggle && getComputedStyle(toggle).display !== 'none') {
    // The slide is a transition, which a tab in the background does not run; the
    // open drawer's resting place is what is under test.
    document.querySelector('.sidebar').style.transition = 'none';
    toggle.click();
    await new Promise((r) => setTimeout(r, 100));
    const bar = document.querySelector('.topbar').getBoundingClientRect().bottom;
    const sb = document.querySelector('.sidebar');
    const zones = sb.querySelector('.sidebar-zones');
    const list = sb.querySelector('.sidebar-list');
    const sr = sb.getBoundingClientRect();
    if (!sb.classList.contains('open') || sr.left < -1 || sr.right > W + 1) drawer = `did not open on screen (${Math.round(sr.left)}..${Math.round(sr.right)})`;
    else if (zones) {
      // Whichever of the two scrolls: the list on the drawer as built, the drawer
      // itself on one whose list has gone back to `display: contents`.
      if (list) list.scrollTop = 400;
      sb.scrollTop = 400;
      await new Promise((r) => setTimeout(r, 50));
      const top = zones.getBoundingClientRect().top;
      if (top < bar - 1) drawer = `zone strip ${Math.round(bar - top)}px under the top bar`;
    }
    toggle.click();
  }
  return { W, docW: Math.max(document.documentElement.scrollWidth, document.body.scrollWidth), wide, drawer };
};
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

  // 1b. Every page at phone width: nothing makes the document wider than the screen
  // (a wide element belongs inside its own scroller - a table wrap, a diagram frame, a
  // `pre`), and the drawer opens with its zone strip under the top bar, also with the
  // page list scrolled. Both failures read as fine at a desktop width.
  if (want('phone')) {
    const queue = list.slice();
    const context = await browser.newContext({ viewport: { width: PHONE[0], height: PHONE[1] }, deviceScaleFactor: 3, isMobile: true, hasTouch: true });
    await Promise.all(Array.from({ length: TABS }, async () => {
      const page = await context.newPage();
      let label = '';
      watch(page, base, failures, () => `${label} (phone)`);
      while (queue.length) {
        label = queue.shift();
        try {
          await page.goto(base + label, { waitUntil: 'load', timeout: 120000 });
          await page.waitForTimeout(300);
          const got = await page.evaluate(PHONE_PROBE, PHONE[0]);
          if (got.docW > got.W + 1 || got.wide.length) failures.push(`${label}: ${got.docW}px wide on a ${got.W}px phone (${got.wide.join('; ') || 'no single element'})`);
          if (got.drawer) failures.push(`${label}: phone drawer ${got.drawer}`);
        } catch (e) {
          failures.push(`${label} (phone): ${e.message.split('\n')[0]}`);
        }
        await page.goto('about:blank').catch(() => {});
      }
      await page.close();
    }));
    await context.close();
    console.log(`[check-pages] ${list.length} pages fit a ${PHONE[0]}px phone`);
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

  // 5. What only a browser has: the refusal of a file that is not the game, the on-screen
  // pad on a touch screen, a controller, fullscreen, sound after the first gesture.
  if (want('controls')) {
    // A file with no game in it is refused in words, and the picker still takes the next one.
    {
      const context = await browser.newContext({ viewport: { width: 1280, height: 900 } });
      const page = await context.newPage();
      watch(page, base, failures, 'play.html (refusals)');
      await page.goto(base + 'play.html', { waitUntil: 'load' });
      const cooked = Buffer.alloc(40 * 2048);
      cooked[16 * 2048] = 1;
      cooked.write('CD001', 16 * 2048 + 1);
      const refused = [
        ['garbage.bin', Buffer.alloc(3 << 20, 0x5a), /not a raw CD image/],
        ['empty.bin', Buffer.alloc(0), /too small/],
        ['game.cue', Buffer.from('FILE "game.bin" BINARY\n  TRACK 01 MODE2/2352\n'), /cue sheet/],
        ['cooked.iso', cooked, /2048-byte/],
      ];
      for (const [name, buffer, says] of refused) {
        await page.evaluate(() => document.getElementById('play-status').classList.remove('is-error'));
        await (await page.$('#play-file')).setInputFiles({ name, mimeType: 'application/octet-stream', buffer });
        await page.waitForFunction(() => document.getElementById('play-status').classList.contains('is-error') || !!window.__playRuntime,
          null, { timeout: 120000 }).catch(() => {});
        const got = await page.evaluate(() => ({ text: document.getElementById('play-status').textContent.trim(), runtime: !!window.__playRuntime }));
        if (got.runtime) failures.push(`play.html: ${name} was taken as a disc`);
        else if (!says.test(got.text)) failures.push(`play.html: ${name} refused as "${got.text.slice(0, 120)}"`);
      }
      await context.close();
      console.log(`[check-pages] play page refused ${refused.length} files that are not the game`);
    }
    if (!disc || !fs.existsSync(disc)) {
      console.log('[skip] LEGAIA_DISC_BIN unset: the play page\'s touch pad, controller and fullscreen were not driven');
    } else {
      const lead = (page) => page.evaluate(() => {
        const m = /slot: 0,\s*x: (-?\d+),\s*y: (-?\d+),\s*z: (-?\d+)/.exec(window.__playRuntime.debug_host_trace_text());
        return m ? [+m[1], +m[3]] : null;
      });
      const enter = async (context, label) => {
        const page = await context.newPage();
        watch(page, base, failures, label);
        await page.goto(base + 'play.html', { waitUntil: 'load' });
        await (await page.$('#play-file')).setInputFiles(disc);
        await page.waitForFunction(() => !!window.__playRuntime, null, { timeout: 300000 });
        await page.evaluate(() => document.querySelector('[data-scene="town01"]').click());
        await page.waitForFunction(() => window.__playView && window.__playView.scene, null, { timeout: 120000 });
        await page.waitForTimeout(2500);
        return page;
      };
      const moved = (a, b) => a && b && (a[0] !== b[0] || a[1] !== b[1]);

      // A touch screen: the pad is up, its D-pad walks, START opens the pause menu.
      {
        const context = await browser.newContext({ viewport: { width: 900, height: 700 }, hasTouch: true, isMobile: true });
        const page = await enter(context, 'play.html (touch)');
        const pad = await page.evaluate(() => {
          const root = document.querySelector('.touch-pad');
          const box = (e) => { const r = e.getBoundingClientRect(); return [r.left + r.width / 2, r.top + r.height / 2, r.height]; };
          const start = root && [...root.querySelectorAll('.tp-small')].find((e) => /START/.test(e.textContent));
          return root && !root.classList.contains('is-hidden') && start ? { dpad: box(root.querySelector('.tp-dpad')), start: box(start) } : null;
        });
        if (!pad) {
          failures.push('play.html: no on-screen pad on a touch-only device');
        } else {
          const cdp = await context.newCDPSession(page);
          const touch = async (x, y, ms) => {
            await cdp.send('Input.dispatchTouchEvent', { type: 'touchStart', touchPoints: [{ x, y, id: 1 }] });
            await page.waitForTimeout(ms);
            await cdp.send('Input.dispatchTouchEvent', { type: 'touchEnd', touchPoints: [] });
            await page.waitForTimeout(300);
          };
          const at = await lead(page);
          await touch(pad.dpad[0], pad.dpad[1] + pad.dpad[2] * 0.4, 1200);
          if (!moved(at, await lead(page))) failures.push('play.html: the on-screen D-pad did not walk the lead');
          if (await page.evaluate(() => /is-(up|down|left|right)/.test(document.querySelector('.tp-dpad').className))) {
            failures.push('play.html: the on-screen D-pad stayed held after the touch ended');
          }
          if (!(await page.evaluate(() => !!window.__playView._audioUp))) failures.push('play.html: no sound output after the first touch');
          await touch(pad.start[0], pad.start[1], 120);
          await page.waitForTimeout(1200);
          if (!(await page.evaluate(() => window.__playRuntime.play_menu_is_open()))) failures.push('play.html: the on-screen START did not open the pause menu');
        }
        await context.close();
      }

      // A controller on the standard mapping, then fullscreen.
      {
        const context = await browser.newContext({ viewport: { width: 1280, height: 900 } });
        await context.addInitScript(() => {
          window.__pad = { id: 'check-pages pad', index: 0, connected: true, mapping: 'standard', timestamp: 0, axes: [0, 0, 0, 0],
            buttons: Array.from({ length: 17 }, () => ({ pressed: false, touched: false, value: 0 })) };
          navigator.getGamepads = () => [window.__pad, null, null, null];
        });
        const page = await enter(context, 'play.html (controller)');
        const button = (i, on) => page.evaluate(([i, on]) => { const b = window.__pad.buttons[i]; b.pressed = on; b.value = on ? 1 : 0; }, [i, on]);
        const audio = await page.evaluate(() => !!window.__playView._audioUp);
        if (!audio) failures.push('play.html: no sound output after the scene was picked');
        let at = await lead(page);
        await button(13, true); /* D-pad down */
        await page.waitForTimeout(1200);
        await button(13, false);
        await page.waitForTimeout(300);
        if (!moved(at, await lead(page))) failures.push('play.html: a controller\'s D-pad did not walk the lead');
        at = await lead(page);
        await page.evaluate(() => { window.__pad.axes[0] = 1; });
        await page.waitForTimeout(1200);
        await page.evaluate(() => { window.__pad.axes[0] = 0; });
        await page.waitForTimeout(300);
        if (!moved(at, await lead(page))) failures.push('play.html: a controller\'s left stick did not walk the lead');
        for (const open of [true, false]) {
          await button(9, true); /* Start */
          await page.waitForTimeout(150);
          await button(9, false);
          await page.waitForTimeout(1500);
          if ((await page.evaluate(() => window.__playRuntime.play_menu_is_open())) !== open) {
            failures.push(`play.html: a controller's Start did not ${open ? 'open' : 'close'} the pause menu`);
          }
        }
        await page.click('#play-fullscreen');
        await page.waitForTimeout(1000);
        const fsq = await page.evaluate(() => {
          const r = document.querySelector('.play-canvas-wrap canvas').getBoundingClientRect();
          return { el: document.fullscreenElement ? document.fullscreenElement.className : '', w: r.width, h: r.height, vh: innerHeight, focus: document.activeElement ? document.activeElement.className : '' };
        });
        if (!/play-screen/.test(fsq.el)) failures.push(`play.html: the Fullscreen button presented "${fsq.el}"`);
        else if (fsq.h < fsq.vh * 0.9 || Math.abs(fsq.w / fsq.h - 4 / 3) > 0.02) failures.push(`play.html: fullscreen picture is ${fsq.w | 0}x${fsq.h | 0} in a ${fsq.vh}px-high screen`);
        else if (!/play-canvas/.test(fsq.focus)) failures.push(`play.html: fullscreen left the keyboard on "${fsq.focus}"`);
        // The native window's key for the same toggle: F11 leaves, and F11 comes back to
        // the stage, not the browser's whole-document full screen.
        const fsKey = async () => {
          await page.keyboard.press('F11');
          await page.waitForTimeout(800);
          return page.evaluate(() => (document.fullscreenElement ? document.fullscreenElement.className : ''));
        };
        const left = await fsKey();
        const back = await fsKey();
        if (left) failures.push(`play.html: F11 left "${left}" full screen`);
        else if (!/play-screen/.test(back)) failures.push(`play.html: F11 presented "${back}"`);
        await context.close();
      }
      // Controllers plugged in mid-game: a pad without the standard mapping walks on its
      // hat (the engine's raw table and hat finder), a second pad drives too, unplugging is
      // said, and Key Config takes a controller button into the stored table.
      {
        const context = await browser.newContext({ viewport: { width: 1280, height: 900 } });
        await context.addInitScript(() => {
          window.__pads = [null, null, null, null];
          navigator.getGamepads = () => window.__pads;
          window.__mkPad = (index, id, mapping, nAxes, nButtons) => ({ id, index, connected: true, mapping, timestamp: 0,
            axes: Array.from({ length: nAxes }, () => 0),
            buttons: Array.from({ length: nButtons }, () => ({ pressed: false, touched: false, value: 0 })) });
          window.__plug = (pad, on) => {
            if (on) window.__pads[pad.index] = pad; else window.__pads[pad.index] = null;
            const e = new Event(on ? 'gamepadconnected' : 'gamepaddisconnected');
            e.gamepad = pad;
            window.dispatchEvent(e);
          };
        });
        const page = await enter(context, 'play.html (hot-plug)');
        const notice = () => page.evaluate(() => document.getElementById('play-notice').textContent);
        await page.evaluate(() => { window.__raw = window.__mkPad(0, 'check-pages raw pad', '', 6, 12); window.__plug(window.__raw, true); });
        await page.waitForTimeout(300);
        if (!/Controller connected.*guessed/.test(await notice())) failures.push(`play.html: plugging a non-standard pad said "${await notice()}"`);
        const status = await page.evaluate(() => document.getElementById('play-pad-status').textContent);
        if (!/buttons guessed/.test(status)) failures.push(`play.html: the controller line reads "${status}" with a non-standard pad in`);
        let at = await lead(page);
        await page.evaluate(() => { window.__raw.axes[5] = 1; }); /* hat down */
        await page.waitForTimeout(1200);
        await page.evaluate(() => { window.__raw.axes[5] = 0; });
        await page.waitForTimeout(300);
        if (!moved(at, await lead(page))) failures.push('play.html: a non-standard pad\'s hat did not walk the lead');
        await page.evaluate(() => { window.__std = window.__mkPad(1, 'check-pages second pad (STANDARD GAMEPAD)', 'standard', 4, 17); window.__plug(window.__std, true); });
        const press = async (pad, i, ms = 150) => {
          await page.evaluate(([pad, i]) => { const b = window[pad].buttons[i]; b.pressed = true; b.value = 1; }, [pad, i]);
          await page.waitForTimeout(ms);
          await page.evaluate(([pad, i]) => { const b = window[pad].buttons[i]; b.pressed = false; b.value = 0; }, [pad, i]);
          await page.waitForTimeout(250);
        };
        at = await lead(page);
        await press('__std', 12, 1200); /* the second pad's D-pad up: back the way the hat came */
        if (!moved(at, await lead(page))) failures.push('play.html: a second controller did not walk the lead');
        await page.evaluate(() => window.__plug(window.__raw, false));
        await page.waitForTimeout(200);
        if (!/Controller disconnected/.test(await notice())) failures.push(`play.html: unplugging a pad said "${await notice()}"`);
        // Options opened (the root ring's own walk is not under test), then Select (Key
        // Config), Cross on its first row (Cross) and the top face button, all on the
        // controller: Cross moves onto standard index 3, into the stored table.
        const keyConfig = () => page.evaluate(() => JSON.parse(window.__playRuntime.play_menu_key_config_json()));
        const until = (want) => page.waitForFunction((w) => {
          const k = JSON.parse(window.__playRuntime.play_menu_key_config_json());
          return k.open === w.open && k.awaiting === w.awaiting;
        }, want, { timeout: 4000 }).then(() => true, () => false);
        await page.waitForTimeout(800);
        if (!(await page.evaluate(() => window.__playRuntime.play_menu_open_row('Options')))) {
          failures.push('play.html: the pause menu\'s Options row did not open');
        }
        await page.waitForTimeout(800);
        await press('__std', 8);
        if (!(await until({ open: true, awaiting: false }))) failures.push(`play.html: a controller's Select did not open Key Config (${JSON.stringify(await keyConfig())})`);
        await press('__std', 0);
        if (!(await until({ open: true, awaiting: true }))) failures.push(`play.html: a controller's Cross did not start a Key Config wait (${JSON.stringify(await keyConfig())})`);
        await page.evaluate(() => { const b = window.__std.buttons[3]; b.pressed = true; b.value = 1; });
        await page.waitForTimeout(500);
        /* Still held: through the new table it is Cross, which must not re-open the wait. */
        const held = await keyConfig();
        await page.evaluate(() => { const b = window.__std.buttons[3]; b.pressed = false; b.value = 0; });
        await page.waitForTimeout(300);
        if (held.awaiting) failures.push('play.html: the controller button that made a Key Config bind re-opened the row\'s wait');
        const stored = await page.evaluate(() => { try { return JSON.parse(localStorage.getItem('legaia.bindings') || 'null'); } catch (e) { return null; } });
        const slot = stored && stored.gamepad && stored.gamepad.buttons && stored.gamepad.buttons[3];
        if (slot !== 'Cross') failures.push(`play.html: Key Config with a controller left standard button 3 on "${slot}" (stored: ${JSON.stringify(stored && stored.gamepad)})`);
        await context.close();
      }
      console.log('[check-pages] play page: touch pad, controller, fullscreen and sound driven');
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
