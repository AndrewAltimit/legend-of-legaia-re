#!/usr/bin/env node
// The play page's automatic quality step-down: when it acts, and what it must leave alone.
//
//   scripts/ci/quality-ladder-web.mjs [--site site] [--gl swiftshader|egl]
//
// The decision (site/js/play-quality-ladder.js) runs under node on synthetic frame gaps:
// no step on a healthy cadence, one stall, a short bad stretch or a main-thread limit; one
// step at a time, in order, down only; an exempt state and a player's choice both hold.
//
// With LEGAIA_DISC_BIN and a built bundle it then runs on the page: the same synthetic gaps
// reach the page's own ladder, so a step does not depend on how slow this box is. Retail mode
// and PSX rasterisation never engage it, on a page that does engage with them off; every
// localStorage key is byte-identical after the ladder fired and after its undo; the option
// boxes and the engine's options still read the player's choice while the rung is in force.
// Without a disc or bundle the page half says it skipped: a skip is not a pass of that half.
//
// Exit 1 with every failure listed.

import fs from 'fs';
import path from 'path';
import vm from 'vm';
import { fileURLToPath } from 'url';
import { launchChromium, serveSite } from './browser-session.mjs';

const arg = (name, dflt) => {
  const i = process.argv.indexOf('--' + name);
  return i < 0 ? dflt : (process.argv[i + 1] && !process.argv[i + 1].startsWith('--') ? process.argv[i + 1] : true);
};
const REPO = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const site = path.resolve(arg('site', path.join(REPO, 'site')));
const failures = [];
const check = (ok, what) => {
  console.log(`${ok ? 'ok  ' : 'FAIL'} ${what}`);
  if (!ok) failures.push(what);
};

/* --- The decision --- */
const sandbox = {};
vm.runInNewContext(fs.readFileSync(path.join(REPO, 'site/js/play-quality-ladder.js'), 'utf8'), sandbox);
const Ladder = sandbox.LegaiaQualityLadder;
const make = (over = {}) => {
  const applied = [];
  const rung = (id) => ({ id, applicable: () => !(over.off || []).includes(id), apply: () => applied.push(id) });
  const l = new Ladder(['a', 'b', 'c'].map(rung), over.exempt || (() => false));
  return { l, applied, t: 0 };
};
/* `ms` of frames, each `gap` apart, each costing `cost`. */
const run = (x, ms, gap, cost = 1, counts = true) => {
  for (let at = 0; at < ms; at += gap) { x.t += gap; x.l.sample(x.t, cost, counts); }
};
{
  const x = make();
  run(x, 120000, 16.7);
  check(x.applied.length === 0, 'a healthy cadence never steps');
  run(x, 120000, 33.4);
  check(x.applied.length === 0, 'a 30 Hz display never steps');
}
{
  const x = make();
  run(x, 7000, 16.7);
  x.t += 900; x.l.sample(x.t, 1, true);
  x.t += 900; x.l.sample(x.t, 1, true);
  run(x, 60000, 16.7);
  check(x.applied.length === 0, 'stalls inside a healthy minute never step');
}
{
  const x = make();
  run(x, 7000, 100);
  run(x, 5000, 16.7);
  run(x, 7000, 100);
  check(x.applied.length === 0, 'a bad stretch shorter than two windows never steps');
}
{
  const x = make();
  run(x, 7900, 100);
  check(x.applied.length === 0, 'nothing before eight seconds of it');
  run(x, 400, 100);
  check(x.applied.join() === 'a', 'eight seconds of missed cadence steps one rung');
  run(x, 9400, 100);
  check(x.applied.join() === 'a', 'the next step waits for the settle and two fresh windows');
  run(x, 400, 100);
  check(x.applied.join() === 'a,b', 'then one more, in order');
  run(x, 60000, 16.7);
  run(x, 60000, 100);
  check(x.applied.join() === 'a,b,c', 'recovering steps nothing back up, and the ladder runs out');
}
{
  const x = make();
  run(x, 60000, 100, 70);
  check(x.applied.length === 0, 'a main-thread limit never steps: no rung relieves it');
}
{
  const x = make();
  run(x, 60000, 1500);
  check(x.applied.length === 0, 'gaps of over a second are a hidden tab or a load, not a cadence');
  run(x, 60000, 100, 1, false);
  check(x.applied.length === 0, 'a frame that does not count (paused, a movie) never steps');
}
{
  let held = true;
  const x = make({ exempt: () => held });
  run(x, 120000, 100);
  check(x.applied.length === 0, 'an exempt state never steps');
  held = false;
  run(x, 7900, 100);
  check(x.applied.length === 0, 'leaving it starts the count over');
  run(x, 400, 100);
  check(x.applied.join() === 'a', 'and then it can step');
}
{
  const x = make();
  x.l.choose('a');
  run(x, 8300, 100);
  check(x.applied.join() === 'b', "a player's choice takes that rung out of the ladder");
  const y = make({ off: ['a'] });
  run(y, 8300, 100);
  check(y.applied.join() === 'b', 'a rung already off is passed over');
  const z = make();
  z.l.disarm();
  run(z, 60000, 100);
  check(z.applied.length === 0, 'disarmed stays quiet');
}

