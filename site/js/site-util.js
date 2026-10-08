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
  function modulatedSprite(src, u, v, w, h, top, bottom) {
    if (!src || !top || !bottom || w <= 0 || h <= 0) return null;
    var neutral = true;
    for (var k = 0; k < 3; k++) if (top[k] !== 128 || bottom[k] !== 128) neutral = false;
    if (neutral) return null;
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

  window.escapeHtml = escapeHtml;
  window.LegaiaUtil = {
    escapeHtml: escapeHtml,
    rgbaCanvas: rgbaCanvas,
    downloadFile: downloadFile,
    poseClipInto: poseClipInto,
    modulatedSprite: modulatedSprite,
  };
})();
