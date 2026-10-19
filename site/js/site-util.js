/* Small helpers every page shares. Classic script (no type="module"),
 * loaded from the page <head> by the template in site/_gen.py, so inline page
 * scripts, classic scripts and modules alike reach it as a global.
 *
 * `window.escapeHtml` is the one escaper; the rest hang off
 * `window.LegaiaUtil`. Add a helper here the second time a page needs it,
 * not the ninth. */
(function () {
  'use strict';
  var ENTITIES = { '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' };

  /* Escape text for HTML element content or a quoted attribute value.
   * `null` / `undefined` render as the empty string. */
  function escapeHtml(s) {
    return String(s == null ? '' : s).replace(/[&<>"']/g, function (c) { return ENTITIES[c]; });
  }

  /* Wrap a `w x h` RGBA8 buffer (typically from a WASM export) in an
   * offscreen canvas. A missing or wrongly-sized buffer gives `null`, so a
   * caller can treat "the export is absent from this build" and "the export
   * had nothing" alike. */
  function rgbaCanvas(bytes, w, h) {
    if (!bytes || bytes.length !== w * h * 4) return null;
    var c = document.createElement('canvas');
    c.width = w; c.height = h;
    c.getContext('2d').putImageData(
      new ImageData(new Uint8ClampedArray(bytes), w, h), 0, 0);
    return c;
  }

  /* Hand the viewer a file to save. `data` is a Blob or anything a Blob
   * accepts (a typed array, a string); the object URL is revoked a few
   * seconds later, once the download has started. */
  function downloadFile(data, filename) {
    var blob = data instanceof Blob
      ? data
      : new Blob([data], { type: 'application/octet-stream' });
    var url = URL.createObjectURL(blob);
    var a = document.createElement('a');
    a.href = url;
    a.download = filename;
    document.body.appendChild(a);
    a.click();
    a.remove();
    setTimeout(function () { URL.revokeObjectURL(url); }, 4000);
  }

  var A2R = (Math.PI * 2) / 4096;   /* PSX 12-bit angle -> radians */

  /* Pose `base` (object-local verts) through one frame of `clip` into `out`,
   * then spin the whole figure `yaw` about Y and shift it `dx` / `dz` along
   * X / Z. The per-object composition is retail's Rz.Ry.Rx . v + T (see
   * mesh-view.js); the world transform sits on top. `clip` is
   * `{ parts, frameCount, frames }` with six values per part per frame
   * (three translations, three 12-bit angles), the stream shape the
   * minigame pose exports return. `oids[v]` names vertex v's object; an
   * object past the clip's part count keeps its rest position. `vertBase`
   * selects one figure's slice of combined buffers. Shared by the minigame
   * pages and the play page's minigame layer. */
  function poseClipInto(out, base, oids, clip, frame, vertBase, dx, yaw, dz) {
    var pc = clip.parts, f = clip.frames;
    var ff = ((frame % clip.frameCount) + clip.frameCount) % clip.frameCount;
    var sin = new Float32Array(pc * 3), cos = new Float32Array(pc * 3);
    var tr = new Float32Array(pc * 3);
    for (var p = 0; p < pc; p++) {
      var fo = (ff * pc + p) * 6;
      for (var k = 0; k < 3; k++) {
        var a = f[fo + 3 + k] * A2R;
        sin[p * 3 + k] = Math.sin(a);
        cos[p * 3 + k] = Math.cos(a);
        tr[p * 3 + k] = f[fo + k];
      }
    }
    var wsin = Math.sin(yaw || 0), wcos = Math.cos(yaw || 0);
    var ox = dx || 0, oz = dz || 0;
    var n = oids.length;
    for (var v = 0; v < n; v++) {
      var vi = (vertBase + v) * 3;
      var o = oids[v];
      var x = base[vi], y = base[vi + 1], z = base[vi + 2];
      if (o < pc) {
        var sx = sin[o * 3], cxx = cos[o * 3];
        var sy = sin[o * 3 + 1], cyy = cos[o * 3 + 1];
        var sz = sin[o * 3 + 2], czz = cos[o * 3 + 2];
        var ny = y * cxx - z * sx, nz = y * sx + z * cxx; y = ny; z = nz;
        var nx = x * cyy + z * sy; nz = -x * sy + z * cyy; x = nx; z = nz;
        nx = x * czz - y * sz; ny = x * sz + y * czz; x = nx; y = ny;
        x += tr[o * 3]; y += tr[o * 3 + 1]; z += tr[o * 3 + 2];
      }
      out[vi] = x * wcos + z * wsin + ox;
      out[vi + 1] = y;
      out[vi + 2] = -x * wsin + z * wcos + oz;
    }
  }

  /* A textured PSX quad's colour modulation, `texel * c / 128` per channel,
   * with `c` the Gouraud colour shaded top -> bottom (the hub-screen quads'
   * brightness fade rides it). Returns a w x h canvas of the modulated texels
   * (alpha kept), or null when both colours are the neutral 128 - the caller
   * then blits the sheet as is. `top` / `bottom` are [r, g, b]. */
  /*
   * The result is memoised per source canvas and (rect, colours): a hub
   * screen asks for the same handful of sprites at the same fade level on
   * every frame of a hold, and building one is a fresh canvas plus a pixel
   * readback - dozens of them per frame was what made the INTERVAL screen
   * run at a few frames a second. Callers only draw from the returned
   * canvas, so sharing it is safe. */
  var modulatedCache = new WeakMap();
  var MODULATED_CACHE_CAP = 512;
  function modulatedSprite(src, u, v, w, h, top, bottom) {
    if (!src || !top || !bottom || w <= 0 || h <= 0) return null;
    var neutral = true;
    for (var k = 0; k < 3; k++) if (top[k] !== 128 || bottom[k] !== 128) neutral = false;
    if (neutral) return null;
    var perSrc = modulatedCache.get(src);
    if (!perSrc) { perSrc = new Map(); modulatedCache.set(src, perSrc); }
    var key = u + ',' + v + ',' + w + ',' + h + ',' + top.join(',') + ',' + bottom.join(',');
    var hit = perSrc.get(key);
    if (hit) return hit;
    if (perSrc.size >= MODULATED_CACHE_CAP) perSrc.clear();
    var c = buildModulatedSprite(src, u, v, w, h, top, bottom);
    perSrc.set(key, c);
    return c;
  }
  function buildModulatedSprite(src, u, v, w, h, top, bottom) {
    var dims = true;
    for (var j = 0; j < 3; j++) if (top[j] > 128 || bottom[j] > 128) dims = false;
    if (dims) return dimmedSprite(src, u, v, w, h, top, bottom);
    var c = document.createElement('canvas');
    c.width = w; c.height = h;
    var g = c.getContext('2d', { willReadFrequently: true });
    g.drawImage(src, u, v, w, h, 0, 0, w, h);
    var img = g.getImageData(0, 0, w, h), d = img.data;
    for (var y = 0; y < h; y++) {
      var t = h > 1 ? y / (h - 1) : 0;
      var m0 = (top[0] + (bottom[0] - top[0]) * t) / 128;
      var m1 = (top[1] + (bottom[1] - top[1]) * t) / 128;
      var m2 = (top[2] + (bottom[2] - top[2]) * t) / 128;
      for (var x = 0; x < w; x++) {
        var i = (y * w + x) * 4;
        d[i] = Math.min(255, d[i] * m0);
        d[i + 1] = Math.min(255, d[i + 1] * m1);
        d[i + 2] = Math.min(255, d[i + 2] * m2);
      }
    }
    g.putImageData(img, 0, 0);
    return c;
  }

  /* `texel * c / 128` for colours at or below neutral (every hub fade), on
   * the GPU: 'multiply' each row by its colour, then 'destination-in' with
   * the sprite to put its alpha back (the fill leaves every pixel opaque).
   * Within one 8-bit step of the per-pixel form; it skips the pixel
   * readback, which forces a GPU sync on an accelerated sheet. */
  function dimmedSprite(src, u, v, w, h, top, bottom) {
    var c = document.createElement('canvas');
    c.width = w; c.height = h;
    var g = c.getContext('2d');
    if (!g) return null;
    g.drawImage(src, u, v, w, h, 0, 0, w, h);
    g.globalCompositeOperation = 'multiply';
    var band = null, bandY = 0;
    for (var y = 0; y <= h; y++) {
      var key = null;
      if (y < h) {
        var t = h > 1 ? y / (h - 1) : 0;
        var rgb = [];
        for (var k = 0; k < 3; k++) {
          rgb.push(Math.round((top[k] + (bottom[k] - top[k]) * t) * 255 / 128));
        }
        key = rgb.join(',');
      }
      if (key !== band) {
        if (band !== null && y > bandY) {
          g.fillStyle = 'rgb(' + band + ')';
          g.fillRect(0, bandY, w, y - bandY);
        }
        band = key; bandY = y;
      }
    }
    g.globalCompositeOperation = 'destination-in';
    g.drawImage(src, u, v, w, h, 0, 0, w, h);
    return c;
  }

  /* The first visit's backdrop shade (FUN_801D1610): an untextured Gouraud
   * quad drawn with ABR 2 (tpage 0x46, `B - F`), applied to what is already
   * down on `g`. Per canvas row F is the ramp at that row's logical y
   * (`q.top` -> `q.bottom` over `q.dh`, rounded) and every channel drops by
   * F, clamped at 0. `sx` / `sy` map logical 320x240 pixels to canvas
   * pixels.
   *
   * A 2D canvas has no subtractive composite, but `max(0, B - F)` is
   * `1 - min(1, (1 - B) + F)`: invert ('difference' with white), add F
   * ('lighter'), invert back - all on the GPU, exact in 8-bit on opaque
   * pixels. The work happens on a scratch canvas so the region's alpha can
   * be put back ('destination-in' with the original), since the inverts
   * make every pixel opaque. Reading the pixels back instead
   * (getImageData / putImageData) stalled each frame of the hub's first
   * visit by tens of milliseconds. Returns true (the caller's black-band
   * fallback is for a canvas that cannot do this). */
  var shadeScratch = null;
  function subtractShade(g, q, sx, sy) {
    var cw = g.canvas.width, ch = g.canvas.height;
    var x0 = Math.max(0, Math.round(q.x * sx)), y0 = Math.max(0, Math.round(q.y * sy));
    var x1 = Math.min(cw, Math.round((q.x + q.dw) * sx));
    var y1 = Math.min(ch, Math.round((q.y + q.dh) * sy));
    var w = x1 - x0, h = y1 - y0;
    if (w <= 0 || h <= 0 || !(q.dh > 0)) return true;
    if (!shadeScratch) shadeScratch = document.createElement('canvas');
    var o = shadeScratch;
    if (o.width < w) o.width = w;
    if (o.height < h) o.height = h;
    var og = o.getContext('2d');
    if (!og) return false;
    og.save();
    og.globalCompositeOperation = 'copy';
    og.drawImage(g.canvas, x0, y0, w, h, 0, 0, w, h);
    og.globalCompositeOperation = 'difference';
    og.fillStyle = '#fff';
    og.fillRect(0, 0, w, h);
    og.globalCompositeOperation = 'lighter';
    /* One band per run of rows sharing a rounded F. */
    var bandF = 0, bandY = 0;
    for (var r = 0; r <= h; r++) {
      var f = 0;
      if (r < h) {
        var ly = (y0 + r + 0.5) / sy - q.y;
        f = Math.max(0, Math.min(255, Math.round(q.top + (q.bottom - q.top) * (ly / q.dh))));
      }
      if (r === h || f !== bandF) {
        if (bandF > 0 && r > bandY) {
          og.fillStyle = 'rgb(' + bandF + ',' + bandF + ',' + bandF + ')';
          og.fillRect(0, bandY, w, r - bandY);
        }
        bandF = f; bandY = r;
      }
    }
    og.globalCompositeOperation = 'difference';
    og.fillStyle = '#fff';
    og.fillRect(0, 0, w, h);
    og.globalCompositeOperation = 'destination-in';
    og.drawImage(g.canvas, x0, y0, w, h, 0, 0, w, h);
    og.restore();
    g.save();
    g.setTransform(1, 0, 0, 1, 0, 0);
    g.globalCompositeOperation = 'source-over';
    g.globalAlpha = 1;
    g.clearRect(x0, y0, w, h);
    g.drawImage(o, 0, 0, w, h, x0, y0, w, h);
    g.restore();
    return true;
  }

  /* Continue a class body in another file. `source` is a class (usually an
   * anonymous one) whose methods, getters and setters are copied onto
   * `target`'s prototype with the descriptors a class body gives them
   * (writable, configurable, non-enumerable), so a method behaves the same
   * whichever file declares it. Used where one class is too large for one
   * file: the first file declares it, the others install onto it. */
  function installMethods(target, source) {
    var names = Object.getOwnPropertyNames(source.prototype);
    for (var i = 0; i < names.length; i++) {
      if (names[i] === 'constructor') continue;
      Object.defineProperty(target.prototype, names[i],
        Object.getOwnPropertyDescriptor(source.prototype, names[i]));
    }
  }

  window.escapeHtml = escapeHtml;
  window.LegaiaUtil = {
    escapeHtml: escapeHtml,
    rgbaCanvas: rgbaCanvas,
    downloadFile: downloadFile,
    poseClipInto: poseClipInto,
    modulatedSprite: modulatedSprite,
    subtractShade: subtractShade,
    installMethods: installMethods,
  };
})();
