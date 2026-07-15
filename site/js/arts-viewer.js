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
 * Playback extras, both retail-mechanism:
 *   - VOICE: the character's arts shout is the XA30.XA channel the battle
 *     overlay plays through FUN_8003D53C(0x1D, chan, dur) -> CdlSetfilter
 *     (Vahn ch0 / Noa ch4 / Gala ch6; keyed on the character, not the art;
 *     Terra has no case). Demuxed by the WASM side, fired at art start.
 *   - TRAIL: the tinted after-image (Vahn red / Noa green / Gala blue) -
 *     delayed poses of the same mesh re-drawn additively via
 *     MeshView.setTrail (the PSX ABE semi-transparency trick).
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

  /* Retail draws a delayed translucent after-image of the character during
   * an art, tinted per character: Vahn RED, Noa GREEN, Gala BLUE (a PSX ABE
   * additive re-draw of the animated mesh a few frames behind). The tints
   * are the site's match of the retail look; the mechanism (delayed tinted
   * additive mesh copy) is the retail one. Terra performs no arts - her row
   * gets a neutral violet in case a bank record ever resolves. */
  const TRAIL_TINT = {
    Vahn: [1.0, 0.16, 0.10],
    Noa:  [0.18, 1.0, 0.22],
    Gala: [0.16, 0.36, 1.0],
    Terra: [0.7, 0.4, 1.0],
  };
  /* Echo train: pose delays (clip keyframes) + additive intensities. */
  const TRAIL_ECHOES = [
    { delay: 4, alpha: 0.40 },
    { delay: 8, alpha: 0.22 },
    { delay: 12, alpha: 0.11 },
  ];

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
      this.cueFrames = null;  /* Set of clip frames that fire the strike cue */
      this.cueLog = [];       /* [{frame, t}] - the headless-check hook */
      this.voiceKey = null;   /* LegaiaSfx PCM key of the character's arts
                               * voice (XA30 channel), or null when absent */
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
        /* Sound: render the strike cue off the same disc bytes (SCUS -> the
         * SFX descriptor table, PROT 869 -> the class-2 sound bank, through
         * the clean-room SPU). Non-blocking, and a no-op on a raw PROT.DAT
         * load (no SCUS to read the descriptors from). */
        if (window.LegaiaSfx) LegaiaSfx.init(mod, buf);
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
      /* Arts VOICE: the character's XA30.XA channel (the battle overlay's
       * FUN_8003D53C(0x1D, chan, dur) cue - Vahn ch0 / Noa ch4 / Gala ch6),
       * demuxed by the WASM side off the loaded disc. Registered once per
       * character; null on a raw PROT.DAT load or for Terra. */
      this.voiceKey = null;
      if (this.charState.voice && window.LegaiaSfx && this.api.art_voice_pcm_i16) {
        const key = `arts-voice:${name}`;
        if (!LegaiaSfx.hasPcm(key)) {
          const pcm = this.api.art_voice_pcm_i16();
          if (pcm && pcm.length) {
            LegaiaSfx.registerPcm(key, pcm,
              this.charState.voice.rate, this.charState.voice.stereo);
          }
        }
        if (LegaiaSfx.hasPcm(key)) this.voiceKey = key;
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

    /* Fire the art strike cue on the clip frames `frames` (a Set).
     *
     * Retail times an art's sound from the art record's Hit Effect Cue words
     * ([frame][kind], see docs/formats/art-data.md) - a field whose offset in
     * the record is not pinned, and the move-power table's per-move sound cue
     * covers enemy specials only. So the *cue id* is retail's documented
     * generic "play sound" kind (0x1A, resolved through the disc's SFX
     * descriptor table) and the *frames* are derived from the clip itself -
     * the local peaks of the rig's extension, i.e. where each swing lands.
     * Fitted timing, real cue: the page's note says so. */
    _armCues(frames) {
      this.cueFrames = frames;
      if (!this.view) return;
      this.view.onFrame = (f) => {
        if (!this.cueFrames || !this.cueFrames.has(f)) return;
        this.cueLog.push({ frame: f, t: Date.now() });
        if (this.cueLog.length > 200) this.cueLog.shift();
        /* The strike/impact "punch" SFX is intentionally NOT played: we have
         * not yet faithfully recreated that cue, and the placeholder rendered
         * off the SFX descriptor table is a high-pitched annoyance. The impact
         * frames are still tracked (cueLog / the headless __artsState hook) so
         * the timing stays observable; only the audible cue is suppressed. The
         * VOICE shout (playArt's playPcm path) is a separate, faithful cue and
         * still fires. */
      };
    }

    playIdle(note) {
      if (!this.ready || !this.view) return;
      this.currentArt = null;
      this._armCues(null);          /* the idle loop is silent */
      this.view.setTrail(null);     /* no after-image outside an art */
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
      /* Strike frames of every segment, offset into the concatenated clip. */
      const cues = new Set();
      if (this.api.art_strike_frames) {
        let base = 0;
        chain.forEach((i, k) => {
          const segFrames = segs[k].length / (parts * 6);
          for (const f of this.api.art_strike_frames(i)) cues.add(base + f);
          base += segFrames;
        });
      }
      this._armCues(cues);
      this.currentArt = art.name;
      /* VOICE: retail starts the character's shout as the art begins
       * executing - one XA channel per character (XA30.XA: Vahn ch0 /
       * Noa ch4 / Gala ch6), the same clip for every art of that
       * character. Fired once per activation, not per loop. */
      if (this.voiceKey && window.LegaiaSfx) {
        LegaiaSfx.playPcm(this.voiceKey, 'arts.voice');
      }
      /* Per-character tinted after-image echoes (the retail arts trail). */
      this.view.setTrail({
        tint: TRAIL_TINT[this.charName] || [1, 1, 1],
        echoes: TRAIL_ECHOES,
      });
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
      /* Sound note: only the VOICE shout is surfaced. The strike/impact SFX is
       * not played (see _armCues) - the placeholder punch cue is not yet a
       * faithful recreation - so the page no longer claims a strike cue fires. */
      const voiceNote = (this.voiceKey && this.charState.voice)
        ? `; voice XA30 ch${this.charState.voice.channel} (retail cue)`
        : '';
      this.els.now.textContent =
        `${this.charName} - ${art.name}${dev}`;
      this.els.note.textContent =
        `record 0x${bank[first].anim_id.toString(16).toUpperCase()}` +
        `, ${total / (parts * 6)} keyframes @ rate ${bank[first].rate}${segNote}${voiceNote}`;
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
    /* The shared sound gate (js/audio-toggle.js), parked with the file input.
     * Every cue the viewer fires checks it. */
    if (window.LegaiaSound && fileInput && fileInput.parentElement) {
      LegaiaSound.attach(fileInput.parentElement);
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
      /* Sound: the armed impact frames, the cues actually fired, and what the
       * WASM cue renderer decoded off the disc. */
      cueFrames: app.cueFrames ? Array.from(app.cueFrames).sort((a, b) => a - b) : null,
      cueLog: app.cueLog.slice(),
      /* Trail: the configured tint + how many echo passes drew last frame. */
      trail: app.view && app.view.trail
        ? { tint: app.view.trail.tint, echoes: app.view.trail.echoes.length }
        : null,
      ghostPasses: app.view && app.view.renderer && app.view.renderer.ghostTrail
        ? app.view.renderer.ghostTrail.passes.length : 0,
      /* Voice: the character's XA30 channel metadata + registered key. */
      voice: app.charState ? app.charState.voice || null : null,
      voiceKey: app.voiceKey,
      sfxReady: !!(window.LegaiaSfx && LegaiaSfx.ready()),
      sfxInfo: window.LegaiaSfx && LegaiaSfx.ready() ? LegaiaSfx.info() : null,
      sfxLog: window.LegaiaSfx ? LegaiaSfx.log() : [],
    });
    return app;
  };

  window.ArtsViewerApp = ArtsViewerApp;
})();
