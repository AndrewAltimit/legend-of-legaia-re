/* canvas-abr.js - PSX semi-transparency for sprites drawn on a 2D canvas.
 *
 * A retail quad's `abr` picks one of four blend equations over the
 * framebuffer B and the sprite F. Three of them are canvas composites; the
 * fourth, `B - F`, has none, and is drawn as the sprite's black silhouette
 * instead. LegaiaCanvasAbr.withAbr wraps one draw in the right composite
 * state; LegaiaCanvasAbr.silhouette is the cached silhouette of a decoded
 * sheet.
 *
 * Classic script; loads before minigame-muscle.js. */
window.LegaiaCanvasAbr = (function () {
  'use strict';

  /* PSX semi-transparency for a hub quad's `abr` (null = opaque): 0 is
   * `B/2 + F/2`, 1 `B + F`, 3 `B + F/4` - canvas composites (a transparent
   * texel has alpha 0 and adds nothing). 2 is `B - F`, which a 2D canvas has
   * no composite for: `draw` receives `true` and must hand in the sprite's
   * black silhouette (`silhouette`) instead. Every ABR-2 hub quad samples
   * one of the all-white knockout palettes the variant-2 emitter bump
   * selects (7 under 6, 9 under 8, ...), and the arena uploads those
   * STP-set, so `B - white` clamps to black under every texel - which is the
   * silhouette, exactly, at full fade. */
  function withAbr(g, abr, draw, rgb) {
    if (abr == null) return draw(false);
    if (abr === 2) {
      /* `B - F` with F = white * c / 128: the quad's Gouraud colour scales
       * what it takes away, so a row the tally has not faded in yet removes
       * nothing. Drawing the silhouette at full strength whatever the
       * colour knocked a black box out of every row still at level 0 (the
       * "glitched" score tally). The silhouette at alpha c / 128 is
       * `B * (1 - c/128)`, which meets the exact `B - 255 * c/128` at both
       * ends of the fade. */
      const k = subtractLevel(rgb);
      if (k <= 0) return undefined;
      g.save();
      g.globalAlpha *= k;
      try { return draw(true); } finally { g.restore(); }
    }
    g.save();
    if (abr === 0) g.globalAlpha *= 0.5;
    else {
      g.globalCompositeOperation = 'lighter';
      if (abr === 3) g.globalAlpha *= 0.25;
    }
    try { return draw(false); } finally { g.restore(); }
  }

  /* A subtractive quad's strength in 0..1 off its Gouraud colour pair
   * `[top, bottom]` (each `[r, g, b]`, 0x80 = neutral); 1 with none. */
  function subtractLevel(rgb) {
    if (!rgb || !rgb[0] || !rgb[1]) return 1;
    let sum = 0;
    for (let k = 0; k < 3; k++) sum += rgb[0][k] + rgb[1][k];
    return Math.max(0, Math.min(1, sum / 6 / 128));
  }

  /* The black silhouette of a decoded sheet (every opaque texel -> black,
   * alpha kept), cached per sheet canvas. */
  const silhouettes = new WeakMap();
  function silhouette(c) {
    if (!c) return c;
    let s = silhouettes.get(c);
    if (!s) {
      s = document.createElement('canvas');
      s.width = c.width; s.height = c.height;
      const sg = s.getContext('2d');
      sg.drawImage(c, 0, 0);
      sg.globalCompositeOperation = 'source-in';
      sg.fillStyle = '#000';
      sg.fillRect(0, 0, s.width, s.height);
      silhouettes.set(c, s);
    }
    return s;
  }

  return { withAbr, silhouette, subtractLevel };
})();
