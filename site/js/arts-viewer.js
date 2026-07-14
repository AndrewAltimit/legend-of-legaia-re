/* Tactical-Arts animation viewer for arts.html.
 *
 * Hosts a `LegaiaArts` WASM instance (crates/web-viewer/src/arts_view.rs):
 * the character's battle mesh is ASSEMBLED from the player battle file's
 * equipment sections the way the retail battle loader builds it, textured
 * from the same file's texture pools + decoded battle palette, and posed
 * from the on-disc keyframe streams - the record[0] idle loop between arts,
 * and each art's "ME"-archive clip when a card is clicked.
 *
 * Art card -> bank record resolution ladder (mirrored 1:1 by the disc-gated
 * `crates/web-viewer/tests/arts_view_real.rs` so coverage is asserted):
 *   0. action_constant == staged anim_id (exact on retail);
 *   1. name + combo both match a named record;
 *   2. combo match (>= 2 directions; named records preferred);
 *   3. name match (placeholder-combo Super-Art tail records preferred).
 * Consecutive bank records sharing the hit's name or full combo are the
 * art's strike segments and play chained (e.g. Noa's Hurricane Kick).
 *
 * Rendering reuses MeshView (webgl-tmd.js TmdRenderer + the object-local
 * bone poser). Requires webgl-math.js, webgl-shaders.js, webgl-tmd.js and
 * mesh-view.js first; rom-cache.js + load-progress.js for the disc input.
 *
 * Graceful fallback contract: an art whose clip did not resolve/decode
 * plays the battle idle loop with a visible per-art note - never a broken
 * canvas.
 */
