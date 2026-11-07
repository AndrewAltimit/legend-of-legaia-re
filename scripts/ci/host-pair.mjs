#!/usr/bin/env node
// Host pair: the same scene on the same pad script in the native window and in the
// browser play page, paired by world frame, frame against frame and state against state.
//
//   scripts/ci/host-pair.mjs --site site --engine target/release-test/legaia-engine --out DIR
//       [--scene town_walk,menu,...] [--mode retail,enhanced] [--every n] [--ticks n]
//       [--gl swiftshader|egl] [--threshold 32] [--keep-all] [--from n]
//
// The native side is `play-window`'s own capture sweep (`--screenshot-every`, `--pad-script`)
// with `LEGAIA_SWEEP_TRACE` writing `engine-session::host_trace` beside each PNG. The page side
// steps the page's frame by hand as frame-hash-web.mjs does, keys the same script through the
// page's own listeners, and at each frame the native run captured composites the GL canvas and
// the overlay canvas and reads the same trace through `debug_host_trace_text`.
//
// Each pair is reduced to 320x240 and scored: the share of pixels off by more than the threshold
// in any channel. A trace difference names its first differing lines. Pairs over `--report` land
// in DIR as `<scene>-<mode>-<tick>-{native,page,diff}.png`. Everything in DIR is Sony-derived:
// scratch only, never committed. See docs/tooling/host-drift.md#the-host-pair-run.

import { spawnSync } from 'child_process';
import fs from 'fs';
import path from 'path';
import { fileURLToPath } from 'url';
import { launchChromium, playBundle, serveSite } from './browser-session.mjs';

const argv = process.argv;
const arg = (name, dflt) => {
  const i = argv.indexOf('--' + name);
  return i < 0 ? dflt : (argv[i + 1] && !argv[i + 1].startsWith('--') ? argv[i + 1] : true);
};
const REPO = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');

const KEY = { Up: 'ArrowUp', Down: 'ArrowDown', Left: 'ArrowLeft', Right: 'ArrowRight', Cross: 'KeyZ', Circle: 'KeyX', Triangle: 'KeyC', Square: 'KeyV', Start: 'Enter', L1: 'KeyQ', R1: 'KeyE' };

const walk = [[30, 110, 'Down'], [120, 200, 'Up'], [210, 290, 'Left']];
const menuTaps = () => {
  const t = [[30, 'Start']];
  for (let row = 0; row < 7; row++) {
    const at = 90 + row * 120;
    t.push([at, 'Cross'], [at + 60, 'Circle'], [at + 90, 'Down']);
  }
  return t;
};

const arts = (from, to) => {
  const cyc = ['Cross', 'Left', 'Right', 'Up', 'Down', 'Left', 'Cross', 'Cross'];
  const t = [];
  for (let n = from, i = 0; n <= to; n += 12, i++) t.push([n, cyc[i % cyc.length]]);
  return t;
};
const savePause = () => {
  const t = [[30, 'Start']];
  for (let i = 0; i < 6; i++) t.push([60 + i * 15, 'Down']); /* Items .. Save, the seventh row */
  for (const n of [170, 260, 330, 420]) t.push([n, 'Cross']);
  t.push([470, 'Up'], [500, 'Cross']); /* the confirm starts on No */
  return t; /* the save closes the menu; `slot_save` resumes a window-written save on both */
};
const every = (from, to, step, b) => { const t = []; for (let n = from; n <= to; n += step) t.push([n, b]); return t; };

/* `hold` [from, to, button], `taps` [tick, button], `env` for the native side with the page's
 * twin in `at` (tick -> a call on the runtime), `args` extra native switches. */
