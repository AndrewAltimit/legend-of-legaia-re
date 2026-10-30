/* play-expansion-live.js - the play page's end of the expansion workbench's
 * edit-test loop.
 *
 * Two ways in, one engine call:
 *
 *   - **Play here** (an ordinary play tab): the workbench posts the working
 *     pack's `.lxpack` bytes on BroadcastChannel `legaia-expansion-live`. A
 *     tab whose active pack has the same id hot-reloads it in place
 *     (`PlayView.reloadExpansion` -> `LegaiaRuntime::reload_expansion`): the
 *     engine re-activates the pack and, when the player stands on one of its
 *     maps, re-enters that map with the player where they stood. The tab
 *     answers, so the workbench opens a new tab only when none did.
 *   - **The 3D preview** (`play.html?xwpreview=1`, framed by the workbench):
 *     the same page, embedded, driven by `postMessage` from its parent - load
 *     a pack and enter a map, reload it after an edit, set the editor
 *     overlay (play-view-editor-overlay.js), switch between the free orbit
 *     camera and the player's follow camera, stand the player on a tile,
 *     play a cutscene. A click in the picture answers with the map tile under
 *     it, and the preview reports its frame cost once a second.
 *
 * Messages, both directions, are `{ type, ... }` objects; the channel ones
 * carry a `nonce` the answer repeats. Nothing here reads or writes storage:
 * the bytes travel in the message.
 *
 * Classic script. `attach(api)` is called by play.html once, with:
 *   runtime()             the live LegaiaRuntime (or null)
 *   view()                the PlayView (or null)
 *   loadPack(bytes, from) load + activate a pack; the engine's info JSON
 *   enterMap(map, spawn)  enter a pack map through the page's entry path
 *   status(text, bad)     the page's expansion status line
 *   adopted(bytes)        a hot reload took (the page keeps the bytes)
 */
