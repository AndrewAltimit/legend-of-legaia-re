/* minigame-saves.js - the ALWAYS-VISIBLE slim save bar on minigames.html.
 *
 * A thin coin-economy driver over the shared rich save bar
 * (`LegaiaSaves.createRichBar` in js/legaia-saves.js). The shared bar draws
 * the retail-style memory-card tiles - the SC block's baked lead-face icon,
 * the party's disc portraits, lead / displayed level / gold / casino coins /
 * location - and owns the file picker, download, delete and status line. This
 * file supplies only the pieces that are specific to the minigames page:
 *
 *   - the disc portrait harvest (the page's LegaiaMinigames instance decodes
 *     the three load-screen portrait TIMs off the visitor's disc);
 *   - the primary per-tile action: "Load" makes a save the session's wallet
 *     (its casino coin bank, RAM 0x800845A4 in the SC block, becomes the
 *     session coin balance the slot machine racks from and Baka payouts add
 *     to, plus any coins already won this browser before a save was loaded),
 *     and "Export" writes the live session balance back into that save slot's
 *     bytes in localStorage;
 *   - the coin session line and the import handler.
 *
 * Byte-level work (summaries, icon decode, the 4-byte coin patch) is the WASM
 * save tools (crates/web-viewer/src/session_save.rs), loaded lazily.
 */
(function () {
  'use strict';

  var COIN_CAP = 9999999;

  var containerEl = null;
  var bar = null;              /* LegaiaSaves.createRichBar instance */
  var wasmPromise = null;

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

  /* Pull the three load-screen portrait TIMs off the loaded disc (the
   * minigames page's LegaiaMinigames instance) and cache the pixels. Returns
   * true the first time the faces become available. */
  function harvestPortraits() {
    if (LegaiaSaves.portraitCanvases()) return false;
    var api = window.__mgApi;
    if (!api || typeof api.save_portrait_rgba !== 'function') return false;
    var rgba = [];
    for (var i = 0; i < 3; i++) {
      var buf = api.save_portrait_rgba(i);
      if (!buf || buf.length !== 16 * 16 * 4) return false;
      rgba.push(buf);
    }
    return !!LegaiaSaves.cachePortraits(rgba);
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
    if (bar) bar.setStatus('Loaded "' + (meta.name || meta.id) + '" - ' + coins + ' coins in the session'
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
      if (bar) bar.setStatus('The bytes for that save are gone from this browser.', 'bad');
      return;
    }
    var coins = Math.min(COIN_CAP, Math.max(0, cs.coins || 0));
    wasm().then(function (mod) {
      var patched = mod.card_patch_coins(stored.bytes, meta.block, coins);
      var id = LegaiaSaves.putSession(
        Object.assign({}, stored.meta, { coins: coins }), patched);
      if (id === null) {
        if (bar) bar.setStatus('Could not store the updated save (storage full?).', 'bad');
        return;
      }
      if (bar) bar.invalidateIcon(meta.id);
      confirmBlip();
      if (bar) bar.setStatus('Wrote ' + coins + ' coins into "' + (meta.name || meta.id)
        + '" - use its download button for the emulator file.', 'good');
      render();
    }).catch(function (err) {
      if (bar) bar.setStatus('Export failed: ' + (err && err.message ? err.message : err), 'bad');
    });
  }

  /* Import a picked save file: every valid Legaia save block inside a card
   * container becomes its own bar entry; an .lgsf is one entry. */
  function addFile(file) {
    if (bar) bar.setStatus('Reading ' + file.name + '…');
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
          if (bar) bar.setStatus('Added engine save "' + base + '".', 'good');
          return 1;
        }
        var valid = (sum.saves || []).filter(function (s) { return s.valid; });
        if (!valid.length) {
          if (bar) bar.setStatus('No Legaia save found in that container.', 'bad');
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
        if (bar) bar.setStatus('Added ' + valid.length + ' save' + (valid.length === 1 ? '' : 's')
          + ' from ' + file.name + '.', 'good');
        return valid.length;
      });
    }).catch(function (err) {
      if (bar) bar.setStatus('Could not read that file: ' + (err && err.message ? err.message : err), 'bad');
      return 0;
    });
  }

  /* ---------------- session line ---------------- */

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

  /* The primary per-tile button: Export when this tile is the loaded wallet,
   * Load otherwise (disabled for engine .lgsf saves - no coin bank). */
  function primaryButton(meta, ctx) {
    if (meta.id === ctx.loadedId) {
      return {
        text: 'Export', cls: 'lgrb-primary',
        title: 'Write the session coin balance back into this save slot',
        onClick: exportSave,
      };
    }
    if (meta.kind !== 'card' || meta.coins == null) {
      return {
        text: 'Load', cls: 'lgrb-primary', disabled: true,
        title: 'Engine .lgsf saves carry no casino coin bank - open this one on the play page',
      };
    }
    return {
      text: 'Load', cls: 'lgrb-primary',
      title: 'Import this save’s coin bank into the minigame session',
      onClick: loadSave,
    };
  }

  /* When a loaded save is deleted, drop the coin session too. */
  function onDelete(meta) {
    var cs = LegaiaSaves.coinSession();
    if (cs && cs.saveId === meta.id) LegaiaSaves.setCoinSession(null);
  }

  /* ---------------- render / attach ---------------- */

  function render() {
    if (bar) bar.render();
  }

  function attach(elOrId) {
    containerEl = typeof elOrId === 'string' ? document.getElementById(elOrId) : elOrId;
    if (!containerEl) return;
    bar = LegaiaSaves.createRichBar(containerEl, {
      heading: 'Saves',
      wasm: wasm,
      harvest: harvestPortraits,
      sessionLine: sessionLine,
      showCoins: true,
      acceptExts: '.mcr,.mcd,.gme,.mcs,.lgsf',
      onAdd: addFile,
      onDelete: onDelete,
      primaryButton: primaryButton,
      loadedId: function () {
        var cs = LegaiaSaves.coinSession();
        return cs ? cs.saveId : null;
      },
      emptyText:
        'No saves in this browser yet. Add a memory-card save (<b>.mcr / .gme / .mcs</b> from '
        + 'your emulator) or an engine <b>.lgsf</b> - it persists here, loads its casino coin '
        + 'bank into the games below, and exports back with your winnings. Nothing is uploaded.',
    });
    render();
    window.addEventListener('legaia-saves-change', render);
    window.addEventListener('legaia-coin-session-change', function () {
      if (bar) bar.renderSessionLine();
    });
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
    portraitsReady: function () { return !!LegaiaSaves.portraitCanvases(); },
  };
})();
