/* Seru-magic summon viewer for magic.html.
 *
 * Hosts a `LegaiaSummons` WASM instance (crates/web-viewer/src/summon_view.rs).
 * Every player Seru-magic cast (spell ids 0x81..0xA0) streams its creature out
 * of \data\battle\summon.dat while the cast plays; the group's last slot is the
 * summon-creature actor record ([u32 name][u32 TMD][u32 texture pool], per-part
 * keyframe entries from +0x4C). This page decodes that record off the visitor's
 * own disc, relocates it through the same battle texture placement retail uses,
 * and plays the cast's own keyframe clips.
 *
 * Two bands, and the split is why the seven big summons had no model here
 * before:
 *   - 0x81..0x99: the summon reuses an ordinary battle_data enemy body.
 *   - 0x9A..0xA0: Palma, Mule, Horn, Jedo, Meta, Terra, Ozma - a BESPOKE mesh
 *     that byte-matches no archive record. Its texture pool and its per-part
 *     keyframe entries live in the group's third raw slot, not in the record.
 *
 * Rendering reuses MeshView (webgl-tmd.js TmdRenderer + the object-local bone
 * poser), so requires webgl-math.js, webgl-shaders.js, webgl-tmd.js and
 * mesh-view.js first; rom-cache.js + load-progress.js for the disc input.
 *
 * Shading is retail: the WASM side uploads the TMD's own per-vertex packet
 * colour and no light source is applied. (An unbound colour attribute defaults
 * to white, and white is texel * 255/128 - a blowout that reads as "too
 * bright", never as "unlit".)
 *
 * Graceful fallback contract: a cast whose clip did not decode shows the rest
 * pose with a visible note - never a broken canvas, and never a model under
 * the wrong name.
 */
