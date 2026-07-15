/* legaia-saves.js - shared localStorage persistence for engine sessions,
 * imported memory-card saves, and minigame progress. No WASM dependency:
 * pages that need the save APIs import the module themselves and hand this
 * layer nothing but bytes + metadata.
 *
 * localStorage schema:
 *
 *   legaia.saves.v1.index
 *     JSON array of session metadata, newest first, capped at 12 entries
 *     (older entries are evicted along with their slot keys):
 *       [{ id,               unique key, e.g. "lgsf-1720000000000"
 *          kind,             "lgsf" (engine session) | "card" (retail card)
 *          name,             display name
 *          scene,            CDNAME scene label the save was made in ("" ok)
 *          block,            card saves: the save block inside the container
 *          format,           card saves: "mcr" | "gme" | "mcs"
 *          party,            array of character names
 *          money,            gold
 *          coins,            casino coins (card saves)
 *          updated }, ...]   epoch ms
 *
 *   legaia.saves.v1.slot.<id>
 *     base64 of the raw save bytes - the .lgsf file, or the FULL card
 *     container exactly as imported (so a download rebuilds the original).
 *
 *   legaia.minigames.v1
 *     JSON object of per-game progress plus the pending-coins pool:
 *       { dance: { bestScore, plays, lastScore, updated },
 *         baka:  { bestBank, totalBanked, updated },
 *         slot:  { coinsWon, bestPayout, updated },
 *         coinsWon,    integer: casino coins won with NO save loaded, not
 *                      yet banked anywhere (absorbed when a save is loaded)
 *         coinSession }  { saveId, coins }: the save the minigames page has
 *                      loaded + the session's live coin balance (see the
 *                      coinSession/setCoinSession/updateCoinSessionCoins API;
 *                      the save bar on minigames.html drives it).
 *
 * Every write is wrapped in try/catch: on QuotaExceededError (or a disabled
 * localStorage) the write is dropped with a console.warn and the mutator
 * returns null rather than throwing at the caller.
 *
 * Global API (`window.LegaiaSaves`):
 *   listSessions()            -> [meta, ...] (newest first)
 *   getSession(id)            -> { meta, bytes: Uint8Array } | null
 *   putSession(meta, bytes)   -> id | null (id = meta.id or `${kind}-${now}`)
 *   deleteSession(id)
 *   getMinigames()            -> the legaia.minigames.v1 object
 *   putMinigame(game, patch)  -> shallow-merge `patch` into that game's
 *                                record + stamp `updated`
 *   addCoinsWon(n) / takeCoinsWon() / coinsWon()
 *   b64encode(u8) / b64decode(str)
 *   renderStrip(containerEl, opts) -> render the "Your games" strip;
 *     opts.onResume(entry)    resume handler (pages with a runtime), or
 *     opts.resumeHref(entry)  -> URL (pages without one; Resume is a link)
 *
 * A `legaia-saves-change` window event fires after every successful write,
 * so strips on the same page can re-render without polling.
 */