export const SCENES = {
  town_walk: { scene: 'town01', ticks: 300, every: 30, hold: walk },
  map_walk: { scene: 'map01', ticks: 300, every: 30, hold: walk },
  /* A scene with a ground-fog bank (the Ravine), for `--mode fog_bloom`. */
  fog_walk: { scene: 'keikoku', ticks: 85, every: 10, hold: walk }, /* the walk leaves the scene at about tick 90 */
  menu: { scene: 'town01', ticks: 960, every: 30, taps: menuTaps() },
  dialog: { scene: 'town01', ticks: 600, every: 30, talk: { tick: 30, slot: 0 }, taps: every(120, 590, 40, 'Cross') },
  shop: { scene: 'bylon', ticks: 420, every: 15, shop: { tick: 30, idx: 0 }, taps: [[90, 'Cross'], [150, 'Down'], [200, 'Cross'], [260, 'Circle'], [320, 'Circle']] },
  battle: { scene: 'map01', ticks: 900, every: 30, battle: 30, taps: every(300, 890, 40, 'Cross') },
  results: { scene: 'map01', ticks: 3600, every: 60, battle: 30, taps: every(300, 3590, 20, 'Cross') },
  cast: { scene: 'map01', ticks: 900, every: 30, battle: 30, inflight: [0, 138, 3] },
  /* The round prompt's Begin, then the command ring's Item window (Up), its cursor,
   * back out, the Magic direction, and an item picked: engine-screens' battle windows. */
  battle_menus: { scene: 'map01', ticks: 640, every: 10, battle: 30, taps: [[380, 'Cross'], [420, 'Up'], [450, 'Down'], [470, 'Down'], [500, 'Circle'], [530, 'Right'], [560, 'Circle'], [590, 'Up'], [620, 'Cross']] },
  /* Arts input: open the command ring, then direction presses and confirms on a cycle. */
  arts: { scene: 'map01', ticks: 1500, every: 30, battle: 30, taps: arts(300, 1490) },
  /* The retock innkeeper (placement 26, the 240 G stay): the field-VM inn record's prompt,
   * picker and rest fade, stepped on Cross. */
  inn: { scene: 'retock', ticks: 900, every: 30, talk: { tick: 30, slot: 26 }, taps: every(90, 890, 30, 'Cross') },
  /* Pause menu Save on the world map (saving works anywhere there): slot, block, Yes. */
  save: { scene: 'map01', ticks: 720, every: 30, taps: savePause() },
  /* Off retail's vantage over sloped ground: the player seated at town01 tile (30, 22) and
   * the follow knobs held at orbit 2.7, tilt -0.5, the view past the plateau ramp toward the
   * sea (the field ground's dropped-floor rule, docs/subsystems/renderer.md). */
  town_slope: { scene: 'town01', ticks: 120, every: 10, seat: [30 * 128 + 0x40, 22 * 128 + 0x40], knobs: [2.7, -0.5, 1.0] },
  town_seat: { scene: 'town01', ticks: 120, every: 10, seat: [30 * 128 + 0x40, 22 * 128 + 0x40] },
  /* Dungeons: Snowdrift Cave and Drake Castle's mist entrance. */
  cave_walk: { scene: 'cave01', ticks: 300, every: 30, hold: walk },
  dolk_walk: { scene: 'dolk', ticks: 300, every: 30, hold: walk },
  /* Ending vignettes under the cutscene's own camera lock (no picker entry: the page enters
   * them from its running scene). */
  ed_bylon: { scene: 'edbylon', viaTown: true, ticks: 900, every: 30 },
  ed_korout: { scene: 'edkorout', viaTown: true, ticks: 900, every: 30 },
};
/* The five minigames from the picker's own entries, on a rhythm of presses. */
const mgPad = () => {
  const t = [];
  for (let n = 40; n < 700; n += 16) t.push([n, ['Cross', 'Square', 'Circle'][((n / 16) | 0) % 3]]);
  t.push([700, 'Start'], [760, 'Start']);
  return t;
};
for (const [name, sub, key] of [['slots', 3, 'O'], ['baka', 4, 'B'], ['dance', 6, 'K'], ['muscle', 5, 'M'], ['fishing', 0, 'L']]) {
  SCENES[name] = { scene: 'town01', ticks: 840, every: 30, warp: { tick: 20, sub, key }, coins: 300, taps: mgPad() };
}

/* A retail save off a card: the page lands it and writes the LGSF the window then resumes, so
 * every one of these also crosses a save from one host to the other. `card` is under saves/library/cards. */
const CARD = { late: 'playthrough-endgame-7saves.mcr', ladder: 'playthrough-ladder-pro00-14.mcr' };
const cardScenes = (key, card, block) => ({
  [`${key}_walk`]: { card, block, ticks: 300, every: 30, hold: walk },
  [`${key}_menu`]: { card, block, ticks: 960, every: 30, taps: menuTaps() },
  [`${key}_fight`]: { card, block, ticks: 900, every: 30, battle: 30, inflight: [0, 138, 3], taps: every(300, 890, 40, 'Cross') },
});
Object.assign(SCENES, cardScenes('late', CARD.late, 1), cardScenes('late4', CARD.late, 4), cardScenes('ladder', CARD.ladder, 3));

