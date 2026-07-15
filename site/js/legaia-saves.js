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
  };
})();
