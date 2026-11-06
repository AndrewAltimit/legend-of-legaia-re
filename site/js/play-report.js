/* play-report.js - the play page's Copy report button.
 *
 * What a player pastes into a bug report: the bundle, the browser, the GL
 * renderer, what the engine was doing and the page's last warnings and errors.
 * The desktop twin is `legaia_app_theme::game_exit::report_text`, on the
 * launcher's Ctrl+C and Explorer's banner. It holds no disc bytes: the disc is
 * named by file name and size alone. Nothing leaves the page but through the
 * clipboard, on a click.
 *
 * Global API (`window.LegaiaPlayReport`):
 *   note(line)        add a line to the recent list (a trap, a recovery)
 *   text()            the report
 *   install(button)   wire a button to copy it
 */
(function () {
  'use strict';
  const KEEP = 30;
  const recent = [];
  const started = Date.now();

  /* A failing frame says the same thing sixty times a second: one line, counted. */
  function note(line) {
    const text = String(line).slice(0, 400);
    const last = recent[recent.length - 1];
    if (last && last.text === text) { last.count++; return; }
    recent.push({ at: ((Date.now() - started) / 1000).toFixed(1), text, count: 1 });
    if (recent.length > KEEP) recent.shift();
  }

  /* Ahead of every other script's use, so a warning at load is on the list. */
  for (const level of ['warn', 'error']) {
    const orig = console[level].bind(console);
    console[level] = (...args) => {
      note(`${level}: ${args.map((a) => (a && a.message) || String(a)).join(' ')}`);
      orig(...args);
    };
  }
  window.addEventListener('error', (e) => note(`uncaught: ${e.message}`));
  window.addEventListener('unhandledrejection', (e) => {
    note(`unhandled: ${(e.reason && e.reason.message) || e.reason}`);
  });

  /* A trapped engine throws on every call, and a report is most wanted then. */
  function ask(fn) {
    try { return fn(); } catch (e) { return `(unavailable: ${e.message || e})`; }
  }

  function glLine() {
    return ask(() => {
      const gl = document.createElement('canvas').getContext('webgl2');
      if (!gl) return 'no WebGL2';
      const x = gl.getExtension('WEBGL_debug_renderer_info');
      const line = x ? gl.getParameter(x.UNMASKED_RENDERER_WEBGL) : gl.getParameter(gl.RENDERER);
      const lose = gl.getExtension('WEBGL_lose_context');
      if (lose) lose.loseContext();
      return line;
    });
  }

  /* What the player is pressing with: a touch-only phone, a keyboard, a pad. */
  function inputLine() {
    return ask(() => {
      const coarse = matchMedia('(pointer: coarse)').matches;
      const fine = matchMedia('(any-pointer: fine)').matches;
      const pads = Array.from((navigator.getGamepads && navigator.getGamepads()) || [])
        .filter((g) => g && g.connected).map((g) => `${g.id} (${g.mapping || 'no mapping'})`);
      const tp = document.querySelector('.touch-pad');
      return `pointer ${coarse ? 'coarse' : 'fine'}${fine ? '' : ' only'}, ${navigator.maxTouchPoints | 0} touch points, `
        + `touch pad ${tp ? (tp.classList.contains('is-hidden') ? 'hidden' : 'shown') : 'absent'}, `
        + `gamepads: ${pads.length ? pads.join('; ') : 'none'}`;
    });
  }

  function text() {
    const rt = window.__playRuntime;
    const disc = window.__playDisc;
    const input = document.getElementById('play-file');
    const picked = input && input.files && input.files[0];
    const lines = [
      'Legend of Legaia port: play page',
      `Bundle: ${window.LEGAIA_WASM_V || 'unknown'}`,
      `Browser: ${navigator.userAgent}`,
      `GL: ${glLine()}`,
      `Window: ${innerWidth}x${innerHeight} at ${devicePixelRatio}x`,
      `Input: ${inputLine()}`,
      `Disc file: ${picked ? `${picked.name}, ${picked.size} bytes` : disc ? `${disc.size} bytes` : 'none'}`,
      `Status: ${ask(() => document.getElementById('play-status').textContent.trim().replace(/\s+/g, ' '))}`,
    ];
    if (rt) {
      lines.push(`Engine: ${ask(() => rt.state_json())}`);
      lines.push(`Mode: ${ask(() => rt.mode_state_json())}`);
    }
    for (const key of ['legaia.options', 'legaia.switches', 'legaia.touch-pad']) {
      lines.push(`${key}: ${ask(() => localStorage.getItem(key))}`);
    }
    lines.push(`Recent warnings and errors (${recent.length}):`);
    const list = recent.map((r) => `[${r.at}s] ${r.text}${r.count > 1 ? ` (x${r.count})` : ''}`);
    return lines.concat(list).join('\n') + '\n';
  }

  /* No clipboard on a plain-http page, or no permission: the text goes in a box. */
  function showBox(button, report) {
    let box = document.getElementById('play-report-box');
    if (!box) {
      box = document.createElement('textarea');
      box.id = 'play-report-box';
      box.readOnly = true;
      box.rows = 8;
      box.setAttribute('aria-label', 'Report text: select it and copy');
      button.closest('.disc-bar').insertAdjacentElement('afterend', box);
    }
    box.value = report;
    box.focus();
    box.select();
  }

  function install(button) {
    if (!button) return;
    const label = button.textContent;
    let timer = 0;
    button.addEventListener('click', async () => {
      const report = text();
      let copied = false;
      try {
        await navigator.clipboard.writeText(report);
        copied = true;
      } catch (e) { showBox(button, report); }
      button.textContent = copied ? 'Copied' : 'Select the text below';
      clearTimeout(timer);
      timer = setTimeout(() => { button.textContent = label; }, 2500);
    });
  }

  window.LegaiaPlayReport = { note, text, install };
})();