(function () {
  'use strict';

  const CHANNEL = 'legaia-expansion-live';

  function packId(rt) {
    try {
      const info = JSON.parse(rt.expansion_info_json());
      return { id: info.pack ? info.pack.id : null, map: info.map || null };
    } catch (e) {
      return { id: null, map: null };
    }
  }

  function attach(api) {
    const preview = /[?&]xwpreview=1\b/.test(location.search);
    let channel = null;
    try { channel = new BroadcastChannel(CHANNEL); } catch (e) { channel = null; }

    /* Reload `bytes` into the running engine. A tab not yet on any pack
     * loads it; one on the pack hot-reloads. */
    function reload(bytes, from) {
      const rt = api.runtime();
      const v = api.view();
      if (!rt) return { ok: false, errors: ['load the disc first'] };
      const t0 = performance.now();
      const cur = packId(rt);
      let r;
      if (!v || !cur.id) {
        const info = api.loadPack(bytes, from);
        r = Object.assign({ reentered: false, map: null }, info || {});
      } else {
        try { r = v.reloadExpansion(bytes); } catch (e) { r = { ok: false, errors: [String(e && e.message ? e.message : e)] }; }
        if (r.ok) {
          if (api.adopted) api.adopted(bytes);
          api.status(`Reloaded ${from}${r.reentered ? ` on ${r.map}` : ''} (${Math.round(performance.now() - t0)} ms).`);
        } else {
          api.status(`${from} was not reloaded: ${(r.errors || []).slice(0, 2).join(' | ')}`, true);
        }
      }
      r.totalMs = Math.round(performance.now() - t0);
      if (v && typeof v.refreshEditorOverlay === 'function') v.refreshEditorOverlay();
      return r;
    }

    /* --- Play here: an ordinary play tab --- */
    if (channel && !preview) {
      channel.onmessage = (ev) => {
        const m = ev.data || {};
        const rt = api.runtime();
        if (m.type === 'reload') {
          if (!rt) return;
          const cur = packId(rt);
          if (cur.id !== m.id) return;
          const r = reload(m.bytes, `${m.id}.lxpack`);
          channel.postMessage({
            type: 'reloaded', nonce: m.nonce, ok: !!r.ok, map: r.map || null,
            reentered: !!r.reentered, ms: r.totalMs, errors: r.errors || [],
          });
        }
      };
    }

    if (!preview) return { reload };
    // Headless-verification hook: the framed PlayView.
    window.__xwView = () => api.view();

    /* --- The workbench's 3D preview --- */
    const parentWin = window.parent && window.parent !== window ? window.parent : null;
    const post = (msg, transfer) => {
      if (parentWin) parentWin.postMessage(msg, location.origin, transfer || []);
    };
    let wantOrbit = true;
    let focusTile = null;
    let pendingEdit = null;

    function viewReady() {
      const v = api.view();
      if (!v) return null;
      v.editorOverlayOn = true;
      v.maxOrbitHalf = 16000;
      /* Draw at the size the frame shows, not the play page's 960 x 720:
       * the picture is a panel in the editor, and the fill is the cost. */
      if (!v._xwSized && !v.psxRender) {
        v._xwSized = true;
        for (const c of [v.canvas, v.menuOverlay].filter(Boolean)) {
          const w = Math.max(320, Math.min(960, Math.round(c.clientWidth || 640)));
          c.width = w;
          c.height = Math.round(w * 3 / 4);
        }
      }
      if (!v._xwFrameHooked) {
        /* Frame cost: the CPU time of each `_frame`, averaged over a second,
         * and the display rate. */
        v._xwFrameHooked = true;
        const inner = v._frame;
        let acc = 0, n = 0, worst = 0, last = performance.now(), rest = 0;
        v._frame = function (skipDraw) {
          /* The frame shares the workbench's main thread (a same-origin
           * frame runs on its parent's), so a frame that cost more than a
           * display frame buys idle frames after it: the editor keeps
           * answering on a heavy map. The sim clock is wall time
           * (`play_drain_sim_steps`), so the world keeps its speed. */
          if (rest > 0 && !skipDraw) { rest--; return undefined; }
          const t = performance.now();
          const out = inner.call(this, skipDraw);
          const d = performance.now() - t;
          /* At most every other display frame (30 fps on a 60 Hz screen):
           * an editor's preview, not the game. */
          rest = Math.max(1, Math.min(3, Math.floor(d / 16)));
          acc += d; n++; if (d > worst) worst = d;
          if (t - last >= 1000) {
            post({ type: 'xw-stats', frameMs: acc / Math.max(n, 1), worstMs: worst, fps: n * 1000 / (t - last) });
            acc = 0; n = 0; worst = 0; last = t;
          }
          return out;
        };
        attachPicking(v);
      }
      /* The map editor's layer (play-view-editor-gizmos.js): its pointer
       * handlers go on first, so an edit drag is not also a camera turn. */
      if (typeof v.attachEditorEditing === 'function') {
        v.attachEditorEditing(post);
        if (pendingEdit) { v.setEditorEdit(pendingEdit); pendingEdit = null; }
      }
      return v;
    }

    function applyCamera(v) {
      v.debugCamera = wantOrbit;
      if (wantOrbit) {
        if (focusTile) v.focusEditorTile(focusTile[0], focusTile[1]);
        else if (!v.editorFocus) v.focusEditorTile();
      }
    }

    /* A click (a press and release with almost no travel) picks the tile
     * under it; a right or middle drag pans the orbit focus. */
    function attachPicking(v) {
      const cv = v.canvas;
      let down = null;
      cv.addEventListener('pointerdown', (e) => {
        down = { x: e.clientX, y: e.clientY, b: e.button };
      });
      cv.addEventListener('contextmenu', (e) => e.preventDefault());
      cv.addEventListener('pointermove', (e) => {
        if (!down || down.b === 0 || !v.debugCamera || !v.editorFocus) return;
        const dx = e.clientX - down.x, dy = e.clientY - down.y;
        down.x = e.clientX; down.y = e.clientY;
        down.moved = true;
        const k = (v.cam.halfWidth * 2) / Math.max(cv.clientWidth, 1);
        const sy = Math.sin(v.cam.yaw), cy = Math.cos(v.cam.yaw);
        /* Screen X is mirrored on this camera (buildWorldOrbitVp). */
        v.editorFocus[0] += (dx * cy + dy * sy) * k;
        v.editorFocus[2] += (dx * sy - dy * cy) * k;
        focusTile = null;
      });
      cv.addEventListener('pointerup', (e) => {
        const d = down;
        down = null;
        if (!d || d.moved || e.button !== 0) return;
        if (Math.hypot(e.clientX - d.x, e.clientY - d.y) > 4) return;
        const t = v.pickTile(e.clientX, e.clientY);
        if (t) {
          v.setEditorOverlay({ select: { tile: t } });
          post({ type: 'xw-pick', tile: t });
        }
      });
    }

    function enter(map, spawn) {
      const t0 = performance.now();
      api.enterMap(map, spawn || '');
      if (packId(api.runtime()).map !== map) throw new Error(`map ${map} did not enter`);
      const v = viewReady();
      if (v) {
        /* A new map: frame it whole, not the last map's focus. */
        v.editorFocus = null;
        focusTile = null;
        v.refreshEditorOverlay();
        applyCamera(v);
      }
      return Math.round(performance.now() - t0);
    }

    window.addEventListener('message', (ev) => {
      if (ev.origin !== location.origin || ev.source !== parentWin) return;
      const m = ev.data || {};
      const rt = api.runtime();
      const v = api.view();
      if (m.type === 'xw-load' || m.type === 'xw-reload') {
        if (!rt) { post({ type: 'xw-loaded', ok: false, errors: ['the preview has no disc yet'] }); return; }
        const t0 = performance.now();
        const cur = packId(rt);
        let r;
        /* A first load, or the workbench moved to another map: activate and
         * enter it. Otherwise the same map hot-reloads where the player is. */
        if (m.type === 'xw-load' || !cur.id || !v || (m.map && cur.map !== m.map)) {
          const info = api.loadPack(m.bytes, `${m.id || 'pack'}.lxpack`) || {};
          r = { ok: !!info.ok, errors: info.errors || [], reentered: false };
          if (r.ok && m.map) {
            try { r.enterMs = enter(m.map, m.spawn); r.map = m.map; r.reentered = true; }
            catch (e) { r.ok = false; r.errors = [String(e && e.message ? e.message : e)]; }
          }
        } else {
          /* The binder the workbench opened is the preview's own screen:
           * close it rather than wait for it. */
          try {
            if (typeof rt.play_cards_json === 'function' && JSON.parse(rt.play_cards_json()).open) rt.play_cards_close();
          } catch (e) { /* no card screen */ }
          r = reload(m.bytes, `${m.id || 'pack'}.lxpack`);
        }
        const vv = viewReady();
        if (vv) {
          vv.refreshEditorOverlay();
          if (r.ok && typeof vv.editorRebuilt === 'function') vv.editorRebuilt();
          applyCamera(vv);
        }
        post({
          type: 'xw-loaded', ok: !!r.ok, busy: !!r.busy, map: r.map || null, reentered: !!r.reentered,
          errors: r.errors || [], engineMs: r.ms != null ? r.ms : null,
          ms: Math.round(performance.now() - t0),
        });
      } else if (m.type === 'xw-edit') {
        if (v && typeof v.setEditorEdit === 'function') v.setEditorEdit(m);
        else pendingEdit = Object.assign(pendingEdit || {}, m);
      } else if (m.type === 'xw-overlay') {
        if (v) v.setEditorOverlay(m);
      } else if (m.type === 'xw-camera') {
        wantOrbit = m.mode !== 'player';
        if (m.focus !== undefined) focusTile = m.focus;
        if (v) {
          if (m.focus === null) v.editorFocus = null;
          applyCamera(v);
        }
      } else if (m.type === 'xw-seat') {
        if (rt && m.tile && typeof rt.expansion_seat_player === 'function') {
          rt.expansion_seat_player(m.tile[0], m.tile[1]);
        }
      } else if (m.type === 'xw-cutscene') {
        const ok = !!(rt && typeof rt.expansion_preview_cutscene === 'function' && rt.expansion_preview_cutscene(m.id));
        if (ok && v) { wantOrbit = false; applyCamera(v); }
        post({ type: 'xw-action', what: 'cutscene', id: m.id, ok });
      } else if (m.type === 'xw-fight') {
        const ok = !!(rt && typeof rt.expansion_preview_fight === 'function' && rt.expansion_preview_fight(m.kind, m.id));
        if (ok && v) { wantOrbit = false; applyCamera(v); }
        post({ type: 'xw-action', what: 'fight', id: m.id, ok });
      } else if (m.type === 'xw-cards') {
        const n = rt && typeof rt.expansion_preview_cards === 'function' ? rt.expansion_preview_cards() : -1;
        post({ type: 'xw-action', what: 'cards', ok: n >= 0, added: n });
      } else if (m.type === 'xw-run') {
        /* The workbench hides the preview off the Maps and Cutscenes tabs:
         * stop the loop there rather than draw into a hidden frame. */
        if (v) { if (m.on) v.start(); else v.stop(); }
      }
    });

    return {
      reload,
      /* play.html calls this once the disc is up. */
      ready() { post({ type: 'xw-ready' }); },
    };
  }

  window.LegaiaExpansionLive = { attach, CHANNEL };
})();
