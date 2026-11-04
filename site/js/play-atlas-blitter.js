/* play-atlas-blitter.js - tinted atlas blits onto a 2D canvas.
 *
 * AtlasBlitter draws sub-rects of a font or chrome atlas with a per-quad
 * colour multiply. The play page's overlay (play-view-overlay.js) and the
 * boot-title controller in play.html both draw through it
 * (window.LegaiaAtlasBlitter).
 *
 * Classic script; loads before play-app.js. */
(function () {
  'use strict';

  /* Blits sub-rects of a source atlas (font glyphs or the menu-chrome sheet)
   * with a per-quad RGBA multiply tint, over a 2D canvas. The retail pause menu
   * emits its geometry as `{ dst, src, color }` quads (the shipped
   * `legaia-engine-ui` draw builders); this is the browser twin of the native
   * window's textured-quad overlay pass.
   *
   * The tint is a colour MULTIPLY that preserves the source alpha - identity for
   * white (`(1,1,1)`), so plain chrome blits straight through, while the
   * whitewashed font atlas takes the ink colour and the navy filigree takes its
   * darkening tint. Multiplied full-atlas copies are cached per colour, so the
   * per-quad cost is a single `drawImage`.
   *
   * `tinter` (optional, `AtlasBlitter.tinterFor(runtime)`) builds each tinted
   * copy through the engine's shared overlay kernel instead of a canvas
   * multiply: retail's 5-bit modulation and the font CLUTs' own shadow under
   * each ink (`legaia_render_kernels::ui_tint`, the same law the native text
   * shader runs). Without it - an older wasm bundle - the canvas multiply
   * stands in. */
  class AtlasBlitter {
    /* `lazyRects`: this sheet takes many tints over a few of its rects (the window
     * bands of the menu chrome), so a tinted copy fills only the rects it is asked
     * for. Same canvas, same coordinates, same pixels where any draw reads them. */
    constructor(rgba, w, h, tinter, lazyRects) {
      this.lazyRects = !!lazyRects;
      this.w = w; this.h = h;
      this.rgba = new Uint8Array(rgba);
      this.tinter = typeof tinter === 'function' ? tinter : null;
      this.base = document.createElement('canvas');
      this.base.width = w; this.base.height = h;
      const bctx = this.base.getContext('2d');
      bctx.putImageData(new ImageData(new Uint8ClampedArray(rgba), w, h), 0, 0);
      this.cache = new Map();
      /* While the page is entering a scene, not on the first menu's frame. */
      if (this.tinter) this._indexTexels();
    }
    /* A tinter over the runtime's `psx_tint_atlas`, or null when the loaded
     * bundle predates it. */
    static tinterFor(rt) {
      if (!rt || typeof rt.psx_tint_atlas !== 'function') return null;
      return (rgba, r, g, b) => rt.psx_tint_atlas(rgba, r, g, b);
    }
    /* Forget the tinted copies (after the runtime's tint mode changed). */
    invalidate() { this.cache.clear(); }
    /* The tint kernel reads one texel and the colour, and an atlas holds few
     * distinct texels, most of them nothing: the kernel sees the palette, not the
     * sheet, and the answer goes only to the texels with any coverage. A transparent
     * texel stays transparent under every tint, and a canvas holds no colour under
     * alpha 0. Tinting a new colour is what a first menu, dialog or results screen
     * stalls on. */
    _indexTexels() {
      const px = new Uint32Array(this.rgba.buffer, this.rgba.byteOffset, this.rgba.byteLength >> 2);
      let solid = 0;
      for (let i = 0; i < px.length; i++) if (px[i] >>> 24) solid++;
      const at = new Uint32Array(solid);
      const index = new Uint16Array(solid);
      const seen = new Map();
      const palette = [];
      let last = 0, lastAt = -1, n = 0;
      for (let i = 0; i < px.length; i++) {
        const v = px[i];
        if (!(v >>> 24)) continue;
        if (v !== last || lastAt < 0) {
          let k = seen.get(v);
          if (k === undefined) {
            k = palette.length;
            seen.set(v, k);
            palette.push(v);
          }
          last = v;
          lastAt = k;
        }
        at[n] = i;
        index[n++] = lastAt;
      }
      /* A true-colour sheet past the index's reach goes through whole. */
      this.texelIndex = palette.length < 0xFFFF ? index : null;
      this.texelAt = at;
      this.texelPalette = new Uint8Array(new Uint32Array(palette).buffer);
      if (this.lazyRects && this.texelIndex) {
        const dense = new Uint16Array(this.w * this.h).fill(0xFFFF);
        for (let i = 0; i < n; i++) dense[at[i]] = index[i];
        this.texelDense = dense;
      }
    }
    /* The copy for tint `col` with rect `s` filled in. */
    _tintedRect(col, s) {
      const key = col[0] + ',' + col[1] + ',' + col[2];
      let e = this.cache.get(key);
      if (!e) {
        const tinted = this.tinter(this.texelPalette, col[0], col[1], col[2]);
        const canvas = document.createElement('canvas');
        canvas.width = this.w; canvas.height = this.h;
        e = { canvas, ctx: canvas.getContext('2d'), filled: new Set(),
          lut: new Uint32Array(tinted.buffer, tinted.byteOffset, tinted.byteLength >> 2) };
        this.cache.set(key, e);
      }
      const rect = s[0] + ',' + s[1] + ',' + s[2] + ',' + s[3];
      if (!e.filled.has(rect)) {
        e.filled.add(rect);
        const x0 = Math.max(0, Math.floor(s[0])), y0 = Math.max(0, Math.floor(s[1]));
        const x1 = Math.min(this.w, Math.ceil(s[0] + s[2])), y1 = Math.min(this.h, Math.ceil(s[1] + s[3]));
        if (x1 > x0 && y1 > y0) {
          const rw = x1 - x0, rh = y1 - y0;
          const out = new Uint32Array(rw * rh);
          const dense = this.texelDense, lut = e.lut;
          for (let y = 0, o = 0; y < rh; y++) {
            const row = (y0 + y) * this.w + x0;
            for (let x = 0; x < rw; x++, o++) {
              const k = dense[row + x];
              if (k !== 0xFFFF) out[o] = lut[k];
            }
          }
          e.ctx.putImageData(new ImageData(new Uint8ClampedArray(out.buffer), rw, rh), x0, y0);
        }
      }
      return e.canvas;
    }
    _tintThroughPalette(col) {
      if (this.texelIndex === null) {
        const whole = this.tinter(this.rgba, col[0], col[1], col[2]);
        return new Uint32Array(whole.buffer, whole.byteOffset, whole.byteLength >> 2);
      }
      const tinted = this.tinter(this.texelPalette, col[0], col[1], col[2]);
      const lut = new Uint32Array(tinted.buffer, tinted.byteOffset, tinted.byteLength >> 2);
      const index = this.texelIndex, at = this.texelAt;
      const out = new Uint32Array(this.w * this.h);
      for (let i = 0; i < index.length; i++) out[at[i]] = lut[index[i]];
      return out;
    }
    _tinted(col) {
      const r = Math.round(col[0] * 255), g = Math.round(col[1] * 255), b = Math.round(col[2] * 255);
      if (this.tinter) {
        /* Keyed on the tint as given: an ink is stored on the 5-bit grid, and
         * rounding it to 8 bits first would lose what the kernel reads. */
        const key = col[0] + ',' + col[1] + ',' + col[2];
        let cv = this.cache.get(key);
        if (cv) return cv;
        const out = this._tintThroughPalette(col);
        cv = document.createElement('canvas');
        cv.width = this.w; cv.height = this.h;
        cv.getContext('2d').putImageData(
          new ImageData(new Uint8ClampedArray(out.buffer, out.byteOffset, out.byteLength), this.w, this.h), 0, 0);
        this.cache.set(key, cv);
        return cv;
      }
      if (r === 255 && g === 255 && b === 255) return this.base;   /* identity multiply */
      const key = (r << 16) | (g << 8) | b;
      let cv = this.cache.get(key);
      if (cv) return cv;
      cv = document.createElement('canvas');
      cv.width = this.w; cv.height = this.h;
      const cx = cv.getContext('2d');
      cx.drawImage(this.base, 0, 0);
      cx.globalCompositeOperation = 'multiply';
      cx.fillStyle = 'rgb(' + r + ',' + g + ',' + b + ')';
      cx.fillRect(0, 0, this.w, this.h);
      /* Restore the source alpha the flat fill just clobbered on the transparent
       * texels, so only the glyph / sprite footprint remains. */
      cx.globalCompositeOperation = 'destination-in';
      cx.drawImage(this.base, 0, 0);
      cx.globalCompositeOperation = 'source-over';
      this.cache.set(key, cv);
      return cv;
    }
    blit(ctx, draws) {
      if (!draws) return;
      for (const d of draws) {
        const s = d.src, dst = d.dst, col = d.color;
        if (!s || !dst || s[2] <= 0 || s[3] <= 0 || dst[2] <= 0 || dst[3] <= 0) continue;
        ctx.globalAlpha = col ? col[3] : 1;
        const c = col || [1, 1, 1, 1];
        ctx.drawImage(this.tinter && this.texelDense ? this._tintedRect(c, s) : this._tinted(c),
          s[0], s[1], s[2], s[3], dst[0], dst[1], dst[2], dst[3]);
      }
      ctx.globalAlpha = 1;
    }
    /* A draw list carrying the save screen's subtractive darkening
     * (`darken: {at, level}`, the engine's `SaveScreenDarken`): retail pushes
     * one `B - F` quad under its active panel. A 2D canvas has no subtract
     * composite, so blit the sprites under the quad, subtract `level` from
     * every channel of what the canvas holds, then blit the rest past the quad
     * itself. The native window draws the same quad through its ABR-2
     * pipeline. */
    blitDarkened(ctx, draws, darken) {
      if (!draws) return;
      if (!darken || !(darken.level > 0)) { this.blit(ctx, draws); return; }
      const at = Math.max(0, Math.min(draws.length, darken.at | 0));
      this.blit(ctx, draws.slice(0, at));
      const w = ctx.canvas.width, h = ctx.canvas.height, g = darken.level;
      if (w > 0 && h > 0) {
        const img = ctx.getImageData(0, 0, w, h);
        const d = img.data;
        for (let i = 0; i < d.length; i += 4) {
          d[i] = d[i] > g ? d[i] - g : 0;
          d[i + 1] = d[i + 1] > g ? d[i + 1] - g : 0;
          d[i + 2] = d[i + 2] > g ? d[i + 2] - g : 0;
        }
        ctx.putImageData(img, 0, 0);
      }
      this.blit(ctx, draws.slice(at + 1));
    }
  }

  /* Shared by the boot-title controller in play.html, which renders the retail
   * title card onto the same overlay canvas before a scene exists. */
  window.LegaiaAtlasBlitter = AtlasBlitter;
})();
