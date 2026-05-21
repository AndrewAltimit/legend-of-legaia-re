/* rom-cache.js - shared IndexedDB-backed cache for the user-supplied
 * Legend of Legaia disc image (.bin Mode2/2352 or PROT.DAT).
 *
 * Several interactive pages (viewer, world-overview, monsters, audio) ask
 * the user to pick a disc image and then read it into the WASM build.
 * Re-picking the file on every navigation or reload is tedious, so the
 * picked Blob is stashed in IndexedDB - the only browser store that
 * comfortably holds a hundreds-of-MB disc - under a single shared key.
 * All pages read the same slot, so one pick serves the whole site and no
 * Sony bytes ever leave the machine (IndexedDB is same-origin local).
 *
 * Global API (`window.RomCache`):
 *   put(file)        -> Promise<void>                cache a picked File/Blob
 *   get()            -> Promise<{name,size,blob}|null>
 *   meta()           -> Promise<{name,size,savedAt}|null>   (no blob handle)
 *   clear()          -> Promise<void>
 *   resolve(input)   -> Promise<source|null>          picked file, else cache
 *   attach(input,opts) wire a file <input> to the cache (see below)
 *
 * A `source` is shaped `{ name, size, arrayBuffer() }` so existing
 * handlers that do `f.name / f.size / await f.arrayBuffer()` work
 * unchanged whether the bytes came from a fresh pick or the cache.
 *
 * attach(input, { onLoad, autoLoad }):
 *   - on `change`, caches the picked file then calls onLoad(file).
 *   - on init, if a disc is already cached, renders a small chip next to
 *     the input ("Auto-loaded cached disc: NAME (SIZE)  [Forget]") and,
 *     unless autoLoad === false, calls onLoad(cachedSource) so the page
 *     renders with zero clicks. With autoLoad:false the chip still shows
 *     (so a button-driven loader can pull the disc via resolve()).
 */
