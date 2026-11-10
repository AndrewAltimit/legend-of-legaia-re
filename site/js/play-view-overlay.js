/* play-view-overlay.js - PlayView's 2D overlay canvas.
 *
 * Part of the PlayView class (play-app.js): the menu font and chrome
 * blitters, the fishing HUD, and the overlay draw (dialogue, menus, battle
 * HUD, banners) with its text wash.
 *
 * Classic script. Requires play-atlas-blitter.js and play-app.js first. */
(function () {
  'use strict';

  const AtlasBlitter = window.LegaiaAtlasBlitter;

  window.LegaiaUtil.installMethods(window.PlayView, class {

    /* Upload the pause-menu atlases (font glyphs + the disc's menu-chrome sheet)
     * as `AtlasBlitter`s. Idempotent, and safe to call from any frame - which is
     * the whole point, because it no longer runs only when the menu opens.
     *
     * Every `play_menu_*` getter reads the engine's *cached* menu assets and
     * answers a default when the engine has not built them yet:
     * `play_menu_font_dims()` gives `[0, 0]` and `play_menu_has_chrome()` gives
     * `false`. So "this disc has no chrome" and "nobody has asked the engine to
     * build the chrome yet" are the same answer over the wire, and a resolution
     * taken off it before the engine is ready is wrong - permanently, because
     * the chrome branch only ever ran while `_menuChrome` was `undefined`.
     *
     * That used to be unreachable: every caller sat behind `play_menu_open()`,
     * a shop, a naming prompt or a dialog draw, all of which build the assets
     * first. It is reachable now - the reading-box capability probe calls this
     * from the top of `_drawOverlay`, i.e. from frame one - and taking the
     * not-ready answer as final cost the page every chrome sprite for the rest
     * of the session: pause menu, shop, naming prompt and the retail dialog
     * reading box all fell back to bare glyphs on black.
     *
     * So: don't resolve anything until the engine actually has assets, and
     * treat a chrome-less answer as provisional rather than final. `null` still
     * means "no chrome to blit right now" for every call site that guards on
     * it, but a later `true` from the engine upgrades it. Once both blitters
     * exist the early return makes this free. */
    _ensureMenuBlitters() {
      if (this._menuFont && this._menuChrome) return;
      const rt = this.rt;
      try {
        const fd = rt.play_menu_font_dims();
        /* The engine's readiness signal: `[0, 0]` until it has built its menu
         * assets at all. Ask again next frame rather than answering for it. */
        if (!(fd && fd.length === 2 && fd[0] > 0 && fd[1] > 0)) return;
        if (!this._menuFont) {
          const rgba = rt.play_menu_font_rgba();
          if (rgba && rgba.length) this._menuFont = new AtlasBlitter(rgba, fd[0], fd[1], AtlasBlitter.tinterFor(rt));
        }
        if (!this._menuChrome) {
          if (rt.play_menu_has_chrome()) {
            const cd = rt.play_menu_chrome_dims();
            const rgba = rt.play_menu_chrome_rgba();
            if (cd && cd.length === 2 && cd[0] > 0 && rgba && rgba.length) {
              this._menuChrome = new AtlasBlitter(rgba, cd[0], cd[1], AtlasBlitter.tinterFor(rt), true);
            } else {
              this._menuChrome = null;
            }
          } else {
            /* A PROT.DAT-only load really has no chrome sheet. Provisional:
             * re-asked next call, which is what lets a late-arriving scene
             * host upgrade it. */
            this._menuChrome = null;
          }
        }
      } catch (e) { console.warn('play menu: atlas upload', e); this._menuChrome = null; }
    }

    /* Headless-verification hook for the overlay atlases - the sibling of
     * `window.__fsState` / `window.__woWalkStamps` on the other 3D pages, and
     * the reason this defect is now assertable in one line instead of by
     * eyeballing a screenshot.
     *
     * The invariant a driver should check, once a scene is running:
     *
     *     engineHasChrome === true  =>  chrome === 'built'
     *
     * A page that reports `engineHasChrome: true` with `chrome: 'absent'` has
     * resolved the blitter from an answer the engine gave before it had
     * built its menu assets, and every chrome sprite - pause menu, shop,
     * naming prompt, reading box - is being dropped on the floor. */
    overlayAtlasState() {
      const rt = this.rt;
      let engineHasChrome = null, ready = null;
      try { engineHasChrome = rt.play_menu_has_chrome(); } catch (e) {}
      try {
        const fd = rt.play_menu_font_dims();
        ready = !!(fd && fd.length === 2 && fd[0] > 0 && fd[1] > 0);
      } catch (e) {}
      return {
        ready, engineHasChrome,
        font: !!this._menuFont,
        chrome: this._menuChrome === undefined ? 'unresolved'
          : (this._menuChrome ? 'built' : 'absent'),
        dialogCanvasCapable: this.dialogCanvasCapable,
      };
    }

    /* Fishing HUD layer. Returns `true` when it drew (a session is live), so
     * `_drawOverlay` can stop there.
     *
     * The text quads come from the engine's shared draw-list consumer
     * (`fishing_hud_draws_for`, the same call the native window makes) and are
     * blitted from the font atlas like every other overlay layer. The `bars`
     * channel is separate for one reason: the fishing sprite page is the one
     * asset in the chain nobody has decoded, so the consumer's atlas is blind
     * and it drops the gauge fills. Their geometry still comes from the engine
     * (the ported cap/body/cap frame), so this only fills the rect it is told
     * to. */
    _drawFishingHud(ctx, ov) {
      if (typeof this.rt.play_fishing_hud_json !== 'function') return false;
      let hud = null;
      try { hud = JSON.parse(this.rt.play_fishing_hud_json(ov.width, ov.height)); }
      catch (e) { return false; }
      if (!hud || !hud.open) return false;
      this._ensureMenuBlitters();
      ctx.clearRect(0, 0, ov.width, ov.height);
      /* Gauge fills first, so the digit rows read on top of them. Coordinates
       * are retail 320x240 stage pixels; `stage` is the engine's own
       * origin/scale for this surface, the same transform its text quads were
       * already scaled by. */
      const st = hud.stage || [0, 0, 1];
      for (const b of (hud.bars || [])) {
        if (b.w <= 0 || b.h <= 0) continue;
        ctx.fillStyle = 'rgb(' + b.rgb[0] + ',' + b.rgb[1] + ',' + b.rgb[2] + ')';
        ctx.fillRect(st[0] + b.x * st[2], st[1] + b.y * st[2],
          Math.max(1, b.w * st[2]), Math.max(1, b.h * st[2]));
      }
      if (this._menuFont) this._menuFont.blit(ctx, hud.texts);
      this._overlayActive = true;
      return true;
    }

    /* Blit the current pause-menu OR retail-dialog draw lists onto the 2D
     * overlay canvas: the gold 9-slice / filigree chrome from the menu sheet,
     * then the font glyphs. A no-op (and a one-shot clear) when neither is up.
     *
     * The pause menu blacks the whole surface (retail suppresses the 3D draw
     * under it); the dialog reading box does NOT - retail draws it over the
     * live, still-running field, so only the box quads paint. */
    _drawOverlay() {
      const ov = this.menuOverlay;
      if (!ov) return;
      const ctx = this._menuCtx || (this._menuCtx = ov.getContext('2d'));
      this._drawOverlayBase(ctx, ov);
      /* Developer-menu overlay (the visitor's explicit opt-in - the browser
       * twin of the native LEGAIA_DEV_MENU surface). Painted last so it rides
       * whatever the base layers drew, the same way the native window folds
       * its dev draws into the field HUD's list; the engine returns
       * `open:false` while the opt-in is off or the pause menu owns the
       * screen, so this is a no-op on the shipped default. */
      if (typeof this.rt.play_dev_menu_draws_json === 'function') {
        let dev = null;
        try { dev = JSON.parse(this.rt.play_dev_menu_draws_json(ov.width, ov.height)); }
        catch (e) { dev = null; }
        if (dev && dev.open) {
          this._ensureMenuBlitters();
          if (this._menuFont) this._menuFont.blit(ctx, dev.texts);
          this._overlayActive = true;
          this._overlayPainted = null;
        }
      }
    }

    _drawOverlayBase(ctx, ov) {
      /* What the shop and dialog layers last painted (below), and only if
       * nothing has painted since: every other branch of this method, the
       * developer menu and a stopped view's title screens leave it null. */
      const paintedLast = this._overlayPainted;
      this._overlayPainted = null;
      /* PSX UI is nearest-neighbour: the native wgpu overlay samples the atlas
       * with no filtering, so the integer-scaled tiles butt edge-to-edge. Canvas
       * 2D defaults to bilinear (`imageSmoothingEnabled` true), which bleeds a
       * half-texel across every tile boundary - the visible seams in the 9-slice
       * chrome's repeated fill. Force nearest so the repeat is seamless and the
       * glyphs stay crisp, matching native. */
      ctx.imageSmoothingEnabled = false;
      /* Settle the reading-box capability once, ahead of every early return
       * below, so it is already true before the first conversation and cannot
       * flip with whatever else owns the overlay this frame. Both halves have
       * to hold: the engine must export the builder (a cached WASM predating
       * it does not), and the font atlas must have uploaded, because without
       * `_menuFont` the blit paints no glyphs. `_ensureMenuBlitters` is
       * idempotent and returns immediately once built. */
      if (!this.dialogCanvasCapable
          && typeof this.rt.play_dialog_draws_json === 'function') {
        this._ensureMenuBlitters();
        if (this._menuFont) this.dialogCanvasCapable = true;
      }
      let open = false;
      try { open = this.rt.play_menu_is_open(); } catch (e) {}
      if (open) {
        this._overlayActive = true;
        this._ensureMenuBlitters();
        /* As below for the shop and dialog layers: the answer is all this paint
         * reads, and the canvas keeps what it last painted from it. */
        let drawsJson;
        try { drawsJson = this.rt.play_menu_draws_json(ov.width, ov.height); }
        catch (e) { return; }
        const menuKey = [drawsJson, this._menuFont, this._menuChrome, ov.width, ov.height];
        if (paintedLast && menuKey.every((v, i) => v === paintedLast[i])) {
          this._overlayPainted = paintedLast;
          return;
        }
        let draws;
        try { draws = JSON.parse(drawsJson); }
        catch (e) { return; }
        if (!draws || !draws.open) {
          /* The pause wipe: the menu is open but not spawned yet, and the
           * field darkens under the wipe quad. Nothing of the field's layers
           * may stay on this canvas - retail's party readout is gone from the
           * first field tick after the menu press (the press rearms its idle
           * countdown, `FUN_801D0D38`), and the native window draws none.
           * Returning without a clear kept the last field frame's readout
           * here, dimmed by the wipe drawn over it. */
          ctx.clearRect(0, 0, ov.width, ov.height);
          this._overlayPainted = menuKey;
          return;
        }
        /* Native blacks the whole framebuffer while the pause menu is up
         * (`boot_ui.is_active()` clears to black + suppresses every 3D draw) - the
         * frozen scene is NOT visible around the windows. This overlay canvas sits
         * over the GL view, so paint it fully opaque black first, then blit the
         * menu on top: same result as native's black backdrop. */
        ctx.clearRect(0, 0, ov.width, ov.height);
        ctx.globalAlpha = 1;
        ctx.fillStyle = '#000';
        ctx.fillRect(0, 0, ov.width, ov.height);
        if (this._menuChrome) this._menuChrome.blitDarkened(ctx, draws.sprites, draws.darken);
        if (this._menuFont) this._menuFont.blit(ctx, draws.texts);
        this._overlayPainted = menuKey;
        return;
      }
      /* Opening name-entry overlay (`town01`'s naming prompt). Modal, so it is
       * checked before the shop / dialog layers - but like them it composites
       * over the live scene rather than blacking it: retail draws the grid and
       * name-field windows over the frozen establishing shot. */
      let naming = null;
      if (typeof this.rt.name_entry_draws_json === 'function') {
        try { naming = JSON.parse(this.rt.name_entry_draws_json(ov.width, ov.height)); }
        catch (e) { naming = null; }
      }
      if (naming && naming.open) {
        this._ensureMenuBlitters();
        ctx.clearRect(0, 0, ov.width, ov.height);
        if (this._menuChrome) this._menuChrome.blit(ctx, naming.sprites);
        if (this._menuFont) this._menuFont.blit(ctx, naming.texts);
        this._overlayActive = true;
        return;
      }

      /* Fishing minigame HUD (the retail persistent + catch rows through the
       * shared `fishing_hud_draws_for` consumer, plus the gauge frames the
       * undecoded sprite page cannot fill). Checked before the shop / dialog
       * layers because fishing is a mode *suspend*: while it runs, the field
       * underneath is frozen and nothing else can be up. Composites over the
       * live scene rather than blacking it. */
      if (this._drawFishingHud(ctx, ov)) return;

      /* Field merchant panel + post-action banners (level-up, Seru capture).
       * Same builders as the native window (`shop_screen_draws`,
       * `level_up_draws_for`, `capture_banner_draws_for`). The banners
       * composite over the live field like the dialog box; a shop asks for a
       * black backdrop (`backdrop`), as retail's does. Sits above
       * the dialog check because a merchant's box closes before the shop
       * opens, and a banner should not be hidden by one. */
      let shopJson = null;
      if (typeof this.rt.play_overlay_draws_json === 'function') {
        try { shopJson = this.rt.play_overlay_draws_json(ov.width, ov.height); }
        catch (e) { shopJson = null; }
      }
      let dlgJson = null;
      if (typeof this.rt.play_dialog_draws_json === 'function') {
        try { dlgJson = this.rt.play_dialog_draws_json(ov.width, ov.height); }
        catch (e) { dlgJson = null; }
      }
      /* The canvas keeps its pixels. The answers these layers paint from are
       * all of what they read, so a frame that hands back the strings the last
       * one painted has nothing to do: the field HUD standing still costs no
       * parse, no blit and no fresh canvas for the compositor. */
      const washJson = this._textWashJson();
      const paintKey = [shopJson, dlgJson, this._menuFont, this._menuChrome, ov.width, ov.height, washJson];
      if (paintedLast && paintKey.every((v, i) => v === paintedLast[i])) {
        this._overlayPainted = paintedLast;
        return;
      }
      /* A fade: the same quads under another wash. The pixels read back for the
       * first of its frames stand for them all, and a readback waits on the GPU. */
      const held = this._washHeld;
      if (paintedLast && held && held.key.every((v, i) => v === paintKey[i])
          && this._washFrom(ctx, held, washJson)) {
        this._overlayPainted = paintKey;
        return;
      }
      this._washHeld = null;
      let shop = null;
      if (shopJson !== null) {
        try { shop = JSON.parse(shopJson); }
        catch (e) { shop = null; }
      }
      /* This layer does NOT own the frame: the battle HUD rides it, and a
       * reading box can be up during battle (the sparring fight talks over
       * the running battle). Returning here suppressed the canvas reading box
       * for every in-battle line, so the page fell back to the DOM
       * `.play-dialog` strip - CSS-positioned near the BOTTOM - while the
       * engine's own box geometry says top (`dialog_reading_box_layout`,
       * FUN_801D84D0). So blit and fall through to the dialog layer. */
      let overlayDrew = false;
      const shopBlack = !!(shop && shop.open && shop.backdrop === 'black');
      if (shop && shop.open) {
        this._ensureMenuBlitters();
        ctx.clearRect(0, 0, ov.width, ov.height);
        /* A shop is a menu-overlay session: retail swaps the field overlay
         * out and draws the shop's windows on black (`MenuRuntime::
         * covers_field`), the same backdrop the native window clears to. */
        if (shop.backdrop === 'black') {
          ctx.globalAlpha = 1;
          ctx.fillStyle = '#000';
          ctx.fillRect(0, 0, ov.width, ov.height);
        }
        if (this._menuChrome) this._menuChrome.blit(ctx, shop.sprites);
        if (this._menuFont) this._menuFont.blit(ctx, shop.texts);
        this._overlayActive = true;
        overlayDrew = true;
      }

      /* Retail dialog reading box (field NPC / event message): the engine
       * serves the byte-pinned chrome + glyph quads; blit them over the live
       * GL view. The DOM text box stands in only where this whole path is
       * unavailable (`dialogCanvasCapable` false - a cached WASM without the
       * export, or no font atlas); it is never a per-frame alternative to
       * this, because the two are seated differently and a page that switches
       * between them mid-conversation shows the box move. */
      let dlg = null;
      if (dlgJson !== null) {
        try { dlg = JSON.parse(dlgJson); }
        catch (e) { dlg = null; }
      }
      if (dlg && dlg.open) {
        this._ensureMenuBlitters();
        /* Only clear when nothing has painted this frame - the shop / battle
         * layer above already cleared and its quads must survive. */
        if (!overlayDrew) ctx.clearRect(0, 0, ov.width, ov.height);
        if (this._menuChrome) this._menuChrome.blit(ctx, dlg.sprites);
        if (this._menuFont) this._menuFont.blit(ctx, dlg.texts);
        this._overlayActive = true;
        this._applyTextWash(ctx, ov, shopBlack ? null : [dlg.sprites, dlg.texts, overlayDrew && shop.sprites, overlayDrew && shop.texts], paintKey);
        /* Blitters that were still missing at the key's read came up inside
         * this paint, so the next frame's key differs and paints once more. */
        this._overlayPainted = paintKey;
        return;
      }
      if (overlayDrew) {
        this._applyTextWash(ctx, ov, shopBlack ? null : [shop.sprites, shop.texts], paintKey);
        this._overlayPainted = paintKey;
        return;
      }
      /* Opening-cutscene narration crawl / title card / "It was the Seru."
       * caption: font-atlas text quads + one faded image quad over the live
       * 3D prologue scene. */
      let cutDrew = false;
      if (this._cut && (this._cut.narration || this._cut.card
          || this._cut.caption_alpha > 0.001)
          && typeof this.rt.play_cutscene_text_draws_json === 'function') {
        let txt = null;
        try { txt = JSON.parse(this.rt.play_cutscene_text_draws_json(ov.width, ov.height)); }
        catch (e) { txt = null; }
        this._ensureMenuBlitters();
        ctx.clearRect(0, 0, ov.width, ov.height);
        if (txt && txt.open && this._menuFont) {
          this._menuFont.blit(ctx, txt.texts);
          cutDrew = true;
        }
        /* Caption image (a baked TIM): centered horizontally, mid-screen
         * ~y110 of the PSX 240-line frame, scaled by h/240, faded by the
         * engine's alpha - the native window's caption quad. */
        if (this._cut.caption_alpha > 0.001) {
          if (this._captionBlit === undefined) {
            this._captionBlit = null;
            try {
              const cd = this.rt.cutscene_caption_dims();
              if (cd && cd[0] > 0 && cd[1] > 0) {
                const rgba = this.rt.cutscene_caption_rgba();
                if (rgba && rgba.length) {
                  this._captionBlit = new AtlasBlitter(rgba, cd[0], cd[1]);
                }
              }
            } catch (e) { this._captionBlit = null; }
          }
          if (this._captionBlit) {
            const scale = ov.height / 240;
            const dw = Math.round(this._captionBlit.w * scale);
            const dh = Math.round(this._captionBlit.h * scale);
            const dx = Math.round((ov.width - dw) / 2);
            const dy = Math.round((110 / 240) * ov.height - dh / 2);
            this._captionBlit.blit(ctx, [{
              dst: [dx, dy, dw, dh],
              src: [0, 0, this._captionBlit.w, this._captionBlit.h],
              color: [1, 1, 1, Math.min(1, this._cut.caption_alpha)],
            }]);
            cutDrew = true;
          }
        }
        if (cutDrew) {
          this._overlayActive = true;
          this._applyTextWash(ctx, ov);
          return;
        }
      }
      if (this._overlayActive) {
        ctx.clearRect(0, 0, ov.width, ov.height);
        this._overlayActive = false;
      }
    }

    /* The screen-effect pushes that draw OVER the text layer, applied to this
     * frame's freshly painted overlay pixels. Retail links every glyph at OT
     * bucket 1 and a bucket-0 push after it, so the opening's push to black
     * dims the crawl with the scene; the GL pass already washed the scene,
     * but this canvas sits above it. The engine resolves which pushes those
     * are and their colours (`play_text_layer_washes_json`); the equations
     * are the PSX ABR modes `legaia_render_kernels::screen_prim::wash_channel`
     * names. Only called right after a branch repainted the canvas, so a
     * wash never compounds across frames. */
    _textWashJson() {
      if (typeof this.rt.play_text_layer_washes_json !== 'function') return null;
      try { return this.rt.play_text_layer_washes_json(); }
      catch (e) { return null; }
    }

    /* `quads`: the draw lists this paint made, when they are all it made. */
    _applyTextWash(ctx, ov, quads, paintKey) {
      const json = this._textWashJson();
      if (json === null) return;
      let washes = null;
      try { washes = JSON.parse(json); }
      catch (e) { return; }
      if (!washes || !washes.length || !ov.width || !ov.height) return;
      /* Only where this paint put anything: reading a canvas back is the cost. */
      let x0 = 0, y0 = 0, x1 = ov.width, y1 = ov.height;
      if (quads) {
        x0 = ov.width; y0 = ov.height; x1 = 0; y1 = 0;
        for (const list of quads) {
          for (const q of list || []) {
            const d = q.dst;
            if (!d || d[2] <= 0 || d[3] <= 0) continue;
            if (d[0] < x0) x0 = d[0];
            if (d[1] < y0) y0 = d[1];
            if (d[0] + d[2] > x1) x1 = d[0] + d[2];
            if (d[1] + d[3] > y1) y1 = d[1] + d[3];
          }
        }
        x0 = Math.max(0, x0); y0 = Math.max(0, y0);
        x1 = Math.min(ov.width, x1); y1 = Math.min(ov.height, y1);
        if (x1 <= x0 || y1 <= y0) return;
      }
      const img = ctx.getImageData(x0, y0, x1 - x0, y1 - y0);
      if (paintKey) this._washHeld = { key: paintKey.slice(0, 6), x0, y0, w: img.width, h: img.height, px: img.data.slice() };
      this._washPixels(img.data, washes);
      ctx.putImageData(img, x0, y0);
    }

    /* The held pixels under `json`'s washes. False for no wash at all: that frame
     * paints, since a canvas does not take back an unwashed copy exactly. */
    _washFrom(ctx, held, json) {
      let washes = null;
      try { washes = JSON.parse(json); } catch (e) { return false; }
      if (!washes || !washes.length) return false;
      const img = new ImageData(held.px.slice(), held.w, held.h);
      this._washPixels(img.data, washes);
      ctx.putImageData(img, held.x0, held.y0);
      return true;
    }

    /* One equation per channel, so a table per channel. */
    _washPixels(px, washes) {
      const lut = this._washLut || (this._washLut = [new Uint8Array(256), new Uint8Array(256), new Uint8Array(256)]);
      for (const [abr, fr, fg, fb] of washes) {
        const f = [fr, fg, fb];
        for (let c = 0; c < 3; c++) {
          const t = lut[c], fc = f[c];
          for (let b = 0; b < 256; b++) {
            let v;
            switch (abr & 3) {
              case 0: v = (b + fc) >> 1; break;
              case 1: v = b + fc; break;
              case 2: v = b - fc; break;
              default: v = b + (fc >> 2); break;
            }
            t[b] = v < 0 ? 0 : (v > 255 ? 255 : v);
          }
        }
        const r = lut[0], g = lut[1], bl = lut[2];
        for (let i = 0; i < px.length; i += 4) {
          if (px[i + 3] === 0) continue;
          px[i] = r[px[i]];
          px[i + 1] = g[px[i + 1]];
          px[i + 2] = bl[px[i + 2]];
        }
      }
    }
  });
})();
