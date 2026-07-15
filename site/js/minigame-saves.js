/* minigame-saves.js - the ALWAYS-VISIBLE slim save bar on minigames.html.
 *
 * One horizontal bar of the saves stored in this browser (localStorage via
 * js/legaia-saves.js - permanent across visits). Each save renders as a
 * retail-style memory-card tile:
 *
 *   - the 16x16 icon baked into the SC block itself (`card_icon_rgba` in the
 *     WASM save tools) - for Legaia saves that IS the lead character's face,
 *     because the retail save writer copies the load-screen portrait TIM
 *     into every save block;
 *   - the party's faces beside it, decoded from the visitor's own disc: the
 *     three 16x16 load-screen portrait TIMs (Vahn / Noa / Gala) pulled via
 *     `LegaiaMinigames.save_portrait_rgba` and cached as pixels in
 *     localStorage, so faces survive visits that never load a disc. A party
 *     member without an on-disc portrait (Terra) gets an initial chip - no
 *     stand-in art;
 *   - identifying stats off the save block: lead + displayed level
 *     (record +0x130), gold, casino coins, location name.
 *
 * "Load" makes a save the session's wallet: its casino coin bank (RAM
 * 0x800845A4 in the SC block) becomes the session coin balance the slot
 * machine racks from and Baka payouts add to (plus any coins already won
 * this browser before a save was loaded). The loaded tile is highlighted.
 * "Export" writes the live session balance back into that save slot's bytes
 * in localStorage; the download button beside it is the secondary
 * affordance that hands the container back as a file.
 *
 * The bar is presentation + storage only: byte-level work (summaries, icon
 * decode, the 4-byte coin patch) is the WASM save tools
 * (crates/web-viewer/src/session_save.rs), loaded lazily.
 */
