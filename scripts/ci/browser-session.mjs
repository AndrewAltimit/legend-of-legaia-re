// What the browser measurement drivers share: frame-bench-web.mjs and
// frame-hash-web.mjs serve a `site/` directory themselves and drive its play page in
// a headless Chromium through playwright-core. PLAYWRIGHT_CORE names playwright-core's
// directory when `~/.npm/_npx` does not hold one.

import { createRequire } from 'module';
import { spawn } from 'child_process';
import fs from 'fs';
import os from 'os';
import path from 'path';

export function playwrightCore() {
  const home = os.homedir();
  const roots = process.env.PLAYWRIGHT_CORE ? [process.env.PLAYWRIGHT_CORE] : [];
  const npx = path.join(home, '.npm/_npx');
  if (fs.existsSync(npx)) {
    for (const d of fs.readdirSync(npx)) roots.push(path.join(npx, d, 'node_modules/playwright-core'));
  }
  for (const r of roots) if (fs.existsSync(path.join(r, 'package.json'))) return createRequire(import.meta.url)(r);
  throw new Error('playwright-core not found; set PLAYWRIGHT_CORE');
}

/* A static server over `site`, on a port of its own: `{ port, stop }`. */
export async function serveSite(site) {
  const server = spawn('python3', ['-u', '-m', 'http.server', '0', '--bind', '127.0.0.1', '--directory', site],
    { stdio: ['ignore', 'pipe', 'pipe'] });
  const port = await new Promise((res, rej) => {
    const on = (d) => { const m = /port (\d+)/.exec(String(d)); if (m) res(+m[1]); };
    server.stdout.on('data', on); server.stderr.on('data', on);
    setTimeout(() => rej(new Error('no server')), 10000);
  });
  const stop = () => { try { server.kill(); } catch (e) { /* gone */ } };
  process.on('exit', stop);
  return { port, stop };
}

/* `gl`: 'egl' asks for the real GPU, 'swiftshader' for the software rasteriser.
 * `vsync`: leave the browser's frame pacing on, which is what a display does to the
 * page; off lets requestAnimationFrame run as fast as the page gets through it. */
export async function launchChromium({ gl = 'egl', vsync = false } = {}) {
  const { chromium } = playwrightCore();
  const dirs = fs.readdirSync(path.join(os.homedir(), '.cache/ms-playwright')).filter((d) => /^chromium-/.test(d)).sort();
  const dir = path.join(os.homedir(), '.cache/ms-playwright', dirs[dirs.length - 1]);
  const exe = [path.join(dir, 'chrome-linux/chrome'), path.join(dir, 'chrome-linux64/chrome')].find((p) => fs.existsSync(p));
  const glArgs = gl === 'swiftshader'
    ? ['--use-gl=angle', '--use-angle=swiftshader', '--enable-unsafe-swiftshader']
    : ['--use-gl=angle', '--use-angle=gl-egl', '--ignore-gpu-blocklist'];
  const pacing = vsync ? [] : ['--disable-frame-rate-limit', '--disable-gpu-vsync'];
  return chromium.launch({ executablePath: exe, headless: true,
    args: [...glArgs, '--enable-webgl', ...pacing, '--autoplay-policy=no-user-gesture-required'] });
}

/* The GL renderer string the page's context gets. */
export function rendererString(page) {
  return page.evaluate(() => {
    const gl = document.createElement('canvas').getContext('webgl2');
    const x = gl && gl.getExtension('WEBGL_debug_renderer_info');
    return x ? gl.getParameter(x.UNMASKED_RENDERER_WEBGL) : 'unknown';
  });
}

/* Every process under this one, by Chromium role: `{ renderer: [{pid, rssMb, peakMb}], .. }`.
 * Read off /proc, so it costs the page nothing; empty off Linux. Which renderer is the
 * page's is not named here: a spare is small, so take the largest. */
export function chromiumProcesses() {
  const out = {};
  let pids;
  try { pids = fs.readdirSync('/proc').filter((d) => /^\d+$/.test(d)); } catch (e) { return out; }
  const kids = new Map();
  for (const pid of pids) {
    try {
      const stat = fs.readFileSync(`/proc/${pid}/stat`, 'latin1');
      const ppid = +stat.slice(stat.lastIndexOf(')') + 2).split(' ')[1];
      if (!kids.has(ppid)) kids.set(ppid, []);
      kids.get(ppid).push(pid);
    } catch (e) { /* gone */ }
  }
  const walk = [String(process.pid)];
  while (walk.length) {
    for (const pid of kids.get(+walk.pop()) || []) {
      walk.push(pid);
      try {
        /* Chromium rewrites its own title, so its arguments are one string, not NUL-split. */
        const cmd = fs.readFileSync(`/proc/${pid}/cmdline`, 'latin1');
        if (!/chrome/.test(cmd)) continue;
        const type = (/--type=([\w-]+)/.exec(cmd) || [0, 'browser'])[1];
        const role = type === 'utility' ? (/--utility-sub-type=([\w.-]+)/.exec(cmd) || [0, 'utility'])[1] : type;
        const status = fs.readFileSync(`/proc/${pid}/status`, 'latin1');
        const kb = (k) => { const m = new RegExp(`^${k}:\\s+(\\d+)`, 'm').exec(status); return m ? +m[1] : 0; };
        (out[role] = out[role] || []).push({ pid: +pid, rssMb: kb('VmRSS') / 1024, peakMb: kb('VmHWM') / 1024 });
      } catch (e) { /* gone */ }
    }
  }
  return out;
}

/* The largest process of a role, `{ rssMb, peakMb }` or null. */
export function largestOf(procs, role) {
  const list = procs[role] || [];
  return list.reduce((a, p) => (!a || p.rssMb > a.rssMb ? p : a), null);
}