/* --- The page --- */
const disc = process.env.LEGAIA_DISC_BIN;
if (!disc || !fs.existsSync(disc) || !fs.existsSync(path.join(site, 'wasm', 'legaia_web_viewer_bg.wasm'))) {
  console.log('[skip] the page half needs LEGAIA_DISC_BIN and a built site/wasm bundle');
} else {
  const { port, stop } = await serveSite(site);
  const browser = await launchChromium({ gl: arg('gl', 'swiftshader'), vsync: true });
  const fresh = async (search = '') => {
    const context = await browser.newContext({ viewport: { width: 1280, height: 900 } });
    const page = await context.newPage();
    page.on('pageerror', (e) => check(false, 'page error: ' + e.message));
    await page.goto(`http://127.0.0.1:${port}/play.html${search}`);
    await (await page.$('input[type=file]')).setInputFiles(disc);
    await page.waitForFunction(() => !!window.__playRuntime, null, { timeout: 300000 });
    return page;
  };
  /* The page's own ladder, fed `ms` of a missed cadence. */
  const missFor = (page, ms) => page.evaluate((ms) => {
    const v = window.__playView;
    let t = window.__qualityClock || performance.now();
    v._frameCost = 1;
    for (let at = 0; at < ms; at += 100) { t += 100; v._sampleQuality(t); }
    window.__qualityClock = t;
    return v.qualitySteps();
  }, ms);
  const enter = (page) => page.evaluate(() => {
    document.querySelector('[data-scene="town01"]').click();
    const v = window.__playView;
    v.stop();                      /* Only this script feeds the ladder. */
  });
  const store = (page) => page.evaluate(() => JSON.stringify(Object.keys(localStorage).sort().map((k) => [k, localStorage.getItem(k)])));

  {
    const page = await fresh();
    await enter(page);
    const before = await store(page);
    const size = () => page.evaluate(() => { const c = window.__playView.canvas; return `${c.width}x${c.height}`; });
    check(await size() === '960x720', 'an idle ladder leaves the canvas at its window size');
    const steps = await missFor(page, 90000);
    check(steps.join() === 'ssao,scale75,scale50,shadows', `the rungs fire in order: ${steps.join()}`);
    check(await size() === '480x360', 'the scale rungs leave the 3D canvas at half its size');
    const view = await page.evaluate(() => {
      const v = window.__playView;
      const rt = window.__playRuntime;
      return { hook: v.renderer.ssaoFrame === null, shadows: v.dynShadows,
        overlay: v.menuOverlay.width,
        boxSsao: document.getElementById('play-ssao').checked, boxShadows: document.getElementById('play-dyn-shadows').checked,
        rtSsao: rt.ssao(), rtShadows: rt.light_shadows(),
        note: !document.getElementById('play-quality-note').classList.contains('is-hidden'),
        noteText: document.getElementById('play-quality-note').textContent };
    });
    check(view.hook && view.shadows === false, 'the rungs are in force on the view');
    check(view.overlay === 960, 'the overlay canvas keeps the window size');
    check(view.boxSsao && view.boxShadows, "the boxes still read the player's choice");
    check(view.rtSsao && view.rtShadows, "the engine's options still read the player's choice");
    check(view.note && /session/.test(view.noteText), 'the note is showing, and says it is for this session');
    await page.evaluate(() => window.__playApplyRenderToggles());
    check(await page.evaluate(() => window.__playView.renderer.ssaoFrame === null),
      "the page's re-apply on a view rebuild does not lift a rung");
    check(await store(page) === before, 'localStorage is byte-identical after a session in which it fired');
    await page.evaluate(() => document.getElementById('play-quality-restore').click());
    const back = await page.evaluate(() => {
      const v = window.__playView;
      return { hook: v.renderer.ssaoFrame !== null, shadows: v.dynShadows, width: v.canvas.width,
        note: document.getElementById('play-quality-note').classList.contains('is-hidden') };
    });
    check(back.hook && back.shadows === true && back.width === 960 && back.note, 'the undo puts everything back');
    check((await missFor(page, 90000)).length === 0, 'and no more steps this session');
    check(await store(page) === before, 'localStorage is byte-identical after the undo');
    await page.context().close();
  }
  {
    const page = await fresh();
    await enter(page);
    await page.evaluate(() => document.getElementById('play-ssao').click());
    await page.evaluate(() => document.getElementById('play-ssao').click());
    const steps = await missFor(page, 12000);
    check(steps.join() === 'scale75', `a box the player touched is theirs: ${steps.join()}`);
    check(await page.evaluate(() => window.__playView.renderer.ssaoFrame !== null), 'and its rung stays as the player set it');
    await page.context().close();
  }
  for (const [label, boxes] of [['Retail mode', ['play-retail-mode']], ['PSX rasterisation', ['play-psx-render']],
    ['Retail mode with PSX rasterisation', ['play-retail-mode', 'play-psx-render']]]) {
    const page = await fresh();
    await enter(page);
    await page.evaluate((boxes) => { for (const id of boxes) document.getElementById(id).click(); }, boxes);
    const before = await store(page);
    check((await missFor(page, 120000)).length === 0, `${label} never engages it`);
    check(await page.evaluate(() => window.__playView._quality.stepped.length === 0), `${label}: nothing even recorded`);
    check(await store(page) === before, `${label}: localStorage unchanged`);
    /* And the same page steps once the box comes off, so the checks above are not vacuous. */
    await page.evaluate((boxes) => { for (const id of boxes) document.getElementById(id).click(); }, boxes);
    check((await missFor(page, 12000)).length > 0, `${label}: the page steps once the mode is off`);
    await page.evaluate(() => document.getElementById('play-retail-mode').click());
    const held = await page.evaluate(() => window.__playView.qualitySteps().length);
    check(held === 0, `${label}: a stepped rung stops counting when Retail mode comes on`);
    await page.context().close();
  }
  {
    const page = await fresh('?autoquality=0');
    await enter(page);
    check(await page.evaluate(() => !window.__playView._quality), '?autoquality=0 leaves it unarmed');
    await page.context().close();
  }
  await browser.close();
  stop();
}

if (failures.length) {
  console.error(`\n${failures.length} failed:\n  ${failures.join('\n  ')}`);
  process.exit(1);
}
console.log('\nquality ladder: all checks passed');
process.exit(0);