/* A seeded rhythm of presses over the whole pad: both hosts reach whichever screens it reaches,
 * and agreeing is the point, not where it goes. */
const monkey = (seed, from, to, step) => {
  const pad = ['Up', 'Down', 'Left', 'Right', 'Cross', 'Circle', 'Triangle', 'Cross', 'Square', 'Start', 'Cross', 'L1', 'R1'];
  const t = [];
  let s = seed >>> 0;
  for (let n = from; n < to; n += step) {
    s = (Math.imul(s, 1664525) + 1013904223) >>> 0;
    t.push([n, pad[(s >>> 16) % pad.length]]);
  }
  return t;
};
SCENES.monkey_town = { scene: 'town01', ticks: 1800, every: 30, taps: monkey(1, 30, 1790, 11) };
SCENES.monkey_map = { scene: 'map01', ticks: 1800, every: 30, taps: monkey(2, 30, 1790, 13) };
SCENES.monkey_shop = { scene: 'bylon', ticks: 1200, every: 30, shop: { tick: 30, idx: 0 }, taps: monkey(3, 60, 1190, 17) };
SCENES.monkey_fight = { card: CARD.late, block: 1, ticks: 1800, every: 30, battle: 30, taps: monkey(4, 240, 1790, 19) };
SCENES.monkey_menu = { card: CARD.late, block: 4, ticks: 1800, every: 30, taps: [[30, 'Start'], ...monkey(5, 60, 1790, 23)] };
/* A save the window wrote, resumed on both. */
SCENES.slot_save = { lgsfPath: path.join(REPO, 'saves/slot_00.bin'), card: 'native', ticks: 300, every: 30, hold: walk };

const MODES = {
  retail: { native: ['--retail', '--psx-render'], boxes: ['play-retail-mode', 'play-psx-render'] },
  enhanced: { native: [], boxes: [] },
  psx_only: { native: ['--psx-render'], boxes: ['play-psx-render'] },
  retail_only: { native: ['--retail'], boxes: ['play-retail-mode'] },
  /* Enhanced with one pass off on both hosts: splits a pixel difference by the pass behind it. */
  no_ssao: { native: ['--no-ssao'], boxes: [], unboxes: ['play-ssao'] },
  no_lighting: { native: ['--no-dynamic-lighting'], boxes: [], unboxes: ['play-dyn-light'] },
  no_fog: { native: ['--no-volumetric-fog'], boxes: [], unboxes: ['play-ground-fog'] },
  no_fade: { native: ['--no-occlusion-fade'], boxes: [], unboxes: ['play-see-through'] },
  /* The two passes that default off, on for both hosts: the window has no switch that turns
   * either on, so its run reads them from the options file in its own working directory. */
  fog_bloom: { native: [], options: 'volumetric_fog = true\nbloom = true\n', boxes: ['play-ground-fog', 'play-bloom'] },
};

function nativeRun(engine, name, sc, mode, dir, opts) {
  fs.rmSync(dir, { recursive: true, force: true });
  fs.mkdirSync(dir, { recursive: true });
  const script = [
    ...(sc.hold || []).map(([a, b, k]) => `${a}-${b}:${k}`),
    ...(sc.taps || []).map(([t, k]) => `${t}:${k}`),
  ].join(',');
  const a = ['play-window', '--disc', process.env.LEGAIA_DISC_BIN, '--scene', sc.scene || sc.landed,
    ...MODES[mode].native,
    '--screenshot-every', String(opts.every || sc.every), '--screenshot-dir', dir,
    '--screenshot-last-tick', String(opts.ticks || sc.ticks)];
  if (script) a.push('--pad-script', script);
  if (sc.coins) a.push('--cheat-coins', String(sc.coins));
  if (sc.warp) a.push('--key-script', `${sc.warp.tick}:${sc.warp.key}`);
  if (sc.lgsfPath) a.push('--resume-save', sc.lgsfPath);
  const env = { ...process.env, LEGAIA_SWEEP_TRACE: '1', LEGAIA_CAPTURE_NO_PRESENT: '1', LEGAIA_AUDIO_DETACHED: '1' };
  if (sc.talk) env.LEGAIA_CAPTURE_TALK = `${sc.talk.slot}@${sc.talk.tick}`;
  if (sc.shop) env.LEGAIA_CAPTURE_SHOP = `${sc.shop.idx}@${sc.shop.tick}`;
  if (sc.seat) env.LEGAIA_SEAT = sc.seat.join(',');
  if (sc.knobs) env.LEGAIA_DIAG_FOLLOW_KNOBS = sc.knobs.join(',');
  if (sc.battle) env.LEGAIA_CAPTURE_BATTLE = [sc.battle, ...(sc.inflight || [])].join(',');
  const cwd = fs.mkdtempSync(path.join(dir, 'cwd-')); /* a clean options file on each run */
  if (MODES[mode].options) fs.writeFileSync(path.join(cwd, 'legaia-options.toml'), MODES[mode].options);
  const r = spawnSync(engine, a, { env, cwd, encoding: 'utf8', timeout: 1200000 });
  if (r.status !== 0) console.error(`[host-pair] native ${name}/${mode}: exit ${r.status} ${r.signal || ''}\n${(r.stderr || '').slice(-1500)}`);
  return fs.readdirSync(dir).filter((f) => /^tick_\d+\.png$/.test(f)).sort();
}