(function () {
  'use strict';

  var PORTRAIT_KEY = 'legaia.portraits.v1';
  var PORTRAIT_NAMES = { vahn: 0, noa: 1, gala: 2 };
  var COIN_CAP = 9999999;

  var containerEl = null;
  var wasmPromise = null;
  var portraits = null;        /* [canvas x3] | null */
  var iconCache = {};          /* save id -> canvas | 'none' */
  var enriched = {};           /* save id -> true (summary re-derived once) */
  var statusMsg = '';
  var statusCls = '';

  /* ---------------- plumbing ---------------- */

  function wasm() {
    if (!wasmPromise) {
      /* Resolve against the DOCUMENT (dynamic import inside a classic script
       * resolves against this script's own js/ directory otherwise). */
      var url = new URL('wasm/legaia_web_viewer.js', document.baseURI).href;
      wasmPromise = import(url).then(function (mod) {
        return mod.default().then(function () { return mod; });
      });
      wasmPromise.catch(function (e) {
        console.warn('MgSaveBar: WASM save tools unavailable -', e);
      });
    }
    return wasmPromise;
  }

  function readJson(key) {
    try {
      var raw = window.localStorage.getItem(key);
      return raw ? JSON.parse(raw) : null;
    } catch (e) { return null; }
  }

  function writeJson(key, value) {
    try { window.localStorage.setItem(key, JSON.stringify(value)); }
    catch (e) { console.warn('MgSaveBar: could not write ' + key + ' -', e); }
  }

  function rgbaCanvas(bytes, w, h) {
    if (!bytes || bytes.length !== w * h * 4) return null;
    var c = document.createElement('canvas');
    c.width = w; c.height = h;
    c.getContext('2d').putImageData(
      new ImageData(new Uint8ClampedArray(bytes), w, h), 0, 0);
    return c;
  }

  /* ---------------- disc portraits (cached pixels) ---------------- */

  function portraitsFromCache() {
    var rec = readJson(PORTRAIT_KEY);
    if (!rec || rec.v !== 1 || !Array.isArray(rec.p) || rec.p.length !== 3) return null;
    try {
      var out = rec.p.map(function (b64) {
        return rgbaCanvas(LegaiaSaves.b64decode(b64), 16, 16);
      });
      return out.every(Boolean) ? out : null;
    } catch (e) { return null; }
  }

  /* Pull the three load-screen portrait TIMs off the loaded disc (the
   * minigames page's LegaiaMinigames instance) and cache the pixels. */
  function harvestPortraits() {
    if (portraits) return true;
    var api = window.__mgApi;
    if (!api || typeof api.save_portrait_rgba !== 'function') return false;
    var faces = [], b64 = [];
    for (var i = 0; i < 3; i++) {
      var rgba = api.save_portrait_rgba(i);
      var c = rgbaCanvas(rgba, 16, 16);
      if (!c) return false;
      faces.push(c);
      b64.push(LegaiaSaves.b64encode(rgba));
    }
    portraits = faces;
    writeJson(PORTRAIT_KEY, { v: 1, p: b64 });
    return true;
  }

  function portraitFor(name) {
    if (!portraits || !name) return null;
    var idx = PORTRAIT_NAMES[String(name).toLowerCase()];
    return idx != null ? portraits[idx] : null;
  }

  /* ---------------- style ---------------- */

  function ensureStyle() {
    if (document.getElementById('mg-save-bar-css')) return;
    var s = document.createElement('style');
    s.id = 'mg-save-bar-css';
    s.textContent = [
      '.mgsb{border:1px solid var(--border,#334);background:var(--bg-card,#12141c);',
      'padding:.45rem .6rem;font-family:var(--font-mono,monospace);}',
      '.mgsb-head{display:flex;align-items:center;gap:.7rem;flex-wrap:wrap;',
      'font-size:.68rem;color:var(--text-dim,#778);text-transform:uppercase;',
      'letter-spacing:.06em;padding-bottom:.35rem;}',
      '.mgsb-session{color:var(--accent,#6cf);text-transform:none;letter-spacing:0;}',
      '.mgsb-status{text-transform:none;letter-spacing:0;color:var(--text-muted,#9aa);}',
      '.mgsb-status.is-good{color:#6ec896;}',
      '.mgsb-status.is-bad{color:#d84b4b;}',
      '.mgsb-spacer{flex:1;min-width:.5rem;}',
      '.mgsb-row{display:flex;gap:.5rem;overflow-x:auto;padding-bottom:.15rem;',
      'align-items:stretch;}',
      '.mgsb-tile{flex:0 0 auto;display:flex;align-items:center;gap:.5rem;',
      'border:1px solid var(--border,#334);border-radius:4px;',
      'background:var(--bg-code,#0c0e12);padding:.35rem .5rem;max-width:21rem;}',
      '.mgsb-tile.is-loaded{border-color:var(--accent,#6cf);',
      'box-shadow:inset 0 0 0 1px var(--accent,#6cf);}',
      '.mgsb-icon{flex:0 0 auto;display:flex;align-items:center;gap:2px;',
      'background:#1a1d26;border:1px solid var(--border,#334);border-radius:3px;',
      'padding:3px;}',
      '.mgsb-icon canvas{width:32px;height:32px;display:block;',
      'image-rendering:pixelated;image-rendering:crisp-edges;}',
      '.mgsb-faces{display:flex;flex-direction:column;gap:2px;}',
      '.mgsb-faces canvas{width:14px;height:14px;display:block;',
      'image-rendering:pixelated;image-rendering:crisp-edges;}',
      '.mgsb-chip{width:14px;height:14px;border-radius:2px;display:flex;',
      'align-items:center;justify-content:center;font-size:9px;font-weight:700;',
      'color:#0c0e12;}',
      '.mgsb-info{font-size:.72rem;line-height:1.45;color:var(--text-muted,#9aa);',
      'min-width:0;}',
      '.mgsb-name{color:var(--text,#dde);font-weight:600;white-space:nowrap;',
      'overflow:hidden;text-overflow:ellipsis;max-width:11.5rem;}',
      '.mgsb-stats{white-space:nowrap;}',
      '.mgsb-stats b{color:var(--text,#dde);font-weight:600;}',
      '.mgsb-loc{color:var(--text-dim,#778);white-space:nowrap;overflow:hidden;',
      'text-overflow:ellipsis;max-width:11.5rem;}',
      '.mgsb-btns{display:flex;flex-direction:column;gap:.2rem;flex:0 0 auto;}',
      '.mgsb-btnrow{display:flex;gap:.2rem;}',
      '.mgsb-btn{background:var(--bg-card,#12141c);color:var(--text,#dde);',
      'border:1px solid var(--border,#334);border-radius:3px;cursor:pointer;',
      'font-family:var(--font-mono,monospace);font-size:.66rem;',
      'padding:.14rem .4rem;}',
      '.mgsb-btn:hover:not([disabled]){border-color:var(--accent,#6cf);}',
      '.mgsb-btn[disabled]{opacity:.4;cursor:not-allowed;}',
      '.mgsb-btn.mgsb-primary{background:var(--accent,#6cf);',
      'border-color:var(--accent,#6cf);color:var(--bg,#0c0e12);font-weight:700;}',
      '.mgsb-empty{font-size:.74rem;color:var(--text-dim,#778);',
      'padding:.25rem 0;}',
    ].join('');
    document.head.appendChild(s);
  }

  function el(tag, cls, text) {
    var e = document.createElement(tag);
    if (cls) e.className = cls;
    if (text != null) e.textContent = text;
    return e;
  }

  /* ---------------- status line ---------------- */

  function setStatus(msg, cls) {
    statusMsg = msg || '';
    statusCls = cls || '';
    var s = containerEl && containerEl.querySelector('.mgsb-status');
    if (s) {
      s.textContent = statusMsg;
      s.className = 'mgsb-status' + (statusCls ? ' is-' + statusCls : '');
    }
  }

  /* ---------------- tile pieces ---------------- */

  var CHIP_COLORS = ['#7798d4', '#df74a6', '#2dcca7', '#d8b34b'];

  function initialChip(name, i) {
    var chip = el('span', 'mgsb-chip', (name || '?').charAt(0).toUpperCase());
    chip.style.background = CHIP_COLORS[i % CHIP_COLORS.length];
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

  /* The main 32px icon: the save block's own baked memory-card icon for
   * card saves; the lead's disc portrait for LGSF sessions. Falls back to
   * an initial chip drawn at icon size. */
  function iconCanvasFor(meta, done) {
    if (iconCache[meta.id] && iconCache[meta.id] !== 'none') {
      done(iconCache[meta.id]);
      return;
    }
    var lead = (meta.party && meta.party[0]) || meta.name || '?';
    var fallback = function () {
      var c = document.createElement('canvas');
      c.width = 16; c.height = 16;
      var g = c.getContext('2d');
      var pf = portraitFor(lead);
      if (pf) g.drawImage(pf, 0, 0);
      else {
        g.fillStyle = CHIP_COLORS[0];
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
    var stored = LegaiaSaves.getSession(meta.id);
    if (!stored) { fallback(); return; }
    wasm().then(function (mod) {
      var c = rgbaCanvas(mod.card_icon_rgba(stored.bytes, meta.block), 16, 16);
      if (!c) { fallback(); return; }
      iconCache[meta.id] = c;
      done(c);
    }).catch(fallback);
  }

  /* Re-derive level / location for sessions stored before the bar existed
   * (their meta predates those fields). One shot per save per page load. */
  function enrichMeta(meta) {
    if (enriched[meta.id] || meta.level != null) return;
    enriched[meta.id] = true;
    var stored = LegaiaSaves.getSession(meta.id);
    if (!stored) return;
    wasm().then(function (mod) {
      var sum = JSON.parse(mod.save_summary_json(stored.bytes));
      var s = sum.kind === 'lgsf'
        ? sum
        : (sum.saves || []).filter(function (x) { return x.block === meta.block; })[0];
      if (!s) return;
      LegaiaSaves.patchSessionMeta(meta.id, {
        level: s.level != null ? s.level : null,
        location: s.location || meta.location || '',
        party: (s.party && s.party.length) ? s.party : meta.party,
        money: s.money != null ? s.money : meta.money,
        coins: s.coins != null ? s.coins : meta.coins,
      });
    }).catch(function () { /* stays as-is; the bar shows what meta has */ });
  }

  /* ---------------- actions ---------------- */

  /* The retail menu confirm blip (static cue 0x20), when the shared cue
   * player has it rendered off the visitor's disc. Silent otherwise. */
  function confirmBlip() {
    if (window.LegaiaSfx && LegaiaSfx.ready()) LegaiaSfx.playCue(0x20, 'saves.confirm');
  }

  function loadSave(meta) {
    if (meta.kind !== 'card' || meta.coins == null) return;
    /* Coins won before any save was loaded ride along into the session. */
    var pending = LegaiaSaves.takeCoinsWon();
    var coins = Math.min(COIN_CAP, (meta.coins || 0) + pending);
    LegaiaSaves.setCoinSession(meta.id, coins);
    confirmBlip();
    setStatus('Loaded "' + (meta.name || meta.id) + '" - ' + coins + ' coins in the session'
      + (pending ? ' (' + pending + ' pending winnings absorbed)' : '') + '.', 'good');
    if (window.MgSession && typeof MgSession.onSaveLoaded === 'function') {
      MgSession.onSaveLoaded();
    }
    render();
  }

  function exportSave(meta) {
    var cs = LegaiaSaves.coinSession();
    if (!cs || cs.saveId !== meta.id) return;
    var stored = LegaiaSaves.getSession(meta.id);
    if (!stored) {
      setStatus('The bytes for that save are gone from this browser.', 'bad');
      return;
    }
    var coins = Math.min(COIN_CAP, Math.max(0, cs.coins || 0));
    wasm().then(function (mod) {
      var patched = mod.card_patch_coins(stored.bytes, meta.block, coins);
      var id = LegaiaSaves.putSession(
        Object.assign({}, stored.meta, { coins: coins }), patched);
      if (id === null) {
        setStatus('Could not store the updated save (storage full?).', 'bad');
        return;
      }
      delete iconCache[meta.id];
      confirmBlip();
      setStatus('Wrote ' + coins + ' coins into "' + (meta.name || meta.id)
        + '" - use its download button for the emulator file.', 'good');
      render();
    }).catch(function (err) {
      setStatus('Export failed: ' + (err && err.message ? err.message : err), 'bad');
    });
  }

  function downloadSave(meta) {
    var stored = LegaiaSaves.getSession(meta.id);
    if (!stored) { setStatus('The bytes for that save are gone.', 'bad'); return; }
    var base = (meta.name || meta.id).replace(/[^\w.-]+/g, '-');
    var ext = meta.kind === 'card' ? (meta.format || 'mcr') : 'lgsf';
    var url = URL.createObjectURL(new Blob([stored.bytes], { type: 'application/octet-stream' }));
    var a = document.createElement('a');
    a.href = url;
    a.download = base + '.' + ext;
    a.click();
    setTimeout(function () { URL.revokeObjectURL(url); }, 5000);
  }

  function deleteSave(meta) {
    var cs = LegaiaSaves.coinSession();
    if (cs && cs.saveId === meta.id) LegaiaSaves.setCoinSession(null);
    LegaiaSaves.deleteSession(meta.id);
    delete iconCache[meta.id];
    setStatus('Deleted "' + (meta.name || meta.id) + '".');
    render();
  }

  /* Import a picked save file: every valid Legaia save block inside a card
   * container becomes its own bar entry; an .lgsf is one entry. */
  function addFile(file) {
    setStatus('Reading ' + file.name + '…');
    return file.arrayBuffer().then(function (buf) {
      var bytes = new Uint8Array(buf);
      return wasm().then(function (mod) {
        var sum = JSON.parse(mod.save_summary_json(bytes));
        var base = file.name.replace(/\.[^.]+$/, '');
        if (sum.kind === 'lgsf') {
          LegaiaSaves.putSession({
            kind: 'lgsf', name: base, party: sum.party || [],
            level: sum.level != null ? sum.level : null,
            money: sum.money != null ? sum.money : null,
          }, bytes);
          setStatus('Added engine save "' + base + '".', 'good');
          return 1;
        }
        var valid = (sum.saves || []).filter(function (s) { return s.valid; });
        if (!valid.length) {
          setStatus('No Legaia save found in that container.', 'bad');
          return 0;
        }
        valid.forEach(function (s) {
          LegaiaSaves.putSession({
            kind: 'card',
            name: valid.length > 1 ? base + ' (block ' + s.block + ')' : base,
            block: s.block,
            format: sum.format || 'mcr',
            party: s.party || [],
            level: s.level != null ? s.level : null,
            money: s.money != null ? s.money : null,
            coins: s.coins != null ? s.coins : null,
            location: s.location || '',
            scene: s.scene || '',
          }, bytes);
        });
        setStatus('Added ' + valid.length + ' save' + (valid.length === 1 ? '' : 's')
          + ' from ' + file.name + '.', 'good');
        return valid.length;
      });
    }).catch(function (err) {
      setStatus('Could not read that file: ' + (err && err.message ? err.message : err), 'bad');
      return 0;
    });
  }

  /* ---------------- render ---------------- */

  function sessionLine() {
    var cs = LegaiaSaves.coinSession();
    if (cs) {
      var meta = LegaiaSaves.listSessions().filter(function (s) {
        return s.id === cs.saveId;
      })[0];
      return 'Session: ' + cs.coins + ' coins · '
        + (meta ? (meta.name || meta.id) : 'deleted save');
    }
    var pending = LegaiaSaves.coinsWon();
    return pending > 0
      ? pending + ' coins won · load a save to bank them'
      : 'No save loaded · winnings stay in this browser';
  }

  function tileFor(meta, loadedId) {
    var tile = el('div', 'mgsb-tile' + (meta.id === loadedId ? ' is-loaded' : ''));
    tile.dataset.saveId = meta.id;

    var iconBox = el('span', 'mgsb-icon');
    var main = document.createElement('canvas');
    main.width = 16; main.height = 16;
    iconBox.appendChild(main);
    iconCanvasFor(meta, function (c) {
      var g = main.getContext('2d');
      g.clearRect(0, 0, 16, 16);
      g.drawImage(c, 0, 0);
    });
    /* Party faces stacked beside the card icon - disc portraits where the
     * character has one, initial chips where it doesn't. */
    var facesBox = el('span', 'mgsb-faces');
    (meta.party || []).slice(0, 3).forEach(function (name, i) {
      var pf = portraitFor(name);
      facesBox.appendChild(pf ? scaledFace(pf, name) : initialChip(name, i));
    });
    iconBox.appendChild(facesBox);
    tile.appendChild(iconBox);

    var info = el('div', 'mgsb-info');
    var lead = (meta.party && meta.party[0]) || '';
    info.appendChild(el('div', 'mgsb-name',
      (meta.name || meta.id) + (meta.kind === 'card' ? '' : ' (engine)')));
    var stats = el('div', 'mgsb-stats');
    var bits = [];
    if (lead) bits.push(lead + (meta.level != null ? ' Lv <b>' + meta.level + '</b>' : ''));
    if (meta.money != null) bits.push('<b>' + meta.money + '</b> G');
    if (meta.coins != null) bits.push('<b>' + meta.coins + '</b> coins');
    stats.innerHTML = bits.join(' · ') || '&mdash;';
    info.appendChild(stats);
    var loc = meta.location || meta.scene || '';
    if (loc) info.appendChild(el('div', 'mgsb-loc', loc));
    tile.appendChild(info);

    var btns = el('div', 'mgsb-btns');
    if (meta.id === loadedId) {
      var exp = el('button', 'mgsb-btn mgsb-primary', 'Export');
      exp.type = 'button';
      exp.title = 'Write the session coin balance back into this save slot';
      exp.addEventListener('click', function () { exportSave(meta); });
      btns.appendChild(exp);
    } else {
      var load = el('button', 'mgsb-btn mgsb-primary', 'Load');
      load.type = 'button';
      if (meta.kind !== 'card' || meta.coins == null) {
        load.disabled = true;
        load.title = 'Engine .lgsf saves carry no casino coin bank - open this one on the play page';
      } else {
        load.title = 'Import this save’s coin bank into the minigame session';
        load.addEventListener('click', function () { loadSave(meta); });
      }
      btns.appendChild(load);
    }
    var row2 = el('div', 'mgsb-btnrow');
    var dl = el('button', 'mgsb-btn', '↓');
    dl.type = 'button';
    dl.title = 'Download this save as a file';
    dl.addEventListener('click', function () { downloadSave(meta); });
    row2.appendChild(dl);
    var del = el('button', 'mgsb-btn', '×');
    del.type = 'button';
    del.title = 'Remove this save from the browser';
    del.addEventListener('click', function () { deleteSave(meta); });
    row2.appendChild(del);
    btns.appendChild(row2);
    tile.appendChild(btns);

    enrichMeta(meta);
    return tile;
  }

  function render() {
    if (!containerEl) return;
    ensureStyle();
    if (!portraits) portraits = portraitsFromCache();
    harvestPortraits();

    containerEl.textContent = '';
    containerEl.style.display = '';
    var box = el('div', 'mgsb');

    var head = el('div', 'mgsb-head');
    head.appendChild(el('span', null, 'Saves'));
    head.appendChild(el('span', 'mgsb-session', sessionLine()));
    var status = el('span', 'mgsb-status' + (statusCls ? ' is-' + statusCls : ''), statusMsg);
    head.appendChild(status);
    head.appendChild(el('span', 'mgsb-spacer'));

    var add = el('button', 'mgsb-btn', '+ Add save file');
    add.type = 'button';
    add.title = 'Import a memory-card save (.mcr / .mcd / .gme / .mcs) or an engine .lgsf';
    var picker = document.createElement('input');
    picker.type = 'file';
    picker.accept = '.mcr,.mcd,.gme,.mcs,.lgsf';
    picker.style.display = 'none';
    picker.id = 'mgsb-file';
    picker.addEventListener('change', function () {
      if (picker.files && picker.files[0]) {
        addFile(picker.files[0]).then(function () { picker.value = ''; render(); });
      }
    });
    add.addEventListener('click', function () { picker.click(); });
    head.appendChild(add);
    head.appendChild(picker);
    box.appendChild(head);

    var sessions = LegaiaSaves.listSessions();
    if (!sessions.length) {
      box.appendChild(el('div', 'mgsb-empty',
        'No saves in this browser yet. Add a memory-card save (.mcr / .gme / .mcs from '
        + 'your emulator) or an engine .lgsf - it persists here, loads its casino coin '
        + 'bank into the games below, and exports back with your winnings. '
        + 'Nothing is uploaded.'));
    } else {
      var cs = LegaiaSaves.coinSession();
      var loadedId = cs ? cs.saveId : null;
      var row = el('div', 'mgsb-row');
      sessions.forEach(function (meta) { row.appendChild(tileFor(meta, loadedId)); });
      box.appendChild(row);
    }
    containerEl.appendChild(box);
  }

  /* Coin-only updates (per-spin balance syncs): refresh the session line
   * without rebuilding the tiles. */
  function renderSessionLine() {
    var s = containerEl && containerEl.querySelector('.mgsb-session');
    if (s) s.textContent = sessionLine();
  }

  function attach(elOrId) {
    containerEl = typeof elOrId === 'string' ? document.getElementById(elOrId) : elOrId;
    if (!containerEl) return;
    render();
    window.addEventListener('legaia-saves-change', render);
    window.addEventListener('legaia-coin-session-change', renderSessionLine);
    /* The page fires this once its LegaiaMinigames instance has a disc -
     * the moment the portrait TIMs become decodable. */
    window.addEventListener('mg-disc-loaded', function () {
      if (harvestPortraits()) render();
    });
  }

  window.MgSaveBar = {
    attach: attach,
    render: render,
    addFile: addFile,
    loadSave: loadSave,
    exportSave: exportSave,
    portraitsReady: function () { return !!portraits; },
  };
})();