(function () {
  'use strict';

  var DB_NAME = 'legaia-rom-cache';
  var STORE = 'disc';
  var KEY = 'current';

  var hasIdb = typeof indexedDB !== 'undefined';

  function openDb() {
    return new Promise(function (resolve, reject) {
      var req = indexedDB.open(DB_NAME, 1);
      req.onupgradeneeded = function () {
        var db = req.result;
        if (!db.objectStoreNames.contains(STORE)) db.createObjectStore(STORE);
      };
      req.onsuccess = function () { resolve(req.result); };
      req.onerror = function () { reject(req.error); };
    });
  }

  /* Run `fn(store)` inside a transaction and resolve with the request's
   * result once the transaction commits. */
  function tx(db, mode, fn) {
    return new Promise(function (resolve, reject) {
      var t = db.transaction(STORE, mode);
      var req = fn(t.objectStore(STORE));
      var out;
      if (req) req.onsuccess = function () { out = req.result; };
      t.oncomplete = function () { resolve(out); };
      t.onerror = function () { reject(t.error); };
      t.onabort = function () { reject(t.error); };
    });
  }

  function put(file) {
    if (!hasIdb) return Promise.resolve();
    var rec = {
      name: file.name || 'disc.bin',
      size: file.size,
      type: file.type || '',
      savedAt: Date.now(),
      blob: file, // a File is a Blob; structured-cloneable into IndexedDB
    };
    return openDb().then(function (db) {
      return tx(db, 'readwrite', function (s) { return s.put(rec, KEY); })
        .finally(function () { db.close(); });
    });
  }

  function getRecord() {
    if (!hasIdb) return Promise.resolve(null);
    return openDb().then(function (db) {
      return tx(db, 'readonly', function (s) { return s.get(KEY); })
        .finally(function () { db.close(); });
    });
  }

  function get() {
    return getRecord().then(function (rec) {
      if (!rec || !rec.blob) return null;
      return { name: rec.name, size: rec.size, blob: rec.blob };
    });
  }

  function meta() {
    return getRecord().then(function (rec) {
      if (!rec) return null;
      return { name: rec.name, size: rec.size, savedAt: rec.savedAt };
    });
  }

  function clear() {
    if (!hasIdb) return Promise.resolve();
    return openDb().then(function (db) {
      return tx(db, 'readwrite', function (s) { return s.delete(KEY); })
        .finally(function () { db.close(); });
    });
  }

  /* Wrap a cache record's Blob as a File-shaped source. */
  function sourceFromRecord(rec) {
    return {
      name: rec.name,
      size: rec.size,
      blob: rec.blob,
      arrayBuffer: function () { return rec.blob.arrayBuffer(); },
    };
  }

  /* Best available disc for a button-driven loader: a freshly-picked
   * file wins; otherwise fall back to the cached disc. */
  function resolve(input) {
    if (input && input.files && input.files[0]) return Promise.resolve(input.files[0]);
    return get().then(function (rec) { return rec ? sourceFromRecord(rec) : null; });
  }

  // ---------------- UI chip ----------------

  function ensureStyle() {
    if (document.getElementById('rom-cache-style')) return;
    var s = document.createElement('style');
    s.id = 'rom-cache-style';
    s.textContent = [
      '.rom-cache-chip{display:inline-flex;align-items:center;gap:.5rem;margin:.5rem 0;',
      'padding:.35rem .65rem;border:1px solid var(--border,#334);border-radius:6px;',
      'background:rgba(110,200,150,.08);font-family:var(--font-mono,monospace);',
      'font-size:.8rem;color:var(--text-muted,#9aa);max-width:100%;}',
      '.rom-cache-chip .rom-cache-name{color:var(--text,#dde);font-weight:600;',
      'overflow:hidden;text-overflow:ellipsis;white-space:nowrap;max-width:18rem;}',
      '.rom-cache-chip .rom-cache-dot{flex:0 0 auto;width:.5rem;height:.5rem;',
      'border-radius:50%;background:#5b8;}',
      '.rom-cache-chip button{cursor:pointer;border:1px solid var(--border,#334);',
      'background:transparent;color:inherit;border-radius:4px;padding:.1rem .5rem;',
      'font:inherit;}',
      '.rom-cache-chip button:hover{background:rgba(200,90,90,.18);color:#fff;}',
    ].join('');
    document.head.appendChild(s);
  }

  function makeChip(input) {
    ensureStyle();
    var chip = document.createElement('div');
    chip.className = 'rom-cache-chip';
    chip.style.display = 'none';
    var anchor = (input.closest && input.closest('.file-input-group')) || input;
    anchor.insertAdjacentElement('afterend', chip);
    return chip;
  }

  function fmtSize(n) { return (n / 1024 / 1024).toFixed(1) + ' MB'; }

  function renderChip(chip, rec, auto, onForget) {
    chip.textContent = '';
    chip.style.display = 'inline-flex';

    var dot = document.createElement('span');
    dot.className = 'rom-cache-dot';
    chip.appendChild(dot);

    var label = document.createElement('span');
    label.appendChild(document.createTextNode(
      auto ? 'Auto-loaded cached disc: ' : 'Cached disc ready: '));
    var name = document.createElement('span');
    name.className = 'rom-cache-name';
    name.textContent = rec.name;
    label.appendChild(name);
    label.appendChild(document.createTextNode(' (' + fmtSize(rec.size) + ')'));
    chip.appendChild(label);

    var forget = document.createElement('button');
    forget.type = 'button';
    forget.textContent = 'Forget';
    forget.title = 'Remove the cached disc from this browser';
    forget.addEventListener('click', function () {
      clear().catch(function () {}).then(function () {
        chip.style.display = 'none';
        if (onForget) onForget();
      });
    });
    chip.appendChild(forget);
  }

  function attach(input, opts) {
    if (!input) return;
    opts = opts || {};
    var onLoad = opts.onLoad || function () {};
    var autoLoad = opts.autoLoad !== false;
    var chip = makeChip(input);

    input.addEventListener('change', function () {
      var f = input.files && input.files[0];
      if (!f) return;
      put(f).then(function () {
        renderChip(chip, { name: f.name, size: f.size }, false);
      }).catch(function (e) {
        console.warn('RomCache: could not cache disc -', e);
      });
      onLoad(f);
    });

    if (!hasIdb) return;
    get().then(function (rec) {
      if (!rec || !rec.blob) return;
      renderChip(chip, rec, autoLoad);
      if (autoLoad) {
        try { onLoad(sourceFromRecord(rec)); }
        catch (e) { console.warn('RomCache: auto-load handler threw -', e); }
      }
    }).catch(function (e) {
      console.warn('RomCache: cache read failed -', e);
    });
  }

  window.RomCache = {
    put: put,
    get: get,
    meta: meta,
    clear: clear,
    resolve: resolve,
    attach: attach,
  };
})();