const traceFrame = (text) => { const m = /^\s*frames?: (\d+),/m.exec(text || ''); return m ? +m[1] : null; };

/* In the page: step, and at each wanted tick hand back the composite and the trace. */
async function pageRun(cfg) {
  const rt = window.__playRuntime;
  const sc = cfg.sc;
  /* A scene the picker does not list (the ending vignettes) is entered from town01's. */
  const pick = document.querySelector(`[data-scene="${sc.scene || 'town01'}"]`);
  const btn = pick || document.querySelector('[data-scene="town01"]');
  if (!btn) throw new Error('no scene button ' + sc.scene);
  for (const id of cfg.boxes) { const el = document.getElementById(id); if (!el) throw new Error('no box ' + id); if (!el.checked) el.click(); }
  for (const id of cfg.unboxes) { const el = document.getElementById(id); if (!el) throw new Error('no box ' + id); if (el.checked) el.click(); }
  const P = (window.__pair = window.__pair);
  P.vt = 1e6; P.virtual = true;
  btn.click();
  const view = window.__playView;
  view.stop();
  view.setPaused(true);
  if (!pick) view.enter(sc.scene);
  if (sc.seat && !rt.play_debug_seat(sc.seat[0], sc.seat[1])) throw new Error('seat refused');
  /* The native side's LEGAIA_DIAG_FOLLOW_KNOBS: the knobs written raw, every frame. */
  const holdKnobs = () => {
    if (!sc.knobs) return;
    rt.play_camera_set_orbit(sc.knobs[0]);
    rt.play_camera_tilt_by(sc.knobs[1] - rt.play_camera_tilt());
    rt.play_camera_zoom_by(sc.knobs[2] / rt.play_camera_zoom());
  };
  if (cfg.ssaoView) view.renderer.ssaoDebug = true;
  if (cfg.lgsf) {
    const bin = atob(cfg.lgsf);
    const bytes = new Uint8Array(bin.length);
    for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
    rt.import_save(bytes);
    view.resumeSave();
  }
  if (sc.coins) rt.cheat_apply('coins', sc.coins, '');
  const send = (type, code) => window.dispatchEvent(new KeyboardEvent(type, { code, key: code, bubbles: true }));
  const padAt = (n) => {
    for (const [from, to, b] of sc.hold || []) {
      if (n === from) send('keydown', cfg.key[b]);
      if (n === to + 1) send('keyup', cfg.key[b]);
    }
    for (const [at, b] of sc.taps || []) {
      if (n === at) send('keydown', cfg.key[b]);
      if (n === at + 1) send('keyup', cfg.key[b]);
    }
  };
  const comp = document.createElement('canvas');
  comp.width = 320; comp.height = 240;
  const cx = comp.getContext('2d', { willReadFrequently: true });
  const load = (b64) => new Promise((res, rej) => { const i = new Image(); i.onload = () => res(i); i.onerror = rej; i.src = 'data:image/png;base64,' + b64; });
  const out = [];
  const wanted = new Set(cfg.ticks.map((t) => t - 1)); /* the page's step n ends on world frame n + 1 */
  const last = Math.max(...cfg.ticks) - 1;
  for (let n = 0; n <= last; n++) {
    if (sc.battle === n + 1) {
      if (sc.inflight) rt.debug_seed_inflight_cast(...sc.inflight);
      rt.debug_force_battle(-1);
    }
    if (sc.talk && sc.talk.tick === n + 1) rt.debug_talk_to_placement(sc.talk.slot);
    if (sc.shop && sc.shop.tick === n + 1) rt.debug_arm_scene_shop(sc.shop.idx);
    if (sc.warp && sc.warp.tick === n + 1) {
      if (sc.warp.sub === 0) rt.play_fishing_start(); else rt.play_mg_debug_warp(sc.warp.sub);
    }
    holdKnobs();
    padAt(n + 1); /* the native script names the world frame the tick ends on, and step n ends on n + 1 */
    if (wanted.has(n)) {
      P.vt += 1000 / 60;
      view.step();
      view._frame();
      const o = view.menuOverlay;
      /* The page's picture: the GL canvas, what stacks on it. */
      const page = document.createElement('canvas');
      page.width = cfg.size[0]; page.height = cfg.size[1];
      const pc = page.getContext('2d');
      pc.imageSmoothingEnabled = false;
      pc.drawImage(view.canvas, 0, 0, page.width, page.height);
      /* Every layer stacked over it, in stacking order: the menu overlay and a minigame's hub layer. */
      for (const c of view.canvas.parentNode.querySelectorAll('canvas')) {
        /* A frame that composites its layers into GL hides their canvases by visibility. */
        if (c !== view.canvas && !c.hidden && c.style.visibility !== 'hidden') pc.drawImage(c, 0, 0, page.width, page.height);
      }
      const trace = rt.debug_host_trace_text();
      const nativeB64 = await window.__pairNative(n + 1);
      const img = await load(nativeB64);
      cx.imageSmoothingEnabled = true; cx.imageSmoothingQuality = 'high';
      cx.drawImage(img, 0, 0, 320, 240);
      const na = cx.getImageData(0, 0, 320, 240).data;
      cx.drawImage(page, 0, 0, 320, 240);
      const pa = cx.getImageData(0, 0, 320, 240).data;
      let off = 0, sum = 0, x0 = 320, y0 = 240, x1 = -1, y1 = -1;
      for (let i = 0, p = 0; i < na.length; i += 4, p++) {
        const d = Math.max(Math.abs(na[i] - pa[i]), Math.abs(na[i + 1] - pa[i + 1]), Math.abs(na[i + 2] - pa[i + 2]));
        sum += d;
        if (d > cfg.threshold) {
          off++;
          const x = p % 320, y = (p / 320) | 0;
          if (x < x0) x0 = x; if (x > x1) x1 = x; if (y < y0) y0 = y; if (y > y1) y1 = y;
        }
      }
      const score = off / (320 * 240);
      const r = { tick: n + 1, score, mean: sum / (320 * 240), box: x1 < 0 ? null : [x0, y0, x1, y1], native: img.width + 'x' + img.height, page: `${view.canvas.width}x${view.canvas.height}+${o ? o.width + 'x' + o.height : '-'}`, trace };
      if (score > cfg.report || cfg.keepAll) {
        const d = cx.createImageData(320, 240);
        for (let i = 0; i < na.length; i += 4) {
          const v = Math.max(Math.abs(na[i] - pa[i]), Math.abs(na[i + 1] - pa[i + 1]), Math.abs(na[i + 2] - pa[i + 2]));
          d.data[i] = d.data[i + 1] = d.data[i + 2] = Math.min(255, v * 3); d.data[i + 3] = 255;
        }
        cx.putImageData(d, 0, 0);
        await window.__pairShot(n + 1, 'page', page.toDataURL('image/png'));
        await window.__pairShot(n + 1, 'diff', comp.toDataURL('image/png'));
      }
      out.push(r);
    } else {
      P.vt += 1000 / 60;
      view.step();
      view._frame();
    }
    if (n % 20 === 19) await new Promise((res) => setTimeout(res, 0));
  }
  return { out, stats: rt.state_json() };
}

