#!/usr/bin/env node
// The teaser's browser shots: each site page recorded through Chromium's own
// screencast while a short list of steps drives it (scrolls, clicks, drags).
//
//   node scripts/site-media/teaser-browser.mjs SHOTS.json OUT_DIR
//
// SHOTS.json is `{ "site": "<dir>", "fps": 30, "shots": [ ... ] }`, written by
// teaser.py from teaser.toml's `[[shot]]` rows with `page = ...`. Every shot
// shares one browser context, so the disc (LEGAIA_DISC_BIN, given to the
// first page with a file input) is read once and the other pages take it from
// the site's own disc cache. Each shot writes `OUT_DIR/<name>/f_00000.jpg ...`
// resampled to `fps` from the screencast's timestamps (teaser.py crops them).
// A shot with `cropTo` (a selector) writes that element's box after its prep
// as `crop.json` ([x, y, w, h]), which teaser.py takes over the row's `crop`.
//
// Steps (run in order while recording; `prep` runs the same kinds before it):
//   { wait: ms }                       let the page run
//   { scroll: px, over: ms }           smooth-scroll the window (or `in`: selector)
//   { scrollTo: selector }             put an element at the top, instantly
//   { click: selector }                click the first match
//   { clickAt: [x, y] }                click at a page point
//   { hover: selector }                move the pointer onto it
//   { drag: [x0, y0, x1, y1], over: ms }   a left-button drag in page pixels
//   { move: [x, y], over: ms }         move the pointer
//   { key: "Space" }                   press a key
//   { hold: "ArrowUp", for: ms }       hold a key down
//   { select: [selector, value] }      pick a <select> option
//   { wheel: dy, at: [x, y], over: ms } scroll the wheel over a point
//   { dragIn: selector, by: [dx0, dy0, dx1, dy1], over: ms }
//                                      a drag measured from an element's centre
//   { type: "text", into: selector }   type into a field
//   { eval: "js" }                     run a page expression (awaited)
//   { waitFor: "js expression" }       wait until it is truthy (timeout 300 s)

import fs from 'fs';
import path from 'path';
import { launchChromium, serveSite } from '../ci/browser-session.mjs';

const [shotsFile, outDir] = process.argv.slice(2);
if (!shotsFile || !outDir) {
  console.error('usage: teaser-browser.mjs SHOTS.json OUT_DIR');
  process.exit(2);
}
const spec = JSON.parse(fs.readFileSync(shotsFile, 'utf8'));
const fps = spec.fps || 30;
const disc = process.env.LEGAIA_DISC_BIN;
const log = (m) => console.error(`teaser-browser: ${m}`);

const { port, stop } = await serveSite(path.resolve(spec.site));
const base = `http://127.0.0.1:${port}/`;
const browser = await launchChromium({ gl: 'egl', vsync: true });
const context = await browser.newContext({ viewport: { width: 1280, height: 720 }, colorScheme: 'dark' });
let discLoaded = false;

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function step(page, s) {
  if (s.wait !== undefined) return page.waitForTimeout(s.wait);
  if (s.scroll !== undefined) {
    const over = s.over || 1000;
    return page.evaluate(async ({ px, over, sel }) => {
      const el = sel ? document.querySelector(sel) : null;
      const get = () => (el ? el.scrollTop : window.scrollY);
      // `instant`: a page's own `scroll-behavior: smooth` would turn every
      // per-frame set into an animation the next frame interrupts.
      const set = (y) => (el ? el.scrollTo({ top: y, behavior: 'instant' }) : window.scrollTo({ top: y, behavior: 'instant' }));
      const y0 = get();
      const t0 = performance.now();
      await new Promise((done) => {
        const tick = (t) => {
          const k = Math.min(1, (t - t0) / over);
          const e = k < 0.5 ? 2 * k * k : 1 - Math.pow(-2 * k + 2, 2) / 2;
          set(y0 + px * e);
          if (k < 1) requestAnimationFrame(tick); else done();
        };
        requestAnimationFrame(tick);
      });
    }, { px: s.scroll, over, sel: s.in || null });
  }
  if (s.scrollTo) {
    return page.evaluate((sel) => {
      const el = document.querySelector(sel);
      if (el) window.scrollTo({ top: el.getBoundingClientRect().top + window.scrollY - 64, behavior: 'instant' });
    }, s.scrollTo);
  }
  if (s.click) return page.click(s.click, { timeout: 30000 });
  if (s.clickAt) return page.mouse.click(s.clickAt[0], s.clickAt[1]);
  if (s.hover) return page.hover(s.hover, { timeout: 30000 });
  if (s.move) {
    const n = Math.max(1, Math.round((s.over || 300) / 16));
    return page.mouse.move(s.move[0], s.move[1], { steps: n });
  }
  if (s.drag) {
    const [x0, y0, x1, y1] = s.drag;
    const n = Math.max(1, Math.round((s.over || 1000) / 16));
    await page.mouse.move(x0, y0);
    await page.mouse.down();
    for (let i = 1; i <= n; i++) {
      await page.mouse.move(x0 + ((x1 - x0) * i) / n, y0 + ((y1 - y0) * i) / n);
      await sleep(16);
    }
    return page.mouse.up();
  }
  if (s.key) return page.keyboard.press(s.key);
  if (s.hold) {
    await page.keyboard.down(s.hold);
    await page.waitForTimeout(s.for || 1000);
    return page.keyboard.up(s.hold);
  }
  if (s.select) return page.selectOption(s.select[0], s.select[1]);
  if (s.wheel !== undefined) {
    if (s.at) await page.mouse.move(s.at[0], s.at[1]);
    const n = s.steps || 8;
    for (let i = 0; i < n; i++) {
      await page.mouse.wheel(0, s.wheel / n);
      await page.waitForTimeout((s.over || 400) / n);
    }
    return;
  }
  if (s.dragIn) {
    const box = await page.locator(s.dragIn).first().boundingBox();
    if (!box) throw new Error(`no ${s.dragIn}`);
    const cx = box.x + box.width / 2, cy = box.y + box.height / 2;
    const [dx0, dy0, dx1, dy1] = s.by;
    return step(page, { drag: [cx + dx0, cy + dy0, cx + dx1, cy + dy1], over: s.over });
  }
  if (s.type !== undefined) {
    if (s.into) await page.click(s.into);
    return page.keyboard.type(s.type, { delay: s.delay || 70 });
  }
  if (s.eval) return page.evaluate(`(async () => { ${s.eval} })()`);
  if (s.waitFor) return page.waitForFunction(s.waitFor, null, { timeout: 300000 });
  throw new Error(`unknown step ${JSON.stringify(s)}`);
}