(function () {
  'use strict';

  var INDEX_KEY = 'legaia.saves.v1.index';
  var SLOT_PREFIX = 'legaia.saves.v1.slot.';
  var MINIGAMES_KEY = 'legaia.minigames.v1';
  var MAX_SESSIONS = 12;

  /* ---------------- storage plumbing ---------------- */

  function readJson(key, fallback) {
    try {
      var raw = window.localStorage.getItem(key);
      return raw ? JSON.parse(raw) : fallback;
    } catch (e) {
      console.warn('LegaiaSaves: could not read ' + key + ' -', e);
      return fallback;
    }
  }

  /* Returns true on success, null on failure (quota / disabled storage). */
  function writeRaw(key, value) {
    try {
      window.localStorage.setItem(key, value);
      return true;
    } catch (e) {
      console.warn('LegaiaSaves: could not write ' + key + ' -', e);
      return null;
    }
  }

  function removeRaw(key) {
    try { window.localStorage.removeItem(key); } catch (e) { /* ignore */ }
  }

  function changed() {
    try { window.dispatchEvent(new CustomEvent('legaia-saves-change')); }
    catch (e) { /* very old browser: strips just won't live-update */ }
  }

  /* ---------------- base64 <-> Uint8Array ----------------
   * Chunked so a 128 KB card container doesn't blow the argument-list
   * limit of String.fromCharCode.apply. */

  var B64_CHUNK = 0x8000;

  function b64encode(bytes) {
    var parts = [];
    for (var i = 0; i < bytes.length; i += B64_CHUNK) {
      parts.push(String.fromCharCode.apply(null, bytes.subarray(i, i + B64_CHUNK)));
    }
    return btoa(parts.join(''));
  }

  function b64decode(str) {
    var bin = atob(str);
    var out = new Uint8Array(bin.length);
    for (var i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
    return out;
  }

  /* ---------------- sessions ---------------- */

  function listSessions() {
    var idx = readJson(INDEX_KEY, []);
    return Array.isArray(idx) ? idx : [];
  }

  function getSession(id) {
    var meta = null;
    var idx = listSessions();
    for (var i = 0; i < idx.length; i++) {
      if (idx[i].id === id) { meta = idx[i]; break; }
    }
    if (!meta) return null;
    var raw;
    try { raw = window.localStorage.getItem(SLOT_PREFIX + id); }
    catch (e) { raw = null; }
    if (!raw) return null;
    try { return { meta: meta, bytes: b64decode(raw) }; }
    catch (e) {
      console.warn('LegaiaSaves: slot ' + id + ' is corrupt -', e);
      return null;
    }
  }

  function putSession(meta, bytes) {
    var kind = meta.kind === 'card' ? 'card' : 'lgsf';
    var id = meta.id || (kind + '-' + Date.now());
    var entry = {
      id: id,
      kind: kind,
      name: meta.name || id,
      scene: meta.scene || '',
      block: meta.block != null ? meta.block : null,
      format: meta.format || null,
      party: Array.isArray(meta.party) ? meta.party : [],
      level: meta.level != null ? meta.level : null,
      money: meta.money != null ? meta.money : null,
      coins: meta.coins != null ? meta.coins : null,
      location: meta.location || '',
      updated: Date.now(),
    };
    /* Bytes first: an index entry without a slot is worse than no entry. */
    if (writeRaw(SLOT_PREFIX + id, b64encode(bytes)) === null) return null;
    var idx = listSessions().filter(function (s) { return s.id !== id; });
    idx.unshift(entry);
    while (idx.length > MAX_SESSIONS) {
      var evicted = idx.pop();
      removeRaw(SLOT_PREFIX + evicted.id);
    }
    if (writeRaw(INDEX_KEY, JSON.stringify(idx)) === null) {
      removeRaw(SLOT_PREFIX + id);
      return null;
    }
    changed();
    return id;
  }

  function deleteSession(id) {
    var idx = listSessions().filter(function (s) { return s.id !== id; });
    removeRaw(SLOT_PREFIX + id);
    writeRaw(INDEX_KEY, JSON.stringify(idx));
    changed();
  }

  /* ---------------- minigame progress ---------------- */

  function getMinigames() {
    var mg = readJson(MINIGAMES_KEY, {});
    return (mg && typeof mg === 'object') ? mg : {};
  }

  function putMinigame(game, patch) {
    var mg = getMinigames();
    var rec = mg[game] || {};
    for (var k in patch) {
      if (Object.prototype.hasOwnProperty.call(patch, k)) rec[k] = patch[k];
    }
    rec.updated = Date.now();
    mg[game] = rec;
    if (writeRaw(MINIGAMES_KEY, JSON.stringify(mg)) === null) return null;
    changed();
    return rec;
  }

  /* ---------------- the loaded-save coin session ----------------
   *
   * The minigames page's save bar loads one stored save into the session:
   * its casino coin bank becomes the session's coin balance (the slot
   * machine racks from it, Baka payouts add to it), and "Export" writes the
   * live balance back into that save slot. Stored inside the minigames
   * object as `coinSession: { saveId, coins }`. Coin-only updates fire the
   * lighter `legaia-coin-session-change` event so per-spin balance syncs
   * don't force full strip/bar re-renders. */

  function coinSession() {
    var cs = getMinigames().coinSession;
    return (cs && typeof cs === 'object' && cs.saveId) ? cs : null;
  }

  function coinChanged() {
    try { window.dispatchEvent(new CustomEvent('legaia-coin-session-change')); }
    catch (e) { /* ignore */ }
  }

  function setCoinSession(saveId, coins) {
    var mg = getMinigames();
    if (saveId == null) delete mg.coinSession;
    else mg.coinSession = { saveId: saveId, coins: Math.max(0, Math.floor(coins) || 0) };
    if (writeRaw(MINIGAMES_KEY, JSON.stringify(mg)) === null) return null;
    changed();
    return mg.coinSession || null;
  }

  function updateCoinSessionCoins(coins) {
    var mg = getMinigames();
    var cs = mg.coinSession;
    if (!cs || !cs.saveId) return null;
    cs.coins = Math.max(0, Math.floor(coins) || 0);
    if (writeRaw(MINIGAMES_KEY, JSON.stringify(mg)) === null) return null;
    coinChanged();
    return cs;
  }

  /* Patch a stored session's index entry in place (no byte rewrite, no
   * reorder) - e.g. refresh `coins` after an export. */
  function patchSessionMeta(id, patch) {
    var idx = listSessions();
    for (var i = 0; i < idx.length; i++) {
      if (idx[i].id !== id) continue;
      for (var k in patch) {
        if (Object.prototype.hasOwnProperty.call(patch, k)) idx[i][k] = patch[k];
      }
      if (writeRaw(INDEX_KEY, JSON.stringify(idx)) === null) return null;
      changed();
      return idx[i];
    }
    return null;
  }

  function coinsWon() {
    var n = getMinigames().coinsWon;
    return (typeof n === 'number' && n > 0) ? Math.floor(n) : 0;
  }

  function addCoinsWon(n) {
    n = Math.floor(n);
    if (!(n > 0)) return coinsWon();
    var mg = getMinigames();
    mg.coinsWon = coinsWon() + n;
    if (writeRaw(MINIGAMES_KEY, JSON.stringify(mg)) === null) return null;
    changed();
    return mg.coinsWon;
  }

  function takeCoinsWon() {
    var n = coinsWon();
    if (n > 0) {
      var mg = getMinigames();
      mg.coinsWon = 0;
      if (writeRaw(MINIGAMES_KEY, JSON.stringify(mg)) === null) return 0;
      changed();
    }
    return n;
  }

  /* ---------------- the "Your games" strip ---------------- */

  function ensureStyle() {
    if (document.getElementById('lg-saves-css')) return;
    var s = document.createElement('style');
    s.id = 'lg-saves-css';
    s.textContent = [
      '.lg-saves{border:1px solid var(--border,#334);background:var(--bg-card,#12141c);',
      'padding:.55rem .75rem;display:flex;flex-direction:column;}',
      '.lg-saves-title{font-family:var(--font-mono,monospace);font-size:.7rem;',
      'text-transform:uppercase;letter-spacing:.06em;color:var(--text-dim,#778);',
      'padding-bottom:.35rem;}',
      '.lg-saves-row{display:flex;align-items:center;gap:.55rem;flex-wrap:wrap;',
      'padding:.32rem 0;border-top:1px solid var(--border,#334);',
      'font-family:var(--font-mono,monospace);font-size:.76rem;color:var(--text-muted,#9aa);}',
      '.lg-saves-kind{flex:none;font-size:.6rem;padding:.08rem .35rem;',
      'border:1px solid var(--border,#334);border-radius:3px;color:var(--accent,#6cf);',
      'text-transform:uppercase;letter-spacing:.04em;}',
      '.lg-saves-name{color:var(--text,#dde);font-weight:600;}',
      '.lg-saves-meta{color:var(--text-dim,#778);}',
      '.lg-saves-note{color:var(--accent,#6cf);}',
      '.lg-saves-spacer{flex:1;min-width:.5rem;}',
      '.lg-saves-btn{background:var(--bg-code,#0c0e12);color:var(--text,#dde);',
      'border:1px solid var(--border,#334);padding:.18rem .5rem;border-radius:3px;',
      'font-family:var(--font-mono,monospace);font-size:.7rem;cursor:pointer;',
      'text-decoration:none;display:inline-block;}',
      '.lg-saves-btn:hover{border-color:var(--accent,#6cf);}',
    ].join('');
    document.head.appendChild(s);
  }

  function el(tag, cls, text) {
    var e = document.createElement(tag);
    if (cls) e.className = cls;
    if (text != null) e.textContent = text;
    return e;
  }

  function relTime(ms) {
    var d = Date.now() - ms;
    if (!(d >= 0)) return '';
    if (d < 60000) return 'just now';
    if (d < 3600000) return Math.floor(d / 60000) + 'm ago';
    if (d < 86400000) return Math.floor(d / 3600000) + 'h ago';
    return Math.floor(d / 86400000) + 'd ago';
  }

  function downloadName(meta) {
    var base = (meta.name || meta.id).replace(/[^\w.-]+/g, '-');
    var ext = meta.kind === 'card' ? (meta.format || 'mcr') : 'lgsf';
    return base + '.' + ext;
  }

  function triggerDownload(bytes, filename) {
    var url = URL.createObjectURL(new Blob([bytes], { type: 'application/octet-stream' }));
    var a = document.createElement('a');
    a.href = url;
    a.download = filename;
    a.click();
    setTimeout(function () { URL.revokeObjectURL(url); }, 5000);
  }

  function sessionRow(meta, opts, containerEl) {
    var row = el('div', 'lg-saves-row');
    row.appendChild(el('span', 'lg-saves-kind',
      meta.kind === 'card' ? (meta.format || 'card') : 'lgsf'));
    row.appendChild(el('span', 'lg-saves-name', meta.name || meta.id));

    var bits = [];
    if (meta.scene) bits.push(meta.scene);
    if (meta.party && meta.party.length) bits.push(meta.party.slice(0, 3).join(', '));
    if (meta.money != null) bits.push(meta.money + ' G');
    if (meta.coins != null) bits.push(meta.coins + ' coins');
    if (meta.updated) bits.push(relTime(meta.updated));
    row.appendChild(el('span', 'lg-saves-meta', bits.join(' - ')));

    row.appendChild(el('span', 'lg-saves-spacer'));

    if (typeof opts.onResume === 'function') {
      var resume = el('button', 'lg-saves-btn', 'Resume');
      resume.type = 'button';
      resume.addEventListener('click', function () { opts.onResume(meta); });
      row.appendChild(resume);
    } else if (typeof opts.resumeHref === 'function') {
      var link = el('a', 'lg-saves-btn', 'Resume');
      link.href = opts.resumeHref(meta);
      row.appendChild(link);
    }

    var dl = el('button', 'lg-saves-btn', 'Download');
    dl.type = 'button';
    dl.addEventListener('click', function () {
      var stored = getSession(meta.id);
      if (!stored) {
        console.warn('LegaiaSaves: bytes for ' + meta.id + ' are gone');
        dl.textContent = 'missing';
        return;
      }
      triggerDownload(stored.bytes, downloadName(meta));
    });
    row.appendChild(dl);

    var del = el('button', 'lg-saves-btn', 'Delete');
    del.type = 'button';
    del.addEventListener('click', function () {
      deleteSession(meta.id);
      renderStrip(containerEl, opts);
    });
    row.appendChild(del);

    return row;
  }

  function minigameRow(mg) {
    var row = el('div', 'lg-saves-row');
    row.appendChild(el('span', 'lg-saves-kind', 'mini'));
    var bits = [];
    if (mg.dance && mg.dance.bestScore != null) {
      bits.push('dance best ' + mg.dance.bestScore +
        (mg.dance.plays ? ' (' + mg.dance.plays + ' plays)' : ''));
    }
    if (mg.baka && mg.baka.bestBank != null) {
      bits.push('Baka best bank ' + mg.baka.bestBank + ' G');
    }
    if (mg.slot && mg.slot.coinsWon != null) {
      bits.push('slot won ' + mg.slot.coinsWon +
        (mg.slot.bestPayout ? ' (best ' + mg.slot.bestPayout + ')' : ''));
    }
    row.appendChild(el('span', 'lg-saves-meta',
      bits.length ? 'Minigames: ' + bits.join(' - ') : 'Minigames'));
    var pending = coinsWon();
    if (pending > 0) {
      row.appendChild(el('span', 'lg-saves-note',
        pending + ' coins won - export to your save on the minigames page'));
    }
    return row;
  }

  function renderStrip(containerEl, opts) {
    if (!containerEl) return;
    opts = opts || {};
    ensureStyle();
    containerEl.textContent = '';
    var sessions = listSessions();
    var mg = getMinigames();
    var hasMg = !!(mg.dance || mg.baka || mg.slot || coinsWon() > 0);
    if (!sessions.length && !hasMg) {
      containerEl.style.display = 'none';
      return;
    }
    containerEl.style.display = '';
    var box = el('div', 'lg-saves');
    box.appendChild(el('div', 'lg-saves-title', 'Your games'));
    for (var i = 0; i < sessions.length; i++) {
      box.appendChild(sessionRow(sessions[i], opts, containerEl));
    }
    if (hasMg) box.appendChild(minigameRow(mg));
    containerEl.appendChild(box);
  }

  /* ================= shared rich save bar =================
   *
   * The polished, always-visible memory-card bar used by BOTH the play page
   * and the minigames page. Each stored save is a retail-style tile: the SC
   * block's own baked lead-face icon (or the lead's disc portrait for engine
   * sessions), the party's faces stacked beside it, and identifying stats off
   * the save block (lead + displayed level, gold, location, and - on the
   * minigames page - casino coins). The two pages differ only in the primary
   * per-tile action (Resume into the engine vs. Load the coin bank) and in the
   * session line; everything visual lives here so the two bars stay identical.
   *
   * Portrait pixels are cached once per browser (PORTRAIT_KEY) so faces survive
   * visits that never touch a disc. Each page harvests the three 16x16
   * load-screen portrait TIMs off its own WASM surface (the play page's
   * `disc_portrait_rgba`, the minigames page's `LegaiaMinigames.save_portrait_
   * rgba`) and hands the raw RGBA to `cachePortraits`. */

  var PORTRAIT_KEY = 'legaia.portraits.v1';
  var PORTRAIT_NAMES = { vahn: 0, noa: 1, gala: 2 };
  var RICH_CHIP_COLORS = ['#7798d4', '#df74a6', '#2dcca7', '#d8b34b'];
  var portraitCanvasCache = null;   /* [canvas x3] | null (in-memory) */

  function rgbaCanvas(bytes, w, h) {
    if (!bytes || bytes.length !== w * h * 4) return null;
    var c = document.createElement('canvas');
    c.width = w; c.height = h;
    c.getContext('2d').putImageData(
      new ImageData(new Uint8ClampedArray(bytes), w, h), 0, 0);
    return c;
  }

  /* The three party portraits as 16x16 canvases, from the in-memory cache or
   * localStorage; null until a disc has been harvested at least once. */
  function portraitCanvases() {
    if (portraitCanvasCache) return portraitCanvasCache;
    var rec = readJson(PORTRAIT_KEY, null);
    if (!rec || rec.v !== 1 || !Array.isArray(rec.p) || rec.p.length !== 3) return null;
    try {
      var out = rec.p.map(function (b64) { return rgbaCanvas(b64decode(b64), 16, 16); });
      if (out.every(Boolean)) { portraitCanvasCache = out; return out; }
    } catch (e) { /* fall through */ }
    return null;
  }

  /* Persist harvested portrait pixels. `rgbaList` = three 1024-byte RGBA8
   * buffers (Vahn/Noa/Gala). Returns the canvases, or null if any is missing. */
  function cachePortraits(rgbaList) {
    if (!Array.isArray(rgbaList) || rgbaList.length !== 3) return null;
    var faces = [], b64 = [];
    for (var i = 0; i < 3; i++) {
      var c = rgbaCanvas(rgbaList[i], 16, 16);
      if (!c) return null;
      faces.push(c);
      b64.push(b64encode(rgbaList[i]));
    }
    portraitCanvasCache = faces;
    writeRaw(PORTRAIT_KEY, JSON.stringify({ v: 1, p: b64 }));
    return faces;
  }

  function portraitForName(name) {
    var faces = portraitCanvases();
    if (!faces || !name) return null;
    var idx = PORTRAIT_NAMES[String(name).toLowerCase()];
    return idx != null ? faces[idx] : null;
  }

  function ensureRichStyle() {
    if (document.getElementById('lg-richbar-css')) return;
    var s = document.createElement('style');
    s.id = 'lg-richbar-css';
    s.textContent = [
      '.lgrb{border:1px solid var(--border,#334);background:var(--bg-card,#12141c);',
      'border-radius:4px;padding:.5rem .65rem;font-family:var(--font-mono,monospace);}',
      '.lgrb-head{display:flex;align-items:center;gap:.6rem;flex-wrap:wrap;',
      'font-size:.68rem;color:var(--text-dim,#778);text-transform:uppercase;',
      'letter-spacing:.06em;padding-bottom:.4rem;}',
      '.lgrb-title{color:var(--text,#dde);font-weight:700;}',
      '.lgrb-session{color:var(--accent,#6cf);text-transform:none;letter-spacing:0;}',
      '.lgrb-status{text-transform:none;letter-spacing:0;color:var(--text-muted,#9aa);}',
      '.lgrb-status.is-good{color:#6ec896;}',
      '.lgrb-status.is-bad{color:#d84b4b;}',
      '.lgrb-spacer{flex:1;min-width:.5rem;}',
      '.lgrb-row{display:flex;gap:.5rem;overflow-x:auto;padding-bottom:.15rem;',
      'align-items:stretch;}',
      '.lgrb-tile{flex:0 0 auto;display:flex;align-items:center;gap:.5rem;',
      'border:1px solid var(--border,#334);border-radius:4px;',
      'background:var(--bg-code,#0c0e12);padding:.4rem .55rem;max-width:22rem;}',
      '.lgrb-tile.is-loaded{border-color:var(--accent,#6cf);',
      'box-shadow:inset 0 0 0 1px var(--accent,#6cf);}',
      '.lgrb-icon{flex:0 0 auto;display:flex;align-items:center;gap:2px;',
      'background:#1a1d26;border:1px solid var(--border,#334);border-radius:3px;',
      'padding:3px;}',
      '.lgrb-icon canvas{width:34px;height:34px;display:block;',
      'image-rendering:pixelated;image-rendering:crisp-edges;}',
      '.lgrb-faces{display:flex;flex-direction:column;gap:2px;}',
      '.lgrb-faces canvas{width:15px;height:15px;display:block;',
      'image-rendering:pixelated;image-rendering:crisp-edges;}',
      '.lgrb-chip{width:15px;height:15px;border-radius:2px;display:flex;',
      'align-items:center;justify-content:center;font-size:9px;font-weight:700;',
      'color:#0c0e12;}',
      '.lgrb-info{font-size:.72rem;line-height:1.45;color:var(--text-muted,#9aa);',
      'min-width:0;}',
      '.lgrb-name{color:var(--text,#dde);font-weight:600;white-space:nowrap;',
      'overflow:hidden;text-overflow:ellipsis;max-width:12rem;}',
      '.lgrb-stats{white-space:nowrap;}',
      '.lgrb-stats b{color:var(--text,#dde);font-weight:600;}',
      '.lgrb-loc{color:var(--text-dim,#778);white-space:nowrap;overflow:hidden;',
      'text-overflow:ellipsis;max-width:12rem;}',
      '.lgrb-when{color:var(--text-dim,#778);font-size:.66rem;}',
      '.lgrb-btns{display:flex;flex-direction:column;gap:.2rem;flex:0 0 auto;}',
      '.lgrb-btnrow{display:flex;gap:.2rem;}',
      '.lgrb-btn{background:var(--bg-card,#12141c);color:var(--text,#dde);',
      'border:1px solid var(--border,#334);border-radius:3px;cursor:pointer;',
      'font-family:var(--font-mono,monospace);font-size:.66rem;',
      'padding:.16rem .45rem;text-decoration:none;display:inline-block;text-align:center;}',
      '.lgrb-btn:hover:not([disabled]){border-color:var(--accent,#6cf);}',
      '.lgrb-btn[disabled]{opacity:.4;cursor:not-allowed;}',
      '.lgrb-btn.lgrb-primary{background:var(--accent,#6cf);',
      'border-color:var(--accent,#6cf);color:var(--bg,#0c0e12);font-weight:700;}',
      '.lgrb-empty{display:flex;align-items:center;gap:.75rem;flex-wrap:wrap;',
      'font-size:.76rem;color:var(--text-muted,#9aa);line-height:1.5;',
      'border:1px dashed var(--border,#334);border-radius:4px;padding:.55rem .7rem;}',
      '.lgrb-empty .lgrb-empty-text{flex:1;min-width:12rem;}',
      '.lgrb-empty b{color:var(--text,#dde);}',
    ].join('');
    document.head.appendChild(s);
  }

  function initialChip(name, i) {
    var chip = el('span', 'lgrb-chip', (name || '?').charAt(0).toUpperCase());
    chip.style.background = RICH_CHIP_COLORS[i % RICH_CHIP_COLORS.length];
    chip.title = name || '';
    return chip;
  }

  function scaledFace(src, name) {
    var c = document.createElement('canvas');
    c.width = 16; c.height = 16;
    var g = c.getContext('2d');
    g.imageSmoothingEnabled = false;
    g.drawImage(src, 0, 0);
    if (name) c.title = name;
    return c;
  }

  /* createRichBar(containerEl, opts) -> { render, renderSessionLine, setStatus,
   *   promptAdd }.
   *
   * opts:
   *   heading        bar title (default "Saves")
   *   wasm()         -> Promise<module> with card_icon_rgba + save_summary_json
   *                    (used for the baked icon + one-shot meta enrichment)
   *   harvest()      called once per render; the page decodes portraits off its
   *                  own disc/WASM and calls LegaiaSaves.cachePortraits. Return
   *                  true if faces became newly available (triggers a re-render).
   *   sessionLine()  -> string | null shown beside the heading (coin session)
   *   showCoins      include a coin stat in the tile (minigames)
   *   emptyText      HTML for the empty state's prose
   *   acceptExts     file-picker accept list
   *   onAdd(file)    -> Promise, import a picked save file; the bar re-renders
   *   primaryButton(meta, ctx)  -> { text, cls, title, disabled, onClick } | null
   *   loadedId()     -> id of the currently-loaded tile (highlight), or null
   */
  function createRichBar(containerEl, opts) {
    opts = opts || {};
    var statusMsg = '', statusCls = '';
    var iconCache = {};   /* save id -> canvas */
    var enriched = {};    /* save id -> true (summary re-derived once) */

    function wasm() {
      return (typeof opts.wasm === 'function') ? opts.wasm() : Promise.reject('no wasm');
    }

    function setStatus(msg, cls) {
      statusMsg = msg || '';
      statusCls = cls || '';
      var s = containerEl && containerEl.querySelector('.lgrb-status');
      if (s) {
        s.textContent = statusMsg;
        s.className = 'lgrb-status' + (statusCls ? ' is-' + statusCls : '');
      }
    }

    /* The main 34px icon: the SC block's own baked memory-card icon for card
     * saves; the lead's disc portrait for engine sessions; an initial chip at
     * icon size as the last resort. */
    function iconCanvasFor(meta, done) {
      if (iconCache[meta.id]) { done(iconCache[meta.id]); return; }
      var lead = (meta.party && meta.party[0]) || meta.name || '?';
      var fallback = function () {
        var c = document.createElement('canvas');
        c.width = 16; c.height = 16;
        var g = c.getContext('2d');
        var pf = portraitForName(lead);
        if (pf) { g.drawImage(pf, 0, 0); }
        else {
          g.fillStyle = RICH_CHIP_COLORS[0];
          g.fillRect(0, 0, 16, 16);
          g.fillStyle = '#0c0e12';
          g.font = 'bold 11px monospace';
          g.textAlign = 'center';
          g.textBaseline = 'middle';
          g.fillText(String(lead).charAt(0).toUpperCase(), 8, 9);
        }
        done(c);
      };
      if (meta.kind !== 'card' || meta.block == null) { fallback(); return; }
      var stored = getSession(meta.id);
      if (!stored) { fallback(); return; }
      wasm().then(function (mod) {
        var c = rgbaCanvas(mod.card_icon_rgba(stored.bytes, meta.block), 16, 16);
        if (!c) { fallback(); return; }
        iconCache[meta.id] = c;
        done(c);
      }).catch(fallback);
    }

    /* Re-derive level / location / party for sessions stored before those
     * fields existed. One shot per save per page load. */
    function enrichMeta(meta) {
      if (enriched[meta.id] || meta.level != null) return;
      enriched[meta.id] = true;
      var stored = getSession(meta.id);
      if (!stored) return;
      wasm().then(function (mod) {
        var sum = JSON.parse(mod.save_summary_json(stored.bytes));
        var s = sum.kind === 'lgsf'
          ? sum
          : (sum.saves || []).filter(function (x) { return x.block === meta.block; })[0];
        if (!s) return;
        patchSessionMeta(meta.id, {
          level: s.level != null ? s.level : null,
          location: s.location || meta.location || '',
          party: (s.party && s.party.length) ? s.party : meta.party,
          money: s.money != null ? s.money : meta.money,
          coins: s.coins != null ? s.coins : meta.coins,
        });
      }).catch(function () { /* keep whatever meta already has */ });
    }

    function tileFor(meta, loadedId) {
      var tile = el('div', 'lgrb-tile' + (meta.id === loadedId ? ' is-loaded' : ''));
      tile.dataset.saveId = meta.id;

      var iconBox = el('span', 'lgrb-icon');
      var main = document.createElement('canvas');
      main.width = 16; main.height = 16;
      iconBox.appendChild(main);
      iconCanvasFor(meta, function (c) {
        var g = main.getContext('2d');
        g.clearRect(0, 0, 16, 16);
        g.drawImage(c, 0, 0);
      });
      var facesBox = el('span', 'lgrb-faces');
      (meta.party || []).slice(0, 3).forEach(function (name, i) {
        var pf = portraitForName(name);
        facesBox.appendChild(pf ? scaledFace(pf, name) : initialChip(name, i));
      });
      iconBox.appendChild(facesBox);
      tile.appendChild(iconBox);

      var info = el('div', 'lgrb-info');
      var lead = (meta.party && meta.party[0]) || '';
      info.appendChild(el('div', 'lgrb-name',
        (meta.name || meta.id) + (meta.kind === 'card' ? '' : ' (engine)')));
      var stats = el('div', 'lgrb-stats');
      var bits = [];
      if (lead) bits.push(lead + (meta.level != null ? ' Lv <b>' + meta.level + '</b>' : ''));
      if (meta.money != null) bits.push('<b>' + meta.money + '</b> G');
      if (opts.showCoins && meta.coins != null) bits.push('<b>' + meta.coins + '</b> coins');
      stats.innerHTML = bits.join(' · ') || '&mdash;';
      info.appendChild(stats);
      var loc = meta.location || meta.scene || '';
      if (loc) info.appendChild(el('div', 'lgrb-loc', loc));
      if (meta.updated) info.appendChild(el('div', 'lgrb-when', relTime(meta.updated)));
      tile.appendChild(info);

      var btns = el('div', 'lgrb-btns');
      var primary = typeof opts.primaryButton === 'function'
        ? opts.primaryButton(meta, { loadedId: loadedId }) : null;
      if (primary) {
        var pb = el('button', 'lgrb-btn ' + (primary.cls || 'lgrb-primary'), primary.text);
        pb.type = 'button';
        if (primary.title) pb.title = primary.title;
        if (primary.disabled) pb.disabled = true;
        else if (typeof primary.onClick === 'function') {
          pb.addEventListener('click', function () { primary.onClick(meta); });
        }
        btns.appendChild(pb);
      }
      var row2 = el('div', 'lgrb-btnrow');
      var dl = el('button', 'lgrb-btn', '↓');
      dl.type = 'button';
      dl.title = 'Download this save as a file';
      dl.addEventListener('click', function () {
        var stored = getSession(meta.id);
        if (!stored) { setStatus('The bytes for that save are gone from this browser.', 'bad'); return; }
        triggerDownload(stored.bytes, downloadName(meta));
      });
      row2.appendChild(dl);
      var del = el('button', 'lgrb-btn', '×');
      del.type = 'button';
      del.title = 'Remove this save from the browser';
      del.addEventListener('click', function () {
        if (typeof opts.onDelete === 'function') opts.onDelete(meta);
        deleteSession(meta.id);
        delete iconCache[meta.id];
        setStatus('Deleted "' + (meta.name || meta.id) + '".');
        render();
      });
      row2.appendChild(del);
      btns.appendChild(row2);
      tile.appendChild(btns);

      enrichMeta(meta);
      return tile;
    }

    function addPicker(head) {
      if (typeof opts.onAdd !== 'function') return;
      var add = el('button', 'lgrb-btn lgrb-primary', '+ Add save file');
      add.type = 'button';
      add.title = 'Import a memory-card save (.mcr / .mcd / .gme / .mcs) or an engine .lgsf';
      var picker = document.createElement('input');
      picker.type = 'file';
      picker.accept = opts.acceptExts || '.mcr,.mcd,.gme,.mcs,.lgsf';
      picker.style.display = 'none';
      picker.addEventListener('change', function () {
        if (picker.files && picker.files[0]) {
          Promise.resolve(opts.onAdd(picker.files[0])).then(function () {
            picker.value = ''; render();
          });
        }
      });
      add.addEventListener('click', function () { picker.click(); });
      head.appendChild(add);
      head.appendChild(picker);
    }

    function render() {
      if (!containerEl) return;
      ensureRichStyle();
      if (typeof opts.harvest === 'function') { try { opts.harvest(); } catch (e) { /* ignore */ } }

      containerEl.textContent = '';
      containerEl.style.display = '';
      var box = el('div', 'lgrb');

      var head = el('div', 'lgrb-head');
      head.appendChild(el('span', 'lgrb-title', opts.heading || 'Saves'));
      if (typeof opts.sessionLine === 'function') {
        var sl = opts.sessionLine();
        head.appendChild(el('span', 'lgrb-session', sl || ''));
      }
      head.appendChild(el('span', 'lgrb-status' + (statusCls ? ' is-' + statusCls : ''), statusMsg));
      head.appendChild(el('span', 'lgrb-spacer'));
      addPicker(head);
      box.appendChild(head);

      var sessions = listSessions();
      var loadedId = typeof opts.loadedId === 'function' ? opts.loadedId() : null;
      if (!sessions.length) {
        var empty = el('div', 'lgrb-empty');
        empty.appendChild(el('div', 'lgrb-empty-text', null)).innerHTML =
          opts.emptyText || 'No saves in this browser yet. Nothing is uploaded - saves stay here.';
        box.appendChild(empty);
      } else {
        var row = el('div', 'lgrb-row');
        sessions.forEach(function (meta) { row.appendChild(tileFor(meta, loadedId)); });
        box.appendChild(row);
      }
      containerEl.appendChild(box);
    }

    function renderSessionLine() {
      if (typeof opts.sessionLine !== 'function') return;
      var s = containerEl && containerEl.querySelector('.lgrb-session');
      if (s) s.textContent = opts.sessionLine() || '';
    }

    return {
      render: render,
      renderSessionLine: renderSessionLine,
      setStatus: setStatus,
      invalidateIcon: function (id) { delete iconCache[id]; },
    };
  }

  window.LegaiaSaves = {
    listSessions: listSessions,
    getSession: getSession,
    putSession: putSession,
    deleteSession: deleteSession,
    patchSessionMeta: patchSessionMeta,
    getMinigames: getMinigames,
    putMinigame: putMinigame,
    addCoinsWon: addCoinsWon,
    takeCoinsWon: takeCoinsWon,
    coinsWon: coinsWon,
    coinSession: coinSession,
    setCoinSession: setCoinSession,
    updateCoinSessionCoins: updateCoinSessionCoins,
    b64encode: b64encode,
    b64decode: b64decode,
    renderStrip: renderStrip,
    /* shared rich bar + portrait cache (play + minigames) */
    createRichBar: createRichBar,
    portraitCanvases: portraitCanvases,
    cachePortraits: cachePortraits,
    portraitForName: portraitForName,
  };
})();
