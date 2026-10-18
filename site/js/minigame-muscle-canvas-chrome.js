/* minigame-muscle-canvas-chrome.js - fitted canvas chrome for the Muscle
 * Dome HUD.
 *
 * The drawn (not disc-sourced) plates, chips, bars and labels the panel
 * falls back on, plus the subtractive shade the first-visit backdrop needs.
 * All of it draws in the retail 320x240 frame onto the 2x HUD canvas.
 * MgMuscleCanvasChrome.create(g) returns the draw functions bound to one
 * 2D context.
 *
 * Classic script; loads before minigame-muscle.js. */
window.MgMuscleCanvasChrome = (function () {
  'use strict';

  function create(g) {
    /* ------------------------------------------------------------ HUD draw
     *
     * Canvas approximations of the retail battle chrome (blue-marble plates
     * with gold borders, bevelled gold chips, the crossed-out Item chip, the
     * pointed AP / status plates). Geometry + colours are FITTED to the
     * retail captures; the wording is the captures' own. */

    function bar(x, y, w, h, frac, col, back) {
      g.fillStyle = back || 'rgba(0,0,0,0.55)';
      g.fillRect(x * 2, y * 2, w * 2, h * 2);
      g.fillStyle = col;
      g.fillRect(x * 2, y * 2, Math.max(0, Math.min(1, frac)) * w * 2, h * 2);
      g.strokeStyle = 'rgba(255,255,255,0.35)';
      g.strokeRect(x * 2 + 0.5, y * 2 + 0.5, w * 2, h * 2);
    }

    function text(s, x, y, size, col, align, boldness) {
      g.font = (boldness || 'bold ') + (size * 2) + 'px ui-monospace, monospace';
      g.textAlign = align || 'left';
      g.textBaseline = 'middle';
      g.fillStyle = 'rgba(0,0,0,0.65)';
      g.fillText(s, x * 2 + 2, y * 2 + 2);
      g.fillStyle = col || '#e8ecf2';
      g.fillText(s, x * 2, y * 2);
    }

    /* One chrome plate. style: 'blue' marble / 'gold' bevel / 'grey'
     * (disabled). pointed: extend hexagonal points on both ends. */
    function plate(x, y, w, h, style, pointed) {
      const X = x * 2, Y = y * 2, W = w * 2, H = h * 2, P = pointed ? H / 2 : 0;
      g.save();
      /* Outline as a Path2D so the border strokes stay on the plate even
       * after the mottling loop replaces the context's current path. */
      const outline = new Path2D();
      if (pointed) {
        outline.moveTo(X - P, Y + H / 2);
        outline.lineTo(X, Y); outline.lineTo(X + W, Y);
        outline.lineTo(X + W + P, Y + H / 2);
        outline.lineTo(X + W, Y + H); outline.lineTo(X, Y + H);
      } else {
        const r = Math.min(7, H / 2);
        outline.moveTo(X + r, Y);
        outline.lineTo(X + W - r, Y); outline.quadraticCurveTo(X + W, Y, X + W, Y + r);
        outline.lineTo(X + W, Y + H - r);
        outline.quadraticCurveTo(X + W, Y + H, X + W - r, Y + H);
        outline.lineTo(X + r, Y + H); outline.quadraticCurveTo(X, Y + H, X, Y + H - r);
        outline.lineTo(X, Y + r); outline.quadraticCurveTo(X, Y, X + r, Y);
      }
      outline.closePath();
      const grad = g.createLinearGradient(0, Y, 0, Y + H);
      if (style === 'gold') {
        grad.addColorStop(0, '#d8b268'); grad.addColorStop(0.45, '#b98f3e');
        grad.addColorStop(1, '#8a6526');
      } else if (style === 'grey') {
        grad.addColorStop(0, '#6b6f7e'); grad.addColorStop(1, '#494c58');
      } else {
        grad.addColorStop(0, '#7d82c8'); grad.addColorStop(0.5, '#565b9e');
        grad.addColorStop(1, '#3c4084');
      }
      g.fillStyle = grad;
      g.fill(outline);
      /* Marble mottling on the blue plates (cheap, deterministic). */
      if (style === 'blue') {
        g.save(); g.clip(outline);
        g.fillStyle = 'rgba(255,255,255,0.10)';
        for (let i = 0; i < Math.max(2, (w / 18) | 0); i++) {
          const mx = X + ((i * 73 + x * 31 + y * 17) % Math.max(1, W));
          const my = Y + ((i * 41 + x * 13) % Math.max(1, H));
          g.beginPath(); g.ellipse(mx, my, 9, 4, 0.6, 0, Math.PI * 2); g.fill();
        }
        g.restore();
      }
      g.lineWidth = 2.5;
      g.strokeStyle = style === 'gold' ? '#5d431a'
        : style === 'grey' ? '#2e3038' : '#c8a24a';
      g.stroke(outline);
      g.lineWidth = 1;
      g.strokeStyle = 'rgba(255,244,200,0.5)';
      g.stroke(outline);
      g.restore();
    }

    /* A command chip: plate + centred label. */
    function chip(x, y, w, h, style, label, labelCol) {
      plate(x, y, w, h, style);
      const col = labelCol || (style === 'gold' ? '#2e1f06'
        : style === 'grey' ? '#b9bcc6' : '#f2f4fa');
      text(label, x + w / 2, y + h / 2 + 0.5, Math.min(8, h - 6), col, 'center');
    }

    /* The retail Item chip's red cross-out. */
    function crossOut(x, y, w, h) {
      g.save();
      g.strokeStyle = '#c41f1f';
      g.lineWidth = 7;
      g.lineCap = 'round';
      g.beginPath();
      g.moveTo((x - 2) * 2, (y - 1) * 2); g.lineTo((x + w + 2) * 2, (y + h + 1) * 2);
      g.moveTo((x + w + 2) * 2, (y - 1) * 2); g.lineTo((x - 2) * 2, (y + h + 1) * 2);
      g.stroke();
      g.restore();
    }

    /* The grey D-pad glyph between Attack and the Ra-Seru chip. */
    function dpadGlyph(cx, cy, r) {
      const X = cx * 2, Y = cy * 2, R = r * 2, a = R * 0.38;
      g.save();
      g.fillStyle = '#cfd2da';
      g.strokeStyle = '#5a5d68';
      g.lineWidth = 2;
      g.beginPath();
      g.moveTo(X - a, Y - R); g.lineTo(X + a, Y - R); g.lineTo(X + a, Y - a);
      g.lineTo(X + R, Y - a); g.lineTo(X + R, Y + a); g.lineTo(X + a, Y + a);
      g.lineTo(X + a, Y + R); g.lineTo(X - a, Y + R); g.lineTo(X - a, Y + a);
      g.lineTo(X - R, Y + a); g.lineTo(X - R, Y - a); g.lineTo(X - a, Y - a);
      g.closePath();
      g.fill(); g.stroke();
      g.fillStyle = '#9a9daa';
      g.beginPath(); g.arc(X, Y, a * 0.7, 0, Math.PI * 2); g.fill();
      g.restore();
    }

    /* The first visit's backdrop shade (FUN_801D1610, ABR 2 `B - F` over what
     * is already down): the shared composite-op form in site-util.js. */
    function subtractShade(g, q, sx, sy) {
      return window.LegaiaUtil.subtractShade(g, q, sx, sy);
    }

    return { bar, text, plate, chip, crossOut, dpadGlyph, subtractShade };
  }

  return { create };
})();