function traceDiff(a, b) {
  const la = (a || '').split('\n'), lb = (b || '').split('\n');
  const diffs = [];
  for (let i = 0; i < Math.max(la.length, lb.length); i++) {
    if (la[i] !== lb[i]) diffs.push(`${(la[i] || '').trim().slice(0, 300)}  |  ${(lb[i] || '').trim().slice(0, 300)}`);
  }
  return diffs;
}

/* The page lands a retail card save and writes it back as LGSF: what both hosts then resume. */
async function cardToLgsf(browser, port, sc, file) {
  const cardPath = path.join(REPO, 'saves/library/cards', sc.card);
  const context = await browser.newContext({ viewport: { width: 1280, height: 900 } });
  const page = await context.newPage();
  await page.goto(`http://127.0.0.1:${port}/play.html`);
  await (await page.$('input[type=file]')).setInputFiles(process.env.LEGAIA_DISC_BIN);
  await page.waitForFunction(() => !!window.__playRuntime, null, { timeout: 300000 });
  const r = await page.evaluate(async ({ b64, block }) => {
    const rt = window.__playRuntime;
    document.querySelector('[data-scene="town01"]').click();
    const view = window.__playView;
    view.stop();
    view.setPaused(true);
    const bin = atob(b64);
    const bytes = new Uint8Array(bin.length);
    for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
    const sum = JSON.parse(rt.import_card_save(bytes, block));
    const land = view.resumeSave();
    view.step(); view._frame();
    const out = rt.export_save();
    let s = '';
    for (let i = 0; i < out.length; i += 0x8000) s += String.fromCharCode.apply(null, out.subarray(i, i + 0x8000));
    return { lgsf: btoa(s), scene: land.scene, sum };
  }, { b64: fs.readFileSync(cardPath).toString('base64'), block: sc.block });
  await context.close();
  fs.writeFileSync(file, Buffer.from(r.lgsf, 'base64'));
  return r;
}