/* A step that fails is logged and skipped: one stale selector costs that
 * step, not the whole batch of pages. */
async function safeStep(page, s, name) {
  try {
    await step(page, s);
  } catch (e) {
    log(`${name}: step ${JSON.stringify(s).slice(0, 80)} failed: ${String(e.message || e).split('\n')[0]}`);
  }
}

async function loadDisc(page) {
  const input = await page.$('input[type=file]');
  if (!input || !disc) return;
  log('loading the disc');
  await input.setInputFiles(disc);
  discLoaded = true;
}

for (const shot of spec.shots) {
  const dir = path.join(outDir, shot.name);
  fs.rmSync(dir, { recursive: true, force: true });
  fs.mkdirSync(dir, { recursive: true });
  const raw = path.join(dir, 'raw');
  fs.mkdirSync(raw);
  const page = await context.newPage();
  if (shot.viewport) await page.setViewportSize({ width: shot.viewport[0], height: shot.viewport[1] });
  log(`${shot.name}: ${shot.page}`);
  await page.goto(base + shot.page, { waitUntil: 'load' });
  if (shot.disc && !discLoaded) await loadDisc(page);
  for (const s of shot.prep || []) await safeStep(page, s, shot.name);
  await page.waitForTimeout(shot.settle ?? 800);
  if (shot.cropTo) {
    // Late layout above the element (a map or a table filling in) can move
    // it after the prep's scroll: centre it again, then measure.
    await page.evaluate((sel) => { const e = document.querySelector(sel); if (e) e.scrollIntoView({ block: 'center' }); }, shot.cropTo);
    await page.waitForTimeout(400);
    const box = await page.locator(shot.cropTo).first().boundingBox().catch(() => null);
    if (box) {
      // Clamped to the viewport: a tall element crops to what is on screen.
      const vp = page.viewportSize();
      const x0 = Math.max(0, box.x), y0 = Math.max(0, box.y);
      const x1 = Math.min(vp.width, box.x + box.width), y1 = Math.min(vp.height, box.y + box.height);
      fs.writeFileSync(path.join(dir, 'crop.json'), JSON.stringify([x0, y0, x1 - x0, y1 - y0].map(Math.round)));
    }
    else log(`${shot.name}: no ${shot.cropTo} to crop to`);
  }

  const cdp = await context.newCDPSession(page);
  const frames = [];
  cdp.on('Page.screencastFrame', async (f) => {
    const file = path.join(raw, `r_${String(frames.length).padStart(5, '0')}.jpg`);
    fs.writeFileSync(file, Buffer.from(f.data, 'base64'));
    frames.push({ t: f.metadata.timestamp, file });
    cdp.send('Page.screencastFrameAck', { sessionId: f.sessionId }).catch(() => {});
  });
  const vp = page.viewportSize();
  await cdp.send('Page.startScreencast', { format: 'jpeg', quality: 95, maxWidth: vp.width, maxHeight: vp.height, everyNthFrame: 1 });
  const t0 = Date.now() / 1000;
  for (const s of shot.steps || []) await safeStep(page, s, shot.name);
  const end = t0 + (shot.seconds || 4);
  const left = end * 1000 - Date.now();
  if (left > 0) await page.waitForTimeout(left);
  await cdp.send('Page.stopScreencast');
  await sleep(200);
  await page.close();

  if (!frames.length) throw new Error(`${shot.name}: no screencast frames`);
  // Constant-rate frames: at each output instant, the last screencast frame
  // that had arrived (a still page sends none, so its frame repeats).
  const total = Math.round((shot.seconds || 4) * fps);
  const list = [];
  let j = 0;
  for (let i = 0; i < total; i++) {
    const t = t0 + i / fps;
    while (j + 1 < frames.length && frames[j + 1].t <= t) j++;
    list.push(frames[j].file);
  }
  list.forEach((f, i) => fs.copyFileSync(f, path.join(dir, `f_${String(i).padStart(5, '0')}.jpg`)));
  fs.rmSync(raw, { recursive: true, force: true });
  log(`${shot.name}: ${frames.length} screencast frames -> ${total}`);
}

await browser.close();
stop();
