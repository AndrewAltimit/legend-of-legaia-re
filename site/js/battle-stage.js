/* battle-stage.js - the view toggle both art/spell pages share.
 *
 * Holds one slot that shows either the full battle (the play page's engine
 * runtime in an iframe: party member, enemy, stage, camera, HUD, the cast's
 * own effect layer and sound) or the bare mesh the page's own canvas draws.
 * Used by magic.html (js/summon-view.js) and arts.html (js/arts-viewer.js);
 * neither page forks it.
 *
 * Why it holds exactly one engine: the runtime is a whole wasm instance and a
 * disc copy, so it is built only when the full scene is the view and on the
 * first selection, and torn down (iframe unloaded) as soon as the view leaves
 * it. Selecting through a list debounces, then re-uses the running engine
 * through its `__playLaunch` hook: the previous fight is replaced in place,
 * not stacked or rebooted. A launch before the engine is up waits for it.
 *
 * Disc model: the runtime reads the same IndexedDB slot as the rest of the
 * site (rom-cache.js), so it starts only after the page's disc has loaded.
 * If the frame has not produced an engine after BOOT_TIMEOUT_MS the page
 * gives up, says why and falls back to the model view rather than leaving a
 * blank pane.
 *
 *   const stage = new BattleStage({
 *     frame: iframeEl, root: rootEl, toggle: toggleEl, boot: bootEl,
 *     poster: posterEl, storageKey: 'legaia-stage-view:magic',
 *     onViewChange: (view) => { ... },
 *   });
 *   stage.request({ cast: '0x9e', target: '3' });   // the selection's launch
 *
 * The page keeps everything that only belongs to a view inside containers
 * marked `data-stage-view="scene"` or `"mesh"`; the toggle shows and hides
 * them through `root[data-view]`.
 */