async function main() {
  const site = path.resolve(arg('site', path.join(REPO, 'site')));
  const engine = path.resolve(arg('engine', path.join(REPO, 'target/release-test/legaia-engine')));
  const out = path.resolve(arg('out', 'host-pair-out'));
  if (!process.env.LEGAIA_DISC_BIN || !fs.existsSync(process.env.LEGAIA_DISC_BIN) || !playBundle(site) || !fs.existsSync(engine)) {
    console.log('[skip] host-pair needs LEGAIA_DISC_BIN, a built play bundle and legaia-engine');
    return;
  }
  const names = arg('scene') ? arg('scene').split(',') : Object.keys(SCENES);
  const modes = arg('mode') ? arg('mode').split(',') : ['retail'];
  const opts = { every: +arg('every', 0), ticks: +arg('ticks', 0), threshold: +arg('threshold', 32), report: +arg('report', 0.01), from: +arg('from', 0), keepAll: !!arg('keep-all') };
  const { port, stop } = await serveSite(site);
  const browser = await launchChromium({ gl: arg('gl', 'swiftshader'), vsync: false });
  const lines = [];
  for (const name of names) for (const mode of modes) {
    const sc = SCENES[name];
    if (!sc) throw new Error('unknown scene ' + name);
    const dir = path.join(out, `${name}-${mode}`);
    fs.mkdirSync(dir, { recursive: true });
    if (sc.card === 'native') {
      sc.landed = 'town01';
    }
    if (sc.card && !sc.lgsfPath) {
      /* Beside the run, not under native/, which each run clears. */
      sc.lgsfPath = path.join(dir, 'landed.lgsf');
      const c = await cardToLgsf(browser, port, sc, sc.lgsfPath);
      sc.landed = c.scene;
      console.error(`[host-pair] ${name}: ${sc.card} block ${sc.block} lands on ${c.scene}`);
    }
    console.error(`[host-pair] ${name}/${mode}: native`);
    const shots = nativeRun(engine, name, sc, mode, path.join(dir, 'native'), opts);
    if (!shots.length) { lines.push(`${name}/${mode}: NO NATIVE FRAMES`); continue; }
    /* `--from n` pairs only the native frames from tick n on (the page still steps from 0). */
    const ticks = shots.map((f) => +/\d+/.exec(f)[0]).filter((t) => t >= opts.from);
    const first = fs.readFileSync(path.join(dir, 'native', shots[0]));
    const size = [first.readUInt32BE(16), first.readUInt32BE(20)];
    console.error(`[host-pair] ${name}/${mode}: ${ticks.length} native frames ${size.join('x')}, page`);
    const context = await browser.newContext({ viewport: { width: 1280, height: 900 } });
    const page = await context.newPage();
    const problems = [];
    page.on('pageerror', (e) => problems.push('page error: ' + e.message));
    await page.exposeFunction('__pairNative', (t) => fs.readFileSync(path.join(dir, 'native', `tick_${String(t).padStart(5, '0')}.png`)).toString('base64'));
    await page.exposeFunction('__pairShot', (t, kind, url) => fs.writeFileSync(path.join(dir, `${String(t).padStart(5, '0')}-${kind}.png`), Buffer.from(url.split(',')[1], 'base64')));
    await page.addInitScript((noMsaa) => {
      const P = (window.__pair = { virtual: false, vt: 0 });
      const real = performance.now.bind(performance);
      performance.now = () => (P.virtual ? P.vt : real());
      const get = HTMLCanvasElement.prototype.getContext;
      HTMLCanvasElement.prototype.getContext = function (kind, attrs) {
        return get.call(this, kind, /^webgl/.test(kind) ? { ...attrs, preserveDrawingBuffer: true, ...(noMsaa ? { antialias: false } : {}) } : (kind === '2d' ? { ...attrs, willReadFrequently: true } : attrs));
      };
    }, !!arg('no-msaa'));
    await page.goto(`http://127.0.0.1:${port}/play.html`);
    await (await page.$('input[type=file]')).setInputFiles(process.env.LEGAIA_DISC_BIN);
    await page.waitForFunction(() => !!window.__playRuntime, null, { timeout: 300000 });
    await page.waitForFunction(() => window.__playDisc && (window.__playDisc.fillMs !== undefined || !window.__playDisc.startFill), null, { timeout: 120000 });
    const lgsf = sc.lgsfPath ? fs.readFileSync(sc.lgsfPath).toString('base64') : null;
    const r = await page.evaluate(pageRun, { lgsf, sc, ticks, boxes: MODES[mode].boxes, unboxes: MODES[mode].unboxes || [], ssaoView: !!process.env.LEGAIA_SSAO_VIEW, key: KEY, size, threshold: opts.threshold, report: opts.report, keepAll: opts.keepAll });
    await context.close();
    let worst = null, sum = 0, offFrames = 0, firstTrace = null, traceOff = 0;
    const rows = [];
    for (const f of r.out) {
      const nt = fs.readFileSync(path.join(dir, 'native', `tick_${String(f.tick).padStart(5, '0')}.trace`), 'utf8');
      /* A card landing, or a scene the picker does not list, enters from the page's
       * running scene first, so the audio record starts a command apart; both fields
       * say only that. */
      const d = traceDiff(nt, f.trace).filter((l) => !(sc.card || sc.viaTown) || !/^audio_recent|^\d+,\s+\|\s+\d+,$|^\d{10,},\s+\|\s+\d{10,},$/.test(l));
      fs.writeFileSync(path.join(dir, `${String(f.tick).padStart(5, '0')}-page.trace`), f.trace);
      if (d.length) { traceOff++; if (!firstTrace) firstTrace = { tick: f.tick, d }; }
      sum += f.score;
      if (f.score > opts.report) offFrames++;
      if (!worst || f.score > worst.score) worst = f;
      rows.push(`${f.tick}\tframe ${traceFrame(nt)}/${traceFrame(f.trace)}\tpix ${(f.score * 100).toFixed(2)}%\tmean ${f.mean.toFixed(2)}\tbox ${f.box ? f.box.join(',') : '-'}\ttrace ${d.length}`);
    }
    fs.writeFileSync(path.join(dir, 'frames.tsv'), rows.join('\n') + '\n');
    const say = `${name}/${mode}: ${r.out.length} pairs (native ${r.out[0]?.native}, page ${r.out[0]?.page}); mean ${(100 * sum / r.out.length).toFixed(2)}% off, ${offFrames} over ${(opts.report * 100).toFixed(1)}%, worst tick ${worst?.tick} ${(100 * (worst?.score || 0)).toFixed(2)}%; trace differs on ${traceOff}${firstTrace ? `, first tick ${firstTrace.tick}: ${firstTrace.d.slice(0, 4).join(' ;; ')}` : ''}${problems.length ? '; ' + problems.join('; ') : ''}`;
    console.log(say);
    lines.push(say);
  }
  await browser.close();
  stop();
  fs.mkdirSync(out, { recursive: true });
  fs.writeFileSync(path.join(out, 'summary.txt'), lines.join('\n') + '\n');
}

main().catch((e) => { console.error(e); process.exit(1); });
