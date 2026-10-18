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
    constructor(rgba, w, h, tinter) {
      this.w = w; this.h = h;
      this.rgba = new Uint8Array(rgba);
      this.tinter = typeof tinter === 'function' ? tinter : null;
      this.base = document.createElement('canvas');
      this.base.width = w; this.base.height = h;
      const bctx = this.base.getContext('2d');
      bctx.putImageData(new ImageData(new Uint8ClampedArray(rgba), w, h), 0, 0);
      this.cache = new Map();
    }
    /* A tinter over the runtime's `psx_tint_atlas`, or null when the loaded
     * bundle predates it. */
    static tinterFor(rt) {
      if (!rt || typeof rt.psx_tint_atlas !== 'function') return null;
      return (rgba, r, g, b) => rt.psx_tint_atlas(rgba, r, g, b);
    }
    /* Forget the tinted copies (after the runtime's tint mode changed). */
    invalidate() { this.cache.clear(); }
    _tinted(col) {
      const r = Math.round(col[0] * 255), g = Math.round(col[1] * 255), b = Math.round(col[2] * 255);
      if (this.tinter) {
        /* Keyed on the tint as given: an ink is stored on the 5-bit grid, and
         * rounding it to 8 bits first would lose what the kernel reads. */
        const key = col[0] + ',' + col[1] + ',' + col[2];
        let cv = this.cache.get(key);
        if (cv) return cv;
        const out = this.tinter(this.rgba, col[0], col[1], col[2]);
        cv = document.createElement('canvas');
        cv.width = this.w; cv.height = this.h;
        cv.getContext('2d').putImageData(
          new ImageData(new Uint8ClampedArray(out), this.w, this.h), 0, 0);
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
        ctx.drawImage(this._tinted(col || [1, 1, 1, 1]),
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