(function () {
  'use strict';

  const BOOT_TIMEOUT_MS = 90000;   /* a 700 MB disc can take a while to cache */
  const BOOT_POLL_MS = 400;
  const SELECT_DEBOUNCE_MS = 220;  /* quick selection coalesces to the last */
  const VIEWS = ['scene', 'mesh'];
  const DEFAULT_VIEW = 'scene';

  class BattleStage {
    /* `o`: { frame, root, toggle, boot, poster, storageKey, onViewChange } */
    constructor(o) {
      this.frame = o.frame;
      this.root = o.root;
      this.toggle = o.toggle;
      this.boot = o.boot || null;
      this.poster = o.poster || null;
      this.storageKey = o.storageKey;
      this.onViewChange = o.onViewChange || null;
      this.view = this._readView();
      this.current = null;       /* the selection's query, or null */
      this.booted = false;       /* iframe holds a live engine */
      this.bootStart = 0;
      this._debounce = 0;
      this._poll = 0;
      this._wireToggle();
      this._applyView();
    }

    _readView() {
      try {
        const v = window.localStorage.getItem(this.storageKey);
        return VIEWS.includes(v) ? v : DEFAULT_VIEW;
      } catch (e) { return DEFAULT_VIEW; }
    }

    _writeView(view) {
      try { window.localStorage.setItem(this.storageKey, view); } catch (e) { /* private mode */ }
    }

    get isScene() { return this.view === 'scene'; }

    /* Switch view. A forced fallback (`opts.fallback`) keeps the visitor's
     * remembered choice: the next page load tries the battle again. */
    setView(view, opts) {
      if (!VIEWS.includes(view) || view === this.view) return;
      this.view = view;
      if (!(opts && opts.fallback)) this._writeView(view);
      this._applyView();
      if (this.view === 'scene') {
        if (this.current) this._launchSoon();
        else this._showPoster();
      } else {
        this._teardown();
      }
      if (this.onViewChange) this.onViewChange(this.view, opts && opts.reason);
    }

    /* The selection changed. In the mesh view this only records it, so that
     * a later switch to the battle plays the same thing. */
    request(query) {
      this.current = query || null;
      if (this.isScene && this.current) this._launchSoon();
    }

    /* Nothing is selected: drop the battle and show the prompt. */
    clear(message) {
      this.current = null;
      this._teardown();
      if (message && this.poster) this.poster.textContent = message;
      this._showPoster();
    }

    /* Replay the current selection in the running engine. */
    again() {
      if (this.isScene && this.current) this._launchNow();
    }

    _launchSoon() {
      clearTimeout(this._debounce);
      this._debounce = setTimeout(() => this._launchNow(), SELECT_DEBOUNCE_MS);
    }

    _launchNow() {
      clearTimeout(this._debounce);
      if (!this.isScene || !this.current) return;
      this._hidePoster();
      const w = this.frame.contentWindow;
      if (this.booted && w && typeof w.__playLaunch === 'function') {
        try { w.__playLaunch(this.current); return; } catch (e) { console.warn('battle-stage', e); }
      }
      if (this.frame.hasAttribute('src') && !this.booted) return;
      this._boot();
    }

    _boot() {
      /* No cached disc: the frame would show its own disc picker. */
      if (window.RomCache && window.RomCache.meta) {
        window.RomCache.meta().then((m) => {
          if (!m && !this.booted) this._fallBack('The disc is not in this browser\'s cache, so the battle engine cannot read it.');
        }).catch(() => {});
      }
      const q = new URLSearchParams(Object.assign({}, this.current, { embed: '1' }));
      this.booted = false;
      this.bootStart = performance.now();
      this.frame.hidden = false;
      this._setBoot(true);
      this.frame.src = 'play.html?' + q.toString();
      clearInterval(this._poll);
      this._poll = setInterval(() => this._pollBoot(), BOOT_POLL_MS);
    }

    _pollBoot() {
      let ready = false;
      try {
        const w = this.frame.contentWindow;
        ready = !!(w && w.__playRuntime && typeof w.__playLaunch === 'function');
      } catch (e) { ready = false; }
      if (ready) {
        clearInterval(this._poll);
        this.booted = true;
        this._setBoot(false);
        try { this.frame.focus(); } catch (e) { /* iframe policy */ }
        /* A selection made during the boot wins over the one in the URL. */
        const w = this.frame.contentWindow;
        const pending = this.current;
        const sent = this._frameQuery();
        if (pending && JSON.stringify(sent) !== JSON.stringify(pending)) w.__playLaunch(pending);
        return;
      }
      if (performance.now() - this.bootStart > BOOT_TIMEOUT_MS) {
        clearInterval(this._poll);
        this._fallBack('The battle engine did not start here.');
      }
    }

    /* Give up on the battle for this visit: say why, show the model. */
    _fallBack(reason) {
      console.warn('battle-stage: ' + reason);
      if (this.isScene) this.setView('mesh', { fallback: true, reason });
    }

    /* The query the iframe was booted with, for the boot-race compare. */
    _frameQuery() {
      const out = {};
      try {
        new URL(this.frame.src).searchParams.forEach((v, k) => { if (k !== 'embed') out[k] = v; });
      } catch (e) { /* no src */ }
      return out;
    }

    /* Unload the engine: this frees its wasm memory and stops its audio. */
    _teardown() {
      clearTimeout(this._debounce);
      clearInterval(this._poll);
      this.booted = false;
      this.frame.removeAttribute('src');
      this.frame.hidden = true;
      this._setBoot(false);
    }

    _showPoster() { if (this.poster && this.isScene) this.poster.hidden = false; }
    _hidePoster() { if (this.poster) this.poster.hidden = true; }

    _setBoot(on) {
      if (!this.boot) return;
      this.boot.hidden = !on;
      if (on) this._setBootMessage('Starting the battle engine… the first start reads your disc from the browser cache.');
    }

    _setBootMessage(text) { if (this.boot) this.boot.textContent = text; }

    _applyView() {
      this.root.setAttribute('data-view', this.view);
      this.toggle.querySelectorAll('[data-view]').forEach((b) => {
        const on = b.getAttribute('data-view') === this.view;
        b.setAttribute('aria-pressed', on ? 'true' : 'false');
      });
    }

    _wireToggle() {
      this.toggle.addEventListener('click', (e) => {
        const b = e.target.closest('[data-view]');
        if (b) this.setView(b.getAttribute('data-view'));
      });
    }
  }

  window.BattleStage = BattleStage;
})();