(function () {
  'use strict';

  /* FUN_80047430 advances rate/8 keyframes per 60 Hz tick. */
  const fpsForRate = (rate) => (rate > 0 ? 7.5 * rate : 15);

  /* Per-element accent, used for the card chips only. */
  const ELEMENT_TINT = {
    earth: '#b08a4a', water: '#4a8fd0', fire: '#d05a3a', wind: '#5ab98a',
    thunder: '#d0bb3a', light: '#d9d4c0', dark: '#8a5ad0', neutral: '#8a8aa0',
  };

  const esc = (s) => String(s == null ? '' : s).replace(/[&<>"']/g, (c) => (
    { '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));

  const hex2 = (n) => '0x' + Number(n).toString(16).toUpperCase().padStart(2, '0');

  /* Legaia dialog markup uses caret pairs (^H = a colour switch). A couple of
   * actor records carry one inside the attack name; strip it for display so a
   * card reads "Juggernaut", not "^H Juggernaut". The raw string still goes in
   * the card's title attribute. */
  const plain = (s) => String(s == null ? '' : s).replace(/\^[A-Za-z]/g, '').trim();

  /* A cast's display label. Prefer the summon's own name (the creature it
   * brings out); fall back to the disc's own attack string for the handful of
   * ids no name map covers - never to a guess. */
  const label = (c) => c.summon || plain(c.attack) || hex2(c.spell_id);

  /* Camera framing. OFF (default) frames the creature's own body and follows
   * it: the scale is set by the farthest part that is not a distance outlier
   * among the parts, so one part thrown clear of the body cannot shrink
   * everything, at the cost of that part sometimes sitting outside the frame. ON hands framing back to
   * MeshView's own fit over the union of every frame, which guarantees the
   * whole cast is visible and is therefore the way to see a far part - at the
   * cost of drawing the body small, because these casts travel a long way. */
  const FIT_KEY = 'legaia-magic-fit-all';

  /* Headroom on the robust framing radius: enough that a limb swinging out
   * mid-clip is not clipped at the canvas edge, small enough that the body
   * still fills the frame. Tuned by measuring the drawn pixel extent of all
   * seven summons, not by eye. */
  const FRAME_SLACK = 1.15;

  class SummonViewerApp {
    /* els: { canvas, status, stage, now, note, clips, fx, gallery, grid,
     *        glbBtn } */
    constructor(els) {
      this.els = els;
      this.api = null;        /* LegaiaSummons */
      this.mod = null;        /* the wasm module (framing helpers live there) */
      this.view = null;       /* MeshView */
      this.casts = null;      /* catalog().casts */
      this.state = null;      /* set_cast() JSON for the current cast */
      this.currentId = null;
      this.fitAll = false;
      /* Null = use the WASM default. Overridable so the framing can be swept
       * and measured against the rendered pixels rather than tuned by eye. */
      this.frameOutlierK = null;
      try {
        this.fitAll = window.localStorage.getItem(FIT_KEY) === '1';
      } catch (e) { /* private mode */ }
    }

    /* Flip the framing mode; re-frames the clip already playing. */
    setFitAll(on) {
      this.fitAll = !!on;
      try {
        window.localStorage.setItem(FIT_KEY, this.fitAll ? '1' : '0');
      } catch (e) { /* ignore */ }
      if (this.state && this.state.ok) this.playSequence();
    }

    get ready() { return !!(this.api && this.casts); }

    async load(file) {
      if (!file) return;
      const prog = LoadProgress.create(this.els.status);
      try {
        const buf = await prog.read(file, `Reading ${file.name}`);
        prog.indeterminate('Initialising the summon decoder…');
        /* Resolve against the PAGE (this file lives in js/, the wasm glue in
         * wasm/ next to the page). */
        const v = window.LEGAIA_WASM_V || '0';
        const mod = await import(new URL('wasm/legaia_web_viewer.js?v=' + v, document.baseURI).href);
        await mod.default(new URL('wasm/legaia_web_viewer_bg.wasm?v=' + v, document.baseURI));
        if (typeof mod.LegaiaSummons !== 'function') {
          prog.fail('This build of the viewer has no summon support.');
          return;
        }
        this.api = new mod.LegaiaSummons();
        /* The framing statistic is a free function on the module, not a method
         * on the instance - keep the handle so the camera can reach it. */
        this.mod = mod;
        prog.indeterminate('Parsing PROT.DAT…');
        await prog.paint();
        this.api.load_disc(buf);
        prog.indeterminate('Reading summon.dat…');
        await prog.paint();
        const cat = JSON.parse(this.api.catalog());
        if (!cat.ok) {
          prog.fail(cat.why || 'summon.dat did not decode from this image.');
          return;
        }
        this.casts = cat.casts;
        this.render();
        this.els.stage.hidden = false;
        document.body.classList.add('magic-live');
        const named = this.casts.filter((c) => c.ra_seru).length;
        const ready = `${this.casts.length} Seru-magic casts decoded, ${named} of them Ra-Seru / Sim-Seru summons. Click any card to play its cast.`;
        prog.done(`Ready - ${ready}`);
        /* The bar fades; leave the fact behind in the status line itself. */
        if (this.els.statusLive) this.els.statusLive.textContent = ready;
        /* Open on Meta (0x9E) - the summon the page is named for. */
        this.play(0x9E);
      } catch (err) {
        prog.fail(`Failed to decode: ${err.message || err}`);
        console.error(err);
      }
    }

    /* Replace the static placeholder lists with the disc's own rows. */
    render() {
      const card = (c) => {
        const tint = ELEMENT_TINT[c.element] || '#8a8aa0';
        const body = c.bespoke
          ? 'bespoke model'
          : (c.creature != null ? `battle_data creature ${c.creature}` : 'reused body');
        return `
          <button type="button" class="summon-card${c.ra_seru ? ' summon-card-ra' : ''}"
                  data-spell="${c.spell_id}" title="${esc(c.attack || '')}"
                  aria-label="Play ${esc(label(c))}">
            <span class="summon-card-name">${esc(label(c))}</span>
            <span class="summon-card-attack">${esc(plain(c.attack))}</span>
            <span class="summon-card-meta">
              <span class="summon-el" style="color:${tint}">${esc(c.element)}</span>
              <span class="summon-id">${hex2(c.spell_id)}</span>
            </span>
            <span class="summon-card-sub">${esc(body)} &middot; ${c.clips.length} clip${c.clips.length === 1 ? '' : 's'}</span>
          </button>`;
      };
      if (this.els.gallery) {
        this.els.gallery.innerHTML = this.casts.filter((c) => c.ra_seru).map(card).join('');
      }
      if (this.els.grid) {
        this.els.grid.innerHTML = this.casts.filter((c) => !c.ra_seru).map(card).join('');
      }
    }

    /* Show cast `spellId`: build the mesh, list its clips, play the full cast
     * sequence (every phase clip back to back). */
    play(spellId) {
      if (!this.ready) return;
      const st = JSON.parse(this.api.set_cast(spellId));
      this.state = st;
      this.currentId = spellId;
      if (!st.ok) {
        this.els.now.textContent = `${hex2(spellId)}: ${st.why || 'did not decode'}`;
        this.els.note.textContent = '';
        if (this.els.glbBtn) this.els.glbBtn.disabled = true;
        return;
      }
      if (!this.view) {
        /* These bodies are tall and thin, and MeshView frames on the posed
         * AABB's half-diagonal - so a generous camera distance leaves the
         * creature as a sliver. 1.7 fills the canvas the way the monsters
         * page does. */
        try {
          this.view = new window.MeshView(this.els.canvas, {
            cam: { yaw: Math.PI / 2, pitch: 0.08, distance: 1.7, autoRotate: true },
            zoom: { min: 0.9, max: 7 },
          });
        } catch (err) {
          /* TmdRenderer raises the shared no-WebGL2 banner (js/main.js
           * legaiaWebgl2Failure) before throwing. Say so here too rather than
           * leaving a blank canvas and an uncaught error on every card click. */
          this.els.now.textContent = err && err.noWebgl2
            ? 'This browser has no WebGL2 - the summon models cannot draw.'
            : `Renderer failed: ${(err && err.message) || err}`;
          this.els.note.textContent = '';
          console.error(err);
          return;
        }
      }
      this.view.uploadVram(this.api.vram_bytes());
      /* TmdRenderer binds UVs as u8 texels and cba/tsb as u16 pairs; the WASM
       * getters return i32 / u32 - convert (same as arts.html). */
      const uvsI32 = this.api.mesh_uvs();
      const uvs8 = new Uint8Array(uvsI32.length);
      for (let i = 0; i < uvsI32.length; i++) uvs8[i] = uvsI32[i] & 0xFF;
      this.view.setMesh(
        this.api.mesh_positions(),
        uvs8,
        Uint16Array.from(this.api.mesh_cba_tsb()),
        this.api.mesh_indices(),
        this.api.mesh_bounds(),
        this.api.mesh_object_ids(),
        this.api.mesh_flat_rgba());
      this._renderClips(st);
      this._renderFx(st);
      this.playSequence();
      /* A decoded cast is on the canvas - the .glb download can act on it. */
      if (this.els.glbBtn) this.els.glbBtn.disabled = false;
      /* Highlight the active card. */
      document.querySelectorAll('.summon-card-playing')
        .forEach((el) => el.classList.remove('summon-card-playing'));
      const btn = document.querySelector(`.summon-card[data-spell="${spellId}"]`);
      if (btn) btn.classList.add('summon-card-playing');
    }

    _renderClips(st) {
      const host = this.els.clips;
      if (!host) return;
      host.textContent = '';
      (st.clips || []).forEach((k) => {
        const b = document.createElement('button');
        b.type = 'button';
        b.className = 'summon-clip-btn';
        b.textContent = `phase ${k.index + 1}`;
        b.dataset.clip = String(k.index);
        b.title = `${k.frames} keyframes over ${k.parts} parts @ rate ${k.rate} (action tag ${k.tag})`;
        host.appendChild(b);
      });
      if ((st.clips || []).length > 1) {
        const b = document.createElement('button');
        b.type = 'button';
        b.className = 'summon-clip-btn summon-clip-all';
        b.textContent = 'whole cast';
        b.dataset.clip = 'all';
        host.appendChild(b);
      }
    }

    /* The cast's own FX texture pages: the CLUT row + 4bpp page the applier
     * uploads to VRAM while the cast plays (summon.dat's per-action texture
     * slots). Drawn straight to a 2D canvas each. */
    _renderFx(st) {
      const host = this.els.fx;
      if (!host) return;
      host.textContent = '';
      const sizes = st.fx_page_sizes || [];
      sizes.forEach((wh, i) => {
        const rgba = this.api.fx_page_rgba(i);
        if (!rgba || !rgba.length) return;
        const [w, h] = wh;
        const cv = document.createElement('canvas');
        cv.width = w; cv.height = h;
        cv.className = 'summon-fx-page';
        cv.title = `cast FX texture page ${i + 1} - ${w}x${h}, 4bpp through its CLUT row`;
        const ctx = cv.getContext('2d');
        ctx.putImageData(new ImageData(new Uint8ClampedArray(rgba), w, h), 0, 0);
        host.appendChild(cv);
      });
    }

    /* Play one phase clip. */
    playClip(index) {
      if (!this.state || !this.state.ok || !this.view) return;
      const k = (this.state.clips || [])[index];
      const frames = this.api.clip_pose_frames(index);
      if (!k || !frames.length) return;
      this.view.setAnimation({
        partCount: k.parts,
        frameCount: k.frames,
        frames,
        fps: fpsForRate(k.rate),
        fitAll: this.fitAll,
      });
      this._armCamera();
      this.view.setPlaying(true);
      this._label(`phase ${index + 1}`, `${k.frames} keyframes over ${k.parts} parts @ rate ${k.rate}`);
      this._markClip(String(index));
    }

    /* Play every phase clip back to back - what "the cast" looks like. */
    playSequence() {
      if (!this.state || !this.state.ok || !this.view) return;
      const seq = this.api.sequence_clip_indices();
      const frames = this.api.sequence_pose_frames();
      const clips = this.state.clips || [];
      const parts = this.state.part_count;
      if (!seq.length || !frames.length || !parts) {
        this.view.setAnimation(null);
        this._label('rest pose', 'no decodable keyframe clip on this disc');
        return;
      }
      const total = frames.length / (parts * 6);
      this.view.setAnimation({
        partCount: parts,
        frameCount: total,
        frames,
        fps: fpsForRate(clips.length ? clips[0].rate : 0),
        fitAll: this.fitAll,
      });
      this._armCamera();
      this.view.setPlaying(true);
      const dropped = clips.length - seq.length;
      const note = `${total} keyframes across ${seq.length} phase${seq.length === 1 ? '' : 's'}`
        + (dropped > 0 ? ` (${dropped} phase${dropped === 1 ? '' : 's'} use a different rig width - play them on their own)` : '');
      this._label('whole cast', note);
      this._markClip('all');
    }

    /* Keep the creature framed and centred while its cast plays.
     *
     * MeshView frames once, on the clip's first pose, using the bounding box
     * of every posed vertex. Two things break that here:
     *
     *  1. a cast TRANSLATES the whole body a long way (it flies in, dives,
     *     lands), so a frame fixed on pose 0 loses the creature; and
     *  2. a summon rig routinely holds ONE part far from the body - Meta's
     *     sword is thrown clear of the knight - so the box is dominated by
     *     that separation and the camera pulls back to fit a mostly empty
     *     volume. The box is correct and the frame is still useless.
     *
     * So the scale comes from a percentile of vertex distance rather than the
     * extremes (`summon_framing_bound`), and the centre is re-read each frame
     * from the same statistic (`summon_framing_center`). Both live in WASM -
     * `crates/web-viewer/src/summon_view.rs` - so there is one implementation
     * and it is unit-tested against exactly the "body plus distant part" case.
     * The radius is fixed at clip start so the model does not breathe.
     *
     * Both are skipped when the user ticks "frame the whole cast", which is
     * the deliberate way to see a far part that this framing crops.
     */
    _follow() {
      const v = this.view;
      if (!v || !v.anim || this.fitAll || !this.mod) return;
      const out = v.anim.out, ids = v._objIds, pc = v.anim.partCount;
      if (!out || !ids) return;
      const c = this.mod.summon_framing_center(out, ids, pc);
      if (c && c.length === 3) v.center = [c[0], c[1], c[2]];
    }

    /* Set the clip's scale from the robust bound, then arm the follow-cam. */
    _armCamera() {
      if (!this.view) return;
      const v = this.view;
      if (!this.fitAll && this.mod && v.anim && v.anim.out && v._objIds) {
        const k = this.frameOutlierK != null
          ? this.frameOutlierK : this.mod.summon_framing_outlier_k();
        const b = this.mod.summon_framing_bound(
          v.anim.out, v._objIds, v.anim.partCount, k);
        if (b && b.length === 4 && b[3] > 0) {
          v.center = [b[0], b[1], b[2]];
          /* Slack so a limb that swings out mid-clip is not clipped at the
           * canvas edge; the percentile already excluded the far outliers. */
          v.radius = b[3] * FRAME_SLACK;
        }
      }
      v.onFrame = () => this._follow();
      this._follow();
    }

    /* Bake the current cast's summon model + every phase clip (as named glTF
     * TRS animations) into a .glb, entirely client-side (WASM
     * `export_summon_glb`). Returns { bytes, filename } or null. */
    exportGlb() {
      if (!this.state || !this.state.ok || !this.api || !this.api.export_summon_glb) return null;
      const bytes = this.api.export_summon_glb();
      if (!bytes || !bytes.length) return null;
      const name = (this.api.export_name() || label(Object.assign({ spell_id: this.currentId }, this.state)))
        .toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-+|-+$/g, '') || 'cast';
      return { bytes, filename: `legaia-summon-${name}.glb` };
    }

    _markClip(which) {
      if (!this.els.clips) return;
      this.els.clips.querySelectorAll('.summon-clip-playing')
        .forEach((el) => el.classList.remove('summon-clip-playing'));
      const b = this.els.clips.querySelector(`.summon-clip-btn[data-clip="${which}"]`);
      if (b) b.classList.add('summon-clip-playing');
    }

    _label(what, note) {
      const st = this.state || {};
      const name = label(Object.assign({ spell_id: this.currentId }, st));
      const shown = plain(st.attack);
      const attack = shown && shown !== name ? ` - "${shown}"` : '';
      const mp = st.mp != null ? `, ${st.mp} MP` : '';
      this.els.now.textContent = `${name}${attack} (${st.element}${mp}) - ${what}`;
      const body = st.bespoke
        ? 'bespoke summon body from the group\'s raw slot'
        : (st.creature != null ? `battle_data creature ${st.creature}` : 'reused body');
      this.els.note.textContent = `spell ${hex2(this.currentId)}, summon.dat slot ${st.actor_slot}; ${body}; ${note}`;
    }
  }

  /* Wire the page: disc input + canvas + card / clip-chip delegation. */
  SummonViewerApp.mount = function (ids) {
    const $ = (id) => document.getElementById(id);
    const els = {
      canvas: $(ids.canvas), status: $(ids.status), stage: $(ids.stage),
      now: $(ids.now), note: $(ids.note), clips: $(ids.clips),
      fx: ids.fx ? $(ids.fx) : null,
      statusLive: ids.statusLive ? $(ids.statusLive) : null,
      gallery: ids.gallery ? $(ids.gallery) : null,
      grid: ids.grid ? $(ids.grid) : null,
      glbBtn: ids.glb ? $(ids.glb) : null,
    };
    const app = new SummonViewerApp(els);
    /* .glb download: the current cast's summon model + its phase clips,
     * baked in WASM and saved via Blob + anchor (nothing is uploaded).
     * Disabled until a cast is on the canvas (see play()). */
    if (els.glbBtn) {
      els.glbBtn.addEventListener('click', async () => {
        if (!app.state || !app.state.ok) return;
        const prev = els.glbBtn.textContent;
        els.glbBtn.disabled = true;
        els.glbBtn.textContent = 'baking…';
        /* The bake is synchronous inside WASM - repaint the label first. */
        await new Promise((r) => setTimeout(r, 30));
        let msg = null;
        try {
          const out = app.exportGlb();
          if (!out) {
            msg = 'no model';
          } else {
            const url = URL.createObjectURL(
              new Blob([out.bytes], { type: 'model/gltf-binary' }));
            const a = document.createElement('a');
            a.href = url;
            a.download = out.filename;
            a.click();
            setTimeout(() => URL.revokeObjectURL(url), 5000);
          }
        } catch (err) {
          console.warn('summons: glb export failed', err);
          msg = 'export failed';
        }
        els.glbBtn.textContent = msg || prev;
        els.glbBtn.disabled = false;
        if (msg) setTimeout(() => { els.glbBtn.textContent = prev; }, 1500);
      });
    }
    const fitToggle = ids.fit ? $(ids.fit) : null;
    if (fitToggle) {
      fitToggle.checked = app.fitAll;
      fitToggle.addEventListener('change', () => app.setFitAll(fitToggle.checked));
    }
    const fileInput = $(ids.file);
    if (fileInput && window.RomCache) {
      RomCache.attach(fileInput, { onLoad: (f) => app.load(f) });
    }
    const onCard = (e) => {
      const btn = e.target.closest('.summon-card[data-spell]');
      if (!btn || !app.ready) return;
      app.play(Number(btn.dataset.spell));
      els.stage.scrollIntoView({ behavior: 'smooth', block: 'nearest' });
    };
    if (els.gallery) els.gallery.addEventListener('click', onCard);
    if (els.grid) els.grid.addEventListener('click', onCard);
    if (els.clips) {
      els.clips.addEventListener('click', (e) => {
        const btn = e.target.closest('.summon-clip-btn[data-clip]');
        if (!btn || !app.ready) return;
        if (btn.dataset.clip === 'all') app.playSequence();
        else app.playClip(Number(btn.dataset.clip));
      });
    }
    /* Headless-verification hooks (see the Playwright driver). */
    window.__summonApp = app;
    window.__summonLoad = (f) => app.load(f);
    window.__summonState = () => ({
      ready: app.ready,
      count: app.casts ? app.casts.length : 0,
      named: app.casts ? app.casts.filter((c) => c.ra_seru).map((c) => c.summon) : [],
      current: app.currentId,
      state: app.state,
      fitAll: app.fitAll,
      /* What the camera is actually framing - the posed radius, not the raw
       * object-local TMD spread. A summon's parts sit at their own origins
       * until a pose assembles them, so the mesh's own bounds are large and
       * framing on them would draw the creature as a speck. */
      framedRadius: app.view ? app.view.radius : null,
      /* Content probes: the drawn triangle count and whether the packet-colour
       * stream is white (the texel*2 blowout this repo has shipped four
       * times). Both read off the live buffers, not off the JSON. */
      indices: app.view ? app.view.indexCount : 0,
      flatWhite: (() => {
        if (!app.api || !app.api.mesh_flat_rgba) return null;
        const f = app.api.mesh_flat_rgba();
        if (!f || !f.length) return null;
        for (let i = 0; i < f.length; i += 4) {
          if (f[i] !== 255 || f[i + 1] !== 255 || f[i + 2] !== 255) return false;
        }
        return true;
      })(),
      playing: app.view ? app.view.playing : false,
    });
    return app;
  };

  window.SummonViewerApp = SummonViewerApp;
})();
