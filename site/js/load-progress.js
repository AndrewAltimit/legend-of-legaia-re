/* load-progress.js - shared loading-progress widget for the interactive
 * disc pages (viewer, world-overview, monsters, audio).
 *
 * These pages read a hundreds-of-MB disc image and then hand it to a
 * synchronous WASM `load_disc` parse. Two distinct waits:
 *
 *   1. Reading the file into memory - asynchronous, and FileReader fires
 *      `progress` events, so this phase gets a *real* determinate bar.
 *   2. The WASM parse / LZS decode - a single synchronous call that
 *      blocks the main thread, so a bar can't animate *during* it. We
 *      show an indeterminate (animated) bar with a clear phase label and
 *      paint it before the blocking call via `paint()`, so the user sees
 *      "Parsing ..." rather than a frozen page with no feedback.
 *
 * Global API (`window.LoadProgress`):
 *   LoadProgress.create(hostEl) -> controller
 *
 * controller:
 *   .stage(text, pct)      determinate bar; pct in 0..1
 *   .indeterminate(text)   animated indeterminate bar
 *   .read(source, label)   read source bytes with determinate progress;
 *                          resolves to a Uint8Array. `source` is a File,
 *                          a Blob, or a RomCache-style {blob, arrayBuffer}.
 *   .paint()               await two animation frames so the bar repaints
 *                          before a long synchronous call
 *   .done(text?)           success: fill + fade out
 *   .fail(text)            error state (red), stays visible
 *   .hide()                remove the bar immediately
 */
(function () {
  'use strict';

  function ensureStyle() {
    if (document.getElementById('load-progress-style')) return;
    var s = document.createElement('style');
    s.id = 'load-progress-style';
    s.textContent = [
      '.load-progress{margin:.5rem 0;font-family:var(--font-mono,monospace);',
      'font-size:.78rem;color:var(--text-muted,#9aa);}',
      '.load-progress-label{margin-bottom:.3rem;display:flex;justify-content:space-between;gap:1rem;}',
      '.load-progress-pct{color:var(--text-dim,#778);}',
      '.load-progress-track{height:8px;border-radius:4px;',
      'background:rgba(120,130,200,.15);overflow:hidden;position:relative;}',
      '.load-progress-fill{height:100%;width:0;border-radius:4px;',
      'background:linear-gradient(90deg,var(--accent,#6cf),#9bd0ff);',
      'transition:width .15s ease;}',
      '.load-progress.is-indeterminate .load-progress-fill{width:40%;',
      'position:absolute;left:-40%;animation:load-progress-slide 1.1s ease-in-out infinite;',
      'transition:none;}',
      '@keyframes load-progress-slide{0%{left:-40%}100%{left:100%}}',
      '.load-progress.is-error .load-progress-fill{background:#d36;animation:none;',
      'width:100%;position:static;}',
      '.load-progress.is-done .load-progress-fill{background:#5b8;animation:none;',
      'width:100%;position:static;}',
      '.load-progress.is-done{transition:opacity .4s ease;}',
    ].join('');
    document.head.appendChild(s);
  }

  function create(hostEl) {
    ensureStyle();

    // Reuse an existing bar for this host rather than stacking duplicates
    // across repeated loads.
    if (hostEl && hostEl._loadProgress) {
      var c = hostEl._loadProgress;
      c._reset();
      return c;
    }

    var el = document.createElement('div');
    el.className = 'load-progress';
    el.setAttribute('role', 'progressbar');

    var labelRow = document.createElement('div');
    labelRow.className = 'load-progress-label';
    var label = document.createElement('span');
    label.className = 'load-progress-text';
    var pct = document.createElement('span');
    pct.className = 'load-progress-pct';
    labelRow.appendChild(label);
    labelRow.appendChild(pct);

    var track = document.createElement('div');
    track.className = 'load-progress-track';
    var fill = document.createElement('div');
    fill.className = 'load-progress-fill';
    track.appendChild(fill);

    el.appendChild(labelRow);
    el.appendChild(track);

    if (hostEl && hostEl.insertAdjacentElement) {
      hostEl.insertAdjacentElement('afterend', el);
    } else {
      document.body.appendChild(el);
    }

    var ctrl = {
      el: el,
      _reset: function () {
        el.style.display = '';
        el.style.opacity = '';
        el.classList.remove('is-error', 'is-done', 'is-indeterminate');
        fill.style.width = '0';
        label.textContent = '';
        pct.textContent = '';
        clearTimeout(this._hideTimer);
      },
      stage: function (text, p) {
        el.classList.remove('is-indeterminate', 'is-error', 'is-done');
        if (text != null) label.textContent = text;
        var clamped = Math.max(0, Math.min(1, p || 0));
        fill.style.width = (clamped * 100).toFixed(1) + '%';
        pct.textContent = Math.round(clamped * 100) + '%';
        el.setAttribute('aria-valuenow', String(Math.round(clamped * 100)));
      },
      indeterminate: function (text) {
        el.classList.remove('is-error', 'is-done');
        el.classList.add('is-indeterminate');
        if (text != null) label.textContent = text;
        pct.textContent = '';
        el.removeAttribute('aria-valuenow');
      },
      paint: function () {
        return new Promise(function (resolve) {
          requestAnimationFrame(function () { requestAnimationFrame(resolve); });
        });
      },
      read: function (source, text) {
        var self = this;
        var blob = (source instanceof Blob) ? source
          : (source && source.blob instanceof Blob) ? source.blob : null;
        if (blob && typeof FileReader !== 'undefined') {
          self.stage(text || 'Reading file', 0);
          return new Promise(function (resolve, reject) {
            var fr = new FileReader();
            fr.onprogress = function (e) {
              if (e.lengthComputable) self.stage(text || 'Reading file', e.loaded / e.total);
            };
            fr.onload = function () {
              self.stage(text || 'Reading file', 1);
              resolve(new Uint8Array(fr.result));
            };
            fr.onerror = function () { reject(fr.error); };
            fr.readAsArrayBuffer(blob);
          });
        }
        // No Blob handle (or no FileReader): can't report progress.
        self.indeterminate(text || 'Reading file');
        return source.arrayBuffer().then(function (ab) { return new Uint8Array(ab); });
      },
      done: function (text) {
        var self = this;
        el.classList.remove('is-indeterminate', 'is-error');
        el.classList.add('is-done');
        if (text != null) label.textContent = text;
        pct.textContent = '';
        this._hideTimer = setTimeout(function () {
          el.style.opacity = '0';
          self._hideTimer = setTimeout(function () { el.style.display = 'none'; }, 400);
        }, 500);
      },
      fail: function (text) {
        el.classList.remove('is-indeterminate', 'is-done');
        el.classList.add('is-error');
        if (text != null) label.textContent = text;
        pct.textContent = '';
      },
      hide: function () {
        clearTimeout(this._hideTimer);
        el.style.display = 'none';
      },
    };

    if (hostEl) hostEl._loadProgress = ctrl;
    ctrl._reset();
    return ctrl;
  }

  window.LoadProgress = { create: create };
})();