(function () {
  'use strict';

  const CHAR_SLOT = { Vahn: 0, Noa: 1, Gala: 2, Terra: 3 };
  /* FUN_80047430 advances rate/8 keyframes per 60 Hz tick. */
  const fpsForRate = (rate) => (rate > 0 ? 7.5 * rate : 15);

  const norm = (s) => String(s || '').toLowerCase().replace(/[^a-z0-9]/g, '');

  function comboEq(a, b) {
    if (!a || !b || a.length !== b.length) return false;
    for (let i = 0; i < a.length; i++) if (a[i] !== b[i]) return false;
    return true;
  }

  /* The resolution ladder (see header). `bank` is set_character()'s arts
   * array; `art` is one arts.json row ({name, directions, action_constant}).
   * Returns the bank index or -1. */
  function resolveArt(bank, art) {
    const ac = art.action_constant;
    if (ac != null && ac >= 0x10) {
      const i = bank.findIndex((r) => r.anim_id === ac);
      if (i >= 0) return i;
    }
    const want = norm(art.name);
    const dirs = art.directions || [];
    let i = bank.findIndex((r) =>
      r.name && norm(r.name) === want && comboEq(r.combo, dirs));
    if (i >= 0) return i;
    if (dirs.length >= 2) {
      const hits = [];
      bank.forEach((r, k) => { if (comboEq(r.combo, dirs)) hits.push(k); });
      const named = hits.find((k) => bank[k].name);
      if (named !== undefined) return named;
      if (hits.length) return hits[0];
    }
    const hits = [];
    bank.forEach((r, k) => { if (r.name && norm(r.name) === want) hits.push(k); });
    const tail = hits.find((k) => (bank[k].combo || []).length <= 1);
    if (tail !== undefined) return tail;
    return hits.length ? hits[0] : -1;
  }

  /* The strike-segment chain: `first` plus every immediately following
   * record sharing its non-empty name or its full (>= 2 direction) combo. */
  function chainOf(bank, first) {
    const chain = [first];
    const name = bank[first].name;
    const combo = bank[first].combo || [];
    for (let i = first + 1; i < bank.length; i++) {
      const sameName = !!name && bank[i].name === name;
      const sameCombo = combo.length >= 2 && comboEq(bank[i].combo, combo);
      if (!(sameName || sameCombo)) break;
      chain.push(i);
    }
    return chain;
  }

  class ArtsViewerApp {
    /* els: { canvas, status, stage, now, note } (DOM elements). */
    constructor(els) {
      this.els = els;
      this.api = null;        /* LegaiaArts */
      this.view = null;       /* MeshView */
      this.charState = null;  /* set_character() JSON for the current char */
      this.charName = null;
      this.currentArt = null; /* the playing art's display name, or null=idle */
    }

    get ready() { return !!(this.api && this.charState && this.charState.ok); }

    async load(file) {
      if (!file) return;
      const prog = LoadProgress.create(this.els.status);
      try {
        const buf = await prog.read(file, `Reading ${file.name}`);
        prog.indeterminate('Initialising the arts decoder…');
        /* Resolve against the PAGE (this file lives in js/, the wasm glue in
         * wasm/ next to the page - a bare './wasm/...' would resolve against
         * this script's URL). */
        const mod = await import(new URL('wasm/legaia_web_viewer.js', document.baseURI).href);
        await mod.default();
        if (typeof mod.LegaiaArts !== 'function') {
          prog.fail('This build of the viewer has no arts support.');
          return;
        }
        this.api = new mod.LegaiaArts();
        prog.indeterminate('Parsing PROT.DAT…');
        await prog.paint();
        const st = JSON.parse(this.api.load_disc(buf));
        prog.indeterminate('Assembling the battle mesh…');
        await prog.paint();
        const name = this.charName || 'Vahn';
        const ok = this.setCharacter(name);
        if (!ok) {
          prog.fail(`Character assembly failed: ${(this.charState || {}).why || 'unknown'}`);
          return;
        }
        this.els.stage.hidden = false;
        prog.done(`Ready - ${st.entries} PROT entries; click any art card below.`);
        document.body.classList.add('arts-live');
      } catch (err) {
        prog.fail(`Failed to decode: ${err.message || err}`);
        console.error(err);
      }
    }

    /* Assemble + show `name`'s battle mesh, dropping into the idle loop.
     * Returns false (with a status note) when the character doesn't build. */
    setCharacter(name) {
      this.charName = name;
      if (!this.api) return false;
      const slot = CHAR_SLOT[name];
      if (slot === undefined) return false;
      this.charState = JSON.parse(this.api.set_character(slot));
      if (!this.charState.ok) {
        this.els.now.textContent = `${name}: ${this.charState.why || 'did not assemble'}`;
        return false;
      }
      if (!this.view) {
        this.view = new window.MeshView(this.els.canvas, {
          cam: { yaw: Math.PI / 2, pitch: 0.05, distance: 2.2, autoRotate: false },
          zoom: { min: 1.2, max: 6 },
        });
      }
      this.view.uploadVram(this.api.vram_bytes());
      /* TmdRenderer binds UVs as u8 texels and cba/tsb as u16 pairs; the
       * WASM getters return i32 / u32 - convert (same as characters.html). */
      const uvsI32 = this.api.mesh_uvs();
      const uvs8 = new Uint8Array(uvsI32.length);
      for (let i = 0; i < uvsI32.length; i++) uvs8[i] = uvsI32[i] & 0xFF;
      this.view.setMesh(
        this.api.mesh_positions(),
        uvs8,
        Uint16Array.from(this.api.mesh_cba_tsb()),
        this.api.mesh_indices(),
        this.api.mesh_bounds(),
        this.api.mesh_object_ids());
      this.playIdle();
      return true;
    }

    playIdle(note) {
      if (!this.ready || !this.view) return;
      this.currentArt = null;
      const idle = this.charState.idle;
      const frames = this.api.idle_pose_frames();
      if (idle && frames.length) {
        this.view.setAnimation({
          partCount: this.charState.part_count,
          frameCount: idle.frames,
          frames,
          fps: fpsForRate(idle.rate),
        });
        this.view.setPlaying(true);
        this.els.now.textContent = `${this.charName} - battle idle`;
      } else {
        this.view.setAnimation(null);
        this.els.now.textContent = `${this.charName} - rest pose (no idle stream)`;
      }
      this.els.note.textContent = note || '';
    }

    /* Play one arts.json card: {name, directions, action_constant}. Falls
     * back to the idle loop with a visible note when the art has no
     * decodable clip on this disc. */
    playArt(art) {
      if (!this.ready || !this.view) return;
      const bank = this.charState.arts;
      const first = resolveArt(bank, art);
      if (first < 0) {
        this.playIdle(`"${art.name}" has no dedicated animation record on this disc - showing the battle idle.`);
        return;
      }
      const chain = chainOf(bank, first).filter((i) => bank[i].ok && bank[i].frames > 0);
      if (!chain.length) {
        this.playIdle(`"${art.name}"'s keyframe stream did not decode (${bank[first].why || 'unknown'}) - showing the battle idle.`);
        return;
      }
      /* Concatenate the chain's segments into one clip. Segments share the
       * rig width; playback rate follows the first segment's rate byte. */
      const parts = this.charState.part_count;
      const segs = chain.map((i) => this.api.art_pose_frames(i));
      let total = 0;
      for (const s of segs) total += s.length;
      const frames = new Int32Array(total);
      let o = 0;
      for (const s of segs) { frames.set(s, o); o += s.length; }
      this.currentArt = art.name;
      this.view.setAnimation({
        partCount: parts,
        frameCount: total / (parts * 6),
        frames,
        fps: fpsForRate(bank[first].rate),
        /* Arts translate the actor (flips, lunges): frame the whole clip. */
        fitAll: true,
      });
      this.view.setPlaying(true);
      const dev = bank[first].name && norm(bank[first].name) !== norm(art.name)
        ? ` (dev name "${bank[first].name}")` : '';
      const segNote = chain.length > 1 ? `, ${chain.length} chained segments` : '';
      this.els.now.textContent =
        `${this.charName} - ${art.name}${dev}`;
      this.els.note.textContent =
        `record 0x${bank[first].anim_id.toString(16).toUpperCase()}` +
        `, ${total / (parts * 6)} keyframes @ rate ${bank[first].rate}${segNote}`;
    }
  }

  /* Wire the page: disc input + canvas + the art-card / tab delegation.
   * Card clicks are delegated from `panelsEl` so the tab script's
   * re-renders don't need re-wiring; a card carries its art as a
   * `data-art` JSON attribute (set by arts.html's renderer). */
  ArtsViewerApp.mount = function (ids) {
    const $ = (id) => document.getElementById(id);
    const els = {
      canvas: $(ids.canvas), status: $(ids.status), stage: $(ids.stage),
      now: $(ids.now), note: $(ids.note),
    };
    const app = new ArtsViewerApp(els);
    const fileInput = $(ids.file);
    if (fileInput && window.RomCache) {
      RomCache.attach(fileInput, { onLoad: (f) => app.load(f) });
    }
    const panels = $(ids.panels);
    if (panels) {
      const activate = (card) => {
        if (!app.ready) return;
        try {
          app.playArt(JSON.parse(card.dataset.art));
          document.querySelectorAll('.art-card-playing')
            .forEach((el) => el.classList.remove('art-card-playing'));
          card.classList.add('art-card-playing');
          els.stage.scrollIntoView({ behavior: 'smooth', block: 'nearest' });
        } catch (err) { console.warn('arts: bad card payload', err); }
      };
      panels.addEventListener('click', (e) => {
        const card = e.target.closest('.art-card[data-art]');
        if (card) activate(card);
      });
      panels.addEventListener('keydown', (e) => {
        if (e.key !== 'Enter' && e.key !== ' ') return;
        const card = e.target.closest('.art-card[data-art]');
        if (card) { e.preventDefault(); activate(card); }
      });
    }
    /* Headless-verification hooks (see the Playwright driver). */
    window.__artsApp = app;
    window.__artsLoad = (f) => app.load(f);
    window.__artsState = () => ({
      ready: app.ready,
      character: app.charName,
      current: app.currentArt,
      bank: app.charState ? app.charState.arts : null,
    });
    return app;
  };

  window.ArtsViewerApp = ArtsViewerApp;
})();
